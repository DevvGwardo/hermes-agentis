//! Durable preferences vault — port of `src/utils/settingsVaultClient.ts` and the
//! server-side `createPreferencesVaultStore` from `src/utils/appUpdater.ts`
//! (spec §5.1, §7.4, ticket P1-03 store side).
//!
//! Replaces the web `/api/preferences/*` HTTP roundtrips with native storage:
//! * atomic JSON vault (`preferences-vault.json`, tmp-file + rename),
//! * SQLite sync (`voxcell.db`, `kv_json` table shared with [`crate::VoxcellStore`]),
//! * rolling 12-backup history (`manual-*`, `pre-update-*`) with prune & restore,
//! * `UpdateReceipt` persistence (`update-receipt.json` + SQLite mirror).
//!
//! Snapshots are stored in the **web camelCase JSON contract** so vaults written
//! by the TypeScript server round-trip unchanged. Full snapshot normalization /
//! legacy-emerald migration lives in `voxcell-core::snapshot` (slate-snapshot
//! workstream). Until that module's public API lands, this crate ships a
//! storage-level [`WebJsonAdapter`]; wire the core normalizer in via
//! [`SnapshotAdapter`] / [`PreferencesVault::with_adapter`] at integration —
//! no vault internals need to change.

use rand::Rng;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

pub const MAX_BACKUPS: usize = 12;
pub const VAULT_FILE_NAME: &str = "preferences-vault.json";
pub const BACKUPS_DIR_NAME: &str = "backups";
pub const RECEIPT_FILE_NAME: &str = "update-receipt.json";
pub const DB_FILE_NAME: &str = "voxcell.db";

const KV_VAULT: &str = "preferences_vault";
const KV_META: &str = "preferences_meta";
const KV_RECEIPT: &str = "update_receipt";

/// Valid `reason` values in the web snapshot contract (`SnapshotReason` in TS).
pub const SNAPSHOT_REASONS: [&str; 5] = ["sync", "manual", "pre-update", "restore", "import"];

pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Atomic JSON write: serialize to a unique `.tmp` sibling, then rename over
/// the target (parity with `writeJsonAtomic` in appUpdater.ts).
pub fn write_json_atomic(path: &Path, payload: &Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create dir: {e}"))?;
    }
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "vault.json".into());
    let tmp = path.with_file_name(format!(
        "{file_name}.{}.{}.tmp",
        std::process::id(),
        now_ms()
    ));
    let bytes = serde_json::to_vec_pretty(payload).map_err(|e| format!("serialize json: {e}"))?;
    std::fs::write(&tmp, bytes).map_err(|e| format!("write tmp: {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("atomic rename: {e}")
    })
}

fn read_json_file(path: &Path) -> Option<Value> {
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

/// Why a backup was taken (TS restricts `createBackup` to these two).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackupReason {
    Manual,
    PreUpdate,
}

impl BackupReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::PreUpdate => "pre-update",
        }
    }
}

/// Metadata row for one restore point (`PreferenceBackupSummary` in TS).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreferenceBackupSummary {
    pub id: String,
    pub created_at: i64,
    pub reason: String,
    pub app_version: String,
    pub accent_hex: String,
    pub accent_preset: String,
    pub background_id: String,
    pub custom_slot_count: usize,
    pub workspace_title: String,
}

/// One preserved-settings group shown in the update flow
/// (`PreservedCategorySummary` in settingsSnapshot.ts).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreservedCategorySummary {
    pub id: String,
    pub label: String,
    pub detail: String,
    pub customized: bool,
}

/// Post-update receipt displayed on next launch (`UpdateReceipt` in TS).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateReceipt {
    pub updated_at: i64,
    pub previous_version: String,
    pub new_version: String,
    #[serde(default)]
    pub previous_commit: Option<String>,
    #[serde(default)]
    pub new_commit: Option<String>,
    pub backup_id: String,
    /// `"git-pull"` or `"managed-refresh"` (TS union).
    pub mode: String,
    pub preserved_categories: Vec<String>,
}

/// Snapshot contract seam. The vault never inspects snapshot internals beyond
/// this trait, so `voxcell-core::snapshot` can be plugged in without touching
/// storage code (see module docs).
pub trait SnapshotAdapter: Send + Sync {
    /// Normalize a raw JSON snapshot; `None` rejects the payload.
    fn normalize(&self, raw: &Value, fallback_version: &str) -> Option<Value>;
    /// Build the restore-point summary row for a stored snapshot.
    fn backup_summary(&self, id: &str, snapshot: &Value) -> PreferenceBackupSummary;
    /// Preserved-category diff used by the updater receipt.
    fn preserved_categories(&self, snapshot: &Value) -> Vec<PreservedCategorySummary>;
    /// Stamp a snapshot being written as a backup (`snapshotId`, `capturedAt`, `reason`).
    fn stamp_backup(&self, snapshot: &Value, id: &str, stamped_ms: i64, reason: &str) -> Value;
    /// Stamp a snapshot being restored (fresh `capturedAt`, `reason: restore`).
    fn stamp_restore(&self, snapshot: &Value, backup_id: &str, now_ms: i64) -> Value;
}

fn obj_get<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    v.as_object().and_then(|o| o.get(key))
}

fn get_str<'a>(v: &'a Value, key: &str, default: &'a str) -> String {
    obj_get(v, key)
        .and_then(|x| x.as_str())
        .unwrap_or(default)
        .to_string()
}

fn get_f64(v: &Value, key: &str, default: f64) -> f64 {
    obj_get(v, key).and_then(|x| x.as_f64()).unwrap_or(default)
}

fn get_bool(v: &Value, key: &str, default: bool) -> bool {
    obj_get(v, key).and_then(|x| x.as_bool()).unwrap_or(default)
}

