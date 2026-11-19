//! Streaming STT: backend trait + fallback router.
//!
//! - [`FluxClient`]: Deepgram Flux (`v2/listen`) WebSocket protocol over
//!   `tokio-tungstenite`. Generic over the transport so tests run it against
//!   an in-memory duplex socket with JSON fixtures — no network, no keys.
//! - [`WhisperCppBackend`]: local whisper.cpp stub (activates only when a
//!   binary + model are configured on disk).
//! - [`OsSttBackend`]: OS speech-recognition fallback stub.
//! - [`SttRouter`]: ordered fallback chain (Flux → whisper.cpp → OS).

use crate::dsp::{encode_linear16, resample_linear};
use futures_util::future::BoxFuture;
use futures_util::{SinkExt, StreamExt};
use std::path::PathBuf;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::http::Request;
use tokio_tungstenite::WebSocketStream;

pub const DEFAULT_FLUX_URL: &str = "wss://api.deepgram.com/v2/listen";
pub const DEFAULT_FLUX_MODEL: &str = "flux-general-en";
/// Audio send granularity for streaming backends.
pub const CHUNK_MS: u32 = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SttBackendKind {
    DeepgramFlux,
    WhisperCpp,
    Os,
}

#[derive(Clone, Debug)]
pub struct AudioChunk {
    pub pcm: Vec<f32>,
    pub sample_rate: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SttEvent {
    Partial { text: String },
    Final { text: String, speech_final: bool },
    UtteranceEnd,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SttError {
    /// Backend not configured / not present on this machine.
    Unavailable(String),
    Auth(String),
    Connection(String),
    Protocol(String),
}

impl std::fmt::Display for SttError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(e) => write!(f, "STT backend unavailable: {e}"),
            Self::Auth(e) => write!(f, "STT auth failed: {e}"),
            Self::Connection(e) => write!(f, "STT connection failed: {e}"),
            Self::Protocol(e) => write!(f, "STT protocol error: {e}"),
        }
    }
}

impl std::error::Error for SttError {}

#[derive(Clone, Debug)]
pub struct SttConfig {
    pub api_key: Option<String>,
    pub flux_model: String,
    /// Override for tests / proxies.
    pub flux_url: Option<String>,
    pub whisper_bin: Option<PathBuf>,
    pub whisper_model: Option<PathBuf>,
    pub os_enabled: bool,
    pub sample_rate: u32,
}

impl Default for SttConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            flux_model: DEFAULT_FLUX_MODEL.into(),
            flux_url: None,
            whisper_bin: None,
            whisper_model: None,
            os_enabled: true,
            sample_rate: 16_000,
        }
    }
}

/// Dyn-compatible async backend interface (boxed futures).
pub trait SttBackend: Send {
    fn kind(&self) -> SttBackendKind;
    fn connect(&mut self) -> BoxFuture<'_, Result<(), SttError>>;
    fn send_audio(&mut self, chunk: AudioChunk) -> BoxFuture<'_, Result<(), SttError>>;
    /// Signal end-of-utterance and drain remaining events.
    fn finish(&mut self) -> BoxFuture<'_, Result<Vec<SttEvent>, SttError>>;
}

// ---------------------------------------------------------------------------
// Deepgram Flux WebSocket client
// ---------------------------------------------------------------------------

/// Build the Flux upgrade request (auth header + query params). Pure so it
/// can be unit-tested without opening a socket.
pub fn flux_request(
    api_key: &str,
    model: &str,
    sample_rate: u32,
    url_override: Option<&str>,
) -> Result<Request<()>, SttError> {
    if api_key.trim().is_empty() {
        return Err(SttError::Auth("DEEPGRAM_API_KEY not configured".into()));
    }
    let base = url_override.unwrap_or(DEFAULT_FLUX_URL);
    let model = if model.trim().is_empty() {
        DEFAULT_FLUX_MODEL
    } else {
        model.trim()
    };
    let url = format!(
        "{base}?model={model}&encoding=linear16&sample_rate={sample_rate}&interim_results=true"
    );
    Request::builder()
        .uri(&url)
        .header("Authorization", format!("Token {}", api_key.trim()))
        .body(())
        .map_err(|e| SttError::Connection(e.to_string()))
}

