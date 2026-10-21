# FUber

FUber is a women-only ride-hailing app for Windhoek, Namibia: every rider and every driver is a verified woman.

## What's in this repo

| Folder | What it is | Status |
| --- | --- | --- |
| [`demo/`](demo/) | The working prototype: a Rider app, a Driver app and a presentation panel on one live simulation, set on Windhoek's real road network. Also an Android wrapper (`demo/android/`). | **Use this.** Runs with Node.js 16+ and nothing else. |
| [`rider_app/`](rider_app/) | A Flutter project for a future native rider app. | Not started: still the default Flutter template. |
| [`ride-hailing-backend/`](ride-hailing-backend/) | A NestJS + PostgreSQL API for a future production backend (`docker-compose.yml` starts Postgres and Redis for it). | Early stub: create and list rides only. Not used by the demo. |

## Run the demo

```bash
cd demo
npm start
```

Then open http://localhost:3100. See [`demo/README.md`](demo/README.md) for the demo script, features, pricing and how matching works.
