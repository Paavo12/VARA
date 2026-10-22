/* Rider portal */
(function () {
  'use strict';
  const { E, Store, esc, money, mins, km, arriveAt, dateShort, mmss, firstName, range, greeting, ic, stars, avatar, carSVG, readImage, patch, bind, toast, statusBanner, emsConfirm } = Hera;

  const PAY = {
    card: { icon: 'card', label: 'Visa •••• 4242' },
    cash: { icon: 'cash', label: 'Cash' },
    wallet: { icon: 'wallet', label: 'FUber Wallet' },
  };
  const RATE_TAGS = ['Safe driving', 'Friendly', 'Clean car', 'On time', 'Felt safe', 'Great music'];
  const QUICK_MSGS = ["I'm on my way down", "I'm by the entrance", 'Running 2 min late', 'Thank you!'];
  const byId = (s, id) => s.drivers.find((d) => d.id === id);
  const shortName = (n) => (n.length > 22 ? n.slice(0, 21) + '…' : n);

  function defaultSchedule() {
    const d = new Date(Date.now() + 2 * 3600000);
    d.setMinutes(Math.ceil(d.getMinutes() / 15) * 15, 0, 0);
    const p = (n) => String(n).padStart(2, '0');
    return d.getFullYear() + '-' + p(d.getMonth() + 1) + '-' + p(d.getDate()) + 'T' + p(d.getHours()) + ':' + p(d.getMinutes());
  }
  function whenLabel(v) {
    const d = new Date(v);
    return isNaN(d) ? v : d.toLocaleString([], { weekday: 'short', month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit' });
  }
  function setPath(obj, path, val) {
    const parts = path.split('.');
    let o = obj;
    while (parts.length > 1) o = o[parts.shift()];
    o[parts[0]] = val;
  }
  function getPath(obj, path) { return path.split('.').reduce((o, k) => (o == null ? o : o[k]), obj); }

  class RiderApp {
    constructor(phone) {
      this.phone = phone;
      this.content = phone.querySelector('.content');
      this.overlay = phone.querySelector('.overlay');
      this.mapHost = phone.querySelector('.map-host');
      this.map = new Hera.MapView(this.mapHost);
      this.resetUi();
      phone.addEventListener('click', (e) => this.onClick(e));
      phone.addEventListener('input', (e) => this.onInput(e));
      phone.addEventListener('change', (e) => this.onChange(e));
    }

    resetUi() {
      this.ui = {
        v: 0, tab: 'home', flow: 'idle', query: '', dest: null, option: 'classic', wav: false, when: 'now', schedAt: defaultSchedule(),
        modal: null, signup: { name: '', contact: '', password: '', photo: null }, contacts: [{ name: '', phone: '' }],
        consent: { sos: true, rideCheck: true, shareEta: true }, idPreview: null, selfiePreview: null,
        rating: { stars: 0, tags: [], favourite: false, comment: '' }, payMethod: null, chat: '', newContact: { name: '', phone: '' },
        callStart: 0, forOther: false, guest: { name: '', phone: '' }, seats: 1,
      };
    }

    set(p) { Object.assign(this.ui, p); this.ui.v++; this.render(Store.state); }

    async act(type, payload) {
      const r = await Store.send(type, payload);
      if (r && !r.ok) toast(this.phone, r.error, 'err');
      return r || { ok: false };
    }

    // ------------------------------------------------------------------------
    render(s) {
      if (!s) return;
      if (this.epoch !== s.startedWall) {
        this.epoch = s.startedWall;
        this.resetUi();
        this.lastNotice = s.noticeSeq;
      }
      const rideId = s.ride ? s.ride.id : null;
      if (rideId !== this.lastRide) {
        this.lastRide = rideId;
        this.ui.flow = 'idle';
        this.ui.tab = 'home';
        this.ui.modal = null;
        this.ui.rating = { stars: 0, tags: [], favourite: false, comment: '' };
        this.ui.payMethod = null;
        this.ui.v++;
      }
      this.s = s;
      const scr = this.screen(s);
      patch(this.content, scr.key + '|' + this.ui.v, scr.html);
      bind(this.content, scr.vals);
      this.mapHost.hidden = !scr.map;
      if (scr.map) this.map.update(scr.map);
      this.renderOverlay(s);
      for (const n of s.notices) {
        if (n.id > this.lastNotice) {
          this.lastNotice = n.id;
          if (n.to === 'rider' || n.to === 'all') toast(this.phone, n.text, n.kind);
        }
      }
    }

    screen(s) {
      const rd = s.rider;
      if (!rd.onboarded) return this.onboarding(s);
      if (s.ride) return this.rideScreen(s, s.ride);
      if (this.ui.tab === 'trips') return this.tripsTab(s);
      if (this.ui.tab === 'safety') return this.safetyTab(s);
      if (this.ui.tab === 'account') return this.accountTab(s);
      if (this.ui.flow === 'search') return this.searchFlow(s);
      if (this.ui.flow === 'pickdest' || this.ui.flow === 'adjust') return this.tapFlow(s);
      if (this.ui.flow === 'options' && this.ui.dest) return this.optionsFlow(s);
      return this.home(s);
    }

    sos() { return '<button class="sos-btn" data-a="sos" title="Emergency SOS">' + ic('shield') + 'SOS</button>'; }

    tabbar(active) {
      const t = (id, icon, label) => '<button class="tab' + (active === id ? ' on' : '') + '" data-a="tab" data-v="' + id + '">' + ic(icon) + '<span>' + label + '</span></button>';
      return '<nav class="tabbar">' + t('home', 'home', 'Ride') + t('trips', 'list', 'Trips') + t('safety', 'shieldCheck', 'Safety') + t('account', 'user', 'Account') + '</nav>';
    }

    nearbyCars(s, near, maxD) {
      return s.drivers.filter((d) => d.online && E.dist(d, near) < (maxD || 3500))
        .map((d) => ({ id: d.id, x: d.x, y: d.y, h: d.heading, kind: d.busy ? 'busy' : 'idle' }));
    }

    // ------------------------------------------------------------------------
    // Onboarding
    // ------------------------------------------------------------------------
    onboarding(s) {
      const rd = s.rider, u = this.ui;
      const step = rd.step;
      if (step === 'welcome') {
        return {
          key: 'onb-welcome',
          html: () => '<div class="page onb-welcome">' +
            '<div class="welcome-art"><div class="brand-orb">' + ic('shieldCheck') + '</div></div>' +
            '<div class="brand-word">FUber</div>' +
            '<h1 class="h1">Rides by women,<br>for women.</h1>' +
            '<p class="muted">Every driver and every rider on FUber is a verified woman. Up to ' + E.CFG.MAX_SEATS + ' women riders per trip.</p>' +
            '<ul class="trust-list">' +
            '<li>' + ic('shieldCheck') + '<span><b>Verified women only</b><br>ID and selfie checks for drivers and riders</span></li>' +
            '<li>' + ic('user') + '<span><b>No shared rides</b><br>Just you in the car, every time</span></li>' +
            '<li>' + ic('alert') + '<span><b>Safety built in</b><br>SOS, PIN check, RideCheck and live trip sharing</span></li></ul>' +
            '<div class="stack-sm push-bottom"><button class="btn btn-primary btn-block" data-a="goto" data-v="signup">Create account</button>' +
            '<button class="btn btn-ghost btn-block" data-a="demoLogin">Use demo account (skip onboarding)</button></div></div>',
        };
      }
      if (step === 'signup') {
        const f = u.signup;
        return {
          key: 'onb-signup',
          html: () => '<div class="page">' + this.stepHead('welcome', 1) +
            '<h2 class="h2">Create your account</h2><p class="muted small">You will verify your identity in the next step.</p>' +
            '<label class="photo-pick">' + (f.photo ? '<img src="' + f.photo + '" alt="">' : '<span>' + ic('camera') + '</span>') +
            '<input type="file" accept="image/*" data-file="photo" hidden><em>' + (f.photo ? 'Change photo' : 'Add profile photo') + '</em></label>' +
            this.field('Full name', 'signup.name', 'text', 'e.g. Ndeshi Amutenya') +
            this.field('Phone or email', 'signup.contact', 'text', '+264 81 123 4567 or you@example.com') +
            this.field('Password', 'signup.password', 'password', 'At least 6 characters') +
            '<div class="stack-sm push-bottom"><button class="btn btn-primary btn-block" data-a="signup">Continue</button>' +
            '<button class="btn btn-link" data-a="fillSignup">Fill sample details</button></div></div>',
        };
      }
      if (step === 'verify') {
        const v = rd.verification;
        const tile = (doc, icon, title, sub, preview, capture) => {
          const done = v[doc];
          return '<div class="upload-tile' + (done ? ' done' : '') + '"><div class="ut-icon">' + (preview ? '<img src="' + preview + '" alt="">' : ic(done ? 'check' : icon)) + '</div>' +
            '<div class="ut-main"><b>' + title + '</b><div class="tiny muted">' + sub + '</div></div>' +
            (done ? '<span class="ut-ok">' + ic('check') + ' Added</span>' :
              '<div class="ut-actions"><label class="btn btn-sm btn-soft">' + ic(capture ? 'camera' : 'upload') + (capture ? 'Take' : 'Upload') +
              '<input type="file" accept="image/*" ' + (capture ? 'capture="user" ' : '') + 'data-file="' + doc + '" hidden></label>' +
              '<button class="btn btn-sm btn-link" data-a="sampleDoc" data-v="' + doc + '">Sample</button></div>') + '</div>';
        };
        let status = '';
        if (v.status === 'none') status = '<button class="btn btn-primary btn-block" data-a="submitVerify"' + (v.id && v.selfie ? '' : ' disabled') + '>Submit for verification</button>';
        else if (v.status === 'pending') status = '<div class="verify-state pending"><div class="spinner"></div><div><b>Pending verification</b><div class="tiny muted">Our safety team is reviewing your documents (simulated: ~5 s)</div></div></div>' +
          '<button class="btn btn-link" data-a="approveNow">Approve now (presenter)</button>';
        else status = '<div class="verify-state verified"><div class="big-check">' + ic('check') + '</div><div><b>Verified</b><div class="tiny muted">Welcome to a women-only community</div></div></div>' +
          '<button class="btn btn-primary btn-block" data-a="goto" data-v="contacts">Continue</button>';
        return {
          key: 'onb-verify:' + v.id + v.selfie + v.status,
          html: () => '<div class="page">' + this.stepHead('signup', 2) +
            '<h2 class="h2">Verify you\'re a woman</h2>' +
            '<p class="muted small">FUber is women-only. We check a government ID and a live selfie for every rider. Documents are encrypted and deleted after review.</p>' +
            tile('id', 'idcard', 'Government ID', 'Passport, national ID or driving licence', u.idPreview) +
            tile('selfie', 'camera', 'Live selfie', 'Matched against your ID photo', u.selfiePreview, true) +
            '<div class="stack-sm push-bottom">' + status + '</div></div>',
        };
      }
      // contacts + consent
      return {
        key: 'onb-contacts',
        html: () => '<div class="page">' + this.stepHead('verify', 3) +
          '<h2 class="h2">Safety setup</h2><p class="muted small">Emergency contacts receive your live location when you press SOS.</p>' +
          '<div class="eyebrow">Emergency contacts</div>' +
          u.contacts.map((c, i) => '<div class="contact-edit"><input class="input" placeholder="Name" data-m="contacts.' + i + '.name" value="' + esc(c.name) + '">' +
            '<input class="input" placeholder="Phone" data-m="contacts.' + i + '.phone" value="' + esc(c.phone) + '"></div>').join('') +
          (u.contacts.length < 3 ? '<button class="btn btn-link" data-a="addContactRow">' + ic('plus') + 'Add another contact</button>' : '') +
          '<div class="eyebrow">Safety consent</div>' +
          this.toggle('consent.sos', 'Share my live location with emergency contacts during SOS') +
          this.toggle('consent.rideCheck', 'RideCheck: check in with me if my trip looks unusual') +
          this.toggle('consent.shareEta', 'Let me share live trip links with friends') +
          '<div class="stack-sm push-bottom"><button class="btn btn-primary btn-block" data-a="saveContacts">Finish setup</button>' +
          '<button class="btn btn-link" data-a="fillContacts">Fill sample contacts</button></div></div>',
      };
    }

    stepHead(back, n) {
      return '<div class="page-head"><button class="icon-btn" data-a="goto" data-v="' + back + '">' + ic('chevronLeft') + '</button>' +
        '<div class="steps">' + [1, 2, 3].map((i) => '<i class="' + (i <= n ? 'on' : '') + '"></i>').join('') + '</div><span class="tiny muted">Step ' + n + ' of 3</span></div>';
    }
    field(label, path, type, ph) {
      return '<label class="field"><span>' + label + '</span><input class="input" type="' + type + '" placeholder="' + esc(ph) + '" data-m="' + path + '" value="' + esc(getPath(this.ui, path) || '') + '"></label>';
    }
    toggle(path, label, live) {
      const on = !!getPath(this.ui, path);
      return '<label class="toggle-row"><span>' + label + '</span><input type="checkbox" class="switch" ' + (live ? 'data-live="' + live + '" ' : 'data-m="' + path + '" ') + (on ? 'checked' : '') + '></label>';
    }

    // ------------------------------------------------------------------------
    // Booking
    // ------------------------------------------------------------------------
    home(s) {
      const rd = s.rider, pk = rd.pickup;
      const upcoming = rd.scheduled[0];
      const recent = ['postst', 'maerua', 'grove', 'whkhosp'].map((id) => E.PLACES.find((p) => p.id === id));
      const nearby = s.drivers.filter((d) => d.online && !d.busy && E.dist(d, pk) <= E.CFG.RADIUS).length;
      return {
        key: 'home:' + pk.x + ',' + pk.y + ':' + s.surge + ':' + rd.scheduled.length,
        map: { fit: [pk], minSpan: 3200, pickup: { x: pk.x, y: pk.y, label: 'You are here' }, cars: this.nearbyCars(s, pk), pad: { t: 100, r: 20, b: 420, l: 20 } },
        vals: { nearby: nearby + ' verified driver' + (nearby === 1 ? '' : 's') + ' nearby' },
        html: () => this.sos() +
          '<div class="float-top home-top">' + avatar(rd.profile, 38) + '<div><div class="tiny muted">' + greeting() + '</div><div class="h3">' + esc(firstName(rd.profile.name)) +
          ' <span class="badge-verified">' + ic('check') + 'Verified</span></div></div></div>' +
          '<div class="sheet with-tabs">' +
          (s.surge > 1 ? '<div class="surge-note">' + ic('zap') + '<span><b>' + E.DEMAND[s.settings.demand].label + '</b> · fares are ' + s.surge.toFixed(1) + '× right now</span></div>' : '') +
          '<button class="where-to" data-a="flow" data-v="search">' + ic('search') + '<span>Where to?</span><span class="chip">' + ic('clock') + 'Now</span></button>' +
          '<div class="pickup-row"><span class="dot-pickup"></span><div class="grow"><div class="tiny muted">Pickup · ' + (pk.source === 'gps' ? 'GPS location' : 'Pin adjusted') + '</div><div class="small strong">' + esc(E.addressOf(pk)) + ', ' + E.zoneOf(pk) + '</div></div>' +
          '<button class="btn btn-sm btn-soft" data-a="flow" data-v="adjust">Adjust</button></div>' +
          '<div class="quick-places">' + recent.map((p) => '<button class="qp" data-a="pickPlace" data-v="' + p.id + '">' + ic('clock') + '<span>' + esc(p.name) + '</span></button>').join('') + '</div>' +
          (upcoming ? '<div class="sched-card">' + ic('calendar') + '<div class="grow"><div class="tiny muted">Scheduled</div><b>' + esc(whenLabel(upcoming.when)) + '</b><div class="tiny muted">To ' + esc(upcoming.dest.name) + '</div></div>' +
            '<button class="btn btn-sm btn-soft" data-a="tab" data-v="trips">View</button></div>' : '') +
          '<div class="safety-strip">' + ic('shieldCheck') + '<span data-b="nearby"></span><span class="sep">·</span><span>Up to ' + E.CFG.MAX_SEATS + ' riders per trip</span></div>' +
          '</div>' + this.tabbar('home'),
      };
    }

    searchFlow(s) {
      const pk = s.rider.pickup;
      return {
        key: 'search',
        html: () => '<div class="page search-page">' +
          '<div class="page-head"><button class="icon-btn" data-a="flow" data-v="idle">' + ic('chevronLeft') + '</button><b>Plan your ride</b><span></span></div>' +
          '<div class="route-inputs"><div class="ri"><span class="dot-pickup sm"></span><input class="input" readonly value="Current location · ' + esc(E.addressOf(pk)) + '"></div>' +
          '<div class="ri"><span class="sq-dest"></span><input class="input" data-m="query" data-results placeholder="Where to?" value="' + esc(this.ui.query) + '" autofocus></div></div>' +
          '<button class="list-item" data-a="flow" data-v="pickdest">' + ic('pin') + '<div><b>Set destination on map</b><div class="tiny muted">Tap anywhere on the city map</div></div></button>' +
          '<div class="eyebrow">Places</div><div class="results">' + this.resultsHtml(s) + '</div></div>',
      };
    }

    resultsHtml(s) {
      const pk = s.rider.pickup;
      // Search ignores upper/lower case and accents, and covers landmarks, suburbs and street names.
      const list = this.results = E.searchPlaces(this.ui.query, 20);
      if (!list.length) return '<div class="empty small muted">Nothing matches “' + esc(this.ui.query) + '”. Try a street, suburb or place name, or set a pin on the map.</div>';
      return list.map((p, i) => '<button class="list-item" data-a="pickResult" data-v="' + i + '">' + ic(p.type === 'place' ? 'pin' : p.type === 'suburb' ? 'home' : 'route') + '<div class="grow"><b>' + esc(p.name) + '</b><div class="tiny muted">' + esc(p.sub) + '</div></div><span class="tiny muted">' + km(E.roadDist(pk, p)) + '</span></button>').join('');
    }

    tapFlow(s) {
      const adjust = this.ui.flow === 'adjust';
      const pk = s.rider.pickup;
      return {
        key: 'tap:' + this.ui.flow + ':' + pk.x + ',' + pk.y,
        map: {
          fit: [pk], minSpan: adjust ? 2600 : 6200, pickup: { x: pk.x, y: pk.y, label: adjust ? 'Pickup' : '' }, cars: adjust ? [] : this.nearbyCars(s, pk), pad: { t: 160, r: 16, b: 150, l: 16 },
          onTap: (p) => this.onMapTap(p),
        },
        html: () => '<div class="float-top tap-banner">' + ic(adjust ? 'target' : 'pin') + '<div class="grow"><b>' + (adjust ? 'Tap the map to move your pickup pin' : 'Tap the map to set your drop-off') + '</b>' +
          '<div class="tiny muted">' + (adjust ? 'Pins snap to the nearest street. Current: ' + esc(E.addressOf(pk)) : 'Pins snap to the nearest street') + '</div></div></div>' +
          '<div class="float-bottom row">' + (adjust ? '<button class="btn btn-soft" data-a="gps">' + ic('target') + 'Use GPS</button><button class="btn btn-primary grow" data-a="flow" data-v="idle">Done</button>'
            : '<button class="btn btn-soft grow" data-a="flow" data-v="search">Cancel</button>') + '</div>',
      };
    }

    onMapTap(p) {
      if (this.ui.flow === 'adjust') { this.act('R_SET_PICKUP', p); return; }
      const q = E.snap(p);
      this.set({ dest: { x: q.x, y: q.y, name: 'Pin · ' + E.addressOf(q) }, flow: 'options' });
    }

    optionsFlow(s) {
      const u = this.ui, pk = s.rider.pickup, dest = u.dest;
      const opts = Object.values(E.OPTIONS).map((o) => {
        const est = E.estimate(s, pk, dest, o.id);
        const rk = E.rankDrivers(s, pk, o.id, u.wav, []);
        const best = rk.candidates.reduce((m, c) => (m == null || c.eta < m ? c.eta : m), null);
        return { o, est, best, avail: rk.candidates.length > 0 };
      });
      if (!opts.find((x) => x.o.id === u.option && x.avail)) {
        const first = opts.find((x) => x.avail);
        if (first) u.option = first.o.id;
      }
      const sel = opts.find((x) => x.o.id === u.option) || opts[0];
      const pay = u.payMethod || s.rider.payment;
      const vals = {};
      for (const x of opts) vals['eta_' + x.o.id] = x.avail ? mins(x.best) + ' away' : (u.wav ? 'No accessible car within 3 km' : 'None within 3 km');
      vals.tripLine = sel.est.distKm.toFixed(1) + ' km · ~' + mins(sel.est.durMin) + ' trip · arrive ~' + arriveAt(sel.est.durMin + (sel.best || 0));
      const route = E.route(pk, dest);
      return {
        key: 'opts:' + dest.x + ',' + dest.y + ':' + s.surge + ':' + opts.map((x) => x.avail ? 1 : 0).join('') + ':' + pay,
        map: { fit: [pk, dest], route, pickup: { x: pk.x, y: pk.y, label: 'Pickup' }, dest: { x: dest.x, y: dest.y, label: shortName(dest.name) }, cars: this.nearbyCars(s, pk), minSpan: 1400, pad: { t: 90, r: 40, b: 560, l: 40 } },
        vals,
        html: () => this.sos() + '<button class="icon-btn float-back" data-a="flow" data-v="search">' + ic('chevronLeft') + '</button>' +
          '<div class="sheet tall">' +
          '<div class="trip-head"><div class="grow"><div class="tiny muted">Drop-off</div><div class="h3">' + esc(dest.name) + '</div><div class="small muted" data-b="tripLine"></div></div>' +
          (s.surge > 1 ? '<span class="badge-surge">' + ic('zap') + s.surge.toFixed(1) + '×</span>' : '') + '</div>' +
          this.toggle('wav', ic('wheelchair') + ' Wheelchair-accessible vehicle', 'wav') +
          '<div class="seg"><button data-a="rideFor" data-v="me"' + (u.forOther ? '' : ' aria-pressed="true"') + '>' + ic('user') + 'For me</button>' +
          '<button data-a="rideFor" data-v="other"' + (u.forOther ? ' aria-pressed="true"' : '') + '>' + ic('users') + 'Someone else</button></div>' +
          (u.forOther ? '<div class="grid2">' + this.field('Passenger name', 'guest.name', 'text', 'e.g. Ndapandula Shikongo') + this.field('Passenger phone', 'guest.phone', 'tel', '+264 81 …') + '</div>' +
            '<div class="note">' + ic('message') + '<span>They do not need the app or internet. We text them the PIN and driver details by SMS. Set the pickup pin to where they are. <button class="btn-link" data-a="fillGuest">Fill sample</button></span></div>' : '') +
          '<div class="eyebrow">Passengers · women only</div><div class="seg">' + [1, 2, 3, 4].map((n) => '<button data-a="seats" data-v="' + n + '"' + (u.seats === n ? ' aria-pressed="true"' : '') + '>' + ic(n > 1 ? 'users' : 'user') + n + '</button>').join('') + '</div>' +
          '<div class="opt-list">' + opts.map((x) => '<button class="opt-card" data-a="opt" data-v="' + x.o.id + '"' + (x.o.id === u.option ? ' aria-pressed="true"' : '') + (x.avail ? '' : ' disabled') + '>' +
            '<div class="opt-car">' + carSVG(x.o.id === 'softlife' ? '#6b2c91' : x.o.id === 'comfort' ? '#47506a' : '#e0457b', u.wav) + '</div>' +
            '<div class="opt-main"><div class="opt-name">' + esc(x.o.name) + '</div><div class="tiny muted">' + esc(x.o.blurb) + '</div><div class="tiny strong" data-b="eta_' + x.o.id + '"></div></div>' +
            '<div class="opt-price">' + money(x.est.total) + (s.surge > 1 ? '<span class="tiny surge-txt">incl. ' + s.surge.toFixed(1) + '×</span>' : '') + '</div></button>').join('') + '</div>' +
          '<div class="seg"><button data-a="when" data-v="now"' + (u.when === 'now' ? ' aria-pressed="true"' : '') + '>' + ic('zap') + 'Now</button>' +
          '<button data-a="when" data-v="later"' + (u.when === 'later' ? ' aria-pressed="true"' : '') + '>' + ic('calendar') + 'Schedule</button></div>' +
          (u.when === 'later' ? '<label class="field"><span>Pickup time</span><input class="input" type="datetime-local" data-m="schedAt" value="' + esc(u.schedAt) + '"></label>' : '') +
          '<div class="row gap"><button class="pay-chip" data-a="cyclePay">' + ic(PAY[pay].icon) + PAY[pay].label + (pay === 'wallet' ? ' · ' + money(s.rider.wallet) : '') + ic('chevronRight') + '</button>' +
          '<span class="badge-wo">' + ic('shieldCheck') + 'Women-only</span></div>' +
          '<button class="btn btn-primary btn-block btn-lg" data-a="request"' + (sel.avail ? '' : ' disabled') + '>' + (u.when === 'later' ? 'Schedule ' : 'Request ') + esc(sel.o.short) + ' · ' + money(sel.est.total) + '</button>' +
          '<p class="tiny muted center">Up to ' + E.CFG.MAX_SEATS + ' passengers · no shared rides with strangers · price locked at request</p></div>',
      };
    }

    // ------------------------------------------------------------------------
    // Active ride
    // ------------------------------------------------------------------------
    rideScreen(s, r) {
      const d = r.driverId ? byId(s, r.driverId) : null;
      switch (r.state) {
        case 'REQUESTED': case 'SEARCHING': case 'NO_DRIVERS': return this.searching(s, r);
        case 'DRIVER_ASSIGNED': case 'DRIVER_EN_ROUTE': case 'DRIVER_ARRIVED': return this.enRoute(s, r, d);
        case 'PIN_VERIFIED': case 'IN_TRIP': return this.inTrip(s, r, d);
        case 'COMPLETED': return this.completed(s, r, d);
        case 'RATED': return this.rated(s, r, d);
        default: return this.cancelled(s, r, d);
      }
    }

    searching(s, r) {
      const R = E.CFG.RADIUS, pk = r.pickup;
      const sr = r.search, o = sr.offer;
      const ranked = new Set(sr.ranking.map((c) => c.driverId));
      const cars = s.drivers.filter((d) => d.online).map((d) => ({
        id: d.id, x: d.x, y: d.y, h: d.heading,
        kind: o && o.driverId === d.id ? 'offer' : sr.phase === 'offering' && ranked.has(d.id) && !sr.declined.includes(d.id) ? 'cand' : d.busy ? 'busy' : 'idle',
      }));
      const none = r.state === 'NO_DRIVERS';
      let sub = 'Scanning verified drivers within 3 km…';
      if (sr.phase === 'offering' && o) sub = 'Request sent to the best-matched driver (offer ' + sr.offers.length + ' of ' + (sr.ranking.length + sr.declined.length) + ') · ' + Math.max(0, Math.ceil((o.expiresAt - s.realT) / 1000)) + ' s to respond';
      if (r.state === 'REQUESTED') sub = 'Sending your request…';
      const pct = o ? Math.max(0, Math.min(100, ((s.realT - o.startedAt) / E.CFG.OFFER_MS) * 100)) : 0;
      const opt = E.OPTIONS[r.option];
      return {
        key: 'searching:' + r.id + ':' + r.state + ':' + none + ':' + !!r.driverCancelled,
        map: { ring: { x: pk.x, y: pk.y, r: R, pulse: !none }, fit: [{ x: pk.x - R, y: pk.y - R }, { x: pk.x + R, y: pk.y + R }], pickup: { x: pk.x, y: pk.y, label: 'Pickup' }, cars, minSpan: 1000, pad: { t: 110, r: 10, b: 400, l: 10 } },
        vals: { searchSub: sub, offerPct: pct + '%', offerShow: !!o },
        html: () => statusBanner(r.state, 'rider') + this.sos() +
          '<div class="sheet">' +
          (none ? '<div class="h2">No drivers accepted</div><p class="small muted">Every eligible driver within 3 km declined or was unavailable. You have not been charged.</p>' +
            '<div class="row gap"><button class="btn btn-soft grow" data-a="cancelRide">Cancel</button><button class="btn btn-primary grow" data-a="retry">' + ic('refresh') + 'Try again</button></div>'
            : (r.driverCancelled ? '<div class="note warn">' + ic('info') + '<span>' + esc(r.driverCancelled.name) + ' had to cancel. We\'re finding you another driver — no charge.</span></div>' : '') +
            '<div class="searching-head"><div class="radar"><span></span><span></span>' + ic('car') + '</div><div><div class="h2">Finding your driver…</div><div class="small muted" data-b="searchSub"></div></div></div>' +
            '<div class="offer-timer" data-bshow="offerShow"><div data-bw="offerPct"></div></div>' +
            '<ul class="check-list"><li>' + ic('check') + 'Only verified women drivers</li><li>' + ic('check') + 'No shared rides with strangers</li><li>' + ic('check') + 'Your exact address is shared only on arrival</li></ul>' +
            '<div class="trip-mini">' + ic('car') + '<span><b>' + esc(opt.short) + '</b> to ' + esc(r.dest.name) + (r.passenger ? ' · for ' + esc(firstName(r.passenger.name)) : '') + ' · ' + r.seats + ' passenger' + (r.seats > 1 ? 's' : '') + '</span><span class="grow"></span><b>' + money(r.estimate.total) + '</b></div>' +
            '<button class="btn btn-ghost btn-block" data-a="cancelRide">Cancel request</button>') +
          '</div>',
      };
    }

    driverCard(d, compact) {
      const v = d.vehicle;
      if (compact) {
        return '<div class="driver-row">' + '<div class="dc-photo">' + avatar(d, 44) + '<span class="dc-check">' + ic('check') + '</span></div>' +
          '<div class="grow"><b>' + esc(d.name) + '</b><div class="tiny muted">' + stars(d.rating) + ' ' + d.rating.toFixed(2) + ' · ' + esc(v.color + ' ' + v.make + ' ' + v.model) + '</div></div><span class="plate">' + esc(v.plate) + '</span></div>';
      }
      return '<div class="driver-card"><div class="dc-photo">' + avatar(d, 64) + '<span class="dc-check">' + ic('check') + '</span></div>' +
        '<div class="dc-main"><div class="dc-name">' + esc(d.name) + '</div><div class="tiny">' + ic('star', 'star-ic', true) + '<b>' + d.rating.toFixed(2) + '</b> · ' + d.ratingCount.toLocaleString() + ' trips</div>' +
        '<span class="badge-verified">' + ic('shieldCheck') + 'Verified woman driver</span></div>' +
        '<div class="dc-car">' + carSVG(v.hex, v.wav) + '<span class="plate">' + esc(v.plate) + '</span><div class="tiny muted">' + esc(v.color) + ' ' + esc(v.make + ' ' + v.model) + '</div></div></div>';
    }

    enRoute(s, r, d) {
      const pk = r.pickup, st = r.state, arrived = st === 'DRIVER_ARRIVED';
      const path = d.path && st !== 'DRIVER_ASSIGNED' ? E.remainingPath(d.path, d.pathPos) : E.route(d, pk);
      const eta = r.eta || {};
      const waitLeft = r.wait ? E.CFG.WAIT_FREE_SIM_MS - (s.simT - r.wait.startedSim) : null;
      const last = r.messages[r.messages.length - 1];
      const fav = s.rider.favourites.includes(d.id);
      const head = st === 'DRIVER_ASSIGNED' ? '<div class="h2">' + esc(d.first) + ' accepted your ride</div><div class="small muted">Preparing her route to you…</div>'
        : arrived ? '<div class="h2">' + esc(d.first) + ' is here</div><div class="small muted">Look for the ' + esc(d.vehicle.color + ' ' + d.vehicle.make + ' ' + d.vehicle.model) + ' · check the plate</div>'
          : '<div class="h2">Arriving in <span data-b="etaP"></span></div><div class="small muted">Meet at ' + esc(pk.address) + ' · ' + esc(pk.area) + '</div>';
      return {
        key: 'enroute:' + r.id + ':' + st + ':' + !!r.wait + ':' + r.messages.length + ':' + fav,
        map: { route: path, fit: [d, pk], pickup: { x: pk.x, y: pk.y, label: 'Pickup' }, cars: [{ id: d.id, x: d.x, y: d.y, h: d.heading, kind: 'me' }], minSpan: 900, pad: { t: 110, r: 40, b: 470, l: 40 } },
        vals: {
          etaP: arrived ? 'Here' : mins(eta.pickupMin), etaT: mins(eta.tripMin),
          wait: waitLeft != null ? (waitLeft > 0 ? mmss(waitLeft) + ' of free waiting left' : 'free waiting time is over') : '',
        },
        html: () => statusBanner(st, 'rider') + this.sos() +
          '<div class="sheet ride-sheet">' + head +
          '<div class="eta-tiles"><div class="eta-tile"><span class="tiny">Driver arriving in</span><b data-b="etaP"></b></div><div class="eta-tile"><span class="tiny">Trip will take</span><b data-b="etaT"></b></div></div>' +
          this.driverCard(d) +
          '<div class="pin-card' + (arrived ? ' hot' : '') + '"><div class="grow"><div class="eyebrow">' + (r.passenger ? esc(firstName(r.passenger.name)) + '’s ride PIN' : 'Your ride PIN') + '</div><div class="tiny muted">' +
            (r.passenger ? 'Sent by SMS to ' + esc(r.passenger.phone) + ' · they tell ' + esc(d.first) + ' at the car' : arrived ? 'Tell ' + esc(d.first) + ' this PIN to start the trip' : 'Share it only once you are at the car') + '</div></div>' +
          '<div class="pin-boxes">' + r.pin.split('').map((c) => '<span>' + c + '</span>').join('') + '</div></div>' +
          (r.wait ? '<div class="note warn">' + ic('clock') + '<span>' + esc(d.first) + ' is waiting · <b data-b="wait"></b></span></div>' : '') +
          (last ? '<button class="note msg" data-a="modal" data-v="chat">' + ic('message') + '<span><b>' + (last.from === 'driver' ? esc(d.first) : 'You') + ':</b> ' + esc(last.text) + '</span></button>' : '') +
          '<div class="action-row">' +
          '<button class="action" data-a="call">' + ic('phone') + '<span>Call</span></button>' +
          '<button class="action" data-a="modal" data-v="chat">' + ic('message') + '<span>Message</span></button>' +
          '<button class="action" data-a="share">' + ic('share') + '<span>Share trip</span></button>' +
          '<button class="action' + (fav ? ' on' : '') + '" data-a="fav" data-v="' + d.id + '">' + ic('heart', '', fav) + '<span>Favourite</span></button></div>' +
          '<button class="btn btn-link danger" data-a="modal" data-v="cancel">Cancel ride</button></div>',
      };
    }

    inTrip(s, r, d) {
      const verified = r.state === 'PIN_VERIFIED';
      const path = !verified && d.path ? E.remainingPath(d.path, d.pathPos) : E.route(r.pickup, r.dest);
      const remain = !verified && d.path ? d.pathLen - d.pathPos : E.polyLen(path);
      const pct = Math.max(2, Math.min(100, (r.tripMeters / Math.max(1, r.tripMeters + remain)) * 100));
      const deviated = r.offRouteM > 40 || r.rideChecks.some((c) => c.reason === 'deviation');
      const eta = r.eta || {};
      const lastCheck = r.rideChecks[r.rideChecks.length - 1];
      return {
        key: 'trip:' + r.id + ':' + r.state + ':' + r.rideChecks.length + ':' + !!r.stop,
        map: {
          route: path, planned: deviated ? r.planned : null, fit: [d, r.dest], dest: { x: r.dest.x, y: r.dest.y, label: shortName(r.dest.name) },
          cars: [{ id: d.id, x: d.x, y: d.y, h: d.heading, kind: 'me' }], minSpan: 1200, pad: { t: 110, r: 40, b: 400, l: 40 },
        },
        vals: { etaT: r.atDest ? 'Arrived' : mins(eta.tripMin), arr: r.atDest ? 'now' : arriveAt(eta.tripMin), tripPct: pct + '%', distLeft: km(remain) + ' to go' },
        html: () => statusBanner(r.state, 'rider') + this.sos() +
          '<div class="sheet ride-sheet">' +
          (verified ? '<div class="pin-ok">' + ic('check') + '<div><b>PIN verified</b><div class="tiny muted">' + esc(d.first) + ' can now see your destination. Starting trip…</div></div></div>' : '') +
          '<div class="trip-head"><div class="grow"><div class="tiny muted">Heading to</div><div class="h3">' + esc(r.dest.name) + '</div><div class="small muted">' + esc(r.dest.address) + ' · ' + esc(r.dest.area) + '</div></div>' +
          '<div class="eta-big"><b data-b="etaT"></b><span class="tiny">arrive <span data-b="arr"></span></span></div></div>' +
          '<div class="progress"><div data-bw="tripPct"></div></div><div class="tiny muted right" data-b="distLeft"></div>' +
          this.driverCard(d, true) +
          '<div class="action-row">' +
          '<button class="action" data-a="share">' + ic('share') + '<span>Share trip</span></button>' +
          '<button class="action" data-a="call">' + ic('phone') + '<span>Call</span></button>' +
          '<button class="action" data-a="modal" data-v="chat">' + ic('message') + '<span>Message</span></button>' +
          '<button class="action danger" data-a="sos">' + ic('alert') + '<span>Emergency</span></button></div>' +
          '<div class="note ok">' + ic('shieldCheck') + '<span><b>RideCheck is on.</b> ' + (lastCheck ? 'Last check-in: ' + (lastCheck.status === 'ok' ? 'you confirmed you were fine.' : 'help requested — safety team engaged.') : 'We\'ll check in if the car leaves the route or stops unexpectedly.') + '</span></div>' +
          '</div>',
      };
    }

    completed(s, r, d) {
      const u = this.ui, f = r.fare, pay = u.payMethod || r.payment;
      const line = (l, v, cls) => '<div class="fare-line ' + (cls || '') + '"><span>' + l + '</span><span>' + v + '</span></div>';
      const rated = !!r.riderRating;
      return {
        key: 'done:' + r.id + ':' + rated,
        html: () => this.sos() + '<div class="page done-page">' +
          '<div class="done-hero">' + '<div class="big-check">' + ic('check') + '</div><div class="h2">You\'ve arrived</div><div class="small muted">' + esc(r.dest.name) + ' · ' + r.distKm.toFixed(1) + ' km · ' + Math.round(r.durMin) + ' min</div></div>' +
          '<div class="card"><div class="eyebrow">Fare summary</div>' +
          line('Base fare', money(f.base)) + line('Distance (' + f.distKm.toFixed(1) + ' km)', money(f.distance)) + line('Time (' + Math.round(f.durMin) + ' min)', money(f.time)) +
          (f.minAdj > 0 ? line('Minimum fare adjustment', money(f.minAdj)) : '') +
          (f.surge > 1 ? line('Demand multiplier ' + f.surge.toFixed(1) + '×', money(f.surgeAmt), 'surge') : '') +
          line('Total', money(f.total), 'total') + '</div>' +
          (rated ? '<div class="card center"><div class="spinner sm"></div><div class="small muted">Processing payment…</div></div>' :
            '<div class="card"><div class="eyebrow">Pay with</div><div class="pay-list">' + Object.entries(PAY).map(([k, p]) =>
              '<button class="pay-opt" data-a="payMethod" data-v="' + k + '"' + (pay === k ? ' aria-pressed="true"' : '') + '>' + ic(p.icon) + '<span class="grow">' + p.label + (k === 'wallet' ? ' · ' + money(s.rider.wallet) : '') + (k === 'cash' ? ' · pay on arrival' : '') + '</span><i class="radio"></i></button>').join('') + '</div></div>' +
            '<div class="card"><div class="rate-head">' + avatar(d, 48) + '<div><div class="eyebrow">Rate your driver</div><b>How was your ride with ' + esc(d.first) + '?</b></div></div>' +
            '<div class="star-input">' + [1, 2, 3, 4, 5].map((n) => '<button data-a="star" data-v="' + n + '" class="' + (n <= u.rating.stars ? 'on' : '') + '">' + ic('star', '', true) + '</button>').join('') + '</div>' +
            '<div class="tag-list">' + RATE_TAGS.map((t) => '<button class="tag' + (u.rating.tags.includes(t) ? ' on' : '') + '" data-a="tag" data-v="' + esc(t) + '">' + esc(t) + '</button>').join('') + '</div>' +
            '<label class="toggle-row fav-row"><span>' + ic('heart', '', true) + ' Add ' + esc(d.first) + ' to Favourite Drivers<br><small class="muted">Favourites get priority when they\'re nearby</small></span><input type="checkbox" class="switch" data-m="rating.favourite"' + (u.rating.favourite ? ' checked' : '') + '></label>' +
            '<textarea class="input" rows="2" placeholder="Add a comment (optional)" data-m="rating.comment">' + esc(u.rating.comment) + '</textarea></div>' +
            '<button class="btn btn-primary btn-block btn-lg" data-a="payRate"' + (u.rating.stars ? '' : ' disabled') + '>Pay ' + money(f.total) + ' & submit rating</button>') +
          '</div>',
      };
    }

    rated(s, r, d) {
      return {
        key: 'rated:' + r.id + ':' + !!r.driverRating,
        html: () => this.sos() + '<div class="page done-page"><div class="done-hero"><div class="big-check">' + ic('heart', '', true) + '</div><div class="h2">Thanks for riding with FUber</div>' +
          '<div class="small muted">Paid ' + money(r.fare.total) + ' with ' + PAY[r.payment].label + '</div></div>' +
          '<div class="card"><div class="fare-line"><span>You rated ' + esc(d.first) + '</span><span>' + stars(r.riderRating.stars) + '</span></div>' +
          '<div class="fare-line"><span>' + esc(d.first) + ' rated you</span><span>' + (r.driverRating ? stars(r.driverRating.stars) : '<span class="tiny muted">pending…</span>') + '</span></div>' +
          (s.rider.favourites.includes(d.id) ? '<div class="note ok">' + ic('heart', '', true) + '<span>' + esc(d.first) + ' is in your Favourite Drivers. She\'ll get priority when nearby.</span></div>' : '') + '</div>' +
          '<button class="btn btn-primary btn-block btn-lg" data-a="finish">Done</button></div>',
      };
    }

    cancelled(s, r) {
      const c = r.cancel || {};
      return {
        key: 'cancel:' + r.id,
        html: () => this.sos() + '<div class="page done-page"><div class="done-hero"><div class="big-check muted-check">' + ic('x') + '</div><div class="h2">Ride cancelled</div>' +
          '<div class="small muted">' + (c.by === 'driver' ? esc(c.reason || 'Cancelled by driver') : 'You cancelled this ride') + '</div></div>' +
          '<div class="card"><p class="small">' + (c.by === 'driver' ? 'Your driver waited 5 minutes at the pickup. ' : '') + 'You have not been charged.</p></div>' +
          '<button class="btn btn-primary btn-block btn-lg" data-a="finish">Done</button></div>',
      };
    }

    // ------------------------------------------------------------------------
    // Tabs
    // ------------------------------------------------------------------------
    tripsTab(s) {
      const rd = s.rider;
      return {
        key: 'trips:' + rd.history.length + ':' + rd.scheduled.length + ':' + rd.history.map((h) => h.rating).join(),
        html: () => this.sos() + '<div class="page with-tabs"><div class="h1 page-title">Your trips</div>' +
          '<div class="eyebrow">Upcoming</div>' +
          (rd.scheduled.length ? rd.scheduled.map((x) => '<div class="card sched"><div class="row gap">' + ic('calendar') + '<div class="grow"><b>' + esc(whenLabel(x.when)) + '</b><div class="tiny muted">' + esc(E.OPTIONS[x.option].short) + ' to ' + esc(x.dest.name) + ' · ' + (x.total != null ? money(x.total) : range(x.low, x.high)) + '</div></div></div>' +
            '<div class="row gap"><button class="btn btn-sm btn-soft" data-a="cancelSched" data-v="' + x.id + '">Cancel</button><button class="btn btn-sm btn-primary" data-a="dispatchSched" data-v="' + x.id + '">' + ic('zap') + 'Simulate: it\'s time</button></div></div>').join('')
            : '<div class="empty small muted">' + ic('calendar') + 'No scheduled rides. Choose “Schedule” when booking.</div>') +
          '<div class="eyebrow">Past trips</div>' +
          rd.history.map((h) => '<div class="trip-item"><div class="ti-icon">' + ic(h.status === 'completed' ? 'car' : 'x') + '</div><div class="grow"><b>' + esc(h.to) + '</b>' +
            '<div class="tiny muted">' + dateShort(h.at) + ' · ' + esc(h.driverName) + ' · ' + esc(E.OPTIONS[h.option].short) + '</div>' +
            '<div class="tiny">' + (h.status === 'completed' ? (h.rating ? stars(h.rating) : '<span class="muted">Not rated</span>') : '<span class="tag-cancel">' + (h.status === 'no-show' ? 'No-show' : 'Cancelled') + '</span>') + '</div></div>' +
            '<b>' + money(h.fare) + '</b></div>').join('') + '</div>' + this.tabbar('trips'),
      };
    }

    safetyTab(s) {
      const rd = s.rider, u = this.ui;
      const favs = rd.favourites.map((id) => byId(s, id)).filter(Boolean);
      return {
        key: 'safety:' + rd.contacts.length + ':' + rd.favourites.join() + ':' + JSON.stringify(rd.consent),
        html: () => this.sos() + '<div class="page with-tabs"><div class="h1 page-title">Safety</div>' +
          '<div class="card sos-explain">' + ic('shield') + '<div><b>SOS is on every screen</b><div class="tiny muted">Tap the red SOS button. After a 3-second cancel window, we alert your emergency contacts and the FUber Safety Team with your live location.</div></div></div>' +
          '<div class="eyebrow">Emergency contacts</div>' +
          rd.contacts.map((c, i) => '<div class="list-row">' + ic('user') + '<div class="grow"><b>' + esc(c.name) + '</b><div class="tiny muted">' + esc(c.phone) + '</div></div><button class="icon-btn sm" data-a="removeContact" data-v="' + i + '">' + ic('trash') + '</button></div>').join('') +
          '<div class="contact-edit"><input class="input" placeholder="Name" data-m="newContact.name" value="' + esc(u.newContact.name) + '"><input class="input" placeholder="Phone" data-m="newContact.phone" value="' + esc(u.newContact.phone) + '"><button class="icon-btn" data-a="addContact">' + ic('plus') + '</button></div>' +
          '<div class="eyebrow">Favourite drivers</div>' +
          (favs.length ? favs.map((d) => '<div class="list-row">' + avatar(d, 36) + '<div class="grow"><b>' + esc(d.name) + '</b><div class="tiny muted">★ ' + d.rating.toFixed(2) + ' · ' + esc(d.vehicle.make + ' ' + d.vehicle.model) + '</div></div><button class="icon-btn sm on" data-a="fav" data-v="' + d.id + '">' + ic('heart', '', true) + '</button></div>').join('')
            : '<div class="empty small muted">' + ic('heart') + 'Add drivers after a trip. Favourites get a matching boost when nearby.</div>') +
          '<div class="eyebrow">Preferences</div>' +
          this.consentToggle(rd, 'rideCheck', 'RideCheck automatic check-ins') + this.consentToggle(rd, 'sos', 'Share live location during SOS') + this.consentToggle(rd, 'shareEta', 'Allow live trip sharing links') +
          '<div class="eyebrow">How FUber keeps you safe</div><ul class="check-list">' +
          ['Every driver passes ID, gender, licence and criminal-record checks', 'Riders are verified with ID + selfie', 'Up to ' + E.CFG.MAX_SEATS + ' women passengers per trip — never shared with strangers', 'Drivers see your exact address only on arrival, and your destination only after PIN check', 'Masked phone numbers for calls and messages'].map((t) => '<li>' + ic('check') + t + '</li>').join('') +
          '</ul></div>' + this.tabbar('safety'),
      };
    }

    consentToggle(rd, key, label) {
      return '<label class="toggle-row"><span>' + label + '</span><input type="checkbox" class="switch" data-consent="' + key + '"' + (rd.consent[key] ? ' checked' : '') + '></label>';
    }

    accountTab(s) {
      const rd = s.rider;
      return {
        key: 'account:' + rd.payment + ':' + rd.wallet + ':' + rd.rating,
        html: () => this.sos() + '<div class="page with-tabs"><div class="profile-head">' + avatar(rd.profile, 72) +
          '<div><div class="h2">' + esc(rd.profile.name) + '</div><div class="small muted">' + esc(rd.profile.contact) + '</div>' +
          '<span class="badge-verified">' + ic('shieldCheck') + 'Verified woman rider</span></div></div>' +
          '<div class="stat-grid"><div><b>' + rd.rating.toFixed(2) + '</b><span>Rider rating</span></div><div><b>' + rd.history.filter((h) => h.status === 'completed').length + '</b><span>Trips</span></div><div><b>' + money(rd.wallet) + '</b><span>Wallet</span></div></div>' +
          '<div class="eyebrow">Default payment</div><div class="pay-list">' + Object.entries(PAY).map(([k, p]) =>
            '<button class="pay-opt" data-a="setPayment" data-v="' + k + '"' + (rd.payment === k ? ' aria-pressed="true"' : '') + '>' + ic(p.icon) + '<span class="grow">' + p.label + '</span><i class="radio"></i></button>').join('') + '</div>' +
          '<div class="eyebrow">Accessibility</div><div class="note">' + ic('wheelchair') + '<span>Wheelchair-accessible vehicles can be requested from the ride options screen.</span></div>' +
          '<button class="btn btn-ghost btn-block" data-a="signOut">Sign out (restart onboarding)</button></div>' + this.tabbar('account'),
      };
    }

    // ------------------------------------------------------------------------
    // Overlays (SOS, RideCheck, sheets)
    // ------------------------------------------------------------------------
    renderOverlay(s) {
      const r = s.ride, sos = s.sos.rider, u = this.ui;
      let key = '', html = () => '', vals = {};
      if (sos) {
        const t = sos.details, v = t && t.vehicle;
        const row = (label, value) => '<div><span>' + label + '</span><b>' + esc(value) + '</b></div>';
        const ems = u.ems === sos.at;
        key = 'sos:' + sos.status + (ems ? ':ems' : '');
        vals = { sosCount: String(Math.max(1, Math.ceil((3000 - (s.realT - sos.at)) / 1000))) };
        html = () => sos.status === 'sending'
          ? '<div class="modal-back sos-back"><div class="modal center-modal sos-modal"><div class="sos-count" data-b="sosCount"></div><div class="h2">Sending SOS…</div>' +
          '<p class="small">Alerting your emergency contacts and the FUber Safety Team.</p><button class="btn btn-light btn-block btn-lg" data-a="sosCancel">Cancel — I pressed by mistake</button></div></div>'
          : '<div class="modal-back sos-back"><div class="modal center-modal sos-modal sent"><div class="big-check">' + ic('check') + '</div><div class="h2">Alert sent</div>' +
          '<ul class="sos-list"><li>' + ic('users') + '<span>Emergency contacts notified: ' + esc(s.rider.contacts.map((c) => c.name).join(', ') || '—') + '</span></li>' +
          '<li>' + ic('shieldCheck') + '<span>FUber Safety Team alerted — an agent will call you within 60 s</span></li>' +
          '<li>' + ic('pin') + '<span>Live location shared' + (t ? ' · ' + esc(t.location) : '') + '</span></li>' +
          (sos.via === 'RideCheck' ? '<li>' + ic('info') + '<span>Triggered from RideCheck</span></li>' : '') + '</ul>' +
          (t ? '<div class="sos-details"><div class="eyebrow">' + ic('car') + 'Ride details sent</div>' +
            (v ? '<div class="sos-plate"><span class="plate">' + esc(v.plate) + '</span><b>' + esc(v.color + ' ' + v.make + ' ' + v.model) + '</b></div>' : '') +
            '<div class="sos-grid">' + (v ? row('Make', v.make) + row('Model', v.model) + row('Colour', v.color) + row('Year', String(v.year)) : '') +
            (t.driver ? row('Driver', t.driver.name) : row('Driver', 'Not assigned yet')) + row('Trip', t.rideId + ' · ' + t.option) +
            row('Pickup', t.pickup) + row('Destination', t.dest) + '</div></div>' : '') +
          (ems ? emsConfirm() : '<button class="btn btn-danger btn-block btn-lg" data-a="call911">' + ic('phone') + 'Call emergency services</button>') +
          '<button class="btn btn-light btn-block" data-a="sosCancel">I\'m safe — close alert</button></div></div>';
      } else if (r && r.rideCheck) {
        const rc = r.rideCheck;
        key = 'rc:' + rc.at;
        html = () => '<div class="modal-back"><div class="modal center-modal rc-modal"><div class="rc-icon">' + ic('shieldCheck') + '</div><div class="eyebrow">RideCheck</div><div class="h2">Are you okay?</div>' +
          '<p class="small muted">' + (rc.reason === 'deviation' ? 'Your car has gone off the expected route (' + rc.offRouteM + ' m away).' : 'Your car has been stopped for longer than expected.') + ' We just want to make sure everything is fine.</p>' +
          '<button class="btn btn-primary btn-block btn-lg" data-a="rcOk">' + ic('check') + 'Yes, I\'m fine</button>' +
          '<button class="btn btn-danger btn-block btn-lg" data-a="rcHelp">' + ic('phone') + 'Call for help</button></div></div>';
      } else if (u.modal) {
        const d = r && r.driverId && byId(s, r.driverId);
        if (u.modal === 'share' && r) {
          key = 'share:' + (r.share && r.share.code);
          html = () => this.sheetModal('Share my trip', r.share ? '<div class="share-link">' + ic('link') + '<span>fuber.app/t/' + r.share.code + '</span><button class="btn btn-sm btn-soft" data-a="copyLink">' + ic('copy') + 'Copy</button></div>' +
            '<p class="tiny muted">Anyone with this link can follow your trip live until you arrive. It expires automatically.</p>' +
            '<ul class="check-list compact"><li>' + ic('pin') + 'Live location on a map</li>' + (d ? '<li>' + ic('user') + esc(d.name) + ' · ' + esc(d.vehicle.color + ' ' + d.vehicle.make + ' ' + d.vehicle.model) + ' · <b>' + esc(d.vehicle.plate) + '</b></li>' : '<li>' + ic('user') + 'Driver details once matched</li>') +
            '<li>' + ic('flag') + 'Destination: ' + esc(r.dest.name) + '</li><li>' + ic('clock') + 'Live ETA updates</li></ul>' +
            '<button class="btn btn-primary btn-block" data-a="sendContacts">' + ic('users') + 'Send to my emergency contacts</button>' : '<div class="spinner"></div>');
        } else if (u.modal === 'call' && d) {
          key = 'call';
          vals = { callT: mmss(Date.now() - u.callStart) };
          html = () => '<div class="modal-back"><div class="modal center-modal call-modal">' + avatar(d, 84) + '<div class="h2">' + esc(d.first) + '</div><div class="small muted">FUber secure line · <span data-b="callT"></span></div>' +
            '<div class="note">' + ic('lock') + '<span>Both numbers are masked. ' + esc(d.first) + ' sees <b>+264 61 000 ' + r.pin.slice(0, 2) + '00</b>, not your number.</span></div>' +
            '<button class="btn btn-danger btn-block btn-lg" data-a="closeModal">' + ic('phone') + 'End call</button></div></div>';
        } else if (u.modal === 'chat' && d) {
          key = 'chat:' + r.messages.length;
          html = () => this.sheetModal('Message ' + esc(d.first), '<div class="chat">' + (r.messages.length ? r.messages.map((m) => '<div class="bubble ' + (m.from === 'rider' ? 'me' : '') + '">' + esc(m.text) + '</div>').join('') : '<div class="tiny muted center">Messages are sent through FUber. Your number stays private.</div>') + '</div>' +
            '<div class="quick-msgs">' + QUICK_MSGS.map((q) => '<button class="tag" data-a="quickMsg" data-v="' + esc(q) + '">' + esc(q) + '</button>').join('') + '</div>' +
            '<div class="row gap"><input class="input grow" placeholder="Type a message" data-m="chat" value="' + esc(u.chat) + '"><button class="btn btn-primary" data-a="sendMsg">Send</button></div>');
        } else if (u.modal === 'cancel' && r) {
          const assigned = !!r.driverId;
          key = 'cancel';
          html = () => this.sheetModal('Cancel this ride?', '<p class="small">' + (assigned ? 'Your driver is already on her way to you.' : 'We are still finding your driver.') + ' There is no charge for cancelling.' + '</p>' +
            '<div class="row gap"><button class="btn btn-soft grow" data-a="closeModal">Keep ride</button><button class="btn btn-danger grow" data-a="cancelRide">Cancel ride</button></div>');
        } else { u.modal = null; }
      }
      patch(this.overlay, key, html);
      bind(this.overlay, vals);
    }

    sheetModal(title, body) {
      return '<div class="modal-back" data-a="closeModal" data-self><div class="modal sheet-modal"><div class="modal-head"><b>' + title + '</b><button class="icon-btn sm" data-a="closeModal">' + ic('x') + '</button></div>' + body + '</div></div>';
    }

    // ------------------------------------------------------------------------
    // Events
    // ------------------------------------------------------------------------
    onInput(e) {
      const el = e.target;
      if (el.dataset.m) {
        setPath(this.ui, el.dataset.m, el.type === 'checkbox' ? el.checked : el.value);
        if (el.hasAttribute('data-results')) this.content.querySelector('.results').innerHTML = this.resultsHtml(Store.state);
      }
    }

    async onChange(e) {
      const el = e.target;
      if (el.dataset.live === 'wav') { this.set({ wav: el.checked }); return; }
      if (el.dataset.consent) { this.act('R_SET_CONSENT', { key: el.dataset.consent, value: el.checked }); return; }
      if (el.dataset.file) {
        const img = await Hera.readImage(el.files[0], el.dataset.file === 'photo' ? 128 : 96);
        const f = el.dataset.file;
        if (f === 'photo') { this.ui.signup.photo = img; this.ui.v++; this.render(Store.state); return; }
        this.ui[f === 'id' ? 'idPreview' : 'selfiePreview'] = img;
        this.ui.v++;
        this.act('R_UPLOAD', { doc: f });
      }
    }

    async onClick(e) {
      const el = e.target.closest('[data-a]');
      if (!el || !this.phone.contains(el)) return;
      if (el.hasAttribute('data-self') && e.target !== el) return;
      const a = el.dataset.a, v = el.dataset.v, s = Store.state, u = this.ui;
      switch (a) {
        case 'goto': this.act('R_GOTO', { step: v }); break;
        case 'demoLogin': this.act('R_DEMO_LOGIN'); break;
        case 'fillSignup':
          u.signup = { name: 'Ndeshi Amutenya', contact: '+264 81 234 5678', password: 'hera-demo', photo: u.signup.photo };
          this.set({});
          break;
        case 'signup': {
          const f = u.signup;
          if (!f.name.trim() || !f.contact.trim()) return toast(this.phone, 'Please enter your name and phone or email', 'err');
          if ((f.password || '').length < 6) return toast(this.phone, 'Password must be at least 6 characters', 'err');
          this.act('R_SIGNUP', { name: f.name.trim(), contact: f.contact.trim(), photo: f.photo });
          break;
        }
        case 'sampleDoc':
          u[v === 'id' ? 'idPreview' : 'selfiePreview'] = null;
          this.act('R_UPLOAD', { doc: v });
          break;
        case 'submitVerify': this.act('R_SUBMIT_VERIFICATION'); break;
        case 'approveNow': this.act('X_APPROVE'); break;
        case 'addContactRow': u.contacts.push({ name: '', phone: '' }); this.set({}); break;
        case 'fillContacts':
          u.contacts = [{ name: 'Meme Selma (Mum)', phone: '+264 81 555 0142' }, { name: 'Taimi (sister)', phone: '+264 85 555 0199' }];
          this.set({});
          break;
        case 'saveContacts': this.act('R_SAVE_CONTACTS', { contacts: u.contacts, consent: u.consent }); break;
        case 'tab': this.set({ tab: v, flow: 'idle' }); break;
        case 'flow': this.set({ flow: v, tab: 'home' }); break;
        case 'gps': this.act('R_PICKUP_GPS'); break;
        case 'pickResult': {
          const p = this.results && this.results[+v];
          if (p) this.set({ dest: { x: p.x, y: p.y, name: p.name }, flow: 'options', tab: 'home', when: 'now' });
          break;
        }
        case 'pickPlace': {
          const p = E.PLACES.find((x) => x.id === v);
          this.set({ dest: { x: p.x, y: p.y, name: p.name }, flow: 'options', tab: 'home', when: 'now' });
          break;
        }
        case 'opt': this.set({ option: v }); break;
        case 'rideFor': this.set({ forOther: v === 'other' }); break;
        case 'seats': this.set({ seats: Number(v) }); break;
        case 'fillGuest': this.set({ guest: { name: 'Ndapandula Shikongo', phone: '+264 81 555 0177' } }); break;
        case 'when': this.set({ when: v }); break;
        case 'cyclePay': {
          const keys = Object.keys(PAY), cur = u.payMethod || s.rider.payment;
          this.set({ payMethod: keys[(keys.indexOf(cur) + 1) % keys.length] });
          break;
        }
        case 'request': {
          if (u.forOther && (!u.guest.name.trim() || !u.guest.phone.trim())) return toast(this.phone, "Enter the passenger's name and phone number", 'err');
          const passenger = u.forOther ? { name: u.guest.name.trim(), phone: u.guest.phone.trim() } : null;
          const payload = { option: u.option, wav: u.wav, seats: u.seats, dest: u.dest, payment: u.payMethod || s.rider.payment, passenger };
          if (u.when === 'later') {
            const r = await this.act('R_SCHEDULE', { ...payload, when: u.schedAt });
            if (r.ok) this.set({ flow: 'idle', tab: 'trips', dest: null });
          } else {
            this.act('R_REQUEST', payload);
          }
          break;
        }
        case 'cancelRide': u.modal = null; this.act('R_CANCEL'); break;
        case 'retry': this.act('R_RETRY'); break;
        case 'finish': this.act('R_FINISH'); break;
        case 'modal': this.set({ modal: v }); break;
        case 'closeModal': this.set({ modal: null }); break;
        case 'share': this.act('R_SHARE'); this.set({ modal: 'share' }); break;
        case 'copyLink':
          try { await navigator.clipboard.writeText('https://fuber.app/t/' + s.ride.share.code); } catch (err) { /* clipboard may be blocked */ }
          toast(this.phone, 'Link copied', 'ok');
          break;
        case 'sendContacts': toast(this.phone, 'Trip link sent to ' + s.rider.contacts.length + ' emergency contact' + (s.rider.contacts.length === 1 ? '' : 's'), 'ok'); this.set({ modal: null }); break;
        case 'call': this.act('R_CALL'); this.set({ modal: 'call', callStart: Date.now() }); break;
        case 'quickMsg': this.act('R_MESSAGE', { text: v }); break;
        case 'sendMsg':
          if (u.chat.trim()) { this.act('R_MESSAGE', { text: u.chat.trim() }); u.chat = ''; this.set({}); }
          break;
        case 'fav': this.act('R_TOGGLE_FAV', { driverId: v }); break;
        case 'sos': this.act('R_SOS'); break;
        case 'sosCancel': this.act('R_SOS_CANCEL'); break;
        // Placeholder until a real emergency-services integration exists: just confirm the notification.
        case 'call911': if (s.sos.rider) { this.set({ ems: s.sos.rider.at }); toast(this.phone, 'Emergency services have been notified', 'ok'); } break;
        case 'rcOk': this.act('R_RIDECHECK', { answer: 'ok' }); break;
        case 'rcHelp': this.act('R_RIDECHECK', { answer: 'help' }); break;
        case 'payMethod': this.set({ payMethod: v }); break;
        case 'star': u.rating.stars = Number(v); this.set({}); break;
        case 'tag': {
          const t = u.rating.tags, i = t.indexOf(v);
          if (i >= 0) t.splice(i, 1); else t.push(v);
          this.set({});
          break;
        }
        case 'payRate': {
          const r = s.ride;
          this.act('R_PAY_RATE', { method: u.payMethod || r.payment, stars: u.rating.stars, tags: u.rating.tags, comment: u.rating.comment, favourite: u.rating.favourite });
          break;
        }
        case 'cancelSched': this.act('R_CANCEL_SCHEDULED', { id: v }); break;
        case 'dispatchSched': this.act('R_DISPATCH_SCHEDULED', { id: v }); break;
        case 'removeContact': this.act('R_REMOVE_CONTACT', { index: Number(v) }); break;
        case 'addContact': {
          const c = u.newContact;
          if (!c.name.trim() || !c.phone.trim()) return toast(this.phone, 'Enter a name and phone number', 'err');
          const r = await this.act('R_ADD_CONTACT', { name: c.name.trim(), phone: c.phone.trim() });
          if (r.ok) { u.newContact = { name: '', phone: '' }; this.set({}); }
          break;
        }
        case 'setPayment': this.act('R_SET_PAYMENT', { method: v }); break;
        case 'signOut': this.act('R_SIGN_OUT'); break;
      }
    }
  }

  window.RiderApp = RiderApp;
})();
