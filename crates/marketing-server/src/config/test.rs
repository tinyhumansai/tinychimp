//! Tests for environment configuration validation without mutating process state.

use std::collections::HashMap;
use std::fmt::Display;

use super::{Config, validate_http_url, validate_origin};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn error_text<T, E: Display>(result: Result<T, E>) -> Option<String> {
    result.err().map(|error| error.to_string())
}

fn valid_values() -> HashMap<&'static str, String> {
    HashMap::from([
        ("MONGODB_URI", "mongodb://localhost:27017".into()),
        ("MONGODB_DATABASE", "marketing".into()),
        (
            "TINYFLOWS_WEBHOOK_URL",
            "https://flows.example.test/hooks".into(),
        ),
        ("CLICKHOUSE_URL", "http://localhost:8123".into()),
        ("CLICKHOUSE_DATABASE", "analytics".into()),
        ("GOOGLE_CLIENT_ID", "client-id".into()),
        ("GOOGLE_CLIENT_SECRET", "client-secret".into()),
        (
            "GOOGLE_REDIRECT_URL",
            "https://dashboard.example.test/callback".into(),
        ),
        ("JWT_SECRET", "at-least-thirty-two-bytes-long-secret".into()),
    ])
}

#[test]
fn loads_required_values_and_default_public_url() -> TestResult {
    let values = valid_values();

    let config = Config::from_lookup(|name| values.get(name).cloned())?;

    assert_eq!(config.mongodb_database, "marketing");
    assert_eq!(config.public_base_url, "http://localhost:3000");
    assert_eq!(config.dashboard_origin, "http://localhost:5173");
    assert_eq!(
        config.google_redirect_url,
        "https://dashboard.example.test/callback"
    );
    Ok(())
}

#[test]
fn accepts_a_configured_dashboard_origin_and_rejects_non_origins() -> TestResult {
    let mut values = valid_values();
    values.insert("DASHBOARD_ORIGIN", "https://dashboard.example.test".into());
    let config = Config::from_lookup(|name| values.get(name).cloned())?;
    assert_eq!(config.dashboard_origin, "https://dashboard.example.test");

    for origin in [
        "not a URL",
        "https://dashboard.example.test/path",
        "https://dashboard.example.test?query=true",
        "ftp://dashboard.example.test",
        "https://user:password@dashboard.example.test",
    ] {
        assert!(validate_origin("DASHBOARD_ORIGIN", origin).is_err());
    }
    Ok(())
}

#[test]
fn accepts_an_explicit_https_public_url() -> TestResult {
    let mut values = valid_values();
    values.insert("PUBLIC_BASE_URL", "https://dashboard.example.test/".into());

    let config = Config::from_lookup(|name| values.get(name).cloned())?;

    assert_eq!(config.public_base_url, "https://dashboard.example.test/");
    Ok(())
}

#[test]
fn rejects_missing_and_blank_required_values() {
    let mut missing = valid_values();
    missing.remove("MONGODB_URI");
    let error = error_text(Config::from_lookup(|name| missing.get(name).cloned()));
    assert_eq!(error.as_deref(), Some("missing required MONGODB_URI"));

    let mut blank = valid_values();
    blank.insert("GOOGLE_CLIENT_SECRET", "  ".into());
    let error = error_text(Config::from_lookup(|name| blank.get(name).cloned()));
    assert_eq!(
        error.as_deref(),
        Some("required GOOGLE_CLIENT_SECRET must not be empty")
    );
}

#[test]
fn rejects_invalid_urls_and_weak_jwt_secrets() {
    let mut invalid_url = valid_values();
    invalid_url.insert("GOOGLE_REDIRECT_URL", "not a URL".into());
    let error = error_text(Config::from_lookup(|name| invalid_url.get(name).cloned()));
    assert_eq!(
        error.as_deref(),
        Some("GOOGLE_REDIRECT_URL must be an absolute HTTP URL")
    );

    let mut weak_secret = valid_values();
    weak_secret.insert("JWT_SECRET", "too-short".into());
    let error = error_text(Config::from_lookup(|name| weak_secret.get(name).cloned()));
    assert_eq!(
        error.as_deref(),
        Some("JWT_SECRET must be at least 32 bytes")
    );
}

#[test]
fn rejects_a_blank_or_non_http_public_url() {
    let mut blank = valid_values();
    blank.insert("PUBLIC_BASE_URL", " ".into());
    let error = error_text(Config::from_lookup(|name| blank.get(name).cloned()));
    assert_eq!(error.as_deref(), Some("PUBLIC_BASE_URL must not be empty"));

    let mut file_url = valid_values();
    file_url.insert("PUBLIC_BASE_URL", "file:///dashboard".into());
    let error = error_text(Config::from_lookup(|name| file_url.get(name).cloned()));
    assert_eq!(
        error.as_deref(),
        Some("PUBLIC_BASE_URL must be an absolute HTTP URL")
    );
}

#[test]
fn reports_each_required_setting_when_it_is_missing_or_blank() {
    for name in [
        "MONGODB_URI",
        "MONGODB_DATABASE",
        "TINYFLOWS_WEBHOOK_URL",
        "CLICKHOUSE_URL",
        "CLICKHOUSE_DATABASE",
        "GOOGLE_CLIENT_ID",
        "GOOGLE_CLIENT_SECRET",
        "GOOGLE_REDIRECT_URL",
        "JWT_SECRET",
    ] {
        let mut missing = valid_values();
        missing.remove(name);
        let error = error_text(Config::from_lookup(|key| missing.get(key).cloned()));
        let expected = format!("missing required {name}");
        assert_eq!(error.as_deref(), Some(expected.as_str()));

        let mut blank = valid_values();
        blank.insert(name, " \t".into());
        let error = error_text(Config::from_lookup(|key| blank.get(key).cloned()));
        let expected = format!("required {name} must not be empty");
        assert_eq!(error.as_deref(), Some(expected.as_str()));
    }
}

#[test]
fn environment_lookup_has_the_same_validation_contract() {
    let _result = Config::from_env();
}

#[test]
fn debug_output_redacts_secrets_and_urls_cover_both_invalid_forms() -> TestResult {
    let values = valid_values();
    let config = Config::from_lookup(|name| values.get(name).cloned())?;
    let debug = format!("{config:?}");
    assert!(debug.contains("Config"));
    assert!(!debug.contains("client-secret"));
    assert!(!debug.contains("JWT_SECRET"));

    assert!(validate_http_url("endpoint", "not a URL").is_err());
    assert!(validate_http_url("endpoint", "mailto:person@example.test").is_err());
    Ok(())
}
