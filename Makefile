# ============================================================
# Ignite Pay — Deployment Commands
# ============================================================

.PHONY: help build up down logs ps health init keys \
        build-app build-merchant clean \
        test test-unit test-integration build-sbf test-svm test-svm-all \
        test-unit-state-channel test-integration-state-channel \
        build-sbf-state-channel test-svm-state-channel

# --------------- Configuration ---------------
DOCKER_COMPOSE = docker compose
ENV_FILE = .env

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | \
		awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-22s\033[0m %s\n", $$1, $$2}'

# --------------- Prerequisites ---------------

init: ## Copy .env.example to .env (first time only)
	@if [ ! -f $(ENV_FILE) ]; then \
		cp .env.example $(ENV_FILE); \
		echo "Created .env — edit it with real values before deploying"; \
	else \
		echo ".env already exists, skipping"; \
	fi

keys: ## Create deploy/keys/ directory with placeholder structure
	@mkdir -p deploy/keys
	@echo "Place Solana keypair files in deploy/keys/:"
	@echo "  user.key      — Channel User keypair"
	@echo "  provider.key  — Channel Provider keypair"
	@echo "  hub.key       — Channel Hub keypair"
	@echo "  payer.key     — DID Registry payer keypair"

certs: ## Create deploy/certs/ directory for TLS
	@mkdir -p deploy/certs
	@echo "Place tls.crt and tls.key in deploy/certs/"

# --------------- Docker Build ---------------

build: ## Build all Docker images
	$(DOCKER_COMPOSE) build

build-service: ## Build a single service. Usage: make build-service S=did-registry
	$(DOCKER_COMPOSE) build $(S)

# --------------- Docker Lifecycle ---------------

up: ## Start all services (detached)
	$(DOCKER_COMPOSE) up -d

down: ## Stop all services
	$(DOCKER_COMPOSE) down

restart: ## Restart all services
	$(DOCKER_COMPOSE) restart

ps: ## Show running containers
	$(DOCKER_COMPOSE) ps

logs: ## Tail logs for all services (or make logs S=hub-registry)
	$(DOCKER_COMPOSE) logs -f $(S)

# --------------- Health & Debug ---------------

health: ## Check health of all services
	@echo "--- PostgreSQL ---"
	@$(DOCKER_COMPOSE) exec postgres pg_isready -U ignite 2>/dev/null || echo "not ready"
	@echo "--- Hub Registry ---"
	@curl -sf http://localhost:3004/v1/hubs >/dev/null 2>&1 && echo "OK" || echo "not reachable (direct :3004)"
	@echo "--- DIDComm Router (user) ---"
	@curl -sf http://localhost:8080/health >/dev/null 2>&1 && echo "OK" || echo "not reachable (direct :8080)"
	@echo "--- DIDComm Router (merchant) ---"
	@curl -sf http://localhost:4000/health >/dev/null 2>&1 && echo "OK" || echo "not reachable (direct :4000)"
	@echo "--- DID Registry ---"
	@curl -sf http://localhost:8081/health >/dev/null 2>&1 && echo "OK" || echo "not reachable (direct :8081)"
	@echo "--- Channel User ---"
	@curl -sf http://localhost:3001/health >/dev/null 2>&1 && echo "OK" || echo "not reachable (direct :3001)"
	@echo "--- Channel Provider ---"
	@curl -sf http://localhost:3002/health >/dev/null 2>&1 && echo "OK" || echo "not reachable (direct :3002)"
	@echo "--- Channel Hub ---"
	@curl -sf http://localhost:3003/health >/dev/null 2>&1 && echo "OK" || echo "not reachable (direct :3003)"

# --------------- Flutter Mobile Apps ---------------

build-app: ## Build consumer Android APK
	cd ignite_pay_app && flutter build apk --split-per-abi

build-merchant: ## Build merchant Android APK
	cd ignite_pay_merchant_app && flutter build apk --split-per-abi

# --------------- Cleanup ---------------

clean: ## Remove all containers, volumes, and built images
	$(DOCKER_COMPOSE) down -v --rmi local

clean-data: ## Remove data volumes only (keeps images)
	$(DOCKER_COMPOSE) down -v

# --------------- Testing ---------------

test: test-unit test-integration test-svm   ## Run all tests (unit + integration + SVM)

test-unit:   ## Run unit tests for all crates (no .so needed)
	@echo "=== Running unit tests ==="
	cargo test --manifest-path ignite-pay-core/Cargo.toml
	cargo test --manifest-path ignite-pay-solana/Cargo.toml
	cargo test --manifest-path ignite-pay-mcp/Cargo.toml
	cargo test --manifest-path ignite-pay-merchant-mcp/Cargo.toml
	cargo test --manifest-path didcomm-router/Cargo.toml
	cargo test --manifest-path did-registry/Cargo.toml
	cargo test --manifest-path ignite-pay-did-program/Cargo.toml
	cargo test --manifest-path ignite-pay-mb/sdk/Cargo.toml
	cargo test --manifest-path ignite-pay-mb/programs/ignite-pay-mb/Cargo.toml
	@echo "=== Unit tests passed ==="

test-unit-state-channel:   ## Run unit tests for state-channel crates
	@echo "=== Running state-channel unit tests ==="
	cargo test --manifest-path ignite-pay-state-channel/Cargo.toml
	cargo test --manifest-path ignite-pay-channel-service/Cargo.toml
	cargo test --manifest-path ignite-pay-program/Cargo.toml
	@echo "=== State-channel unit tests passed ==="

test-integration:   ## Run integration tests
	@echo "=== Running integration tests ==="
	cargo test --manifest-path ignite-pay-solana/Cargo.toml --test session_integration
	cargo test --manifest-path ignite-pay-mcp/Cargo.toml --test flow_tests
	cargo test --manifest-path didcomm-router/Cargo.toml --test integration_test
	@echo "=== Integration tests passed ==="

test-integration-state-channel:   ## Run integration tests for state-channel crates
	@echo "=== Running state-channel integration tests ==="
	cargo test --manifest-path ignite-pay-state-channel/Cargo.toml --test channel_tests --test signing_tests --test merkle_tests
	cargo test --manifest-path ignite-pay-channel-service/Cargo.toml --test service_tests
	@echo "=== State-channel integration tests passed ==="

build-sbf:   ## Build on-chain programs (.so) — run in WSL
	@echo "=== Building on-chain programs ==="
	cd ignite-pay-did-program && cargo build-sbf
	cd ignite-pay-session-program && cargo build-sbf
	cd ignite-pay-mb/programs/ignite-pay-mb && cargo build-sbf
	@echo "=== Build complete ==="

build-sbf-state-channel:   ## Build state-channel on-chain program (.so) — run in WSL
	@echo "=== Building state-channel program ==="
	cd ignite-pay-program && cargo build-sbf
	@echo "=== Build complete ==="

test-svm: build-sbf test-svm-all   ## Build .so then run SVM tests

test-svm-all:   ## Run SVM tests (requires pre-built .so files)
	@echo "=== Running SVM tests ==="
	SBF_OUT_DIR=ignite-pay-did-program/target/deploy cargo test --manifest-path tests/svm-mollusk-did/Cargo.toml
	SBF_OUT_DIR=ignite-pay-did-program/target/deploy cargo test --manifest-path tests/svm-litesvm-did/Cargo.toml
	@echo "=== SVM tests passed ==="

test-svm-state-channel: build-sbf-state-channel   ## Build and run state-channel SVM tests
	@echo "=== Running state-channel SVM tests ==="
	SBF_OUT_DIR=ignite-pay-program/target/deploy cargo test --manifest-path tests/svm-mollusk/Cargo.toml
	SBF_OUT_DIR=ignite-pay-program/target/deploy cargo test --manifest-path tests/svm-litesvm/Cargo.toml
	@echo "=== State-channel SVM tests passed ==="
