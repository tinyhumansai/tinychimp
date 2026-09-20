//! API and persistence model definitions.

use chrono::{DateTime, TimeZone, Utc};
use mongodb::bson::oid::ObjectId;
use serde::{
    Deserialize, Deserializer, Serialize, Serializer, de::Error as _, ser::SerializeStruct,
};
use std::fmt;

/// Request to create a contact.
#[derive(Debug, Deserialize)]
pub struct CreateContact {
    /// Email address to subscribe.
    pub email: String,
    /// Display name used for personalization.
    pub first_name: Option<String>,
}

/// A marketing contact persisted in MongoDB.
#[derive(Clone, Deserialize, Serialize)]
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
    #[serde(with = "bson_datetime")]
    pub created_at: DateTime<Utc>,
    /// Most recent subscription preference change.
    #[serde(with = "bson_datetime")]
    pub updated_at: DateTime<Utc>,
}

impl fmt::Debug for Contact {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Contact")
            .field("id", &self.id)
            .field("email", &self.email)
            .field("first_name", &self.first_name)
            .field("subscribed", &self.subscribed)
            .field("created_at", &self.created_at)
            .field("updated_at", &self.updated_at)
            .finish_non_exhaustive()
    }
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
#[derive(Clone, Debug, Deserialize)]
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
    #[serde(with = "bson_datetime")]
    pub created_at: DateTime<Utc>,
    /// Launch time, if the flow was started.
    #[serde(deserialize_with = "bson_datetime_option::deserialize")]
    pub launched_at: Option<DateTime<Utc>>,
}

/// Campaign lifecycle state.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CampaignStatus {
    /// Campaign can be edited but has not entered an automation flow.
    Draft,
    /// Launch request was accepted and delivery automation is being started.
    Launching,
    /// Campaign delivery automation has started.
    Launched,
}

/// The result of requesting a campaign launch.
#[derive(Clone, Debug)]
pub struct LaunchOutcome {
    /// The campaign after its launch state was resolved.
    pub campaign: Campaign,
    /// Whether this request moved the campaign into its final launched state.
    pub newly_launched: bool,
}

impl Serialize for Campaign {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state =
            serializer.serialize_struct("Campaign", 7 + usize::from(self.id.is_some()))?;
        if let Some(id) = self.id {
            state.serialize_field("_id", &id)?;
        }
        state.serialize_field("name", &self.name)?;
        state.serialize_field("subject", &self.subject)?;
        state.serialize_field("html_body", &self.html_body)?;
        state.serialize_field("status", &self.status)?;
        state.serialize_field("created_at", &BsonDateTime(&self.created_at))?;
        state.serialize_field("launched_at", &self.launched_at.as_ref().map(BsonDateTime))?;
        state.end()
    }
}

/// Borrowed UTC timestamp that serializes as a BSON datetime.
struct BsonDateTime<'a>(&'a DateTime<Utc>);

impl Serialize for BsonDateTime<'_> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        mongodb::bson::DateTime::from_millis(self.0.timestamp_millis()).serialize(serializer)
    }
}

/// Serde adapter that persists a UTC timestamp as a BSON datetime.
mod bson_datetime {
    use super::*;

    pub(super) fn serialize<S>(
        value: &DateTime<Utc>,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        mongodb::bson::DateTime::from_millis(value.timestamp_millis()).serialize(serializer)
    }

    pub(super) fn deserialize<'de, D>(
        deserializer: D,
    ) -> std::result::Result<DateTime<Utc>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = mongodb::bson::DateTime::deserialize(deserializer)?;
        Utc.timestamp_millis_opt(value.timestamp_millis())
            .single()
            .ok_or_else(|| D::Error::custom("BSON datetime is outside Chrono's range"))
    }
}

/// Serde adapter that persists an optional UTC timestamp as a BSON datetime.
mod bson_datetime_option {
    use super::*;

    pub(super) fn deserialize<'de, D>(
        deserializer: D,
    ) -> std::result::Result<Option<DateTime<Utc>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        Option::<mongodb::bson::DateTime>::deserialize(deserializer)?.map_or(Ok(None), |value| {
            Utc.timestamp_millis_opt(value.timestamp_millis())
                .single()
                .map(Some)
                .ok_or_else(|| D::Error::custom("BSON datetime is outside Chrono's range"))
        })
    }
}

#[cfg(test)]
mod test {
    //! Serialization tests for MongoDB persistence models.

    use chrono::{TimeZone, Utc};
    use mongodb::bson::{Bson, to_document};

    use super::{Campaign, CampaignStatus, Contact};

    #[test]
    fn contact_timestamps_use_bson_datetime_values() -> Result<(), Box<dyn std::error::Error>> {
        let timestamp = Utc
            .with_ymd_and_hms(2026, 9, 20, 12, 34, 56)
            .single()
            .ok_or_else(|| std::io::Error::other("fixed test timestamp is valid"))?;
        let contact = Contact {
            id: None,
            email: "ada@example.com".into(),
            first_name: Some("Ada".into()),
            subscribed: true,
            unsubscribe_token: "token".into(),
            created_at: timestamp,
            updated_at: timestamp,
        };

        let document = to_document(&contact)?;

        assert!(matches!(
            document.get("created_at"),
            Some(Bson::DateTime(_))
        ));
        assert!(matches!(
            document.get("updated_at"),
            Some(Bson::DateTime(_))
        ));
        Ok(())
    }

    #[test]
    fn campaign_round_trips_bson_datetime_and_status() -> Result<(), Box<dyn std::error::Error>> {
        let timestamp = Utc
            .with_ymd_and_hms(2026, 9, 20, 12, 34, 56)
            .single()
            .ok_or_else(|| std::io::Error::other("fixed test timestamp is valid"))?;
        let campaign = Campaign {
            id: None,
            name: "Launch".into(),
            subject: "Hello".into(),
            html_body: "<a href=\"{{unsubscribe_url}}\">unsubscribe</a>".into(),
            status: CampaignStatus::Launched,
            created_at: timestamp,
            launched_at: Some(timestamp),
        };

        let document = to_document(&campaign)?;
        assert!(matches!(
            document.get("created_at"),
            Some(Bson::DateTime(_))
        ));
        assert!(matches!(
            document.get("launched_at"),
            Some(Bson::DateTime(_))
        ));
        assert_eq!(document.get_str("status")?, "launched");
        let decoded: Campaign = mongodb::bson::from_document(document)?;

        assert_eq!(decoded.status, CampaignStatus::Launched);
        assert_eq!(decoded.launched_at, Some(timestamp));
        Ok(())
    }

    #[test]
    fn contact_debug_output_redacts_the_unsubscribe_token() {
        let timestamp = Utc::now();
        let contact = Contact {
            id: None,
            email: "ada@example.com".into(),
            first_name: Some("Ada".into()),
            subscribed: true,
            unsubscribe_token: "opaque-unsubscribe-token".into(),
            created_at: timestamp,
            updated_at: timestamp,
        };

        let debug = format!("{contact:?}");

        assert!(debug.contains("ada@example.com"));
        assert!(!debug.contains("opaque-unsubscribe-token"));
    }
}
