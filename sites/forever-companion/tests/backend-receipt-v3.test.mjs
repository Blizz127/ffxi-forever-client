import test from 'node:test';
import assert from 'node:assert/strict';
import {MappingUnavailable,receiptReadRequest,mapReceiptRead} from '../lib/backend-read-map.js';

// Source-only native DTO fixtures. These selectors and owner lists stand in for
// independently verified BFF context; they create no identity or live provider.
const OP='8b7c5102-cb1e-4b90-9fcb-17fc67c4927f';
const OTHER_OP='11111111-1111-4111-8111-111111111111';
const scope=()=>({operationId:OP,ownedCharacterIds:[42]});
const receipt=(overrides={})=>({kind:'receipt',receipt:{operation_id:OP,character_id:42,state:'running',...overrides}});
const unavailable=error=>error instanceof MappingUnavailable&&error.code==='unavailable'&&error.status===503;

test('receipt request is the exact frozen ID-only dedicated native read selector',()=>{
  const request=receiptReadRequest(OP);
  assert.deepEqual(request,{kind:'receipt',operation_id:OP});
  assert.ok(Object.isFrozen(request));
  assert.throws(()=>{request.operation_id=OTHER_OP;},TypeError);
  assert.deepEqual(Object.keys(request),['kind','operation_id']);
});

test('receipt request rejects noncanonical UUIDv4 selectors without coercion or normalization',()=>{
  const invalid=[undefined,null,true,1,{},[],{operation_id:OP},OP.toUpperCase(),
    OP.replace('-4b90-','-1b90-'),OP.replace('-9fcb-','-7fcb-'),OP.slice(1),`${OP}0`,
    ` ${OP}`,`${OP}\n`,OP.replaceAll('-',''),'00000000-0000-0000-0000-000000000000'];
  for(const id of invalid)assert.throws(()=>receiptReadRequest(id),unavailable);
});

test('wrapped running pending reconciliation maps unchanged without inferring terminal state',()=>{
  const mapped=mapReceiptRead(receipt({code:'pending_reconciliation'}),scope());
  assert.deepEqual(mapped,{operation_id:OP,character_id:42,state:'running',code:'pending_reconciliation'});
  for(const key of ['terminal','confirmed','inventory','gil','expected_revision','online','expires_at','capabilities']){
    assert.equal(Object.hasOwn(mapped,key),false);
  }
});

for(const state of ['queued','running','failed','rejected','cancelled']){
  test(`native ${state} receipt remains the same existing Receipt shape`,()=>{
    assert.deepEqual(mapReceiptRead(receipt({state}),scope()),{operation_id:OP,character_id:42,state});
  });
}

test('successful native receipt preserves its authoritative object result only',()=>{
  const result={item_id:705,quantity:1,hq_tier:0,skillups:[],nested:{source:'fixture'}};
  assert.deepEqual(mapReceiptRead(receipt({state:'succeeded',result}),scope()),{
    operation_id:OP,character_id:42,state:'succeeded',result,
  });
  assert.deepEqual(mapReceiptRead(receipt({state:'succeeded',result:{}}),scope()).result,{});
});

test('success requires an object result and native receipt state names stay closed',()=>{
  for(const result of [undefined,null,[],1,'success',true]){
    assert.throws(()=>mapReceiptRead(receipt({state:'succeeded',result}),scope()),unavailable);
  }
  for(const state of ['expired','uncertain','pending_reconciliation','completed','SUCCEEDED','',null,1]){
    assert.throws(()=>mapReceiptRead(receipt({state}),scope()),unavailable);
  }
  // Expiry and uncertainty already use the original native state/code mapping.
  assert.deepEqual(mapReceiptRead(receipt({state:'rejected',code:'expired'}),scope()),{
    operation_id:OP,character_id:42,state:'rejected',code:'expired',
  });
});

test('non-success cannot carry a result, while null optional fields remain omitted',()=>{
  for(const state of ['queued','running','failed','rejected','cancelled']){
    const mapped=mapReceiptRead(receipt({state,result:null,code:null}),scope());
    assert.deepEqual(mapped,{operation_id:OP,character_id:42,state});
    for(const result of [{},[],false,0,'outcome']){
      assert.throws(()=>mapReceiptRead(receipt({state,result}),scope()),unavailable);
    }
  }
});

test('receipt operation identity is exact and cannot be replaced by the returned operation',()=>{
  assert.throws(()=>mapReceiptRead(receipt({operation_id:OTHER_OP}),scope()),unavailable);
  assert.throws(()=>mapReceiptRead(receipt({operation_id:OP.toUpperCase()}),scope()),unavailable);
  assert.throws(()=>mapReceiptRead(receipt(),{...scope(),operationId:OTHER_OP}),unavailable);
  for(const operationId of [undefined,null,OP.toUpperCase(),`${OP} `,'not-an-operation']){
    assert.throws(()=>mapReceiptRead(receipt(),{...scope(),operationId}),unavailable);
  }
});

