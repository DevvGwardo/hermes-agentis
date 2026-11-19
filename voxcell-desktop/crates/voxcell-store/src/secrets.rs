//! OS-keychain secret storage — fixes scaffold drift #5 (spec §4, §7.4, ticket P1-03).
//!
//! Replaces plaintext `0600` `.key` files with the `keyring` crate:
//! Windows Credential Manager, macOS Keychain, or Linux Secret Service.
//! When no OS keychain is reachable (e.g. headless Linux without D-Bus),
//! secrets fall back to restricted local files with an **explicit warning**.
//!
//! Managed keys: `HERMES_API_KEY`, `HERMES_API_KEY_<PROFILE>`, `DEEPGRAM_API_KEY`
//! (plus non-secret settings such as `HERMES_API_URL` historically stored here).
//! Environment variables always take precedence over stored secrets.
//!
//! Tests never touch the real OS keychain: they inject [`MemoryKeychain`] or
//! use [`SecretStore::file_only`].

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Service name used for every keyring entry (the "account" is the key name).
pub const KEYRING_SERVICE: &str = "voxcell-desktop";

/// Which backend currently holds secrets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SecretBackendKind {
    /// Windows Credential Manager / macOS Keychain / Linux Secret Service.
    OsKeychain,
    /// Restricted local files (Unix `0600`) used when no OS keychain exists.
    RestrictedFile,
}

/// Sanitize a secret key name to `[A-Za-z0-9_-]` (same rule the scaffold used).
/// Returns `None` when nothing usable remains.
pub fn clean_key_name(key_name: &str) -> Option<String> {
    let clean: String = key_name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .collect();
    if clean.is_empty() { None } else { Some(clean) }
}

/// Pluggable keychain backend so tests can run without a real OS keychain.
pub trait Keychain: Send + Sync {
    /// Human-readable backend label for diagnostics.
    fn label(&self) -> String;
    fn set(&self, key: &str, value: &str) -> Result<(), String>;
    /// `Ok(None)` means "no entry"; `Err` means the backend is unreachable.
    fn get(&self, key: &str) -> Result<Option<String>, String>;
    fn delete(&self, key: &str) -> Result<(), String>;
}

/// Real OS keychain via the `keyring` crate.
pub struct OsKeychain {
    service: String,
}

impl OsKeychain {
    pub fn new() -> Self {
        Self { service: KEYRING_SERVICE.to_string() }
    }

    pub fn with_service(service: impl Into<String>) -> Self {
        Self { service: service.into() }
    }

    fn entry(&self, key: &str) -> Result<keyring::Entry, String> {
        keyring::Entry::new(&self.service, key).map_err(|e| format!("keyring entry: {e}"))
    }
}

impl Default for OsKeychain {
    fn default() -> Self {
        Self::new()
    }
}

impl Keychain for OsKeychain {
    fn label(&self) -> String {
        format!("OS keychain ({})", self.service)
    }

    fn set(&self, key: &str, value: &str) -> Result<(), String> {
        self.entry(key)?
            .set_password(value)
            .map_err(|e| format!("keychain write failed: {e}"))
    }

    fn get(&self, key: &str) -> Result<Option<String>, String> {
        match self.entry(key)?.get_password() {
            Ok(v) => Ok(Some(v)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(format!("keychain read failed: {e}")),
        }
    }

    fn delete(&self, key: &str) -> Result<(), String> {
        match self.entry(key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(format!("keychain delete failed: {e}")),
        }
    }
}

/// Restricted-file fallback. On Unix the file is chmod `0600`; on Windows it
/// lives inside the user's private data directory (per-user ACL by default).
pub struct FileKeychain {
    dir: PathBuf,
}

impl FileKeychain {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn path_for(&self, key: &str) -> PathBuf {
        self.dir.join(format!("{key}.key"))
    }

