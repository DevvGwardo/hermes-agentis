//! Hermes Gateway protocol parity with `src/utils/hermesProtocol.ts` (P1-01).
//!
//! Pure parsing/mapping layer: no network I/O lives here (see `client.rs`).
//! Semantics intentionally mirror the TypeScript reference, including JS
//! truthiness (`a || b`) chains and `Boolean()`/`String()`/`Number()` coercions.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use uuid::Uuid;
use voxcell_core::{
    ActivityKind, ApprovalKind, EntryRole, GatewayCapabilities, GatewayTransport, HermesCronJob,
    HermesModelOption, HermesSkill, HermesToolset, SessionOrigin, TerminalEntry, TerminalOutcome,
};

// ---------------------------------------------------------------------------
// JS-style coercion helpers (parity with `||` chains, Boolean/String/Number)
// ---------------------------------------------------------------------------

/// JS `Boolean(v)` for JSON values.
pub fn js_truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0 && !f.is_nan()),
        Value::String(s) => !s.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// JS `String(v)` for JSON primitives (objects/arrays approximate JS output).
pub fn js_string(v: &Value) -> String {
    match v {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i.to_string()
            } else if let Some(u) = n.as_u64() {
                u.to_string()
            } else {
                let f = n.as_f64().unwrap_or(f64::NAN);
                if f.is_nan() {
                    "NaN".to_string()
                } else if f == f.trunc() && f.abs() < 1e21 {
                    format!("{}", f as i64)
                } else {
                    format!("{f}")
                }
            }
        }
        Value::String(s) => s.clone(),
        Value::Array(a) => a
            .iter()
            .map(|x| if x.is_null() { String::new() } else { js_string(x) })
            .collect::<Vec<_>>()
            .join(","),
        Value::Object(_) => "[object Object]".to_string(),
    }
}

/// `String(obj[k1] || obj[k2] || ... || '')` — truthy selection then coercion.
pub fn truthy_str_chain(obj: &serde_json::Map<String, Value>, keys: &[&str]) -> String {
    for k in keys {
        if let Some(v) = obj.get(*k) {
            if js_truthy(v) {
                return js_string(v);
            }
        }
    }
    String::new()
}

/// `(typeof obj[k] === 'string' && obj[k]) || ...` — truthy *string-only* chain.
pub fn truthy_string_opt(obj: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<String> {
    for k in keys {
        if let Some(Value::String(s)) = obj.get(*k) {
            if !s.is_empty() {
                return Some(s.clone());
            }
        }
    }
    None
}

/// `typeof obj[k] === 'string' ? obj[k] : undefined` (empty string preserved).
pub fn string_opt(obj: &serde_json::Map<String, Value>, key: &str) -> Option<String> {
    obj.get(key).and_then(|v| v.as_str()).map(ToOwned::to_owned)
}

/// JS `Number(v)` (partial: numbers, numeric strings, booleans, null -> 0).
pub fn js_number(v: &Value) -> Option<f64> {
    match v {
        Value::Null => Some(0.0),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        Value::Number(n) => n.as_f64(),
        Value::String(s) => {
            let t = s.trim();
            if t.is_empty() {
                Some(0.0)
            } else {
                t.parse::<f64>().ok()
            }
        }
        _ => None,
    }
}

fn obj_get<'a>(obj: &'a serde_json::Map<String, Value>, key: &str) -> Option<&'a Value> {
    obj.get(key)
}

// ---------------------------------------------------------------------------
// Profile gateway config (resolveProfileGatewayConfig / buildGatewayUrl)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileGatewayConfig {
    pub base_url: String,
    pub profile: String,
    pub api_key: String,
    pub route_prefix: String,
}

pub fn resolve_profile_gateway_config(
    base_url: Option<&str>,
    profile: Option<&str>,
    default_key: Option<&str>,
    profile_key: Option<&str>,
    env_vars: &HashMap<String, String>,
) -> ProfileGatewayConfig {
    // TS: (env.HERMES_API_URL || default).replace(/\/+$/, '') — no whitespace trim.
    let base = base_url
        .filter(|s| !s.is_empty())
        .unwrap_or("http://127.0.0.1:8642")
        .trim_end_matches('/')
        .to_string();

    let raw_profile = profile
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("default")
        .to_string();
    let is_default = raw_profile.eq_ignore_ascii_case("default");
    let route_prefix = if is_default {
        String::new()
    } else {
        format!("/p/{}", percent_encode_segment(&raw_profile))
    };

    // TS: rawProfile.replace(/[^a-zA-Z0-9]+/g, '_').toUpperCase() — runs collapse.
    let mut slug_env = String::new();
    let mut in_sep = false;
    for c in raw_profile.chars() {
        if c.is_ascii_alphanumeric() {
            slug_env.push(c.to_ascii_uppercase());
            in_sep = false;
        } else if !in_sep {
            slug_env.push('_');
            in_sep = true;
        }
    }
    let env_profile_key = env_vars
        .get(&format!("HERMES_API_KEY_{slug_env}"))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let pkey = env_profile_key
        .or_else(|| {
            profile_key
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(ToOwned::to_owned)
        })
        .unwrap_or_default();
    let dkey = default_key.map(str::trim).unwrap_or("").to_string();

    ProfileGatewayConfig {
        base_url: base,
        profile: raw_profile,
        api_key: if is_default || pkey.is_empty() { dkey } else { pkey },
        route_prefix,
    }
}

/// JS `encodeURIComponent` unreserved set: A-Z a-z 0-9 - _ . ! ~ * ' ( )
fn percent_encode_segment(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for b in raw.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')') {
            out.push(*b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

pub fn build_gateway_url(cfg: &ProfileGatewayConfig, pathname: &str, unprefixed: bool) -> String {
    let clean = if pathname.starts_with('/') {
        pathname.to_string()
    } else {
        format!("/{pathname}")
    };
    let prefix = if unprefixed { "" } else { &cfg.route_prefix };
    format!("{}{prefix}{clean}", cfg.base_url)
}

pub fn sanitize_session_key_header(raw: Option<&str>) -> Option<String> {
    let s = raw?;
    let cleaned: String = s
        .chars()
        .filter(|&c| c != '\r' && c != '\n' && c != '\0')
        .collect::<String>()
        .trim()
        .to_string();
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned.chars().take(256).collect())
    }
}

pub fn create_idempotency_key(prefix: &str) -> String {
    let clean: String = prefix
        .chars()
        .map(|c| if ('!'..='~').contains(&c) { c } else { '-' })
        .take(32)
        .collect();
    let p = if clean.is_empty() { "voxcell" } else { clean.as_str() };
    let key = format!("{p}-{}", Uuid::new_v4());
    key.chars().take(255).collect()
}

// ---------------------------------------------------------------------------
// Capabilities (parseCapabilitiesPayload / chooseGatewayTransport)
// ---------------------------------------------------------------------------

/// Full TS `HermesCapabilities` parity (the core `GatewayCapabilities` struct is
/// a coarse adapter kept for `HermesGatewayInfo`; this one preserves features).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HermesCapabilities {
    pub object: String,
    pub platform: String,
    pub model: String,
    pub session_key_header: Option<String>,
    pub features: HashMap<String, bool>,
    pub endpoints: Option<HashMap<String, String>>,
}

impl HermesCapabilities {
    pub fn feature(&self, key: &str) -> bool {
        self.features.get(key).copied().unwrap_or(false)
    }
    /// TS: `f.chat_completions !== false` (defaults to supported when absent).
    pub fn supports_chat(&self) -> bool {
        self.features.get("chat_completions").copied().unwrap_or(true)
    }
    pub fn supports_runs(&self) -> bool {
        self.feature("run_submission") && self.feature("run_events_sse")
    }
    pub fn supports_session_stream(&self) -> bool {
        self.feature("session_chat_stream")
    }
}

pub fn parse_capabilities_full(raw: &Value) -> Option<HermesCapabilities> {
    let obj = raw.as_object()?;
    let mut features = HashMap::new();
    if let Some(f) = obj_get(obj, "features").and_then(|v| v.as_object()) {
        for (k, v) in f {
            features.insert(k.clone(), js_truthy(v));
        }
    }
    let mut endpoints = HashMap::new();
    if let Some(e) = obj_get(obj, "endpoints").and_then(|v| v.as_object()) {
        for (k, v) in e {
            if let Some(s) = v.as_str() {
                endpoints.insert(k.clone(), s.to_string());
            }
        }
    }
    Some(HermesCapabilities {
        object: string_opt(obj, "object")
            .unwrap_or_else(|| "hermes.api_server.capabilities".to_string()),
        platform: string_opt(obj, "platform").unwrap_or_else(|| "hermes-agent".to_string()),
        model: string_opt(obj, "model").unwrap_or_else(|| "hermes-agent".to_string()),
        session_key_header: string_opt(obj, "session_key_header"),
        features,
        endpoints: if endpoints.is_empty() { None } else { Some(endpoints) },
    })
}

/// Derive the coarse core `GatewayCapabilities` from full TS-parity capabilities.
pub fn capabilities_to_core(caps: &HermesCapabilities, raw: &Value) -> GatewayCapabilities {
    let flag = |k: &str, default: bool| caps.features.get(k).copied().unwrap_or(default);
    let profiles = raw
        .get("profiles")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|p| p.as_str().map(ToOwned::to_owned)).collect())
        .unwrap_or_default();
    GatewayCapabilities {
        runs: caps.supports_runs(),
        approvals: flag("run_approval", false),
        clarify: flag("run_clarify", false),
        steering: flag("run_steer", false),
        session_stream: caps.supports_session_stream(),
        chat_completions: caps.supports_chat(),
        skills: flag("skills", true),
        toolsets: flag("toolsets", true),
        cron_jobs: flag("cron_jobs", true),
        profiles,
    }
}

/// Legacy adapter: coarse capabilities + advertised model.
pub fn parse_capabilities_payload(raw: &Value) -> Option<(GatewayCapabilities, Option<String>)> {
    let full = parse_capabilities_full(raw)?;
    let model = raw.get("model").and_then(|v| v.as_str()).map(ToOwned::to_owned);
    Some((capabilities_to_core(&full, raw), model))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreferredTransport {
    Auto,
    Runs,
    SessionStream,
    ChatCompletions,
}

pub fn choose_gateway_transport(
    preferred: PreferredTransport,
    caps: Option<&HermesCapabilities>,
) -> GatewayTransport {
    let supports_runs = caps.is_some_and(|c| c.supports_runs());
    let supports_session = caps.is_some_and(|c| c.supports_session_stream());
    let supports_chat = caps.is_none_or(|c| c.supports_chat());
    let reasoning_streaming = caps.is_some_and(|c| c.feature("reasoning_streaming"));

    match preferred {
        PreferredTransport::Runs => {
            if supports_runs {
                GatewayTransport::Runs
            } else if supports_chat {
                GatewayTransport::ChatCompletions
            } else if supports_session {
                GatewayTransport::SessionStream
            } else {
                GatewayTransport::ChatCompletions
            }
        }
        PreferredTransport::SessionStream => {
            if supports_session {
                GatewayTransport::SessionStream
            } else if supports_runs {
                GatewayTransport::Runs
            } else {
                GatewayTransport::ChatCompletions
            }
        }
        PreferredTransport::ChatCompletions => {
            if supports_chat {
                GatewayTransport::ChatCompletions
            } else if supports_runs {
                GatewayTransport::Runs
            } else if supports_session {
                GatewayTransport::SessionStream
            } else {
                GatewayTransport::ChatCompletions
            }
        }
        PreferredTransport::Auto => {
            // TS: prefer Runs when run controls are advertised OR reasoning
            // streaming is absent; otherwise reasoning_streaming + chat wins.
            let run_controls = caps.is_some_and(|c| {
                c.feature("run_approval") || c.feature("run_stop") || !c.feature("reasoning_streaming")
            });
            if supports_runs && run_controls {
                GatewayTransport::Runs
            } else if reasoning_streaming && supports_chat {
                GatewayTransport::ChatCompletions
            } else if supports_runs {
                GatewayTransport::Runs
            } else if supports_session {
                GatewayTransport::SessionStream
            } else {
                GatewayTransport::ChatCompletions
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Model options (extractModelOptions)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelProviderInfo {
    pub id: String,
    pub label: String,
    pub configured: Option<bool>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelOptions {
    pub models: Vec<String>,
    pub active_model: Option<String>,
    pub active_provider: Option<String>,
    pub providers: Vec<ModelProviderInfo>,
}

pub fn extract_model_options(raw: &Value) -> ModelOptions {
    let Some(obj) = raw.as_object() else {
        return ModelOptions::default();
    };
    let mut models: Vec<String> = Vec::new();
    let mut providers: Vec<ModelProviderInfo> = Vec::new();

    fn push_model(models: &mut Vec<String>, m: &Value) {
        if let Some(s) = m.as_str() {
            let t = s.trim();
            if !t.is_empty() && !models.iter().any(|x| x == t) {
                models.push(t.to_string());
            }
        } else if let Some(mo) = m.as_object() {
            let id = truthy_string_opt(mo, &["id", "model", "name"]).unwrap_or_default();
            let t = id.trim();
            if !t.is_empty() && !models.iter().any(|x| x == t) {
                models.push(t.to_string());
            }
        }
    }

    for key in ["models", "data", "curated_models"] {
        if let Some(arr) = obj_get(obj, key).and_then(|v| v.as_array()) {
            for item in arr {
                push_model(&mut models, item);
            }
        }
    }

    let selected = obj_get(obj, "selected").and_then(|v| v.as_object());
    let active_model = truthy_string_opt(obj, &["current_model", "active_model", "model"])
        .or_else(|| selected.and_then(|s| truthy_string_opt(s, &["model"])))
        .or_else(|| models.first().cloned());
    let active_provider = truthy_string_opt(obj, &["current_provider", "active_provider", "provider"])
        .or_else(|| selected.and_then(|s| truthy_string_opt(s, &["provider"])));

    if let Some(provs) = obj_get(obj, "providers").and_then(|v| v.as_array()) {
        for p in provs {
            if let Some(s) = p.as_str() {
                let t = s.trim();
                if !t.is_empty() {
                    providers.push(ModelProviderInfo {
                        id: t.to_string(),
                        label: t.to_string(),
                        configured: Some(true),
                    });
                }
            } else if let Some(po) = p.as_object() {
                let Some(id) = truthy_string_opt(po, &["id", "slug", "name"]) else {
                    continue;
                };
                let label = truthy_string_opt(po, &["label"]).unwrap_or_else(|| id.clone());
                let configured = match po.get("configured") {
                    Some(v) => Some(js_truthy(v)),
                    None => po.get("authenticated").map(js_truthy),
                };
                providers.push(ModelProviderInfo { id, label, configured });
                if let Some(arr) = po.get("models").and_then(|v| v.as_array()) {
                    for pm in arr {
                        push_model(&mut models, pm);
                    }
                }
            }
        }
    }

    if let Some(active) = &active_model {
        if !models.iter().any(|m| m == active) {
            models.insert(0, active.clone());
        }
    }

    ModelOptions { models, active_model, active_provider, providers }
}

/// Adapter to the core `HermesModelOption` shape used by the gateway probe.
/// Provider attribution is preserved for models listed under a provider.
pub fn parse_model_options_payload(raw: &Value) -> (Vec<HermesModelOption>, Option<String>) {
    let opts = extract_model_options(raw);
    let mut attribution: HashMap<String, String> = HashMap::new();
    if let Some(provs) = raw.get("providers").and_then(|v| v.as_array()) {
        for p in provs {
            let Some(po) = p.as_object() else { continue };
            let Some(pid) = truthy_string_opt(po, &["id", "slug", "name"]) else { continue };
            if let Some(arr) = po.get("models").and_then(|v| v.as_array()) {
                for pm in arr {
                    let id = if let Some(s) = pm.as_str() {
                        s.trim().to_string()
                    } else if let Some(mo) = pm.as_object() {
                        truthy_string_opt(mo, &["id", "model", "name"])
                            .unwrap_or_default()
                            .trim()
                            .to_string()
                    } else {
                        continue;
                    };
                    if !id.is_empty() {
                        attribution.entry(id).or_insert_with(|| pid.clone());
                    }
                }
            }
        }
    }
    let models = opts
        .models
        .iter()
        .map(|id| HermesModelOption {
            id: id.clone(),
            label: id.clone(),
            provider: attribution.get(id).cloned(),
        })
        .collect();
    (models, opts.active_model)
}

// ---------------------------------------------------------------------------
// Skills / Toolsets / Cron jobs
// ---------------------------------------------------------------------------

/// TS `HermesSkill` parity record (core `HermesSkill` lacks `source`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewaySkillRecord {
    pub name: String,
    pub source: String,
    pub summary: String,
    pub category: Option<String>,
    pub enabled: bool,
}

pub fn map_gateway_skill_records(raw: &Value) -> Vec<GatewaySkillRecord> {
    let Some(items) = payload_list(raw, &["skills", "data"]) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let obj = item.as_object()?;
            let name = obj.get("name")?.as_str()?.trim();
            if name.is_empty() {
                return None;
            }
            let category = obj.get("category").and_then(|v| v.as_str()).map(|s| s.trim().to_string());
            let source = truthy_string_opt(obj, &["source", "path"])
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| match category.as_deref() {
                    Some(c) if !c.is_empty() => format!("category:{c}"),
                    _ => "gateway:/v1/skills".to_string(),
                });
            let summary = truthy_string_opt(obj, &["description", "summary"])
                .map(|s| s.trim().to_string())
                .unwrap_or_default();
            let enabled = match obj.get("enabled") {
                Some(v) => js_truthy(v),
                None => true,
            };
            Some(GatewaySkillRecord { name: name.to_string(), source, summary, category, enabled })
        })
        .collect()
}

pub fn map_gateway_skills(raw: &Value) -> Vec<HermesSkill> {
    map_gateway_skill_records(raw)
        .into_iter()
        .map(|r| HermesSkill { name: r.name, description: r.summary, category: r.category, enabled: r.enabled })
        .collect()
}

/// TS `HermesToolset` parity record (core type lacks `label`/`configured`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayToolsetRecord {
    pub name: String,
    pub label: String,
    pub description: String,
    pub enabled: bool,
    pub configured: bool,
    pub tools: Vec<String>,
}

pub fn map_gateway_toolset_records(raw: &Value) -> Vec<GatewayToolsetRecord> {
    let Some(items) = payload_list(raw, &["toolsets", "data"]) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            if let Some(s) = item.as_str() {
                let name = s.trim();
                if name.is_empty() {
                    return None;
                }
                return Some(GatewayToolsetRecord {
                    name: name.to_string(),
                    label: name.to_string(),
                    description: String::new(),
                    enabled: true,
                    configured: true,
                    tools: Vec::new(),
                });
            }
            let obj = item.as_object()?;
            let name = obj.get("name")?.as_str()?.trim();
            if name.is_empty() {
                return None;
            }
            let tools = obj
                .get("tools")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|t| t.as_str().map(ToOwned::to_owned)).collect())
                .unwrap_or_default();
            Some(GatewayToolsetRecord {
                name: name.to_string(),
                label: obj
                    .get("label")
                    .and_then(|v| v.as_str())
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| name.to_string()),
                description: obj
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim()
                    .to_string(),
                enabled: obj.get("enabled").map(js_truthy).unwrap_or(true),
                configured: obj.get("configured").map(js_truthy).unwrap_or(true),
                tools,
            })
        })
        .collect()
}

