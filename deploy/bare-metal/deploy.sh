#!/usr/bin/env bash
# ============================================================
# Ignite Pay — Bare-Metal Deployment Script
#
# Usage:
#   sudo ./deploy.sh build     # Compile all Rust services (release)
#   sudo ./deploy.sh install   # Install binaries, configs, systemd units
#   sudo ./deploy.sh start     # Enable and start all services
#   sudo ./deploy.sh stop      # Stop all services
#   sudo ./deploy.sh status    # Show status of all services
#   sudo ./deploy.sh logs SVC  # Tail logs for a specific service
#   sudo ./deploy.sh health    # Curl health endpoints
#   sudo ./deploy.sh uninstall # Remove everything
# ============================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
CONF_FILE="$SCRIPT_DIR/deploy.conf"

# Source config
if [ ! -f "$CONF_FILE" ]; then
    echo "ERROR: $CONF_FILE not found. Copy deploy.conf.example and edit it."
    exit 1
fi
# shellcheck source=deploy.conf
source "$CONF_FILE"

CARGO="${CARGO_BIN:-cargo}"

# --------------- Service Definitions ---------------
# Each entry: name|binary|config_file|port
SERVICES=(
    "router-user|didcomm-router|router-user.toml|${ROUTER_USER_PORT}"
    "router-merchant|didcomm-router|router-merchant.toml|${ROUTER_MERCHANT_PORT}"
    "did-registry|did-registry|did-registry.toml|${DID_REGISTRY_PORT}"
    "channel-user|channel-user|channel-user.toml|${CHANNEL_USER_PORT}"
    "channel-provider|channel-provider|channel-provider.toml|${CHANNEL_PROVIDER_PORT}"
    "channel-hub|channel-hub|channel-hub.toml|${CHANNEL_HUB_PORT}"
    "hub-registry|ignite-pay-hub-registry|hub-registry.toml|${HUB_REGISTRY_PORT}"
)

SYSTEMD_UNITS=()

# --------------- Commands ---------------

cmd_build() {
    echo "==> Building all Rust services in release mode..."
    echo ""

    # Build didcomm-router
    echo "  [1/5] didcomm-router"
    (cd "$PROJECT_ROOT/didcomm-router" && $CARGO build --release 2>&1) | sed 's/^/    /'

    # Build did-registry
    echo "  [2/5] did-registry"
    (cd "$PROJECT_ROOT/did-registry" && $CARGO build --release 2>&1) | sed 's/^/    /'

    # Build channel-service (all 3 binaries)
    echo "  [3/5] ignite-pay-channel-service (user, provider, hub)"
    (cd "$PROJECT_ROOT/ignite-pay-channel-service" && $CARGO build --release 2>&1) | sed 's/^/    /'

    # Build hub-registry
    echo "  [4/5] ignite-pay-hub-registry"
    (cd "$PROJECT_ROOT/ignite-pay-hub-registry" && $CARGO build --release 2>&1) | sed 's/^/    /'

    # Build MCP services
    echo "  [5/5] MCP services (user + merchant)"
    (cd "$PROJECT_ROOT/ignite-pay-mcp" && $CARGO build --release 2>&1) | sed 's/^/    /'
    (cd "$PROJECT_ROOT/ignite-pay-merchant-mcp" && $CARGO build --release 2>&1) | sed 's/^/    /'

    echo ""
    echo "==> Build complete. Binaries in $PROJECT_ROOT/target/release/"
}

