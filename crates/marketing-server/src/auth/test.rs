//! Tests for Google OAuth authorization URLs, state, and session issuance.

use chrono::{Duration, Utc};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};
use std::{
    fmt::Display,
    io::{Error as IoError, ErrorKind, Read, Write},
    net::TcpListener,
    thread,
};

use super::{Claims, GoogleOAuth, GoogleUser, StateClaims, TokenResponse, validated_access_token};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct MockResponse {
    status: u16,
    body: &'static str,
}

struct CapturedRequest {
    request_line: String,
    headers: String,
    body: Vec<u8>,
}

type OAuthServer = (
    String,
    thread::JoinHandle<std::io::Result<Vec<CapturedRequest>>>,
);

fn oauth_server(responses: Vec<MockResponse>) -> TestResult<OAuthServer> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let base_url = format!("http://{}", listener.local_addr()?);
    let server = thread::spawn(move || -> std::io::Result<Vec<CapturedRequest>> {
        let mut requests = Vec::with_capacity(responses.len());
        for response in responses {
            let (mut stream, _) = listener.accept()?;
            let mut request = Vec::new();
            let mut buffer = [0_u8; 4096];
            let header_end = loop {
                let read = stream.read(&mut buffer)?;
                if read == 0 {
                    return Err(IoError::new(
                        ErrorKind::UnexpectedEof,
                        "request ended before headers",
                    ));
                }
                request.extend_from_slice(&buffer[..read]);
                if let Some(position) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
                    break position + 4;
                }
            };
            let headers = std::str::from_utf8(&request[..header_end])
                .map_err(IoError::other)?
                .to_owned();
            let content_length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then_some(value.trim())
                })
                .unwrap_or("0")
                .parse::<usize>()
                .map_err(IoError::other)?;
            while request.len() < header_end + content_length {
                let read = stream.read(&mut buffer)?;
                if read == 0 {
                    return Err(IoError::new(
                        ErrorKind::UnexpectedEof,
                        "request ended before body",
                    ));
                }
                request.extend_from_slice(&buffer[..read]);
            }
            stream.write_all(
                format!(
                    "HTTP/1.1 {} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    response.status,
                    response.body.len(),
                    response.body,
                )
                .as_bytes(),
            )?;
            requests.push(CapturedRequest {
                request_line: headers
                    .lines()
                    .next()
                    .ok_or_else(|| IoError::other("request line is present"))?
                    .into(),
                headers,
                body: request[header_end..header_end + content_length].to_vec(),
            });
        }
        Ok(requests)
    });
    Ok((base_url, server))
}

fn mock_oauth(base_url: &str) -> GoogleOAuth {
    oauth().with_endpoints(format!("{base_url}/token"), format!("{base_url}/userinfo"))
}

fn error_text<T, E: Display>(result: Result<T, E>) -> Option<String> {
    result.err().map(|error| error.to_string())
}

fn oauth() -> GoogleOAuth {
    GoogleOAuth::new(
        "client id".into(),
        "secret".into(),
        "https://dashboard.example.test/callback".into(),
        "a sufficiently long JWT secret for deterministic tests".into(),
    )
}

#[test]
fn authorization_url_contains_encoded_oauth_parameters() {
    let url = oauth().authorization_url("random state");

    assert!(url.starts_with("https://accounts.google.com/o/oauth2/v2/auth?"));
    assert!(url.contains("client_id=client+id"));
    assert!(url.contains("redirect_uri=https%3A%2F%2Fdashboard.example.test%2Fcallback"));
    assert!(url.contains("response_type=code"));
    assert!(url.contains("scope=openid+email+profile"));
    assert!(url.contains("state=random+state"));
}

#[test]
fn authorization_url_encodes_reserved_state_characters() {
    let url = oauth().authorization_url("state&code=attacker-value");

    assert!(url.contains("state=state%26code%3Dattacker-value"));
    assert!(!url.contains("&code=attacker-value"));
}

