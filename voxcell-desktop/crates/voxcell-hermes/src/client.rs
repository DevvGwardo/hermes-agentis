use crate::discovery::{collect_env_map, discover_local_profiles};
use crate::protocol::{
    ChatCompletionsStreamParser, PreferredTransport, ProfileGatewayConfig, RunsEventStreamParser,
    StreamSignal, StreamTerminalResult, build_gateway_url, choose_gateway_transport,
    create_idempotency_key, map_gateway_jobs, map_gateway_sessions, map_gateway_skills,
    HermesCapabilities, map_gateway_toolsets, parse_capabilities_full, parse_capabilities_payload,
    parse_model_options_payload,
    resolve_profile_gateway_config, sanitize_session_key_header, GatewaySessionRecord,
};
use futures_util::StreamExt;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use reqwest::{Client, Response, StatusCode};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use voxcell_core::{
    GatewayCapabilities, GatewayProfile, GatewayTransport, HermesCronJob, HermesGatewayInfo,
    HermesModelOption, HermesSkill, HermesToolset,
};

#[derive(Clone, Debug)]
pub enum GatewayStreamEvent {
    RunStarted { channel_id: String, run_id: String },
    Signal { channel_id: String, signal: StreamSignal },
    Completed { channel_id: String, result: StreamTerminalResult },
    Error { channel_id: String, message: String },
}

#[derive(Clone)]
pub struct HermesClient {
    http: Client,
    pub config: ProfileGatewayConfig,
    pub preferred_transport: PreferredTransport,
}

impl HermesClient {
    pub fn from_env() -> Self {
        let env = collect_env_map();
        let cfg = resolve_profile_gateway_config(
            env.get("HERMES_API_URL").map(String::as_str),
            env.get("HERMES_PROFILE").map(String::as_str),
            env.get("HERMES_API_KEY").map(String::as_str),
            env.get("HERMES_PROFILE_API_KEY").map(String::as_str),
            &env,
        );
        Self::new(cfg)
    }

    pub fn new(config: ProfileGatewayConfig) -> Self {
        let http = Client::builder()
            .connect_timeout(Duration::from_secs(4))
            .build()
            .unwrap_or_else(|_| Client::new());
        Self {
            http,
            config,
            preferred_transport: PreferredTransport::Auto,
        }
    }

    pub fn with_profile(&self, profile: &str, api_key: Option<&str>) -> Self {
        let env = collect_env_map();
        let next_cfg = resolve_profile_gateway_config(
            Some(&self.config.base_url),
            Some(profile),
            Some(&self.config.api_key),
            api_key,
            &env,
        );
        Self {
            http: self.http.clone(),
            config: next_cfg,
            preferred_transport: self.preferred_transport,
        }
    }

