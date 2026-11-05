resource "google_artifact_registry_repository" "images" {
  repository_id = "fuber"
  location      = var.region
  format        = "DOCKER"
  description   = "FUber API, worker, migration and OSRM images"
  labels        = local.labels
  depends_on    = [google_project_service.apis]

  # Keep the 20 newest versions of each image; older untagged ones are deleted.
  cleanup_policies {
    id     = "keep-recent"
    action = "KEEP"
    most_recent_versions {
      keep_count = 20
    }
  }

  cleanup_policies {
    id     = "delete-old-untagged"
    action = "DELETE"
    condition {
      tag_state  = "UNTAGGED"
      older_than = "2592000s" # 30 days
    }
  }
}

locals {
  registry = "${var.region}-docker.pkg.dev/${var.project_id}/${google_artifact_registry_repository.images.repository_id}"
}
