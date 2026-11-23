export type SlashCommandCategory = 'session' | 'hermes' | 'runs' | 'workspace';

export interface SlashCommandSpec {
  command: string;
  usage: string;
  category: SlashCommandCategory;
  summary: string;
  action: 'run' | 'insert';
}

export const SLASH_COMMAND_CATALOG: readonly SlashCommandSpec[] = [
  {
    command: '/new',
    usage: '/new [prompt]',
    category: 'session',
    summary: 'Start an isolated Hermes Agent session on a fresh lane',
    action: 'insert',
  },
  {
    command: '/skills',
    usage: '/skills',
    category: 'hermes',
    summary: 'List loaded Hermes skills from GET /v1/skills',
    action: 'run',
  },
  {
    command: '/recall',
    usage: '/recall [on|off]',
    category: 'hermes',
    summary: 'Toggle Jev × uniichat-recall accuracy projection graph & SKILL.md',
    action: 'run',
  },
  {
    command: '/context',
    usage: '/context [on|off]',
    category: 'hermes',
    summary: 'Toggle live model info and accurate context window meter',
    action: 'run',
  },
  {
    command: '/activity',
    usage: '/activity [on|off|expand|collapse]',
    category: 'hermes',
    summary: 'Toggle inline activity & tool call cards in chat',
    action: 'run',
  },
  {
    command: '/toolsets',
    usage: '/toolsets',
    category: 'hermes',
    summary: 'Inspect available toolsets and tools from GET /v1/toolsets',
    action: 'run',
  },
  {
    command: '/gateway',
    usage: '/gateway',
    category: 'hermes',
    summary: 'Check gateway health, active model, capabilities, and transport',
    action: 'run',
  },
  {
    command: '/doctor',
    usage: '/doctor',
    category: 'hermes',
    summary: 'Run full Hermes Agent gateway & endpoint diagnostics',
    action: 'run',
  },
  {
    command: '/cron',
    usage: '/cron',
    category: 'hermes',
    summary: 'List scheduled background jobs from GET /api/jobs',
    action: 'run',
  },
  {
    command: '/profiles',
    usage: '/profiles',
    category: 'session',
    summary: 'List isolated ~/.hermes/profiles lanes for parallel work',
    action: 'run',
  },
  {
    command: '/profile',
    usage: '/profile <slug>',
    category: 'session',
    summary: 'Switch default Hermes profile for new session launches',
    action: 'insert',
  },
  {
    command: '/sessions',
    usage: '/sessions',
    category: 'session',
    summary: 'Show synced Hermes sessions from GET /api/sessions',
    action: 'run',
  },
  {
    command: '/switch',
    usage: '/switch <session>',
    category: 'session',
    summary: 'Switch active conversation lane by slug or session id',
    action: 'insert',
  },
  {
    command: '/resume',
    usage: '/resume <session>',
    category: 'session',
    summary: 'Resume an existing Hermes session lane',
    action: 'insert',
  },
  {
    command: '/channels',
    usage: '/channels',
    category: 'session',
    summary: 'List all available conversation lanes',
    action: 'run',
  },
  {
    command: '/tasks',
    usage: '/tasks',
    category: 'runs',
    summary: 'Inspect active and settled run tasks across sessions',
    action: 'run',
  },
  {
    command: '/approve',
    usage: '/approve [id] [once|always|deny]',
    category: 'runs',
    summary: 'Approve a pending tool execution gate',
    action: 'insert',
  },
  {
    command: '/reject',
    usage: '/reject [id]',
    category: 'runs',
    summary: 'Deny a pending tool execution gate',
    action: 'insert',
  },
  {
    command: '/brief',
    usage: '/brief',
    category: 'runs',
    summary: 'Review pending approvals and action queue items',
    action: 'run',
  },
  {
    command: '/stop',
    usage: '/stop',
    category: 'runs',
    summary: 'Stop active run in the current session (/v1/runs/{id}/stop)',
    action: 'run',
  },
  {
    command: '/transport',
    usage: '/transport <auto|runs|session-stream|completions>',
    category: 'hermes',
    summary: 'Switch turn transport protocol for Hermes Gateway',
    action: 'insert',
  },
  {
    command: '/memory',
    usage: '/memory',
    category: 'hermes',
    summary: 'Inspect X-Hermes-Session-Id & X-Hermes-Session-Key scoping',
    action: 'run',
  },
  {
    command: '/status',
    usage: '/status',
    category: 'hermes',
    summary: 'Report gateway link, active transport, tasks, and audio state',
    action: 'run',
  },
  {
    command: '/customize',
    usage: '/customize [notes]',
    category: 'workspace',
    summary: 'Open Settings & UI Studio drawer cleanly',
    action: 'run',
  },
  {
    command: '/ui',
    usage: '/ui <preset|reset|spec>',
    category: 'workspace',
    summary: 'Apply workspace preset or natural-language layout customization',
    action: 'insert',
  },
  {
    command: '/mood',
    usage: '/mood <neutral|happy|focused|alert|thinking>',
    category: 'workspace',
    summary: 'Set live Braille portrait expression state',
    action: 'insert',
  },
  {
    command: '/style',
    usage: '/style',
    category: 'workspace',
    summary: 'Confirm monochrome Braille halftone portrait mode',
    action: 'run',
  },
  {
    command: '/mute',
    usage: '/mute',
    category: 'workspace',
    summary: 'Toggle Hermes voice audio output',
    action: 'run',
  },
  {
    command: '/clear',
    usage: '/clear',
    category: 'workspace',
    summary: 'Clear local entries in the current session feed',
    action: 'run',
  },
  {
    command: '/help',
    usage: '/help',
    category: 'workspace',
    summary: 'Print complete Hermes Agent command reference',
    action: 'run',
  },
];

