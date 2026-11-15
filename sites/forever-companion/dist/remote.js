/**
 * Proposed live companion API client. This module is not enabled by app.js.
 * A same-origin authenticated BFF and authoritative game-server routes are required.
 * It never reads or writes sandbox state, native credentials, or browser storage.
 */
const API = '/api/v1';
const TERMINAL = new Set(['succeeded', 'failed', 'rejected', 'cancelled', 'expired']);
const STATES = new Set(['queued', 'running', 'uncertain', ...TERMINAL]);
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;
const CAPABILITIES = ['snapshot', 'recipes', 'auction', 'craft', 'bid'];
const object = value => value !== null && typeof value === 'object' && !Array.isArray(value);
function freeze(value) {
  if (value && typeof value === 'object') {Object.values(value).forEach(freeze); Object.freeze(value);}
  return value;
}
function projection(value) {return freeze(JSON.parse(JSON.stringify(value)));}
function invalid(message) {throw new RemoteError('invalid_input', message);}
function id(value, label, max = 0xffffffff) {
  if (typeof value !== 'number' || !Number.isSafeInteger(value) || value < 1 || value > max) invalid(`Invalid ${label}.`);
  return value;
}
function revision(value) {
  if (typeof value !== 'string' || !/^[\x21-\x7e]{1,128}$/.test(value)) invalid('A fresh server revision is required.');
  return value;
}
function operationId(value) {if (typeof value !== 'string' || !UUID.test(value)) invalid('Invalid operation ID.'); return value.toLowerCase();}
function deviceId(value) {if (typeof value !== 'string' || !/^[a-zA-Z0-9_-]{8,128}$/.test(value)) invalid('Invalid device sign-in request.'); return value;}
function protocol(message) {throw new RemoteError('invalid_response', message);}
function isoTime(value, label = 'observation') {
  if (typeof value !== 'string') protocol(`Invalid ${label} time.`);
  const time = Date.parse(value);
  if (!Number.isFinite(time) || new Date(time).toISOString() !== value) protocol(`Invalid ${label} UTC time.`);
  return time;
}
const uint32 = value => Number.isSafeInteger(value) && value >= 0 && value <= 0xffffffff;


export class RemoteError extends Error {
  constructor(code, message, {status = 0, operationId, cause} = {}) {
    super(message, cause ? {cause} : undefined);
    this.name = 'RemoteError'; this.code = code; this.status = status;
    if (operationId) this.operationId = operationId;
  }
}
export class UnknownOutcomeError extends RemoteError {
  constructor(id, cause) {
    super('unknown_outcome', 'The result is not yet confirmed. Check the same operation ID before submitting another action.', {operationId:id, cause});
    this.name = 'UnknownOutcomeError';
  }
}

