//! Desktop self-updater — port of `src/utils/appUpdater.ts` plus the
//! `/api/app/update/*` orchestration from `server.ts` (spec §5.1, §7.4,
//! ticket P1-04).
//!
//! * Local build metadata (version / commit / branch / dirty worktree) via git.
//! * Upstream GitHub release/commit probe (`DevvGwardo/hermes-agentis`).
//! * Lenient TS-parity `compare_semver`.
//! * Pre-update vault snapshot hook, atomic update apply, `UpdateReceipt`
//!   lifecycle (write on success, acknowledge/clear on next launch).
//! * Desktop extension: SHA-256 verification of `.dmg` / `.msi` / `.AppImage`
//!   assets **before** an atomic binary swap.
//!
//! All git/HTTP access is injectable ([`GitRunner`], `reqwest::Client` +
//! `api_base`) so unit tests run against temp dirs and mock HTTP only.

use crate::vault::{BackupReason, PreferencesVault, UpdateReceipt, now_ms};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const DEFAULT_REPO_SLUG: &str = "DevvGwardo/hermes-agentis";
pub const GITHUB_API_BASE: &str = "https://api.github.com";
pub const UPSTREAM_TIMEOUT_MS: u64 = 4000;
pub const DEFAULT_FALLBACK_VERSION: &str = "0.1.0";

/// `LocalBuildMetadata` in appUpdater.ts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalBuildMetadata {
    pub version: String,
    pub has_git_repo: bool,
    pub commit: Option<String>,
    pub branch: Option<String>,
    pub dirty_worktree: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dirty_reason: Option<String>,
}

/// `UpstreamReleaseInfo` in appUpdater.ts.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpstreamReleaseInfo {
    pub latest_version: Option<String>,
    pub latest_commit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_notes: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Update mode union from the TS contract.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum UpdateMode {
    #[serde(rename = "git-pull")]
    GitPull,
    #[serde(rename = "managed-refresh")]
    ManagedRefresh,
}

impl UpdateMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::GitPull => "git-pull",
            Self::ManagedRefresh => "managed-refresh",
        }
    }
}

/// `UpdateStepResult` in settingsSnapshot.ts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStepResult {
    pub id: String,
    pub label: String,
    pub status: StepStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StepStatus {
    Ok,
    Skipped,
    Failed,
}

impl UpdateStepResult {
    fn ok(id: &str, label: impl Into<String>) -> Self {
        Self { id: id.into(), label: label.into(), status: StepStatus::Ok, detail: None }
    }
    fn ok_detail(id: &str, label: impl Into<String>, detail: impl Into<String>) -> Self {
        Self { id: id.into(), label: label.into(), status: StepStatus::Ok, detail: Some(detail.into()) }
    }
    fn failed(id: &str, label: impl Into<String>, detail: impl Into<String>) -> Self {
        Self { id: id.into(), label: label.into(), status: StepStatus::Failed, detail: Some(detail.into()) }
    }
    fn skipped(id: &str, label: impl Into<String>) -> Self {
        Self { id: id.into(), label: label.into(), status: StepStatus::Skipped, detail: None }
    }
}

/// `AppUpdateStatus` in settingsSnapshot.ts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUpdateStatus {
    pub current_version: String,
    pub current_commit: Option<String>,
    pub branch: Option<String>,
    pub latest_version: Option<String>,
    pub latest_commit: Option<String>,
    pub update_available: bool,
    pub update_mode: UpdateMode,
    pub can_self_update: bool,
    pub dirty_worktree: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dirty_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_notes: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_url: Option<String>,
    pub checked_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream_error: Option<String>,
    pub pending_receipt: Option<UpdateReceipt>,
}

/// `ApplyUpdateResponse` in settingsVaultClient.ts.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyUpdateOutcome {
    pub ok: bool,
    pub status: AppUpdateStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt: Option<UpdateReceipt>,
    pub steps: Vec<UpdateStepResult>,
    pub needs_reload: bool,
    pub restarting: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Result of `applyGitPullUpdate` in appUpdater.ts.
#[derive(Clone, Debug)]
pub struct GitPullOutcome {
    pub ok: bool,
    pub steps: Vec<UpdateStepResult>,
    pub new_version: String,
    pub new_commit: Option<String>,
    pub error: Option<String>,
}

/// Failure of an external command (git / npm / bun).
#[derive(Clone, Debug)]
pub struct RunError {
    pub message: String,
    pub exit_code: Option<i32>,
}

/// Injectable command runner so tests never spawn real processes.
pub trait GitRunner: Send + Sync {
    fn run(
        &self,
        program: &str,
        args: &[&str],
        cwd: &Path,
        timeout_ms: u64,
    ) -> Result<String, RunError>;
}

/// Real process runner with TS-parity timeouts (kill on expiry).
pub struct SystemRunner;

impl GitRunner for SystemRunner {
    fn run(
        &self,
        program: &str,
        args: &[&str],
        cwd: &Path,
        timeout_ms: u64,
    ) -> Result<String, RunError> {
        use std::io::Read;
        use std::process::{Command, Stdio};
        let mut child = Command::new(program)
            .args(args)
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| RunError { message: format!("{program}: {e}"), exit_code: None })?;

        // Drain pipes concurrently so large outputs cannot deadlock the child.
        let mut stdout_pipe = child.stdout.take().expect("piped stdout");
        let mut stderr_pipe = child.stderr.take().expect("piped stderr");
        let out_handle = std::thread::spawn(move || {
            let mut buf = String::new();
            let _ = stdout_pipe.read_to_string(&mut buf);
            buf
        });
        let err_handle = std::thread::spawn(move || {
            let mut buf = String::new();
            let _ = stderr_pipe.read_to_string(&mut buf);
            buf
        });

        let deadline = std::time::Instant::now() + Duration::from_millis(timeout_ms);
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => {
                    if std::time::Instant::now() >= deadline {
                        let _ = child.kill();
                        let _ = child.wait();
                        let _ = out_handle.join();
                        let _ = err_handle.join();
                        return Err(RunError {
                            message: format!("{program} timed out after {timeout_ms}ms"),
                            exit_code: None,
                        });
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(e) => {
                    let _ = out_handle.join();
                    let _ = err_handle.join();
                    return Err(RunError { message: format!("{program}: {e}"), exit_code: None });
                }
            }
        };
        let stdout = out_handle.join().unwrap_or_default();
        let stderr = err_handle.join().unwrap_or_default();
        if status.success() {
            Ok(stdout.trim().to_string())
        } else {
            Err(RunError {
                message: format!(
                    "{program} exited with {}: {}",
                    status.code().map(|c| c.to_string()).unwrap_or_else(|| "signal".into()),
                    stderr.trim()
                ),
                exit_code: status.code(),
            })
        }
    }
}

/// `readPackageVersion` in appUpdater.ts.
pub fn read_package_version(app_root: &Path) -> String {
    let raw = std::fs::read_to_string(app_root.join("package.json")).unwrap_or_default();
    serde_json::from_str::<Value>(&raw)
        .ok()
        .and_then(|v| v.get("version").and_then(|x| x.as_str()).map(|s| s.trim().to_string()))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_FALLBACK_VERSION.to_string())
}

fn non_empty(s: String) -> Option<String> {
    if s.is_empty() { None } else { Some(s) }
}

/// `readLocalBuildMetadata` in appUpdater.ts (injectable runner).
pub fn read_local_build_metadata_with(app_root: &Path, runner: &dyn GitRunner) -> LocalBuildMetadata {
    let version = read_package_version(app_root);
    if !app_root.join(".git").exists() {
        return LocalBuildMetadata {
            version,
            has_git_repo: false,
            commit: None,
            branch: None,
            dirty_worktree: false,
            dirty_reason: None,
        };
    }
    let commit = runner
        .run("git", &["rev-parse", "--short", "HEAD"], app_root, 15_000)
        .ok()
        .and_then(non_empty);
    let branch = runner
        .run("git", &["rev-parse", "--abbrev-ref", "HEAD"], app_root, 15_000)
        .ok()
        .and_then(non_empty);
    let porcelain = runner
        .run("git", &["status", "--porcelain"], app_root, 15_000)
        .unwrap_or_default();
    let dirty_count = porcelain.lines().map(|l| l.trim()).filter(|l| !l.is_empty()).count();
    LocalBuildMetadata {
        version,
        has_git_repo: true,
        commit,
        branch,
        dirty_worktree: dirty_count > 0,
        dirty_reason: if dirty_count > 0 {
            Some(format!(
                "{dirty_count} modified {} in local repository",
                if dirty_count == 1 { "file" } else { "files" }
            ))
        } else {
            None
        },
    }
}

/// Real-git convenience wrapper.
pub fn read_local_build_metadata(app_root: &Path) -> LocalBuildMetadata {
    read_local_build_metadata_with(app_root, &SystemRunner)
}

