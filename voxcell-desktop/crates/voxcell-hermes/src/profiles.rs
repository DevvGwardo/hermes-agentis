//! Multi-profile registry — port of `src/utils/profileRegistry.ts` (spec P1-02).
//! See docs/RUNE_DESKTOP_SPEC.md §5.1 / §7.3. Owned by the hermes-profiles workstream.
//!
//! Redaction guarantee: raw bearer tokens never leave this module. Definitions and
//! runtimes keep `api_key` private (no public getter, redacted `Debug`, no `Serialize`);
//! only [`HermesProfileSummary`] (with `has_api_key: bool`) is serializable for UI state.
//! The [`ProfileClientPool`] consumes keys internally to build per-profile clients.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fmt;

/// Gateway routing mode for a profile (TS `HermesProfileMode`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HermesProfileMode {
    /// Shares the primary gateway, routed under `/p/<profile>/`.
    Multiplexed,
    /// Dedicated gateway `base_url`, no route prefix.
    Standalone,
}

/// Where a profile definition came from (TS `HermesProfileSource`).
/// Merge precedence: `Custom` > `Discovered` > `Env`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HermesProfileSource {
    Env,
    Discovered,
    Custom,
}

/// Slugs that collide with gateway routes and must never be used as profile ids.
pub const RESERVED_PROFILE_SLUGS: [&str; 6] = ["api", "v1", "health", "p", "sessions", "runs"];

// Field-presence bits mirroring JS object-spread semantics: a key present with an
// `undefined` value still overwrites the previous entry, an absent key does not.
const F_LABEL: u8 = 1;
const F_BASE_URL: u8 = 2;
const F_API_KEY: u8 = 4;
const F_DESCRIPTION: u8 = 8;
const F_ALL_OPTIONAL: u8 = F_LABEL | F_BASE_URL | F_API_KEY | F_DESCRIPTION;

/// A profile definition before runtime resolution (TS `HermesProfileDefinition`).
/// `api_key` is private and never exposed; `Debug` redacts it.
#[derive(Clone, PartialEq, Eq)]
pub struct HermesProfileDefinition {
    id: String,
    label: Option<String>,
    mode: HermesProfileMode,
    base_url: Option<String>,
    api_key: Option<String>,
    source: HermesProfileSource,
    description: Option<String>,
    present: u8,
}

impl fmt::Debug for HermesProfileDefinition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HermesProfileDefinition")
            .field("id", &self.id)
            .field("label", &self.label)
            .field("mode", &self.mode)
            .field("base_url", &self.base_url)
            .field("api_key", &self.api_key.as_ref().map(|_| "[REDACTED]"))
            .field("source", &self.source)
            .field("description", &self.description)
            .finish()
    }
}

impl HermesProfileDefinition {
    /// Minimal definition (only `id`, `mode`, `source` keys present — mirrors a
    /// sparse JS object literal; absent optional keys survive merges).
    pub fn new(
        id: impl Into<String>,
        mode: HermesProfileMode,
        source: HermesProfileSource,
    ) -> Self {
        Self {
            id: id.into(),
            label: None,
            mode,
            base_url: None,
            api_key: None,
            source,
            description: None,
            present: 0,
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn label(&self) -> Option<&str> {
        self.label.as_deref()
    }
    pub fn mode(&self) -> HermesProfileMode {
        self.mode
    }
    pub fn base_url(&self) -> Option<&str> {
        self.base_url.as_deref()
    }
    pub fn source(&self) -> HermesProfileSource {
        self.source
    }
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }
    /// Whether a raw key is attached (never returns the key itself).
    pub fn has_api_key(&self) -> bool {
        self.api_key.as_deref().is_some_and(|k| !k.trim().is_empty())
    }

    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        let t = label.into().trim().to_string();
        self.label = if t.is_empty() { None } else { Some(t) };
        self.present |= F_LABEL;
        self
    }
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        let t = base_url.into().trim().to_string();
        self.base_url = if t.is_empty() { None } else { Some(t) };
        self.present |= F_BASE_URL;
        self
    }
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        let t = api_key.into().trim().to_string();
        self.api_key = if t.is_empty() { None } else { Some(t) };
        self.present |= F_API_KEY;
        self
    }
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        let t = description.into().trim().to_string();
        self.description = if t.is_empty() { None } else { Some(t) };
        self.present |= F_DESCRIPTION;
        self
    }

    /// Marks optional keys as present-without-value (JS `key: undefined`).
    fn with_field_presence(mut self, bits: u8) -> Self {
        self.present |= bits;
        self
    }
}

/// Trim + strip trailing slashes (TS `normalizeBaseUrl`).
pub fn normalize_base_url(raw: Option<&str>) -> String {
    raw.unwrap_or("").trim().trim_end_matches('/').to_string()
}

/// `http:` / `https:` absolute URL check (TS `isValidHttpUrl`).
pub fn is_valid_http_url(raw: Option<&str>) -> bool {
    let clean = normalize_base_url(raw);
    if clean.is_empty() {
        return false;
    }
    match reqwest::Url::parse(&clean) {
        Ok(u) => u.scheme() == "http" || u.scheme() == "https",
        Err(_) => false,
    }
}

/// Slug normalization with reserved-route guard (TS `normalizeProfileId`).
/// Empty / reserved slugs collapse to `"default"`.
pub fn normalize_profile_id(raw: Option<&str>) -> String {
    let lowered = raw.unwrap_or("").trim().to_lowercase();
    // Replace each run of characters outside [a-z0-9_-] with a single '-'.
    let mut cleaned = String::with_capacity(lowered.len());
    let mut in_invalid_run = false;
    for c in lowered.chars() {
        if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
            cleaned.push(c);
            in_invalid_run = false;
        } else if !in_invalid_run {
            cleaned.push('-');
            in_invalid_run = true;
        }
    }
    // Strip leading/trailing dashes BEFORE the 40-char cut (TS order); the cut
    // itself may leave a trailing dash, which TS does not re-strip.
    let cut: String = cleaned.trim_matches('-').chars().take(40).collect();
    if cut.is_empty() || RESERVED_PROFILE_SLUGS.contains(&cut.as_str()) {
        return "default".to_string();
    }
    cut
}

