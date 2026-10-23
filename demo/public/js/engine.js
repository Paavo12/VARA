/*
 * FUber ride-hailing demo — simulation engine.
 *
 * Shared by server.js (authoritative, multi-device mode) and the browser
 * (stand-alone mode when index.html is opened directly from disk).
 * Everything here is plain data + pure-ish functions so state can be
 * serialised and broadcast to every connected screen.
 */
(function (root, factory) {
  if (typeof module === 'object' && module.exports) module.exports = factory(require('./roads.js'));
  else root.HeraEngine = factory(root.HeraRoads);
})(typeof self !== 'undefined' ? self : this, function (ROADS) {
  'use strict';

  // ---------------------------------------------------------------------------
  // City model: real Windhoek. Roads come from OpenStreetMap (public/js/roads.js, built by
  // tools/build-roads.js) in local metres: x grows east, y grows south. The projection is
  // Web Mercator scaled by cos(latitude), so OSM map tiles line up with it exactly.
  // ---------------------------------------------------------------------------
  const BOUNDS = { w: ROADS.w, h: ROADS.h };
  const WORLD = Math.max(ROADS.w, ROADS.h);
  const EARTH_R = 6378137;
  function fromLatLng(lat, lon) {
    const mx = EARTH_R * lon * Math.PI / 180, my = EARTH_R * Math.log(Math.tan(Math.PI / 4 + lat * Math.PI / 360));
    return { x: Math.round((mx - ROADS.mx0) * ROADS.k), y: Math.round((ROADS.my0 - my) * ROADS.k) };
  }
  function toLatLng(p) {
    const mx = p.x / ROADS.k + ROADS.mx0, my = ROADS.my0 - p.y / ROADS.k;
    return { lat: (2 * Math.atan(Math.exp(my / EARTH_R)) - Math.PI / 2) * 180 / Math.PI, lon: mx / EARTH_R * 180 / Math.PI };
  }
  const at = (lat, lon, o) => Object.assign(fromLatLng(lat, lon), o);
  // Landmarks at their real OpenStreetMap locations (the Women's Health Clinic is fictional).
  const PLACES = [
    at(-22.56665, 17.08126, { id: 'postst', name: 'Post Street Mall', kind: 'Shopping · CBD' }),
    at(-22.58359, 17.09373, { id: 'maerua', name: 'Maerua Mall', kind: 'Shopping' }),
    at(-22.62043, 17.09314, { id: 'grove', name: 'Grove Mall of Namibia', kind: 'Shopping' }),
    at(-22.55288, 17.06947, { id: 'whkhosp', name: 'Windhoek Central Hospital', kind: 'Hospital' }),
    at(-22.54156, 17.09066, { id: 'mediclinic', name: 'Mediclinic Windhoek', kind: 'Hospital' }),
    at(-22.53444, 17.06482, { id: 'katuhosp', name: 'Katutura State Hospital', kind: 'Hospital' }),
    at(-22.53244, 17.05044, { id: 'sqmarket', name: 'Single Quarters Market', kind: 'Market · Katutura' }),
    at(-22.61179, 17.05860, { id: 'unam', name: 'UNAM Main Campus', kind: 'University' }),
    at(-22.60172, 17.08138, { id: 'eros', name: 'Eros Airport', kind: 'Airport' }),
    at(-22.56702, 17.08070, { id: 'wernhil', name: 'Wernhil Park Mall', kind: 'Shopping' }),
    at(-22.56872, 17.08799, { id: 'museum', name: 'Independence Memorial Museum', kind: 'Museum' }),
    at(-22.55081, 17.09046, { id: 'joes', name: "Joe's Beerhouse", kind: 'Restaurant' }),
    at(-22.54500, 17.05300, { id: 'clinic', name: "Women's Health Clinic", kind: 'Clinic' }),
    at(-22.56351, 17.05772, { id: 'dorado', name: 'Dorado Park', kind: 'Neighbourhood' }),
  ];
  // Suburb centres from OpenStreetMap; a point belongs to the nearest one.
  const SUBURBS = [
    ['Havana', -22.50094, 17.01538], ['Ombili', -22.50461, 17.04438], ['Okuryangava', -22.50521, 17.05539], ['Hakahana', -22.50523, 17.03476],
    ['Greenwell Matongo', -22.51581, 17.0216], ['Wanaheda', -22.51961, 17.03283], ['Soweto', -22.52033, 17.04403], ['Katutura', -22.52104, 17.0557],
    ['Eros Park', -22.53835, 17.09096], ['Eros', -22.54746, 17.09479], ['Khomasdal', -22.54939, 17.04704], ['Windhoek North', -22.55201, 17.07722],
    ['Otjomuise', -22.5562, 17.0182], ['Klein Windhoek', -22.56275, 17.10091], ['Windhoek West', -22.56299, 17.07044], ['Dorado Park', -22.56351, 17.05772],
    ['Ludwigsdorf', -22.56629, 17.11366], ['Windhoek Central', -22.56998, 17.08404], ['Rocky Crest', -22.57153, 17.04488], ['Tauben Glen', -22.57399, 17.05405],
    ['Luxury Hill', -22.57582, 17.09465], ['Hochland Park', -22.57654, 17.06704], ['Avis', -22.57837, 17.11843], ['Southern Industrial Area', -22.58386, 17.07953],
    ['Pionierspark', -22.59347, 17.06792], ['Suiderhof', -22.5957, 17.0864], ['Auasblick', -22.59579, 17.10293], ['Olympia', -22.6024, 17.09648],
    ['Academia', -22.60989, 17.06784], ['Kleine Kuppe', -22.61932, 17.10603], ['Prosperita', -22.62232, 17.08015],
  ].map(([name, lat, lon]) => at(lat, lon, { name }));

  const OPTIONS = {
    // Prices in Namibian dollars (N$).
    // Ordered cheapest → most expensive; a higher tier charges more per trip.
    classic: { id: 'classic', name: 'Classic', short: 'Classic', blurb: 'Affordable everyday rides', base: 15, perKm: 8, perMin: 1.2, min: 35, serviceFee: 0.02 },
    comfort: { id: 'comfort', name: 'Comfort', short: 'Comfort', blurb: 'Roomier cars · top-rated drivers', base: 20, perKm: 10, perMin: 1.5, min: 45, serviceFee: 0.025 },
    softlife: { id: 'softlife', name: 'Soft Life', short: 'Soft Life', blurb: 'Premium cars · the full treatment', base: 28, perKm: 13, perMin: 2, min: 60, serviceFee: 0.03 },
  };
  const DEMAND = {
    normal: { id: 'normal', label: 'Normal demand', surge: 1.0, traffic: 1.2, weights: { eta: 0.7, acc: 0.3 } },
    high: { id: 'high', label: 'High demand', surge: 1.4, traffic: 1.4, weights: { eta: 0.5, acc: 0.5 } },
    peak: { id: 'peak', label: 'Peak demand', surge: 1.8, traffic: 1.6, weights: { eta: 0.45, acc: 0.55 } },
  };
  const CFG = {
    RADIUS: 3000, // dispatch radius, metres (straight line)
    ARRIVE_M: 50, // pickup reveal radius
    OFFER_MS: 15000, // driver response window (real time)
    SPEED_KMH: 34, // free-flow driving speed, divided by the traffic factor
    CRUISE_KMH: 20, // idle drivers roaming
    DEVIATION_M: 150, // RideCheck: distance off the planned route
    STOP_SIM_MS: 2 * 60 * 1000, // RideCheck: unexpected stop length (sim time)
    WAIT_FREE_SIM_MS: 5 * 60 * 1000, // free waiting at pickup (sim time)
    ETA_NORM_MIN: 12, // ETA at which the ETA score reaches 0
    MAX_SEATS: 4, // passengers per trip (every car seats four)
    FAV_BONUS: 0.08,
    // Priority points: hidden from riders and drivers, only the presenter panel shows them.
    PRIORITY_START: 100,
    PRIORITY_MAX: 150,
    PRIORITY_CANCEL_STEP: 5, // nth cancellation costs n × 5 points
    PRIORITY_TRIP_GAIN: 2, // each completed trip earns points back
    PRIORITY_HOLD_PER: 15, // a rider loses her nearest driver for every 15 points below the start value
    PRIORITY_DRIVER_W: 0.3, // driver score shift per 100 points from the start value
  };
  const STATES = ['REQUESTED', 'SEARCHING', 'DRIVER_ASSIGNED', 'DRIVER_EN_ROUTE', 'DRIVER_ARRIVED', 'PIN_VERIFIED', 'IN_TRIP', 'COMPLETED', 'RATED'];
  const DRIVER_LOCKED = new Set(['DRIVER_ASSIGNED', 'DRIVER_EN_ROUTE', 'DRIVER_ARRIVED', 'PIN_VERIFIED', 'IN_TRIP']);
  // No vehicle-registration document: a driver may be driving a car she doesn't own.
  const DRIVER_DOCS = ['identity', 'licence', 'insurance', 'background'];

  // ---------------------------------------------------------------------------
  // Geometry helpers
  // ---------------------------------------------------------------------------
  const EPS = 0.5;
  const BLOCK = 500; // roaming distances are given in rough city blocks
  const clamp = (v, lo, hi) => Math.max(lo, Math.min(hi, v));
  const rand = (a, b) => a + Math.random() * (b - a);
  const randInt = (a, b) => Math.floor(rand(a, b + 1));
  const pick = (arr) => arr[Math.floor(Math.random() * arr.length)];
  const r2 = (n) => Math.round(n * 100) / 100;
  const dist = (a, b) => Math.hypot(a.x - b.x, a.y - b.y);
  const inBounds = (p) => ({ x: clamp(p.x, 0, BOUNDS.w), y: clamp(p.y, 0, BOUNDS.h) });

  // ---- Road graph -----------------------------------------------------------
  const NN = ROADS.nodes.length / 2;
  const NX = new Float64Array(NN), NY = new Float64Array(NN);
  for (let i = 0; i < NN; i++) { NX[i] = ROADS.nodes[2 * i]; NY[i] = ROADS.nodes[2 * i + 1]; }
  const EDGES = ROADS.edges.map((e, id) => {
    const pts = [{ x: NX[e[0]], y: NY[e[0]] }];
    for (let i = 4; i < e.length; i += 2) pts.push({ x: e[i], y: e[i + 1] });
    pts.push({ x: NX[e[1]], y: NY[e[1]] });
    const cum = [0];
    for (let i = 1; i < pts.length; i++) cum.push(cum[i - 1] + dist(pts[i - 1], pts[i]));
    return { id, a: e[0], b: e[1], name: e[2], cls: e[3] & 7, oneway: (e[3] & 8) > 0, pts, cum, len: cum[cum.length - 1] };
  });
  const ADJ = Array.from({ length: NN }, () => []);
  for (const e of EDGES) {
    ADJ[e.a].push({ e, to: e.b, fwd: true });
    if (!e.oneway) ADJ[e.b].push({ e, to: e.a, fwd: false });
  }
  const streetName = (i) => (i >= 0 ? ROADS.names[i] : null);

  // Spatial index of road segments (200 m cells) for snapping points to the nearest road.
  const CELL = 200, CW = Math.ceil(BOUNDS.w / CELL) + 1;
  const CELLS = new Map();
  for (const e of EDGES) {
    for (let k = 1; k < e.pts.length; k++) {
      const a = e.pts[k - 1], b = e.pts[k];
      for (let cx = Math.floor(Math.min(a.x, b.x) / CELL); cx <= Math.floor(Math.max(a.x, b.x) / CELL); cx++) {
        for (let cy = Math.floor(Math.min(a.y, b.y) / CELL); cy <= Math.floor(Math.max(a.y, b.y) / CELL); cy++) {
          const key = cy * CW + cx;
          if (!CELLS.has(key)) CELLS.set(key, []);
          CELLS.get(key).push([e, k]);
        }
      }
    }
  }

  /** Nearest point on the road network: the edge, how far along it, and the point itself. */
  function snapInfo(p, namedOnly, maxRing) {
    p = inBounds(p);
    const cx = Math.floor(p.x / CELL), cy = Math.floor(p.y / CELL);
    let best = null;
    for (let r = 0; r <= (maxRing || 80); r++) {
      if (best && best.d < (r - 1) * CELL) break;
      for (let x = cx - r; x <= cx + r; x++) {
        for (let y = cy - r; y <= cy + r; y++) {
          if (x < 0 || y < 0 || x >= CW || Math.max(Math.abs(x - cx), Math.abs(y - cy)) !== r) continue;
          const list = CELLS.get(y * CW + x);
          if (!list) continue;
          for (const [e, k] of list) {
            if (namedOnly && e.name < 0) continue;
            const a = e.pts[k - 1], b = e.pts[k];
            const dx = b.x - a.x, dy = b.y - a.y, L2 = dx * dx + dy * dy;
            const t = L2 ? clamp(((p.x - a.x) * dx + (p.y - a.y) * dy) / L2, 0, 1) : 0;
            const qx = a.x + dx * t, qy = a.y + dy * t, d = Math.hypot(p.x - qx, p.y - qy);
            if (!best || d < best.d) best = { e, k, x: qx, y: qy, d, along: e.cum[k - 1] + t * (e.cum[k] - e.cum[k - 1]) };
          }
        }
      }
    }
    return best;
  }

  function snap(p) {
    const s = snapInfo(p);
    return { x: Math.round(s.x), y: Math.round(s.y) };
  }

  function pointOnEdge(e, d) {
    let i = 1;
    while (i < e.pts.length - 1 && e.cum[i] < d) i++;
    const a = e.pts[i - 1], b = e.pts[i], L = e.cum[i] - e.cum[i - 1];
    const t = L > 0 ? clamp((d - e.cum[i - 1]) / L, 0, 1) : 0;
    return { x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t };
  }

  /** Points along one edge from distance d1 to d2 (either direction), ending at d2. Each point
   *  carries `s`, the street of the segment that arrives at it, for turn-by-turn directions. */
  function edgeSlice(e, d1, d2) {
    const out = [];
    if (d2 >= d1) { for (let i = 0; i < e.pts.length; i++) if (e.cum[i] > d1 && e.cum[i] < d2) out.push(e.pts[i]); }
    else { for (let i = e.pts.length - 1; i >= 0; i--) if (e.cum[i] < d1 && e.cum[i] > d2) out.push(e.pts[i]); }
    out.push(pointOnEdge(e, d2));
    return out.map((p) => ({ x: Math.round(p.x), y: Math.round(p.y), s: e.name }));
  }

  function clean(pts) {
    const out = [];
    for (const p of pts) if (!out.length || dist(out[out.length - 1], p) > EPS) out.push(p.s === undefined ? { x: p.x, y: p.y } : { x: p.x, y: p.y, s: p.s });
    if (out.length === 1) out.push({ ...out[0] });
    return out;
  }

  // Binary min-heap keyed on f = cost so far + straight-line distance to go (A*).
  function heapPush(h, n, f) {
    h.push([n, f]);
    let i = h.length - 1;
    while (i > 0) { const j = (i - 1) >> 1; if (h[j][1] <= h[i][1]) break; [h[i], h[j]] = [h[j], h[i]]; i = j; }
  }
  function heapPop(h) {
    const top = h[0], last = h.pop();
    if (h.length) {
      h[0] = last;
      let i = 0;
      for (;;) {
        const l = 2 * i + 1, r = l + 1;
        let m = i;
        if (l < h.length && h[l][1] < h[m][1]) m = l;
        if (r < h.length && h[r][1] < h[m][1]) m = r;
        if (m === i) break;
        [h[i], h[m]] = [h[m], h[i]];
        i = m;
      }
    }
    return top;
  }

  /** Shortest drivable route over the real road network, respecting one-way streets. */
  function findRoute(from, to) {
    const A = snapInfo(from), B = snapInfo(to);
    const start = { x: Math.round(A.x), y: Math.round(A.y), s: A.e.name };
    if (A.e === B.e && (!A.e.oneway || B.along >= A.along)) return clean([start, ...edgeSlice(A.e, A.along, B.along)]);
    const endX = B.x, endY = B.y;
    const g = new Float64Array(NN).fill(Infinity), via = new Array(NN), done = new Uint8Array(NN), heap = [];
    const relax = (n, cost, step) => {
      if (cost >= g[n]) return;
      g[n] = cost; via[n] = step;
      heapPush(heap, n, cost + Math.hypot(NX[n] - endX, NY[n] - endY));
    };
    relax(A.e.b, A.e.len - A.along, { first: true, exit: A.e.len });
    if (!A.e.oneway) relax(A.e.a, A.along, { first: true, exit: 0 });
    let best = Infinity, last = -1;
    while (heap.length) {
      const [n, f] = heapPop(heap);
      if (f >= best) break;
      if (done[n]) continue;
      done[n] = 1;
      if (n === B.e.a && g[n] + B.along < best) { best = g[n] + B.along; last = n; }
      if (n === B.e.b && !B.e.oneway && g[n] + B.e.len - B.along < best) { best = g[n] + B.e.len - B.along; last = n; }
      for (const st of ADJ[n]) if (!done[st.to]) relax(st.to, g[n] + st.e.len, st);
    }
    if (last < 0) return clean([start, { x: Math.round(B.x), y: Math.round(B.y) }]);
    const steps = [];
    let n = last;
    while (!via[n].first) { steps.push(via[n]); n = via[n].fwd ? via[n].e.a : via[n].e.b; }
    const pts = [start, ...edgeSlice(A.e, A.along, via[n].exit)];
    for (let i = steps.length - 1; i >= 0; i--) {
      const st = steps[i];
      const ep = st.fwd ? st.e.pts.slice(1) : st.e.pts.slice(0, -1).reverse();
      for (const p of ep) pts.push({ x: Math.round(p.x), y: Math.round(p.y), s: st.e.name });
    }
    pts.push(...edgeSlice(B.e, last === B.e.a ? 0 : B.e.len, B.along));
    return clean(pts);
  }

  // Small LRU caches: routes for drawing and driving, road distances for ETAs.
  function cached(cache, key, make) {
    let v = cache.get(key);
    if (v !== undefined) { cache.delete(key); cache.set(key, v); return v; }
    v = make();
    cache.set(key, v);
    if (cache.size > 1500) cache.delete(cache.keys().next().value);
    return v;
  }
  const routeCache = new Map(), distCache = new Map();

  /** Road route between two points, as a polyline snapped to real streets. */
  function route(from, to) {
    const key = Math.round(from.x) + ',' + Math.round(from.y) + '>' + Math.round(to.x) + ',' + Math.round(to.y);
    return cached(routeCache, key, () => findRoute(from, to)).map((p) => ({ ...p }));
  }

  /** Road distance in metres. Endpoints are rounded to 25 m so moving cars reuse cached answers. */
  function roadDist(from, to) {
    const q = (v) => Math.round(v / 25);
    return cached(distCache, q(from.x) + ',' + q(from.y) + '>' + q(to.x) + ',' + q(to.y), () => polyLen(findRoute(from, to)));
  }

  function polyLen(pts) {
    let L = 0;
    for (let i = 1; i < pts.length; i++) L += dist(pts[i - 1], pts[i]);
    return L;
  }

  function pointAlong(pts, d) {
    let acc = 0;
    for (let i = 1; i < pts.length; i++) {
      const a = pts[i - 1], b = pts[i], L = dist(a, b);
      if (L > 0 && acc + L >= d) {
        const t = (d - acc) / L;
        return { x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t, heading: Math.atan2(b.y - a.y, b.x - a.x) * 180 / Math.PI, seg: i };
      }
      acc += L;
    }
    const n = pts.length, a = pts[n - 2], b = pts[n - 1];
    const heading = a && dist(a, b) > 0 ? Math.atan2(b.y - a.y, b.x - a.x) * 180 / Math.PI : null;
    return { x: b.x, y: b.y, heading, seg: n - 1 };
  }

  function remainingPath(pts, d) {
    if (!pts) return [];
    const p = pointAlong(pts, d);
    return clean([{ x: p.x, y: p.y, s: pts[p.seg] && pts[p.seg].s }, ...pts.slice(p.seg)]);
  }

  function distToSeg(p, a, b) {
    const dx = b.x - a.x, dy = b.y - a.y, L2 = dx * dx + dy * dy;
    const t = L2 ? clamp(((p.x - a.x) * dx + (p.y - a.y) * dy) / L2, 0, 1) : 0;
    return Math.hypot(p.x - (a.x + dx * t), p.y - (a.y + dy * t));
  }

  function distToPoly(p, pts) {
    let m = Infinity;
    for (let i = 1; i < pts.length; i++) m = Math.min(m, distToSeg(p, pts[i - 1], pts[i]));
    return m;
  }

  // Street of the segment that ends at pts[i]; looked up on the map for points without a label.
  function segStreet(pts, i) {
    const p = pts[i];
    if (p.s !== undefined) return p.s;
    const q = pts[i - 1] || p;
    return snapInfo({ x: (p.x + q.x) / 2, y: (p.y + q.y) / 2 }).e.name;
  }

  // Signed turn at pts[i] in degrees: positive turns right (y grows south), negative turns left.
  function turnAt(pts, i) {
    const a = pts[i - 1], b = pts[i], c = pts[i + 1];
    const t = Math.atan2(c.y - b.y, c.x - b.x) - Math.atan2(b.y - a.y, b.x - a.x);
    return (((t * 180 / Math.PI) + 540) % 360) - 180;
  }

  /** Turn-by-turn: the next manoeuvre along a path from `d` metres in. */
  function nextTurn(pts, d) {
    if (!pts || pts.length < 2) return { dir: 'arrive', inM: 0, street: null };
    const cur = pointAlong(pts, d);
    let i = cur.seg;
    let remain = dist(cur, pts[i]);
    const onto = segStreet(pts, i);
    let street = onto;
    while (i < pts.length - 1) {
      const next = segStreet(pts, i + 1), turn = turnAt(pts, i);
      // A turn is a real bend onto a different street, or a sharp bend on the same one.
      if ((next !== street && Math.abs(turn) > 25) || Math.abs(turn) > 70) {
        const dir = Math.abs(turn) > 150 ? 'uturn' : turn > 0 ? 'right' : 'left';
        return { dir, inM: remain, street: streetName(next), current: streetName(onto) };
      }
      street = next;
      remain += dist(pts[i], pts[i + 1]);
      i++;
    }
    return { dir: 'arrive', inM: remain, street: null, current: streetName(onto) };
  }

  function zoneOf(p) {
    let best = SUBURBS[0];
    for (const z of SUBURBS) if (dist(p, z) < dist(p, best)) best = z;
    return best.name;
  }

  function streetOf(p) {
    const s = snapInfo(p);
    if (s.e.name >= 0) return streetName(s.e.name);
    const named = snapInfo(p, true, 3);
    return named ? streetName(named.e.name) : null;
  }

  function addressOf(p) {
    const s = snapInfo(p);
    const name = streetOf(p);
    // House numbers are illustrative: they count up along each street segment.
    return name ? Math.max(1, Math.round(s.along / 12) + 1) + ' ' + name : 'Street in ' + zoneOf(p);
  }

  function nearestPlace(p, maxD) {
    let best = null;
    for (const pl of PLACES) {
      const d = dist(p, pl);
      if (d <= maxD && (!best || d < best.d)) best = { place: pl, d };
    }
    return best && best.place;
  }

  function approxArea(p) {
    const pl = nearestPlace(p, 1000);
    return pl ? 'Near ' + pl.name : zoneOf(p) + ' area';
  }

  // ---- Search for the rider's "Where to?" box ----------------------------------
  // Matches landmarks, suburbs and every street name, ignoring upper/lower case, accents,
  // punctuation and a leading house number ("5 sam nujoma" finds Sam Nujoma Drive).
  const fold = (t) => String(t).normalize('NFD').replace(/[̀-ͯ]/g, '').toLowerCase().replace(/[^a-z0-9]+/g, ' ').trim();
  let STREET_INDEX = null;
  function streetIndex() {
    if (STREET_INDEX) return STREET_INDEX;
    // One point per street: the middle of its longest stretch of road.
    const longest = new Map();
    for (const e of EDGES) if (e.name >= 0 && (!longest.has(e.name) || e.len > longest.get(e.name).len)) longest.set(e.name, e);
    STREET_INDEX = [...longest.values()].map((e) => {
      const p = pointOnEdge(e, e.len / 2);
      return { type: 'street', name: ROADS.names[e.name], key: fold(ROADS.names[e.name]), x: Math.round(p.x), y: Math.round(p.y) };
    });
    return STREET_INDEX;
  }

  function searchPlaces(query, limit) {
    const q = fold(query).replace(/^\d+[a-z]?\s+/, '');
    const all = [
      ...PLACES.map((p) => ({ type: 'place', id: p.id, name: p.name, key: fold(p.name + ' ' + p.kind), x: p.x, y: p.y, kind: p.kind })),
      ...SUBURBS.map((z) => ({ type: 'suburb', name: z.name, key: fold(z.name), x: z.x, y: z.y })),
    ];
    if (!q) return all.filter((r) => r.type === 'place');
    const words = q.split(' ');
    const matches = [...all, ...streetIndex()].filter((r) => words.every((w) => r.key.includes(w)));
    // Landmarks first, then suburbs, then streets; within each, names that start with the query first.
    const rank = { place: 0, suburb: 1, street: 2 };
    matches.sort((a, b) => rank[a.type] - rank[b.type] || (b.key.startsWith(q) - a.key.startsWith(q)) || a.name.localeCompare(b.name));
    return matches.slice(0, limit || 20).map((r) => {
      const p = snap(r);
      return { type: r.type, id: r.id, name: r.name, x: p.x, y: p.y, sub: r.type === 'place' ? r.kind + ' · ' + addressOf(p) : r.type === 'suburb' ? 'Suburb of Windhoek' : 'Street · ' + zoneOf(p) };
    });
  }

  // The rider's "GPS" location: a street in Windhoek Central, near Post Street Mall.
  const HOME = snap(fromLatLng(-22.5712, 17.0846));
  const nearHome = (dx, dy) => snap({ x: HOME.x + dx, y: HOME.y + dy });

  // ---------------------------------------------------------------------------
  // Pricing, ETA, matching
  // ---------------------------------------------------------------------------
  const speedMs = (s, kmh) => (kmh || CFG.SPEED_KMH) / 3.6 / s.traffic;

  function fareFor(optionId, distKm, durMin, surge) {
    const o = OPTIONS[optionId];
    const base = o.base, distance = o.perKm * distKm, time = o.perMin * durMin;
    const sub = base + distance + time;
    const subtotal = Math.max(o.min, sub);
    const surgeAmt = subtotal * (surge - 1);
    const total = subtotal + surgeAmt;
    return {
      base: r2(base), distance: r2(distance), time: r2(time), minAdj: r2(subtotal - sub), surge, surgeAmt: r2(surgeAmt),
      total: r2(total), distKm, durMin,
    };
  }

  function estimate(s, pickup, dest, optionId) {
    const roadM = roadDist(pickup, dest);
    const durMin = roadM / speedMs(s) / 60; // live ETA: follows traffic jitter
    // The price uses the demand level's base traffic, not the live jitter, so the quote the rider sees
    // on the options screen is exactly what she is charged when she taps Request.
    const priceMin = roadM / (CFG.SPEED_KMH / 3.6 / DEMAND[s.settings.demand].traffic) / 60;
    const f = fareFor(optionId, roadM / 1000, priceMin, s.surge);
    return { distKm: roadM / 1000, durMin, priceMin, total: f.total, low: r2(f.total * 0.93), high: r2(f.total * 1.12), surge: s.surge };
  }

  /**
   * Dispatch ranking. Filters, in priority order:
   *   1. online and not on an active trip
   *   2. vehicle type matches (and wheelchair access if requested)
   *   3. within the 3 km straight-line radius
   * Then ranks by a blend of road-distance ETA and acceptance probability.
   */
  function rankDrivers(s, pickup, optionId, wav, declined, favourites) {
    declined = declined || [];
    favourites = favourites || (s.rider && s.rider.favourites) || [];
    const weights = DEMAND[s.settings.demand].weights;
    const v = speedMs(s);
    // Low-priority riders lose the nearest drivers; high-priority riders get closeness weighted up.
    const rp = s.rider ? s.rider.priority.points : CFG.PRIORITY_START;
    const etaBoost = Math.max(1, rp / CFG.PRIORITY_START);
    const candidates = [], excluded = [];
    for (const d of s.drivers) {
      const straight = dist(d, pickup);
      let reason = null;
      if (!d.online) reason = 'Offline';
      else if (!signedIn(s, d.id)) reason = 'Not signed in on a phone';
      else if (d.busy) reason = 'On an active trip';
      else if (d.vehicle.type !== optionId) reason = 'Vehicle is ' + OPTIONS[d.vehicle.type].short;
      else if (wav && !d.vehicle.wav) reason = 'Not wheelchair accessible';
      else if (declined.includes(d.id)) reason = 'Declined / timed out';
      else if (straight > CFG.RADIUS) reason = 'Outside 3 km (' + (straight / 1000).toFixed(1) + ' km)';
      if (reason) { excluded.push({ driverId: d.id, reason, straight }); continue; }
      const road = roadDist(d, pickup);
      const eta = road / v / 60;
      const etaScore = Math.max(0, 1 - eta / CFG.ETA_NORM_MIN);
      const fav = favourites.includes(d.id);
      const prio = CFG.PRIORITY_DRIVER_W * (d.priority.points - CFG.PRIORITY_START) / CFG.PRIORITY_START;
      const score = weights.eta * etaBoost * etaScore + weights.acc * d.acceptProb + (fav ? CFG.FAV_BONUS : 0) + prio;
      candidates.push({ driverId: d.id, straight, road, eta, acceptProb: d.acceptProb, etaScore, fav, prio, priority: d.priority.points, score });
    }
    // Hold the nearest drivers for higher-priority riders, but always leave at least one.
    const hold = Math.min(Math.floor(Math.max(0, CFG.PRIORITY_START - rp) / CFG.PRIORITY_HOLD_PER), candidates.length - 1);
    if (hold > 0) {
      candidates.sort((a, b) => a.straight - b.straight);
      for (const c of candidates.splice(0, hold)) excluded.push({ driverId: c.driverId, reason: 'Held for higher-priority riders (rider ' + Math.round(rp) + ' pts)', straight: c.straight });
    }
    candidates.sort((a, b) => b.score - a.score);
    return { candidates, excluded, weights };
  }

  // ---------------------------------------------------------------------------
  // Seed data
  // ---------------------------------------------------------------------------
  // Four demo drivers, each a demo account on the driver app; every phone signs in as one of them.
  // All start offline: a driver goes online only when someone signs in as her and taps Go online,
  // so every ride is accepted by a person on a driver phone. Anyone who applies to drive is added to this list.
  // Each home is an offset from the rider's location in metres (east, south), snapped to the nearest road.
  const DEMO_DRIVER_ID = 'd1';
  const SEED_DRIVERS = [
    { id: DEMO_DRIVER_ID, name: 'Hilma Nangolo', phone: '+264 81 640 2219', type: 'classic', make: 'Toyota', model: 'Corolla', year: 2022, color: 'Silver', hex: '#c9ccd1', plate: 'N 6152 W', acc: 0.9, rating: 4.92, n: 214, home: nearHome(900, -700), wander: 1, offline: true, av: ['#8d5524', '#1b1410', '#f3e8ff', 'long'] },
    { id: 'd2', name: 'Selma Nghipandulwa', phone: '+264 81 432 1987', type: 'classic', make: 'Volkswagen', model: 'Polo Vivo', year: 2021, color: 'Pearl White', hex: '#f1ede4', plate: 'N 4471 W', acc: 0.88, rating: 4.88, n: 167, home: nearHome(-800, 600), wander: 1, offline: true, av: ['#c68642', '#120d0b', '#fde8ef', 'bun'] },
    { id: 'd3', name: 'Ndapewa Haufiku', phone: '+264 85 610 2245', type: 'comfort', make: 'Toyota', model: 'Corolla Cross', year: 2023, color: 'Midnight Blue', hex: '#26375f', plate: 'N 2318 W', acc: 0.92, rating: 4.95, n: 301, home: nearHome(700, 900), wander: 1, offline: true, av: ['#6b3e26', '#0d0a08', '#e8f4ff', 'bob'] },
    { id: 'd4', name: 'Martha van Wyk', phone: '+264 81 298 7730', type: 'softlife', make: 'Mercedes-Benz', model: 'C-Class', year: 2023, color: 'Graphite', hex: '#5b5f66', plate: 'N 7705 W', acc: 0.85, rating: 4.97, n: 122, home: nearHome(-900, -600), wander: 1, offline: true, av: ['#e0ac69', '#6b3e26', '#eafaf1', 'long'] },
  ];

  // Earlier trips today, so a demo driver's Earnings tab lists trips that match her trip count.
  const PAST_RIDERS = ['Hilma', 'Ester', 'Loide', 'Aina', 'Johanna', 'Saara', 'Frieda', 'Penda'];
  function seedStats(type) {
    const o = OPTIONS[type], now = Date.now(), recent = [];
    for (let i = randInt(3, 6); i > 0; i--) {
      const fare = r2(Math.max(o.min, o.base + o.perKm * rand(2, 9) + o.perMin * rand(6, 22)));
      recent.push({ rideId: 'FU-' + randInt(3000, 3999), rider: pick(PAST_RIDERS), fare, earned: r2(fare * (1 - o.serviceFee)), at: now - (recent.length + 1) * rand(30, 55) * 60000, to: pick(PLACES).name, ratingFromRider: pick([4, 5, 5, 5]), ratingGiven: 5 });
    }
    return {
      today: r2(recent.reduce((t, x) => t + x.earned, 0)), trips: recent.length, onlineMs: rand(2, 6) * 3600000, recent,
      week: [rand(600, 1300), rand(600, 1300), rand(600, 1300), rand(600, 1300), rand(600, 1300), rand(800, 1600), 0].map(r2),
    };
  }

  function jitterPos(home, w) {
    return snap({ x: home.x + rand(-w, w) * BLOCK, y: home.y + rand(-w, w) * BLOCK });
  }

  function newPriority() { return { points: CFG.PRIORITY_START, cancels: 0 }; }

  // The more often someone cancels, the more each new cancellation costs.
  function penalise(pr) {
    pr.cancels++;
    const lost = CFG.PRIORITY_CANCEL_STEP * pr.cancels;
    pr.points = clamp(pr.points - lost, 0, CFG.PRIORITY_MAX);
    return lost;
  }
  function reward(pr) { pr.points = clamp(pr.points + CFG.PRIORITY_TRIP_GAIN, 0, CFG.PRIORITY_MAX); }

  function makeDriver(o) {
    const parts = o.name.split(' ');
    const w = o.wander || 2;
    const pos = jitterPos(o.home, w);
    return {
      id: o.id, name: o.name, first: parts[0], short: parts[0] + ' ' + (parts[1] ? parts[1][0] + '.' : ''),
      avatar: { skin: o.av[0], hair: o.av[1], bg: o.av[2], style: o.av[3] }, photo: o.photo || null,
      rating: o.rating, ratingCount: o.n, acceptProb: o.acc, verified: true,
      vehicle: { type: o.type, make: o.make, model: o.model, year: o.year, color: o.color, hex: o.hex, plate: o.plate, wav: !!o.wav, photo: o.vphoto || null },
      online: !o.offline, busy: !!o.busy, home: { ...o.home }, wander: w, priority: newPriority(),
      x: pos.x, y: pos.y, heading: 0, path: null, pathPos: 0, pathLen: 0,
      stats: o.stats || seedStats(o.type),
      isNew: !!o.isNew,
    };
  }

  function seedRider() {
    const now = Date.now();
    return {
      onboarded: false, step: 'welcome',
      profile: { name: '', contact: '', photo: null },
      verification: { id: false, selfie: false, status: 'none', submittedAt: 0 },
      contacts: [], consent: { sos: true, rideCheck: true, shareEta: true },
      pickup: { ...HOME, source: 'gps' }, payment: 'card', wallet: 400, rating: 4.92, ratingCount: 38,
      favourites: [], scheduled: [], priority: newPriority(),
      history: [],
    };
  }

  function seedDriverPortal() {
    const docs = {};
    for (const k of DRIVER_DOCS) docs[k] = { status: 'none' };
    return { signedInId: null, step: 'welcome', profile: null, docs, vehicle: null };
  }

  // Each screen running the driver app has its own portal, keyed by the session id it sends with
  // every action, so several phones or emulators can each be signed in as a different driver.
  const portalKey = (sid) => String(sid || 'local').slice(0, 64);
  function portalOf(s, sid) {
    const k = portalKey(sid);
    return s.driverPortals[k] || (s.driverPortals[k] = seedDriverPortal());
  }
  const signedIn = (s, driverId) => Object.values(s.driverPortals).some((p) => p.signedInId === driverId);

  function defaultSettings() {
    return { timeScale: 20, demand: 'normal' };
  }

  function createState(settings) {
    const s = {
      rev: 0, realT: 0, simT: 0, startedWall: Date.now(), seq: 1, noticeSeq: 0, lastEtaAt: 0,
      settings: { ...settings },
      surge: DEMAND[settings.demand].surge, traffic: DEMAND[settings.demand].traffic,
      rider: seedRider(), drivers: SEED_DRIVERS.map(makeDriver), driverPortals: {},
      ride: null, sos: { rider: null, driver: null },
      log: [], notices: [],
    };
    log(s, 'Demo reset — ' + s.drivers.length + ' verified women drivers seeded', 'system');
    return s;
  }

  // ---------------------------------------------------------------------------
  // Bookkeeping
  // ---------------------------------------------------------------------------
  function log(s, text, kind) {
    s.log.unshift({ t: s.realT, wall: Date.now(), text, kind: kind || 'info' });
    if (s.log.length > 80) s.log.length = 80;
  }
  function notify(s, to, text, kind, driverId) {
    s.notices.push({ id: ++s.noticeSeq, to, text, kind: kind || 'info', driverId: driverId || null, t: s.realT });
    if (s.notices.length > 30) s.notices.shift();
  }
  const byId = (s, id) => s.drivers.find((d) => d.id === id);
  const fail = (error) => ({ ok: false, error });
  const code = () => Math.random().toString(36).slice(2, 8).toUpperCase();

  function setState(s, r, st) {
    r.state = st;
    r.stateAt = s.realT;
    r.timeline.push({ state: st, t: s.realT });
    log(s, 'Ride ' + r.id + ' → ' + st, 'state');
  }

  function setPath(d, path) { d.path = path; d.pathPos = 0; d.pathLen = polyLen(path); }
  const pathDone = (d) => !d.path || d.pathPos >= d.pathLen - 0.01;

  function advance(s, d, dtSim, kmh) {
    if (!d.path) return 0;
    const before = d.pathPos;
    d.pathPos = Math.min(d.pathLen, d.pathPos + speedMs(s, kmh) * dtSim / 1000);
    const p = pointAlong(d.path, d.pathPos);
    d.x = p.x; d.y = p.y;
    if (p.heading != null) d.heading = p.heading;
    return d.pathPos - before;
  }

  function wander(s, d, dtSim) {
    if (pathDone(d)) {
      setPath(d, route(d, jitterPos(d.home, d.wander)));
    }
    advance(s, d, dtSim, d.busy ? CFG.SPEED_KMH : CFG.CRUISE_KMH);
  }

  // ---------------------------------------------------------------------------
  // Ride lifecycle
  // ---------------------------------------------------------------------------
  const seatsOf = (n) => clamp(Math.round(Number(n) || 1), 1, CFG.MAX_SEATS);

  function newRide(s, p) {
    const rider = s.rider;
    const dp = snap(p.dest);
    const dest = { x: dp.x, y: dp.y, name: p.dest.name || 'Dropped pin', address: addressOf(dp), area: zoneOf(dp) };
    const pk = rider.pickup;
    const pickup = { x: pk.x, y: pk.y, address: addressOf(pk), area: zoneOf(pk), near: approxArea(pk), street: streetOf(pk) };
    const est = estimate(s, pickup, dest, p.option);
    const r = {
      id: 'FU-' + (4100 + s.seq++), state: 'REQUESTED', stateAt: s.realT, timeline: [{ state: 'REQUESTED', t: s.realT }],
      option: p.option, wav: !!p.wav, seats: seatsOf(p.seats), pickup, dest, pin: String(randInt(1000, 9999)), payment: p.payment || rider.payment,
      // Booked on behalf of someone without internet: they get the PIN and driver details by SMS.
      passenger: p.passenger ? { name: String(p.passenger.name).slice(0, 40), phone: String(p.passenger.phone).slice(0, 30) } : null,
      surge: s.surge, estimate: est, createdWall: Date.now(), scheduled: !!p.scheduled,
      approx: { x: pickup.x + rand(-160, 160), y: pickup.y + rand(-160, 160), r: 380 },
      search: { phase: 'scanning', radius: CFG.RADIUS, ranking: [], excluded: [], weights: null, offers: [], declined: [], offer: null },
      driverId: null, eta: { pickupMin: null, tripMin: est.durMin, at: s.realT },
      messages: [], rideChecks: [], rideCheck: null, stop: null, wait: null, pinErrors: 0, pinErrorAt: 0,
      tripMeters: 0, planned: null, atDest: false, offRouteM: 0, fare: null, paid: false,
      riderRating: null, driverRating: null, cancel: null, share: null, driverShare: null, reports: [], driverDone: false, driverCancelled: null,
    };
    s.ride = r;
    log(s, 'Ride ' + r.id + ' requested: ' + OPTIONS[r.option].short + (r.wav ? ' (wheelchair accessible)' : '') + ' to ' + dest.name + ' · ' + r.seats + ' passenger' + (r.seats > 1 ? 's' : '') + ' · N$' + est.total.toFixed(2) + ' · PIN ' + r.pin, 'state');
    if (r.passenger) {
      notify(s, 'rider', 'PIN ' + r.pin + ' sent by SMS to ' + r.passenger.name + ' (' + r.passenger.phone + ')', 'ok');
      log(s, 'Booked for ' + r.passenger.name + ' · PIN and driver details sent by SMS to ' + r.passenger.phone + ' (no app needed)', 'info');
    }
    return r;
  }

  function nextOffer(s, r) {
    const m = rankDrivers(s, r.pickup, r.option, r.wav, r.search.declined);
    Object.assign(r.search, { ranking: m.candidates, excluded: m.excluded, weights: m.weights, demand: s.settings.demand });
    if (!m.candidates.length) {
      r.search.offer = null;
      setState(s, r, 'NO_DRIVERS');
      notify(s, 'rider', 'No driver accepted nearby. Try again or pick another ride type.', 'err');
      return;
    }
    // Every candidate is signed in on a driver phone: the offer waits for her to tap Accept or Decline.
    const c = m.candidates[0], d = byId(s, c.driverId);
    r.search.offer = {
      driverId: d.id, startedAt: s.realT, expiresAt: s.realT + CFG.OFFER_MS,
      eta: c.eta, road: c.road, straight: c.straight, score: c.score,
    };
    r.search.offers.push({ driverId: d.id, at: s.realT, result: 'pending', score: c.score, eta: c.eta });
    log(s, 'Offer #' + r.search.offers.length + ' → ' + d.name + ' · score ' + Math.round(c.score * 100) + ' · ETA ' + c.eta.toFixed(1) + ' min · accept ' + Math.round(d.acceptProb * 100) + '%', 'match');
    notify(s, 'driver', 'New ride request — 15 seconds to respond', 'info', d.id);
  }

  function resolveOffer(s, r, result) {
    const o = r.search.offer;
    if (!o) return fail('No open offer');
    const d = byId(s, o.driverId);
    const entry = r.search.offers[r.search.offers.length - 1];
    entry.result = result;
    entry.after = s.realT - o.startedAt;
    r.search.offer = null;
    if (result === 'accepted' && d.online && !d.busy) {
      r.driverId = d.id;
      d.busy = true;
      d.path = null;
      r.assignedAt = s.realT;
      setState(s, r, 'DRIVER_ASSIGNED');
      refreshEta(s);
      log(s, d.name + ' accepted after ' + (entry.after / 1000).toFixed(1) + ' s', 'match');
      notify(s, 'rider', d.first + ' accepted your ride', 'ok');
      return { ok: true };
    }
    r.search.declined.push(d.id);
    log(s, d.name + (result === 'timeout' ? ' did not respond within 15 s' : ' declined') + ' → offering to next best driver', 'warn');
    nextOffer(s, r);
    return { ok: true };
  }

  function tickSearch(s, r) {
    const sr = r.search, t = s.realT;
    if (sr.phase === 'scanning') {
      if (t - r.stateAt >= 2000) { sr.phase = 'offering'; nextOffer(s, r); }
      return;
    }
    const o = sr.offer;
    if (!o) return;
    if (t >= o.expiresAt) resolveOffer(s, r, 'timeout');
  }

  function refreshEta(s) {
    s.lastEtaAt = s.realT;
    s.traffic = Math.round(DEMAND[s.settings.demand].traffic * rand(0.95, 1.07) * 1000) / 1000;
    const r = s.ride;
    if (!r) return;
    const v = speedMs(s);
    const d = r.driverId && byId(s, r.driverId);
    let pickupM = null, tripM;
    if (d && r.state === 'DRIVER_EN_ROUTE' && d.path) pickupM = d.pathLen - d.pathPos;
    else if (d && r.state === 'DRIVER_ASSIGNED') pickupM = roadDist(d, r.pickup);
    else if (r.state === 'DRIVER_ARRIVED') pickupM = 0;
    if (r.state === 'IN_TRIP' && d && d.path) tripM = d.pathLen - d.pathPos;
    else if (r.state === 'COMPLETED' || r.state === 'RATED') tripM = 0;
    else tripM = roadDist(r.pickup, r.dest);
    r.eta = { pickupMin: pickupM == null ? null : pickupM / v / 60, pickupM, tripMin: tripM / v / 60, tripM, at: s.realT, traffic: s.traffic };
  }

  function arrive(s, r, d) {
    setState(s, r, 'DRIVER_ARRIVED');
    r.arrivedSim = s.simT;
    notify(s, 'rider', d.first + ' has arrived. Share your PIN ' + r.pin + ' to start the trip.', 'warn');
    notify(s, 'driver', 'Arrived — exact pickup address unlocked. Ask the rider for her PIN.', 'ok', d.id);
    log(s, d.first + ' is within ' + CFG.ARRIVE_M + ' m of pickup → exact address revealed to driver', 'privacy');
  }

  function verifyPin(s, r, pin) {
    if (r.state !== 'DRIVER_ARRIVED') return fail('The PIN can only be entered at the pickup point');
    if (String(pin) !== r.pin) {
      r.pinErrors++;
      r.pinErrorAt = s.realT;
      log(s, 'Incorrect PIN entered by driver (attempt ' + r.pinErrors + ')', 'warn');
      return fail('Incorrect PIN');
    }
    setState(s, r, 'PIN_VERIFIED');
    notify(s, 'rider', 'PIN verified — your driver will start the trip', 'ok');
    notify(s, 'driver', 'PIN verified — destination unlocked: ' + r.dest.name, 'ok', r.driverId);
    log(s, 'PIN verified → destination revealed to driver', 'privacy');
    return { ok: true };
  }

  function startTrip(s, r, d) {
    setPath(d, route(d, r.dest));
    r.planned = d.path.map((p) => ({ x: p.x, y: p.y }));
    r.tripStartSim = s.simT;
    r.tripMeters = 0;
    setState(s, r, 'IN_TRIP');
    refreshEta(s);
  }

  function openRideCheck(s, r, reason) {
    r.rideCheck = { reason, at: s.realT, status: 'open', offRouteM: Math.round(r.offRouteM) };
    log(s, 'RideCheck triggered: ' + (reason === 'deviation' ? 'route deviation (' + Math.round(r.offRouteM) + ' m off planned route)' : 'vehicle stopped for over 2 min'), 'safety');
    notify(s, 'driver', 'RideCheck: FUber is checking in with your rider.', 'warn', r.driverId);
  }

  function tickTrip(s, r, d, dtSim) {
    const t = s.realT;
    if (r.stop) {
      if (!r.rideCheck && !r.stop.checked && s.simT - r.stop.since >= CFG.STOP_SIM_MS) {
        r.stop.checked = true;
        openRideCheck(s, r, 'stop');
      }
    } else if (!r.atDest) {
      r.tripMeters += advance(s, d, dtSim, CFG.SPEED_KMH);
      if (pathDone(d)) {
        r.atDest = true;
        r.atDestAt = t;
        notify(s, 'driver', "You've reached the destination — slide to complete the trip.", 'ok', d.id);
        notify(s, 'rider', 'Arriving at ' + r.dest.name, 'ok');
      }
    }
    if (!r.atDest && !r.rideCheck && r.planned) {
      r.offRouteM = distToPoly(d, r.planned);
      if (r.offRouteM > CFG.DEVIATION_M) openRideCheck(s, r, 'deviation');
    }
  }

  function completeTrip(s, r, d) {
    if (r.state !== 'IN_TRIP') return fail('Trip is not in progress');
    const durMin = Math.max(1, (s.simT - r.tripStartSim) / 60000);
    const distKm = Math.max(0.3, r.tripMeters / 1000);
    // The price is locked at request: the rider pays the quote, and the driver earns the quote minus
    // the service fee, however long the trip actually took (detours, traffic or a skipped demo trip).
    r.fare = fareFor(r.option, r.estimate.distKm, r.estimate.priceMin != null ? r.estimate.priceMin : r.estimate.durMin, r.surge);
    r.durMin = durMin;
    r.distKm = distKm;
    r.rideCheck = null;
    r.stop = null;
    d.busy = false;
    d.path = null;
    const net = r2(r.fare.total * (1 - OPTIONS[r.option].serviceFee));
    r.driverEarning = net;
    reward(s.rider.priority);
    reward(d.priority);
    d.stats.today = r2(d.stats.today + net);
    d.stats.trips++;
    d.stats.recent.unshift({ rideId: r.id, rider: firstName(s.rider.profile.name), fare: r.fare.total, earned: net, at: Date.now(), to: r.dest.name, ratingFromRider: null, ratingGiven: null });
    s.rider.history.unshift({ id: r.id, at: Date.now(), from: r.pickup.address, to: r.dest.name, driverId: d.id, driverName: d.short, option: r.option, fare: r.fare.total, rating: null, status: 'completed', payment: r.payment, distKm, durMin });
    setState(s, r, 'COMPLETED');
    notify(s, 'rider', "You've arrived. Total fare N$" +r.fare.total.toFixed(2), 'ok');
    return { ok: true };
  }

  function rateRider(s, r, p) {
    if (r.driverRating) return fail('Already rated');
    const stars = clamp(Math.round(p.stars || 5), 1, 5);
    r.driverRating = { stars, tags: p.tags || [] };
    const rd = s.rider;
    rd.rating = r2((rd.rating * rd.ratingCount + stars) / (rd.ratingCount + 1));
    rd.ratingCount++;
    const d = byId(s, r.driverId);
    const rec = d && d.stats.recent.find((x) => x.rideId === r.id);
    if (rec) rec.ratingGiven = stars;
    log(s, 'Driver rated rider ★' + stars, 'info');
    if (r.riderRating && r.state === 'COMPLETED') setState(s, r, 'RATED');
    return { ok: true };
  }

  function cancelByRider(s, r) {
    const assigned = ['DRIVER_ASSIGNED', 'DRIVER_EN_ROUTE', 'DRIVER_ARRIVED'].includes(r.state);
    if (!assigned && !['REQUESTED', 'SEARCHING', 'NO_DRIVERS'].includes(r.state)) return fail('Ride can no longer be cancelled');
    const d = r.driverId && byId(s, r.driverId);
    if (d) {
      d.busy = false;
      d.path = null;
      notify(s, 'driver', 'The rider cancelled this trip.', 'warn', d.id);
    }
    r.search.offer = null;
    // Cancelling is free. Instead, every cancellation costs the rider priority points (see penalise).
    r.cancel = { by: 'rider' };
    if (assigned) s.rider.history.unshift({ id: r.id, at: Date.now(), from: r.pickup.address, to: r.dest.name, driverId: d.id, driverName: d.short, option: r.option, fare: 0, rating: null, status: 'cancelled', payment: r.payment });
    setState(s, r, 'CANCELLED');
    log(s, 'Rider cancelled' + (assigned ? ' after a driver was assigned' : ' before a driver was assigned'), 'warn');
    log(s, 'Rider priority −' + penalise(s.rider.priority) + ' → ' + s.rider.priority.points + ' pts', 'system');
    return { ok: true };
  }

  function cancelByDriver(s, r, reason) {
    if (!['DRIVER_ASSIGNED', 'DRIVER_EN_ROUTE', 'DRIVER_ARRIVED'].includes(r.state)) return fail('Trip can no longer be cancelled');
    const d = byId(s, r.driverId);
    if (reason === 'no_show') {
      if (!r.wait || s.simT - r.wait.startedSim < CFG.WAIT_FREE_SIM_MS) return fail('Start the wait timer and wait 5 minutes first');
      d.busy = false;
      d.path = null;
      r.cancel = { by: 'driver', reason: 'Rider did not show up' };
      s.rider.history.unshift({ id: r.id, at: Date.now(), from: r.pickup.address, to: r.dest.name, driverId: d.id, driverName: d.short, option: r.option, fare: 0, rating: null, status: 'no-show', payment: r.payment });
      setState(s, r, 'CANCELLED');
      notify(s, 'rider', 'Your driver waited 5 minutes and cancelled the trip.', 'err');
      log(s, 'Driver cancelled: rider did not show up', 'warn');
      log(s, 'Rider priority −' + penalise(s.rider.priority) + ' (no-show) → ' + s.rider.priority.points + ' pts', 'system');
      return { ok: true };
    }
    // Any other reason: the request goes back into the pool.
    d.busy = false;
    d.path = null;
    r.search.declined.push(d.id);
    r.driverCancelled = { name: d.first, reason };
    r.driverId = null;
    r.wait = null;
    r.search.phase = 'scanning';
    r.search.offer = null;
    setState(s, r, 'SEARCHING');
    notify(s, 'rider', 'Your driver had to cancel. Finding you another driver — no charge.', 'warn');
    log(s, d.name + ' cancelled (' + reason + ') → request returned to the pool', 'warn');
    log(s, d.first + ' priority −' + penalise(d.priority) + ' → ' + d.priority.points + ' pts', 'system');
    return { ok: true };
  }

  // What a rider's SOS sends along with the alert: the car, the driver and the trip, frozen at the moment SOS was pressed.
  function rideDetails(s) {
    const r = s.ride;
    if (!r || ['COMPLETED', 'RATED', 'CANCELLED', 'NO_DRIVERS'].includes(r.state)) return null;
    const d = r.driverId && byId(s, r.driverId);
    const where = toLatLng(d && r.state === 'IN_TRIP' ? d : r.pickup);
    return {
      rideId: r.id, state: r.state, option: OPTIONS[r.option].name, pin: r.pin,
      pickup: r.pickup.address, dest: r.dest.name,
      location: where.lat.toFixed(5) + ', ' + where.lon.toFixed(5),
      driver: d ? { name: d.name, rating: d.rating } : null,
      vehicle: d ? { make: d.vehicle.make, model: d.vehicle.model, year: d.vehicle.year, color: d.vehicle.color, plate: d.vehicle.plate } : null,
    };
  }

  function startSos(s, who, via, driverId) {
    const sent = via === 'RideCheck';
    s.sos[who] = { status: sent ? 'sent' : 'sending', at: s.realT, via: via || 'button', rideId: s.ride ? s.ride.id : null, driverId: driverId || null, details: who === 'rider' ? rideDetails(s) : null };
    log(s, 'SOS pressed by ' + who + (via === 'RideCheck' ? ' via RideCheck' : '') + (sent ? ' → alert sent' : ' → 3 s to cancel'), 'safety');
    if (sent) logSosDetails(s, s.sos[who]);
  }

  function logSosDetails(s, a) {
    const t = a.details;
    if (!t) return;
    const v = t.vehicle;
    log(s, 'SOS ride details sent: ' + t.rideId + (v ? ' · ' + v.color + ' ' + v.make + ' ' + v.model + ' (' + v.year + ') · plate ' + v.plate + ' · driver ' + t.driver.name : ' · no driver assigned yet') +
      ' · ' + t.pickup + ' → ' + t.dest + ' · GPS ' + t.location, 'safety');
  }

  function tickSos(s) {
    for (const who of ['rider', 'driver']) {
      const a = s.sos[who];
      if (a && a.status === 'sending' && s.realT - a.at >= 3000) {
        a.status = 'sent';
        a.sentAt = s.realT;
        log(s, 'SOS from ' + who + ': alert + live location sent to emergency contacts and the FUber Safety Team', 'safety');
        logSosDetails(s, a);
      }
    }
  }

  // Busy areas, shown as the demand heat map on the driver's map.
  const HOTSPOTS = [['postst', 3], ['sqmarket', 2], ['maerua', 2], ['wernhil', 2], ['katuhosp', 1], ['grove', 1], ['unam', 1]]
    .map(([id, w]) => { const pl = PLACES.find((x) => x.id === id); return { x: pl.x, y: pl.y, w }; });
  // Verifications are approved by a click (on the phone or in the presentation panel), never on a timer.
  function approveRider(s) {
    const v = s.rider.verification;
    if (v.status !== 'pending') return false;
    v.status = 'verified';
    notify(s, 'rider', 'Identity verified — welcome to FUber', 'ok');
    log(s, 'Rider gender/ID verification approved', 'system');
    return true;
  }

  // Approves every document in review and, once the application is submitted, the driver herself.
  function approvePortal(s, dp) {
    let any = false;
    for (const k of DRIVER_DOCS) if (dp.docs[k].status === 'review') { dp.docs[k].status = 'approved'; any = true; }
    if (dp.step === 'review' && DRIVER_DOCS.every((k) => dp.docs[k].status === 'approved')) { createPortalDriver(s, dp); any = true; }
    return any;
  }

  function createPortalDriver(s, dp) {
    const pf = dp.profile, v = dp.vehicle;
    const ang = rand(0, Math.PI * 2), rr = rand(600, 1000);
    const home = snap({ x: HOME.x + Math.cos(ang) * rr, y: HOME.y + Math.sin(ang) * rr });
    const id = 'd' + (s.drivers.length + 1);
    const tones = [['#8d5524', '#1b1410'], ['#f1c99a', '#3a2418'], ['#c68642', '#120d0b'], ['#e0ac69', '#6b3e26']];
    const tone = pick(tones);
    const d = makeDriver({
      id, name: pf.name, type: v.type, make: v.make, model: v.model, year: v.year, color: v.color, hex: v.hex, plate: v.plate,
      wav: v.wav, acc: 0.9, rating: 5, n: 0, home, wander: 1, offline: true, photo: pf.photo, vphoto: v.photo,
      av: [tone[0], tone[1], '#f3e8ff', 'long'], isNew: true,
      stats: { today: 0, trips: 0, onlineMs: 0, recent: [], week: [0, 0, 0, 0, 0, 0, 0] },
    });
    d.x = home.x; d.y = home.y;
    s.drivers.push(d);
    dp.signedInId = id;
    dp.step = 'approved';
    notify(s, 'driver', 'Application approved — you can go online', 'ok', id);
    log(s, 'Driver ' + pf.name + ' approved and added to the pool (offline until she goes online)', 'system');
  }

  function firstName(n) { return (n || 'Rider').split(' ')[0]; }

  function tickRide(s, dtSim) {
    const r = s.ride;
    if (!r) return;
    const since = s.realT - r.stateAt;
    const d = r.driverId ? byId(s, r.driverId) : null;
    switch (r.state) {
      case 'REQUESTED':
        if (since > 800) { setState(s, r, 'SEARCHING'); r.search.phase = 'scanning'; }
        break;
      case 'SEARCHING':
        tickSearch(s, r);
        break;
      case 'DRIVER_ASSIGNED':
        if (since > 1800) { setPath(d, route(d, r.pickup)); setState(s, r, 'DRIVER_EN_ROUTE'); refreshEta(s); }
        break;
      case 'DRIVER_EN_ROUTE':
        advance(s, d, dtSim, CFG.SPEED_KMH);
        if (dist(d, r.pickup) <= CFG.ARRIVE_M) arrive(s, r, d);
        break;
      case 'DRIVER_ARRIVED':
        advance(s, d, dtSim, CFG.SPEED_KMH);
        break;
      case 'IN_TRIP':
        tickTrip(s, r, d, dtSim);
        break;
    }
  }

  function tick(s, dt) {
    s.realT += dt;
    const dtSim = dt * s.settings.timeScale;
    s.simT += dtSim;
    tickSos(s);
    const locked = s.ride && DRIVER_LOCKED.has(s.ride.state) ? s.ride.driverId : null;
    for (const d of s.drivers) {
      if (!d.online) continue;
      d.stats.onlineMs += dtSim;
      if (d.id !== locked) wander(s, d, dtSim);
    }
    tickRide(s, dtSim);
    if (s.realT - s.lastEtaAt >= 2000) refreshEta(s);
    s.rev++;
  }

  // ---------------------------------------------------------------------------
  // Actions
  // ---------------------------------------------------------------------------
  const rideFor = (s) => s.ride;
  const H = {
    // ---- global / presenter -------------------------------------------------
    SET_SETTING(s, p) {
      if (p.key === 'timeScale') s.settings.timeScale = clamp(Number(p.value) || 20, 1, 120);
      else return fail('Unknown setting');
      log(s, 'Setting ' + p.key + ' = ' + s.settings[p.key], 'system');
    },
    SET_DEMAND(s, p) {
      if (!DEMAND[p.level]) return fail('Unknown demand level');
      s.settings.demand = p.level;
      s.surge = DEMAND[p.level].surge;
      refreshEta(s);
      const w = DEMAND[p.level].weights;
      log(s, DEMAND[p.level].label + ' → surge ' + s.surge.toFixed(1) + '×, matching weights ETA ' + Math.round(w.eta * 100) + '% / acceptance ' + Math.round(w.acc * 100) + '%', 'system');
    },
    X_FORCE_OFFER(s, p) {
      const r = rideFor(s);
      if (!r || !r.search.offer) return fail('No open offer');
      return resolveOffer(s, r, p.accept ? 'accepted' : 'declined');
    },
    X_ARRIVE_NOW(s) {
      const r = rideFor(s);
      if (!r || r.state !== 'DRIVER_EN_ROUTE') return fail('Driver is not en route');
      const d = byId(s, r.driverId);
      d.pathPos = Math.max(d.pathPos, d.pathLen - 60);
    },
    X_JUMP_DEST(s) {
      const r = rideFor(s);
      if (!r || r.state !== 'IN_TRIP') return fail('No trip in progress');
      const d = byId(s, r.driverId);
      r.stop = null;
      r.tripMeters += Math.max(0, d.pathLen - 40 - d.pathPos);
      d.pathPos = Math.max(d.pathPos, d.pathLen - 40);
    },
    X_DEVIATE(s) {
      const r = rideFor(s);
      if (!r || r.state !== 'IN_TRIP' || r.atDest) return fail('Deviation can only be simulated during a trip');
      const d = byId(s, r.driverId);
      const h = (d.heading || 0) * Math.PI / 180;
      const dir = { x: Math.cos(h), y: Math.sin(h) };
      const options = [1, -1].map((side) => {
        const left = { x: dir.y * side, y: -dir.x * side };
        const wp = snap({ x: d.x + dir.x * 400 + left.x * 900, y: d.y + dir.y * 400 + left.y * 900 });
        return { wp, off: distToPoly(wp, r.planned) };
      });
      const best = options[0].off >= options[1].off ? options[0] : options[1];
      const path = clean([...route(d, best.wp), ...route(best.wp, r.dest)]);
      setPath(d, path);
      r.stop = null;
      log(s, 'Scenario: driver leaves the planned route', 'scenario');
    },
    X_STOP(s) {
      const r = rideFor(s);
      if (!r || r.state !== 'IN_TRIP' || r.atDest) return fail('Stops can only be simulated during a trip');
      r.stop = { since: s.simT, checked: false };
      log(s, 'Scenario: vehicle stops unexpectedly (RideCheck after 2 sim-min)', 'scenario');
    },
    X_APPROVE(s) {
      let any = approveRider(s);
      for (const dp of Object.values(s.driverPortals)) any = approvePortal(s, dp) || any;
      if (!any) return fail('Nothing is waiting for approval');
    },
    X_SHUFFLE(s) {
      const locked = s.ride && s.ride.driverId;
      for (const d of s.drivers) {
        if (d.id === locked) continue;
        const p = jitterPos(d.home, d.wander);
        d.x = p.x; d.y = p.y; d.path = null;
      }
      log(s, 'Driver positions shuffled', 'scenario');
    },
    X_TOGGLE_ONLINE(s, p) {
      const d = byId(s, p.id);
      if (!d) return fail('No such driver');
      if (s.ride && s.ride.driverId === d.id && DRIVER_LOCKED.has(s.ride.state)) return fail('Driver is on this trip');
      d.online = !d.online;
      if (!d.online) d.path = null;
      log(s, d.name + ' is now ' + (d.online ? 'online' : 'offline'), 'scenario');
    },
    X_TOGGLE_BUSY(s, p) {
      const d = byId(s, p.id);
      if (!d) return fail('No such driver');
      if (s.ride && s.ride.driverId === d.id) return fail('Driver is on this trip');
      d.busy = !d.busy;
      log(s, d.name + (d.busy ? ' started another trip (busy)' : ' finished her other trip (available)'), 'scenario');
    },

    // ---- rider: onboarding --------------------------------------------------
    R_GOTO(s, p) { s.rider.step = p.step; },
    R_DEMO_LOGIN(s) {
      const r = s.rider;
      r.profile = { name: 'Ndeshi Amutenya', contact: '+264 81 234 5678', photo: null };
      r.verification = { id: true, selfie: true, status: 'verified', submittedAt: 0 };
      r.contacts = [{ name: 'Meme Selma (Mum)', phone: '+264 81 555 0142' }, { name: 'Taimi (sister)', phone: '+264 85 555 0199' }];
      r.consent = { sos: true, rideCheck: true, shareEta: true };
      r.onboarded = true;
      r.step = 'done';
      log(s, 'Rider signed in with demo account (verified)', 'system');
    },
    R_SIGNUP(s, p) {
      if (!p.name || !p.contact) return fail('Name and phone/email are required');
      s.rider.profile = { name: String(p.name).slice(0, 40), contact: String(p.contact).slice(0, 60), photo: p.photo || null };
      s.rider.step = 'verify';
      log(s, 'Rider account created: ' + s.rider.profile.name, 'system');
    },
    R_UPLOAD(s, p) {
      if (p.doc !== 'id' && p.doc !== 'selfie') return fail('Unknown document');
      s.rider.verification[p.doc] = true;
    },
    R_SUBMIT_VERIFICATION(s) {
      const v = s.rider.verification;
      if (!v.id || !v.selfie) return fail('Upload your ID and selfie first');
      v.status = 'pending';
      v.submittedAt = s.realT;
      log(s, 'Rider submitted ID + selfie → pending verification', 'system');
    },
    R_APPROVE(s) { if (!approveRider(s)) return fail('Nothing is waiting for approval'); },
    R_SAVE_CONTACTS(s, p) {
      const contacts = (p.contacts || []).filter((c) => c && c.name && c.phone).slice(0, 5);
      if (!contacts.length) return fail('Add at least one emergency contact');
      s.rider.contacts = contacts;
      s.rider.consent = { ...s.rider.consent, ...p.consent };
      s.rider.onboarded = true;
      s.rider.step = 'done';
      log(s, 'Rider onboarding complete (' + contacts.length + ' emergency contact' + (contacts.length > 1 ? 's' : '') + ')', 'system');
    },
    R_ADD_CONTACT(s, p) {
      if (!p.name || !p.phone) return fail('Name and phone required');
      s.rider.contacts.push({ name: p.name, phone: p.phone });
    },
    R_REMOVE_CONTACT(s, p) { s.rider.contacts.splice(p.index, 1); },
    R_SET_CONSENT(s, p) { s.rider.consent[p.key] = !!p.value; },
    R_SET_PAYMENT(s, p) { s.rider.payment = p.method; },
    R_TOGGLE_FAV(s, p) {
      const f = s.rider.favourites, i = f.indexOf(p.driverId);
      if (i >= 0) f.splice(i, 1); else f.push(p.driverId);
    },
    R_SIGN_OUT(s) {
      if (s.ride) return fail('Finish your ride first');
      s.rider = seedRider();
    },

    // ---- rider: booking -----------------------------------------------------
    R_SET_PICKUP(s, p) {
      if (s.ride) return fail('Ride in progress');
      const q = snap(p);
      s.rider.pickup = { x: q.x, y: q.y, source: 'pin' };
    },
    R_PICKUP_GPS(s) { if (!s.ride) s.rider.pickup = { ...HOME, source: 'gps' }; },
    R_REQUEST(s, p) {
      if (s.ride) return fail('You already have an active ride');
      if (!s.rider.onboarded || s.rider.verification.status !== 'verified') return fail('Verification required');
      if (!OPTIONS[p.option] || !p.dest) return fail('Choose a destination and ride type');
      if (p.passenger && (!p.passenger.name || !p.passenger.phone)) return fail("Enter the passenger's name and phone number");
      newRide(s, p);
    },
    R_SCHEDULE(s, p) {
      if (!OPTIONS[p.option] || !p.dest || !p.when) return fail('Choose a destination, ride type and time');
      const dp = snap(p.dest);
      const est = estimate(s, s.rider.pickup, dp, p.option);
      s.rider.scheduled.push({ id: 'S' + s.seq++, when: p.when, option: p.option, wav: !!p.wav, seats: seatsOf(p.seats), payment: p.payment, total: est.total, passenger: p.passenger || null, dest: { x: dp.x, y: dp.y, name: p.dest.name || addressOf(dp) }, low: est.low, high: est.high });
      notify(s, 'rider', 'Ride scheduled — we will dispatch a driver 10 minutes before pickup', 'ok');
      log(s, 'Ride scheduled for ' + p.when + ' → ' + (p.dest.name || 'dropped pin'), 'state');
    },
    R_CANCEL_SCHEDULED(s, p) { s.rider.scheduled = s.rider.scheduled.filter((x) => x.id !== p.id); },
    R_DISPATCH_SCHEDULED(s, p) {
      if (s.ride) return fail('Finish the current ride first');
      const item = s.rider.scheduled.find((x) => x.id === p.id);
      if (!item) return fail('Not found');
      s.rider.scheduled = s.rider.scheduled.filter((x) => x.id !== p.id);
      newRide(s, { ...item, scheduled: true });
      log(s, 'Scheduled ride is due → dispatching now', 'state');
    },
    R_CANCEL(s) {
      const r = rideFor(s);
      if (!r) return fail('No active ride');
      return cancelByRider(s, r);
    },
    R_RETRY(s) {
      const r = rideFor(s);
      if (!r || r.state !== 'NO_DRIVERS') return fail('Nothing to retry');
      r.search.declined = [];
      r.search.phase = 'scanning';
      setState(s, r, 'SEARCHING');
    },
    R_FINISH(s) {
      const r = rideFor(s);
      if (r && !['RATED', 'CANCELLED'].includes(r.state)) return fail('Ride still active');
      s.ride = null;
    },
    R_PAY_RATE(s, p) {
      const r = rideFor(s);
      if (!r || r.state !== 'COMPLETED' || r.riderRating) return fail('Nothing to rate');
      const d = byId(s, r.driverId);
      const stars = clamp(Math.round(p.stars || 5), 1, 5);
      r.paid = true;
      r.payment = p.method || r.payment;
      if (r.payment === 'wallet') s.rider.wallet = r2(s.rider.wallet - r.fare.total);
      r.riderRating = { stars, tags: p.tags || [], comment: String(p.comment || '').slice(0, 200) };
      r.riderRatedAt = s.realT;
      d.rating = r2((d.rating * d.ratingCount + stars) / (d.ratingCount + 1));
      d.ratingCount++;
      if (p.favourite && !s.rider.favourites.includes(d.id)) {
        s.rider.favourites.push(d.id);
        log(s, d.name + ' added to Favourite Drivers (+' + CFG.FAV_BONUS * 100 + ' match score next time)', 'match');
      }
      const h = s.rider.history.find((x) => x.id === r.id);
      if (h) { h.rating = stars; h.payment = r.payment; }
      const rec = d.stats.recent.find((x) => x.rideId === r.id);
      if (rec) rec.ratingFromRider = stars;
      notify(s, 'driver', firstName(s.rider.profile.name) + ' rated you ★' + stars, 'ok', d.id);
      log(s, 'Rider paid N$' + r.fare.total.toFixed(2) + ' (' + r.payment + ') and rated driver ★' + stars, 'info');
      if (r.driverRating || r.driverDone) setState(s, r, 'RATED');
    },
    R_RIDECHECK(s, p) {
      const r = rideFor(s);
      if (!r || !r.rideCheck) return fail('No open RideCheck');
      const rc = r.rideCheck;
      rc.status = p.answer === 'help' ? 'help' : 'ok';
      rc.answeredAt = s.realT;
      r.rideChecks.push(rc);
      r.rideCheck = null;
      const d = byId(s, r.driverId);
      if (rc.reason === 'deviation') r.planned = remainingPath(d.path, d.pathPos);
      if (rc.reason === 'stop') r.stop = null;
      if (rc.status === 'help') startSos(s, 'rider', 'RideCheck');
      log(s, 'RideCheck answered: ' + (rc.status === 'ok' ? "\"Yes, I'm fine\"" : '"Call for help" → SOS escalated'), 'safety');
    },
    R_MESSAGE(s, p) {
      const r = rideFor(s);
      if (!r || !r.driverId) return fail('No driver yet');
      r.messages.push({ from: 'rider', text: String(p.text).slice(0, 200), t: s.realT });
      notify(s, 'driver', firstName(s.rider.profile.name) + ': ' + p.text, 'msg', r.driverId);
    },
    R_SHARE(s) {
      const r = rideFor(s);
      if (!r) return fail('No active trip to share');
      if (!r.share) r.share = { code: code(), at: s.realT };
      log(s, 'Rider shared trip link fuber.app/t/' + r.share.code, 'safety');
    },
    R_CALL(s) { log(s, 'Rider called driver through masked number', 'privacy'); },
    R_SOS(s) { startSos(s, 'rider'); },
    R_SOS_CANCEL(s) { if (s.sos.rider) log(s, 'Rider marked herself safe — SOS closed', 'safety'); s.sos.rider = null; },

    // ---- driver portal ------------------------------------------------------
    D_GOTO(s, p, sid) { portalOf(s, sid).step = p.step; },
    D_DEMO_LOGIN(s, p, sid) {
      const dp = portalOf(s, sid);
      if (dp.signedInId) return fail('Already signed in');
      if (!OPTIONS[p.type]) return fail('Choose a ride option');
      const seed = SEED_DRIVERS.find((x) => x.id === (p.driverId || DEMO_DRIVER_ID));
      if (!seed) return fail('Choose a demo driver');
      const d = byId(s, seed.id);
      // One phone per driver: signing in here signs her out on the other phone (e.g. a closed emulator).
      for (const [k, other] of Object.entries(s.driverPortals)) {
        if (other !== dp && other.signedInId === d.id) { delete s.driverPortals[k]; log(s, d.name + ' was signed out on another phone', 'system'); }
      }
      dp.profile = { name: d.name, contact: seed.phone, photo: null };
      for (const k of DRIVER_DOCS) dp.docs[k] = { status: 'approved' };
      if (!d.busy) d.vehicle.type = p.type;
      dp.vehicle = { ...d.vehicle };
      dp.signedInId = d.id;
      dp.step = 'done';
      log(s, d.name + ' signed in with her demo account (' + OPTIONS[d.vehicle.type].short + ')', 'system');
    },
    D_SIGNUP(s, p, sid) {
      if (!p.name || !p.contact) return fail('Name and phone/email are required');
      const dp = portalOf(s, sid);
      dp.profile = { name: String(p.name).slice(0, 40), contact: String(p.contact).slice(0, 60), photo: p.photo || null };
      dp.step = 'docs';
    },
    D_UPLOAD_DOC(s, p, sid) {
      const doc = portalOf(s, sid).docs[p.doc];
      if (!doc) return fail('Unknown document');
      if (doc.status === 'none') doc.status = 'review';
    },
    D_APPROVE_DOC(s, p, sid) {
      const doc = portalOf(s, sid).docs[p.doc];
      if (!doc || doc.status !== 'review') return fail('This document is not in review');
      doc.status = 'approved';
    },
    D_APPROVE(s, p, sid) { if (!approvePortal(s, portalOf(s, sid))) return fail('Nothing is waiting for approval'); },
    D_DOCS_NEXT(s, p, sid) {
      const dp = portalOf(s, sid);
      if (!DRIVER_DOCS.every((k) => dp.docs[k].status !== 'none')) return fail('Upload every document first');
      dp.step = 'vehicle';
    },
    D_SAVE_VEHICLE(s, p, sid) {
      const v = p.vehicle || {};
      if (!v.make || !v.model || !v.plate || !OPTIONS[v.type]) return fail('Make, model, plate and type are required');
      const dp = portalOf(s, sid);
      dp.vehicle = { type: v.type, make: v.make, model: v.model, year: Number(v.year) || 2022, color: v.color || 'White', hex: v.hex || '#e8e4dc', plate: String(v.plate).toUpperCase(), wav: !!v.wav, photo: v.photo || null };
      dp.step = 'review';
      log(s, 'Driver application submitted for review', 'system');
    },
    D_SIGN_OUT(s, p, sid) {
      const dp = s.driverPortals[portalKey(sid)];
      if (!dp) return;
      if (s.ride && s.ride.driverId === dp.signedInId && DRIVER_LOCKED.has(s.ride.state)) return fail('Finish your trip first');
      // Nobody is left to answer her requests, so she goes offline.
      const d = dp.signedInId && byId(s, dp.signedInId);
      if (d && d.online) { d.online = false; d.path = null; log(s, d.name + ' signed out and went offline', 'system'); }
      delete s.driverPortals[portalKey(sid)];
    },
    D_GO_ONLINE(s, p) {
      const d = byId(s, p.driverId);
      if (!d) return fail('Not signed in');
      d.online = true;
      log(s, d.name + ' went online', 'system');
    },
    D_GO_OFFLINE(s, p) {
      const d = byId(s, p.driverId);
      if (!d) return fail('Not signed in');
      if (s.ride && s.ride.driverId === d.id && DRIVER_LOCKED.has(s.ride.state)) return fail('Finish your trip first');
      d.online = false;
      d.path = null;
      log(s, d.name + ' went offline', 'system');
    },
    D_RESPOND(s, p) {
      const r = rideFor(s);
      if (!r || !r.search.offer || r.search.offer.driverId !== p.driverId) return fail('This request is no longer available');
      return resolveOffer(s, r, p.accept ? 'accepted' : 'declined');
    },
    D_START_WAIT(s) {
      const r = rideFor(s);
      if (!r || r.state !== 'DRIVER_ARRIVED') return fail('You must be at the pickup point');
      if (!r.wait) r.wait = { startedSim: s.simT };
      notify(s, 'rider', 'Your driver is waiting at the pickup point — free waiting time 5 min', 'warn');
      log(s, 'Driver started the wait timer (5 sim-min free wait)', 'info');
    },
    D_ENTER_PIN(s, p) {
      const r = rideFor(s);
      if (!r) return fail('No trip');
      return verifyPin(s, r, p.pin);
    },
    D_START_TRIP(s) {
      const r = rideFor(s);
      if (!r || r.state !== 'PIN_VERIFIED') return fail('Verify the PIN first');
      startTrip(s, r, byId(s, r.driverId));
    },
    D_CANCEL(s, p) {
      const r = rideFor(s);
      if (!r) return fail('No trip');
      return cancelByDriver(s, r, p.reason || 'Other');
    },
    D_COMPLETE(s) {
      const r = rideFor(s);
      if (!r || r.state !== 'IN_TRIP') return fail('No trip in progress');
      const d = byId(s, r.driverId);
      if (!r.atDest && dist(d, r.dest) > 250) return fail('You can complete the trip once you reach the destination');
      return completeTrip(s, r, d);
    },
    D_RATE(s, p) {
      const r = rideFor(s);
      if (!r || !['COMPLETED', 'RATED'].includes(r.state)) return fail('Nothing to rate');
      return rateRider(s, r, p);
    },
    D_FINISH(s) {
      const r = rideFor(s);
      if (!r) return;
      r.driverDone = true;
      // Leaving without rating skips the rating instead of inventing one.
      if (r.state === 'COMPLETED' && !r.driverRating) {
        log(s, 'Driver skipped rating the rider', 'info');
        if (r.riderRating) setState(s, r, 'RATED');
      }
    },
    D_MESSAGE(s, p) {
      const r = rideFor(s);
      if (!r || !r.driverId) return fail('No trip');
      r.messages.push({ from: 'driver', text: String(p.text).slice(0, 200), t: s.realT });
      notify(s, 'rider', byId(s, r.driverId).first + ': ' + p.text, 'msg');
    },
    D_SHARE(s) {
      const r = rideFor(s);
      if (!r) return fail('No trip');
      if (!r.driverShare) r.driverShare = { code: code(), at: s.realT };
      log(s, 'Driver shared route fuber.app/r/' + r.driverShare.code + ' with FUber Safety', 'safety');
    },
    D_REPORT(s, p) {
      const r = rideFor(s);
      if (!r) return fail('No trip');
      r.reports.push({ reason: p.reason, t: s.realT });
      notify(s, 'driver', 'Report submitted. The FUber Safety Team will review it within 24 h.', 'ok', r.driverId);
      log(s, 'Driver reported rider: ' + p.reason, 'safety');
    },
    D_CALL(s) { log(s, 'Driver called rider through masked number', 'privacy'); },
    D_SOS(s, p) { startSos(s, 'driver', null, p.driverId); },
    D_SOS_CANCEL(s, p) {
      const a = s.sos.driver;
      if (!a || (p.driverId && a.driverId && a.driverId !== p.driverId)) return;
      log(s, 'Driver marked herself safe — SOS closed', 'safety');
      s.sos.driver = null;
    },
  };

  class Sim {
    constructor() { this.state = createState(defaultSettings()); }
    reset() { this.state = createState(this.state.settings); }
    dispatch(action) {
      if (!action || typeof action.type !== 'string') return fail('Bad action');
      if (action.type === 'RESET') { this.reset(); return { ok: true }; }
      const fn = H[action.type];
      if (!fn) return fail('Unknown action ' + action.type);
      let res;
      try { res = fn(this.state, action.payload || {}, action.sid); } catch (e) { return fail(e.message); }
      this.state.rev++;
      return res || { ok: true };
    }
    tick(dt) { tick(this.state, dt); }
  }

  return {
    Sim, WORLD, BOUNDS, ROADS, PLACES, SUBURBS, HOME, HOTSPOTS, OPTIONS, DEMAND, CFG, STATES, DRIVER_DOCS,
    DEMO_DRIVER_IDS: SEED_DRIVERS.map((d) => d.id), seedDriverPortal,
    snap, route, roadDist, fromLatLng, toLatLng, searchPlaces, polyLen, pointAlong, remainingPath, distToPoly, nextTurn, dist, zoneOf, streetOf, addressOf, approxArea, nearestPlace,
    estimate, fareFor, rankDrivers, speedMs,
  };
});
