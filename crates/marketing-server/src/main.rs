//! Executable entrypoint for the marketing automation API.

use marketing_server::{AnalyticsWriter, AppState, Config, GoogleOAuth, TinyFlowsClient, router};
use mongodb::Client;

/// Starts the Axum server using environment-backed infrastructure settings.
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::from_env()?;
    let database = Client::with_uri_str(&config.mongodb_uri)
        .await?
        .database(&config.mongodb_database);
    let state = AppState::new(
        database,
        TinyFlowsClient::new(config.tinyflows_webhook_url),
        AnalyticsWriter::new(&config.clickhouse_url, &config.clickhouse_database),
        GoogleOAuth::new(
            config.google_client_id,
            config.google_client_secret,
            config.google_redirect_url,
            config.jwt_secret,
        ),
        config.public_base_url,
    )
    .await?;
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    axum::serve(listener, router(state)).await?;
    Ok(())
}
