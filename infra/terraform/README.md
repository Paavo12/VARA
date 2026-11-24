# Staging on Google Cloud

Terraform for one VARA environment in **africa-south1 (Johannesburg)**. Running these steps
creates paid resources. Nothing is created until you run `terraform apply` yourself.

## What gets created

| Piece | Google Cloud service | Notes |
| --- | --- | --- |
| API | Cloud Run service `fuber-staging-api` | Public. CPU always allocated, 3600 s request timeout (WebSockets), session affinity, min 1 instance. |
| Worker | Cloud Run worker pool `fuber-staging-worker` | 1 instance, always running (job queue, 1 s matching tick, offer expiry). No HTTP port. |
| Migrations | Cloud Run job `fuber-staging-migrate` | CI runs it and waits before each deploy. Connects as `fuber_migrator`. |
| Database | Cloud SQL for PostgreSQL 16 (+ PostGIS) | Private IP only, TLS required, daily backups + point-in-time recovery. |
| Redis | Memorystore for Redis 7.2 (Basic, 1 GB) | Private, TLS + AUTH. Rate-limit counters only. |
| ClamAV + OSRM | One e2-standard-2 VM, Container-Optimized OS | Internal IP `10.10.0.10`, no public IP. |
| Documents | Cloud Storage bucket | Uniform access, public access prevention enforced, Cloud KMS key (rotated every 90 days). |
| Images | Artifact Registry `fuber` | api, worker, migrate, osrm. |
| Secrets | Secret Manager | Terraform creates the secrets; **you** add the values. |
| Network | VPC, private services access, Cloud NAT | Cloud Run uses Direct VPC egress. |
| CI login | Workload Identity Federation | Only `main` of `Paavo12/FUber` can deploy. No key files. |

