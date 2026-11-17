/* Shared client helpers: transport, formatting, icons, avatars, map, view patching. */
(function () {
  'use strict';
  const E = window.HeraEngine;

  // ---------------------------------------------------------------------------
  // Transport — talks to server.js over SSE, or runs the engine in-page.
  // ---------------------------------------------------------------------------
  const Store = {
    state: null,
    mode: 'local',
    subs: new Set(),
    // Identifies this screen to the engine, so every phone, emulator or tab running the driver app
    // signs in as its own driver. Lives in sessionStorage (one per tab / WebView); ?sid= overrides it.
    sid: (function () {
      const fromUrl = new URLSearchParams(location.search).get('sid');
      if (fromUrl) return fromUrl;
      const fresh = Math.random().toString(36).slice(2, 10);
      try {
        const kept = sessionStorage.getItem('hera.sid');
        if (kept) return kept;
        sessionStorage.setItem('hera.sid', fresh);
      } catch (e) { /* storage unavailable */ }
      return fresh;
    })(),
    async init() {
      const params = new URLSearchParams(location.search);
      if (location.protocol.startsWith('http') && !params.has('local')) {
        try {
          const res = await fetch('api/state', { cache: 'no-store' });
          if (res.ok) {
            this.state = await res.json();
            this.mode = 'server';
            const es = new EventSource('api/events');
            es.onmessage = (e) => { this.state = JSON.parse(e.data); this.emit(); };
            es.onerror = () => { document.body.dataset.conn = 'lost'; };
            es.onopen = () => { document.body.dataset.conn = 'ok'; };
            return;
          }
        } catch (e) { /* fall through to local mode */ }
      }
      this.mode = 'local';
      this.sim = new E.Sim();
      this.state = this.sim.state;
      let last = performance.now();
      setInterval(() => {
        const now = performance.now();
        this.sim.tick(Math.min(1000, now - last));
        last = now;
        this.state = this.sim.state;
        this.emit();
      }, 200);
    },
    async send(type, payload) {
      if (this.mode === 'local') {
        const r = this.sim.dispatch({ type, payload, sid: this.sid });
        this.state = this.sim.state;
        this.emit();
        return r;
      }
      try {
        const res = await fetch('api/action', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ type, payload, sid: this.sid }) });
        return await res.json();
      } catch (e) {
        return { ok: false, error: 'Server unreachable' };
      }
    },
    on(fn) { this.subs.add(fn); },
    emit() { for (const fn of this.subs) fn(this.state); },
  };

  // ---------------------------------------------------------------------------
  // Formatting
  // ---------------------------------------------------------------------------
  const esc = (v) => String(v == null ? '' : v).replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
  const money = (n) => 'N$' + (Number(n) || 0).toFixed(2);
  const mins = (m) => (m == null ? '—' : m < 1 ? '<1 min' : Math.round(m) + ' min');
  const km = (m) => (m < 995 ? Math.round(m / 10) * 10 + ' m' : (m / 1000).toFixed(1) + ' km');
  const clock = (d) => d.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });
  const arriveAt = (minutes) => clock(new Date(Date.now() + (minutes || 0) * 60000));
  const dateShort = (t) => new Date(t).toLocaleDateString([], { weekday: 'short', month: 'short', day: 'numeric' });
  const mmss = (ms) => { const t = Math.max(0, Math.ceil(ms / 1000)); return Math.floor(t / 60) + ':' + String(t % 60).padStart(2, '0'); };
  const firstName = (n) => (n || '').split(' ')[0] || 'Rider';
  const initials = (n) => (n || '?').split(' ').map((p) => p[0]).slice(0, 2).join('').toUpperCase();
  const range = (lo, hi) => 'N$' + Math.round(lo) + '–' + Math.round(hi);
  const greeting = () => { const h = new Date().getHours(); return h < 12 ? 'Good morning' : h < 18 ? 'Good afternoon' : 'Good evening'; };

  // ---------------------------------------------------------------------------
  // Icons (24×24 stroke icons)
  // ---------------------------------------------------------------------------
  const P = {
    check: 'M20 6 9 17l-5-5',
    x: 'M18 6 6 18M6 6l12 12',
    shield: 'M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z',
    shieldCheck: 'M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10zM9 12l2 2 4-4',
    phone: 'M22 16.9v3a2 2 0 0 1-2.2 2 19.8 19.8 0 0 1-8.6-3.1 19.5 19.5 0 0 1-6-6A19.8 19.8 0 0 1 2.1 4.2 2 2 0 0 1 4.1 2h3a2 2 0 0 1 2 1.7c.1 1 .4 1.9.7 2.8a2 2 0 0 1-.5 2.1L8.1 9.9a16 16 0 0 0 6 6l1.3-1.3a2 2 0 0 1 2.1-.4c.9.3 1.8.6 2.8.7a2 2 0 0 1 1.7 2z',
    message: 'M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z',
    share: 'M4 12v8a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-8M16 6l-4-4-4 4M12 2v13',
    star: 'M12 2l3.1 6.3 6.9 1-5 4.9 1.2 6.8L12 17.8 5.8 21l1.2-6.8-5-4.9 6.9-1z',
    pin: 'M20 10c0 6-8 12-8 12s-8-6-8-12a8 8 0 0 1 16 0zM12 13a3 3 0 1 0 0-6 3 3 0 0 0 0 6z',
    flag: 'M4 15s1-1 4-1 5 2 8 2 4-1 4-1V3s-1 1-4 1-5-2-8-2-4 1-4 1zM4 22v-7',
    search: 'M11 19a8 8 0 1 0 0-16 8 8 0 0 0 0 16zM21 21l-4.3-4.3',
    clock: 'M12 22a10 10 0 1 0 0-20 10 10 0 0 0 0 20zM12 6v6l4 2',
    calendar: 'M8 2v4M16 2v4M3 10h18M5 4h14a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2z',
    wheelchair: 'M13 4a1.5 1.5 0 1 0 0-3 1.5 1.5 0 0 0 0 3zM19 20l-2.5-6H11l-1-6M8.5 9.5a6 6 0 1 0 7.8 7.8',
    lock: 'M5 11h14v10H5zM8 11V7a4 4 0 0 1 8 0v4',
    unlock: 'M5 11h14v10H5zM8 11V7a4 4 0 0 1 7.7-1.5',
    alert: 'M10.3 3.9 1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0zM12 9v4M12 17h.01',
    user: 'M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2M12 11a4 4 0 1 0 0-8 4 4 0 0 0 0 8z',
    users: 'M17 21v-2a4 4 0 0 0-4-4H5a4 4 0 0 0-4 4v2M9 11a4 4 0 1 0 0-8 4 4 0 0 0 0 8zM23 21v-2a4 4 0 0 0-3-3.9M16 3.1a4 4 0 0 1 0 7.8',
    home: 'M3 10.5 12 3l9 7.5V21h-6v-6H9v6H3z',
    list: 'M8 6h13M8 12h13M8 18h13M3 6h.01M3 12h.01M3 18h.01',
    wallet: 'M20 7H5a2 2 0 0 1 0-4h13v4M3 5v14a2 2 0 0 0 2 2h15V7M16 14h.01',
    card: 'M2 5h20v14H2zM2 10h20',
    cash: 'M2 6h20v12H2zM12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM6 12h.01M18 12h.01',
    heart: 'M20.8 4.6a5.5 5.5 0 0 0-7.7 0L12 5.7l-1.1-1.1a5.5 5.5 0 0 0-7.7 7.8L12 21.2l8.8-8.8a5.5 5.5 0 0 0 0-7.8z',
    turnLeft: 'M9 14 4 9l5-5M4 9h10a6 6 0 0 1 6 6v6',
    turnRight: 'M15 14l5-5-5-5M20 9H10a6 6 0 0 0-6 6v6',
    straight: 'M12 21V4M5 11l7-7 7 7',
    uturn: 'M9 14 4 9l5-5M4 9h9a5 5 0 0 1 0 10h-2',
    chevronRight: 'm9 18 6-6-6-6',
    chevronLeft: 'm15 18-6-6 6-6',
    power: 'M18.4 6.6a9 9 0 1 1-12.8 0M12 2v10',
    info: 'M12 22a10 10 0 1 0 0-20 10 10 0 0 0 0 20zM12 16v-4M12 8h.01',
    nav: 'M3 11 22 2l-9 19-2-8z',
    upload: 'M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4M17 8l-5-5-5 5M12 3v12',
    camera: 'M23 19a2 2 0 0 1-2 2H3a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h4l2-3h6l2 3h4a2 2 0 0 1 2 2zM12 17a4 4 0 1 0 0-8 4 4 0 0 0 0 8z',
    doc: 'M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8zM14 2v6h6M8 13h8M8 17h8',
    idcard: 'M3 5h18v14H3zM8.5 12a2 2 0 1 0 0-4 2 2 0 0 0 0 4zM5.5 16a3 3 0 0 1 6 0M14 9h4M14 13h4',
    copy: 'M9 9h13v13H9zM5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1',
    link: 'M10 13a5 5 0 0 0 7.5.5l3-3a5 5 0 0 0-7-7l-1.8 1.7M14 11a5 5 0 0 0-7.5-.5l-3 3a5 5 0 0 0 7 7l1.7-1.7',
    plus: 'M12 5v14M5 12h14',
    trash: 'M3 6h18M8 6V4h8v2M19 6l-1 14H6L5 6',
    zap: 'M13 2 3 14h9l-1 8 10-12h-9z',
    flame: 'M8.5 14.5A2.5 2.5 0 0 0 11 12c0-1.4-.5-2-1-3-1.1-2.1-.2-4 2-6 .5 2.5 2 4.9 4 6.5 2 1.6 3 3.5 3 5.5a7 7 0 1 1-14 0c0-1.2.4-2.3 1-3a2.5 2.5 0 0 0 2.5 2.5z',
    target: 'M12 2v3M12 19v3M2 12h3M19 12h3M12 18a6 6 0 1 0 0-12 6 6 0 0 0 0 12zM12 14a2 2 0 1 0 0-4 2 2 0 0 0 0 4z',
    eye: 'M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8zM12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6z',
    refresh: 'M3 12a9 9 0 0 1 15-6.7L21 8M21 3v5h-5M21 12a9 9 0 0 1-15 6.7L3 16M3 21v-5h5',
    monitor: 'M2 3h20v13H2zM12 16v5M8 21h8',
    route: 'M6 19a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM18 11a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM6 13V9a4 4 0 0 1 4-4h5M18 11v4a4 4 0 0 1-4 4H9',
    car: 'M5 17h14v-5l-2-5H7l-2 5zM5 17v2M19 17v2M7.5 14h.01M16.5 14h.01M5 12h14',
    settings: 'M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z',
    chart: 'M3 3v18h18M7 15l4-4 3 3 6-6',
    report: 'M4 15s1-1 4-1 5 2 8 2 4-1 4-1V3s-1 1-4 1-5-2-8-2-4 1-4 1zM4 22v-7M12 7v3M12 12h.01',
    gauge: 'M12 14l4-4M3.3 19a10 10 0 1 1 17.4 0',
  };
  function ic(name, cls, filled) {
    const fill = filled ? 'currentColor' : 'none';
    return '<svg class="ic ' + (cls || '') + '" viewBox="0 0 24 24" fill="' + fill + '" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="' + P[name] + '"/></svg>';
  }
  function stars(n, max) {
    let h = '<span class="stars" aria-label="' + n + ' stars">';
    for (let i = 1; i <= (max || 5); i++) h += ic('star', i <= Math.round(n) ? 'on' : 'off', true);
    return h + '</span>';
  }

  // ---------------------------------------------------------------------------
  // Generated portraits & vehicle illustrations (offline-safe)
  // ---------------------------------------------------------------------------
  const avatarCache = new Map();
  function avatarURI(av) {
    const key = JSON.stringify(av);
    if (avatarCache.has(key)) return avatarCache.get(key);
    const { skin, hair, bg, style } = av;
    const back = style === 'long' ? '<path d="M15 35c0-15 7-23 17-23s17 8 17 23c0 9-2 15-4 19H19c-2-4-4-10-4-19z" fill="' + hair + '"/>'
      : style === 'bob' ? '<path d="M17 33c0-13 6-20 15-20s15 7 15 20c0 5-1 9-2 11H19c-1-2-2-6-2-11z" fill="' + hair + '"/>'
        : '<circle cx="32" cy="12" r="7" fill="' + hair + '"/>';
    const svg = '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><rect width="64" height="64" fill="' + bg + '"/>' + back +
      '<path d="M12 64c0-11 9-17 20-17s20 6 20 17z" fill="#6b2c91" opacity=".85"/>' +
      '<rect x="28" y="38" width="8" height="10" rx="3" fill="' + skin + '"/>' +
      '<ellipse cx="32" cy="30" rx="10" ry="12" fill="' + skin + '"/>' +
      '<path d="M21.5 28c1-9 5.5-14 10.5-14s9.5 5 10.5 14c-4-1-8-4-10-8-2 4-6.5 7-11 8z" fill="' + hair + '"/>' +
      '<circle cx="28" cy="31" r="1.2" fill="#2b2024"/><circle cx="36" cy="31" r="1.2" fill="#2b2024"/>' +
      '<path d="M29 36.2q3 2.2 6 0" stroke="#7a3b3b" stroke-width="1.3" fill="none" stroke-linecap="round"/></svg>';
    const uri = 'data:image/svg+xml;charset=utf-8,' + encodeURIComponent(svg);
    avatarCache.set(key, uri);
    return uri;
  }
  function photoOf(person) {
    if (!person) return avatarURI({ skin: '#e0ac69', hair: '#2b1d16', bg: '#efe3fb', style: 'long' });
    if (person.photo) return person.photo;
    if (person.avatar) return avatarURI(person.avatar);
    return avatarURI({ skin: '#c68642', hair: '#1b1410', bg: '#fde7ef', style: 'bun' });
  }
  function avatar(person, size, extra) {
    return '<img class="avatar ' + (extra || '') + '" style="width:' + size + 'px;height:' + size + 'px" src="' + photoOf(person) + '" alt="">';
  }
  function carSVG(hex, wav) {
    return '<svg class="car-illus" viewBox="0 0 120 50" aria-hidden="true">' +
      '<ellipse cx="60" cy="45" rx="52" ry="4" fill="rgba(0,0,0,.12)"/>' +
      '<path d="M8 36c0-6 3-9 9-10l14-3 13-10c3-2 6-3 10-3h22c4 0 7 1 10 4l11 10 10 2c5 1 7 4 7 9v3c0 2-1 3-3 3H11c-2 0-3-1-3-3z" fill="' + hex + '" stroke="rgba(0,0,0,.25)" stroke-width="1"/>' +
      '<path d="M37 23l10-8c2-1.5 4-2 7-2h9v10zM67 13h9c3 0 5 1 7 3l8 7H67z" fill="#bcd3e6" stroke="rgba(0,0,0,.2)" stroke-width=".8"/>' +
      (wav ? '<g transform="translate(52 26) scale(.5)" stroke="#1d4ed8" fill="none" stroke-width="3"><circle cx="10" cy="4" r="3"/><path d="M10 8v8h8l4 8M6 13a8 8 0 1 0 10 10"/></g>' : '') +
      '<circle cx="30" cy="40" r="7" fill="#222"/><circle cx="30" cy="40" r="3" fill="#999"/><circle cx="92" cy="40" r="7" fill="#222"/><circle cx="92" cy="40" r="3" fill="#999"/></svg>';
  }

  // Resize an uploaded image to a small data URL (keeps state broadcasts light).
  function readImage(file, size) {
    return new Promise((resolve) => {
      if (!file) return resolve(null);
      const fr = new FileReader();
      fr.onload = () => {
        const img = new Image();
        img.onload = () => {
          const c = document.createElement('canvas');
          const s = size || 128;
          c.width = s; c.height = s;
          const ctx = c.getContext('2d');
          const m = Math.min(img.width, img.height);
          ctx.drawImage(img, (img.width - m) / 2, (img.height - m) / 2, m, m, 0, 0, s, s);
          resolve(c.toDataURL('image/jpeg', 0.8));
        };
        img.onerror = () => resolve(null);
        img.src = fr.result;
      };
      fr.readAsDataURL(file);
    });
  }

  // ---------------------------------------------------------------------------
  // View patching
  // ---------------------------------------------------------------------------
  function patch(el, key, htmlFn) {
    if (el._key === key) return false;
    el._key = key;
    el.innerHTML = htmlFn();
    return true;
  }
  function bind(root, vals) {
    if (!vals) return;
    for (const n of root.querySelectorAll('[data-b]')) {
      const v = vals[n.dataset.b];
      if (v !== undefined && n.textContent !== String(v)) n.textContent = v;
    }
    for (const n of root.querySelectorAll('[data-bw]')) {
      const v = vals[n.dataset.bw];
      if (v !== undefined) n.style.width = v;
    }
    for (const n of root.querySelectorAll('[data-bshow]')) {
      const v = vals[n.dataset.bshow];
      if (v !== undefined) n.hidden = !v;
    }
    for (const n of root.querySelectorAll('[data-bdis]')) {
      const v = vals[n.dataset.bdis];
      if (v !== undefined) n.disabled = !!v;
    }
    for (const n of root.querySelectorAll('[data-btone]')) {
      const v = vals[n.dataset.btone];
      if (v !== undefined) n.dataset.tone = v;
    }
  }

  function toast(phone, text, kind) {
    const host = phone.querySelector('.toasts');
    const el = document.createElement('div');
    el.className = 'toast';
    el.dataset.kind = kind || 'info';
    const icon = { ok: 'check', err: 'alert', warn: 'alert', msg: 'message', info: 'info' }[kind] || 'info';
    el.innerHTML = ic(icon) + '<span>' + esc(text) + '</span>';
    host.appendChild(el);
    setTimeout(() => el.classList.add('out'), 3600);
    setTimeout(() => el.remove(), 4000);
    while (host.children.length > 3) host.firstChild.remove();
  }

  const STATE_META = {
    REQUESTED: { tone: 'plum', rider: 'Request sent', driver: 'Request sent' },
    SEARCHING: { tone: 'plum', rider: 'Finding your driver', driver: 'Dispatching' },
    DRIVER_ASSIGNED: { tone: 'blue', rider: 'Driver assigned', driver: 'Trip accepted' },
    DRIVER_EN_ROUTE: { tone: 'blue', rider: 'Driver on the way', driver: 'Heading to pickup' },
    DRIVER_ARRIVED: { tone: 'amber', rider: 'Your driver has arrived', driver: 'At pickup — verify PIN' },
    PIN_VERIFIED: { tone: 'teal', rider: 'PIN verified', driver: 'PIN verified' },
    IN_TRIP: { tone: 'green', rider: 'Trip in progress', driver: 'Trip in progress' },
    COMPLETED: { tone: 'ink', rider: 'Trip completed', driver: 'Trip completed' },
    RATED: { tone: 'ink', rider: 'Thanks for riding', driver: 'Trip closed' },
    CANCELLED: { tone: 'gray', rider: 'Ride cancelled', driver: 'Trip cancelled' },
    NO_DRIVERS: { tone: 'red', rider: 'No drivers available', driver: 'No drivers' },
  };
  function statusBanner(state, who) {
    const m = STATE_META[state];
    return '<div class="status-banner" data-tone="' + m.tone + '"><span class="pulse-dot"></span><span>' + m[who] + '</span></div>';
  }
  // The women-only identity checks, ticking off one by one while they run.
  function checksShown(v, now) {
    if (v.status !== 'checking') return v.checks.length;
    return Math.min(v.checks.length, Math.floor((now - v.checkAt) / E.CHECK_STEP_MS));
  }
  function idChecks(v, now) {
    const shown = checksShown(v, now);
    return '<ul class="id-checks">' + v.checks.map((c, i) => {
      if (i >= shown) return '<li class="wait"><span class="spinner xs"></span><span><b>' + esc(c.label) + '</b></span></li>';
      const tone = c.ok === true ? 'ok' : c.ok === null ? 'ask' : 'bad';
      return '<li class="' + tone + '">' + ic(tone === 'ok' ? 'check' : tone === 'ask' ? 'eye' : 'x') + '<span><b>' + esc(c.label) + '</b><span class="tiny">' + esc(c.detail) + '</span></span></li>';
    }).join('') + '</ul>';
  }

  // Shown in the SOS modal after "Call emergency services". Placeholder: nothing is dialled yet.
  function emsConfirm() {
    return '<div class="ems-confirm" role="status">' + ic('shieldCheck') + '<div><b>Emergency services have been notified</b>' +
      '<div class="tiny">They have your live location. Keep your phone with you — help is on the way.</div></div></div>';
  }

  // ---------------------------------------------------------------------------
  // Map — SVG overlays on OpenStreetMap tiles of Windhoek, with smooth camera + marker interpolation.
  // ---------------------------------------------------------------------------
  let baseCache = null;
  // Offline base map: Windhoek's roads drawn from the built-in OpenStreetMap graph. When
  // online, OSM map tiles cover it; it shows through wherever tiles are missing.
  function baseMap() {
    if (baseCache) return baseCache;
    const R = E.ROADS, B = E.BOUNDS;
    const d = ['', '', ''];
    for (const e of R.edges) {
      const c = e[3] & 7, k = c <= 1 ? 0 : c <= 3 ? 1 : 2;
      let s = 'M' + R.nodes[2 * e[0]] + ' ' + R.nodes[2 * e[0] + 1];
      for (let i = 4; i < e.length; i += 2) s += 'L' + e[i] + ' ' + e[i + 1];
      d[k] += s + 'L' + R.nodes[2 * e[1]] + ' ' + R.nodes[2 * e[1] + 1];
    }
    baseCache = '<rect x="-20000" y="-20000" width="' + (B.w + 40000) + '" height="' + (B.h + 40000) + '" fill="var(--map-outside)"/>' +
      '<rect width="' + B.w + '" height="' + B.h + '" fill="var(--map-block)"/>' +
      '<path class="m-road m-road-local" d="' + d[2] + '"/><path class="m-road m-road-mid" d="' + d[1] + '"/><path class="m-road m-road-major" d="' + d[0] + '"/>' +
      '<g class="m-zone-labels">' + E.SUBURBS.map((z) => '<text x="' + z.x + '" y="' + z.y + '">' + esc(z.name.toUpperCase()) + '</text>').join('') + '</g>';
    return baseCache;
  }

  function poiLayer() {
    return E.PLACES.map((p) => '<circle cx="' + p.x + '" cy="' + p.y + '" r="45"/><text x="' + (p.x + 90) + '" y="' + (p.y - 70) + '">' + esc(p.name) + '</text>').join('');
  }

  // OpenStreetMap raster tiles, placed in the engine's local metres. The engine's projection is
  // Web Mercator scaled by cos(latitude), so every tile is an exact square in those units.
  const TILE_URL = 'https://tile.openstreetmap.org/{z}/{x}/{y}.png';
  const EARTH = 2 * Math.PI * 6378137;
  const SVGNS = 'http://www.w3.org/2000/svg';
  function tileRect(z, tx, ty) {
    const R = E.ROADS, size = EARTH / 2 ** z;
    return { x: (-EARTH / 2 + tx * size - R.mx0) * R.k, y: (R.my0 - (EARTH / 2 - ty * size)) * R.k, s: size * R.k };
  }

  const CAR_PATH = 'M-13 -5.5 Q-13 -7 -11 -7 L9 -7 Q13 -7 13.5 -3 L13.5 3 Q13 7 9 7 L-11 7 Q-13 7 -13 5.5 Z';
  const maps = new Set();
  let uid = 0;

  class MapView {
    constructor(host, opts) {
      this.host = host;
      this.opts = opts || {};
      this.id = ++uid;
      this.vb = { x: 0, y: 0, w: E.BOUNDS.w, h: E.BOUNDS.h };
      this.target = null;
      this.cars = new Map();
      this.spec = {};
      this.pad = this.opts.pad || { t: 110, r: 30, b: 360, l: 30 };
      host.innerHTML =
        '<svg class="map-svg" xmlns="http://www.w3.org/2000/svg" preserveAspectRatio="xMidYMid slice">' +
        '<defs><radialGradient id="heat' + this.id + '"><stop offset="0" stop-color="#ff3d71" stop-opacity=".5"/><stop offset=".55" stop-color="#ff7a59" stop-opacity=".2"/><stop offset="1" stop-color="#ffb35c" stop-opacity="0"/></radialGradient></defs>' +
        '<g class="m-base">' + baseMap() + '</g><g class="m-tiles"></g><g class="m-poi">' + poiLayer() + '</g>' +
        '<g class="m-heat"></g><g class="m-ring"></g><g class="m-approx"></g>' +
        '<polyline class="m-planned" fill="none"/><polyline class="m-route2" fill="none"/><polyline class="m-route" fill="none"/>' +
        '<g class="m-cars"></g><g class="m-pins"></g></svg>';
      this.svg = host.firstChild;
      this.g = {};
      for (const k of ['heat', 'ring', 'approx', 'cars', 'pins']) this.g[k] = this.svg.querySelector('.m-' + k);
      this.lines = { route: this.svg.querySelector('.m-route'), route2: this.svg.querySelector('.m-route2'), planned: this.svg.querySelector('.m-planned') };
      this.labels = [this.svg.querySelector('.m-base'), this.svg.querySelector('.m-poi')];
      this.tileLayer = this.svg.querySelector('.m-tiles');
      this.tiles = new Map();
      this.tileSig = '';
      this.tileAt = 0;
      host.insertAdjacentHTML('beforeend', '<a class="map-attrib" href="https://www.openstreetmap.org/copyright" target="_blank" rel="noopener">© OpenStreetMap contributors</a>');
      this.svg.addEventListener('click', (e) => {
        if (!this.spec.onTap) return;
        const r = this.svg.getBoundingClientRect();
        const k = this.vb.w / r.width;
        this.spec.onTap({ x: this.vb.x + (e.clientX - r.left) * k, y: this.vb.y + (e.clientY - r.top) * k });
      });
      maps.add(this);
    }

    update(spec) {
      spec = spec || {};
      this.spec = spec;
      this.host.classList.toggle('tap-mode', !!spec.onTap);
      this.fitPts = spec.fit || null;
      this.pad = spec.pad || this.opts.pad || { t: 110, r: 30, b: 360, l: 30 };
      const ptsAttr = (pts) => (pts && pts.length ? pts.map((p) => p.x.toFixed(0) + ',' + p.y.toFixed(0)).join(' ') : '');
      for (const k of ['route', 'route2', 'planned']) {
        const v = ptsAttr(spec[k]);
        if (this.lines[k].getAttribute('points') !== v) this.lines[k].setAttribute('points', v);
      }
      // Ring (dispatch radius)
      const ringKey = spec.ring ? [spec.ring.x, spec.ring.y, spec.ring.r, spec.ring.pulse].join() : '';
      if (this.g.ring._k !== ringKey) {
        this.g.ring._k = ringKey;
        const r = spec.ring;
        this.g.ring.innerHTML = r ? '<circle class="ring-fill" cx="' + r.x + '" cy="' + r.y + '" r="' + r.r + '"/>' +
          (r.pulse ? '<circle class="ring-pulse" cx="' + r.x + '" cy="' + r.y + '" r="0"><animate attributeName="r" from="0" to="' + r.r + '" dur="2.2s" repeatCount="indefinite"/><animate attributeName="opacity" from=".7" to="0" dur="2.2s" repeatCount="indefinite"/></circle>' : '') +
          '<text class="ring-label" x="' + r.x + '" y="' + (r.y - r.r - 60) + '">3 km dispatch radius</text>' : '';
      }
      const apxKey = spec.approx ? [Math.round(spec.approx.x), Math.round(spec.approx.y), spec.approx.r].join() : '';
      if (this.g.approx._k !== apxKey) {
        this.g.approx._k = apxKey;
        const a = spec.approx;
        this.g.approx.innerHTML = a ? '<circle class="approx" cx="' + a.x + '" cy="' + a.y + '" r="' + a.r + '"/>' : '';
      }
      const heatKey = (spec.heat || []).map((h) => h.x + ':' + h.y + ':' + h.r).join('|');
      if (this.g.heat._k !== heatKey) {
        this.g.heat._k = heatKey;
        this.g.heat.innerHTML = (spec.heat || []).map((h) => '<circle cx="' + h.x + '" cy="' + h.y + '" r="' + h.r + '" fill="url(#heat' + this.id + ')" opacity="' + (h.o || 1) + '"/>').join('');
      }
      // Cars
      const seen = new Set();
      for (const c of spec.cars || []) {
        seen.add(c.id);
        let m = this.cars.get(c.id);
        if (!m) {
          const el = document.createElementNS('http://www.w3.org/2000/svg', 'g');
          el.innerHTML = '<circle class="car-halo" r="16"/><g class="car-rot"><path class="car-body" d="' + CAR_PATH + '"/><path class="car-glass" d="M3 -5.5 L8 -5 Q10 0 8 5 L3 5.5 Q4.5 0 3 -5.5Z"/></g>';
          this.g.cars.appendChild(el);
          m = { el, rot: el.querySelector('.car-rot'), cur: { x: c.x, y: c.y, h: c.h || 0 } };
          this.cars.set(c.id, m);
        }
        m.target = { x: c.x, y: c.y, h: c.h || 0 };
        if (m.kind !== c.kind) { m.kind = c.kind; m.el.setAttribute('class', 'car car-' + c.kind); }
      }
      for (const [id, m] of this.cars) if (!seen.has(id)) { m.el.remove(); this.cars.delete(id); }
      // Pins
      const pinKey = JSON.stringify([spec.pickup, spec.dest]);
      if (this.g.pins._k !== pinKey) {
        this.g.pins._k = pinKey;
        let h = '';
        if (spec.pickup) h += '<g class="pin pin-pickup" data-x="' + spec.pickup.x + '" data-y="' + spec.pickup.y + '"><circle class="pin-pulse" r="18"/><circle class="pin-dot" r="8"/>' +
          (spec.pickup.label ? '<g class="pin-label" transform="translate(0 -30)"><rect x="-' + (spec.pickup.label.length * 3.4 + 10) + '" y="-11" width="' + (spec.pickup.label.length * 6.8 + 20) + '" height="22" rx="11"/><text y="4">' + esc(spec.pickup.label) + '</text></g>' : '') + '</g>';
        if (spec.dest) h += '<g class="pin pin-dest" data-x="' + spec.dest.x + '" data-y="' + spec.dest.y + '"><rect x="-8" y="-8" width="16" height="16" rx="3"/><rect class="pin-dest-in" x="-3" y="-3" width="6" height="6"/>' +
          (spec.dest.label ? '<g class="pin-label" transform="translate(0 -28)"><rect x="-' + (spec.dest.label.length * 3.4 + 10) + '" y="-11" width="' + (spec.dest.label.length * 6.8 + 20) + '" height="22" rx="11"/><text y="4">' + esc(spec.dest.label) + '</text></g>' : '') + '</g>';
        this.g.pins.innerHTML = h;
      }
    }

    /** Keeps the OSM tiles covering the current view at a zoom level that suits it. */
    updateTiles(W) {
      const vb = this.vb, now = performance.now();
      const sig = [vb.x, vb.y, vb.w].map((v) => Math.round(v / 20)).join();
      if (sig === this.tileSig || now - this.tileAt < 150 || (typeof navigator !== 'undefined' && navigator.onLine === false)) return;
      this.tileSig = sig;
      this.tileAt = now;
      const R = E.ROADS, dpr = Math.min(2, window.devicePixelRatio || 1);
      const z = Math.max(11, Math.min(18, Math.round(Math.log2(EARTH * R.k * (W / vb.w) * dpr / 256))));
      if (z !== this.tileZ) { this.tileZ = z; this.zAt = now; }
      const size = EARTH / 2 ** z;
      const tx0 = Math.floor((vb.x / R.k + R.mx0 + EARTH / 2) / size), tx1 = Math.floor(((vb.x + vb.w) / R.k + R.mx0 + EARTH / 2) / size);
      const ty0 = Math.floor((EARTH / 2 - (R.my0 - vb.y / R.k)) / size), ty1 = Math.floor((EARTH / 2 - (R.my0 - (vb.y + vb.h) / R.k)) / size);
      const want = new Set();
      if ((tx1 - tx0 + 1) * (ty1 - ty0 + 1) <= 80) {
        for (let tx = tx0; tx <= tx1; tx++) for (let ty = ty0; ty <= ty1; ty++) want.add(z + '/' + tx + '/' + ty);
      }
      for (const key of want) {
        const old = this.tiles.get(key);
        if (old && !(old.failedAt && now - old.failedAt > 15000)) continue;
        const [tz, tx, ty] = key.split('/').map(Number);
        const r = tileRect(tz, tx, ty);
        const el = document.createElementNS(SVGNS, 'image');
        el.setAttribute('x', r.x.toFixed(1)); el.setAttribute('y', r.y.toFixed(1));
        el.setAttribute('width', (r.s + 0.6 * r.s / 256).toFixed(1)); el.setAttribute('height', (r.s + 0.6 * r.s / 256).toFixed(1));
        el.setAttribute('preserveAspectRatio', 'none');
        const t = { el, z: tz, failedAt: 0 };
        // Count loads and failures on the map element: handy when checking the map on a patchy connection.
        el.addEventListener('load', () => { el.classList.add('ok'); this.svg.classList.add('tiles-on'); this.host.dataset.tilesOk = (+this.host.dataset.tilesOk || 0) + 1; });
        el.addEventListener('error', () => { el.remove(); t.failedAt = performance.now(); this.tileSig = ''; this.host.dataset.tilesFailed = (+this.host.dataset.tilesFailed || 0) + 1; });
        el.setAttribute('href', TILE_URL.replace('{z}', tz).replace('{x}', tx).replace('{y}', ty));
        this.tileLayer.appendChild(el);
        this.tiles.set(key, t);
      }
      // Drop tiles from other zoom levels once the new ones have had time to load, and old ones out of view.
      for (const [key, t] of this.tiles) {
        if (want.has(key)) continue;
        if (t.z !== z ? now - this.zAt > 1500 : this.tiles.size > 120) { t.el.remove(); this.tiles.delete(key); }
      }
    }

    computeTarget(W, H) {
      const pts = this.fitPts;
      const pad = this.pad;
      let minX, minY, maxX, maxY;
      if (!pts || !pts.length) { minX = 0; minY = 0; maxX = E.BOUNDS.w; maxY = E.BOUNDS.h; }
      else {
        minX = Math.min(...pts.map((p) => p.x)); maxX = Math.max(...pts.map((p) => p.x));
        minY = Math.min(...pts.map((p) => p.y)); maxY = Math.max(...pts.map((p) => p.y));
      }
      const iw = Math.max(80, W - pad.l - pad.r), ih = Math.max(80, H - pad.t - pad.b);
      let k = Math.max((maxX - minX) / iw, (maxY - minY) / ih, (this.spec.minSpan || 1500) / iw);
      const cx = (minX + maxX) / 2, cy = (minY + maxY) / 2;
      return { x: cx - (pad.l + iw / 2) * k, y: cy - (pad.t + ih / 2) * k, w: W * k, h: H * k };
    }

    frame(dt) {
      const W = this.svg.clientWidth, H = this.svg.clientHeight;
      if (!W || !H) return;
      const t = this.computeTarget(W, H);
      const a = this.snapNext ? 1 : 1 - Math.exp(-dt / 260);
      this.snapNext = false;
      const vb = this.vb;
      vb.x += (t.x - vb.x) * a; vb.y += (t.y - vb.y) * a; vb.w += (t.w - vb.w) * a; vb.h += (t.h - vb.h) * a;
      // Only touch the DOM when something visibly moved, so an idle map costs nothing to render.
      setIfChanged(this.svg, 'viewBox', vb.x.toFixed(0) + ' ' + vb.y.toFixed(0) + ' ' + vb.w.toFixed(0) + ' ' + vb.h.toFixed(0));
      const k = vb.w / W;
      if (Math.abs((this.lastK || 0) - k) > k * 0.002) {
        this.lastK = k;
        this.svg.style.setProperty('--k', k);
        for (const l of this.labels) l.style.fontSize = (10.5 * k).toFixed(1) + 'px';
      }
      this.updateTiles(W);
      const b = 1 - Math.exp(-dt / 160);
      for (const m of this.cars.values()) {
        const c = m.cur, tg = m.target;
        if (Math.hypot(tg.x - c.x, tg.y - c.y) > 900) { c.x = tg.x; c.y = tg.y; }
        c.x += (tg.x - c.x) * b; c.y += (tg.y - c.y) * b;
        let dh = ((tg.h - c.h + 540) % 360) - 180;
        c.h += dh * b;
        setIfChanged(m.el, 'transform', 'translate(' + c.x.toFixed(0) + ' ' + c.y.toFixed(0) + ') scale(' + k.toFixed(2) + ')');
        setIfChanged(m.rot, 'transform', 'rotate(' + c.h.toFixed(0) + ')');
      }
      for (const p of this.g.pins.children) setIfChanged(p, 'transform', 'translate(' + p.dataset.x + ' ' + p.dataset.y + ') scale(' + k.toFixed(2) + ')');
    }
  }

  function setIfChanged(el, attr, value) {
    const key = '_' + attr;
    if (el[key] !== value) { el[key] = value; el.setAttribute(attr, value); }
  }

  let lastFrame = performance.now();
  function loop(now) {
    const dt = Math.min(100, now - lastFrame);
    lastFrame = now;
    for (const m of maps) {
      try { m.frame(dt); } catch (e) { console.error(e); }
    }
    requestAnimationFrame(loop);
  }
  requestAnimationFrame(loop);

  window.Hera = {
    E, Store, esc, money, mins, km, clock, arriveAt, dateShort, mmss, firstName, initials, range, greeting,
    ic, stars, avatar, photoOf, avatarURI, carSVG, readImage, patch, bind, toast, statusBanner, emsConfirm, STATE_META, MapView,
    idChecks, checksShown,
  };
})();
