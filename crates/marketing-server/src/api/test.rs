//! Deterministic unit tests for public-route helpers and response shaping.

use axum::{
    body::{Body, to_bytes},
    http::{HeaderMap, HeaderValue, Request, StatusCode, header},
    response::IntoResponse,
};
use chrono::{TimeZone, Utc};
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use mongodb::Client;
use serde::Serialize;
use testcontainers::{
    GenericImage,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use tower::ServiceExt;

use super::{
    ContactResponse, callback_state_is_valid, clear_oauth_state_cookie, cookie_value,
    has_public_origin, health, html_attribute_escape, is_unsubscribe_token, login_response,
    oauth_state_cookie, unsubscribe_page,
};
use crate::{AnalyticsWriter, AppState, GoogleOAuth, TinyFlowsClient, models::Contact, router};

fn auth() -> GoogleOAuth {
    GoogleOAuth::new(
        "client-id".into(),
        "client-secret".into(),
        "https://dashboard.example.test/callback".into(),
        "01234567890123456789012345678901".into(),
    )
}

#[test]
fn extracts_named_cookie_without_matching_prefixes() {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::COOKIE,
        HeaderValue::from_static("theme=dark; oauth_state=issued; oauth_state_backup=ignored"),
    );

    assert_eq!(cookie_value(&headers, "oauth_state"), Some("issued"));
    assert_eq!(cookie_value(&headers, "missing"), None);
}

#[test]
fn ignores_missing_cookie_headers() {
    assert_eq!(cookie_value(&HeaderMap::new(), "oauth_state"), None);
}

#[test]
fn requires_the_configured_public_origin_for_unsubscribe_posts() {
    let mut headers = HeaderMap::new();
    assert!(!has_public_origin(
        &headers,
        "https://dashboard.example.test"
    ));
    headers.insert(
        header::ORIGIN,
        HeaderValue::from_static("https://attacker.example.test"),
    );
    assert!(!has_public_origin(
        &headers,
        "https://dashboard.example.test"
    ));
    headers.insert(
        header::ORIGIN,
        HeaderValue::from_static("https://dashboard.example.test"),
    );
    assert!(has_public_origin(
        &headers,
        "https://dashboard.example.test"
    ));
}

#[test]
fn produces_a_secure_short_lived_oauth_cookie() -> Result<(), Box<dyn std::error::Error>> {
    let cookie = oauth_state_cookie("signed-state")?;
    assert_eq!(
        cookie.to_str()?,
        "oauth_state=signed-state; Path=/api/auth/google; Max-Age=600; HttpOnly; Secure; SameSite=Lax"
    );
    assert_eq!(
        clear_oauth_state_cookie().to_str()?,
        "oauth_state=; Path=/api/auth/google; Max-Age=0; HttpOnly; Secure; SameSite=Lax"
    );
    Ok(())
}

#[test]
fn rejects_cookie_values_that_cannot_be_used_in_an_http_header() {
    assert!(oauth_state_cookie("contains\nnewline").is_err());
}

#[test]
fn requires_a_matching_signed_state_bound_to_the_browser() -> Result<(), Box<dyn std::error::Error>>
{
    let auth = auth();
    let state = auth.generate_state()?;
    let mut headers = HeaderMap::new();
    headers.insert(
        header::COOKIE,
        HeaderValue::from_str(&format!("oauth_state={state}"))?,
    );

    assert!(callback_state_is_valid(&auth, &headers, &state));
    assert!(!callback_state_is_valid(&auth, &headers, "forged"));

    let mut mismatched = HeaderMap::new();
    mismatched.insert(
        header::COOKIE,
        HeaderValue::from_static("oauth_state=other"),
    );
    assert!(!callback_state_is_valid(&auth, &mismatched, &state));
    Ok(())
}

