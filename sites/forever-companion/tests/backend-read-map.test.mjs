import test from 'node:test';
import assert from 'node:assert/strict';
import {MappingUnavailable,unixSecondsToISO,mapSessionRead,mapSnapshotRead,mapCatalogRead,mapMarketRead} from '../lib/backend-read-map.js';

// Typed core JSON shapes from xi-companion eff6da4a reads.rs/lib.rs. These are
// fake provider responses, never an enabled native service or account session.
const TIME=1794434146;
const ISO=new Date(TIME*1000).toISOString();
const count=(item_id,quantity)=>({item_id,quantity});
function snapshot(overrides={}) {
  return {kind:'snapshot',snapshot:{character_id:42,character_name:'Fixture',revision:'read:99:4:1',observed_at:TIME,
    online:true,era:'disabled:[ROTZ]',craft_cap:70,gil:123,free_inventory_slots:29,
    inventory:[count(640,2)],skills:{wood:5},key_items:[1987],...overrides}};
}
function catalog(overrides={}) {
  return {kind:'recipes',catalog:{character_id:42,revision:'read:99:4:2',observed_at:TIME,era:'disabled:[ROTZ]',craft_cap:70,
    recipes:[{recipe_id:2,crystal_id:4098,ingredients:[count(688,1),count(688,1)],
      results:[count(705,1),null,count(705,3),count(705,4)],skills:{wood:2,cloth:1},key_item:null}],...overrides}};
}
function market(overrides={}) {
  return {kind:'market',market:{item_id:688,stack:true,single_listings:7,stack_listings:2,observed_at:TIME,
    history:[{price:100,sold_at:TIME-20,seller:'First',buyer:'Buyer1'},
      {price:101,sold_at:TIME-20,seller:'Second',buyer:'Buyer2'},
      {price:102,sold_at:TIME-10,seller:'Third',buyer:'Buyer3'}],...overrides}};
}
const unavailable=error=>error instanceof MappingUnavailable&&error.code==='unavailable'&&error.status===503;
const scope={characterId:42};
const auction={characterId:42,itemId:688,stack:true};

class FakeReadProvider {
  constructor(responses){this.responses=responses;this.calls=[];}
  read(request){this.calls.push(structuredClone(request));return structuredClone(this.responses.shift());}
}

test('Unix seconds convert strictly to canonical UTC with no invalid-to-zero fallback',()=>{
  assert.equal(unixSecondsToISO(TIME),ISO);
  assert.equal(unixSecondsToISO(0),'1970-01-01T00:00:00.000Z');
  for(const bad of [null,undefined,true,false,'0','123',-1,0.5,NaN,Infinity,Number.MAX_SAFE_INTEGER+1,8_640_000_000_001]){
    assert.throws(()=>unixSecondsToISO(bad),unavailable);
  }
});

test('session maps bounded native character selectors without inferring authentication',()=>{
  const core={kind:'session',characters:[{character_id:42,name:'Fixture',account_id:7,native_password:'strip'}],
    authenticated:true,csrf_token:'strip',capabilities:{craft:true},access_token:'strip'};
  const projected=mapSessionRead(core);
  assert.deepEqual(projected,{characters:[{id:42,name:'Fixture'}]});
  assert.ok(Object.isFrozen(projected.characters[0]));
  assert.equal(projected.authenticated,undefined);assert.equal(projected.capabilities,undefined);
  assert.equal(projected.csrf_token,undefined);
});

test('session enforces native32-character cap, unique IDs and UTF8-byte name limits',()=>{
  const rows=Array.from({length:32},(_,i)=>({character_id:i+1,name:`Fixture${i}`}));
  assert.equal(mapSessionRead({kind:'session',characters:rows}).characters.length,32);
  assert.throws(()=>mapSessionRead({kind:'session',characters:[...rows,{character_id:33,name:'Over'}]}),unavailable);
  for(const rows of [[{character_id:0,name:'Bad'}],[{character_id:42,name:'A'},{character_id:42,name:'B'}],
    [{character_id:42,name:'é'.repeat(17)}],[{character_id:42,name:'Bad\nName'}],[{character_id:true,name:'Bad'}]]){
    assert.throws(()=>mapSessionRead({kind:'session',characters:rows}),unavailable);
  }
});

