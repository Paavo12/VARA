# VARA on Google Cloud: one environment per project (TRD §2.3). See README.md for the
# first-time setup and docs/02-technical-requirements.md for why each piece exists.

data "google_project" "this" {}

locals {
  name = "fuber-${var.environment}"
  # Cloud Run's deterministic URL, known before the service exists (signed upload links
  # point here). https://cloud.google.com/run/docs/triggering/https-request#deterministic
  api_url = "https://${local.name}-api-${data.google_project.this.number}.${var.region}.run.app"
  labels  = { app = "fuber", environment = var.environment }
}

resource "google_project_service" "apis" {
  for_each = toset([
    "artifactregistry.googleapis.com",
    "cloudkms.googleapis.com",
    "compute.googleapis.com",
    "iam.googleapis.com",
    "iamcredentials.googleapis.com",
    "iap.googleapis.com",
    "redis.googleapis.com",
    "run.googleapis.com",
    "secretmanager.googleapis.com",
    "servicenetworking.googleapis.com",
    "sqladmin.googleapis.com",
    "storage.googleapis.com",
    "sts.googleapis.com",
  ])
  service            = each.value
  disable_on_destroy = false
}
