/*
 * Builds public/js/roads.js — Windhoek's drivable road network from OpenStreetMap,
 * compacted into a routing graph the simulation can use offline.
 *
 *   node tools/build-roads.js            use the saved download in tools/data/
 *   node tools/build-roads.js --fetch    download fresh data from the Overpass API first
 *
 * Map data © OpenStreetMap contributors, available under the Open Database Licence (ODbL).
 */
'use strict';
const fs = require('fs');
const path = require('path');

// South, west, north, east. Covers Okuryangava in the north to Grove Mall / Kleine Kuppe in the south.
const BBOX = [-22.63, 17.01, -22.48, 17.13];
const RAW = path.join(__dirname, 'data', 'windhoek-roads.osm.json');
const OUT = path.join(__dirname, '..', 'public', 'js', 'roads.js');
const HIGHWAYS = ['motorway', 'trunk', 'primary', 'secondary', 'tertiary', 'unclassified', 'residential', 'living_street',
  'motorway_link', 'trunk_link', 'primary_link', 'secondary_link', 'tertiary_link'];
// 0 = highway, 1 = primary, 2 = secondary, 3 = tertiary, 4 = local street
const CLASS = { motorway: 0, trunk: 0, primary: 1, secondary: 2, tertiary: 3 };
const SIMPLIFY_M = 2.5;

async function fetchRaw() {
  const q = '[out:json][timeout:120];(way["highway"~"^(' + HIGHWAYS.join('|') + ')$"](' + BBOX.join(',') + '););out body;>;out skel qt;';
  const res = await fetch('https://overpass-api.de/api/interpreter', {
    method: 'POST', body: new URLSearchParams({ data: q }), headers: { 'User-Agent': 'VARA-demo-build/1.0 (local prototype)' },
  });
  if (!res.ok) throw new Error('Overpass returned ' + res.status + ' — it is often busy, try again in a minute');
  fs.mkdirSync(path.dirname(RAW), { recursive: true });
  fs.writeFileSync(RAW, await res.text());
}

// Local metres: Web Mercator scaled by cos(lat0), so OSM map tiles line up exactly.
const R = 6378137;
const LAT0 = (BBOX[0] + BBOX[2]) / 2;
const K = Math.cos(LAT0 * Math.PI / 180);
const mercX = (lon) => R * lon * Math.PI / 180;
const mercY = (lat) => R * Math.log(Math.tan(Math.PI / 4 + lat * Math.PI / 360));
const MX0 = mercX(BBOX[1]), MY0 = mercY(BBOX[2]);
const project = (lat, lon) => [(mercX(lon) - MX0) * K, (MY0 - mercY(lat)) * K];

function simplify(pts, tol) {
  if (pts.length <= 2) return pts;
  const keep = new Uint8Array(pts.length);
  keep[0] = keep[pts.length - 1] = 1;
  const stack = [[0, pts.length - 1]];
  while (stack.length) {
    const [i, j] = stack.pop();
    const [ax, ay] = pts[i], [bx, by] = pts[j];
    const dx = bx - ax, dy = by - ay, L2 = dx * dx + dy * dy;
    let best = -1, bd = tol;
    for (let k = i + 1; k < j; k++) {
      const [px, py] = pts[k];
      const t = L2 ? Math.max(0, Math.min(1, ((px - ax) * dx + (py - ay) * dy) / L2)) : 0;
      const d = Math.hypot(px - ax - dx * t, py - ay - dy * t);
      if (d > bd) { bd = d; best = k; }
    }
    if (best > 0) { keep[best] = 1; stack.push([i, best], [best, j]); }
  }
  return pts.filter((_, i) => keep[i]);
}