**Rough cost (staging, list prices, check the
[pricing calculator](https://cloud.google.com/products/calculator)):** Cloud SQL 1 vCPU ≈ US$55,
Memorystore Basic 1 GB ≈ US$40, VM e2-standard-2 ≈ US$55, API with min 1 instance and CPU
always on ≈ US$45, worker ≈ US$45, NAT ≈ US$35, the rest a few dollars: **about US$280 per
month**. To pause spending, scale the API min instances and worker to 0 and stop the VM and
Cloud SQL instance.

## Before you start

- A Google account with billing.
- Locally: [gcloud CLI](https://cloud.google.com/sdk/docs/install) and
  [Terraform ≥ 1.9](https://developer.hashicorp.com/terraform/install).
- Commands below are for Git Bash. Set these first, with your own values:

```bash
export PROJECT_ID=fuber-staging-123      # must be globally unique
export REGION=africa-south1
```

## First-time setup

### 1. Project and billing

```bash
gcloud auth login
gcloud auth application-default login     # Terraform uses these credentials
gcloud projects create "$PROJECT_ID" --name="VARA staging"
gcloud billing accounts list
gcloud billing projects link "$PROJECT_ID" --billing-account=XXXXXX-XXXXXX-XXXXXX
gcloud config set project "$PROJECT_ID"
```

Set a budget alert while you are there (Billing → Budgets & alerts), for example US$300.

### 2. APIs Terraform needs to start

Terraform enables the rest itself.

```bash
gcloud services enable cloudresourcemanager.googleapis.com serviceusage.googleapis.com \
  iam.googleapis.com storage.googleapis.com
```

### 3. Bucket for Terraform state

```bash
gcloud storage buckets create "gs://$PROJECT_ID-tfstate" --location="$REGION" \
  --uniform-bucket-level-access --public-access-prevention
gcloud storage buckets update "gs://$PROJECT_ID-tfstate" --versioning
```

### 4. Variables

```bash
cd infra/terraform
cp terraform.tfvars.example terraform.tfvars   # set project_id and trip_link_base; terraform.tfvars is git-ignored
terraform init -backend-config="bucket=$PROJECT_ID-tfstate"
```

### 5. First apply: everything except the apps

The API, worker and migration job read secrets that have no value yet, so create the
foundation first:

```bash
terraform apply \
  -target=google_project_service.apis \
  -target=google_service_networking_connection.services \
  -target=google_compute_router_nat.main \
  -target=google_compute_firewall.internal_services \
  -target=google_compute_firewall.iap_ssh \
  -target=google_sql_database.fuber \
  -target=google_redis_instance.main \
  -target=google_storage_bucket.documents \
  -target=google_artifact_registry_repository.images \
  -target=google_secret_manager_secret_version.cloudsql_ca \
  -target=google_secret_manager_secret_version.redis_ca \
  -target=google_secret_manager_secret.app \
  -target=google_service_account.runtime \
  -target=google_compute_instance.internal \
  -target=google_iam_workload_identity_pool_provider.github
```

Cloud SQL and Memorystore take 10–15 minutes each.

### 6. Database users

Two roles: `fuber_migrator` creates the schema (the job runs as it); `fuber_app` (API and
worker) can read and write rows but cannot change the schema. Generate two strong passwords
(`openssl rand -base64 32 | tr -d '/+='`) and keep them only long enough for step 7.

Create the migrator with gcloud. Cloud SQL makes such users members of `cloudsqlsuperuser`,
which the migrations need for `CREATE EXTENSION postgis`:

```bash
gcloud sql users create fuber_migrator --instance=fuber-staging --password='MIGRATOR_PASSWORD'
```

The database has no public IP, so connect from the internal VM, which is on the same
network (`terraform output cloud_sql_private_ip` gives `DB_IP`):

```bash
gcloud compute ssh fuber-staging-internal-services --zone=africa-south1-a --tunnel-through-iap
# on the VM:
docker run --rm -it postgres:16 psql "host=DB_IP dbname=fuber user=fuber_migrator sslmode=require"
```

Then create the app role **in SQL**, so it is not a `cloudsqlsuperuser` member, and give it
row access to everything the migrations will create:

```sql
CREATE ROLE fuber_app LOGIN PASSWORD 'APP_PASSWORD';
GRANT CONNECT ON DATABASE fuber TO fuber_app;
GRANT USAGE ON SCHEMA public TO fuber_app;
ALTER DEFAULT PRIVILEGES IN SCHEMA public GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO fuber_app;
ALTER DEFAULT PRIVILEGES IN SCHEMA public GRANT USAGE, SELECT ON SEQUENCES TO fuber_app;
ALTER DEFAULT PRIVILEGES IN SCHEMA public GRANT EXECUTE ON FUNCTIONS TO fuber_app;
\q
```

Exit the VM afterwards (`exit`). `psql` keeps no history inside the throw-away container.

### 7. Secret values

Get the hosts:

```bash
terraform output cloud_sql_private_ip      # e.g. 10.20.0.3
terraform output redis_host                # e.g. 10.20.16.4:6378
gcloud redis instances get-auth-string fuber-staging --region="$REGION"
```

Then add each value. `printf` avoids a trailing newline; nothing is written to disk:

```bash
CA=sslmode=verify-ca\&sslrootcert=/secrets/cloudsql/server-ca.pem
printf '%s' "postgres://fuber_app:APP_PASSWORD@DB_IP:5432/fuber?$CA" \
  | gcloud secrets versions add fuber-staging-database-url --data-file=-
printf '%s' "postgres://fuber_migrator:MIGRATOR_PASSWORD@DB_IP:5432/fuber?$CA" \
  | gcloud secrets versions add fuber-staging-migrate-database-url --data-file=-
printf '%s' "rediss://:REDIS_AUTH_STRING@REDIS_HOST:6378" \
  | gcloud secrets versions add fuber-staging-redis-url --data-file=-
for s in hash-secret data-key jwt-secret; do
  openssl rand -base64 48 | tr -d '\n' | gcloud secrets versions add "fuber-staging-$s" --data-file=-
done
```

If passwords contain characters other than letters and digits, URL-encode them. Never reuse
the local `.env` values; the API refuses anything containing `local-only` outside local.
Losing `data-key` makes encrypted columns unreadable, so don't delete its versions.

### 8. Full apply

```bash
terraform apply
```

This creates the API, worker and migration job with Google's placeholder images. The
API URL is in `terraform output api_url`.

### 9. GitHub

In the repository: Settings → Secrets and variables → Actions → **Variables** (not secrets;
none of these are secret):

| Variable | Value |
| --- | --- |
| `GCP_PROJECT_ID` | your project ID |
| `GCP_REGION` | `africa-south1` |
| `GCP_REGISTRY` | `terraform output -raw registry` |
| `GCP_WORKLOAD_IDENTITY_PROVIDER` | `terraform output -raw workload_identity_provider` |
| `GCP_DEPLOYER_SA` | `terraform output -raw deployer_service_account` |
| `GCP_API_SERVICE` | `fuber-staging-api` |
| `GCP_WORKER_POOL` | `fuber-staging-worker` |
| `GCP_MIGRATE_JOB` | `fuber-staging-migrate` |

The deploy job is skipped while `GCP_PROJECT_ID` is empty, so CI stays green before setup.

Then build the routing image once: Actions → **OSRM image** → Run workflow (about 10
minutes). Afterwards set `osrm_image` in `terraform.tfvars` to
`africa-south1-docker.pkg.dev/$PROJECT_ID/fuber/osrm:latest`, run `terraform apply`, and
restart the VM so it starts OSRM:

```bash
gcloud compute instances reset fuber-staging-internal-services --zone=africa-south1-a
```

### 10. First deploy

Merge anything into `main` (or re-run the latest CI run on `main`). The `deploy-staging`
job builds the images, runs the migrations, deploys the worker and the API, and calls
`/v1/health`.

## Checking staging

```bash
URL=$(terraform output -raw api_url)
curl -i "$URL/v1/health"     # 200 {"status":"ok"}: the process is up
curl -i "$URL/v1/ready"      # 200 {"db":"ok","redis":"ok"}: Postgres and Redis answer
```

If `/v1/ready` says `"redis":"unavailable"`, the API still works (rate limits fall back to
per-instance memory). Check the API logs for the TLS error and the `redis-ca-cert` secret.

Logs: Console → Cloud Run → fuber-staging-api → Logs, or
`gcloud run services logs read fuber-staging-api --region="$REGION"`.

ClamAV and OSRM on the VM:

```bash
gcloud compute ssh fuber-staging-internal-services --zone=africa-south1-a --tunnel-through-iap \
  --command='sudo systemctl status clamav osrm --no-pager; curl -s "localhost:5000/route/v1/driving/17.08,-22.56;17.09,-22.58?overview=false" | head -c 200'
```

The first ClamAV start downloads signatures, which takes a few minutes.

## Known gaps

- **The Share trip page needs a real domain.** `trip_link_base` is required: SOS texts and
  Share trip links point at it, and the API and worker refuse to start with a placeholder
  domain such as `vara.example`. The website (which serves `/t/<token>`) must be live there.

- **Uploads don't work on staging yet.** `STORAGE_BACKEND=local` writes to the container's
  own disk, which the worker can't see. The Cloud Storage backend is a later item
  (implementation plan, Phase 1 vendors); the bucket and permissions are already here.
- **No load balancer / Cloud Armor yet.** The TRD's "CDN + WAF" edge is a production step:
  an external Application Load Balancer with a serverless NEG and Cloud Armor rate rules.
- **Single zone.** Cloud SQL is zonal and Redis is Basic on staging; `environment =
  "production"` switches them to regional / Standard HA. The VM is a single point of failure
  for scanning and routing; uploads wait and quotes fail while it is down.
- **Memorystore AUTH string in Terraform state.** Terraform cannot avoid storing it. The
  state bucket is private and versioned; keep access to it limited to whoever runs Terraform.
