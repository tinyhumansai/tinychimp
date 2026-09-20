//! Executable entrypoint for the marketing automation API.

use marketing_server::{AnalyticsWriter, AppState, Config, GoogleOAuth, TinyFlowsClient, router};
use mongodb::{Client, Database};

const DEFAULT_LISTEN_ADDRESS: &str = "0.0.0.0:3000";

type StartupResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

struct RuntimeDependencies {
    database: Database,
    workflows: TinyFlowsClient,
    analytics: AnalyticsWriter,
    auth: GoogleOAuth,
    public_base_url: String,
    dashboard_origin: String,
}

/// Starts the Axum server using environment-backed infrastructure settings.
#[tokio::main]
async fn main() -> StartupResult<()> {
    run(Config::from_env()?, DEFAULT_LISTEN_ADDRESS).await
}

async fn run(config: Config, listen_address: &str) -> StartupResult<()> {
    let listener = tokio::net::TcpListener::bind(listen_address).await?;
    serve(config, listener).await
}

async fn serve(config: Config, listener: tokio::net::TcpListener) -> StartupResult<()> {
    let dependencies = runtime_dependencies(config).await?;
    let state = AppState::new(
        dependencies.database,
        dependencies.workflows,
        dependencies.analytics,
        dependencies.auth,
        dependencies.public_base_url,
        dependencies.dashboard_origin,
    )
    .await?;
    axum::serve(listener, router(state)).await?;
    Ok(())
}

async fn runtime_dependencies(config: Config) -> StartupResult<RuntimeDependencies> {
    let database = Client::with_uri_str(&config.mongodb_uri)
        .await?
        .database(&config.mongodb_database);
    Ok(RuntimeDependencies {
        database,
        workflows: TinyFlowsClient::new(config.tinyflows_webhook_url),
        analytics: AnalyticsWriter::new(&config.clickhouse_url, &config.clickhouse_database),
        auth: GoogleOAuth::new(
            config.google_client_id,
            config.google_client_secret,
            config.google_redirect_url,
            config.jwt_secret,
        ),
        public_base_url: config.public_base_url,
        dashboard_origin: config.dashboard_origin,
    })
}

#[cfg(test)]
mod test {
    //! Entrypoint tests using an isolated MongoDB container and ephemeral listener.

    use std::time::Duration;

    use mongodb::Client;
    use testcontainers::{
        GenericImage,
        core::{IntoContainerPort, WaitFor},
        runners::AsyncRunner,
    };
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpStream,
        time::timeout,
    };

    use super::{StartupResult, run, runtime_dependencies, serve};
    use marketing_server::Config;

    fn config(mongodb_uri: String) -> Config {
        Config {
            mongodb_uri,
            mongodb_database: "entrypoint_contract".into(),
            tinyflows_webhook_url: "http://127.0.0.1:9/tinyflows".into(),
            public_base_url: "https://dashboard.example.test".into(),
            dashboard_origin: "https://dashboard.example.test".into(),
            clickhouse_url: "http://127.0.0.1:9".into(),
            clickhouse_database: "analytics".into(),
            google_client_id: "client-id".into(),
            google_client_secret: "client-secret".into(),
            google_redirect_url: "https://dashboard.example.test/api/auth/google/callback".into(),
            jwt_secret: "a-32-byte-secret-for-entrypoint-tests".into(),
        }
    }

    #[tokio::test]
    async fn rejects_invalid_listener_and_mongodb_addresses_without_starting_a_server() {
        assert!(
            run(config("mongodb://127.0.0.1:1".into()), "not-an-address")
                .await
                .is_err()
        );
        assert!(
            runtime_dependencies(config("not a mongodb uri".into()))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn serves_health_from_an_ephemeral_listener() -> StartupResult<()> {
        let container = GenericImage::new("mongo", "8.0.0")
            .with_exposed_port(27017.tcp())
            .with_wait_for(WaitFor::message_on_either_std("Waiting for connections"))
            .start()
            .await?;
        let port = container.get_host_port_ipv4(27017.tcp()).await?;
        let mongodb_uri = format!("mongodb://127.0.0.1:{port}");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let server = tokio::spawn(serve(config(mongodb_uri.clone()), listener));

        let mut stream = timeout(Duration::from_secs(5), TcpStream::connect(address)).await??;
        stream
            .write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .await?;
        let mut response = [0; 1024];
        let response_length = timeout(Duration::from_secs(5), stream.read(&mut response)).await??;
        assert!(std::str::from_utf8(&response[..response_length])?.starts_with("HTTP/1.1 204"));

        server.abort();
        let _ = server.await;
        Client::with_uri_str(&format!("{mongodb_uri}/?directConnection=true"))
            .await?
            .database("entrypoint_contract")
            .drop()
            .await?;
        Ok(())
    }
}
