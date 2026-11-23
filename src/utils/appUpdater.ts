import { execFile } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { promisify } from 'node:util';
import {
  compareSemver,
  normalizePreferencesSnapshot,
  summarizeBackupRecord,
  type AppUpdateStatus,
  type PreferenceBackupSummary,
  type SnapshotReason,
  type UpdateReceipt,
  type UpdateStepResult,
  type UserPreferencesSnapshot,
} from './settingsSnapshot.ts';

const execFileAsync = promisify(execFile);

const MAX_BACKUPS = 12;

export interface LocalBuildMetadata {
  version: string;
  hasGitRepo: boolean;
  commit: string | null;
  branch: string | null;
  dirtyWorktree: boolean;
  dirtyReason?: string;
}

export interface UpstreamReleaseInfo {
  latestVersion: string | null;
  latestCommit: string | null;
  releaseTitle?: string;
  releaseNotes?: string;
  releaseUrl?: string;
  error?: string;
}

export interface PreferencesVaultStore {
  dataDir: string;
  vaultPath: string;
  backupsDir: string;
  receiptPath: string;
  readVault: () => UserPreferencesSnapshot | null;
  writeVault: (
    snapshot: UserPreferencesSnapshot,
    reason?: SnapshotReason
  ) => UserPreferencesSnapshot;
  createBackup: (
    snapshot: UserPreferencesSnapshot,
    reason?: SnapshotReason
  ) => { backup: PreferenceBackupSummary; backups: PreferenceBackupSummary[] };
  listBackups: () => PreferenceBackupSummary[];
  restoreBackup: (
    backupId: string
  ) => { snapshot: UserPreferencesSnapshot; backups: PreferenceBackupSummary[] } | null;
  readReceipt: () => UpdateReceipt | null;
  writeReceipt: (receipt: UpdateReceipt) => void;
  clearReceipt: () => void;
}

function writeJsonAtomic(filePath: string, payload: unknown): void {
  const dir = path.dirname(filePath);
  fs.mkdirSync(dir, { recursive: true });
  const tmpPath = `${filePath}.${process.pid}.${Date.now()}.tmp`;
  fs.writeFileSync(tmpPath, JSON.stringify(payload, null, 2), 'utf8');
  fs.renameSync(tmpPath, filePath);
}

export function resolveVoxcellDataDir(
  appRoot: string,
  env: NodeJS.ProcessEnv = process.env
): string {
  const explicit = env.VOXCELL_DATA_DIR?.trim();
  if (explicit) {
    return path.resolve(explicit);
  }
  const home = env.HOME?.trim() || os.homedir();
  const candidate = path.join(home, '.voxcell');
  try {
    fs.mkdirSync(candidate, { recursive: true });
    fs.accessSync(candidate, fs.constants.W_OK);
    return candidate;
  } catch {
    const fallback = path.join(appRoot, '.voxcell-data');
    fs.mkdirSync(fallback, { recursive: true });
    return fallback;
  }
}

