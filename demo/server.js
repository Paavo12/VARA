/*
 * FUber demo server — zero dependencies.
 *
 * Runs the simulation authoritatively, serves the web app, and pushes state to
 * every connected screen over Server-Sent Events. That lets you open the Rider
 * portal on one device and the Driver portal on another and see them in sync.
 */
'use strict';
const http = require('http');
const fs = require('fs');
const os = require('os');
const path = require('path');
const { Sim } = require('./public/js/engine.js');

const PORT = Number(process.env.PORT) || 3100;
const TICK_MS = 200;
const PUBLIC = path.join(__dirname, 'public');
const MIME = { '.html': 'text/html; charset=utf-8', '.js': 'text/javascript; charset=utf-8', '.css': 'text/css; charset=utf-8', '.svg': 'image/svg+xml', '.png': 'image/png', '.ico': 'image/x-icon', '.json': 'application/json' };

const sim = new Sim();
const clients = new Set();

function broadcast() {
  const msg = 'data: ' + JSON.stringify(sim.state) + '\n\n';
  for (const res of clients) res.write(msg);
}

let last = Date.now();
setInterval(() => {
  const now = Date.now();
  sim.tick(Math.min(1000, now - last));
  last = now;
  broadcast();
}, TICK_MS);

function readBody(req) {
  return new Promise((resolve, reject) => {
    let body = '';
    req.on('data', (c) => {
      body += c;
      if (body.length > 2e6) { reject(new Error('Body too large')); req.destroy(); }
    });
    req.on('end', () => resolve(body));
  });
}

const server = http.createServer(async (req, res) => {
  const url = new URL(req.url, 'http://localhost');

  if (url.pathname === '/api/events') {
    res.writeHead(200, { 'Content-Type': 'text/event-stream', 'Cache-Control': 'no-cache', Connection: 'keep-alive' });
    res.write('retry: 1000\n');
    res.write('data: ' + JSON.stringify(sim.state) + '\n\n');
    clients.add(res);
    req.on('close', () => clients.delete(res));
    return;
  }

  if (url.pathname === '/api/state') {
    res.writeHead(200, { 'Content-Type': 'application/json' });
    return res.end(JSON.stringify(sim.state));
  }

  if (url.pathname === '/api/action' && req.method === 'POST') {
    try {
      const action = JSON.parse(await readBody(req));
      const result = sim.dispatch(action);
      broadcast();
      res.writeHead(200, { 'Content-Type': 'application/json' });
      return res.end(JSON.stringify(result));
    } catch (e) {
      res.writeHead(400, { 'Content-Type': 'application/json' });
      return res.end(JSON.stringify({ ok: false, error: e.message }));
    }
  }

  // Static files
  let file = path.normalize(path.join(PUBLIC, decodeURIComponent(url.pathname)));
  if (!file.startsWith(PUBLIC)) { res.writeHead(403); return res.end(); }
  if (url.pathname === '/' || (fs.existsSync(file) && fs.statSync(file).isDirectory())) file = path.join(PUBLIC, 'index.html');
  fs.readFile(file, (err, data) => {
    if (err) { res.writeHead(404); return res.end('Not found'); }
    res.writeHead(200, { 'Content-Type': MIME[path.extname(file)] || 'application/octet-stream', 'Cache-Control': 'no-store' });
    res.end(data);
  });
});

server.listen(PORT, () => {
  const lan = Object.values(os.networkInterfaces()).flat().filter((i) => i && i.family === 'IPv4' && !i.internal).map((i) => i.address);
  console.log('\n  FUber — women-only ride-hailing demo\n');
  console.log('  Presenter view (both portals):  http://localhost:' + PORT + '/');
  console.log('  Rider portal only:              http://localhost:' + PORT + '/?view=rider');
  console.log('  Driver portal only:             http://localhost:' + PORT + '/?view=driver');
  for (const ip of lan) console.log('  On your network:                http://' + ip + ':' + PORT + '/?view=rider');
  console.log('\n  Press Ctrl+C to stop.\n');
});
