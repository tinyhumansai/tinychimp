//! MongoDB persistence and index management.

use chrono::Utc;
use mongodb::{
    Collection, Database, IndexModel,
    bson::{Bson, doc, oid::ObjectId},
    options::IndexOptions,
};

use crate::{
    error::{Error, Result},
    models::{Campaign, CampaignStatus, Contact, CreateCampaign, CreateContact, LaunchOutcome},
};
use uuid::Uuid;

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
        let mut contact = new_contact(input, Utc::now())?;
        let insert_result = self.contacts.insert_one(&contact).await?;
        let inserted = inserted_object_id(&insert_result.inserted_id)?;
        contact.id = Some(inserted);
        Ok(contact)
    }

    /// Inserts a campaign in draft state.
    ///
    /// # Errors
    ///
    /// Returns validation or database errors.
    pub async fn create_campaign(&self, input: CreateCampaign) -> Result<Campaign> {
        let mut campaign = new_campaign(input, Utc::now())?;
        let insert_result = self.campaigns.insert_one(&campaign).await?;
        let inserted = inserted_object_id(&insert_result.inserted_id)?;
        campaign.id = Some(inserted);
        Ok(campaign)
    }

    /// Atomically claims a draft campaign for launch.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the campaign is not a draft, or a
    /// not-found or database error.
    pub async fn launch_campaign(&self, id: &str) -> Result<LaunchOutcome> {
        let id = campaign_object_id(id)?;
        if let Some(campaign) = self
            .campaigns
            .find_one_and_update(
                doc! { "_id": id, "status": "draft" },
                doc! { "$set": { "status": "launching" } },
            )
            .return_document(mongodb::options::ReturnDocument::After)
            .await?
        {
            return Ok(LaunchOutcome {
                campaign,
                newly_launched: true,
            });
        }

        let campaign = self
            .campaigns
            .find_one(doc! { "_id": id })
            .await?
            .ok_or_else(|| Error::NotFound("campaign not found".into()))?;
        Err(Error::Validation(format!(
            "campaign cannot be launched from {} state",
            campaign_status_name(&campaign.status)
        )))
    }

    /// Marks an already claimed campaign as launched after its handoff succeeds.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the campaign is not being launched, or
    /// a not-found or database error.
    pub async fn complete_campaign_launch(&self, id: &str) -> Result<Campaign> {
        let id = campaign_object_id(id)?;
        if let Some(campaign) = self
            .campaigns
            .find_one_and_update(
                doc! { "_id": id, "status": "launching" },
                doc! { "$set": { "status": "launched", "launched_at": mongodb::bson::DateTime::from_millis(Utc::now().timestamp_millis()) } },
            )
            .return_document(mongodb::options::ReturnDocument::After)
            .await?
        {
            return Ok(campaign);
        }

        let campaign = self
            .campaigns
            .find_one(doc! { "_id": id })
            .await?
            .ok_or_else(|| Error::NotFound("campaign not found".into()))?;
        Err(Error::Validation(format!(
            "campaign cannot complete launch from {} state",
            campaign_status_name(&campaign.status)
        )))
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

fn inserted_object_id(inserted_id: &Bson) -> Result<ObjectId> {
    inserted_id.as_object_id().ok_or_else(|| {
        Error::Database(mongodb::error::Error::custom(
            "MongoDB did not return an object id",
        ))
    })
}

fn campaign_object_id(id: &str) -> Result<ObjectId> {
    ObjectId::parse_str(id).map_err(|_| Error::Validation("campaign id is invalid".into()))
}

fn campaign_status_name(status: &CampaignStatus) -> &'static str {
    match status {
        CampaignStatus::Draft => "draft",
        CampaignStatus::Launching => "launching",
        CampaignStatus::Launched => "launched",
    }
}

fn new_contact(input: CreateContact, now: chrono::DateTime<Utc>) -> Result<Contact> {
    Ok(Contact {
        id: None,
        email: normalize_email(&input.email)?,
        first_name: input.first_name.filter(|name| !name.trim().is_empty()),
        subscribed: true,
        unsubscribe_token: Uuid::new_v4().simple().to_string(),
        created_at: now,
        updated_at: now,
    })
}

