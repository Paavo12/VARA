# VARA — women-only ride-hailing demo (Windhoek, Namibia)

A working prototype with a **Rider app**, a **Driver app** and an **Admin dashboard** (the ops console), all running on one simulated dispatch engine. What the admin changes (prices, fees, approvals, pauses) shows up on the phones straight away. It's built for a live presentation, so the city, GPS, matching, payments and verification are all simulated, while every state change is visible.

- **Zero dependencies.** It needs Node.js 16+ and nothing else: no `npm install`, no database and no API keys. Internet is optional: with it you get the OpenStreetMap map and the web font; without it the app draws Windhoek's roads itself and uses the system font.
- **Set in real Windhoek.** All prices are in **Namibian dollars (N$)**. The map is the free OpenStreetMap map, and cars drive Windhoek's real roads: routes, ETAs, street names, addresses and turn-by-turn directions come from OpenStreetMap road data built into the app, and landmarks (Post Street Mall, Maerua Mall, Grove Mall, Katutura State Hospital, Eros Airport…) sit at their real locations. Plates look like Windhoek plates (`N 9090 W`), and phone numbers use +264.
- **One rider, four drivers.** The demo starts with a single rider (Ndeshi, the rider phone's demo account) and four women drivers with demo accounts: Hilma (Classic), Selma (Classic), Ndapewa (Comfort) and Martha (Soft Life). Every driver starts offline. Nothing is automated: a driver goes online only when someone signs in as her on a driver phone and taps *Go online*, and every offer, PIN, trip start, completion, rating and verification approval happens when someone clicks. Anyone who signs up through *Apply to drive* is added as another driver.
- **One driver per phone.** Each phone, emulator or browser tab running the driver app signs in separately, so two emulators (or more) can each be a different driver at the same time. Signing in as a driver who is already signed in elsewhere signs her out on the other screen.
- **Two ways to run.** Run it through the server so screens on several devices stay in sync, or open `public/index.html` directly from disk to run everything in one browser tab.

---

## 1. Start the app

```bash
cd demo
npm start          # or: node server.js
```

Open **http://localhost:3100**. You get both phones side by side plus the presentation panel.

| URL | What it shows |
| --- | --- |
| `http://localhost:3100/` | Presenter stage: Rider + Driver phones + presentation panel |
| `http://localhost:3100/?view=rider` | Rider app only (full-screen when opened on a phone) |
| `http://localhost:3100/?view=driver` | Driver app only (full-screen when opened on a phone) |
| `http://localhost:3100/?view=admin` | Admin dashboard only: open it in a second window or on a second screen |
| `http://<your-LAN-IP>:3100/?view=rider` | Use a real phone as the rider. The server prints this address at startup |

Every screen connected to the server shares one live state. You can book on your phone and accept on the laptop. The top bar's **Show** switch also has **Admin** and **Phones + admin** (both phones with the dashboard beside them).

**No Node?** Double-click `public/index.html`. The demo runs entirely in that tab (the top bar shows *Standalone*). Multi-device sync isn't available in this mode, so use **Phones + admin** to show the dashboard next to the phones.

Use a different port with `PORT=4000 npm start`.

---

## 2. A 3-minute demo script

1. **Onboarding.** Pick one:
   - **Rider:** *Create account → Fill sample details → Continue → Sample* (ID) and *Sample* (selfie) *→ Submit for verification*. It shows **Pending verification** until you tap **Approve verification** (or *Approve verifications* in the panel), then **Verified ✓**, then *Continue → Fill sample contacts → Finish setup*. There's no code-of-conduct step.
   - **Driver:** *Apply to drive → Fill sample details → Upload all samples* (ID/gender verification, driver's licence, insurance, criminal-record check). Each document shows **In review** with an **Approve** button. Then *Fill sample vehicle → Submit application → Approve application → Start driving → Go online* (any document still in review is approved with the application). The new driver joins the dispatch pool 0.6–1 km from the rider. There's no vehicle-registration document, because a driver may be driving a car she doesn't own.
   - **Shortcut:** *Use demo account* skips onboarding on either phone. On the driver phone you pick which of the four drivers to be, then Classic, Comfort or Soft Life, and each tier shows its rates (higher tier, higher fare). Then tap **Go online**. The rider must book a ride type that an online driver drives; until a driver is online, the rider's options say *No driver online within 3 km*.
2. **Book.** On the rider phone, tap *Where to?* and search for a landmark, suburb or any Windhoek street (for example *nelson mandela*, *Klein Windhoek* or *mall*; upper/lower case and accents don't matter), or tap a quick-place chip such as *Post Street Mall*. You get Classic / Comfort / Soft Life with an exact, locked price and live ETAs, a wheelchair-accessible filter, *For me / Someone else*, **1–4 passengers** (all women, never shared with strangers), Now/Schedule and a payment method. Tap **Request**.
3. **Matching.** *Finding your driver…* shows the pulsing **3 km dispatch radius**. After about 2–3 s the best-matched online driver gets the request card on her phone with a **15-second countdown**. Tap **Accept**. If she declines or doesn't answer in time, it goes to the next online driver, and when nobody is left the rider sees *No drivers accepted → Try again*.
4. **Driver en route.** The rider sees the driver's photo, verified badge, rating, car, plate, both ETAs (*Driver arriving in X min* / *Trip will take Y min*) and the **4-digit PIN**. The driver sees turn-by-turn directions to a **fuzzy pickup circle**. The exact address and destination stay locked.
5. **Arrival → PIN.** Within 50 m of the pin, the driver phone **unlocks the exact pickup address** and shows a PIN keypad. The rider's PIN card turns amber. Type a wrong PIN first to show the rejection and the tries left, then the real PIN (it's on the rider phone and in the panel).
6. **PIN verified → destination revealed → In trip.** The driver phone shows *Destination unlocked*. Tap **Start trip** on the driver phone, and both phones follow the live route.
7. **RideCheck.** In the presentation panel, press **Route deviation** (or **Unexpected stop**). The rider gets *Are you okay?* with *Yes, I'm fine* / *Call for help*.
8. **Complete.** Press **Jump to destination** (panel or driver phone), then **slide to complete trip** on the driver phone.
9. **Pay & rate.** The rider sees the fare breakdown (base + distance + time + surge), pays cash (cards come after the pilot), rates the driver and can **add her to Favourite Drivers**. The driver rates the rider and sees her earnings. Once both have rated (or the driver taps *Back to requests* without rating), the ride state becomes `RATED`.
10. **PIN block (optional, about 1 minute).** Book another ride. With the driver at the pickup, press **5 wrong PINs** in the panel: both phones stop the trip, and the driver gets no requests after that. Press **Close incident** under *Safety desk* in the panel to release her.
11. **Admin dashboard (about 2 minutes).** See section 4a.
12. **What's real today.** Scroll to *Real product status* at the bottom of the panel. It separates what is built from what this demo only previews.
13. **Reset demo** (top right; click twice to confirm) clears everything for the next run, including admin changes.

**On a hosted link:** everyone who opens the link shares one live demo, so anyone with it can see and change the same ride. Press **Reset demo** just before presenting, and share the link only with the presenter until it's over.

---

## 3. How to show specific features

| Feature | How to trigger it |
| --- | --- |
| **SOS** | The red **SOS** pill is in the top-right of every screen on both phones. Pressing it opens a 3-second *Sending SOS…* countdown (cancellable), then an alert to her emergency contacts and the VARA safety desk with live location and trip details. |
| **Share My Trip** | Rider phone during a ride → **Share trip**. You get a link (`vara.example/t/XXXXXX`) with live location, driver, plate, destination and ETA, plus *Copy* and *Send to my emergency contacts*. The driver phone has **Share my route**. |
| **PIN step & destination reveal** | Wait for the driver to arrive, or press **Driver arrives now** in the panel. The panel's *Exact pickup → driver* and *Destination → driver* tiles flip from `hidden` to `revealed` at each step. |
| **PIN block** | With the driver at the pickup, type 5 wrong PINs or press **5 wrong PINs** in the panel. Both phones show the blocked trip. The driver can stay online but gets no requests, and her phone says why. In the panel's **Safety desk** section, press **Close incident** to release her. No fare, and no priority points lost. |
| **Messages / calls** | **Message** opens in-app chat with quick replies. Messages appear on the other phone as toasts. **Call** explains that private calling is planned and offers chat instead. |
| **RideCheck** | Panel → **Route deviation** (fires once the car is more than 150 m off the planned route) or **Unexpected stop** (fires after 2 simulated minutes stopped). |
| **Surge pricing** | Top bar → Demand **High** (1.4×) or **Peak** (1.8×). The fare cards show the multiplier, the driver map shows a stronger demand heat map, and the price is locked at request time. |
| **Cancelling (no fees)** | Cancelling is always free for the rider and the driver. Instead, each cancellation quietly costs **priority points** (see section 6). The panel shows the points; the phones never do. |
| **Driver cancels** | Driver phone → **Cancel** → pick a reason. The request goes **back to the pool** and that driver is excluded. If no other online driver drives that ride type, the rider then sees *No drivers accepted*. |
| **Wait timer / no-show** | At pickup, tap **Rider not here? Start wait timer** (5 free minutes of simulated time, about 15 s at 20×). After it expires the driver can cancel. It's free, but it counts as a rider cancellation for priority points. |
| **Request for someone else** | Ride options → **Someone else** → enter the passenger's name and phone (or *Fill sample*) and tick **The passenger is an adult woman** (required: children only ride with an adult woman, and no men ride). The passenger needs no app or internet: the PIN and driver details go to them by SMS (simulated). Set the pickup pin to where they are. The driver sees the passenger's name and enters the passenger's PIN. |
| **Kids Guardian** | Ride options → **Children with her** → **+**. Children count among the passengers (up to 3, always with an adult, so 2 children means at least 3 passengers). With children on, tick **Child seat**: it adds the admin's surcharge (N$0 until set, see section 4a), which goes to the driver in full with no fee. The driver sees the children and the seat on the request card and again at the PIN screen; the receipt shows a *Child seat* line. Children never ride alone. |
| **Map Expert / launch promotion** | Set them in the admin dashboard (section 4a). The driver's **Earnings** tab shows her fee now and why, and her request card and trip earnings use it. |
| **Scheduled rides** | Ride options → **Schedule** → pick a time → **Schedule**. It appears under *Trips → Upcoming*. Tap **Simulate: it's time** to dispatch it. |
| **Wheelchair-accessible** | Ride options → toggle *Wheelchair-accessible vehicle*. Only drivers who ticked *Wheelchair-accessible vehicle* when applying qualify; the demo driver hasn't, so register one through *Apply to drive* to show it. |
| **Trip history / favourites** | Rider → **Trips** tab (past rides with fare, driver and rating) and **Safety** tab (emergency contacts, favourite drivers). |
| **Earnings dashboard** | Driver → **Earnings** tab: today's earnings, trips, hours online, weekly bars, and recent trips with the rider's rating. |
| **Demand heat map** | The driver map shows red demand zones around busy places (malls, markets, hospitals). They never show exact pins. |
| **No drivers available** | Book a ride type the driver doesn't drive (for example Soft Life when she signed in as Classic), or decline the offer on the driver phone. You get *No drivers accepted → Try again*. |

---

## 4. Presentation panel

Toggle **Presentation mode** in the top bar. The panel is kept simple:

- **Ride progress:** the steps of a ride, from `REQUESTED` to `RATED`. Finished steps are ticked and the current one is highlighted.
- **Ride details:** ride type, the rider's PIN, the driver, when she arrives, the trip length and who the passenger is.
- **Priority points:** the rider's and each driver's points and cancellations. Only the panel shows these; the phones never do.
- **Safety desk:** open incidents, with a button to close each one.
- **Demo shortcuts:** driver arrives now, jump to destination, route deviation, unexpected stop, 5 wrong PINs, approve verifications.
- **Settings:** demo speed (10–60× simulated time).
- **What just happened:** the last 12 events.
- **Real product status:** what is built, what is only previewed here and what comes later. Point to it when someone asks what exists today.

---

## 4a. Admin dashboard

Open it with **Show → Admin** (or `?view=admin` in a second window), then **Sign in as demo admin**. Every change is written to the **Audit log** and reaches the phones at once.

| Section | What it does | What changes on the phones |
| --- | --- | --- |
| **Overview** | Drivers online, trips, what riders paid, VARA fees, drivers' earnings, a live map and the live ride (children, locked price, the driver's fee) | — |
| **Verifications** | Approve or reject a new rider's ID and selfie (a rejection needs a reason); approve a driver applicant's documents one by one, then her application | A rejected rider sees *Not approved* with the reason and uploads again; an approved driver can go online |
| **Drivers** | Each driver's status, rating, priority points and fee now. **Grant** makes her a Map Expert until a date (at most 366 days); **Pause** with a reason stops her requests until **Release** | Her Earnings tab and next request card show the 15 % Map Expert fee; a paused driver sees *Requests paused* and the reason |
| **Safety desk** | SOS alerts (**Acknowledge**), PIN-lockout incidents and drivers' reports. Closing an incident or report needs a written outcome | Acknowledging tells her the desk has her alert; closing a PIN incident releases the driver |
| **Pricing** | Rates per ride type (base, per km, per minute, minimum, service fee 15–18 %), demand and surge, the **launch promotion** date (or **Launch today**) and the **child seat surcharge** | New quotes on the rider phone use the new rates at once; a ride already booked keeps its locked price |
| **Trips** | Every finished trip: what the rider paid, the fee and which fee it was, the child seat surcharge and what the driver earned | — |
| **Audit log** | Who changed what and when | — |

**A 2-minute admin script:**

1. Pricing → set the child seat surcharge to 30. On the rider phone, add 2 children and a child seat: the price goes up by N$30.
2. Raise Comfort's per-km rate and save: the rider's Comfort price changes before she requests.
3. Drivers → **Grant** Map Expert to the driver who is online: her Earnings tab shows 15 %.
4. Book and finish the ride. The driver's earnings show the Map Expert fee and the surcharge with no fee taken; Trips shows the split.
5. Pricing → **Launch today**: every driver now pays 13 % for 90 days, and the launch date can no longer be changed, as in the real console (**Reset demo** to try again).
6. Drivers → **Pause** her with a reason, show her phone, then **Release**.
7. Audit log.

The sign-in is a demo button. In the real console staff sign in with a second factor, the session is an HttpOnly cookie with CSRF protection, and roles (support, safety, finance, admin) limit what each person can do.

---

## 5. Pricing (N$)

Starting rates (the admin dashboard can change them; see section 4a):

| Option | Base | Per km | Per min | Minimum | Service fee |
| --- | --- | --- | --- | --- | --- |
| Classic | N$15 | N$8 | N$1.20 | N$35 | 15% |
| Comfort | N$20 | N$10 | N$1.50 | N$45 | 16.5% |
| Soft Life | N$28 | N$13 | N$2.00 | N$60 | 18% |

The price is locked when the rider requests: she pays exactly the quote, and the driver earns the quote minus the service fee, shown on her request card before she accepts. A detour, traffic or a skipped demo trip never changes it. The number of passengers (1–4) doesn't change the price either. There is no booking fee yet; how a ride fee will work is still to be decided. There are no cancellation or no-show fees either: cancellations cost priority points instead. The surge multiplier applies at High (1.4×) and Peak (1.8×) demand. VARA's service fee comes out of the driver's share: she keeps 82–85 % of the fare.

**Lower fees, as in the real app (the lowest that applies wins):** a **Map Expert** pays a flat 15 % on every ride type until the date the admin set. During the **launch promotion** every driver pays 13 % for 90 days from the launch date, then the normal fees come back by themselves. The driver's fee is booked when she accepts.

**Child seat surcharge (Kids Guardian):** a flat amount the admin sets (N$0 until then, at most N$200), added to the rider's price when she brings children and asks for a child seat. The driver keeps all of it; no fee is taken on it.

The starting rates are `OPTIONS` at the top of `public/js/engine.js`; the fee rules are in `CFG` and `feeFor`. Admin changes live in the demo state (`s.pricing`) until **Reset demo**.

---

## 6. How the matching works

1. **Filter, in priority order.** The driver must be online and signed in on a driver phone, with no active trip, drive the requested vehicle type (plus wheelchair access if requested), and be within **3 km straight-line** of the pickup.
2. **ETA.** Uses **road distance**: the shortest route over Windhoek's real roads (respecting one-way streets), divided by `34 km/h ÷ traffic factor`. The traffic factor depends on demand and is jittered every 2 s, so ETAs update live.
3. **Score.** `w_eta × (1 − ETA/12 min) + w_acc × acceptance probability (+0.08 if she's a favourite driver)`.
   - Normal demand: `w_eta = 0.7`, `w_acc = 0.3`.
   - High/peak demand: weights move toward acceptance (0.5/0.5 and 0.45/0.55).
4. **Priority points (hidden).** Riders and drivers start at 100 points, and only the presentation panel shows them. Each cancellation costs more than the last (−5, −10, −15…), and each completed trip earns +2 back, up to 150. A rider no-show counts as a rider cancellation.
   - **Rider:** for every 15 points below 100, the nearest eligible driver is *held for higher-priority riders*, but at least one driver always stays available. Above 100, closeness is weighted up, so she gets nearer drivers.
   - **Driver:** her score shifts by ±0.3 per 100 points away from 100, so drivers who cancel get fewer requests and reliable drivers get more. Points are set in `CFG.PRIORITY_*` in `public/js/engine.js`.
5. **Sequential offers.** The top-scoring driver gets the request for **15 s**. A decline or timeout moves it to the next best, re-ranked with fresh positions. If nobody is left, the state becomes `NO_DRIVERS`.

**Demo data.** One rider and four drivers, as described at the top. More drivers join when people apply to drive, and the matching rules above then choose between them. The driver list is `SEED_DRIVERS` in `public/js/engine.js`.

---

## 7. Android app (wrapper)

`android/` holds a small native Android app with two launcher icons, **VARA Rider** and **VARA Driver**. Each opens its portal full-screen with no browser bar.

- **Server first.** On launch it looks for the demo server at `localhost:3100` (via `adb reverse`), then `10.0.2.2:3100` (the emulator's alias for your PC), then the Wi-Fi address in `android/gradle.properties` (`heraLanUrl`, for a real phone).
- **Offline fallback.** If none answers, it runs the copy of the web app bundled inside the APK, standalone.

**Build and install without Gradle or internet.** This uses only the SDK build-tools and Android Studio's JDK:

```bash
cd demo/android
./build-apk.sh install      # builds build/hera-demo.apk, installs it on every running emulator/phone, sets up adb reverse on each
```

Run it from Git Bash. If your SDK or Android Studio aren't in `D:\Android`, set `SDK=` / `JDK=`.

**Or open `demo/android` in Android Studio** and press Run. It's a normal Gradle project (AGP 9.1, compileSdk 35, Java, no libraries). The first sync downloads Gradle and the Android plugin, which takes a while on a slow connection.

After editing the web app, rebuild the APK so the bundled offline copy matches. When the app is connected to the server, it always shows the latest version.

---

## 8. Project layout

```
demo/
├── server.js            Zero-dependency HTTP server + SSE broadcast + authoritative simulation
├── package.json
├── android/             Android wrapper app (VARA Rider / VARA Driver) + build-apk.sh
├── tools/build-roads.js Builds public/js/roads.js from OpenStreetMap data (tools/data/)
└── public/
    ├── index.html
    ├── css/styles.css
    └── js/
        ├── roads.js     Windhoek's road network (generated: do not edit by hand)
        ├── engine.js    Simulation: road routing, pricing, matching, state machine, safety logic
        ├── common.js    Transport (server or in-tab), map (OSM tiles + overlays), icons, generated avatars, helpers
        ├── rider.js     Rider portal
        ├── driver.js    Driver portal
        ├── debug.js     Presentation panel (simple live view for the audience)
        ├── admin.js     Admin dashboard (ops console): verifications, drivers, safety desk, pricing, trips, audit log
        └── main.js      Stage layout, top bar, phone scaling
```

**Out of scope, by design:** real payments, real GPS or background location, production-scale matching, and legal/compliance workflows. All of these are simulated with clear UI states.

---

## 9. Map (OpenStreetMap)

- **Map tiles.** When the device is online, the map shows standard OpenStreetMap tiles (`tile.openstreetmap.org`). They're free and need no API key, but OSM's tile servers are meant for light use only, which is fine for demos. For a real launch, switch `TILE_URL` in `public/js/common.js` to a tile provider's plan or your own tile server. The "© OpenStreetMap contributors" credit on the map is required, so keep it.
- **Offline.** With no internet, or if tiles fail to load, the map draws Windhoek's roads from the built-in data instead. Everything else keeps working, because routing never needs the internet.
- **Road data.** `public/js/roads.js` holds about 6,200 junctions and 8,600 road segments covering Okuryangava to Kleine Kuppe, about 12 × 17 km. It was downloaded once from OpenStreetMap through the Overpass API. To refresh it, run `node tools/build-roads.js --fetch`. Without `--fetch`, it rebuilds from the saved copy in `tools/data/`.
- **Places.** Landmarks and suburb names are in `PLACES` and `SUBURBS` at the top of `public/js/engine.js`, as latitude/longitude. The rider's demo location is `HOME`, on Sam Nujoma Drive in Windhoek Central.
- **Addresses.** Street names are real. House numbers are illustrative, because OpenStreetMap has few house numbers for Windhoek.

Map data © OpenStreetMap contributors, available under the Open Database Licence.
