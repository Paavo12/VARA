# FUber — Product Requirements Document (PRD)

| | |
| --- | --- |
| Version | 1.0 (draft for sign-off) |
| Date | 2026-10-26 |
| Owner | Product (founders) |
| Reference implementation | `demo/` — behaviour described in `demo/README.md` is the baseline |

---

## 1. Summary

FUber is a **women-only ride-hailing service for Windhoek, Namibia**. Every rider and every driver is a verified woman. The product's promise is **safety you can see**: verified identities on both sides, a PIN before the trip starts, the destination hidden until the right rider is in the car, live trip sharing, RideCheck and a one-tap SOS that reaches a human safety team.

v1 ships three products:

1. **FUber Rider** — iOS and Android app.
2. **FUber Driver** — iOS and Android app.
3. **FUber Ops** — web console for verification, safety, support and pricing.

Plus a public website (`fuber.app`, placeholder domain) for driver applications, trip-share links and legal pages.

## 2. Problem

- Women in Windhoek report harassment and unsafe experiences in taxis and mixed ride-hailing services, particularly at night and when travelling alone.
- Existing services verify drivers lightly and riders not at all; nothing guarantees who is in the car.
- Women who would like to drive for income avoid it because of the risk of picking up unknown men.

## 3. Goals and non-goals

### Goals (v1)