    /// True when a legacy/fallback file exists for `key`.
    pub fn has_file(&self, key: &str) -> bool {
        self.path_for(key).is_file()
    }
}

impl Keychain for FileKeychain {
    fn label(&self) -> String {
        format!("restricted files ({})", self.dir.display())
    }

    fn set(&self, key: &str, value: &str) -> Result<(), String> {
        std::fs::create_dir_all(&self.dir).map_err(|e| format!("create secrets dir: {e}"))?;
        let path = self.path_for(key);
        std::fs::write(&path, value).map_err(|e| format!("write secret file: {e}"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
                .map_err(|e| format!("restrict secret file: {e}"))?;
        }
        Ok(())
    }

    fn get(&self, key: &str) -> Result<Option<String>, String> {
        match std::fs::read_to_string(self.path_for(key)) {
            Ok(s) => {
                let t = s.trim();
                Ok(if t.is_empty() { None } else { Some(t.to_string()) })
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("read secret file: {e}")),
        }
    }

    fn delete(&self, key: &str) -> Result<(), String> {
        match std::fs::remove_file(self.path_for(key)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("delete secret file: {e}")),
        }
    }
}

/// In-memory keychain for unit tests (never touches the OS).
pub struct MemoryKeychain {
    entries: Mutex<HashMap<String, String>>,
    /// When true, every operation fails — simulates an unreachable keychain.
    pub fail_all: bool,
}

impl MemoryKeychain {
    pub fn new() -> Self {
        Self { entries: Mutex::new(HashMap::new()), fail_all: false }
    }

    pub fn failing() -> Self {
        Self { entries: Mutex::new(HashMap::new()), fail_all: true }
    }

    fn guard(&self) -> Result<std::sync::MutexGuard<'_, HashMap<String, String>>, String> {
        if self.fail_all {
            return Err("simulated keychain failure (no D-Bus / headless session)".into());
        }
        self.entries.lock().map_err(|e| format!("keychain lock: {e}"))
    }
}

impl Default for MemoryKeychain {
    fn default() -> Self {
        Self::new()
    }
}

impl Keychain for MemoryKeychain {
    fn label(&self) -> String {
        "memory keychain (test)".into()
    }

    fn set(&self, key: &str, value: &str) -> Result<(), String> {
        self.guard()?.insert(key.to_string(), value.to_string());
        Ok(())
    }

    fn get(&self, key: &str) -> Result<Option<String>, String> {
        Ok(self.guard()?.get(key).cloned())
    }

    fn delete(&self, key: &str) -> Result<(), String> {
        self.guard()?.remove(key);
        Ok(())
    }
}

/// Secret storage with OS-keychain preference and a restricted-file fallback.
///
/// * `set_secret` tries the OS keychain first; on failure it records an
///   explicit warning and writes the restricted fallback file instead.
/// * `get_secret` checks the environment first (parity with the scaffold),
///   then the OS keychain, then the fallback file. Legacy `.key` files are
///   migrated into the OS keychain automatically when it is reachable.
pub struct SecretStore {
    primary: Box<dyn Keychain>,
    fallback: FileKeychain,
    mode: SecretBackendKind,
    warning: Option<String>,
    /// True when the primary backend IS the restricted-file store
    /// ([`SecretStore::file_only`]). Deliberate file mode must never be
    /// "upgraded" to keychain mode, and its files must never be treated as
    /// stale fallback copies to clean up (primary and fallback share a dir).
    primary_is_file: bool,
}

impl SecretStore {
    /// Production default: OS keychain preferred, restricted files as fallback.
    pub fn new(fallback_dir: impl Into<PathBuf>) -> Self {
        Self::with_keychain(Box::new(OsKeychain::new()), fallback_dir)
    }

    /// Fallback-only store (headless deployments / unit tests).
    pub fn file_only(dir: impl Into<PathBuf>) -> Self {
        let dir = dir.into();
        Self {
            primary: Box::new(FileKeychain::new(dir.clone())),
            fallback: FileKeychain::new(dir),
            mode: SecretBackendKind::RestrictedFile,
            warning: None,
            primary_is_file: true,
        }
    }

