# Email marketing automation implementation order

1. Configure MongoDB indexes and ClickHouse event storage.
2. Start the Axum API and connect TinyFlows webhook-triggered workflows.
3. Add Google OAuth state persistence and JWT authentication middleware.
4. Connect the Vite dashboard to authenticated campaign, contact, and analytics APIs.
5. Add a dedicated email delivery worker that filters unsubscribed contacts before provider delivery.
