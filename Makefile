# fileloft — development tasks
# Run `make help` (default) for available targets.

.DEFAULT_GOAL := help

# Root of the workspace (allows `make -C path` or unusual invocations).
ROOT := $(dir $(abspath $(lastword $(MAKEFILE_LIST))))
E2E_CRATE := $(ROOT)crates/fileloft-e2e-uppy

IMAGE ?= ghcr.io/sound-systems/fileloft
# Used by test-e2e (scripts/run-e2e-tests.sh); override e.g. CHROMEDRIVER_PORT=4444 make test-e2e
CHROMEDRIVER_PORT ?= 9515

.PHONY: help setup e2e-assets e2e-server test-unit test-integration test-e2e test-all \
	docker-build-fs docker-build-s3 docker-build-gcs docker-build-azure docker-build-all

help: ## Show available targets and what they do
	@printf '\n'
	@printf '  \033[1mfileloft\033[0m — development tasks\n'
	@printf '\n'
	@grep -hE '^[a-zA-Z0-9_.-]+:.*?## ' $(MAKEFILE_LIST) | \
		awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-18s\033[0m  %s\n", $$1, $$2}' | sort
	@printf '\n'

setup: ## Fetch Rust deps and install npm packages for the e2e Uppy asset bundle
	cargo fetch --manifest-path "$(ROOT)Cargo.toml"
	cd "$(E2E_CRATE)" && npm ci

e2e-assets: ## Install npm deps and build vendored Uppy bundle (static/vendor/uppy-e2e.js)
	cd "$(E2E_CRATE)" && npm ci && npm run build

e2e-server: e2e-assets ## Build assets and start the Uppy + tus demo server on http://localhost:3000
	cargo run --manifest-path "$(ROOT)Cargo.toml" -p fileloft-e2e-uppy

test-unit: ## Run library/unit tests (workspace crates except integration + e2e + server packages)
	cargo test --manifest-path "$(ROOT)Cargo.toml" --workspace \
		--exclude fileloft-integration-tests --exclude fileloft-e2e-uppy --exclude fileloft-server

test-integration: ## Run workspace integration tests (fileloft-integration-tests)
	cargo test --manifest-path "$(ROOT)Cargo.toml" -p fileloft-integration-tests

test-e2e: e2e-assets ## Run headless e2e tests (starts chromedriver if port free; needs Chrome installed)
	CHROMEDRIVER_PORT="$(CHROMEDRIVER_PORT)" WEBDRIVER_URL="$(WEBDRIVER_URL)" "$(ROOT)scripts/run-e2e-tests.sh"

test-all: test-unit test-integration test-e2e ## Run unit, then integration, then e2e tests

docker-build-fs: ## Build the filesystem Docker image (default)
	docker build --build-arg BACKEND=fs -t $(IMAGE):latest -t $(IMAGE):fs .

docker-build-s3: ## Build the S3 Docker image
	docker build --build-arg BACKEND=s3 -t $(IMAGE):s3 .

docker-build-gcs: ## Build the GCS Docker image
	docker build --build-arg BACKEND=gcs -t $(IMAGE):gcs .

docker-build-azure: ## Build the Azure Blob Storage Docker image
	docker build --build-arg BACKEND=azure -t $(IMAGE):azure .

docker-build-all: docker-build-fs docker-build-s3 docker-build-gcs docker-build-azure ## Build all Docker images