/// `isCommitAncestorOfHead`: Some(true) = local contains the commit,
/// Some(false) = local is missing it, None = git could not answer.
pub fn is_commit_ancestor_of_head_with(
    app_root: &Path,
    commit_sha: &str,
    runner: &dyn GitRunner,
) -> Option<bool> {
    match runner.run(
        "git",
        &["merge-base", "--is-ancestor", commit_sha, "HEAD"],
        app_root,
        10_000,
    ) {
        Ok(_) => Some(true),
        Err(e) if e.exit_code == Some(1) => Some(false),
        Err(_) => None,
    }
}

pub fn is_commit_ancestor_of_head(app_root: &Path, commit_sha: &str) -> Option<bool> {
    is_commit_ancestor_of_head_with(app_root, commit_sha, &SystemRunner)
}

/// `resolveVoxcellDataDir` with injectable env/home (TS test parity).
pub fn resolve_voxcell_data_dir_with(
    app_root: &Path,
    env: &HashMap<String, String>,
    home: &Path,
) -> Result<PathBuf, String> {
    if let Some(explicit) = env.get("VOXCELL_DATA_DIR").map(|s| s.trim()).filter(|s| !s.is_empty()) {
        return std::path::absolute(explicit).map_err(|e| format!("resolve data dir: {e}"));
    }
    let candidate = home.join(".voxcell");
    if probe_writable(&candidate).is_ok() {
        return Ok(candidate);
    }
    let fallback = app_root.join(".voxcell-data");
    std::fs::create_dir_all(&fallback).map_err(|e| format!("create fallback data dir: {e}"))?;
    Ok(fallback)
}

fn probe_writable(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let probe = dir.join(format!(".voxcell-write-probe-{}", std::process::id()));
    std::fs::write(&probe, b"ok").map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&probe);
    Ok(())
}

/// Real-env convenience wrapper (`VOXCELL_DATA_DIR`, `$HOME` / OS home).
pub fn resolve_voxcell_data_dir(app_root: &Path) -> Result<PathBuf, String> {
    let mut env = HashMap::new();
    if let Ok(v) = std::env::var("VOXCELL_DATA_DIR") {
        env.insert("VOXCELL_DATA_DIR".to_string(), v);
    }
    let home = std::env::var("HOME")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| app_root.to_path_buf());
    resolve_voxcell_data_dir_with(app_root, &env, &home)
}

/// Lenient semver comparison — exact port of `compareSemver` in
/// settingsSnapshot.ts (strips `v`, ignores prerelease/build suffixes,
/// missing/non-numeric parts count as 0). Returns -1 / 0 / 1.
///
/// Deliberately NOT the strict `semver` crate: the TS contract treats
/// `"1.2"` == `"1.2.0"` and `"1.2.3-beta"` == `"1.2.3"`.
pub fn compare_semver(current_raw: &str, latest_raw: &str) -> i32 {
    fn parse(v: &str) -> [i64; 3] {
        let trimmed = v.trim();
        let stripped = trimmed.strip_prefix('v').or_else(|| trimmed.strip_prefix('V')).unwrap_or(trimmed);
        let core = stripped.split('-').next().unwrap_or("");
        let mut out = [0i64; 3];
        for (i, part) in core.split('.').take(3).enumerate() {
            // TS parseInt: leading integer prefix, NaN/negative -> 0.
            let digits: String = part
                .trim()
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect();
            out[i] = digits.parse::<i64>().unwrap_or(0);
        }
        out
    }
    let a = parse(current_raw);
    let b = parse(latest_raw);
    for i in 0..3 {
        if a[i] < b[i] {
            return -1;
        }
        if a[i] > b[i] {
            return 1;
        }
    }
    0
}

fn short_sha(s: &str) -> String {
    s.chars().take(7).collect()
}

fn non_empty_str(v: Option<&Value>) -> Option<String> {
    v.and_then(|x| x.as_str()).map(|s| s.to_string()).filter(|s| !s.is_empty())
}

/// `fetchUpstreamReleaseInfo` — GitHub releases/latest probe with commits/main
/// fallback. `api_base` is injectable so tests hit a local mock HTTP server.
pub async fn fetch_upstream_release_info(
    client: &reqwest::Client,
    api_base: &str,
    repo_slug: &str,
    timeout: Duration,
) -> UpstreamReleaseInfo {
    let with_headers = |req: reqwest::RequestBuilder| {
        req.header("Accept", "application/vnd.github+json")
            .header("User-Agent", "voxcell-hermes-updater")
            .timeout(timeout)
    };
    let fail = |msg: String| UpstreamReleaseInfo { error: Some(msg), ..Default::default() };

    let release_url = format!("{api_base}/repos/{repo_slug}/releases/latest");
    let rel_res = match with_headers(client.get(&release_url)).send().await {
        Ok(r) => r,
        Err(e) => return fail(e.to_string()),
    };
    if rel_res.status().is_success() {
        let rel: Value = match rel_res.json().await {
            Ok(v) => v,
            Err(e) => return fail(e.to_string()),
        };
        let tag = non_empty_str(rel.get("tag_name"));
        return UpstreamReleaseInfo {
            latest_version: tag.as_deref().map(|t| {
                t.strip_prefix('v').or_else(|| t.strip_prefix('V')).unwrap_or(t).to_string()
            }),
            latest_commit: non_empty_str(rel.get("target_commitish")).map(|c| short_sha(&c)),
            release_title: non_empty_str(rel.get("name")).or(tag),
            release_notes: rel
                .get("body")
                .and_then(|b| b.as_str())
                .filter(|b| !b.is_empty())
                .map(|b| b.chars().take(500).collect()),
            release_url: non_empty_str(rel.get("html_url")),
            error: None,
        };
    }

    let release_status = rel_res.status().as_u16();
    let commit_url = format!("{api_base}/repos/{repo_slug}/commits/main");
    let commit_res = match with_headers(client.get(&commit_url)).send().await {
        Ok(r) => r,
        Err(e) => return fail(e.to_string()),
    };
    if commit_res.status().is_success() {
        let data: Value = match commit_res.json().await {
            Ok(v) => v,
            Err(e) => return fail(e.to_string()),
        };
        let first_line = data
            .pointer("/commit/message")
            .and_then(|m| m.as_str())
            .and_then(|m| m.split('\n').next())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        return UpstreamReleaseInfo {
            latest_version: None,
            latest_commit: non_empty_str(data.get("sha")).map(|s| short_sha(&s)),
            release_title: first_line,
            release_url: non_empty_str(data.get("html_url")),
            ..Default::default()
        };
    }
    fail(format!("Upstream check returned HTTP {release_status}"))
}

/// Convenience probe against the real GitHub API (never used in tests).
pub async fn fetch_upstream_release_info_default() -> UpstreamReleaseInfo {
    let client = match reqwest::Client::builder().build() {
        Ok(c) => c,
        Err(e) => {
            return UpstreamReleaseInfo { error: Some(e.to_string()), ..Default::default() };
        }
    };
    fetch_upstream_release_info(
        &client,
        GITHUB_API_BASE,
        DEFAULT_REPO_SLUG,
        Duration::from_millis(UPSTREAM_TIMEOUT_MS),
    )
    .await
}

/// `buildAppUpdateStatus` — exact TS logic, including the ancestry-aware
/// commit-divergence heuristic (local AHEAD of upstream is NOT an update).
pub fn build_app_update_status(
    local: &LocalBuildMetadata,
    upstream: Option<&UpstreamReleaseInfo>,
    receipt: Option<UpdateReceipt>,
    checked_at: Option<i64>,
    upstream_ancestor_of_local: Option<bool>,
) -> AppUpdateStatus {
    let latest_version = upstream.and_then(|u| u.latest_version.clone());
    let latest_commit = upstream.and_then(|u| u.latest_commit.clone());

    let mut update_available = false;
    let version_behind = latest_version
        .as_deref()
        .map(|lv| compare_semver(&local.version, lv) < 0)
        .unwrap_or(false);
    if version_behind {
        update_available = true;
    } else if local.has_git_repo {
        if let (Some(local_commit), Some(upstream_commit)) = (&local.commit, &latest_commit) {
            if !upstream_commit.starts_with(local_commit.as_str())
                && !local_commit.starts_with(upstream_commit.as_str())
            {
                // Local may simply be ahead of upstream (unpushed commits).
                // Only flag when ancestry proves local is missing upstream
                // commits, or when ancestry is unknown.
                update_available = upstream_ancestor_of_local != Some(true);
            }
        }
    }

    AppUpdateStatus {
        current_version: local.version.clone(),
        current_commit: local.commit.clone(),
        branch: local.branch.clone(),
        latest_version,
        latest_commit,
        update_available,
        update_mode: if local.has_git_repo { UpdateMode::GitPull } else { UpdateMode::ManagedRefresh },
        can_self_update: !local.has_git_repo || !local.dirty_worktree,
        dirty_worktree: local.dirty_worktree,
        dirty_reason: local.dirty_reason.clone(),
        release_title: upstream.and_then(|u| u.release_title.clone()),
        release_notes: upstream.and_then(|u| u.release_notes.clone()),
        release_url: upstream.and_then(|u| u.release_url.clone()),
        checked_at: checked_at.unwrap_or_else(now_ms),
        upstream_error: upstream.and_then(|u| u.error.clone()),
        pending_receipt: receipt,
    }
}