async function main() {
  if (process.argv.includes('--fetch') || !fs.existsSync(RAW)) await fetchRaw();
  const raw = JSON.parse(fs.readFileSync(RAW, 'utf8'));
  const coord = new Map();
  const ways = [];
  for (const el of raw.elements) {
    if (el.type === 'node') coord.set(el.id, project(el.lat, el.lon));
    else if (el.type === 'way' && el.tags && HIGHWAYS.includes(el.tags.highway)) ways.push(el);
  }

  // A node splits ways into edges when it is shared by several ways or ends one.
  const uses = new Map();
  for (const w of ways) w.nodes.forEach((id, i) => uses.set(id, (uses.get(id) || 0) + (i === 0 || i === w.nodes.length - 1 ? 2 : 1)));
  const names = [], nameIdx = new Map();
  const nameOf = (n) => {
    if (!n) return -1;
    if (!nameIdx.has(n)) { nameIdx.set(n, names.length); names.push(n); }
    return nameIdx.get(n);
  };
  const graphNode = new Map();
  const nodeXY = [];
  const nodeFor = (id) => {
    if (!graphNode.has(id)) { graphNode.set(id, nodeXY.length); nodeXY.push(coord.get(id)); }
    return graphNode.get(id);
  };
  const edges = [];
  for (const w of ways) {
    const t = w.tags;
    let ids = w.nodes.filter((id) => coord.has(id));
    if (ids.length < 2) continue;
    let oneway = ['yes', '1', 'true'].includes(t.oneway) || t.junction === 'roundabout' || t.highway === 'motorway' ? 1 : 0;
    if (t.oneway === '-1') { ids = ids.slice().reverse(); oneway = 1; }
    const cls = CLASS[t.highway.replace('_link', '')] ?? 4;
    const name = nameOf(t.name || t.ref || '');
    let start = 0;
    for (let i = 1; i < ids.length; i++) {
      if (i === ids.length - 1 || uses.get(ids[i]) > 1) {
        const seg = ids.slice(start, i + 1);
        edges.push({ a: nodeFor(seg[0]), b: nodeFor(seg[seg.length - 1]), pts: simplify(seg.map((id) => coord.get(id)), SIMPLIFY_M), name, cls, oneway });
        start = i;
      }
    }
  }

  // Keep only the largest connected piece, so every snapped point can reach every other.
  const parent = nodeXY.map((_, i) => i);
  const find = (i) => { while (parent[i] !== i) i = parent[i] = parent[parent[i]]; return i; };
  for (const e of edges) parent[find(e.a)] = find(e.b);
  const size = new Map();
  for (let i = 0; i < nodeXY.length; i++) size.set(find(i), (size.get(find(i)) || 0) + 1);
  const main = [...size.entries()].sort((a, b) => b[1] - a[1])[0][0];
  const remap = new Map();
  const nodes = [];
  for (let i = 0; i < nodeXY.length; i++) if (find(i) === main) { remap.set(i, nodes.length / 2); nodes.push(Math.round(nodeXY[i][0]), Math.round(nodeXY[i][1])); }
  const outEdges = [];
  for (const e of edges) {
    if (!remap.has(e.a) || e.a === e.b && e.pts.length < 3) continue;
    const mid = [];
    for (let i = 1; i < e.pts.length - 1; i++) mid.push(Math.round(e.pts[i][0]), Math.round(e.pts[i][1]));
    outEdges.push([remap.get(e.a), remap.get(e.b), e.name, e.cls | (e.oneway << 3), ...mid]);
  }

  const [w, h] = project(BBOX[0], BBOX[3]);
  const data = { bbox: BBOX, k: K, mx0: MX0, my0: MY0, w: Math.round(w), h: Math.round(h), names, nodes, edges: outEdges };
  const js = '/* Windhoek road network. Generated by tools/build-roads.js — do not edit by hand.\n' +
    ' * Map data © OpenStreetMap contributors, ODbL (https://www.openstreetmap.org/copyright). */\n' +
    '(function (root) {\n  var data = ' + JSON.stringify(data) + ';\n' +
    "  if (typeof module === 'object' && module.exports) module.exports = data;\n  else root.HeraRoads = data;\n" +
    "})(typeof self !== 'undefined' ? self : this);\n";
  fs.writeFileSync(OUT, js);
  console.log('roads.js: ' + nodes.length / 2 + ' junctions, ' + outEdges.length + ' road segments, ' + names.length + ' street names, ' +
    (js.length / 1024).toFixed(0) + ' KB, ' + (w / 1000).toFixed(1) + ' × ' + (h / 1000).toFixed(1) + ' km');
}

main().catch((e) => { console.error(e.message); process.exit(1); });
