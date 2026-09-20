# Chimpboard

Email marketing automation scaffolding inspired by the operational shape of
Mautic, Mailchimp, and Loops: contacts and campaigns in MongoDB, immutable
analytics in ClickHouse, and lifecycle automation handed off to TinyFlows.

## Run locally

Copy `.env.example` to `.env`, fill in infrastructure and Google OAuth values,
then start the API with `cargo run -p marketing-server`. Initialize ClickHouse
with `infra/clickhouse/init.sql`, and start the dashboard with `cd dashboard &&
npm install && npm run dev`.

The API provides `POST /api/contacts`, `POST /api/campaigns`,
`POST /api/campaigns/{id}/launch`, and the public `GET|POST /unsubscribe/{token}`
page. Point `TINYFLOWS_WEBHOOK_URL` at a TinyFlows webhook trigger; it receives
an `email-marketing` workflow envelope and lifecycle event data.

## Architecture

- `crates/marketing-server` is the Axum service. MongoDB owns contact and
  campaign state; ClickHouse owns append-only analytics.
- `dashboard/` is a small React + Vite operator console.
- `infra/clickhouse/init.sql` creates the analytics event table.
- `docs/specs/email-marketing-automation.md` records the security and delivery
  boundary; the delivery implementation belongs in a TinyFlows flow.

The Google callback currently returns the signed JWT as JSON so the dashboard
can choose its own token storage policy. Before public deployment, add a
server-side, short-lived OAuth state store and JWT middleware to protect every
operator-only route.

## Validation

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo build --all-targets --all-features
cargo test --all-features
cd dashboard && npm run build
```
