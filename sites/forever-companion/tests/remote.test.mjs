import test from 'node:test';
import assert from 'node:assert/strict';
import {LiveCompanion, RemoteError, UnknownOutcomeError} from '../dist/remote.js';
const OP = '8b7c5102-cb1e-4b90-9fcb-17fc67c4927f';
const reply = (data, status=200) => new Response(status===204?null:JSON.stringify(data),{status,headers:{'Content-Type':'application/json'}});
const auth = (overrides={}) => ({authenticated:true,realm:'ffxi-forever',csrf_token:'csrf-in-memory',characters:[{id:42,name:'Adventurer'}],capabilities:{snapshot:true,recipes:true,auction:true,craft:true,bid:true},...overrides});
const snapshot = (overrides={}) => ({character_id:42,character_name:'Adventurer',revision:'r:42:1',observed_at:new Date().toISOString(),online:true,era:'base',craft_cap:70,free_inventory_slots:20,inventory:[{item_id:688,quantity:6}],skills:{wood:10},key_items:[],gil:50000,...overrides});
const catalog = (overrides={}) => ({character_id:42,revision:'read:15:4:2',observed_at:new Date().toISOString(),era:'base',craft_cap:70,recipes:[{recipe_id:2,crystal_id:4098,ingredients:[{item_id:688,quantity:1},{item_id:688,quantity:1}],results:[{item_id:705,quantity:1},{item_id:705,quantity:2},null,null],skills:{wood:2},key_item:null}],...overrides});
const market = (overrides={}) => ({character_id:42,item_id:688,stack:true,stock:4,single_listings:9,stack_listings:4,stock_unit:'listings',observed_at:new Date().toISOString(),history:[{price:900,sold_at:'2026-11-11T00:00:00.000Z',seller:'Seller',buyer:'Buyer'}],...overrides});
const receipt = (state='queued', overrides={}) => ({operation_id:OP,character_id:42,state,...overrides});
function fixture(responses, options={}) {
  const calls=[];
  const fetch=async(url,init)=>{calls.push({url,init});const next=responses.shift();if(typeof next==='function')return next(url,init);if(next instanceof Error)throw next;if(!next)throw new Error('Unexpected fetch');return next;};
  const client=new LiveCompanion({fetch,crypto:{randomUUID:()=>OP},timeoutMs:1000,...options});
  return {client,calls,responses};
}
async function ready(extra=[],session=auth(),snap=snapshot()) {
  const f=fixture([reply(session),reply(snap),...extra]);await f.client.session();await f.client.snapshot(42);return f;
}
const craft = client => client.prepareCraft({characterId:42,recipeId:2,expectedRevision:'r:42:1'});
const code = expected => error => error instanceof RemoteError && error.code===expected;

test('session is same-origin cookie-only and exposes no account credential or CSRF token',async()=>{
  const {client,calls}=fixture([reply(auth({access_token:'never-browser',native_password:'never-browser'}))]);
  const session=await client.session();
  assert.equal(session.authenticated,true);assert.deepEqual(session.characters,[{id:42,name:'Adventurer'}]);
  assert.equal(session.csrf_token,undefined);assert.equal(session.access_token,undefined);assert.equal(session.native_password,undefined);
  assert.equal(calls[0].url,'/api/v1/session');assert.equal(calls[0].init.credentials,'same-origin');assert.equal(calls[0].init.mode,'same-origin');assert.equal(calls[0].init.redirect,'error');
  assert.equal(calls[0].init.headers.Authorization,undefined);assert.ok(Object.isFrozen(session.characters));
});

test('unauthenticated, wrong-character and disabled-capability actions fail before fetching',async()=>{
  const f=fixture([]);assert.throws(()=>craft(f.client),code('authentication_required'));assert.equal(f.calls.length,0);
  f.responses.push(reply(auth({capabilities:{snapshot:true}})));await f.client.session();
  assert.throws(()=>f.client.prepareCraft({characterId:43,recipeId:2,expectedRevision:'r:42:1'}),code('forbidden'));
  assert.throws(()=>craft(f.client),code('unavailable'));assert.equal(f.calls.length,1);
});

