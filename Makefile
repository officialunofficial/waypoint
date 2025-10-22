# Version and Directory Configuration
SNAPCHAIN_VER := v0.10.0
SNAPCHAIN_DIR := snapchain-0.10.0
PROTO_DIR := src/proto
REGISTRY ?= localhost
IMAGE_NAME := waypoint
DEFAULT_TAG := latest
ARGS ?= 

# Import environment variables from .env file if it exists
ifneq (,$(wildcard ./.env))
	include .env
	export
endif

.PHONY: proto clean init build run backfill-queue backfill-queue-fids backfill-queue-max backfill-worker backfill-update-user-data backfill-update-user-data-max backfill-onchain-all backfill-onchain-fid backfill-onchain-missing migrate test docker-build docker-run docker-push docker-tag metrics-start metrics-stop metrics-open fmt fmt-rust fmt-biome changelog help env-setup

init:
	mkdir -p $(PROTO_DIR)

# Create a .env file from .env.example if it doesn't exist
env-setup:
	@if [ ! -f .env ]; then \
		echo "Creating .env file from .env.example"; \
		cp .env.example .env; \
		echo "Created .env file. Please edit it with your configuration."; \
	else \
		echo ".env file already exists. Skipping."; \
	fi

proto: init
	curl -s -L "https://github.com/farcasterxyz/snapchain/archive/refs/tags/$(SNAPCHAIN_VER).tar.gz" \
	| tar -zxvf - -C $(PROTO_DIR) --strip-components 3 $(SNAPCHAIN_DIR)/src/proto/

build: proto
	SQLX_OFFLINE=true cargo build

