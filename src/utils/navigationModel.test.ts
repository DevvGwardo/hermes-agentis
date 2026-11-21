import test from 'node:test';
import assert from 'node:assert/strict';
import {
  WORKSPACE_NAV_ITEMS,
  buildQuickJumpItems,
  filterQuickJumpItems,
  resolveWorkspaceShortcut,
  stepQuickJumpIndex,
} from './navigationModel';
import type { CanvasSession } from './canvasSessions';

const SAMPLE_SESSIONS: CanvasSession[] = [
  {
    id: 'session-release',
    channelId: 'release-prep',
    hermesSessionId: 'hermes-release-prep',
    profile: 'ops',
    agentPersona: 'Release Engineer',
    name: 'release-prep',
    topic: 'Ship v2 release checklist',
    shape: 'clover',
    tint: 'cyan',
    xPct: 35,
    yPct: 45,
    mood: 'focused',
    eyeStyle: 'open',
    autonomyMode: 'manual',
    scheduleLabel: 'On demand',
    sandboxEnv: 'local-repo',
    workspaceDir: '/workspace/release',
    skills: ['git', 'ci'],
  },
  {
    id: 'session-research',
    channelId: 'design-audit',
    hermesSessionId: 'hermes-design-audit',
    profile: 'default',
    agentPersona: 'UX Researcher',
    name: 'design-audit',
    topic: 'Navigation UX simplification',
    shape: 'star',
    tint: 'lime',
    xPct: 65,
    yPct: 50,
    mood: 'happy',
    eyeStyle: 'sleepy',
    autonomyMode: 'watch',
    scheduleLabel: 'Continuous',
    sandboxEnv: 'browser-worker',
    workspaceDir: '/workspace/audit',
    skills: ['ux', 'playwright'],
  },
];

test('WORKSPACE_NAV_ITEMS covers all five views with unique Alt+1..5 shortcuts', () => {
  assert.equal(WORKSPACE_NAV_ITEMS.length, 5);
  assert.deepEqual(
    WORKSPACE_NAV_ITEMS.map((v) => v.id),
    ['home', 'canvas', 'voice', 'queue', 'cli']
  );
  assert.deepEqual(
    WORKSPACE_NAV_ITEMS.map((v) => v.shortcutDigit),
    ['1', '2', '3', '4', '5']
  );
});

test('buildQuickJumpItems includes views, sessions, dynamic badges, and contextual retry action', () => {
  const items = buildQuickJumpItems({
    activeView: 'home',
    activeChannel: 'release-prep',
    sidebarVisible: true,
    isMuted: false,
    gatewayConnected: false,
    pendingApprovalCount: 2,
    activeRunningTaskCount: 1,
    sessions: SAMPLE_SESSIONS,
    thinkingByChannel: { 'release-prep': true },
  });

  const queueItem = items.find((i) => i.id === 'view:queue');
  assert.equal(queueItem?.badge, '2 pending');

  const threadItem = items.find((i) => i.id === 'view:voice');
  assert.equal(threadItem?.badge, '1 live');

  const releaseSession = items.find((i) => i.id === 'session:session-release');
  assert.equal(releaseSession?.badge, 'working');
  assert.equal(releaseSession?.active, true);

  assert.ok(items.some((i) => i.id === 'action:retry-gateway'));
  assert.equal(
    items.find((i) => i.id === 'action:toggle-sidebar')?.title,
    'Hide sidebar'
  );
});

test('buildQuickJumpItems omits retry action when gateway is connected', () => {
  const items = buildQuickJumpItems({
    activeView: 'canvas',
    activeChannel: '',
    sidebarVisible: false,
    isMuted: true,
    gatewayConnected: true,
    pendingApprovalCount: 0,
    activeRunningTaskCount: 0,
    sessions: [],
    thinkingByChannel: {},
  });

  assert.equal(items.some((i) => i.id === 'action:retry-gateway'), false);
  assert.equal(
    items.find((i) => i.id === 'action:toggle-sidebar')?.title,
    'Show sidebar'
  );
  assert.equal(
    items.find((i) => i.id === 'action:toggle-mute')?.title,
    'Unmute voice audio'
  );
});

test('filterQuickJumpItems ranks exact/prefix matches first and supports multi-token queries', () => {
  const items = buildQuickJumpItems({
    activeView: 'home',
    activeChannel: '',
    sidebarVisible: true,
    isMuted: false,
    gatewayConnected: true,
    pendingApprovalCount: 0,
    activeRunningTaskCount: 0,
    sessions: SAMPLE_SESSIONS,
    thinkingByChannel: {},
  });

  const cliMatches = filterQuickJumpItems(items, 'cli');
  assert.equal(cliMatches[0]?.id, 'view:cli');

  const opsMatches = filterQuickJumpItems(items, 'approvals');
  assert.equal(opsMatches[0]?.id, 'view:queue');

  const sessionMatches = filterQuickJumpItems(items, 'ux navigation');
  assert.equal(sessionMatches[0]?.id, 'session:session-research');

  const emptyMatches = filterQuickJumpItems(items, 'zzz-no-match');
  assert.equal(emptyMatches.length, 0);
});

test('resolveWorkspaceShortcut maps Cmd/Ctrl+K, Cmd/Ctrl+B, Cmd/Ctrl+comma, and Alt+1..5 safely', () => {
  assert.deepEqual(
    resolveWorkspaceShortcut({
      key: 'k',
      metaKey: true,
      ctrlKey: false,
      altKey: false,
      shiftKey: false,
      isEditableTarget: true,
    }),
    { type: 'toggle-quick-jump' }
  );

  // Cmd+B ignored while editing text so bold/cursor shortcuts aren't hijacked
  assert.equal(
    resolveWorkspaceShortcut({
      key: 'b',
      metaKey: true,
      ctrlKey: false,
      altKey: false,
      shiftKey: false,
      isEditableTarget: true,
    }),
    null
  );

  assert.deepEqual(
    resolveWorkspaceShortcut({
      key: 'b',
      metaKey: false,
      ctrlKey: true,
      altKey: false,
      shiftKey: false,
      isEditableTarget: false,
    }),
    { type: 'toggle-sidebar' }
  );

  assert.deepEqual(
    resolveWorkspaceShortcut({
      key: '2',
      metaKey: false,
      ctrlKey: false,
      altKey: true,
      shiftKey: false,
      isEditableTarget: false,
    }),
    { type: 'switch-view', view: 'canvas' }
  );

  assert.deepEqual(
    resolveWorkspaceShortcut({
      key: ',',
      metaKey: true,
      ctrlKey: false,
      altKey: false,
      shiftKey: false,
      isEditableTarget: false,
    }),
    { type: 'open-settings' }
  );
});

test('stepQuickJumpIndex wraps smoothly in both directions', () => {
  assert.equal(stepQuickJumpIndex(0, 4, 'prev'), 3);
  assert.equal(stepQuickJumpIndex(3, 4, 'next'), 0);
  assert.equal(stepQuickJumpIndex(1, 4, 'next'), 2);
  assert.equal(stepQuickJumpIndex(0, 0, 'next'), 0);
});