#[test]
fn debug_output_does_not_expose_secrets() {
    let output = format!("{:?}", oauth());

    assert!(output.contains("client id"));
    assert!(!output.contains("jwt secret"));
    assert!(!output.contains("secret\""));
}

#[test]
fn generated_state_is_valid_and_unique() -> TestResult {
    let oauth = oauth();

    let first = oauth.generate_state()?;
    let second = oauth.generate_state()?;

    assert_ne!(first, second);
    assert!(oauth.is_valid_state(&first));
    assert!(oauth.is_valid_state(&second));
    Ok(())
}

#[test]
fn state_signed_with_another_secret_is_rejected() -> TestResult {
    let trusted = oauth();
    let untrusted = GoogleOAuth::new(
        "client".into(),
        "secret".into(),
        "https://dashboard.example.test/callback".into(),
        "another sufficiently long JWT secret for tests".into(),
    );

    let state = untrusted.generate_state()?;

    assert!(!trusted.is_valid_state(&state));
    assert!(!trusted.is_valid_state("not-a-jwt"));
    Ok(())
}

#[test]
fn expired_state_is_rejected() -> TestResult {
    let oauth = oauth();
    let expired = jsonwebtoken::encode(
        &jsonwebtoken::Header::new(Algorithm::HS256),
        &StateClaims {
            iat: (Utc::now() - Duration::minutes(20)).timestamp(),
            exp: (Utc::now() - Duration::minutes(2)).timestamp(),
            nonce: 0,
        },
        &jsonwebtoken::EncodingKey::from_secret(
            b"a sufficiently long JWT secret for deterministic tests",
        ),
    )?;

    assert!(!oauth.is_valid_state(&expired));
    Ok(())
}

#[test]
fn rejects_an_empty_google_access_token() {
    let error = error_text(validated_access_token(TokenResponse {
        access_token: " ".into(),
    }));

    assert_eq!(
        error.as_deref(),
        Some("Google did not return an access token")
    );
}

#[tokio::test]
async fn exchange_code_rejects_an_empty_code_before_network_access() {
    let error = error_text(oauth().exchange_code(" \t").await);

    assert_eq!(
        error.as_deref(),
        Some("OAuth authorization code is required")
    );
}

#[tokio::test]
async fn exchange_code_posts_the_code_then_uses_the_access_token() -> TestResult {
    let (base_url, server) = oauth_server(vec![
        MockResponse {
            status: 200,
            body: r#"{"access_token":"access-token"}"#,
        },
        MockResponse {
            status: 200,
            body: r#"{"sub":"google-id","email":"person@example.test","email_verified":true}"#,
        },
    ])?;

    let session = mock_oauth(&base_url).exchange_code("code value").await?;
    let requests = server
        .join()
        .map_err(|_| IoError::other("OAuth mock thread panicked"))??;

    assert_eq!(session.email, "person@example.test");
    assert!(requests[0].request_line.starts_with("POST /token HTTP/1.1"));
    assert_eq!(
        std::str::from_utf8(&requests[0].body)?,
        "code=code+value&client_id=client+id&client_secret=secret&redirect_uri=https%3A%2F%2Fdashboard.example.test%2Fcallback&grant_type=authorization_code"
    );
    assert!(
        requests[1]
            .request_line
            .starts_with("GET /userinfo HTTP/1.1")
    );
    assert!(
        requests[1]
            .headers
            .lines()
            .any(|line| line == "authorization: Bearer access-token")
    );
    Ok(())
}

#[tokio::test]
async fn exchange_code_maps_a_rejected_token_response_to_a_workflow_error() -> TestResult {
    let (base_url, server) = oauth_server(vec![MockResponse {
        status: 400,
        body: r#"{"error":"invalid_grant"}"#,
    }])?;

    let result = mock_oauth(&base_url).exchange_code("rejected-code").await;
    let _requests = server
        .join()
        .map_err(|_| IoError::other("OAuth mock thread panicked"))??;

    assert!(matches!(result, Err(crate::Error::Workflow(_))));
    Ok(())
}