test('fresh own online snapshot and its exact revision are required for mutations',async()=>{
  const f=fixture([reply(auth()),reply(snapshot({online:false})),reply(snapshot({observed_at:'2000-01-01T00:00:00.000Z'})),reply(snapshot())]);
  await f.client.session();assert.throws(()=>craft(f.client),code('stale_snapshot'));
  await assert.rejects(()=>f.client.snapshot(42),code('unavailable'));assert.throws(()=>craft(f.client),code('stale_snapshot'));
  await f.client.snapshot(42);assert.throws(()=>craft(f.client),code('stale_snapshot'));
  await f.client.snapshot(42);assert.throws(()=>f.client.prepareCraft({characterId:42,recipeId:2,expectedRevision:'different'}),code('stale_revision'));
  assert.ok(craft(f.client));assert.equal(f.calls.length,4);
});

test('reads preserve native catalog shape and character-scoped listing counts/history',async()=>{
  const recipeCatalog=catalog();const observedMarket=market();
  const f=await ready([reply({...recipeCatalog,native_password:'strip'}),reply({...observedMarket,asking_price:1})],auth(),snapshot({native_password:'strip'}));
  const result=await f.client.recipes(42);assert.equal(result.native_password,undefined);
  assert.deepEqual(result,recipeCatalog);assert.equal(result.recipes[0].ingredients.length,2);assert.equal(result.recipes[0].results.length,4);
  const resultMarket=await f.client.auctionItem(688,{characterId:42,stack:true});assert.deepEqual(resultMarket,observedMarket);
  assert.equal(resultMarket.stock,4);assert.equal(resultMarket.stock_unit,'listings');
  assert.equal(f.calls.at(-1).url,'/api/v1/auction/items/688?stack=1&character_id=42');
  assert.ok(Object.isFrozen(resultMarket.history[0]));
});

test('invalid inputs never fetch, including arbitrary packet/result/batch fields',async()=>{
  const f=await ready();const before=f.calls.length;
  for(const value of [null,true,0,-1,1.5,'2',Number.MAX_SAFE_INTEGER])assert.throws(()=>f.client.prepareCraft({characterId:42,recipeId:value,expectedRevision:'r:42:1'}),code('invalid_input'));
  for(const value of ['',null,42,'has space','a'.repeat(129)])assert.throws(()=>f.client.prepareCraft({characterId:42,recipeId:2,expectedRevision:value}),code('invalid_input'));
  assert.throws(()=>f.client.prepareCraft({characterId:42,recipeId:2,expectedRevision:'r:42:1',quantity:5}),code('invalid_input'));
  assert.throws(()=>f.client.prepareBid({characterId:42,itemId:688,stack:false,offeredGil:9,expectedRevision:'r:42:1',quantity:2}),code('invalid_input'));
  for(const value of [0,-1,1.5,'900',true,1000000000])assert.throws(()=>f.client.prepareBid({characterId:42,itemId:688,stack:false,offeredGil:value,expectedRevision:'r:42:1'}),code('invalid_input'));
  assert.throws(()=>f.client.prepareBid({characterId:42,itemId:688,stack:1,offeredGil:9,expectedRevision:'r:42:1'}),code('invalid_input'));
  await assert.rejects(()=>f.client.snapshot(43),code('forbidden'));assert.equal(f.calls.length,before);
});

test('prepared craft is immutable, single-operation, scoped and sends CSRF with no local result',async()=>{
  const inventory={688:6};const f=await ready([reply(receipt('queued',{result:{inventory:{688:0}}}),202)]);
  const request=craft(f.client);assert.ok(Object.isFrozen(request));assert.ok(Object.isFrozen(request.body));
  assert.throws(()=>request.body.recipe_id=999,TypeError);
  const result=await f.client.submit(request);
  assert.equal(result.confirmed,false);assert.equal(result.terminal,false);assert.equal(result.result,undefined);assert.deepEqual(inventory,{688:6});
  assert.equal(f.calls.at(-1).url,'/api/v1/characters/42/crafts');
  assert.equal(f.calls.at(-1).init.headers['X-CSRF-Token'],'csrf-in-memory');
  assert.deepEqual(JSON.parse(f.calls.at(-1).init.body),{operation_id:OP,expected_revision:'r:42:1',recipe_id:2});
});

