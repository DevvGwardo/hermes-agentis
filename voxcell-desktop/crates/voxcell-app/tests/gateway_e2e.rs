//! P4-04 integration E2E: voxcell-hermes client against an in-process mock
//! gateway (loopback TCP; spec §9.2). Covers runs, session_stream, approval
//! replay, and multi-profile `/p/<slug>/` routing. No live network, no keys.
//!
//! NOTE: the axum-based mock lives in `voxcell-uphill::mock_gateway` (axum is
//! not a voxcell-app dependency; app/Cargo.toml is owned by theme-chrome).
//! This mock speaks the same routes over tokio TcpListener so the E2E runs
//! with voxcell-app's existing dependency set.

use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use voxcell_core::GatewayTransport;
use voxcell_hermes::client::{GatewayStreamEvent, HermesClient};
use voxcell_hermes::protocol::{ProfileGatewayConfig, StreamSignal};

#[derive(Clone, Debug)]
struct Recorded {
    method: String,
    path: String,
    profile: Option<String>,
    body: serde_json::Value,
}

#[derive(Clone, Default)]
struct MockState {
    requests: Arc<Mutex<Vec<Recorded>>>,
    approvals: Arc<Mutex<Vec<(String, String)>>>,
    runs: Arc<Mutex<u32>>,
}

