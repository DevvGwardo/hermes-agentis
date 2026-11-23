import test from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { DEFAULT_BACKGROUND_SETTINGS } from '../backgrounds/persistence';
import { DEFAULT_UI_CONFIG, applyUiPatch } from './uiCustomization';
import {
  DEFAULT_WORKSPACE_PREFERENCES,
  type UserPreferencesSnapshot,
} from './settingsSnapshot';
import {
  applyGitPullUpdate,
  buildAppUpdateStatus,
  createPreferencesVaultStore,
  readLocalBuildMetadata,
  resolveVoxcellDataDir,
  selectGitPullTarget,
} from './appUpdater';

function sampleSnapshot(accentPreset: 'violet' | 'amber' = 'violet'): UserPreferencesSnapshot {
  return {
    schemaVersion: 1,
    capturedAt: Date.now(),
    appVersion: '0.1.0',
    reason: 'sync',
    uiConfig: applyUiPatch(DEFAULT_UI_CONFIG, {
      presetId: accentPreset === 'violet' ? 'cyber-violet' : 'solaris-amber',
      workspaceTitle: 'Custom Studio Rig',
    }).next,
    background: {
      ...DEFAULT_BACKGROUND_SETTINGS,
      activeId: 'custom-slot-1',
      customSlots: [
        {
          id: 'custom-slot-1',
          label: 'Studio Wallpaper',
          mediaType: 'image',
          src: 'https://example.com/wallpaper.png',
          createdAt: 1000,
        },
      ],
    },
    workspace: {
      ...DEFAULT_WORKSPACE_PREFERENCES,
      showThinking: false,
      onboardingCompleted: true,
    },
    canvas: {
      sessions: [],
      pins: [],
      dismissedIds: [],
    },
  };
}

test('preferences vault persists themes, custom wallpapers, backups, and restore points atomically', () => {
  const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'voxcell-vault-test-'));
  try {
    const store = createPreferencesVaultStore(tempDir, '0.1.0');
    assert.equal(store.readVault(), null);

    const initial = sampleSnapshot('violet');
    const saved = store.writeVault(initial);
    assert.equal(saved.uiConfig.accentPreset, 'violet');
    assert.equal(store.readVault()?.uiConfig.workspaceTitle, 'Custom Studio Rig');
    assert.equal(store.readVault()?.background.customSlots.length, 1);

    // Create a pre-update backup before changing theme
    const preUpdate = store.createBackup(saved, 'pre-update');
    assert.equal(preUpdate.backups.length, 1);
    assert.equal(preUpdate.backup.accentPreset, 'violet');

    // Change live vault to amber
    const amber = sampleSnapshot('amber');
    store.writeVault(amber);
    assert.equal(store.readVault()?.uiConfig.accentPreset, 'amber');

    // Restore pre-update backup -> vault reverts to violet cleanly
    const restored = store.restoreBackup(preUpdate.backup.id);
    assert.ok(restored);
    assert.equal(restored.snapshot.uiConfig.accentPreset, 'violet');
    assert.equal(restored.snapshot.reason, 'restore');
    assert.equal(store.readVault()?.uiConfig.accentPreset, 'violet');
    assert.equal(store.readVault()?.background.customSlots[0].label, 'Studio Wallpaper');
  } finally {
    fs.rmSync(tempDir, { recursive: true, force: true });
  }
});

test('preferences vault writes and clears post-update receipts reliably', () => {
  const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'voxcell-receipt-test-'));
  try {
    const store = createPreferencesVaultStore(tempDir, '0.1.0');
    assert.equal(store.readReceipt(), null);

    store.writeReceipt({
      updatedAt: 1_750_000_000_000,
      previousVersion: '0.1.0',
      newVersion: '0.2.0',
      previousCommit: 'abc1234',
      newCommit: 'def5678',
      backupId: 'pre-update-123',
      mode: 'git-pull',
      preservedCategories: ['Theme & Colors', 'Backdrop & Media'],
    });

    const loaded = store.readReceipt();
    assert.ok(loaded);
    assert.equal(loaded.newVersion, '0.2.0');
    assert.equal(loaded.backupId, 'pre-update-123');

    store.clearReceipt();
    assert.equal(store.readReceipt(), null);
  } finally {
    fs.rmSync(tempDir, { recursive: true, force: true });
  }
});

