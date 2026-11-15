import test from 'node:test';
import assert from 'node:assert/strict';
import {LiveCompanion, RemoteError} from '../dist/remote.js';
import {mapSessionRead,mapSnapshotRead,mapCatalogRead,mapMarketRead,MappingUnavailable} from '../lib/backend-read-map.js';

// Source-only transport simulation. No verifier, endpoint or capability is
// activated: this metadata stands in for a future independently verified BFF.
const metadata = (capabilities={}) => ({authenticated:true,realm:'ffxi-forever',csrf_token:'test-only-csrf',capabilities});
const response = (body,status=200) => new Response(JSON.stringify(body),{status,headers:{'Content-Type':'application/json'}});
const seconds=()=>Math.floor(Date.now()/1000);
const snapshot = (overrides={}) => ({kind:'snapshot',snapshot:{character_id:42,character_name:'Fixture',revision:'read:15:4:1',observed_at:seconds(),online:true,era:'disabled:[ROTZ]',craft_cap:70,gil:50000,free_inventory_slots:20,inventory:[{item_id:688,quantity:6}],skills:{wood:10},key_items:[],...overrides}});
const recipes = () => ({kind:'recipes',catalog:{character_id:42,revision:'read:15:4:2',observed_at:seconds(),era:'disabled:[ROTZ]',craft_cap:70,recipes:[{recipe_id:2,crystal_id:4098,ingredients:[{item_id:688,quantity:1},{item_id:688,quantity:1}],results:[{item_id:705,quantity:1},null,{item_id:705,quantity:3},null],skills:{wood:2},key_item:null}]}});
const market = stack => ({kind:'market',market:{item_id:688,stack,single_listings:7,stack_listings:2,observed_at:seconds(),history:[{price:900,sold_at:100,seller:'First',buyer:'Buyer'},{price:1100,sold_at:101,seller:'Second',buyer:'Buyer'}]}});
const isCode=code=>error=>error instanceof RemoteError&&error.code===code;
function fakeTransport({caps={},snap=snapshot(),catalog=recipes()}={}) {
  const calls=[],coreCalls=[];
  const ownCharacters=mapSessionRead({kind:'session',characters:[{character_id:42,name:'Fixture'}]});
  const fetch=async(path,init)=>{
    calls.push({path,init});const url=new URL(path,'https://test.invalid');
    if(init.method!=='GET')return response({error:'live_bridge_unavailable'},503);
    try {
      if(url.pathname==='/api/v1/session')return response({...metadata(caps),...ownCharacters});
      if(url.pathname==='/api/v1/characters/42/snapshot'){
        coreCalls.push({kind:'snapshot',character_id:42});return response(mapSnapshotRead(snap,{characterId:42}));
      }
      if(url.pathname==='/api/v1/characters/42/recipes'){
        coreCalls.push({kind:'recipes',character_id:42});return response(mapCatalogRead(catalog,{characterId:42}));
      }
      if(url.pathname==='/api/v1/auction/items/688'){
        const characterId=Number(url.searchParams.get('character_id'));
        if(!ownCharacters.characters.some(c=>c.id===characterId))return response({error:'forbidden'},403);
        const stack=url.searchParams.get('stack')==='1';
        coreCalls.push({kind:'market',character_id:characterId,item_id:688,stack});
        return response(mapMarketRead(market(stack),{characterId,itemId:688,stack}));
      }
      return response({error:'not_found'},404);
    } catch(error){
      if(error instanceof MappingUnavailable)return response({error:'unavailable'},503);
      throw error;
    }
  };
  return {client:new LiveCompanion({fetch,crypto:{randomUUID:()=> '47c03504-a5b5-4b24-a4bd-49eb9ccbd8cc'}}),calls,coreCalls};
}

test('fake core reads map through BFF projections into the browser with own selector and exact native shapes',async()=>{
  const f=fakeTransport({caps:{snapshot:true,recipes:true,auction:true}});
  const session=await f.client.session();assert.deepEqual(session.characters,[{id:42,name:'Fixture'}]);
  const snap=await f.client.snapshot(42);assert.equal(snap.revision,'read:15:4:1');assert.equal(new Date(snap.observed_at).toISOString(),snap.observed_at);
  const catalog=await f.client.recipes(42);assert.equal(catalog.recipes[0].recipe_id,2);assert.equal(catalog.recipes[0].ingredients.length,2);assert.deepEqual(catalog.recipes[0].results,[{item_id:705,quantity:1},null,{item_id:705,quantity:3},null]);
  const stacks=await f.client.auctionItem(688,{characterId:42,stack:true});assert.equal(stacks.stock,2);assert.equal(stacks.stock_unit,'listings');
  const singles=await f.client.auctionItem(688,{characterId:42});assert.equal(singles.stock,7);
  assert.deepEqual(stacks.history.map(sale=>sale.seller),['First','Second']);assert.equal(stacks.history[0].sold_at,'1970-01-01T00:01:40.000Z');
  assert.deepEqual(f.coreCalls,[{kind:'snapshot',character_id:42},{kind:'recipes',character_id:42},{kind:'market',character_id:42,item_id:688,stack:true},{kind:'market',character_id:42,item_id:688,stack:false}]);
  assert.ok(f.calls.every(c=>c.init.headers.Authorization===undefined));
});

