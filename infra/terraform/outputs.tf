output "api_url" {
  description = "Public URL of the API. Check it with: curl <api_url>/v1/health"
  value       = local.api_url
}

output "registry" {
  description = "Artifact Registry path for images (GitHub variable GCP_REGISTRY)."
  value       = local.registry
}

output "workload_identity_provider" {
  description = "Full provider name for google-github-actions/auth (GitHub variable GCP_WORKLOAD_IDENTITY_PROVIDER)."
  value       = google_iam_workload_identity_pool_provider.github.name
}

output "deployer_service_account" {
  description = "Service account GitHub Actions impersonates (GitHub variable GCP_DEPLOYER_SA)."
  value       = google_service_account.deployer.email
}

output "cloud_sql_private_ip" {
  description = "Host for DATABASE_URL."
  value       = google_sql_database_instance.main.private_ip_address
}

output "redis_host" {
  description = "Host and TLS port for REDIS_URL (rediss://:<auth>@host:port)."
  value       = "${google_redis_instance.main.host}:${google_redis_instance.main.port}"
}

output "documents_bucket" {
  value = google_storage_bucket.documents.name
}

output "names" {
  description = "Resource names CI needs (GitHub variables)."
  value = {
    api_service = google_cloud_run_v2_service.api.name
    worker_pool = google_cloud_run_v2_worker_pool.worker.name
    migrate_job = google_cloud_run_v2_job.migrate.name
  }
}