fn trimmed_slice(v: &Value, key: &str, max_chars: usize) -> Option<String> {
    let s = obj_get(v, key)?.as_str()?.trim();
    if s.is_empty() {
        return None;
    }
    Some(s.chars().take(max_chars).collect())
}

/// Web-default UI config (slate-v1) used for preserved-category diffs.
/// Mirrors `DEFAULT_UI_CONFIG` in src/utils/uiCustomization.ts.
fn default_ui_field(key: &str) -> Value {
    match key {
        "accentPreset" => json!("slate"),
        "accentHex" => json!("#94A3B8"),
        "surfaceHex" => json!("#11141B"),
        "panelAlpha" => json!(0.80),
        "borderOpacity" => json!(0.22),
        "glowIntensity" => json!(0.30),
        "sidebarVisible" => json!(true),
        "sidebarPosition" => json!("left"),
        "sidebarWidthPx" => json!(296),
        "cornerRadius" => json!("rounded"),
        "density" => json!("comfortable"),
        "fontPreset" => json!("jakarta"),
        "fontScale" => json!(1.0),
        "showTopNav" => json!(true),
        "composerStyle" => json!("docked"),
        "workspaceTitle" => json!("Hermes"),
        "portraitVisible" => json!(true),
        "portraitScale" => json!(1.0),
        "portraitTintHex" => Value::Null,
        "brailleDotProfile" => json!("balanced"),
        "horizonVisible" => json!(true),
        "horizonStyle" => json!("silk"),
        "reducedMotion" => json!(false),
        _ => Value::Null,
    }
}

fn ui_field(ui: &Value, key: &str) -> Value {
    obj_get(ui, key).cloned().unwrap_or_else(|| default_ui_field(key))
}

fn ui_changed(ui: &Value, key: &str) -> bool {
    ui_field(ui, key) != default_ui_field(key)
}

/// Storage-level adapter for the web camelCase snapshot contract.
///
/// Implements the storage-relevant parts of `normalizePreferencesSnapshot`
/// (reason whitelist, capturedAt/appVersion defaults, id/origin trimming).
/// Deep UI sanitization and legacy-emerald auto-migration are delegated to
/// `voxcell-core::snapshot::normalize_snapshot` once wired via
/// [`PreferencesVault::with_adapter`].
pub struct WebJsonAdapter;

impl SnapshotAdapter for WebJsonAdapter {
    fn normalize(&self, raw: &Value, fallback_version: &str) -> Option<Value> {
        let obj = raw.as_object()?;
        let mut out = serde_json::Map::new();
        out.insert("schemaVersion".into(), json!(1));

        if obj.get("themeEpoch").and_then(|v| v.as_str()) == Some("slate-v1") {
            out.insert("themeEpoch".into(), json!("slate-v1"));
        }
        if let Some(id) = trimmed_slice(raw, "snapshotId", 96) {
            out.insert("snapshotId".into(), json!(id));
        }
        let captured_at = obj
            .get("capturedAt")
            .and_then(|v| v.as_f64())
            .filter(|n| n.is_finite())
            .map(|n| n as i64)
            .unwrap_or_else(now_ms);
        out.insert("capturedAt".into(), json!(captured_at));

        let app_version = obj
            .get("appVersion")
            .and_then(|v| v.as_str())
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.chars().take(48).collect::<String>())
            .unwrap_or_else(|| fallback_version.to_string());
        out.insert("appVersion".into(), json!(app_version));

        if let Some(origin) = trimmed_slice(raw, "sourceOrigin", 200) {
            out.insert("sourceOrigin".into(), json!(origin));
        }
        let reason = obj
            .get("reason")
            .and_then(|v| v.as_str())
            .filter(|r| SNAPSHOT_REASONS.contains(r))
            .unwrap_or("sync");
        out.insert("reason".into(), json!(reason));

