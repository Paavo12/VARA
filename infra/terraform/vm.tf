# ClamAV and OSRM on one small VM with no public IP. Neither fits Cloud Run: clamd speaks raw
# TCP (Cloud Run only serves HTTP), and the config requires plain HTTP for OSRM on the
# private network while Cloud Run URLs are HTTPS-only. Container-Optimized OS runs both
# containers from systemd units defined in cloud-init.

resource "google_compute_address" "internal" {
  name         = "${local.name}-internal-services"
  region       = var.region
  subnetwork   = google_compute_subnetwork.main.id
  address_type = "INTERNAL"
  address      = "10.10.0.10"
}

locals {
  cloud_init = <<-EOT
    #cloud-config
    write_files:
      - path: /etc/systemd/system/clamav.service
        permissions: "0644"
        content: |
          [Unit]
          Description=ClamAV daemon (upload scanning)
          Wants=network-online.target
          After=network-online.target docker.service
          [Service]
          ExecStartPre=-/usr/bin/docker rm -f clamav
          ExecStart=/usr/bin/docker run --rm --name clamav -p 3310:3310 -v clamav-db:/var/lib/clamav ${var.clamav_image}
          ExecStop=/usr/bin/docker stop clamav
          Restart=always
          RestartSec=10
      - path: /etc/systemd/system/osrm.service
        permissions: "0644"
        content: |
          [Unit]
          Description=OSRM routing (Namibia)
          Wants=network-online.target
          After=network-online.target docker.service
          [Service]
          Environment=HOME=/home/osrm
          ExecStartPre=/usr/bin/mkdir -p /home/osrm
          ExecStartPre=/usr/bin/docker-credential-gcr configure-docker --registries=${var.region}-docker.pkg.dev
          ExecStartPre=-/usr/bin/docker rm -f osrm
          ExecStartPre=/usr/bin/docker pull ${var.osrm_image}
          ExecStart=/usr/bin/docker run --rm --name osrm -p 5000:5000 ${var.osrm_image}
          ExecStop=/usr/bin/docker stop osrm
          Restart=always
          RestartSec=10
    runcmd:
      - systemctl daemon-reload
      - systemctl start clamav.service
      %{if var.osrm_image != ""}- systemctl start osrm.service%{endif}
  EOT
}

resource "google_compute_instance" "internal" {
  name         = "${local.name}-internal-services"
  zone         = var.zone
  machine_type = var.internal_vm_machine_type
  tags         = ["internal-services"]
  labels       = local.labels

  boot_disk {
    initialize_params {
      image = "cos-cloud/cos-stable"
      size  = 20
      type  = "pd-balanced"
    }
  }

  network_interface {
    subnetwork = google_compute_subnetwork.main.id
    network_ip = google_compute_address.internal.address
    # No access_config: no public IP. Outbound goes through Cloud NAT.
  }

  service_account {
    email  = google_service_account.runtime["internal"].email
    scopes = ["cloud-platform"]
  }

  metadata = {
    user-data              = local.cloud_init
    block-project-ssh-keys = "true"
    enable-oslogin         = "TRUE"
    google-logging-enabled = "true"
  }

  shielded_instance_config {
    enable_secure_boot          = true
    enable_vtpm                 = true
    enable_integrity_monitoring = true
  }

  allow_stopping_for_update = true
}
