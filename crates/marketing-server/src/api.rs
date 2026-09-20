//! Axum routes for the dashboard, automation service, and public unsubscribe page.

use std::{fmt, sync::Arc};

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderValue, Method, StatusCode},
    response::{Html, IntoResponse, Redirect},
    routing::{get, post},
};
use chrono::Utc;
use mongodb::Database;
use serde::{Deserialize, Serialize};
use tower_http::cors::{Any, CorsLayer};

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
    ) -> Result<Self> {
        Ok(Self {
            repository: MarketingRepository::new(database).await?,
            workflows,
            analytics,
            auth,
            public_base_url,
        })
    }
}

/// Creates the application router.
pub fn router(state: AppState) -> Router {
    let dashboard_origin = HeaderValue::from_static("http://localhost:5173");
    Router::new()
        .route("/health", get(health))
        .route("/api/contacts", post(create_contact))
        .route("/api/campaigns", post(create_campaign))
        .route("/api/campaigns/{id}/launch", post(launch_campaign))
        .route("/api/auth/google", get(google_login))
        .route("/api/auth/google/callback", get(google_callback))
        .route(
            "/unsubscribe/{token}",
            get(unsubscribe_page).post(unsubscribe),
        )
        .layer(
            CorsLayer::new()
                .allow_origin(dashboard_origin)
                .allow_methods([Method::GET, Method::POST])
                .allow_headers(Any),
        )
        .with_state(Arc::new(state))
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
    let campaign = state.repository.launch_campaign(&id).await?;
    state
        .workflows
        .trigger("campaign.launched", &campaign)
        .await?;
    state
        .analytics
        .record(&CampaignEvent {
            event_name: "campaign_launched".into(),
            campaign_id: id,
            contact_id: String::new(),
            occurred_at: Utc::now(),
        })
        .await?;
    Ok(Json(campaign))
}

#[derive(Debug, Deserialize)]
struct LoginQuery {
    state: String,
}

async fn google_login(
    State(state): State<Arc<AppState>>,
    Query(query): Query<LoginQuery>,
) -> Redirect {
    Redirect::temporary(&state.auth.authorization_url(&query.state))
}

#[derive(Debug, Deserialize)]
struct CallbackQuery {
    code: String,
    state: String,
}

async fn google_callback(
    State(state): State<Arc<AppState>>,
    Query(query): Query<CallbackQuery>,
) -> Result<Json<crate::auth::Session>> {
    if query.state.trim().is_empty() {
        return Err(crate::Error::Validation("OAuth state is required".into()));
    }
    Ok(Json(state.auth.exchange_code(&query.code).await?))
}

async fn unsubscribe_page() -> Html<&'static str> {
    Html(
        "<!doctype html><title>Unsubscribe</title><main><h1>Unsubscribe from email</h1><p>Use the form in the message to stop marketing email.</p></main>",
    )
}

async fn unsubscribe(
    State(state): State<Arc<AppState>>,
    Path(token): Path<String>,
) -> Result<Html<&'static str>> {
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

impl fmt::Debug for AppState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("AppState").finish_non_exhaustive()
    }
}

#[derive(Debug, Serialize)]
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
