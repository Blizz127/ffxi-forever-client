import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {integer, needs, materialLimit, missingFor, craftBlock, shoppingList, marketQuote, newSandbox, validateState, practiceCraft, practiceBid} from '../dist/domain.js';

const data = JSON.parse(await readFile(new URL('../dist/data/recipes.json',import.meta.url),'utf8'));
const first = data.recipes.find(r => r.id === 2);
const clone = obj => structuredClone(obj);
const invFor = (recipe, count=1) => Object.fromEntries(needs(recipe,count).map(i=>[i.itemId,i.quantity]));
const readyFor = (recipe, count=1) => ({...newSandbox(data), inventory:invFor(recipe,count), skills:Object.fromEntries(data.crafts.map(c=>[c.id,70]))});
function unchangedAfterFailure(state, fn, message) {
  const before=clone(state);
  assert.throws(fn,message);
  assert.deepEqual(state,before,'failure must not consume inventory, gil, stock or log entries');
}

test('catalogue has unique source recipe IDs, complete names, and valid referenced items',()=>{
  assert.equal(data.recipes.length,1004);
  assert.equal(new Set(data.recipes.map(r=>r.id)).size,1004);
  assert.equal(data.crafts.length,8);
  assert.equal(data.recipes.filter(r=>r.baseEligible).length,919);
  assert.equal(data.meta.nameCoverage,1759);
  for(const r of data.recipes){
    assert.ok(data.crafts.some(c=>c.id===r.craft));
    for(const id of [r.crystal,r.result.itemId,...r.ingredients.map(i=>i.itemId),...r.hq.map(i=>i.itemId).filter(Boolean)]) assert.ok(data.items[id]?.name,`item ${id} recipe ${r.id}`);
  }
});

test('integer accepts UI whole-number strings and rejects invalid bounds and fractional numbers',()=>{
  assert.equal(integer('12',1,999),12);
  for(const value of [-1,1.5,NaN,Infinity,'abc',1000,true,false,null,undefined,'','   ',[],[1],{}, {valueOf:()=>1}]) assert.throws(()=>integer(value,0,999));
});

test('needs retains actual repeated-material counts and scales crystals and ingredients together',()=>{
  const r=data.recipes.find(r=>r.id===8);
  assert.deepEqual(needs(r,2),[{itemId:4098,quantity:2},{itemId:705,quantity:2},{itemId:768,quantity:2},{itemId:840,quantity:4}]);
  assert.throws(()=>needs(r,0));
  assert.throws(()=>needs(r,1000));
});

test('needs counts the crystal again if it is also an ingredient (synthetic regression)',()=>{
  const r={...first,ingredients:[{itemId:first.crystal,quantity:2},{itemId:688,quantity:3}]};
  assert.deepEqual(needs(r,3),[{itemId:first.crystal,quantity:9},{itemId:688,quantity:9}]);
  assert.equal(materialLimit(r,{[first.crystal]:8,688:12}),2);
});

test('material limits and missing quantities never double-spend common supplies',()=>{
  const r=data.recipes.find(r=>r.id===8);
  const inv={4098:10,705:5,768:5,840:3};
  assert.equal(materialLimit(r,inv),1);
  assert.equal(missingFor(r,inv,2).find(i=>i.itemId===840).missing,1);
  assert.equal(materialLimit(r,{}),0);
});

test('shopping planner aggregates shared ingredients before subtracting inventory',()=>{
  const a=data.recipes.find(r=>r.id===2), b=data.recipes.find(r=>r.id===6);
  assert.equal(a.crystal,b.crystal);
  const result=shoppingList(data,[{recipeId:a.id,quantity:2},{recipeId:b.id,quantity:3}],{[a.crystal]:4,688:1});
  assert.deepEqual(result.find(i=>i.itemId===a.crystal),{itemId:a.crystal,quantity:5,have:4,missing:1});
  assert.deepEqual(result.find(i=>i.itemId===688),{itemId:688,quantity:2,have:1,missing:1});
  assert.equal(shoppingList(data,[{recipeId:-1,quantity:1}],{}).length,0);
});

test('fresh sample state survives a validated export/import round-trip',()=>{
  const s=newSandbox(data);
  s.favorites=[2,8];s.plan=[{recipeId:2,quantity:3}];s.marketSold[688]=2;
  s.activity=[{time:'2026-11-11T19:00:00.000Z',text:'Local test receipt'}];
  assert.deepEqual(validateState(JSON.parse(JSON.stringify(s)),data),s);
});

test('import rejects unknown inventory, impossible counts, skills and plan quantities',()=>{
  assert.throws(()=>validateState(null,data));
  assert.throws(()=>validateState({...newSandbox(data),version:3},data));
  assert.throws(()=>validateState({...newSandbox(data),inventory:[]},data));
  assert.throws(()=>validateState({...newSandbox(data),inventory:{99999999:1}},data));
  assert.throws(()=>validateState({...newSandbox(data),inventory:{constructor:1}},data),/Unknown item/);
  assert.throws(()=>validateState({...newSandbox(data),inventory:JSON.parse('{"__proto__":1}')},data),/Unknown item/);
  for(const value of [true,false,null,'','   ',[],[1],{}]) assert.throws(()=>validateState({...newSandbox(data),inventory:{688:value}},data));
  assert.throws(()=>validateState({...newSandbox(data),inventory:{688:-1}},data));
  assert.throws(()=>validateState({...newSandbox(data),inventory:{688:0.5}},data));
  const badSkill=newSandbox(data);badSkill.skills.wood=71;
  assert.throws(()=>validateState(badSkill,data));
  assert.throws(()=>validateState({...newSandbox(data),plan:[{recipeId:2,quantity:1000}]},data));
  assert.throws(()=>validateState({...newSandbox(data),gil:-1},data));
});

