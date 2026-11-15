/**
 * Pure projection of native read DTOs into the proposed companion BFF, including
 * the additive retained-observation and owner-receipt read contracts.
 * This module performs no authentication, I/O or capability activation. A future
 * trusted BFF must verify the session and own-character selector before calling
 * the native boundary. Read revisions never grant authority; obs1 identifies a
 * server-retained projection lookup, not complete mutation state or readiness.
 */
const U16 = 0xffff;
const U32 = 0xffffffff;
const U64 = (1n << 64n) - 1n;
const MAX_REPLY_BYTES = 2 * 1024 * 1024;
const CRAFTS = new Set(['wood','smith','gold','cloth','leather','bone','alchemy','cook']);
const RECEIPT_ID = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
const RECEIPT_STATES = new Set(['queued','running','succeeded','failed','rejected','cancelled']);
const object = value => value !== null && typeof value === 'object' && !Array.isArray(value);
const utf8 = new TextEncoder();

export class MappingUnavailable extends Error {
  constructor(reason = 'Invalid native read response.') {
    super('The native read response is unavailable.');
    this.name = 'MappingUnavailable';
    this.code = 'unavailable';
    this.status = 503;
    this.reason = reason; // Fixed field labels only; never retain raw payloads or credentials.
  }
}
function fail(reason) {throw new MappingUnavailable(reason);}
function integer(value, min, max, label) {
  if (typeof value !== 'number' || !Number.isSafeInteger(value) || value < min || value > max) fail(label);
  return value;
}
function text(value, maxBytes, label, {empty = false, visible = false} = {}) {
  if (typeof value !== 'string' || (!empty && !value.length) || utf8.encode(value).byteLength > maxBytes || /[\p{Cc}]/u.test(value)) fail(label);
  if (visible && !/^[\x21-\x7e]+$/.test(value)) fail(label);
  return value;
}
function array(value, max, label, min = 0) {
  if (!Array.isArray(value) || value.length < min || value.length > max) fail(label);
  return value;
}
function freeze(value) {
  if (value && typeof value === 'object') {Object.values(value).forEach(freeze);Object.freeze(value);}
  return value;
}
function native(core, kind) {
  if (!object(core) || core.kind !== kind) fail('native variant');
  try {
    if (utf8.encode(JSON.stringify(core)).byteLength > MAX_REPLY_BYTES) fail('native reply capacity');
  } catch (error) {
    if (error instanceof MappingUnavailable) throw error;
    fail('native JSON');
  }
  return core;
}
function selector(value, label = 'character selector', maximum = U32) {return integer(value, 1, maximum, label);}
function selectors(value, keys) {
  if (!object(value) || Object.keys(value).some(key => !keys.includes(key))) fail('read selectors');
  return value;
}
function observationRevision(value) {
  text(value, 128, 'read revision', {visible:true});
  const parts = /^(read|obs1):([1-9][0-9]*):([1-9][0-9]*):([1-9][0-9]*)$/.exec(value);
  if (!parts) fail('read revision');
  const middleMaximum = parts[1] === 'read' ? BigInt(U32) : U64;
  if (BigInt(parts[2]) > U64 || BigInt(parts[3]) > middleMaximum || BigInt(parts[4]) > U64) fail('read revision');
  return value;
}
/** Strict Unix seconds -> representable canonical UTC ISO. Invalid input never becomes zero. */
export function unixSecondsToISO(value) {
  integer(value, 0, 8_640_000_000_000, 'Unix timestamp');
  const milliseconds = value * 1000;
  if (!Number.isSafeInteger(milliseconds)) fail('Unix timestamp');
  const date = new Date(milliseconds);
  if (!Number.isFinite(date.getTime())) fail('Unix timestamp');
  try {return date.toISOString();} catch {return fail('Unix timestamp');}
}
function itemCount(value, label) {
  if (!object(value)) fail(label);
  return {item_id:selector(value.item_id, label, U16),quantity:integer(value.quantity,1,U32,label)};
}
function skills(value) {
  if (!object(value) || Object.keys(value).length > 8 || Object.keys(value).some(key=>!CRAFTS.has(key))) fail('craft skills');
  return Object.fromEntries(Object.entries(value).map(([key,level])=>[key,integer(level,0,U16,'craft skill')]));
}
function characterProjection(value, expectedId) {
  if (!object(value) || value.character_id !== expectedId) fail('read character identity');
  return {
    character_id:expectedId,
    revision:observationRevision(value.revision),
    observed_at:unixSecondsToISO(value.observed_at),
    era:text(value.era,4096,'era label',{visible:true}),
    craft_cap:integer(value.craft_cap,0,U16,'craft cap'),
  };
}
/** Return display selectors only. This deliberately cannot mint auth, CSRF or capabilities. */
export function mapSessionRead(core) {
  const response=native(core,'session');
  const seen=new Set();
  const characters=array(response.characters,32,'character capacity').map(character=>{
    if (!object(character)) fail('character entry');
    const id=selector(character.character_id);
    if (seen.has(id)) fail('duplicate character');
    seen.add(id);
    return {id,name:text(character.name,32,'character name')};
  });
  return freeze({characters});
}
export function mapSnapshotRead(core, options) {
  const {characterId}=selectors(options,['characterId']);
  selector(characterId);
  const value=native(core,'snapshot').snapshot;
  const projection=characterProjection(value,characterId);
  if (value.online !== true) fail('current human Session required');
  const seen=new Set();
  const inventory=array(value.inventory,U16,'inventory capacity').map(row=>{
    const item=itemCount(row,'inventory item');
    if (seen.has(item.item_id)) fail('duplicate aggregated inventory item');
    seen.add(item.item_id);return item;
  });
  const keySeen=new Set();
  const key_items=array(value.key_items,4096,'key item capacity').map(key=>{
    integer(key,1,4095,'recipe key item');
    if (keySeen.has(key)) fail('duplicate key item');
    keySeen.add(key);return key;
  });
  return freeze({
    ...projection,
    character_name:text(value.character_name,32,'character name'),
    online:true,
    gil:integer(value.gil,0,U32,'gil'),
    free_inventory_slots:integer(value.free_inventory_slots,0,U16,'free inventory slots'),
    inventory,skills:skills(value.skills),key_items,
  });
}
export function mapCatalogRead(core, options) {
  const {characterId}=selectors(options,['characterId']);
  selector(characterId);
  const value=native(core,'recipes').catalog;
  const projection=characterProjection(value,characterId);
  const seen=new Set();
  const recipes=array(value.recipes,4096,'recipe capacity').map(recipe=>{
    if (!object(recipe)) fail('recipe');
    const recipe_id=selector(recipe.recipe_id,'recipe ID');
    if (seen.has(recipe_id)) fail('duplicate recipe');
    seen.add(recipe_id);
    const ingredients=array(recipe.ingredients,8,'ingredient positions',1).map(row=>itemCount(row,'ingredient'));
    const results=array(recipe.results,4,'result tiers',4).map(row=>row===null?null:itemCount(row,'result'));
    if (results[0]===null) fail('normal result');
    const key_item=recipe.key_item===null?null:integer(recipe.key_item,1,4095,'recipe key item');
    return {recipe_id,crystal_id:selector(recipe.crystal_id,'crystal ID',U16),ingredients,results,
      skills:skills(recipe.skills),key_item};
  });
  return freeze({...projection,recipes});
}
export function mapMarketRead(core, options) {
  const {characterId,itemId,stack}=selectors(options,['characterId','itemId','stack']);
  selector(characterId);selector(itemId,'auction item',U16);
  if (typeof stack !== 'boolean') fail('single/stack selector');
  const market=native(core,'market').market;
  if (!object(market) || market.item_id!==itemId || market.stack!==stack) fail('market selector identity');
  const single_listings=integer(market.single_listings,0,U32,'single listing count');
  const stack_listings=integer(market.stack_listings,0,U32,'stack listing count');
  const observed_at=unixSecondsToISO(market.observed_at);
  const history=array(market.history,10,'sale history capacity').map(sale=>{
    if (!object(sale)) fail('sale history');
    return {price:integer(sale.price,0,U32,'sale price'),sold_at:unixSecondsToISO(sale.sold_at),
      seller:text(sale.seller,MAX_REPLY_BYTES,'seller name',{empty:true}),
      buyer:text(sale.buyer,MAX_REPLY_BYTES,'buyer name',{empty:true})};
  });
  // Preserve the native newest-ten, oldest-first sequence, including equal-time ties.
  // character_id is request context from the verified BFF caller, not new ownership evidence.
  return freeze({character_id:characterId,item_id:itemId,stack,stock:stack?stack_listings:single_listings,
    stock_unit:'listings',single_listings,stack_listings,history,observed_at});
}