fn normalize_email(email: &str) -> Result<String> {
    if email.is_empty() || email.len() > 254 || email.chars().any(char::is_whitespace) {
        return Err(Error::Validation("email address is invalid".into()));
    }
    let (local, domain) = email
        .split_once('@')
        .filter(|(_, remainder)| !remainder.contains('@'))
        .ok_or_else(|| Error::Validation("email address is invalid".into()))?;
    let valid_local = !local.is_empty()
        && local.len() <= 64
        && !local.starts_with('.')
        && !local.ends_with('.')
        && !local.contains("..")
        && local
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b".!#$%&'*+-/=?^_`{|}~".contains(&byte));
    let valid_domain = domain.contains('.')
        && domain.len() <= 253
        && domain.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        });
    if !(valid_local && valid_domain) {
        return Err(Error::Validation("email address is invalid".into()));
    }
    Ok(email.to_lowercase())
}

fn new_campaign(input: CreateCampaign, now: chrono::DateTime<Utc>) -> Result<Campaign> {
    if input.name.trim().is_empty() || input.subject.trim().is_empty() {
        return Err(Error::Validation(
            "campaign name and subject are required".into(),
        ));
    }
    if input
        .html_body
        .match_indices(UNSUBSCRIBE_PLACEHOLDER)
        .count()
        != 1
    {
        return Err(Error::Validation(format!(
            "campaign HTML must include exactly one {UNSUBSCRIBE_PLACEHOLDER}"
        )));
    }
    Ok(Campaign {
        id: None,
        name: input.name,
        subject: input.subject,
        html_body: input.html_body,
        status: CampaignStatus::Draft,
        created_at: now,
        launched_at: None,
    })
}

#[cfg(test)]
mod test {
    //! Database-free tests for repository input and state construction.

    use chrono::{TimeZone, Utc};

    use mongodb::{
        Client,
        bson::{Bson, oid::ObjectId},
    };
    use testcontainers::{
        GenericImage,
        core::{IntoContainerPort, WaitFor},
        runners::AsyncRunner,
    };

    use super::{
        MarketingRepository, UNSUBSCRIBE_PLACEHOLDER, campaign_status_name, inserted_object_id,
        new_campaign, new_contact, normalize_email,
    };
    use crate::{
        Error,
        models::{CampaignStatus, CreateCampaign, CreateContact},
    };

    fn timestamp() -> Result<chrono::DateTime<Utc>, std::io::Error> {
        Utc.with_ymd_and_hms(2026, 9, 20, 12, 34, 56)
            .single()
            .ok_or_else(|| std::io::Error::other("fixed test timestamp is valid"))
    }