/// `applyGitPullUpdate` — fetch + fast-forward pull + optional dependency
/// sync/rebuild for source/dev installs (injectable runner).
pub fn apply_git_pull_update_with(app_root: &Path, runner: &dyn GitRunner) -> GitPullOutcome {
    let mut steps = Vec::new();
    let before_commit = runner
        .run("git", &["rev-parse", "--short", "HEAD"], app_root, 15_000)
        .unwrap_or_default();
    let fail = |steps: Vec<UpdateStepResult>, error: String, commit: &str| GitPullOutcome {
        ok: false,
        steps,
        new_version: read_package_version(app_root),
        new_commit: non_empty(commit.to_string()),
        error: Some(error),
    };

    match runner.run("git", &["fetch", "--tags", "--prune"], app_root, 30_000) {
        Ok(_) => steps.push(UpdateStepResult::ok("git-fetch", "Fetched upstream repository")),
        Err(e) => {
            steps.push(UpdateStepResult::failed("git-fetch", "Fetch upstream repository", &e.message));
            return fail(steps, format!("Unable to fetch updates: {}", e.message), &before_commit);
        }
    }

    let pull_out = match runner.run("git", &["pull", "--ff-only"], app_root, 45_000) {
        Ok(out) => out,
        Err(e) => {
            steps.push(UpdateStepResult::failed(
                "git-pull",
                "Fast-forward repository update",
                &e.message,
            ));
            return fail(
                steps,
                format!("Repository fast-forward blocked: {}", e.message),
                &before_commit,
            );
        }
    };
    steps.push(UpdateStepResult::ok_detail(
        "git-pull",
        "Updated source code (fast-forward)",
        pull_out.lines().next().filter(|l| !l.is_empty()).unwrap_or("Up to date"),
    ));

    let after_commit = runner
        .run("git", &["rev-parse", "--short", "HEAD"], app_root, 15_000)
        .unwrap_or_else(|_| before_commit.clone());

    if !after_commit.is_empty() && !before_commit.is_empty() && after_commit != before_commit {
        let changed = runner.run(
            "git",
            &["diff", "--name-only", &before_commit, &after_commit],
            app_root,
            15_000,
        );
        let rebuild = |steps: &mut Vec<UpdateStepResult>| -> Result<(), String> {
            let files = changed.map_err(|e| e.message)?;
            let deps_changed = files.lines().any(|l| {
                matches!(l.trim(), "package.json" | "bun.lock" | "package-lock.json")
            });
            if deps_changed {
                let use_bun = app_root.join("bun.lock").exists();
                runner
                    .run(if use_bun { "bun" } else { "npm" }, &["install"], app_root, 120_000)
                    .map_err(|e| e.message)?;
                steps.push(UpdateStepResult::ok("deps", "Synced updated dependencies"));
            }
            runner
                .run("npm", &["run", "build"], app_root, 120_000)
                .map_err(|e| e.message)?;
            steps.push(UpdateStepResult::ok("build", "Rebuilt production assets"));
            Ok(())
        };
        if let Err(msg) = rebuild(&mut steps) {
            steps.push(UpdateStepResult::failed("build", "Build after update", &msg));
            return fail(
                steps,
                format!("Source updated, but rebuild failed: {msg}"),
                &after_commit,
            );
        }
    } else {
        steps.push(UpdateStepResult::skipped(
            "build",
            "Already at latest commit — preferences verified",
        ));
    }

    GitPullOutcome {
        ok: true,
        steps,
        new_version: read_package_version(app_root),
        new_commit: non_empty(after_commit),
        error: None,
    }
}

pub fn apply_git_pull_update(app_root: &Path) -> GitPullOutcome {
    apply_git_pull_update_with(app_root, &SystemRunner)
}

fn fallback_categories() -> Vec<String> {
    vec![
        "Theme & Colors".to_string(),
        "Layout & Typography".to_string(),
        "Stage, Voice & Trace".to_string(),
    ]
}

fn effective_preserved_categories(vault: &PreferencesVault, snapshot: &Value) -> Vec<String> {
    let customized: Vec<String> = vault
        .preserved_categories(snapshot)
        .into_iter()
        .filter(|c| c.customized)
        .map(|c| c.label)
        .collect();
    if customized.is_empty() { fallback_categories() } else { customized }
}

fn failure_outcome(
    status: AppUpdateStatus,
    steps: Vec<UpdateStepResult>,
    error: impl Into<String>,
) -> ApplyUpdateOutcome {
    ApplyUpdateOutcome {
        ok: false,
        status,
        receipt: None,
        steps,
        needs_reload: false,
        restarting: false,
        error: Some(error.into()),
    }
}

/// Port of `POST /api/app/update/apply` (server.ts) /
/// `applyAppUpdateWithSnapshot` (settingsVaultClient.ts):
/// pre-update backup hook → dirty gate → git pull or managed refresh →
/// receipt write. `upstream` may be pre-fetched (async probe) or `None`.
pub fn apply_update_with_snapshot(
    vault: &PreferencesVault,
    app_root: &Path,
    raw_snapshot: Option<&Value>,
    upstream: Option<&UpstreamReleaseInfo>,
    upstream_ancestor_of_local: Option<bool>,
    runner: &dyn GitRunner,
) -> ApplyUpdateOutcome {
    let snapshot = raw_snapshot
        .and_then(|raw| vault.normalize_snapshot(raw))
        .or_else(|| vault.read_vault());
    let Some(snapshot) = snapshot else {
        let local = read_local_build_metadata_with(app_root, runner);
        let status = build_app_update_status(&local, upstream, vault.read_receipt(), None, upstream_ancestor_of_local);
        return failure_outcome(status, Vec::new(), "Unable to snapshot user preferences before update.");
    };

    // 1. Lock user themes, custom backdrops, and settings into a restore point.
    let mut steps = Vec::new();
    let backup = match vault.create_backup(&snapshot, BackupReason::PreUpdate) {
        Ok(outcome) => outcome.backup,
        Err(e) => {
            let local = read_local_build_metadata_with(app_root, runner);
            let status = build_app_update_status(&local, upstream, vault.read_receipt(), None, upstream_ancestor_of_local);
            return failure_outcome(status, steps, format!("Unable to snapshot user preferences before update: {e}"));
        }
    };
    steps.push(UpdateStepResult::ok(
        "vault-backup",
        format!("Locked themes & settings ({})", backup.id),
    ));

    let local = read_local_build_metadata_with(app_root, runner);
    let effective_categories = effective_preserved_categories(vault, &snapshot);

    if local.has_git_repo {
        if local.dirty_worktree {
            steps.push(UpdateStepResult::failed(
                "git-check",
                "Checked repository cleanliness",
                local.dirty_reason.clone().unwrap_or_else(|| "Local uncommitted modifications detected".into()),
            ));
            let status = build_app_update_status(&local, upstream, vault.read_receipt(), None, upstream_ancestor_of_local);
            return failure_outcome(
                status,
                steps,
                "Pre-update snapshot saved safely, but git update was paused because local files have uncommitted edits.",
            );
        }

        let git_outcome = apply_git_pull_update_with(app_root, runner);
        steps.extend(git_outcome.steps);
        if !git_outcome.ok {
            let status = build_app_update_status(&local, upstream, vault.read_receipt(), None, upstream_ancestor_of_local);
            return failure_outcome(
                status,
                steps,
                git_outcome.error.unwrap_or_else(|| "Update failed (your themes & settings backup is intact).".into()),
            );
        }

        let receipt = UpdateReceipt {
            updated_at: now_ms(),
            previous_version: local.version.clone(),
            new_version: git_outcome.new_version,
            previous_commit: local.commit.clone(),
            new_commit: git_outcome.new_commit,
            backup_id: backup.id,
            mode: UpdateMode::GitPull.as_str().to_string(),
            preserved_categories: effective_categories,
        };
        if let Err(e) = vault.write_receipt(&receipt) {
            let status = build_app_update_status(&local, upstream, None, None, upstream_ancestor_of_local);
            return failure_outcome(status, steps, format!("Update applied but receipt write failed: {e}"));
        }
        let local_after = read_local_build_metadata_with(app_root, runner);
        let status = build_app_update_status(&local_after, upstream, Some(receipt.clone()), None, upstream_ancestor_of_local);
        return ApplyUpdateOutcome {
            ok: true,
            status,
            receipt: Some(receipt),
            steps,
            needs_reload: true,
            restarting: false,
            error: None,
        };
    }

    // Managed runtime / non-git environment: lock vault, write receipt.
    steps.push(UpdateStepResult::ok_detail(
        "vault-verify",
        "Verified durable preferences vault on disk",
        format!("{} setting groups protected", effective_categories.len()),
    ));
    let receipt = UpdateReceipt {
        updated_at: now_ms(),
        previous_version: local.version.clone(),
        new_version: local.version.clone(),
        previous_commit: None,
        new_commit: None,
        backup_id: backup.id,
        mode: UpdateMode::ManagedRefresh.as_str().to_string(),
        preserved_categories: effective_categories,
    };
    if let Err(e) = vault.write_receipt(&receipt) {
        let status = build_app_update_status(&local, upstream, None, None, upstream_ancestor_of_local);
        return failure_outcome(status, steps, format!("Receipt write failed: {e}"));
    }
    steps.push(UpdateStepResult::ok(
        "runtime-sync",
        "Synchronized update receipt for post-reload restore",
    ));
    let status = build_app_update_status(&local, upstream, Some(receipt.clone()), None, upstream_ancestor_of_local);
    ApplyUpdateOutcome {
        ok: true,
        status,
        receipt: Some(receipt),
        steps,
        needs_reload: true,
        restarting: false,
        error: None,
    }
}

