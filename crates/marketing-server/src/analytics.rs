//! ClickHouse-backed immutable engagement analytics.

use std::fmt;

use chrono::{DateTime, Utc};
use clickhouse::{Client, Row};
use serde::Serialize;

use crate::error::Result;

/// An event written to the `ClickHouse` `campaign_events` table.
#[derive(Clone, Debug, Row, Serialize)]
pub struct CampaignEvent {
    /// Event kind, such as `contact_unsubscribed`.
    pub event_name: String,
    /// Associated campaign identifier when available.
    pub campaign_id: String,
    /// Associated contact identifier when available.
    pub contact_id: String,
    /// Event timestamp in UTC.
    #[serde(with = "clickhouse::serde::chrono::datetime64::millis")]
    pub occurred_at: DateTime<Utc>,
}

/// Appends engagement events to `ClickHouse`.
#[derive(Clone)]
pub struct AnalyticsWriter {
    client: Client,
}

impl AnalyticsWriter {
    /// Builds a writer for a `ClickHouse` database.
    #[must_use]
    pub fn new(url: &str, database: &str) -> Self {
        Self {
            client: Client::default().with_url(url).with_database(database),
        }
    }

    /// Records an immutable campaign event.
    ///
    /// The database must provide a `campaign_events` table matching
    /// [`CampaignEvent`].
    ///
    /// # Errors
    ///
    /// Returns an error if `ClickHouse` cannot accept the event.
    pub async fn record(&self, event: &CampaignEvent) -> Result<()> {
        let mut insert = self
            .client
            .insert("campaign_events")
            .map_err(|error| crate::error::Error::Validation(error.to_string()))?;
        insert
            .write(event)
            .await
            .map_err(|error| crate::error::Error::Validation(error.to_string()))?;
        insert
            .end()
            .await
            .map_err(|error| crate::error::Error::Validation(error.to_string()))?;
        Ok(())
    }
}

impl fmt::Debug for AnalyticsWriter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AnalyticsWriter")
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod test;
