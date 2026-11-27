# VARA threat model (STRIDE)

Required by TRD §7.7 for authentication, dispatch, safety and payments. Written 2026-11-27 against `main` (after PR #36) and checked against the code, not only the TRD: each control below names where it lives. Card payments are not built yet (the pilot is cash only), so their section lists what the integration must do rather than what it does.

**Status key:** ✅ in place and tested · ⚠️ partly in place / accepted risk · ❌ open, needs work.

## 1. What we protect

| Asset | Why it matters |
| --- | --- |
| A rider's live location, pickup and destination | Physical safety. Someone who can follow a woman's trip can find her. |
| Safety tools (SOS, RideCheck, trip share, PIN) | Must work every time, including under load and when other parts fail. |
| Identity data: ID photos, selfies, national ID numbers, dates of birth | Identity theft; also proves who is a woman on a women-only service. |
| Driver documents and payout details | Fraud, impersonating a vetted driver, diverting earnings. |
| Emergency contacts and guest phone numbers | Third parties who never agreed to anything. |
| Accounts and sessions | Taking over an account means seeing her trips and booking as her. |
| Staff powers (approve drivers, suspend, read documents, settle money) | One compromised or dishonest staff account touches everyone. |
| Money: fares, service fees, driver statements | Cash now, cards later. |

## 2. Who might attack

- **An outsider on the internet**: credential stuffing, OTP guessing, SMS pumping, scraping, DoS.
- **A man trying to get in** to a women-only service, as a rider (fake identity) or as a "driver" (borrowed documents, someone else's phone).
- **A stalker or abusive ex** who knows her phone number, has had her phone in his hands, or receives a share link.
- **A dishonest driver**: GPS spoofing to win requests, fake trips, guessing PINs, following a rider after drop-off.
- **A dishonest or compromised staff member.**
- **A lost or stolen phone.**

## 3. Trust boundaries

```
 Phone apps (rider, driver) ──HTTPS/WSS──► API (Cloud Run) ──► Postgres (Cloud SQL), Redis
 Ops console (browser)   ──HTTPS, cookie─┘        │
 Website /t/{token} (anyone with the link) ───────┤
                                                  ├──► Cloud Storage (signed URLs only)
 Worker (Cloud Run) ─── jobs, retention ──────────┘──► SMS, FCM, ClamAV + OSRM (internal VM)
```

The phone and the browser are never trusted: everything they send is validated on the server, and every object is loaded with an ownership or role check.

## 4. Authentication and sessions

| STRIDE | Threat | Control | Status |
| --- | --- | --- | --- |
| Spoofing | Guessing a sign-in code | 6-digit OTP, 5-minute expiry, hashed at rest; 5 wrong codes lock the challenge; per-phone and per-IP limits (`routes/auth.rs`, tests `otp_challenge_locks_after_five_wrong_codes_and_resend_has_cooldown`) | ✅ |
| Spoofing | SIM swap or someone holding her phone receives the code | Out of our control; driver sign-in on a new phone signs out the old one | ⚠️ |
| Spoofing | Stolen refresh token reused | 256-bit random, stored hashed, rotated on every use, reuse revokes the whole family (`auth.rs`) | ✅ |
| Spoofing | Forged access token | Ed25519 JWT, 15-minute lifetime, `kid` with rotation overlap (`TokenSigner`) | ✅ |
| Tampering | Tokens read from the phone | Refresh token in Keychain / Keystore (`@fuber/account` token store), never `localStorage` | ✅ |
| Repudiation | "I never accepted the Terms" | Consent stored per document version with time, IP and app version (`routes/legal.rs`) | ✅ |
| Info disclosure | Codes, tokens or phone numbers in logs | Phones masked, codes only printed by the fake SMS gateway in local development (`sms.rs`) | ✅ |
| DoS / cost | SMS pumping to premium numbers | `+264` only; per-phone and per-IP limits | ⚠️ No CAPTCHA yet (TRD §7.1 asks for one after 3 requests per IP): **finding F-3** |
| Spoofing | Someone signs in on a new device unnoticed | Drivers: one device at a time. Riders: no notice to her other devices | ❌ **finding F-4** |
| Elevation | Using the API without accepting new Terms | **Fixed on this branch**: `require_consents` on uploads, verification, emergency contacts, booking, going online and application submit; never on trip actions, SOS, chat or account deletion | ✅ **finding F-1** |

## 5. Staff and the ops console

| STRIDE | Threat | Control | Status |
| --- | --- | --- | --- |
| Spoofing | Password guessing | Password + TOTP; 5 attempts / 15 min per account and IP, 20 / hour per account (`ops/auth.rs`) | ✅ |
| Spoofing | Cookie theft / CSRF | `HttpOnly; Secure; SameSite=Strict` cookie, CSRF token on mutations (`ops/auth.rs`, `ops/mod.rs`) | ✅ |
| Elevation | A support agent approving drivers or settling money | Role checks per handler (`require_reviewer`, `require_safety`, `require_support`, `require_admin`) | ✅ |
| Info disclosure | Browsing ID photos for no reason | Short-lived signed URLs, every view audited (`identity.view`, `driver_documents.view`) | ✅ |
| Repudiation | Staff deny an action | Audit log entry for every ops action, with staff ID and IP | ✅ |
| Tampering | A staff member with database access edits or deletes audit entries | Append-only **by convention**: no database grants enforce it (TRD §7.6 says the app role has INSERT only) | ❌ **finding F-2** |

## 6. Dispatch and location

| STRIDE | Threat | Control | Status |
| --- | --- | --- | --- |
| Spoofing | Driver fakes GPS to be "nearest" | Positions must be inside the service area; a position she could not have reached from her last one is ignored (`presence.rs`, `FIX_MAX_SPEED_MPS`) | ⚠️ A determined spoofer can still lie about her position; review driver ratings and trip traces in ops |
| Info disclosure | Driver learns the rider's exact pickup and destination before she should | Driver sees a fuzzed pickup until arrival and the destination only after the PIN (`pickup_fuzzed`, App Flow §6) | ✅ |
| Info disclosure | Phone numbers exchanged | Never shown to the other party; in-app chat only until masked calling exists | ✅ |
| Tampering | Changing the price | Fare locked in a signed quote token at request time (`routes/rides.rs` quote) | ✅ |
| DoS | Flooding requests or location updates | `POST /rides` 5/min and 30/hour per user; WebSocket 2 msg/s, burst 10, then closed (`live.rs`) | ✅ |
| Elevation | A man registers as a driver | Manual review of ID, selfie, licence, police clearance; approval by a reviewer; audited | ⚠️ Depends on reviewer diligence; women-only eligibility policy still open (PRD §10 decision 2) |

## 7. Trip and safety

| STRIDE | Threat | Control | Status |
| --- | --- | --- | --- |
| Spoofing | Wrong car picks her up | 4-digit PIN said at pickup, never sent to the driver; plate shown to her | ✅ |
| Spoofing | Driver guesses the PIN | 5 wrong PINs block the trip and open a `pin_lockout` incident for the safety desk (`routes/trip.rs`) | ✅ |
| DoS | SOS rejected by rate limits | SOS is not rate limited (logged instead); texts retried by the worker if the first send fails (`safety.sos_sms`) | ✅ |
| DoS | SOS while the API is unreachable | App falls back to an SMS to the safety desk (`VITE_SAFETY_DESK_PHONE`) | ⚠️ Needs the real desk number and a 24/7 desk |
| Info disclosure | A trip-share link reaches the wrong person | 256-bit token, stored hashed; expires 30 minutes after the ride ends; shows first name, car and area only; dies if the ride moves to another driver | ✅ |
| Info disclosure | Lock screen shows trip details | Push texts carry no names, addresses, plates or PINs (`worker/src/push.rs`, test `the_rider_hears_about_her_trip_and_lock_screens_show_no_details`) | ✅ |
| Info disclosure | A new account on the same phone gets the old one's notices | A device token belongs to one session; deleted on sign-out (`PUT /me/push-token`) | ✅ |
| Repudiation | Disputes about what happened | Ride events, GPS track, chat and RideCheck answers kept; an incident holds them past the 90-day deletion | ✅ |
| Blocking safety | New Terms lock her out mid-trip | Consent never required for trip actions, SOS, RideCheck or chat (test `new_terms_hold_back_new_rides_but_never_a_trip_under_way`) | ✅ |

## 8. Personal data and uploads

| STRIDE | Threat | Control | Status |
| --- | --- | --- | --- |
| Tampering | Malicious file (script in a PDF, polyglot, decompression bomb) | Signed upload into quarantine; worker checks magic bytes, rejects active PDF content (including `#hh` escapes), re-encodes images, caps decoded size, virus-scans (`worker/src/lib.rs`, `integrations/src/media.rs`) | ✅ |
| Info disclosure | Database leak exposes ID numbers | National ID, date of birth, emergency-contact and guest phones, payout account sealed with AES-256-GCM (`Sealer`); national ID also hashed for duplicate checks | ✅ |
| Info disclosure | Data kept longer than the policy says | Retention jobs for every row of schema §12, including the 5-year incident review (PRs #33, #36) | ✅ |
| Info disclosure | IDOR: reading another user's ride, upload or chat | Every query filters by caller; "wrong user" tests for every endpoint (`wrong_user_and_unauthenticated_access_is_refused_everywhere`) | ✅ |
| Info disclosure | Erased account leaves her routes behind | Erasure clears addresses, route lines and RideCheck positions (PR #33) | ✅ |

## 9. Payments (cash now, cards later)

Cash pilot: the driver confirms cash received, which books VARA's fee against her; statements are settled by admins and audited.

| STRIDE | Threat | Control | Status |
| --- | --- | --- | --- |
| Repudiation | Driver claims a cash trip never happened, or rider says she paid | Ride events and GPS track; driver confirms cash; support can see the trip | ⚠️ Cash disputes are word against word |
| Tampering | Admin edits a settlement | Settlements recorded with reference and audited | ✅ |
| For the card provider (DPO Pay or PayToday) | Card data on our servers; forged webhooks; double charges | Must use the provider's hosted fields (no card data touches VARA), verify webhook signatures and timestamps (±5 min), and make webhooks idempotent (TRD §7.3) | ❌ Not built; review this section when the provider is chosen |

## 10. Infrastructure

| Threat | Control | Status |
| --- | --- | --- |
| Leaked secrets | Secret Manager at runtime; gitleaks in CI; no key files (Workload Identity, metadata tokens) | ✅ |
| Over-privileged services | Separate service accounts for API, worker, migrations; only the API signs URLs; only the worker sends push | ✅ |
| Vulnerable dependencies | `cargo deny`, `npm audit` in CI | ✅ |
| Volumetric DoS, bots | No load balancer / Cloud Armor yet (infra README "Known gaps") | ❌ Production step |
| Redis AUTH string in Terraform state | State bucket private and versioned | ⚠️ Accepted |
| Untested by an outsider | Penetration test before the pilot (Implementation Plan §6) | ❌ To book |

## 11. Findings

| # | Finding | Severity | Status |
| --- | --- | --- | --- |
| F-1 | Consent to the Terms and Privacy Policy was checked by the apps' screens only (and on driver application submit). A client calling the API directly could upload ID photos, add contacts, book rides and go online without accepting them, or without accepting a new version. | Medium | **Fixed on `security/consent-threat-model`**: server-side check; the apps open the consent screen when the server asks. |
| F-2 | The audit log is append-only by convention only: the database role the API and worker use can update and delete it. | Medium | Open. Recommended: run migrations as a separate owner role and grant the runtime role `INSERT, SELECT` on `audit_log` only, with deletion of entries past 5 years done by a narrow `SECURITY DEFINER` function the worker calls. Needs the Cloud SQL users split in Terraform. |
| F-3 | No CAPTCHA on `POST /auth/otp/request` (TRD §7.1). Per-phone and per-IP limits apply, so the cost of an SMS-pumping attack is bounded but not small across many IPs. | Medium | Open. Add Turnstile (or hCaptcha) after 3 requests per IP; needs a site key. |
| F-4 | Riders are not told when their account signs in on a new device (TRD §7.4). | Low | Open. Text or push to her existing devices on a new sign-in. |
| F-5 | The women-only eligibility rests on manual review. | Policy | Waits for PRD §10 decision 2 and legal advice. |

Review this document when card payments, masked calling or a new data type are added, and before the penetration test.
