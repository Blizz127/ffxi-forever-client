import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {MappingUnavailable,mapSnapshotRead,mapCatalogRead} from '../lib/backend-read-map.js';

// Expectations supplied by backend contract v2. The fixture is copied verbatim
// after manifest/hash verification; it is not a report that a backend is live.
const fixtureBytes=readFileSync(new URL('./fixtures/backend-revision-v2-vectors.json',import.meta.url));
const vectors=JSON.parse(fixtureBytes);
const FIXTURE_SHA='46f421dfb5580b470a29a2df714a81324f9adfcb2098d8fbd2f99e2923addf10';
const U64='18446744073709551615';
const U64_OVER='18446744073709551616';
const U32='4294967295';
const U32_OVER='4294967296';
const TIME=1794434146;
const scope={characterId:42};
const unavailable=error=>error instanceof MappingUnavailable&&error.code==='unavailable'&&error.status===503;

function snapshot(revision) {
  return {kind:'snapshot',snapshot:{character_id:42,character_name:'Fixture',revision,observed_at:TIME,
    online:true,era:'disabled:[ROTZ]',craft_cap:70,gil:123,free_inventory_slots:29,
    inventory:[{item_id:640,quantity:2}],skills:{wood:5},key_items:[1987]}};
}
function catalog(revision) {
  return {kind:'recipes',catalog:{character_id:42,revision,observed_at:TIME,
    era:'disabled:[ROTZ]',craft_cap:70,recipes:[]}};
}
function accepted(revision) {
  const reads=[mapSnapshotRead(snapshot(revision),scope),mapCatalogRead(catalog(revision),scope)];
  for(const read of reads){assert.equal(typeof read.revision,'string');assert.equal(read.revision,revision);}
  return reads;
}
function rejected(revision) {
  assert.throws(()=>mapSnapshotRead(snapshot(revision),scope),unavailable,`Snapshot accepted ${JSON.stringify(revision)}`);
  assert.throws(()=>mapCatalogRead(catalog(revision),scope),unavailable,`Catalog accepted ${JSON.stringify(revision)}`);
}

test('backend-v2 supplied revision vectors retain their independently verified bytes',()=>{
  assert.equal(createHash('sha256').update(fixtureBytes).digest('hex'),FIXTURE_SHA);
  assert.equal(vectors.mapping_valid.length,5);assert.equal(vectors.mapping_invalid.length,14);
});

for(const {revision} of vectors.mapping_valid){
  test(`backend-v2 snapshot and catalog accept exact revision ${revision}`,()=>accepted(revision));
}
for(const {revision} of vectors.mapping_invalid){
  test(`backend-v2 snapshot and catalog reject supplied invalid revision ${revision}`,()=>rejected(revision));
}

test('all revision components require canonical ASCII positive decimal',()=>{
  const invalid=['','0','00','01','+1','-1','1.0','1e0',' 1','1 ','1\n','1\u0000','1\u200b','١','１'];
  for(const family of ['read','obs1']){
    for(let position=0;position<3;position++){
      for(const bad of invalid){
        const components=['1','1','1'];components[position]=bad;
        rejected(`${family}:${components.join(':')}`);
      }
    }
  }
});

test('each revision family preserves string-safe component maxima and rejects each overflow',()=>{
  for(const family of ['read','obs1']){
    const maxima=[U64,family==='read'?U32:U64,U64];
    accepted(`${family}:${maxima.join(':')}`);
    for(let position=0;position<3;position++){
      const components=['1','1','1'];
      components[position]=family==='read'&&position===1?U32_OVER:U64_OVER;
      rejected(`${family}:${components.join(':')}`);
    }
  }
  // These neighboring values exceed JS's safe integer range; they must remain
  // different exact strings, rather than being round-tripped through Number.
  for(const value of ['18446744073709551614',U64]){
    accepted(`obs1:${value}:${value}:${value}`);
    accepted(`read:${value}:1:${value}`);
  }
});

test('obs1 retained-store lifetime is u64 while read generation remains u32',()=>{
  accepted(`obs1:1:${U32_OVER}:1`);
  rejected(`read:1:${U32_OVER}:1`);
  accepted(`obs1:1:${U64}:1`);
  rejected(`read:1:${U64}:1`);
});

test('revision acceptance rejects type coercion, unknown families and changed arity',()=>{
  for(const value of [null,undefined,true,false,1,{},[],['obs1:1:1:1'],
    'READ:1:1:1','Obs1:1:1:1','obs:1:1:1',':1:1:1','obs1:1:1','obs1:1:1:1:1']){
    rejected(value);
  }
});

test('snapshot and catalog retain separate obs1 keys without inventing expiry or capability fields',()=>{
  const snapshotRead=mapSnapshotRead(snapshot('obs1:99:4294967296:1'),scope);
  const catalogRead=mapCatalogRead(catalog('obs1:99:4294967296:2'),scope);
  assert.equal(snapshotRead.revision,'obs1:99:4294967296:1');
  assert.equal(catalogRead.revision,'obs1:99:4294967296:2');
  for(const read of [snapshotRead,catalogRead]){
    for(const key of ['expires_at','expires_in','expiry','retained_until','capabilities','authenticated','mutation_authority']){
      assert.equal(read[key],undefined);
    }
    assert.equal(read.observed_at,new Date(TIME*1000).toISOString());
  }
});