    /// Inject any backend (tests use [`MemoryKeychain`]).
    pub fn with_keychain(primary: Box<dyn Keychain>, fallback_dir: impl Into<PathBuf>) -> Self {
        Self {
            primary,
            fallback: FileKeychain::new(fallback_dir),
            mode: SecretBackendKind::OsKeychain,
            warning: None,
            primary_is_file: false,
        }
    }

    pub fn backend(&self) -> SecretBackendKind {
        self.mode
    }

    /// Explicit warning shown to the user when the OS keychain is unavailable.
    pub fn warning(&self) -> Option<&str> {
        self.warning.as_deref()
    }

    pub fn fallback_dir(&self) -> &Path {
        self.fallback.dir()
    }

    fn record_fallback(&mut self, reason: &str) {
        self.mode = SecretBackendKind::RestrictedFile;
        if self.primary_is_file {
            // Deliberate file mode is not a degraded keychain fallback; an
            // I/O hiccup here must not surface an "OS keychain unavailable"
            // warning. The underlying error still propagates to the caller.
            return;
        }
        self.warning = Some(format!(
            "OS keychain unavailable ({reason}). Secrets fall back to restricted local files in {}. \
             On Unix these are chmod 0600; prefer a desktop session with Windows Credential Manager, \
             macOS Keychain, or Linux Secret Service (D-Bus) for full protection.",
            self.fallback.dir().display()
        ));
    }

    /// Store (or delete, when `secret` is blank) a secret. Never logs the value.
    pub fn set_secret(&mut self, key_name: &str, secret: &str) -> Result<(), String> {
        let key = clean_key_name(key_name).ok_or("invalid secret key name")?;
        let trimmed = secret.trim();
        if trimmed.is_empty() {
            return self.delete_secret(&key);
        }
        match self.primary.set(&key, trimmed) {
            Ok(()) => {
                if self.primary_is_file {
                    // Deliberate file-only mode: the primary IS the restricted
                    // file. Stay in file mode without a degraded-fallback
                    // warning, and never delete the "fallback copy" — it is
                    // the very file we just wrote (same directory).
                    self.mode = SecretBackendKind::RestrictedFile;
                    self.warning = None;
                } else {
                    self.mode = SecretBackendKind::OsKeychain;
                    self.warning = None;
                    // Clean up any stale fallback copy once the keychain works.
                    let _ = self.fallback.delete(&key);
                }
                Ok(())
            }
            Err(e) => {
                self.record_fallback(&e);
                self.fallback.set(&key, trimmed)
            }
        }
    }

    /// Environment override first, then OS keychain, then restricted file.
    pub fn get_secret(&mut self, key_name: &str) -> Option<String> {
        if let Ok(env_val) = std::env::var(key_name) {
            let t = env_val.trim();
            if !t.is_empty() {
                return Some(t.to_string());
            }
        }
        let key = clean_key_name(key_name)?;
        match self.primary.get(&key) {
            Ok(Some(v)) => {
                let t = v.trim();
                if !t.is_empty() {
                    return Some(t.to_string());
                }
            }
            Ok(None) => {}
            Err(e) => self.record_fallback(&e),
        }
        // Fallback / legacy migration path.
        match self.fallback.get(&key) {
            Ok(Some(v)) => {
                if !self.primary_is_file && self.mode == SecretBackendKind::OsKeychain {
                    // Migrate legacy plaintext file into the OS keychain.
                    // (Never in file-only mode: primary and fallback are the
                    // same file, so "migrate then delete" would destroy it.)
                    if self.primary.set(&key, &v).is_ok() {
                        let _ = self.fallback.delete(&key);
                    }
                }
                Some(v)
            }
            _ => None,
        }
    }