export function createPreferencesVaultStore(
  dataDir: string,
  fallbackVersion = '0.1.0'
): PreferencesVaultStore {
  const vaultPath = path.join(dataDir, 'preferences-vault.json');
  const backupsDir = path.join(dataDir, 'backups');
  const receiptPath = path.join(dataDir, 'update-receipt.json');

  const ensureDirs = () => {
    fs.mkdirSync(dataDir, { recursive: true });
    fs.mkdirSync(backupsDir, { recursive: true });
  };

  const readJsonSnapshot = (filePath: string): UserPreferencesSnapshot | null => {
    try {
      if (!fs.existsSync(filePath)) return null;
      const raw = JSON.parse(fs.readFileSync(filePath, 'utf8')) as unknown;
      return normalizePreferencesSnapshot(raw, fallbackVersion);
    } catch {
      return null;
    }
  };

  const listBackups = (): PreferenceBackupSummary[] => {
    try {
      if (!fs.existsSync(backupsDir)) return [];
      const files = fs
        .readdirSync(backupsDir)
        .filter((f) => f.endsWith('.json') && !f.endsWith('.tmp'));
      const items: PreferenceBackupSummary[] = [];
      for (const file of files) {
        const id = file.replace(/\.json$/i, '');
        const snap = readJsonSnapshot(path.join(backupsDir, file));
        if (snap) {
          items.push(summarizeBackupRecord(id, snap));
        }
      }
      items.sort((a, b) => b.createdAt - a.createdAt);
      return items;
    } catch {
      return [];
    }
  };

  const pruneBackups = () => {
    const all = listBackups();
    if (all.length <= MAX_BACKUPS) return;
    for (const stale of all.slice(MAX_BACKUPS)) {
      try {
        fs.unlinkSync(path.join(backupsDir, `${stale.id}.json`));
      } catch {
        // ignore prune failures
      }
    }
  };

  const readVault = () => readJsonSnapshot(vaultPath);

  const writeVault = (
    rawSnapshot: UserPreferencesSnapshot,
    reason?: SnapshotReason
  ): UserPreferencesSnapshot => {
    ensureDirs();
    const normalized =
      normalizePreferencesSnapshot(
        {
          ...rawSnapshot,
          reason: reason ?? rawSnapshot.reason ?? 'sync',
        },
        fallbackVersion
      ) ?? rawSnapshot;
    writeJsonAtomic(vaultPath, normalized);
    return normalized;
  };

  const createBackup = (
    rawSnapshot: UserPreferencesSnapshot,
    reason: SnapshotReason = 'manual'
  ): { backup: PreferenceBackupSummary; backups: PreferenceBackupSummary[] } => {
    ensureDirs();
    const stamped = Date.now();
    const safePrefix = reason === 'pre-update' ? 'pre-update' : 'manual';
    const rand = Math.random().toString(36).slice(2, 6);
    const id = `${safePrefix}-${stamped}-${rand}`;
    const stored = writeVault(
      {
        ...rawSnapshot,
        snapshotId: id,
        capturedAt: rawSnapshot.capturedAt || stamped,
        reason,
      },
      reason
    );
    writeJsonAtomic(path.join(backupsDir, `${id}.json`), stored);
    pruneBackups();
    const backup = summarizeBackupRecord(id, stored);
    return {
      backup,
      backups: listBackups(),
    };
  };

  const restoreBackup = (backupId: string) => {
    const safeId = path.basename(backupId).replace(/\.json$/i, '');
    if (!safeId || safeId.includes('..') || safeId.includes('/')) return null;
    const targetPath = path.join(backupsDir, `${safeId}.json`);
    const loaded = readJsonSnapshot(targetPath);
    if (!loaded) return null;
    const restored = writeVault(
      {
        ...loaded,
        capturedAt: Date.now(),
        reason: 'restore',
        snapshotId: safeId,
      },
      'restore'
    );
    return {
      snapshot: restored,
      backups: listBackups(),
    };
  };

  const readReceipt = (): UpdateReceipt | null => {
    try {
      if (!fs.existsSync(receiptPath)) return null;
      const parsed = JSON.parse(fs.readFileSync(receiptPath, 'utf8')) as Partial<UpdateReceipt>;
      if (!parsed || typeof parsed.updatedAt !== 'number') return null;
      return {
        updatedAt: parsed.updatedAt,
        previousVersion: String(parsed.previousVersion || fallbackVersion),
        newVersion: String(parsed.newVersion || fallbackVersion),
        previousCommit: parsed.previousCommit ?? null,
        newCommit: parsed.newCommit ?? null,
        backupId: String(parsed.backupId || ''),
        mode: parsed.mode === 'git-pull' ? 'git-pull' : 'managed-refresh',
        preservedCategories: Array.isArray(parsed.preservedCategories)
          ? parsed.preservedCategories.map(String)
          : [],
      };
    } catch {
      return null;
    }
  };

  const writeReceipt = (receipt: UpdateReceipt) => {
    ensureDirs();
    writeJsonAtomic(receiptPath, receipt);
  };

  const clearReceipt = () => {
    try {
      if (fs.existsSync(receiptPath)) fs.unlinkSync(receiptPath);
    } catch {
      // ignore
    }
  };

  return {
    dataDir,
    vaultPath,
    backupsDir,
    receiptPath,
    readVault,
    writeVault,
    createBackup,
    listBackups,
    restoreBackup,
    readReceipt,
    writeReceipt,
    clearReceipt,
  };
}

