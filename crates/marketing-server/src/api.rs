//! Axum routes for the dashboard, automation service, and public unsubscribe page.

use std::{fmt, sync::Arc};

use axum::{
    Json, Router,
    extract::{Path, Query, Request, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    middleware::{self, Next},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
};
use chrono::Utc;
use mongodb::Database;
use serde::{Deserialize, Serialize};
use tower_http::cors::{Any, CorsLayer};
use url::Url;

use crate::{
    AnalyticsWriter, GoogleOAuth, MarketingRepository, TinyFlowsClient,
    analytics::CampaignEvent,
    error::Result,
    models::{CreateCampaign, CreateContact},
};

/// Shared dependencies for Axum handlers.
#[derive(Clone)]
pub struct AppState {
    repository: MarketingRepository,
    workflows: TinyFlowsClient,
    analytics: AnalyticsWriter,
    auth: GoogleOAuth,
    public_base_url: String,
    dashboard_origin: HeaderValue,
    public_origin: String,
}

impl AppState {
    /// Builds state and initializes MongoDB indexes.
    ///
    /// # Errors
    ///
    /// Returns an error if MongoDB index initialization fails.
    pub async fn new(
        database: Database,
        workflows: TinyFlowsClient,
        analytics: AnalyticsWriter,
        auth: GoogleOAuth,
        public_base_url: String,
        dashboard_origin: String,
    ) -> Result<Self> {
        let dashboard_origin = HeaderValue::from_str(&dashboard_origin)
            .map_err(|error| crate::Error::Validation(error.to_string()))?;
        let public_origin = Url::parse(&public_base_url)
            .map_err(|_| {
                crate::Error::Validation("PUBLIC_BASE_URL must be an absolute HTTP URL".into())
            })?
            .origin()
            .ascii_serialization();
        Ok(Self {
            repository: MarketingRepository::new(database).await?,
            workflows,
            analytics,
            auth,
            public_base_url,
            dashboard_origin,
            public_origin,
        })
    }
}

/// Creates the application router.
pub fn router(state: AppState) -> Router {
    let state = Arc::new(state);
    let protected_api = Router::new()
        .route("/api/contacts", post(create_contact))
        .route("/api/campaigns", post(create_campaign))
        .route("/api/campaigns/{id}/launch", post(launch_campaign))
        .route_layer(middleware::from_fn_with_state(
            Arc::clone(&state),
            require_dashboard_session,
        ));
    Router::new()
        .route("/health", get(health))
        .route("/api/auth/google", get(google_login))
        .route("/api/auth/google/callback", get(google_callback))
        .route(
            "/unsubscribe/{token}",
            get(unsubscribe_page).post(unsubscribe),
        )
        .merge(protected_api)
        .layer(
            CorsLayer::new()
                .allow_origin(state.dashboard_origin.clone())
                .allow_methods([Method::GET, Method::POST])
                .allow_headers(Any),
        )
        .with_state(state)
}

async fn require_dashboard_session(
    State(state): State<Arc<AppState>>,
    request: Request,
    next: Next,
) -> Response {
    let token = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    if token.is_none_or(|token| state.auth.validate_dashboard_token(token).is_err()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    next.run(request).await
}

async fn health() -> StatusCode {
    StatusCode::NO_CONTENT
}

async fn create_contact(
    State(state): State<Arc<AppState>>,
    Json(input): Json<CreateContact>,
) -> Result<impl IntoResponse> {
    let contact = state.repository.create_contact(input).await?;
    state.workflows.trigger("contact.created", &contact).await?;
    Ok((
        StatusCode::CREATED,
        Json(ContactResponse::from_contact(
            &contact,
            &state.public_base_url,
        )),
    ))
}

async fn create_campaign(
    State(state): State<Arc<AppState>>,
    Json(input): Json<CreateCampaign>,
) -> Result<impl IntoResponse> {
    let campaign = state.repository.create_campaign(input).await?;
    Ok((StatusCode::CREATED, Json(campaign)))
}

async fn launch_campaign(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse> {
    let outcome = state.repository.launch_campaign(&id).await?;
    if outcome.newly_launched {
        state
            .workflows
            .trigger("campaign.launched", &outcome.campaign)
            .await?;
        state
            .analytics
            .record(&CampaignEvent {
                event_name: "campaign_launched".into(),
                campaign_id: id.clone(),
                contact_id: String::new(),
                occurred_at: Utc::now(),
            })
            .await?;
        let campaign = state.repository.complete_campaign_launch(&id).await?;
        return Ok(Json(campaign));
    }
    Ok(Json(outcome.campaign))
}

async fn google_login(State(state): State<Arc<AppState>>) -> Result<impl IntoResponse> {
    let oauth_state = state.auth.generate_state()?;
    login_response(&state.auth, &oauth_state)
}

#[derive(Deserialize)]
struct CallbackQuery {
    code: String,
    state: String,
}

async fn google_callback(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<CallbackQuery>,
) -> Result<impl IntoResponse> {
    if !callback_state_is_valid(&state.auth, &headers, &query.state) {
        return Err(crate::Error::Validation("OAuth state is invalid".into()));
    }
    let session = state.auth.exchange_code(&query.code).await?;
    Ok((
        [(header::SET_COOKIE, clear_oauth_state_cookie())],
        Json(session),
    ))
}

fn login_response(auth: &GoogleOAuth, oauth_state: &str) -> Result<Response> {
    Ok((
        [(header::SET_COOKIE, oauth_state_cookie(oauth_state)?)],
        Redirect::temporary(&auth.authorization_url(oauth_state)),
    )
        .into_response())
}

fn oauth_state_cookie(oauth_state: &str) -> Result<HeaderValue> {
    HeaderValue::from_str(&format!(
        "oauth_state={oauth_state}; Path=/api/auth/google; Max-Age=600; HttpOnly; Secure; SameSite=Lax"
    ))
    .map_err(|error| crate::Error::Validation(error.to_string()))
}

fn clear_oauth_state_cookie() -> HeaderValue {
    HeaderValue::from_static(
        "oauth_state=; Path=/api/auth/google; Max-Age=0; HttpOnly; Secure; SameSite=Lax",
    )
}

fn callback_state_is_valid(auth: &GoogleOAuth, headers: &HeaderMap, state: &str) -> bool {
    cookie_value(headers, "oauth_state").is_some_and(|value| value == state)
        && auth.is_valid_state(state)
}

fn cookie_value<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .map(str::trim)
        .find_map(|part| {
            part.split_once('=')
                .filter(|(key, _)| *key == name)
                .map(|(_, value)| value)
        })
}

async fn unsubscribe_page(Path(token): Path<String>) -> Result<Html<String>> {
    if !is_unsubscribe_token(&token) {
        return Err(crate::Error::Validation(
            "unsubscribe link is invalid".into(),
        ));
    }
    let action_token = html_attribute_escape(&token);
    Ok(Html(format!(
        "<!doctype html><title>Unsubscribe</title><main><h1>Unsubscribe from email</h1><form method=\"post\" action=\"/unsubscribe/{action_token}\"><button type=\"submit\">Unsubscribe</button></form></main>"
    )))
}

async fn unsubscribe(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(token): Path<String>,
) -> Result<Html<&'static str>> {
    if !has_public_origin(&headers, &state.public_origin) {
        return Err(crate::Error::Validation(
            "unsubscribe request has an invalid origin".into(),
        ));
    }
    let contact = state.repository.unsubscribe(&token).await?;
    state
        .workflows
        .trigger("contact.unsubscribed", &contact)
        .await?;
    state
        .analytics
        .record(&CampaignEvent {
            event_name: "contact_unsubscribed".into(),
            campaign_id: String::new(),
            contact_id: contact
                .id
                .map_or_else(String::new, mongodb::bson::oid::ObjectId::to_hex),
            occurred_at: Utc::now(),
        })
        .await?;
    Ok(Html(
        "<!doctype html><title>Unsubscribed</title><main><h1>You are unsubscribed</h1><p>You will no longer receive marketing email from us.</p></main>",
    ))
}

fn has_public_origin(headers: &HeaderMap, public_origin: &str) -> bool {
    headers
        .get(header::ORIGIN)
        .and_then(|origin| origin.to_str().ok())
        .is_some_and(|origin| origin == public_origin)
}

fn is_unsubscribe_token(token: &str) -> bool {
    token.len() == 32 && token.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn html_attribute_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

impl fmt::Debug for AppState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("AppState").finish_non_exhaustive()
    }
}

#[derive(Serialize)]
struct ContactResponse {
    email: String,
    subscribed: bool,
    unsubscribe_url: String,
}

impl ContactResponse {
    fn from_contact(contact: &crate::models::Contact, public_base_url: &str) -> Self {
        Self {
            email: contact.email.clone(),
            subscribed: contact.subscribed,
            unsubscribe_url: format!(
                "{}/unsubscribe/{}",
                public_base_url.trim_end_matches('/'),
                contact.unsubscribe_token
            ),
        }
    }
}

#[cfg(test)]
mod test;
