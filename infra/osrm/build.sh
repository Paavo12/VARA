#!/usr/bin/env bash
# Downloads the Namibia OpenStreetMap extract and prepares it for OSRM (car profile, MLD).
# Re-run monthly to pick up map changes (TRD §8). Output goes to infra/osrm/data/ (git-ignored).
set -euo pipefail
cd "$(dirname "$0")"
mkdir -p data
IMAGE=ghcr.io/project-osrm/osrm-backend:v5.27.1
curl -fL -o data/namibia-latest.osm.pbf https://download.geofabrik.de/africa/namibia-latest.osm.pbf
docker run --rm -v "$PWD/data:/data" "$IMAGE" osrm-extract -p /opt/car.lua /data/namibia-latest.osm.pbf
docker run --rm -v "$PWD/data:/data" "$IMAGE" osrm-partition /data/namibia-latest.osrm
docker run --rm -v "$PWD/data:/data" "$IMAGE" osrm-customize /data/namibia-latest.osrm
echo "OSRM data ready. Start it with: docker compose -f infra/docker-compose.yml --profile routing up -d osrm"
