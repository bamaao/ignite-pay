CREATE TABLE IF NOT EXISTS hubs (
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

CREATE INDEX IF NOT EXISTS idx_hubs_status ON hubs(status);