    fn auth_headers(&self, session_id: Option<&str>, session_key: Option<&str>) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        if !self.config.api_key.is_empty() {
            if let Ok(val) = HeaderValue::from_str(&format!("Bearer {}", self.config.api_key)) {
                headers.insert(AUTHORIZATION, val);
            }
        }
        if let Some(sid) = session_id.and_then(|s| HeaderValue::from_str(s).ok()) {
            headers.insert("X-Hermes-Session-Id", sid);
        }
        if let Some(skey) = sanitize_session_key_header(session_key)
            .and_then(|s| HeaderValue::from_str(&s).ok())
        {
            headers.insert("X-Hermes-Session-Key", skey);
        }
        headers
    }

    pub async fn fetch_with_429_retry<F>(&self, mut make_req: F) -> Result<Response, String>
    where
        F: FnMut() -> reqwest::RequestBuilder,
    {
        let mut attempt = 0u32;
        let max_retries = 3u32;
        loop {
            let res = make_req()
                .send()
                .await
                .map_err(|e| format!("gateway request failed: {e}"))?;
            if res.status() != StatusCode::TOO_MANY_REQUESTS || attempt >= max_retries {
                return Ok(res);
            }
            let retry_ms = res
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.trim().parse::<f64>().ok())
                .map(|sec| (sec * 1000.0).clamp(50.0, 30_000.0) as u64)
                .unwrap_or_else(|| (350u64 << attempt).min(5000));
            attempt += 1;
            tokio::time::sleep(Duration::from_millis(retry_ms)).await;
        }
    }

    pub async fn probe_gateway(&self) -> (HermesGatewayInfo, Vec<HermesModelOption>, Vec<GatewayProfile>) {
        let t0 = Instant::now();
        let health_url = build_gateway_url(&self.config, "/health", false);
        let health_res = self.http.get(&health_url).headers(self.auth_headers(None, None)).send().await;

        let mut online = false;
        let mut version = None;
        let mut last_error = None;

        match health_res {
            Ok(resp) if resp.status().is_success() => {
                online = true;
                if let Ok(json) = resp.json::<Value>().await {
                    version = json.get("version").and_then(|v| v.as_str()).map(ToOwned::to_owned);
                }
            }
            Ok(resp) => {
                last_error = Some(format!("HTTP {}", resp.status()));
            }
            Err(e) => {
                last_error = Some(format!("{e}"));
            }
        }

        let latency_ms = Some(t0.elapsed().as_millis() as u64);
        let mut caps = GatewayCapabilities::default();
        let mut cap_model = None;
        let mut full_caps: Option<HermesCapabilities> = None;

        if online {
            let cap_url = build_gateway_url(&self.config, "/v1/capabilities", false);
            if let Ok(resp) = self.http.get(&cap_url).headers(self.auth_headers(None, None)).send().await {
                if resp.status().is_success() {
                    if let Ok(json) = resp.json::<Value>().await {
                        if let Some((parsed, model)) = parse_capabilities_payload(&json) {
                            caps = parsed;
                            cap_model = model;
                            full_caps = parse_capabilities_full(&json);
                        }
                    }
                }
            }
        }

        let (models, active_model) = if online {
            let model_url = build_gateway_url(&self.config, "/api/model/options", false);
            match self.http.get(&model_url).headers(self.auth_headers(None, None)).send().await {
                Ok(resp) if resp.status().is_success() => resp
                    .json::<Value>()
                    .await
                    .ok()
                    .map(|v| parse_model_options_payload(&v))
                    .unwrap_or_default(),
                _ => (Vec::new(), cap_model.clone()),
            }
        } else {
            (Vec::new(), cap_model.clone())
        };

        let transport = choose_gateway_transport(self.preferred_transport, full_caps.as_ref());
        let model = active_model
            .or(cap_model)
            .unwrap_or_else(|| "hermes-agent".to_string());

        let mut profiles = discover_local_profiles(&self.config.base_url);
        for p in &mut profiles {
            p.active = p.name.eq_ignore_ascii_case(&self.config.profile);
            if p.active {
                p.online = online;
                p.model = Some(model.clone());
            }
        }

        let info = HermesGatewayInfo {
            online,
            connected: online,
            base_url: self.config.base_url.clone(),
            active_profile: self.config.profile.clone(),
            model,
            active_transport: transport,
            capabilities: caps,
            version,
            latency_ms,
            last_error,
        };
        (info, models, profiles)
    }

    pub async fn fetch_sessions(&self) -> Result<Vec<GatewaySessionRecord>, String> {
        let url = build_gateway_url(&self.config, "/api/sessions", false);
        let resp = self
            .http
            .get(&url)
            .headers(self.auth_headers(None, None))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(format!("sessions HTTP {}", resp.status()));
        }
        let val: Value = resp.json().await.map_err(|e| e.to_string())?;
        Ok(map_gateway_sessions(&val))
    }

    pub async fn fetch_skills(&self) -> Result<Vec<HermesSkill>, String> {
        let url = build_gateway_url(&self.config, "/v1/skills", false);
        let resp = self
            .http
            .get(&url)
            .headers(self.auth_headers(None, None))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(format!("skills HTTP {}", resp.status()));
        }
        let val: Value = resp.json().await.map_err(|e| e.to_string())?;
        Ok(map_gateway_skills(&val))
    }

    pub async fn fetch_toolsets(&self) -> Result<Vec<HermesToolset>, String> {
        let url = build_gateway_url(&self.config, "/v1/toolsets", false);
        let resp = self
            .http
            .get(&url)
            .headers(self.auth_headers(None, None))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(format!("toolsets HTTP {}", resp.status()));
        }
        let val: Value = resp.json().await.map_err(|e| e.to_string())?;
        Ok(map_gateway_toolsets(&val))
    }

    pub async fn fetch_jobs(&self) -> Result<Vec<HermesCronJob>, String> {
        let url = build_gateway_url(&self.config, "/api/jobs", false);
        let resp = self
            .http
            .get(&url)
            .headers(self.auth_headers(None, None))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(format!("jobs HTTP {}", resp.status()));
        }
        let val: Value = resp.json().await.map_err(|e| e.to_string())?;
        Ok(map_gateway_jobs(&val))
    }

    pub async fn submit_approval(
        &self,
        run_id: &str,
        request_id: &str,
        execution_generation: u32,
        decision: &str,
        is_clarify: bool,
    ) -> Result<(), String> {
        let suffix = if is_clarify { "clarify" } else { "approval" };
        let url = build_gateway_url(&self.config, &format!("/v1/runs/{run_id}/{suffix}"), false);
        let body = if is_clarify {
            json!({ "request_id": request_id, "answer": decision, "execution_generation": execution_generation })
        } else {
            json!({ "request_id": request_id, "decision": decision, "execution_generation": execution_generation })
        };
        let resp = self
            .http
            .post(&url)
            .headers(self.auth_headers(None, None))
            .json(&body)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(format!("approval HTTP {}", resp.status()));
        }
        Ok(())
    }

    pub async fn stop_run(&self, run_id: &str) -> Result<(), String> {
        let url = build_gateway_url(&self.config, &format!("/v1/runs/{run_id}/stop"), false);
        let resp = self
            .http
            .post(&url)
            .headers(self.auth_headers(None, None))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(format!("stop HTTP {}", resp.status()));
        }
        Ok(())
    }

    pub async fn steer_run(&self, run_id: &str, text: &str) -> Result<(), String> {
        let url = build_gateway_url(&self.config, &format!("/v1/runs/{run_id}/steer"), false);
        let resp = self
            .http
            .post(&url)
            .headers(self.auth_headers(None, None))
            .json(&json!({ "text": text }))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(format!("steer HTTP {}", resp.status()));
        }
        Ok(())
    }

    pub async fn reset_session(&self, session_id: &str) -> Result<(), String> {
        let url = build_gateway_url(&self.config, &format!("/api/sessions/{session_id}/reset"), false);
        let resp = self
            .http
            .post(&url)
            .headers(self.auth_headers(Some(session_id), None))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(format!("reset HTTP {}", resp.status()));
        }
        Ok(())
    }

    pub async fn job_action(&self, job_id: &str, action: &str) -> Result<(), String> {
        let path = match action {
            "delete" => return self.delete_job(job_id).await,
            "pause" | "resume" | "run" => format!("/api/jobs/{job_id}/{action}"),
            _ => return Err(format!("unknown job action: {action}")),
        };
        let url = build_gateway_url(&self.config, &path, false);
        let resp = self
            .http
            .post(&url)
            .headers(self.auth_headers(None, None))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(format!("job {action} HTTP {}", resp.status()));
        }
        Ok(())
    }

    pub async fn delete_job(&self, job_id: &str) -> Result<(), String> {
        let url = build_gateway_url(&self.config, &format!("/api/jobs/{job_id}"), false);
        let resp = self
            .http
            .delete(&url)
            .headers(self.auth_headers(None, None))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(format!("job delete HTTP {}", resp.status()));
        }
        Ok(())
    }

    pub async fn stream_turn(
        self: Arc<Self>,
        transport: GatewayTransport,
        channel_id: String,
        session_id: String,
        session_key: String,
        prompt: String,
        model: Option<String>,
        tx: mpsc::UnboundedSender<GatewayStreamEvent>,
        wake: impl Fn() + Send + Sync + 'static,
    ) {
        let idem_key = create_idempotency_key("voxcell");
        match transport {
            GatewayTransport::Runs => {
                let run_url = build_gateway_url(&self.config, "/v1/runs", false);
                let body = json!({
                    "input": prompt,
                    "session_id": session_id,
                    "session_key": session_key,
                    "model": model,
                });
                let res = self
                    .fetch_with_429_retry(|| {
                        let mut headers = self.auth_headers(Some(&session_id), Some(&session_key));
                        if let Ok(iv) = HeaderValue::from_str(&idem_key) {
                            headers.insert("Idempotency-Key", iv);
                        }
                        self.http.post(&run_url).headers(headers).json(&body)
                    })
                    .await;
                let resp = match res {
                    Ok(r) if r.status().is_success() => r,
                    Ok(r) => {
                        let _ = tx.send(GatewayStreamEvent::Error {
                            channel_id,
                            message: format!("run submission HTTP {}", r.status()),
                        });
                        wake();
                        return;
                    }
                    Err(e) => {
                        let _ = tx.send(GatewayStreamEvent::Error { channel_id, message: e });
                        wake();
                        return;
                    }
                };
                let Ok(val) = resp.json::<Value>().await else {
                    let _ = tx.send(GatewayStreamEvent::Error {
                        channel_id,
                        message: "invalid run JSON".into(),
                    });
                    wake();
                    return;
                };
                let run_id = val
                    .get("run_id")
                    .or_else(|| val.get("id"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                if run_id.is_empty() {
                    let _ = tx.send(GatewayStreamEvent::Error {
                        channel_id,
                        message: "missing run_id".into(),
                    });
                    wake();
                    return;
                }
                let _ = tx.send(GatewayStreamEvent::RunStarted {
                    channel_id: channel_id.clone(),
                    run_id: run_id.clone(),
                });
                wake();

                let ev_url = build_gateway_url(&self.config, &format!("/v1/runs/{run_id}/events"), false);
                let ev_resp = match self
                    .http
                    .get(&ev_url)
                    .headers(self.auth_headers(Some(&session_id), Some(&session_key)))
                    .send()
                    .await
                {
                    Ok(r) if r.status().is_success() => r,
                    Ok(r) => {
                        let _ = tx.send(GatewayStreamEvent::Error {
                            channel_id,
                            message: format!("events HTTP {}", r.status()),
                        });
                        wake();
                        return;
                    }
                    Err(e) => {
                        let _ = tx.send(GatewayStreamEvent::Error {
                            channel_id,
                            message: e.to_string(),
                        });
                        wake();
                        return;
                    }
                };
                let mut parser = RunsEventStreamParser::new();
                let mut stream = ev_resp.bytes_stream();
                while let Some(chunk) = stream.next().await {
                    if let Ok(bytes) = chunk {
                        let text = String::from_utf8_lossy(&bytes);
                        for sig in parser.push(&text) {
                            let _ = tx.send(GatewayStreamEvent::Signal {
                                channel_id: channel_id.clone(),
                                signal: sig,
                            });
                            wake();
                        }
                    }
                }
                let (tail, result) = parser.finish();
                for sig in tail {
                    let _ = tx.send(GatewayStreamEvent::Signal {
                        channel_id: channel_id.clone(),
                        signal: sig,
                    });
                }
                let _ = tx.send(GatewayStreamEvent::Completed { channel_id, result });
                wake();
            }
            GatewayTransport::SessionStream | GatewayTransport::ChatCompletions => {
                let path = if transport == GatewayTransport::SessionStream {
                    format!("/api/sessions/{session_id}/chat/stream")
                } else {
                    "/v1/chat/completions".to_string()
                };
                let url = build_gateway_url(&self.config, &path, false);
                let body = if transport == GatewayTransport::SessionStream {
                    json!({ "message": prompt, "model": model })
                } else {
                    json!({
                        "model": model.unwrap_or_else(|| "hermes-agent".into()),
                        "stream": true,
                        "messages": [{ "role": "user", "content": prompt }],
                    })
                };
                let res = self
                    .fetch_with_429_retry(|| {
                        let mut headers = self.auth_headers(Some(&session_id), Some(&session_key));
                        if let Ok(iv) = HeaderValue::from_str(&idem_key) {
                            headers.insert("Idempotency-Key", iv);
                        }
                        self.http.post(&url).headers(headers).json(&body)
                    })
                    .await;
                let resp = match res {
                    Ok(r) if r.status().is_success() => r,
                    Ok(r) => {
                        let _ = tx.send(GatewayStreamEvent::Error {
                            channel_id,
                            message: format!("chat HTTP {}", r.status()),
                        });
                        wake();
                        return;
                    }
                    Err(e) => {
                        let _ = tx.send(GatewayStreamEvent::Error { channel_id, message: e });
                        wake();
                        return;
                    }
                };
                let mut stream = resp.bytes_stream();
                if transport == GatewayTransport::SessionStream {
                    let mut parser = RunsEventStreamParser::new();
                    while let Some(chunk) = stream.next().await {
                        if let Ok(bytes) = chunk {
                            let text = String::from_utf8_lossy(&bytes);
                            for sig in parser.push(&text) {
                                let _ = tx.send(GatewayStreamEvent::Signal {
                                    channel_id: channel_id.clone(),
                                    signal: sig,
                                });
                                wake();
                            }
                        }
                    }
                    let (tail, result) = parser.finish();
                    for sig in tail {
                        let _ = tx.send(GatewayStreamEvent::Signal {
                            channel_id: channel_id.clone(),
                            signal: sig,
                        });
                    }
                    let _ = tx.send(GatewayStreamEvent::Completed { channel_id, result });
                    wake();
                } else {
                    let mut parser = ChatCompletionsStreamParser::new();
                    while let Some(chunk) = stream.next().await {
                        if let Ok(bytes) = chunk {
                            let text = String::from_utf8_lossy(&bytes);
                            for sig in parser.push(&text) {
                                let _ = tx.send(GatewayStreamEvent::Signal {
                                    channel_id: channel_id.clone(),
                                    signal: sig,
                                });
                                wake();
                            }
                        }
                    }
                    let (tail, result) = parser.finish();
                    for sig in tail {
                        let _ = tx.send(GatewayStreamEvent::Signal {
                            channel_id: channel_id.clone(),
                            signal: sig,
                        });
                    }
                    let _ = tx.send(GatewayStreamEvent::Completed { channel_id, result });
                    wake();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn streams_runs_events_and_approvals_against_live_mock_gateway() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    break;
                };
                tokio::spawn(async move {
                    let mut buf = [0u8; 4096];
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]);
                    let first_line = req.lines().next().unwrap_or("");
                    if first_line.contains("/health") {
                        let body = r#"{"status":"ok","version":"hermes-test"}"#;
                        let resp = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                            body.len()
                        );
                        let _ = sock.write_all(resp.as_bytes()).await;
                    } else if first_line.contains("/v1/capabilities") {
                        let body = r#"{"model":"glm-5.3","features":{"run_submission":true,"run_events_sse":true,"run_approval":true}}"#;
                        let resp = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                            body.len()
                        );
                        let _ = sock.write_all(resp.as_bytes()).await;
                    } else if first_line.contains("/v1/runs/r-99/events") {
                        let sse = concat!(
                            ": keepalive\n\n",
                            "data: {\"event\":\"tool.started\",\"tool\":\"terminal\",\"preview\":\"cargo test\"}\n\n",
                            "data: {\"event\":\"message.delta\",\"delta\":\"All tests green.\"}\n\n",
                            "data: {\"event\":\"run.completed\",\"output\":\"All tests green.\"}\n\n",
                        );
                        let resp = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\r\n{sse}",
                            sse.len()
                        );
                        let _ = sock.write_all(resp.as_bytes()).await;
                    } else if first_line.contains("/v1/runs") {
                        let body = r#"{"run_id":"r-99"}"#;
                        let resp = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                            body.len()
                        );
                        let _ = sock.write_all(resp.as_bytes()).await;
                    } else {
                        let _ = sock
                            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}")
                            .await;
                    }
                });
            }
        });

        let client = Arc::new(HermesClient::new(ProfileGatewayConfig {
            base_url: format!("http://{addr}"),
            profile: "default".into(),
            api_key: String::new(),
            route_prefix: String::new(),
        }));
        let (info, _, _) = client.probe_gateway().await;
        assert!(info.online);
        assert_eq!(info.active_transport, GatewayTransport::Runs);

        let (tx, mut rx) = mpsc::unbounded_channel();
        client
            .stream_turn(
                GatewayTransport::Runs,
                "ch1".into(),
                "s1".into(),
                "voxcell-s1".into(),
                "run tests".into(),
                None,
                tx,
                || {},
            )
            .await;

        let mut events = Vec::new();
        while let Some(ev) = rx.recv().await {
            let done = matches!(ev, GatewayStreamEvent::Completed { .. });
            events.push(ev);
            if done {
                break;
            }
        }
        assert!(events.iter().any(|e| matches!(
            e,
            GatewayStreamEvent::Completed { result, .. } if result.output == "All tests green."
        )));
    }
}