cmd_install() {
    echo "==> Installing Ignite Pay services..."

    # Create user
    if ! id "$SERVICE_USER" &>/dev/null; then
        useradd --system --no-create-home --shell /usr/sbin/nologin "$SERVICE_USER"
        echo "  Created system user: $SERVICE_USER"
    fi

    # Create directories
    mkdir -p "$INSTALL_DIR" "$DATA_DIR" "$CONFIG_DIR" "$LOG_DIR"
    chown "$SERVICE_USER:$SERVICE_USER" "$DATA_DIR" "$LOG_DIR"

    # Install binaries
    echo "  Installing binaries to $INSTALL_DIR/ ..."
    install -m 755 "$PROJECT_ROOT/target/release/didcomm-router" "$INSTALL_DIR/didcomm-router"
    install -m 755 "$PROJECT_ROOT/target/release/did-registry" "$INSTALL_DIR/did-registry"
    install -m 755 "$PROJECT_ROOT/target/release/channel-user" "$INSTALL_DIR/channel-user"
    install -m 755 "$PROJECT_ROOT/target/release/channel-provider" "$INSTALL_DIR/channel-provider"
    install -m 755 "$PROJECT_ROOT/target/release/channel-hub" "$INSTALL_DIR/channel-hub"
    install -m 755 "$PROJECT_ROOT/target/release/ignite-pay-hub-registry" "$INSTALL_DIR/ignite-pay-hub-registry"
    install -m 755 "$PROJECT_ROOT/target/release/ignite-pay-mcp" "$INSTALL_DIR/ignite-pay-mcp"
    install -m 755 "$PROJECT_ROOT/target/release/ignite-pay-merchant-mcp" "$INSTALL_DIR/ignite-pay-merchant-mcp"

    # Generate config files
    echo "  Generating config files in $CONFIG_DIR/ ..."
    generate_configs

    # Create data subdirectories
    mkdir -p "$DATA_DIR"/{router-user,router-merchant,did-registry,channel-user,channel-provider,channel-hub,hub-registry,mcp-user,mcp-merchant}
    chown -R "$SERVICE_USER:$SERVICE_USER" "$DATA_DIR"

    # Install systemd units
    echo "  Installing systemd units..."
    install_systemd_units

    # Install nginx config
    install_nginx

    systemctl daemon-reload
    echo ""
    echo "==> Installation complete. Run 'sudo $0 start' to launch services."
}

cmd_start() {
    echo "==> Starting all services..."
    for unit in "${SYSTEMD_UNITS[@]}"; do
        systemctl enable --now "$unit"
        echo "  started $unit"
    done
    echo ""
    echo "==> All services started."
}

cmd_stop() {
    echo "==> Stopping all services..."
    for unit in "${SYSTEMD_UNITS[@]}"; do
        systemctl stop "$unit" 2>/dev/null || true
        echo "  stopped $unit"
    done
}

cmd_restart() {
    echo "==> Restarting all services..."
    for unit in "${SYSTEMD_UNITS[@]}"; do
        systemctl restart "$unit"
        echo "  restarted $unit"
    done
}

cmd_status() {
    for unit in "${SYSTEMD_UNITS[@]}"; do
        echo "--- $unit ---"
        systemctl status "$unit" --no-pager -l 2>/dev/null || true
        echo ""
    done
}

cmd_logs() {
    local svc="${1:-}"
    if [ -z "$svc" ]; then
        echo "Usage: $0 logs <service-name>"
        echo "Available services:"
        for entry in "${SERVICES[@]}"; do IFS='|' read -r name _ _ _ <<< "$entry"; echo "  $name"; done
        exit 1
    fi
    journalctl -u "ignite-pay-${svc}" -f
}

cmd_health() {
    echo "==> Health check..."
    for entry in "${SERVICES[@]}"; do
        IFS='|' read -r name _ _ port <<< "$entry"
        if curl -sf "http://127.0.0.1:${port}/health" >/dev/null 2>&1; then
            echo "  $name (:$port) — OK"
        else
            echo "  $name (:$port) — NOT REACHABLE"
        fi
    done
}

cmd_uninstall() {
    echo "==> Uninstalling Ignite Pay services..."
    cmd_stop
    for unit in "${SYSTEMD_UNITS[@]}"; do
        rm -f "/etc/systemd/system/$unit"
    done
    systemctl daemon-reload
    rm -rf "$INSTALL_DIR" "$CONFIG_DIR" "$LOG_DIR"
    echo "  Removed binaries, configs, logs."
    echo "  Data preserved at $DATA_DIR (remove manually if desired)."
    echo "  System user '$SERVICE_USER' preserved (remove manually if desired)."
}

# --------------- Config Generation ---------------

