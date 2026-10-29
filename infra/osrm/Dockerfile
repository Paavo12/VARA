# OSRM with the Namibia road network baked in, for staging and production (TRD §2.1, §8).
# Same steps as build.sh, inside the image, so the server needs no disk of its own. Rebuilt
# monthly by .github/workflows/osrm-image.yml to pick up OpenStreetMap changes.
#
#   docker build -t fuber-osrm infra/osrm

ARG OSRM_IMAGE=ghcr.io/project-osrm/osrm-backend:v5.27.1

FROM ${OSRM_IMAGE} AS data
ADD https://download.geofabrik.de/africa/namibia-latest.osm.pbf /data/namibia-latest.osm.pbf
RUN osrm-extract -p /opt/car.lua /data/namibia-latest.osm.pbf \
 && osrm-partition /data/namibia-latest.osrm \
 && osrm-customize /data/namibia-latest.osrm \
 && rm /data/namibia-latest.osm.pbf

FROM ${OSRM_IMAGE}
COPY --from=data /data /data
# The base image has no unprivileged user; nobody can read /data and needs nothing else.
USER 65534:65534
EXPOSE 5000
ENTRYPOINT ["osrm-routed", "--algorithm", "mld", "--port", "5000", "/data/namibia-latest.osrm"]