/// Parse one Flux server message into an event. `Ok(None)` = ignore
/// (metadata, keepalive, empty interim result, ping/pong).
pub fn parse_flux_message(msg: Message) -> Result<Option<SttEvent>, SttError> {
    let text = match msg {
        Message::Text(t) => t.to_string(),
        Message::Close(_) => return Ok(None),
        _ => return Ok(None),
    };
    let v: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| SttError::Protocol(e.to_string()))?;
    match v.get("type").and_then(|t| t.as_str()).unwrap_or("") {
        "Results" => {
            let transcript = v
                .pointer("/channel/alternatives/0/transcript")
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            if transcript.is_empty() {
                return Ok(None);
            }
            let is_final = v.get("is_final").and_then(|b| b.as_bool()).unwrap_or(false);
            let speech_final = v
                .get("speech_final")
                .and_then(|b| b.as_bool())
                .unwrap_or(false);
            if is_final {
                Ok(Some(SttEvent::Final {
                    text: transcript,
                    speech_final,
                }))
            } else {
                Ok(Some(SttEvent::Partial { text: transcript }))
            }
        }
        "UtteranceEnd" => Ok(Some(SttEvent::UtteranceEnd)),
        "Error" => {
            let desc = v
                .get("description")
                .or_else(|| v.get("message"))
                .and_then(|d| d.as_str())
                .unwrap_or("unknown server error");
            Err(SttError::Protocol(desc.to_string()))
        }
        // Metadata, KeepAlive, unknown types: ignore.
        _ => Ok(None),
    }
}

pub struct FluxClient<S> {
    ws: WebSocketStream<S>,
    sample_rate: u32,
}

impl<S: AsyncRead + AsyncWrite + Unpin + Send> FluxClient<S> {
    /// Wrap an already-handshaked socket (used by tests with duplex IO).
    pub fn from_socket(ws: WebSocketStream<S>, sample_rate: u32) -> Self {
        Self { ws, sample_rate }
    }

    /// Resample to the negotiated rate and send one linear16 binary frame.
    pub async fn send_pcm(&mut self, pcm: &[f32], source_rate: u32) -> Result<(), SttError> {
        if pcm.is_empty() {
            return Ok(());
        }
        let resampled = resample_linear(pcm, source_rate, self.sample_rate);
        let bytes = encode_linear16(&resampled);
        self.ws
            .send(Message::binary(bytes))
            .await
            .map_err(|e| SttError::Connection(e.to_string()))
    }