export function readPackageVersion(appRoot: string): string {
  try {
    const pkg = JSON.parse(
      fs.readFileSync(path.join(appRoot, 'package.json'), 'utf8')
    ) as { version?: string };
    return typeof pkg.version === 'string' && pkg.version.trim()
      ? pkg.version.trim()
      : '0.1.0';
  } catch {
    return '0.1.0';
  }
}

async function runGit(appRoot: string, args: string[], timeoutMs = 15_000): Promise<string> {
  const { stdout } = await execFileAsync('git', args, {
    cwd: appRoot,
    timeout: timeoutMs,
    maxBuffer: 2 * 1024 * 1024,
  });
  return stdout.trim();
}

/**
 * Checks whether `commitSha` is an ancestor of local HEAD. Returns true when
 * local already contains that commit, false when local is missing it, and null
 * when git could not answer (unknown object, git failure, non-repo).
 */
export async function isCommitAncestorOfHead(
  appRoot: string,
  commitSha: string,
  timeoutMs = 10_000
): Promise<boolean | null> {
  try {
    await runGit(appRoot, ['merge-base', '--is-ancestor', commitSha, 'HEAD'], timeoutMs);
    return true;
  } catch (err) {
    const exitCode = (err as { code?: unknown })?.code;
    if (typeof exitCode === 'number' && exitCode === 1) {
      return false;
    }
    return null;
  }
}

export async function readLocalBuildMetadata(appRoot: string): Promise<LocalBuildMetadata> {
  const version = readPackageVersion(appRoot);
  const gitDir = path.join(appRoot, '.git');
  if (!fs.existsSync(gitDir)) {
    return {
      version,
      hasGitRepo: false,
      commit: null,
      branch: null,
      dirtyWorktree: false,
    };
  }

  try {
    const [commit, branch, porcelain] = await Promise.all([
      runGit(appRoot, ['rev-parse', '--short', 'HEAD']).catch(() => ''),
      runGit(appRoot, ['rev-parse', '--abbrev-ref', 'HEAD']).catch(() => ''),
      runGit(appRoot, ['status', '--porcelain']).catch(() => ''),
    ]);
    const dirtyLines = porcelain
      .split('\n')
      .map((l) => l.trim())
      .filter(Boolean);
    return {
      version,
      hasGitRepo: true,
      commit: commit || null,
      branch: branch || null,
      dirtyWorktree: dirtyLines.length > 0,
      ...(dirtyLines.length > 0
        ? {
            dirtyReason: `${dirtyLines.length} modified ${
              dirtyLines.length === 1 ? 'file' : 'files'
            } in local repository`,
          }
        : {}),
    };
  } catch {
    return {
      version,
      hasGitRepo: true,
      commit: null,
      branch: null,
      dirtyWorktree: false,
    };
  }
}

export async function fetchUpstreamReleaseInfo(
  repoSlug = 'DevvGwardo/hermes-agentis',
  timeoutMs = 4000
): Promise<UpstreamReleaseInfo> {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  try {
    const headers = {
      Accept: 'application/vnd.github+json',
      'User-Agent': 'voxcell-hermes-updater',
    };
    const relRes = await fetch(`https://api.github.com/repos/${repoSlug}/releases/latest`, {
      headers,
      signal: controller.signal,
    });
    if (relRes.ok) {
      const rel = (await relRes.json()) as {
        tag_name?: string;
        name?: string;
        body?: string;
        html_url?: string;
        target_commitish?: string;
      };
      return {
        latestVersion: rel.tag_name ? rel.tag_name.replace(/^v/i, '') : null,
        latestCommit: rel.target_commitish ? rel.target_commitish.slice(0, 7) : null,
        releaseTitle: rel.name || rel.tag_name || undefined,
        releaseNotes: rel.body ? rel.body.slice(0, 500) : undefined,
        releaseUrl: rel.html_url || undefined,
      };
    }

    const commitRes = await fetch(`https://api.github.com/repos/${repoSlug}/commits/main`, {
      headers,
      signal: controller.signal,
    });
    if (commitRes.ok) {
      const data = (await commitRes.json()) as {
        sha?: string;
        html_url?: string;
        commit?: { message?: string };
      };
      const shaShort = data.sha ? data.sha.slice(0, 7) : null;
      const firstLine = data.commit?.message?.split('\n')[0]?.trim();
      return {
        latestVersion: null,
        latestCommit: shaShort,
        releaseTitle: firstLine || undefined,
        releaseUrl: data.html_url || undefined,
      };
    }

    return {
      latestVersion: null,
      latestCommit: null,
      error: `Upstream check returned HTTP ${relRes.status}`,
    };
  } catch (err) {
    return {
      latestVersion: null,
      latestCommit: null,
      error: err instanceof Error ? err.message : 'Unable to reach upstream repository',
    };
  } finally {
    clearTimeout(timer);
  }
}