        // Payload sections: keep well-formed objects/arrays as-is; fill missing
        // sections with web defaults. Deep sanitization belongs to core::snapshot.
        let ui = obj.get("uiConfig").filter(|v| v.is_object()).cloned();
        out.insert("uiConfig".into(), ui.unwrap_or_else(|| json!({})));
        let background = obj.get("background").filter(|v| v.is_object()).cloned();
        out.insert(
            "background".into(),
            background.unwrap_or_else(|| {
                json!({"activeId": "aurora-drift", "opacity": 0.64, "blurPx": 12, "customSlots": []})
            }),
        );
        let workspace = obj.get("workspace").filter(|v| v.is_object()).cloned();
        out.insert(
            "workspace".into(),
            workspace.unwrap_or_else(|| {
                json!({
                    "showThinking": true, "voiceMuted": false, "mood": "happy",
                    "draftProfile": "default", "onboardingCompleted": false
                })
            }),
        );
        let canvas = obj.get("canvas").filter(|v| v.is_object()).cloned();
        out.insert(
            "canvas".into(),
            canvas.unwrap_or_else(|| json!({"sessions": [], "pins": [], "dismissedIds": []})),
        );
        Some(Value::Object(out))
    }

    fn backup_summary(&self, id: &str, snapshot: &Value) -> PreferenceBackupSummary {
        let ui = obj_get(snapshot, "uiConfig").cloned().unwrap_or_else(|| json!({}));
        let bg = obj_get(snapshot, "background").cloned().unwrap_or_else(|| json!({}));
        PreferenceBackupSummary {
            id: id.to_string(),
            created_at: obj_get(snapshot, "capturedAt").and_then(|v| v.as_i64()).unwrap_or(0),
            reason: get_str(snapshot, "reason", "manual"),
            app_version: get_str(snapshot, "appVersion", ""),
            accent_hex: get_str(&ui, "accentHex", ""),
            accent_preset: get_str(&ui, "accentPreset", ""),
            background_id: get_str(&bg, "activeId", ""),
            custom_slot_count: obj_get(&bg, "customSlots")
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0),
            workspace_title: get_str(&ui, "workspaceTitle", ""),
        }
    }

    fn preserved_categories(&self, snapshot: &Value) -> Vec<PreservedCategorySummary> {
        let ui = obj_get(snapshot, "uiConfig").cloned().unwrap_or_else(|| json!({}));
        let bg = obj_get(snapshot, "background").cloned().unwrap_or_else(|| json!({}));
        let ws = obj_get(snapshot, "workspace").cloned().unwrap_or_else(|| json!({}));
        let canvas = obj_get(snapshot, "canvas").cloned().unwrap_or_else(|| json!({}));

        let theme_customized = ["accentPreset", "accentHex", "surfaceHex", "panelAlpha", "borderOpacity", "glowIntensity"]
            .iter()
            .any(|k| ui_changed(&ui, k));

        let bg_active = get_str(&bg, "activeId", "aurora-drift");
        let bg_opacity = get_f64(&bg, "opacity", 0.64);
        let bg_blur = get_f64(&bg, "blurPx", 12.0);
        let slots = obj_get(&bg, "customSlots").and_then(|v| v.as_array()).cloned().unwrap_or_default();
        let bg_customized = bg_active != "aurora-drift" || bg_opacity != 0.64 || bg_blur != 12.0 || !slots.is_empty();

        let layout_customized = [
            "sidebarVisible", "sidebarPosition", "sidebarWidthPx", "cornerRadius", "density",
            "fontPreset", "fontScale", "showTopNav", "composerStyle", "workspaceTitle",
        ]
        .iter()
        .any(|k| ui_changed(&ui, k));

        let stage_customized = [
            "portraitVisible", "portraitScale", "portraitTintHex", "brailleDotProfile",
            "horizonVisible", "horizonStyle", "reducedMotion",
        ]
        .iter()
        .any(|k| ui_changed(&ui, k))
            || get_bool(&ws, "showThinking", true) != true
            || get_bool(&ws, "voiceMuted", false) != false
            || get_str(&ws, "mood", "happy") != "happy";

        let sessions = obj_get(&canvas, "sessions").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0);
        let pins = obj_get(&canvas, "pins").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0);
        let onboarding = get_bool(&ws, "onboardingCompleted", false);

        let panel_pct = (get_f64(&ui, "panelAlpha", 0.80) * 100.0).round() as i64;
        let bg_detail = if !slots.is_empty() {
            format!("{bg_active} · {} custom", slots.len())
        } else {
            format!("{bg_active} · {}%", (bg_opacity * 100.0).round() as i64)
        };
        vec![
            PreservedCategorySummary {
                id: "theme".into(),
                label: "Theme & Colors".into(),
                detail: format!(
                    "{} · {} · {panel_pct}% glass",
                    get_str(&ui, "accentPreset", "slate"),
                    get_str(&ui, "accentHex", "#94A3B8")
                ),
                customized: theme_customized,
            },
            PreservedCategorySummary {
                id: "backdrop".into(),
                label: "Backdrop & Media".into(),
                detail: bg_detail,
                customized: bg_customized,
            },
            PreservedCategorySummary {
                id: "layout".into(),
                label: "Layout & Typography".into(),
                detail: format!(
                    "{} rail · {} · {}",
                    get_str(&ui, "sidebarPosition", "left"),
                    get_str(&ui, "density", "comfortable"),
                    get_str(&ui, "fontPreset", "jakarta")
                ),
                customized: layout_customized,
            },
            PreservedCategorySummary {
                id: "stage".into(),
                label: "Stage, Voice & Trace".into(),
                detail: format!(
                    "{} braille · {} · {}",
                    get_str(&ui, "brailleDotProfile", "balanced"),
                    if get_bool(&ws, "showThinking", true) { "trace on" } else { "clean" },
                    if get_bool(&ws, "voiceMuted", false) { "muted" } else { "audio on" }
                ),
                customized: stage_customized,
            },
            PreservedCategorySummary {
                id: "canvas".into(),
                label: "Canvas & Tour State".into(),
                detail: format!(
                    "{sessions} dots · {pins} pins · {}",
                    if onboarding { "tour done" } else { "tour pending" }
                ),
                customized: sessions > 0 || pins > 0 || onboarding,
            },
        ]
    }

    fn stamp_backup(&self, snapshot: &Value, id: &str, stamped_ms: i64, reason: &str) -> Value {
        let mut out = snapshot.clone();
        if let Some(obj) = out.as_object_mut() {
            obj.insert("snapshotId".into(), json!(id));
            let existing = obj.get("capturedAt").and_then(|v| v.as_i64()).unwrap_or(0);
            obj.insert(
                "capturedAt".into(),
                json!(if existing > 0 { existing } else { stamped_ms }),
            );
            obj.insert("reason".into(), json!(reason));
        }
        out
    }

    fn stamp_restore(&self, snapshot: &Value, backup_id: &str, now: i64) -> Value {
        let mut out = snapshot.clone();
        if let Some(obj) = out.as_object_mut() {
            obj.insert("capturedAt".into(), json!(now));
            obj.insert("reason".into(), json!("restore"));
            obj.insert("snapshotId".into(), json!(backup_id));
        }
        out
    }
}

/// Outcome of creating a restore point (TS `BackupCreateResponse` payload).
#[derive(Clone, Debug)]
pub struct BackupCreateOutcome {
    pub backup: PreferenceBackupSummary,
    pub backups: Vec<PreferenceBackupSummary>,
}

/// Outcome of restoring a restore point (TS `RestoreResponse` payload).
#[derive(Clone, Debug)]
pub struct RestoreOutcome {
    pub snapshot: Value,
    pub backups: Vec<PreferenceBackupSummary>,
}

