use anyhow::{anyhow, bail, Context, Result};
use argon2::{password_hash::SaltString, Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use axum::http::{header, HeaderMap};
use chrono::{DateTime, Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use rand_core::OsRng;
use rusqlite::{params, OptionalExtension};
use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration as StdDuration, Instant},
};

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
    let (user, password_updated_at) = conn
        .query_row(
            "SELECT id, username, last_login_ip, last_login_at, updated_at
             FROM users WHERE username = ?1",
            params![claims.sub],
            |row| {
                Ok((
                    ApiUser {
                        id: row.get(0)?,
                        username: row.get(1)?,
                        last_login_ip: row.get(2)?,
                        last_login_at: row.get(3)?,
                    },
                    row.get::<_, String>(4)?,
                ))
            },
        )
        .optional()?
        .context("User not found")?;

    if token_predates_password_change(claims.iat, &password_updated_at) {
        bail!("登录状态已失效，请重新登录");
    }

    Ok(user)
}

/// `users.updated_at` only moves when the password is set or rotated, so a token
/// issued before that moment must no longer be accepted.
fn token_predates_password_change(issued_at: usize, password_updated_at: &str) -> bool {
    DateTime::parse_from_rfc3339(password_updated_at)
        .map(|changed_at| (issued_at as i64) < changed_at.timestamp())
        .unwrap_or(false)
}

pub fn issue_token(state: &AppState, username: &str) -> Result<String> {
    let now = Utc::now();
    let claims = Claims {
        sub: username.to_string(),
        iat: now.timestamp() as usize,
        exp: (now + Duration::hours(12)).timestamp() as usize,
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

const MAX_LOGIN_FAILURES: u32 = 5;
const LOGIN_FAILURE_WINDOW: StdDuration = StdDuration::from_secs(300);
const LOGIN_LOCKOUT: StdDuration = StdDuration::from_secs(300);
const MAX_THROTTLE_ENTRIES: usize = 4096;

/// In-memory throttle for failed logins. There is a single admin account, so a
/// small per-client lockout removes the brute force path without extra config.
#[derive(Debug, Default)]
pub struct LoginThrottle {
    entries: Mutex<HashMap<String, FailureRecord>>,
}

#[derive(Debug)]
struct FailureRecord {
    failures: u32,
    last_failure: Instant,
    locked_until: Option<Instant>,
}

impl LoginThrottle {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn retry_after(&self, key: &str) -> Option<StdDuration> {
        self.retry_after_at(key, Instant::now())
    }

    pub fn record_failure(&self, key: &str) {
        self.record_failure_at(key, Instant::now());
    }

    pub fn record_success(&self, key: &str) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.remove(key);
        }
    }

    fn retry_after_at(&self, key: &str, now: Instant) -> Option<StdDuration> {
        let entries = self.entries.lock().ok()?;
        let locked_until = entries.get(key)?.locked_until?;
        (locked_until > now).then(|| locked_until - now)
    }

    fn record_failure_at(&self, key: &str, now: Instant) {
        let Ok(mut entries) = self.entries.lock() else {
            return;
        };

        if entries.len() >= MAX_THROTTLE_ENTRIES {
            // ponytail: 只在超过条目上限时全表清理，正常负载下不会走到这里
            entries.retain(|_, record| {
                now.saturating_duration_since(record.last_failure) < LOGIN_FAILURE_WINDOW
            });
        }

        let record = entries.entry(key.to_string()).or_insert(FailureRecord {
            failures: 0,
            last_failure: now,
            locked_until: None,
        });

        if now.saturating_duration_since(record.last_failure) > LOGIN_FAILURE_WINDOW {
            record.failures = 0;
            record.locked_until = None;
        }

        record.failures += 1;
        record.last_failure = now;
        if record.failures >= MAX_LOGIN_FAILURES {
            record.locked_until = Some(now + LOGIN_LOCKOUT);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_issued_before_password_change_is_rejected() {
        let changed_at = "2026-09-13T00:30:00+00:00";
        let changed_ts = DateTime::parse_from_rfc3339(changed_at)
            .unwrap()
            .timestamp() as usize;

        assert!(token_predates_password_change(changed_ts - 1, changed_at));
        assert!(!token_predates_password_change(changed_ts, changed_at));
        assert!(!token_predates_password_change(changed_ts + 1, changed_at));
        assert!(!token_predates_password_change(0, "not-a-timestamp"));
    }

    #[test]
    fn throttle_locks_out_after_repeated_failures() {
        let throttle = LoginThrottle::new();
        let now = Instant::now();

        for _ in 0..MAX_LOGIN_FAILURES {
            assert!(throttle.retry_after_at("10.0.0.1", now).is_none());
            throttle.record_failure_at("10.0.0.1", now);
        }

        assert!(throttle.retry_after_at("10.0.0.1", now).unwrap() > StdDuration::ZERO);
        assert!(throttle.retry_after_at("10.0.0.2", now).is_none());

        throttle.record_success("10.0.0.1");
        assert!(throttle.retry_after_at("10.0.0.1", now).is_none());
    }

    #[test]
    fn throttle_forgets_failures_outside_the_window() {
        let throttle = LoginThrottle::new();
        let start = Instant::now();

        for _ in 0..(MAX_LOGIN_FAILURES - 1) {
            throttle.record_failure_at("10.0.0.1", start);
        }

        let later = start + LOGIN_FAILURE_WINDOW + StdDuration::from_secs(1);
        throttle.record_failure_at("10.0.0.1", later);

        assert!(throttle.retry_after_at("10.0.0.1", later).is_none());
    }
}
