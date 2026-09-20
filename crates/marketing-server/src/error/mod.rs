//! Crate-wide error responses.

use axum::{
    Json,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use thiserror::Error;

/// Errors returned by the marketing automation service.
#[derive(Debug, Error)]
pub enum ServiceError {
    /// MongoDB rejected an operation.
    #[error("database operation failed")]
    Database(#[source] mongodb::error::Error),
    /// An API request did not meet the contract.
    #[error("{0}")]
    Validation(String),
    /// A requested resource does not exist.
    #[error("{0}")]
    NotFound(String),
    /// `TinyFlows` could not accept an automation event.
    #[error("workflow delivery failed")]
    Workflow(#[source] reqwest::Error),
}

/// Convenient result alias for service operations.
pub type Result<T> = std::result::Result<T, ServiceError>;

#[derive(Debug, Serialize)]
struct ErrorBody {
    error: String,
}

impl IntoResponse for ServiceError {
    fn into_response(self) -> Response {
        let (status, error) = match self {
            Self::Database(_) => (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "internal server error".into(),
            ),
            Self::Validation(error) => (axum::http::StatusCode::BAD_REQUEST, error),
            Self::NotFound(error) => (axum::http::StatusCode::NOT_FOUND, error),
            Self::Workflow(_) => (
                axum::http::StatusCode::BAD_GATEWAY,
                "workflow delivery failed".into(),
            ),
        };
        (status, Json(ErrorBody { error })).into_response()
    }
}

impl From<mongodb::error::Error> for ServiceError {
    fn from(error: mongodb::error::Error) -> Self {
        Self::Database(error)
    }
}
impl From<reqwest::Error> for ServiceError {
    fn from(error: reqwest::Error) -> Self {
        Self::Workflow(error)
    }
}

/// Backwards-compatible public error name.
pub type Error = ServiceError;

#[cfg(test)]
mod test;