/// Native preferences vault: atomic JSON + SQLite sync + rolling backups.
pub struct PreferencesVault {
    data_dir: PathBuf,
    vault_path: PathBuf,
    backups_dir: PathBuf,
    receipt_path: PathBuf,
    conn: Connection,
    adapter: Box<dyn SnapshotAdapter>,
    fallback_version: String,
}

impl PreferencesVault {
    /// Open (creating if needed) a vault under `data_dir` with the default
    /// web-JSON adapter and its own `voxcell.db` connection.
    pub fn open(data_dir: impl AsRef<Path>) -> Result<Self, String> {
        Self::with_adapter(data_dir, Box::new(WebJsonAdapter), "0.1.0")
    }

    /// Open with an explicit snapshot adapter (e.g. a `voxcell-core::snapshot`
    /// bridge) and fallback app version.
    pub fn with_adapter(
        data_dir: impl AsRef<Path>,
        adapter: Box<dyn SnapshotAdapter>,
        fallback_version: &str,
    ) -> Result<Self, String> {
        let data_dir = data_dir.as_ref().to_path_buf();
        let db_path = data_dir.join(DB_FILE_NAME);
        Self::open_with_db(data_dir, &db_path, adapter, fallback_version)
    }

    /// Open sharing an existing SQLite database file (e.g. the one used by
    /// [`crate::VoxcellStore`]) so both see the same `kv_json` rows.
    pub fn open_with_db(
        data_dir: impl AsRef<Path>,
        db_path: &Path,
        adapter: Box<dyn SnapshotAdapter>,
        fallback_version: &str,
    ) -> Result<Self, String> {
        let data_dir = data_dir.as_ref().to_path_buf();
        let backups_dir = data_dir.join(BACKUPS_DIR_NAME);
        std::fs::create_dir_all(&backups_dir).map_err(|e| format!("create vault dirs: {e}"))?;
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("create db dir: {e}"))?;
        }
        let conn = Connection::open(db_path).map_err(|e| format!("open vault sqlite: {e}"))?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS kv_json (
                 k TEXT PRIMARY KEY,
                 v TEXT NOT NULL,
                 updated_at INTEGER NOT NULL DEFAULT (strftime('%s','now'))
             );",
        )
        .map_err(|e| format!("init vault schema: {e}"))?;
        Ok(Self {
            vault_path: data_dir.join(VAULT_FILE_NAME),
            backups_dir,
            receipt_path: data_dir.join(RECEIPT_FILE_NAME),
            data_dir,
            conn,
            adapter,
            fallback_version: fallback_version.to_string(),
        })
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }
    pub fn vault_path(&self) -> &Path {
        &self.vault_path
    }
    pub fn backups_dir(&self) -> &Path {
        &self.backups_dir
    }
    pub fn receipt_path(&self) -> &Path {
        &self.receipt_path
    }

    /// TS parity label: home-dir vaults get the friendly `~/.voxcell` label.
    pub fn storage_label(&self) -> String {
        storage_label_for(&self.vault_path)
    }

    fn kv_get(&self, key: &str) -> Option<String> {
        self.conn
            .query_row("SELECT v FROM kv_json WHERE k = ?1", params![key], |r| r.get(0))
            .optional()
            .ok()
            .flatten()
    }

    fn kv_set(&self, key: &str, value: &str) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT INTO kv_json (k, v, updated_at) VALUES (?1, ?2, strftime('%s','now'))
                 ON CONFLICT(k) DO UPDATE SET v = excluded.v, updated_at = excluded.updated_at",
                params![key, value],
            )
            .map_err(|e| format!("sqlite sync: {e}"))?;
        Ok(())
    }

    fn kv_delete(&self, key: &str) {
        let _ = self.conn.execute("DELETE FROM kv_json WHERE k = ?1", params![key]);
    }

    /// Normalize a raw snapshot through the configured [`SnapshotAdapter`].
    /// Integration point for `voxcell-core::snapshot::normalize_snapshot`.
    pub fn normalize_snapshot(&self, raw: &Value) -> Option<Value> {
        self.adapter.normalize(raw, &self.fallback_version)
    }

    /// Preserved-category diff for a snapshot (delegates to the adapter).
    pub fn preserved_categories(&self, snapshot: &Value) -> Vec<PreservedCategorySummary> {
        self.adapter.preserved_categories(snapshot)
    }

    /// Read the live vault snapshot. JSON file first; on missing/corrupt file
    /// the SQLite mirror is used and the JSON file is healed.
    pub fn read_vault(&self) -> Option<Value> {
        if let Some(raw) = read_json_file(&self.vault_path) {
            if let Some(normalized) = self.normalize_snapshot(&raw) {
                return Some(normalized);
            }
        }
        let mirrored = self.kv_get(KV_VAULT)?;
        let raw: Value = serde_json::from_str(&mirrored).ok()?;
        let normalized = self.normalize_snapshot(&raw)?;
        let _ = write_json_atomic(&self.vault_path, &normalized);
        Some(normalized)
    }

    /// Normalize + atomically persist a snapshot to JSON **and** SQLite.
    /// `reason` overrides the snapshot's own reason (TS `writeVault` parity).
    pub fn write_vault(&self, raw_snapshot: &Value, reason: Option<&str>) -> Result<Value, String> {
        let mut merged = raw_snapshot.clone();
        if let Some(obj) = merged.as_object_mut() {
            let effective = reason
                .map(|s| s.to_string())
                .or_else(|| obj.get("reason").and_then(|v| v.as_str()).map(|s| s.to_string()))
                .unwrap_or_else(|| "sync".into());
            obj.insert("reason".into(), json!(effective));
        }
        let normalized = self.normalize_snapshot(&merged).unwrap_or(merged);
        write_json_atomic(&self.vault_path, &normalized)?;
        let serialized =
            serde_json::to_string(&normalized).map_err(|e| format!("serialize vault: {e}"))?;
        self.kv_set(KV_VAULT, &serialized)?;
        let captured_at = obj_get(&normalized, "capturedAt").and_then(|v| v.as_i64()).unwrap_or(0);
        let _ = self.kv_set(KV_META, &json!({ "capturedAt": captured_at }).to_string());
        Ok(normalized)
    }

    /// `preferencesMeta.v1` parity: when the live vault was captured.
    pub fn preferences_captured_at(&self) -> i64 {
        self.kv_get(KV_META)
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .and_then(|v| obj_get(&v, "capturedAt").and_then(|c| c.as_i64()))
            .unwrap_or(0)
    }

    /// Create a `manual-*` / `pre-update-*` restore point. Also updates the
    /// live vault with the stamped snapshot (TS `createBackup` parity), then
    /// prunes history to the newest [`MAX_BACKUPS`].
    pub fn create_backup(
        &self,
        raw_snapshot: &Value,
        reason: BackupReason,
    ) -> Result<BackupCreateOutcome, String> {
        let stamped = now_ms();
        let id = format!("{}-{stamped}-{}", reason.as_str(), random_suffix());
        let prepared = self.adapter.stamp_backup(raw_snapshot, &id, stamped, reason.as_str());
        let stored = self.write_vault(&prepared, Some(reason.as_str()))?;
        write_json_atomic(&self.backups_dir.join(format!("{id}.json")), &stored)?;
        self.prune_backups();
        let backup = self.adapter.backup_summary(&id, &stored);
        Ok(BackupCreateOutcome { backup, backups: self.list_backups() })
    }

    /// All restore points, newest first (TS `listBackups` parity).
    pub fn list_backups(&self) -> Vec<PreferenceBackupSummary> {
        let Ok(entries) = std::fs::read_dir(&self.backups_dir) else {
            return Vec::new();
        };
        let mut items = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.ends_with(".json") || name.ends_with(".tmp") {
                continue;
            }
            let id = name[..name.len() - 5].to_string();
            if let Some(raw) = read_json_file(&entry.path()) {
                if let Some(snap) = self.normalize_snapshot(&raw) {
                    items.push(self.adapter.backup_summary(&id, &snap));
                }
            }
        }
        // Newest first; deterministic tie-break on id.
        items.sort_by(|a, b| b.created_at.cmp(&a.created_at).then_with(|| b.id.cmp(&a.id)));
        items
    }

    fn prune_backups(&self) {
        let all = self.list_backups();
        if all.len() <= MAX_BACKUPS {
            return;
        }
        for stale in &all[MAX_BACKUPS..] {
            let _ = std::fs::remove_file(self.backups_dir.join(format!("{}.json", stale.id)));
        }
    }

    /// Restore a backup into the live vault with `reason: restore` and a fresh
    /// `capturedAt`. Rejects path-traversal ids (TS `restoreBackup` parity).
    pub fn restore_backup(&self, backup_id: &str) -> Option<RestoreOutcome> {
        let base = backup_id
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or("")
            .trim();
        let safe_id = if base.to_ascii_lowercase().ends_with(".json") {
            &base[..base.len() - 5]
        } else {
            base
        };
        if safe_id.is_empty() || safe_id.contains("..") || safe_id.contains('/') || safe_id.contains('\\') {
            return None;
        }
        let raw = read_json_file(&self.backups_dir.join(format!("{safe_id}.json")))?;
        let loaded = self.normalize_snapshot(&raw)?;
        let stamped = self.adapter.stamp_restore(&loaded, safe_id, now_ms());
        let restored = self.write_vault(&stamped, Some("restore")).ok()?;
        Some(RestoreOutcome { snapshot: restored, backups: self.list_backups() })
    }

    /// Read the pending post-update receipt (file first, SQLite mirror fallback).
    pub fn read_receipt(&self) -> Option<UpdateReceipt> {
        if let Some(raw) = read_json_file(&self.receipt_path) {
            if let Some(receipt) = parse_receipt(&raw, &self.fallback_version) {
                return Some(receipt);
            }
        }
        let mirrored = self.kv_get(KV_RECEIPT)?;
        let raw: Value = serde_json::from_str(&mirrored).ok()?;
        let receipt = parse_receipt(&raw, &self.fallback_version)?;
        let _ = write_json_atomic(
            &self.receipt_path,
            &serde_json::to_value(&receipt).ok()?,
        );
        Some(receipt)
    }

    /// Persist a receipt atomically to JSON + SQLite.
    pub fn write_receipt(&self, receipt: &UpdateReceipt) -> Result<(), String> {
        let value = serde_json::to_value(receipt).map_err(|e| format!("serialize receipt: {e}"))?;
        write_json_atomic(&self.receipt_path, &value)?;
        self.kv_set(KV_RECEIPT, &value.to_string())
    }

    /// Acknowledge / clear the pending receipt (TS `acknowledgeUpdateReceipt`).
    pub fn clear_receipt(&self) {
        let _ = std::fs::remove_file(&self.receipt_path);
        self.kv_delete(KV_RECEIPT);
    }
}

