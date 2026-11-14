resource "google_project_service" "apis" {
  for_each = toset([
    "artifactregistry.googleapis.com",
    "run.googleapis.com",
  ])

  project            = var.project_id
  service            = each.value
  disable_on_destroy = false
}

resource "google_artifact_registry_repository" "signaling" {
  location      = var.region
  repository_id = "signaling"
  description   = "Container images for the screen-share signaling service"
  format        = "DOCKER"

  depends_on = [google_project_service.apis]
}

resource "google_cloud_run_v2_service" "signaling" {
  name                = "signaling"
  location            = var.region
  deletion_protection = false
  ingress             = "INGRESS_TRAFFIC_ALL"

  template {
    session_affinity                 = true
    timeout                          = "3600s"
    max_instance_request_concurrency = 1000

    scaling {
      min_instance_count = 1
      max_instance_count = 1
    }

    containers {
      image = var.image

      ports {
        container_port = 8080
      }

    }
  }

  depends_on = [google_project_service.apis]
}

resource "google_cloud_run_v2_service_iam_member" "public_invoker" {
  project  = var.project_id
  location = google_cloud_run_v2_service.signaling.location
  name     = google_cloud_run_v2_service.signaling.name
  role     = "roles/run.invoker"
  member   = "allUsers"
}

output "signaling_ws_url" {
  description = "WebSocket endpoint for the Tauri client"
  value       = replace(google_cloud_run_v2_service.signaling.uri, "https://", "wss://")
}