    /// Remove a secret from both backends (best effort).
    pub fn delete_secret(&mut self, key_name: &str) -> Result<(), String> {
        let key = clean_key_name(key_name).ok_or("invalid secret key name")?;
        let _ = self.primary.delete(&key);
        self.fallback.delete(&key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "voxcell-secrets-test-{}-{tag}-{}",
            std::process::id(),
            now_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn now_nanos() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    }

    #[test]
    fn os_keychain_roundtrip_set_get_delete() {
        let dir = temp_dir("roundtrip");
        let mut store = SecretStore::with_keychain(Box::new(MemoryKeychain::new()), dir.clone());
        assert_eq!(store.backend(), SecretBackendKind::OsKeychain);
        assert!(store.warning().is_none());

        store.set_secret("HERMES_API_KEY", "fake-token-123").unwrap();
        store.set_secret("HERMES_API_KEY_CODER", "fake-coder-456").unwrap();
        store.set_secret("DEEPGRAM_API_KEY", "fake-dg-789").unwrap();
        assert_eq!(store.get_secret("HERMES_API_KEY").as_deref(), Some("fake-token-123"));
        assert_eq!(store.get_secret("HERMES_API_KEY_CODER").as_deref(), Some("fake-coder-456"));
        assert_eq!(store.get_secret("DEEPGRAM_API_KEY").as_deref(), Some("fake-dg-789"));
        assert_eq!(store.backend(), SecretBackendKind::OsKeychain);
        // Nothing leaked to disk when the keychain works.
        assert!(!dir.join("HERMES_API_KEY.key").exists());

        store.delete_secret("HERMES_API_KEY").unwrap();
        assert_eq!(store.get_secret("HERMES_API_KEY"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn falls_back_with_explicit_warning_when_keychain_unreachable() {
        let dir = temp_dir("fallback");
        let mut store =
            SecretStore::with_keychain(Box::new(MemoryKeychain::failing()), dir.clone());
        store.set_secret("HERMES_API_KEY", "fake-fallback-token").unwrap();
        assert_eq!(store.backend(), SecretBackendKind::RestrictedFile);
        let warning = store.warning().expect("fallback must carry an explicit warning");
        assert!(warning.contains("OS keychain unavailable"));
        assert!(warning.contains("0600"));
        assert!(dir.join("HERMES_API_KEY.key").is_file());
        assert_eq!(store.get_secret("HERMES_API_KEY").as_deref(), Some("fake-fallback-token"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fallback_read_failure_also_warns_and_recovers() {
        let dir = temp_dir("read-fallback");
        // Pre-seed a legacy file, then make the primary fail on read.
        std::fs::write(dir.join("DEEPGRAM_API_KEY.key"), "legacy-dg-key").unwrap();
        let mut store =
            SecretStore::with_keychain(Box::new(MemoryKeychain::failing()), dir.clone());
        assert_eq!(store.get_secret("DEEPGRAM_API_KEY").as_deref(), Some("legacy-dg-key"));
        assert_eq!(store.backend(), SecretBackendKind::RestrictedFile);
        assert!(store.warning().is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn fallback_files_are_chmod_0600() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp_dir("perms");
        let mut store = SecretStore::file_only(dir.clone());
        store.set_secret("HERMES_API_KEY", "fake-perm-token").unwrap();
        let mode = std::fs::metadata(dir.join("HERMES_API_KEY.key"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn legacy_file_migrates_into_keychain_on_read() {
        let dir = temp_dir("migrate");
        std::fs::write(dir.join("HERMES_API_KEY.key"), "legacy-migrated-token").unwrap();
        let mut store = SecretStore::with_keychain(Box::new(MemoryKeychain::new()), dir.clone());
        assert_eq!(store.get_secret("HERMES_API_KEY").as_deref(), Some("legacy-migrated-token"));
        // File removed after migration; value now lives in the keychain only.
        assert!(!dir.join("HERMES_API_KEY.key").exists());
        assert_eq!(store.get_secret("HERMES_API_KEY").as_deref(), Some("legacy-migrated-token"));
        assert_eq!(store.backend(), SecretBackendKind::OsKeychain);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn blank_secret_deletes_from_both_backends() {
        let dir = temp_dir("blank");
        let mut store = SecretStore::with_keychain(Box::new(MemoryKeychain::new()), dir.clone());
        store.set_secret("HERMES_API_KEY", "fake-token").unwrap();
        store.set_secret("HERMES_API_KEY", "   ").unwrap();
        assert_eq!(store.get_secret("HERMES_API_KEY"), None);
        assert!(!dir.join("HERMES_API_KEY.key").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn invalid_key_names_rejected_and_names_sanitized() {
        let dir = temp_dir("names");
        let mut store = SecretStore::file_only(dir.clone());
        assert!(store.set_secret("///", "x").is_err());
        assert!(store.set_secret("", "x").is_err());
        // Same sanitization rule as the scaffold: strip disallowed chars.
        store.set_secret("HERMES_API_KEY@2", "fake-sanitized").unwrap();
        assert!(dir.join("HERMES_API_KEY2.key").is_file());
        assert_eq!(store.get_secret("HERMES_API_KEY2").as_deref(), Some("fake-sanitized"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn file_only_store_never_uses_primary_keychain() {
        let dir = temp_dir("file-only");
        let mut store = SecretStore::file_only(dir.clone());
        assert_eq!(store.backend(), SecretBackendKind::RestrictedFile);
        store.set_secret("DEEPGRAM_API_KEY", "fake-file-only").unwrap();
        assert_eq!(store.get_secret("DEEPGRAM_API_KEY").as_deref(), Some("fake-file-only"));
        assert!(store.warning().is_none(), "deliberate file mode is not a degraded fallback");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn file_only_set_persists_file_and_survives_reopen() {
        // Regression: the keychain-success cleanup used to delete the very
        // file the file-only primary had just written (same directory), and
        // flipped the backend to OsKeychain.
        let dir = temp_dir("file-only-reopen");
        let mut store = SecretStore::file_only(dir.clone());
        store.set_secret("HERMES_API_KEY", "fake-persisted").unwrap();
        assert!(dir.join("HERMES_API_KEY.key").is_file(), "file-only write must persist");
        assert_eq!(store.backend(), SecretBackendKind::RestrictedFile);
        assert!(store.warning().is_none());
        drop(store);

        // A fresh store over the same directory must read the secret back.
        let mut reopened = SecretStore::file_only(dir.clone());
        assert_eq!(reopened.get_secret("HERMES_API_KEY").as_deref(), Some("fake-persisted"));
        assert_eq!(reopened.backend(), SecretBackendKind::RestrictedFile);
        assert!(reopened.warning().is_none());
        reopened.delete_secret("HERMES_API_KEY").unwrap();
        assert!(!dir.join("HERMES_API_KEY.key").exists());
        assert_eq!(reopened.get_secret("HERMES_API_KEY"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn env_override_takes_precedence() {
        let dir = temp_dir("env");
        let mut store = SecretStore::with_keychain(Box::new(MemoryKeychain::new()), dir.clone());
        store.set_secret("VOXCELL_TEST_ENV_SECRET", "stored-value").unwrap();
        // Unique variable name; edition 2024 requires unsafe for env mutation.
        unsafe { std::env::set_var("VOXCELL_TEST_ENV_SECRET", "env-value") };
        assert_eq!(store.get_secret("VOXCELL_TEST_ENV_SECRET").as_deref(), Some("env-value"));
        unsafe { std::env::remove_var("VOXCELL_TEST_ENV_SECRET") };
        assert_eq!(store.get_secret("VOXCELL_TEST_ENV_SECRET").as_deref(), Some("stored-value"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn os_keychain_backend_constructs_without_io() {
        // Entry construction must not touch the real credential store.
        let kc = OsKeychain::with_service("voxcell-desktop-test-construction");
        assert!(kc.label().contains("voxcell-desktop-test-construction"));
        assert!(kc.entry("HERMES_API_KEY").is_ok());
    }
}
