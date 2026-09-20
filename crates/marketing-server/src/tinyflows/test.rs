//! Tests for `TinyFlows` webhook delivery.

use std::{
    io::{Error as IoError, ErrorKind, Read, Write},
    net::{TcpListener, TcpStream},
    thread,
};

use serde_json::{Value, json};

use crate::error::Error;

use super::TinyFlowsClient;

struct CapturedWebhook {
    request_line: String,
    body: Vec<u8>,
}

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;
type WebhookServer = (String, thread::JoinHandle<std::io::Result<CapturedWebhook>>);

fn webhook_server(status: u16) -> TestResult<WebhookServer> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let url = format!("http://{}", listener.local_addr()?);
    let server = thread::spawn(move || -> std::io::Result<CapturedWebhook> {
        let (mut stream, _) = listener.accept()?;
        let mut request = [0_u8; 4096];
        let request_len = stream.read(&mut request)?;
        let request = &request[..request_len];
        let header_end = request
            .windows(4)
            .position(|bytes| bytes == b"\r\n\r\n")
            .ok_or_else(|| IoError::new(ErrorKind::UnexpectedEof, "request has no headers"))?
            + 4;
        let (request_line, content_length) = {
            let headers = std::str::from_utf8(&request[..header_end]).map_err(IoError::other)?;
            let request_line = headers
                .lines()
                .next()
                .ok_or_else(|| IoError::other("webhook request includes a request line"))?
                .into();
            let content_length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then_some(value.trim())
                })
                .ok_or_else(|| IoError::other("webhook request includes a content length"))?
                .parse::<usize>()
                .map_err(IoError::other)?;
            (request_line, content_length)
        };
        stream.write_all(
            format!("HTTP/1.1 {status} Test\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )?;
        Ok(CapturedWebhook {
            request_line,
            body: request[header_end..header_end + content_length].to_vec(),
        })
    });
    Ok((url, server))
}

#[tokio::test]
async fn trigger_posts_the_expected_workflow_envelope() -> Result<(), Box<dyn std::error::Error>> {
    let (url, server) = webhook_server(204)?;
    let client = TinyFlowsClient::new(url);

    client
        .trigger("campaign_sent", &json!({ "campaign_id": "campaign-42" }))
        .await?;

    let request = server
        .join()
        .map_err(|_| IoError::other("test webhook thread panicked"))??;
    assert_eq!(request.request_line, "POST / HTTP/1.1");
    assert_eq!(
        serde_json::from_slice::<Value>(&request.body)?,
        json!({
            "workflow": "email-marketing",
            "event": "campaign_sent",
            "data": { "campaign_id": "campaign-42" },
        })
    );
    Ok(())
}

#[tokio::test]
async fn trigger_maps_rejected_webhooks_to_workflow_errors()
-> Result<(), Box<dyn std::error::Error>> {
    let (url, server) = webhook_server(500)?;
    let client = TinyFlowsClient::new(url);

    let result = client.trigger("campaign_sent", &json!({})).await;

    let _body = server
        .join()
        .map_err(|_| IoError::other("test webhook thread panicked"))??;
    assert!(matches!(result, Err(Error::Workflow(_))));
    Ok(())
}

#[test]
fn tinyflows_client_debug_output_redacts_the_webhook_url() {
    let client = TinyFlowsClient::new("https://secret@example.test/webhook".into());

    let debug = format!("{client:?}");

    assert!(debug.contains("TinyFlowsClient"));
    assert!(!debug.contains("secret"));
    assert!(!debug.contains("example.test"));
}

#[test]
fn webhook_harness_rejects_requests_without_a_content_length()
-> Result<(), Box<dyn std::error::Error>> {
    let (url, server) = webhook_server(204)?;
    let address = url.strip_prefix("http://").ok_or("loopback server URL")?;
    let mut stream = TcpStream::connect(address)?;
    stream.write_all(b"POST / HTTP/1.1\r\nHost: localhost\r\n\r\n")?;
    drop(stream);

    let result = server
        .join()
        .map_err(|_| IoError::other("test webhook thread panicked"))?;
    assert!(result.is_err());
    Ok(())
}
