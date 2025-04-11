# Version and Directory Configuration
SNAPCHAIN_VER := v0.2.3
SNAPCHAIN_DIR := snapchain-0.2.3
PROTO_DIR := src/proto
REGISTRY ?= localhost
IMAGE_NAME := waypoint
DEFAULT_TAG := latest

.PHONY: proto clean init build run backfill-queue backfill-queue-fids backfill-queue-max backfill-worker backfill-worker-highperf backfill-update-user-data backfill-update-user-data-max backfill-block-queue backfill-block-queue-range backfill-block-worker backfill-block-worker-highperf test docker-build docker-run docker-push docker-tag fmt fmt-rust fmt-biome changelog help

init:
	mkdir -p $(PROTO_DIR)

proto: init
	curl -s -L "https://github.com/farcasterxyz/snapchain/archive/refs/tags/$(SNAPCHAIN_VER).tar.gz" \
	| tar -zxvf - -C $(PROTO_DIR) --strip-components 3 $(SNAPCHAIN_DIR)/src/proto/

build: proto
	SQLX_OFFLINE=true cargo build

clean:
	rm -rf $(PROTO_DIR)/*
	cargo clean

run: proto build
	WAYPOINT_CONFIG=config/examples/config.default.toml cargo run -- start
	

backfill-queue: proto build
	WAYPOINT_CONFIG=config/examples/config.default.toml cargo run --bin backfill -- queue

backfill-queue-fids: proto build
	@if [ "$(FIDS)" = "" ]; then \
		echo "Please specify FIDS=<comma-separated list of FIDs> to backfill"; \
		exit 1; \
	fi
	WAYPOINT_CONFIG=config/examples/config.default.toml cargo run --bin backfill -- queue --fids $(FIDS)
	
backfill-queue-max: proto build
	@if [ "$(MAX_FID)" = "" ]; then \
		echo "Please specify MAX_FID=<number> to backfill up to"; \
		exit 1; \
	fi
	WAYPOINT_CONFIG=config/examples/config.default.toml cargo run --bin backfill -- queue --max-fid $(MAX_FID)

backfill-worker: proto build
	WAYPOINT_CONFIG=config/examples/config.default.toml BACKFILL_CONCURRENCY=50 cargo run --bin backfill -- worker

# Run high-performance backfill worker with increased concurrency
backfill-worker-highperf: proto build
	WAYPOINT_CONFIG=config/examples/config.default.toml BACKFILL_CONCURRENCY=100 RUST_LOG=info cargo run --release --bin backfill -- worker

# Run the user-data update process
backfill-update-user-data: proto build
	WAYPOINT_CONFIG=config/examples/config.default.toml cargo run --release --bin backfill -- user-data

# Run the user-data update process with max FID limit
backfill-update-user-data-max: proto build
	@if [ "$(MAX_FID)" = "" ]; then \
		echo "Please specify MAX_FID=<number> to update user data up to"; \
		exit 1; \
	fi
	WAYPOINT_CONFIG=config/examples/config.default.toml cargo run --release --bin backfill -- user-data --max-fid $(MAX_FID)

# Block-based backfill queue commands
backfill-block-queue: proto build
	WAYPOINT_CONFIG=config/examples/config.default.toml cargo run --bin backfill -- block-queue

backfill-block-queue-range: proto build
	@if [ "$(START_BLOCK)" = "" ] || [ "$(END_BLOCK)" = "" ]; then \
		echo "Please specify START_BLOCK=<number> END_BLOCK=<number> for block range"; \
		exit 1; \
	fi
	WAYPOINT_CONFIG=config/examples/config.default.toml cargo run --bin backfill -- block-queue --start-block $(START_BLOCK) --end-block $(END_BLOCK)

# Block-based backfill worker commands
backfill-block-worker: proto build
	WAYPOINT_CONFIG=config/examples/config.default.toml BACKFILL_CONCURRENCY=25 cargo run --bin backfill -- block-worker

backfill-block-worker-highperf: proto build
	WAYPOINT_CONFIG=config/examples/config.default.toml BACKFILL_CONCURRENCY=50 RUST_LOG=info cargo run --release --bin backfill -- block-worker

# Run the container with proper env vars and port mapping
docker-run: docker-build
	docker run -it --rm \
		-p 8080:8080 \
		-e HOST=0.0.0.0 \
		-e PORT=8080 \
		$(REGISTRY)/$(IMAGE_NAME):$(DEFAULT_TAG)

# Build Docker image using buildx
docker-build:
	docker buildx bake

# Push Docker image to registry
docker-push: docker-build
	docker buildx bake --push

# Build and tag Docker image with specific tag
docker-tag:
	@if [ "$(TAG)" = "" ]; then \
		echo "Please specify TAG=<version> to tag the image"; \
		exit 1; \
	fi
	docker buildx bake --set "*.tags=$(REGISTRY)/$(IMAGE_NAME):$(TAG)" --push

test: proto build
	cargo test

# Format code
fmt: fmt-rust fmt-biome

# Format Rust code
fmt-rust:
	cargo fmt --all

# Format JavaScript/TypeScript with Biome
fmt-biome:
	@echo "Formatting with Biome..."
	@if command -v biome >/dev/null 2>&1; then \
		biome format --write .; \
	else \
		echo "Biome is not installed. Skipping."; \
	fi

# Generate changelog using git-cliff
changelog:
	@echo "Generating changelog..."
	@if command -v git-cliff >/dev/null 2>&1; then \
		git-cliff -o CHANGELOG.md; \
	else \
		echo "git-cliff is not installed. Please install it with 'cargo install git-cliff'."; \
		exit 1; \
	fi

# Help target - displays available commands
help:
	@echo "Waypoint Makefile Commands:"
	@echo ""
	@echo "Development:"
	@echo "  make build                    - Build the project"
	@echo "  make run                      - Run the main service"
		@echo "  make test                     - Run tests"
	@echo "  make fmt                      - Format all code"
	@echo ""
	@echo "Backfill:"
	@echo "  make backfill-queue           - Queue all FIDs for backfill"
	@echo "  make backfill-queue-fids FIDS=1,2,3  - Queue specific FIDs"
	@echo "  make backfill-queue-max MAX_FID=1000 - Queue FIDs up to 1000"
	@echo "  make backfill-worker          - Run standard worker (50 concurrent jobs)"
	@echo "  make backfill-worker-highperf - Run high-performance worker (100 concurrent jobs)"
	@echo "  make backfill-update-user-data - Update user_data for all FIDs"
	@echo "  make backfill-update-user-data-max MAX_FID=1000 - Update user_data for FIDs up to 1000"
	@echo "  make backfill-block-queue     - Queue all blocks for backfill"
	@echo "  make backfill-block-queue-range START_BLOCK=1 END_BLOCK=1000 - Queue specific block range"
	@echo "  make backfill-block-worker    - Run block-based backfill worker"
	@echo "  make backfill-block-worker-highperf - Run high-performance block worker"
	@echo ""
	@echo "Docker:"
	@echo "  make docker-build             - Build Docker image"
	@echo "  make docker-run               - Run Docker container"
	@echo "  make docker-push              - Push image to registry ($(REGISTRY)/$(IMAGE_NAME):$(DEFAULT_TAG))"
	@echo "  make docker-tag TAG=v1.0.0    - Build and push with specific tag"
	@echo ""
	@echo "Other:"
	@echo "  make clean                    - Clean build artifacts"
	@echo "  make proto                    - Generate protobuf files"
	@echo "  make changelog                - Generate changelog"
	@echo ""
	@echo "Environment Variables:"
	@echo "  REGISTRY                      - Docker registry (default: $(REGISTRY))"
	@echo "  IMAGE_NAME                    - Docker image name (default: $(IMAGE_NAME))"
	@echo "  TAG                           - Docker image tag (for docker-tag target)"
	@echo "  FIDS                          - Comma-separated list of FIDs (for backfill-queue-fids)"
	@echo "  MAX_FID                       - Maximum FID (for backfill-queue-max and backfill-update-user-data-max)"
	@echo "  START_BLOCK                   - Starting block number (for backfill-block-queue-range)"
	@echo "  END_BLOCK                     - Ending block number (for backfill-block-queue-range)"