test('snapshot unwraps existing1074 fields and converts only its timestamp',()=>{
  const core=snapshot({native_password:'strip',account_id:7,session_key:'strip'});
  const result=mapSnapshotRead(core,scope);
  assert.deepEqual(result,{character_id:42,revision:'read:99:4:1',observed_at:ISO,era:'disabled:[ROTZ]',craft_cap:70,
    character_name:'Fixture',online:true,gil:123,free_inventory_slots:29,inventory:[count(640,2)],skills:{wood:5},key_items:[1987]});
  assert.equal(result.account_id,undefined);assert.equal(result.session_key,undefined);
  assert.equal(core.snapshot.observed_at,TIME);assert.ok(Object.isFrozen(result.inventory[0]));
});

test('snapshot refuses foreign/offline/malformed projections rather than manufacturing saved data',()=>{
  for(const change of [{character_id:43},{online:false},{online:1},{observed_at:null},{inventory:[count(640,1),count(640,2)]},
    {gil:-1},{free_inventory_slots:65536},{skills:{unknown:5}},{key_items:[4096]},{key_items:[7,7]},
    {inventory:[count(0,2)]},{inventory:[count(640,0)]}]){
    assert.throws(()=>mapSnapshotRead(snapshot(change),scope),unavailable);
  }
});

test('read observation revisions remain exact strings with native counter bounds',()=>{
  const max='18446744073709551615';
  assert.equal(mapSnapshotRead(snapshot({revision:`read:${max}:4294967295:${max}`}),scope).revision,`read:${max}:4294967295:${max}`);
  for(const revision of ['r:42:1','read:0:4:1','read:99:0:1','read:99:4:0','read:99:4:01',
    'read:18446744073709551616:4:1','read:99:4294967296:1','read:99:4:18446744073709551616',true]){
    assert.throws(()=>mapSnapshotRead(snapshot({revision}),scope),unavailable);
  }
});

test('catalog preserves ordered multiplicity, four nullable tiers, skill map and metadata',()=>{
  const core=catalog();const result=mapCatalogRead(core,scope);
  assert.deepEqual(result,{...core.catalog,observed_at:ISO});
  assert.deepEqual(result.recipes[0].ingredients,[count(688,1),count(688,1)]);
  assert.equal(result.recipes[0].results[1],null);
  assert.ok(Object.isFrozen(result.recipes[0].results));
  assert.throws(()=>result.recipes[0].ingredients.push(count(999,1)),TypeError);
});

test('catalog rejects wrong ownership context and malformed or oversized native recipes',()=>{
  assert.throws(()=>mapCatalogRead(catalog({character_id:43}),scope),unavailable);
  const recipe=catalog().catalog.recipes[0];
  const badRecipes=[{...recipe,ingredients:[]},{...recipe,ingredients:Array(9).fill(count(688,1))},
    {...recipe,results:[count(705,1),null,null]},{...recipe,results:[null,null,null,null]},
    {...recipe,key_item:4096},{...recipe,skills:{wood:65536}},{...recipe,crystal_id:0}];
  for(const bad of badRecipes)assert.throws(()=>mapCatalogRead(catalog({recipes:[bad]}),scope),unavailable);
  assert.throws(()=>mapCatalogRead(catalog({recipes:[recipe,recipe]}),scope),unavailable);
  assert.throws(()=>mapCatalogRead(catalog({recipes:Array.from({length:4097},(_,i)=>({...recipe,recipe_id:i+1}))}),scope),unavailable);
});

