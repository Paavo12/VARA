# Private network: Cloud Run reaches Cloud SQL, Memorystore and the internal VM through
# Direct VPC egress; nothing here has a public IP.

resource "google_compute_network" "vpc" {
  name                    = local.name
  auto_create_subnetworks = false
  depends_on              = [google_project_service.apis]
}

resource "google_compute_subnetwork" "main" {
  name                     = "${local.name}-${var.region}"
  network                  = google_compute_network.vpc.id
  region                   = var.region
  ip_cidr_range            = "10.10.0.0/24"
  private_ip_google_access = true
}

# Address range for Google-managed services (Cloud SQL, Memorystore) peered into the VPC.
resource "google_compute_global_address" "services" {
  name          = "${local.name}-services"
  purpose       = "VPC_PEERING"
  address_type  = "INTERNAL"
  prefix_length = 20
  network       = google_compute_network.vpc.id
}

resource "google_service_networking_connection" "services" {
  network                 = google_compute_network.vpc.id
  service                 = "servicenetworking.googleapis.com"
  reserved_peering_ranges = [google_compute_global_address.services.name]
}

# Outbound internet for the VM (image pulls, ClamAV signature updates). Cloud Run sends only
# private ranges through the VPC, so its own internet traffic does not use NAT.
resource "google_compute_router" "main" {
  name    = local.name
  network = google_compute_network.vpc.id
  region  = var.region
}

resource "google_compute_router_nat" "main" {
  name                               = local.name
  router                             = google_compute_router.main.name
  region                             = var.region
  nat_ip_allocate_option             = "AUTO_ONLY"
  source_subnetwork_ip_ranges_to_nat = "ALL_SUBNETWORKS_ALL_IP_RANGES"

  log_config {
    enable = true
    filter = "ERRORS_ONLY"
  }
}

# Cloud Run (API and worker) → ClamAV and OSRM on the internal VM.
resource "google_compute_firewall" "internal_services" {
  name          = "${local.name}-internal-services"
  network       = google_compute_network.vpc.id
  direction     = "INGRESS"
  source_ranges = [google_compute_subnetwork.main.ip_cidr_range]
  target_tags   = ["internal-services"]

  allow {
    protocol = "tcp"
    ports    = ["3310", "5000"]
  }
}

# SSH to the VM only through Identity-Aware Proxy (`gcloud compute ssh --tunnel-through-iap`).
resource "google_compute_firewall" "iap_ssh" {
  name          = "${local.name}-iap-ssh"
  network       = google_compute_network.vpc.id
  direction     = "INGRESS"
  source_ranges = ["35.235.240.0/20"]
  target_tags   = ["internal-services"]

  allow {
    protocol = "tcp"
    ports    = ["22"]
  }
}