    pub async fn keep_alive(&mut self) -> Result<(), SttError> {
        self.ws
            .send(Message::text(r#"{"type":"KeepAlive"}"#))
            .await
            .map_err(|e| SttError::Connection(e.to_string()))
    }

    pub async fn close_stream(&mut self) -> Result<(), SttError> {
        self.ws
            .send(Message::text(r#"{"type":"CloseStream"}"#))
            .await
            .map_err(|e| SttError::Connection(e.to_string()))
    }

    /// Next meaningful event; `Ok(None)` on clean stream close.
    pub async fn next_event(&mut self) -> Result<Option<SttEvent>, SttError> {
        loop {
            match self.ws.next().await {
                Some(Ok(Message::Close(_))) | None => return Ok(None),
                Some(Ok(msg)) => {
                    if let Some(ev) = parse_flux_message(msg)? {
                        return Ok(Some(ev));
                    }
                }
                Some(Err(e)) => return Err(SttError::Connection(e.to_string())),
            }
        }
    }

    /// End-of-utterance drain: CloseStream, then collect events until
    /// `speech_final` / `UtteranceEnd` or server close. Bounded by a 10 s
    /// timeout so a silent server cannot hang the pipeline.
    pub async fn drain_utterance(mut self) -> Result<Vec<SttEvent>, SttError> {
        self.close_stream().await?;
        let mut events = Vec::new();
        let deadline = tokio::time::Duration::from_secs(10);
        loop {
            let next = tokio::time::timeout(deadline, self.next_event()).await;
            match next {
                Err(_) => return Err(SttError::Protocol("drain timed out".into())),
                Ok(Err(e)) => return Err(e),
                Ok(Ok(None)) => break,
                Ok(Ok(Some(ev))) => {
                    let done = matches!(
                        ev,
                        SttEvent::UtteranceEnd | SttEvent::Final {
                            speech_final: true,
                            ..
                        }
                    );
                    events.push(ev);
                    if done {
                        break;
                    }
                }
            }
        }
        Ok(events)
    }
}

/// Live Deepgram Flux connection (production transport).
pub async fn connect_flux(
    api_key: &str,
    model: &str,
    sample_rate: u32,
    url_override: Option<&str>,
) -> Result<FluxClient<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>, SttError> {
    let req = flux_request(api_key, model, sample_rate, url_override)?;
    let (ws, _resp) = tokio_tungstenite::connect_async(req)
        .await
        .map_err(|e| SttError::Connection(e.to_string()))?;
    Ok(FluxClient::from_socket(ws, sample_rate))
}

/// Production trait wrapper around the live Flux connection.
pub struct FluxSttBackend {
    config: SttConfig,
    client: Option<FluxClient<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>>,
}

impl FluxSttBackend {
    pub fn new(config: SttConfig) -> Self {
        Self {
            config,
            client: None,
        }
    }
}

impl SttBackend for FluxSttBackend {
    fn kind(&self) -> SttBackendKind {
        SttBackendKind::DeepgramFlux
    }

    fn connect(&mut self) -> BoxFuture<'_, Result<(), SttError>> {
        Box::pin(async move {
            let key = self
                .config
                .api_key
                .as_deref()
                .filter(|k| !k.trim().is_empty())
                .ok_or_else(|| SttError::Unavailable("DEEPGRAM_API_KEY not configured".into()))?;
            let client = connect_flux(
                key,
                &self.config.flux_model,
                self.config.sample_rate,
                self.config.flux_url.as_deref(),
            )
            .await?;
            self.client = Some(client);
            Ok(())
        })
    }

    fn send_audio(&mut self, chunk: AudioChunk) -> BoxFuture<'_, Result<(), SttError>> {
        Box::pin(async move {
            let client = self
                .client
                .as_mut()
                .ok_or_else(|| SttError::Connection("not connected".into()))?;
            client.send_pcm(&chunk.pcm, chunk.sample_rate).await
        })
    }

    fn finish(&mut self) -> BoxFuture<'_, Result<Vec<SttEvent>, SttError>> {
        Box::pin(async move {
            let client = self
                .client
                .take()
                .ok_or_else(|| SttError::Connection("not connected".into()))?;
            client.drain_utterance().await
        })
    }
}

// ---------------------------------------------------------------------------
// whisper.cpp local backend (stub: activates only when configured)
// ---------------------------------------------------------------------------

/// Parse whisper.cpp `--output-json` transcription output.
pub fn parse_whisper_json(json: &str) -> Result<Vec<SttEvent>, SttError> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| SttError::Protocol(e.to_string()))?;
    let mut text = String::new();
    if let Some(segments) = v.get("transcription").and_then(|t| t.as_array()) {
        for seg in segments {
            if let Some(t) = seg.get("text").and_then(|t| t.as_str()) {
                text.push_str(t.trim());
                text.push(' ');
            }
        }
    } else if let Some(t) = v.get("text").and_then(|t| t.as_str()) {
        text.push_str(t);
    }
    let text = text.trim().to_string();
    if text.is_empty() {
        return Ok(Vec::new());
    }
    Ok(vec![SttEvent::Final {
        text,
        speech_final: true,
    }])
}

/// Argument vector for whisper-cli (testable without a binary).
pub fn whisper_args(model: &std::path::Path, wav: &std::path::Path) -> Vec<String> {
    vec![
        "-m".into(),
        model.display().to_string(),
        "-f".into(),
        wav.display().to_string(),
        "--output-json".into(),
        "--no-prints".into(),
    ]
}

pub struct WhisperCppBackend {
    pub bin: Option<PathBuf>,
    pub model: Option<PathBuf>,
    buffer: Vec<f32>,
}

impl WhisperCppBackend {
    pub fn new(bin: Option<PathBuf>, model: Option<PathBuf>) -> Self {
        Self {
            bin,
            model,
            buffer: Vec::new(),
        }
    }

