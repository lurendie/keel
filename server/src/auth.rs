use anyhow::{anyhow, Context, Result};
use argon2::{password_hash::SaltString, Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use axum::http::{header, HeaderMap};
use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use rand_core::OsRng;
use rusqlite::{params, OptionalExtension};

use crate::{
    models::{ApiUser, Claims},
    state::AppState,
};

fn extract_bearer(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(header::AUTHORIZATION)?;
    let text = value.to_str().ok()?;
    text.strip_prefix("Bearer ").map(|s| s.to_string())
}

pub fn authenticate(headers: &HeaderMap, state: &AppState) -> Result<ApiUser> {
    let token = extract_bearer(headers).context("Missing Bearer token")?;
    let claims = decode::<Claims>(
        &token,
        &DecodingKey::from_secret(state.jwt_secret.as_bytes()),
        &Validation::default(),
    )
    .context("Invalid token")?
    .claims;

    let conn = state.db.lock().map_err(|_| anyhow!("DB lock poisoned"))?;
    let user = conn
        .query_row(
            "SELECT id, username FROM users WHERE username = ?1",
            params![claims.sub],
            |row| {
                Ok(ApiUser {
                    id: row.get(0)?,
                    username: row.get(1)?,
                })
            },
        )
        .optional()?
        .context("User not found")?;

    Ok(user)
}

pub fn issue_token(state: &AppState, username: &str) -> Result<String> {
    let exp = (Utc::now() + Duration::hours(12)).timestamp() as usize;
    let claims = Claims {
        sub: username.to_string(),
        exp,
    };
    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(state.jwt_secret.as_bytes()),
    )?;
    Ok(token)
}

pub fn hash_password(password: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow!("Hash password failed: {e}"))?
        .to_string();
    Ok(hash)
}

pub fn verify_password(password: &str, password_hash: &str) -> Result<bool> {
    let parsed =
        PasswordHash::new(password_hash).map_err(|e| anyhow!("Invalid password hash: {e}"))?;
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok())
}
