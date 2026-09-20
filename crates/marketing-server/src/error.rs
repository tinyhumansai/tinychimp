//! Crate-wide error responses.

use std::fmt;

use axum::{
    Json,
    response::{IntoResponse, Response},
};
use serde::Serialize;

/// Errors returned by the marketing automation service.
#[derive(Debug)]
pub enum Error {
    /// MongoDB rejected an operation.
    Database(mongodb::error::Error),
    /// An API request did not meet the contract.
    Validation(String),
    /// A requested resource does not exist.
    NotFound(String),
    /// `TinyFlows` could not accept an automation event.
    Workflow(reqwest::Error),
}

/// Convenient result alias for service operations.
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Serialize)]
struct ErrorBody {
    error: String,
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let (status, error) = match self {
            Self::Database(error) => (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                error.to_string(),
            ),
            Self::Validation(error) => (axum::http::StatusCode::BAD_REQUEST, error),
            Self::NotFound(error) => (axum::http::StatusCode::NOT_FOUND, error),
            Self::Workflow(error) => (axum::http::StatusCode::BAD_GATEWAY, error.to_string()),
        };
        (status, Json(ErrorBody { error })).into_response()
    }
}

impl From<mongodb::error::Error> for Error {
    fn from(error: mongodb::error::Error) -> Self {
        Self::Database(error)
    }
}

impl From<reqwest::Error> for Error {
    fn from(error: reqwest::Error) -> Self {
        Self::Workflow(error)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(error) => write!(formatter, "database error: {error}"),
            Self::Validation(error) | Self::NotFound(error) => formatter.write_str(error),
            Self::Workflow(error) => write!(formatter, "workflow error: {error}"),
        }
    }
}

impl std::error::Error for Error {}
