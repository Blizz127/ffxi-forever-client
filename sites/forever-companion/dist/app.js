import {STORAGE_KEY, integer, nameOf, needs, materialLimit, missingFor, craftBlock, shoppingList, marketQuote, newSandbox, validateState, practiceCraft, practiceBid} from './domain.js';
const $ = s => document.querySelector(s);
const $$ = s => [...document.querySelectorAll(s)];
const esc = x => String(x ?? '').replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
const num = x => Number(x).toLocaleString();
let data, state, selected, view = 'craft', craft = 'wood', quantity = 1, baseOnly = true, readyOnly = false, favoritesOnly = false, marketItem = null, noticeTimer, pendingInstall;
const symbols = {wood:'♧',smith:'⚒',gold:'◇',cloth:'✂',leather:'◈',bone:'☽',alchemy:'⚗',cook:'♨'};
function itemIcon(id, large = false) {
  const crystal = id >= 4096 && id <= 4103;
  const label = crystal ? '◆' : nameOf(data,id).split(' ').map(s=>s[0]).slice(0,2).join('');
  return `<span class="item-icon ${crystal?'crystal':''} ${large?'large':''}" aria-hidden="true">${esc(label)}</span>`;
}
function notify(message) {clearTimeout(noticeTimer); $('#toast').textContent = message; $('#toast').hidden = false; noticeTimer=setTimeout(()=>$('#toast').hidden=true,4500);}
function save() {
  try {localStorage.setItem(STORAGE_KEY, JSON.stringify(state));}
  catch {notify('Browser storage is unavailable or full. Export your sandbox to keep these changes.');}
}
function run(fn) {try {fn();} catch(error) {notify(error.message);}}
function modal(html, actions = [{label:'Close'}]) {
  $('#dialog-content').innerHTML = html;
  $('#dialog-actions').replaceChildren();
  for (const a of actions) {
    const b = document.createElement('button'); b.textContent=a.label; b.className=a.primary?'primary':'secondary';
    b.addEventListener('click',()=>run(()=>{if (a.action) {const result=a.action();if(result===false)return;}$('#dialog').close();}));
    $('#dialog-actions').append(b);
  }
  $('#dialog').showModal();
}
function currentRecipe() {return data.recipes.find(r=>r.id===selected);}
function craftName(id) {return data.crafts.find(c=>c.id===id)?.name || id;}
function showView(next) {
  view = ['craft','market','planner','inventory','activity'].includes(next) ? next : 'craft';
  $$('.view').forEach(e=>e.hidden=e.id!==`view-${view}`);
  $$('.nav-button').forEach(e=>{e.classList.toggle('active',e.dataset.view===view);if(e.dataset.view===view)e.setAttribute('aria-current','page');else e.removeAttribute('aria-current');});
  $('#workspace').classList.remove('detail-open');
  history.replaceState(null,'',`#${view}`);
  document.title=`Forever Companion · ${{craft:'Crafting Log',market:'Auction House',planner:'Shopping List',inventory:'Inventory',activity:'Activity'}[view]}`;
  refresh();
  window.scrollTo({top:0,behavior:'instant'});
}
function renderTabs() {
  $('#craft-tabs').innerHTML=[{id:'all',name:'All crafts'},...data.crafts].map(c=>`<button class="tab-button ${craft===c.id?'active':''}" data-craft="${c.id}" aria-pressed="${craft===c.id}"><span aria-hidden="true">${symbols[c.id]||'✧'}</span> ${esc(c.name)}</button>`).join('');
}
function renderRecipes() {
  const query = $('#recipe-search').value.trim().toLowerCase();
  const band = $('#level-filter').value;
  const recipes = data.recipes.filter(r=> (!baseOnly || r.baseEligible) && (craft==='all'||r.craft===craft) && (band==='all'|| (band==='70'?r.level>=71:r.level>=Number(band)+1&&r.level<=Number(band)+10)) && (!readyOnly||materialLimit(r,state.inventory)>0) && (!favoritesOnly||state.favorites.includes(r.id)) && (!query||[nameOf(data,r.result.itemId),...needs(r).map(i=>nameOf(data,i.itemId))].some(n=>n.toLowerCase().includes(query)))).sort((a,b)=>a.level-b.level||nameOf(data,a.result.itemId).localeCompare(nameOf(data,b.result.itemId))||a.id-b.id);
  if (!recipes.some(r=>r.id===selected)) {selected=recipes[0]?.id; quantity=1;}
  $('#filter-count').textContent=`${num(recipes.length)} recipes`;
  $('#recipe-list').innerHTML=recipes.length ? recipes.map(r=>{
    const favorite=state.favorites.includes(r.id), available=materialLimit(r,state.inventory);
    return `<div class="recipe-row ${r.id===selected?'selected':''}"><button class="recipe-select" data-recipe="${r.id}" aria-pressed="${r.id===selected}">${itemIcon(r.result.itemId)}<span class="recipe-copy"><strong>${esc(nameOf(data,r.result.itemId))}</strong><span class="recipe-meta">${esc(craftName(r.craft))} · ${r.result.quantity>1?`Yields ${r.result.quantity} · `:''}<span class="${available?'success':'muted'}">${available?'Materials ready':'Materials needed'}</span></span></span><span class="level">${r.level}</span></button><button class="favorite ${favorite?'active':''}" data-favorite="${r.id}" aria-label="${favorite?'Unfavorite':'Favorite'} ${esc(nameOf(data,r.result.itemId))}" aria-pressed="${favorite}">${favorite?'★':'☆'}</button></div>`;
  }).join('') : '<div class="empty-state"><h3>No recipes found</h3><p>Try another craft, search, level range or filter.</p><button class="secondary" data-reset-filters>Clear filters</button></div>';
  renderDetail();
}
function renderDetail() {
  const r=currentRecipe(); if (!r){$('#detail').innerHTML='<div class="empty-state">Select a recipe to see its ingredients.</div>';return;}
  const ingredients=missingFor(r,state.inventory,quantity), missing=ingredients.filter(i=>i.missing).length, limit=materialLimit(r,state.inventory), blocked=craftBlock(r,state,quantity);
  $('#detail').innerHTML=`<button class="secondary mobile-back" id="back-recipes">‹ Recipes</button><div class="panel-heading"><span>RECIPE DETAILS</span><span>No. ${r.id}</span></div><div class="item-hero">${itemIcon(r.result.itemId,true)}<div><div class="eyebrow">${esc(craftName(r.craft))} · SKILL CAP ${r.level}</div><h2>${esc(nameOf(data,r.result.itemId))}</h2><span class="muted small">${r.result.quantity} item${r.result.quantity===1?'':'s'} per synthesis</span></div><button class="favorite ${state.favorites.includes(r.id)?'active':''}" data-favorite="${r.id}" aria-label="Toggle favorite for selected recipe" aria-pressed="${state.favorites.includes(r.id)}">${state.favorites.includes(r.id)?'★':'☆'}</button></div>
  <div class="detail-section"><div class="stat-grid">${r.skills.map(s=>`<div><span class="muted small">${esc(craftName(s.craft))}</span><strong>${state.skills[s.craft] || 0} <span class="muted small">/ ${s.level} cap</span></strong></div>`).join('')}<div><span class="muted small">Materials for</span><strong>${limit} <span class="muted small">syntheses</span></strong></div></div>${r.keyItem?`<p class="missing small">Requires crafting key item #${r.keyItem}.</p>`:''}${!r.baseEligible?'<p class="missing small">Outside the sandbox’s base-era rules.</p>':''}</div>
  <div class="detail-section"><div class="quantity-row"><h3>Materials</h3><div class="qty-control"><button data-qty="-1" aria-label="Fewer syntheses">−</button><label><span class="screen-reader">Number of syntheses</span><input id="craft-quantity" type="number" min="1" max="999" value="${quantity}" inputmode="numeric"></label><button data-qty="1" aria-label="More syntheses">+</button></div></div><div class="ingredients">${ingredients.map(i=>`<div class="ingredient-row">${itemIcon(i.itemId)}<div class="recipe-copy"><strong>${esc(nameOf(data,i.itemId))}</strong><span class="small ${i.missing?'missing':'success'}">${i.missing?`${i.missing} missing`:'Ready'}${i.itemId===r.crystal?' · Crystal':''}</span></div><span class="${i.missing?'missing':'muted'}">${num(i.have)} <span class="muted">/ ${num(i.quantity)}</span></span>${i.missing?`<button class="icon-button" data-market-item="${i.itemId}" aria-label="Search auction house for ${esc(nameOf(data,i.itemId))}">⌕</button>`:'<span class="success">✓</span>'}</div>`).join('')}</div></div>
  <div class="detail-section"><h3>High-quality results</h3><p class="muted small">${[...new Set(r.hq.filter(h=>h.itemId&&h.quantity).map(h=>`${nameOf(data,h.itemId)} × ${h.quantity}`))].map(esc).join(' · ')||'No high-quality tier listed.'}</p><p class="muted small">Real success, breaks, HQ rolls and skill-ups are decided by the game server.</p></div>
  <div class="detail-footer"><p class="small ${blocked?'muted':'success'}">${blocked?esc(blocked):'Materials and skill margin met for practice synthesis.'}</p><div class="actions"><button class="secondary" id="add-plan">＋ Add to shopping list</button><button class="primary" id="practice-craft" ${blocked?'disabled':''}>Practice synthesis <span aria-hidden="true">›</span></button></div><p class="small muted">${num(quantity*r.result.quantity)} normal result item(s) · local sandbox only</p></div>`;
}
function openMarket(itemId) {
  marketItem=Number(itemId);$('#market-search').value=nameOf(data,marketItem);showView('market');
}
function renderMarket() {
  const query=$('#market-search').value.trim().toLowerCase();
  const candidates=Object.values(data.items).filter(i=>i.auctionable && (!baseOnly||i.era==='BASE'&&i.level<=60) && (!marketItem||i.id===marketItem) && (!query||i.name.toLowerCase().includes(query)));
  $('#market-context').innerHTML=marketItem?`<p class="muted">Ingredient search: <strong>${esc(nameOf(data,marketItem))}</strong> <button class="secondary" data-view="craft">Return to recipe</button></p>`:'';
  $('#market-list').innerHTML=candidates.length ? `<div class="panel-heading"><span>ITEM · ${num(candidates.length)} RESULTS</span><span>SAMPLE MARKET</span></div>`+candidates.slice(0,100).map(i=>{
    const q=marketQuote(data,i.id), stock=Math.max(0,q.stock-(state.marketSold[i.id]||0));
    return `<div class="inventory-row">${itemIcon(i.id)}<div class="recipe-copy"><strong>${esc(i.name)}</strong><span class="muted small">${stock} sample units · ${esc(i.auctionCategory.replaceAll('_',' ').toLowerCase())}</span></div><span class="gold">${num(q.price)}<span class="muted small"> gil / unit</span></span><button class="secondary" data-bid="${i.id}" ${stock?'':'disabled'}>Practice bid</button></div>`;
  }).join('')+(candidates.length>100?'<p class="muted small">Showing 100 results. Search to narrow the list.</p>':'') : '<div class="empty-state"><h3>No auctionable items found</h3><p>Some ingredients cannot be auctioned. Try another item or clear your search.</p></div>';
}
function bidDialog(id) {
  const q=marketQuote(data,id); if (!q)return;
  const suggested=shoppingList(data,state.plan,state.inventory).find(i=>i.itemId===id)?.missing||missingFor(currentRecipe()||data.recipes[0],state.inventory,quantity).find(i=>i.itemId===id)?.missing||1;
  modal(`<div class="eyebrow">LOCAL AUCTION SIMULATION</div><h2>Bid on ${esc(nameOf(data,id))}</h2><p class="muted">Sample asking price: ${num(q.price)} gil per unit. A successful bid charges your offered amount.</p><div class="form-grid"><label class="field">Units<input id="bid-quantity" type="number" min="1" max="999" value="${Math.min(suggested,999)}"></label><label class="field">Bid per unit (gil)<input id="bid-price" type="number" min="1" value="${q.price}"></label></div><p class="muted">This changes your local sample inventory and gil. No game purchase is made.</p>`,[{label:'Cancel'},{label:'Confirm practice bid',primary:true,action:()=>{const cost=practiceBid(data,state,id,$('#bid-quantity').value,$('#bid-price').value);save();refresh();notify(`Practice bid won · ${num(cost)} sandbox gil spent.`);}}]);
}
function renderPlan() {
  $('#plan-recipes').innerHTML=state.plan.length?'<div class="panel-heading">PLANNED RECIPES</div>'+state.plan.map(p=>{const r=data.recipes.find(r=>r.id===p.recipeId);return `<div class="inventory-row">${itemIcon(r.result.itemId)}<div class="recipe-copy"><strong>${esc(nameOf(data,r.result.itemId))}</strong><span class="small muted">${p.quantity} syntheses · ${p.quantity*r.result.quantity} result items</span></div><button class="icon-button" data-remove-plan="${r.id}" aria-label="Remove ${esc(nameOf(data,r.result.itemId))}">×</button></div>`;}).join(''):'<div class="empty-state"><h2>Your next creation starts here.</h2><p>Add a recipe from the crafting log to combine its ingredients into one shopping list.</p><button class="primary" data-view="craft">Browse recipes</button></div>';
  const list=shoppingList(data,state.plan,state.inventory);
  $('#plan-materials').innerHTML=list.length?`<div class="panel-heading"><span>COMBINED MATERIALS</span><span>${list.filter(i=>i.missing).length} TO GATHER</span></div>`+list.sort((a,b)=>b.missing-a.missing).map(i=>`<div class="ingredient-row">${itemIcon(i.itemId)}<div class="recipe-copy"><strong>${esc(nameOf(data,i.itemId))}</strong><span class="small muted">${num(i.have)} owned / ${num(i.quantity)} required</span></div><span class="${i.missing?'missing':'success'}">${i.missing?`${num(i.missing)} missing`:'Ready'}</span>${i.missing?`<button class="secondary" data-market-item="${i.itemId}">Find on AH</button>`:''}</div>`).join(''):'';
}
function renderInventory() {
  $('#skills').innerHTML=data.crafts.map(c=>`<label class="field"><span>${esc(c.name)}</span><input data-skill="${c.id}" type="number" min="0" max="70" value="${state.skills[c.id] || 0}" aria-label="${esc(c.name)} sandbox skill"></label>`).join('');
  const query=$('#inventory-search').value.trim().toLowerCase();
  const items=Object.values(data.items).filter(i=>(query?i.name.toLowerCase().includes(query):(state.inventory[i.id]||0)>0)).sort((a,b)=>a.name.localeCompare(b.name));
  $('#inventory-list').innerHTML=items.length?items.slice(0,100).map(i=>`<div class="inventory-row">${itemIcon(i.id)}<div class="recipe-copy"><strong>${esc(i.name)}</strong><span class="small muted">Item ${i.id}${query&&!(state.inventory[i.id]>0)?' · not held':''}</span></div><label class="field"><span class="screen-reader">${esc(i.name)} quantity</span><input data-stock="${i.id}" type="number" min="0" max="99999" value="${state.inventory[i.id]||0}"></label></div>`).join('')+(items.length>100?'<p class="muted small">Showing 100 matches. Narrow your search.</p>':''):'<div class="empty-state"><p>No items found. Search the catalog to add supplies.</p></div>';
}
function renderActivity() {
  $('#activity-list').innerHTML=state.activity.length?state.activity.map(a=>`<div class="inventory-row"><span class="success" aria-hidden="true">✧</span><div><strong>${esc(a.text)}</strong><p class="small muted">${esc(new Date(a.time).toLocaleString())}</p></div></div>`).join(''):'<div class="empty-state"><h2>A fresh page.</h2><p>Your practice bids and synthesis will appear here.</p></div>';
}
function refresh() {
  $('#plan-count').textContent=state.plan.reduce((s,p)=>s+p.quantity,0);
  $('#market-gil').textContent=$('#inventory-gil').textContent=`${num(state.gil)} sandbox gil`;
  if(view==='craft'){renderTabs();renderRecipes();}
  if(view==='market')renderMarket();
  if(view==='planner')renderPlan();
  if(view==='inventory')renderInventory();
  if(view==='activity')renderActivity();
}
function about() {
  modal(`<div class="eyebrow">FOREVER COMPANION · PREVIEW</div><h2>Your workshop, wherever you are.</h2><p>Browse the same ${num(data.recipes.length)} recipes bundled in the Forever client, track materials and plan auction-house ingredients.</p><p><strong>Local sandbox:</strong> the sample character, inventory, gil, asking prices and stock are stored in this browser. Practice synthesis guarantees a normal result and does not model success chances, breaks or skill-ups.</p><p><strong>Game connection pending:</strong> real character login, purchases and crafting need the server companion API. This app does not connect to your game account.</p><p class="muted">Install from your browser’s menu on a supported desktop or phone. Offline browsing is available after the first online visit on HTTPS or localhost. Your browser data stays on that device; export to transfer it.</p><p class="muted small"><a href="licenses/THIRD_PARTY_NOTICES.txt" target="_blank" rel="noopener">Data attribution & licenses</a></p><p class="muted small">Public item labels are formatted from server SQL identifiers. No game artwork or localized DAT assets are included. Interface inspired by a crafting log; uses FFXI’s recipes and rules.</p>`);
}
function bind() {
  document.addEventListener('click',e=>run(()=>{
    const b=e.target.closest('button');if(!b || b.disabled)return;
    if(b.dataset.view){showView(b.dataset.view);return;}
    if(b.dataset.craft){craft=b.dataset.craft;$('#workspace').classList.remove('detail-open');refresh();return;}
    if(b.dataset.recipe){selected=Number(b.dataset.recipe);quantity=1;renderRecipes();$('#workspace').classList.add('detail-open');if(matchMedia('(max-width:700px)').matches)$('#detail').scrollIntoView({block:'start',behavior:'instant'});return;}
    if(b.dataset.favorite){const id=Number(b.dataset.favorite);state.favorites=state.favorites.includes(id)?state.favorites.filter(x=>x!==id):[...state.favorites,id];save();refresh();return;}
    if(b.dataset.qty){quantity=Math.max(1,Math.min(999,quantity+Number(b.dataset.qty)));renderDetail();return;}
    if(b.dataset.marketItem){openMarket(b.dataset.marketItem);return;}
    if(b.dataset.bid){bidDialog(Number(b.dataset.bid));return;}
    if(b.dataset.removePlan){state.plan=state.plan.filter(p=>p.recipeId!==Number(b.dataset.removePlan));save();refresh();return;}
    if(b.hasAttribute('data-reset-filters')){craft='all';$('#recipe-search').value='';$('#level-filter').value='all';readyOnly=false;favoritesOnly=false;$('#ready-filter').classList.remove('active');$('#ready-filter').setAttribute('aria-pressed','false');$('#favorite-filter').classList.remove('active');$('#favorite-filter').setAttribute('aria-pressed','false');refresh();return;}
    if(b.id==='back-recipes'){$('#workspace').classList.remove('detail-open');$('.recipe-panel').scrollIntoView({block:'start',behavior:'instant'});}
    if(b.id==='add-plan'){
      const p=state.plan.find(p=>p.recipeId===selected);
      if(p) p.quantity=integer(p.quantity+quantity,1,999);else state.plan.push({recipeId:selected,quantity});
      save();refresh();notify('Recipe added to your shopping list.');
    }
    if(b.id==='practice-craft'){
      const r=currentRecipe(), id=r.id, count=quantity;
      modal(`<div class="eyebrow">LOCAL PRACTICE SYNTHESIS</div><h2>Make ${esc(nameOf(data,r.result.itemId))}?</h2><p>Use materials for ${count} syntheses and add ${count*r.result.quantity} normal result item(s) to your sandbox.</p><p class="muted">Practice always succeeds. No real game items, HQ rolls, breaks or skill-ups are involved.</p>`,[{label:'Cancel'},{label:'Confirm practice synthesis',primary:true,action:()=>{const made=practiceCraft(data,state,id,count);save();refresh();notify(`Practice complete · ${made} ${nameOf(data,r.result.itemId)} added.`);}}]);
    }
  }));
  $('#recipe-search').addEventListener('input',renderRecipes);
  $('#level-filter').addEventListener('change',renderRecipes);
  for(const [id,get,set] of [['era-filter',()=>baseOnly,x=>baseOnly=x],['ready-filter',()=>readyOnly,x=>readyOnly=x],['favorite-filter',()=>favoritesOnly,x=>favoritesOnly=x]])$('#'+id).addEventListener('click',()=>{set(!get());$('#'+id).classList.toggle('active',get());$('#'+id).setAttribute('aria-pressed',get());renderRecipes();});
  $('#market-search').addEventListener('input',()=>{marketItem=null;renderMarket();});
  $('#market-clear').addEventListener('click',()=>{marketItem=null;$('#market-search').value='';renderMarket();});
  $('#inventory-search').addEventListener('input',renderInventory);
  document.addEventListener('change',e=>run(()=>{
    if(e.target.id==='craft-quantity'){try{quantity=integer(e.target.value,1,999);}finally{renderDetail();}}
    if(e.target.dataset.skill){const key=e.target.dataset.skill;try{state.skills[key]=integer(e.target.value,0,70);save();}finally{e.target.value=state.skills[key];}}
    if(e.target.dataset.stock){const key=e.target.dataset.stock;try{state.inventory[key]=integer(e.target.value,0,99999);save();notify('Sandbox inventory updated.');}finally{e.target.value=state.inventory[key]||0;}}
  }));
  $('#help').addEventListener('click',about);
  $('#clear-plan').addEventListener('click',()=>{if(!state.plan.length)return;modal('<h2>Clear your shopping list?</h2><p>Your inventory and favorites will stay saved.</p>',[{label:'Cancel'},{label:'Clear list',action:()=>{state.plan=[];save();refresh();}}]);});
  $('#reset').addEventListener('click',()=>modal('<h2>Start a fresh sandbox?</h2><p>This replaces the sample inventory, gil, skills, favorites, shopping list and activity saved in this browser.</p>',[{label:'Cancel'},{label:'Reset sandbox',action:()=>{state=newSandbox(data);save();refresh();notify('A fresh workshop is ready.');}}]));
  $('#export').addEventListener('click',()=>{const url=URL.createObjectURL(new Blob([JSON.stringify(state,null,2)],{type:'application/json'}));const a=document.createElement('a');a.href=url;a.download='forever-companion-sandbox.json';a.click();setTimeout(()=>URL.revokeObjectURL(url),5000);});
  $('#import').addEventListener('click',()=>$('#import-file').click());
  $('#import-file').addEventListener('change',async()=>{
    const f=$('#import-file').files[0];$('#import-file').value='';if(!f)return;
    try {if(f.size>1024*1024)throw new Error('Choose a sandbox export smaller than 1 MB.');const imported=validateState(JSON.parse(await f.text()),data);modal('<h2>Replace this sandbox?</h2><p>The imported file replaces your local inventory, gil, skills, favorites, shopping list and activity. It does not affect a game character.</p>',[{label:'Cancel'},{label:'Import sandbox',action:()=>{state=imported;save();refresh();notify('Sandbox imported.');}}]);}catch(error){notify(`Import failed: ${error.message}`);}
  });
  $('#install').addEventListener('click',async()=>{if(pendingInstall){await pendingInstall.prompt();pendingInstall=null;$('#install').hidden=true;}else about();});
  window.addEventListener('hashchange',()=>showView(location.hash.slice(1)));
}
window.addEventListener('beforeinstallprompt',e=>{e.preventDefault();pendingInstall=e;$('#install').hidden=false;});
window.addEventListener('appinstalled',()=>$('#install').hidden=true);
async function start() {
  try {
    const response=await fetch('./data/recipes.json');if(!response.ok)throw new Error('Recipe catalog unavailable.');data=await response.json();
    let raw;try{raw=localStorage.getItem(STORAGE_KEY);}catch{}
    try{state=raw?validateState(JSON.parse(raw),data):newSandbox(data);}catch{state=newSandbox(data);notify('Saved sandbox could not be read. A fresh local sandbox was loaded.');}
    $('#catalog-count').textContent=`${num(data.recipes.length)} recipes · 8 crafts`;
    $('#footer-count').textContent=`· ${num(data.meta.baseEligibleCount)} base-era recipes`;
    bind();showView(location.hash.slice(1));
    if('serviceWorker' in navigator)navigator.serviceWorker.register('./sw.js').catch(()=>{});
  } catch(error) {$('#recipe-list').innerHTML=`<div class="empty-state"><h2>Couldn’t open your workshop</h2><p>${esc(error.message)}</p><p>Start the local server with npm start and reload.</p></div>`;}
}
start();