test('buildAppUpdateStatus detects version updates, commit divergence, and dirty git worktrees', () => {
  const upToDate = buildAppUpdateStatus({
    local: {
      version: '0.1.0',
      hasGitRepo: false,
      commit: null,
      branch: null,
      dirtyWorktree: false,
    },
    upstream: { latestVersion: '0.1.0', latestCommit: null },
    receipt: null,
    checkedAt: 1000,
  });
  assert.equal(upToDate.updateAvailable, false);
  assert.equal(upToDate.updateMode, 'managed-refresh');
  assert.equal(upToDate.canSelfUpdate, true);

  const newerRelease = buildAppUpdateStatus({
    local: {
      version: '0.1.0',
      hasGitRepo: true,
      commit: '111aaaa',
      branch: 'main',
      dirtyWorktree: false,
    },
    upstream: { latestVersion: '0.2.0', latestCommit: '222bbbb', releaseTitle: 'v0.2.0' },
    receipt: null,
    checkedAt: 2000,
  });
  assert.equal(newerRelease.updateAvailable, true);
  assert.equal(newerRelease.updateMode, 'git-pull');
  assert.equal(newerRelease.canSelfUpdate, true);

  const dirtyRepo = buildAppUpdateStatus({
    local: {
      version: '0.1.0',
      hasGitRepo: true,
      commit: '111aaaa',
      branch: 'main',
      dirtyWorktree: true,
      dirtyReason: '2 modified files in local repository',
    },
    upstream: { latestVersion: '0.2.0', latestCommit: '222bbbb' },
    receipt: null,
    checkedAt: 3000,
  });
  assert.equal(dirtyRepo.canSelfUpdate, false);
  assert.equal(dirtyRepo.dirtyWorktree, true);
});

test('buildAppUpdateStatus does not flag an update when local is ahead of upstream (upstream commit is an ancestor of HEAD)', () => {
  const local: Parameters<typeof buildAppUpdateStatus>[0]['local'] = {
    version: '0.1.0',
    hasGitRepo: true,
    commit: 'b58f223',
    branch: 'main',
    dirtyWorktree: false,
  };

  // Local HEAD contains the upstream tip (e.g. local has an unpushed commit on top)
  const ahead = buildAppUpdateStatus({
    local,
    upstream: { latestVersion: null, latestCommit: 'fd4ae0a' },
    receipt: null,
    upstreamAncestorOfLocal: true,
  });
  assert.equal(ahead.updateAvailable, false);

  // Upstream genuinely newer: ancestry check says upstream is NOT in local history
  const behind = buildAppUpdateStatus({
    local,
    upstream: { latestVersion: null, latestCommit: '9998887' },
    receipt: null,
    upstreamAncestorOfLocal: false,
  });
  assert.equal(behind.updateAvailable, true);

  // Ancestry unknown (git error / missing object): fall back to divergence heuristic
  const unknownAncestry = buildAppUpdateStatus({
    local,
    upstream: { latestVersion: null, latestCommit: '9998887' },
    receipt: null,
  });
  assert.equal(unknownAncestry.updateAvailable, true);
});

test('resolveVoxcellDataDir honors VOXCELL_DATA_DIR override', () => {
  const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'voxcell-env-test-'));
  try {
    const resolved = resolveVoxcellDataDir('/app/applet', {
      VOXCELL_DATA_DIR: tempDir,
    });
    assert.equal(resolved, path.resolve(tempDir));
  } finally {
    fs.rmSync(tempDir, { recursive: true, force: true });
  }
});

test('readLocalBuildMetadata handles non-git directories gracefully', async () => {
  const tempApp = fs.mkdtempSync(path.join(os.tmpdir(), 'voxcell-app-test-'));
  try {
    fs.writeFileSync(
      path.join(tempApp, 'package.json'),
      JSON.stringify({ name: 'test', version: '0.4.2' }),
      'utf8'
    );
    const meta = await readLocalBuildMetadata(tempApp);
    assert.equal(meta.version, '0.4.2');
    assert.equal(meta.hasGitRepo, false);
    assert.equal(meta.dirtyWorktree, false);
  } finally {
    fs.rmSync(tempApp, { recursive: true, force: true });
  }
});

