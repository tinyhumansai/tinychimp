//! MongoDB persistence and index management.

use chrono::Utc;
use mongodb::{
    Collection, Database, IndexModel,
    bson::{doc, oid::ObjectId},
    options::IndexOptions,
};

use crate::{
    error::{Error, Result},
    models::{Campaign, CampaignStatus, Contact, CreateCampaign, CreateContact},
};

const UNSUBSCRIBE_PLACEHOLDER: &str = "{{unsubscribe_url}}";

/// MongoDB repository for contacts and campaigns.
#[derive(Clone, Debug)]
pub struct MarketingRepository {
    contacts: Collection<Contact>,
    campaigns: Collection<Campaign>,
}

impl MarketingRepository {
    /// Creates a repository and its required unique indexes.
    ///
    /// # Errors
    ///
    /// Returns an error if MongoDB cannot create an index.
    pub async fn new(database: Database) -> Result<Self> {
        let contacts = database.collection("contacts");
        let campaigns = database.collection("campaigns");
        contacts
            .create_index(
                IndexModel::builder()
                    .keys(doc! { "email": 1 })
                    .options(IndexOptions::builder().unique(true).build())
                    .build(),
            )
            .await?;
        contacts
            .create_index(
                IndexModel::builder()
                    .keys(doc! { "unsubscribe_token": 1 })
                    .options(IndexOptions::builder().unique(true).build())
                    .build(),
            )
            .await?;
        Ok(Self {
            contacts,
            campaigns,
        })
    }

    /// Inserts a subscribed contact.
    ///
    /// # Errors
    ///
    /// Returns validation or database errors.
    pub async fn create_contact(&self, input: CreateContact) -> Result<Contact> {
        let email = input.email.trim().to_lowercase();
        let mut parts = email.split('@');
        if email.chars().any(char::is_whitespace)
            || parts.next().is_none_or(str::is_empty)
            || parts.next().is_none_or(str::is_empty)
            || parts.next().is_some()
        {
            return Err(Error::Validation("email address is invalid".into()));
        }
        let now = Utc::now();
        let mut contact = Contact {
            id: None,
            email,
            first_name: input.first_name.filter(|name| !name.trim().is_empty()),
            subscribed: true,
            unsubscribe_token: ObjectId::new().to_hex(),
            created_at: now,
            updated_at: now,
        };
        let inserted = self
            .contacts
            .insert_one(&contact)
            .await?
            .inserted_id
            .as_object_id()
            .ok_or_else(|| {
                Error::Database(mongodb::error::Error::custom(
                    "MongoDB did not return an object id",
                ))
            })?;
        contact.id = Some(inserted);
        Ok(contact)
    }

    /// Inserts a campaign in draft state.
    ///
    /// # Errors
    ///
    /// Returns validation or database errors.
    pub async fn create_campaign(&self, input: CreateCampaign) -> Result<Campaign> {
        if input.name.trim().is_empty() || input.subject.trim().is_empty() {
            return Err(Error::Validation(
                "campaign name and subject are required".into(),
            ));
        }
        if !input.html_body.contains(UNSUBSCRIBE_PLACEHOLDER) {
            return Err(Error::Validation(format!(
                "campaign HTML must include {UNSUBSCRIBE_PLACEHOLDER}"
            )));
        }
        let mut campaign = Campaign {
            id: None,
            name: input.name,
            subject: input.subject,
            html_body: input.html_body,
            status: CampaignStatus::Draft,
            created_at: Utc::now(),
            launched_at: None,
        };
        let inserted = self
            .campaigns
            .insert_one(&campaign)
            .await?
            .inserted_id
            .as_object_id()
            .ok_or_else(|| {
                Error::Database(mongodb::error::Error::custom(
                    "MongoDB did not return an object id",
                ))
            })?;
        campaign.id = Some(inserted);
        Ok(campaign)
    }

    /// Marks a campaign as launched and returns it.
    ///
    /// # Errors
    ///
    /// Returns not-found or database errors.
    pub async fn launch_campaign(&self, id: &str) -> Result<Campaign> {
        let id = ObjectId::parse_str(id)
            .map_err(|_| Error::Validation("campaign id is invalid".into()))?;
        let now = Utc::now();
        let campaign = self.campaigns.find_one_and_update(doc! { "_id": id }, doc! { "$set": { "status": "launching", "launched_at": mongodb::bson::DateTime::from_millis(now.timestamp_millis()) } }).return_document(mongodb::options::ReturnDocument::After).await?;
        campaign.ok_or_else(|| Error::NotFound("campaign not found".into()))
    }

    /// Removes a contact from marketing delivery by token.
    ///
    /// # Errors
    ///
    /// Returns not-found or database errors.
    pub async fn unsubscribe(&self, token: &str) -> Result<Contact> {
        let contact = self.contacts.find_one_and_update(doc! { "unsubscribe_token": token }, doc! { "$set": { "subscribed": false, "updated_at": mongodb::bson::DateTime::from_millis(Utc::now().timestamp_millis()) } }).return_document(mongodb::options::ReturnDocument::After).await?;
        contact.ok_or_else(|| Error::NotFound("unsubscribe link is invalid".into()))
    }
}
