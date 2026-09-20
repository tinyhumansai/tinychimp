//! HTTP response tests for service errors.

use axum::{body::to_bytes, response::IntoResponse};

use super::ServiceError;

async fn response_body(error: ServiceError) -> Result<(u16, String), Box<dyn std::error::Error>> {
    let response = error.into_response();
    let status = response.status().as_u16();
    let body = String::from_utf8(to_bytes(response.into_body(), usize::MAX).await?.to_vec())?;
    Ok((status, body))
}

#[tokio::test]
async fn validation_and_missing_records_return_safe_client_errors()
-> Result<(), Box<dyn std::error::Error>> {
    let (status, body) = response_body(ServiceError::Validation("bad input".into())).await?;
    assert_eq!(status, 400);
    assert_eq!(body, r#"{"error":"bad input"}"#);

    let (status, body) = response_body(ServiceError::NotFound("absent".into())).await?;
    assert_eq!(status, 404);
    assert_eq!(body, r#"{"error":"absent"}"#);
    Ok(())
}

#[tokio::test]
async fn infrastructure_errors_do_not_expose_internal_details()
-> Result<(), Box<dyn std::error::Error>> {
    let database = mongodb::error::Error::custom("database credential leaked");
    let (status, body) = response_body(ServiceError::from(database)).await?;
    assert_eq!(status, 500);
    assert_eq!(body, r#"{"error":"internal server error"}"#);

    let request_error = reqwest::Client::new()
        .get("http://127.0.0.1:1")
        .send()
        .await
        .err()
        .ok_or_else(|| std::io::Error::other("closed local port rejects the request"))?;
    let (status, body) = response_body(ServiceError::from(request_error)).await?;
    assert_eq!(status, 502);
    assert_eq!(body, r#"{"error":"workflow delivery failed"}"#);
    Ok(())
}