/** Select the dedicated native read_receipt entry, never generic read/read_retained. */
export function receiptReadRequest(operationId) {
  text(operationId,36,'receipt operation ID',{visible:true});
  if (!RECEIPT_ID.test(operationId)) fail('receipt operation ID');
  return freeze({kind:'receipt',operation_id:operationId});
}

/**
 * ownedCharacterIds must come from current independently verified BFF ownership,
 * never from browser selectors or the native payload. This pure mapper cannot
 * authenticate an owner or establish the native committed-read proof itself.
 * Receipt reads require no online Session, snapshot or mutation capability.
 */
export function mapReceiptRead(core, options) {
  const {operationId,ownedCharacterIds}=selectors(options,['operationId','ownedCharacterIds']);
  receiptReadRequest(operationId);
  const owned=new Set();
  for (const characterId of array(ownedCharacterIds,32,'receipt owner characters',1)) {
    selector(characterId,'receipt owner character');
    if (owned.has(characterId)) fail('duplicate receipt owner character');
    owned.add(characterId);
  }
  const response=native(core,'receipt');
  if (Object.keys(response).some(key=>!['kind','receipt'].includes(key))) fail('receipt wrapper');
  const value=response.receipt;
  if (!object(value) || Object.keys(value).some(key=>!['operation_id','character_id','state','result','code'].includes(key))) fail('receipt fields');
  if (value.operation_id!==operationId) fail('receipt operation identity');
  selector(value.character_id,'receipt character');
  if (!owned.has(value.character_id)) fail('receipt owner character');
  if (!RECEIPT_STATES.has(value.state)) fail('receipt state');
  if (value.state==='succeeded' ? !object(value.result) : value.result!=null) fail('receipt result');
  if (value.code!=null && (typeof value.code!=='string' || !/^[a-z0-9_]{1,64}$/.test(value.code))) fail('receipt code');
  // Clone the authoritative JSON result before freezing, without freezing or
  // retaining a reference to the provider's input. No local game state changes.
  const receipt=JSON.parse(JSON.stringify({operation_id:operationId,character_id:value.character_id,state:value.state,
    ...(value.state==='succeeded'?{result:value.result}:{}),...(value.code==null?{}:{code:value.code})}));
  if (receipt.state==='succeeded' && !object(receipt.result)) fail('receipt result');
  return freeze(receipt);
}
