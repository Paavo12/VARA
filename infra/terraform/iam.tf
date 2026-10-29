# Separate identities with least privilege (TRD §7.3).

resource "google_service_account" "runtime" {
  for_each = {
    api      = "FUber API (Cloud Run service)"
    worker   = "FUber worker (Cloud Run worker pool)"
    migrate  = "FUber migrations (Cloud Run job)"
    internal = "FUber internal services VM (ClamAV, OSRM)"
  }
  account_id   = "${local.name}-${each.key}"
  display_name = each.value
  depends_on   = [google_project_service.apis]
}

# Logs, metrics and traces for every runtime identity.
resource "google_project_iam_member" "telemetry" {
  for_each = {
    for pair in setproduct(keys(google_service_account.runtime), ["roles/logging.logWriter", "roles/monitoring.metricWriter", "roles/cloudtrace.agent"]) :
    "${pair[0]}/${pair[1]}" => { sa = pair[0], role = pair[1] }
  }
  project = var.project_id
  role    = each.value.role
  member  = "serviceAccount:${google_service_account.runtime[each.value.sa].email}"
}

# --- Documents bucket --------------------------------------------------------------------
# The API only issues signed URLs: V4 signing without a key file needs signBlob on its own
# identity, and a signed URL carries the signer's object permissions (create for uploads,
# read for the 5-minute reviewer links). The worker reads, re-encodes and deletes objects
# but has no signing permission, so only the API can hand out URLs.

resource "google_service_account_iam_member" "api_signs_as_itself" {
  service_account_id = google_service_account.runtime["api"].name
  role               = "roles/iam.serviceAccountTokenCreator"
  member             = "serviceAccount:${google_service_account.runtime["api"].email}"
}

resource "google_storage_bucket_iam_member" "api_documents" {
  for_each = toset(["roles/storage.objectCreator", "roles/storage.objectViewer"])
  bucket   = google_storage_bucket.documents.name
  role     = each.value
  member   = "serviceAccount:${google_service_account.runtime["api"].email}"
}

resource "google_storage_bucket_iam_member" "worker_documents" {
  bucket = google_storage_bucket.documents.name
  role   = "roles/storage.objectUser"
  member = "serviceAccount:${google_service_account.runtime["worker"].email}"
}

# --- Internal VM pulls the OSRM image ---------------------------------------------------------

resource "google_artifact_registry_repository_iam_member" "internal_pulls" {
  repository = google_artifact_registry_repository.images.name
  location   = var.region
  role       = "roles/artifactregistry.reader"
  member     = "serviceAccount:${google_service_account.runtime["internal"].email}"
}

# --- GitHub Actions deploys through Workload Identity Federation (no key files) -------------

resource "google_service_account" "deployer" {
  account_id   = "${local.name}-deployer"
  display_name = "GitHub Actions deployer"
  depends_on   = [google_project_service.apis]
}

resource "google_iam_workload_identity_pool" "github" {
  workload_identity_pool_id = "github"
  display_name              = "GitHub Actions"
  depends_on                = [google_project_service.apis]
}

resource "google_iam_workload_identity_pool_provider" "github" {
  workload_identity_pool_id          = google_iam_workload_identity_pool.github.workload_identity_pool_id
  workload_identity_pool_provider_id = "fuber"
  display_name                       = "FUber repository"

  attribute_mapping = {
    "google.subject"       = "assertion.sub"
    "attribute.repository" = "assertion.repository"
    "attribute.ref"        = "assertion.ref"
  }
  # Only this repository's deploy branch can obtain tokens: pull requests and forks cannot.
  attribute_condition = "assertion.repository == '${var.github_repository}' && assertion.ref == 'refs/heads/${var.deploy_branch}'"

  oidc {
    issuer_uri = "https://token.actions.githubusercontent.com"
  }
}

resource "google_service_account_iam_member" "github_impersonates_deployer" {
  service_account_id = google_service_account.deployer.name
  role               = "roles/iam.workloadIdentityUser"
  member             = "principalSet://iam.googleapis.com/${google_iam_workload_identity_pool.github.name}/attribute.repository/${var.github_repository}"
}

resource "google_artifact_registry_repository_iam_member" "deployer_pushes" {
  repository = google_artifact_registry_repository.images.name
  location   = var.region
  role       = "roles/artifactregistry.writer"
  member     = "serviceAccount:${google_service_account.deployer.email}"
}

# Deploy new revisions and run the migration job (not create services or change IAM).
resource "google_project_iam_member" "deployer_run" {
  project = var.project_id
  role    = "roles/run.developer"
  member  = "serviceAccount:${google_service_account.deployer.email}"
}

# Deploying a revision that runs as a service account requires actAs on that account.
resource "google_service_account_iam_member" "deployer_acts_as_runtime" {
  for_each           = toset(["api", "worker", "migrate"])
  service_account_id = google_service_account.runtime[each.value].name
  role               = "roles/iam.serviceAccountUser"
  member             = "serviceAccount:${google_service_account.deployer.email}"
}
