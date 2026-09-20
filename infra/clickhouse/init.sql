CREATE TABLE IF NOT EXISTS campaign_events (
    event_name String,
    campaign_id String,
    contact_id String,
    occurred_at DateTime64(3, 'UTC')
) ENGINE = MergeTree
ORDER BY (event_name, occurred_at);