test('bid wire sends offered total gil and one single/stack choice without browser account IDs',async()=>{
  const f=await ready([reply(receipt(),202)]);
  const request=f.client.prepareBid({characterId:42,itemId:688,stack:true,offeredGil:1900,expectedRevision:'r:42:1'});
  await f.client.submit(request);
  assert.deepEqual(JSON.parse(f.calls.at(-1).init.body),{operation_id:OP,expected_revision:'r:42:1',item_id:688,stack:true,offered_gil:1900});
  assert.equal(f.calls.at(-1).url,'/api/v1/characters/42/bids');
});

test('imported/forged operation objects cannot be submitted',async()=>{
  const f=await ready();const request=craft(f.client);const before=f.calls.length;
  await assert.rejects(()=>f.client.submit(JSON.parse(JSON.stringify(request))),code('invalid_input'));assert.equal(f.calls.length,before);
});

test('lost mutation reply preserves operation ID and never retries automatically',async()=>{
  const f=await ready([new Error('socket lost'),reply(receipt('running',{code:'pending_reconciliation'}))]);
  const request=craft(f.client);
  await assert.rejects(()=>f.client.submit(request),error=>error instanceof UnknownOutcomeError&&error.operationId===OP);
  assert.equal(f.calls.length,3);
  const result=await f.client.operationReceipt(OP);assert.equal(result.state,'running');assert.equal(result.confirmed,false);assert.equal(result.code,'pending_reconciliation');
  assert.equal(f.calls.at(-1).url,`/api/v1/operations/${OP}`);assert.equal(f.calls.at(-1).init.method,'GET');
});

test('timeout bounds a hung fetch and keeps the same operation available for explicit reconciliation',async()=>{
  const f=await ready();const hung=()=>new Promise(()=>{});f.responses.push(hung);
  const client=new LiveCompanion({fetch:async(url,init)=>{f.calls.push({url,init});if(url.endsWith('/session'))return reply(auth());if(url.endsWith('/snapshot'))return reply(snapshot());return hung();},crypto:{randomUUID:()=>OP},timeoutMs:15});
  await client.session();await client.snapshot(42);
  const request=craft(client);const before=f.calls.length;
  await assert.rejects(()=>client.submit(request),error=>error.code==='unknown_outcome'&&error.operationId===OP&&error.cause.code==='timeout');
  assert.equal(f.calls.length,before+1);assert.equal(f.calls.at(-1).init.signal.aborted,true);assert.equal(request.body.operation_id,OP);
});

test('explicit resubmission uses the identical immutable operation ID and payload',async()=>{
  const f=await ready([new Error('lost reply'),reply(receipt('succeeded',{result:{item_id:705,quantity:1}}))]);
  const request=craft(f.client);await assert.rejects(()=>f.client.submit(request),code('unknown_outcome'));
  const result=await f.client.submit(request);assert.equal(result.confirmed,true);assert.equal(result.terminal,true);
  assert.deepEqual(f.calls[2].init.body,f.calls[3].init.body);assert.equal(f.calls.length,4);
});

test('only a matching confirmed success receipt exposes its server outcome',async()=>{
  const f=await ready([reply(receipt('succeeded',{result:{item_id:705,quantity:1}})),reply(receipt('succeeded',{operation_id:'11111111-1111-4111-8111-111111111111',result:{}})),reply(receipt('succeeded'))]);
  const result=await f.client.operationReceipt(OP);assert.equal(result.confirmed,true);assert.deepEqual(result.result,{item_id:705,quantity:1});
  await assert.rejects(()=>f.client.operationReceipt(OP),code('invalid_response'));
  await assert.rejects(()=>f.client.operationReceipt(OP),code('invalid_response'));
});