test('invalid native timestamp yields unavailable through fake transport with no invented epoch time',async()=>{
  const f=fakeTransport({caps:{snapshot:true},snap:snapshot({observed_at:'invalid'})});await f.client.session();
  await assert.rejects(()=>f.client.snapshot(42),isCode('unavailable'));
  assert.equal(f.coreCalls.length,1);
});

test('read nonce never becomes mutation authority even under deliberately overprivileged fake session metadata',async()=>{
  const f=fakeTransport({caps:{snapshot:true,craft:true,bid:true}});await f.client.session();const snap=await f.client.snapshot(42);
  assert.throws(()=>f.client.prepareCraft({characterId:42,recipeId:2,expectedRevision:snap.revision}),isCode('read_only_snapshot'));
  assert.throws(()=>f.client.prepareBid({characterId:42,itemId:688,stack:false,offeredGil:900,expectedRevision:snap.revision}),isCode('read_only_snapshot'));
  assert.ok(f.calls.every(call=>call.init.method==='GET'));
});

test('fake session with all capabilities absent remains disabled; native character list cannot grant them',async()=>{
  const f=fakeTransport();const session=await f.client.session();
  assert.ok(Object.values(session.capabilities).every(v=>v===false));
  await assert.rejects(()=>f.client.snapshot(42),isCode('unavailable'));
  await assert.rejects(()=>f.client.auctionItem(688,{characterId:42}),isCode('unavailable'));
  assert.deepEqual(f.coreCalls,[]);
  const client=new LiveCompanion({fetch:async()=>response(mapSessionRead({kind:'session',characters:[{character_id:42,name:'Fixture'}]}))});
  await assert.rejects(()=>client.session(),isCode('invalid_response'));
});

const SNAPSHOT_KEY='obs1:18446744073709551615:18446744073709551615:18446744073709551614';
const CATALOG_KEY='obs1:18446744073709551615:18446744073709551615:18446744073709551615';
function retainedReads() {
  const catalog=recipes();catalog.catalog.revision=CATALOG_KEY;
  return {snap:snapshot({revision:SNAPSHOT_KEY}),catalog};
}
const prepareCraft=(client,key)=>client.prepareCraft({characterId:42,recipeId:2,expectedRevision:key});
const prepareBid=(client,key)=>client.prepareBid({characterId:42,itemId:688,stack:false,offeredGil:900,expectedRevision:key});

test('obs1 snapshot and catalog survive fake transport exactly without enabling craft or bid or inventing expiry',async()=>{
  const f=fakeTransport({caps:{snapshot:true,recipes:true},...retainedReads()});
  const session=await f.client.session();
  const snap=await f.client.snapshot(42),catalog=await f.client.recipes(42);
  assert.equal(snap.revision,SNAPSHOT_KEY);assert.equal(catalog.revision,CATALOG_KEY);
  assert.equal(session.capabilities.craft,false);assert.equal(session.capabilities.bid,false);
  for(const read of [snap,catalog]){
    for(const key of ['expires_at','expires_in','expiry','retained_until','capabilities'])assert.equal(Object.hasOwn(read,key),false);
  }
  assert.throws(()=>prepareCraft(f.client,SNAPSHOT_KEY),isCode('unavailable'));
  assert.throws(()=>prepareBid(f.client,SNAPSHOT_KEY),isCode('unavailable'));
  assert.ok(f.calls.every(call=>call.init.method==='GET'));
});

test('separately scoped fake capabilities prepare with cached obs1 snapshot A and reject catalog B',async()=>{
  // Only preparation is exercised with synthetic capability metadata. This does
  // not perform a mutation or simulate the native retained lookup/engine checks.
  const f=fakeTransport({caps:{snapshot:true,recipes:true,craft:true,bid:true},...retainedReads()});
  await f.client.session();await f.client.snapshot(42);await f.client.recipes(42);
  assert.throws(()=>prepareCraft(f.client,CATALOG_KEY),isCode('stale_revision'));
  assert.throws(()=>prepareBid(f.client,CATALOG_KEY),isCode('stale_revision'));
  const craft=prepareCraft(f.client,SNAPSHOT_KEY),bid=prepareBid(f.client,SNAPSHOT_KEY);
  assert.deepEqual(craft.body,{operation_id:'47c03504-a5b5-4b24-a4bd-49eb9ccbd8cc',expected_revision:SNAPSHOT_KEY,recipe_id:2});
  assert.deepEqual(bid.body,{operation_id:'47c03504-a5b5-4b24-a4bd-49eb9ccbd8cc',expected_revision:SNAPSHOT_KEY,item_id:688,stack:false,offered_gil:900});
  assert.ok(Object.isFrozen(craft.body)&&Object.isFrozen(bid.body));
  assert.ok(f.calls.every(call=>call.init.method==='GET'));
});

test('obs1 does not bypass browser freshness; native retained expiry is not inferred from the timestamp',async()=>{
  const f=fakeTransport({caps:{snapshot:true,craft:true,bid:true},snap:snapshot({revision:SNAPSHOT_KEY,observed_at:seconds()-121})});
  await f.client.session();const snap=await f.client.snapshot(42);
  assert.equal(snap.revision,SNAPSHOT_KEY);assert.equal(Object.hasOwn(snap,'expires_at'),false);
  assert.throws(()=>prepareCraft(f.client,SNAPSHOT_KEY),isCode('stale_snapshot'));
  assert.throws(()=>prepareBid(f.client,SNAPSHOT_KEY),isCode('stale_snapshot'));
  assert.ok(f.calls.every(call=>call.init.method==='GET'));
});
