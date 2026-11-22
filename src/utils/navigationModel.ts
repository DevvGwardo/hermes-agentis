import type { WorkspacePanelView } from '../components/VoiceWorkspacePane';
import type { CanvasSession } from './canvasSessions';
import { formatDisplaySessionTitle } from './sidebarSessions';

export interface WorkspaceNavDescriptor {
  id: WorkspacePanelView;
  label: string;
  shortLabel: string;
  description: string;
  shortcut: string;
  shortcutDigit: string;
  keywords: readonly string[];
}

export const WORKSPACE_NAV_ITEMS: readonly WorkspaceNavDescriptor[] = [
  {
    id: 'home',
    label: 'Home',
    shortLabel: 'Home',
    description: 'Ambient voice stage & quick prompt starters',
    shortcut: 'Alt+1',
    shortcutDigit: '1',
    keywords: ['home', 'start', 'voice', 'character', 'new', 'overview'],
  },
  {
    id: 'canvas',
    label: 'Canvas',
    shortLabel: 'Canvas',
    description: 'Spatial map of parallel Hermes Dots & pinned notes',
    shortcut: 'Alt+2',
    shortcutDigit: '2',
    keywords: ['canvas', 'dots', 'map', 'spatial', 'parallel', 'sessions'],
  },
  {
    id: 'voice',
    label: 'Thread',
    shortLabel: 'Thread',
    description: 'Focused conversation transcript, search & run tasks',
    shortcut: 'Alt+3',
    shortcutDigit: '3',
    keywords: ['thread', 'chat', 'messages', 'transcript', 'conversation', 'tasks'],
  },
  {
    id: 'queue',
    label: 'Queue',
    shortLabel: 'Queue',
    description: 'Gateway health, run approvals, profiles & scheduled jobs',
    shortcut: 'Alt+4',
    shortcutDigit: '4',
    keywords: ['queue', 'ops', 'operations', 'approvals', 'gateway', 'jobs', 'cron', 'skills', 'tools'],
  },
  {
    id: 'cli',
    label: 'CLI',
    shortLabel: 'CLI',
    description: 'Direct slash command console & diagnostic output stream',
    shortcut: 'Alt+5',
    shortcutDigit: '5',
    keywords: ['cli', 'terminal', 'console', 'slash', 'commands', 'shell'],
  },
] as const;

export type QuickJumpCategory = 'views' | 'sessions' | 'actions';

export interface QuickJumpItem {
  id: string;
  category: QuickJumpCategory;
  title: string;
  subtitle: string;
  shortcut?: string;
  badge?: string;
  active?: boolean;
  keywords: string;
  action:
    | { type: 'view'; view: WorkspacePanelView }
    | { type: 'session'; channelId: string; sessionId: string }
    | {
        type: 'command';
        command:
          | 'new-session'
          | 'toggle-sidebar'
          | 'open-settings'
          | 'toggle-mute'
          | 'retry-gateway';
      };
}

export interface BuildQuickJumpInput {
  activeView: WorkspacePanelView;
  activeChannel: string;
  sidebarVisible: boolean;
  isMuted: boolean;
  gatewayConnected: boolean;
  pendingApprovalCount: number;
  activeRunningTaskCount: number;
  sessions: readonly CanvasSession[];
  thinkingByChannel: Readonly<Record<string, boolean>>;
}

