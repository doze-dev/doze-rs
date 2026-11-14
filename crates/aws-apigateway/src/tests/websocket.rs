//! A WebSocket API through a real gateway on a socket: the handshake, a routed message answered by a MOCK integration with a route
//! response, a post from the `@connections` API, reading the connection, and closing it from the server side.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::sync::Arc;

use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use super::World;

/// A masked client text frame.
fn text_frame(s: &str) -> Vec<u8> {
    let mask = [7u8, 1, 9, 3];
    let mut out = vec![0x81, 0x80 | u8::try_from(s.len()).unwrap()];
    out.extend_from_slice(&mask);
    out.extend(s.bytes().enumerate().map(|(i, b)| b ^ mask[i % 4]));
    out
}

/// One unmasked server frame: its opcode and payload.
async fn read_frame(s: &mut TcpStream) -> (u8, String) {
    let mut head = [0u8; 2];
    s.read_exact(&mut head).await.unwrap();
    let len = usize::from(head[1] & 0x7f);
    assert!(len < 126, "short frames only in this test");
    let mut payload = vec![0u8; len];
    s.read_exact(&mut payload).await.unwrap();
    (
        head[0] & 0x0f,
        String::from_utf8_lossy(&payload).into_owned(),
    )
}

async fn http(addr: std::net::SocketAddr, method: &str, path: &str, body: &str) -> (u16, String) {
    let mut s = TcpStream::connect(addr).await.unwrap();
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes()).await.unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).await.unwrap();
    let status = out.split(' ').nth(1).unwrap().parse().unwrap();
    (
        status,
        out.split("\r\n\r\n").nth(1).unwrap_or_default().to_owned(),
    )
}

#[tokio::test]
async fn a_websocket_api_connects_routes_answers_and_is_managed() {
    let w = World::new();
    let api = w
        .http("CreateApi", json!({"name": "ws", "protocolType": "WEBSOCKET", "routeSelectionExpression": "$request.body.action"}))
        .await
        .unwrap();
    let id = api["apiId"].as_str().unwrap().to_owned();
    assert!(api["apiEndpoint"].as_str().unwrap().starts_with("ws://"));
    let integ = w
        .http(
            "CreateIntegration",
            json!({"ApiId": id, "integrationType": "MOCK"}),
        )
        .await
        .unwrap();
    let iid = integ["integrationId"].as_str().unwrap().to_owned();
    w.http(
        "CreateIntegrationResponse",
        json!({"ApiId": id, "IntegrationId": iid, "integrationResponseKey": "$default",
        "responseTemplates": {"$default": "echo:$input.path('$.x'):$context.connectionId"}}),
    )
    .await
    .unwrap();
    let route = w
        .http("CreateRoute", json!({"ApiId": id, "routeKey": "echo", "target": format!("integrations/{iid}"), "routeResponseSelectionExpression": "$default"}))
        .await
        .unwrap();
    w.http(
        "CreateRouteResponse",
        json!({"ApiId": id, "RouteId": route["routeId"], "routeResponseKey": "$default"}),
    )
    .await
    .unwrap();
    w.http(
        "CreateStage",
        json!({"ApiId": id, "stageName": "prod", "autoDeploy": true}),
    )
    .await
    .unwrap();

    let gateway =
        aws_gateway::Gateway::new(aws_gateway::Config::new("127.0.0.1:0".parse().unwrap()))
            .with(Arc::new(w.v1.clone()))
            .with(Arc::new(w.v2.clone()))
            .with(Arc::new(w.v1.management()));
    let running = gateway.bind().await.unwrap();
    let addr = running.local_addr();

    let mut ws = TcpStream::connect(addr).await.unwrap();
    let hello = format!(
        "GET /_aws/execute-api/{id}/prod HTTP/1.1\r\nHost: {addr}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n"
    );
    ws.write_all(hello.as_bytes()).await.unwrap();
    let mut head = Vec::new();
    while !head.ends_with(b"\r\n\r\n") {
        let mut b = [0u8; 1];
        ws.read_exact(&mut b).await.unwrap();
        head.push(b[0]);
    }
    let head = String::from_utf8(head).unwrap();
    assert!(head.starts_with("HTTP/1.1 101"), "{head}");
    assert!(
        head.to_ascii_lowercase()
            .contains("sec-websocket-accept: s3pplmbitxaq9kygzzhzrbk+xoo="),
        "{head}"
    );

    ws.write_all(&text_frame(r#"{"action": "echo", "x": 7}"#))
        .await
        .unwrap();
    let (op, answer) = read_frame(&mut ws).await;
    assert_eq!(op, 1);
    let conn = answer.strip_prefix("echo:7:").unwrap().to_owned();
    // A route that is not there, and no `$default`: AWS's Forbidden message.
    ws.write_all(&text_frame(r#"{"action": "nope"}"#))
        .await
        .unwrap();
    let (_, forbidden) = read_frame(&mut ws).await;
    assert!(forbidden.contains("\"Forbidden\""), "{forbidden}");

    let at = format!(
        "/_aws/execute-api/{id}/prod/@connections/{}",
        conn.replace('=', "%3D")
            .replace('/', "%2F")
            .replace('+', "%2B")
    );
    assert_eq!(http(addr, "POST", &at, "pushed").await.0, 200);
    assert_eq!(read_frame(&mut ws).await, (1, "pushed".to_owned()));
    let (status, info) = http(addr, "GET", &at, "").await;
    assert_eq!(status, 200);
    assert!(info.contains("connectedAt"), "{info}");
    assert_eq!(http(addr, "DELETE", &at, "").await.0, 204);
    assert_eq!(read_frame(&mut ws).await.0, 8);
    // Once the session has ended the connection is gone.
    let mut gone = 0;
    for _ in 0..50 {
        gone = http(addr, "POST", &at, "late").await.0;
        if gone == 410 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(gone, 410);
    running.shutdown().await;
    w.tasks.shutdown().await;
}