    #[test]
    fn normalizes_a_valid_email_address() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(
            normalize_email("Ada.Smith+news@Example.COM")?,
            "ada.smith+news@example.com"
        );
        Ok(())
    }

    #[test]
    fn rejects_invalid_email_addresses() {
        for email in [
            "",
            " ada@example.com",
            "ada @example.com",
            "ada@@example.com",
            ".ada@example.com",
            "ada@example",
            "ada@-example.com",
            "ada@example..com",
        ] {
            assert!(
                matches!(normalize_email(email), Err(Error::Validation(_))),
                "{email} should be rejected"
            );
        }
    }

    #[test]
    fn creates_contact_with_an_opaque_random_token() -> Result<(), Box<dyn std::error::Error>> {
        let contact = new_contact(
            CreateContact {
                email: "ada@example.com".into(),
                first_name: Some("  ".into()),
            },
            timestamp()?,
        )?;

        assert_eq!(contact.first_name, None);
        assert_eq!(contact.unsubscribe_token.len(), 32);
        assert!(
            contact
                .unsubscribe_token
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        );
        assert_ne!(
            contact.unsubscribe_token,
            new_contact(
                CreateContact {
                    email: "grace@example.com".into(),
                    first_name: None
                },
                timestamp()?
            )?
            .unsubscribe_token
        );
        Ok(())
    }

    #[test]
    fn creates_a_draft_with_exactly_one_unsubscribe_placeholder()
    -> Result<(), Box<dyn std::error::Error>> {
        let campaign = new_campaign(
            CreateCampaign {
                name: "September".into(),
                subject: "Update".into(),
                html_body: format!("<a href=\"{UNSUBSCRIBE_PLACEHOLDER}\">unsubscribe</a>"),
            },
            timestamp()?,
        )?;

        assert_eq!(campaign.status, CampaignStatus::Draft);
        assert_eq!(campaign.created_at, timestamp()?);
        assert_eq!(campaign.launched_at, None);
        Ok(())
    }

    #[test]
    fn rejects_missing_or_repeated_unsubscribe_placeholders()
    -> Result<(), Box<dyn std::error::Error>> {
        for html_body in [
            "<p>No link</p>".into(),
            format!("{UNSUBSCRIBE_PLACEHOLDER}{UNSUBSCRIBE_PLACEHOLDER}"),
        ] {
            let result = new_campaign(
                CreateCampaign {
                    name: "September".into(),
                    subject: "Update".into(),
                    html_body,
                },
                timestamp()?,
            );
            assert!(matches!(result, Err(Error::Validation(_))));
        }
        Ok(())
    }

    #[test]
    fn rejects_a_non_object_id_insert_result() {
        assert!(matches!(
            inserted_object_id(&Bson::Null),
            Err(Error::Database(_))
        ));
    }

    #[test]
    fn names_every_campaign_lifecycle_state() {
        assert_eq!(campaign_status_name(&CampaignStatus::Draft), "draft");
        assert_eq!(
            campaign_status_name(&CampaignStatus::Launching),
            "launching"
        );
        assert_eq!(campaign_status_name(&CampaignStatus::Launched), "launched");
    }

    #[tokio::test]
    async fn persists_contacts_campaigns_and_suppression_state()
    -> Result<(), Box<dyn std::error::Error>> {
        let container = GenericImage::new("mongo", "8.0.0")
            .with_exposed_port(27017.tcp())
            .with_wait_for(WaitFor::message_on_either_std("Waiting for connections"))
            .start()
            .await?;
        let port = container.get_host_port_ipv4(27017.tcp()).await?;
        let database = Client::with_uri_str(format!("mongodb://127.0.0.1:{port}"))
            .await?
            .database("repository_contract");
        let repository = MarketingRepository::new(database).await?;

        let contact = repository
            .create_contact(CreateContact {
                email: "Ada@example.com".into(),
                first_name: Some("Ada".into()),
            })
            .await?;
        assert!(contact.id.is_some());
        assert_eq!(contact.email, "ada@example.com");
        assert_eq!(contact.first_name.as_deref(), Some("Ada"));
        assert!(contact.subscribed);

        let duplicate = repository
            .create_contact(CreateContact {
                email: "ada@example.com".into(),
                first_name: None,
            })
            .await;
        assert!(matches!(duplicate, Err(Error::Database(_))));

        let campaign = repository
            .create_campaign(CreateCampaign {
                name: "September update".into(),
                subject: "News".into(),
                html_body: format!("<a href=\"{UNSUBSCRIBE_PLACEHOLDER}\">Unsubscribe</a>"),
            })
            .await?;
        let campaign_id = campaign.id.ok_or("campaign id is assigned")?.to_hex();
        assert_eq!(campaign.status, CampaignStatus::Draft);

        assert!(matches!(
            repository.launch_campaign("not-an-id").await,
            Err(Error::Validation(_))
        ));
        assert!(matches!(
            repository.complete_campaign_launch("not-an-id").await,
            Err(Error::Validation(_))
        ));
        assert!(matches!(
            repository.launch_campaign(&ObjectId::new().to_hex()).await,
            Err(Error::NotFound(_))
        ));

        let launching = repository.launch_campaign(&campaign_id).await?;
        assert!(launching.newly_launched);
        assert_eq!(launching.campaign.status, CampaignStatus::Launching);
        assert_eq!(launching.campaign.launched_at, None);

        assert!(matches!(
            repository.launch_campaign(&campaign_id).await,
            Err(Error::Validation(message)) if message == "campaign cannot be launched from launching state"
        ));

        let launched = repository.complete_campaign_launch(&campaign_id).await?;
        assert_eq!(launched.status, CampaignStatus::Launched);
        assert!(launched.launched_at.is_some());
        assert!(matches!(
            repository.complete_campaign_launch(&campaign_id).await,
            Err(Error::Validation(message)) if message == "campaign cannot complete launch from launched state"
        ));
        assert!(matches!(
            repository.launch_campaign(&campaign_id).await,
            Err(Error::Validation(message)) if message == "campaign cannot be launched from launched state"
        ));

        let unsubscribed = repository.unsubscribe(&contact.unsubscribe_token).await?;
        assert!(!unsubscribed.subscribed);
        assert!(matches!(
            repository.unsubscribe("unknown-token").await,
            Err(Error::NotFound(_))
        ));

        Ok(())
    }
}
