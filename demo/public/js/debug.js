/* Presentation panel: a simple live view of the ride for the audience, plus a few demo shortcuts. */
(function () {
  'use strict';
  const { E, Store, esc, mins, km, ic, STATE_META } = Hera;
  const byId = (s, id) => s.drivers.find((d) => d.id === id);
  // Presenter-only: priority points are never shown on the rider or driver phones.
  const priorityLabel = (p) => Math.round(p.points) + ' pts · ' + p.cancels + ' cancel' + (p.cancels === 1 ? '' : 's');
  const fmtT = (ms) => { const t = Math.floor(ms / 1000); return String(Math.floor(t / 60)).padStart(2, '0') + ':' + String(t % 60).padStart(2, '0'); };

  class DebugPanel {
    constructor(el) {
      this.el = el;
      el.innerHTML =
        '<div class="dbg-head"><div><div class="dbg-title">' + ic('monitor') + 'Presentation panel</div><div class="dbg-sub">What is happening right now</div></div><span class="state-chip" data-k="chip"></span></div>' +
        '<section><h4>Ride progress</h4><div data-k="sm"></div></section>' +
        '<section><h4>Ride details</h4><div data-k="metrics"></div></section>' +
        '<section><h4>Priority points <small class="dbg-hint">hidden from rider and driver</small></h4><div data-k="priority"></div></section>' +
        '<section><h4>Demo shortcuts</h4><div class="dbg-btns">' +
        this.btn('X_ARRIVE_NOW', 'Driver arrives now', 'pin') +
        this.btn('X_JUMP_DEST', 'Jump to destination', 'flag') +
        this.btn('X_DEVIATE', 'Route deviation', 'route', null, 'RideCheck') +
        this.btn('X_STOP', 'Unexpected stop', 'alert', null, 'RideCheck') +
        this.btn('X_APPROVE', 'Approve verifications', 'shieldCheck') +
        '</div></section>' +
        '<section><h4>Settings</h4><div class="dbg-settings">' +
        '<label>Demo speed <select data-set="timeScale"><option value="10">10× (slow)</option><option value="20">20×</option><option value="30">30×</option><option value="60">60× (fast)</option></select></label>' +
        '<label class="dbg-check"><input type="checkbox" data-set="autopilot"> Autopilot driver <small>the driver accepts, enters the PIN and completes trips by herself</small></label>' +
        '</div></section>' +
        '<section><h4>What just happened</h4><div class="dbg-log" data-k="log"></div></section>';
      this.k = {};
      for (const n of el.querySelectorAll('[data-k]')) this.k[n.dataset.k] = n;
      el.addEventListener('click', async (e) => {
        const b = e.target.closest('[data-x]');
        if (!b) return;
        const r = await Store.send(b.dataset.x, b.dataset.p ? JSON.parse(b.dataset.p) : undefined);
        if (r && !r.ok) this.flash(r.error);
      });
      el.addEventListener('change', (e) => {
        const t = e.target;
        if (!t.dataset.set) return;
        Store.send('SET_SETTING', { key: t.dataset.set, value: t.type === 'checkbox' ? t.checked : Number(t.value) });
      });
    }

    btn(type, label, icon, payload, tag) {
      return '<button class="dbg-btn" data-x="' + type + '"' + (payload ? " data-p='" + JSON.stringify(payload) + "'" : '') + '>' + ic(icon) + '<span>' + label + '</span>' + (tag ? '<em>' + tag + '</em>' : '') + '</button>';
    }

    flash(msg) {
      const f = document.createElement('div');
      f.className = 'dbg-flash';
      f.textContent = msg;
      this.el.appendChild(f);
      setTimeout(() => f.remove(), 2600);
    }

    set(key, html) {
      const n = this.k[key];
      if (n._h !== html) { n._h = html; n.innerHTML = html; }
    }

    render(s) {
      if (!s || this.el.hidden) return;
      const r = s.ride;
      for (const inp of this.el.querySelectorAll('[data-set]')) {
        if (document.activeElement === inp) continue;
        const v = s.settings[inp.dataset.set];
        if (inp.type === 'checkbox') inp.checked = !!v;
        else if (String(inp.value) !== String(v)) {
          if (![...inp.options].some((o) => o.value === String(v))) inp.insertAdjacentHTML('beforeend', '<option value="' + v + '">' + v + '×</option>');
          inp.value = String(v);
        }
      }

      // Ride progress
      const st = r ? r.state : 'IDLE';
      this.k.chip.textContent = r ? STATE_META[st].rider : 'No ride';
      this.k.chip.dataset.tone = r ? STATE_META[st].tone : 'gray';
      const visited = new Set(r ? r.timeline.map((x) => x.state) : []);
      let sm = '<div class="sm">';
      for (const x of E.STATES) {
        const cls = r && r.state === x ? 'cur' : visited.has(x) ? 'done' : '';
        sm += '<span class="sm-step ' + cls + '" data-tone="' + STATE_META[x].tone + '">' + x.replace(/_/g, ' ') + '</span>';
      }
      if (r && (r.state === 'CANCELLED' || r.state === 'NO_DRIVERS')) sm += '<span class="sm-step cur" data-tone="' + STATE_META[r.state].tone + '">' + r.state.replace('_', ' ') + '</span>';
      this.set('sm', sm + '</div>');

      // Ride details
      const d = r && r.driverId && byId(s, r.driverId);
      const m = (k, v, cls) => '<div class="mt' + (cls ? ' ' + cls : '') + '"><span>' + k + '</span><b>' + v + '</b></div>';
      let metrics = '<div class="metrics">';
      if (r) {
        const eta = r.eta || {};
        metrics += m('Ride type', esc(E.OPTIONS[r.option].name)) + m('Rider PIN', '<span class="mono">' + r.pin + '</span>') +
          m('Driver', d ? esc(d.short) : 'searching…') + m('Driver arrives in', eta.pickupMin != null ? mins(eta.pickupMin) : '—') +
          m('Trip length', mins(eta.tripMin) + (eta.tripM ? ' · ' + km(eta.tripM) : '')) +
          m('Passenger', (r.passenger ? esc(r.passenger.name) + ' (booked for)' : esc(s.rider.profile.name || 'Rider')) + ' · ' + (r.seats || 1) + ' in total') +
          m('Locked fare', 'N$' + r.estimate.total.toFixed(2));
      } else {
        const online = s.drivers.filter((x) => x.online && !x.busy).length;
        metrics += m('Ride', 'none yet') + m('Drivers online', online + ' of ' + s.drivers.length) +
          m('Demand', esc(E.DEMAND[s.settings.demand].label)) + m('Sim clock', fmtT(s.simT));
      }
      this.set('metrics', metrics + '</div>');

      // Priority points: everyone starts at 100; cancelling costs more each time, finished trips earn some back.
      let pr = '<div class="metrics">' + m('Rider · ' + esc(s.rider.profile.name || 'not signed in'), priorityLabel(s.rider.priority));
      for (const x of s.drivers) pr += m('Driver · ' + esc(x.short), priorityLabel(x.priority));
      this.set('priority', pr + '</div>');

      // Log
      this.set('log', s.log.slice(0, 12).map((l) => '<div class="lg" data-kind="' + l.kind + '"><time>' + fmtT(l.t) + '</time><span>' + esc(l.text) + '</span></div>').join(''));
    }
  }

  window.DebugPanel = DebugPanel;
})();
