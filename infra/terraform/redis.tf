# Redis 7 for rate-limit counters (TRD §1: presence and fan-out run on Postgres). TLS and
# AUTH are on. Memorystore signs its certificate with its own CA, which the API trusts
# through REDIS_CA_CERT.
#
# Note: Terraform cannot create a Memorystore instance with AUTH without the generated AUTH
# string appearing in its state. The state bucket is private and versioned; nothing outputs
# the string, and you copy it into Secret Manager yourself (README step 7).

resource "google_redis_instance" "main" {
  name                    = local.name
  tier                    = var.environment == "production" ? "STANDARD_HA" : "BASIC"
  memory_size_gb          = var.redis_memory_gb
  region                  = var.region
  redis_version           = "REDIS_7_2"
  authorized_network      = google_compute_network.vpc.id
  connect_mode            = "PRIVATE_SERVICE_ACCESS"
  auth_enabled            = true
  transit_encryption_mode = "SERVER_AUTHENTICATION"
  labels                  = local.labels
  depends_on              = [google_service_networking_connection.services]
}
