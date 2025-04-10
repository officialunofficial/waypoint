variable "TAG" {
    default = "latest"
}

variable "REGISTRY" {
    default = "officialunofficial"
}

// Define the platforms to build for
variable "PLATFORMS" {
    default = ["linux/amd64", "linux/arm64"]
}

// Default build target for Docker Hub
target "default" {
    context = "."
    dockerfile = "Dockerfile"
    platforms = var.PLATFORMS
    tags = [
        "${REGISTRY}/waypoint:${TAG}",
        "${REGISTRY}/waypoint:latest"
    ]
}

// GCR build target for backward compatibility
target "gcr" {
    context = "."
    dockerfile = "Dockerfile"
    platforms = ["linux/amd64"]
    tags = [
        "gcr.io/official-unofficial/waypoint:${TAG}",
        "gcr.io/official-unofficial/waypoint:latest"
    ]
}