test('selectGitPullTarget resolves tracked, matching untracked, default-head, and detached targets', () => {
  // 1. Tracked upstream wins directly
  assert.deepEqual(
    selectGitPullTarget({
      currentBranch: 'rune-desktop',
      upstreamRef: 'origin/rune-desktop',
      remotes: ['origin'],
      remoteBranches: { origin: ['main', 'rune-desktop'] },
    }),
    { mode: 'tracked', ref: 'origin/rune-desktop' }
  );

  // 2. Untracked local branch matching remote branch sets upstream
  assert.deepEqual(
    selectGitPullTarget({
      currentBranch: 'rune-desktop',
      upstreamRef: null,
      remotes: ['origin'],
      remoteBranches: { origin: ['main', 'rune-desktop'] },
      remoteHeadBranch: { origin: 'main' },
    }),
    {
      mode: 'explicit',
      remote: 'origin',
      branch: 'rune-desktop',
      ref: 'origin/rune-desktop',
      shouldSetUpstream: true,
    }
  );

  // 3. Untracked local branch with only origin/main falls back to origin/main
  assert.deepEqual(
    selectGitPullTarget({
      currentBranch: 'rune-desktop',
      upstreamRef: null,
      remotes: ['origin'],
      remoteBranches: { origin: ['main'] },
      remoteHeadBranch: { origin: 'main' },
    }),
    {
      mode: 'explicit',
      remote: 'origin',
      branch: 'main',
      ref: 'origin/main',
      shouldSetUpstream: false,
    }
  );

  // 4. No remotes returns null
  assert.equal(
    selectGitPullTarget({
      currentBranch: 'rune-desktop',
      upstreamRef: null,
      remotes: [],
      remoteBranches: {},
    }),
    null
  );
});

test('applyGitPullUpdate fast-forwards an untracked branch (rune-desktop) without tracking errors', async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'voxcell-git-ff-test-'));
  const bareDir = path.join(root, 'remote.git');
  const seedDir = path.join(root, 'seed');
  const localDir = path.join(root, 'local');

  const git = (cwd: string, args: string[]) =>
    execFileSync('git', args, {
      cwd,
      stdio: 'pipe',
      env: {
        ...process.env,
        GIT_AUTHOR_NAME: 'Test',
        GIT_AUTHOR_EMAIL: 'test@example.com',
        GIT_COMMITTER_NAME: 'Test',
        GIT_COMMITTER_EMAIL: 'test@example.com',
      },
    })
      .toString('utf8')
      .trim();

  try {
    fs.mkdirSync(bareDir, { recursive: true });
    git(bareDir, ['init', '--bare', '-b', 'main']);

    git(root, ['clone', bareDir, seedDir]);
    fs.writeFileSync(
      path.join(seedDir, 'package.json'),
      JSON.stringify({ name: 'test-app', version: '0.1.0', scripts: { build: 'node -e "process.exit(0)"' } }),
      'utf8'
    );
    fs.writeFileSync(path.join(seedDir, 'notes.txt'), 'v1\n', 'utf8');
    git(seedDir, ['add', '.']);
    git(seedDir, ['commit', '-m', 'initial']);
    git(seedDir, ['push', '-u', 'origin', 'main']);

    // Clone locally and create an untracked branch `rune-desktop` (reproducing exact user error)
    git(root, ['clone', bareDir, localDir]);
    git(localDir, ['checkout', '-b', 'rune-desktop']);
    // Confirm there is NO upstream tracking branch initially
    assert.throws(() => git(localDir, ['rev-parse', '--abbrev-ref', '@{u}']));

    // Advance origin/main with a new commit
    fs.writeFileSync(path.join(seedDir, 'notes.txt'), 'v2\n', 'utf8');
    git(seedDir, ['add', 'notes.txt']);
    git(seedDir, ['commit', '-m', 'upstream update']);
    git(seedDir, ['push', 'origin', 'main']);
    const upstreamSha = git(seedDir, ['rev-parse', '--short', 'HEAD']);

    // applyGitPullUpdate should resolve origin/main and fast-forward cleanly!
    const result = await applyGitPullUpdate(localDir);
    assert.equal(result.ok, true, `Expected update to succeed, got error: ${result.error}`);
    assert.equal(result.newCommit, upstreamSha);
    assert.equal(fs.readFileSync(path.join(localDir, 'notes.txt'), 'utf8'), 'v2\n');
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});
