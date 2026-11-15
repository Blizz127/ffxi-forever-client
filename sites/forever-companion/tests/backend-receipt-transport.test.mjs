import test from 'node:test';
import assert from 'node:assert/strict';
import {LiveCompanion,RemoteError} from '../dist/remote.js';
import {receiptReadRequest,mapReceiptRead,MappingUnavailable} from '../lib/backend-read-map.js';

const OP='8b7c5102-cb1e-4b90-9fcb-17fc67c4927f';
const OTHER='11111111-1111-4111-8111-111111111111';
const wrapped=(overrides={})=>({kind:'receipt',receipt:{operation_id:OP,character_id:42,state:'running',code:'pending_reconciliation',...overrides}});
const response=(body,status=200)=>new Response(JSON.stringify(body),{status,headers:{'Content-Type':'application/json'}});
const isCode=code=>error=>error instanceof RemoteError&&error.code===code;

// Entirely synthetic verified identity/ownership and native provider. There is
// deliberately no online Session, snapshot route, mutation body or engine here.
// A real BFF must obtain these checks independently for the current receipt read.
function fixture({nativeRead=()=>wrapped(),owner={characterIds:[42],session:null},authenticated=true}={}) {
  const calls=[],nativeCalls=[];
  const fetch=async(path,init)=>{
    calls.push({path,init});
    if(init.method!=='GET')return response({error:'live_bridge_unavailable'},503);
    if(path==='/api/v1/session')return response({authenticated,realm:'ffxi-forever',csrf_token:'test-only-csrf',
      characters:authenticated?[{id:42,name:'Fixture'}]:[],capabilities:{}});
    if(!authenticated)return response({error:'unauthenticated'},401);
    if(path.startsWith('/api/v1/operations/')){
      try{
        const operationId=path.slice('/api/v1/operations/'.length);
        const request=receiptReadRequest(operationId);
        // Explicit dedicated entry: a generic read/read_retained call is never made.
        nativeCalls.push({entry:'read_receipt',request});
        const core=await nativeRead(request);
        return response(mapReceiptRead(core,{operationId,ownedCharacterIds:owner.characterIds}));
      }catch(error){
        if(error instanceof MappingUnavailable)return response({error:'unavailable'},503);
        throw error;
      }
    }
    throw new Error('Unexpected route: receipt reads must not fetch a snapshot or mutation.');
  };
  return {client:new LiveCompanion({fetch}),calls,nativeCalls,owner};
}

test('offline owner reads wrapped running/pending_reconciliation with no snapshot, mutation body or action capabilities',async()=>{
  const f=fixture();const session=await f.client.session();
  assert.equal(f.owner.session,null);assert.ok(Object.values(session.capabilities).every(value=>value===false));
  const receipt=await f.client.operationReceipt(OP);
  assert.deepEqual(receipt,{operation_id:OP,character_id:42,state:'running',terminal:false,confirmed:false,code:'pending_reconciliation'});
  assert.deepEqual(f.nativeCalls,[{entry:'read_receipt',request:{kind:'receipt',operation_id:OP}}]);
  assert.deepEqual(f.calls.map(call=>call.path),['/api/v1/session',`/api/v1/operations/${OP}`]);
  assert.ok(f.calls.every(call=>call.init.method==='GET'&&call.init.body===undefined&&call.init.headers.Authorization===undefined));
});

test('explicit second lookup preserves the same ID and exposes only the stored succeeded result',async()=>{
  const replies=[wrapped(),wrapped({state:'succeeded',code:null,result:{item_id:705,quantity:1}})];
  const f=fixture({nativeRead:()=>replies.shift()});await f.client.session();
  assert.equal((await f.client.operationReceipt(OP)).confirmed,false);
  assert.equal(f.nativeCalls.length,1);assert.equal(replies.length,1);
  const receipt=await f.client.operationReceipt(OP);
  assert.deepEqual(receipt,{operation_id:OP,character_id:42,state:'succeeded',terminal:true,confirmed:true,result:{item_id:705,quantity:1}});
  assert.equal(f.nativeCalls.length,2);
  assert.ok(f.nativeCalls.every(call=>call.request.operation_id===OP));
  assert.ok(f.calls.every(call=>call.init.method==='GET'));
});

for(const [label,nativeRead] of [
  ['wrong returned operation',()=>wrapped({operation_id:OTHER})],
  ['foreign returned character',()=>wrapped({character_id:43})],
  ['missing native row',()=>null],
  ['native refusal',()=>{throw new MappingUnavailable('native receipt refusal');}],
  ['unknown native read commit',()=>{throw new MappingUnavailable('native read completion unknown');}],
]){
  test(`${label} stays unavailable without a synthetic receipt or automatic retry`,async()=>{
    const f=fixture({nativeRead});await f.client.session();
    await assert.rejects(()=>f.client.operationReceipt(OP),isCode('unavailable'));
    assert.equal(f.nativeCalls.length,1);assert.equal(f.calls.length,2);
    assert.ok(f.calls.every(call=>call.init.method==='GET'));
  });
}

test('current verified owner context overrides a stale browser character list',async()=>{
  const f=fixture();await f.client.session();
  f.owner.characterIds=[43];
  await assert.rejects(()=>f.client.operationReceipt(OP),isCode('unavailable'));
  assert.equal(f.nativeCalls.length,1);assert.equal(f.calls.length,2);
});

test('unauthenticated receipt lookup cannot reach the fake native provider',async()=>{
  const f=fixture({authenticated:false});await f.client.session();
  await assert.rejects(()=>f.client.operationReceipt(OP),isCode('authentication_required'));
  assert.deepEqual(f.nativeCalls,[]);assert.equal(f.calls.length,1);
});

test('receipt transport failure is reported once and never resubmits a mutation',async()=>{
  const f=fixture({nativeRead:()=>{throw new Error('test-only lost connection');}});await f.client.session();
  await assert.rejects(()=>f.client.operationReceipt(OP),isCode('network_error'));
  assert.equal(f.nativeCalls.length,1);assert.equal(f.calls.length,2);
  assert.ok(f.calls.every(call=>call.init.method==='GET'));
});
