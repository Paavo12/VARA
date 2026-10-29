variable "project_id" {
  description = "Google Cloud project ID for this environment."
  type        = string
}

variable "region" {
  description = "Region for every regional resource. Johannesburg is the closest to Namibia (TRD §2.3)."
  type        = string
  default     = "africa-south1"
}

variable "zone" {
  description = "Zone for the internal services VM."
  type        = string
  default     = "africa-south1-a"
}

variable "environment" {
  description = "Environment name, used in resource names and APP_ENV."
  type        = string
  default     = "staging"

  validation {
    condition     = contains(["staging", "production"], var.environment)
    error_message = "environment must be staging or production."
  }
}

variable "github_repository" {
  description = "GitHub repository allowed to deploy, as owner/name."
  type        = string
  default     = "Paavo12/FUber"
}

variable "deploy_branch" {
  description = "Only workflows running on this branch can deploy."
  type        = string
  default     = "main"
}

variable "cors_origins" {
  description = "Exact origins allowed by the API's CORS (web apps and Capacitor shells)."
  type        = list(string)
  default     = ["capacitor://localhost", "https://localhost"]
}

variable "db_tier" {
  description = "Cloud SQL machine type. Staging is small; production needs high availability (see README)."
  type        = string
  default     = "db-custom-1-3840"
}

variable "redis_memory_gb" {
  description = "Memorystore capacity. Redis only holds rate-limit counters."
  type        = number
  default     = 1
}

variable "api_min_instances" {
  description = "API instances kept running. At least 1: they hold WebSockets and the Postgres LISTEN connection."
  type        = number
  default     = 1
}

variable "api_max_instances" {
  description = "Upper bound for API autoscaling."
  type        = number
  default     = 4
}

variable "worker_instances" {
  description = "Worker instances. The matching tick is safe with several (SKIP LOCKED), but one is enough for staging."
  type        = number
  default     = 1
}

variable "internal_vm_machine_type" {
  description = "Machine for ClamAV and OSRM. ClamAV needs about 3 GB of memory for its signatures."
  type        = string
  default     = "e2-standard-2"
}

# Placeholder images let the first `terraform apply` succeed before CI has pushed anything.
# Afterwards CI deploys new images and Terraform ignores the image field.
variable "api_image" {
  type    = string
  default = "us-docker.pkg.dev/cloudrun/container/hello"
}

variable "worker_image" {
  type    = string
  default = "us-docker.pkg.dev/cloudrun/container/worker-pool:latest"
}

variable "migrate_image" {
  type    = string
  default = "us-docker.pkg.dev/cloudrun/container/job:latest"
}

variable "osrm_image" {
  description = "OSRM image with the Namibia data, pushed by the osrm-image workflow. Empty until then (OSRM is not started)."
  type        = string
  default     = ""
}

variable "clamav_image" {
  type    = string
  default = "docker.io/clamav/clamav:1.4"
}
