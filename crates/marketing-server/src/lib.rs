//! Email marketing automation service primitives.
//!
//! This crate provides an Axum API backed by MongoDB, with an outbound
//! `TinyFlows` webhook adapter for campaign lifecycle automation. It deliberately
//! does not send email itself: delivery providers belong behind a dedicated
//! worker or `TinyFlows` flow.

pub mod analytics;
pub mod api;
pub mod auth;
pub mod config;
pub mod error;
pub mod models;
pub mod repository;
pub mod tinyflows;

pub use analytics::AnalyticsWriter;
pub use api::{AppState, router};
pub use auth::GoogleOAuth;
pub use config::Config;
pub use error::{Error, Result};
pub use repository::MarketingRepository;
pub use tinyflows::TinyFlowsClient;
