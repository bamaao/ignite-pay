# ============================================================
# Ignite Pay — Windows Local Testing Configuration
# Edit this file, then run: .\deploy-local.ps1 start
# ============================================================

# --------------- Paths ---------------
# Project root (auto-detected if empty)
$Script:PROJECT_ROOT = ""

# Data directory for persistent storage (sled databases etc.)
$Script:DATA_DIR = ".\local-data"

# Keys directory — put Solana keypair files here
$Script:KEYS_DIR = ".\deploy\keys"

# --------------- Solana RPC ---------------
$Script:SOLANA_RPC_URL = "https://api.devnet.solana.com"
$Script:SOLANA_NETWORK = "devnet"

# --------------- On-chain Program IDs ---------------
$Script:CHANNEL_PROGRAM_ID = "DJBHr35jL3JAGoU7bKMsEFmpeNMrCSK7oYQE4HJ3GBUe"
$Script:DID_PROGRAM_ID = "D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D1D"
$Script:SESSION_PROGRAM_ID = "6EFvVTh7rEBpHH2JGryjKQmBLRtbYtSEerGNfkHqKiei"

# --------------- DIDComm Router (user-side) ---------------
$Script:ROUTER_USER_PORT = 8080

# --------------- DIDComm Router (merchant-side) ---------------
$Script:ROUTER_MERCHANT_PORT = 4000

# --------------- DID Registry ---------------
$Script:DID_REGISTRY_PORT = 8081
$Script:DID_REGISTRY_JWT_SECRET = "did-registry-secret"
$Script:DID_REGISTRY_PAYER_KEYPAIR = ""
$Script:PLATFORM_PUBLIC_KEY = ""
$Script:PLATFORM_SIGNING_KEY_PATH = ""

# --------------- Channel Services ---------------
$Script:CHANNEL_USER_PORT = 3001
$Script:CHANNEL_PROVIDER_PORT = 3002
$Script:CHANNEL_HUB_PORT = 3003

# --------------- Hub Registry ---------------
$Script:HUB_REGISTRY_PORT = 3004
# PostgreSQL connection — adjust if you have a local Postgres instance
$Script:HUB_REGISTRY_DB_URL = "postgres://ignite:ignite@127.0.0.1:5432/hub_registry"

# --------------- User MCP ---------------
$Script:USER_MCP_MEDIATOR_WS = "ws://127.0.0.1:8080/ws"
$Script:USER_MCP_PHONE_DID = ""
$Script:USER_MCP_AUTO_APPROVE_MAX = 0
$Script:USER_MCP_AUTH_TIMEOUT = 300
$Script:USER_MCP_PLATFORM_DID = "did:ignite:zPlatformDIDPlaceholder"

# --------------- Merchant MCP ---------------
$Script:MERCHANT_MCP_HUB_ENDPOINT = "http://127.0.0.1:3003"
$Script:MERCHANT_MCP_HUB_WS_URL = "ws://127.0.0.1:3003/ws"
$Script:MERCHANT_MCP_MEDIATOR_WS = "ws://127.0.0.1:4000/ws"

# --------------- Rust Build ---------------
$Script:CARGO_BIN = "cargo"
$Script:RUST_LOG = "info"

# --------------- Keys (absolute or relative paths) ---------------
$Script:KEY_USER = ""
$Script:KEY_PROVIDER = ""
$Script:KEY_HUB = ""