generate_configs() {
    # Router — user
    cat > "$CONFIG_DIR/router-user.toml" <<EOF
[server]
host = "0.0.0.0"
port = ${ROUTER_USER_PORT}

[router]
did = "${ROUTER_USER_DID}"
max_queued_messages = 1000
max_message_age_seconds = 86400

[storage]
path = "${DATA_DIR}/router-user"
EOF

    # Router — merchant
    cat > "$CONFIG_DIR/router-merchant.toml" <<EOF
[server]
host = "0.0.0.0"
port = ${ROUTER_MERCHANT_PORT}

[router]
did = "${ROUTER_MERCHANT_DID}"
max_queued_messages = 1000
max_message_age_seconds = 86400

[storage]
path = "${DATA_DIR}/router-merchant"
EOF

    # DID Registry
    cat > "$CONFIG_DIR/did-registry.toml" <<EOF
[server]
host = "0.0.0.0"
port = ${DID_REGISTRY_PORT}

[solana]
rpc_url = "${SOLANA_RPC_URL}"
did_program_id = "${DID_PROGRAM_ID}"
payer_keypair_path = "${DID_REGISTRY_PAYER_KEYPAIR}"

[light]
photon_url = "${PHOTON_RPC_URL}"

[auth]
jwt_secret = "${DID_REGISTRY_JWT_SECRET}"
platform_signing_key_path = "${PLATFORM_SIGNING_KEY_PATH}"

[fees]
register_fee_lamports = 5000
update_vc_fee_lamports = 2000
rotate_key_fee_lamports = 2000
EOF

    # Channel User
    cat > "$CONFIG_DIR/channel-user.toml" <<EOF
[server]
host = "0.0.0.0"
port = ${CHANNEL_USER_PORT}

[solana]
rpc_url = "${SOLANA_RPC_URL}"
channel_program_id = "${CHANNEL_PROGRAM_ID}"
keypair_path = "${KEY_USER}"

[channel]
default_tree_depth = 4
default_challenge_duration = 5000
default_min_challenge_delay = 1000
default_settle_window = 10000
auto_close_offset = 500000
db_path = "${DATA_DIR}/channel-user"

[compliance]
spending_threshold = 1000000000
per_channel_limit = 100000000
window_slots = 100000
travel_rule_threshold = 500000000
EOF

    # Channel Provider
    cat > "$CONFIG_DIR/channel-provider.toml" <<EOF
[server]
host = "0.0.0.0"
port = ${CHANNEL_PROVIDER_PORT}

[solana]
rpc_url = "${SOLANA_RPC_URL}"
channel_program_id = "${CHANNEL_PROGRAM_ID}"
keypair_path = "${KEY_PROVIDER}"

[channel]
default_tree_depth = 4
default_challenge_duration = 5000
default_min_challenge_delay = 1000
default_settle_window = 10000
auto_close_offset = 500000
db_path = "${DATA_DIR}/channel-provider"
EOF

    # Channel Hub
    cat > "$CONFIG_DIR/channel-hub.toml" <<EOF
[server]
host = "0.0.0.0"
port = ${CHANNEL_HUB_PORT}

[solana]
rpc_url = "${SOLANA_RPC_URL}"
channel_program_id = "${CHANNEL_PROGRAM_ID}"
keypair_path = "${KEY_HUB}"

[channel]
default_tree_depth = 4
default_challenge_duration = 5000
default_min_challenge_delay = 1000
default_settle_window = 10000
auto_close_offset = 500000
db_path = "${DATA_DIR}/channel-hub"

[compliance]
spending_threshold = 1000000000
per_channel_limit = 100000000
window_slots = 100000
travel_rule_threshold = 500000000
EOF

    # Hub Registry
    cat > "$CONFIG_DIR/hub-registry.toml" <<EOF
[server]
host = "0.0.0.0"
port = ${HUB_REGISTRY_PORT}

[database]
url = "${HUB_REGISTRY_DB_URL}"
EOF

    # User MCP
    cat > "$CONFIG_DIR/mcp-user.toml" <<EOF
[mediator]
ws_url = "${USER_MCP_MEDIATOR_WS}"
phone_did = "${USER_MCP_PHONE_DID}"

[storage]
path = "${DATA_DIR}/mcp-user"

[policy]
auto_approve_max = ${USER_MCP_AUTO_APPROVE_MAX}
auth_timeout = ${USER_MCP_AUTH_TIMEOUT}

[platform]
did = "${USER_MCP_PLATFORM_DID}"

[ipfs]
mode = "mock"

[solana]
rpc_url = "${SOLANA_RPC_URL}"
tree_address = ""
tree_authority = ""
das_endpoint = ""
pay_mode = "self_funded"
default_owner = ""
tree_authority_keypair_b58 = ""
EOF

    # Merchant MCP
    cat > "$CONFIG_DIR/mcp-merchant.toml" <<EOF
[merchant]
did = ""
hub_endpoint = "${MERCHANT_MCP_HUB_ENDPOINT}"
hub_ws_url = "${MERCHANT_MCP_HUB_WS_URL}"

[mediator]
ws_url = "${MERCHANT_MCP_MEDIATOR_WS}"

[storage]
path = "${DATA_DIR}/mcp-merchant"

[solana]
rpc_url = "${SOLANA_RPC_URL}"
program_id = "${CHANNEL_PROGRAM_ID}"

[hub]
token_mint = ""
provider_pubkey = ""
EOF

    chown -R "$SERVICE_USER:$SERVICE_USER" "$CONFIG_DIR"
    chmod 600 "$CONFIG_DIR"/*.toml
}

# --------------- Systemd Unit Generation ---------------

install_systemd_units() {
    for entry in "${SERVICES[@]}"; do
        IFS='|' read -r name binary config port <<< "$entry"
        local unit_file="/etc/systemd/system/ignite-pay-${name}.service"

        cat > "$unit_file" <<EOF
[Unit]
Description=Ignite Pay — ${name}
After=network-online.target
Wants=network-online.target
$([ "$name" = "hub-registry" ] && echo "After=postgresql.service" || true)
$([ "$name" = "channel-hub" ] && echo "After=ignite-pay-hub-registry.service" || true)

[Service]
Type=simple
User=${SERVICE_USER}
Group=${SERVICE_USER}
ExecStart=${INSTALL_DIR}/${binary} ${CONFIG_DIR}/${config}
WorkingDirectory=${INSTALL_DIR}
Environment=RUST_LOG=${RUST_LOG}
Restart=on-failure
RestartSec=5
LimitNOFILE=65536

# Security hardening
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=${DATA_DIR} ${LOG_DIR}
BindPaths=${CONFIG_DIR}
PrivateTmp=true

# Logging
StandardOutput=journal
StandardError=journal
SyslogIdentifier=ignite-pay-${name}

[Install]
WantedBy=multi-user.target
EOF

        SYSTEMD_UNITS+=("ignite-pay-${name}")
    done
}

# --------------- Nginx Install ---------------

install_nginx() {
    if command -v nginx &>/dev/null; then
        echo "  Installing nginx config..."
        cp "$SCRIPT_DIR/../nginx/nginx.conf" /etc/nginx/sites-available/ignite-pay
        ln -sf /etc/nginx/sites-available/ignite-pay /etc/nginx/sites-enabled/ignite-pay
        nginx -t && systemctl reload nginx
    else
        echo "  nginx not found, skipping. Install manually or use the config in deploy/nginx/"
    fi
}

# --------------- Main ---------------

case "${1:-help}" in
    build)     cmd_build ;;
    install)   cmd_install ;;
    start)     cmd_start ;;
    stop)      cmd_stop ;;
    restart)   cmd_restart ;;
    status)    cmd_status ;;
    logs)      cmd_logs "${2:-}" ;;
    health)    cmd_health ;;
    uninstall) cmd_uninstall ;;
    *)
        echo "Ignite Pay — Bare-Metal Deployment"
        echo ""
        echo "Usage: $0 <command>"
        echo ""
        echo "Commands:"
        echo "  build       Compile all Rust services in release mode"
        echo "  install     Install binaries, configs, and systemd units"
        echo "  start       Enable and start all services"
        echo "  stop        Stop all services"
        echo "  restart     Restart all services"
        echo "  status      Show systemd status for all services"
        echo "  logs <svc>  Tail journal logs for a service"
        echo "  health      Curl health endpoints"
        echo "  uninstall   Stop services and remove installation"
        echo ""
        echo "Edit deploy.conf before running 'install'."
        ;;
esac
