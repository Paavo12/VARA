/* Stage bootstrap: builds both phones, the top bar and the presentation panel. */
(function () {
  'use strict';
  const { E, Store, ic } = Hera;
  const params = new URLSearchParams(location.search);
  const store = {
    get(k, d) { try { const v = localStorage.getItem('hera.' + k); return v == null ? d : v; } catch (e) { return d; } },
    put(k, v) { try { localStorage.setItem('hera.' + k, v); } catch (e) { /* storage unavailable */ } },
  };

  const phoneHtml = (portal, label, sub) =>
    '<div class="phone-wrap" data-portal="' + portal + '"><div class="phone-label"><b>' + label + '</b><span>' + sub + '</span></div>' +
    '<div class="phone-box"><div class="phone"><div class="screen">' +
    '<div class="statusbar"><span class="sb-time"></span><span class="island"></span><span class="sb-icons"><i class="sig"></i><i class="bat"></i></span></div>' +
    '<div class="map-host"></div><div class="content"></div><div class="overlay"></div><div class="toasts"></div><div class="home-ind"></div>' +
    '</div></div></div></div>';

  document.getElementById('app').innerHTML =
    '<header class="topbar">' +
    '<div class="tb-brand"><span class="logo">' + ic('shieldCheck') + '</span><div><b>VARA</b><span>Women-only ride-hailing · Windhoek · Simulated demo</span></div></div>' +
    '<div class="tb-group"><span class="tb-label">Show</span><div class="tb-seg" data-group="view"><button data-v="both">Both portals</button><button data-v="rider">Rider</button><button data-v="driver">Driver</button><button data-v="admin">Admin</button><button data-v="all">Phones + admin</button></div></div>' +
    '<div class="tb-group"><span class="tb-label">Demand</span><div class="tb-seg" data-group="demand"><button data-v="normal">Normal</button><button data-v="high">High</button><button data-v="peak">Peak</button></div><span class="tb-surge" id="tbSurge"></span></div>' +
    '<div class="tb-right"><span class="tb-mode" id="tbMode"></span>' +
    '<label class="tb-toggle"><input type="checkbox" id="tbPres"><span>Presentation mode</span></label>' +
    '<button class="tb-reset" id="tbReset">' + ic('refresh') + '<span>Reset demo</span></button></div></header>' +
    '<main class="stage">' + phoneHtml('rider', 'Rider app', 'Book · track · stay safe') + phoneHtml('driver', 'Driver app', 'Accept · navigate · verify PIN') +
    '<section class="admin-wrap" id="admin" aria-label="Admin dashboard"></section>' +
    '<aside class="debug" id="debug"></aside></main>';

  const riderEl = document.querySelector('[data-portal="rider"]');
  const driverEl = document.querySelector('[data-portal="driver"]');
  const rider = new RiderApp(riderEl);
  const driver = new DriverApp(driverEl);
  const debugEl = document.getElementById('debug');
  const debug = new DebugPanel(debugEl);
  const adminEl = document.getElementById('admin');
  const admin = new AdminApp(adminEl);

  // View / presentation state
  let view = params.get('view') || store.get('view', 'both');
  let pres = params.has('debug') ? params.get('debug') !== '0' : store.get('pres', '1') === '1';
  const solo = params.has('view') && view !== 'admin' && view !== 'all' && window.matchMedia('(max-width: 600px)').matches;
  if (solo) document.body.classList.add('solo');

  function applyLayout() {
    // The admin dashboard is a desktop screen: on its own, or next to both phones.
    riderEl.hidden = view === 'driver' || view === 'admin';
    driverEl.hidden = view === 'rider' || view === 'admin';
    adminEl.hidden = view !== 'admin' && view !== 'all';
    document.body.classList.toggle('admin-only', view === 'admin');
    debugEl.hidden = !pres || solo || view === 'admin';
    for (const b of document.querySelectorAll('[data-group="view"] button')) b.setAttribute('aria-pressed', String(b.dataset.v === view));
    document.getElementById('tbPres').checked = pres;
    fit();
    admin.render(Store.state, true);
  }

  function fit() {
    if (solo) return;
    const top = document.querySelector('.topbar').offsetHeight;
    document.documentElement.style.setProperty('--tbh', top + 'px');
    const avail = window.innerHeight - top - 64;
    const s = Math.max(0.5, Math.min(1, avail / 844));
    document.documentElement.style.setProperty('--ps', s.toFixed(3));
  }
  window.addEventListener('resize', fit);

  document.querySelector('.topbar').addEventListener('click', (e) => {
    const b = e.target.closest('button');
    if (!b) return;
    const group = b.parentElement.dataset.group;
    if (group === 'view') {
      view = b.dataset.v;
      store.put('view', view);
      applyLayout();
    } else if (group === 'demand') {
      Store.send('SET_DEMAND', { level: b.dataset.v });
    }
  });
  document.getElementById('tbPres').addEventListener('change', (e) => { pres = e.target.checked; store.put('pres', pres ? '1' : '0'); applyLayout(); debug.render(Store.state); });

  const reset = document.getElementById('tbReset');
  let armed = 0;
  reset.addEventListener('click', async () => {
    if (Date.now() - armed > 3000) {
      armed = Date.now();
      reset.classList.add('armed');
      reset.querySelector('span').textContent = 'Click again to reset';
      setTimeout(() => { reset.classList.remove('armed'); reset.querySelector('span').textContent = 'Reset demo'; }, 3000);
      return;
    }
    armed = 0;
    reset.classList.remove('armed');
    reset.querySelector('span').textContent = 'Reset demo';
    await Store.send('RESET');
  });

  function tickClock() {
    const t = new Date().toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' }).replace(/\s?[AP]M/i, '');
    for (const n of document.querySelectorAll('.sb-time')) n.textContent = t;
  }
  setInterval(tickClock, 10000);
  tickClock();

  Store.on((s) => {
    rider.render(s);
    driver.render(s);
    debug.render(s);
    admin.render(s);
    for (const b of document.querySelectorAll('[data-group="demand"] button')) b.setAttribute('aria-pressed', String(b.dataset.v === s.settings.demand));
    const sg = document.getElementById('tbSurge');
    sg.textContent = s.surge > 1 ? s.surge.toFixed(1) + '× surge' : '1.0× no surge';
    sg.classList.toggle('hot', s.surge > 1);
  });

  Store.init().then(() => {
    const m = document.getElementById('tbMode');
    m.innerHTML = Store.mode === 'server' ? '<i></i>Synced across devices' : '<i></i>Standalone (this tab)';
    m.dataset.mode = Store.mode;
    applyLayout();
    Store.emit();
  });
})();
