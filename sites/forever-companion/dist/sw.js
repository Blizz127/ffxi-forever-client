const CACHE = 'forever-companion-shell-0.1.0-preview2';
const ASSETS = ['./','./index.html','./styles.css','./app.js','./domain.js','./icon.svg','./icon-192.png','./icon-512.png','./licenses/THIRD_PARTY_NOTICES.txt','./licenses/LandSandBoat-GPL-3.0.txt','./manifest.webmanifest','./data/recipes.json'];
self.addEventListener('install', event => {event.waitUntil(caches.open(CACHE).then(cache=>cache.addAll(ASSETS)).then(()=>self.skipWaiting()));});
self.addEventListener('activate', event => {event.waitUntil(caches.keys().then(keys=>Promise.all(keys.filter(k=>k.startsWith('forever-companion-shell-') && k!==CACHE).map(k=>caches.delete(k)))).then(()=>self.clients.claim()));});
self.addEventListener('fetch', event => {
  const url = new URL(event.request.url);
  if (event.request.method !== 'GET' || url.origin !== location.origin || url.pathname.includes('/api/')) return;
  event.respondWith(fetch(event.request).then(response=>{if(response.ok){const copy=response.clone();event.waitUntil(caches.open(CACHE).then(cache=>cache.put(event.request,copy)));}return response;}).catch(()=>caches.match(event.request).then(cached=>cached || (event.request.mode==='navigate' ? caches.match('./index.html') : Response.error()))));
});