#[test]
fn login_response_redirects_to_google_and_sets_the_bound_state_cookie()
-> Result<(), Box<dyn std::error::Error>> {
    let auth = auth();
    let response = login_response(&auth, "signed-state")?;

    assert_eq!(response.status(), StatusCode::TEMPORARY_REDIRECT);
    assert_eq!(
        response
            .headers()
            .get(header::SET_COOKIE)
            .and_then(|value| value.to_str().ok()),
        Some(
            "oauth_state=signed-state; Path=/api/auth/google; Max-Age=600; HttpOnly; Secure; SameSite=Lax"
        )
    );
    let location = response
        .headers()
        .get(header::LOCATION)
        .ok_or("redirect has location")?;
    assert!(
        location
            .to_str()?
            .starts_with("https://accounts.google.com/o/oauth2/v2/auth?")
    );
    Ok(())
}

#[tokio::test]
async fn health_route_is_empty_and_unsubscribe_page_posts_a_validated_token()
-> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(health().await, StatusCode::NO_CONTENT);
    let response = unsubscribe_page(axum::extract::Path(
        "0123456789abcdef0123456789abcdef".into(),
    ))
    .await?
    .into_response();
    let body = to_bytes(response.into_body(), usize::MAX).await?;
    assert_eq!(
        std::str::from_utf8(&body)?,
        "<!doctype html><title>Unsubscribe</title><main><h1>Unsubscribe from email</h1><form method=\"post\" action=\"/unsubscribe/0123456789abcdef0123456789abcdef\"><button type=\"submit\">Unsubscribe</button></form></main>"
    );
    assert!(
        unsubscribe_page(axum::extract::Path("invalid".into()))
            .await
            .is_err()
    );
    Ok(())
}

#[test]
fn validates_opaque_tokens_and_escapes_html_attribute_values() {
    assert!(is_unsubscribe_token("0123456789abcdef0123456789abcdef"));
    assert!(!is_unsubscribe_token("short"));
    assert!(!is_unsubscribe_token("0123456789abcdef0123456789abcdeg"));
    assert_eq!(
        html_attribute_escape("<a href='x'>&\"</a>"),
        "&lt;a href=&#x27;x&#x27;&gt;&amp;&quot;&lt;/a&gt;"
    );
}

#[test]
fn contact_response_uses_a_single_normalized_public_base_url()
-> Result<(), Box<dyn std::error::Error>> {
    let created_at = Utc
        .with_ymd_and_hms(2026, 9, 20, 12, 0, 0)
        .single()
        .ok_or("fixed timestamp is valid")?;
    let contact = Contact {
        id: None,
        email: "ada@example.com".into(),
        first_name: Some("Ada".into()),
        subscribed: true,
        unsubscribe_token: "opaque-token".into(),
        created_at,
        updated_at: created_at,
    };

    let response = ContactResponse::from_contact(&contact, "https://app.example.test///");
    assert_eq!(response.email, "ada@example.com");
    assert!(response.subscribed);
    assert_eq!(
        response.unsubscribe_url,
        "https://app.example.test/unsubscribe/opaque-token"
    );
    Ok(())
}

#[derive(Serialize)]
struct TestClaims<'a> {
    sub: &'a str,
    email: &'a str,
    exp: i64,
    iat: i64,
}

fn dashboard_token() -> Result<String, jsonwebtoken::errors::Error> {
    let now = Utc::now().timestamp();
    encode(
        &Header::new(Algorithm::HS256),
        &TestClaims {
            sub: "google-user",
            email: "ada@example.com",
            iat: now,
            exp: now + 3600,
        },
        &EncodingKey::from_secret(b"01234567890123456789012345678901"),
    )
}

async fn start_http_server(requests: usize) -> Result<String, Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    tokio::spawn(async move {
        for _ in 0..requests {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let mut request = [0_u8; 8192];
            if stream.read(&mut request).await.is_ok() {
                let _ = stream
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                    .await;
            }
        }
    });
    Ok(format!("http://{address}"))
}

