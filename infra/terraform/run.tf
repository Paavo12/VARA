# API (Cloud Run service), worker (Cloud Run worker pool) and migrations (Cloud Run job).
# CI deploys new images (gcloud run deploy …); Terraform owns everything else and ignores
# the image so the two never fight.

locals {
  internal_ip = google_compute_address.internal.address

  # Non-secret settings shared by the API and worker.
  common_env = {
    APP_ENV         = var.environment
    RUST_LOG        = "info"
    STORAGE_BACKEND = "gcs"
    GCS_BUCKET      = google_storage_bucket.documents.name
    PUBLIC_API_URL  = local.api_url
    VIRUS_SCAN      = "clamav"
    CLAMAV_ADDR     = "${local.internal_ip}:3310"
    ROUTING         = "osrm"
    OSRM_URL        = "http://${local.internal_ip}:5000"
    # SOS texts and Share trip links (API and worker both send them).
    TRIP_LINK_BASE = var.trip_link_base
  }

  # Secret env vars for the API and worker (name → Secret Manager secret).
  runtime_secrets = { for name, s in local.secrets : s.env => google_secret_manager_secret.app[name].secret_id if contains(s.readers, "api") }

  # Cloud SQL's CA, mounted as a file for `sslmode=verify-ca&sslrootcert=…` in DATABASE_URL.
  cloudsql_ca_dir = "/secrets/cloudsql"
}

resource "google_cloud_run_v2_service" "api" {
  name                = "${local.name}-api"
  location            = var.region
  ingress             = "INGRESS_TRAFFIC_ALL"
  deletion_protection = var.environment == "production"
  labels              = local.labels

  template {
    service_account       = google_service_account.runtime["api"].email
    execution_environment = "EXECUTION_ENVIRONMENT_GEN2"
    # WebSockets are long-lived requests: allow the maximum (clients reconnect after it).
    timeout = "3600s"
    # Keep a rider's or driver's reconnects on the same instance where possible.
    session_affinity                 = true
    max_instance_request_concurrency = 250

    scaling {
      min_instance_count = var.api_min_instances
      max_instance_count = var.api_max_instances
    }

    vpc_access {
      network_interfaces {
        network    = google_compute_network.vpc.id
        subnetwork = google_compute_subnetwork.main.id
      }
      egress = "PRIVATE_RANGES_ONLY"
    }

    containers {
      image = var.api_image

      ports {
        container_port = 8080
      }

      resources {
        limits = { cpu = "1", memory = "512Mi" }
        # CPU stays allocated between requests: sockets, heartbeats and the Postgres LISTEN
        # connection keep working when no HTTP request is in flight.
        cpu_idle          = false
        startup_cpu_boost = true
      }

      dynamic "env" {
        for_each = merge(local.common_env, {
          CORS_ORIGINS = join(",", var.cors_origins)
          # Cloud Run's front end appends one X-Forwarded-For entry.
          TRUSTED_PROXY_HOPS = "1"
        })
        content {
          name  = env.key
          value = env.value
        }
      }

      dynamic "env" {
        for_each = local.runtime_secrets
        content {
          name = env.key
          value_source {
            secret_key_ref {
              secret  = env.value
              version = "latest"
            }
          }
        }
      }

      env {
        name = "REDIS_CA_CERT"
        value_source {
          secret_key_ref {
            secret  = google_secret_manager_secret.redis_ca.secret_id
            version = "latest"
          }
        }
      }

      volume_mounts {
        name       = "cloudsql-ca"
        mount_path = local.cloudsql_ca_dir
      }

      startup_probe {
        http_get {
          path = "/v1/health"
        }
        period_seconds    = 3
        failure_threshold = 10
      }

      liveness_probe {
        http_get {
          path = "/v1/health"
        }
        period_seconds = 30
      }
    }

    volumes {
      name = "cloudsql-ca"
      secret {
        secret = google_secret_manager_secret.cloudsql_ca.secret_id
        items {
          path    = "server-ca.pem"
          version = "latest"
        }
      }
    }
  }

  lifecycle {
    ignore_changes = [template[0].containers[0].image, client, client_version]
  }

  depends_on = [
    google_secret_manager_secret_iam_member.readers,
    google_secret_manager_secret_iam_member.cloudsql_ca,
    google_secret_manager_secret_iam_member.redis_ca,
  ]
}