test('import sanitizes duplicate/unknown favorite and planned recipe identifiers',()=>{
  const raw={...newSandbox(data),favorites:[2,2,-1,'2'],plan:[{recipeId:2,quantity:1},{recipeId:2,quantity:2},{recipeId:-1,quantity:3}]};
  const s=validateState(raw,data);
  assert.deepEqual(s.favorites,[2]);
  assert.deepEqual(s.plan,[{recipeId:2,quantity:1}]);
});

test('practice synthesis consumes multiplied materials, grants the normal result, and logs once',()=>{
  const r=data.recipes.find(r=>r.id===8),s=readyFor(r,2);
  const made=practiceCraft(data,s,r.id,2);
  assert.equal(made,66);
  for(const i of needs(r,2))assert.equal(s.inventory[i.itemId],0);
  assert.equal(s.inventory[r.result.itemId],66);
  assert.equal(s.activity.length,1);
  assert.match(s.activity[0].text,/Guaranteed normal result/);
});

test('missing supplies and invalid recipe/quantity fail synthesis without any mutation',()=>{
  const s=readyFor(first);s.inventory[first.crystal]=0;
  unchangedAfterFailure(s,()=>practiceCraft(data,s,first.id),/missing materials/);
  unchangedAfterFailure(s,()=>practiceCraft(data,s,-1),/not found/);
  unchangedAfterFailure(s,()=>practiceCraft(data,s,first.id,0));
});

test('all required skills use the fifteen-level margin, including subcrafts',()=>{
  const r=data.recipes.find(r=>r.baseEligible&&r.skills.length>1&&r.skills.some(s=>s.level>15));
  assert.ok(r);
  const s=readyFor(r);
  for(const skill of r.skills)s.skills[skill.craft]=Math.max(0,skill.level-15);
  assert.equal(craftBlock(r,s),'');
  const high=r.skills.find(s=>s.level>15);s.skills[high.craft]-=1;
  unchangedAfterFailure(s,()=>practiceCraft(data,s,r.id),/skill is too low/);
});

test('base-era and required-key-item gates fail synthesis atomically',()=>{
  const out=data.recipes.find(r=>!r.baseEligible),s=readyFor(out);
  unchangedAfterFailure(s,()=>practiceCraft(data,s,out.id),/base-era/);
  const gated={...first,keyItem:123},gatedData={...data,recipes:[gated]},keyState=readyFor(gated);
  unchangedAfterFailure(keyState,()=>practiceCraft(gatedData,keyState,gated.id),/key item/);
});

test('crafted result overflow is checked before consuming supplies',()=>{
  const s=readyFor(first);s.inventory[first.result.itemId]=99999;
  unchangedAfterFailure(s,()=>practiceCraft(data,s,first.id),/limit reached/);
});

test('successful practice bid charges the offered bid and updates stock and inventory once',()=>{
  const s=newSandbox(data),id=688,q=marketQuote(data,id),bid=q.price+25,before=s.inventory[id]||0,gil=s.gil;
  assert.equal(practiceBid(data,s,id,2,bid),2*bid);
  assert.equal(s.gil,gil-2*bid);
  assert.equal(s.inventory[id],before+2);
  assert.equal(s.marketSold[id],2);
  assert.equal(s.activity.length,1);
});

test('underprice, overspend, exhausted stock and invalid bid inputs are atomic failures',()=>{
  const s=newSandbox(data),id=688,q=marketQuote(data,id);
  unchangedAfterFailure(s,()=>practiceBid(data,s,id,1,q.price-1),/unsuccessful/);
  unchangedAfterFailure(s,()=>practiceBid(data,s,id,1,s.gil+1),/Not enough sandbox gil/);
  unchangedAfterFailure(s,()=>practiceBid(data,s,id,q.stock+1,q.price),/stock/);
  unchangedAfterFailure(s,()=>practiceBid(data,s,id,0,q.price));
  unchangedAfterFailure(s,()=>practiceBid(data,s,id,1,1.5));
  unchangedAfterFailure(s,()=>practiceBid(data,s,999999,1,q.price),/cannot be auctioned/);
  s.marketSold[id]=q.stock;
  unchangedAfterFailure(s,()=>practiceBid(data,s,id,1,q.price),/stock/);
});

test('bid result overflow cannot spend gil or reserve sample stock',()=>{
  const s=newSandbox(data),id=688,q=marketQuote(data,id);s.inventory[id]=99999;
  unchangedAfterFailure(s,()=>practiceBid(data,s,id,1,q.price),/limit reached/);
});