test('receipt needs a positive bounded currently owned character from trusted context',()=>{
  for(const character_id of [0,-1,1.5,'42',true,null,0x100000000,Number.MAX_SAFE_INTEGER]){
    assert.throws(()=>mapReceiptRead(receipt({character_id}),scope()),unavailable);
  }
  assert.throws(()=>mapReceiptRead(receipt({character_id:43}),scope()),unavailable);
  assert.equal(mapReceiptRead(receipt({character_id:0xffffffff}),{
    operationId:OP,ownedCharacterIds:[0xffffffff],
  }).character_id,0xffffffff);
  const ownedCharacterIds=Array.from({length:32},(_,i)=>i+1);
  assert.equal(mapReceiptRead(receipt({character_id:32}),{operationId:OP,ownedCharacterIds}).character_id,32);
});

test('missing malformed duplicated oversized or browser-asserted owner context fails closed',()=>{
  for(const context of [undefined,null,true,[],{},
    {operationId:OP},{ownedCharacterIds:[42]},
    {...scope(),ownedCharacterIds:[]},{...scope(),ownedCharacterIds:'42'},
    {...scope(),ownedCharacterIds:[42,42]},
    {...scope(),ownedCharacterIds:[42,0]},{...scope(),ownedCharacterIds:[42,'43']},
    {...scope(),ownedCharacterIds:[42,true]},{...scope(),ownedCharacterIds:[42,0x100000000]},
    {...scope(),ownedCharacterIds:Array.from({length:33},(_,i)=>i+1)},
    {...scope(),account_id:7},{...scope(),character_id:42},
    {...scope(),online:false},{...scope(),expected_revision:'obs1:1:1:1'}]){
    assert.throws(()=>mapReceiptRead(receipt(),context),unavailable);
  }
});

test('wrapped DTO validation rejects missing foreign refused and malformed native responses',()=>{
  for(const core of [undefined,null,[],false,{kind:'receipt'},
    {kind:'snapshot',receipt:receipt().receipt},{kind:'unavailable'},
    {error:'unavailable'},{kind:'receipt',receipt:null},
    {kind:'receipt',receipt:[]},{kind:'receipt',receipt:false},receipt().receipt]){
    assert.throws(()=>mapReceiptRead(core,scope()),unavailable);
  }
});

test('receipt wrapper and Receipt fields exclude native or browser authority assertions',()=>{
  for(const key of ['account_id','character_id','proof','snapshot','authenticated']){
    assert.throws(()=>mapReceiptRead({...receipt(),[key]:42},scope()),unavailable);
  }
  for(const key of ['account_id','generation','expected_revision','online','terminal','confirmed','command','capabilities']){
    assert.throws(()=>mapReceiptRead(receipt({[key]:42}),scope()),unavailable);
  }
});

test('optional code preserves bounded existing codes and rejects malformed values',()=>{
  assert.equal(mapReceiptRead(receipt({code:'a'.repeat(64)}),scope()).code,'a'.repeat(64));
  for(const code of ['',true,1,{},[],'UPPER','has space','bad\ncode','é','a'.repeat(65)]){
    assert.throws(()=>mapReceiptRead(receipt({code}),scope()),unavailable);
  }
});

test('reply capacity and unrepresentable native JSON fail unavailable without an outcome',()=>{
  assert.throws(()=>mapReceiptRead(receipt({state:'succeeded',result:{padding:'x'.repeat(2*1024*1024)}}),scope()),unavailable);
  assert.throws(()=>mapReceiptRead(receipt({state:'succeeded',result:{value:1n}}),scope()),unavailable);
  const cyclic={};cyclic.self=cyclic;
  assert.throws(()=>mapReceiptRead(receipt({state:'succeeded',result:cyclic}),scope()),unavailable);
});

test('serialization cannot turn a successful object result into a non-object outcome',()=>{
  for(const result of [new Date(0),{toJSON:()=> 'text'},{toJSON:()=> null},{toJSON:()=> []},{toJSON:()=> undefined}]){
    assert.throws(()=>mapReceiptRead(receipt({state:'succeeded',result}),scope()),unavailable);
  }
});

test('receipt projection is detached deeply frozen and leaves input/context unchanged',()=>{
  const core=receipt({state:'succeeded',result:{items:[{item_id:705,quantity:1}],status:{source:'native'}}});
  const context=scope();const before=structuredClone(core);const ownersBefore=[...context.ownedCharacterIds];
  const mapped=mapReceiptRead(core,context);
  assert.deepEqual(core,before);assert.deepEqual(context.ownedCharacterIds,ownersBefore);
  assert.notEqual(mapped,core.receipt);assert.notEqual(mapped.result,core.receipt.result);
  for(const value of [mapped,mapped.result,mapped.result.items,mapped.result.items[0],mapped.result.status]){
    assert.ok(Object.isFrozen(value));
  }
  assert.equal(Object.isFrozen(core),false);assert.equal(Object.isFrozen(core.receipt.result),false);
  core.receipt.result.items[0].quantity=99;context.ownedCharacterIds.push(43);
  assert.equal(mapped.result.items[0].quantity,1);
  assert.throws(()=>{mapped.result.items[0].quantity=2;},TypeError);
});

test('mapper errors do not expose a rejected native payload or synthesize an outcome',()=>{
  const secret='fixture-only-sensitive-native-field';
  assert.throws(()=>mapReceiptRead(receipt({proof:secret}),scope()),error=>{
    assert.ok(unavailable(error));assert.equal(error.message.includes(secret),false);
    assert.equal(String(error.reason).includes(secret),false);
    assert.equal(error.receipt,undefined);assert.equal(error.result,undefined);return true;
  });
});
