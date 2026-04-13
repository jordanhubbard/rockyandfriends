# CCC — Makefile
# Entry points for common operator tasks.
# All the actual logic lives in deploy/ scripts.

.PHONY: help init register build test release clean \
        docker-build docker-up docker-down docker-logs

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | \
		awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-20s\033[0m %s\n", $$1, $$2}'

# ── Onboarding ─────────────────────────────────────────────────────────────

init: ## Interactive setup: configure this node
	@bash deploy/ccc-init.sh

register: ## Register this agent with the CCC hub
	@bash deploy/register-agent.sh

# ── Build ──────────────────────────────────────────────────────────────────

build: ## Build all Rust binaries (ccc-server, ccc-agent)
	@cargo build --release --manifest-path ccc/dashboard/Cargo.toml

# ── Testing ────────────────────────────────────────────────────────────────

test: ## Run all Rust tests
	@cargo test --manifest-path ccc/dashboard/Cargo.toml

# ── Release ────────────────────────────────────────────────────────────────

release: ## Cut a release: bump patch version, update CHANGELOG, tag, push, GH release
	@bash scripts/release.sh patch

release-minor: ## Cut a minor release
	@bash scripts/release.sh minor

release-major: ## Cut a major release
	@bash scripts/release.sh major

# ── Docker (Operator path) ─────────────────────────────────────────────────

docker-build: ## Build the CCC Docker image locally
	docker build -t ccc:local .

docker-up: ## Start the full CCC stack via Docker Compose
	docker compose up -d

docker-down: ## Stop the CCC Docker stack
	docker compose down

docker-logs: ## Tail logs from all CCC containers
	docker compose logs -f

# ── Utilities ──────────────────────────────────────────────────────────────

clean: ## Remove build artifacts
	@cargo clean --manifest-path ccc/dashboard/Cargo.toml
	@echo "Cleaned build artifacts."
