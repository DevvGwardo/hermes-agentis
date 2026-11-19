//! In-process mock Hermes gateway (axum) for E2E tests — spec §9.2.
//! Replays `runs`, `session_stream`, approval request/resolution cycles and
//! multi-profile `/p/<slug>/` routing. Loopback only; no live network/keys.

use axum::body::Bytes;
use axum::extract::Request;
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use axum::Router;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;

#[derive(Clone, Debug)]
pub struct RecordedRequest {
    pub method: String,
    /// Full path including any `/p/<slug>/` prefix.
    pub path: String,
    /// Profile slug when routed under `/p/<slug>/`, else None.
    pub profile: Option<String>,
    pub body: Value,
}

#[derive(Clone, Default)]
struct GatewayState {
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
    /// request_id -> decision, replayed on the next run's event stream.
    approvals: Arc<Mutex<HashMap<String, String>>>,
    run_counter: Arc<Mutex<u32>>,
}

/// A running mock gateway. Drop the handle to stop serving (the task is
/// aborted via the shutdown watch).
pub struct MockGateway {
    pub addr: SocketAddr,
    pub requests: Arc<Mutex<Vec<RecordedRequest>>>,
    shutdown: tokio::sync::watch::Sender<bool>,
}

impl MockGateway {
    pub fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }
    pub fn recorded(&self) -> Vec<RecordedRequest> {
        self.requests.lock().unwrap().clone()
    }
    pub fn paths(&self) -> Vec<String> {
        self.recorded().into_iter().map(|r| r.path).collect()
    }
}

impl Drop for MockGateway {
    fn drop(&mut self) {
        let _ = self.shutdown.send(true);
    }
}

fn json_response(v: Value) -> Response {
    (
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        serde_json::to_string(&v).unwrap(),
    )
        .into_response()
}

fn sse_response(events: &[Value]) -> Response {
    let mut body = String::from(": keepalive\n\n");
    for ev in events {
        body.push_str(&format!("data: {}\n\n", serde_json::to_string(ev).unwrap()));
    }
    (
        [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
        body,
    )
        .into_response()
}

fn run_events(st: &GatewayState, run_id: &str) -> Vec<Value> {
    let mut events = vec![
        json!({"event": "tool.started", "tool": "terminal", "preview": "cargo test"}),
    ];
    // Approval replay: first run requests approval; later runs replay the
    // recorded resolutions so the client sees the full request/resolve cycle.
    let approvals = st.approvals.lock().unwrap().clone();
    if approvals.is_empty() {
        events.push(json!({
            "event": "approval.request",
            "request_id": "req-1",
            "run_id": run_id,
            "tool": "terminal",
            "command": "rm -rf target/",
            "choices": ["once", "always", "deny"],
            "execution_generation": 0,
        }));
    } else {
        // Approval replay: echo the recorded resolutions into the stream.
        for (req, decision) in &approvals {
            events.push(json!({
                "event": "message.delta",
                "delta": format!("[approval {req} replayed: {decision}]"),
            }));
        }
    }
    events.push(json!({"event": "message.delta", "delta": "All tests green."}));
    events.push(json!({"event": "run.completed", "output": "All tests green."}));
    events
}

async fn handle(st: GatewayState, req: Request) -> Response {
    let method = req.method().as_str().to_string();
    let full_path = req.uri().path().to_string();
    let body_bytes = axum::body::to_bytes(req.into_body(), 1024 * 1024)
        .await
        .unwrap_or_default();
    let body: Value = serde_json::from_slice(&body_bytes).unwrap_or(Value::Null);

    // Strip and record the multi-profile prefix `/p/<slug>/...`.
    let (profile, path) = if let Some(rest) = full_path.strip_prefix("/p/") {
        match rest.split_once('/') {
            Some((slug, tail)) => (Some(slug.to_string()), format!("/{tail}")),
            None => (Some(rest.to_string()), "/".to_string()),
        }
    } else {
        (None, full_path.clone())
    };
    st.requests.lock().unwrap().push(RecordedRequest {
        method: method.clone(),
        path: full_path,
        profile,
        body: body.clone(),
    });

    match (method.as_str(), path.as_str()) {
        ("GET", "/health") => json_response(json!({"status": "ok", "version": "hermes-mock"})),
        ("GET", "/v1/capabilities") => json_response(json!({
            "object": "capabilities",
            "platform": "mock",
            "model": "hermes-agent",
            "features": {
                "run_submission": true,
                "run_events_sse": true,
                "run_approval": true,
                "session_chat_stream": true,
                "chat_completions": true,
            }
        })),
        ("GET", "/v1/models") => json_response(json!({"data": [{"id": "hermes-agent"}]})),
        ("POST", "/v1/runs") => {
            let mut n = st.run_counter.lock().unwrap();
            *n += 1;
            json_response(json!({"run_id": format!("r-{n}")}))
        }
        ("GET", p) if p.starts_with("/v1/runs/") && p.ends_with("/events") => {
            let run_id = p.trim_start_matches("/v1/runs/").trim_end_matches("/events");
            sse_response(&run_events(&st, run_id))
        }
        ("POST", p) if p.starts_with("/v1/runs/") && p.ends_with("/approval") => {
            if let Some(req_id) = body.get("request_id").and_then(|v| v.as_str()) {
                let decision = body.get("decision").and_then(|v| v.as_str()).unwrap_or("once");
                st.approvals.lock().unwrap().insert(req_id.to_string(), decision.to_string());
            }
            json_response(json!({"ok": true}))
        }
        ("POST", p) if p.starts_with("/v1/runs/") && p.ends_with("/clarify") => {
            json_response(json!({"ok": true}))
        }
        ("POST", p) if p.starts_with("/v1/runs/") && p.ends_with("/stop") => {
            json_response(json!({"ok": true}))
        }
        ("POST", p) if p.starts_with("/api/sessions/") && p.ends_with("/chat/stream") => {
            sse_response(&run_events(&st, "session-stream"))
        }
        ("POST", "/v1/chat/completions") => sse_response(&[
            json!({"choices": [{"delta": {"content": "All tests green."}}]}),
            json!({"choices": [{"delta": {}, "finish_reason": "stop"}]}),
        ]),
        _ => json_response(json!({"ok": true, "mock": "fallback"})),
    }
}

/// Spawn the mock gateway on an ephemeral loopback port.
pub async fn spawn() -> MockGateway {
    let st = GatewayState::default();
    let requests = st.requests.clone();
    let app = Router::new().fallback(any(move |req| {
        let st = st.clone();
        async move { handle(st, req).await }
    }));
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind loopback");
    let addr = listener.local_addr().unwrap();
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);
    tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.changed().await;
            })
            .await
            .ok();
    });
    MockGateway { addr, requests, shutdown: shutdown_tx }
}

/// Silence unused-import warning for Bytes in some axum feature sets.
#[allow(dead_code)]
fn _bytes_type_check(_: Bytes) {}
