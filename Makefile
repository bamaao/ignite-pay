# ============================================================
# Ignite Pay — Deployment Commands
# ============================================================

.PHONY: help build up down logs ps health init keys \
        build-app build-merchant clean

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
