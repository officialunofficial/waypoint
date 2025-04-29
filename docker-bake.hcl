variable "TAG" {
    default = "latest"
}

variable "REGISTRY" {
    default = "docker.io/officialunofficial"
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
