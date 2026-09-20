//! Tests for immutable campaign analytics payloads.

use chrono::{DateTime, Utc};
use serde_json::json;

use super::{AnalyticsWriter, CampaignEvent};

#[test]
fn campaign_event_serializes_the_clickhouse_column_names() -> Result<(), Box<dyn std::error::Error>>
{
    let occurred_at = DateTime::parse_from_rfc3339("2025-02-03T04:05:06Z")?.with_timezone(&Utc);
    let event = CampaignEvent {
        event_name: "email_opened".into(),
        campaign_id: "campaign-42".into(),
        contact_id: "contact-7".into(),
        occurred_at,
    };

    assert_eq!(
        serde_json::to_value(event)?,
        json!({
            "event_name": "email_opened",
            "campaign_id": "campaign-42",
            "contact_id": "contact-7",
            "occurred_at": occurred_at.timestamp_millis(),
        })
    );
    Ok(())
}

#[test]
fn analytics_writer_debug_output_redacts_connection_details() {
    let writer = AnalyticsWriter::new(
        "https://analytics-user:secret@clickhouse.example.test",
        "private_campaign_data",
    );

    let debug = format!("{writer:?}");

    assert!(debug.contains("AnalyticsWriter"));
    assert!(!debug.contains("analytics-user"));
    assert!(!debug.contains("secret"));
    assert!(!debug.contains("private_campaign_data"));
}

#[tokio::test]
async fn record_returns_a_validation_error_when_clickhouse_is_unreachable() {
    let writer = AnalyticsWriter::new("http://127.0.0.1:1", "analytics");
    let event = CampaignEvent {
        event_name: "email_opened".into(),
        campaign_id: "campaign-42".into(),
        contact_id: "contact-7".into(),
        occurred_at: Utc::now(),
    };

    assert!(writer.record(&event).await.is_err());
}
