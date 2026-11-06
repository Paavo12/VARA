# VARA — product & engineering documents

These documents take VARA from the working demo (`demo/`) to a production app built with **Rust** (backend) and **Svelte** (apps). The demo stays as the reference for behaviour: when a document says "as in the demo", the demo's behaviour is the requirement.

| # | Document | Answers |
| --- | --- | --- |
| 1 | [Product Requirements (PRD)](01-product-requirements.md) | What we build, for whom, and what "done" means for v1 |
| 2 | [Technical Requirements (TRD)](02-technical-requirements.md) | Architecture, stack, security, performance, operations |
| 3 | [App Flow](03-app-flow.md) | Every screen-to-screen journey and the ride state machine |
| 4 | [UI/UX Design Brief](04-ui-ux-design-brief.md) | Brand, design principles, screens and components for the designer |
| 5 | [Backend Schema](05-backend-schema.md) | PostgreSQL/PostGIS tables, Redis keys, API surface |
| 6 | [Implementation Plan](06-implementation-plan.md) | Phases, milestones, team, exit criteria |
| L1 | [Privacy Policy](legal/privacy-policy.md) | Draft — needs Namibian legal review |
| L2 | [Terms & Conditions](legal/terms-and-conditions.md) | Draft — needs Namibian legal review |
| L3 | [Cookie Policy](legal/cookie-policy.md) | Draft — web properties only |

## The stages from demo to launch

| Stage | What happens | Output | Gate to move on |
| --- | --- | --- | --- |
| **0. Definition** (now) | Turn the demo into written requirements; settle open decisions (PRD §10). | These documents, signed off. | Founders approve PRD + TRD. |
| **1. Legal & business setup** (runs in parallel from week 1) | Company registration, transport permits, legal opinion on women-only service, payment-provider account, insurance. | Legal opinion, licences, signed vendor contracts. | Counsel confirms we may operate; payment provider live keys obtainable. |
| **2. Design** | UX flows → wireframes → high-fidelity UI → clickable prototype; usability test with 8–10 Windhoek women. | Figma file, design tokens, component library spec. | Test participants complete booking + SOS unaided. |
| **3. Foundations** | Monorepo, CI/CD, environments, auth, observability, security baseline (rate limiting, validation, secrets). | Deployable skeleton on staging. | Security baseline checklist (TRD §7) passes. |
| **4. Build (MVP)** | Onboarding & verification → booking & dispatch → trip & safety → payments & ratings → ops console. | Feature-complete MVP on staging. | All PRD "Must" stories pass acceptance tests. |
| **5. Hardening** | Load test, penetration test, accessibility audit, app-store review prep, finalise legal texts. | Pen-test report with no open High/Critical. | Launch checklist (Implementation Plan §6) green. |
| **6. Closed pilot** | 30–50 vetted drivers, 300–500 invited riders in Windhoek, 24/7 safety desk. | Pilot metrics, bug backlog. | PRD success metrics met for 4 weeks. |
| **7. Public launch** | App Store + Google Play, marketing site, driver recruitment. | Live service. | — |
| **8. Operate & iterate** | On-call, incident response, monthly releases, roadmap (wallet, more cities, languages). | — | — |

## What happens to the existing folders

- `demo/` — kept, frozen as the behavioural reference and for pitches.
- `ride-hailing-backend/` (NestJS) and `rider_app/` (Flutter) — superseded by the Rust + Svelte stack. Archive them (move to an `archive/` branch) once the new skeleton exists, so nobody builds on them by mistake.
