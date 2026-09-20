//! Tests for Google OAuth authorization URL construction.

use super::GoogleOAuth;

#[test]
fn authorization_url_contains_encoded_oauth_parameters() {
    let oauth = GoogleOAuth::new(
        "client id".into(),
        "secret".into(),
        "https://dashboard.example.test/callback".into(),
        "jwt-secret".into(),
    );

    let url = oauth.authorization_url("random state");

    assert!(url.starts_with("https://accounts.google.com/o/oauth2/v2/auth?"));
    assert!(url.contains("client_id=client+id"));
    assert!(url.contains("redirect_uri=https%3A%2F%2Fdashboard.example.test%2Fcallback"));
    assert!(url.contains("state=random+state"));
}