/// TS `storageLabel` parity (server.ts `/api/preferences`).
pub fn storage_label_for(vault_path: &Path) -> String {
    if let Some(home) = dirs::home_dir() {
        if vault_path.starts_with(&home) {
            return "~/.voxcell/preferences-vault.json".to_string();
        }
    }
    "Server preferences vault".to_string()
}

fn random_suffix() -> String {
    let mut rng = rand::rng();
    (0..4)
        .map(|_| {
            let idx: u32 = rng.random_range(0..36);
            char::from_digit(idx, 36).unwrap_or('0')
        })
        .collect()
}

/// Lenient TS-parity receipt parsing (`readReceipt` in appUpdater.ts):
/// `updatedAt` must be a finite number; strings are coerced; unknown modes
/// collapse to `managed-refresh`.
fn parse_receipt(raw: &Value, fallback_version: &str) -> Option<UpdateReceipt> {
    let obj = raw.as_object()?;
    let updated_at = obj.get("updatedAt")?.as_f64().filter(|n| n.is_finite())? as i64;
    let ts_string = |key: &str, default: &str| -> String {
        match obj.get(key) {
            Some(Value::String(s)) if !s.is_empty() => s.clone(),
            Some(Value::Number(n)) => n.to_string(),
            Some(Value::Bool(b)) => b.to_string(),
            _ => default.to_string(),
        }
    };
    let commit = |key: &str| -> Option<String> {
        match obj.get(key) {
            Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
            _ => None,
        }
    };
    let preserved_categories = obj
        .get("preservedCategories")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .map(|v| match v {
                    Value::String(s) => s.clone(),
                    Value::Number(n) => n.to_string(),
                    Value::Bool(b) => b.to_string(),
                    Value::Null => "null".to_string(),
                    other => other.to_string(),
                })
                .collect()
        })
        .unwrap_or_default();
    Some(UpdateReceipt {
        updated_at,
        previous_version: ts_string("previousVersion", fallback_version),
        new_version: ts_string("newVersion", fallback_version),
        previous_commit: commit("previousCommit"),
        new_commit: commit("newCommit"),
        backup_id: ts_string("backupId", ""),
        mode: if obj.get("mode").and_then(|v| v.as_str()) == Some("git-pull") {
            "git-pull".into()
        } else {
            "managed-refresh".into()
        },
        preserved_categories,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "voxcell-vault-test-{}-{tag}-{}",
            std::process::id(),
            now_ms()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Mirrors `sampleSnapshot()` in appUpdater.test.ts (web camelCase contract).
    fn sample_snapshot(preset: &str) -> Value {
        let (accent_hex, preset_id) = match preset {
            "violet" => ("#A855F7", "cyber-violet"),
            _ => ("#F59E0B", "solaris-amber"),
        };
        json!({
            "schemaVersion": 1,
            "capturedAt": now_ms(),
            "appVersion": "0.1.0",
            "reason": "sync",
            "uiConfig": {
                "accentPreset": preset,
                "accentHex": accent_hex,
                "presetId": preset_id,
                "workspaceTitle": "Custom Studio Rig"
            },
            "background": {
                "activeId": "custom-slot-1",
                "opacity": 0.64,
                "blurPx": 12,
                "customSlots": [{
                    "id": "custom-slot-1",
                    "label": "Studio Wallpaper",
                    "mediaType": "image",
                    "src": "https://example.com/wallpaper.png",
                    "createdAt": 1000
                }]
            },
            "workspace": {
                "showThinking": false,
                "voiceMuted": false,
                "mood": "happy",
                "draftProfile": "default",
                "onboardingCompleted": true
            },
            "canvas": { "sessions": [], "pins": [], "dismissedIds": [] }
        })
    }

    #[test]
    fn vault_persists_themes_wallpapers_backups_and_restore_atomically() {
        let dir = temp_dir("persist");
        let vault = PreferencesVault::open(&dir).unwrap();
        assert!(vault.read_vault().is_none());

        let initial = sample_snapshot("violet");
        let saved = vault.write_vault(&initial, None).unwrap();
        assert_eq!(saved["uiConfig"]["accentPreset"], json!("violet"));
        let read = vault.read_vault().unwrap();
        assert_eq!(read["uiConfig"]["workspaceTitle"], json!("Custom Studio Rig"));
        assert_eq!(read["background"]["customSlots"].as_array().unwrap().len(), 1);

        // Pre-update backup before changing theme.
        let pre_update = vault.create_backup(&saved, BackupReason::PreUpdate).unwrap();
        assert_eq!(pre_update.backups.len(), 1);
        assert_eq!(pre_update.backup.accent_preset, "violet");
        assert!(pre_update.backup.id.starts_with("pre-update-"));

        // Change live vault to amber.
        vault.write_vault(&sample_snapshot("amber"), None).unwrap();
        assert_eq!(vault.read_vault().unwrap()["uiConfig"]["accentPreset"], json!("amber"));

        // Restore -> vault reverts to violet cleanly.
        let restored = vault.restore_backup(&pre_update.backup.id).expect("restore must succeed");
        assert_eq!(restored.snapshot["uiConfig"]["accentPreset"], json!("violet"));
        assert_eq!(restored.snapshot["reason"], json!("restore"));
        let live = vault.read_vault().unwrap();
        assert_eq!(live["uiConfig"]["accentPreset"], json!("violet"));
        assert_eq!(live["background"]["customSlots"][0]["label"], json!("Studio Wallpaper"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn vault_writes_and_clears_post_update_receipts_reliably() {
        let dir = temp_dir("receipt");
        let vault = PreferencesVault::open(&dir).unwrap();
        assert!(vault.read_receipt().is_none());

        let receipt = UpdateReceipt {
            updated_at: 1_750_000_000_000,
            previous_version: "0.1.0".into(),
            new_version: "0.2.0".into(),
            previous_commit: Some("abc1234".into()),
            new_commit: Some("def5678".into()),
            backup_id: "pre-update-123".into(),
            mode: "git-pull".into(),
            preserved_categories: vec!["Theme & Colors".into(), "Backdrop & Media".into()],
        };
        vault.write_receipt(&receipt).unwrap();
        let loaded = vault.read_receipt().unwrap();
        assert_eq!(loaded.new_version, "0.2.0");
        assert_eq!(loaded.backup_id, "pre-update-123");
        assert_eq!(loaded, receipt);

        vault.clear_receipt();
        assert!(vault.read_receipt().is_none());
        assert!(!vault.receipt_path().exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rolling_history_prunes_to_twelve_newest_backups() {
        let dir = temp_dir("prune");
        let vault = PreferencesVault::open(&dir).unwrap();
        let snap = sample_snapshot("violet");
        let mut ids = Vec::new();
        for i in 0..15 {
            let outcome = vault.create_backup(&snap, BackupReason::Manual).unwrap();
            ids.push(outcome.backup.id.clone());
            assert!(outcome.backup.id.starts_with("manual-"));
            // Ensure distinct millisecond stamps (Windows timer granularity).
            std::thread::sleep(std::time::Duration::from_millis(20));
            let _ = i;
        }
        let listed = vault.list_backups();
        assert_eq!(listed.len(), MAX_BACKUPS);
        // Newest 12 kept (ids were created in order).
        let kept: Vec<&str> = listed.iter().map(|b| b.id.as_str()).collect();
        for id in &ids[3..] {
            assert!(kept.contains(&id.as_str()), "newest backup {id} must survive prune");
        }
        for id in &ids[..3] {
            assert!(!kept.contains(&id.as_str()), "oldest backup {id} must be pruned");
            assert!(!dir.join("backups").join(format!("{id}.json")).exists());
        }
        let on_disk = std::fs::read_dir(dir.join("backups")).unwrap().count();
        assert_eq!(on_disk, MAX_BACKUPS);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn restore_rejects_traversal_and_unknown_ids() {
        let dir = temp_dir("traversal");
        let vault = PreferencesVault::open(&dir).unwrap();
        let outcome = vault.create_backup(&sample_snapshot("violet"), BackupReason::Manual).unwrap();
        for bad in ["../evil", "..", "", "a/b", "a\\b", "missing-id"] {
            assert!(vault.restore_backup(bad).is_none(), "must reject {bad:?}");
        }
        // Basename of a valid id still works (TS path.basename parity).
        let full = format!("backups/{}.json", outcome.backup.id);
        assert!(vault.restore_backup(&full).is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sqlite_mirror_heals_missing_or_corrupt_json_vault() {
        let dir = temp_dir("heal");
        let vault = PreferencesVault::open(&dir).unwrap();
        vault.write_vault(&sample_snapshot("violet"), None).unwrap();
        // SQLite sync happened.
        let mirrored: String = vault
            .conn
            .query_row("SELECT v FROM kv_json WHERE k = 'preferences_vault'", [], |r| r.get(0))
            .unwrap();
        assert!(mirrored.contains("Custom Studio Rig"));
        assert!(vault.preferences_captured_at() > 0);

        // Corrupt the JSON file -> read falls back to SQLite and heals it.
        std::fs::write(vault.vault_path(), "{not json").unwrap();
        let healed = vault.read_vault().expect("sqlite fallback must heal");
        assert_eq!(healed["uiConfig"]["workspaceTitle"], json!("Custom Studio Rig"));
        assert!(serde_json::from_str::<Value>(&std::fs::read_to_string(vault.vault_path()).unwrap()).is_ok());

        // Delete the JSON file entirely -> same recovery.
        std::fs::remove_file(vault.vault_path()).unwrap();
        assert!(vault.read_vault().is_some());
        assert!(vault.vault_path().exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn receipt_survives_deleted_file_via_sqlite_mirror() {
        let dir = temp_dir("receipt-mirror");
        let vault = PreferencesVault::open(&dir).unwrap();
        let receipt = UpdateReceipt {
            updated_at: 42,
            previous_version: "0.1.0".into(),
            new_version: "0.2.0".into(),
            previous_commit: None,
            new_commit: None,
            backup_id: "manual-1".into(),
            mode: "managed-refresh".into(),
            preserved_categories: vec![],
        };
        vault.write_receipt(&receipt).unwrap();
        std::fs::remove_file(vault.receipt_path()).unwrap();
        assert_eq!(vault.read_receipt(), Some(receipt));
        assert!(vault.receipt_path().exists(), "mirror read heals the file");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn atomic_writes_leave_no_tmp_files_and_replace_existing() {
        let dir = temp_dir("atomic");
        let target = dir.join("nested").join("vault.json");
        write_json_atomic(&target, &json!({"v": 1})).unwrap();
        write_json_atomic(&target, &json!({"v": 2})).unwrap();
        assert_eq!(read_json_file(&target).unwrap()["v"], json!(2));
        let leftovers: Vec<_> = std::fs::read_dir(dir.join("nested"))
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reopen_sees_persisted_vault_backups_and_receipt() {
        let dir = temp_dir("reopen");
        let receipt = UpdateReceipt {
            updated_at: 7,
            previous_version: "0.1.0".into(),
            new_version: "0.1.1".into(),
            previous_commit: None,
            new_commit: None,
            backup_id: "manual-x".into(),
            mode: "managed-refresh".into(),
            preserved_categories: vec!["Theme & Colors".into()],
        };
        {
            let vault = PreferencesVault::open(&dir).unwrap();
            vault.write_vault(&sample_snapshot("amber"), None).unwrap();
            vault.create_backup(&sample_snapshot("amber"), BackupReason::Manual).unwrap();
            vault.write_receipt(&receipt).unwrap();
        }
        let reopened = PreferencesVault::open(&dir).unwrap();
        assert_eq!(reopened.read_vault().unwrap()["uiConfig"]["accentPreset"], json!("amber"));
        assert_eq!(reopened.list_backups().len(), 1);
        assert_eq!(reopened.read_receipt(), Some(receipt));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn shared_db_path_syncs_with_voxcell_store_kv_table() {
        let dir = temp_dir("shared-db");
        let db = dir.join("shared.db");
        let store = crate::VoxcellStore::open_at(&db, dir.join("secrets")).unwrap();
        store.save_window_prefs(&crate::WindowPrefs::default()).unwrap();
        let vault = PreferencesVault::open_with_db(
            dir.join("vault-data"),
            &db,
            Box::new(WebJsonAdapter),
            "0.1.0",
        )
        .unwrap();
        vault.write_vault(&sample_snapshot("violet"), None).unwrap();
        // Both connections see the same kv_json table.
        let raw: String = store
            .conn
            .query_row("SELECT v FROM kv_json WHERE k = 'preferences_vault'", [], |r| r.get(0))
            .unwrap();
        assert!(raw.contains("Custom Studio Rig"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn normalizer_rejects_non_objects_and_sanitizes_meta() {
        let adapter = WebJsonAdapter;
        assert!(adapter.normalize(&Value::Null, "0.1.0").is_none());
        assert!(adapter.normalize(&json!([1, 2]), "0.1.0").is_none());
        assert!(adapter.normalize(&json!("text"), "0.1.0").is_none());

        let normalized = adapter
            .normalize(
                &json!({
                    "capturedAt": "not-a-number",
                    "appVersion": "   ",
                    "reason": "bogus",
                    "snapshotId": "  id-1  ",
                    "sourceOrigin": "https://example.com"
                }),
                "9.9.9",
            )
            .unwrap();
        assert_eq!(normalized["schemaVersion"], json!(1));
        assert_eq!(normalized["appVersion"], json!("9.9.9"));
        assert_eq!(normalized["reason"], json!("sync"));
        assert_eq!(normalized["snapshotId"], json!("id-1"));
        assert!(normalized["capturedAt"].as_i64().unwrap() > 0);
        assert!(normalized.get("themeEpoch").is_none());
        // Missing sections filled with web defaults.
        assert_eq!(normalized["background"]["activeId"], json!("aurora-drift"));
        assert_eq!(normalized["workspace"]["mood"], json!("happy"));
        assert_eq!(normalized["canvas"]["sessions"], json!([]));

        let slate = adapter.normalize(&json!({"themeEpoch": "slate-v1", "reason": "pre-update"}), "0.1.0").unwrap();
        assert_eq!(slate["themeEpoch"], json!("slate-v1"));
        assert_eq!(slate["reason"], json!("pre-update"));
    }

    #[test]
    fn preserved_categories_flag_customization_and_defaults() {
        let adapter = WebJsonAdapter;
        let default_snap = adapter.normalize(&json!({}), "0.1.0").unwrap();
        let cats = adapter.preserved_categories(&default_snap);
        assert_eq!(cats.len(), 5);
        assert!(cats.iter().all(|c| !c.customized), "web defaults must not flag customization");
        assert_eq!(cats[0].label, "Theme & Colors");
        assert_eq!(cats[1].label, "Backdrop & Media");
        assert_eq!(cats[2].label, "Layout & Typography");
        assert_eq!(cats[3].label, "Stage, Voice & Trace");
        assert_eq!(cats[4].label, "Canvas & Tour State");

        let custom = sample_snapshot("violet");
        let cats = adapter.preserved_categories(&custom);
        assert!(cats.iter().find(|c| c.id == "theme").unwrap().customized);
        assert!(cats.iter().find(|c| c.id == "backdrop").unwrap().customized);
        assert!(cats.iter().find(|c| c.id == "layout").unwrap().customized); // workspaceTitle
        assert!(cats.iter().find(|c| c.id == "stage").unwrap().customized); // showThinking false
        assert!(cats.iter().find(|c| c.id == "canvas").unwrap().customized); // onboarding done
    }

    #[test]
    fn storage_label_matches_server_parity() {
        let home = dirs::home_dir().unwrap();
        assert_eq!(
            storage_label_for(&home.join(".voxcell").join("preferences-vault.json")),
            "~/.voxcell/preferences-vault.json"
        );
        assert_eq!(
            storage_label_for(Path::new("/srv/other/preferences-vault.json")),
            "Server preferences vault"
        );
    }

    #[test]
    fn receipt_parser_coerces_like_typescript() {
        let parsed = parse_receipt(
            &json!({
                "updatedAt": 123.0,
                "previousVersion": "",
                "newVersion": 5,
                "backupId": null,
                "mode": "weird-mode",
                "preservedCategories": ["A", 7, null]
            }),
            "0.1.0",
        )
        .unwrap();
        assert_eq!(parsed.updated_at, 123);
        assert_eq!(parsed.previous_version, "0.1.0");
        assert_eq!(parsed.new_version, "5");
        assert_eq!(parsed.backup_id, "");
        assert_eq!(parsed.mode, "managed-refresh");
        assert_eq!(parsed.preserved_categories, vec!["A", "7", "null"]);
        assert!(parse_receipt(&json!({"previousVersion": "1.0.0"}), "0.1.0").is_none());
        assert!(parse_receipt(&json!({"updatedAt": "soon"}), "0.1.0").is_none());
    }

    #[test]
    fn backup_stamping_preserves_existing_captured_at() {
        let adapter = WebJsonAdapter;
        let snap = json!({"capturedAt": 555, "reason": "sync"});
        let stamped = adapter.stamp_backup(&snap, "manual-1", 999, "manual");
        assert_eq!(stamped["capturedAt"], json!(555));
        assert_eq!(stamped["snapshotId"], json!("manual-1"));
        assert_eq!(stamped["reason"], json!("manual"));
        let zero = adapter.stamp_backup(&json!({"capturedAt": 0}), "manual-2", 999, "manual");
        assert_eq!(zero["capturedAt"], json!(999));
        let restored = adapter.stamp_restore(&stamped, "manual-1", 1234);
        assert_eq!(restored["capturedAt"], json!(1234));
        assert_eq!(restored["reason"], json!("restore"));
        assert_eq!(restored["snapshotId"], json!("manual-1"));
    }
}
