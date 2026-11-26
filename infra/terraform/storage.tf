# Private bucket for identity documents (TRD §2.3, §7.6): uniform access, public access
# prevention enforced, encrypted with our own Cloud KMS key (the Google Cloud counterpart of
# the TRD's "SSE-KMS").

resource "google_kms_key_ring" "main" {
  name       = local.name
  location   = var.region
  depends_on = [google_project_service.apis]
}

resource "google_kms_crypto_key" "documents" {
  name            = "documents"
  key_ring        = google_kms_key_ring.main.id
  rotation_period = "7776000s" # 90 days

  lifecycle {
    prevent_destroy = true
  }
}

# Cloud Storage encrypts with the key on our behalf.
data "google_storage_project_service_account" "gcs" {}

resource "google_kms_crypto_key_iam_member" "gcs_uses_documents_key" {
  crypto_key_id = google_kms_crypto_key.documents.id
  role          = "roles/cloudkms.cryptoKeyEncrypterDecrypter"
  member        = "serviceAccount:${data.google_storage_project_service_account.gcs.email_address}"
}

resource "google_storage_bucket" "documents" {
  name                        = "${var.project_id}-${var.environment}-documents"
  location                    = var.region
  storage_class               = "STANDARD"
  uniform_bucket_level_access = true
  public_access_prevention    = "enforced"
  force_destroy               = false
  labels                      = local.labels

  encryption {
    default_kms_key_name = google_kms_crypto_key.documents.id
  }

  # The phone apps upload straight to the bucket through signed URLs, so the WebView asks
  # first (CORS). Reviewers' images are plain <img> loads and need none.
  cors {
    origin          = var.cors_origins
    method          = ["PUT"]
    response_header = ["Content-Type", "X-Goog-Content-Length-Range"]
    max_age_seconds = 3600
  }

  # Documents are deleted by the application's retention jobs; soft delete keeps an
  # accidental deletion recoverable for a week.
  soft_delete_policy {
    retention_duration_seconds = 604800
  }

  depends_on = [google_kms_crypto_key_iam_member.gcs_uses_documents_key]
}
