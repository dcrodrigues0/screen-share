variable "project_id" {
  description = "Google Cloud project ID"
  type        = string
}

variable "region" {
  description = "Google Cloud region"
  type        = string
  default     = "southamerica-east1"
}

variable "image" {
  description = "Signaling server image URI in Artifact Registry"
  type        = string
  default     = "us-docker.pkg.dev/cloudrun/container/hello"
}