export class LiveCompanion {
  #fetch; #crypto; #timeout; #csrf = null; #session = null; #prepared = new WeakSet(); #snapshots = new Map(); #epoch = 0;
  constructor({fetch: fetcher = globalThis.fetch?.bind(globalThis), crypto = globalThis.crypto, timeoutMs = 15000} = {}) {
    if (typeof fetcher !== 'function' || !Number.isSafeInteger(timeoutMs) || timeoutMs < 1 || timeoutMs > 120000) invalid('Invalid API client configuration.');
    this.#fetch = fetcher; this.#crypto = crypto; this.#timeout = timeoutMs;
  }
  #clearSession() {this.#epoch++; this.#csrf = null; this.#session = null; this.#prepared = new WeakSet(); this.#snapshots.clear();}
  #assertEpoch(epoch) {if (epoch !== this.#epoch) throw new RemoteError('stale_session', 'The companion session changed while the request was running.');}
  #scope(characterId, capability) {
    if (!this.#session?.authenticated) throw new RemoteError('authentication_required', 'Sign in to use your game character.');
    if (characterId !== undefined && !this.#session.characters.some(c => c.id === characterId)) throw new RemoteError('forbidden', 'That character is outside this session.');
    if (capability && this.#session.capabilities[capability] !== true) throw new RemoteError('unavailable', 'The server has not enabled this companion feature.');
  }
  #evidence(characterId, expectedRevision) {
    const snapshot = this.#snapshots.get(characterId);
    if (!snapshot || Date.now() - snapshot.receivedAt > 30000 || Math.abs(Date.now() - snapshot.observedAt) > 30000) throw new RemoteError('stale_snapshot', 'Read a fresh character snapshot before submitting an action.');
    if (!snapshot.online) throw new RemoteError('character_offline', 'This server increment requires your character to be online.');
    // ReadService's nonce identifies an observation; it is not complete native
    // mutation state, even if a future/mock session advertises an action scope.
    if (snapshot.revision.startsWith('read:')) throw new RemoteError('read_only_snapshot', 'This observation is for viewing and cannot authorize crafting or bids.');
    if (snapshot.revision !== expectedRevision) throw new RemoteError('stale_revision', 'Refresh the character snapshot before preparing another action.');
  }
  async #request(path, {method = 'GET', body, csrf = false, operationId: op} = {}) {
    const requestEpoch = this.#epoch;
    if (csrf && !this.#csrf) throw new RemoteError('csrf_required', 'Refresh the companion session before submitting an action.');
    const headers = {'Accept':'application/json'};
    if (body !== undefined) headers['Content-Type'] = 'application/json';
    if (csrf) headers['X-CSRF-Token'] = this.#csrf;
    const controller = new AbortController(); let timer;
    const timedOut = new Promise((_, reject) => {
      timer = setTimeout(() => {controller.abort(); reject(new RemoteError('timeout', 'The companion server did not reply in time.', {operationId:op}));}, this.#timeout);
    });
    try {
      return await Promise.race([timedOut, (async () => {
        let response;
        try {
          response = await this.#fetch(API + path, {method, headers, body:body === undefined ? undefined : JSON.stringify(body), credentials:'same-origin', mode:'same-origin', redirect:'error', cache:'no-store', signal:controller.signal});
        } catch (cause) {throw new RemoteError('network_error', 'The companion server could not be reached.', {operationId:op, cause});}
        if (response.status === 401 && requestEpoch === this.#epoch) this.#clearSession();
        let payload = null;
        if (response.status !== 204) {
          try {payload = await response.json();}
          catch (cause) {
            if (response.ok) throw new RemoteError('invalid_response', 'The companion server returned an unreadable response.', {status:response.status, operationId:op, cause});
          }
        }
        if (!response.ok) {
          const wireCode = object(payload) && typeof payload.error === 'string' ? payload.error : '';
          let code = 'server_error';
          if (response.status === 401) code = 'authentication_required';
          else if (response.status === 403) code = wireCode === 'csrf_invalid' ? 'csrf_required' : 'forbidden';
          else if (response.status === 409) code = wireCode === 'operation_conflict' ? 'operation_conflict' : 'stale_revision';
          else if (response.status === 400 || response.status === 422) code = 'invalid_input';
          else if (response.status === 404) code = 'not_found';
          else if (response.status === 429) code = 'rate_limited';
          else if (response.status === 503 && (!op || wireCode === 'live_bridge_unavailable')) code = 'unavailable';
          throw new RemoteError(code, code === 'unavailable' ? 'The live game connection is not configured or ready.' : 'The companion server rejected the request.', {status:response.status, operationId:op});
        }
        return payload;
      })()]);
    } finally {clearTimeout(timer);}
  }
  /** Returns only display metadata; the CSRF token is retained in memory. */
  async session() {
    // Cookie identity may change during sign-in; old requests stay invalid even
    // if a later account happens to expose the same display metadata.
    this.#clearSession();
    const epoch = this.#epoch;
    const data = await this.#request('/session');
    this.#assertEpoch(epoch);
    if (!object(data) || typeof data.authenticated !== 'boolean' || (data.realm !== undefined && data.realm !== 'ffxi-forever')) protocol('Invalid companion session.');
    const csrf = data.csrf_token;
    let characters = [];
    if (data.authenticated) {
      if (!Array.isArray(data.characters) || data.characters.length > 32) protocol('Invalid own-character list.');
      const seen = new Set();
      characters = data.characters.map(c => {
        if (!object(c) || typeof c.name !== 'string' || !c.name.trim() || new TextEncoder().encode(c.name).length > 32 || /[\x00-\x1f\x7f-\x9f]/.test(c.name)) protocol('Invalid own-character entry.');
        const charId = id(c.id, 'server character');
        if (seen.has(charId)) protocol('Duplicate own-character entry.');
        seen.add(charId); return {id:charId,name:c.name};
      });
    }
    this.#session = projection({authenticated:data.authenticated,characters,capabilities:Object.fromEntries(CAPABILITIES.map(c => [c,data.authenticated && data.capabilities?.[c] === true]))});
    this.#csrf = typeof csrf === 'string' && /^[\x21-\x7e]{1,256}$/.test(csrf) ? csrf : null;
    return this.#session;
  }
  /** Begin device authorization. The backend owns account tokens and verification hosts. */
  async beginDeviceSignIn() {
    const epoch = this.#epoch;
    const data = await this.#request('/session/device', {method:'POST', body:{}, csrf:true});
    this.#assertEpoch(epoch);
    if (!object(data) || typeof data.request_id !== 'string' || !/^[a-zA-Z0-9_-]{8,128}$/.test(data.request_id) || typeof data.user_code !== 'string' || !/^[a-zA-Z0-9 -]{1,32}$/.test(data.user_code)) protocol('Invalid device sign-in response.');
    let uri;
    try {uri = new URL(data.verification_uri);} catch {protocol('Invalid device sign-in address.');}
    if (uri.protocol !== 'https:' || uri.username || uri.password) protocol('Device sign-in requires an HTTPS address.');
    if (!Number.isSafeInteger(data.expires_in) || data.expires_in < 1 || data.expires_in > 3600 || !Number.isSafeInteger(data.interval) || data.interval < 1 || data.interval > 60) protocol('Invalid device sign-in timing.');
    return projection({request_id:data.request_id,user_code:data.user_code,verification_uri:uri.href,expires_in:data.expires_in,interval:data.interval});
  }
  /** Poll once; callers respect the supplied interval. No account token is returned. */
  async pollDeviceSignIn(requestId) {
    const key = deviceId(requestId), epoch = this.#epoch;
    const data = await this.#request('/session/device/poll',{method:'POST',body:{request_id:key},csrf:true});
    this.#assertEpoch(epoch);
    if (!object(data) || !['pending','approved','denied','expired'].includes(data.state)) protocol('Invalid device sign-in state.');
    if (data.interval !== undefined && (!Number.isSafeInteger(data.interval) || data.interval < 1 || data.interval > 60)) protocol('Invalid device sign-in interval.');
    return projection({state:data.state,...(data.interval === undefined ? {} : {interval:data.interval})});
  }
  async logout() {
    const response = this.#request('/session/logout', {method:'POST', body:{}, csrf:true});
    this.#clearSession();
    await response;
  }
  async snapshot(characterId) {
    id(characterId,'character'); this.#scope(characterId,'snapshot');
    this.#snapshots.delete(characterId);
    const epoch = this.#epoch;
    const data = await this.#request(`/characters/${characterId}/snapshot`);
    this.#assertEpoch(epoch);
    if (!object(data) || data.character_id !== characterId || typeof data.online !== 'boolean') protocol('Invalid character snapshot identity.');
    revision(data.revision);
    const observedAt = isoTime(data.observed_at);
    if (!data.online) throw new RemoteError('unavailable', 'This backend increment provides current online character reads only.');
    this.#snapshots.set(characterId,{online:true,revision:data.revision,observedAt,receivedAt:Date.now()});
    const fields = ['character_id','character_name','revision','observed_at','online','era','craft_cap','gil','free_inventory_slots','inventory','skills','key_items'];
    return projection(Object.fromEntries(fields.filter(k=>Object.hasOwn(data,k)).map(k=>[k,data[k]])));
  }
  async recipes(characterId) {
    id(characterId,'character'); this.#scope(characterId,'recipes');
    const epoch = this.#epoch;
    const data = await this.#request(`/characters/${characterId}/recipes`);
    this.#assertEpoch(epoch);
    if (!object(data) || data.character_id !== characterId || !Array.isArray(data.recipes) || data.recipes.length > 4096 || typeof data.era !== 'string' || !Number.isSafeInteger(data.craft_cap) || data.craft_cap < 0 || data.craft_cap > 65535) protocol('Invalid character recipe catalog.');
    revision(data.revision); isoTime(data.observed_at, 'catalog');
    // Keep the native table's ordered ingredients, four optional result tiers,
    // skill map and key-item selector. Do not substitute the sandbox TSV catalog.
    return projection({character_id:data.character_id,revision:data.revision,observed_at:data.observed_at,era:data.era,craft_cap:data.craft_cap,recipes:data.recipes});
  }
  async auctionItem(itemId, {characterId, stack = false} = {}) {
    id(characterId,'character'); id(itemId,'item',65535);
    if (typeof stack !== 'boolean') invalid('Choose a single item or a stack.');
    this.#scope(characterId,'auction');
    const epoch = this.#epoch;
    const data = await this.#request(`/auction/items/${itemId}?stack=${stack?1:0}&character_id=${characterId}`);
    this.#assertEpoch(epoch);
    if (!object(data) || data.character_id !== characterId || data.item_id !== itemId || data.stack !== stack
        || !uint32(data.single_listings) || !uint32(data.stack_listings) || !uint32(data.stock)
        || data.stock !== (stack ? data.stack_listings : data.single_listings)
        || data.stock_unit !== 'listings' || !Array.isArray(data.history) || data.history.length > 10) protocol('Invalid character auction stock/history response.');
    isoTime(data.observed_at, 'market');
    const history = data.history.map(sale => {
      if (!object(sale) || !uint32(sale.price) || typeof sale.seller !== 'string' || typeof sale.buyer !== 'string') protocol('Invalid auction sale history.');
      isoTime(sale.sold_at, 'sale');
      return {price:sale.price,sold_at:sale.sold_at,seller:sale.seller,buyer:sale.buyer};
    });
    // Native newest-ten history is already oldest-first. Keep that order and
    // count listings, including whole-stack listings, rather than item units.
    return projection({character_id:characterId,item_id:itemId,stack,stock:data.stock,
      single_listings:data.single_listings,stack_listings:data.stack_listings,stock_unit:'listings',
      observed_at:data.observed_at,history});
  }
  #prepare(characterId, capability, command, expectedRevision) {
    id(characterId,'character'); revision(expectedRevision); this.#scope(characterId,capability);
    this.#evidence(characterId,expectedRevision);
    if (typeof this.#crypto?.randomUUID !== 'function') throw new RemoteError('unavailable', 'A secure browser context is required to create operation IDs.');
    const op = operationId(this.#crypto.randomUUID());
    const request = freeze({character_id:characterId,path:`/characters/${characterId}/${capability === 'craft' ? 'crafts' : 'bids'}`,body:{operation_id:op,expected_revision:expectedRevision,...command}});
    this.#prepared.add(request); return request;
  }
  prepareCraft(options = {}) {
    if (!object(options) || Object.keys(options).some(k=>!['characterId','recipeId','expectedRevision'].includes(k))) invalid('A craft request contains only one recipe and its character revision.');
    const {characterId, recipeId, expectedRevision} = options;
    id(recipeId,'recipe'); return this.#prepare(characterId,'craft',{recipe_id:recipeId},expectedRevision);
  }
  prepareBid(options = {}) {
    if (!object(options) || Object.keys(options).some(k=>!['characterId','itemId','stack','offeredGil','expectedRevision'].includes(k))) invalid('Submit one single-item or stack bid at a time.');
    const {characterId, itemId, stack, offeredGil, expectedRevision} = options;
    id(itemId,'item',65535);
    if (typeof stack !== 'boolean') invalid('Choose a single item or a stack.');
    id(offeredGil,'offered total gil',999999999);
    return this.#prepare(characterId,'bid',{item_id:itemId,stack,offered_gil:offeredGil},expectedRevision);
  }
  #receipt(data, expectedId, characterId) {
    if (!object(data) || data.operation_id !== expectedId || !STATES.has(data.state) || (characterId !== undefined && data.character_id !== characterId) || !this.#session?.characters.some(c => c.id === data.character_id)) protocol('Invalid operation receipt identity or state.');
    if (data.state === 'succeeded' && !object(data.result)) protocol('A successful operation requires a confirmed result.');
    // Unknown fields are not interpreted as success. In particular, an accepted
    // queue request and pending_reconciliation are never local inventory changes.
    return projection({operation_id:expectedId,character_id:data.character_id,state:data.state,terminal:TERMINAL.has(data.state),confirmed:data.state === 'succeeded',...(data.state === 'succeeded' ? {result:data.result} : {}),...(typeof data.code === 'string' && /^[a-z0-9_]{1,64}$/.test(data.code) ? {code:data.code} : {})});
  }
  /** Submit once. Keep this exact frozen request to reconcile an uncertain reply. */
  async submit(request) {
    if (!this.#prepared.has(request)) invalid('Submit a prepared operation, not imported state.');
    const capability = request.path.endsWith('/crafts') ? 'craft' : 'bid';
    this.#scope(request.character_id,capability);
    this.#evidence(request.character_id,request.body.expected_revision);
    const epoch = this.#epoch;
    try {
      const data = await this.#request(request.path,{method:'POST',body:request.body,csrf:true,operationId:request.body.operation_id});
      this.#assertEpoch(epoch);
      return this.#receipt(data,request.body.operation_id,request.character_id);
    } catch (error) {
      if (error instanceof RemoteError && !['network_error','timeout','invalid_response','server_error','stale_session'].includes(error.code)) throw error;
      throw new UnknownOutcomeError(request.body.operation_id,error);
    }
  }
  /** One owner-scoped lookup, with no automatic polling or mutation retries. */
  async operationReceipt(op) {
    const key = operationId(op); this.#scope();
    const epoch = this.#epoch;
    const data = await this.#request(`/operations/${key}`);
    this.#assertEpoch(epoch);
    return this.#receipt(data,key);
  }
}
