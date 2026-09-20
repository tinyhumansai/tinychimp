//! API and persistence model definitions.

use chrono::{DateTime, Utc};
use mongodb::bson::oid::ObjectId;
use serde::{Deserialize, Serialize};

/// Request to create a contact.
#[derive(Debug, Deserialize)]
pub struct CreateContact {
    /// Email address to subscribe.
    pub email: String,
    /// Display name used for personalization.
    pub first_name: Option<String>,
}

/// A marketing contact persisted in MongoDB.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Contact {
    /// MongoDB document identifier.
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,
    /// Contact email address.
    pub email: String,
    /// Optional display name.
    pub first_name: Option<String>,
    /// Whether the contact can receive marketing email.
    pub subscribed: bool,
    /// Opaque token embedded in the unsubscribe URL.
    pub unsubscribe_token: String,
    /// Record creation time.
    pub created_at: DateTime<Utc>,
    /// Most recent subscription preference change.
    pub updated_at: DateTime<Utc>,
}

/// Request to create a campaign draft.
#[derive(Debug, Deserialize)]
pub struct CreateCampaign {
    /// Dashboard-facing campaign title.
    pub name: String,
    /// Email subject line.
    pub subject: String,
    /// HTML email body. It must include the supplied unsubscribe URL placeholder.
    pub html_body: String,
}

/// A campaign draft or launched campaign.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Campaign {
    /// MongoDB document identifier.
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,
    /// Dashboard-facing campaign title.
    pub name: String,
    /// Email subject line.
    pub subject: String,
    /// HTML email body.
    pub html_body: String,
    /// Current lifecycle state.
    pub status: CampaignStatus,
    /// Record creation time.
    pub created_at: DateTime<Utc>,
    /// Launch time, if the flow was started.
    pub launched_at: Option<DateTime<Utc>>,
}

/// Campaign lifecycle state.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CampaignStatus {
    /// Campaign can be edited but has not entered an automation flow.
    Draft,
    /// Launch request was accepted and delivery automation is being started.
    Launching,
    /// Campaign delivery automation has started.
    Launched,
}
