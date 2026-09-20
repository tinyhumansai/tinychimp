//! Google OAuth authorization-code exchange and dashboard JWT issuance.

use chrono::{Duration, Utc};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};
use std::{
    fmt,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
use url::form_urlencoded;

use crate::error::Result;

const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const GOOGLE_USERINFO_URL: &str = "https://openidconnect.googleapis.com/v1/userinfo";

/// Google OAuth client and JWT session issuer.
#[derive(Clone)]
pub struct GoogleOAuth {
    client_id: String,
    client_secret: String,
    redirect_url: String,
    jwt_secret: String,
    token_url: String,
    userinfo_url: String,
    next_state_nonce: Arc<AtomicU64>,
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
#[derive(Serialize)]
pub struct Session {
    /// Signed dashboard API token.
    pub token: String,
    /// Authenticated user's email.
    pub email: String,
}

impl fmt::Debug for Session {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Session")
            .field("email", &self.email)
            .finish_non_exhaustive()
    }
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
#[derive(Debug, Deserialize, Serialize)]
struct Claims {
    sub: String,
    email: String,
    exp: i64,
    iat: i64,
}

#[derive(Debug, Deserialize, Serialize)]
struct StateClaims {
    exp: i64,
    iat: i64,
    nonce: u64,
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
            token_url: GOOGLE_TOKEN_URL.into(),
            userinfo_url: GOOGLE_USERINFO_URL.into(),
            next_state_nonce: Arc::new(AtomicU64::new(0)),
        }
    }

    #[cfg(test)]
    fn with_endpoints(mut self, token_url: String, userinfo_url: String) -> Self {
        self.token_url = token_url;
        self.userinfo_url = userinfo_url;
        self
    }

    /// Creates a signed, short-lived OAuth state value for one login attempt.
    ///
    /// The caller must bind the returned value to the initiating browser (for
    /// example, in a `Secure`, `HttpOnly`, `SameSite=Lax` cookie), then require
    /// an exact match when processing the callback.
    ///
    /// # Errors
    ///
    /// Returns an error if the state token cannot be signed.
    pub fn generate_state(&self) -> Result<String> {
        let now = Utc::now();
        let claims = StateClaims {
            iat: now.timestamp(),
            exp: (now + Duration::minutes(10)).timestamp(),
            nonce: self.next_state_nonce.fetch_add(1, Ordering::Relaxed),
        };
        encode(
            &Header::new(Algorithm::HS256),
            &claims,
            &EncodingKey::from_secret(self.jwt_secret.as_bytes()),
        )
        .map_err(|error| crate::error::Error::Validation(error.to_string()))
    }

    /// Checks that an OAuth state was issued by this server and has not expired.
    #[must_use]
    pub fn is_valid_state(&self, state: &str) -> bool {
        let mut validation = Validation::new(Algorithm::HS256);
        validation.validate_exp = true;
        decode::<StateClaims>(
            state,
            &DecodingKey::from_secret(self.jwt_secret.as_bytes()),
            &validation,
        )
        .is_ok()
    }

    /// Validates a dashboard session JWT for use by request middleware.
    ///
    /// Only unexpired tokens signed with this instance's secret using HS256 are
    /// accepted. The method deliberately returns no claims because callers that
    /// only authorize a request should not need to handle identity data.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the token is malformed, expired, signed
    /// by another key, or uses an unexpected algorithm.
    pub fn validate_dashboard_token(&self, token: &str) -> Result<()> {
        let mut validation = Validation::new(Algorithm::HS256);
        validation.validate_exp = true;
        decode::<Claims>(
            token,
            &DecodingKey::from_secret(self.jwt_secret.as_bytes()),
            &validation,
        )
        .map(|_| ())
        .map_err(|_| crate::error::Error::Validation("invalid dashboard token".into()))
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
        if code.trim().is_empty() {
            return Err(crate::error::Error::Validation(
                "OAuth authorization code is required".into(),
            ));
        }
        let http = reqwest::Client::new();
        let token = http
            .post(&self.token_url)
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
        let access_token = validated_access_token(token)?;
        let user = http
            .get(&self.userinfo_url)
            .bearer_auth(access_token)
            .send()
            .await?
            .error_for_status()?
            .json::<GoogleUser>()
            .await?;
        self.issue_session(user, Utc::now())
    }

    fn issue_session(&self, user: GoogleUser, now: chrono::DateTime<Utc>) -> Result<Session> {
        if !user.email_verified {
            return Err(crate::error::Error::Validation(
                "Google account email is not verified".into(),
            ));
        }
        if user.sub.trim().is_empty() {
            return Err(crate::error::Error::Validation(
                "Google account identifier is required".into(),
            ));
        }
        if user.email.trim().is_empty() {
            return Err(crate::error::Error::Validation(
                "Google account email is required".into(),
            ));
        }
        let claims = Claims {
            sub: user.sub,
            email: user.email.clone(),
            iat: now.timestamp(),
            exp: (now + Duration::hours(24)).timestamp(),
        };
        let token = encode(
            &Header::new(Algorithm::HS256),
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

fn validated_access_token(token: TokenResponse) -> Result<String> {
    if token.access_token.trim().is_empty() {
        return Err(crate::error::Error::Validation(
            "Google did not return an access token".into(),
        ));
    }
    Ok(token.access_token)
}

#[cfg(test)]
mod test;