test('uncertain stays unconfirmed and expired is terminal without an inventory result',async()=>{
  const f=await ready([reply(receipt('uncertain',{code:'pending_reconciliation',result:{inventory:{}}})),reply(receipt('expired',{code:'expired'}))]);
  const pending=await f.client.operationReceipt(OP);assert.equal(pending.confirmed,false);assert.equal(pending.terminal,false);assert.equal(pending.result,undefined);
  const expired=await f.client.operationReceipt(OP);assert.equal(expired.confirmed,false);assert.equal(expired.terminal,true);assert.equal(expired.code,'expired');
});

test('typed HTTP errors distinguish unavailable, authentication, stale revision and commit ambiguity',async()=>{
  const unavailable=fixture([reply({error:'live_bridge_unavailable'},503)]);await assert.rejects(()=>unavailable.client.session(),code('unavailable'));
  const f=await ready([reply({error:'stale_revision'},409),reply({error:'internal_commit_unknown'},500),reply({error:'live_bridge_unavailable'},503),reply({error:'session_expired'},401)]);
  const request=craft(f.client);
  await assert.rejects(()=>f.client.submit(request),code('stale_revision'));
  await assert.rejects(()=>f.client.submit(request),code('unknown_outcome'));
  await assert.rejects(()=>f.client.submit(request),code('unavailable'));
  await assert.rejects(()=>f.client.submit(request),code('authentication_required'));
  assert.throws(()=>craft(f.client),code('authentication_required'));
});

test('CSRF is required, while device replies and polls never expose bearer credentials',async()=>{
  const f=fixture([reply({authenticated:false,csrf_token:'anonymous-csrf'}),reply({request_id:'device_request_1',user_code:'ABCD-EFGH',verification_uri:'https://accounts.example/device',expires_in:300,interval:5,access_token:'strip'}),reply({state:'approved',access_token:'strip'}),reply(null,204)]);
  await assert.rejects(()=>f.client.beginDeviceSignIn(),code('csrf_required'));
  await f.client.session();const device=await f.client.beginDeviceSignIn();assert.equal(device.access_token,undefined);
  const status=await f.client.pollDeviceSignIn(device.request_id);assert.deepEqual(status,{state:'approved'});
  assert.equal(f.calls.at(-1).url,'/api/v1/session/device/poll');assert.equal(f.calls.at(-1).init.headers['X-CSRF-Token'],'anonymous-csrf');
  await f.client.logout();assert.throws(()=>craft(f.client),code('authentication_required'));
});

test('session refresh invalidates old prepared operations and cached live snapshots',async()=>{
  const f=await ready([reply(auth())]);const request=craft(f.client);await f.client.session();
  await assert.rejects(()=>f.client.submit(request),code('invalid_input'));assert.throws(()=>craft(f.client),code('stale_snapshot'));
});

test('late session response cannot reinstall authentication or CSRF after logout',async()=>{
  let resolveSession;
  const f=fixture([reply(auth()),()=>new Promise(resolve=>{resolveSession=resolve;}),reply(null,204)]);
  await f.client.session();const late=f.client.session();
  // The session refresh cleared its old CSRF, so it cannot start logout itself.
  // Start a replacement session to obtain a newer epoch, then let old data arrive.
  f.responses.splice(0,0,reply({authenticated:false,csrf_token:'new-anonymous'}));
  await f.client.session();await f.client.logout();
  resolveSession(reply(auth({csrf_token:'late-old-token'})));
  await assert.rejects(()=>late,code('stale_session'));
  await assert.rejects(()=>f.client.beginDeviceSignIn(),code('csrf_required'));
});

test('late snapshot from a previous session cannot authorize a new account operation',async()=>{
  let resolveSnapshot;const f=fixture([reply(auth()),()=>new Promise(resolve=>{resolveSnapshot=resolve;}),reply(auth())]);
  await f.client.session();const late=f.client.snapshot(42);await f.client.session();resolveSnapshot(reply(snapshot()));
  await assert.rejects(()=>late,code('stale_session'));assert.throws(()=>craft(f.client),code('stale_snapshot'));
});

