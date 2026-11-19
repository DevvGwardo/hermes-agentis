//! P4-04 mock axum gateway E2E (spec §9.2): runs, session_stream, approval
//! replay, and multi-profile `/p/<slug>/` routing — loopback only, no keys.

use std::sync::Arc;
use tokio::sync::mpsc;
use voxcell_core::GatewayTransport;
use voxcell_hermes::client::{GatewayStreamEvent, HermesClient};
use voxcell_hermes::protocol::{ProfileGatewayConfig, StreamSignal};
use voxcell_uphill::mock_gateway::{self, MockGateway};

fn client(gw: &MockGateway) -> Arc<HermesClient> {
    Arc::new(HermesClient::new(ProfileGatewayConfig {
        base_url: gw.base_url(),
        profile: "default".into(),
        api_key: String::new(),
        route_prefix: String::new(),
    }))
}

async fn turn(
    c: Arc<HermesClient>,
    transport: GatewayTransport,
    session: &str,
) -> Vec<GatewayStreamEvent> {
    let (tx, mut rx) = mpsc::unbounded_channel();
    c.stream_turn(
        transport,
        "ch-e2e".into(),
        session.into(),
        format!("voxcell-{session}"),
        "run the uphill suite".into(),
        None,
        tx,
        || {},
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
async fn probe_detects_online_runs_gateway() {
    let gw = mock_gateway::spawn().await;
    let (info, _models, _profiles) = client(&gw).probe_gateway().await;
    assert!(info.online, "mock gateway online");
    assert_eq!(info.active_transport, GatewayTransport::Runs);
    assert!(info.capabilities.runs);
    assert!(info.capabilities.session_stream);
    assert!(gw.paths().iter().any(|p| p == "/health"));
    assert!(gw.paths().iter().any(|p| p == "/v1/capabilities"));
}

#[tokio::test]
async fn runs_transport_streams_tools_approval_and_completion() {
    let gw = mock_gateway::spawn().await;
    let events = turn(client(&gw), GatewayTransport::Runs, "s1").await;
    assert!(matches!(events.first(), Some(GatewayStreamEvent::RunStarted { run_id, .. }) if run_id == "r-1"));
    let signals: Vec<&StreamSignal> = events
        .iter()
        .filter_map(|e| match e {
            GatewayStreamEvent::Signal { signal, .. } => Some(signal),
            _ => None,
        })
        .collect();
    assert!(signals.iter().any(|s| matches!(s, StreamSignal::ToolEvent(_))), "tool event streamed");
    let approval = signals
        .iter()
        .find_map(|s| match s {
            StreamSignal::ApprovalRequest(a) => Some(a),
            _ => None,
        })
        .expect("approval request replayed");
    assert_eq!(approval.request_id, "req-1");
    assert!(signals.iter().any(|s| matches!(s, StreamSignal::ContentDelta { chunk, .. } if chunk == "All tests green.")));
    assert!(matches!(events.last(), Some(GatewayStreamEvent::Completed { result, .. }) if result.output == "All tests green."));
    // Routing: POST /v1/runs then GET /v1/runs/r-1/events.
    let paths = gw.paths();
    assert!(paths.iter().any(|p| p == "/v1/runs"));
    assert!(paths.iter().any(|p| p == "/v1/runs/r-1/events"));
}

#[tokio::test]
async fn approval_decision_is_submitted_and_replayed_on_next_run() {
    let gw = mock_gateway::spawn().await;
    let c = client(&gw);
    let first = turn(c.clone(), GatewayTransport::Runs, "s1").await;
    assert!(first.iter().any(|e| matches!(
        e,
        GatewayStreamEvent::Signal { signal: StreamSignal::ApprovalRequest(a), .. } if a.request_id == "req-1"
    )));

    c.submit_approval("r-1", "req-1", 0, "once", false).await.expect("approval accepted");
    let rec = gw.recorded();
    let post = rec.iter().find(|r| r.path == "/v1/runs/r-1/approval").expect("approval POST recorded");
    assert_eq!(post.method, "POST");
    assert_eq!(post.body["decision"], "once");
    assert_eq!(post.body["request_id"], "req-1");

    // Second run replays the recorded resolution into the stream.
    let second = turn(c, GatewayTransport::Runs, "s1").await;
    let text: String = second
        .iter()
        .filter_map(|e| match e {
            GatewayStreamEvent::Signal { signal: StreamSignal::ContentDelta { chunk, .. }, .. } => Some(chunk.clone()),
            _ => None,
        })
        .collect();
    assert!(text.contains("[approval req-1 replayed: once]"), "approval replay missing: {text:?}");
}

#[tokio::test]
async fn session_stream_transport_uses_session_chat_route() {
    let gw = mock_gateway::spawn().await;
    let events = turn(client(&gw), GatewayTransport::SessionStream, "s2").await;
    assert!(matches!(events.last(), Some(GatewayStreamEvent::Completed { result, .. }) if result.output == "All tests green."));
    assert!(gw.paths().iter().any(|p| p == "/api/sessions/s2/chat/stream"));
}

#[tokio::test]
async fn multi_profile_routes_under_p_slug_prefix() {
    let gw = mock_gateway::spawn().await;
    let base = client(&gw);
    // Default-profile probe traffic must stay unprefixed.
    let (info, _, _) = base.probe_gateway().await;
    assert!(info.online);
    let ops = Arc::new(base.with_profile("ops", None));
    assert_eq!(ops.config.route_prefix, "/p/ops");
    let events = turn(ops, GatewayTransport::Runs, "s3").await;
    assert!(matches!(events.last(), Some(GatewayStreamEvent::Completed { .. })));
    let rec = gw.recorded();
    let routed: Vec<&_> = rec.iter().filter(|r| r.profile.as_deref() == Some("ops")).collect();
    assert!(routed.iter().any(|r| r.path == "/p/ops/v1/runs"));
    assert!(routed.iter().any(|r| r.path == "/p/ops/v1/runs/r-1/events"));
    // Default-profile traffic must NOT carry the prefix.
    assert!(rec.iter().any(|r| r.profile.is_none() && r.path == "/health"));
}

#[tokio::test]
async fn profile_slug_is_percent_encoded() {
    let gw = mock_gateway::spawn().await;
    let base = client(&gw);
    let lab = Arc::new(base.with_profile("research lab", None));
    assert_eq!(lab.config.route_prefix, "/p/research%20lab");
    let _ = turn(lab, GatewayTransport::Runs, "s4").await;
    assert!(gw.paths().iter().any(|p| p.starts_with("/p/research%20lab/v1/runs")));
}