    fn configured_paths(&self) -> Result<(PathBuf, PathBuf), SttError> {
        let bin = self
            .bin
            .clone()
            .filter(|p| p.is_file())
            .ok_or_else(|| SttError::Unavailable("whisper.cpp binary not configured".into()))?;
        let model = self
            .model
            .clone()
            .filter(|p| p.is_file())
            .ok_or_else(|| SttError::Unavailable("whisper.cpp model not configured".into()))?;
        Ok((bin, model))
    }
}

impl SttBackend for WhisperCppBackend {
    fn kind(&self) -> SttBackendKind {
        SttBackendKind::WhisperCpp
    }

    fn connect(&mut self) -> BoxFuture<'_, Result<(), SttError>> {
        Box::pin(async move {
            self.configured_paths()?;
            self.buffer.clear();
            Ok(())
        })
    }

    fn send_audio(&mut self, chunk: AudioChunk) -> BoxFuture<'_, Result<(), SttError>> {
        Box::pin(async move {
            self.buffer
                .extend(resample_linear(&chunk.pcm, chunk.sample_rate, 16_000));
            Ok(())
        })
    }

    fn finish(&mut self) -> BoxFuture<'_, Result<Vec<SttEvent>, SttError>> {
        Box::pin(async move {
            let (bin, model) = self.configured_paths()?;
            let pcm = std::mem::take(&mut self.buffer);
            if pcm.is_empty() {
                return Ok(Vec::new());
            }
            let wav = crate::tts::encode_wav(&pcm, 16_000);
            let dir = std::env::temp_dir();
            let wav_path = dir.join(format!("voxcell-whisper-{}.wav", std::process::id()));
            tokio::fs::write(&wav_path, &wav)
                .await
                .map_err(|e| SttError::Unavailable(e.to_string()))?;
            let args = whisper_args(&model, &wav_path);
            let out = tokio::process::Command::new(&bin)
                .args(&args)
                .output()
                .await
                .map_err(|e| SttError::Unavailable(e.to_string()))?;
            let _ = tokio::fs::remove_file(&wav_path).await;
            if !out.status.success() {
                return Err(SttError::Protocol(format!(
                    "whisper.cpp exited with {}",
                    out.status
                )));
            }
            let json = String::from_utf8_lossy(&out.stdout);
            parse_whisper_json(&json)
        })
    }
}

// ---------------------------------------------------------------------------
// OS fallback stub
// ---------------------------------------------------------------------------

/// OS speech-recognition fallback. Stub: platform recognizers
/// (Windows.Media.SpeechRecognition / SFSpeechRecognizer / SpeechDispatcher)
/// are not wired yet; reports Unavailable so the router can surface a clean
/// error instead of silently dropping utterances.
pub struct OsSttBackend;

impl SttBackend for OsSttBackend {
    fn kind(&self) -> SttBackendKind {
        SttBackendKind::Os
    }

    fn connect(&mut self) -> BoxFuture<'_, Result<(), SttError>> {
        Box::pin(async move {
            Err(SttError::Unavailable(format!(
                "OS speech recognition not wired on {} (stub)",
                std::env::consts::OS
            )))
        })
    }

    fn send_audio(&mut self, _chunk: AudioChunk) -> BoxFuture<'_, Result<(), SttError>> {
        Box::pin(async move { Ok(()) })
    }

    fn finish(&mut self) -> BoxFuture<'_, Result<Vec<SttEvent>, SttError>> {
        Box::pin(async move { Ok(Vec::new()) })
    }
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

pub struct SttRouter {
    backends: Vec<Box<dyn SttBackend>>,
    active_kind: Option<SttBackendKind>,
    last_error: Option<String>,
    chunk_ms: u32,
}

impl SttRouter {
    /// Build the fallback chain from config: Flux (if key) → whisper.cpp
    /// (if configured) → OS (if enabled).
    pub fn from_config(config: &SttConfig) -> Self {
        let mut backends: Vec<Box<dyn SttBackend>> = Vec::new();
        if config.api_key.as_deref().map(|k| !k.trim().is_empty()).unwrap_or(false) {
            backends.push(Box::new(FluxSttBackend::new(config.clone())));
        }
        if config.whisper_bin.is_some() || config.whisper_model.is_some() {
            backends.push(Box::new(WhisperCppBackend::new(
                config.whisper_bin.clone(),
                config.whisper_model.clone(),
            )));
        }
        if config.os_enabled {
            backends.push(Box::new(OsSttBackend));
        }
        Self {
            backends,
            active_kind: None,
            last_error: None,
            chunk_ms: CHUNK_MS,
        }
    }