/// Convenience wrapper with the real system runner and no upstream probe.
pub fn apply_update(
    vault: &PreferencesVault,
    app_root: &Path,
    raw_snapshot: Option<&Value>,
) -> ApplyUpdateOutcome {
    apply_update_with_snapshot(vault, app_root, raw_snapshot, None, None, &SystemRunner)
}

/// `acknowledgeUpdateReceipt` — clear the pending receipt after display.
pub fn acknowledge_update_receipt(vault: &PreferencesVault) {
    vault.clear_receipt();
}

// ---------------------------------------------------------------------------
// Desktop extension: SHA-256 asset verification + atomic binary swap
// ---------------------------------------------------------------------------

/// Lowercase hex SHA-256 of a byte buffer.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// Verify a desktop binary asset (`.dmg` / `.msi` / `.AppImage`) against its
/// expected SHA-256 before any swap. Accepts uppercase and `sha256:` prefixes.
pub fn verify_asset_sha256(bytes: &[u8], expected_hex: &str) -> Result<(), String> {
    let mut expected = expected_hex.trim().to_ascii_lowercase();
    if let Some(rest) = expected.strip_prefix("sha256:") {
        expected = rest.to_string();
    }
    if expected.len() != 64 || !expected.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("invalid expected SHA-256 digest: {expected_hex:?}"));
    }
    let actual = sha256_hex(bytes);
    if actual != expected {
        return Err(format!("SHA-256 mismatch: expected {expected}, got {actual}"));
    }
    Ok(())
}

/// File-based variant of [`verify_asset_sha256`].
pub fn verify_asset_file_sha256(path: &Path, expected_hex: &str) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read asset {}: {e}", path.display()))?;
    verify_asset_sha256(&bytes, expected_hex)
}

/// Atomically replace `dest` with `bytes` (tmp sibling + rename). The target
/// must not be locked by a running process; stage the swap for next launch
/// when updating the running executable itself.
pub fn atomic_replace_file(dest: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create dir: {e}"))?;
    }
    let file_name = dest
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "binary".into());
    let tmp = dest.with_file_name(format!("{file_name}.{}.{}.tmp", std::process::id(), now_ms()));
    std::fs::write(&tmp, bytes).map_err(|e| format!("write staged binary: {e}"))?;
    std::fs::rename(&tmp, dest).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("atomic binary swap: {e}")
    })
}

