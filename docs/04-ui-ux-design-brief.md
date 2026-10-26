# FUber — UI/UX Design Brief

| | |
| --- | --- |
| Version | 1.0 |
| Date | 2026-10-26 |
| For | Product designer(s) |
| Inputs | [PRD](01-product-requirements.md), [App Flow](03-app-flow.md), the demo (`demo/` — run `npm start` in `demo/` and open http://localhost:3100) |

---

## 1. The job

Design production-ready UI for **FUber Rider**, **FUber Driver** (iOS + Android, built in Svelte with Capacitor), the **Ops console** (desktop web) and the **website** (trip-share page, driver application landing, legal pages).

The demo already proves the flows and has a visual direction (plum + gold, Plus Jakarta Sans). Keep what works, make it production-grade: accessible, consistent, tested with real users, and documented as a component library.

## 2. Users and context of use

| Who | Context that should shape the design |
| --- | --- |
| Riders (18–55, women in Windhoek) | Often at night, outdoors, alone; one hand on the phone; mid-range Android; prepaid data and patchy 3G; bright sunlight in the day. |
| Drivers (25–60, women) | Phone in a dashboard mount; glances only while driving; long sessions; night driving; must act on an offer in 15 s. |
| Guests & emergency contacts | No app; they see only SMS and the trip-share web page, possibly on a basic browser. |
| Ops agents | Desktop, long shifts; speed and clarity during incidents matter more than polish. |

## 3. Design principles

1. **Safety you can see.** Verification badges, PIN, Share trip and SOS are always visible, never buried — but calm, not alarming. Safety is reassurance, not fear.
2. **No surprises.** Price, driver, car and ETA are shown before commitment. What the other side can see about you is explained in plain words (*Your driver sees your exact pickup only when she arrives*).
3. **Glanceable for drivers.** One primary action per screen, large type, high contrast, works in a car mount at arm's length. Offer card readable in < 2 s.
4. **Light on data and battery.** Few images, vector map tiles, skeleton states, no autoplay media.
5. **Warm and respectful.** Women-to-women tone: friendly, direct, never cute about danger.
6. **Inclusive.** Wheelchair users, older riders, low literacy, screen-reader users — designed in from the start, not added later.

## 4. Brand

### 4.1 Name and voice

- **FUber** — always written with capital F and U.
- Voice: confident, warm, plain English. Short sentences. Second person (*you*). Address the driver as *your driver* and by first name.
- Examples (from the demo, keep the style): *Finding your driver…*, *Show your driver this PIN*, *Destination unlocked*, *Are you okay?*, *No fee charged*.
- Avoid: jargon (*dispatch*, *surge* → *busy times*), blame (*You cancelled too many times*), fear (*danger*, *threat*).

### 4.2 Colour (from `demo/public/css/styles.css`, to be validated for contrast)

| Token | Hex | Use |
| --- | --- | --- |
| `plum` | `#6b2c91` | Primary brand, primary buttons, selected states |
| `plum-700` | `#521f72` | Pressed, headings on light |
| `plum-50` / `plum-100` | `#f6f0fb` / `#ecdff6` | Tinted surfaces |
| `gold` | `#c9a14a` | Premium accent (Soft Life, verified badge ring, focus ring) |
| `gold-grad` | `#f3d98b → #c9a14a → #a07c2c` | Soft Life tier, special moments only |
| `rose` | `#e0457b` | Secondary accent, illustrations |
| `ink` / `ink-2` / `muted` | `#1d1724` / `#463f50` / `#7a7285` | Text |
| `bg` / `card` / `line` | `#f7f5fa` / `#ffffff` / `#ebe6f0` | Surfaces, dividers |
| `green` | `#0f9d76` | Success, verified, PIN confirmed |
| `amber` | `#d97706` | Attention (PIN card at arrival, document expiring) |
| `red` | `#e11d48` | **SOS only** and destructive actions |
| `blue` / `teal` | `#2563eb` / `#0e8f9b` | Info, route line |

Rules:

- **Red is reserved for SOS** (and destructive confirmations) so it is never diluted.
- Every text/background pair must meet WCAG 2.2 AA (4.5:1 body, 3:1 large text and UI). `muted` on `bg` is borderline — check and darken if needed.
- Provide a **dark theme** for both apps (required for the driver app at night; follows system for the rider app). The demo's stage colours (`#110c17`, `#1b1523`) are a starting point.

### 4.3 Typography

- **Plus Jakarta Sans** (bundled in the app, not loaded from Google Fonts at runtime) with system fallback.
- Scale (mobile): 12 / 14 / 16 (body) / 18 / 22 / 28 / 34. Driver app minimum 16 pt for anything read while driving; offer-card earnings ≥ 28 pt.
- Support Dynamic Type / Android font scaling up to 200 % without truncating critical info (PIN, price, plate).

### 4.4 Iconography, imagery, map

- One icon set (e.g. Lucide or Phosphor), 24 px grid, 1.5–2 px stroke.
- People imagery: real Namibian women, diverse ages and body types; no stock clichés. Avatars fall back to generated initials (as in the demo).
- Map: MapLibre with a custom light and dark style in brand-neutral tones so plum overlays (route, fuzzy pickup circle, car markers) stand out. Demand heat map for drivers in soft rose. Keep "© OpenStreetMap contributors".
- Car markers: top-down car icon tinted by tier; rider pickup pin in plum; fuzzy pickup as a translucent plum circle with dashed border.

## 5. Screens to design

### 5.1 Rider app (priority order)

1. Welcome, phone entry, OTP (auto-read on Android)
2. Your details, ID capture (guide frame, glare/blur feedback), selfie with liveness prompt, pending / verified / rejected states
3. Emergency contacts (pick from contacts or type)
4. Home map + *Where to?* sheet; recent & saved places
5. Search results (landmarks, suburbs, streets) and pin-adjust
6. **Ride options**: three tier cards with price, ETA, seats; passengers stepper; wheelchair toggle; For me / Someone else; Now / Schedule; payment method; disabled state *No driver nearby*
7. Finding your driver (radius animation), No drivers accepted
8. **Driver en route**: driver card (photo, name, verified, rating, car, colour, plate in Namibian plate style), ETAs, **PIN card**, Call, Message, Share trip, Cancel
9. Arrived (PIN card amber), In trip, RideCheck full-screen prompt
10. **SOS**: button, 3 s countdown, sent confirmation with *Call 10111*
11. Share trip sheet; masked call screen; chat with quick replies
12. Trip complete: fare breakdown, payment, rating (reason chips for < 3), add to favourites
13. Trips (upcoming / past), trip detail, report an issue
14. Safety tab, Account, payment methods (provider-hosted card form), legal, delete account
15. Empty, loading (skeleton), offline / reconnecting, error states for each of the above

### 5.2 Driver app

1. Application: details, document upload checklist with per-document status chips, vehicle, tier, payout details, status screen
2. Home offline / **Go online** (big toggle), checks (docs, location *Always*), waiting with heat map
3. **Offer card**: 15 s countdown ring, earnings (largest), pickup area & distance, ETA, tier, passengers, wheelchair, rider first name & rating, Accept (primary, large) / Decline
4. To pickup: turn-by-turn banner, fuzzy circle, rider card, Call/Message, Cancel with reasons
5. At pickup: exact address reveal, **PIN keypad** (wrong-PIN shake + attempts left), wait timer
6. Destination unlocked → Start trip; In trip; **Slide to complete**
7. Cash confirmation, rate rider, earnings for this trip
8. Earnings dashboard (today, week bars, trips, hours online, balance, payouts)
9. Documents with expiry warnings; forced-offline state
10. SOS (same pattern as rider)

### 5.3 Ops console

Dashboard live map · verification queue + review screen (ID and selfie side by side, zoom, reject reasons) · incident queue with audible alert + incident detail (map, timeline, actions) · trip detail · user profile · pricing & surge editor · staff & roles · audit log. Dense, keyboard-friendly, desktop-first (≥ 1280 px); data tables with filters.

### 5.4 Website

Trip-share page (works without JavaScript frameworks heavy load; < 200 KB; live map + details + *Call 10111*), driver application landing, legal pages, cookie banner (only if non-essential cookies are introduced — see Cookie Policy).

## 6. Key components (deliver as a Figma library + Svelte specs)

Button (primary, secondary, ghost, destructive, SOS) · Bottom sheet (snap points) · Tier card · Driver card · Vehicle plate · PIN display & PIN keypad · Countdown ring · Slide-to-confirm · Status chip (In review / Approved / Rejected / Expiring) · Rating stars + reason chips · Map markers & overlays · Toast & banner (incl. offline) · Stepper · Toggle · Text field with validation messages · OTP input · Document upload tile · Empty state · Skeleton · Tab bar · Top bar with SOS slot.

Each component: states (default, hover/pressed, focus, disabled, loading, error), light + dark, RTL-safe spacing, accessibility notes (role, label, focus order).

## 7. Accessibility requirements

- WCAG 2.2 AA throughout.
- Touch targets ≥ 44 × 44 pt (≥ 56 pt for driver primary actions and SOS).
- Never colour alone: PIN-card state, status chips and map overlays also use text/icons.
- Screen readers: every control labelled; live regions announce ride-state changes (*Your driver has arrived*).
- Reduced motion: replace radius pulse and countdown animation with static equivalents.
- SOS reachable by screen reader in ≤ 2 swipes from any ride screen.
- Plain language, reading age ~12.

## 8. Localisation

English for v1; layouts must accommodate +40 % text length for Afrikaans, Oshiwambo and Otjiherero (phase 2). Currency `N$ 123.40`; dates `26 Oct 2026`; 24-hour time; phone `+264 81 123 4567`.

## 9. Deliverables and timeline

| Week | Deliverable |
| --- | --- |
| 1 | Audit of demo, user flows confirmed, information architecture |
| 2–3 | Low-fi wireframes all priority screens; review |
| 4 | Usability test #1 with 8–10 Windhoek women (riders + drivers) on clickable wireframes |
| 5–6 | High-fidelity UI (light + dark), component library, design tokens (JSON, three layers: primitive → semantic → component) |
| 7 | Usability test #2 on hi-fi prototype; iterate |
| 8 | Handoff: Figma with dev mode, tokens exported to `packages/ui`, redlines for complex components, motion specs, app-store screenshots and icon |

Success criteria for the design: in test #2, ≥ 9 of 10 participants book a ride, find and trigger SOS, and share a trip without help; drivers read the offer card and decide within 10 s.

## 10. Out of scope for this brief

Marketing campaigns, brand identity rework (logo refresh optional — the demo's icon is a placeholder), printed material.