async fn asserts_public_routes(app: axum::Router) -> Result<(), Box<dyn std::error::Error>> {
    let health = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/health")
                .header(header::ORIGIN, "https://dashboard.example.test")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(health.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        health
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .and_then(|value| value.to_str().ok()),
        Some("https://dashboard.example.test")
    );

    let login = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/auth/google")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(login.status(), StatusCode::TEMPORARY_REDIRECT);
    let set_cookie = login
        .headers()
        .get(header::SET_COOKIE)
        .and_then(|value| value.to_str().ok())
        .ok_or("login state cookie")?;
    let state = set_cookie
        .strip_prefix("oauth_state=")
        .and_then(|value| value.split(';').next())
        .ok_or("login state value")?;
    let callback = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/auth/google/callback?code=unused&state={state}"
                ))
                .header(header::COOKIE, "oauth_state=not-the-issued-state")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(callback.status(), StatusCode::BAD_REQUEST);
    Ok(())
}

async fn creates_contact(
    app: &axum::Router,
    authorization: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/contacts")
                .header(header::AUTHORIZATION, authorization)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"email":"ada@example.com","first_name":"Ada"}"#,
                ))?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::CREATED);
    let body = to_bytes(response.into_body(), usize::MAX).await?;
    let contact: serde_json::Value = serde_json::from_slice(&body)?;
    let unsubscribe_url = contact["unsubscribe_url"]
        .as_str()
        .ok_or("contact has unsubscribe URL")?;
    Ok(unsubscribe_url
        .rsplit('/')
        .next()
        .ok_or("unsubscribe URL has token")?
        .to_owned())
}

async fn creates_and_launches_campaign(
    app: &axum::Router,
    authorization: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/campaigns")
                .header(header::AUTHORIZATION, authorization)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"name":"Launch","subject":"News","html_body":"{{unsubscribe_url}}"}"#,
                ))?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::CREATED);
    let campaign: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await?)?;
    let id = campaign["_id"]["$oid"]
        .as_str()
        .ok_or("campaign has Mongo extended JSON id")?;
    for expected_status in [StatusCode::OK, StatusCode::BAD_REQUEST] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/campaigns/{id}/launch"))
                    .header(header::AUTHORIZATION, authorization)
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(response.status(), expected_status);
    }
    Ok(())
}

#[tokio::test]
async fn router_authenticates_mutations_and_drives_the_campaign_lifecycle()
-> Result<(), Box<dyn std::error::Error>> {
    let mongo = GenericImage::new("mongo", "8.0.0")
        .with_exposed_port(27017.tcp())
        .with_wait_for(WaitFor::message_on_either_std("Waiting for connections"))
        .start()
        .await?;
    let port = mongo.get_host_port_ipv4(27017.tcp()).await?;
    let database = Client::with_uri_str(format!("mongodb://127.0.0.1:{port}"))
        .await?
        .database("api_contract");
    let http_url = start_http_server(5).await?;
    let state = AppState::new(
        database,
        TinyFlowsClient::new(http_url.clone()),
        AnalyticsWriter::new(&http_url, "analytics"),
        auth(),
        "https://dashboard.example.test/".into(),
        "https://dashboard.example.test".into(),
    )
    .await?;
    let app = router(state);
    asserts_public_routes(app.clone()).await?;

    let unauthenticated = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/campaigns")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"name":"Launch","subject":"News","html_body":"{{unsubscribe_url}}"}"#,
                ))?,
        )
        .await?;
    assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);

    let authorization = format!("Bearer {}", dashboard_token()?);
    let token = creates_contact(&app, &authorization).await?;
    creates_and_launches_campaign(&app, &authorization).await?;

    let unsubscribe_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/unsubscribe/{token}"))
                .header(header::ORIGIN, "https://attacker.example.test")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(unsubscribe_response.status(), StatusCode::BAD_REQUEST);

    let unsubscribe_response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/unsubscribe/{token}"))
                .header(header::ORIGIN, "https://dashboard.example.test")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(unsubscribe_response.status(), StatusCode::OK);
    Ok(())
}