pub fn map_gateway_toolsets(raw: &Value) -> Vec<HermesToolset> {
    map_gateway_toolset_records(raw)
        .into_iter()
        .map(|r| HermesToolset { name: r.name, description: r.description, tools: r.tools, enabled: r.enabled })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CronJobMode {
    Agent,
    NoAgent,
}

/// TS `HermesCronJob` parity record (core type lacks mode/paused/status).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayCronJobRecord {
    pub id: String,
    pub name: Option<String>,
    pub schedule: String,
    pub channel_id: String,
    pub mode: CronJobMode,
    pub summary: String,
    pub paused: bool,
    pub enabled: bool,
    pub status: String,
    pub last_run_at: Option<String>,
    pub next_run_at: Option<String>,
    pub last_status: Option<String>,
}

pub fn map_gateway_job_records(raw: &Value) -> Vec<GatewayCronJobRecord> {
    let Some(items) = payload_list(raw, &["jobs", "data"]) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let obj = item.as_object()?;
            let id = truthy_string_opt(obj, &["id", "job_id", "name"])?;
            let paused = obj.get("paused").map(js_truthy).unwrap_or(false)
                || obj
                    .get("status")
                    .and_then(|v| v.as_str())
                    .is_some_and(|s| s.eq_ignore_ascii_case("paused"))
                || obj.get("enabled").and_then(|v| v.as_bool()) == Some(false);
            let enabled = match obj.get("enabled") {
                Some(v) => js_truthy(v),
                None => !paused,
            };
            let mode = if obj.get("command").map(js_truthy).unwrap_or(false)
                && !obj.get("prompt").map(js_truthy).unwrap_or(false)
            {
                CronJobMode::NoAgent
            } else {
                CronJobMode::Agent
            };
            let summary = truthy_string_opt(obj, &["prompt", "command", "name"])
                .unwrap_or_else(|| id.clone());
            Some(GatewayCronJobRecord {
                id: id.clone(),
                name: string_opt(obj, "name"),
                schedule: truthy_string_opt(obj, &["schedule", "cron", "schedule_display"])
                    .unwrap_or_else(|| "* * * * *".to_string()),
                channel_id: truthy_string_opt(obj, &["channel_id", "delivery_target", "channel"])
                    .unwrap_or_else(|| "general".to_string()),
                mode,
                summary,
                paused,
                enabled,
                status: string_opt(obj, "status")
                    .unwrap_or_else(|| if paused { "paused" } else { "active" }.to_string()),
                last_run_at: string_opt(obj, "last_run_at"),
                next_run_at: string_opt(obj, "next_run_at"),
                last_status: string_opt(obj, "last_status"),
            })
        })
        .collect()
}

pub fn map_gateway_jobs(raw: &Value) -> Vec<HermesCronJob> {
    map_gateway_job_records(raw)
        .into_iter()
        .map(|r| HermesCronJob {
            id: r.id,
            name: r.name.unwrap_or_default(),
            schedule: r.schedule,
            prompt: r.summary,
            channel_id: Some(r.channel_id),
            enabled: r.enabled,
            last_run_at: r.last_run_at,
            next_run_at: r.next_run_at,
            last_status: r.last_status,
        })
        .collect()
}

/// Shared list extraction: array, or `{ <key>: [...] }` / `{ data: [...] }`.
fn payload_list<'a>(raw: &'a Value, keys: &[&str]) -> Option<&'a Vec<Value>> {
    if let Some(arr) = raw.as_array() {
        return Some(arr);
    }
    for k in keys {
        if let Some(arr) = raw.get(*k).and_then(|v| v.as_array()) {
            return Some(arr);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Session origin classification (isOpaqueSessionIdentifier / classifyGatewaySessionOrigin)
// ---------------------------------------------------------------------------

fn is_hex(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_hexdigit())
}

/// Hand-rolled equivalent of the TS OPAQUE_* regexes (no regex dependency).
/// Patterns (case-insensitive, optional leading `#` and `hermes-`):
/// `local[-_:]<hex6+>`, `run[-_:]<hex6+>`, `cron[-_:]<any>`,
/// UUID 8-4-4-4-12, raw `<hex20+>`.
pub fn is_opaque_session_identifier(raw: Option<&str>) -> bool {
    let s = raw.unwrap_or("").trim();
    let s = s.strip_prefix('#').unwrap_or(s);
    if s.is_empty() {
        return false;
    }
    let lower = s.to_ascii_lowercase();
    let body = lower.strip_prefix("hermes-").unwrap_or(&lower);

    // local / run hash forms
    for prefix in ["local", "run"] {
        if let Some(rest) = body.strip_prefix(prefix) {
            if let Some(rest) = rest.strip_prefix(['-', '_', ':']) {
                if rest.len() >= 6 && is_hex(rest) {
                    return true;
                }
            }
        }
    }
    // cron id form
    if let Some(rest) = body.strip_prefix("cron") {
        if let Some(rest) = rest.strip_prefix(['-', '_', ':']) {
            if !rest.is_empty() {
                return true;
            }
        }
    }
    // UUID form
    let parts: Vec<&str> = body.split('-').collect();
    if parts.len() == 5
        && [8usize, 4, 4, 4, 12]
            .iter()
            .zip(parts.iter())
            .all(|(len, p)| p.len() == *len && is_hex(p))
    {
        return true;
    }
    // raw hex form
    if body.len() >= 20 && is_hex(body) {
        return true;
    }
    false
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SessionOriginParams<'a> {
    pub session_id: Option<&'a str>,
    pub session_key: Option<&'a str>,
    pub channel_id: Option<&'a str>,
    pub source: Option<&'a str>,
    pub created_in_workspace: bool,
}

pub fn classify_gateway_session_origin(params: SessionOriginParams<'_>) -> SessionOrigin {
    if params.created_in_workspace {
        return SessionOrigin::Workspace;
    }
    let key = params.session_key.unwrap_or("").trim().to_ascii_lowercase();
    if key.starts_with("voxcell-") {
        return SessionOrigin::Workspace;
    }
    let src = params.source.unwrap_or("").trim().to_ascii_lowercase();
    // "desktop" is a native-only extension of the web/voxcell/ui workspace set.
    if matches!(src.as_str(), "web" | "voxcell" | "ui" | "desktop") {
        return SessionOrigin::Workspace;
    }
    if matches!(
        src.as_str(),
        "cli" | "cron" | "scheduler" | "daemon" | "telegram" | "discord" | "slack" | "whatsapp" | "signal"
    ) {
        return SessionOrigin::External;
    }
    let sid = params.session_id.unwrap_or("").trim();
    let cid = params.channel_id.unwrap_or("").trim();
    if sid.to_ascii_lowercase().starts_with("hermes-")
        && !is_opaque_session_identifier(Some(sid))
        && !is_opaque_session_identifier(Some(cid))
    {
        return SessionOrigin::Workspace;
    }
    SessionOrigin::External
}

// ---------------------------------------------------------------------------
// Gateway sessions & messages (mapGatewaySessions / mapGatewaySessionMessages)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GatewaySessionRecord {
    pub id: String,
    pub session_key: Option<String>,
    pub title: Option<String>,
    pub source: Option<String>,
    pub model: Option<String>,
    pub parent_session_id: Option<String>,
    pub message_count: usize,
    pub last_message: Option<String>,
    pub created_at_ms: Option<u64>,
    pub updated_at_ms: Option<u64>,
    pub origin: SessionOrigin,
}

/// TS `toEpochMs`: epoch seconds (<1e10) scale to ms; numeric strings parse;
/// date strings go through `Date.parse` (RFC3339 / RFC2822 / naive ISO here).
pub fn to_epoch_ms(val: Option<&Value>) -> Option<u64> {
    let v = val?;
    let from_num = |n: f64| -> Option<u64> {
        if n.is_finite() && n >= 0.0 {
            Some(if n < 10_000_000_000.0 { (n * 1000.0).round() as u64 } else { n.round() as u64 })
        } else {
            None
        }
    };
    if let Some(n) = v.as_f64() {
        return from_num(n);
    }
    if let Some(s) = v.as_str() {
        let t = s.trim();
        if t.is_empty() {
            return None;
        }
        if let Ok(n) = t.parse::<f64>() {
            return from_num(n);
        }
        return parse_date_ms(t);
    }
    None
}

fn parse_date_ms(s: &str) -> Option<u64> {
    use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        let ms = dt.timestamp_millis();
        return u64::try_from(ms).ok();
    }
    if let Ok(dt) = DateTime::parse_from_rfc2822(s) {
        let ms = dt.timestamp_millis();
        return u64::try_from(ms).ok();
    }
    for fmt in ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%d %H:%M:%S%.f", "%Y-%m-%d"] {
        if let Ok(ndt) = NaiveDateTime::parse_from_str(s, fmt) {
            let ms = Utc.from_utc_datetime(&ndt).timestamp_millis();
            return u64::try_from(ms).ok();
        }
    }
    // Date-only strings parse as midnight UTC above; NaiveDate fallback:
    if let Ok(nd) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        let ms = Utc.from_utc_datetime(&nd.and_hms_opt(0, 0, 0)?).timestamp_millis();
        return u64::try_from(ms).ok();
    }
    None
}