test('a late unauthorized reply from an old session cannot clear the newer signed-in session',async()=>{
  let resolveOld;const f=await ready([()=>new Promise(resolve=>{resolveOld=resolve;}),reply(auth()),reply(snapshot())]);
  const oldRead=f.client.recipes(42);await f.client.session();
  resolveOld(new Response('not JSON',{status:401}));
  await assert.rejects(()=>oldRead,code('authentication_required'));
  await f.client.snapshot(42);assert.ok(craft(f.client));
});

test('a late mutation result after account refresh remains an unknown outcome for its original ID',async()=>{
  let resolveOld;const f=await ready([()=>new Promise(resolve=>{resolveOld=resolve;}),reply(auth())]);
  const request=craft(f.client),pending=f.client.submit(request);await f.client.session();
  resolveOld(reply(receipt('succeeded',{result:{item_id:705,quantity:1}})));
  await assert.rejects(()=>pending,error=>error instanceof UnknownOutcomeError&&error.operationId===OP&&error.cause.code==='stale_session');
  await assert.rejects(()=>f.client.submit(request),code('invalid_input'));
});


test('read observation revision cannot authorize mutations even with fake advertised action capabilities',async()=>{
  const read='read:15:4:1';const f=await ready([],auth(),snapshot({revision:read}));const count=f.calls.length;
  assert.throws(()=>f.client.prepareCraft({characterId:42,recipeId:2,expectedRevision:read}),code('read_only_snapshot'));
  assert.throws(()=>f.client.prepareBid({characterId:42,itemId:688,stack:false,offeredGil:900,expectedRevision:read}),code('read_only_snapshot'));
  assert.equal(f.calls.length,count);
});

test('market requires an own character selector and checks returned selector before exposing data',async()=>{
  const f=await ready([reply(market({character_id:43})),reply(market())]);const count=f.calls.length;
  await assert.rejects(()=>f.client.auctionItem(688),code('invalid_input'));
  await assert.rejects(()=>f.client.auctionItem(688,{characterId:43}),code('forbidden'));
  assert.equal(f.calls.length,count);
  await assert.rejects(()=>f.client.auctionItem(688,{characterId:42,stack:true}),code('invalid_response'));
  assert.equal((await f.client.auctionItem(688,{characterId:42,stack:true})).character_id,42);
});

test('single and stack market stock stays listing counts and malformed counts or oversized histories are rejected',async()=>{
  const single=market({stack:false,stock:9});
  const f=await ready([reply(single),reply(market({stock:48})),reply(market({stack_listings:-1})),reply(market({history:Array(11).fill(market().history[0])}))]);
  const result=await f.client.auctionItem(688,{characterId:42});assert.equal(result.stock,9);assert.equal(result.stack_listings,4);
  for(let i=0;i<3;i++)await assert.rejects(()=>f.client.auctionItem(688,{characterId:42,stack:true}),code('invalid_response'));
});

test('mapped UTC timestamps must be canonical and failed reads discard old snapshot evidence',async()=>{
  const f=await ready([reply(snapshot({observed_at:1794355200})),reply(snapshot({observed_at:'0'})),reply(catalog({observed_at:'invalid'})),reply(market({history:[{...market().history[0],sold_at:'0'}]}))]);
  for(let i=0;i<2;i++)await assert.rejects(()=>f.client.snapshot(42),code('invalid_response'));
  assert.throws(()=>craft(f.client),code('stale_snapshot'));
  await assert.rejects(()=>f.client.recipes(42),code('invalid_response'));
  await assert.rejects(()=>f.client.auctionItem(688,{characterId:42,stack:true}),code('invalid_response'));
});

test('session character projection respects native maximum32 and UTF8 name size',async()=>{
  const f=fixture([reply(auth({characters:Array.from({length:33},(_,i)=>({id:i+1,name:'Adventurer'}))})),reply(auth({characters:[{id:42,name:'é'.repeat(17)}]})),reply(auth({characters:[{id:42,name:'bad\u0085name'}]}))]);
  for(let i=0;i<3;i++)await assert.rejects(()=>f.client.session(),code('invalid_response'));
});