# The API is public (riders and drivers call it from their phones).
resource "google_cloud_run_v2_service_iam_member" "api_public" {
  name     = google_cloud_run_v2_service.api.name
  location = var.region
  role     = "roles/run.invoker"
  member   = "allUsers"
}

# The worker has no HTTP port: it runs the job queue, the 1-second matching tick and offer
# expiry. A worker pool keeps a fixed number of instances running with CPU always allocated
# and nothing to scale on requests, which is exactly this.
resource "google_cloud_run_v2_worker_pool" "worker" {
  name                = "${local.name}-worker"
  location            = var.region
  deletion_protection = var.environment == "production"
  labels              = local.labels

  scaling {
    scaling_mode          = "MANUAL"
    manual_instance_count = var.worker_instances
  }

  template {
    service_account = google_service_account.runtime["worker"].email

    vpc_access {
      network_interfaces {
        network    = google_compute_network.vpc.id
        subnetwork = google_compute_subnetwork.main.id
      }
      egress = "PRIVATE_RANGES_ONLY"
    }

    containers {
      image = var.worker_image

      resources {
        limits = { cpu = "1", memory = "512Mi" }
      }

      dynamic "env" {
        for_each = local.common_env
        content {
          name  = env.key
          value = env.value
        }
      }

      dynamic "env" {
        for_each = local.runtime_secrets
        content {
          name = env.key
          value_source {
            secret_key_ref {
              secret  = env.value
              version = "latest"
            }
          }
        }
      }

      env {
        name = "REDIS_CA_CERT"
        value_source {
          secret_key_ref {
            secret  = google_secret_manager_secret.redis_ca.secret_id
            version = "latest"
          }
        }
      }

      volume_mounts {
        name       = "cloudsql-ca"
        mount_path = local.cloudsql_ca_dir
      }
    }

    volumes {
      name = "cloudsql-ca"
      secret {
        secret = google_secret_manager_secret.cloudsql_ca.secret_id
        items {
          path    = "server-ca.pem"
          version = "latest"
        }
      }
    }
  }

  lifecycle {
    ignore_changes = [template[0].containers[0].image, client, client_version]
  }

  depends_on = [
    google_secret_manager_secret_iam_member.readers,
    google_secret_manager_secret_iam_member.cloudsql_ca,
    google_secret_manager_secret_iam_member.redis_ca,
  ]
}

# Applies migrations; CI runs it (and waits) before deploying a new API and worker.
resource "google_cloud_run_v2_job" "migrate" {
  name                = "${local.name}-migrate"
  location            = var.region
  deletion_protection = var.environment == "production"
  labels              = local.labels

  template {
    task_count = 1

    template {
      service_account = google_service_account.runtime["migrate"].email
      max_retries     = 0
      timeout         = "600s"

      vpc_access {
        network_interfaces {
          network    = google_compute_network.vpc.id
          subnetwork = google_compute_subnetwork.main.id
        }
        egress = "PRIVATE_RANGES_ONLY"
      }

      containers {
        image = var.migrate_image

        env {
          name = "DATABASE_URL"
          value_source {
            secret_key_ref {
              secret  = google_secret_manager_secret.app["migrate-database-url"].secret_id
              version = "latest"
            }
          }
        }

        volume_mounts {
          name       = "cloudsql-ca"
          mount_path = local.cloudsql_ca_dir
        }
      }

      volumes {
        name = "cloudsql-ca"
        secret {
          secret = google_secret_manager_secret.cloudsql_ca.secret_id
          items {
            path    = "server-ca.pem"
            version = "latest"
          }
        }
      }
    }
  }

  lifecycle {
    ignore_changes = [template[0].template[0].containers[0].image, client, client_version]
  }

  depends_on = [
    google_secret_manager_secret_iam_member.readers,
    google_secret_manager_secret_iam_member.cloudsql_ca,
  ]
}
