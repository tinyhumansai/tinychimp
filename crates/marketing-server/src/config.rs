//! Environment-backed application configuration.

use std::fmt;

use crate::error::{Error, Result};

/// Configuration required to start the API.
#[derive(Clone)]
pub struct Config {
    /// MongoDB connection URI.
    pub mongodb_uri: String,
    /// MongoDB database name.
    pub mongodb_database: String,
    /// `TinyFlows` webhook endpoint receiving lifecycle events.
    pub tinyflows_webhook_url: String,
    /// Public URL used when producing unsubscribe links.
    pub public_base_url: String,
    /// `ClickHouse` HTTP URL used for engagement analytics.
    pub clickhouse_url: String,
    /// `ClickHouse` database holding the event table.
    pub clickhouse_database: String,
    /// Google OAuth client identifier.
    pub google_client_id: String,
    /// Google OAuth client secret.
    pub google_client_secret: String,
    /// Registered Google OAuth redirect URL.
    pub google_redirect_url: String,
    /// Secret used to sign dashboard JWTs.
    pub jwt_secret: String,
}

impl fmt::Debug for Config {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Config")
            .field("mongodb_database", &self.mongodb_database)
            .field("public_base_url", &self.public_base_url)
            .field("clickhouse_database", &self.clickhouse_database)
            .field("google_client_id", &self.google_client_id)
            .finish_non_exhaustive()
    }
}

impl Config {
    /// Reads configuration from environment variables.
    ///
    /// # Errors
    ///
    /// Returns validation errors for missing required variables.
    pub fn from_env() -> Result<Self> {
        fn required(name: &str) -> Result<String> {
            let value = std::env::var(name)
                .map_err(|_| Error::Validation(format!("missing required {name}")))?;
            if value.trim().is_empty() {
                return Err(Error::Validation(format!(
                    "required {name} must not be empty"
                )));
            }
            Ok(value)
        }
        Ok(Self {
            mongodb_uri: required("MONGODB_URI")?,
            mongodb_database: required("MONGODB_DATABASE")?,
            tinyflows_webhook_url: required("TINYFLOWS_WEBHOOK_URL")?,
            public_base_url: std::env::var("PUBLIC_BASE_URL")
                .unwrap_or_else(|_| "http://localhost:3000".into()),
            clickhouse_url: required("CLICKHOUSE_URL")?,
            clickhouse_database: required("CLICKHOUSE_DATABASE")?,
            google_client_id: required("GOOGLE_CLIENT_ID")?,
            google_client_secret: required("GOOGLE_CLIENT_SECRET")?,
            google_redirect_url: required("GOOGLE_REDIRECT_URL")?,
            jwt_secret: required("JWT_SECRET")?,
        })
    }
}
