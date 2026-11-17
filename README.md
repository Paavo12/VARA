# VARA

VARA is a women-only ride-hailing app for Windhoek, Namibia: every rider and every driver is a verified woman.

## What's in this repo

| Folder | What it is | Status |
| --- | --- | --- |
| [`demo/`](demo/) | The working prototype: a Rider app, a Driver app and a presentation panel on one live simulation, set on Windhoek's real road network. Also an Android wrapper (`demo/android/`). | **Use this.** Runs with Node.js 16+ and nothing else. |
| [`backend/`](backend/) | The production API, worker and migrations (Rust). | In development: Phases 0–3 done, see [`docs/06`](docs/06-implementation-plan.md). |
| [`apps/`](apps/) | Rider and driver apps (Svelte + Capacitor), the ops console and the website. | In development. |
| [`packages/`](packages/) | Shared Svelte UI, account screens and the generated API client. | In development. |
| [`infra/`](infra/) | Local Docker Compose (PostGIS, Redis) and Terraform for Google Cloud. | Staging written; see `infra/terraform/README.md`. |
| [`docs/`](docs/README.md) | Requirements, design brief, schema, plan and draft legal texts. | |

The earlier NestJS stub (`ride-hailing-backend/`) and Flutter template (`rider_app/`) were removed on 2026-11-17; they are kept on the `archive/legacy` branch.

## Production development

The production app is being built in **Rust** (backend) and **Svelte** (rider, driver, ops and web apps). The requirements, design brief, schema, implementation plan and draft legal policies are in [`docs/`](docs/README.md).

## Run the demo

```bash
cd demo
npm start
```

Then open http://localhost:3100. See [`demo/README.md`](demo/README.md) for the demo script, features, pricing and how matching works.

## Run the production app locally (Rust + Svelte)

Needs Docker Desktop, Rust and Node.js 22+.

```bash
docker compose -f infra/docker-compose.yml up -d     # Postgres + Redis
cd backend && cp .env.example .env
cargo run -p fuber-api                               # API on http://127.0.0.1:8080 (migrations: see below)
cargo run -p fuber-worker                            # second terminal: checks uploads, expiry, clean-up
```

Apply migrations once with `cargo test -p fuber-db` pointed at your database
(`TEST_DATABASE_URL=postgres://fuber:fuber-local-only@127.0.0.1:5432/fuber`), or with `sqlx migrate run`.

Then, from the repo root: `npm install`, `npm run dev -w @fuber/rider` (or `@fuber/driver`).
Sign-in codes are printed in the API's log (fake SMS, local only). Create a reviewer for the ops API with
`FUBER_STAFF_PASSWORD=… cargo run -p fuber-api --bin staff -- create you@example.com "Your Name" ops_reviewer`.