export const CATEGORY_LABELS: Record<SlashCommandCategory, string> = {
  session: 'Sessions',
  hermes: 'Hermes Agent',
  runs: 'Runs & Gates',
  workspace: 'Workspace',
};

/**
 * Determines whether the inline slash command palette should be visible above
 * the composer based on raw input text and explicit toggle state.
 */
export function shouldShowSlashPalette(input: string, forceOpen: boolean): boolean {
  const trimmedStart = input.trimStart();
  if (forceOpen) {
    if (!trimmedStart) return true;
    if (!trimmedStart.startsWith('/')) return false;
  } else {
    if (!trimmedStart.startsWith('/')) return false;
  }
  // Hide automatically once user has moved past the command token into arguments
  return !/^\/\S+\s/.test(trimmedStart);
}

/**
 * Filters and ranks slash commands matching the user's partial `/...` token.
 * Surfaces pending approval commands to the top when approvals await action.
 */
export function filterSlashCommands(
  input: string,
  opts?: { pendingApprovalCount?: number }
): SlashCommandSpec[] {
  const trimmedStart = input.trimStart();
  const rawToken = trimmedStart.startsWith('/')
    ? trimmedStart.slice(1).split(/\s+/)[0].toLowerCase()
    : '';
  const pendingCount = opts?.pendingApprovalCount ?? 0;

  if (!rawToken) {
    if (pendingCount <= 0) {
      return [...SLASH_COMMAND_CATALOG];
    }
    const urgentSet = new Set(['/approve', '/reject', '/brief']);
    return [
      ...SLASH_COMMAND_CATALOG.filter((item) => urgentSet.has(item.command)),
      ...SLASH_COMMAND_CATALOG.filter((item) => !urgentSet.has(item.command)),
    ];
  }

  const scored: Array<{ item: SlashCommandSpec; score: number }> = [];
  for (let i = 0; i < SLASH_COMMAND_CATALOG.length; i += 1) {
    const item = SLASH_COMMAND_CATALOG[i];
    const cmdName = item.command.slice(1).toLowerCase();
    const summaryLower = item.summary.toLowerCase();
    const catLower = item.category.toLowerCase();

    let score = 0;
    if (cmdName === rawToken) {
      score = 1000;
    } else if (cmdName.startsWith(rawToken)) {
      score = 700 - cmdName.length;
    } else if (cmdName.includes(rawToken)) {
      score = 450;
    } else if (summaryLower.includes(rawToken) || catLower.includes(rawToken)) {
      score = 200;
    }

    if (score > 0) {
      if (pendingCount > 0 && (item.command === '/approve' || item.command === '/brief')) {
        score += 50;
      }
      scored.push({ item, score: score - i * 0.01 });
    }
  }

  scored.sort((a, b) => b.score - a.score);
  return scored.map((entry) => entry.item);
}

/**
 * Resolves whether clicking or pressing Enter on a palette item should execute
 * immediately or insert the command prefix with a trailing space for arguments.
 */
export function resolvePaletteSelection(spec: SlashCommandSpec): {
  mode: 'run' | 'insert';
  value: string;
} {
  if (spec.action === 'run') {
    return { mode: 'run', value: spec.command };
  }
  return { mode: 'insert', value: `${spec.command} ` };
}