#[tokio::test]
async fn exchange_code_rejects_an_unverified_google_user() -> TestResult {
    let (base_url, server) = oauth_server(vec![
        MockResponse {
            status: 200,
            body: r#"{"access_token":"access-token"}"#,
        },
        MockResponse {
            status: 200,
            body: r#"{"sub":"google-id","email":"person@example.test","email_verified":false}"#,
        },
    ])?;

    let result = mock_oauth(&base_url).exchange_code("code").await;
    let _requests = server
        .join()
        .map_err(|_| IoError::other("OAuth mock thread panicked"))??;

    assert_eq!(
        error_text(result).as_deref(),
        Some("Google account email is not verified")
    );
    Ok(())
}

#[test]
fn issue_session_signs_a_24_hour_hs256_token() -> TestResult {
    let oauth = oauth();
    let now = Utc::now();
    let session = oauth.issue_session(
        GoogleUser {
            sub: "google-user-id".into(),
            email: "person@example.test".into(),
            email_verified: true,
        },
        now,
    )?;
    let claims = decode::<Claims>(
        &session.token,
        &DecodingKey::from_secret(b"a sufficiently long JWT secret for deterministic tests"),
        &Validation::new(Algorithm::HS256),
    )?
    .claims;

    assert_eq!(session.email, "person@example.test");
    assert_eq!(claims.sub, "google-user-id");
    assert_eq!(claims.email, "person@example.test");
    assert_eq!(claims.iat, now.timestamp());
    assert_eq!(claims.exp, (now + Duration::hours(24)).timestamp());
    Ok(())
}

#[test]
fn dashboard_token_validation_accepts_a_current_session() -> TestResult {
    let oauth = oauth();
    let session = oauth.issue_session(
        GoogleUser {
            sub: "google-user-id".into(),
            email: "person@example.test".into(),
            email_verified: true,
        },
        Utc::now(),
    )?;

    oauth.validate_dashboard_token(&session.token)?;
    Ok(())
}

#[test]
fn dashboard_token_validation_rejects_tampered_and_expired_tokens() -> TestResult {
    let oauth = oauth();
    let tampered = oauth
        .issue_session(
            GoogleUser {
                sub: "google-user-id".into(),
                email: "person@example.test".into(),
                email_verified: true,
            },
            Utc::now(),
        )?
        .token;
    let mut bytes = tampered.into_bytes();
    let last = bytes.len() - 1;
    bytes[last] = if bytes[last] == b'a' { b'b' } else { b'a' };
    let tampered = String::from_utf8(bytes)?;
    let expired = jsonwebtoken::encode(
        &jsonwebtoken::Header::new(Algorithm::HS256),
        &Claims {
            sub: "google-user-id".into(),
            email: "person@example.test".into(),
            iat: (Utc::now() - Duration::hours(2)).timestamp(),
            exp: (Utc::now() - Duration::minutes(2)).timestamp(),
        },
        &jsonwebtoken::EncodingKey::from_secret(
            b"a sufficiently long JWT secret for deterministic tests",
        ),
    )?;

    for token in [&tampered, &expired] {
        let error = error_text(oauth.validate_dashboard_token(token));
        assert_eq!(error.as_deref(), Some("invalid dashboard token"));
    }
    Ok(())
}

#[test]
fn issue_session_rejects_unverified_or_incomplete_google_identities() {
    let oauth = oauth();
    let cases = [
        GoogleUser {
            sub: "id".into(),
            email: "person@example.test".into(),
            email_verified: false,
        },
        GoogleUser {
            sub: " ".into(),
            email: "person@example.test".into(),
            email_verified: true,
        },
        GoogleUser {
            sub: "id".into(),
            email: " ".into(),
            email_verified: true,
        },
    ];

    for user in cases {
        assert!(oauth.issue_session(user, Utc::now()).is_err());
    }
}
