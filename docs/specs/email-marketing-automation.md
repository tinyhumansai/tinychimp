# Email marketing automation

The service manages contacts and campaign drafts in MongoDB. It records immutable lifecycle events in ClickHouse and sends the same lifecycle events to a TinyFlows webhook, where delivery and branching automation run.

The public unsubscribe URL contains an opaque per-contact token. Submitting it makes the contact unsubscribed before analytics and flow notifications happen; email senders must query `subscribed = true` before delivery.

Dashboard sign-in begins with Google OAuth and exchanges a successful authorization code for a 24-hour JWT. The deployment must persist and validate a high-entropy OAuth state value per browser session before exposing the callback.
