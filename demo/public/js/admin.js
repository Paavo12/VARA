/*
 * Admin dashboard: the demo's version of the VARA ops console.
 *
 * Everything here is an action on the shared simulation, so a change made here shows up on the
 * rider and driver phones straight away: new prices on the next quote, a Map Expert's 15 % fee on
 * her next offer, an approved driver can go online, a paused driver stops getting requests.
 * Every change is written to the audit log, as the real console does.
 */
(function () {
  'use strict';
  const { E, Store, esc, money, mins, ic, avatar, STATE_META, MapView } = Hera;
  const byId = (s, id) => s.drivers.find((d) => d.id === id);
  const pct = (f) => +(f * 100).toFixed(1) + '%';
  const day = (t) => new Date(t).toLocaleDateString([], { day: 'numeric', month: 'short', year: 'numeric' });
  const time = (t) => new Date(t).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
  const isoDay = (t) => { const d = new Date(t); return d.getFullYear() + '-' + String(d.getMonth() + 1).padStart(2, '0') + '-' + String(d.getDate()).padStart(2, '0'); };
  const DOC_NAMES = { identity: 'ID and gender check', licence: "Driver's licence", insurance: 'Insurance', background: 'Criminal-record check' };
  const SECTIONS = [
    ['overview', 'gauge', 'Overview'], ['verify', 'idcard', 'Verifications'], ['drivers', 'car', 'Drivers'],
    ['safety', 'shieldCheck', 'Safety desk'], ['pricing', 'cash', 'Pricing'], ['trips', 'list', 'Trips'], ['audit', 'doc', 'Audit log'],
  ];
  const store = {
    get(k, d) { try { const v = sessionStorage.getItem('hera.admin.' + k); return v == null ? d : v; } catch (e) { return d; } },
    put(k, v) { try { sessionStorage.setItem('hera.admin.' + k, v); } catch (e) { /* storage unavailable */ } },
  };

  // What needs a person: shown as counts in the side bar.
  function queues(s) {
    const riderPending = s.rider.verification.status === 'pending' ? 1 : 0;
    const apps = applications(s);
    const docs = apps.reduce((n, a) => n + E.DRIVER_DOCS.filter((k) => a.dp.docs[k].status === 'review').length + (a.dp.step === 'review' ? 1 : 0), 0);
    const sos = ['rider', 'driver'].filter((w) => s.sos[w] && s.sos[w].status === 'sent' && !s.sos[w].ack).length;
    const reports = s.reports.filter((x) => x.status === 'open').length;
    return { verify: riderPending + docs, safety: s.incidents.length + sos + reports };
  }
  // Driver applications in progress: every driver phone that has started "Apply to drive".
  function applications(s) {
    return Object.entries(s.driverPortals).filter(([, dp]) => dp.profile && dp.step !== 'approved' && !dp.signedInId)
      .map(([key, dp]) => ({ key, dp }));
  }

  class AdminApp {
    constructor(el) {
      this.el = el;
      this.ui = { section: store.get('section', 'overview'), signedIn: store.get('in', '0') === '1', drafts: {} };
      el.innerHTML = '<div class="adm-shell"></div><div class="adm-flash" hidden></div>';
      this.shell = el.firstChild;
      this.flashEl = el.lastChild;
      el.addEventListener('click', (e) => this.onClick(e));
      el.addEventListener('input', (e) => { const t = e.target; if (t.dataset.draft) this.ui.drafts[t.dataset.draft] = t.value; });
    }

    flash(msg, kind) {
      const f = this.flashEl;
      f.textContent = msg;
      f.dataset.kind = kind || 'err';
      f.hidden = false;
      clearTimeout(this.flashT);
      this.flashT = setTimeout(() => { f.hidden = true; }, 2800);
    }

    async act(type, payload, ok) {
      const r = await Store.send(type, payload);
      if (r && r.ok === false) { this.flash(r.error); return false; }
      if (ok) this.flash(ok, 'ok');
      return true;
    }

    draft(key, fallback) { return this.ui.drafts[key] != null ? this.ui.drafts[key] : fallback; }
    val(name) { const n = this.el.querySelector('[data-draft="' + name + '"]'); return n ? n.value : ''; }

    async onClick(e) {
      const b = e.target.closest('[data-a]');
      if (!b || !this.el.contains(b)) return;
      const a = b.dataset.a, v = b.dataset.v, s = Store.state;
      switch (a) {
        case 'signIn': this.ui.signedIn = true; store.put('in', '1'); this.render(s, true); break;
        case 'signOut': this.ui.signedIn = false; store.put('in', '0'); this.render(s, true); break;
        case 'nav': this.ui.section = v; store.put('section', v); this.render(s, true); break;
        case 'approveRider': this.act('A_APPROVE_RIDER', null, 'Rider approved: she can book now'); break;
        case 'rejectRider': {
          const reason = this.val('rejectReason').trim() || 'The selfie does not match the ID photo.';
          if (await this.act('A_REJECT_RIDER', { reason }, 'Rider told her documents were not approved')) delete this.ui.drafts.rejectReason;
          break;
        }
        case 'approveDoc': { const [portal, doc] = v.split('|'); this.act('A_APPROVE_DOC', { portal, doc }); break; }
        case 'approveApp': this.act('A_APPROVE_APPLICATION', { portal: v }, 'Driver approved: she can go online'); break;
        case 'mapExpert': {
          const until = this.val('me:' + v);
          if (await this.act('A_MAP_EXPERT', { driverId: v, until }, 'Map Expert granted: 15 % fee from her next offer')) delete this.ui.drafts['me:' + v];
          break;
        }
        case 'mapExpertEnd': this.act('A_MAP_EXPERT_END', { driverId: v }, 'Map Expert ended'); break;
        case 'suspend': {
          const reason = this.val('sus:' + v).trim();
          if (await this.act('A_SUSPEND', { driverId: v, reason }, 'Driver paused: she gets no requests')) delete this.ui.drafts['sus:' + v];
          break;
        }
        case 'release': this.act('A_RELEASE', { driverId: v }, 'Driver released'); break;
        case 'closeIncident': {
          const outcome = this.val('inc:' + v).trim();
          if (await this.act('A_CLOSE_INCIDENT', { id: v, outcome }, 'Incident closed: the driver gets requests again')) delete this.ui.drafts['inc:' + v];
          break;
        }
        case 'ackSos': this.act('A_ACK_SOS', { who: v }, 'Acknowledged'); break;
        case 'closeReport': {
          const outcome = this.val('rpt:' + v).trim();
          if (await this.act('A_CLOSE_REPORT', { id: v, outcome }, 'Report closed')) delete this.ui.drafts['rpt:' + v];
          break;
        }
        case 'saveRates': {
          const g = (k) => this.val('rate:' + v + ':' + k);
          if (await this.act('A_SET_RATES', { option: v, base: g('base'), perKm: g('perKm'), perMin: g('perMin'), min: g('min'), feePct: g('fee') }, E.OPTIONS[v].name + ' rates saved: new quotes use them now')) {
            for (const k of ['base', 'perKm', 'perMin', 'min', 'fee']) delete this.ui.drafts['rate:' + v + ':' + k];
          }
          break;
        }
        case 'demand': this.act('A_SET_DEMAND', { level: v }); break;
        case 'launch': {
          const date = v === 'today' ? isoDay(Date.now()) : v === 'clear' ? null : this.val('launchDate');
          if (date === '') return this.flash('Pick a launch date');
          if (await this.act('A_SET_LAUNCH', { date }, date ? 'Launch date saved' : 'Launch date cleared')) delete this.ui.drafts.launchDate;
          break;
        }
        case 'kids': {
          if (await this.act('A_SET_KIDS', { amount: this.val('kids') }, 'Child seat surcharge saved')) delete this.ui.drafts.kids;
          break;
        }
        case 'demoShortcut': this.act(v); break;
      }
    }

    render(s, force) {
      if (!s || this.el.hidden) return;
      if (!this.ui.signedIn) {
        if (force || this.shell._k !== 'signin') { this.shell._k = 'signin'; this.shell.innerHTML = this.signIn(); this.map = null; }
        return;
      }
      const q = queues(s);
      const section = this.sections()[this.ui.section] ? this.ui.section : 'overview';
      const view = this.sections()[section].call(this, s);
      const frameKey = 'frame:' + section + ':' + q.verify + ':' + q.safety;
      if (force || this.shell._k !== frameKey) {
        this.shell._k = frameKey;
        this.shell.innerHTML = this.frame(section, q);
        this.body = this.shell.querySelector('.adm-main');
        this.body._k = null;
        this.map = null;
      }
      // Re-draw a section only when what it shows has changed, so typing in a field is never interrupted.
      if (force || this.body._k !== view.key) {
        this.body._k = view.key;
        const focus = document.activeElement && this.body.contains(document.activeElement) ? document.activeElement.dataset.draft : null;
        this.body.innerHTML = view.html();
        this.map = null;
        if (focus) { const n = this.body.querySelector('[data-draft="' + focus + '"]'); if (n) n.focus(); }
      }
      if (view.map) {
        const host = this.body.querySelector('.adm-map');
        if (host && !this.map) this.map = new MapView(host);
        if (this.map) this.map.update(view.map);
      }
      Hera.bind(this.body, view.vals);
    }

    signIn() {
      return '<div class="adm-signin"><div class="adm-card adm-signin-card">' +
        '<div class="adm-brand big">' + ic('shieldCheck') + '<div><b>VARA Ops</b><span>Staff console</span></div></div>' +
        '<p class="adm-muted">Staff sign in with their work account and a second factor. Roles decide what each person can see and change: support, safety, finance or admin.</p>' +
        '<label class="adm-field"><span>Work email</span><input class="adm-input" value="admin@vara.example" readonly></label>' +
        '<label class="adm-field"><span>Password</span><input class="adm-input" type="password" value="demo-password" readonly></label>' +
        '<button class="adm-btn primary block" data-a="signIn">' + ic('lock') + 'Sign in as demo admin</button>' +
        '<p class="adm-tiny adm-muted">Demo account: no real sign-in. In the real console the session is an HttpOnly cookie with CSRF protection.</p></div></div>';
    }

    frame(section, q) {
      const badge = (n) => (n ? '<em class="adm-count">' + n + '</em>' : '');
      return '<aside class="adm-side"><div class="adm-brand">' + ic('shieldCheck') + '<div><b>VARA Ops</b><span>Admin · demo</span></div></div><nav>' +
        SECTIONS.map(([id, icon, label]) => '<button data-a="nav" data-v="' + id + '" title="' + label + '"' + (id === section ? ' aria-current="page"' : '') + '>' + ic(icon) + '<span>' + label + '</span>' +
          badge(id === 'verify' ? q.verify : id === 'safety' ? q.safety : 0) + '</button>').join('') +
        '</nav><button class="adm-signout" data-a="signOut" title="Sign out">' + ic('power') + 'Sign out</button></aside>' +
        '<main class="adm-main"></main>';
    }

    sections() {
      return { overview: this.overview, verify: this.verify, drivers: this.drivers, safety: this.safety, pricing: this.pricing, trips: this.trips, audit: this.auditLog };
    }

    head(title, sub) {
      return '<div class="adm-head"><div><h2>' + title + '</h2><p class="adm-muted">' + sub + '</p></div><span class="adm-live"><i></i>Live · changes reach the phones instantly</span></div>';
    }

    // ---------------------------------------------------------------- Overview
    overview(s) {
      const r = s.ride, trips = s.trips;
      const online = s.drivers.filter((d) => d.online).length;
      const gross = trips.reduce((t, x) => t + x.paid, 0), fees = trips.reduce((t, x) => t + x.fee, 0), earned = trips.reduce((t, x) => t + x.earned, 0);
      const promo = E.promoInfo(s);
      const d = r && r.driverId ? byId(s, r.driverId) : null;
      const cars = s.drivers.filter((x) => x.online).map((x) => ({ id: x.id, x: x.x, y: x.y, h: x.heading, kind: x.hold ? 'muted' : r && r.driverId === x.id ? 'me' : x.busy ? 'busy' : 'idle' }));
      const active = r && !['RATED', 'CANCELLED', 'NO_DRIVERS', 'BLOCKED'].includes(r.state);
      const kpi = (label, val, sub, bkey) => '<div class="adm-kpi"><span>' + label + '</span><b' + (bkey ? ' data-b="' + bkey + '"' : '') + '>' + val + '</b><small>' + sub + '</small></div>';
      return {
        key: 'ov:' + online + ':' + trips.length + ':' + (r ? r.id + r.state + (r.driverId || '') : '') + ':' + promo.state + ':' + s.pricing.version + ':' + s.settings.demand,
        map: { cars, pickup: active ? { x: r.pickup.x, y: r.pickup.y, label: 'Pickup' } : null, dest: active && ['PIN_VERIFIED', 'IN_TRIP'].includes(r.state) ? { x: r.dest.x, y: r.dest.y, label: r.dest.name } : null, fit: cars.length ? cars.concat(active ? [r.pickup] : []) : [E.HOME], minSpan: 4200, pad: { t: 30, r: 30, b: 30, l: 30 } },
        vals: { simClock: s.settings.timeScale + '× demo speed' },
        html: () => this.head('Overview', 'Windhoek right now: drivers, the live ride and today\'s numbers.') +
          '<div class="adm-kpis">' +
          kpi('Drivers online', online + ' / ' + s.drivers.length, s.drivers.filter((x) => x.hold).length + ' on hold') +
          kpi('Trips completed', trips.length, 'since the demo started') +
          kpi('Riders paid', money(gross), 'cash, incl. child seats') +
          kpi('VARA fees', money(fees), trips.length ? 'avg ' + pct(fees / Math.max(1, trips.reduce((t, x) => t + x.fare, 0))) + ' of fares' : 'no trips yet') +
          kpi('Drivers earned', money(earned), 'after fees') + '</div>' +
          '<div class="adm-grid2"><div class="adm-card adm-map-card"><div class="adm-card-head"><b>Live map</b><span class="adm-tiny adm-muted" data-b="simClock"></span></div><div class="adm-map"></div></div>' +
          '<div class="adm-stack">' +
          '<div class="adm-card"><div class="adm-card-head"><b>Live ride</b>' + (r ? '<span class="adm-pill" data-tone="' + STATE_META[r.state].tone + '">' + esc(r.state.replace(/_/g, ' ')) + '</span>' : '') + '</div>' +
          (r ? this.rideFacts(s, r, d) : '<p class="adm-muted">No ride right now. Book one on the rider phone.</p>') + '</div>' +
          '<div class="adm-card"><div class="adm-card-head"><b>Pricing now</b><button class="adm-link" data-a="nav" data-v="pricing">Change</button></div>' +
          this.facts([['Demand', esc(E.DEMAND[s.settings.demand].label) + ' · ' + s.surge.toFixed(1) + '×'], ['Launch promotion', this.promoLine(promo)], ['Child seat surcharge', money(s.pricing.kidsSurcharge)], ['Pricing version', 'v' + s.pricing.version]]) + '</div>' +
          '</div></div>',
      };
    }

    facts(rows) { return '<dl class="adm-facts">' + rows.map(([k, v]) => '<div><dt>' + k + '</dt><dd>' + v + '</dd></div>').join('') + '</dl>'; }

    rideFacts(s, r, d) {
      const kids = r.children ? r.children + (r.children > 1 ? ' children' : ' child') + (r.childSeat ? ' · child seat +' + money(r.kidsSurcharge) : '') : 'none';
      return this.facts([
        ['Ride', '<span class="adm-mono">' + esc(r.id) + '</span> · ' + esc(E.OPTIONS[r.option].name)],
        ['Rider', esc(r.passenger ? r.passenger.name + ' (booked by ' + s.rider.profile.name + ')' : s.rider.profile.name || 'Rider') + ' · ' + r.seats + ' passenger' + (r.seats > 1 ? 's' : '')],
        ['Children', kids],
        ['Driver', d ? esc(d.name) + ' · ' + esc(d.vehicle.plate) : 'matching…'],
        ['Locked price', money(r.estimate.total + (r.kidsSurcharge || 0))],
        ['Driver fee', r.feeRate != null ? pct(r.feeRate) + ' (' + E.FEE_WHY[r.feeWhy] + ')' : 'set when she accepts'],
        ['To', esc(r.dest.name)],
      ]);
    }

    promoLine(p) {
      if (p.state === 'unset') return 'no launch date set';
      if (p.state === 'scheduled') return 'starts ' + day(p.startsAt);
      if (p.state === 'active') return '<b class="adm-good">13 % fee · ' + p.daysLeft + ' days left</b>';
      return 'ended ' + day(p.endsAt);
    }

    // ----------------------------------------------------------- Verifications
    verify(s) {
      const v = s.rider.verification, apps = applications(s);
      const docSig = apps.map((a) => a.key + a.dp.step + E.DRIVER_DOCS.map((k) => a.dp.docs[k].status).join()).join('|');
      return {
        key: 'vf:' + v.status + ':' + s.rider.profile.name + ':' + docSig,
        html: () => this.head('Verifications', 'Every rider and driver is a verified woman. Nobody books or drives until a person here approves her.') +
          '<div class="adm-card"><div class="adm-card-head"><b>Riders waiting</b></div>' +
          (v.status === 'pending' ? '<div class="adm-review"><div class="adm-docs">' +
            '<div class="adm-doc">' + ic('idcard') + '<b>Government ID</b><span>Uploaded · encrypted</span></div>' +
            '<div class="adm-doc">' + ic('camera') + '<b>Live selfie</b><span>Uploaded · encrypted</span></div></div>' +
            '<div class="adm-grow">' + this.facts([['Name', esc(s.rider.profile.name)], ['Phone or email', esc(s.rider.profile.contact)], ['Checks', 'Face matches ID · ID shows female · ID not expired']]) +
            '<label class="adm-field"><span>Reason if you reject</span><input class="adm-input" data-draft="rejectReason" placeholder="e.g. The selfie does not match the ID photo." value="' + esc(this.draft('rejectReason', '')) + '"></label>' +
            '<div class="adm-row"><button class="adm-btn primary" data-a="approveRider">' + ic('check') + 'Approve</button><button class="adm-btn danger" data-a="rejectRider">' + ic('x') + 'Reject</button></div></div></div>'
            : '<p class="adm-muted">' + (v.status === 'verified' ? esc(s.rider.profile.name) + ' is verified. ' : '') + 'Nothing waiting. On the rider phone: <i>Create account → Submit for verification</i>.</p>') + '</div>' +
          '<div class="adm-card"><div class="adm-card-head"><b>Driver applications</b></div>' +
          (apps.length ? apps.map(({ key, dp }) => {
            const allOk = E.DRIVER_DOCS.every((k) => dp.docs[k].status === 'approved');
            return '<div class="adm-app"><div class="adm-app-head"><b>' + esc(dp.profile.name) + '</b><span class="adm-muted">' + esc(dp.profile.contact || '') + (dp.vehicle ? ' · ' + esc(dp.vehicle.color + ' ' + dp.vehicle.make + ' ' + dp.vehicle.model + ' · ' + dp.vehicle.plate) + ' · ' + esc(E.OPTIONS[dp.vehicle.type].name) : ' · vehicle not added yet') + '</span>' +
              '<span class="adm-pill" data-tone="' + (dp.step === 'review' ? 'amber' : 'gray') + '">' + (dp.step === 'review' ? 'Submitted' : 'In progress') + '</span></div>' +
              '<table class="adm-table"><tbody>' + E.DRIVER_DOCS.map((k) => {
                const st = dp.docs[k].status;
                return '<tr><td>' + DOC_NAMES[k] + '</td><td><span class="adm-pill" data-tone="' + (st === 'approved' ? 'green' : st === 'review' ? 'amber' : 'gray') + '">' + (st === 'approved' ? 'Approved' : st === 'review' ? 'In review' : 'Not uploaded') + '</span></td>' +
                  '<td class="adm-right">' + (st === 'review' ? '<button class="adm-btn sm" data-a="approveDoc" data-v="' + esc(key + '|' + k) + '">Approve</button>' : '') + '</td></tr>';
              }).join('') + '</tbody></table>' +
              (dp.step === 'review' ? '<button class="adm-btn primary" data-a="approveApp" data-v="' + esc(key) + '">' + ic('check') + (allOk ? 'Approve application' : 'Approve application and documents in review') + '</button>'
                : '<p class="adm-tiny adm-muted">She can be approved once she submits the application on her phone.</p>') + '</div>';
          }).join('') : '<p class="adm-muted">No applications. On a driver phone: <i>Apply to drive</i>.</p>') + '</div>',
      };
    }

    // ----------------------------------------------------------------- Drivers
    drivers(s) {
      const r = s.ride, now = Date.now();
      const status = (d) => (d.hold ? (d.hold.by === 'admin' ? ['Paused', 'red'] : ['Safety hold', 'red']) : r && r.driverId === d.id && ['DRIVER_ASSIGNED', 'DRIVER_EN_ROUTE', 'DRIVER_ARRIVED', 'PIN_VERIFIED', 'IN_TRIP'].includes(r.state) ? ['On a trip', 'blue'] : d.online ? ['Online', 'green'] : ['Offline', 'gray']);
      const sig = s.drivers.map((d) => d.id + status(d)[0] + (d.mapExpertUntil || '') + d.rating + d.stats.trips + Math.round(d.priority.points)).join('|') + ':' + E.promoInfo(s).state + s.pricing.version;
      const inYear = isoDay(now + 90 * E.DAY_MS);
      return {
        key: 'dr:' + sig,
        html: () => this.head('Drivers', 'Fees, Map Experts and pauses take effect on her phone at once. Priority points are only visible here.') +
          '<div class="adm-card"><table class="adm-table adm-drivers"><thead><tr><th>Driver</th><th>Car</th><th>Status</th><th>Rating</th><th>Trips today</th><th>Priority</th><th>Fee now</th><th>Map Expert</th><th>Pause</th></tr></thead><tbody>' +
          s.drivers.map((d) => {
            const st = status(d), fee = E.feeFor(s, d, d.vehicle.type), expert = d.mapExpertUntil && d.mapExpertUntil > now;
            return '<tr><td><div class="adm-who">' + avatar(d, 32) + '<div><b>' + esc(d.name) + '</b><span>' + esc(d.phone || '') + (d.isNew ? ' · new' : '') + '</span></div></div></td>' +
              '<td>' + esc(E.OPTIONS[d.vehicle.type].name) + '<span class="adm-sub">' + esc(d.vehicle.plate) + (d.vehicle.wav ? ' · wheelchair' : '') + '</span></td>' +
              '<td><span class="adm-pill" data-tone="' + st[1] + '">' + st[0] + '</span>' + (d.hold && d.hold.reason ? '<span class="adm-sub">' + esc(d.hold.reason) + '</span>' : '') + '</td>' +
              '<td>★ ' + d.rating.toFixed(2) + '</td><td>' + d.stats.trips + '</td><td>' + Math.round(d.priority.points) + ' pts</td>' +
              '<td><b>' + pct(fee.rate) + '</b><span class="adm-sub">' + E.FEE_WHY[fee.why] + '</span></td>' +
              '<td>' + (expert ? '<span class="adm-pill" data-tone="plum">until ' + day(d.mapExpertUntil) + '</span> <button class="adm-link" data-a="mapExpertEnd" data-v="' + d.id + '">End</button>'
                : '<div class="adm-inline"><input class="adm-input sm" type="date" data-draft="me:' + d.id + '" value="' + esc(this.draft('me:' + d.id, inYear)) + '"><button class="adm-btn sm" data-a="mapExpert" data-v="' + d.id + '">Grant</button></div>') + '</td>' +
              '<td>' + (d.hold ? (d.hold.by === 'admin' ? '<button class="adm-btn sm" data-a="release" data-v="' + d.id + '">Release</button>' : '<button class="adm-link" data-a="nav" data-v="safety">See incident</button>')
                : '<div class="adm-inline"><input class="adm-input sm" data-draft="sus:' + d.id + '" placeholder="Reason" value="' + esc(this.draft('sus:' + d.id, '')) + '"><button class="adm-btn sm danger" data-a="suspend" data-v="' + d.id + '">Pause</button></div>') + '</td></tr>';
          }).join('') + '</tbody></table></div>' +
          '<p class="adm-tiny adm-muted">Map Experts know the city well and pay a flat 15 % on every ride type, for up to ' + E.CFG.MAP_EXPERT_MAX_DAYS + ' days. When the launch promotion is on, everyone pays 13 %: the lowest fee wins.</p>',
      };
    }

    // ------------------------------------------------------------- Safety desk
    safety(s) {
      const sos = ['rider', 'driver'].map((w) => [w, s.sos[w]]).filter(([, a]) => a && a.status === 'sent');
      const sig = s.incidents.map((x) => x.id).join() + ':' + sos.map(([w, a]) => w + a.at + !!a.ack).join() + ':' + s.reports.map((x) => x.id + x.status).join();
      const outcome = (k) => '<input class="adm-input" data-draft="' + k + '" placeholder="Written outcome (required, audit-logged)" value="' + esc(this.draft(k, '')) + '">';
      return {
        key: 'sf:' + sig,
        html: () => this.head('Safety desk', 'SOS alerts, PIN lockouts and reports from drivers. Closing anything needs a written outcome.') +
          '<div class="adm-card"><div class="adm-card-head"><b>SOS alerts</b></div>' +
          (sos.length ? sos.map(([w, a]) => {
            const t = a.details, d = a.driverId ? byId(s, a.driverId) : null;
            return '<div class="adm-alert"><div class="adm-alert-ic">' + ic('alert') + '</div><div class="adm-grow"><b>SOS from the ' + w + (w === 'driver' && d ? ' · ' + esc(d.name) : w === 'rider' ? ' · ' + esc(s.rider.profile.name) : '') + '</b>' +
              '<span class="adm-sub">' + (t ? esc(t.rideId + ' · ' + (t.vehicle ? t.vehicle.color + ' ' + t.vehicle.make + ' ' + t.vehicle.model + ' · ' + t.vehicle.plate : 'no driver yet') + ' · GPS ' + t.location) : a.rideId ? 'Ride ' + esc(a.rideId) : 'Not on a ride') + '</span>' +
              '<span class="adm-sub">Sent to her emergency contacts and this desk' + (a.via === 'RideCheck' ? ' via RideCheck' : '') + '</span></div>' +
              (a.ack ? '<span class="adm-pill" data-tone="green">Acknowledged</span>' : '<button class="adm-btn primary" data-a="ackSos" data-v="' + w + '">Acknowledge</button>') + '</div>';
          }).join('') : '<p class="adm-muted">No SOS alerts. The red SOS pill on either phone sends one.</p>') + '</div>' +
          '<div class="adm-card"><div class="adm-card-head"><b>Open incidents</b></div>' +
          (s.incidents.length ? s.incidents.map((x) => '<div class="adm-alert"><div class="adm-alert-ic">' + ic('lock') + '</div><div class="adm-grow"><b>' + esc(x.kind) + ' · ' + esc(x.driverName) + '</b><span class="adm-sub">' + esc(x.id) + ' · ride ' + esc(x.rideId) + ' · the driver gets no requests until this is closed</span>' +
            outcome('inc:' + x.id) + '</div><button class="adm-btn primary" data-a="closeIncident" data-v="' + esc(x.id) + '">Close incident</button></div>').join('')
            : '<p class="adm-muted">No open incidents. Five wrong PINs at a pickup opens one.</p>') + '</div>' +
          '<div class="adm-card"><div class="adm-card-head"><b>Reports from drivers</b></div>' +
          (s.reports.length ? s.reports.map((x) => '<div class="adm-alert"><div class="adm-alert-ic">' + ic('report') + '</div><div class="adm-grow"><b>' + esc(x.reason) + '</b><span class="adm-sub">' + esc(x.id) + ' · ride ' + esc(x.rideId) + ' · by ' + esc(x.by) + ' about ' + esc(x.rider || 'the rider') + ' · ' + time(x.at) + '</span>' +
            (x.status === 'open' ? outcome('rpt:' + x.id) : '<span class="adm-sub">Outcome: ' + esc(x.outcome) + '</span>') + '</div>' +
            (x.status === 'open' ? '<button class="adm-btn primary" data-a="closeReport" data-v="' + esc(x.id) + '">Close report</button>' : '<span class="adm-pill" data-tone="green">Closed</span>') + '</div>').join('')
            : '<p class="adm-muted">No reports. During a trip the driver can tap <i>Report rider</i>.</p>') + '</div>',
      };
    }

    // ----------------------------------------------------------------- Pricing
    pricing(s) {
      const p = s.pricing, promo = E.promoInfo(s);
      const fixed = promo.state === 'active' || promo.state === 'ended';
      const cell = (o, k, v, step) => '<td><input class="adm-input sm num" type="number" min="0" step="' + step + '" data-draft="rate:' + o + ':' + k + '" value="' + esc(this.draft('rate:' + o + ':' + k, v)) + '"></td>';
      return {
        key: 'pr:' + p.version + ':' + s.settings.demand + ':' + promo.state + ':' + p.launchPromo.startsAt + ':' + p.kidsSurcharge,
        html: () => this.head('Pricing', 'Prices are locked when a rider requests, so changes apply to new quotes only, never to a ride already booked.') +
          '<div class="adm-card"><div class="adm-card-head"><b>Rates per ride type</b><span class="adm-tiny adm-muted">version ' + p.version + ' · fee ' + E.CFG.FEE_MIN * 100 + '–' + E.CFG.FEE_MAX * 100 + ' %</span></div>' +
          '<table class="adm-table"><thead><tr><th>Ride type</th><th>Base N$</th><th>Per km N$</th><th>Per min N$</th><th>Minimum N$</th><th>Service fee %</th><th></th></tr></thead><tbody>' +
          Object.values(E.OPTIONS).map((o) => { const t = p.tiers[o.id]; return '<tr><td><b>' + esc(o.name) + '</b></td>' + cell(o.id, 'base', t.base, 1) + cell(o.id, 'perKm', t.perKm, 0.5) + cell(o.id, 'perMin', t.perMin, 0.1) + cell(o.id, 'min', t.min, 1) + cell(o.id, 'fee', +(t.serviceFee * 100).toFixed(1), 0.5) +
            '<td class="adm-right"><button class="adm-btn sm primary" data-a="saveRates" data-v="' + o.id + '">Save</button></td></tr>'; }).join('') + '</tbody></table>' +
          '<p class="adm-tiny adm-muted">Try it: raise Classic\'s per-km rate and save. The rider phone\'s ride options show the new price straight away.</p></div>' +
          '<div class="adm-grid3">' +
          '<div class="adm-card"><div class="adm-card-head"><b>Demand and surge</b></div><div class="adm-seg">' + Object.values(E.DEMAND).map((x) => '<button data-a="demand" data-v="' + x.id + '"' + (x.id === s.settings.demand ? ' aria-pressed="true"' : '') + '>' + esc(x.label.replace(' demand', '')) + '<small>' + x.surge.toFixed(1) + '×</small></button>').join('') + '</div>' +
          '<p class="adm-tiny adm-muted">The multiplier applies to new quotes. Riders see it before they request.</p></div>' +
          '<div class="adm-card"><div class="adm-card-head"><b>Launch promotion</b><span class="adm-pill" data-tone="' + (promo.state === 'active' ? 'green' : promo.state === 'scheduled' ? 'amber' : 'gray') + '">' + { unset: 'Not set', scheduled: 'Scheduled', active: 'Running', ended: 'Ended' }[promo.state] + '</span></div>' +
          '<p class="adm-small">13 % service fee for every driver for ' + E.CFG.LAUNCH_PROMO_DAYS + ' days from the launch date, then back to the normal fees by itself.</p>' +
          (promo.state !== 'unset' ? this.facts([['Starts', day(promo.startsAt)], ['Ends', day(promo.endsAt)]].concat(promo.state === 'active' ? [['Days left', String(promo.daysLeft)]] : [])) : '') +
          (fixed ? '<p class="adm-tiny adm-muted">It has started, so the launch date is fixed. Reset the demo to try again.</p>'
            : '<div class="adm-inline"><input class="adm-input sm" type="date" data-draft="launchDate" value="' + esc(this.draft('launchDate', promo.startsAt ? isoDay(promo.startsAt) : '')) + '"><button class="adm-btn sm" data-a="launch">Save date</button></div>' +
              '<div class="adm-row"><button class="adm-btn primary sm" data-a="launch" data-v="today">' + ic('zap') + 'Launch today</button>' + (promo.state === 'scheduled' ? '<button class="adm-link" data-a="launch" data-v="clear">Clear date</button>' : '') + '</div>') + '</div>' +
          '<div class="adm-card"><div class="adm-card-head"><b>Child seat surcharge</b></div>' +
          '<p class="adm-small">Kids Guardian: a flat amount added when a rider brings children and asks for a child seat. No fee is taken on it; the driver keeps all of it.</p>' +
          '<div class="adm-inline"><span>N$</span><input class="adm-input sm num" type="number" min="0" max="' + E.CFG.KIDS_SURCHARGE_MAX + '" step="5" data-draft="kids" value="' + esc(this.draft('kids', p.kidsSurcharge)) + '"><button class="adm-btn sm primary" data-a="kids">Save</button></div>' +
          '<p class="adm-tiny adm-muted">Currently ' + money(p.kidsSurcharge) + '. Riders under 18 never ride alone: unaccompanied minors stay off until legal sign-off.</p></div>' +
          '</div>',
      };
    }

    // ------------------------------------------------------------------- Trips
    trips(s) {
      const t = s.trips;
      return {
        key: 'tr:' + t.length,
        html: () => this.head('Trips', 'Completed trips and how each fare was split. Cash is collected by the driver; VARA\'s fee goes on her balance.') +
          '<div class="adm-card">' + (t.length ? '<table class="adm-table"><thead><tr><th>Ride</th><th>Time</th><th>Driver</th><th>Type</th><th>To</th><th class="adm-right">Rider paid</th><th class="adm-right">Fee</th><th class="adm-right">Child seat</th><th class="adm-right">Driver earned</th></tr></thead><tbody>' +
            t.map((x) => '<tr><td class="adm-mono">' + esc(x.id) + '</td><td>' + time(x.at) + '</td><td>' + esc(x.driverName) + '</td><td>' + esc(E.OPTIONS[x.option].name) + '</td><td>' + esc(x.to) + '</td>' +
              '<td class="adm-right">' + money(x.paid) + '</td><td class="adm-right">' + money(x.fee) + '<span class="adm-sub">' + pct(x.feeRate) + ' ' + E.FEE_WHY[x.feeWhy] + '</span></td>' +
              '<td class="adm-right">' + (x.kids ? money(x.kids) : '—') + '</td><td class="adm-right"><b>' + money(x.earned) + '</b></td></tr>').join('') + '</tbody></table>'
            : '<p class="adm-muted">No completed trips yet.</p>') + '</div>',
      };
    }

    // --------------------------------------------------------------- Audit log
    auditLog(s) {
      const a = s.audit;
      return {
        key: 'au:' + a.length + ':' + (a[0] ? a[0].id : ''),
        html: () => this.head('Audit log', 'Every change made in this console: who, when and what. In the real console it cannot be edited or deleted.') +
          '<div class="adm-card">' + (a.length ? '<table class="adm-table"><thead><tr><th>Time</th><th>Who</th><th>Action</th><th>Detail</th></tr></thead><tbody>' +
            a.map((x) => '<tr><td>' + time(x.at) + '</td><td>' + esc(x.who) + '</td><td class="adm-mono">' + esc(x.action) + '</td><td>' + esc(x.detail) + '</td></tr>').join('') + '</tbody></table>'
            : '<p class="adm-muted">Nothing yet. Approve someone or change a price and it appears here.</p>') + '</div>',
      };
    }
  }

  window.AdminApp = AdminApp;
})();