/// Desktop binary update flow: pre-update vault backup → SHA-256 verification
/// (abort **before** touching the target on mismatch) → atomic swap → receipt.
pub fn apply_binary_update(
    vault: &PreferencesVault,
    app_root: &Path,
    target_binary: &Path,
    asset_path: &Path,
    expected_sha256: &str,
    raw_snapshot: Option<&Value>,
) -> ApplyUpdateOutcome {
    let local = read_local_build_metadata(app_root);
    let snapshot = raw_snapshot
        .and_then(|raw| vault.normalize_snapshot(raw))
        .or_else(|| vault.read_vault());
    let Some(snapshot) = snapshot else {
        let status = build_app_update_status(&local, None, vault.read_receipt(), None, None);
        return failure_outcome(status, Vec::new(), "Unable to snapshot user preferences before update.");
    };

    let mut steps = Vec::new();
    let backup = match vault.create_backup(&snapshot, BackupReason::PreUpdate) {
        Ok(outcome) => outcome.backup,
        Err(e) => {
            let status = build_app_update_status(&local, None, vault.read_receipt(), None, None);
            return failure_outcome(status, steps, format!("Unable to snapshot user preferences before update: {e}"));
        }
    };
    steps.push(UpdateStepResult::ok(
        "vault-backup",
        format!("Locked themes & settings ({})", backup.id),
    ));

    let bytes = match std::fs::read(asset_path) {
        Ok(b) => b,
        Err(e) => {
            let msg = format!("read asset {}: {e}", asset_path.display());
            steps.push(UpdateStepResult::failed("asset-verify", "Verify desktop binary checksum", &msg));
            let status = build_app_update_status(&local, None, vault.read_receipt(), None, None);
            return failure_outcome(status, steps, msg);
        }
    };
    if let Err(e) = verify_asset_sha256(&bytes, expected_sha256) {
        steps.push(UpdateStepResult::failed("asset-verify", "Verify desktop binary checksum", &e));
        let status = build_app_update_status(&local, None, vault.read_receipt(), None, None);
        return failure_outcome(status, steps, format!("Update aborted before swap: {e}"));
    }
    steps.push(UpdateStepResult::ok_detail(
        "asset-verify",
        "Verify desktop binary checksum",
        format!("SHA-256 matched {}", sha256_hex(&bytes)),
    ));

    if let Err(e) = atomic_replace_file(target_binary, &bytes) {
        steps.push(UpdateStepResult::failed("binary-swap", "Atomically swap desktop binary", &e));
        let status = build_app_update_status(&local, None, vault.read_receipt(), None, None);
        return failure_outcome(status, steps, e);
    }
    steps.push(UpdateStepResult::ok("binary-swap", "Atomically swapped desktop binary"));

    let version = read_package_version(app_root);
    let receipt = UpdateReceipt {
        updated_at: now_ms(),
        previous_version: version.clone(),
        new_version: version,
        previous_commit: None,
        new_commit: None,
        backup_id: backup.id,
        mode: UpdateMode::ManagedRefresh.as_str().to_string(),
        preserved_categories: effective_preserved_categories(vault, &snapshot),
    };
    if let Err(e) = vault.write_receipt(&receipt) {
        let status = build_app_update_status(&local, None, None, None, None);
        return failure_outcome(status, steps, format!("Binary swapped but receipt write failed: {e}"));
    }
    steps.push(UpdateStepResult::ok(
        "runtime-sync",
        "Synchronized update receipt for post-reload restore",
    ));
    let status = build_app_update_status(&local, None, Some(receipt.clone()), None, None);
    ApplyUpdateOutcome {
        ok: true,
        status,
        receipt: Some(receipt),
        steps,
        needs_reload: true,
        restarting: false,
        error: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::{PreservedCategorySummary, SnapshotAdapter, WebJsonAdapter, now_ms};
    use serde_json::json;
    use std::sync::Mutex;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "voxcell-updater-test-{}-{tag}-{}",
            std::process::id(),
            now_ms()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    // -- scripted command runner -------------------------------------------

    #[derive(Clone)]
    struct FakeRunner {
        responses: HashMap<String, Result<String, RunError>>,
        calls: std::sync::Arc<Mutex<Vec<String>>>,
    }

    impl FakeRunner {
        fn new() -> Self {
            Self { responses: HashMap::new(), calls: std::sync::Arc::new(Mutex::new(Vec::new())) }
        }
        fn on(&mut self, program: &str, args: &[&str], result: Result<&str, RunError>) -> &mut Self {
            self.responses.insert(format!("{program} {}", args.join(" ")), result.map(|s| s.to_string()));
            self
        }
        fn ok(&mut self, program: &str, args: &[&str], out: &str) -> &mut Self {
            self.on(program, args, Ok(out))
        }
        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl GitRunner for FakeRunner {
        fn run(&self, program: &str, args: &[&str], _cwd: &Path, _timeout_ms: u64) -> Result<String, RunError> {
            let key = format!("{program} {}", args.join(" "));
            self.calls.lock().unwrap().push(key.clone());
            self.responses.get(&key).cloned().unwrap_or(Err(RunError {
                message: format!("unexpected command: {key}"),
                exit_code: None,
            }))
        }
    }

    fn err(msg: &str, code: Option<i32>) -> RunError {
        RunError { message: msg.to_string(), exit_code: code }
    }

    fn clean_git(runner: &mut FakeRunner, commit: &str) {
        runner
            .ok("git", &["rev-parse", "--short", "HEAD"], commit)
            .ok("git", &["rev-parse", "--abbrev-ref", "HEAD"], "main")
            .ok("git", &["status", "--porcelain"], "");
    }

    // -- compareSemver ------------------------------------------------------

    #[test]
    fn compare_semver_matches_typescript_leniency() {
        assert_eq!(compare_semver("0.1.0", "0.2.0"), -1);
        assert_eq!(compare_semver("0.2.0", "0.1.0"), 1);
        assert_eq!(compare_semver("0.1.0", "0.1.0"), 0);
        assert_eq!(compare_semver("v1.2.3", "1.2.3"), 0);
        assert_eq!(compare_semver("V2.0.0", "v1.9.9"), 1);
        assert_eq!(compare_semver("1.2", "1.2.0"), 0);
        assert_eq!(compare_semver("1.2.3-beta.1", "1.2.3"), 0);
        assert_eq!(compare_semver("1.2.3+build5", "1.2.3"), 0);
        assert_eq!(compare_semver("abc", "0.0.0"), 0);
        assert_eq!(compare_semver("1.2.3.4", "1.2.3"), 0);
        assert_eq!(compare_semver("0.1.0", "0.1.1"), -1);
        assert_eq!(compare_semver("1.0.0", "0.99.99"), 1);
        assert_eq!(compare_semver(" 1.2.3 ", "1.2.3"), 0);
    }

    // -- buildAppUpdateStatus (ported TS expectations) ----------------------

    fn local_meta(version: &str, git: bool, commit: Option<&str>, dirty: bool) -> LocalBuildMetadata {
        LocalBuildMetadata {
            version: version.into(),
            has_git_repo: git,
            commit: commit.map(|s| s.into()),
            branch: if git { Some("main".into()) } else { None },
            dirty_worktree: dirty,
            dirty_reason: if dirty { Some("2 modified files in local repository".into()) } else { None },
        }
    }

    #[test]
    fn status_detects_version_updates_commit_divergence_and_dirty_worktrees() {
        let up_to_date = build_app_update_status(
            &local_meta("0.1.0", false, None, false),
            Some(&UpstreamReleaseInfo { latest_version: Some("0.1.0".into()), latest_commit: None, ..Default::default() }),
            None,
            Some(1000),
            None,
        );
        assert!(!up_to_date.update_available);
        assert_eq!(up_to_date.update_mode, UpdateMode::ManagedRefresh);
        assert!(up_to_date.can_self_update);
        assert_eq!(up_to_date.checked_at, 1000);

        let newer_release = build_app_update_status(
            &local_meta("0.1.0", true, Some("111aaaa"), false),
            Some(&UpstreamReleaseInfo {
                latest_version: Some("0.2.0".into()),
                latest_commit: Some("222bbbb".into()),
                release_title: Some("v0.2.0".into()),
                ..Default::default()
            }),
            None,
            Some(2000),
            None,
        );
        assert!(newer_release.update_available);
        assert_eq!(newer_release.update_mode, UpdateMode::GitPull);
        assert!(newer_release.can_self_update);
        assert_eq!(newer_release.release_title.as_deref(), Some("v0.2.0"));

        let dirty_repo = build_app_update_status(
            &local_meta("0.1.0", true, Some("111aaaa"), true),
            Some(&UpstreamReleaseInfo { latest_version: Some("0.2.0".into()), latest_commit: Some("222bbbb".into()), ..Default::default() }),
            None,
            Some(3000),
            None,
        );
        assert!(!dirty_repo.can_self_update);
        assert!(dirty_repo.dirty_worktree);
        assert_eq!(dirty_repo.dirty_reason.as_deref(), Some("2 modified files in local repository"));
    }

    #[test]
    fn status_does_not_flag_update_when_local_is_ahead_of_upstream() {
        let local = local_meta("0.1.0", true, Some("b58f223"), false);
        let ahead = build_app_update_status(
            &local,
            Some(&UpstreamReleaseInfo { latest_version: None, latest_commit: Some("fd4ae0a".into()), ..Default::default() }),
            None,
            None,
            Some(true),
        );
        assert!(!ahead.update_available);

        let behind = build_app_update_status(
            &local,
            Some(&UpstreamReleaseInfo { latest_version: None, latest_commit: Some("9998887".into()), ..Default::default() }),
            None,
            None,
            Some(false),
        );
        assert!(behind.update_available);

        let unknown = build_app_update_status(
            &local,
            Some(&UpstreamReleaseInfo { latest_version: None, latest_commit: Some("9998887".into()), ..Default::default() }),
            None,
            None,
            None,
        );
        assert!(unknown.update_available);

        // Matching short-sha prefixes are never divergence.
        let same = build_app_update_status(
            &local,
            Some(&UpstreamReleaseInfo { latest_version: None, latest_commit: Some("b58f2239999".into()), ..Default::default() }),
            None,
            None,
            None,
        );
        assert!(!same.update_available);
    }

    // -- data dir / package version -----------------------------------------

    #[test]
    fn data_dir_honors_voxcell_data_dir_override() {
        let dir = temp_dir("env-override");
        let mut env = HashMap::new();
        env.insert("VOXCELL_DATA_DIR".to_string(), dir.to_string_lossy().to_string());
        let resolved = resolve_voxcell_data_dir_with(Path::new("/app/applet"), &env, Path::new("/home/tester")).unwrap();
        assert_eq!(resolved, std::path::absolute(&dir).unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn data_dir_falls_back_to_home_then_app_root() {
        let base = temp_dir("data-dir");
        let home = base.join("home");
        std::fs::create_dir_all(&home).unwrap();
        let app_root = base.join("app");
        std::fs::create_dir_all(&app_root).unwrap();
        let resolved = resolve_voxcell_data_dir_with(&app_root, &HashMap::new(), &home).unwrap();
        assert_eq!(resolved, home.join(".voxcell"));
        assert!(resolved.is_dir());

        // Unwritable home (a file, not a dir) -> app-root fallback.
        let bad_home = base.join("not-a-dir");
        std::fs::write(&bad_home, b"block").unwrap();
        let resolved = resolve_voxcell_data_dir_with(&app_root, &HashMap::new(), &bad_home).unwrap();
        assert_eq!(resolved, app_root.join(".voxcell-data"));
        assert!(resolved.is_dir());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn package_version_reads_trims_and_falls_back() {
        let dir = temp_dir("pkg-version");
        assert_eq!(read_package_version(&dir), "0.1.0");
        std::fs::write(dir.join("package.json"), json!({"name":"test","version":"0.4.2"}).to_string()).unwrap();
        assert_eq!(read_package_version(&dir), "0.4.2");
        std::fs::write(dir.join("package.json"), json!({"version":"  "}).to_string()).unwrap();
        assert_eq!(read_package_version(&dir), "0.1.0");
        std::fs::write(dir.join("package.json"), "{broken").unwrap();
        assert_eq!(read_package_version(&dir), "0.1.0");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn local_metadata_handles_non_git_directories_gracefully() {
        let dir = temp_dir("non-git");
        std::fs::write(dir.join("package.json"), json!({"name":"test","version":"0.4.2"}).to_string()).unwrap();
        let meta = read_local_build_metadata(&dir);
        assert_eq!(meta.version, "0.4.2");
        assert!(!meta.has_git_repo);
        assert!(!meta.dirty_worktree);
        assert!(meta.commit.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn local_metadata_counts_dirty_files_with_singular_plural_reason() {
        let dir = temp_dir("dirty-meta");
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        let mut runner = FakeRunner::new();
        clean_git(&mut runner, "abc1234");
        runner.ok("git", &["status", "--porcelain"], " M src/a.ts\n M src/b.ts\n");
        let meta = read_local_build_metadata_with(&dir, &runner);
        assert!(meta.has_git_repo);
        assert_eq!(meta.commit.as_deref(), Some("abc1234"));
        assert_eq!(meta.branch.as_deref(), Some("main"));
        assert!(meta.dirty_worktree);
        assert_eq!(meta.dirty_reason.as_deref(), Some("2 modified files in local repository"));

        runner.ok("git", &["status", "--porcelain"], " M src/a.ts\n");
        let meta = read_local_build_metadata_with(&dir, &runner);
        assert_eq!(meta.dirty_reason.as_deref(), Some("1 modified file in local repository"));

        // Git failures degrade to unknown commit/branch, clean worktree.
        let mut failing = FakeRunner::new();
        failing
            .on("git", &["rev-parse", "--short", "HEAD"], Err(err("git broken", Some(128))))
            .on("git", &["rev-parse", "--abbrev-ref", "HEAD"], Err(err("git broken", Some(128))))
            .on("git", &["status", "--porcelain"], Err(err("git broken", Some(128))));
        let meta = read_local_build_metadata_with(&dir, &failing);
        assert!(meta.has_git_repo);
        assert!(meta.commit.is_none());
        assert!(meta.branch.is_none());
        assert!(!meta.dirty_worktree);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ancestry_check_maps_exit_codes_to_tri_state() {
        let dir = temp_dir("ancestry");
        let mut runner = FakeRunner::new();
        runner.ok("git", &["merge-base", "--is-ancestor", "aaa", "HEAD"], "");
        assert_eq!(is_commit_ancestor_of_head_with(&dir, "aaa", &runner), Some(true));
        runner.on("git", &["merge-base", "--is-ancestor", "bbb", "HEAD"], Err(err("not ancestor", Some(1))));
        assert_eq!(is_commit_ancestor_of_head_with(&dir, "bbb", &runner), Some(false));
        runner.on("git", &["merge-base", "--is-ancestor", "ccc", "HEAD"], Err(err("unknown object", Some(128))));
        assert_eq!(is_commit_ancestor_of_head_with(&dir, "ccc", &runner), None);
        runner.on("git", &["merge-base", "--is-ancestor", "ddd", "HEAD"], Err(err("spawn failed", None)));
        assert_eq!(is_commit_ancestor_of_head_with(&dir, "ddd", &runner), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // -- applyGitPullUpdate --------------------------------------------------

    #[test]
    fn git_pull_unchanged_commit_skips_rebuild() {
        let dir = temp_dir("pull-unchanged");
        std::fs::write(dir.join("package.json"), json!({"version":"0.2.0"}).to_string()).unwrap();
        let mut runner = FakeRunner::new();
        runner
            .ok("git", &["rev-parse", "--short", "HEAD"], "aaa1111")
            .ok("git", &["fetch", "--tags", "--prune"], "")
            .ok("git", &["pull", "--ff-only"], "Already up to date.");
        let outcome = apply_git_pull_update_with(&dir, &runner);
        assert!(outcome.ok);
        assert_eq!(outcome.new_commit.as_deref(), Some("aaa1111"));
        assert_eq!(outcome.new_version, "0.2.0");
        let ids: Vec<(&str, StepStatus)> = outcome.steps.iter().map(|s| (s.id.as_str(), s.status)).collect();
        assert_eq!(
            ids,
            vec![
                ("git-fetch", StepStatus::Ok),
                ("git-pull", StepStatus::Ok),
                ("build", StepStatus::Skipped),
            ]
        );
        assert_eq!(outcome.steps[1].detail.as_deref(), Some("Already up to date."));
        // No npm/bun calls when the commit did not move.
        assert!(!runner.calls().iter().any(|c| c.starts_with("npm") || c.starts_with("bun")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Runner whose HEAD answer advances after `git pull` runs.
    struct AdvancingRunner {
        inner: FakeRunner,
        pulled: std::sync::Arc<Mutex<bool>>,
        before: String,
        after: String,
    }

    impl GitRunner for AdvancingRunner {
        fn run(&self, program: &str, args: &[&str], cwd: &Path, timeout_ms: u64) -> Result<String, RunError> {
            if program == "git" && args == ["pull", "--ff-only"].as_slice() {
                *self.pulled.lock().unwrap() = true;
            }
            if program == "git" && args == ["rev-parse", "--short", "HEAD"].as_slice() {
                let pulled = *self.pulled.lock().unwrap();
                self.inner.calls.lock().unwrap().push("git rev-parse --short HEAD".into());
                return Ok(if pulled { self.after.clone() } else { self.before.clone() });
            }
            self.inner.run(program, args, cwd, timeout_ms)
        }
    }

    #[test]
    fn git_pull_changed_commit_runs_deps_and_build_steps() {
        let dir = temp_dir("pull-changed");
        std::fs::write(dir.join("package.json"), json!({"version":"0.2.0"}).to_string()).unwrap();
        let mut inner = FakeRunner::new();
        inner
            .ok("git", &["fetch", "--tags", "--prune"], "")
            .ok("git", &["pull", "--ff-only"], "Updating aaa1111..bbb2222\nFast-forward")
            .ok("git", &["diff", "--name-only", "aaa1111", "bbb2222"], "README.md\npackage.json\nbun.lock\n")
            .ok("bun", &["install"], "installed")
            .ok("npm", &["run", "build"], "built");
        std::fs::write(dir.join("bun.lock"), "{}").unwrap();
        let runner = AdvancingRunner {
            inner,
            pulled: std::sync::Arc::new(Mutex::new(false)),
            before: "aaa1111".into(),
            after: "bbb2222".into(),
        };
        let outcome = apply_git_pull_update_with(&dir, &runner);
        assert!(outcome.ok);
        assert_eq!(outcome.new_version, "0.2.0");
        assert_eq!(outcome.new_commit.as_deref(), Some("bbb2222"));
        let ids: Vec<(&str, StepStatus)> = outcome.steps.iter().map(|s| (s.id.as_str(), s.status)).collect();
        assert_eq!(
            ids,
            vec![
                ("git-fetch", StepStatus::Ok),
                ("git-pull", StepStatus::Ok),
                ("deps", StepStatus::Ok),
                ("build", StepStatus::Ok),
            ]
        );
        assert!(runner.inner.calls().contains(&"bun install".to_string()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn git_pull_fetch_failure_aborts_with_receipt_safe_error() {
        let dir = temp_dir("pull-fetch-fail");
        let mut runner = FakeRunner::new();
        runner
            .ok("git", &["rev-parse", "--short", "HEAD"], "aaa1111")
            .on("git", &["fetch", "--tags", "--prune"], Err(err("network down", None)));
        let outcome = apply_git_pull_update_with(&dir, &runner);
        assert!(!outcome.ok);
        assert_eq!(outcome.steps.len(), 1);
        assert_eq!(outcome.steps[0].id, "git-fetch");
        assert_eq!(outcome.steps[0].status, StepStatus::Failed);
        assert!(outcome.error.unwrap().starts_with("Unable to fetch updates:"));
        assert_eq!(outcome.new_commit.as_deref(), Some("aaa1111"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn git_pull_non_ff_failure_reports_blocked_fast_forward() {
        let dir = temp_dir("pull-ff-fail");
        let mut runner = FakeRunner::new();
        runner
            .ok("git", &["rev-parse", "--short", "HEAD"], "aaa1111")
            .ok("git", &["fetch", "--tags", "--prune"], "")
            .on("git", &["pull", "--ff-only"], Err(err("divergent branches", Some(128))));
        let outcome = apply_git_pull_update_with(&dir, &runner);
        assert!(!outcome.ok);
        assert!(outcome.error.unwrap().starts_with("Repository fast-forward blocked:"));
        assert!(outcome.steps.iter().any(|s| s.id == "git-pull" && s.status == StepStatus::Failed));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn git_pull_build_failure_keeps_source_updated_error() {
        let dir = temp_dir("pull-build-fail");
        std::fs::write(dir.join("package.json"), json!({"version":"0.2.0"}).to_string()).unwrap();
        let mut inner = FakeRunner::new();
        inner
            .ok("git", &["fetch", "--tags", "--prune"], "")
            .ok("git", &["pull", "--ff-only"], "Updating\nFast-forward")
            .ok("git", &["diff", "--name-only", "aaa1111", "bbb2222"], "README.md\n")
            .on("npm", &["run", "build"], Err(err("compile error", Some(1))));
        let runner = AdvancingRunner {
            inner,
            pulled: std::sync::Arc::new(Mutex::new(false)),
            before: "aaa1111".into(),
            after: "bbb2222".into(),
        };
        let outcome = apply_git_pull_update_with(&dir, &runner);
        assert!(!outcome.ok);
        assert!(outcome.error.unwrap().starts_with("Source updated, but rebuild failed:"));
        assert_eq!(outcome.new_commit.as_deref(), Some("bbb2222"));
        assert!(outcome.steps.iter().any(|s| s.id == "build" && s.status == StepStatus::Failed));
        let _ = std::fs::remove_dir_all(&dir);
    }

    // -- real git (local temp repos only, no network) ------------------------

    fn real_git(cwd: &Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .args([
                "-c", "user.email=voxcell-test@example.com",
                "-c", "user.name=Voxcell Test",
                "-c", "commit.gpgsign=false",
                "-c", "core.autocrlf=false",
            ])
            .args(args)
            .current_dir(cwd)
            .output()
            .expect("git must be available for local-repo tests");
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    #[test]
    fn real_git_metadata_dirty_check_and_fast_forward_pull() {
        let base = temp_dir("real-git");
        let upstream = base.join("upstream");
        std::fs::create_dir_all(&upstream).unwrap();
        real_git(&upstream, &["init", "-q"]);
        std::fs::write(upstream.join("package.json"), json!({"version":"0.1.0"}).to_string()).unwrap();
        std::fs::write(upstream.join("README.md"), "hello\n").unwrap();
        real_git(&upstream, &["add", "."]);
        real_git(&upstream, &["commit", "-q", "-m", "initial"]);

        let clone = base.join("clone");
        real_git(&base, &["clone", "-q", upstream.to_str().unwrap(), clone.to_str().unwrap()]);

        let meta = read_local_build_metadata(&clone);
        assert!(meta.has_git_repo);
        assert!(meta.commit.is_some());
        assert!(meta.branch.is_some());
        assert!(!meta.dirty_worktree);
        assert_eq!(meta.version, "0.1.0");

        // Dirty worktree detection (1 modified file).
        std::fs::write(clone.join("README.md"), "hello dirty\n").unwrap();
        let dirty = read_local_build_metadata(&clone);
        assert!(dirty.dirty_worktree);
        assert_eq!(dirty.dirty_reason.as_deref(), Some("1 modified file in local repository"));
        std::fs::write(clone.join("README.md"), "hello\n").unwrap();
        assert!(!read_local_build_metadata(&clone).dirty_worktree);

        // Ancestry: HEAD contains itself; unknown object -> None.
        let head_full = real_git(&clone, &["rev-parse", "HEAD"]);
        assert_eq!(is_commit_ancestor_of_head(&clone, &head_full), Some(true));
        assert_eq!(is_commit_ancestor_of_head(&clone, "0123456789abcdef0123456789abcdef01234567"), None);

        // Up-to-date fast-forward apply: fetch + pull ok, build skipped.
        let before = real_git(&clone, &["rev-parse", "--short", "HEAD"]);
        let outcome = apply_git_pull_update(&clone);
        assert!(outcome.ok, "real git pull failed: {:?}", outcome.error);
        assert_eq!(outcome.new_commit.as_deref(), Some(before.as_str()));
        assert!(outcome.steps.iter().any(|s| s.id == "git-fetch" && s.status == StepStatus::Ok));
        assert!(outcome.steps.iter().any(|s| s.id == "git-pull" && s.status == StepStatus::Ok));
        assert!(outcome.steps.iter().any(|s| s.id == "build" && s.status == StepStatus::Skipped));

        // Upstream advances; after a fetch the new tip is NOT an ancestor yet.
        std::fs::write(upstream.join("CHANGELOG.md"), "next\n").unwrap();
        real_git(&upstream, &["add", "."]);
        real_git(&upstream, &["commit", "-q", "-m", "next"]);
        let upstream_tip = real_git(&upstream, &["rev-parse", "HEAD"]);
        assert_eq!(is_commit_ancestor_of_head(&clone, &upstream_tip), None); // unknown locally
        real_git(&clone, &["fetch", "--tags", "--prune"]);
        assert_eq!(is_commit_ancestor_of_head(&clone, &upstream_tip), Some(false));

        let _ = std::fs::remove_dir_all(&base);
    }

    // -- upstream probe against mock HTTP ------------------------------------

    fn spawn_mock_server(responses: Vec<(String, u16, String)>) -> (String, std::thread::JoinHandle<()>) {
        use std::io::{BufRead, BufReader, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            for _ in 0..responses.len() {
                let Ok((stream, _)) = listener.accept() else { break };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut request_line = String::new();
                if reader.read_line(&mut request_line).is_err() {
                    continue;
                }
                // Drain headers.
                loop {
                    let mut line = String::new();
                    match reader.read_line(&mut line) {
                        Ok(0) => break,
                        Ok(_) => {
                            if line.trim().is_empty() {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
                let path = request_line.split_whitespace().nth(1).unwrap_or("/").to_string();
                let (status, body) = responses
                    .iter()
                    .find(|(p, _, _)| *p == path)
                    .map(|(_, s, b)| (*s, b.clone()))
                    .unwrap_or((404, "{}".to_string()));
                let reason = match status {
                    200 => "OK",
                    404 => "Not Found",
                    500 => "Internal Server Error",
                    _ => "Status",
                };
                let response = format!(
                    "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let mut stream = stream;
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            }
        });
        (format!("http://{addr}"), handle)
    }

    fn test_client() -> reqwest::Client {
        reqwest::Client::builder().no_proxy().build().unwrap()
    }

    const REL_PATH: &str = "/repos/DevvGwardo/hermes-agentis/releases/latest";
    const COMMIT_PATH: &str = "/repos/DevvGwardo/hermes-agentis/commits/main";

    #[tokio::test]
    async fn upstream_probe_parses_latest_release() {
        let long_body = "x".repeat(600);
        let body = json!({
            "tag_name": "v0.2.0",
            "name": "Release 0.2.0",
            "body": long_body,
            "html_url": "https://github.com/DevvGwardo/hermes-agentis/releases/tag/v0.2.0",
            "target_commitish": "abcdef1234567890"
        })
        .to_string();
        let (base, server) = spawn_mock_server(vec![(REL_PATH.into(), 200, body)]);
        let info = fetch_upstream_release_info(&test_client(), &base, DEFAULT_REPO_SLUG, Duration::from_millis(3000)).await;
        server.join().unwrap();
        assert_eq!(info.latest_version.as_deref(), Some("0.2.0"));
        assert_eq!(info.latest_commit.as_deref(), Some("abcdef1"));
        assert_eq!(info.release_title.as_deref(), Some("Release 0.2.0"));
        assert_eq!(info.release_notes.as_deref().unwrap().chars().count(), 500);
        assert!(info.release_url.is_some());
        assert!(info.error.is_none());
    }

    #[tokio::test]
    async fn upstream_probe_falls_back_to_commits_main() {
        let commit_body = json!({
            "sha": "1234567890abcdef1234567890abcdef12345678",
            "html_url": "https://github.com/DevvGwardo/hermes-agentis/commit/1234567",
            "commit": { "message": "First line of message\nSecond line" }
        })
        .to_string();
        let (base, server) = spawn_mock_server(vec![
            (REL_PATH.into(), 404, "{}".into()),
            (COMMIT_PATH.into(), 200, commit_body),
        ]);
        let info = fetch_upstream_release_info(&test_client(), &base, DEFAULT_REPO_SLUG, Duration::from_millis(3000)).await;
        server.join().unwrap();
        assert!(info.latest_version.is_none());
        assert_eq!(info.latest_commit.as_deref(), Some("1234567"));
        assert_eq!(info.release_title.as_deref(), Some("First line of message"));
        assert!(info.error.is_none());
    }

    #[tokio::test]
    async fn upstream_probe_reports_http_error_when_both_endpoints_fail() {
        let (base, server) = spawn_mock_server(vec![
            (REL_PATH.into(), 404, "{}".into()),
            (COMMIT_PATH.into(), 500, "{}".into()),
        ]);
        let info = fetch_upstream_release_info(&test_client(), &base, DEFAULT_REPO_SLUG, Duration::from_millis(3000)).await;
        server.join().unwrap();
        assert!(info.latest_version.is_none());
        assert!(info.latest_commit.is_none());
        assert_eq!(info.error.as_deref(), Some("Upstream check returned HTTP 404"));
    }

    #[tokio::test]
    async fn upstream_probe_reports_network_failure() {
        // Bind then drop -> connection refused on the freed port.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        let info = fetch_upstream_release_info(
            &test_client(),
            &format!("http://{addr}"),
            DEFAULT_REPO_SLUG,
            Duration::from_millis(1500),
        )
        .await;
        assert!(info.error.is_some());
        assert!(info.latest_version.is_none());
    }

    // -- SHA-256 verification + atomic swap ----------------------------------

    #[test]
    fn sha256_matches_known_vectors_and_verifies_assets() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        let digest = sha256_hex(b"desktop-binary-payload");
        assert!(verify_asset_sha256(b"desktop-binary-payload", &digest).is_ok());
        assert!(verify_asset_sha256(b"desktop-binary-payload", &digest.to_uppercase()).is_ok());
        assert!(verify_asset_sha256(b"desktop-binary-payload", &format!("sha256:{digest}")).is_ok());
        let mismatch = verify_asset_sha256(b"tampered-binary", &digest).unwrap_err();
        assert!(mismatch.contains("SHA-256 mismatch"));
        assert!(verify_asset_sha256(b"x", "not-a-digest").is_err());
        assert!(verify_asset_sha256(b"x", "").is_err());
    }

    #[test]
    fn file_verification_and_atomic_replace_work() {
        let dir = temp_dir("sha-file");
        let asset = dir.join("voxcell.AppImage");
        std::fs::write(&asset, b"new-binary-bytes").unwrap();
        let digest = sha256_hex(b"new-binary-bytes");
        assert!(verify_asset_file_sha256(&asset, &digest).is_ok());
        assert!(verify_asset_file_sha256(&asset, &sha256_hex(b"other")).is_err());
        assert!(verify_asset_file_sha256(&dir.join("missing.msi"), &digest).is_err());

        let target = dir.join("installed").join("voxcell.exe");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(&target, b"old-binary").unwrap();
        atomic_replace_file(&target, b"new-binary-bytes").unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"new-binary-bytes");
        let leftovers: Vec<_> = std::fs::read_dir(target.parent().unwrap())
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    // -- apply orchestration --------------------------------------------------

    fn sample_snapshot() -> Value {
        json!({
            "schemaVersion": 1,
            "capturedAt": now_ms(),
            "appVersion": "0.1.0",
            "reason": "sync",
            "uiConfig": { "accentPreset": "violet", "accentHex": "#A855F7", "workspaceTitle": "Custom Studio Rig" },
            "background": { "activeId": "custom-slot-1", "customSlots": [{"id":"custom-slot-1","label":"Studio Wallpaper"}] },
            "workspace": { "showThinking": false, "onboardingCompleted": true },
            "canvas": { "sessions": [], "pins": [], "dismissedIds": [] }
        })
    }

    #[test]
    fn managed_apply_locks_vault_writes_receipt_and_preserves_categories() {
        let dir = temp_dir("managed-apply");
        let app_root = dir.join("app");
        std::fs::create_dir_all(&app_root).unwrap();
        std::fs::write(app_root.join("package.json"), json!({"version":"0.1.0"}).to_string()).unwrap();
        let vault = PreferencesVault::open(dir.join("data")).unwrap();
        let runner = FakeRunner::new();

        // Customized snapshot -> categories come from the preserved diff.
        let outcome = apply_update_with_snapshot(&vault, &app_root, Some(&sample_snapshot()), None, None, &runner);
        assert!(outcome.ok);
        assert!(outcome.needs_reload);
        assert!(!outcome.restarting);
        let ids: Vec<&str> = outcome.steps.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, vec!["vault-backup", "vault-verify", "runtime-sync"]);
        let receipt = outcome.receipt.unwrap();
        assert_eq!(receipt.mode, "managed-refresh");
        assert_eq!(receipt.previous_version, "0.1.0");
        assert_eq!(receipt.new_version, "0.1.0");
        assert!(receipt.backup_id.starts_with("pre-update-"));
        assert!(receipt.preserved_categories.contains(&"Theme & Colors".to_string()));
        assert!(receipt.preserved_categories.contains(&"Backdrop & Media".to_string()));
        assert_eq!(vault.read_receipt().unwrap().backup_id, receipt.backup_id);
        assert_eq!(vault.list_backups().len(), 1);

        // Default snapshot -> fallback category labels (server.ts parity).
        let outcome = apply_update_with_snapshot(&vault, &app_root, Some(&json!({})), None, None, &runner);
        assert!(outcome.ok);
        assert_eq!(
            outcome.receipt.unwrap().preserved_categories,
            vec!["Theme & Colors", "Layout & Typography", "Stage, Voice & Trace"]
        );

        // Ack clears the pending receipt.
        acknowledge_update_receipt(&vault);
        assert!(vault.read_receipt().is_none());
        assert!(outcome.status.pending_receipt.is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn apply_without_any_snapshot_fails_cleanly() {
        let dir = temp_dir("no-snapshot");
        let vault = PreferencesVault::open(dir.join("data")).unwrap();
        let runner = FakeRunner::new();
        let outcome = apply_update_with_snapshot(&vault, &dir, None, None, None, &runner);
        assert!(!outcome.ok);
        assert!(outcome.steps.is_empty());
        assert_eq!(outcome.error.as_deref(), Some("Unable to snapshot user preferences before update."));
        assert!(vault.list_backups().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dirty_git_worktree_pauses_update_but_keeps_pre_update_backup() {
        let dir = temp_dir("dirty-apply");
        let app_root = dir.join("app");
        std::fs::create_dir_all(app_root.join(".git")).unwrap();
        let vault = PreferencesVault::open(dir.join("data")).unwrap();
        let mut runner = FakeRunner::new();
        clean_git(&mut runner, "abc1234");
        runner.ok("git", &["status", "--porcelain"], " M src/x.rs\n M src/y.rs");
        let outcome = apply_update_with_snapshot(&vault, &app_root, Some(&sample_snapshot()), None, None, &runner);
        assert!(!outcome.ok);
        assert!(!outcome.needs_reload);
        assert!(outcome.error.unwrap().contains("uncommitted edits"));
        assert!(outcome.steps.iter().any(|s| s.id == "git-check" && s.status == StepStatus::Failed));
        assert!(!outcome.status.can_self_update);
        // Backup survived the pause.
        let backups = vault.list_backups();
        assert_eq!(backups.len(), 1);
        assert!(backups[0].id.starts_with("pre-update-"));
        assert!(vault.read_receipt().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn git_apply_success_writes_git_pull_receipt_with_commits() {
        let dir = temp_dir("git-apply");
        let app_root = dir.join("app");
        std::fs::create_dir_all(app_root.join(".git")).unwrap();
        std::fs::write(app_root.join("package.json"), json!({"version":"0.1.0"}).to_string()).unwrap();
        let vault = PreferencesVault::open(dir.join("data")).unwrap();
        let mut runner = FakeRunner::new();
        clean_git(&mut runner, "aaa1111");
        runner
            .ok("git", &["fetch", "--tags", "--prune"], "")
            .ok("git", &["pull", "--ff-only"], "Already up to date.");
        let outcome = apply_update_with_snapshot(&vault, &app_root, Some(&sample_snapshot()), None, None, &runner);
        assert!(outcome.ok);
        assert!(outcome.needs_reload);
        let receipt = outcome.receipt.unwrap();
        assert_eq!(receipt.mode, "git-pull");
        assert_eq!(receipt.previous_commit.as_deref(), Some("aaa1111"));
        assert_eq!(receipt.new_commit.as_deref(), Some("aaa1111"));
        assert_eq!(receipt.previous_version, "0.1.0");
        assert!(outcome.steps.iter().any(|s| s.id == "vault-backup"));
        assert!(outcome.steps.iter().any(|s| s.id == "build" && s.status == StepStatus::Skipped));
        assert_eq!(outcome.status.pending_receipt, Some(receipt));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn binary_update_verifies_sha_before_atomic_swap_and_writes_receipt() {
        let dir = temp_dir("binary-apply");
        let app_root = dir.join("app");
        std::fs::create_dir_all(&app_root).unwrap();
        let vault = PreferencesVault::open(dir.join("data")).unwrap();
        let target = dir.join("bin").join("voxcell.exe");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(&target, b"old-binary").unwrap();
        let asset = dir.join("voxcell-0.2.0.msi");
        std::fs::write(&asset, b"new-binary-bytes").unwrap();
        let digest = sha256_hex(b"new-binary-bytes");

        let outcome = apply_binary_update(&vault, &app_root, &target, &asset, &digest, Some(&sample_snapshot()));
        assert!(outcome.ok, "binary update failed: {:?}", outcome.error);
        assert_eq!(std::fs::read(&target).unwrap(), b"new-binary-bytes");
        let ids: Vec<&str> = outcome.steps.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, vec!["vault-backup", "asset-verify", "binary-swap", "runtime-sync"]);
        let receipt = outcome.receipt.unwrap();
        assert_eq!(receipt.mode, "managed-refresh");
        assert!(receipt.backup_id.starts_with("pre-update-"));
        assert!(outcome.needs_reload);
        assert_eq!(vault.read_receipt().unwrap(), receipt);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn binary_update_aborts_before_swap_on_sha_mismatch() {
        let dir = temp_dir("binary-mismatch");
        let app_root = dir.join("app");
        std::fs::create_dir_all(&app_root).unwrap();
        let vault = PreferencesVault::open(dir.join("data")).unwrap();
        let target = dir.join("bin").join("voxcell.exe");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(&target, b"old-binary").unwrap();
        let asset = dir.join("tampered.msi");
        std::fs::write(&asset, b"tampered-bytes").unwrap();

        let outcome = apply_binary_update(
            &vault,
            &app_root,
            &target,
            &asset,
            &sha256_hex(b"expected-bytes"),
            Some(&sample_snapshot()),
        );
        assert!(!outcome.ok);
        assert!(outcome.error.unwrap().contains("aborted before swap"));
        assert_eq!(std::fs::read(&target).unwrap(), b"old-binary", "target must be untouched");
        assert!(outcome.receipt.is_none());
        assert!(vault.read_receipt().is_none());
        assert!(outcome.steps.iter().any(|s| s.id == "asset-verify" && s.status == StepStatus::Failed));
        // Pre-update backup still exists for safety.
        assert_eq!(vault.list_backups().len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn adapter_seam_accepts_custom_normalizer() {
        // Proves the core::snapshot bridge can be injected without touching
        // vault internals (slate-snapshot integration point).
        struct TaggingAdapter;
        impl crate::vault::SnapshotAdapter for TaggingAdapter {
            fn normalize(&self, raw: &Value, fallback: &str) -> Option<Value> {
                let mut v = WebJsonAdapter.normalize(raw, fallback)?;
                v.as_object_mut().unwrap().insert("tagged".into(), json!(true));
                Some(v)
            }
            fn backup_summary(&self, id: &str, s: &Value) -> crate::vault::PreferenceBackupSummary {
                WebJsonAdapter.backup_summary(id, s)
            }
            fn preserved_categories(&self, s: &Value) -> Vec<PreservedCategorySummary> {
                WebJsonAdapter.preserved_categories(s)
            }
            fn stamp_backup(&self, s: &Value, id: &str, ms: i64, reason: &str) -> Value {
                WebJsonAdapter.stamp_backup(s, id, ms, reason)
            }
            fn stamp_restore(&self, s: &Value, id: &str, ms: i64) -> Value {
                WebJsonAdapter.stamp_restore(s, id, ms)
            }
        }
        let dir = temp_dir("adapter-seam");
        let vault = PreferencesVault::with_adapter(&dir, Box::new(TaggingAdapter), "0.1.0").unwrap();
        let saved = vault.write_vault(&json!({}), None).unwrap();
        assert_eq!(saved["tagged"], json!(true));
        assert_eq!(vault.read_vault().unwrap()["tagged"], json!(true));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
