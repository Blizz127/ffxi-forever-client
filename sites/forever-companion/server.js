import http from 'node:http';
import { readFile } from 'node:fs/promises';
import { resolve, extname } from 'node:path';
import { fileURLToPath } from 'node:url';
const root = fileURLToPath(new URL('./dist/', import.meta.url));
const types = {'.html':'text/html; charset=utf-8','.js':'text/javascript; charset=utf-8','.css':'text/css; charset=utf-8','.json':'application/json','.webmanifest':'application/manifest+json','.svg':'image/svg+xml'};
const server = http.createServer(async (req, res) => {
  const headers = {'X-Content-Type-Options':'nosniff','Referrer-Policy':'same-origin', 'Content-Security-Policy': "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'; object-src 'none'; base-uri 'self'; frame-ancestors 'none'"};
  try {
    const path = decodeURIComponent(new URL(req.url, 'http://localhost').pathname);
    if (req.method !== 'GET' && req.method !== 'HEAD') {res.writeHead(405,headers); return res.end('Method not allowed');}
    if (path.startsWith('/api/')) {res.writeHead(503,{...headers,'Content-Type':'application/json'}); return res.end(JSON.stringify({error:'live_bridge_unavailable',message:'The game server connection is not configured.'}));}
    const file = resolve(root, '.' + (path === '/' ? '/index.html' : path));
    if (!file.startsWith(root)) {res.writeHead(403,headers); return res.end();}
    const data = await readFile(file);
    res.writeHead(200,{...headers,'Content-Type':types[extname(file)] || 'application/octet-stream','Cache-Control':'no-cache','Service-Worker-Allowed':'/'});
    res.end(req.method === 'HEAD' ? undefined : data);
  } catch {res.writeHead(404,headers); res.end('Not found');}
});
server.listen(Number(process.env.PORT || 4317), process.env.HOST || '127.0.0.1', () => console.log(`Forever Companion: http://${process.env.HOST || '127.0.0.1'}:${server.address().port}`));