clean:
	rm -rf $(PROTO_DIR)/*
	cargo clean

run: proto build env-setup
	cargo run -- start $(ARGS)

backfill-queue: proto build env-setup
	@echo "Queuing FIDs for backfill (using sum of all FIDs in shards as max)"
	cargo run -- backfill fid queue $(ARGS)

backfill-queue-fids: proto build env-setup
	@if [ "$(FIDS)" = "" ]; then \
		echo "Please specify FIDS=<comma-separated list of FIDs> to backfill"; \
		exit 1; \
	fi
	cargo run -- backfill fid queue --fids $(FIDS) $(ARGS)
	
backfill-queue-max: proto build env-setup
	@if [ "$(MAX_FID)" = "" ]; then \
		echo "Please specify MAX_FID=<number> to backfill up to"; \
		exit 1; \
	fi
	cargo run -- backfill fid queue --max-fid $(MAX_FID) $(ARGS)

backfill-worker: proto build env-setup
	BACKFILL_CONCURRENCY=50 cargo run -- backfill fid worker $(ARGS)

# Run the user-data update process
backfill-update-user-data: proto build env-setup
	cargo run --release -- backfill fid user-data $(ARGS)

# Run the user-data update process with max FID limit
backfill-update-user-data-max: proto build env-setup
	@if [ "$(MAX_FID)" = "" ]; then \
		echo "Please specify MAX_FID=<number> to update user data up to"; \
		exit 1; \
	fi
	cargo run --release -- backfill fid user-data --max-fid $(MAX_FID) $(ARGS)

# Database migration commands
migrate: env-setup
	@echo "Running database migrations..."
	@echo "Note: Migrations will be automatically applied when the application starts"
	@echo "To run migrations manually, execute the SQL files in migrations/ directory"
	@echo "Latest migration: migrations/004_add_lend_storage_table.sql"

# Onchain events backfill commands
backfill-onchain-all: proto build env-setup
	@echo "Backfilling onchain events for all FIDs (this may take a while)"
	@echo "Use CTRL+C to cancel if needed"
	@echo "This will backfill signer, signer_migrated, id_register, storage_rent, and tier_purchase events"
	SQLX_OFFLINE=true cargo run --release -- backfill onchain-events --all $(ARGS)

backfill-onchain-fid: proto build env-setup
	@if [ "$(FID)" = "" ]; then \
		echo "Please specify FID=<fid_number> to backfill onchain events for"; \
		exit 1; \
	fi
	@echo "Backfilling onchain events for FID: $(FID)"
	SQLX_OFFLINE=true cargo run --release -- backfill onchain-events --fid $(FID) $(ARGS)

backfill-onchain-missing: proto build env-setup
	@if [ "$(EVENT_TYPE)" = "" ]; then \
		echo "Please specify EVENT_TYPE=<signer|signer_migrated|id_register|storage_rent|tier_purchase> to backfill missing events"; \
		echo "Example: make backfill-onchain-missing EVENT_TYPE=signer"; \
		exit 1; \
	fi
	@echo "Backfilling missing $(EVENT_TYPE) events for all FIDs"
	SQLX_OFFLINE=true cargo run --release -- backfill onchain-events --event-type $(EVENT_TYPE) $(ARGS)


# Run the container with proper env vars and port mapping
docker-run: env-setup
	docker run -it --rm \
		-p ${PORT:-8080}:${PORT:-8080} \
		--env-file .env \
		-e WAYPOINT_DATABASE__URL=postgresql://${POSTGRES_USER:-postgres}:${POSTGRES_PASSWORD:-postgres}@host.docker.internal:5432/${POSTGRES_DB:-waypoint} \
		-e WAYPOINT_REDIS__URL=redis://host.docker.internal:6379 \
		-e HOST=0.0.0.0 \
		$(REGISTRY)/$(IMAGE_NAME):$(DEFAULT_TAG)

# Build Docker image using buildx
docker-build:
	docker buildx build --platform linux/amd64,linux/arm64 -t $(REGISTRY)/$(IMAGE_NAME):$(DEFAULT_TAG) .

# Push Docker image to registry
docker-push:
	docker buildx build --platform linux/amd64,linux/arm64 -t $(REGISTRY)/$(IMAGE_NAME):$(DEFAULT_TAG) . --push

# Build and tag Docker image with specific tag
docker-tag:
	@if [ "$(TAG)" = "" ]; then \
		echo "Please specify TAG=<version> to tag the image"; \
		exit 1; \
	fi
	docker buildx build --platform linux/amd64,linux/arm64 -t $(REGISTRY)/$(IMAGE_NAME):$(TAG) . --push

test: proto build env-setup
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

# Metrics commands
metrics-start:
	@echo "Starting metrics infrastructure..."
	cd grafana && docker-compose up -d

metrics-stop:
	@echo "Stopping metrics infrastructure..."
	cd grafana && docker-compose down

metrics-open:
	@echo "Opening Grafana dashboard in browser..."
	open http://localhost:3000 || xdg-open http://localhost:3000 || echo "Could not open browser automatically. Please visit http://localhost:3000"

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
	@echo "Environment Setup:"
	@echo "  make env-setup                - Create .env file from .env.example if it doesn't exist"
	@echo ""
	@echo "Development:"
	@echo "  make build                    - Build the project"
	@echo "  make run                      - Run the main service"
	@echo "  make test                     - Run tests"
	@echo "  make fmt                      - Format all code"
	@echo ""
	@echo "Database:"
	@echo "  make migrate                  - Show database migration information"
	@echo ""
	@echo "Backfill:"
	@echo "  FID-Based Backfill:"
	@echo "  make backfill-queue           - Queue all FIDs for backfill (using sum of FIDs from all shards)"
	@echo "  make backfill-queue-fids FIDS=1,2,3  - Queue specific FIDs"
	@echo "  make backfill-queue-max MAX_FID=1000 - Queue FIDs up to 1000"
	@echo "  make backfill-worker          - Run backfill worker (50 concurrent jobs by default)"
	@echo "  make backfill-update-user-data - Update user_data for all FIDs"
	@echo "  make backfill-update-user-data-max MAX_FID=1000 - Update user_data for FIDs up to 1000"
	@echo ""
	@echo "  Onchain Events Backfill:"
	@echo "  make backfill-onchain-all     - Backfill onchain events for all FIDs (comprehensive)"
	@echo "  make backfill-onchain-fid FID=12345 - Backfill onchain events for specific FID"
	@echo "  make backfill-onchain-missing EVENT_TYPE=signer - Backfill missing events of specific type"
	@echo ""
	@echo "Docker:"
	@echo "  make docker-build             - Build Docker image"
	@echo "  make docker-run               - Run Docker container using environment from .env"
	@echo "  make docker-push              - Push image to registry ($(REGISTRY)/$(IMAGE_NAME):$(DEFAULT_TAG))"
	@echo "  make docker-tag TAG=v1.0.0    - Build and push with specific tag"
	@echo ""
	@echo "Metrics:"
	@echo "  make metrics-start            - Start metrics infrastructure (Grafana, etc.)"
	@echo "  make metrics-stop             - Stop metrics infrastructure"
	@echo "  make metrics-open             - Open Grafana dashboard in browser (http://localhost:3000)"
	@echo ""
	@echo "Other:"
	@echo "  make clean                    - Clean build artifacts"
	@echo "  make proto                    - Generate protobuf files"
	@echo "  make changelog                - Generate changelog"
	@echo ""
	@echo "Environment Variables:"
	@echo "  All variables can be defined in .env file or passed on command line"
	@echo "  REGISTRY                      - Docker registry (default: $(REGISTRY))"
	@echo "  IMAGE_NAME                    - Docker image name (default: $(IMAGE_NAME))"
	@echo "  TAG                           - Docker image tag (for docker-tag target)"
	@echo "  FIDS                          - Comma-separated list of FIDs (for backfill-queue-fids)"
	@echo "  MAX_FID                       - Maximum FID (for backfill-queue-max and backfill-update-user-data-max)"
	@echo "  FID                           - Single FID number (for backfill-onchain-fid)"
	@echo "  EVENT_TYPE                    - Onchain event type: signer, signer_migrated, id_register, storage_rent, tier_purchase"
	@echo "  ARGS                          - Additional command line arguments to pass to the binary"
	@echo "  PORT                          - Port to run the service on (default: 8080)"
	@echo "  See .env.example for all available configuration options"