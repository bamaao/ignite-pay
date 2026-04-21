use sqlx::PgPool;
use uuid::Uuid;

use crate::error::RegistryError;
use crate::models::{Hub, ListHubsQuery, RegisterHubRequest, UpdateHubRequest, UpdateMetricsRequest};

pub async fn register_hub(pool: &PgPool, req: &RegisterHubRequest) -> Result<Hub, RegistryError> {
    let hub = sqlx::query_as::<_, Hub>(
        r#"
        INSERT INTO hubs (hub_did, endpoint_url, name, description, active_pubkey, collateral, available_liquidity, fee_rate_bps, supported_tokens)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        RETURNING *
        "#,
    )
    .bind(&req.hub_did)
    .bind(&req.endpoint_url)
    .bind(&req.name)
    .bind(&req.description)
    .bind(&req.active_pubkey)
    .bind(req.collateral)
    .bind(req.available_liquidity)
    .bind(req.fee_rate_bps)
    .bind(&req.supported_tokens)
    .fetch_one(pool)
    .await?;

    Ok(hub)
}

pub async fn get_hub(pool: &PgPool, hub_id: Uuid) -> Result<Option<Hub>, RegistryError> {
    let hub = sqlx::query_as::<_, Hub>("SELECT * FROM hubs WHERE hub_id = $1")
        .bind(hub_id)
        .fetch_optional(pool)
        .await?;

    Ok(hub)
}

pub async fn list_hubs(pool: &PgPool, query: &ListHubsQuery) -> Result<Vec<Hub>, RegistryError> {
    let limit = query.limit.unwrap_or(100).min(500);
    let offset = query.offset.unwrap_or(0);

    let hubs = if let Some(status) = &query.status {
        sqlx::query_as::<_, Hub>(
            "SELECT * FROM hubs WHERE status = $1 ORDER BY created_at DESC LIMIT $2 OFFSET $3",
        )
        .bind(status)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as::<_, Hub>(
            "SELECT * FROM hubs ORDER BY created_at DESC LIMIT $1 OFFSET $2",
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await?
    };

    // Filter by token_mint if specified (post-filter since TEXT[] query is awkward)
    let hubs = if let Some(token_mint) = &query.token_mint {
        hubs.into_iter()
            .filter(|h| h.supported_tokens.contains(token_mint))
            .collect()
    } else {
        hubs
    };

    Ok(hubs)
}

pub async fn update_hub(
    pool: &PgPool,
    hub_id: Uuid,
    req: &UpdateHubRequest,
) -> Result<Hub, RegistryError> {
    let hub = sqlx::query_as::<_, Hub>(
        r#"
        UPDATE hubs SET
            endpoint_url = COALESCE($2, endpoint_url),
            name = COALESCE($3, name),
            description = COALESCE($4, description),
            status = COALESCE($5, status),
            active_pubkey = COALESCE($6, active_pubkey),
            collateral = COALESCE($7, collateral),
            available_liquidity = COALESCE($8, available_liquidity),
            fee_rate_bps = COALESCE($9, fee_rate_bps),
            supported_tokens = COALESCE($10, supported_tokens),
            updated_at = NOW()
        WHERE hub_id = $1
        RETURNING *
        "#,
    )
    .bind(hub_id)
    .bind(&req.endpoint_url)
    .bind(&req.name)
    .bind(&req.description)
    .bind(&req.status)
    .bind(&req.active_pubkey)
    .bind(req.collateral)
    .bind(req.available_liquidity)
    .bind(req.fee_rate_bps)
    .bind(&req.supported_tokens)
    .fetch_one(pool)
    .await
    .map_err(|e| match e {
        sqlx::Error::RowNotFound => RegistryError::NotFound(format!("Hub {} not found", hub_id)),
        e => RegistryError::Database(e),
    })?;

    Ok(hub)
}

pub async fn update_metrics(
    pool: &PgPool,
    hub_id: Uuid,
    req: &UpdateMetricsRequest,
) -> Result<Hub, RegistryError> {
    let hub = sqlx::query_as::<_, Hub>(
        r#"
        UPDATE hubs SET
            online_rate = COALESCE($2, online_rate),
            success_rate = COALESCE($3, success_rate),
            avg_latency_ms = COALESCE($4, avg_latency_ms),
            active_channels = COALESCE($5, active_channels),
            available_liquidity = COALESCE($6, available_liquidity),
            fee_rate_bps = COALESCE($7, fee_rate_bps),
            updated_at = NOW()
        WHERE hub_id = $1
        RETURNING *
        "#,
    )
    .bind(hub_id)
    .bind(req.online_rate)
    .bind(req.success_rate)
    .bind(req.avg_latency_ms)
    .bind(req.active_channels)
    .bind(req.available_liquidity)
    .bind(req.fee_rate_bps)
    .fetch_one(pool)
    .await
    .map_err(|e| match e {
        sqlx::Error::RowNotFound => RegistryError::NotFound(format!("Hub {} not found", hub_id)),
        e => RegistryError::Database(e),
    })?;

    Ok(hub)
}

pub async fn deregister_hub(pool: &PgPool, hub_id: Uuid) -> Result<(), RegistryError> {
    let result = sqlx::query("UPDATE hubs SET status = 'inactive', updated_at = NOW() WHERE hub_id = $1")
        .bind(hub_id)
        .execute(pool)
        .await?;

    if result.rows_affected() == 0 {
        return Err(RegistryError::NotFound(format!("Hub {} not found", hub_id)));
    }

    Ok(())
}
