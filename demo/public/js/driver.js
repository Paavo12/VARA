/* Driver portal */
(function () {
  'use strict';
  const { E, Store, esc, money, mins, km, arriveAt, mmss, firstName, range, ic, stars, avatar, carSVG, readImage, patch, bind, toast, statusBanner, emsConfirm } = Hera;

  const byId = (s, id) => s.drivers.find((d) => d.id === id);
  // This screen's own sign-in; other phones running the driver app have their own.
  const BLANK_PORTAL = E.seedDriverPortal();
  const portal = (s) => (s.driverPortals && s.driverPortals[Store.sid]) || BLANK_PORTAL;
  const signedInElsewhere = (s, driverId) => Object.entries(s.driverPortals || {}).some(([k, p]) => k !== Store.sid && p.signedInId === driverId);
  const DOCS = {
    identity: { icon: 'idcard', title: 'Identity & gender verification', sub: 'Government ID + live selfie' },
    licence: { icon: 'car', title: "Driver's licence", sub: 'Full licence, held for 2+ years' },
    insurance: { icon: 'shield', title: 'Insurance certificate', sub: 'Commercial hire & reward cover' },
    background: { icon: 'shieldCheck', title: 'Criminal record check', sub: 'Enhanced background check' },
  };
  const COLORS = [['Pearl White', '#f1ede4'], ['Silver', '#c9ccd1'], ['Graphite', '#5b5f66'], ['Midnight Blue', '#26375f'], ['Ruby Red', '#a3243b'], ['Lavender', '#b9a3e3'], ['Mint Green', '#9fd8c0']];
  const CANCEL_REASONS = ['Rider asked me to cancel', 'Vehicle problem', 'I feel unsafe', 'Too far / heavy traffic'];
  const REPORT_REASONS = ['Rider was abusive or threatening', 'Rider was not the account holder', 'Rider damaged the vehicle', 'Other safety concern'];
  const RATE_TAGS = ['Respectful', 'On time', 'Friendly', 'Clear pickup spot'];
  const QUICK_MSGS = ["I've arrived", "I'm 2 minutes away", 'Traffic is heavy, on my way', 'Thank you!'];
  const TURN_ICON = { left: 'turnLeft', right: 'turnRight', uturn: 'uturn', arrive: 'flag' };
  const DAYS = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'];

  function setPath(obj, path, val) {
    const parts = path.split('.');
    let o = obj;
    while (parts.length > 1) o = o[parts.shift()];
    o[parts[0]] = val;
  }
  // Ride-option picker: higher tiers charge riders more, so the driver sees the rates up front.
  function tierPicker(selected) {
    return '<div class="opt-list">' + Object.values(E.OPTIONS).map((o) => '<button class="opt-card" data-a="vtype" data-v="' + o.id + '"' + (o.id === selected ? ' aria-pressed="true"' : '') + '>' +
      '<div class="opt-main"><div class="opt-name">' + esc(o.name) + '</div><div class="tiny muted">' + esc(o.blurb) + '</div></div>' +
      '<div class="opt-price">from ' + money(o.min) + '<span class="tiny muted">' + money(o.perKm) + '/km · ' + o.serviceFee * 100 + '% fee</span></div></button>').join('') + '</div>' +
      '<p class="tiny muted">The higher the option, the higher the fare riders pay — and the more you earn per trip.</p>';
  }

  // The person being picked up: the passenger when the ride was booked for someone else.
  function paxName(s) {
    const p = s.ride && s.ride.passenger;
    return firstName(p ? p.name : s.rider.profile.name);
  }

  const seatsLabel = (r) => (r.seats || 1) + ' passenger' + ((r.seats || 1) > 1 ? 's' : '');
  // A guest booked by someone else has no account: she is not ID-verified and has no rating of her own.
  const riderBadge = (s, r) => (r && r.passenger
    ? ic('user', 'ic-xs') + 'Guest · booked by ' + esc(firstName(s.rider.profile.name)) + ' (verified)'
    : ic('star', 'star-ic', true) + s.rider.rating.toFixed(2) + ' · ' + ic('shieldCheck', 'ic-xs') + 'Verified');
  // The fare is locked at request, so the driver's take is known before she accepts.
  const pinLeftText = (r) => { const n = E.CFG.PIN_MAX_ATTEMPTS - r.pinErrors; return n + (n === 1 ? ' try' : ' tries') + ' left. Ask the rider to read it again'; };
  const earnFor = (r) => r2(r.estimate.total * (1 - E.OPTIONS[r.option].serviceFee));

  function getPath(obj, path) { return path.split('.').reduce((o, k) => (o == null ? o : o[k]), obj); }

  /** Cut a path where it first enters the approximate pickup circle (privacy). */
  function truncateToCircle(path, c) {
    const out = [path[0]];
    for (let i = 1; i < path.length; i++) {
      const a = path[i - 1], b = path[i], L = E.dist(a, b);
      for (let t = 25; t < L; t += 25) {
        const p = { x: a.x + (b.x - a.x) * t / L, y: a.y + (b.y - a.y) * t / L };
        if (E.dist(p, c) <= c.r) { out.push(p); return out; }
      }
      out.push(b);
      if (E.dist(b, c) <= c.r) return out;
    }
    return out;
  }

  class DriverApp {
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
      phone.addEventListener('pointerdown', (e) => this.onSlideStart(e));
    }

    resetUi() {
      this.ui = {
        v: 0, tab: 'drive', collapsed: false, pin: '', pinShake: 0, modal: null, chat: '', callStart: 0,
        signup: { name: '', contact: '', password: '', photo: null }, demoDriver: E.DEMO_DRIVER_IDS[0],
        vehicle: { type: 'classic', make: '', model: '', year: '2022', color: 'Lavender', hex: '#b9a3e3', plate: '', wav: false, photo: null },
        rating: { stars: 0, tags: [] },
      };
    }

    set(p) { Object.assign(this.ui, p); this.ui.v++; this.render(Store.state); }

    async act(type, payload) {
      const r = await Store.send(type, payload);
      if (r && !r.ok) toast(this.phone, r.error, 'err');
      return r || { ok: false };
    }

    viewId(s) { return portal(s).signedInId; }

    // ------------------------------------------------------------------------
    render(s) {
      if (!s) return;
      if (this.epoch !== s.startedWall) {
        this.epoch = s.startedWall;
        this.resetUi();
        this.lastNotice = s.noticeSeq;
      }
      this.s = s;
      const vid = this.viewId(s);
      this.vid = vid;
      const rideKey = s.ride ? s.ride.id + s.ride.state : '';
      if (rideKey !== this.lastRideKey) {
        this.lastRideKey = rideKey;
        this.ui.pin = '';
        if (s.ride && s.ride.state === 'COMPLETED') this.ui.rating = { stars: 0, tags: [] };
        if (this.ui.modal === 'cancel' || this.ui.modal === 'report') this.ui.modal = null;
      }
      const scr = this.screen(s, vid);
      patch(this.content, scr.key + '|' + this.ui.v + '|' + vid, scr.html);
      bind(this.content, scr.vals);
      this.mapHost.hidden = !scr.map;
      if (scr.map) this.map.update(scr.map);
      this.renderOverlay(s, vid);
      for (const n of s.notices) {
        if (n.id > this.lastNotice) {
          this.lastNotice = n.id;
          if ((n.to === 'driver' && n.driverId === vid) || n.to === 'all') toast(this.phone, n.text, n.kind);
        }
      }
    }

    screen(s, vid) {
      const dp = portal(s);
      if (!vid || dp.step === 'approved') return this.onboarding(s);
      const d = byId(s, vid), r = s.ride;
      if (r && r.driverId === d.id && !r.driverDone) return this.rideScreen(s, r, d);
      if (this.ui.tab === 'earnings') return this.earnings(s, d);
      if (this.ui.tab === 'account') return this.account(s, d);
      if (d.hold) return this.onHold(s, d);
      return d.online ? this.market(s, d) : this.offline(s, d);
    }

    sos() { return '<button class="sos-btn" data-a="sos" title="Emergency SOS">' + ic('shield') + 'SOS</button>'; }

    tabbar(active) {
      const t = (id, icon, label) => '<button class="tab' + (active === id ? ' on' : '') + '" data-a="tab" data-v="' + id + '">' + ic(icon) + '<span>' + label + '</span></button>';
      return '<nav class="tabbar">' + t('drive', 'car', 'Drive') + t('earnings', 'chart', 'Earnings') + t('account', 'user', 'Account') + '</nav>';
    }

    topbar(s, d) {
      return '<div class="float-top driver-top">' + avatar(d, 38) + '<div class="grow"><b>' + esc(d.short) + '</b><div class="tiny muted">' + ic('star', 'star-ic', true) + d.rating.toFixed(2) + ' · ' + esc(E.OPTIONS[d.vehicle.type].short) + ' · ' + esc(d.vehicle.plate) + '</div></div>' +
        '<button class="online-pill' + (d.online ? ' on' : '') + '" data-a="' + (d.online ? 'offline' : 'online') + '"><i></i>' + (d.online ? 'Online' : 'Offline') + '</button></div>';
    }

    heat(s) {
      const k = s.settings.demand === 'normal' ? 1 : s.settings.demand === 'high' ? 1.3 : 1.6;
      const spots = E.HOTSPOTS.map((h) => ({ x: h.x, y: h.y, r: Math.round(500 + h.w * 220 * k), o: Math.min(1, 0.45 + h.w * 0.12 * k) }));
      return spots;
    }

    // ------------------------------------------------------------------------
    // Onboarding
    // ------------------------------------------------------------------------
    onboarding(s) {
      const dp = portal(s), u = this.ui;
      const head =(back, n) => '<div class="page-head"><button class="icon-btn" data-a="goto" data-v="' + back + '">' + ic('chevronLeft') + '</button>' +
        '<div class="steps">' + [1, 2, 3, 4].map((i) => '<i class="' + (i <= n ? 'on' : '') + '"></i>').join('') + '</div><span class="tiny muted">Step ' + n + ' of 4</span></div>';
      const field = (label, path, type, ph) => '<label class="field"><span>' + label + '</span><input class="input" type="' + (type || 'text') + '" placeholder="' + esc(ph || '') + '" data-m="' + path + '" value="' + esc(getPath(u, path) || '') + '"></label>';
      switch (dp.step) {
        case 'welcome':
          return {
            key: 'd-welcome',
            html: () => '<div class="page onb-welcome driver-welcome"><div class="welcome-art"><div class="brand-orb">' + ic('car') + '</div></div><div class="brand-word">VARA <span>Driver</span></div>' +
              '<h1 class="h1">Drive on your terms,<br>with women riders only.</h1>' +
              '<p class="muted">Every rider is ID- and selfie-verified. You see the exact pickup only on arrival, and the destination only after PIN check — so no rider is ever cherry-picked.</p>' +
              '<ul class="trust-list"><li>' + ic('shieldCheck') + '<span><b>Verified women riders</b><br>Two-way ratings after every trip</span></li>' +
              '<li>' + ic('alert') + '<span><b>Driver safety tools</b><br>SOS, route sharing, report a rider</span></li>' +
              '<li>' + ic('wallet') + '<span><b>Keep 82–85% of every fare</b><br>15–18% service fee per trip, no other fees</span></li></ul>' +
              '<div class="stack-sm push-bottom"><button class="btn btn-primary btn-block" data-a="goto" data-v="signup">Apply to drive</button>' +
              '<button class="btn btn-ghost btn-block" data-a="goto" data-v="demo">Use demo account (skip onboarding)</button></div></div>',
          };
        case 'demo': {
          const demo = E.DEMO_DRIVER_IDS.map((id) => byId(s, id)).filter(Boolean);
          const taken = demo.map((d) => signedInElsewhere(s, d.id));
          return {
            key: 'd-demo:' + taken.join(),
            html: () => '<div class="page"><div class="page-head"><button class="icon-btn" data-a="goto" data-v="welcome">' + ic('chevronLeft') + '</button></div>' +
              '<h2 class="h2">Demo driver account</h2><p class="small muted">Pick a verified driver, one per phone, and the ride option she drives for.</p>' +
              '<div class="eyebrow">Driver</div><div class="opt-list">' + demo.map((d, i) => '<button class="opt-card" data-a="demoDriver" data-v="' + d.id + '"' + (d.id === u.demoDriver ? ' aria-pressed="true"' : '') + '>' +
                avatar(d, 38) + '<div class="opt-main"><div class="opt-name">' + esc(d.name) + '</div><div class="tiny muted">' + esc(d.vehicle.color + ' ' + d.vehicle.make + ' ' + d.vehicle.model) + ' · ' + esc(d.vehicle.plate) + '</div></div>' +
                '<div class="opt-price">' + ic('star', 'star-ic', true) + d.rating.toFixed(2) + '<span class="tiny muted">' + (taken[i] ? 'On another phone' : d.online ? 'Online' : 'Offline') + '</span></div></button>').join('') + '</div>' +
              (taken[demo.findIndex((d) => d.id === u.demoDriver)] ? '<p class="tiny muted">She is signed in on another phone. Signing in here signs her out there.</p>' : '') +
              '<div class="eyebrow">Ride option</div>' + tierPicker(u.vehicle.type) +
              '<div class="stack-sm push-bottom"><button class="btn btn-primary btn-block" data-a="demoLogin">Sign in as ' + esc(firstName((byId(s, u.demoDriver) || {}).name)) + '</button></div></div>',
          };
        }
        case 'signup':
          return {
            key: 'd-signup',
            html: () => '<div class="page">' + head('welcome', 1) + '<h2 class="h2">Create your driver account</h2>' +
              '<label class="photo-pick">' + (u.signup.photo ? '<img src="' + u.signup.photo + '" alt="">' : '<span>' + ic('camera') + '</span>') +
              '<input type="file" accept="image/*" data-file="photo" hidden><em>' + (u.signup.photo ? 'Change photo' : 'Add profile photo') + '</em></label>' +
              field('Full name', 'signup.name', 'text', 'e.g. Rauna Shilongo') + field('Phone or email', 'signup.contact', 'text', '+264 81 123 4567') + field('Password', 'signup.password', 'password', 'At least 6 characters') +
              '<div class="stack-sm push-bottom"><button class="btn btn-primary btn-block" data-a="signup">Continue</button><button class="btn btn-link" data-a="fillSignup">Fill sample details</button></div></div>',
          };
        case 'docs': {
          const allUp = E.DRIVER_DOCS.every((k) => dp.docs[k].status !== 'none');
          return {
            key: 'd-docs:' + E.DRIVER_DOCS.map((k) => dp.docs[k].status).join(),
            html: () => '<div class="page">' + head('signup', 2) + '<h2 class="h2">Verification documents</h2>' +
              '<p class="small muted">VARA is women-only. Every document is reviewed by our onboarding team. In this demo, tap Approve on a document in review.</p>' +
              E.DRIVER_DOCS.map((k) => {
                const m = DOCS[k], st = dp.docs[k].status;
                return '<div class="upload-tile' + (st === 'approved' ? ' done' : '') + '"><div class="ut-icon">' + ic(st === 'approved' ? 'check' : m.icon) + '</div><div class="ut-main"><b>' + m.title + '</b><div class="tiny muted">' + m.sub + '</div></div>' +
                  (st === 'none' ? '<div class="ut-actions"><label class="btn btn-sm btn-soft">' + ic('upload') + 'Upload<input type="file" accept="image/*,application/pdf" data-doc="' + k + '" hidden></label><button class="btn btn-sm btn-link" data-a="uploadDoc" data-v="' + k + '">Sample</button></div>'
                    : st === 'review' ? '<div class="ut-actions"><span class="ut-review">' + ic('clock') + 'In review</span><button class="btn btn-sm btn-link" data-a="approveDoc" data-v="' + k + '">Approve</button></div>' :'<span class="ut-ok">' + ic('check') + 'Approved</span>') + '</div>';
              }).join('') +
              '<div class="stack-sm push-bottom"><button class="btn btn-primary btn-block" data-a="docsNext"' + (allUp ? '' : ' disabled') + '>Continue to vehicle details</button>' +
              (allUp ? '' : '<button class="btn btn-link" data-a="allDocs">Upload all samples</button>') + '</div></div>',
          };
        }
        case 'vehicle': {
          const v = u.vehicle;
          return {
            key: 'd-vehicle',
            html: () => '<div class="page">' + head('docs', 3) + '<h2 class="h2">Your vehicle</h2>' +
              '<div class="veh-preview">' + (v.photo ? '<img src="' + v.photo + '" alt="">' : carSVG(v.hex, v.wav)) + '<span class="plate">' + esc(v.plate || 'PLATE') + '</span></div>' +
              '<div class="eyebrow">Ride option</div>' + tierPicker(v.type) +
              '<div class="grid2">' + field('Make', 'vehicle.make', 'text', 'Toyota') + field('Model', 'vehicle.model', 'text', 'Corolla') + field('Year', 'vehicle.year', 'number', '2022') + field('Plate number', 'vehicle.plate', 'text', 'N 1234 W') + '</div>' +
              '<div class="field"><span>Colour · ' + esc(v.color) + '</span><div class="swatches">' + COLORS.map(([n, h]) => '<button class="swatch' + (v.hex === h ? ' on' : '') + '" style="background:' + h + '" data-a="vcolor" data-v="' + n + '|' + h + '" title="' + n + '"></button>').join('') + '</div></div>' +
              '<label class="toggle-row"><span>' + ic('wheelchair') + ' Wheelchair-accessible vehicle</span><input type="checkbox" class="switch" data-m="vehicle.wav" data-rerender' + (v.wav ? ' checked' : '') + '></label>' +
              '<label class="btn btn-soft btn-block">' + ic('camera') + (v.photo ? 'Change vehicle photo' : 'Add vehicle photo (optional)') + '<input type="file" accept="image/*" data-file="vphoto" hidden></label>' +
              '<div class="stack-sm push-bottom"><button class="btn btn-primary btn-block" data-a="saveVehicle">Submit application</button><button class="btn btn-link" data-a="fillVehicle">Fill sample vehicle</button></div></div>',
          };
        }
        case 'review': {
          const done = E.DRIVER_DOCS.filter((k) => dp.docs[k].status === 'approved').length;
          return {
            key: 'd-review:' + done,
            html: () => '<div class="page center-page"><div class="big-check muted-check">' + ic('clock') + '</div><div class="h2">Application in review</div>' +
              '<p class="small muted">' + done + ' of ' + E.DRIVER_DOCS.length + ' documents approved · vehicle inspection booked</p>' +
              '<ul class="check-list">' + E.DRIVER_DOCS.map((k) => '<li class="' + (dp.docs[k].status === 'approved' ? '' : 'pending') + '">' + ic(dp.docs[k].status === 'approved' ? 'check' : 'clock') + DOCS[k].title + '</li>').join('') + '</ul>' +
              '<button class="btn btn-primary btn-block" data-a="approveNow">' + ic('shieldCheck') + 'Approve application</button></div>',
          };
        }
        case 'approved': {
          const d = byId(s, dp.signedInId);
          return {
            key: 'd-approved',
            html: () => '<div class="page center-page"><div class="big-check">' + ic('check') + '</div><div class="h2">You\'re approved, ' + esc(d.first) + '!</div>' +
              '<p class="small muted">Your documents are verified and your vehicle is active. Go online whenever you\'re ready.</p>' +
              '<div class="card veh-card">' + carSVG(d.vehicle.hex, d.vehicle.wav) + '<div><b>' + esc(d.vehicle.color + ' ' + d.vehicle.make + ' ' + d.vehicle.model) + '</b><div class="tiny muted">' + esc(E.OPTIONS[d.vehicle.type].name) + ' · ' + d.vehicle.year + '</div><span class="plate">' + esc(d.vehicle.plate) + '</span></div></div>' +
              '<button class="btn btn-primary btn-block btn-lg" data-a="goto" data-v="done">Start driving</button></div>',
          };
        }
        default:
          return { key: 'd-none', html: () => '' };
      }
    }

    // ------------------------------------------------------------------------
    // Home / marketplace
    // ------------------------------------------------------------------------
    offline(s, d) {
      return {
        key: 'off:' + d.id,
        map: { fit: [d], minSpan: 5200, heat: this.heat(s), cars: [{ id: d.id, x: d.x, y: d.y, h: d.heading, kind: 'muted' }], pad: { t: 100, r: 20, b: 380, l: 20 } },
        vals: { today: money(d.stats.today), trips: String(d.stats.trips), hrs: (d.stats.onlineMs / 3600000).toFixed(1) + ' h' },
        html: () => this.sos() + this.topbar(s, d) +
          '<div class="sheet with-tabs"><div class="h2">You\'re offline</div><p class="small muted">Go online to receive ride requests from verified women riders nearby.</p>' +
          '<button class="go-online" data-a="online">' + ic('power') + '<span>Go online</span></button>' +
          '<div class="stat-grid"><div><b data-b="today"></b><span>Today</span></div><div><b data-b="trips"></b><span>Trips</span></div><div><b data-b="hrs"></b><span>Online</span></div></div>' +
          '<div class="note">' + ic('flame') + '<span>Demand is highest around <b>Post Street Mall</b> and <b>Eros</b> right now.</span></div></div>' + this.tabbar('drive'),
      };
    }

    market(s, d) {
      const r = s.ride, u = this.ui;
      const offer = r && r.state === 'SEARCHING' && r.search.offer;
      const mine = offer && offer.driverId === d.id;
      const other = r && r.state === 'SEARCHING' && !mine && E.dist(d, r.pickup) <= E.CFG.RADIUS * 1.2;
      const vals = {};
      let realCard = '';
      if (r && (mine || other)) {
        const rd = s.rider;
        const roadM = E.roadDist(d, r.pickup);
        const etaMin = roadM / E.speedMs(s) / 60;
        vals.reqDist = km(roadM) + ' · ' + mins(etaMin);
        if (mine) {
          vals.offerSec = Math.max(0, Math.ceil((offer.expiresAt - s.realT) / 1000)) + ' s';
          vals.offerPct = Math.max(0, 100 - ((s.realT - offer.startedAt) / E.CFG.OFFER_MS) * 100) + '%';
        }
        realCard = '<div class="req-card' + (mine ? ' offer' : ' muted-card') + '">' +
          (mine ? '<div class="offer-top"><span class="eyebrow">New request · ' + esc(E.OPTIONS[r.option].short) + (r.wav ? ' · ' + ic('wheelchair') : '') + '</span><b class="offer-sec" data-b="offerSec"></b></div><div class="offer-timer"><div data-bw="offerPct"></div></div>'
            : '<div class="offer-top"><span class="eyebrow">Being offered to a better-matched driver</span></div>') +
          '<div class="req-rider"><span class="initial">' + esc(paxName(s)[0]) + '</span><div class="grow"><b>' + esc(paxName(s)) + '</b>' + (r.passenger ? '' : ' <span class="tiny">' + ic('star', 'star-ic', true) + rd.rating.toFixed(2) + '</span>') +
          '<div class="tiny muted">' + ic('shieldCheck', 'ic-xs') + (r.passenger ? 'Booked by ' + esc(firstName(rd.profile.name)) + ' (verified)' : 'Verified rider') + ' · ' + seatsLabel(r) + '</div></div>' +
          '<div class="req-fare"><b>' + money(r.estimate.total) + '</b><div class="tiny muted">you earn ' + money(earnFor(r)) + '</div></div></div>' +
          '<div class="req-loc">' + ic('pin') + '<div><b>' + esc(r.pickup.near) + '</b><div class="tiny muted">Approximate pickup area</div></div></div>' +
          '<div class="req-grid"><div><span class="tiny muted">To pickup</span><b data-b="reqDist"></b></div><div><span class="tiny muted">Trip length</span><b>~' + mins(r.estimate.durMin) + '</b></div><div><span class="tiny muted">Destination</span><b>' + ic('lock', 'ic-xs') + 'Hidden</b></div></div>' +
          (mine ? '<div class="privacy-note">' + ic('lock') + 'Exact pickup is revealed within 50 m, destination after PIN check.</div>' +
            '<div class="row gap"><button class="btn btn-soft grow" data-a="respond" data-v="0">Decline</button><button class="btn btn-primary grow btn-lg" data-a="respond" data-v="1">Accept</button></div>' : '') + '</div>';
      }
      const key = 'mkt:' + d.id + ':' + (mine ? 'offer' + r.search.offers.length : other ? 'other' : '') + ':' + s.settings.demand + ':' + u.collapsed;
      const approx = r && (mine || other) ? r.approx : null;
      return {
        key,
        map: {
          fit: [d, ...(approx ? [approx] : [])], minSpan: 4600, heat: this.heat(s), approx,
          cars: [{ id: d.id, x: d.x, y: d.y, h: d.heading, kind: 'me' }], pad: { t: 100, r: 20, b: u.collapsed ? 200 : 470, l: 20 },
        },
        vals,
        html: () => this.sos() + this.topbar(s, d) +
          '<div class="sheet with-tabs market' + (u.collapsed ? ' collapsed' : '') + '">' +
          '<button class="sheet-grab" data-a="collapse">' + (u.collapsed ? 'Show requests' : 'Show map') + '</button>' +
          '<div class="between"><div><div class="h3"><span class="live-dot"></span>You\'re online</div><div class="tiny muted">Requests near you · approximate areas only</div></div>' +
          (s.surge > 1 ? '<span class="badge-surge">' + ic('zap') + s.surge.toFixed(1) + '×</span>' : '<span class="chip">' + ic('flame') + 'Normal</span>') + '</div>' +
          realCard +
          (!realCard ? '<div class="empty small muted"><span class="spinner xs"></span>Looking for requests nearby…</div>' : '') +
          '</div>' + this.tabbar('drive'),
      };
    }

    // ------------------------------------------------------------------------
    // Active trip
    // ------------------------------------------------------------------------
    navBanner(d, title) {
      const t = E.nextTurn(d.path, d.pathPos);
      const txt = title || (t.dir === 'arrive' ? 'Continue to the ' + (this.s.ride.state === 'IN_TRIP' ? 'destination' : 'pickup area') : (t.dir === 'uturn' ? 'Make a U-turn onto ' : 'Turn ' + t.dir + ' onto ') + t.street);
      return { html: '<div class="nav-banner"><div class="nav-ic">' + ic(title ? 'flag' : TURN_ICON[t.dir] || 'straight') + '</div><div class="grow"><div class="nav-dist" data-b="navDist"></div><div class="nav-txt">' + esc(txt) + '</div>' + (t.current && !title ? '<div class="tiny nav-on">on ' + esc(t.current) + '</div>' : '') + '</div></div>', key: t.dir + t.street + (title || ''), dist: title ? '' : km(t.inM) };
    }

    rideScreen(s, r, d) {
      switch (r.state) {
        case 'DRIVER_ASSIGNED': case 'DRIVER_EN_ROUTE': return this.toPickup(s, r, d);
        case 'DRIVER_ARRIVED': return this.atPickup(s, r, d);
        case 'PIN_VERIFIED': case 'IN_TRIP': return this.onTrip(s, r, d);
        case 'COMPLETED': case 'RATED': return this.tripDone(s, r, d);
        case 'BLOCKED': return this.tripBlocked(s, r);
        default: return this.tripCancelled(s, r, d);
      }
    }

    riderLine(s, label) {
      const rd = s.rider;
      return '<div class="req-rider"><span class="initial">' + esc(paxName(s)[0]) + '</span><div class="grow"><div class="tiny muted">' + label + '</div><b>' + esc(paxName(s)) + '</b> <span class="tiny">' + riderBadge(s, s.ride) + ' · ' + seatsLabel(s.ride) + '</span></div>';
    }

    toPickup(s, r, d) {
      const assigned = r.state === 'DRIVER_ASSIGNED';
      const full = assigned || !d.path ? E.route(d, r.pickup) : E.remainingPath(d.path, d.pathPos);
      const path = truncateToCircle(full, r.approx);
      const nav = assigned ? { html: '<div class="nav-banner"><div class="nav-ic">' + ic('nav') + '</div><div class="grow"><div class="nav-txt">Calculating route to pickup area…</div></div></div>', key: 'calc' } : this.navBanner(d);
      const eta = r.eta || {};
      return {
        key: 'pick:' + r.id + ':' + r.state + ':' + nav.key,
        map: { route: path, approx: r.approx, fit: [d, { x: r.approx.x - r.approx.r, y: r.approx.y - r.approx.r }, { x: r.approx.x + r.approx.r, y: r.approx.y + r.approx.r }], cars: [{ id: d.id, x: d.x, y: d.y, h: d.heading, kind: 'me' }], minSpan: 1000, pad: { t: 200, r: 40, b: 400, l: 40 } },
        vals: { etaP: mins(eta.pickupMin), distP: eta.pickupM != null ? km(eta.pickupM) : '', navDist: nav.dist || '' },
        html: () => this.sos() + '<div class="top-stack">' + statusBanner(r.state, 'driver') + nav.html + '</div>' +
          '<div class="sheet ride-sheet">' +
          '<div class="between">' + this.riderLine(s, 'Picking up') + '</div><div class="eta-pill"><b data-b="etaP"></b><span class="tiny" data-b="distP"></span></div></div>' +
          '<div class="loc-row locked">' + ic('pin') + '<div class="grow"><div class="tiny muted">Pickup area</div><b>' + esc(r.pickup.near) + '</b><div class="small">' + esc(r.pickup.street) + ' area · ' + esc(r.pickup.area) + '</div>' +
          '<div class="tiny lock-note">' + ic('lock', 'ic-xs') + 'Exact address unlocks within ' + E.CFG.ARRIVE_M + ' m</div></div></div>' +
          '<div class="loc-row locked">' + ic('flag') + '<div class="grow"><div class="tiny muted">Destination</div><b>' + ic('lock', 'ic-xs') + 'Hidden until PIN is verified</b><div class="tiny muted">Est. trip ~' + mins(r.estimate.durMin) + ' · ' + range(r.estimate.low, r.estimate.high) + '</div></div></div>' +
          '<div class="action-row">' +
          '<button class="action" data-a="call">' + ic('phone') + '<span>Call</span></button>' +
          '<button class="action" data-a="modal" data-v="chat">' + ic('message') + '<span>Message' + (r.messages.length ? ' (' + r.messages.length + ')' : '') + '</span></button>' +
          '<button class="action" data-a="share">' + ic('route') + '<span>Share route</span></button>' +
          '<button class="action danger" data-a="modal" data-v="cancel">' + ic('x') + '<span>Cancel</span></button></div></div>',
      };
    }

    atPickup(s, r, d) {
      const waitLeft = r.wait ? E.CFG.WAIT_FREE_SIM_MS - (s.simT - r.wait.startedSim) : null;
      const expired = waitLeft != null && waitLeft <= 0;
      const u = this.ui;
      const vals = { wait: waitLeft != null ? (expired ? 'Free waiting time is over' : mmss(waitLeft) + ' free waiting left') : '', pinErr: r.pinErrors > 0 && s.realT - r.pinErrorAt < 6000, pinLeft: pinLeftText(r) };
      for (let i = 0; i < 4; i++) vals['pin' + i] = u.pin[i] || '';
      return {
        key: 'arr:' + r.id + ':' + !!r.wait + ':' + expired,
        map: { pickup: { x: r.pickup.x, y: r.pickup.y, label: r.pickup.address }, fit: [d, r.pickup], cars: [{ id: d.id, x: d.x, y: d.y, h: d.heading, kind: 'me' }], minSpan: 700, pad: { t: 200, r: 40, b: 560, l: 40 } },
        vals,
        html: () => this.sos() + '<div class="top-stack">' + statusBanner(r.state, 'driver') +
          '<div class="nav-banner arrived"><div class="nav-ic">' + ic('pin') + '</div><div class="grow"><div class="nav-txt">You\'ve arrived at the pickup</div><div class="tiny nav-on">Within ' + E.CFG.ARRIVE_M + ' m of the rider\'s pin</div></div></div></div>' +
          '<div class="sheet ride-sheet tall">' +
          '<div class="loc-row unlocked reveal">' + ic('unlock') + '<div class="grow"><div class="tiny">Exact pickup address — unlocked</div><b>' + esc(r.pickup.address) + '</b><div class="small">' + esc(r.pickup.area) + '</div></div></div>' +
          '<div class="pin-entry-wrap"><div class="between"><b>Enter ' + esc(paxName(s)) + '\'s 4-digit PIN</b><span class="tiny muted">to start the trip</span></div>' +
          '<div class="pin-entry' + (u.pinShake ? ' shake' : '') + '">' + [0, 1, 2, 3].map((i) => '<span data-b="pin' + i + '"></span>').join('') + '</div>' +
          '<div class="pin-error" data-bshow="pinErr">' + ic('alert') + 'Incorrect PIN — <span data-b="pinLeft"></span></div>' +
          '<div class="keypad">' + ['1', '2', '3', '4', '5', '6', '7', '8', '9', 'C', '0', '⌫'].map((k) => '<button data-a="key" data-v="' + k + '">' + k + '</button>').join('') + '</div></div>' +
          '<div class="loc-row locked">' + ic('flag') + '<div class="grow"><b>' + ic('lock', 'ic-xs') + 'Destination hidden until PIN is verified</b></div></div>' +
          (r.wait ? '<div class="note warn">' + ic('clock') + '<span data-b="wait"></span></div>' +
            (expired ? '<button class="btn btn-danger btn-block" data-a="noShow">Cancel — rider did not show up</button>' : '')
            : '<button class="btn btn-soft btn-block" data-a="startWait">' + ic('clock') + 'Rider not here? Start wait timer</button>') +
          '<div class="action-row"><button class="action" data-a="call">' + ic('phone') + '<span>Call</span></button><button class="action" data-a="modal" data-v="chat">' + ic('message') + '<span>Message</span></button>' +
          '<button class="action danger" data-a="modal" data-v="cancel">' + ic('x') + '<span>Cancel</span></button></div></div>',
      };
    }

    onTrip(s, r, d) {
      const verified = r.state === 'PIN_VERIFIED';
      const path = !verified && d.path ? E.remainingPath(d.path, d.pathPos) : E.route(r.pickup, r.dest);
      const remain = !verified && d.path ? d.pathLen - d.pathPos : E.polyLen(path);
      const nav = verified ? { html: '<div class="nav-banner unlocked"><div class="nav-ic">' + ic('unlock') + '</div><div class="grow"><div class="nav-txt">Destination unlocked</div><div class="tiny nav-on">Tap Start trip when the rider is in the car</div></div></div>', key: 'v' }
        : r.atDest ? this.navBanner(d, 'You\'ve arrived at ' + r.dest.name) : r.stop ? { html: '<div class="nav-banner stopped"><div class="nav-ic">' + ic('alert') + '</div><div class="grow"><div class="nav-txt">Vehicle stopped</div><div class="tiny nav-on">Simulated unexpected stop</div></div></div>', key: 'stop' } : this.navBanner(d);
      const deviated = r.offRouteM > 40 || r.rideChecks.some((c) => c.reason === 'deviation');
      const eta = r.eta || {};
      const near = r.atDest || E.dist(d, r.dest) <= 250;
      return {
        key: 'trip:' + r.id + ':' + r.state + ':' + nav.key + ':' + near + ':' + !!r.rideCheck,
        map: {
          route: path, planned: deviated ? r.planned : null, dest: { x: r.dest.x, y: r.dest.y, label: r.dest.name.length > 22 ? r.dest.name.slice(0, 21) + '…' : r.dest.name },
          fit: [d, r.dest], cars: [{ id: d.id, x: d.x, y: d.y, h: d.heading, kind: 'me' }], minSpan: 1200, pad: { t: 200, r: 40, b: 400, l: 40 },
        },
        vals: { navDist: nav.dist || '', etaT: r.atDest ? 'Arrived' : mins(eta.tripMin), distT: km(remain), slideHint: near ? 'Slide when the rider has safely left the car' : 'Available at the destination · ' + km(remain) + ' to go' },
        html: () => this.sos() + '<div class="top-stack">' + statusBanner(r.state, 'driver') + nav.html + '</div>' +
          '<div class="sheet ride-sheet">' +
          '<div class="loc-row unlocked' + (verified ? ' reveal' : '') + '">' + ic('flag') + '<div class="grow"><div class="tiny">Destination</div><b>' + esc(r.dest.name) + '</b><div class="small">' + esc(r.dest.address) + ' · ' + esc(r.dest.area) + '</div></div>' +
          '<div class="eta-pill"><b data-b="etaT"></b><span class="tiny" data-b="distT"></span></div></div>' +
          this.riderLine(s, 'Rider on board') + '</div>' +
          (r.rideCheck ? '<div class="note warn">' + ic('shieldCheck') + '<span><b>RideCheck in progress.</b> VARA is checking in with your rider (' + (r.rideCheck.reason === 'deviation' ? 'route deviation' : 'long stop') + ').</span></div>' : '') +
          '<div class="action-row">' +
          '<button class="action" data-a="share">' + ic('route') + '<span>Share my route</span></button>' +
          '<button class="action" data-a="modal" data-v="chat">' + ic('message') + '<span>Message</span></button>' +
          '<button class="action danger" data-a="modal" data-v="report">' + ic('report') + '<span>Report rider</span></button></div>' +
          (verified ? '<button class="btn btn-primary btn-block btn-lg" data-a="startTrip">' + ic('nav') + 'Start trip</button>'
            : '<div class="slide' + (near ? '' : ' disabled') + '" data-slide><div class="slide-fill"></div><div class="slide-handle">' + ic('chevronRight') + '</div><span>Slide to complete trip</span></div>' +
            '<div class="tiny muted center" data-b="slideHint"></div>' +
            (near ? '' : '<button class="btn btn-link" data-a="jumpDest">Skip ahead to destination (presenter)</button>')) + '</div>',
      };
    }

    tripDone(s, r, d) {
      const u = this.ui, f = r.fare;
      const fee = r2(f.total - r.driverEarning);
      return {
        key: 'tdone:' + r.id + ':' + !!r.driverRating + ':' + !!r.riderRating,
        html: () => this.sos() + '<div class="page done-page">' +
          '<div class="done-hero"><div class="big-check">' + ic('check') + '</div><div class="h2">Trip complete</div><div class="small muted">' + esc(r.dest.name) + ' · ' + r.distKm.toFixed(1) + ' km · ' + Math.round(r.durMin) + ' min</div></div>' +
          '<div class="card"><div class="eyebrow">Earnings</div><div class="fare-line"><span>Collect in cash</span><span>' + money(f.total) + '</span></div>' +
          '<div class="fare-line"><span>VARA service fee (' + E.OPTIONS[r.option].serviceFee * 100 + '%)</span><span>−' + money(fee) + '</span></div>' +
          '<div class="fare-line total"><span>You earned</span><span>' + money(r.driverEarning) + '</span></div><div class="tiny muted">You keep the cash. The service fee is added to your VARA balance.</div></div>' +
          (r.driverRating ? '<div class="card"><div class="fare-line"><span>You rated ' + esc(paxName(s)) + '</span><span>' + stars(r.driverRating.stars) + '</span></div></div>'
            : '<div class="card"><div class="eyebrow">Rate your rider</div><b>How was ' + esc(paxName(s)) + '?</b>' +
            '<div class="star-input">' + [1, 2, 3, 4, 5].map((n) => '<button data-a="star" data-v="' + n + '" class="' + (n <= u.rating.stars ? 'on' : '') + '">' + ic('star', '', true) + '</button>').join('') + '</div>' +
            '<div class="tag-list">' + RATE_TAGS.map((t) => '<button class="tag' + (u.rating.tags.includes(t) ? ' on' : '') + '" data-a="tag" data-v="' + esc(t) + '">' + esc(t) + '</button>').join('') + '</div>' +
            '<button class="btn btn-primary btn-block" data-a="rate"' + (u.rating.stars ? '' : ' disabled') + '>Submit rating</button></div>') +
          '<div class="card"><div class="fare-line"><span>' + esc(paxName(s)) + ' rated you</span><span>' + (r.riderRating ? stars(r.riderRating.stars) : '<span class="tiny muted">waiting for rider…</span>') + '</span></div>' +
          (r.riderRating && r.riderRating.tags.length ? '<div class="tag-list">' + r.riderRating.tags.map((t) => '<span class="tag on">' + esc(t) + '</span>').join('') + '</div>' : '') + '</div>' +
          '<button class="btn btn-dark btn-block btn-lg" data-a="finish">Back to requests</button></div>',
      };
    }

    tripBlocked(s, r) {
      return {
        key: 'tblock:' + r.id,
        html: () => this.sos() + '<div class="page done-page"><div class="done-hero"><div class="big-check muted-check">' + ic('alert') + '</div><div class="h2">Trip blocked</div>' +
          '<div class="small muted">Too many wrong PINs</div></div>' +
          '<div class="note warn">' + ic('shieldCheck') + '<span><b>Please don\'t start this trip.</b> The VARA safety desk has been alerted and will call you.</span></div>' +
          '<button class="btn btn-dark btn-block btn-lg" data-a="finish">Continue</button></div>',
      };
    }

    // She may stay online during the hold, but gets no requests until the safety desk closes the incident.
    onHold(s, d) {
      return {
        key: 'hold:' + d.id + ':' + d.online,
        map: { fit: [d], minSpan: 5200, cars: [{ id: d.id, x: d.x, y: d.y, h: d.heading, kind: 'muted' }], pad: { t: 100, r: 20, b: 380, l: 20 } },
        html: () => this.sos() + this.topbar(s, d) +
          '<div class="sheet with-tabs"><div class="h2">Requests paused</div>' +
          '<div class="note warn">' + ic('shieldCheck') + '<span>After a PIN lockout you get no new requests until the VARA safety desk has reviewed it. They will contact you.</span></div></div>' + this.tabbar('drive'),
      };
    }

    tripCancelled(s, r, d) {
      const c = r.cancel || {};
      return {
        key: 'tcan:' + r.id,
        html: () => this.sos() + '<div class="page done-page"><div class="done-hero"><div class="big-check muted-check">' + ic('x') + '</div><div class="h2">Trip cancelled</div>' +
          '<div class="small muted">' + (c.by === 'rider' ? 'The rider cancelled after you accepted' : esc(c.reason || 'Cancelled')) + '</div></div>' +
          '<button class="btn btn-dark btn-block btn-lg" data-a="finish">Back to requests</button></div>',
      };
    }

    // ------------------------------------------------------------------------
    // Tabs
    // ------------------------------------------------------------------------
    earnings(s, d) {
      const st = d.stats;
      const today = (new Date().getDay() + 6) % 7;
      const week = st.week ? st.week.slice() : [0, 0, 0, 0, 0, 0, 0];
      week[today] = st.today;
      const max = Math.max(...week, 1);
      return {
        key: 'earn:' + d.id + ':' + st.trips + ':' + st.recent.map((x) => '' + x.ratingFromRider + x.ratingGiven).join(),
        vals: { today: money(st.today), hrs: (st.onlineMs / 3600000).toFixed(1) + ' h' },
        html: () => this.sos() + '<div class="page with-tabs"><div class="h1 page-title">Earnings</div>' +
          '<div class="earn-hero"><span class="tiny">Today</span><b data-b="today"></b><span class="tiny">' + st.trips + ' trips · <span data-b="hrs"></span> online</span></div>' +
          '<div class="bars">' + week.map((v, i) => '<div class="bar' + (i === today ? ' on' : '') + '"><i style="height:' + Math.max(4, (v / max) * 100) + '%"></i><span>' + DAYS[i][0] + '</span></div>').join('') + '</div>' +
          '<div class="stat-grid"><div><b>' + d.rating.toFixed(2) + '</b><span>Your rating</span></div><div><b>' + Math.round(d.acceptProb * 100) + '%</b><span>Acceptance</span></div><div><b>' + d.ratingCount.toLocaleString() + '</b><span>Rated trips</span></div></div>' +
          '<div class="eyebrow">Recent trips</div>' +
          (st.recent.length ? st.recent.map((t) => '<div class="trip-item"><div class="ti-icon">' + ic('car') + '</div><div class="grow"><b>' + esc(t.rider) + ' → ' + esc(t.to) + '</b>' +
            '<div class="tiny">' + (t.ratingFromRider ? 'Rider rated you ' + stars(t.ratingFromRider) : '<span class="muted">Rider rating pending</span>') + '</div>' +
            '<div class="tiny muted">' + (t.ratingGiven ? 'You rated ' + t.ratingGiven + '★' : 'Not rated yet') + ' · fare ' + money(t.fare) + '</div></div><b>' + money(t.earned) + '</b></div>').join('')
            : '<div class="empty small muted">' + ic('car') + 'Trips you complete in this demo appear here with the rider\'s rating.</div>') +
          '</div>' + this.tabbar('earnings'),
      };
    }

    account(s, d) {
      const v = d.vehicle;
      return {
        key: 'acct:' + d.id,
        html: () => this.sos() + '<div class="page with-tabs"><div class="profile-head">' + avatar(d, 72) + '<div><div class="h2">' + esc(d.name) + '</div>' +
          '<div class="small muted">' + ic('star', 'star-ic', true) + d.rating.toFixed(2) + ' · ' + d.ratingCount.toLocaleString() + ' trips</div><span class="badge-verified">' + ic('shieldCheck') + 'Verified woman driver</span></div></div>' +
          '<div class="card veh-card">' + (v.photo ? '<img class="veh-photo" src="' + v.photo + '" alt="">' : carSVG(v.hex, v.wav)) + '<div><b>' + esc(v.color + ' ' + v.make + ' ' + v.model) + '</b><div class="tiny muted">' + esc(E.OPTIONS[v.type].name) + ' · ' + v.year + (v.wav ? ' · wheelchair accessible' : '') + '</div><span class="plate">' + esc(v.plate) + '</span></div></div>' +
          '<div class="eyebrow">Documents</div><ul class="check-list">' + E.DRIVER_DOCS.map((k) => '<li>' + ic('check') + DOCS[k].title + '<span class="tiny muted"> · approved</span></li>').join('') + '</ul>' +
          '<button class="btn btn-ghost btn-block" data-a="signOut">Sign out</button></div>' + this.tabbar('account'),
      };
    }

    // ------------------------------------------------------------------------
    // Overlays
    // ------------------------------------------------------------------------
    renderOverlay(s, vid) {
      const sos = s.sos.driver && (!s.sos.driver.driverId || s.sos.driver.driverId === vid) ? s.sos.driver : null;
      const u = this.ui, r = s.ride;
      const d = vid && byId(s, vid);
      let key = '', html = () => '', vals = {};
      const sheet = (title, body) => '<div class="modal-back" data-a="closeModal" data-self><div class="modal sheet-modal"><div class="modal-head"><b>' + title + '</b><button class="icon-btn sm" data-a="closeModal">' + ic('x') + '</button></div>' + body + '</div></div>';
      if (sos) {
        const ems = u.ems === sos.at;
        key = 'sos:' + sos.status + (ems ? ':ems' : '');
        vals = { sosCount: String(Math.max(1, Math.ceil((3000 - (s.realT - sos.at)) / 1000))) };
        html = () => sos.status === 'sending'
          ? '<div class="modal-back sos-back"><div class="modal center-modal sos-modal"><div class="sos-count" data-b="sosCount"></div><div class="h2">Sending SOS…</div><p class="small">Alerting the VARA safety desk with your live location.</p>' +
          '<button class="btn btn-light btn-block btn-lg" data-a="sosCancel">Cancel — I pressed by mistake</button></div></div>'
          : '<div class="modal-back sos-back"><div class="modal center-modal sos-modal sent"><div class="big-check">' + ic('check') + '</div><div class="h2">Alert sent</div>' +
          '<ul class="sos-list"><li>' + ic('shieldCheck') + '<span>VARA safety desk alerted with your live location. They will contact you</span></li><li>' + ic('users') + '<span>Your emergency contact has been notified</span></li>' +
          '<li>' + ic('pin') + '<span>Live location' + (r && r.driverId === vid ? ' and trip ' + esc(r.id) : '') + ' shared</span></li></ul>' +
          (ems ? emsConfirm() : '<button class="btn btn-danger btn-block btn-lg" data-a="call911">' + ic('phone') + 'Call emergency services</button>') +
          '<button class="btn btn-light btn-block" data-a="sosCancel">I\'m safe — close alert</button></div></div>';
      } else if (u.modal && r && d) {
        if (u.modal === 'chat') {
          key = 'chat:' + r.messages.length;
          html = () => sheet('Message ' + esc(paxName(s)), '<div class="chat">' + (r.messages.length ? r.messages.map((m) => '<div class="bubble ' + (m.from === 'driver' ? 'me' : '') + '">' + esc(m.text) + '</div>').join('') : '<div class="tiny muted center">Messages go through VARA — phone numbers stay private.</div>') + '</div>' +
            '<div class="quick-msgs">' + QUICK_MSGS.map((q) => '<button class="tag" data-a="quickMsg" data-v="' + esc(q) + '">' + esc(q) + '</button>').join('') + '</div>' +
            '<div class="row gap"><input class="input grow" placeholder="Type a message" data-m="chat" value="' + esc(u.chat) + '"><button class="btn btn-primary" data-a="sendMsg">Send</button></div>');
        } else if (u.modal === 'call') {
          key = 'call';
          html = () => '<div class="modal-back"><div class="modal center-modal call-modal"><span class="initial xl">' + esc(paxName(s)[0]) + '</span><div class="h2">Private calling is on the way</div>' +
            '<div class="note">' + ic('lock') + '<span>Calls that hide both numbers are being set up with Namibian networks. For now, message ' + esc(paxName(s)) + ' in the app.</span></div>' +
            '<button class="btn btn-primary btn-block btn-lg" data-a="modal" data-v="chat">' + ic('message') + 'Message ' + esc(paxName(s)) + '</button><button class="btn btn-ghost btn-block" data-a="closeModal">Close</button></div></div>';
        } else if (u.modal === 'cancel') {
          key = 'cancel';
          html = () => sheet('Cancel this trip?', '<p class="small muted">The request goes straight back to the pool so the rider is matched with another driver at no cost to her.</p>' +
            CANCEL_REASONS.map((x) => '<button class="list-item" data-a="cancelReason" data-v="' + esc(x) + '">' + ic('chevronRight') + '<b>' + esc(x) + '</b></button>').join('') +
            '<p class="tiny muted">Rider no-show? Start the wait timer at the pickup point — after 5 minutes you can cancel the trip.</p>');
        } else if (u.modal === 'report') {
          key = 'report';
          html = () => sheet('Report rider', '<p class="small muted">Reports go to the VARA safety desk. The rider is not told who reported her.</p>' +
            REPORT_REASONS.map((x) => '<button class="list-item" data-a="report" data-v="' + esc(x) + '">' + ic('report') + '<b>' + esc(x) + '</b></button>').join(''));
        } else if (u.modal === 'share') {
          key = 'share:' + (r.driverShare && r.driverShare.code);
          html = () => sheet('Share my route', r.driverShare ? '<div class="share-link">' + ic('link') + '<span>vara.example/r/' + r.driverShare.code + '</span><button class="btn btn-sm btn-soft" data-a="copyLink">' + ic('copy') + 'Copy</button></div>' +
            '<p class="tiny muted">Your live route, vehicle and trip ID are visible to the VARA safety desk and anyone you share this link with, until the trip ends.</p>' +
            '<button class="btn btn-primary btn-block" data-a="closeModal">Done</button>' : '<div class="spinner"></div>');
        } else { u.modal = null; }
      }
      patch(this.overlay, key, html);
      bind(this.overlay, vals);
    }

    // ------------------------------------------------------------------------
    // Events
    // ------------------------------------------------------------------------
    onInput(e) {
      const el = e.target;
      if (!el.dataset.m) return;
      setPath(this.ui, el.dataset.m, el.type === 'checkbox' ? el.checked : el.value);
      if (el.dataset.m === 'vehicle.plate') {
        const p = this.content.querySelector('.veh-preview .plate');
        if (p) p.textContent = el.value.toUpperCase() || 'PLATE';
      }
    }

    async onChange(e) {
      const el = e.target;
      if (el.hasAttribute('data-rerender')) { this.set({}); return; }
      if (el.dataset.doc) { this.act('D_UPLOAD_DOC', { doc: el.dataset.doc }); return; }
      if (el.dataset.file) {
        const img = await readImage(el.files[0], el.dataset.file === 'vphoto' ? 160 : 128);
        if (el.dataset.file === 'vphoto') this.ui.vehicle.photo = img; else this.ui.signup.photo = img;
        this.set({});
      }
    }

    onSlideStart(e) {
      const slide = e.target.closest('[data-slide]');
      if (!slide || slide.classList.contains('disabled')) return;
      const handle = slide.querySelector('.slide-handle'), fill = slide.querySelector('.slide-fill');
      const rect = slide.getBoundingClientRect();
      const scale = rect.width / slide.offsetWidth || 1;
      const max = slide.offsetWidth - handle.offsetWidth - 8;
      const x0 = e.clientX;
      let pos = 0;
      slide.setPointerCapture(e.pointerId);
      const move = (ev) => {
        pos = Math.max(0, Math.min(max, (ev.clientX - x0) / scale));
        handle.style.transform = 'translateX(' + pos + 'px)';
        fill.style.width = (pos + handle.offsetWidth) + 'px';
      };
      const up = () => {
        slide.removeEventListener('pointermove', move);
        slide.removeEventListener('pointerup', up);
        slide.removeEventListener('pointercancel', up);
        if (pos >= max * 0.85) { handle.style.transform = 'translateX(' + max + 'px)'; this.act('D_COMPLETE'); }
        else { handle.style.transform = ''; fill.style.width = ''; }
      };
      slide.addEventListener('pointermove', move);
      slide.addEventListener('pointerup', up);
      slide.addEventListener('pointercancel', up);
    }

    async onClick(e) {
      const el = e.target.closest('[data-a]');
      if (!el || !this.phone.contains(el)) return;
      if (el.hasAttribute('data-self') && e.target !== el) return;
      const a = el.dataset.a, v = el.dataset.v, s = Store.state, u = this.ui, vid = this.vid;
      switch (a) {
        case 'goto': this.act('D_GOTO', { step: v }); break;
        case 'fillSignup': u.signup = { name: 'Rauna Shilongo', contact: '+264 81 765 4321', password: 'hera-demo', photo: u.signup.photo }; this.set({}); break;
        case 'signup': {
          const f = u.signup;
          if (!f.name.trim() || !f.contact.trim()) return toast(this.phone, 'Please enter your name and phone or email', 'err');
          if ((f.password || '').length < 6) return toast(this.phone, 'Password must be at least 6 characters', 'err');
          this.act('D_SIGNUP', { name: f.name.trim(), contact: f.contact.trim(), photo: f.photo });
          break;
        }
        case 'uploadDoc': this.act('D_UPLOAD_DOC', { doc: v }); break;
        case 'allDocs': for (const k of E.DRIVER_DOCS) if (portal(s).docs[k].status === 'none') await Store.send('D_UPLOAD_DOC', { doc: k }); break;
        case 'docsNext': this.act('D_DOCS_NEXT'); break;
        case 'vtype': u.vehicle.type = v; this.set({}); break;
        case 'demoDriver': { const d = byId(s, v); u.demoDriver = v; if (d) u.vehicle.type = d.vehicle.type; this.set({}); break; }
        case 'demoLogin': this.act('D_DEMO_LOGIN', { driverId: u.demoDriver, type: u.vehicle.type }); break;
        case 'vcolor': { const [n, h] = v.split('|'); u.vehicle.color = n; u.vehicle.hex = h; this.set({}); break; }
        case 'fillVehicle': u.vehicle = { ...u.vehicle, make: 'Toyota', model: 'Corolla', year: '2022', plate: 'N 9090 W', type: 'classic' }; this.set({}); break;
        case 'saveVehicle': this.act('D_SAVE_VEHICLE', { vehicle: u.vehicle }); break;
        case 'approveDoc': this.act('D_APPROVE_DOC', { doc: v }); break;
        case 'approveNow': this.act('D_APPROVE'); break;
        case 'startTrip': this.act('D_START_TRIP'); break;
        case 'tab': this.set({ tab: v }); break;
        case 'online': this.act('D_GO_ONLINE', { driverId: vid }); break;
        case 'offline': this.act('D_GO_OFFLINE', { driverId: vid }); break;
        case 'collapse': this.set({ collapsed: !u.collapsed }); break;
        case 'respond': this.act('D_RESPOND', { driverId: vid, accept: v === '1' }); break;
        case 'key': {
          if (v === 'C') u.pin = '';
          else if (v === '⌫') u.pin = u.pin.slice(0, -1);
          else if (u.pin.length < 4) u.pin += v;
          this.render(s);
          if (u.pin.length === 4) {
            const res = await this.act('D_ENTER_PIN', { pin: u.pin });
            if (!res.ok) {
              const box = this.content.querySelector('.pin-entry');
              if (box) { box.classList.remove('shake'); void box.offsetWidth; box.classList.add('shake'); }
              u.pin = '';
              this.render(Store.state);
            }
          }
          break;
        }
        case 'startWait': this.act('D_START_WAIT'); break;
        case 'noShow': this.act('D_CANCEL', { reason: 'no_show' }); break;
        case 'cancelReason': u.modal = null; this.act('D_CANCEL', { reason: v }); break;
        case 'report': u.modal = null; this.act('D_REPORT', { reason: v }); this.set({}); break;
        case 'jumpDest': this.act('X_JUMP_DEST'); break;
        case 'star': u.rating.stars = Number(v); this.set({}); break;
        case 'tag': { const t = u.rating.tags, i = t.indexOf(v); if (i >= 0) t.splice(i, 1); else t.push(v); this.set({}); break; }
        case 'rate': this.act('D_RATE', { stars: u.rating.stars, tags: u.rating.tags }); break;
        case 'finish': this.act('D_FINISH'); break;
        case 'modal': this.set({ modal: v }); break;
        case 'closeModal': this.set({ modal: null }); break;
        case 'call': this.act('D_CALL'); this.set({ modal: 'call', callStart: Date.now() }); break;
        case 'share': this.act('D_SHARE'); this.set({ modal: 'share' }); break;
        case 'copyLink':
          try { await navigator.clipboard.writeText('https://vara.example/r/' + s.ride.driverShare.code); } catch (err) { /* clipboard may be blocked */ }
          toast(this.phone, 'Link copied', 'ok');
          break;
        case 'quickMsg': this.act('D_MESSAGE', { text: v }); break;
        case 'sendMsg': if (u.chat.trim()) { this.act('D_MESSAGE', { text: u.chat.trim() }); u.chat = ''; this.set({}); } break;
        case 'sos': this.act('D_SOS', { driverId: vid }); break;
        case 'sosCancel': this.act('D_SOS_CANCEL', { driverId: vid }); break;
        // Placeholder until a real emergency-services integration exists: just confirm the notification.
        case 'call911': if (s.sos.driver) { this.set({ ems: s.sos.driver.at }); toast(this.phone, 'Emergency services have been notified', 'ok'); } break;
        case 'signOut': this.act('D_SIGN_OUT'); break;
      }
    }
  }

  function r2(n) { return Math.round(n * 100) / 100; }
  window.DriverApp = DriverApp;
})();