/// Custom (UI-registered) slug validation (TS `isValidCustomProfileSlug`).
/// Equivalent to `/^[a-z0-9][a-z0-9_-]{0,38}[a-z0-9]?$/` after trim+lowercase:
/// 1..=40 chars, first char alphanumeric, all chars in `[a-z0-9_-]` (the optional
/// trailing class lets the middle group absorb trailing dashes, so `a-` is valid).
pub fn is_valid_custom_profile_slug(raw: Option<&str>) -> bool {
    let trimmed = raw.unwrap_or("").trim().to_lowercase();
    if trimmed.is_empty()
        || trimmed == "default"
        || RESERVED_PROFILE_SLUGS.contains(&trimmed.as_str())
    {
        return false;
    }
    let mut chars = trimmed.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_alphanumeric() || trimmed.chars().count() > 40 {
        return false;
    }
    trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Parse `HERMES_PROFILES` as a JSON array or CSV shorthand
/// (TS `parseHermesProfilesEnv`). Invalid JSON falls through to the CSV parser.
pub fn parse_hermes_profiles_env(env: &HashMap<String, String>) -> Vec<HermesProfileDefinition> {
    let raw = env.get("HERMES_PROFILES").map(|s| s.trim()).unwrap_or("");
    if raw.is_empty() {
        return Vec::new();
    }

    let mut defs: Vec<HermesProfileDefinition> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    if raw.starts_with('[') {
        if let Ok(serde_json::Value::Array(items)) = serde_json::from_str::<serde_json::Value>(raw)
        {
            for item in items {
                let Some(rec) = item.as_object() else {
                    continue;
                };
                let id_raw = rec
                    .get("id")
                    .and_then(|v| v.as_str())
                    .or_else(|| rec.get("name").and_then(|v| v.as_str()))
                    .unwrap_or("");
                let id = normalize_profile_id(Some(id_raw));
                if !seen.insert(id.clone()) {
                    continue;
                }
                let url_str = rec.get("baseUrl").and_then(|v| v.as_str());
                let has_url = url_str.is_some_and(|u| is_valid_http_url(Some(u)));
                let mode = if rec.get("mode").and_then(|v| v.as_str()) == Some("standalone")
                    || has_url
                {
                    HermesProfileMode::Standalone
                } else {
                    HermesProfileMode::Multiplexed
                };
                // JSON records carry every key (present-with-undefined overwrites
                // earlier entries on merge), unlike the sparse CSV records.
                let mut def = HermesProfileDefinition::new(id, mode, HermesProfileSource::Env)
                    .with_field_presence(F_ALL_OPTIONAL);
                if let Some(l) = rec.get("label").and_then(|v| v.as_str()) {
                    def = def.with_label(l);
                }
                if has_url {
                    def = def.with_base_url(normalize_base_url(url_str));
                }
                if let Some(k) = rec.get("apiKey").and_then(|v| v.as_str()) {
                    def = def.with_api_key(k);
                }
                if let Some(d) = rec.get("description").and_then(|v| v.as_str()) {
                    def = def.with_description(d);
                }
                defs.push(def);
            }
            return defs;
        }
        // Invalid JSON (or a non-array payload): fall through to the CSV parser.
    }

    for token in raw.split(',') {
        let part = token.trim();
        if part.is_empty() {
            continue;
        }
        let (name_part, val_part) = match part.split_once('=') {
            Some((n, v)) => (n.trim(), v.trim()),
            None => (part, ""),
        };
        let id = normalize_profile_id(Some(name_part));
        if !seen.insert(id.clone()) {
            continue;
        }
        let has_url = is_valid_http_url(Some(val_part));
        let mode = if has_url {
            HermesProfileMode::Standalone
        } else {
            HermesProfileMode::Multiplexed
        };
        // CSV records only carry id/mode/baseUrl/source keys.
        let mut def = HermesProfileDefinition::new(id, mode, HermesProfileSource::Env)
            .with_field_presence(F_BASE_URL);
        if has_url {
            def = def.with_base_url(normalize_base_url(Some(val_part)));
        }
        defs.push(def);
    }
    defs
}

// ---------------------------------------------------------------------------
// Runtime resolution, catalog merge, redacted summaries
// ---------------------------------------------------------------------------

use crate::protocol::{resolve_profile_gateway_config, ProfileGatewayConfig};

/// Resolved per-profile routing (TS `HermesProfileRuntime`). The raw bearer key
/// stays private to this module; only [`HermesProfileSummary`] crosses to UI state.
#[derive(Clone, PartialEq, Eq)]
pub struct HermesProfileRuntime {
    pub id: String,
    pub label: String,
    pub mode: HermesProfileMode,
    pub base_url: String,
    pub route_prefix: String,
    api_key: String,
    pub source: HermesProfileSource,
    pub is_default: bool,
    pub description: Option<String>,
}

impl fmt::Debug for HermesProfileRuntime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HermesProfileRuntime")
            .field("id", &self.id)
            .field("label", &self.label)
            .field("mode", &self.mode)
            .field("base_url", &self.base_url)
            .field("route_prefix", &self.route_prefix)
            .field("api_key", &if self.api_key.is_empty() { "" } else { "[REDACTED]" })
            .field("source", &self.source)
            .field("is_default", &self.is_default)
            .field("description", &self.description)
            .finish()
    }
}

impl HermesProfileRuntime {
    pub fn has_api_key(&self) -> bool {
        !self.api_key.is_empty()
    }

    /// Redacted projection safe for UI state (TS `toRedactedProfileSummary`).
    pub fn to_redacted_summary(
        &self,
        status: Option<&ProfileGatewayStatus>,
    ) -> HermesProfileSummary {
        to_redacted_profile_summary(self, status)
    }
}

/// Minimal gateway-status projection consumed by redacted summaries. The web
/// `HermesGatewayStatus` carries many more fields; only these four reach the
/// profile summary (TS `toRedactedProfileSummary` status spread).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProfileGatewayStatus {
    pub connected: bool,
    pub active_model: Option<String>,
    pub health_status: Option<String>,
    pub error: Option<String>,
}

/// UI-safe profile metadata (TS `HermesProfileSummary`, camelCase wire format).
/// Never contains a raw bearer token — only `has_api_key`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HermesProfileSummary {
    pub id: String,
    pub label: String,
    pub mode: HermesProfileMode,
    pub base_url: String,
    pub route_prefix: String,
    pub source: HermesProfileSource,
    pub has_api_key: bool,
    pub is_default: bool,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub connected: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub active_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub health_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub error: Option<String>,
}

/// Merged profile catalog (TS `buildProfileCatalog` return shape).
#[derive(Clone, Debug)]
pub struct ProfileCatalog {
    pub default_profile_id: String,
    pub runtimes: Vec<HermesProfileRuntime>,
}

impl ProfileCatalog {
    pub fn runtime(&self, id: &str) -> Option<&HermesProfileRuntime> {
        self.runtimes.iter().find(|r| r.id == id)
    }

    pub fn summaries(&self) -> Vec<HermesProfileSummary> {
        self.runtimes
            .iter()
            .map(|r| r.to_redacted_summary(None))
            .collect()
    }
}

fn primary_gateway_config(env: &HashMap<String, String>) -> ProfileGatewayConfig {
    resolve_profile_gateway_config(
        env.get("HERMES_API_URL").map(String::as_str),
        env.get("HERMES_PROFILE").map(String::as_str),
        env.get("HERMES_API_KEY").map(String::as_str),
        env.get("HERMES_PROFILE_API_KEY").map(String::as_str),
        env,
    )
}

/// `HERMES_API_KEY_<SLUG>` env name: runs of non-alphanumerics collapse to one
/// `_`, uppercased (TS `id.replace(/[^a-zA-Z0-9]+/g, '_').toUpperCase()`).
fn profile_key_env_name(id: &str) -> String {
    let mut slug = String::with_capacity(id.len());
    let mut prev_underscore = false;
    for c in id.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_uppercase());
            prev_underscore = false;
        } else if !prev_underscore {
            slug.push('_');
            prev_underscore = true;
        }
    }
    format!("HERMES_API_KEY_{slug}")
}