export function buildQuickJumpItems(input: BuildQuickJumpInput): QuickJumpItem[] {
  const items: QuickJumpItem[] = [];

  for (const nav of WORKSPACE_NAV_ITEMS) {
    let badge: string | undefined;
    if (nav.id === 'queue' && input.pendingApprovalCount > 0) {
      badge = `${input.pendingApprovalCount} pending`;
    } else if (nav.id === 'voice' && input.activeRunningTaskCount > 0) {
      badge = `${input.activeRunningTaskCount} live`;
    }

    items.push({
      id: `view:${nav.id}`,
      category: 'views',
      title: nav.label,
      subtitle: nav.description,
      shortcut: nav.shortcut,
      badge,
      active: input.activeView === nav.id,
      keywords: [nav.label, nav.description, ...nav.keywords].join(' ').toLowerCase(),
      action: { type: 'view', view: nav.id },
    });
  }

  for (const session of input.sessions) {
    const formatted = formatDisplaySessionTitle({
      topic: session.topic,
      name: session.name,
      channelId: session.channelId,
      sessionId: session.hermesSessionId,
    });
    const isWorking = Boolean(input.thinkingByChannel[session.channelId]);
    items.push({
      id: `session:${session.id}`,
      category: 'sessions',
      title: formatted.title,
      subtitle: session.topic || session.hermesSessionId,
      badge: isWorking ? 'working' : session.profile ? `@${session.profile}` : undefined,
      active: input.activeChannel === session.channelId,
      keywords: [
        formatted.title,
        session.name,
        session.topic,
        session.channelId,
        session.hermesSessionId,
        session.profile ?? '',
      ]
        .join(' ')
        .toLowerCase(),
      action: {
        type: 'session',
        channelId: session.channelId,
        sessionId: session.id,
      },
    });
  }

  items.push(
    {
      id: 'action:new-session',
      category: 'actions',
      title: 'Start new session',
      subtitle: 'Jump to Home ready for a fresh prompt',
      shortcut: 'N',
      keywords: 'new session fresh prompt start create chat',
      action: { type: 'command', command: 'new-session' },
    },
    {
      id: 'action:toggle-sidebar',
      category: 'actions',
      title: input.sidebarVisible ? 'Hide sidebar' : 'Show sidebar',
      subtitle: input.sidebarVisible
        ? 'Expand main workspace stage for focus'
        : 'Restore sessions & status sidebar',
      shortcut: '⌘B',
      keywords: 'sidebar collapse expand toggle panel hide show focus',
      action: { type: 'command', command: 'toggle-sidebar' },
    },
    {
      id: 'action:open-settings',
      category: 'actions',
      title: 'Open Settings',
      subtitle: 'Themes, layout density, motion, backdrops & updates',
      shortcut: '⌘,',
      keywords: 'settings preferences theme appearance layout customizer studio updates',
      action: { type: 'command', command: 'open-settings' },
    },
    {
      id: 'action:toggle-mute',
      category: 'actions',
      title: input.isMuted ? 'Unmute voice audio' : 'Mute voice audio',
      subtitle: input.isMuted
        ? 'Re-enable spoken Hermes audio responses'
        : 'Silence spoken Hermes voice synthesis',
      keywords: 'mute unmute audio sound voice volume speaker',
      action: { type: 'command', command: 'toggle-mute' },
    }
  );

  if (!input.gatewayConnected) {
    items.push({
      id: 'action:retry-gateway',
      category: 'actions',
      title: 'Retry gateway connection',
      subtitle: 'Probe the Hermes Agent API server now',
      keywords: 'retry reconnect probe gateway server offline health',
      action: { type: 'command', command: 'retry-gateway' },
    });
  }

  return items;
}

export function filterQuickJumpItems(
  items: readonly QuickJumpItem[],
  rawQuery: string
): QuickJumpItem[] {
  const tokens = rawQuery
    .trim()
    .toLowerCase()
    .split(/\s+/)
    .filter(Boolean);

  if (tokens.length === 0) {
    return [...items];
  }

  const scored: Array<{ item: QuickJumpItem; score: number; index: number }> = [];

  for (let i = 0; i < items.length; i++) {
    const item = items[i];
    const titleLower = item.title.toLowerCase();
    const haystack = `${titleLower} ${item.subtitle.toLowerCase()} ${item.keywords}`;

    let matchedAll = true;
    let score = 0;
    for (const token of tokens) {
      if (!haystack.includes(token)) {
        matchedAll = false;
        break;
      }
      if (titleLower === token) score += 100;
      else if (titleLower.startsWith(token)) score += 60;
      else if (titleLower.includes(token)) score += 35;
      else score += 15;
    }

    if (matchedAll) {
      scored.push({ item, score, index: i });
    }
  }

  scored.sort((a, b) => b.score - a.score || a.index - b.index);
  return scored.map((entry) => entry.item);
}

export interface ShortcutKeyLike {
  key: string;
  metaKey: boolean;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  isEditableTarget: boolean;
}

export type WorkspaceShortcutIntent =
  | { type: 'toggle-quick-jump' }
  | { type: 'toggle-sidebar' }
  | { type: 'open-settings' }
  | { type: 'switch-view'; view: WorkspacePanelView }
  | null;

export function resolveWorkspaceShortcut(evt: ShortcutKeyLike): WorkspaceShortcutIntent {
  const lower = evt.key.toLowerCase();
  const hasPrimaryMod = (evt.metaKey || evt.ctrlKey) && !evt.altKey;

  if (hasPrimaryMod && !evt.shiftKey && lower === 'k') {
    return { type: 'toggle-quick-jump' };
  }

  if (hasPrimaryMod && !evt.shiftKey && lower === 'b' && !evt.isEditableTarget) {
    return { type: 'toggle-sidebar' };
  }

  if (hasPrimaryMod && !evt.shiftKey && evt.key === ',') {
    return { type: 'open-settings' };
  }

  if (evt.altKey && !evt.metaKey && !evt.ctrlKey && !evt.shiftKey && !evt.isEditableTarget) {
    const nav = WORKSPACE_NAV_ITEMS.find((item) => item.shortcutDigit === evt.key);
    if (nav) {
      return { type: 'switch-view', view: nav.id };
    }
  }

  return null;
}

export function stepQuickJumpIndex(
  currentIndex: number,
  total: number,
  direction: 'next' | 'prev'
): number {
  if (total <= 0) return 0;
  const clamped = ((currentIndex % total) + total) % total;
  return direction === 'next' ? (clamped + 1) % total : (clamped - 1 + total) % total;
}
