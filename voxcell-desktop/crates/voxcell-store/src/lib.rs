//! voxcell-store — SQLite persistence, durable preferences vault, OS-keychain
//! secrets, and the desktop self-updater (spec §7.4, tickets P1-03 / P1-04).
//!
//! * [`VoxcellStore`] — SQLite KV store for UI config, canvas dots, window
//!   prefs, and secrets (keychain-backed since drift #5 was fixed).
//! * [`vault::PreferencesVault`] — atomic JSON preferences vault + SQLite sync,
//!   rolling 12-backup history, restore, and `UpdateReceipt` persistence.
//! * [`updater`] — local build metadata, GitHub release/commit probe,
//!   `compare_semver`, pre-update backup hook, atomic apply + SHA-256 asset
//!   verification for desktop binaries.
//! * [`secrets::SecretStore`] — Windows Credential Manager / macOS Keychain /
//!   Linux Secret Service via `keyring`, with an explicit-warning restricted
//!   file fallback.

pub mod secrets;
pub mod updater;
pub mod vault;

pub use secrets::{SecretBackendKind, SecretStore};
pub use updater::{
    AppUpdateStatus, ApplyUpdateOutcome, LocalBuildMetadata, UpdateMode, UpdateStepResult,
    UpstreamReleaseInfo,
};
pub use vault::{
    BackupReason, PreferenceBackupSummary, PreferencesVault, PreservedCategorySummary,
    UpdateReceipt,
};

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use voxcell_core::{
    BackgroundSettings, UiCustomizationConfig, VoiceSettings, canvas::CanvasDotData,
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowPrefs {
    pub width: f32,
    pub height: f32,
    pub phone_widget_open: bool,
    pub phone_x: Option<i32>,
    pub phone_y: Option<i32>,
}

impl Default for WindowPrefs {
    fn default() -> Self {
        Self {
            width: 1280.0,
            height: 800.0,
            phone_widget_open: false,
            phone_x: None,
            phone_y: None,
        }
    }
}

pub struct VoxcellStore {
    conn: Connection,
    secrets: Mutex<SecretStore>,
}

impl VoxcellStore {
    pub fn open_default() -> Result<Self, String> {
        let base = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("voxcell");
        std::fs::create_dir_all(&base).map_err(|e| format!("create data dir: {e}"))?;
        let db_path = base.join("voxcell.db");
        let secrets_dir = base.join("secrets");
        std::fs::create_dir_all(&secrets_dir).map_err(|e| format!("create secrets dir: {e}"))?;
        Self::open_at(&db_path, secrets_dir)
    }

    /// Open with OS-keychain-preferred secrets (restricted-file fallback).
    pub fn open_at(db_path: &Path, secrets_dir: PathBuf) -> Result<Self, String> {
        Self::open_at_with_secrets(db_path, SecretStore::new(secrets_dir))
    }

    /// Open with an explicit secret backend (tests use
    /// [`SecretStore::file_only`] to stay off the real OS keychain).
    pub fn open_at_with_secrets(db_path: &Path, secrets: SecretStore) -> Result<Self, String> {
        std::fs::create_dir_all(secrets.fallback_dir())
            .map_err(|e| format!("create secrets dir: {e}"))?;
        let conn = Connection::open(db_path).map_err(|e| format!("open sqlite: {e}"))?;
        let store = Self { conn, secrets: Mutex::new(secrets) };
        store.init_schema()?;
        Ok(store)
    }

    fn init_schema(&self) -> Result<(), String> {
        self.conn
            .execute_batch(
                "
                PRAGMA journal_mode = WAL;
                CREATE TABLE IF NOT EXISTS kv_json (
                    k TEXT PRIMARY KEY,
                    v TEXT NOT NULL,
                    updated_at INTEGER NOT NULL DEFAULT (strftime('%s','now'))
                );
                CREATE TABLE IF NOT EXISTS canvas_sessions (
                    id TEXT PRIMARY KEY,
                    channel_id TEXT NOT NULL,
                    label TEXT NOT NULL,
                    shape TEXT NOT NULL,
                    x REAL NOT NULL,
                    y REAL NOT NULL,
                    pinned INTEGER NOT NULL DEFAULT 0,
                    color_hex TEXT NOT NULL
                );
                CREATE TABLE IF NOT EXISTS dismissed_channels (
                    channel_id TEXT PRIMARY KEY
                );
                ",
            )
            .map_err(|e| format!("init sqlite schema: {e}"))?;
        Ok(())
    }

    pub fn load_ui_config(&self) -> UiCustomizationConfig {
        self.get_json("ui_config").unwrap_or_default()
    }

    pub fn save_ui_config(&self, cfg: &UiCustomizationConfig) -> Result<(), String> {
        self.set_json("ui_config", cfg)
    }

    pub fn load_background_settings(&self) -> BackgroundSettings {
        self.get_json("background_settings").unwrap_or_default()
    }

    pub fn save_background_settings(&self, bg: &BackgroundSettings) -> Result<(), String> {
        self.set_json("background_settings", bg)
    }

    pub fn load_voice_settings(&self) -> VoiceSettings {
        self.get_json("voice_settings").unwrap_or_default()
    }

    pub fn save_voice_settings(&self, voice: &VoiceSettings) -> Result<(), String> {
        self.set_json("voice_settings", voice)
    }

    pub fn load_window_prefs(&self) -> WindowPrefs {
        self.get_json("window_prefs").unwrap_or_default()
    }

    pub fn save_window_prefs(&self, prefs: &WindowPrefs) -> Result<(), String> {
        self.set_json("window_prefs", prefs)
    }

    pub fn load_canvas_dots(&self) -> Result<Vec<CanvasDotData>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, channel_id, label, shape, x, y, pinned, color_hex FROM canvas_sessions ORDER BY rowid ASC",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                let shape_json: String = row.get(3)?;
                let shape = serde_json::from_str(&shape_json).unwrap_or_default();
                Ok(CanvasDotData {
                    id: row.get(0)?,
                    channel_id: row.get(1)?,
                    label: row.get(2)?,
                    shape,
                    x: row.get(4)?,
                    y: row.get(5)?,
                    pinned: row.get::<_, i64>(6)? != 0,
                    color_hex: row.get(7)?,
                })
            })
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| e.to_string())?);
        }
        Ok(out)
    }

    pub fn save_canvas_dots(&mut self, dots: &[CanvasDotData]) -> Result<(), String> {
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        tx.execute("DELETE FROM canvas_sessions", [])
            .map_err(|e| e.to_string())?;
        {
            let mut stmt = tx
                .prepare(
                    "INSERT INTO canvas_sessions (id, channel_id, label, shape, x, y, pinned, color_hex)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                )
                .map_err(|e| e.to_string())?;
            for dot in dots {
                let shape_json = serde_json::to_string(&dot.shape).unwrap_or_else(|_| "\"hermes_orb\"".into());
                stmt.execute(params![
                    dot.id,
                    dot.channel_id,
                    dot.label,
                    shape_json,
                    dot.x,
                    dot.y,
                    i64::from(dot.pinned),
                    dot.color_hex,
                ])
                .map_err(|e| e.to_string())?;
            }
        }
        tx.commit().map_err(|e| e.to_string())
    }

    pub fn load_dismissed_channels(&self) -> HashSet<String> {
        let Ok(mut stmt) = self.conn.prepare("SELECT channel_id FROM dismissed_channels") else {
            return HashSet::new();
        };
        let Ok(rows) = stmt.query_map([], |r| r.get::<_, String>(0)) else {
            return HashSet::new();
        };
        rows.flatten().collect()
    }

    pub fn dismiss_channel(&self, channel_id: &str) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT OR IGNORE INTO dismissed_channels (channel_id) VALUES (?1)",
                params![channel_id],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Store a secret in the OS keychain (restricted-file fallback with an
    /// explicit warning when no keychain is reachable). Blank values delete.
    pub fn set_secret(&self, key_name: &str, secret: &str) -> Result<(), String> {
        let mut secrets = self.secrets.lock().map_err(|e| format!("secrets lock: {e}"))?;
        secrets.set_secret(key_name, secret)
    }

    /// Environment override first, then OS keychain, then restricted file.
    pub fn get_secret(&self, key_name: &str) -> Option<String> {
        let mut secrets = self.secrets.lock().ok()?;
        secrets.get_secret(key_name)
    }

    pub fn delete_secret(&self, key_name: &str) -> Result<(), String> {
        let mut secrets = self.secrets.lock().map_err(|e| format!("secrets lock: {e}"))?;
        secrets.delete_secret(key_name)
    }

    /// Which secret backend is active (for the settings UI badge).
    pub fn secret_backend(&self) -> SecretBackendKind {
        self.secrets.lock().map(|s| s.backend()).unwrap_or(SecretBackendKind::RestrictedFile)
    }

    /// Explicit warning to surface when secrets fell back to restricted files.
    pub fn secret_warning(&self) -> Option<String> {
        self.secrets.lock().ok().and_then(|s| s.warning().map(|w| w.to_string()))
    }

    fn get_json<T: for<'de> Deserialize<'de>>(&self, key: &str) -> Option<T> {
        let raw: Option<String> = self
            .conn
            .query_row("SELECT v FROM kv_json WHERE k = ?1", params![key], |r| r.get(0))
            .optional()
            .ok()
            .flatten();
        raw.and_then(|s| serde_json::from_str(&s).ok())
    }

    fn set_json<T: Serialize>(&self, key: &str, value: &T) -> Result<(), String> {
        let json = serde_json::to_string(value).map_err(|e| e.to_string())?;
        self.conn
            .execute(
                "INSERT INTO kv_json (k, v, updated_at) VALUES (?1, ?2, strftime('%s','now'))
                 ON CONFLICT(k) DO UPDATE SET v = excluded.v, updated_at = excluded.updated_at",
                params![key, json],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use voxcell_core::canvas::CanvasShapeKind;

    #[test]
    fn persists_ui_config_canvas_dots_and_secrets_to_sqlite() {
        let dir = std::env::temp_dir().join(format!("voxcell-store-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("test.db");
        let secrets = dir.join("secrets");

        // File-only secret backend keeps unit tests off the real OS keychain
        // (assertions unchanged from the original scaffold test).
        let mut store =
            VoxcellStore::open_at_with_secrets(&db, SecretStore::file_only(secrets.clone())).unwrap();
        let mut ui = UiCustomizationConfig::default();
        ui.accent_hex = "#22D3EE".into();
        ui.bloom = 0.61;
        store.save_ui_config(&ui).unwrap();

        let dots = vec![CanvasDotData {
            id: "d1".into(),
            channel_id: "home".into(),
            label: "Home".into(),
            shape: CanvasShapeKind::FoxSpirit,
            x: 240.0,
            y: 180.0,
            pinned: true,
            color_hex: "#34D399".into(),
        }];
        store.save_canvas_dots(&dots).unwrap();
        store.set_secret("HERMES_API_KEY", "tok-123").unwrap();
        drop(store);

        let reopened =
            VoxcellStore::open_at_with_secrets(&db, SecretStore::file_only(secrets)).unwrap();
        let loaded_ui = reopened.load_ui_config();
        assert_eq!(loaded_ui.accent_hex, "#22D3EE");
        assert!((loaded_ui.bloom - 0.61).abs() < 1e-4);

        let loaded_dots = reopened.load_canvas_dots().unwrap();
        assert_eq!(loaded_dots.len(), 1);
        assert_eq!(loaded_dots[0].shape, CanvasShapeKind::FoxSpirit);
        assert_eq!(reopened.get_secret("HERMES_API_KEY"), Some("tok-123".into()));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn store_secret_surface_reports_backend_and_warning() {
        let dir = std::env::temp_dir().join(format!(
            "voxcell-store-backend-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let store = VoxcellStore::open_at_with_secrets(
            &dir.join("test.db"),
            SecretStore::file_only(dir.join("secrets")),
        )
        .unwrap();
        assert_eq!(store.secret_backend(), SecretBackendKind::RestrictedFile);
        assert!(store.secret_warning().is_none());
        store.set_secret("DEEPGRAM_API_KEY", "fake-store-key").unwrap();
        assert_eq!(store.get_secret("DEEPGRAM_API_KEY").as_deref(), Some("fake-store-key"));
        store.delete_secret("DEEPGRAM_API_KEY").unwrap();
        assert_eq!(store.get_secret("DEEPGRAM_API_KEY"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