export function buildAppUpdateStatus(opts: {
  local: LocalBuildMetadata;
  upstream: UpstreamReleaseInfo | null;
  receipt: UpdateReceipt | null;
  checkedAt?: number;
  /**
   * True when the upstream tip commit is already an ancestor of local HEAD
   * (local is up to date or AHEAD of upstream — no pull needed). False when
   * upstream has commits local is missing. Null/undefined when ancestry could
   * not be determined; falls back to the commit-divergence heuristic.
   */
  upstreamAncestorOfLocal?: boolean | null;
}): AppUpdateStatus {
  const { local, upstream, receipt } = opts;
  const checkedAt = opts.checkedAt ?? Date.now();
  const latestVersion = upstream?.latestVersion ?? null;
  const latestCommit = upstream?.latestCommit ?? null;

  let updateAvailable = false;
  if (latestVersion && compareSemver(local.version, latestVersion) < 0) {
    updateAvailable = true;
  } else if (
    local.hasGitRepo &&
    local.commit &&
    latestCommit &&
    !latestCommit.startsWith(local.commit) &&
    !local.commit.startsWith(latestCommit)
  ) {
    // Local may simply be ahead of upstream (unpushed commits on top of the
    // upstream tip) — that is NOT an update. Only flag when ancestry proves
    // local is missing upstream commits, or when ancestry is unknown.
    updateAvailable = opts.upstreamAncestorOfLocal !== true;
  }

  return {
    currentVersion: local.version,
    currentCommit: local.commit,
    branch: local.branch,
    latestVersion,
    latestCommit,
    updateAvailable,
    updateMode: local.hasGitRepo ? 'git-pull' : 'managed-refresh',
    canSelfUpdate: !local.hasGitRepo || !local.dirtyWorktree,
    dirtyWorktree: local.dirtyWorktree,
    ...(local.dirtyReason ? { dirtyReason: local.dirtyReason } : {}),
    ...(upstream?.releaseTitle ? { releaseTitle: upstream.releaseTitle } : {}),
    ...(upstream?.releaseNotes ? { releaseNotes: upstream.releaseNotes } : {}),
    ...(upstream?.releaseUrl ? { releaseUrl: upstream.releaseUrl } : {}),
    ...(upstream?.error ? { upstreamError: upstream.error } : {}),
    checkedAt,
    pendingReceipt: receipt,
  };
}

export type GitPullTarget =
  | { mode: 'tracked'; ref: string }
  | {
      mode: 'explicit';
      remote: string;
      branch: string;
      ref: string;
      shouldSetUpstream: boolean;
    }
  | {
      mode: 'detached';
      remote: string;
      branch: string;
      ref: string;
    };

/**
 * Resolves the safest fast-forward pull target when the current branch may not
 * have an upstream tracking branch configured (e.g., local branches like
 * `rune-desktop` checked out without `-u`).
 */