fn http_json(body: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

fn http_sse(events: &[serde_json::Value]) -> String {
    let mut body = String::from(": keepalive\n\n");
    for ev in events {
        body.push_str(&format!("data: {}\n\n", serde_json::to_string(ev).unwrap()));
    }
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

fn run_events(st: &MockState) -> Vec<serde_json::Value> {
    use serde_json::json;
    let mut events = vec![json!({"event": "tool.started", "tool": "terminal", "preview": "cargo test"})];
    let approvals = st.approvals.lock().unwrap().clone();
    if approvals.is_empty() {
        events.push(json!({
            "event": "approval.request", "request_id": "req-1", "tool": "terminal",
            "command": "rm -rf target/", "choices": ["once", "always", "deny"], "execution_generation": 0,
        }));
    } else {
        for (req, decision) in &approvals {
            events.push(json!({"event": "message.delta", "delta": format!("[approval {req} replayed: {decision}]")}));
        }
    }
    events.push(json!({"event": "message.delta", "delta": "All tests green."}));
    events.push(json!({"event": "run.completed", "output": "All tests green."}));
    events
}

async fn serve_one(mut sock: tokio::net::TcpStream, st: MockState) {
    // Read headers, then the Content-Length body.
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    let header_end = loop {
        let Ok(n) = sock.read(&mut tmp).await else { return };
        if n == 0 {
            return;
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos + 4;
        }
    };
    let headers = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let first_line = headers.lines().next().unwrap_or("").to_string();
    let mut parts = first_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let full_path = parts.next().unwrap_or("").to_string();
    let content_length: usize = headers
        .lines()
        .find_map(|l| {
            let (k, v) = l.split_once(':')?;
            if k.trim().eq_ignore_ascii_case("content-length") { v.trim().parse().ok() } else { None }
        })
        .unwrap_or(0);
    while buf.len() < header_end + content_length {
        let Ok(n) = sock.read(&mut tmp).await else { return };
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
    }
    let body_raw = &buf[header_end..(header_end + content_length).min(buf.len())];
    let body: serde_json::Value = serde_json::from_slice(body_raw).unwrap_or(serde_json::Value::Null);

    let (profile, path) = if let Some(rest) = full_path.strip_prefix("/p/") {
        match rest.split_once('/') {
            Some((slug, tail)) => (Some(slug.to_string()), format!("/{tail}")),
            None => (Some(rest.to_string()), "/".to_string()),
        }
    } else {
        (None, full_path.clone())
    };
    st.requests.lock().unwrap().push(Recorded { method: method.clone(), path: full_path, profile, body: body.clone() });

    let resp = match (method.as_str(), path.as_str()) {
        ("GET", "/health") => http_json(r#"{"status":"ok","version":"hermes-mock"}"#),
        ("GET", "/v1/capabilities") => http_json(
            r#"{"object":"capabilities","platform":"mock","model":"hermes-agent","features":{"run_submission":true,"run_events_sse":true,"run_approval":true,"session_chat_stream":true,"chat_completions":true}}"#,
        ),
        ("GET", "/v1/models") => http_json(r#"{"data":[{"id":"hermes-agent"}]}"#),
        ("POST", "/v1/runs") => {
            let mut n = st.runs.lock().unwrap();
            *n += 1;
            http_json(&format!(r#"{{"run_id":"r-{n}"}}"#))
        }
        ("GET", p) if p.starts_with("/v1/runs/") && p.ends_with("/events") => http_sse(&run_events(&st)),
        ("POST", p) if p.starts_with("/v1/runs/") && p.ends_with("/approval") => {
            if let Some(req) = body.get("request_id").and_then(|v| v.as_str()) {
                let decision = body.get("decision").and_then(|v| v.as_str()).unwrap_or("once");
                st.approvals.lock().unwrap().push((req.to_string(), decision.to_string()));
            }
            http_json(r#"{"ok":true}"#)
        }
        ("POST", p) if p.starts_with("/api/sessions/") && p.ends_with("/chat/stream") => http_sse(&run_events(&st)),
        _ => http_json(r#"{"ok":true,"mock":"fallback"}"#),
    };
    let _ = sock.write_all(resp.as_bytes()).await;
    let _ = sock.shutdown().await;
}

async fn spawn_mock() -> (String, MockState) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let st = MockState::default();
    let task_st = st.clone();
    tokio::spawn(async move {
        loop {
            let Ok((sock, _)) = listener.accept().await else { break };
            let st = task_st.clone();
            tokio::spawn(async move { serve_one(sock, st).await });
        }
    });
    (format!("http://{addr}"), st)
}

fn client(base_url: &str) -> Arc<HermesClient> {
    Arc::new(HermesClient::new(ProfileGatewayConfig {
        base_url: base_url.to_string(),
        profile: "default".into(),
        api_key: String::new(),
        route_prefix: String::new(),
    }))
}

async fn turn(c: Arc<HermesClient>, transport: GatewayTransport, session: &str) -> Vec<GatewayStreamEvent> {
    let (tx, mut rx) = mpsc::unbounded_channel();
    c.stream_turn(
        transport, "ch-e2e".into(), session.into(), format!("voxcell-{session}"),
        "run the uphill suite".into(), None, tx, || {},
    )
    .await;
    let mut events = Vec::new();
    while let Some(ev) = rx.recv().await {
        let done = matches!(ev, GatewayStreamEvent::Completed { .. } | GatewayStreamEvent::Error { .. });
        events.push(ev);
        if done {
            break;
        }
    }
    events
}

#[tokio::test]
async fn probe_and_runs_stream_end_to_end() {
    let (url, st) = spawn_mock().await;
    let c = client(&url);
    let (info, _, _) = c.probe_gateway().await;
    assert!(info.online);
    assert_eq!(info.active_transport, GatewayTransport::Runs);
    assert!(info.capabilities.session_stream);

    let events = turn(c, GatewayTransport::Runs, "s1").await;
    assert!(matches!(events.first(), Some(GatewayStreamEvent::RunStarted { run_id, .. }) if run_id == "r-1"));
    let signals: Vec<&StreamSignal> = events.iter().filter_map(|e| match e {
        GatewayStreamEvent::Signal { signal, .. } => Some(signal),
        _ => None,
    }).collect();
    assert!(signals.iter().any(|s| matches!(s, StreamSignal::ToolEvent(_))));
    assert!(signals.iter().any(|s| matches!(s, StreamSignal::ApprovalRequest(a) if a.request_id == "req-1")));
    assert!(matches!(events.last(), Some(GatewayStreamEvent::Completed { result, .. }) if result.output == "All tests green."));
    let reqs = st.requests.lock().unwrap();
    assert!(reqs.iter().any(|r| r.method == "POST" && r.path == "/v1/runs"));
    assert!(reqs.iter().any(|r| r.method == "GET" && r.path == "/v1/runs/r-1/events"));
}

#[tokio::test]
async fn approval_submit_and_replay_cycle() {
    let (url, st) = spawn_mock().await;
    let c = client(&url);
    let _ = turn(c.clone(), GatewayTransport::Runs, "s1").await;
    c.submit_approval("r-1", "req-1", 0, "once", false).await.expect("approval accepted");
    {
        let reqs = st.requests.lock().unwrap();
        let post = reqs.iter().find(|r| r.path == "/v1/runs/r-1/approval").expect("approval POST");
        assert_eq!(post.body["decision"], "once");
        assert_eq!(post.body["request_id"], "req-1");
    }
    let second = turn(c, GatewayTransport::Runs, "s1").await;
    let text: String = second.iter().filter_map(|e| match e {
        GatewayStreamEvent::Signal { signal: StreamSignal::ContentDelta { chunk, .. }, .. } => Some(chunk.clone()),
        _ => None,
    }).collect();
    assert!(text.contains("[approval req-1 replayed: once]"), "replay missing: {text:?}");
}

#[tokio::test]
async fn session_stream_transport_end_to_end() {
    let (url, st) = spawn_mock().await;
    let events = turn(client(&url), GatewayTransport::SessionStream, "s2").await;
    assert!(matches!(events.last(), Some(GatewayStreamEvent::Completed { result, .. }) if result.output == "All tests green."));
    assert!(st.requests.lock().unwrap().iter().any(|r| r.path == "/api/sessions/s2/chat/stream"));
}

#[tokio::test]
async fn multi_profile_p_slug_routing() {
    let (url, st) = spawn_mock().await;
    let base = client(&url);
    // Default-profile probe traffic must stay unprefixed.
    let (info, _, _) = base.probe_gateway().await;
    assert!(info.online);
    let ops = Arc::new(base.with_profile("ops", None));
    assert_eq!(ops.config.route_prefix, "/p/ops");
    let events = turn(ops, GatewayTransport::Runs, "s3").await;
    assert!(matches!(events.last(), Some(GatewayStreamEvent::Completed { .. })));
    {
        let reqs = st.requests.lock().unwrap();
        assert!(reqs.iter().any(|r| r.profile.as_deref() == Some("ops") && r.path == "/p/ops/v1/runs"));
        assert!(reqs.iter().any(|r| r.profile.as_deref() == Some("ops") && r.path == "/p/ops/v1/runs/r-1/events"));
        assert!(reqs.iter().any(|r| r.profile.is_none() && r.path == "/health"));
    }

    // Percent-encoded slug parity with the TS profile registry.
    let lab = Arc::new(base.with_profile("research lab", None));
    assert_eq!(lab.config.route_prefix, "/p/research%20lab");
    let _ = turn(lab, GatewayTransport::Runs, "s4").await;
    let reqs = st.requests.lock().unwrap();
    assert!(reqs.iter().any(|r| r.path.starts_with("/p/research%20lab/v1/runs")));
}
