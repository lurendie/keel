use axum::{http::StatusCode, Json};
use chrono::Utc;

use crate::models::ErrorResponse;

pub fn now_ts() -> String {
    Utc::now().to_rfc3339()
}

pub fn json_error(status: StatusCode, msg: impl Into<String>) -> (StatusCode, Json<ErrorResponse>) {
    (status, Json(ErrorResponse { error: msg.into() }))
}

pub fn log(message: &str) {
    tracing::info!("{message}");
}

pub fn warn(message: &str) {
    tracing::warn!("{message}");
}

pub fn error(message: &str) {
    tracing::error!("{message}");
}

pub fn debug(message: &str) {
    tracing::debug!("{message}");
}
