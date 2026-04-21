# Hub Registry API

The Hub Registry is a standalone microservice for discovering and managing payment channel Hubs.

## Overview

The Hub Registry provides a REST API for:

- Hub operators to register their Hub instances
- Channel services to publish hub metrics periodically
- Apps to discover available Hubs for channel creation

## Configuration

Create `hub-registry.toml`:

```toml
[server]
host = "0.0.0.0"
port = 3004

[database]
url = "postgres://ignite:ignite@localhost:5432/hub_registry"
```

## Running

```bash
cargo run -p ignite-pay-hub-registry -- hub-registry.toml
```

The database schema is automatically applied on startup.

## API Endpoints

### Register a Hub

```
POST /v1/hubs
```

**Request body:**

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| hub_did | string | yes | Hub DID identifier (unique) |
| endpoint_url | string | yes | Hub API endpoint URL |
| name | string | yes | Display name |
| description | string | no | Description |
| active_pubkey | string | no | Solana pubkey |
| collateral | int64 | no | Collateral amount (lamports) |
| available_liquidity | int64 | no | Available liquidity (lamports) |
| fee_rate_bps | int16 | no | Fee rate in basis points |
| supported_tokens | string[] | no | List of supported token mints |

**Response:** Full Hub object (see below).

### List Hubs

```
GET /v1/hubs?status=active&token_mint=So111111...&limit=100&offset=0
```

**Query parameters:**

| Param | Type | Description |
|-------|------|-------------|
| status | string | Filter by status (e.g., "active") |
| token_mint | string | Filter by supported token |
| limit | int64 | Max results (default 100, max 500) |
| offset | int64 | Pagination offset |

**Response:**

```json
{
  "hubs": [ ... ]
}
```

### Get Hub

```
GET /v1/hubs/{hub_id}
```

**Response:** Full Hub object.

### Update Hub

```
PUT /v1/hubs/{hub_id}
```

All fields are optional. Only provided fields are updated.

### Deregister Hub

```
DELETE /v1/hubs/{hub_id}
```

Sets hub status to `inactive`.

### Get Hub Metrics

```
GET /v1/hubs/{hub_id}/metrics
```

**Response:**

```json
{
  "hub_id": "uuid",
  "online_rate": 100,
  "success_rate": 99,
  "avg_latency_ms": 50,
  "active_channels": 42,
  "available_liquidity": 10000000000,
  "fee_rate_bps": 10,
  "updated_at": "2025-01-01T00:00:00Z"
}
```

### Update Hub Metrics

```
PUT /v1/hubs/{hub_id}/metrics
```

**Request body (all fields optional):**

| Field | Type | Description |
|-------|------|-------------|
| online_rate | int16 | Online percentage (0-100) |
| success_rate | int16 | Success percentage (0-100) |
| avg_latency_ms | int32 | Average latency in milliseconds |
| active_channels | int32 | Number of active channels |
| available_liquidity | int64 | Available liquidity |
| fee_rate_bps | int16 | Fee rate in basis points |

## Hub Object

```json
{
  "hub_id": "uuid",
  "hub_did": "did:ignite:Base58Pubkey",
  "endpoint_url": "http://hub:3003",
  "name": "Hub-ABC12345",
  "description": "Main payment hub",
  "status": "active",
  "active_pubkey": "Base58SolanaPubkey",
  "collateral": 100000000000,
  "available_liquidity": 50000000000,
  "fee_rate_bps": 10,
  "supported_tokens": ["So11111111111111111111111111111111"],
  "online_rate": 100,
  "success_rate": 99,
  "avg_latency_ms": 50,
  "active_channels": 42,
  "created_at": "2025-01-01T00:00:00Z",
  "updated_at": "2025-01-01T00:00:00Z"
}
```

## Channel Service Publishing

When `hub_registry` is configured in the channel service TOML:

```toml
[hub_registry]
url = "http://localhost:3004"
publish_interval_secs = 60
```

The channel-hub binary will:

1. On startup: POST `/v1/hubs` to register itself
2. Every N seconds: PUT `/v1/hubs/{hub_id}/metrics` with current metrics

## Channel Creation Flow

1. App calls `fetch_hub_list(registry_url)` to get available Hubs
2. User selects a Hub and configures deposit/token/tree_depth
3. App calls `send_create_channel_request()` which:
   - Builds a DIDComm `create-channel-request` message
   - Encrypts and sends it to the MCP server
4. MCP server receives the request via DIDComm
5. MCP server calls `channel_client.open_channel()` on the Hub's HTTP API
6. MCP server sends a `create-channel-response` back to the App
7. App receives the response and updates UI

### DIDComm Message Types

**create-channel-request:**

```
Type: https://didcomm.org/ignite-pay/1.0/create-channel-request
```

```json
{
  "hub_endpoint": "http://hub:3003",
  "provider_pubkey": "Base58SolanaPubkey",
  "token_mint": "Base58MintAddress",
  "deposit": 1000000000,
  "tree_depth": 8
}
```

**create-channel-response:**

```
Type: https://didcomm.org/ignite-pay/1.0/create-channel-response
```

```json
{
  "channel_id": "hex_encoded_32_bytes",
  "sequence": 0,
  "current_root": "hex_encoded_root",
  "success": true
}
```

On failure:

```json
{
  "channel_id": "",
  "sequence": 0,
  "current_root": "",
  "success": false,
  "error_message": "Failed to open channel"
}
```

## Database Schema

The registry uses PostgreSQL with a single `hubs` table:

```sql
CREATE TABLE hubs (
    hub_id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    hub_did             VARCHAR(128) NOT NULL UNIQUE,
    endpoint_url        VARCHAR(512) NOT NULL,
    name                VARCHAR(256) NOT NULL,
    description         TEXT DEFAULT '',
    status              VARCHAR(32) NOT NULL DEFAULT 'active',
    active_pubkey       VARCHAR(128),
    collateral          BIGINT NOT NULL DEFAULT 0,
    available_liquidity BIGINT NOT NULL DEFAULT 0,
    fee_rate_bps        SMALLINT NOT NULL DEFAULT 0,
    supported_tokens    TEXT[] DEFAULT '{}',
    online_rate         SMALLINT NOT NULL DEFAULT 0,
    success_rate        SMALLINT NOT NULL DEFAULT 0,
    avg_latency_ms      INTEGER NOT NULL DEFAULT 0,
    active_channels     INTEGER NOT NULL DEFAULT 0,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
```
