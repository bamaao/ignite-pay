use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

use crate::error::{MediatorError, Result};

/// JWT claims for REST API authentication.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthClaims {
    pub user_did: String,
    pub exp: usize,
}

/// Request body for exchanging a DID signature for a JWT.
#[derive(Debug, Deserialize)]
pub struct TokenRequest {
    pub did: String,
    pub signature: String,
}

/// Response body for the auth token endpoint.
#[derive(Debug, Serialize)]
pub struct TokenResponse {
    pub token: String,
    pub expires_in: u64,
}

/// Generate a JWT for a given user DID.
pub fn create_token(user_did: &str, secret: &str) -> Result<String> {
    let expiration = chrono::Utc::now()
        .checked_add_signed(chrono::Duration::hours(1))
        .ok_or_else(|| MediatorError::Unauthorized("Time error".into()))?
        .timestamp() as usize;

    let claims = AuthClaims {
        user_did: user_did.to_string(),
        exp: expiration,
    };

    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| MediatorError::Unauthorized(format!("Token encode error: {}", e)))
}

/// Verify a JWT bearer token and return the claims.
pub fn verify_bearer_token(token: &str, secret: &str) -> Result<AuthClaims> {
    let token_data = decode::<AuthClaims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )
    .map_err(|e| MediatorError::Unauthorized(format!("Token verify error: {}", e)))?;

    Ok(token_data.claims)
}

/// Extract the bearer token from the Authorization header.
fn extract_bearer(parts: &Parts) -> Result<String> {
    let auth_header = parts
        .headers
        .get("Authorization")
        .ok_or_else(|| MediatorError::Unauthorized("Missing Authorization header".into()))?
        .to_str()
        .map_err(|_| MediatorError::Unauthorized("Invalid Authorization header".into()))?;

    if !auth_header.starts_with("Bearer ") {
        return Err(MediatorError::Unauthorized(
            "Invalid Authorization scheme, expected Bearer".into(),
        ));
    }

    Ok(auth_header[7..].to_string())
}

/// Axum extractor for authenticated requests.
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub did: String,
}

#[axum::async_trait]
impl<S: Send + Sync> FromRequestParts<S> for AuthUser {
    type Rejection = MediatorError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> std::result::Result<Self, Self::Rejection> {
        let token = extract_bearer(parts)?;
        let secret = std::env::var("JWT_SECRET").unwrap_or_else(|_| "ignite-pay-mediator-secret".to_string());
        let claims = verify_bearer_token(&token, &secret)?;
        Ok(AuthUser { did: claims.user_did })
    }
}