/// JS `encodeURIComponent` (unreserved set includes `! ~ * ' ( )`).
fn encode_uri_component(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for b in raw.as_bytes() {
        match b {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'!'
            | b'~'
            | b'*'
            | b'\''
            | b'('
            | b')' => out.push(*b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn trimmed_non_empty(raw: Option<&str>) -> Option<&str> {
    raw.map(str::trim).filter(|s| !s.is_empty())
}

/// Resolve gateway routing for one definition (TS `resolveProfileRuntimeConfig`).
pub fn resolve_profile_runtime_config(
    def: &HermesProfileDefinition,
    env: &HashMap<String, String>,
) -> HermesProfileRuntime {
    let primary = primary_gateway_config(env);
    // TS: normalizeProfileId(def.id || primary.profile) — empty id adopts the
    // active HERMES_PROFILE (raw, pre-normalization).
    let raw_id = if def.id.is_empty() {
        primary.profile.as_str()
    } else {
        def.id.as_str()
    };
    let id = normalize_profile_id(Some(raw_id));
    let is_default = id == "default";
    let standalone_url = is_valid_http_url(def.base_url());
    let mode = if is_default || (def.mode() == HermesProfileMode::Standalone && standalone_url) {
        HermesProfileMode::Standalone
    } else {
        HermesProfileMode::Multiplexed
    };
    // Standalone routing needs a valid dedicated URL; the default profile and
    // invalid/missing standalone URLs fall back to the primary gateway.
    let base_url = if !is_default && mode == HermesProfileMode::Standalone && standalone_url {
        normalize_base_url(def.base_url())
    } else {
        primary.base_url
    };
    let route_prefix = if is_default || mode == HermesProfileMode::Standalone {
        String::new()
    } else {
        format!("/p/{}", encode_uri_component(&id))
    };

    // Key precedence: explicit > HERMES_API_KEY_<SLUG> > shared
    // HERMES_PROFILE_API_KEY (non-default only) > HERMES_API_KEY.
    let explicit_key = trimmed_non_empty(def.api_key.as_deref()).unwrap_or("");
    let env_profile_key =
        trimmed_non_empty(env.get(&profile_key_env_name(&id)).map(String::as_str)).unwrap_or("");
    let shared_named_profile_key = if is_default {
        ""
    } else {
        trimmed_non_empty(env.get("HERMES_PROFILE_API_KEY").map(String::as_str)).unwrap_or("")
    };
    let fallback_default_key = env.get("HERMES_API_KEY").map(|s| s.trim()).unwrap_or("");
    let api_key = [
        explicit_key,
        env_profile_key,
        shared_named_profile_key,
        fallback_default_key,
    ]
    .into_iter()
    .find(|k| !k.is_empty())
    .unwrap_or("")
    .to_string();

    HermesProfileRuntime {
        label: trimmed_non_empty(def.label()).unwrap_or(&id).to_string(),
        id,
        mode,
        base_url,
        route_prefix,
        api_key,
        source: def.source(),
        is_default,
        description: def.description.clone(),
    }
}

/// JS object-spread merge: keys present on `def` (even with `None` values)
/// overwrite `prev`; absent keys keep the previous value.
fn merge_spread(prev: Option<&HermesProfileDefinition>, def: &HermesProfileDefinition) -> HermesProfileDefinition {
    let mut out = def.clone();
    if let Some(p) = prev {
        if def.present & F_LABEL == 0 {
            out.label = p.label.clone();
        }
        if def.present & F_BASE_URL == 0 {
            out.base_url = p.base_url.clone();
        }
        if def.present & F_API_KEY == 0 {
            out.api_key = p.api_key.clone();
        }
        if def.present & F_DESCRIPTION == 0 {
            out.description = p.description.clone();
        }
        out.present |= p.present;
    }
    out
}

/// Merge env / discovered / custom definitions with `custom > discovered > env`
/// precedence (TS `buildProfileCatalog`).
pub fn build_profile_catalog(
    env: &HashMap<String, String>,
    discovered: &[HermesProfileDefinition],
    custom: &[HermesProfileDefinition],
) -> ProfileCatalog {
    use std::cmp::Ordering;

    let primary = primary_gateway_config(env);
    let default_profile_id = normalize_profile_id(Some(primary.profile.as_str()));

    // Always seed `default` (or the active HERMES_PROFILE) first. Every optional
    // key is present on the seed, so later spreads can overwrite it.
    let seed = HermesProfileDefinition::new(
        default_profile_id.clone(),
        if default_profile_id == "default" {
            HermesProfileMode::Standalone
        } else {
            HermesProfileMode::Multiplexed
        },
        HermesProfileSource::Env,
    )
    .with_label(default_profile_id.clone())
    .with_base_url(primary.base_url.clone())
    .with_api_key(primary.api_key.clone())
    .with_description(if default_profile_id == "default" {
        "Primary ~/.hermes workspace".to_string()
    } else {
        format!("Primary HERMES_PROFILE={default_profile_id}")
    });

    let mut by_id: HashMap<String, HermesProfileDefinition> = HashMap::new();
    by_id.insert(default_profile_id.clone(), seed);

    // Env defs: plain spread (ids already normalized by the parser).
    for def in parse_hermes_profiles_env(env) {
        let id = def.id().to_string();
        let merged = merge_spread(by_id.get(&id), &def);
        by_id.insert(id, merged);
    }

    // Discovered defs: spread + normalized id + label `prev || def || id`.
    for def in discovered {
        let id = normalize_profile_id(Some(def.id()));
        if id.is_empty() {
            continue;
        }
        let prev = by_id.get(&id);
        let mut merged = merge_spread(prev, def);
        merged.id = id.clone();
        merged.label = Some(
            prev.and_then(|p| p.label.clone())
                .or_else(|| def.label.clone())
                .unwrap_or_else(|| id.clone()),
        );
        merged.present |= F_LABEL;
        by_id.insert(id, merged);
    }

    // Custom defs: spread + normalized id + key `def.trim() || prev` (an empty
    // custom key falls back to the previously merged key instead of clearing it).
    for def in custom {
        let id = normalize_profile_id(Some(def.id()));
        if id.is_empty() {
            continue;
        }
        let prev = by_id.get(&id);
        let mut merged = merge_spread(prev, def);
        merged.id = id.clone();
        merged.api_key = trimmed_non_empty(def.api_key.as_deref())
            .map(str::to_string)
            .or_else(|| prev.and_then(|p| p.api_key.clone()));
        merged.present |= F_API_KEY;
        by_id.insert(id, merged);
    }

    let mut runtimes: Vec<HermesProfileRuntime> = by_id
        .values()
        .map(|def| resolve_profile_runtime_config(def, env))
        .collect();
    // Default-first, then alphabetical (TS sort comparator).
    runtimes.sort_by(|a, b| {
        if a.id == default_profile_id {
            Ordering::Less
        } else if b.id == default_profile_id {
            Ordering::Greater
        } else {
            a.id.cmp(&b.id)
        }
    });

    ProfileCatalog {
        default_profile_id,
        runtimes,
    }
}

/// Redact a runtime into UI-safe metadata (TS `toRedactedProfileSummary`).
pub fn to_redacted_profile_summary(
    runtime: &HermesProfileRuntime,
    status: Option<&ProfileGatewayStatus>,
) -> HermesProfileSummary {
    HermesProfileSummary {
        id: runtime.id.clone(),
        label: runtime.label.clone(),
        mode: runtime.mode,
        base_url: runtime.base_url.clone(),
        route_prefix: runtime.route_prefix.clone(),
        source: runtime.source,
        has_api_key: runtime.has_api_key(),
        is_default: runtime.is_default,
        // TS truthy spread: empty strings are omitted, not serialized as "".
        description: runtime.description.clone().filter(|d| !d.is_empty()),
        connected: status.map(|s| s.connected),
        // activeModel/healthStatus spread verbatim (an empty string is a value);
        // error uses a truthy spread like description.
        active_model: status.and_then(|s| s.active_model.clone()),
        health_status: status.and_then(|s| s.health_status.clone()),
        error: status
            .and_then(|s| s.error.clone())
            .filter(|e| !e.is_empty()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    // ---- Mirrors profileRegistry.test.ts test 1 -----------------------------

    #[test]
    fn ts_normalize_profile_id_normalizes_valid_slugs_and_guards_reserved_routes() {
        assert_eq!(normalize_profile_id(Some("Coder Agent!")), "coder-agent");
        assert_eq!(normalize_profile_id(Some("research_v2")), "research_v2");
        assert_eq!(normalize_profile_id(Some("")), "default");
        assert_eq!(normalize_profile_id(Some("api")), "default");
        assert_eq!(normalize_profile_id(Some("v1")), "default");

        assert!(is_valid_custom_profile_slug(Some("coder")));
        assert!(!is_valid_custom_profile_slug(Some("default")));
        assert!(!is_valid_custom_profile_slug(Some("v1")));
    }

    // ---- Mirrors profileRegistry.test.ts test 2 -----------------------------

    #[test]
    fn ts_parse_hermes_profiles_env_parses_csv_and_json_specs_accurately() {
        let csv = parse_hermes_profiles_env(&env(&[(
            "HERMES_PROFILES",
            "coder,researcher=http://127.0.0.1:8643/",
        )]));
        assert_eq!(csv.len(), 2);
        assert_eq!(csv[0].id(), "coder");
        assert_eq!(csv[0].mode(), HermesProfileMode::Multiplexed);
        assert_eq!(csv[0].base_url(), None);
        assert_eq!(csv[1].id(), "researcher");
        assert_eq!(csv[1].mode(), HermesProfileMode::Standalone);
        assert_eq!(csv[1].base_url(), Some("http://127.0.0.1:8643"));

        let json = parse_hermes_profiles_env(&env(&[(
            "HERMES_PROFILES",
            r#"[{"id":"ops","label":"Ops Profile","mode":"multiplexed"},{"id":"sandbox","mode":"standalone","baseUrl":"http://127.0.0.1:8649","apiKey":"sk-sbx"}]"#,
        )]));
        assert_eq!(json.len(), 2);
        assert_eq!(json[0].label(), Some("Ops Profile"));
        assert_eq!(json[0].mode(), HermesProfileMode::Multiplexed);
        assert_eq!(json[1].base_url(), Some("http://127.0.0.1:8649"));
        assert!(json[1].has_api_key());
    }

    // ---- Edge cases verified against the TS reference -----------------------

    #[test]
    fn normalize_truncates_to_40_chars_after_dash_strip() {
        assert_eq!(normalize_profile_id(Some(&"a".repeat(45))), "a".repeat(40));
        // Cut lands on a dash introduced by normalization; TS does not re-strip.
        assert_eq!(
            normalize_profile_id(Some(&format!("{} bcd", "a".repeat(39)))),
            format!("{}-", "a".repeat(39))
        );
    }

    #[test]
    fn normalize_collapses_invalid_runs_and_lowercases_unicode() {
        assert_eq!(normalize_profile_id(Some("a!-!b")), "a---b");
        assert_eq!(normalize_profile_id(Some("Coder.Agent!!")), "coder-agent");
        assert_eq!(normalize_profile_id(Some("Café Ångström")), "caf-ngstr-m");
        assert_eq!(normalize_profile_id(Some("   ")), "default");
        assert_eq!(normalize_profile_id(None), "default");
        assert_eq!(normalize_profile_id(Some("--x--")), "x");
    }

    #[test]
    fn normalize_guards_every_reserved_slug() {
        for slug in RESERVED_PROFILE_SLUGS {
            assert_eq!(normalize_profile_id(Some(slug)), "default", "{slug}");
            assert_eq!(
                normalize_profile_id(Some(&slug.to_uppercase())),
                "default",
                "{slug} uppercase"
            );
        }
    }

    #[test]
    fn custom_slug_validation_edges() {
        assert!(is_valid_custom_profile_slug(Some("a")));
        assert!(is_valid_custom_profile_slug(Some(&"a".repeat(40))));
        assert!(!is_valid_custom_profile_slug(Some(&"a".repeat(41))));
        // The TS regex admits trailing dashes via the middle character class.
        assert!(is_valid_custom_profile_slug(Some("a-")));
        assert!(is_valid_custom_profile_slug(Some("a--b")));
        assert!(!is_valid_custom_profile_slug(Some("-a")));
        assert!(is_valid_custom_profile_slug(Some("CODER")));
        assert!(!is_valid_custom_profile_slug(Some("coder!")));
        assert!(!is_valid_custom_profile_slug(Some("")));
        assert!(!is_valid_custom_profile_slug(None));
        for slug in RESERVED_PROFILE_SLUGS {
            assert!(!is_valid_custom_profile_slug(Some(slug)), "{slug}");
        }
    }

    #[test]
    fn base_url_normalization_strips_trailing_slashes_and_trims() {
        assert_eq!(
            normalize_base_url(Some("  http://x.com///  ")),
            "http://x.com"
        );
        assert_eq!(normalize_base_url(Some("")), "");
        assert_eq!(normalize_base_url(None), "");
    }

    #[test]
    fn http_url_validation_accepts_only_absolute_http_urls() {
        assert!(is_valid_http_url(Some("http://127.0.0.1:8642/")));
        assert!(is_valid_http_url(Some("https://gateway.example.com")));
        assert!(is_valid_http_url(Some("  http://x.com/  ")));
        assert!(!is_valid_http_url(Some("ftp://x.com")));
        assert!(!is_valid_http_url(Some("ws://x.com")));
        assert!(!is_valid_http_url(Some("http://")));
        assert!(!is_valid_http_url(Some("x.com")));
        assert!(!is_valid_http_url(Some("")));
        assert!(!is_valid_http_url(None));
    }

    #[test]
    fn parse_invalid_json_falls_through_to_csv() {
        let defs = parse_hermes_profiles_env(&env(&[("HERMES_PROFILES", "[oops,coder")]));
        assert_eq!(
            defs.iter().map(|d| d.id()).collect::<Vec<_>>(),
            ["oops", "coder"]
        );
        assert!(defs.iter().all(|d| d.mode() == HermesProfileMode::Multiplexed));
    }

    #[test]
    fn parse_dedupes_normalized_ids_and_maps_reserved_to_default() {
        let defs = parse_hermes_profiles_env(&env(&[("HERMES_PROFILES", "coder,Coder!,api")]));
        assert_eq!(
            defs.iter().map(|d| d.id()).collect::<Vec<_>>(),
            ["coder", "default"]
        );
    }

    #[test]
    fn parse_json_name_fallback_mode_inference_and_trimming() {
        let defs = parse_hermes_profiles_env(&env(&[(
            "HERMES_PROFILES",
            r#"[{"name":"Ops Team"},{"id":"x","mode":"standalone"},{"id":"y","baseUrl":"http://127.0.0.1:9000/","label":"  Y  ","apiKey":"  k  ","description":"  d  "},42,null]"#,
        )]));
        assert_eq!(defs.len(), 3);
        assert_eq!(defs[0].id(), "ops-team");
        assert_eq!(defs[0].mode(), HermesProfileMode::Multiplexed);
        // Explicit standalone without a valid URL stays standalone at definition level.
        assert_eq!(defs[1].mode(), HermesProfileMode::Standalone);
        assert_eq!(defs[1].base_url(), None);
        // A valid baseUrl infers standalone even without an explicit mode.
        assert_eq!(defs[2].mode(), HermesProfileMode::Standalone);
        assert_eq!(defs[2].base_url(), Some("http://127.0.0.1:9000"));
        assert_eq!(defs[2].label(), Some("Y"));
        assert!(defs[2].has_api_key());
        assert_eq!(defs[2].description(), Some("d"));
        assert!(defs.iter().all(|d| d.source() == HermesProfileSource::Env));
    }

    #[test]
    fn parse_json_non_array_falls_through_to_csv() {
        let defs = parse_hermes_profiles_env(&env(&[("HERMES_PROFILES", r#"{"id":"x"}"#)]));
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].id(), "id-x");
    }

    #[test]
    fn parse_json_array_of_non_objects_yields_empty() {
        let defs = parse_hermes_profiles_env(&env(&[("HERMES_PROFILES", r#"["a","b"]"#)]));
        assert!(defs.is_empty());
    }

    #[test]
    fn parse_empty_or_missing_env_yields_empty() {
        assert!(parse_hermes_profiles_env(&env(&[])).is_empty());
        assert!(parse_hermes_profiles_env(&env(&[("HERMES_PROFILES", "   ")])).is_empty());
    }

    #[test]
    fn parse_csv_equals_multiplexed_keyword_is_not_a_url() {
        let defs = parse_hermes_profiles_env(&env(&[("HERMES_PROFILES", "coder=multiplexed")]));
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].id(), "coder");
        assert_eq!(defs[0].mode(), HermesProfileMode::Multiplexed);
        assert_eq!(defs[0].base_url(), None);
    }

    // ---- Mirrors profileRegistry.test.ts test 3 -----------------------------

    #[test]
    fn ts_resolve_routes_multiplexed_under_p_slug_and_prioritizes_profile_keys() {
        let e = env(&[
            ("HERMES_API_URL", "http://127.0.0.1:8642"),
            ("HERMES_API_KEY", "default-key"),
            ("HERMES_PROFILE_API_KEY", "shared-profile-key"),
            ("HERMES_API_KEY_CODER", "coder-specific-key"),
        ]);

        let coder = resolve_profile_runtime_config(
            &HermesProfileDefinition::new("coder", HermesProfileMode::Multiplexed, HermesProfileSource::Env),
            &e,
        );
        assert_eq!(coder.base_url, "http://127.0.0.1:8642");
        assert_eq!(coder.route_prefix, "/p/coder");
        assert_eq!(coder.api_key, "coder-specific-key");

        let researcher = resolve_profile_runtime_config(
            &HermesProfileDefinition::new("researcher", HermesProfileMode::Multiplexed, HermesProfileSource::Discovered),
            &e,
        );
        assert_eq!(researcher.route_prefix, "/p/researcher");
        assert_eq!(researcher.api_key, "shared-profile-key");

        let standalone = resolve_profile_runtime_config(
            &HermesProfileDefinition::new("isolated", HermesProfileMode::Standalone, HermesProfileSource::Custom)
                .with_base_url("http://127.0.0.1:8655/")
                .with_api_key("explicit-standalone-key"),
            &e,
        );
        assert_eq!(standalone.base_url, "http://127.0.0.1:8655");
        assert_eq!(standalone.route_prefix, "");
        assert_eq!(standalone.api_key, "explicit-standalone-key");
    }

    // ---- Mirrors profileRegistry.test.ts test 4 -----------------------------

    #[test]
    fn ts_redacted_summary_never_exposes_api_key_in_serialized_output() {
        let catalog = build_profile_catalog(
            &env(&[
                ("HERMES_API_URL", "http://127.0.0.1:8642"),
                ("HERMES_API_KEY", "super-secret-default"),
                ("HERMES_PROFILES", "coder"),
                ("HERMES_API_KEY_CODER", "super-secret-coder"),
            ]),
            &[],
            &[HermesProfileDefinition::new("lab", HermesProfileMode::Standalone, HermesProfileSource::Custom)
                .with_base_url("http://127.0.0.1:8660")
                .with_api_key("secret-lab-token")],
        );

        let summaries = catalog.summaries();
        assert_eq!(summaries.len(), 3);
        for s in &summaries {
            let json = serde_json::to_string(s).unwrap();
            assert!(!json.contains("apiKey"));
            assert!(!json.contains("super-secret"));
            assert!(!json.contains("secret-lab-token"));
            assert!(s.has_api_key);
        }
    }

    // ---- Resolution edges ----------------------------------------------------

    #[test]
    fn resolve_key_precedence_explicit_over_slug_over_shared_over_default() {
        let e = env(&[
            ("HERMES_API_KEY", "fallback"),
            ("HERMES_PROFILE_API_KEY", "shared"),
            ("HERMES_API_KEY_LAB", "slug-key"),
        ]);
        let def = HermesProfileDefinition::new("lab", HermesProfileMode::Multiplexed, HermesProfileSource::Env);
        assert_eq!(resolve_profile_runtime_config(&def, &e).api_key, "slug-key");
        assert_eq!(
            resolve_profile_runtime_config(&def.clone().with_api_key("explicit"), &e).api_key,
            "explicit"
        );
        assert_eq!(
            resolve_profile_runtime_config(&def, &env(&[("HERMES_API_KEY", "fallback"), ("HERMES_PROFILE_API_KEY", "shared")])).api_key,
            "shared"
        );
        assert_eq!(
            resolve_profile_runtime_config(&def, &env(&[("HERMES_API_KEY", "fallback")])).api_key,
            "fallback"
        );
        assert_eq!(resolve_profile_runtime_config(&def, &env(&[])).api_key, "");
        // Whitespace-only explicit key falls through the chain.
        assert_eq!(
            resolve_profile_runtime_config(&def.clone().with_api_key("   "), &e).api_key,
            "slug-key"
        );
    }

    #[test]
    fn resolve_default_profile_ignores_shared_named_profile_key() {
        let e = env(&[("HERMES_API_KEY", "dk"), ("HERMES_PROFILE_API_KEY", "shared")]);
        let rt = resolve_profile_runtime_config(
            &HermesProfileDefinition::new("default", HermesProfileMode::Multiplexed, HermesProfileSource::Env),
            &e,
        );
        assert_eq!(rt.mode, HermesProfileMode::Standalone);
        assert_eq!(rt.route_prefix, "");
        assert_eq!(rt.api_key, "dk");
        assert!(rt.is_default);
    }

    #[test]
    fn resolve_slug_env_key_collapses_non_alphanumeric_runs() {
        // TS replaces runs of non-alphanumerics with a single underscore.
        let e = env(&[("HERMES_API_KEY_RESEARCH_V2", "dash-key")]);
        let rt = resolve_profile_runtime_config(
            &HermesProfileDefinition::new("research-v2", HermesProfileMode::Multiplexed, HermesProfileSource::Env),
            &e,
        );
        assert_eq!(rt.api_key, "dash-key");
        let e2 = env(&[("HERMES_API_KEY_A_B", "run-key")]);
        let rt2 = resolve_profile_runtime_config(
            &HermesProfileDefinition::new("a--b", HermesProfileMode::Multiplexed, HermesProfileSource::Env),
            &e2,
        );
        assert_eq!(rt2.api_key, "run-key");
    }

    #[test]
    fn resolve_standalone_without_valid_url_falls_back_to_multiplexed() {
        let e = env(&[("HERMES_API_URL", "http://127.0.0.1:8642")]);
        let rt = resolve_profile_runtime_config(
            &HermesProfileDefinition::new("x", HermesProfileMode::Standalone, HermesProfileSource::Env)
                .with_base_url("not-a-url"),
            &e,
        );
        assert_eq!(rt.mode, HermesProfileMode::Multiplexed);
        assert_eq!(rt.base_url, "http://127.0.0.1:8642");
        assert_eq!(rt.route_prefix, "/p/x");
    }

    #[test]
    fn resolve_empty_def_id_uses_primary_hermes_profile() {
        let e = env(&[("HERMES_PROFILE", "agentis"), ("HERMES_API_URL", "http://127.0.0.1:8642/")]);
        let rt = resolve_profile_runtime_config(
            &HermesProfileDefinition::new("", HermesProfileMode::Multiplexed, HermesProfileSource::Env),
            &e,
        );
        assert_eq!(rt.id, "agentis");
        assert_eq!(rt.route_prefix, "/p/agentis");
        assert_eq!(rt.base_url, "http://127.0.0.1:8642");
        assert!(!rt.is_default);
    }

    #[test]
    fn resolve_label_falls_back_to_id_and_trims() {
        let rt = resolve_profile_runtime_config(
            &HermesProfileDefinition::new("lab", HermesProfileMode::Multiplexed, HermesProfileSource::Env)
                .with_label("   "),
            &env(&[]),
        );
        assert_eq!(rt.label, "lab");
        let rt2 = resolve_profile_runtime_config(
            &HermesProfileDefinition::new("lab", HermesProfileMode::Multiplexed, HermesProfileSource::Env)
                .with_label("  Lab Name  "),
            &env(&[]),
        );
        assert_eq!(rt2.label, "Lab Name");
    }

    // ---- Catalog merge -------------------------------------------------------

    #[test]
    fn catalog_seeds_default_and_sorts_default_first_then_alphabetically() {
        let cat = build_profile_catalog(&env(&[("HERMES_PROFILES", "zeta,alpha,coder")]), &[], &[]);
        assert_eq!(cat.default_profile_id, "default");
        assert_eq!(
            cat.runtimes.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            ["default", "alpha", "coder", "zeta"]
        );
        let def = cat.runtime("default").unwrap();
        assert_eq!(def.description.as_deref(), Some("Primary ~/.hermes workspace"));
        assert_eq!(def.mode, HermesProfileMode::Standalone);
        assert_eq!(def.base_url, "http://127.0.0.1:8642");
    }

    #[test]
    fn catalog_hermes_profile_seed_and_csv_env_merge_keep_seed_fields() {
        // Probe env-over-seed: CSV defs lack label/apiKey/description keys, so the
        // seeded primary fields survive the spread merge.
        let cat = build_profile_catalog(
            &env(&[
                ("HERMES_PROFILE", "coder"),
                ("HERMES_API_URL", "http://127.0.0.1:8642"),
                ("HERMES_API_KEY", "dk"),
                ("HERMES_PROFILES", "coder"),
            ]),
            &[],
            &[],
        );
        assert_eq!(cat.default_profile_id, "coder");
        assert_eq!(cat.runtimes.len(), 1);
        let rt = &cat.runtimes[0];
        assert_eq!(rt.id, "coder");
        assert_eq!(rt.label, "coder");
        assert_eq!(rt.description.as_deref(), Some("Primary HERMES_PROFILE=coder"));
        assert_eq!(rt.api_key, "dk");
        assert_eq!(rt.source, HermesProfileSource::Env);
        assert_eq!(rt.route_prefix, "/p/coder");
    }

    #[test]
    fn catalog_json_env_def_overwrites_seed_optional_fields() {
        // JSON records carry every key: present-with-undefined clears the seed's
        // description (JS spread semantics), unlike sparse CSV records.
        let cat = build_profile_catalog(
            &env(&[
                ("HERMES_PROFILE", "coder"),
                ("HERMES_API_KEY", "dk"),
                ("HERMES_PROFILES", r#"[{"id":"coder"}]"#),
            ]),
            &[],
            &[],
        );
        let rt = cat.runtime("coder").unwrap();
        assert_eq!(rt.description, None);
        assert_eq!(rt.label, "coder");
        assert_eq!(rt.api_key, "dk");
    }

    #[test]
    fn catalog_reserved_primary_normalizes_to_default() {
        let cat = build_profile_catalog(&env(&[("HERMES_PROFILE", "API")]), &[], &[]);
        assert_eq!(cat.default_profile_id, "default");
        assert_eq!(cat.runtimes.len(), 1);
        assert_eq!(cat.runtimes[0].route_prefix, "");
        assert_eq!(cat.runtimes[0].mode, HermesProfileMode::Standalone);
    }

    #[test]
    fn catalog_merge_precedence_custom_over_discovered_over_env() {
        // Probe merge-chain: env JSON standalone lab; discovered multiplexed with
        // label/description (no baseUrl/apiKey keys); sparse custom multiplexed.
        let cat = build_profile_catalog(
            &env(&[
                ("HERMES_API_URL", "http://127.0.0.1:8642"),
                ("HERMES_API_KEY", "dk"),
                ("HERMES_PROFILES", r#"[{"id":"lab","mode":"standalone","baseUrl":"http://127.0.0.1:9000","apiKey":"env-lab-key"}]"#),
            ]),
            &[HermesProfileDefinition::new("lab", HermesProfileMode::Multiplexed, HermesProfileSource::Discovered)
                .with_label("Lab Dir")
                .with_description("disc")],
            &[HermesProfileDefinition::new("lab", HermesProfileMode::Multiplexed, HermesProfileSource::Custom)],
        );
        let rt = cat.runtime("lab").unwrap();
        assert_eq!(rt.label, "Lab Dir");
        assert_eq!(rt.mode, HermesProfileMode::Multiplexed);
        // Multiplexed routing uses the primary base even though a standalone URL
        // was merged in from env (custom mode wins).
        assert_eq!(rt.base_url, "http://127.0.0.1:8642");
        assert_eq!(rt.route_prefix, "/p/lab");
        assert_eq!(rt.api_key, "env-lab-key");
        assert_eq!(rt.source, HermesProfileSource::Custom);
        assert_eq!(rt.description.as_deref(), Some("disc"));
    }

    #[test]
    fn catalog_custom_empty_key_falls_back_to_previous_and_keeps_label() {
        let cat = build_profile_catalog(
            &env(&[
                ("HERMES_API_KEY", "dk"),
                ("HERMES_PROFILES", r#"[{"id":"lab","apiKey":"env-key","label":"Env Lab"}]"#),
            ]),
            &[],
            &[HermesProfileDefinition::new("lab", HermesProfileMode::Multiplexed, HermesProfileSource::Custom)
                .with_api_key("  ")],
        );
        let rt = cat.runtime("lab").unwrap();
        assert_eq!(rt.label, "Env Lab");
        assert_eq!(rt.api_key, "env-key");
    }

    #[test]
    fn catalog_custom_full_def_wins_over_discovered_and_env() {
        let cat = build_profile_catalog(
            &env(&[
                ("HERMES_API_KEY", "dk"),
                ("HERMES_PROFILES", r#"[{"id":"lab","mode":"standalone","baseUrl":"http://127.0.0.1:9000","apiKey":"env-key","label":"Env Lab"}]"#),
            ]),
            &[HermesProfileDefinition::new("lab", HermesProfileMode::Multiplexed, HermesProfileSource::Discovered)
                .with_label("Dir Lab")],
            &[HermesProfileDefinition::new("lab", HermesProfileMode::Standalone, HermesProfileSource::Custom)
                .with_label("Custom Lab")
                .with_base_url("http://127.0.0.1:9100/")
                .with_api_key("custom-key")
                .with_description("custom desc")],
        );
        let rt = cat.runtime("lab").unwrap();
        assert_eq!(rt.label, "Custom Lab");
        assert_eq!(rt.mode, HermesProfileMode::Standalone);
        assert_eq!(rt.base_url, "http://127.0.0.1:9100");
        assert_eq!(rt.route_prefix, "");
        assert_eq!(rt.api_key, "custom-key");
        assert_eq!(rt.source, HermesProfileSource::Custom);
        assert_eq!(rt.description.as_deref(), Some("custom desc"));
    }

    #[test]
    fn catalog_discovered_label_prefers_previous_then_def_then_id() {
        // prev label wins over discovered label (TS `prev?.label || def.label || id`).
        let cat = build_profile_catalog(
            &env(&[("HERMES_PROFILES", r#"[{"id":"lab","label":"Env Lab"}]"#)]),
            &[HermesProfileDefinition::new("lab", HermesProfileMode::Multiplexed, HermesProfileSource::Discovered)
                .with_label("Dir Lab")],
            &[],
        );
        assert_eq!(cat.runtime("lab").unwrap().label, "Env Lab");

        let cat2 = build_profile_catalog(
            &env(&[]),
            &[HermesProfileDefinition::new("lab", HermesProfileMode::Multiplexed, HermesProfileSource::Discovered)
                .with_label("Dir Lab")],
            &[],
        );
        assert_eq!(cat2.runtime("lab").unwrap().label, "Dir Lab");

        let cat3 = build_profile_catalog(
            &env(&[]),
            &[HermesProfileDefinition::new("lab", HermesProfileMode::Multiplexed, HermesProfileSource::Discovered)],
            &[],
        );
        assert_eq!(cat3.runtime("lab").unwrap().label, "lab");
    }

    #[test]
    fn catalog_discovered_ids_are_normalized() {
        let cat = build_profile_catalog(
            &env(&[]),
            &[HermesProfileDefinition::new("Coder Agent!", HermesProfileMode::Multiplexed, HermesProfileSource::Discovered)],
            &[],
        );
        assert!(cat.runtime("coder-agent").is_some());
        assert_eq!(cat.runtime("coder-agent").unwrap().source, HermesProfileSource::Discovered);
    }

    // ---- Summaries -----------------------------------------------------------

    #[test]
    fn summary_status_enrichment_and_optional_field_omission() {
        let rt = resolve_profile_runtime_config(
            &HermesProfileDefinition::new("lab", HermesProfileMode::Multiplexed, HermesProfileSource::Env)
                .with_description("d"),
            &env(&[("HERMES_API_KEY", "k")]),
        );
        let plain = rt.to_redacted_summary(None);
        assert!(plain.has_api_key);
        assert_eq!(plain.description.as_deref(), Some("d"));
        assert_eq!(plain.connected, None);
        let json = serde_json::to_string(&plain).unwrap();
        assert!(!json.contains("connected"));
        assert!(!json.contains("activeModel"));

        let status = ProfileGatewayStatus {
            connected: true,
            active_model: Some("glm-5.3".into()),
            health_status: Some("ok".into()),
            error: None,
        };
        let enriched = rt.to_redacted_summary(Some(&status));
        assert_eq!(enriched.connected, Some(true));
        assert_eq!(enriched.active_model.as_deref(), Some("glm-5.3"));
        assert_eq!(enriched.health_status.as_deref(), Some("ok"));
        assert_eq!(enriched.error, None);

        let err_status = ProfileGatewayStatus {
            connected: false,
            active_model: None,
            health_status: None,
            error: Some("HTTP 500".into()),
        };
        let errored = rt.to_redacted_summary(Some(&err_status));
        assert_eq!(errored.error.as_deref(), Some("HTTP 500"));
        assert_eq!(errored.connected, Some(false));
    }

    #[test]
    fn summary_without_key_or_description_omits_them() {
        let rt = resolve_profile_runtime_config(
            &HermesProfileDefinition::new("lab", HermesProfileMode::Multiplexed, HermesProfileSource::Env),
            &env(&[]),
        );
        let s = rt.to_redacted_summary(None);
        assert!(!s.has_api_key);
        assert_eq!(s.description, None);
        let json = serde_json::to_string(&s).unwrap();
        assert!(!json.contains("description"));
        assert!(json.contains("\"hasApiKey\":false"));
    }

    #[test]
    fn summary_wire_format_is_camel_case_and_round_trips() {
        let cat = build_profile_catalog(&env(&[("HERMES_API_KEY", "k")]), &[], &[]);
        let s = &cat.summaries()[0];
        let json = serde_json::to_string(s).unwrap();
        assert!(json.contains("\"hasApiKey\":true"));
        assert!(json.contains("\"baseUrl\":"));
        assert!(json.contains("\"routePrefix\":"));
        assert!(json.contains("\"isDefault\":true"));
        assert!(json.contains("\"mode\":\"standalone\""));
        assert!(json.contains("\"source\":\"env\""));
        let back: HermesProfileSummary = serde_json::from_str(&json).unwrap();
        assert_eq!(&back, s);
    }

    #[test]
    fn runtime_debug_redacts_api_key() {
        let rt = resolve_profile_runtime_config(
            &HermesProfileDefinition::new("lab", HermesProfileMode::Multiplexed, HermesProfileSource::Env),
            &env(&[("HERMES_API_KEY", "super-secret-runtime")]),
        );
        let dbg = format!("{rt:?}");
        assert!(!dbg.contains("super-secret-runtime"));
        assert!(dbg.contains("[REDACTED]"));
        let cat_dbg = format!("{:?}", build_profile_catalog(&env(&[("HERMES_API_KEY", "super-secret-runtime")]), &[], &[]));
        assert!(!cat_dbg.contains("super-secret-runtime"));
    }

    // ---- Cross-language parity table (generated from the TS reference) ------

    #[test]
    fn normalization_parity_table_from_ts_reference() {
        let cases: &[(&str, &str, bool)] = &[
            ("", "default", false),
            (" ", "default", false),
            ("a", "a", true),
            ("A", "a", true),
            ("api", "default", false),
            ("API", "default", false),
            ("v1", "default", false),
            ("health", "default", false),
            ("p", "default", false),
            ("sessions", "default", false),
            ("runs", "default", false),
            ("default", "default", false),
            ("Default", "default", false),
            ("Coder Agent!", "coder-agent", false),
            ("research_v2", "research_v2", true),
            ("a!-!b", "a---b", false),
            ("a--b", "a--b", true),
            ("--x--", "x", false),
            ("xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", false),
            ("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa bcd", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-", false),
            ("Café Ångström", "caf-ngstr-m", false),
            ("Ünïcödé Tëst", "n-c-d-t-st", false),
            ("123", "123", true),
            ("123abc", "123abc", true),
            ("-leading", "leading", false),
            ("trailing-", "trailing", true),
            ("mid_dashes--ok", "mid_dashes--ok", true),
            ("dots.and.dashes", "dots-and-dashes", false),
            ("emoji🔥test", "emoji-test", false),
            ("tab\tchar", "tab-char", false),
            ("new\nline", "new-line", false),
            ("slash/es", "slash-es", false),
            ("back\\slash", "back-slash", false),
            ("colon:port", "colon-port", false),
            ("percent%20", "percent-20", false),
            ("plus+sign", "plus-sign", false),
            ("question?mark", "question-mark", false),
            ("hash#tag", "hash-tag", false),
            ("at@sign", "at-sign", false),
            ("dollar$sign", "dollar-sign", false),
            ("amp&ersand", "amp-ersand", false),
            ("equal=sign", "equal-sign", false),
            ("comma,sep", "comma-sep", false),
            ("semi;colon", "semi-colon", false),
            ("quote\"str", "quote-str", false),
            ("apos'trophe", "apos-trophe", false),
            ("paren(test)", "paren-test", false),
            ("brack[et]", "brack-et", false),
            ("brace{curly}", "brace-curly", false),
            ("angle<lt>", "angle-lt", false),
            ("pipe|line", "pipe-line", false),
            ("tilde~ok", "tilde-ok", false),
            ("star*ok", "star-ok", false),
            ("excl!ok", "excl-ok", false),
            ("underscore_ok", "underscore_ok", true),
            ("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", true),
            ("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", false),
            ("  spaced  out  ", "spaced-out", false),
            ("MiXeD-CaSe_99", "mixed-case_99", true),
        ];
        for (input, expected_norm, expected_slug) in cases {
            assert_eq!(
                normalize_profile_id(Some(input)),
                *expected_norm,
                "normalize({input:?})"
            );
            assert_eq!(
                is_valid_custom_profile_slug(Some(input)),
                *expected_slug,
                "slug({input:?})"
            );
        }
    }

    #[test]
    fn http_url_parity_table_from_ts_reference() {
        let cases: &[(&str, bool)] = &[
            ("http://x", true),
            ("https://x", true),
            ("HTTP://X", true),
            ("http://x:8080/p/", true),
            ("http://user:pass@x/", true),
            ("http://x?q=1#f", true),
            ("//x.com", false),
            ("http:x", true),
            ("mailto:a@b", false),
            ("http://", false),
            ("https://", false),
            ("http://x:99999", false),
            (" http://x.com/ ", true),
            ("http://[::1]:8642/", true),
            ("not a url", false),
            ("", false),
            ("ftp://x", false),
            ("ws://x", false),
            ("wss://x", false),
            ("file:///x", false),
            ("http://x.com\\path", true),
        ];
        for (input, expected) in cases {
            assert_eq!(is_valid_http_url(Some(input)), *expected, "url({input:?})");
        }
    }

    #[test]
    fn definition_debug_redacts_api_key() {
        let def = HermesProfileDefinition::new("lab", HermesProfileMode::Standalone, HermesProfileSource::Custom)
            .with_api_key("super-secret-token");
        let dbg = format!("{def:?}");
        assert!(!dbg.contains("super-secret-token"));
        assert!(dbg.contains("[REDACTED]"));
    }

    // ---- Golden vectors (tests/vectors/profileRegistry/*.json) --------------

    fn vectors_dir() -> std::path::PathBuf {
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        for rel in [
            "../../tests/vectors/profileRegistry",
            "../../../tests/vectors/profileRegistry",
        ] {
            let p = manifest.join(rel);
            if p.is_dir() {
                return p;
            }
        }
        panic!("profileRegistry golden-vector directory not found");
    }

    fn load_vector_cases(file: &str) -> Vec<serde_json::Value> {
        let path = vectors_dir().join(file);
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let doc: serde_json::Value = serde_json::from_str(&text).expect("valid vector JSON");
        doc["cases"].as_array().expect("cases array").clone()
    }

    fn env_from_json(v: &serde_json::Value) -> HashMap<String, String> {
        v.as_object()
            .map(|o| {
                o.iter()
                    .filter_map(|(k, val)| val.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn mode_from_json(v: Option<&serde_json::Value>) -> HermesProfileMode {
        if v.and_then(|x| x.as_str()) == Some("standalone") {
            HermesProfileMode::Standalone
        } else {
            HermesProfileMode::Multiplexed
        }
    }

    fn source_from_json(v: Option<&serde_json::Value>) -> HermesProfileSource {
        match v.and_then(|x| x.as_str()) {
            Some("custom") => HermesProfileSource::Custom,
            Some("discovered") => HermesProfileSource::Discovered,
            _ => HermesProfileSource::Env,
        }
    }

    /// Rebuild a definition from a vector record, preserving JS key presence:
    /// absent keys stay absent; present keys (even null) set the presence bit.
    fn def_from_json(rec: &serde_json::Value) -> HermesProfileDefinition {
        let id = rec.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let mut def = HermesProfileDefinition::new(
            id,
            mode_from_json(rec.get("mode")),
            source_from_json(rec.get("source")),
        );
        if let Some(v) = rec.get("label") {
            def = match v.as_str() {
                Some(s) => def.with_label(s),
                None => def.with_field_presence(F_LABEL),
            };
        }
        if let Some(v) = rec.get("baseUrl") {
            def = match v.as_str() {
                Some(s) => def.with_base_url(s),
                None => def.with_field_presence(F_BASE_URL),
            };
        }
        if let Some(v) = rec.get("apiKey") {
            def = match v.as_str() {
                Some(s) => def.with_api_key(s),
                None => def.with_field_presence(F_API_KEY),
            };
        }
        if let Some(v) = rec.get("description") {
            def = match v.as_str() {
                Some(s) => def.with_description(s),
                None => def.with_field_presence(F_DESCRIPTION),
            };
        }
        def
    }

    fn runtime_from_json(rec: &serde_json::Value) -> HermesProfileRuntime {
        HermesProfileRuntime {
            id: rec["id"].as_str().unwrap_or("").to_string(),
            label: rec["label"].as_str().unwrap_or("").to_string(),
            mode: mode_from_json(rec.get("mode")),
            base_url: rec["baseUrl"].as_str().unwrap_or("").to_string(),
            route_prefix: rec["routePrefix"].as_str().unwrap_or("").to_string(),
            api_key: rec["apiKey"].as_str().unwrap_or("").to_string(),
            source: source_from_json(rec.get("source")),
            is_default: rec["isDefault"].as_bool().unwrap_or(false),
            description: rec
                .get("description")
                .and_then(|v| v.as_str())
                .map(str::to_string),
        }
    }

    fn assert_runtime_matches(rt: &HermesProfileRuntime, exp: &serde_json::Value, ctx: &str) {
        assert_eq!(rt.id, exp["id"].as_str().unwrap_or(""), "{ctx}: id");
        assert_eq!(rt.label, exp["label"].as_str().unwrap_or(""), "{ctx}: label");
        assert_eq!(rt.mode, mode_from_json(exp.get("mode")), "{ctx}: mode");
        assert_eq!(rt.base_url, exp["baseUrl"].as_str().unwrap_or(""), "{ctx}: baseUrl");
        assert_eq!(rt.route_prefix, exp["routePrefix"].as_str().unwrap_or(""), "{ctx}: routePrefix");
        assert_eq!(rt.api_key, exp["apiKey"].as_str().unwrap_or(""), "{ctx}: apiKey");
        assert_eq!(rt.source, source_from_json(exp.get("source")), "{ctx}: source");
        assert_eq!(rt.is_default, exp["isDefault"].as_bool().unwrap_or(false), "{ctx}: isDefault");
        assert_eq!(
            rt.description.as_deref(),
            exp.get("description").and_then(|v| v.as_str()),
            "{ctx}: description"
        );
    }

    #[test]
    fn golden_vectors_resolve_profile_runtime_config() {
        for case in load_vector_cases("resolveProfileRuntimeConfig.json") {
            let name = case["name"].as_str().unwrap_or("?");
            let def = def_from_json(&case["input"]["def"]);
            let e = env_from_json(&case["input"]["env"]);
            let rt = resolve_profile_runtime_config(&def, &e);
            assert_runtime_matches(&rt, &case["expected"], name);
        }
    }

    #[test]
    fn golden_vectors_build_profile_catalog() {
        for case in load_vector_cases("buildProfileCatalog.json") {
            let name = case["name"].as_str().unwrap_or("?");
            let input = &case["input"];
            let e = env_from_json(&input["env"]);
            let discovered: Vec<HermesProfileDefinition> = input
                .get("discoveredProfiles")
                .and_then(|v| v.as_array())
                .map(|a| a.iter().map(def_from_json).collect())
                .unwrap_or_default();
            let custom: Vec<HermesProfileDefinition> = input
                .get("customProfiles")
                .and_then(|v| v.as_array())
                .map(|a| a.iter().map(def_from_json).collect())
                .unwrap_or_default();
            let cat = build_profile_catalog(&e, &discovered, &custom);
            let exp = &case["expected"];
            assert_eq!(
                cat.default_profile_id,
                exp["defaultProfileId"].as_str().unwrap_or(""),
                "{name}: defaultProfileId"
            );
            let exp_rts = exp["runtimes"].as_array().expect("runtimes array");
            assert_eq!(cat.runtimes.len(), exp_rts.len(), "{name}: runtime count");
            for (rt, exp_rt) in cat.runtimes.iter().zip(exp_rts) {
                assert_runtime_matches(rt, exp_rt, &format!("{name}/{}", rt.id));
            }
        }
    }

    #[test]
    fn golden_vectors_to_redacted_profile_summary() {
        for case in load_vector_cases("toRedactedProfileSummary.json") {
            let name = case["name"].as_str().unwrap_or("?");
            let rt = runtime_from_json(&case["input"]["runtime"]);
            let status = case["input"].get("status").map(|s| ProfileGatewayStatus {
                connected: s["connected"].as_bool().unwrap_or(false),
                active_model: s.get("activeModel").and_then(|v| v.as_str()).map(str::to_string),
                health_status: s.get("healthStatus").and_then(|v| v.as_str()).map(str::to_string),
                error: s.get("error").and_then(|v| v.as_str()).map(str::to_string),
            });
            let summary = to_redacted_profile_summary(&rt, status.as_ref());
            let wire = serde_json::to_value(&summary).unwrap();
            assert_eq!(wire, case["expected"], "{name}: camelCase wire format");
            if !rt.api_key.is_empty() {
                assert!(!wire.to_string().contains(&rt.api_key), "{name}: raw key leaked");
            }
        }
    }
}