pub fn map_gateway_sessions(raw: &Value) -> Vec<GatewaySessionRecord> {
    let Some(items) = payload_list(raw, &["sessions", "data"]) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let obj = item.as_object()?;
            let id = truthy_string_opt(obj, &["session_id", "id"])?;
            let id = id.trim();
            if id.is_empty() {
                return None;
            }
            let session_key = truthy_string_opt(obj, &["session_key", "sessionKey"])
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            let last_message = truthy_string_opt(obj, &["last_message", "preview", "first_prompt"])
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            let source = string_opt(obj, "source");
            let origin = classify_gateway_session_origin(SessionOriginParams {
                session_id: Some(id),
                session_key: session_key.as_deref(),
                channel_id: None,
                source: source.as_deref(),
                created_in_workspace: false,
            });
            Some(GatewaySessionRecord {
                id: id.to_string(),
                session_key,
                title: string_opt(obj, "title"),
                source,
                model: string_opt(obj, "model"),
                parent_session_id: string_opt(obj, "parent_session_id"),
                message_count: obj.get("message_count").and_then(|v| v.as_u64()).unwrap_or(0) as usize,
                last_message,
                created_at_ms: to_epoch_ms(obj.get("created_at").or_else(|| obj.get("started_at"))),
                updated_at_ms: to_epoch_ms(
                    obj.get("updated_at")
                        .or_else(|| obj.get("last_active_at"))
                        .or_else(|| obj.get("created_at")),
                ),
                origin,
            })
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionMessageRole {
    User,
    Assistant,
    System,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GatewaySessionMessage {
    pub id: String,
    pub role: SessionMessageRole,
    pub text: String,
    pub reasoning: Option<String>,
    pub created_at_ms: u64,
}

pub fn map_gateway_session_messages(raw: &Value, session_id: &str) -> Vec<GatewaySessionMessage> {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    map_gateway_session_messages_at(raw, session_id, now_ms)
}

/// Deterministic variant of `mapGatewaySessionMessages` (`now_ms` replaces `Date.now()`).
pub fn map_gateway_session_messages_at(raw: &Value, session_id: &str, now_ms: u64) -> Vec<GatewaySessionMessage> {
    let Some(items) = payload_list(raw, &["messages", "data"]) else {
        return Vec::new();
    };
    let base_ts = now_ms.saturating_sub(items.len() as u64 * 1000);
    let mut out = Vec::new();
    for (idx, item) in items.iter().enumerate() {
        let Some(obj) = item.as_object() else { continue };
        let raw_role = obj
            .get("role")
            .map(js_string)
            .unwrap_or_else(|| "assistant".to_string())
            .to_ascii_lowercase();
        if raw_role == "tool" || raw_role == "developer" {
            continue;
        }
        let role = match raw_role.as_str() {
            "user" => SessionMessageRole::User,
            "system" => SessionMessageRole::System,
            _ => SessionMessageRole::Assistant,
        };

        let mut text = String::new();
        if let Some(s) = obj.get("content").and_then(|v| v.as_str()) {
            text = s.to_string();
        } else if let Some(arr) = obj.get("content").and_then(|v| v.as_array()) {
            let parts: Vec<String> = arr
                .iter()
                .filter_map(|part| {
                    if let Some(s) = part.as_str() {
                        return Some(s.to_string());
                    }
                    let p = part.as_object()?;
                    if let Some(t) = p.get("text").and_then(|v| v.as_str()) {
                        return Some(t.to_string());
                    }
                    let ty = p.get("type").and_then(|v| v.as_str()).unwrap_or("");
                    if ty == "input_image" || ty == "image_url" {
                        return Some("[image]".to_string());
                    }
                    Some(String::new())
                })
                .filter(|s| !s.is_empty())
                .collect();
            text = parts.join("\n");
        } else if let Some(s) = obj.get("text").and_then(|v| v.as_str()) {
            text = s.to_string();
        }

        let reasoning = ["reasoning_content", "reasoning"]
            .iter()
            .filter_map(|k| obj.get(*k).and_then(|v| v.as_str()))
            .find(|s| !s.trim().is_empty())
            .map(ToOwned::to_owned);

        if text.trim().is_empty() && reasoning.is_none() {
            continue;
        }
        let id = truthy_string_opt(obj, &["id"])
            .unwrap_or_else(|| format!("gw-msg-{session_id}-{idx}"));
        let created_at_ms = to_epoch_ms(obj.get("created_at").or_else(|| obj.get("timestamp")))
            .unwrap_or(base_ts + idx as u64 * 1000);
        out.push(GatewaySessionMessage {
            id,
            role,
            text: text.trim().to_string(),
            reasoning,
            created_at_ms,
        });
    }
    out
}

// ---------------------------------------------------------------------------
// SSE Line Decoder & shared event types
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SseFrame {
    pub event: String,
    pub data: String,
}

#[derive(Default)]
pub struct SseLineDecoder {
    buffer: String,
    current_event: String,
    data_lines: Vec<String>,
    pub keepalive_count: usize,
}

impl SseLineDecoder {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            current_event: "message".to_string(),
            data_lines: Vec::new(),
            keepalive_count: 0,
        }
    }

    pub fn push(&mut self, chunk: &str) -> Vec<SseFrame> {
        self.buffer.push_str(chunk);
        let mut frames = Vec::new();
        while let Some(nl_idx) = self.buffer.find('\n') {
            let line = self.buffer[..nl_idx].strip_suffix('\r').unwrap_or(&self.buffer[..nl_idx]).to_string();
            self.buffer.drain(..=nl_idx);

            if line.is_empty() {
                if !self.data_lines.is_empty() {
                    frames.push(SseFrame {
                        event: if self.current_event.is_empty() { "message".to_string() } else { self.current_event.clone() },
                        data: self.data_lines.join("\n"),
                    });
                }
                self.current_event = "message".to_string();
                self.data_lines.clear();
                continue;
            }

            // Per SSE spec: comment lines (`: keepalive`, `: ping`) are ignored.
            if line.starts_with(':') {
                self.keepalive_count += 1;
                continue;
            }
            if let Some(rest) = line.strip_prefix("event:") {
                let ev = rest.trim();
                self.current_event = if ev.is_empty() { "message".to_string() } else { ev.to_string() };
                continue;
            }
            if let Some(rest) = line.strip_prefix("data:") {
                // TS uses trimStart() — strip ALL leading whitespace.
                self.data_lines.push(rest.trim_start().to_string());
            }
        }
        frames
    }

    pub fn flush(&mut self) -> Vec<SseFrame> {
        let mut frames = Vec::new();
        if !self.buffer.trim().is_empty() {
            let tail = self.buffer.strip_suffix('\r').unwrap_or(&self.buffer).to_string();
            self.buffer.clear();
            if tail.starts_with(':') {
                self.keepalive_count += 1;
            } else if let Some(rest) = tail.strip_prefix("data:") {
                self.data_lines.push(rest.trim_start().to_string());
            }
        }
        if !self.data_lines.is_empty() {
            frames.push(SseFrame {
                event: if self.current_event.is_empty() { "message".to_string() } else { self.current_event.clone() },
                data: self.data_lines.join("\n"),
            });
        }
        self.current_event = "message".to_string();
        self.data_lines.clear();
        frames
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParsedToolEvent {
    pub phase: String,
    pub tool_name: String,
    pub label: String,
    pub status: String,
    pub summary: String,
    pub duration_sec: Option<f64>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedApprovalRequest {
    pub request_id: String,
    pub execution_generation: u32,
    pub kind: ApprovalKind,
    pub title: String,
    pub tool_name: Option<String>,
    pub command: String,
    pub summary: String,
    pub destination: String,
    pub choices: Vec<String>,
}

/// Signal vocabulary shared by all three SSE parsers. NOTE: voxcell-ui matches
/// these variants exhaustively — do not add variants without updating it.
#[derive(Clone, Debug, PartialEq)]
pub enum StreamSignal {
    ContentDelta { accumulated: String, chunk: String },
    ReasoningDelta { accumulated: String, chunk: String },
    Interim { text: String, already_streamed: bool },
    ToolEvent(ParsedToolEvent),
    ApprovalRequest(ParsedApprovalRequest),
    SubagentStart { summary: String, child_session_id: Option<String>, delegation_id: Option<String> },
    SubagentComplete { status: String, summary: String, child_session_id: Option<String>, delegation_id: Option<String> },
    Steered(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StreamTerminalResult {
    pub output: String,
    pub reasoning: String,
    pub outcome: TerminalOutcome,
    pub error: Option<String>,
    pub error_code: Option<String>,
    pub model: Option<String>,
    pub finish_reason: Option<String>,
    pub session_id: Option<String>,
    pub session_key: Option<String>,
    pub admission_conflict: bool,
    pub partial: bool,
    pub interrupted: bool,
}

impl StreamTerminalResult {
    pub fn completed(output: impl Into<String>) -> Self {
        Self {
            output: output.into(),
            reasoning: String::new(),
            outcome: TerminalOutcome::Completed,
            error: None,
            error_code: None,
            model: None,
            finish_reason: None,
            session_id: None,
            session_key: None,
            admission_conflict: false,
            partial: false,
            interrupted: false,
        }
    }
}

pub type TerminalTurnResultKind = TerminalOutcome;

/// TS `classifyFinishReason` — admission_conflict wins first (case-insensitive,
/// matched on either error message or code), then finish_reason states.
pub fn classify_finish_reason(
    finish_reason: Option<&str>,
    error_code: Option<&str>,
    error_message: Option<&str>,
) -> TerminalOutcome {
    let err_lower = error_message.unwrap_or("").to_ascii_lowercase();
    let code_lower = error_code.unwrap_or("").to_ascii_lowercase();
    if err_lower == "admission_conflict" || code_lower == "admission_conflict" {
        return TerminalOutcome::AdmissionConflict;
    }
    match finish_reason {
        Some("cancelled") => TerminalOutcome::Cancelled,
        Some("error") => TerminalOutcome::Failed,
        Some("length") if code_lower == "iteration_budget_exhausted" => {
            TerminalOutcome::IterationBudgetExhausted
        }
        _ => TerminalOutcome::Completed,
    }
}

/// TS `ChatCompletionsParsedResult` (non-streaming JSON response).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatCompletionsParsedResult {
    pub content: String,
    pub reasoning: String,
    pub finish_reason: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub outcome: TerminalOutcome,
    pub session_id: Option<String>,
    pub session_key: Option<String>,
    pub admission_conflict: bool,
}

pub fn parse_chat_completions_json(raw: &Value) -> Option<ChatCompletionsParsedResult> {
    let obj = raw.as_object()?;
    let first = obj
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|a| a.first())
        .and_then(|v| v.as_object());
    let msg = first
        .and_then(|f| f.get("message"))
        .and_then(|v| v.as_object());
    let hermes_meta = obj.get("hermes").and_then(|v| v.as_object());
    let err_meta = obj.get("error").and_then(|v| v.as_object());

    let content = msg.and_then(|m| m.get("content")).and_then(|v| v.as_str()).unwrap_or("").to_string();
    let reasoning = msg
        .and_then(|m| m.get("reasoning_content"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let finish_reason = first.and_then(|f| f.get("finish_reason")).and_then(|v| v.as_str()).map(ToOwned::to_owned);
    let error_code = hermes_meta
        .and_then(|h| truthy_string_opt(h, &["error_code"]))
        .or_else(|| err_meta.and_then(|e| truthy_string_opt(e, &["code"])));
    let error_message = err_meta.and_then(|e| string_opt(e, "message"));
    let outcome = classify_finish_reason(
        finish_reason.as_deref(),
        error_code.as_deref(),
        error_message.as_deref(),
    );
    Some(ChatCompletionsParsedResult {
        content,
        reasoning,
        finish_reason,
        error_code,
        error_message,
        outcome,
        session_id: string_opt(obj, "session_id"),
        session_key: string_opt(obj, "session_key"),
        admission_conflict: outcome == TerminalOutcome::AdmissionConflict,
    })
}

/// Shared event-type resolution: payload.type || payload.event || SSE event line.
fn frame_event_type<'a>(obj: &'a serde_json::Map<String, Value>, frame: &'a SseFrame) -> &'a str {
    for key in ["type", "event"] {
        if let Some(Value::String(s)) = obj.get(key) {
            if !s.is_empty() {
                return s.as_str();
            }
        }
    }
    &frame.event
}

// ---------------------------------------------------------------------------
// RunsEventStreamParser — POST /v1/runs + GET /v1/runs/{id}/events
// ---------------------------------------------------------------------------

pub struct RunsEventStreamParser {
    sse: SseLineDecoder,
    content: String,
    reasoning: String,
    final_output: Option<String>,
    outcome: Option<TerminalOutcome>,
    error: Option<String>,
    error_code: Option<String>,
    model: Option<String>,
}

impl Default for RunsEventStreamParser {
    fn default() -> Self {
        Self::new()
    }
}

impl RunsEventStreamParser {
    pub fn new() -> Self {
        Self {
            sse: SseLineDecoder::new(),
            content: String::new(),
            reasoning: String::new(),
            final_output: None,
            outcome: None,
            error: None,
            error_code: None,
            model: None,
        }
    }

    pub fn push(&mut self, chunk: &str) -> Vec<StreamSignal> {
        let mut signals = Vec::new();
        for frame in self.sse.push(chunk) {
            self.handle_frame(&frame, &mut signals);
        }
        signals
    }

    pub fn finish(&mut self) -> (Vec<StreamSignal>, StreamTerminalResult) {
        let mut signals = Vec::new();
        for frame in self.sse.flush() {
            self.handle_frame(&frame, &mut signals);
        }
        // A terminal event with an empty `output` must not erase text streamed
        // via message.delta frames — only a non-empty authoritative answer wins.
        let output = self
            .final_output
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| self.content.clone());
        let outcome = self.outcome.unwrap_or_else(|| {
            if self.error.as_deref().is_some_and(|e| !e.is_empty()) {
                TerminalOutcome::Failed
            } else {
                TerminalOutcome::Completed
            }
        });
        (
            signals,
            StreamTerminalResult {
                output,
                reasoning: self.reasoning.clone(),
                outcome,
                error: self.error.clone(),
                error_code: self.error_code.clone(),
                model: self.model.clone(),
                finish_reason: None,
                session_id: None,
                session_key: None,
                admission_conflict: outcome == TerminalOutcome::AdmissionConflict,
                partial: false,
                interrupted: outcome == TerminalOutcome::Interrupted,
            },
        )
    }

    fn handle_frame(&mut self, frame: &SseFrame, out: &mut Vec<StreamSignal>) {
        let trimmed = frame.data.trim();
        if trimmed.is_empty() || trimmed == "[DONE]" {
            return;
        }
        let Ok(payload) = serde_json::from_str::<Value>(trimmed) else {
            return;
        };
        let Some(obj) = payload.as_object() else {
            return;
        };
        let ev_type = frame_event_type(obj, frame);

        match ev_type {
            "message.reasoning" | "reasoning.delta" => {
                let chunk = truthy_str_chain(obj, &["delta", "text"]);
                if !chunk.is_empty() {
                    self.reasoning.push_str(&chunk);
                    out.push(StreamSignal::ReasoningDelta {
                        accumulated: self.reasoning.clone(),
                        chunk,
                    });
                }
            }
            // "assistant.delta" is a native-side alias (SessionChat wire shape);
            // the TS Runs parser only sees message.* — kept additive.
            "message.delta" | "assistant.delta" => {
                if let Some(rc) = obj.get("reasoning_content").and_then(|v| v.as_str()) {
                    if !rc.is_empty() {
                        self.reasoning.push_str(rc);
                        out.push(StreamSignal::ReasoningDelta {
                            accumulated: self.reasoning.clone(),
                            chunk: rc.to_string(),
                        });
                    }
                }
                let chunk = truthy_str_chain(obj, &["delta", "text", "content"]);
                if !chunk.is_empty() {
                    self.content.push_str(&chunk);
                    out.push(StreamSignal::ContentDelta {
                        accumulated: self.content.clone(),
                        chunk,
                    });
                }
            }
            "message.interim" | "assistant.commentary" => {
                if let Some(text) = obj.get("text").and_then(|v| v.as_str()) {
                    if !text.trim().is_empty() {
                        out.push(StreamSignal::Interim {
                            text: text.to_string(),
                            already_streamed: obj.get("already_streamed").map(js_truthy).unwrap_or(false),
                        });
                    }
                }
            }
            "tool.started" => {
                let tool_name = {
                    let n = truthy_str_chain(obj, &["tool", "name"]);
                    if n.is_empty() { "tool".to_string() } else { n }
                };
                let preview = truthy_str_chain(obj, &["preview", "args_preview", "args"]).trim().to_string();
                out.push(StreamSignal::ToolEvent(ParsedToolEvent {
                    phase: "started".to_string(),
                    label: if preview.is_empty() { tool_name.clone() } else { format!("{tool_name}: {preview}") },
                    summary: if preview.is_empty() { format!("Running {tool_name}") } else { format!("{tool_name}: {preview}") },
                    tool_name,
                    status: "running".to_string(),
                    duration_sec: None,
                    error: None,
                }));
            }
            "tool.completed" | "tool.failed" => {
                let tool_name = {
                    let n = truthy_str_chain(obj, &["tool", "name"]);
                    if n.is_empty() { "tool".to_string() } else { n }
                };
                let err_str = obj.get("error").and_then(|v| v.as_str()).map(ToOwned::to_owned);
                let is_err = ev_type == "tool.failed" || obj.get("error").map(js_truthy).unwrap_or(false);
                let mut preview = truthy_str_chain(obj, &["preview", "result"]);
                if preview.is_empty() {
                    if let Some(e) = &err_str {
                        preview = e.clone();
                    }
                }
                let preview = preview.trim().to_string();
                // TS: error = string error, else (isErr ? preview || 'error' : undefined)
                let error = err_str.or_else(|| {
                    if is_err {
                        Some(if preview.is_empty() { "error".to_string() } else { preview.clone() })
                    } else {
                        None
                    }
                });
                out.push(StreamSignal::ToolEvent(ParsedToolEvent {
                    phase: if is_err { "failed" } else { "completed" }.to_string(),
                    label: if preview.is_empty() { tool_name.clone() } else { format!("{tool_name}: {preview}") },
                    summary: if preview.is_empty() {
                        format!("{tool_name} {}", if is_err { "failed" } else { "completed" })
                    } else {
                        format!("{tool_name}: {preview}")
                    },
                    tool_name,
                    status: if is_err { "failed" } else { "done" }.to_string(),
                    duration_sec: obj.get("duration").and_then(|v| v.as_f64()),
                    error,
                }));
            }
            "approval.request" | "clarify.request" => {
                let Some(request_id) = truthy_string_opt(obj, &["request_id", "prompt_id"]) else {
                    return;
                };
                let tool_name = truthy_string_opt(obj, &["tool", "tool_name"]);
                let command = truthy_string_opt(obj, &["command", "args_preview", "draft"]).unwrap_or_default();
                // TS: Number(payload.execution_generation ?? 1), finite else 1.
                // `??` treats null as absent -> default 1.
                let generation = obj
                    .get("execution_generation")
                    .filter(|v| !v.is_null())
                    .and_then(js_number)
                    .filter(|n| n.is_finite())
                    .unwrap_or(1.0);
                // TS keeps an explicitly empty choices array; default only when absent.
                let choices = match obj.get("choices").and_then(|v| v.as_array()) {
                    Some(a) => a.iter().filter_map(|x| x.as_str().map(ToOwned::to_owned)).collect(),
                    None => vec!["once".to_string(), "always".to_string(), "deny".to_string()],
                };
                let title = truthy_string_opt(obj, &["title"]).unwrap_or_else(|| {
                    tool_name
                        .as_ref()
                        .map(|t| format!("Approve {t}"))
                        .unwrap_or_else(|| "Approval required".to_string())
                });
                let summary = truthy_string_opt(obj, &["description", "summary"]).unwrap_or_else(|| {
                    if command.is_empty() {
                        "Hermes run paused awaiting approval.".to_string()
                    } else {
                        format!("Execute: {command}")
                    }
                });
                let destination = truthy_string_opt(obj, &["destination"])
                    .or_else(|| tool_name.clone())
                    .unwrap_or_else(|| "hermes-run".to_string());
                out.push(StreamSignal::ApprovalRequest(ParsedApprovalRequest {
                    request_id,
                    execution_generation: generation as u32,
                    kind: if ev_type == "clarify.request" { ApprovalKind::Clarify } else { ApprovalKind::Approval },
                    title,
                    tool_name,
                    command,
                    summary,
                    destination,
                    choices,
                }));
            }
            "subagent.start" => {
                let summary = truthy_string_opt(obj, &["summary", "task"])
                    .unwrap_or_else(|| "Subagent spawned".to_string());
                out.push(StreamSignal::SubagentStart {
                    summary,
                    child_session_id: string_opt(obj, "child_session_id"),
                    delegation_id: string_opt(obj, "delegation_id"),
                });
            }
            "subagent.complete" => {
                let status = {
                    let s = truthy_str_chain(obj, &["status"]);
                    if s.is_empty() { "completed".to_string() } else { s }
                };
                let summary = truthy_string_opt(obj, &["summary"])
                    .unwrap_or_else(|| format!("Subagent {status}"));
                out.push(StreamSignal::SubagentComplete {
                    status,
                    summary,
                    child_session_id: string_opt(obj, "child_session_id"),
                    delegation_id: string_opt(obj, "delegation_id"),
                });
            }
            "run.steered" => {
                if let Some(t) = obj.get("text").and_then(|v| v.as_str()) {
                    if !t.is_empty() {
                        out.push(StreamSignal::Steered(t.to_string()));
                    }
                }
            }
            "run.completed" => {
                if let Some(output) = obj.get("output").and_then(|v| v.as_str()) {
                    self.final_output = Some(output.to_string());
                }
                if self.reasoning.is_empty() {
                    if let Some(r) = obj.get("reasoning_content").and_then(|v| v.as_str()) {
                        self.reasoning = r.to_string();
                    }
                }
                let ec = string_opt(obj, "error_code");
                if ec.as_deref() == Some("iteration_budget_exhausted") {
                    self.error_code = ec;
                    self.outcome = Some(TerminalOutcome::IterationBudgetExhausted);
                } else {
                    self.outcome = Some(TerminalOutcome::Completed);
                }
                if let Some(m) = obj.get("runtime").and_then(|r| r.get("model")).and_then(|v| v.as_str()) {
                    self.model = Some(m.to_string());
                }
            }
            "run.failed" => {
                self.outcome = Some(TerminalOutcome::Failed);
                self.error = Some(
                    obj.get("error").and_then(|v| v.as_str()).unwrap_or("Run failed").to_string(),
                );
                // TS only overwrites errorCode when a string is present.
                if let Some(ec) = string_opt(obj, "error_code") {
                    self.error_code = Some(ec);
                }
            }
            "run.cancelled" => {
                self.outcome = Some(TerminalOutcome::Cancelled);
                self.error = string_opt(obj, "error");
            }
            "run.interrupted" => {
                self.outcome = Some(TerminalOutcome::Interrupted);
                self.error = Some(
                    obj.get("error")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Gateway shutdown interrupted the run.")
                        .to_string(),
                );
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// ChatCompletionsStreamParser — POST /v1/chat/completions (stream: true)
// ---------------------------------------------------------------------------

pub struct ChatCompletionsStreamParser {
    sse: SseLineDecoder,
    content: String,
    reasoning: String,
    finish_reason: Option<String>,
    error_code: Option<String>,
    error_message: Option<String>,
    session_id: Option<String>,
    session_key: Option<String>,
}

impl Default for ChatCompletionsStreamParser {
    fn default() -> Self {
        Self::new()
    }
}

impl ChatCompletionsStreamParser {
    pub fn new() -> Self {
        Self {
            sse: SseLineDecoder::new(),
            content: String::new(),
            reasoning: String::new(),
            finish_reason: None,
            error_code: None,
            error_message: None,
            session_id: None,
            session_key: None,
        }
    }

    pub fn push(&mut self, chunk: &str) -> Vec<StreamSignal> {
        let mut signals = Vec::new();
        for frame in self.sse.push(chunk) {
            self.handle_frame(&frame, &mut signals);
        }
        signals
    }

    pub fn finish(&mut self) -> (Vec<StreamSignal>, StreamTerminalResult) {
        let mut signals = Vec::new();
        for frame in self.sse.flush() {
            self.handle_frame(&frame, &mut signals);
        }
        let outcome = classify_finish_reason(
            self.finish_reason.as_deref(),
            self.error_code.as_deref(),
            self.error_message.as_deref(),
        );
        (
            signals,
            StreamTerminalResult {
                output: self.content.clone(),
                reasoning: self.reasoning.clone(),
                outcome,
                error: self.error_message.clone(),
                error_code: self.error_code.clone(),
                model: None,
                finish_reason: self.finish_reason.clone(),
                session_id: self.session_id.clone(),
                session_key: self.session_key.clone(),
                admission_conflict: outcome == TerminalOutcome::AdmissionConflict,
                partial: false,
                interrupted: false,
            },
        )
    }

    fn handle_frame(&mut self, frame: &SseFrame, out: &mut Vec<StreamSignal>) {
        let trimmed = frame.data.trim();
        if trimmed.is_empty() || trimmed == "[DONE]" {
            return;
        }
        let Ok(payload) = serde_json::from_str::<Value>(trimmed) else {
            return;
        };
        let Some(obj) = payload.as_object() else {
            return;
        };
        if let Some(sid) = string_opt(obj, "session_id") {
            self.session_id = Some(sid);
        }
        if let Some(skey) = string_opt(obj, "session_key") {
            self.session_key = Some(skey);
        }

        if frame.event == "hermes.tool.progress" {
            let tool_name = {
                let n = truthy_str_chain(obj, &["tool", "name", "tool_name"]);
                if n.is_empty() { "tool".to_string() } else { n }
            };
            let status = {
                let s = truthy_str_chain(obj, &["status"]);
                if s.is_empty() { "running".to_string() } else { s }
            };
            let detail = truthy_str_chain(obj, &["summary", "label", "message", "args"]).trim().to_string();
            let lower_s = status.to_ascii_lowercase();
            let phase = if lower_s == "failed" || lower_s == "error" {
                "failed"
            } else if lower_s == "done" || lower_s == "completed" {
                "completed"
            } else {
                "progress"
            };
            out.push(StreamSignal::ToolEvent(ParsedToolEvent {
                phase: phase.to_string(),
                label: if detail.is_empty() { tool_name.clone() } else { format!("{tool_name}: {detail}") },
                summary: if detail.is_empty() { format!("Running {tool_name}") } else { format!("{tool_name}: {detail}") },
                tool_name,
                status,
                duration_sec: None,
                error: None,
            }));
            return;
        }

        if let Some(h) = obj.get("hermes").and_then(|v| v.as_object()) {
            if let Some(ec) = string_opt(h, "error_code") {
                self.error_code = Some(ec);
            }
        }
        if let Some(err) = obj.get("error").and_then(|v| v.as_object()) {
            if let Some(c) = string_opt(err, "code") {
                self.error_code = Some(c);
            }
            if let Some(m) = string_opt(err, "message") {
                self.error_message = Some(m);
            }
        }

        let Some(first) = obj
            .get("choices")
            .and_then(|c| c.as_array())
            .and_then(|a| a.first())
            .and_then(|v| v.as_object())
        else {
            return;
        };
        // TS: truthy string only — an empty finish_reason does not overwrite.
        if let Some(fr) = first.get("finish_reason").and_then(|v| v.as_str()) {
            if !fr.is_empty() {
                self.finish_reason = Some(fr.to_string());
            }
        }
        // TS: first.delta when truthy object, else first.message when truthy object.
        let delta = ["delta", "message"]
            .iter()
            .filter_map(|k| first.get(*k))
            .find(|v| js_truthy(v) && v.is_object())
            .and_then(|v| v.as_object());
        if let Some(d) = delta {
            let rc = d
                .get("reasoning_content")
                .and_then(|v| v.as_str())
                .or_else(|| d.get("reasoning").and_then(|v| v.as_str()))
                .unwrap_or("");
            if !rc.is_empty() {
                self.reasoning.push_str(rc);
                out.push(StreamSignal::ReasoningDelta {
                    accumulated: self.reasoning.clone(),
                    chunk: rc.to_string(),
                });
            }
            if let Some(cc) = d.get("content").and_then(|v| v.as_str()) {
                if !cc.is_empty() {
                    self.content.push_str(cc);
                    out.push(StreamSignal::ContentDelta {
                        accumulated: self.content.clone(),
                        chunk: cc.to_string(),
                    });
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// SessionChatStreamParser — POST /api/sessions/{id}/chat/stream
// ---------------------------------------------------------------------------

pub struct SessionChatStreamParser {
    sse: SseLineDecoder,
    content: String,
    outcome: Option<TerminalOutcome>,
    error: Option<String>,
    partial: bool,
    interrupted: bool,
}

impl Default for SessionChatStreamParser {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionChatStreamParser {
    pub fn new() -> Self {
        Self {
            sse: SseLineDecoder::new(),
            content: String::new(),
            outcome: None,
            error: None,
            partial: false,
            interrupted: false,
        }
    }

    pub fn push(&mut self, chunk: &str) -> Vec<StreamSignal> {
        let mut signals = Vec::new();
        for frame in self.sse.push(chunk) {
            self.handle_frame(&frame, &mut signals);
        }
        signals
    }

    pub fn finish(&mut self) -> (Vec<StreamSignal>, StreamTerminalResult) {
        let mut signals = Vec::new();
        for frame in self.sse.flush() {
            self.handle_frame(&frame, &mut signals);
        }
        let outcome = self.outcome.unwrap_or_else(|| {
            if self.interrupted {
                TerminalOutcome::Cancelled
            } else if self.error.as_deref().is_some_and(|e| !e.is_empty()) {
                TerminalOutcome::Failed
            } else {
                TerminalOutcome::Completed
            }
        });
        (
            signals,
            StreamTerminalResult {
                output: self.content.clone(),
                reasoning: String::new(),
                outcome,
                error: self.error.clone(),
                error_code: None,
                model: None,
                finish_reason: None,
                session_id: None,
                session_key: None,
                admission_conflict: false,
                partial: self.partial,
                interrupted: self.interrupted,
            },
        )
    }

    fn handle_frame(&mut self, frame: &SseFrame, out: &mut Vec<StreamSignal>) {
        let trimmed = frame.data.trim();
        if trimmed.is_empty() || trimmed == "[DONE]" {
            return;
        }
        let Ok(payload) = serde_json::from_str::<Value>(trimmed) else {
            return;
        };
        let Some(obj) = payload.as_object() else {
            return;
        };
        let ev_type = frame_event_type(obj, frame);

        match ev_type {
            "assistant.delta" => {
                let delta = truthy_str_chain(obj, &["delta", "text"]);
                if !delta.is_empty() {
                    self.content.push_str(&delta);
                    out.push(StreamSignal::ContentDelta {
                        accumulated: self.content.clone(),
                        chunk: delta,
                    });
                }
            }
            "assistant.commentary" => {
                if let Some(text) = obj.get("text").and_then(|v| v.as_str()) {
                    if !text.trim().is_empty() {
                        out.push(StreamSignal::Interim {
                            text: text.to_string(),
                            already_streamed: obj.get("already_streamed").map(js_truthy).unwrap_or(false),
                        });
                    }
                }
            }
            "assistant.completed" => {
                // A finalization frame is NOT proof of success on its own.
                if let Some(c) = obj.get("content").and_then(|v| v.as_str()) {
                    if self.content.is_empty() {
                        self.content = c.to_string();
                    }
                }
                if let Some(p) = obj.get("partial") {
                    self.partial = js_truthy(p);
                }
                if let Some(i) = obj.get("interrupted") {
                    self.interrupted = js_truthy(i);
                }
            }
            "tool.started" => {
                let tool_name = {
                    let n = truthy_str_chain(obj, &["tool", "name"]);
                    if n.is_empty() { "tool".to_string() } else { n }
                };
                let preview = truthy_str_chain(obj, &["args_preview", "preview", "args", "summary"]).trim().to_string();
                out.push(StreamSignal::ToolEvent(ParsedToolEvent {
                    phase: "started".to_string(),
                    label: if preview.is_empty() { tool_name.clone() } else { format!("{tool_name}: {preview}") },
                    summary: if preview.is_empty() { format!("Running {tool_name}") } else { format!("{tool_name}: {preview}") },
                    tool_name,
                    status: "running".to_string(),
                    duration_sec: None,
                    error: None,
                }));
            }
            "tool.completed" | "tool.failed" => {
                let tool_name = {
                    let n = truthy_str_chain(obj, &["tool", "name"]);
                    if n.is_empty() { "tool".to_string() } else { n }
                };
                let is_err = ev_type == "tool.failed" || obj.get("error").map(js_truthy).unwrap_or(false);
                let preview = truthy_str_chain(obj, &["preview", "result", "error"]).trim().to_string();
                out.push(StreamSignal::ToolEvent(ParsedToolEvent {
                    phase: if is_err { "failed" } else { "completed" }.to_string(),
                    label: if preview.is_empty() { tool_name.clone() } else { format!("{tool_name}: {preview}") },
                    summary: if preview.is_empty() {
                        format!("{tool_name} {}", if is_err { "failed" } else { "completed" })
                    } else {
                        format!("{tool_name}: {preview}")
                    },
                    tool_name,
                    status: if is_err { "failed" } else { "done" }.to_string(),
                    duration_sec: obj.get("duration").and_then(|v| v.as_f64()),
                    // TS SessionChat: only a string error is surfaced (no synthesis).
                    error: obj.get("error").and_then(|v| v.as_str()).map(ToOwned::to_owned),
                }));
            }
            "run.completed" => {
                self.outcome = Some(TerminalOutcome::Completed);
                if let Some(output) = obj.get("output").and_then(|v| v.as_str()) {
                    if !output.is_empty() && self.content.is_empty() {
                        self.content = output.to_string();
                    }
                }
            }
            "run.failed" => {
                self.outcome = Some(TerminalOutcome::Failed);
                if let Some(e) = string_opt(obj, "error") {
                    self.error = Some(e);
                }
            }
            "run.cancelled" => {
                self.outcome = Some(TerminalOutcome::Cancelled);
                if let Some(e) = string_opt(obj, "error") {
                    self.error = Some(e);
                }
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// 429 concurrent-run retry (fetchWithConcurrentRetry) + spec jitter extension
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RetryPolicy {
    pub max_retries: u32,
    pub base_delay_ms: u64,
    pub max_delay_ms: u64,
    /// Spec §5.1 asks for jittered 429 backoff. The TS reference is deterministic,
    /// so jitter defaults OFF for exact parity; the desktop client opts in.
    /// Jitter applies to the exponential delay only — Retry-After stays authoritative.
    pub jitter: bool,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self { max_retries: 3, base_delay_ms: 350, max_delay_ms: 5000, jitter: false }
    }
}

/// TS `parseRetryAfterMs`: numeric seconds (cap 30s) or HTTP-date delta (cap 30s,
/// floor 0). `now_ms` is injected for deterministic tests.
pub fn parse_retry_after_ms(header_val: Option<&str>, now_ms: i64) -> Option<u64> {
    let h = header_val?.trim();
    if h.is_empty() {
        return None;
    }
    if let Ok(sec) = h.parse::<f64>() {
        if sec.is_finite() && sec >= 0.0 {
            return Some(((sec * 1000.0).round() as u64).min(30_000));
        }
    }
    if let Some(date_ms) = parse_date_ms_signed(h) {
        let delta = (date_ms - now_ms).max(0) as u64;
        return Some(delta.min(30_000));
    }
    None
}

fn parse_date_ms_signed(s: &str) -> Option<i64> {
    use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
    if let Ok(dt) = DateTime::parse_from_rfc2822(s) {
        return Some(dt.timestamp_millis());
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.timestamp_millis());
    }
    for fmt in ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%d %H:%M:%S%.f"] {
        if let Ok(ndt) = NaiveDateTime::parse_from_str(s, fmt) {
            return Some(Utc.from_utc_datetime(&ndt).timestamp_millis());
        }
    }
    None
}

/// Pure delay computation for attempt `attempt` (0-based, pre-increment).
/// `jitter_factor` in [0.5, 1.0) scales the exponential delay when supplied;
/// pass `None` for exact TS parity. Retry-After wins with a 50ms floor.
pub fn retry_delay_ms(
    attempt: u32,
    policy: &RetryPolicy,
    retry_after_ms: Option<u64>,
    jitter_factor: Option<f64>,
) -> u64 {
    if let Some(ra) = retry_after_ms {
        return ra.max(50);
    }
    let exp = policy
        .base_delay_ms
        .saturating_mul(1u64.checked_shl(attempt).unwrap_or(u64::MAX))
        .min(policy.max_delay_ms);
    match jitter_factor {
        Some(f) if policy.jitter => ((exp as f64) * f.clamp(0.5, 1.0)).round() as u64,
        _ => exp,
    }
}

/// Random equal-jitter factor in [0.5, 1.0) for the exponential backoff.
pub fn jitter_factor() -> f64 {
    use rand::Rng;
    rand::rng().random_range(0.5..1.0)
}

// ---------------------------------------------------------------------------
// SSE tool/step coalescing + terminal diagnostics (threadTrace.ts parity)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TraceStepIcon {
    Reasoning,
    Commentary,
    Spawn,
    Tool,
    Link,
    Status,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClassifiedTraceStep {
    pub id: String,
    pub label: String,
    pub detail: String,
    pub timestamp: String,
    pub icon: TraceStepIcon,
    pub is_reasoning: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoalescedTraceStep {
    pub step: ClassifiedTraceStep,
    pub repeat_count: usize,
}

/// Strip a leading `reasoning\s*[:—-]\s*` / `commentary\s*[:—-]\s*` prefix (case-insensitive).
fn strip_activity_prefix(raw: &str, word: &str) -> String {
    let lower = raw.to_ascii_lowercase();
    if !lower.starts_with(word) {
        return raw.to_string();
    }
    let mut rest = &raw[word.len()..];
    rest = rest.trim_start();
    if let Some(r) = rest.strip_prefix([':', '—', '-']) {
        rest = r;
    }
    rest.trim_start().to_string()
}

const STATUS_SUFFIXES: [&str; 10] = [
    "completed", "complete", "done", "finished", "running", "started", "failed", "error",
    "cancelled", "interrupted",
];

/// Port of `classifyTraceStep` (src/utils/threadTrace.ts).
pub fn classify_trace_step(entry: &TerminalEntry) -> ClassifiedTraceStep {
    let raw = entry.text.trim();
    let lower = raw.to_ascii_lowercase();

    if entry.activity_kind == Some(ActivityKind::Reasoning) {
        let detail = entry
            .reasoning_detail
            .clone()
            .unwrap_or_else(|| strip_activity_prefix(raw, "reasoning"))
            .trim()
            .to_string();
        return ClassifiedTraceStep {
            id: entry.id.clone(),
            label: "Reasoning".to_string(),
            detail,
            timestamp: entry.timestamp.clone(),
            icon: TraceStepIcon::Reasoning,
            is_reasoning: true,
        };
    }
    if entry.activity_kind == Some(ActivityKind::Commentary) {
        let detail = strip_activity_prefix(raw, "commentary").trim().to_string();
        return ClassifiedTraceStep {
            id: entry.id.clone(),
            label: "Commentary".to_string(),
            detail,
            timestamp: entry.timestamp.clone(),
            icon: TraceStepIcon::Commentary,
            is_reasoning: false,
        };
    }

    let icon = if entry.activity_kind == Some(ActivityKind::Subagent) {
        TraceStepIcon::Spawn
    } else if entry.activity_kind == Some(ActivityKind::Tool) {
        TraceStepIcon::Tool
    } else if lower.starts_with("reasoning:") || lower.starts_with("reasoning —") {
        TraceStepIcon::Reasoning
    } else if lower.contains("spawn") || lower.contains("agent") || lower.contains("session") {
        TraceStepIcon::Spawn
    } else if lower.contains("link") || lower.contains("http") || lower.contains("tab")
        || lower.contains("mcp") || lower.contains("gateway")
    {
        TraceStepIcon::Link
    } else if lower.contains("run") || lower.contains("exec") || lower.contains("tool")
        || lower.contains("read") || lower.contains("write") || lower.contains("search")
        || lower.contains("brief")
    {
        TraceStepIcon::Tool
    } else {
        TraceStepIcon::Status
    };

    // Split concise label vs trailing detail around colon or em-dash (char indices).
    let chars: Vec<char> = raw.chars().collect();
    let colon_idx = chars.iter().position(|&c| c == ':');
    let dash_idx = raw.find(" — ").map(|b| raw[..b].chars().count());
    let mut label = raw.to_string();
    let mut detail = String::new();
    if let Some(ci) = colon_idx.filter(|&i| i > 2 && i < 28) {
        label = chars[..ci].iter().collect::<String>().trim().to_string();
        detail = chars[ci + 1..].iter().collect::<String>().trim().to_string();
    } else if let Some(di) = dash_idx.filter(|&i| i > 2 && i < 28) {
        label = chars[..di].iter().collect::<String>().trim().to_string();
        detail = chars[di + 3..].iter().collect::<String>().trim().to_string();
    } else {
        // Trailing status word: /^(.+?)\s+(completed|...|interrupted)$/i
        if let Some(pos) = raw.rfind(char::is_whitespace) {
            let suffix = raw[pos + 1..].to_ascii_lowercase();
            let prefix = raw[..pos].trim();
            if !prefix.is_empty() && STATUS_SUFFIXES.contains(&suffix.as_str()) {
                label = prefix.to_string();
                detail = suffix;
            }
        }
    }

    ClassifiedTraceStep {
        id: entry.id.clone(),
        label: if label.is_empty() { "Activity".to_string() } else { label },
        detail,
        timestamp: entry.timestamp.clone(),
        icon,
        is_reasoning: icon == TraceStepIcon::Reasoning,
    }
}

/// Port of `isTerminalDiagnosticActivity`: only `activity` entries that are not
/// tool/reasoning/subagent/commentary and carry an error code, a failed /
/// cancelled / admission_conflict outcome, or an `error` activity status.
pub fn is_terminal_diagnostic_activity(entry: &TerminalEntry) -> bool {
    if entry.role != EntryRole::Activity {
        return false;
    }
    if matches!(
        entry.activity_kind,
        Some(ActivityKind::Tool) | Some(ActivityKind::Reasoning) | Some(ActivityKind::Subagent) | Some(ActivityKind::Commentary)
    ) {
        return false;
    }
    entry.error_code.as_deref().is_some_and(|c| !c.is_empty())
        || matches!(
            entry.terminal_outcome,
            Some(TerminalOutcome::Failed) | Some(TerminalOutcome::Cancelled) | Some(TerminalOutcome::AdmissionConflict)
        )
        || entry.activity_status.as_deref() == Some("error")
}

/// Port of `coalesceConsecutiveSteps`: merge adjacent identical non-reasoning,
/// non-commentary steps (icon + case-insensitive label/detail), keeping the
/// latest non-empty timestamp and a repeat count.
pub fn coalesce_consecutive_steps(steps: &[ClassifiedTraceStep]) -> Vec<CoalescedTraceStep> {
    let mut result: Vec<CoalescedTraceStep> = Vec::new();
    for step in steps {
        let can_merge = result.last().is_some_and(|prev| {
            !step.is_reasoning
                && !prev.step.is_reasoning
                && step.icon != TraceStepIcon::Commentary
                && prev.step.icon != TraceStepIcon::Commentary
                && prev.step.icon == step.icon
                && prev.step.label.to_ascii_lowercase() == step.label.to_ascii_lowercase()
                && prev.step.detail.to_ascii_lowercase() == step.detail.to_ascii_lowercase()
        });
        if can_merge {
            let prev = result.last_mut().unwrap();
            prev.repeat_count += 1;
            if !step.timestamp.is_empty() {
                prev.step.timestamp = step.timestamp.clone();
            }
        } else {
            result.push(CoalescedTraceStep { step: step.clone(), repeat_count: 1 });
        }
    }
    result
}

/// SSE-level convenience: classify parsed tool events as trace steps (using the
/// same `tool: detail` label shape the UI records) and coalesce consecutive
/// identical completed/failed rows, e.g. repeated `terminal completed` events.
pub fn coalesce_tool_events(events: &[ParsedToolEvent]) -> Vec<CoalescedTraceStep> {
    let steps: Vec<ClassifiedTraceStep> = events
        .iter()
        .enumerate()
        .map(|(i, ev)| {
            let text = if ev.label.is_empty() {
                format!("{} {}", ev.tool_name, ev.status)
            } else {
                ev.label.clone()
            };
            let mut entry = TerminalEntry::new(format!("tool-{i}"), "sse", EntryRole::Activity, text, "");
            entry.activity_kind = Some(ActivityKind::Tool);
            entry.activity_status = Some(ev.status.clone());
            classify_trace_step(&entry)
        })
        .collect();
    coalesce_consecutive_steps(&steps)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // -- Ports of src/utils/hermesProtocol.test.ts ---------------------------

    #[test]
    fn ts_resolve_profile_gateway_config_routes_default_and_named_profiles() {
        let env = HashMap::new();
        let def = resolve_profile_gateway_config(
            Some("http://127.0.0.1:8642/"),
            Some("default"),
            Some("default-key"),
            Some("profile-key"),
            &env,
        );
        assert_eq!(def.route_prefix, "");
        assert_eq!(def.api_key, "default-key");
        assert_eq!(
            build_gateway_url(&def, "/v1/capabilities", false),
            "http://127.0.0.1:8642/v1/capabilities"
        );

        let work = resolve_profile_gateway_config(
            Some("http://127.0.0.1:8642"),
            Some("work-ops"),
            Some("default-key"),
            Some("work-secret"),
            &env,
        );
        assert_eq!(work.route_prefix, "/p/work-ops");
        assert_eq!(work.api_key, "work-secret");
        assert_eq!(
            build_gateway_url(&work, "/v1/runs", false),
            "http://127.0.0.1:8642/p/work-ops/v1/runs"
        );
        assert_eq!(build_gateway_url(&work, "/health", true), "http://127.0.0.1:8642/health");
    }

    #[test]
    fn ts_sanitize_session_key_and_idempotency_key_constraints() {
        assert_eq!(sanitize_session_key_header(Some("  chan\r\n-1\0  ")).as_deref(), Some("chan-1"));
        assert_eq!(sanitize_session_key_header(Some(&"a".repeat(400))).unwrap().chars().count(), 256);
        assert_eq!(sanitize_session_key_header(Some("")), None);
        assert_eq!(sanitize_session_key_header(None), None);
        let k = create_idempotency_key("turn");
        assert!(k.starts_with("turn-"));
        assert!(k.len() > 10 && k.len() <= 255);
        assert!(k.chars().all(|c| ('!'..='~').contains(&c)));
        // Non-visible prefix chars are replaced; empty prefix falls back.
        let k2 = create_idempotency_key("a b\u{00e9}");
        assert!(k2.starts_with("a-b--"));
        assert!(create_idempotency_key("").starts_with("voxcell-"));
    }

    #[test]
    fn ts_capabilities_and_transport_feature_detection() {
        let raw = json!({
            "object": "hermes.api_server.capabilities",
            "platform": "hermes-agent",
            "model": "nous/hermes-4",
            "session_key_header": "X-Hermes-Session-Key",
            "features": {
                "chat_completions": true,
                "reasoning_streaming": true,
                "run_submission": true,
                "run_events_sse": true,
                "run_approval": true,
                "session_chat_stream": true
            }
        });
        let caps = parse_capabilities_full(&raw).unwrap();
        assert_eq!(caps.session_key_header.as_deref(), Some("X-Hermes-Session-Key"));
        assert_eq!(choose_gateway_transport(PreferredTransport::Auto, Some(&caps)), GatewayTransport::Runs);
        assert_eq!(choose_gateway_transport(PreferredTransport::ChatCompletions, Some(&caps)), GatewayTransport::ChatCompletions);
        assert_eq!(choose_gateway_transport(PreferredTransport::SessionStream, Some(&caps)), GatewayTransport::SessionStream);

        let no_runs = parse_capabilities_full(&json!({
            "features": { "chat_completions": true, "reasoning_streaming": true }
        }))
        .unwrap();
        assert_eq!(choose_gateway_transport(PreferredTransport::Auto, Some(&no_runs)), GatewayTransport::ChatCompletions);
        assert_eq!(choose_gateway_transport(PreferredTransport::Runs, Some(&no_runs)), GatewayTransport::ChatCompletions);

        // Defaults when payload fields are missing.
        let bare = parse_capabilities_full(&json!({})).unwrap();
        assert_eq!(bare.object, "hermes.api_server.capabilities");
        assert_eq!(bare.platform, "hermes-agent");
        assert_eq!(bare.model, "hermes-agent");
        assert!(bare.endpoints.is_none());
        assert!(parse_capabilities_full(&json!(null)).is_none());
        assert!(parse_capabilities_full(&json!("nope")).is_none());
    }

    #[test]
    fn ts_chat_completions_keepalives_reasoning_tool_progress_and_budget() {
        let mut parser = ChatCompletionsStreamParser::new();
        let mut reasoning_events: Vec<String> = Vec::new();
        let mut deltas: Vec<String> = Vec::new();
        let mut tools: Vec<String> = Vec::new();

        let chunk1 = [
            ": keepalive",
            "",
            "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"Inspecting git status \"}}],\"session_id\":\"s-1\",\"session_key\":\"k-1\"}",
            "",
            "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"before editing.\"}}]}",
            "",
            "event: hermes.tool.progress",
            "data: {\"tool\":\"terminal\",\"status\":\"running\",\"summary\":\"git status --short\"}",
            "",
            "data: {\"choices\":[{\"delta\":{\"content\":\"Partial summary: \"}}]}",
            "",
            "data: {\"choices\":[{\"delta\":{\"content\":\"2 files changed.\"},\"finish_reason\":\"length\"}],\"hermes\":{\"error_code\":\"iteration_budget_exhausted\"}}",
            "",
            "data: [DONE]",
            "",
        ]
        .join("\n");

        for sig in parser.push(&chunk1) {
            match sig {
                StreamSignal::ReasoningDelta { accumulated, .. } => reasoning_events.push(accumulated),
                StreamSignal::ContentDelta { accumulated, .. } => deltas.push(accumulated),
                StreamSignal::ToolEvent(ev) => tools.push(format!("{}:{}", ev.tool_name, ev.status)),
                _ => {}
            }
        }
        let (tail, result) = parser.finish();
        assert!(tail.is_empty());
        assert_eq!(result.reasoning, "Inspecting git status before editing.");
        assert_eq!(result.output, "Partial summary: 2 files changed.");
        assert_eq!(result.finish_reason.as_deref(), Some("length"));
        assert_eq!(result.error_code.as_deref(), Some("iteration_budget_exhausted"));
        assert_eq!(result.outcome, TerminalOutcome::IterationBudgetExhausted);
        assert_eq!(result.session_id.as_deref(), Some("s-1"));
        assert_eq!(result.session_key.as_deref(), Some("k-1"));
        assert!(!result.admission_conflict);
        assert_eq!(tools, vec!["terminal:running"]);
        assert_eq!(reasoning_events.len(), 2);
        assert_eq!(deltas.len(), 2);
    }

    #[test]
    fn ts_chat_completions_admission_conflict_cancelled_and_non_stream() {
        let mut conflict = ChatCompletionsStreamParser::new();
        conflict.push(
            &[
                "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"error\"}],\"error\":{\"message\":\"admission_conflict\"}}",
                "",
                "data: [DONE]",
                "",
            ]
            .join("\n"),
        );
        let (_, res) = conflict.finish();
        assert_eq!(res.outcome, TerminalOutcome::AdmissionConflict);
        assert!(res.admission_conflict);

        let mut cancelled = ChatCompletionsStreamParser::new();
        cancelled.push("data: {\"choices\":[{\"delta\":{\"content\":\"Stopped halfway\"},\"finish_reason\":\"cancelled\"}]}\n\n");
        let (_, res) = cancelled.finish();
        assert_eq!(res.outcome, TerminalOutcome::Cancelled);
        assert_eq!(res.output, "Stopped halfway");

        let non_stream = parse_chat_completions_json(&json!({
            "choices": [{
                "message": { "content": "Done.", "reasoning_content": "Thought through options." },
                "finish_reason": "stop"
            }]
        }))
        .unwrap();
        assert_eq!(non_stream.reasoning, "Thought through options.");
        assert_eq!(non_stream.outcome, TerminalOutcome::Completed);
        assert_eq!(non_stream.content, "Done.");
        assert!(parse_chat_completions_json(&json!(null)).is_none());
    }

    #[test]
    fn ts_session_chat_delta_commentary_tools_and_terminal_status() {
        let mut parser = SessionChatStreamParser::new();
        let mut commentaries: Vec<String> = Vec::new();
        let mut tools: Vec<String> = Vec::new();
        let mut deltas: Vec<String> = Vec::new();

        let chunk = [
            ": keepalive",
            "",
            "data: {\"type\":\"assistant.commentary\",\"message_id\":\"m1\",\"text\":\"Checking repo files...\",\"already_streamed\":false}",
            "",
            "data: {\"type\":\"tool.started\",\"tool\":\"read_file\",\"args_preview\":\"package.json\"}",
            "",
            "data: {\"type\":\"tool.failed\",\"tool\":\"terminal\",\"error\":\"exit code 1\"}",
            "",
            "data: {\"type\":\"assistant.delta\",\"delta\":\"Build failed in test step.\"}",
            "",
            "data: {\"type\":\"assistant.completed\",\"partial\":true,\"interrupted\":true}",
            "",
            "data: {\"type\":\"run.cancelled\"}",
            "",
        ]
        .join("\n");
        for sig in parser.push(&chunk) {
            match sig {
                StreamSignal::Interim { text, .. } => commentaries.push(text),
                StreamSignal::ToolEvent(ev) => tools.push(format!("{}:{}", ev.phase, ev.tool_name)),
                StreamSignal::ContentDelta { accumulated, .. } => deltas.push(accumulated),
                _ => {}
            }
        }
        let (_, res) = parser.finish();
        assert_eq!(res.output, "Build failed in test step.");
        assert_eq!(res.outcome, TerminalOutcome::Cancelled);
        assert!(res.partial);
        assert!(res.interrupted);
        assert_eq!(commentaries, vec!["Checking repo files..."]);
        assert_eq!(tools, vec!["started:read_file", "failed:terminal"]);
        assert_eq!(deltas.len(), 1);
    }

    #[test]
    fn ts_runs_approval_reasoning_subagent_and_outcomes() {
        let mut parser = RunsEventStreamParser::new();
        let mut approvals: Vec<(String, u32, String)> = Vec::new();
        let mut subagents: Vec<String> = Vec::new();
        let mut reasoning_chunks: Vec<String> = Vec::new();

        let chunk = [
            ": keepalive",
            "",
            "event: message.reasoning",
            "data: {\"type\":\"message.reasoning\",\"delta\":\"Planning safe shell check.\"}",
            "",
            "event: tool.started",
            "data: {\"type\":\"tool.started\",\"tool\":\"terminal\",\"preview\":\"rm -rf /tmp/demo\"}",
            "",
            "event: approval.request",
            "data: {\"type\":\"approval.request\",\"request_id\":\"req_42\",\"execution_generation\":3,\"tool\":\"terminal\",\"command\":\"rm -rf /tmp/demo\",\"choices\":[\"once\",\"always\",\"deny\"]}",
            "",
            "event: subagent.start",
            "data: {\"type\":\"subagent.start\",\"summary\":\"Check build cache\",\"child_session_id\":\"child-9\"}",
            "",
            "event: run.completed",
            "data: {\"type\":\"run.completed\",\"output\":\"Approved and completed cleanly.\",\"runtime\":{\"provider\":\"openai\",\"model\":\"gpt-5\"}}",
            "",
        ]
        .join("\n");
        for sig in parser.push(&chunk) {
            match sig {
                StreamSignal::ReasoningDelta { accumulated, .. } => reasoning_chunks.push(accumulated),
                StreamSignal::ApprovalRequest(req) => {
                    approvals.push((req.request_id, req.execution_generation, req.command))
                }
                StreamSignal::SubagentStart { summary, .. } => subagents.push(summary),
                _ => {}
            }
        }
        let (_, res) = parser.finish();
        assert_eq!(res.output, "Approved and completed cleanly.");
        assert_eq!(res.reasoning, "Planning safe shell check.");
        assert_eq!(res.outcome, TerminalOutcome::Completed);
        assert_eq!(res.model.as_deref(), Some("gpt-5"));
        assert_eq!(approvals, vec![("req_42".to_string(), 3, "rm -rf /tmp/demo".to_string())]);
        assert_eq!(subagents, vec!["Check build cache"]);
        assert_eq!(reasoning_chunks.len(), 1);
    }

    fn gateway_frames(output: &str) -> String {
        [
            ": open",
            "",
            "id: 0",
            "data: {\"event\":\"message.delta\",\"run_id\":\"run_1\",\"delta\":\"S\",\"seq\":0}",
            "",
            "id: 1",
            "data: {\"event\":\"message.delta\",\"run_id\":\"run_1\",\"delta\":\"SE wire check\",\"seq\":1}",
            "",
            "id: 2",
            &format!("data: {{\"event\":\"run.completed\",\"run_id\":\"run_1\",\"output\":\"{output}\",\"completed\":true,\"partial\":false,\"interrupted\":false,\"seq\":2}}"),
            "",
            ": stream closed",
            "",
        ]
        .join("\n")
    }

    #[test]
    fn ts_runs_gateway_wire_shape_event_in_payload() {
        let mut with_output = RunsEventStreamParser::new();
        let mut deltas: Vec<String> = Vec::new();
        for sig in with_output.push(&gateway_frames("SSE wire check")) {
            if let StreamSignal::ContentDelta { chunk, .. } = sig {
                deltas.push(chunk);
            }
        }
        let (_, first) = with_output.finish();
        assert_eq!(first.output, "SSE wire check");
        assert_eq!(first.outcome, TerminalOutcome::Completed);
        assert_eq!(deltas, vec!["S", "SE wire check"]);

        // A run.completed with an empty `output` must not erase streamed text.
        let mut empty_output = RunsEventStreamParser::new();
        empty_output.push(&gateway_frames(""));
        let (_, second) = empty_output.finish();
        assert_eq!(second.output, "SSE wire check");
        assert_eq!(second.outcome, TerminalOutcome::Completed);
    }

    #[test]
    fn ts_session_chat_reads_payload_event_names() {
        let mut parser = SessionChatStreamParser::new();
        parser.push(
            &[
                "data: {\"event\":\"assistant.delta\",\"delta\":\"Hello \",\"seq\":0}",
                "",
                "data: {\"event\":\"assistant.delta\",\"delta\":\"world\",\"seq\":1}",
                "",
                "data: {\"event\":\"run.completed\",\"output\":\"\",\"completed\":true,\"seq\":2}",
                "",
            ]
            .join("\n"),
        );
        let (_, res) = parser.finish();
        assert_eq!(res.output, "Hello world");
        assert_eq!(res.outcome, TerminalOutcome::Completed);
    }

    #[test]
    fn ts_fetch_with_concurrent_retry_delays_50_then_160() {
        // Mirrors the TS test: base 80ms, first 429 carries retry-after: 0,
        // second has none -> delays [max(0,50)=50, 80*2^1=160].
        let policy = RetryPolicy { max_retries: 3, base_delay_ms: 80, max_delay_ms: 5000, jitter: false };
        let d0 = retry_delay_ms(0, &policy, parse_retry_after_ms(Some("0"), 0), None);
        let d1 = retry_delay_ms(1, &policy, parse_retry_after_ms(None, 0), None);
        assert_eq!((d0, d1), (50, 160));
        // Default policy parity: 350 * 2^attempt capped at 5000.
        let def = RetryPolicy::default();
        assert_eq!(retry_delay_ms(0, &def, None, None), 350);
        assert_eq!(retry_delay_ms(1, &def, None, None), 700);
        assert_eq!(retry_delay_ms(2, &def, None, None), 1400);
        assert_eq!(retry_delay_ms(4, &def, None, None), 5000);
        // Retry-After seconds cap at 30s and floor at 50ms.
        assert_eq!(parse_retry_after_ms(Some("2.5"), 0), Some(2500));
        assert_eq!(parse_retry_after_ms(Some("120"), 0), Some(30_000));
        assert_eq!(retry_delay_ms(0, &def, Some(0), None), 50);
        assert_eq!(parse_retry_after_ms(Some("-1"), 0), None);
        assert_eq!(parse_retry_after_ms(None, 0), None);
    }

    #[test]
    fn ts_gateway_payload_mappers_normalize_everything() {
        let skills = map_gateway_skill_records(&json!([
            { "name": "github-pr-workflow", "description": "Review & merge PRs", "category": "git" }
        ]));
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "github-pr-workflow");
        assert_eq!(skills[0].source, "category:git");
        assert_eq!(skills[0].summary, "Review & merge PRs");

        let toolsets = map_gateway_toolset_records(&json!([
            { "name": "core", "label": "Core Tools", "description": "File and terminal ops",
              "enabled": true, "configured": true, "tools": ["read_file", "terminal"] }
        ]));
        assert_eq!(toolsets[0].tools.len(), 2);
        assert_eq!(toolsets[0].label, "Core Tools");
        assert!(toolsets[0].configured);

        let jobs = map_gateway_job_records(&json!([
            { "id": "job-1", "schedule": "0 8 * * *", "prompt": "Summarize overnight runs", "status": "paused" }
        ]));
        assert!(jobs[0].paused);
        assert_eq!(jobs[0].status, "paused");
        assert_eq!(jobs[0].channel_id, "general");
        assert_eq!(jobs[0].mode, CronJobMode::Agent);

        let sessions = map_gateway_sessions(&json!({
            "sessions": [{ "session_id": "hermes-alpha", "title": "Alpha Build", "source": "api", "message_count": 4 }]
        }));
        assert_eq!(sessions[0].id, "hermes-alpha");
        assert_eq!(sessions[0].title.as_deref(), Some("Alpha Build"));
        assert_eq!(sessions[0].message_count, 4);

        let messages = map_gateway_session_messages_at(
            &json!({ "messages": [
                { "id": "u1", "role": "user", "content": "hi" },
                { "id": "a1", "role": "assistant", "content": "hello", "reasoning_content": "pondering greeting" }
            ]}),
            "hermes-alpha",
            1_700_000_000_000,
        );
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[1].reasoning.as_deref(), Some("pondering greeting"));
        assert_eq!(messages[0].role, SessionMessageRole::User);

        let model_opts = extract_model_options(&json!({
            "selected": { "provider": "nous", "model": "hermes-4" },
            "providers": [{ "id": "nous", "label": "Nous Portal", "configured": true, "models": ["hermes-4", "hermes-4-mini"] }]
        }));
        assert_eq!(model_opts.active_model.as_deref(), Some("hermes-4"));
        assert_eq!(model_opts.active_provider.as_deref(), Some("nous"));
        assert_eq!(model_opts.models, vec!["hermes-4", "hermes-4-mini"]);
    }

    #[test]
    fn ts_classify_gateway_session_origin_workspace_vs_external() {
        let p = |sid: Option<&'static str>, key: Option<&'static str>, cid: Option<&'static str>, src: Option<&'static str>| {
            SessionOriginParams { session_id: sid, session_key: key, channel_id: cid, source: src, created_in_workspace: false }
        };
        assert_eq!(
            classify_gateway_session_origin(p(Some("hermes-github-com-mattt-plong"), Some("voxcell-github-com-mattt-plong"), Some("github-com-mattt-plong"), Some("api"))),
            SessionOrigin::Workspace
        );
        assert_eq!(
            classify_gateway_session_origin(p(Some("hermes-fill-me-in"), None, Some("fill-me-in"), Some("api_server"))),
            SessionOrigin::Workspace
        );
        assert_eq!(classify_gateway_session_origin(p(Some("custom-gw-id"), None, None, Some("web"))), SessionOrigin::Workspace);
        assert_eq!(
            classify_gateway_session_origin(p(Some("local:07ee2d7b545bd3db563f9fcd83fb72482"), None, Some("local-07ee2d7b545bd3db563f9fcd83fb72482"), Some("api"))),
            SessionOrigin::External
        );
        assert_eq!(
            classify_gateway_session_origin(p(Some("20260501_142311_abcd1234"), None, Some("20260501_142311_abcd1234"), Some("api"))),
            SessionOrigin::External
        );
        assert_eq!(classify_gateway_session_origin(p(Some("hermes-cli-thread"), None, None, Some("cli"))), SessionOrigin::External);
        assert_eq!(classify_gateway_session_origin(p(Some("cron_watch_1"), None, None, Some("cron"))), SessionOrigin::External);

        let gw_mapped = map_gateway_sessions(&json!({ "sessions": [
            { "session_id": "hermes-my-task", "session_key": "voxcell-my-task", "title": "My Task", "source": "api" },
            { "session_id": "local:5e52c0948b0bfd7907bf92990a4140f94", "title": "Explain local Forgejo CI", "source": "api" }
        ]}));
        assert_eq!(gw_mapped[0].session_key.as_deref(), Some("voxcell-my-task"));
        assert_eq!(gw_mapped[1].session_key, None);
        assert_eq!(gw_mapped[0].origin, SessionOrigin::Workspace);
        assert_eq!(gw_mapped[1].origin, SessionOrigin::External);
    }

    // -- Retained scaffold tests (unchanged expectations) --------------------

    #[test]
    fn builds_profile_prefixed_urls() {
        let env = HashMap::from([(
            "HERMES_API_KEY_RESEARCH_LAB".to_string(),
            "secret-lab".to_string(),
        )]);
        let cfg = resolve_profile_gateway_config(
            Some("http://127.0.0.1:8642/"),
            Some("research lab"),
            Some("default-key"),
            None,
            &env,
        );
        assert_eq!(cfg.route_prefix, "/p/research%20lab");
        assert_eq!(cfg.api_key, "secret-lab");
        assert_eq!(
            build_gateway_url(&cfg, "/v1/runs", false),
            "http://127.0.0.1:8642/p/research%20lab/v1/runs"
        );
        assert_eq!(build_gateway_url(&cfg, "/health", true), "http://127.0.0.1:8642/health");
    }

    #[test]
    fn decodes_sse_with_keepalive_and_runs_events() {
        let mut parser = RunsEventStreamParser::new();
        let s1 = parser.push(": keepalive\n\ndata: {\"event\":\"message.reasoning\",\"delta\":\"think \"}\n\n");
        assert_eq!(s1.len(), 1);
        let s2 = parser.push(
            "data: {\"event\":\"tool.started\",\"tool\":\"terminal\",\"preview\":\"cargo test\"}\n\n\
             data: {\"event\":\"approval.request\",\"request_id\":\"req-1\",\"tool\":\"terminal\",\"command\":\"rm -rf tmp\"}\n\n\
             data: {\"event\":\"message.delta\",\"delta\":\"Hello \"}\n\n\
             data: {\"event\":\"run.completed\",\"output\":\"\"}\n\n",
        );
        assert_eq!(s2.len(), 3);
        let (_, res) = parser.finish();
        assert_eq!(res.output, "Hello ");
        assert_eq!(res.reasoning, "think ");
        assert_eq!(res.outcome, TerminalOutcome::Completed);
    }

    // -- Additional parity edge cases ----------------------------------------

    #[test]
    fn opaque_session_identifier_forms() {
        for opaque in [
            "local-07ee2d7b545bd3db", "#local:abcdef", "hermes-local_abcdef",
            "run-abc123", "#hermes-run:ff00ff", "cron_watch_1", "hermes-cron:nightly",
            "123e4567-e89b-12d3-a456-426614174000", "#hermes-123e4567-e89b-12d3-a456-426614174000",
            "07ee2d7b545bd3db563f9fcd83fb72482", "LOCAL-ABCDEF",
        ] {
            assert!(is_opaque_session_identifier(Some(opaque)), "expected opaque: {opaque}");
        }
        for readable in [
            "hermes-github-com-mattt-plong", "hermes-fill-me-in", "local-abc", "run-12",
            "cron", "cron-", "123e4567-e89b-12d3-a456-42661417400", "xyz", "", "  ",
        ] {
            assert!(!is_opaque_session_identifier(Some(readable)), "expected readable: {readable}");
        }
        assert!(!is_opaque_session_identifier(None));
        // hermes-<slug> with an opaque channel id stays external.
        assert_eq!(
            classify_gateway_session_origin(SessionOriginParams {
                session_id: Some("hermes-my-task"),
                session_key: None,
                channel_id: Some("local-07ee2d7b545bd3db563f"),
                source: Some("api"),
                created_in_workspace: false,
            }),
            SessionOrigin::External
        );
        assert_eq!(
            classify_gateway_session_origin(SessionOriginParams {
                session_id: Some("anything"), session_key: None, channel_id: None,
                source: None, created_in_workspace: true,
            }),
            SessionOrigin::Workspace
        );
    }

    #[test]
    fn sse_decoder_trimstart_multiline_crlf_and_flush() {
        let mut d = SseLineDecoder::new();
        // Multiple leading spaces after data: are all stripped (TS trimStart).
        let frames = d.push("event: custom\ndata:   {\"a\":1}\n\n");
        assert_eq!(frames, vec![SseFrame { event: "custom".into(), data: "{\"a\":1}".into() }]);
        // Multi-line data joins with \n; CRLF tolerated; comments counted.
        let frames = d.push(": ping\r\ndata: line1\r\ndata: line2\r\n\r\n");
        assert_eq!(frames, vec![SseFrame { event: "message".into(), data: "line1\nline2".into() }]);
        // Exactly one comment line (": ping") was fed above; push 1 had none.
        // (TS ignores comments entirely — this counter is a Rust extension.)
        assert_eq!(d.keepalive_count, 1);
        // Split mid-line across chunks.
        assert!(d.push("data: {\"x\":").is_empty());
        let frames = d.push("2}\n");
        assert!(frames.is_empty()); // no blank line yet
        let frames = d.flush();
        assert_eq!(frames, vec![SseFrame { event: "message".into(), data: "{\"x\":2}".into() }]);
        // flush with no trailing newline at all
        let mut d2 = SseLineDecoder::new();
        assert!(d2.push("data: tail").is_empty());
        assert_eq!(d2.flush()[0].data, "tail");
        assert!(d2.flush().is_empty());
        // empty event: value resets to message
        let mut d3 = SseLineDecoder::new();
        let f = d3.push("event:\ndata: x\n\n");
        assert_eq!(f[0].event, "message");
    }

    #[test]
    fn runs_truthy_chains_empty_choices_and_error_synthesis() {
        let mut parser = RunsEventStreamParser::new();
        let sigs = parser.push(
            &[
                // preview:"" must fall through to result (JS || truthiness).
                "data: {\"event\":\"tool.completed\",\"tool\":\"terminal\",\"preview\":\"\",\"result\":\"ok\",\"duration\":1.5}",
                "",
                // tool.failed without error string synthesizes preview || 'error'.
                "data: {\"event\":\"tool.failed\",\"tool\":\"terminal\"}",
                "",
                // Empty choices array is preserved (not replaced by defaults).
                "data: {\"event\":\"approval.request\",\"request_id\":\"r1\",\"choices\":[]}",
                "",
                // execution_generation coerced from string; defaults when absent.
                "data: {\"event\":\"clarify.request\",\"prompt_id\":\"p9\",\"execution_generation\":\"4\",\"draft\":\"which file?\"}",
                "",
                "", // final blank line: SSE dispatches a frame only on an empty line
            ]
            .join("\n"),
        );
        let tools: Vec<&ParsedToolEvent> = sigs
            .iter()
            .filter_map(|s| match s { StreamSignal::ToolEvent(t) => Some(t), _ => None })
            .collect();
        assert_eq!(tools[0].summary, "terminal: ok");
        assert_eq!(tools[0].duration_sec, Some(1.5));
        assert_eq!(tools[0].error, None);
        assert_eq!(tools[1].phase, "failed");
        assert_eq!(tools[1].error.as_deref(), Some("error"));
        assert_eq!(tools[1].summary, "terminal failed");
        let approvals: Vec<&ParsedApprovalRequest> = sigs
            .iter()
            .filter_map(|s| match s { StreamSignal::ApprovalRequest(a) => Some(a), _ => None })
            .collect();
        assert!(approvals[0].choices.is_empty());
        assert_eq!(approvals[0].title, "Approval required");
        assert_eq!(approvals[0].destination, "hermes-run");
        assert_eq!(approvals[0].summary, "Hermes run paused awaiting approval.");
        assert_eq!(approvals[1].kind, ApprovalKind::Clarify);
        assert_eq!(approvals[1].execution_generation, 4);
        assert_eq!(approvals[1].command, "which file?");
        assert_eq!(approvals[1].summary, "Execute: which file?");
        assert_eq!(approvals[1].choices, vec!["once", "always", "deny"]);
    }

    #[test]
    fn runs_failed_preserves_prior_error_code_and_terminal_variants() {
        let mut parser = RunsEventStreamParser::new();
        parser.push(
            &[
                "data: {\"event\":\"run.completed\",\"error_code\":\"iteration_budget_exhausted\",\"output\":\"partial\"}",
                "",
            ]
            .join("\n"),
        );
        let (_, res) = parser.finish();
        assert_eq!(res.outcome, TerminalOutcome::IterationBudgetExhausted);
        assert_eq!(res.error_code.as_deref(), Some("iteration_budget_exhausted"));

        // run.failed without error_code keeps the previous code; default error text.
        // TS hermesProtocol.ts:1314-1320: run.completed stores ONLY
        // iteration_budget_exhausted, so that is the code used to prove retention.
        let mut p2 = RunsEventStreamParser::new();
        p2.push("data: {\"event\":\"run.completed\",\"error_code\":\"iteration_budget_exhausted\"}\n\n");
        p2.push("data: {\"event\":\"run.failed\"}\n\n");
        let (_, res2) = p2.finish();
        assert_eq!(res2.outcome, TerminalOutcome::Failed);
        assert_eq!(res2.error.as_deref(), Some("Run failed"));
        assert_eq!(res2.error_code.as_deref(), Some("iteration_budget_exhausted"));

        // run.interrupted default diagnostic message.
        let mut p3 = RunsEventStreamParser::new();
        p3.push("data: {\"event\":\"run.interrupted\"}\n\n");
        let (_, res3) = p3.finish();
        assert_eq!(res3.outcome, TerminalOutcome::Interrupted);
        assert_eq!(res3.error.as_deref(), Some("Gateway shutdown interrupted the run."));

        // run.steered + subagent.complete with delegation ids.
        let mut p4 = RunsEventStreamParser::new();
        let sigs = p4.push(
            "data: {\"event\":\"run.steered\",\"text\":\"focus on tests\"}\n\n\
             data: {\"event\":\"subagent.complete\",\"status\":\"failed\",\"child_session_id\":\"c1\",\"delegation_id\":\"d1\"}\n\n",
        );
        assert!(matches!(&sigs[0], StreamSignal::Steered(t) if t == "focus on tests"));
        match &sigs[1] {
            StreamSignal::SubagentComplete { status, summary, child_session_id, delegation_id } => {
                assert_eq!(status, "failed");
                assert_eq!(summary, "Subagent failed");
                assert_eq!(child_session_id.as_deref(), Some("c1"));
                assert_eq!(delegation_id.as_deref(), Some("d1"));
            }
            other => panic!("unexpected signal: {other:?}"),
        }
        // message.delta carries reasoning_content alongside content.
        let mut p5 = RunsEventStreamParser::new();
        let sigs = p5.push("data: {\"event\":\"message.delta\",\"reasoning_content\":\"hmm\",\"delta\":\"answer\"}\n\n");
        assert_eq!(sigs.len(), 2);
        let (_, res5) = p5.finish();
        assert_eq!(res5.reasoning, "hmm");
        assert_eq!(res5.output, "answer");
    }

    #[test]
    fn chat_completions_classify_finish_reason_matrix() {
        use TerminalOutcome::*;
        assert_eq!(classify_finish_reason(None, None, Some("ADMISSION_CONFLICT")), AdmissionConflict);
        assert_eq!(classify_finish_reason(Some("stop"), Some("Admission_Conflict"), None), AdmissionConflict);
        assert_eq!(classify_finish_reason(Some("cancelled"), None, None), Cancelled);
        assert_eq!(classify_finish_reason(Some("error"), None, None), Failed);
        assert_eq!(classify_finish_reason(Some("length"), Some("iteration_budget_exhausted"), None), IterationBudgetExhausted);
        assert_eq!(classify_finish_reason(Some("length"), None, None), Completed);
        assert_eq!(classify_finish_reason(Some("stop"), None, None), Completed);
        assert_eq!(classify_finish_reason(None, None, None), Completed);
        // error.code overrides hermes.error_code; session ids tracked mid-stream.
        let mut p = ChatCompletionsStreamParser::new();
        p.push(
            "data: {\"hermes\":{\"error_code\":\"first\"},\"error\":{\"code\":\"second\",\"message\":\"boom\"},\"choices\":[{\"delta\":{\"content\":\"x\"}}]}\n\n",
        );
        let (_, res) = p.finish();
        assert_eq!(res.error_code.as_deref(), Some("second"));
        assert_eq!(res.error.as_deref(), Some("boom"));
        // TS: no finish_reason and a non-admission error => completed outcome.
        assert_eq!(res.outcome, Completed);
    }

    #[test]
    fn chat_completions_delta_message_fallback_and_tool_progress_args() {
        // Non-streaming style `message` delta fallback when `delta` is null.
        let mut p = ChatCompletionsStreamParser::new();
        p.push("data: {\"choices\":[{\"delta\":null,\"message\":{\"content\":\"full\",\"reasoning\":\"think\"}}]}\n\n");
        let (_, res) = p.finish();
        assert_eq!(res.output, "full");
        assert_eq!(res.reasoning, "think");
        // hermes.tool.progress: tool_name + args fallbacks, done/failed phases.
        let mut p2 = ChatCompletionsStreamParser::new();
        let sigs = p2.push(
            "event: hermes.tool.progress\ndata: {\"tool_name\":\"search\",\"status\":\"done\",\"args\":\"query=foo\"}\n\n\
             event: hermes.tool.progress\ndata: {\"tool\":\"terminal\",\"status\":\"ERROR\"}\n\n",
        );
        match &sigs[0] {
            StreamSignal::ToolEvent(t) => {
                assert_eq!(t.tool_name, "search");
                assert_eq!(t.phase, "completed");
                assert_eq!(t.summary, "search: query=foo");
            }
            other => panic!("unexpected: {other:?}"),
        }
        match &sigs[1] {
            StreamSignal::ToolEvent(t) => {
                assert_eq!(t.phase, "failed");
                assert_eq!(t.summary, "Running terminal");
            }
            other => panic!("unexpected: {other:?}"),
        }
        // Malformed JSON frames are skipped without poisoning the stream.
        let mut p3 = ChatCompletionsStreamParser::new();
        let sigs = p3.push("data: not-json\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\n");
        assert_eq!(sigs.len(), 1);
    }

    #[test]
    fn session_chat_completed_fallback_and_outcome_precedence() {
        // assistant.completed content only fills when nothing streamed.
        let mut p = SessionChatStreamParser::new();
        p.push("data: {\"type\":\"assistant.completed\",\"content\":\"final text\"}\n\n");
        let (_, res) = p.finish();
        assert_eq!(res.output, "final text");
        assert_eq!(res.outcome, TerminalOutcome::Completed);

        let mut p2 = SessionChatStreamParser::new();
        p2.push(
            "data: {\"type\":\"assistant.delta\",\"delta\":\"streamed\"}\n\n\
             data: {\"type\":\"assistant.completed\",\"content\":\"ignored\"}\n\n",
        );
        let (_, res2) = p2.finish();
        assert_eq!(res2.output, "streamed");

        // interrupted without terminal event => cancelled fallback.
        let mut p3 = SessionChatStreamParser::new();
        p3.push("data: {\"type\":\"assistant.completed\",\"interrupted\":true}\n\n");
        let (_, res3) = p3.finish();
        assert_eq!(res3.outcome, TerminalOutcome::Cancelled);
        assert!(res3.interrupted);

        // run.failed error string; explicit outcome beats interrupted fallback.
        let mut p4 = SessionChatStreamParser::new();
        p4.push(
            "data: {\"type\":\"assistant.completed\",\"interrupted\":true}\n\n\
             data: {\"type\":\"run.failed\",\"error\":\"boom\"}\n\n",
        );
        let (_, res4) = p4.finish();
        assert_eq!(res4.outcome, TerminalOutcome::Failed);
        assert_eq!(res4.error.as_deref(), Some("boom"));

        // tool.completed with duration + result preview; commentary already_streamed.
        let mut p5 = SessionChatStreamParser::new();
        let sigs = p5.push(
            "data: {\"type\":\"tool.completed\",\"tool\":\"read_file\",\"result\":\"12 lines\",\"duration\":0.25}\n\n\
             data: {\"type\":\"assistant.commentary\",\"text\":\"note\",\"already_streamed\":true}\n\n",
        );
        match &sigs[0] {
            StreamSignal::ToolEvent(t) => {
                assert_eq!(t.phase, "completed");
                assert_eq!(t.summary, "read_file: 12 lines");
                assert_eq!(t.duration_sec, Some(0.25));
                assert_eq!(t.error, None);
            }
            other => panic!("unexpected: {other:?}"),
        }
        assert!(matches!(&sigs[1], StreamSignal::Interim { already_streamed: true, .. }));
    }

    #[test]
    fn retry_after_http_date_and_jitter_bounds() {
        // HTTP-date Retry-After: delta from injected now, clamped to [0, 30000].
        let now = 1_700_000_000_000i64; // 2023-11-14T22:13:20Z
        let header = "Tue, 14 Nov 2023 22:13:25 GMT";
        assert_eq!(parse_retry_after_ms(Some(header), now), Some(5000));
        // Past date floors at 0 (then 50ms floor applies in retry_delay_ms).
        let past = "Tue, 14 Nov 2023 22:13:10 GMT";
        assert_eq!(parse_retry_after_ms(Some(past), now), Some(0));
        // Far future caps at 30s.
        let future = "Tue, 14 Nov 2023 22:20:00 GMT";
        assert_eq!(parse_retry_after_ms(Some(future), now), Some(30_000));
        assert_eq!(parse_retry_after_ms(Some("garbage"), now), None);
        // RFC3339 also accepted.
        assert_eq!(parse_retry_after_ms(Some("2023-11-14T22:13:22Z"), now), Some(2000));

        // Jitter: equal-jitter bounds [exp/2, exp], retry-after untouched.
        let policy = RetryPolicy { jitter: true, ..RetryPolicy::default() };
        for _ in 0..200 {
            let f = jitter_factor();
            assert!((0.5..1.0).contains(&f));
            let d = retry_delay_ms(2, &policy, None, Some(f));
            // Inclusive upper bound: retry_delay_ms rounds exp*f to whole ms (see its
            // `.round()` at protocol.rs:2069), and f in [0.5, 1.0) rounds UP to exactly
            // exp (1400) whenever f >= 1399.5/1400 — a legitimate ~0.04% draw, not a bug.
            assert!((700..=1400).contains(&d), "jittered delay out of bounds: {d}");
        }
        assert_eq!(retry_delay_ms(0, &policy, Some(1200), Some(0.5)), 1200);
        // jitter flag off => factor ignored (TS parity).
        let det = RetryPolicy::default();
        assert_eq!(retry_delay_ms(2, &det, None, Some(0.5)), 1400);
    }

    #[test]
    fn epoch_ms_and_date_parsing_and_session_message_fallbacks() {
        assert_eq!(to_epoch_ms(Some(&json!(1_700_000_000))), Some(1_700_000_000_000));
        assert_eq!(to_epoch_ms(Some(&json!(1_700_000_000_000u64))), Some(1_700_000_000_000));
        assert_eq!(to_epoch_ms(Some(&json!("1700000000"))), Some(1_700_000_000_000));
        assert_eq!(to_epoch_ms(Some(&json!("2023-11-14T22:13:20Z"))), Some(1_700_000_000_000));
        assert_eq!(to_epoch_ms(Some(&json!("not a date"))), None);
        assert_eq!(to_epoch_ms(None), None);

        let sessions = map_gateway_sessions(&json!({"sessions": [{
            "session_id": "hermes-x",
            "created_at": 1_699_999_999,
            "updated_at": "2023-11-14T22:13:20Z",
            "parent_session_id": "hermes-parent",
            "session_key": "   ",
            "last_message": ""
        }]}));
        assert_eq!(sessions[0].created_at_ms, Some(1_699_999_999_000));
        assert_eq!(sessions[0].updated_at_ms, Some(1_700_000_000_000));
        assert_eq!(sessions[0].parent_session_id.as_deref(), Some("hermes-parent"));
        assert_eq!(sessions[0].session_key, None); // empty-after-trim -> undefined
        assert_eq!(sessions[0].last_message, None);

        // Messages: tool/developer skipped, image parts, fallback ids/timestamps.
        let msgs = map_gateway_session_messages_at(
            &json!({"messages": [
                { "role": "tool", "content": "skip me" },
                { "role": "developer", "content": "skip me too" },
                { "role": "user", "content": [
                    "look:", { "type": "input_image", "image_url": "x" }, { "text": "fig" }, { "type": "other" }
                ]},
                { "role": "assistant", "content": "   ", "reasoning_content": "  " },
                { "role": "assistant", "text": "fallback text", "timestamp": 1_700_000_000 }
            ]}),
            "s9",
            1_700_000_100_000,
        );
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].text, "look:\n[image]\nfig");
        assert_eq!(msgs[0].id, "gw-msg-s9-2"); // original list index preserved
        assert_eq!(msgs[0].created_at_ms, 1_700_000_100_000 - 5000 + 2000);
        assert_eq!(msgs[1].text, "fallback text");
        assert_eq!(msgs[1].created_at_ms, 1_700_000_000_000);
        assert_eq!(msgs[1].id, "gw-msg-s9-4");
    }

    #[test]
    fn capabilities_boolean_coercion_and_endpoints_and_auto_transport_matrix() {
        // TS Boolean() coercion: truthy non-bool feature values become true.
        let caps = parse_capabilities_full(&json!({
            "features": { "run_submission": 1, "run_events_sse": "yes", "run_approval": 0, "session_chat_stream": null },
            "endpoints": { "runs": "/v1/runs", "bad": 42 }
        }))
        .unwrap();
        assert!(caps.supports_runs());
        assert!(!caps.feature("run_approval"));
        assert!(!caps.supports_session_stream());
        assert_eq!(caps.endpoints.as_ref().unwrap().get("runs").unwrap(), "/v1/runs");
        assert!(!caps.endpoints.as_ref().unwrap().contains_key("bad"));

        // Auto: runs + reasoning_streaming but no run controls -> chat wins.
        let c = parse_capabilities_full(&json!({"features": {
            "run_submission": true, "run_events_sse": true, "reasoning_streaming": true, "chat_completions": true
        }})).unwrap();
        assert_eq!(choose_gateway_transport(PreferredTransport::Auto, Some(&c)), GatewayTransport::ChatCompletions);
        // run_stop alone counts as a run control.
        let c2 = parse_capabilities_full(&json!({"features": {
            "run_submission": true, "run_events_sse": true, "reasoning_streaming": true, "run_stop": true
        }})).unwrap();
        assert_eq!(choose_gateway_transport(PreferredTransport::Auto, Some(&c2)), GatewayTransport::Runs);
        // No caps at all -> chat_completions default for every preference.
        for pref in [PreferredTransport::Auto, PreferredTransport::Runs, PreferredTransport::SessionStream, PreferredTransport::ChatCompletions] {
            assert_eq!(choose_gateway_transport(pref, None), GatewayTransport::ChatCompletions);
        }
        // chat_completions:false disables chat; runs-only gateway.
        let c3 = parse_capabilities_full(&json!({"features": {
            "chat_completions": false, "run_submission": true, "run_events_sse": true
        }})).unwrap();
        assert_eq!(choose_gateway_transport(PreferredTransport::ChatCompletions, Some(&c3)), GatewayTransport::Runs);
        assert_eq!(choose_gateway_transport(PreferredTransport::Auto, Some(&c3)), GatewayTransport::Runs);
        // session-only gateway.
        let c4 = parse_capabilities_full(&json!({"features": {
            "chat_completions": false, "session_chat_stream": true
        }})).unwrap();
        assert_eq!(choose_gateway_transport(PreferredTransport::Auto, Some(&c4)), GatewayTransport::SessionStream);
        assert_eq!(choose_gateway_transport(PreferredTransport::Runs, Some(&c4)), GatewayTransport::SessionStream);
        // Core adapter derivation.
        let (core_caps, model) = parse_capabilities_payload(&json!({
            "model": "m1", "profiles": ["default", "coder"],
            "features": { "run_submission": true, "run_events_sse": true, "run_approval": true, "run_steer": true, "run_clarify": true }
        })).unwrap();
        assert!(core_caps.runs && core_caps.approvals && core_caps.steering && core_caps.clarify);
        assert!(core_caps.chat_completions); // default true
        assert_eq!(core_caps.profiles, vec!["default", "coder"]);
        assert_eq!(model.as_deref(), Some("m1"));
    }

    #[test]
    fn model_options_dedupe_unshift_and_provider_forms() {
        let opts = extract_model_options(&json!({
            "models": [" a ", "a", "", {"id": "b"}, {"model": "c"}, {"name": "d"}, {"id": "b"}],
            "data": ["e"],
            "curated_models": [{"id": "f"}],
            "current_model": "z",
            "providers": ["plain", {"slug": "s1", "authenticated": false, "models": ["m1"]}, {"id": ""}]
        }));
        assert_eq!(opts.models, vec!["z", "a", "b", "c", "d", "e", "f", "m1"]);
        assert_eq!(opts.active_model.as_deref(), Some("z"));
        assert_eq!(opts.providers.len(), 2);
        assert_eq!(opts.providers[0], ModelProviderInfo { id: "plain".into(), label: "plain".into(), configured: Some(true) });
        assert_eq!(opts.providers[1].configured, Some(false));
        assert_eq!(extract_model_options(&json!(null)), ModelOptions::default());
        // activeModel falls back to first model; provider fallback chain.
        let o2 = extract_model_options(&json!({"models": ["first"], "provider": "p0"}));
        assert_eq!(o2.active_model.as_deref(), Some("first"));
        assert_eq!(o2.active_provider.as_deref(), Some("p0"));
        // Adapter keeps provider attribution for the probe.
        let (models, active) = parse_model_options_payload(&json!({
            "selected": {"provider": "nous", "model": "hermes-4"},
            "providers": [{"id": "nous", "models": ["hermes-4", "hermes-4-mini"]}]
        }));
        assert_eq!(active.as_deref(), Some("hermes-4"));
        assert_eq!(models[0].provider.as_deref(), Some("nous"));
        assert_eq!(models[1].id, "hermes-4-mini");
    }

    #[test]
    fn skills_toolsets_jobs_edge_forms() {
        // source/path fallbacks, enabled coercion, wrapped payloads.
        let skills = map_gateway_skill_records(&json!({"skills": [
            {"name": " s1 ", "path": " /tmp/s1 ", "enabled": 0},
            {"name": "s2", "source": "custom"},
            {"name": "  "},
            "not-an-object"
        ]}));
        assert_eq!(skills.len(), 2);
        assert_eq!(skills[0].source, "/tmp/s1");
        assert!(!skills[0].enabled);
        assert_eq!(skills[1].source, "custom");
        assert!(skills[1].enabled);
        assert_eq!(map_gateway_skill_records(&json!({"data": [{"name": "s3"}]}))[0].source, "gateway:/v1/skills");

        // String toolset entries; label fallback; non-string tools filtered.
        let ts = map_gateway_toolset_records(&json!({"toolsets": [" core ", {"name": "x", "tools": ["a", 5, "b"], "configured": false}]}));
        assert_eq!(ts[0].name, "core");
        assert_eq!(ts[0].label, "core");
        assert!(ts[0].configured);
        assert_eq!(ts[1].label, "x");
        assert_eq!(ts[1].tools, vec!["a", "b"]);
        assert!(!ts[1].configured);

        // command-only job => no-agent; enabled:false => paused; name fallback id.
        let jobs = map_gateway_job_records(&json!({"jobs": [
            {"job_id": "j2", "command": "echo hi", "enabled": false},
            {"name": "Named Job", "cron": "*/5 * * * *", "delivery_target": "ops", "prompt": "p", "command": "c"}
        ]}));
        assert_eq!(jobs[0].id, "j2");
        assert_eq!(jobs[0].mode, CronJobMode::NoAgent);
        assert!(jobs[0].paused);
        assert!(!jobs[0].enabled);
        assert_eq!(jobs[0].status, "paused");
        assert_eq!(jobs[0].summary, "echo hi");
        assert_eq!(jobs[1].id, "Named Job");
        assert_eq!(jobs[1].mode, CronJobMode::Agent);
        assert_eq!(jobs[1].schedule, "*/5 * * * *");
        assert_eq!(jobs[1].channel_id, "ops");
        assert_eq!(jobs[1].summary, "p");
        assert_eq!(jobs[1].status, "active");
        // Core adapters still produce the legacy shapes.
        assert_eq!(map_gateway_skills(&json!([{"name": "s", "description": "d"}]))[0].description, "d");
        assert_eq!(map_gateway_jobs(&json!([{"id": "j", "prompt": "p"}]))[0].channel_id.as_deref(), Some("general"));
    }

    // -- threadTrace.ts parity: classification, coalescing, diagnostics --------

    fn make_entry(id: &str, role: EntryRole, text: &str, timestamp: &str) -> TerminalEntry {
        TerminalEntry::new(id, "general", role, text, timestamp)
    }

    #[test]
    fn trace_classification_spawn_tool_link_status() {
        let spawn = classify_trace_step(&make_entry("1", EntryRole::Activity, "Agent spawned: researcher", "10:47"));
        assert_eq!(spawn.icon, TraceStepIcon::Spawn);
        assert_eq!(spawn.label, "Agent spawned");
        assert_eq!(spawn.detail, "researcher");

        let link = classify_trace_step(&make_entry("2", EntryRole::Activity, "MCP link verified", "10:47"));
        assert_eq!(link.icon, TraceStepIcon::Link);

        let tool = classify_trace_step(&make_entry("3", EntryRole::Activity, "Tool run: brief summary", "10:47"));
        assert_eq!(tool.icon, TraceStepIcon::Tool);

        // Status-suffix split: "terminal completed" -> label terminal, detail completed.
        let status = classify_trace_step(&make_entry("4", EntryRole::Activity, "terminal completed", "08:23"));
        assert_eq!(status.label, "terminal");
        assert_eq!(status.detail, "completed");

        // Reasoning activity uses reasoning_detail and strips the prefix otherwise.
        let mut r = make_entry("5", EntryRole::Activity, "Reasoning: Checking logs", "08:25");
        r.activity_kind = Some(ActivityKind::Reasoning);
        r.reasoning_detail = Some("Checking logs".into());
        let rs = classify_trace_step(&r);
        assert_eq!(rs.icon, TraceStepIcon::Reasoning);
        assert!(rs.is_reasoning);
        assert_eq!(rs.label, "Reasoning");
        assert_eq!(rs.detail, "Checking logs");
        let mut r2 = make_entry("6", EntryRole::Activity, "reasoning — deep thought", "08:25");
        r2.activity_kind = Some(ActivityKind::Reasoning);
        assert_eq!(classify_trace_step(&r2).detail, "deep thought");

        // Commentary prefix strip.
        let mut c = make_entry("7", EntryRole::Activity, "Commentary: scanning files", "08:26");
        c.activity_kind = Some(ActivityKind::Commentary);
        let cs = classify_trace_step(&c);
        assert_eq!(cs.icon, TraceStepIcon::Commentary);
        assert_eq!(cs.label, "Commentary");
        assert_eq!(cs.detail, "scanning files");

        // Em-dash split and fallback label.
        let dash = classify_trace_step(&make_entry("8", EntryRole::Activity, "Deploy — staging", "08:27"));
        assert_eq!(dash.label, "Deploy");
        assert_eq!(dash.detail, "staging");
        let plain = classify_trace_step(&make_entry("9", EntryRole::Activity, "Idle", "08:28"));
        assert_eq!(plain.icon, TraceStepIcon::Status);
        assert_eq!(plain.label, "Idle");
    }

    #[test]
    fn trace_coalescing_merges_adjacent_identical_steps_preserves_reasoning() {
        // Exact scenario from threadTrace.test.ts.
        let tool = |id: &str, ts: &str| {
            let mut e = make_entry(id, EntryRole::Activity, "terminal completed", ts);
            e.activity_kind = Some(ActivityKind::Tool);
            classify_trace_step(&e)
        };
        let mut reasoning = make_entry("4", EntryRole::Activity, "Reasoning: Checking logs", "08:25");
        reasoning.activity_kind = Some(ActivityKind::Reasoning);
        reasoning.reasoning_detail = Some("Checking logs".into());
        let steps = vec![
            tool("1", "08:23"),
            tool("2", "08:24"),
            tool("3", "08:25"),
            classify_trace_step(&reasoning),
            tool("5", "08:26"),
        ];
        let coalesced = coalesce_consecutive_steps(&steps);
        assert_eq!(coalesced.len(), 3);
        assert_eq!(coalesced[0].repeat_count, 3);
        assert_eq!(coalesced[0].step.timestamp, "08:25");
        assert_eq!(coalesced[1].step.icon, TraceStepIcon::Reasoning);
        assert_eq!(coalesced[1].repeat_count, 1);
        assert_eq!(coalesced[2].repeat_count, 1);

        // Commentary rows never merge; case-insensitive label/detail matching.
        let mut c1 = make_entry("c1", EntryRole::Activity, "Commentary: same", "t1");
        c1.activity_kind = Some(ActivityKind::Commentary);
        let mut c2 = make_entry("c2", EntryRole::Activity, "Commentary: same", "t2");
        c2.activity_kind = Some(ActivityKind::Commentary);
        let cs = coalesce_consecutive_steps(&[classify_trace_step(&c1), classify_trace_step(&c2)]);
        assert_eq!(cs.len(), 2);

        let a = classify_trace_step(&make_entry("a", EntryRole::Activity, "Search Files: done", "t1"));
        let b = classify_trace_step(&make_entry("b", EntryRole::Activity, "search files: DONE", "t2"));
        let merged = coalesce_consecutive_steps(&[a, b]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].repeat_count, 2);
        assert_eq!(merged[0].step.timestamp, "t2");
    }

    #[test]
    fn sse_tool_event_coalescing_repeated_terminal_rows() {
        let ev = |label: &str, phase: &str, status: &str| ParsedToolEvent {
            phase: phase.into(),
            tool_name: "terminal".into(),
            label: label.into(),
            status: status.into(),
            summary: label.into(),
            duration_sec: None,
            error: None,
        };
        let events = vec![
            ev("terminal: cargo test", "started", "running"),
            ev("terminal completed", "completed", "done"),
            ev("terminal completed", "completed", "done"),
            ev("terminal completed", "completed", "done"),
        ];
        let coalesced = coalesce_tool_events(&events);
        // started row (detail "cargo test") differs from the completed rows.
        assert_eq!(coalesced.len(), 2);
        assert_eq!(coalesced[0].repeat_count, 1);
        assert_eq!(coalesced[0].step.detail, "cargo test");
        assert_eq!(coalesced[1].repeat_count, 3);
        assert_eq!(coalesced[1].step.label, "terminal");
        assert_eq!(coalesced[1].step.detail, "completed");
    }

    #[test]
    fn terminal_diagnostic_activity_detection() {
        // Non-activity roles never qualify.
        assert!(!is_terminal_diagnostic_activity(&make_entry("u", EntryRole::User, "hi", "t")));
        // Tool/reasoning/subagent/commentary activities never qualify.
        for kind in [ActivityKind::Tool, ActivityKind::Reasoning, ActivityKind::Subagent, ActivityKind::Commentary] {
            let mut e = make_entry("a", EntryRole::Activity, "terminal failed", "t");
            e.activity_kind = Some(kind);
            e.terminal_outcome = Some(TerminalOutcome::Failed);
            assert!(!is_terminal_diagnostic_activity(&e));
        }
        // Error code / failed / cancelled / admission_conflict / status error qualify.
        let mut e = make_entry("d1", EntryRole::Activity, "Gateway 409: admission_conflict", "t");
        e.error_code = Some("admission_conflict".into());
        e.terminal_outcome = Some(TerminalOutcome::AdmissionConflict);
        assert!(is_terminal_diagnostic_activity(&e));
        let mut e2 = make_entry("d2", EntryRole::Activity, "Run failed", "t");
        e2.terminal_outcome = Some(TerminalOutcome::Failed);
        assert!(is_terminal_diagnostic_activity(&e2));
        let mut e3 = make_entry("d3", EntryRole::Activity, "Stopped", "t");
        e3.terminal_outcome = Some(TerminalOutcome::Cancelled);
        assert!(is_terminal_diagnostic_activity(&e3));
        let mut e4 = make_entry("d4", EntryRole::Activity, "oops", "t");
        e4.activity_status = Some("error".into());
        assert!(is_terminal_diagnostic_activity(&e4));
        // Completed / interrupted outcomes and empty codes do not qualify.
        let mut e5 = make_entry("d5", EntryRole::Activity, "done", "t");
        e5.terminal_outcome = Some(TerminalOutcome::Completed);
        assert!(!is_terminal_diagnostic_activity(&e5));
        let mut e6 = make_entry("d6", EntryRole::Activity, "interrupted", "t");
        e6.terminal_outcome = Some(TerminalOutcome::Interrupted);
        assert!(!is_terminal_diagnostic_activity(&e6));
        let mut e7 = make_entry("d7", EntryRole::Activity, "x", "t");
        e7.error_code = Some("".into());
        assert!(!is_terminal_diagnostic_activity(&e7));
    }

    // -- Golden vector consumer (tests/vectors/hermes/*.json, non-blocking) ----

    /// Consumes cross-language golden vectors when the vector workstream has
    /// generated them. Recognized case shape:
    /// `{ "fn": "<name>", "input": <json>, "expected": <json>, ...args }`.
    /// Unknown functions/files are skipped, never failed — the inline fixtures
    /// above are the authoritative parity gate until vectors land.
    #[test]
    fn golden_vectors_hermes_if_present() {
        let manifest = env!("CARGO_MANIFEST_DIR");
        let candidates = [
            std::path::Path::new(manifest).join("../../tests/vectors/hermes"),
            std::path::Path::new(manifest).join("../../../tests/vectors/hermes"),
        ];
        let mut checked = 0usize;
        let mut skipped = 0usize;
        for dir in candidates {
            let Ok(entries) = std::fs::read_dir(&dir) else { continue };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                let Ok(text) = std::fs::read_to_string(&path) else { continue };
                let Ok(doc) = serde_json::from_str::<Value>(&text) else { continue };
                let cases: Vec<&Value> = if let Some(arr) = doc.as_array() {
                    arr.iter().collect()
                } else if let Some(arr) = doc.get("vectors").and_then(|v| v.as_array()) {
                    arr.iter().collect()
                } else if let Some(arr) = doc.get("cases").and_then(|v| v.as_array()) {
                    arr.iter().collect()
                } else {
                    vec![&doc]
                };
                for case in cases {
                    let Some(fname) = case.get("fn").and_then(|v| v.as_str()) else { skipped += 1; continue };
                    let input = case.get("input").cloned().unwrap_or(Value::Null);
                    let expected = case.get("expected").cloned().unwrap_or(Value::Null);
                    let ok = match fname {
                        "sanitize_session_key_header" => {
                            let got = sanitize_session_key_header(input.as_str());
                            got == expected.as_str().map(ToOwned::to_owned)
                        }
                        "is_opaque_session_identifier" => {
                            is_opaque_session_identifier(input.as_str()) == expected.as_bool().unwrap_or(false)
                        }
                        "classify_finish_reason" => {
                            let got = classify_finish_reason(
                                case.get("finish_reason").and_then(|v| v.as_str()),
                                case.get("error_code").and_then(|v| v.as_str()),
                                case.get("error_message").and_then(|v| v.as_str()),
                            );
                            serde_json::to_value(got).ok().as_ref() == Some(&expected)
                        }
                        "choose_gateway_transport" => {
                            let caps = case.get("capabilities").and_then(parse_capabilities_full);
                            let pref = match case.get("preferred").and_then(|v| v.as_str()).unwrap_or("auto") {
                                "runs" => PreferredTransport::Runs,
                                "session_stream" => PreferredTransport::SessionStream,
                                "chat_completions" => PreferredTransport::ChatCompletions,
                                _ => PreferredTransport::Auto,
                            };
                            let got = choose_gateway_transport(pref, caps.as_ref());
                            serde_json::to_value(got).ok().as_ref() == Some(&expected)
                        }
                        "build_gateway_url" => {
                            let cfg = ProfileGatewayConfig {
                                base_url: case.get("base_url").and_then(|v| v.as_str()).unwrap_or("http://127.0.0.1:8642").to_string(),
                                profile: "default".into(),
                                api_key: String::new(),
                                route_prefix: case.get("route_prefix").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                            };
                            let got = build_gateway_url(&cfg, input.as_str().unwrap_or("/"), case.get("unprefixed").and_then(|v| v.as_bool()).unwrap_or(false));
                            got == expected.as_str().unwrap_or("")
                        }
                        "parse_retry_after_ms" => {
                            let now = case.get("now_ms").and_then(|v| v.as_i64()).unwrap_or(0);
                            let got = parse_retry_after_ms(input.as_str(), now);
                            match (got, expected.as_u64()) {
                                (Some(g), Some(e)) => g == e,
                                (None, _) => expected.is_null(),
                                _ => false,
                            }
                        }
                        _ => { skipped += 1; continue }
                    };
                    assert!(ok, "golden vector mismatch in {} for fn={fname}: input={input}, expected={expected}", path.display());
                    checked += 1;
                }
            }
        }
        eprintln!("golden vectors: {checked} checked, {skipped} skipped (dirs absent or unrecognized cases are OK)");
    }
}

