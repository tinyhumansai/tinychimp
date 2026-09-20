//! `TinyFlows` webhook integration.

use serde::Serialize;
use std::fmt;

use crate::error::Result;

/// Sends marketing lifecycle events to a `TinyFlows` webhook-triggered flow.
#[derive(Clone)]
pub struct TinyFlowsClient {
    webhook_url: String,
    http: reqwest::Client,
}

impl fmt::Debug for TinyFlowsClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TinyFlowsClient")
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Serialize)]
struct WebhookEnvelope<'a, T: Serialize> {
    workflow: &'a str,
    event: &'a str,
    data: &'a T,
}

impl TinyFlowsClient {
    /// Creates a webhook client for one `TinyFlows` ingress endpoint.
    #[must_use]
    pub fn new(webhook_url: String) -> Self {
        Self {
            webhook_url,
            http: reqwest::Client::new(),
        }
    }

    /// Delivers a named event to the `email-marketing` `TinyFlows` workflow.
    ///
    /// # Errors
    ///
    /// Returns an error when `TinyFlows` rejects or cannot receive the event.
    pub async fn trigger<T: Serialize>(&self, event: &str, data: &T) -> Result<()> {
        self.http
            .post(&self.webhook_url)
            .json(&WebhookEnvelope {
                workflow: "email-marketing",
                event,
                data,
            })
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }
}

#[cfg(test)]
mod test;
