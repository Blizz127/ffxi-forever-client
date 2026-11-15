export const STORAGE_KEY = 'forever-companion-v1';
export const MAX_QUANTITY = 999;
export function integer(value, min = 0, max = 1_000_000_000) {
  if ((typeof value !== 'number' && typeof value !== 'string') || (typeof value === 'string' && !value.trim())) throw new Error('Enter a whole number.');
  const n = Number(value);
  if (!Number.isSafeInteger(n) || n < min || n > max) throw new Error(`Enter a whole number from ${min} to ${max.toLocaleString()}.`);
  return n;
}
export function nameOf(data, id) { return data.items[id]?.name || `Item ${id}`; }
export function needs(recipe, batches = 1) {
  batches = integer(batches, 1, MAX_QUANTITY);
  const counts = new Map([[recipe.crystal, batches]]);
  for (const i of recipe.ingredients) counts.set(i.itemId, (counts.get(i.itemId) || 0) + i.quantity * batches);
  return [...counts].map(([itemId, quantity]) => ({itemId, quantity}));
}
export function materialLimit(recipe, inventory) {
  return Math.min(...needs(recipe).map(i => Math.floor((inventory[i.itemId] || 0) / i.quantity)));
}
export function missingFor(recipe, inventory, batches = 1) {
  return needs(recipe, batches).map(i => ({...i, have:inventory[i.itemId] || 0, missing:Math.max(0, i.quantity - (inventory[i.itemId] || 0))}));
}
export function craftBlock(recipe, state, batches = 1) {
  if (!recipe.baseEligible) return 'This recipe is outside the sandbox’s base-era rules.';
  if (recipe.keyItem) return 'This recipe needs a crafting key item. Key items are not available in this sandbox.';
  if (recipe.skills.some(s => (state.skills[s.craft] || 0) < Math.max(0, s.level - 15))) return 'A required crafting skill is too low (more than 15 below the recipe).';
  if (materialLimit(recipe, state.inventory) < batches) return 'Gather the missing materials first.';
  return '';
}
export function shoppingList(data, plan, inventory) {
  const counts = new Map();
  for (const line of plan) {
    const recipe = data.recipes.find(r => r.id === line.recipeId);
    if (!recipe) continue;
    for (const i of needs(recipe, line.quantity)) counts.set(i.itemId, (counts.get(i.itemId) || 0) + i.quantity);
  }
  return [...counts].map(([itemId, quantity]) => ({itemId,quantity,have:inventory[itemId] || 0,missing:Math.max(0,quantity - (inventory[itemId] || 0))}));
}
export function marketQuote(data, itemId) {
  const item = data.items[itemId];
  if (!item?.auctionable) return null;
  return {price: 90 + (Number(itemId) * 37 % 1850), stock: 12 + (Number(itemId) * 13 % 65)};
}
export function newSandbox(data) {
  const state = {version:1,name:'Workshop Adventurer',gil:50000,inventory:{},skills:Object.fromEntries(data.crafts.map(c => [c.id, 10])),favorites:[],plan:[],marketSold:{},activity:[]};
  const samples = data.recipes.filter(r => r.baseEligible && !r.keyItem && r.level <= 12);
  for (const c of data.crafts) {
    for (const r of samples.filter(r => r.craft === c.id).slice(0,3)) {
      for (const i of needs(r)) state.inventory[i.itemId] = Math.max(state.inventory[i.itemId] || 0, i.quantity * 6);
    }
  }
  return state;
}
export function validateState(raw, data) {
  if (!raw || raw.version !== 1 || typeof raw !== 'object') throw new Error('This is not a Forever Companion sandbox export.');
  const state = newSandbox(data);
  state.gil = integer(raw.gil);
  state.name = typeof raw.name === 'string' ? raw.name.slice(0,40) : state.name;
  state.inventory = {};
  if (!raw.inventory || typeof raw.inventory !== 'object' || Array.isArray(raw.inventory)) throw new Error('Invalid inventory.');
  for (const [id, count] of Object.entries(raw.inventory)) {if (!Object.hasOwn(data.items,id)) throw new Error(`Unknown item ${id}.`); state.inventory[id] = integer(count,0,99999);}
  for (const c of data.crafts) state.skills[c.id] = integer(raw.skills?.[c.id] ?? 0,0,70);
  state.favorites = [...new Set((Array.isArray(raw.favorites) ? raw.favorites : []).filter(id => data.recipes.some(r => r.id === id)))];
  state.plan = [];
  for (const p of (Array.isArray(raw.plan) ? raw.plan : []).slice(0,1004)) {
    if (!data.recipes.some(r => r.id === p.recipeId) || state.plan.some(x => x.recipeId === p.recipeId)) continue;
    state.plan.push({recipeId:p.recipeId,quantity:integer(p.quantity,1,MAX_QUANTITY)});
  }
  state.marketSold = {};
  for (const [id,count] of Object.entries(raw.marketSold || {})) {if (Object.hasOwn(data.items,id)) state.marketSold[id] = integer(count,0,1000000);}
  state.activity = (Array.isArray(raw.activity) ? raw.activity : []).slice(0,100).filter(a => typeof a.text === 'string' && typeof a.time === 'string').map(a=>({text:a.text.slice(0,250),time:a.time.slice(0,50)}));
  return state;
}
function log(state, text) {state.activity.unshift({text,time:new Date().toISOString()}); state.activity = state.activity.slice(0,100);}
export function practiceCraft(data, state, recipeId, batches = 1) {
  const recipe = data.recipes.find(r => r.id === recipeId);
  if (!recipe) throw new Error('Recipe not found.');
  batches = integer(batches,1,MAX_QUANTITY);
  const block = craftBlock(recipe,state,batches);
  if (block) throw new Error(block);
  const inventory = {...state.inventory};
  for (const i of needs(recipe,batches)) inventory[i.itemId] -= i.quantity;
  const id = recipe.result.itemId;
  inventory[id] = (inventory[id] || 0) + recipe.result.quantity * batches;
  if (inventory[id] > 99999) throw new Error('Sandbox item limit reached.');
  state.inventory = inventory;
  log(state,`Practice synthesis: ${batches} × ${nameOf(data,id)} → ${recipe.result.quantity * batches} item(s). Guaranteed normal result in sandbox.`);
  return recipe.result.quantity * batches;
}
export function practiceBid(data, state, itemId, quantity, bid) {
  const quote = marketQuote(data,itemId);
  quantity = integer(quantity,1,MAX_QUANTITY); bid = integer(bid,1);
  if (!quote) throw new Error('This item cannot be auctioned.');
  if (quantity > quote.stock - (state.marketSold[itemId] || 0)) throw new Error('Not enough sample stock remains.');
  if (bid < quote.price) throw new Error('Sample bid unsuccessful. Offer at least the sample asking price.');
  const total = quantity * bid;
  if (!Number.isSafeInteger(total) || total > state.gil) throw new Error('Not enough sandbox gil.');
  if ((state.inventory[itemId] || 0) + quantity > 99999) throw new Error('Sandbox item limit reached.');
  state.gil -= total;
  state.inventory[itemId] = (state.inventory[itemId] || 0) + quantity;
  state.marketSold[itemId] = (state.marketSold[itemId] || 0) + quantity;
  log(state,`Practice bid won: ${quantity} × ${nameOf(data,itemId)} for ${total.toLocaleString()} gil. Sample inventory updated.`);
  return total;
}
