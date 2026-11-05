# Secret Manager (TRD §7.3). Terraform creates each secret but never its value, so no secret
# is ever in Terraform state; you add values with `gcloud secrets versions add` (README
# step 7). The two CA certificates are public and are filled in here.

locals {
  # Secret name → environment variable, and which runtime identities may read it. The worker
  # also gets JWT_SECRET and REDIS_URL because the shared config requires them at start.
  secrets = {
    "database-url"         = { env = "DATABASE_URL", readers = ["api", "worker"] }
    "migrate-database-url" = { env = "DATABASE_URL", readers = ["migrate"] }
    "redis-url"            = { env = "REDIS_URL", readers = ["api", "worker"] }
    "hash-secret"          = { env = "HASH_SECRET", readers = ["api", "worker"] }
    "data-key"             = { env = "DATA_KEY", readers = ["api", "worker"] }
    "jwt-secret"           = { env = "JWT_SECRET", readers = ["api", "worker"] }
  }

  # (secret, identity) pairs for the IAM bindings below.
  secret_readers = merge([
    for name, s in local.secrets : { for r in s.readers : "${name}/${r}" => { secret = name, reader = r } }
  ]...)
}

resource "google_secret_manager_secret" "app" {
  for_each  = local.secrets
  secret_id = "${local.name}-${each.key}"
  labels    = local.labels

  replication {
    user_managed {
      replicas {
        location = var.region
      }
    }
  }

  depends_on = [google_project_service.apis]
}

resource "google_secret_manager_secret_iam_member" "readers" {
  for_each  = local.secret_readers
  secret_id = google_secret_manager_secret.app[each.value.secret].id
  role      = "roles/secretmanager.secretAccessor"
  member    = "serviceAccount:${google_service_account.runtime[each.value.reader].email}"
}

# --- Public CA certificates, mounted as files / env vars -----------------------------------

resource "google_secret_manager_secret" "cloudsql_ca" {
  secret_id = "${local.name}-cloudsql-server-ca"
  labels    = local.labels
  replication {
    user_managed {
      replicas {
        location = var.region
      }
    }
  }
  depends_on = [google_project_service.apis]
}

resource "google_secret_manager_secret_version" "cloudsql_ca" {
  secret      = google_secret_manager_secret.cloudsql_ca.id
  secret_data = google_sql_database_instance.main.server_ca_cert[0].cert
}

resource "google_secret_manager_secret" "redis_ca" {
  secret_id = "${local.name}-redis-ca-cert"
  labels    = local.labels
  replication {
    user_managed {
      replicas {
        location = var.region
      }
    }
  }
  depends_on = [google_project_service.apis]
}

resource "google_secret_manager_secret_version" "redis_ca" {
  secret      = google_secret_manager_secret.redis_ca.id
  secret_data = join("\n", [for c in google_redis_instance.main.server_ca_certs : c.cert])
}

resource "google_secret_manager_secret_iam_member" "cloudsql_ca" {
  for_each  = toset(["api", "worker", "migrate"])
  secret_id = google_secret_manager_secret.cloudsql_ca.id
  role      = "roles/secretmanager.secretAccessor"
  member    = "serviceAccount:${google_service_account.runtime[each.value].email}"
}

resource "google_secret_manager_secret_iam_member" "redis_ca" {
  for_each  = toset(["api", "worker"])
  secret_id = google_secret_manager_secret.redis_ca.id
  role      = "roles/secretmanager.secretAccessor"
  member    = "serviceAccount:${google_service_account.runtime[each.value].email}"
}
