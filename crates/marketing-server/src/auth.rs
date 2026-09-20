//! Google OAuth authorization-code exchange and dashboard JWT issuance.

use chrono::{Duration, Utc};
use jsonwebtoken::{EncodingKey, Header, encode};
use serde::{Deserialize, Serialize};
use std::fmt;
use url::form_urlencoded;

use crate::error::Result;

/// Google OAuth client and JWT session issuer.
#[derive(Clone)]
pub struct GoogleOAuth {
    client_id: String,
    client_secret: String,
    redirect_url: String,
    jwt_secret: String,
}

impl fmt::Debug for GoogleOAuth {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GoogleOAuth")
            .field("client_id", &self.client_id)
            .field("redirect_url", &self.redirect_url)
            .finish_non_exhaustive()
    }
}

/// Response returned after a successful OAuth callback.
#[derive(Debug, Serialize)]
pub struct Session {
    /// Signed dashboard API token.
    pub token: String,
    /// Authenticated user's email.
    pub email: String,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
}
#[derive(Debug, Deserialize)]
struct GoogleUser {
    sub: String,
    email: String,
    email_verified: bool,
}
#[derive(Debug, Serialize)]
struct Claims {
    sub: String,
    email: String,
    exp: i64,
    iat: i64,
}

impl GoogleOAuth {
    /// Creates an OAuth helper from configured credentials.
    #[must_use]
    pub fn new(
        client_id: String,
        client_secret: String,
        redirect_url: String,
        jwt_secret: String,
    ) -> Self {
        Self {
            client_id,
            client_secret,
            redirect_url,
            jwt_secret,
        }
    }

    /// Builds the Google consent URL for a CSRF state token.
    #[must_use]
    pub fn authorization_url(&self, state: &str) -> String {
        let query = form_urlencoded::Serializer::new(String::new())
            .extend_pairs([
                ("client_id", self.client_id.as_str()),
                ("redirect_uri", self.redirect_url.as_str()),
                ("response_type", "code"),
                ("scope", "openid email profile"),
                ("state", state),
            ])
            .finish();
        format!("https://accounts.google.com/o/oauth2/v2/auth?{query}")
    }

    /// Exchanges a Google authorization code and issues a 24-hour API JWT.
    ///
    /// # Errors
    ///
    /// Returns an error if Google rejects the code or the JWT cannot be signed.
    pub async fn exchange_code(&self, code: &str) -> Result<Session> {
        let http = reqwest::Client::new();
        let token = http
            .post("https://oauth2.googleapis.com/token")
            .form(&[
                ("code", code),
                ("client_id", self.client_id.as_str()),
                ("client_secret", self.client_secret.as_str()),
                ("redirect_uri", self.redirect_url.as_str()),
                ("grant_type", "authorization_code"),
            ])
            .send()
            .await?
            .error_for_status()?
            .json::<TokenResponse>()
            .await?;
        let user = http
            .get("https://openidconnect.googleapis.com/v1/userinfo")
            .bearer_auth(token.access_token)
            .send()
            .await?
            .error_for_status()?
            .json::<GoogleUser>()
            .await?;
        if !user.email_verified {
            return Err(crate::error::Error::Validation(
                "Google account email is not verified".into(),
            ));
        }
        let now = Utc::now();
        let claims = Claims {
            sub: user.sub,
            email: user.email.clone(),
            iat: now.timestamp(),
            exp: (now + Duration::hours(24)).timestamp(),
        };
        let token = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(self.jwt_secret.as_bytes()),
        )
        .map_err(|error| crate::error::Error::Validation(error.to_string()))?;
        Ok(Session {
            token,
            email: user.email,
        })
    }
}

#[cfg(test)]
mod test;
