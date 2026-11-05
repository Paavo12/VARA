# PostgreSQL 16 with PostGIS (the first migration runs CREATE EXTENSION postgis), private IP
# only, TLS required. Database users and their passwords are created with gcloud (README
# step 6) so no password is ever stored in Terraform state.

resource "google_sql_database_instance" "main" {
  name                = local.name
  database_version    = "POSTGRES_16"
  region              = var.region
  deletion_protection = true
  depends_on          = [google_service_networking_connection.services]

  settings {
    tier              = var.db_tier
    edition           = "ENTERPRISE"
    availability_type = var.environment == "production" ? "REGIONAL" : "ZONAL"
    disk_autoresize   = true
    user_labels       = local.labels

    ip_configuration {
      ipv4_enabled    = false
      private_network = google_compute_network.vpc.id
      ssl_mode        = "ENCRYPTED_ONLY"
    }

    backup_configuration {
      enabled                        = true
      point_in_time_recovery_enabled = true
      transaction_log_retention_days = 7
      backup_retention_settings {
        retained_backups = 14
      }
    }

    insights_config {
      query_insights_enabled = true
    }
  }
}

resource "google_sql_database" "fuber" {
  name     = "fuber"
  instance = google_sql_database_instance.main.name
}