    /// Inject arbitrary backends (tests / custom chains).
    pub fn new(backends: Vec<Box<dyn SttBackend>>) -> Self {
        Self {
            backends,
            active_kind: None,
            last_error: None,
            chunk_ms: CHUNK_MS,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.backends.is_empty()
    }

    pub fn active_kind(&self) -> Option<SttBackendKind> {
        self.active_kind
    }

    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    /// Transcribe one utterance, falling back through the chain on
    /// Unavailable / Connection / Auth errors.
    pub async fn transcribe(
        &mut self,
        pcm: &[f32],
        sample_rate: u32,
    ) -> Result<Vec<SttEvent>, SttError> {
        if pcm.is_empty() {
            return Ok(Vec::new());
        }
        if self.backends.is_empty() {
            return Err(SttError::Unavailable("no STT backend configured".into()));
        }
        let chunk_len = ((sample_rate as u64 * self.chunk_ms as u64) / 1000).max(1) as usize;
        let mut last_err = SttError::Unavailable("no STT backend configured".into());
        for backend in &mut self.backends {
            let kind = backend.kind();
            let result = async {
                backend.connect().await?;
                for chunk in pcm.chunks(chunk_len) {
                    backend
                        .send_audio(AudioChunk {
                            pcm: chunk.to_vec(),
                            sample_rate,
                        })
                        .await?;
                }
                backend.finish().await
            }
            .await;
            match result {
                Ok(events) => {
                    self.active_kind = Some(kind);
                    self.last_error = None;
                    return Ok(events);
                }
                Err(e) => {
                    self.last_error = Some(format!("{kind:?}: {e}"));
                    last_err = e;
                }
            }
        }
        Err(last_err)
    }
}

// ---------------------------------------------------------------------------
// Mock backend (shared with engine tests)
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod mock {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    pub enum Script {
        Fail(SttError),
        Events(Vec<SttEvent>),
    }

    pub struct MockSttBackend {
        pub kind: SttBackendKind,
        pub script: Script,
        pub chunks: Arc<Mutex<Vec<AudioChunk>>>,
        pub connects: Arc<Mutex<u32>>,
    }

    impl MockSttBackend {
        pub fn new(kind: SttBackendKind, script: Script) -> Self {
            Self {
                kind,
                script,
                chunks: Arc::new(Mutex::new(Vec::new())),
                connects: Arc::new(Mutex::new(0)),
            }
        }
    }

    impl SttBackend for MockSttBackend {
        fn kind(&self) -> SttBackendKind {
            self.kind
        }
        fn connect(&mut self) -> BoxFuture<'_, Result<(), SttError>> {
            let script = self.script.clone();
            let connects = Arc::clone(&self.connects);
            Box::pin(async move {
                *connects.lock().unwrap() += 1;
                match script {
                    Script::Fail(e) => Err(e),
                    Script::Events(_) => Ok(()),
                }
            })
        }
        fn send_audio(&mut self, chunk: AudioChunk) -> BoxFuture<'_, Result<(), SttError>> {
            let chunks = Arc::clone(&self.chunks);
            Box::pin(async move {
                chunks.lock().unwrap().push(chunk);
                Ok(())
            })
        }
        fn finish(&mut self) -> BoxFuture<'_, Result<Vec<SttEvent>, SttError>> {
            let script = self.script.clone();
            Box::pin(async move {
                match script {
                    Script::Fail(e) => Err(e),
                    Script::Events(events) => Ok(events),
                }
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mock::{MockSttBackend, Script};
    use std::sync::Arc;
    use tokio_tungstenite::tungstenite::protocol::Role;

    const PARTIAL: &str = r#"{"type":"Results","channel":{"alternatives":[{"transcript":"hello wor","confidence":0.9}]},"is_final":false,"speech_final":false}"#;
    const FINAL: &str = r#"{"type":"Results","channel":{"alternatives":[{"transcript":"hello world","confidence":0.98}]},"is_final":true,"speech_final":true}"#;
    const UTTERANCE_END: &str = r#"{"type":"UtteranceEnd"}"#;
    const METADATA: &str = r#"{"type":"Metadata","request_id":"abc","channels":1}"#;

    #[test]
    fn flux_request_url_and_auth_header() {
        let req = flux_request("test-key-123", "flux-general-en", 16_000, None).unwrap();
        let uri = req.uri().to_string();
        assert!(uri.starts_with("wss://api.deepgram.com/v2/listen?"));
        assert!(uri.contains("model=flux-general-en"));
        assert!(uri.contains("encoding=linear16"));
        assert!(uri.contains("sample_rate=16000"));
        assert_eq!(
            req.headers().get("Authorization").unwrap(),
            "Token test-key-123"
        );
        // Empty key is an auth error, not a request.
        assert!(matches!(
            flux_request("  ", "m", 16_000, None),
            Err(SttError::Auth(_))
        ));
        // Override + default model fallback.
        let req = flux_request("k", "", 48_000, Some("ws://localhost:9999/listen")).unwrap();
        assert!(req.uri().to_string().starts_with("ws://localhost:9999/listen?"));
        assert!(req.uri().to_string().contains("model=flux-general-en"));
    }

    #[test]
    fn parses_flux_fixtures() {
        assert_eq!(
            parse_flux_message(Message::text(PARTIAL)).unwrap(),
            Some(SttEvent::Partial {
                text: "hello wor".into()
            })
        );
        assert_eq!(
            parse_flux_message(Message::text(FINAL)).unwrap(),
            Some(SttEvent::Final {
                text: "hello world".into(),
                speech_final: true
            })
        );
        assert_eq!(
            parse_flux_message(Message::text(UTTERANCE_END)).unwrap(),
            Some(SttEvent::UtteranceEnd)
        );
        assert_eq!(parse_flux_message(Message::text(METADATA)).unwrap(), None);
        assert_eq!(
            parse_flux_message(Message::text(r#"{"type":"KeepAlive"}"#)).unwrap(),
            None
        );
        // Empty interim transcript is ignored.
        assert_eq!(
            parse_flux_message(Message::text(
                r#"{"type":"Results","channel":{"alternatives":[{"transcript":""}]},"is_final":false}"#
            ))
            .unwrap(),
            None
        );
        // Server error surfaces as Protocol.
        assert!(matches!(
            parse_flux_message(Message::text(
                r#"{"type":"Error","description":"bad request"}"#
            )),
            Err(SttError::Protocol(_))
        ));
        // Malformed JSON.
        assert!(matches!(
            parse_flux_message(Message::text("not json")),
            Err(SttError::Protocol(_))
        ));
    }

    #[tokio::test]
    async fn flux_client_roundtrip_over_duplex_socket() {
        let (client_io, server_io) = tokio::io::duplex(64 * 1024);
        let client_ws = WebSocketStream::from_raw_socket(client_io, Role::Client, None).await;
        let mut server = WebSocketStream::from_raw_socket(server_io, Role::Server, None).await;
        let mut client = FluxClient::from_socket(client_ws, 16_000);

        // Client audio reaches the server as linear16 binary.
        let pcm: Vec<f32> = (0..1600).map(|i| (i as f32 * 0.01).sin() * 0.5).collect();
        client.send_pcm(&pcm, 16_000).await.unwrap();
        let msg = server.next().await.unwrap().unwrap();
        match msg {
            Message::Binary(bytes) => {
                let decoded = crate::dsp::decode_linear16(&bytes);
                assert_eq!(decoded.len(), pcm.len());
                for (a, b) in pcm.iter().zip(&decoded) {
                    assert!((a - b).abs() < 1.0 / 32768.0 + 1e-6);
                }
            }
            other => panic!("expected binary audio, got {other:?}"),
        }

        // 48 kHz audio is resampled to the negotiated 16 kHz.
        let wide: Vec<f32> = vec![0.25; 4800];
        client.send_pcm(&wide, 48_000).await.unwrap();
        match server.next().await.unwrap().unwrap() {
            Message::Binary(bytes) => assert_eq!(bytes.len(), 1600 * 2),
            other => panic!("expected binary audio, got {other:?}"),
        }

        // Server fixtures flow back as events (metadata skipped).
        server.send(Message::text(METADATA)).await.unwrap();
        server.send(Message::text(PARTIAL)).await.unwrap();
        server.send(Message::text(FINAL)).await.unwrap();
        server.send(Message::text(UTTERANCE_END)).await.unwrap();
        assert_eq!(
            client.next_event().await.unwrap(),
            Some(SttEvent::Partial {
                text: "hello wor".into()
            })
        );
        assert!(matches!(
            client.next_event().await.unwrap(),
            Some(SttEvent::Final { .. })
        ));
        assert_eq!(client.next_event().await.unwrap(), Some(SttEvent::UtteranceEnd));

        // CloseStream reaches the server.
        client.close_stream().await.unwrap();
        match server.next().await.unwrap().unwrap() {
            Message::Text(t) => assert!(t.to_string().contains("CloseStream")),
            other => panic!("expected CloseStream text, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn flux_drain_stops_on_speech_final_and_server_close() {
        let (client_io, server_io) = tokio::io::duplex(64 * 1024);
        let client_ws = WebSocketStream::from_raw_socket(client_io, Role::Client, None).await;
        let mut server = WebSocketStream::from_raw_socket(server_io, Role::Server, None).await;
        let client = FluxClient::from_socket(client_ws, 16_000);

        server.send(Message::text(PARTIAL)).await.unwrap();
        server.send(Message::text(FINAL)).await.unwrap();
        // Server keeps the socket open; drain must stop at speech_final.
        let events = client.drain_utterance().await.unwrap();
        assert_eq!(events.len(), 2);
        assert!(matches!(&events[0], SttEvent::Partial { .. }));
        assert!(matches!(
            &events[1],
            SttEvent::Final {
                speech_final: true,
                ..
            }
        ));
        // CloseStream was sent before draining.
        match server.next().await.unwrap().unwrap() {
            Message::Text(t) => assert!(t.to_string().contains("CloseStream")),
            other => panic!("expected CloseStream, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn router_falls_back_through_chain() {
        let failing = MockSttBackend::new(
            SttBackendKind::DeepgramFlux,
            Script::Fail(SttError::Unavailable("no key".into())),
        );
        let failing_connects = Arc::clone(&failing.connects);
        let working = MockSttBackend::new(
            SttBackendKind::WhisperCpp,
            Script::Events(vec![SttEvent::Final {
                text: "hello world".into(),
                speech_final: true,
            }]),
        );
        let working_chunks = Arc::clone(&working.chunks);
        let mut router = SttRouter::new(vec![Box::new(failing), Box::new(working)]);
        let pcm = vec![0.1; 4000]; // 250 ms @ 16 kHz
        let events = router.transcribe(&pcm, 16_000).await.unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(router.active_kind(), Some(SttBackendKind::WhisperCpp));
        assert!(router.last_error().is_none());
        assert_eq!(*failing_connects.lock().unwrap(), 1);
        // 250 ms split into 100 ms chunks: 1600 + 1600 + 800.
        let chunks = working_chunks.lock().unwrap();
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].pcm.len(), 1600);
        assert_eq!(chunks[2].pcm.len(), 800);
        assert!(chunks.iter().all(|c| c.sample_rate == 16_000));
    }

    #[tokio::test]
    async fn router_all_backends_failing_returns_last_error() {
        let mut router = SttRouter::new(vec![
            Box::new(MockSttBackend::new(
                SttBackendKind::DeepgramFlux,
                Script::Fail(SttError::Connection("refused".into())),
            )),
            Box::new(MockSttBackend::new(
                SttBackendKind::Os,
                Script::Fail(SttError::Unavailable("stub".into())),
            )),
        ]);
        let err = router.transcribe(&[0.1; 1600], 16_000).await.unwrap_err();
        assert!(matches!(err, SttError::Unavailable(_)));
        assert!(router.last_error().unwrap().contains("Os"));
        assert_eq!(router.active_kind(), None);
    }

    #[tokio::test]
    async fn router_first_success_skips_rest_and_empty_audio_is_noop() {
        let second = MockSttBackend::new(
            SttBackendKind::Os,
            Script::Events(vec![SttEvent::UtteranceEnd]),
        );
        let second_connects = Arc::clone(&second.connects);
        let mut router = SttRouter::new(vec![
            Box::new(MockSttBackend::new(
                SttBackendKind::DeepgramFlux,
                Script::Events(vec![SttEvent::Final {
                    text: "ok".into(),
                    speech_final: true,
                }]),
            )),
            Box::new(second),
        ]);
        assert!(router.transcribe(&[], 16_000).await.unwrap().is_empty());
        assert_eq!(*second_connects.lock().unwrap(), 0);
        let events = router.transcribe(&[0.2; 1600], 16_000).await.unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(*second_connects.lock().unwrap(), 0);
        assert_eq!(router.active_kind(), Some(SttBackendKind::DeepgramFlux));
    }

    #[test]
    fn from_config_builds_expected_chain() {
        // No key, no whisper paths, OS enabled -> OS stub only.
        let r = SttRouter::from_config(&SttConfig::default());
        assert_eq!(r.backends.len(), 1);
        // Key present -> Flux first.
        let cfg = SttConfig {
            api_key: Some("test-key".into()),
            ..SttConfig::default()
        };
        let r = SttRouter::from_config(&cfg);
        assert_eq!(r.backends.len(), 2);
        assert_eq!(r.backends[0].kind(), SttBackendKind::DeepgramFlux);
        // Whisper configured -> three backends.
        let cfg = SttConfig {
            api_key: Some("test-key".into()),
            whisper_bin: Some(PathBuf::from("whisper-cli")),
            whisper_model: Some(PathBuf::from("model.bin")),
            ..SttConfig::default()
        };
        let r = SttRouter::from_config(&cfg);
        assert_eq!(r.backends.len(), 3);
        assert_eq!(r.backends[1].kind(), SttBackendKind::WhisperCpp);
    }

    #[test]
    fn whisper_json_fixture_parses_to_final() {
        let fixture = r#"{"systeminfo":"AVX","model":{"type":"base"},"transcription":[{"offsets":{"from":0,"to":1000},"text":" hello"},{"offsets":{"from":1000,"to":2000},"text":" world "}],"result":{"language":"en"}}"#;
        let events = parse_whisper_json(fixture).unwrap();
        assert_eq!(
            events,
            vec![SttEvent::Final {
                text: "hello world".into(),
                speech_final: true
            }]
        );
        // Flat shape + empty transcription.
        assert_eq!(
            parse_whisper_json(r#"{"text":"flat"}"#).unwrap(),
            vec![SttEvent::Final {
                text: "flat".into(),
                speech_final: true
            }]
        );
        assert!(parse_whisper_json(r#"{"transcription":[]}"#).unwrap().is_empty());
        assert!(parse_whisper_json("garbage").is_err());
    }

    #[test]
    fn whisper_args_include_model_wav_and_json_flags() {
        let args = whisper_args(
            std::path::Path::new("models/ggml-base.bin"),
            std::path::Path::new("utt.wav"),
        );
        assert!(args.windows(2).any(|w| w[0] == "-m" && w[1] == "models/ggml-base.bin"));
        assert!(args.windows(2).any(|w| w[0] == "-f" && w[1] == "utt.wav"));
        assert!(args.contains(&"--output-json".to_string()));
    }

    #[tokio::test]
    async fn whisper_backend_without_binary_is_unavailable() {
        let mut b = WhisperCppBackend::new(None, None);
        assert!(matches!(
            b.connect().await,
            Err(SttError::Unavailable(_))
        ));
        let mut b = WhisperCppBackend::new(
            Some(PathBuf::from("definitely-not-a-real-whisper-binary")),
            Some(PathBuf::from("no-model.bin")),
        );
        assert!(matches!(
            b.connect().await,
            Err(SttError::Unavailable(_))
        ));
    }

    #[tokio::test]
    async fn os_stub_reports_unavailable() {
        let mut b = OsSttBackend;
        assert!(matches!(
            b.connect().await,
            Err(SttError::Unavailable(_))
        ));
    }
}