| Goal | Measure | Target (end of 4-week pilot) |
| --- | --- | --- |
| Riders feel safe | Post-trip "I felt safe" (yes/no) | ≥ 95 % yes |
| Fast, reliable matching | Requests matched to a driver | ≥ 85 % in normal demand |
| | Median driver arrival time | ≤ 8 min in central suburbs |
| Trustworthy verification | Rider verification decided within | ≤ 24 h (median ≤ 2 h) |
| | Driver application decided within | ≤ 5 working days |
| Safety response | SOS acknowledged by a human | ≤ 60 s, 24/7 |
| Driver earnings | Drivers keep of each fare | 82–85 % (service fee 15–18 %, below Bolt's 18–24 %) |
| Reliability | API availability | ≥ 99.5 % monthly |

### Non-goals (v1)

- Cities other than Windhoek (data model must not block them).
- Ride pooling with strangers (never: trips are private; 1–4 passengers travel together).
- Deliveries, rentals, corporate accounts.
- In-app wallet top-ups (phase 2 — see §9).
- Languages other than English (phase 2: Afrikaans, Oshiwambo, Otjiherero).

## 4. Users

| Persona | Description | Key needs |
| --- | --- | --- |
| **Ndeshi, rider** | 26, works in the CBD, lives in Katutura. Rides after dark several times a week. Mid-range Android, prepaid data. | Feel safe, know the price up front, tell family where she is. |
| **Martha, driver** | 38, single mother, owns or borrows a car. Wants flexible income without risk. | Only women passengers, fair earnings, know where she is going before committing, quick help if something goes wrong. |
| **Guest passenger** | Elderly mother or teenage daughter (16+, see §10) booked by a verified rider. Possibly no smartphone. | Get the driver details and PIN by SMS. |
| **Emergency contact** | Rider's or driver's chosen person. No app. | Receive a trip link and SOS alerts by SMS. |
| **Ops agent** | FUber staff: verification reviewer, safety desk, support. | Fast review queues, full trip context during incidents, audit trail. |

## 5. Scope — feature requirements

Priority: **M** = must for launch, **S** = should (launch if ready), **C** = could (post-launch).

### 5.1 Accounts & onboarding

| ID | Requirement | P |
| --- | --- | --- |
| A-1 | Sign up and sign in with a Namibian mobile number (+264) and SMS one-time code. No passwords. | M |
| A-2 | Rider verification: full name, date of birth, national ID or passport photo, and a live selfie. Account is *Pending* until approved by ops (or an automated vendor check plus ops review). A pending rider cannot book. | M |
| A-3 | Rider adds 1–3 emergency contacts (name + phone) during setup. Can skip and add later, but is reminded before the first night ride. | M |
| A-4 | Driver application: personal details, ID/gender verification, driver's licence, insurance, police clearance (criminal record check), vehicle details (make, model, colour, plate, year, seats, wheelchair-accessible yes/no), requested tier. No vehicle registration document: the driver may not own the car (as in the demo). | M |
| A-5 | Each driver document has its own status (*In review / Approved / Rejected — reason*) and expiry date; expired documents take the driver offline automatically with 30, 7 and 1 day warnings. | M |
| A-6 | Ops assigns the driver's approved tier(s); a driver can drive only tiers she is approved for. | M |
| A-7 | Accept the Terms, Privacy Policy and (drivers) Driver Agreement at sign-up; re-accept when a new version is published. Consent is recorded with version and timestamp. | M |
| A-8 | Delete account from inside the app (with data retained only as the Privacy Policy allows). | M |
| A-9 | One person can hold both a rider and a driver profile on the same phone number. | S |

### 5.2 Booking (rider)

| ID | Requirement | P |
| --- | --- | --- |
| B-1 | Pickup defaults to current GPS location, adjustable by dragging the pin or searching. | M |
| B-2 | Destination search covering landmarks, suburbs and streets; case- and accent-insensitive; recent and saved places (Home, Work). | M |
| B-3 | Three ride types with an **exact, locked price** and live ETA: Classic, Comfort, Soft Life. Rates in §8. | M |
| B-4 | Options: passengers 1–4 (price unchanged), wheelchair-accessible vehicle filter, *For me / Someone else*. | M |
| B-5 | *Someone else*: passenger name and phone; the passenger gets the driver details, plate and PIN by SMS. | M |
| B-6 | Schedule a ride 30 min – 7 days ahead. Dispatch starts 15 min before pickup time; rider is notified if no driver is found 5 min before. | S |
| B-7 | Payment method chosen before requesting: cash or card (§5.6). | M |
| B-8 | If no eligible driver is online within 3 km, the option shows *No driver nearby* and cannot be requested. | M |
| B-9 | Favourite drivers get a small matching bonus. | S |

### 5.3 Matching & dispatch

| ID | Requirement | P |
| --- | --- | --- |
| D-1 | Eligible drivers: online, no active trip, approved for the tier, wheelchair-accessible if requested, documents valid, within 3 km straight line of pickup. | M |
| D-2 | Rank by score = `w_eta × (1 − ETA/12 min) + w_acc × acceptance probability (+0.08 favourite)`, weights by demand level (normal 0.7/0.3, high 0.5/0.5, peak 0.45/0.55). ETA uses road distance. | M |
| D-3 | Sequential offers: best driver gets 15 s; decline or timeout → next driver, re-ranked with fresh positions. None left → *No drivers accepted → Try again*. | M |
| D-4 | The offer card shows: pickup area (fuzzy, not exact), distance and ETA to pickup, ride type, passengers, wheelchair need, rider first name and rating, and the driver's **earnings for this trip**. The destination is not shown (as in the demo). | M |
| D-5 | Priority points (hidden from users): start 100, max 150; nth cancellation costs n × 5; each completed trip +2. Low-priority riders have their nearest driver held for others (one per 15 points below 100; at least one driver always remains). Driver score shifts ±0.3 per 100 points from 100. | M |
| D-6 | Matching parameters (radius, offer window, weights, priority constants) are configurable in Ops without a release. | S |

### 5.4 Trip

| ID | Requirement | P |
| --- | --- | --- |
| T-1 | Rider sees driver photo, first name, verified badge, rating, car, colour, plate, live position, *arriving in X min*, *trip takes Y min*, and a **4-digit PIN**. | M |
| T-2 | Driver navigates to a **fuzzy pickup circle**. The exact pickup address unlocks within 50 m of the pin. | M |
| T-3 | At pickup the driver enters the rider's PIN. Wrong PIN → rejection; 5 wrong attempts → trip blocked, safety desk alerted, and the driver gets no requests for 15 min (until reviewed after 3 lockouts in 7 days; App Flow §7). Correct PIN → **destination unlocks** for the driver. | M |
| T-4 | Driver starts the trip; both apps follow the live route. Turn-by-turn on the driver app (in-app, or hand-off to Google Maps/Waze — §10). | M |
| T-5 | Wait timer: driver may start a 5-minute free wait at pickup; afterwards she may cancel as rider no-show (counts as a rider cancellation for priority points). | M |
| T-6 | Driver completes the trip with a slide gesture at or near the destination. | M |
| T-7 | Cancellation is always free for both sides; it costs priority points only. Driver cancellation (with reason) sends the request back to the pool excluding her. | M |
| T-8 | Masked communication: in-app chat with quick replies, and calls that hide both numbers. | M |

### 5.5 Safety

| ID | Requirement | P |
| --- | --- | --- |
| S-1 | **SOS** on every screen of both apps during a ride and while online. 3-second cancellable countdown, then: alert to the FUber safety desk with live location and trip details, and SMS to emergency contacts with a live trip link. The app offers one-tap call to Namibian Police (10111). | M |
| S-2 | **Share trip**: rider (and driver) can share a live link (`fuber.app/t/<token>`) showing position, driver first name, car, plate, destination area and ETA. Link expires 30 min after trip end. | M |
| S-3 | **RideCheck**: if the car goes > 150 m off the planned route or stops > 2 min unexpectedly mid-trip, the rider gets *Are you okay?* (*Yes, I'm fine / Call for help*). No answer in 60 s → safety desk alerted. | M |
| S-4 | Safety desk console: live incidents queue, trip map, call/chat both parties, escalate to police, close with notes. | M |
| S-5 | Report an issue after the trip (categories incl. harassment, unsafe driving, wrong person, lost item). | M |
| S-6 | Ratings below 3 stars require a reason; repeated low ratings trigger ops review. | S |
| S-7 | Audio recording during trips (opt-in). | C |

### 5.6 Payments & earnings

| ID | Requirement | P |
| --- | --- | --- |
| P-1 | Price locked at request (quote = charge). Detours, traffic or waiting never change it. | M |
| P-2 | **Cash**: driver confirms cash received at trip end. | M |
| P-3 | **Card**: card saved via a PCI-compliant payment provider (tokenised; FUber never sees card numbers). Pre-authorise at request, capture at completion, release on cancellation. | M |
| P-4 | Fare breakdown after trip: base + distance + time + surge; receipt by email (optional) and in trip history. | M |
| P-5 | Service fee (15 / 16.5 / 18 %) deducted from the driver's share. Drivers see per-trip earnings before accepting. | M |
| P-6 | Driver earnings dashboard: today, week, trips, hours online, per-trip list. Cash trips accrue a service-fee balance owed to FUber; card trips accrue a payout balance. Weekly settlement. | M |
| P-7 | FUber wallet / mobile money top-up. | C |
| P-8 | Tipping. | C |

### 5.7 After the trip

| ID | Requirement | P |
| --- | --- | --- |
| R-1 | Two-way 1–5 star ratings; rider can add the driver to Favourites. | M |
| R-2 | Trip history for riders and drivers with fare, route map, rating. | M |
| R-3 | Lost item flow: rider contacts the driver (masked) up to 24 h after the trip. | S |

### 5.8 Ops console

| ID | Requirement | P |
| --- | --- | --- |
| O-1 | Verification queues (riders, driver documents, driver applications) with approve / reject-with-reason, side-by-side selfie and ID. | M |
| O-2 | User search; suspend / reinstate with reason. | M |
| O-3 | Live map of online drivers and active trips. | M |
| O-4 | Safety incidents (§5.5 S-4). | M |
| O-5 | Pricing and surge control per zone; matching parameters. | M |
| O-6 | Support: view trip, refund card fare (full/partial), adjust priority points. | M |
| O-7 | Role-based access (reviewer, safety, support, admin); every action audit-logged; mandatory 2FA. | M |
| O-8 | Reporting: trips, cancellations, match rate, SOS count, verification times. | S |

### 5.9 Notifications

Push for: driver assigned, driver arriving / arrived, PIN reminder, trip started, trip completed, verification decided, document expiring, scheduled ride reminder. SMS fallback for driver-assigned and arrived when push fails, and always for guests and emergency contacts.

## 6. User stories (acceptance examples)

- *As a rider, when I request a Comfort ride, I am charged exactly the price shown on the options screen, even if the driver takes a detour.* → P-1.
- *As a driver, I do not learn the rider's destination until I enter her correct PIN at pickup.* → D-4, T-3.
- *As a rider, if I press SOS and don't cancel within 3 s, my emergency contacts receive an SMS with a live link and the safety desk sees my trip within 5 s.* → S-1.
- *As a driver whose insurance expires today, I cannot go online and see why.* → A-5.

Full acceptance criteria live with each ticket in the tracker; this list sets the style.

## 7. Non-functional requirements (product view)

Detailed in the [TRD](02-technical-requirements.md).

- Works on Android 9+ with 2 GB RAM and on iOS 16+; usable on 3G.
- App download ≤ 30 MB.
- Accessible: WCAG 2.2 AA; screen-reader labels on every control; 44 × 44 pt touch targets.
- Location: drivers share location only while online; riders only while booking and during trips.
- Privacy by default: phone numbers never shown to the other party; exact addresses revealed only when needed (T-2, T-3).

## 8. Pricing (N$, from the demo)

| Type | Base | Per km | Per min | Minimum | Service fee |
| --- | --- | --- | --- | --- | --- |
| Classic | 15 | 8 | 1.20 | 35 | 15 % |
| Comfort | 20 | 10 | 1.50 | 45 | 16.5 % |
| Soft Life | 28 | 13 | 2.00 | 60 | 18 % |

Service fee raised on 2 November 2026 from the demo's 2 / 2.5 / 3 %, which earned N$2–3 on a N$100 fare against N$2.10–3.60 of per-trip costs (SMS, maps, payment fees), a loss on every ride. At 15–18 % the platform keeps about N$12–15 per N$100 trip after those costs and breaks even at about 7,600 trips a month (fixed overhead N$48,500–97,500 a month, including the 24/7 safety desk). Bolt takes 18–24 % and Yango 15–20 %. Fares for riders did not change (migration `0007_service_fees.sql`).

Surge: High 1.4×, Peak 1.8×, set per zone by ops (automatic surge is phase 2). Time component uses the demand level's base traffic factor, so the quote is stable (fixed demo bug, see commit c284f3e). No booking, cancellation or no-show fees in v1.

## 9. Roadmap after v1

1. Wallet + mobile-money top-up, tipping.
2. Automatic surge from supply/demand.
3. Local languages.
4. Second city (Swakopmund / Walvis Bay).
5. Audio recording, trusted-contact auto-share for night rides.
6. Corporate / university accounts.

## 10. Open decisions (must be closed in Stage 0–1)

| # | Decision | Owner | Notes |
| --- | --- | --- | --- |
| 1 | **Legality of a women-only service** under Namibian law (Constitution Art. 10 equality / Art. 23 affirmative action, transport licensing). | Legal | Blocking. Get a written opinion before public launch. |
| 2 | **Eligibility & gender-verification policy**, including how trans women are treated, and how it is verified without collecting more than needed. | Founders + Legal | Affects A-2, A-4, the Privacy Policy and the Terms. |
| 3 | Minimum age for guest passengers (16 accompanied? 18?). | Founders + Legal | Riders with accounts are 18+. |
| 4 | Driver status: independent contractor vs. employee; tax/VAT on service fee. | Legal + Finance | Affects Driver Agreement. |
| 5 | Transport permits required for drivers/vehicles (Transportation Board, NaTIS). | Legal | May add required documents to A-4. |
| 6 | Payment provider (card acquiring in NAD). | Finance | Candidates to evaluate: local acquiring banks' gateways, DPO Pay, PayToday, Paygate. |
| 7 | Identity-verification vendor vs. manual review only for v1. | Product | Must support Namibian ID; manual review is the fallback. |
| 8 | Navigation: in-app turn-by-turn vs. hand-off to Google Maps/Waze. | Product + Eng | Hand-off is cheaper but can leak the destination before PIN → in-app recommended until PIN is verified. |
| 9 | Masked-calling provider (VoIP in-app vs. number proxy). | Eng | Must work on Namibian networks (MTC, Paratus). |
| 10 | Safety desk staffing 24/7 for pilot. | Ops | S-1 target depends on it. |