export function selectGitPullTarget(params: {
  currentBranch: string | null;
  upstreamRef: string | null;
  remotes: readonly string[];
  remoteBranches: Readonly<Record<string, readonly string[]>>;
  remoteHeadBranch?: Readonly<Record<string, string | null | undefined>>;
}): GitPullTarget | null {
  const upstream = params.upstreamRef?.trim() || null;
  if (upstream) {
    return { mode: 'tracked', ref: upstream };
  }

  const remotes = params.remotes.map((r) => r.trim()).filter(Boolean);
  if (remotes.length === 0) return null;

  const remote = remotes.includes('origin') ? 'origin' : remotes[0];
  const branches = (params.remoteBranches[remote] ?? [])
    .map((b) => b.trim())
    .filter((b) => Boolean(b) && b !== 'HEAD');

  const current = params.currentBranch?.trim() || null;
  const isDetached = !current || current === 'HEAD';
  const defaultHead = params.remoteHeadBranch?.[remote]?.trim() || null;

  const pickBranch = (): { branch: string; matchesCurrent: boolean } | null => {
    if (!isDetached && current && branches.includes(current)) {
      return { branch: current, matchesCurrent: true };
    }
    if (defaultHead && branches.includes(defaultHead)) {
      return { branch: defaultHead, matchesCurrent: false };
    }
    if (branches.includes('main')) {
      return { branch: 'main', matchesCurrent: false };
    }
    if (branches.includes('master')) {
      return { branch: 'master', matchesCurrent: false };
    }
    if (branches.length > 0) {
      return { branch: branches[0], matchesCurrent: false };
    }
    return null;
  };

  const picked = pickBranch();
  if (!picked) return null;

  if (isDetached) {
    return {
      mode: 'detached',
      remote,
      branch: picked.branch,
      ref: `${remote}/${picked.branch}`,
    };
  }

  return {
    mode: 'explicit',
    remote,
    branch: picked.branch,
    ref: `${remote}/${picked.branch}`,
    shouldSetUpstream: picked.matchesCurrent,
  };
}

async function listRemoteBranches(
  appRoot: string,
  remote: string
): Promise<{ branches: string[]; headBranch: string | null }> {
  const prefix = `${remote}/`;
  const refsRaw = await runGit(appRoot, [
    'for-each-ref',
    '--format=%(refname:short)',
    `refs/remotes/${remote}`,
  ]).catch(() => '');

  const branches = refsRaw
    .split('\n')
    .map((line) => line.trim())
    .filter((line) => line.startsWith(prefix))
    .map((line) => line.slice(prefix.length))
    .filter((b) => Boolean(b) && b !== 'HEAD');

  const headRaw = await runGit(appRoot, [
    'symbolic-ref',
    '--short',
    `refs/remotes/${remote}/HEAD`,
  ]).catch(() => '');

  const headBranch =
    headRaw && headRaw.startsWith(prefix) ? headRaw.slice(prefix.length).trim() || null : null;

  return { branches, headBranch };
}