test('market stock counts selected listings, preserving both counts and oldest-first ties',()=>{
  const core=market();
  const stacks=mapMarketRead(core,auction);
  assert.equal(stacks.stock,2);assert.equal(stacks.stock_unit,'listings');
  assert.equal(stacks.single_listings,7);assert.equal(stacks.stack_listings,2);
  assert.deepEqual(stacks.history.map(row=>row.seller),['First','Second','Third']);
  assert.equal(stacks.history[0].sold_at,new Date((TIME-20)*1000).toISOString());
  assert.equal(stacks.observed_at,ISO);assert.equal(stacks.character_id,42);
  const singles=mapMarketRead(market({stack:false}),{...auction,stack:false});
  assert.equal(singles.stock,7);assert.equal(singles.stock_unit,'listings');
});

test('market strips current asks, raw owner/account fields and invented unit quotes',()=>{
  const result=mapMarketRead(market({asking_price:1,unit_count:24,account_id:7,session_key:'strip',
    history:[{price:100,sold_at:TIME-1,seller:'Seller',buyer:'Buyer',current_ask:2,password:'strip'}]}),auction);
  assert.equal(result.asking_price,undefined);assert.equal(result.unit_count,undefined);assert.equal(result.account_id,undefined);
  assert.deepEqual(result.history,[{price:100,sold_at:new Date((TIME-1)*1000).toISOString(),seller:'Seller',buyer:'Buyer'}]);
});

test('market requires character/item/single-stack selectors and bounded matching response',()=>{
  for(const selectors of [{itemId:688,stack:true},{characterId:0,itemId:688,stack:true},
    {characterId:42,itemId:688,stack:1},{...auction,account_id:7},{...auction,itemId:65536}]){
    assert.throws(()=>mapMarketRead(market(),selectors),unavailable);
  }
  for(const changes of [{item_id:689},{stack:false},{single_listings:-1},{stack_listings:true},
    {history:Array(11).fill({price:1,sold_at:TIME,seller:'S',buyer:'B'})},{history:[{price:1,sold_at:null,seller:'S',buyer:'B'}]}]){
    assert.throws(()=>mapMarketRead(market(changes),auction),unavailable);
  }
});

test('denied-item native empty market remains empty without claiming availability or a quote',()=>{
  const result=mapMarketRead(market({single_listings:0,stack_listings:0,history:[]}),auction);
  assert.equal(result.stock,0);assert.deepEqual(result.history,[]);
  assert.equal(result.available,undefined);assert.equal(result.quote,undefined);
});

test('mapping rejects wrong variants, excessive reply bytes and non-JSON native values',()=>{
  assert.throws(()=>mapSessionRead({kind:'snapshot',characters:[]}),unavailable);
  assert.throws(()=>mapSnapshotRead({kind:'snapshot',snapshot:null},scope),unavailable);
  assert.throws(()=>mapSessionRead({kind:'session',characters:[],extra:'x'.repeat(2*1024*1024)}),unavailable);
  const cyclic={kind:'session',characters:[]};cyclic.self=cyclic;
  assert.throws(()=>mapSessionRead(cyclic),unavailable);
});

test('fake provider read inputs stay character-scoped; mapper creates no proof or capabilities',()=>{
  const provider=new FakeReadProvider([snapshot(),catalog(),market()]);
  const gotSnapshot=mapSnapshotRead(provider.read({kind:'snapshot',character_id:42}),scope);
  const gotCatalog=mapCatalogRead(provider.read({kind:'recipes',character_id:42}),scope);
  const gotMarket=mapMarketRead(provider.read({kind:'market',character_id:42,item_id:688,stack:true}),auction);
  assert.deepEqual(provider.calls,[{kind:'snapshot',character_id:42},{kind:'recipes',character_id:42},
    {kind:'market',character_id:42,item_id:688,stack:true}]);
  for(const result of [gotSnapshot,gotCatalog,gotMarket]){
    assert.equal(result.authenticated,undefined);assert.equal(result.capabilities,undefined);assert.equal(result.proof,undefined);
  }
});