export async function applyGitPullUpdate(appRoot: string): Promise<{
  ok: boolean;
  steps: UpdateStepResult[];
  newVersion: string;
  newCommit: string | null;
  error?: string;
}> {
  const steps: UpdateStepResult[] = [];
  const beforeCommit = await runGit(appRoot, ['rev-parse', '--short', 'HEAD']).catch(() => '');
  const currentBranch = await runGit(appRoot, ['rev-parse', '--abbrev-ref', 'HEAD']).catch(() => '');
  const upstreamRef = await runGit(appRoot, [
    'rev-parse',
    '--abbrev-ref',
    '--symbolic-full-name',
    '@{u}',
  ]).catch(() => '');

  const remotesRaw = await runGit(appRoot, ['remote']).catch(() => '');
  const remotes = remotesRaw
    .split('\n')
    .map((r) => r.trim())
    .filter(Boolean);
  const preferredRemote = remotes.includes('origin') ? 'origin' : remotes[0] || '';

  try {
    const fetchArgs = preferredRemote
      ? ['fetch', '--tags', '--prune', preferredRemote]
      : ['fetch', '--tags', '--prune'];
    await runGit(appRoot, fetchArgs, 30_000);
    steps.push({ id: 'git-fetch', label: 'Fetched upstream repository', status: 'ok' });
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err);
    steps.push({ id: 'git-fetch', label: 'Fetch upstream repository', status: 'failed', detail: msg });
    return {
      ok: false,
      steps,
      newVersion: readPackageVersion(appRoot),
      newCommit: beforeCommit || null,
      error: `Unable to fetch updates: ${msg}`,
    };
  }

  const remoteBranches: Record<string, string[]> = {};
  const remoteHeadBranch: Record<string, string | null> = {};
  if (!upstreamRef && preferredRemote) {
    const inspected = await listRemoteBranches(appRoot, preferredRemote);
    remoteBranches[preferredRemote] = inspected.branches;
    remoteHeadBranch[preferredRemote] = inspected.headBranch;
  }

  const target = selectGitPullTarget({
    currentBranch: currentBranch || null,
    upstreamRef: upstreamRef || null,
    remotes,
    remoteBranches,
    remoteHeadBranch,
  });

  if (!target) {
    const detail =
      remotes.length === 0
        ? 'No git remote configured in this repository.'
        : `No remote branches found on "${preferredRemote}" for branch "${currentBranch || 'HEAD'}".`;
    steps.push({
      id: 'git-pull',
      label: 'Resolve upstream tracking branch',
      status: 'failed',
      detail,
    });
    return {
      ok: false,
      steps,
      newVersion: readPackageVersion(appRoot),
      newCommit: beforeCommit || null,
      error: `Repository fast-forward blocked: ${detail}`,
    };
  }

  try {
    let pullOut = '';
    if (target.mode === 'tracked') {
      pullOut = await runGit(appRoot, ['pull', '--ff-only'], 45_000);
    } else if (target.mode === 'detached') {
      pullOut = await runGit(appRoot, ['merge', '--ff-only', target.ref], 45_000);
    } else {
      if (target.shouldSetUpstream && currentBranch && currentBranch !== 'HEAD') {
        await runGit(appRoot, [
          'branch',
          `--set-upstream-to=${target.ref}`,
          currentBranch,
        ]).catch(() => {
          // Non-fatal if branch config is read-only; explicit pull still succeeds
        });
      }
      pullOut = await runGit(appRoot, ['pull', '--ff-only', target.remote, target.branch], 45_000);
    }

    const firstLine = pullOut.split('\n')[0]?.trim() || 'Up to date';
    steps.push({
      id: 'git-pull',
      label: 'Updated source code (fast-forward)',
      status: 'ok',
      detail: target.mode === 'tracked' ? firstLine : `${firstLine} (${target.ref})`,
    });
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err);
    steps.push({ id: 'git-pull', label: 'Fast-forward repository update', status: 'failed', detail: msg });
    return {
      ok: false,
      steps,
      newVersion: readPackageVersion(appRoot),
      newCommit: beforeCommit || null,
      error: `Repository fast-forward blocked: ${msg}`,
    };
  }

  const afterCommit = await runGit(appRoot, ['rev-parse', '--short', 'HEAD']).catch(() => beforeCommit);
  if (afterCommit && beforeCommit && afterCommit !== beforeCommit) {
    try {
      const changedFiles = await runGit(appRoot, ['diff', '--name-only', beforeCommit, afterCommit]);
      if (/(^|\n)(package\.json|bun\.lock|package-lock\.json)(\n|$)/.test(changedFiles)) {
        const useBun = fs.existsSync(path.join(appRoot, 'bun.lock'));
        await execFileAsync(useBun ? 'bun' : 'npm', ['install'], {
          cwd: appRoot,
          timeout: 120_000,
        });
        steps.push({ id: 'deps', label: 'Synced updated dependencies', status: 'ok' });
      }
      await execFileAsync('npm', ['run', 'build'], {
        cwd: appRoot,
        timeout: 120_000,
      });
      steps.push({ id: 'build', label: 'Rebuilt production assets', status: 'ok' });
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      steps.push({ id: 'build', label: 'Build after update', status: 'failed', detail: msg });
      return {
        ok: false,
        steps,
        newVersion: readPackageVersion(appRoot),
        newCommit: afterCommit || null,
        error: `Source updated, but rebuild failed: ${msg}`,
      };
    }
  } else {
    steps.push({
      id: 'build',
      label: 'Already at latest commit — preferences verified',
      status: 'skipped',
    });
  }

  return {
    ok: true,
    steps,
    newVersion: readPackageVersion(appRoot),
    newCommit: afterCommit || null,
  };
}
