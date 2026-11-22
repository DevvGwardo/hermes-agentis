import type {
  ApprovalCard,
  BackgroundTask,
  BriefDecision,
  ChannelId,
  ExpressionMood,
  HermesCapabilities,
  HermesCronJob,
  HermesProfileSummary,
  HermesSkill,
  HermesToolset,
  HermesTransportMode,
} from '../types/hermes';
import {
  CURATED_UI_PRESETS,
  DEFAULT_UI_CONFIG,
  getCuratedPreset,
  parseNaturalUiIntent,
  type UiCustomizationPatch,
} from './uiCustomization';

export interface CliContext {
  activeChannel: ChannelId;
  availableChannels?: ChannelId[];
  approvals: ApprovalCard[];
  tasks: BackgroundTask[];
  brief: BriefDecision[];
  isMuted: boolean;
  skills?: HermesSkill[];
  toolsets?: HermesToolset[];
  cronJobs?: HermesCronJob[];
  gatewayConnected?: boolean;
  gatewayUrl?: string;
  activeModel?: string;
  gatewayProfile?: string;
  profiles?: HermesProfileSummary[];
  draftProfile?: string;
  routePrefix?: string;
  healthStatus?: string;
  sessionKeyHeader?: string;
  capabilities?: HermesCapabilities | null;
  transportMode?: HermesTransportMode;
  resolvedTransport?: string;
  gatewayError?: string;
}

export type CliExecutionResult =
  | {
      type: 'handled';
      output: string;
      mood: ExpressionMood;
      spokenText: string;
      switchChannel?: ChannelId;
      switchProfile?: string;
      startNewSession?: boolean;
      newSessionPrompt?: string;
      approveCardId?: string;
      approveChoice?: 'once' | 'always' | 'deny';
      rejectCardId?: string;
      cancelActiveRun?: boolean;
      setTransportMode?: HermesTransportMode;
      toggleMute?: boolean;
      clearTerminal?: boolean;
      setMood?: ExpressionMood;
      setShader?: 'solid' | 'wire' | 'depth' | 'holo' | 'norm';
      setDitherStyle?: 'braille';
      uiPatch?: UiCustomizationPatch;
    }
  | {
      type: 'chat';
      prompt: string;
    };

export const AVAILABLE_COMMANDS = [
  '/help',
  '/new',
  '/profiles',
  '/profile',
  '/brief',
  '/tasks',
  '/skills',
  '/recall',
  '/context',
  '/toolsets',
  '/memory',
  '/cron',
  '/sessions',
  '/gateway',
  '/doctor',
  '/transport',
  '/channels',
  '/switch',
  '/resume',
  '/approve',
  '/reject',
  '/stop',
  '/ui',
  '/customize',
  '/mood',
  '/style',
  '/mute',
  '/status',
  '/clear',
] as const;

const VALID_MOODS: ExpressionMood[] = [
  'neutral',
  'listening',
  'thinking',
  'speaking',
  'happy',
  'focused',
  'alert',
];

const VALID_TRANSPORTS: Record<string, HermesTransportMode> = {
  auto: 'auto',
  runs: 'runs',
  run: 'runs',
  completions: 'chat_completions',
  chat_completions: 'chat_completions',
  'session-stream': 'session_stream',
  session_stream: 'session_stream',
};

/**
 * Pure parser and local executor for Hermes Agent slash commands.
 * Natural-language prompts (not starting with `/`) route to the active Hermes Agent session.
 */
export function executeCliInput(rawInput: string, ctx: CliContext): CliExecutionResult {
  const trimmed = rawInput.trim();
  if (!trimmed) {
    return {
      type: 'handled',
      output: 'Empty input. Type /help for Hermes Agent commands or speak naturally.',
      mood: 'neutral',
      spokenText: '',
    };
  }

  if (!trimmed.startsWith('/')) {
    return { type: 'chat', prompt: trimmed };
  }

  const [cmdRaw, ...args] = trimmed.split(/\s+/);
  const cmd = cmdRaw.toLowerCase();
  const argStr = args.join(' ').trim();
  const skills = ctx.skills ?? [];
  const toolsets = ctx.toolsets ?? [];
  const cronJobs = ctx.cronJobs ?? [];
  const availableChannels = ctx.availableChannels ?? [];

  switch (cmd) {
    case '/help': {
      return {
        type: 'handled',
        mood: 'happy',
        spokenText:
          'Hermes Agent command reference ready. Inspect capabilities, skills, toolsets, scheduled jobs, or active sessions.',
        output: [
          'HERMES AGENT // COMMAND REFERENCE',
          '  /new [prompt]          Start a fresh isolated Hermes Agent session',
          '  /profiles              List Hermes profiles for parallel sessions',
          '  /profile <slug>        Set default Hermes profile for new Home/Dot launches',
          '  /tasks                 List run & turn tasks per session',
          '  /skills                List skills from GET /v1/skills',
          '  /toolsets              List toolsets & tools from GET /v1/toolsets',
          '  /memory                Inspect X-Hermes-Session-Key memory scoping header',
          '  /cron                  List scheduled jobs from GET /api/jobs',
          '  /sessions              Show synced sessions from GET /api/sessions',
          '  /gateway | /doctor     Inspect /v1/capabilities, /api/model/options & /health',
          '  /transport [mode]      Switch transport (auto | runs | session-stream | completions)',
          '  /switch <session>      Switch active session lane',
          '  /resume <session>      Resume session lane by slug or session id',
          '  /approve [id] [choice] Approve pending run card (once | always | deny)',
          '  /reject [id]           Deny a pending run approval card',
          '  /stop                  Stop active Hermes Agent run (/v1/runs/{id}/stop)',
          '  /ui <spec|preset>      Customize client theme, layout, character & backdrop',
          '  /customize             Open the client UI Studio drawer',
          '  /mood <state>          Shift portrait (neutral|happy|focused|alert|thinking)',
          '  /mute                  Toggle Hermes voice audio output',
          '  /status                Report gateway link, transport, and active tasks',
          '  /clear                 Clear terminal feed',
        ].join('\n'),
      };
    }

    case '/new': {
      return {
        type: 'handled',
        mood: 'happy',
        startNewSession: true,
        newSessionPrompt: argStr || undefined,
        spokenText: argStr ? '' : 'New Hermes Agent session armed on Home.',
        output: argStr
          ? `Starting fresh Hermes Agent session for: "${argStr}"`
          : 'New Hermes Agent session armed on Home. Prompt anything to start.',
      };
    }

    case '/profiles':
    case '/profile': {
      const profiles = ctx.profiles ?? [];
      const currentDraft = ctx.draftProfile || ctx.gatewayProfile || 'default';
      if (!argStr) {
        if (profiles.length === 0) {
          return {
            type: 'handled',
            mood: 'neutral',
            spokenText: `Active profile is ${currentDraft}. Configure extra profiles in Queue -> Gateway & Tools.`,
            output: `HERMES PROFILES // active default="${currentDraft}"\n  Add parallel profiles in Queue -> Gateway & Tools or set HERMES_PROFILES.`,
          };
        }
        const lines = profiles.map((p) => {
          const route = p.mode === 'multiplexed' ? `${p.baseUrl}${p.routePrefix}` : p.baseUrl;
          const marker = p.id === currentDraft ? '[DRAFT]' : '       ';
          const state = p.connected ? 'READY' : 'OFFLINE';
          return `  ${marker} @${p.id.padEnd(14)} ${p.mode.padEnd(11)} ${state.padEnd(7)} · ${route}`;
        });
        return {
          type: 'handled',
          mood: 'focused',
          spokenText: `${profiles.length} Hermes profiles registered. Default launch profile is ${currentDraft}.`,
          output: ['HERMES AGENT // PARALLEL PROFILES', ...lines, '  Usage: /profile <slug>'].join('\n'),
        };
      }
      const normalized = argStr.replace(/^@/, '').trim().toLowerCase().replace(/[^a-z0-9_-]+/g, '-');
      const found = profiles.find((p) => p.id === normalized);
      if (profiles.length > 0 && !found) {
        return {
          type: 'handled',
          mood: 'alert',
          spokenText: `Profile ${normalized} is not registered.`,
          output: `Unknown profile "${normalized}". Available: ${profiles.map((p) => p.id).join(', ')}.\nAdd custom profiles in Queue -> Gateway & Tools.`,
        };
      }
      return {
        type: 'handled',
        mood: 'happy',
        switchProfile: normalized || 'default',
        spokenText: `Default profile for new parallel sessions set to ${normalized || 'default'}.`,
        output: `HERMES PROFILE // New Home & Dot sessions will launch on @${normalized || 'default'}.`,
      };
    }

    case '/brief': {
      const pendingApprovals = ctx.approvals.filter((a) => a.status === 'pending');
      const openBrief = ctx.brief.filter((b) => !b.resolved);
      if (pendingApprovals.length === 0 && openBrief.length === 0) {
        return {
          type: 'handled',
          mood: 'neutral',
          spokenText: 'All action gates clear. Zero pending approvals.',
          output: 'HERMES AGENT // ACTION QUEUE\n  All action gates clear. Zero pending run approvals.',
        };
      }
      const lines = [
        ...pendingApprovals.map(
          (a, i) =>
            `  ${i + 1}. [APPROVAL] ${a.title} (${a.mcpTool || 'tool'}) · run ${a.runId || a.id} -> /approve ${a.id} once`
        ),
        ...openBrief.map(
          (b, i) =>
            `  ${pendingApprovals.length + i + 1}. [${b.priority.toUpperCase()}] ${b.title}`
        ),
      ];
      return {
        type: 'handled',
        mood: 'focused',
        spokenText: `${pendingApprovals.length + openBrief.length} pending items in queue.`,
        output: ['HERMES AGENT // ACTION QUEUE', ...lines].join('\n'),
      };
    }

    case '/tasks': {
      if (ctx.tasks.length === 0) {
        return {
          type: 'handled',
          mood: 'neutral',
          spokenText: 'No turn tasks recorded yet.',
          output: 'HERMES AGENT // SESSION TASKS\n  No tasks recorded yet. Send a prompt to start a run.',
        };
      }
      const lines = ctx.tasks.map(
        (t) =>
          `  [${t.id}] ${t.status.toUpperCase().padEnd(9)} ${String(t.progress).padStart(3)}% · #${t.channelId} (${t.sessionId ?? `hermes-${t.channelId}`}) · ${t.title}`
      );
      return {
        type: 'handled',
        mood: 'focused',
        spokenText: `Tracking ${ctx.tasks.length} Hermes Agent runs across active sessions.`,
        output: ['HERMES AGENT // SESSION & RUN TASKS', ...lines].join('\n'),
      };
    }

    case '/skills': {
      if (!ctx.gatewayConnected && skills.length === 0) {
        return {
          type: 'handled',
          mood: 'alert',
          spokenText: 'Hermes Gateway is offline. Cannot query /v1/skills.',
          output: `HERMES AGENT // GET /v1/skills\n  Gateway offline (${ctx.gatewayError || ctx.gatewayUrl || 'http://127.0.0.1:8642'}).`,
        };
      }
      if (skills.length === 0) {
        return {
          type: 'handled',
          mood: 'neutral',
          spokenText: 'Gateway reported zero loaded skills.',
          output: 'HERMES AGENT // GET /v1/skills\n  0 skills reported by gateway.',
        };
      }
      const lines = skills.map(
        (s) =>
          `  ${s.enabled ? '[ON]' : '[--]'} ${s.name.padEnd(20)} ${s.source}${s.summary ? `\n       ${s.summary}` : ''}`
      );
      return {
        type: 'handled',
        mood: 'focused',
        spokenText: `${skills.filter((s) => s.enabled).length} skills loaded from GET /v1/skills.`,
        output: ['HERMES AGENT // GET /v1/skills', ...lines].join('\n'),
      };
    }

    case '/recall': {
      const arg = argStr.toLowerCase();
      const nextShow = arg === 'off' || arg === 'hide' || arg === 'close' ? false : true;
      return {
        type: 'handled',
        mood: 'focused',
        spokenText: nextShow
          ? 'Opened Jev uniichat-recall accuracy projection monitor.'
          : 'Closed Jev uniichat-recall accuracy projection monitor.',
        output: nextShow
          ? 'HERMES SKILL // uniichat-recall-jev\n  Jev recall accuracy monitor enabled (DevvGwardo/uniichat-recall · 93.3% hit@1 baseline · TypeSafe System One).'
          : 'HERMES SKILL // uniichat-recall-jev\n  Jev recall accuracy monitor hidden.',
        uiPatch: { showJevRecall: nextShow },
      };
    }

    case '/context': {
      const arg = argStr.toLowerCase();
      const nextShow = arg === 'off' || arg === 'hide' || arg === 'close' ? false : true;
      const model = ctx.activeModel || 'hermes-agent';
      return {
        type: 'handled',
        mood: 'focused',
        spokenText: nextShow
          ? `Context window meter enabled for ${model}.`
          : 'Context window meter hidden.',
        output: nextShow
          ? `HERMES AGENT // MODEL CONTEXT WINDOW\n  Active model: ${model}\n  Context meter enabled in workspace top bar.`
          : 'HERMES AGENT // MODEL CONTEXT WINDOW\n  Context meter hidden from top bar.',
        uiPatch: { showContextMeter: nextShow },
      };
    }

    case '/toolsets': {
      if (!ctx.gatewayConnected && toolsets.length === 0) {
        return {
          type: 'handled',
          mood: 'alert',
          spokenText: 'Hermes Gateway is offline. Cannot query /v1/toolsets.',
          output: `HERMES AGENT // GET /v1/toolsets\n  Gateway offline (${ctx.gatewayError || ctx.gatewayUrl || 'http://127.0.0.1:8642'}).`,
        };
      }
      if (toolsets.length === 0) {
        return {
          type: 'handled',
          mood: 'neutral',
          spokenText: 'Gateway reported zero toolsets.',
          output: 'HERMES AGENT // GET /v1/toolsets\n  0 toolsets reported by gateway.',
        };
      }
      const lines = toolsets.map((ts) => {
        const badge = ts.enabled ? (ts.configured === false ? '[UNCFG]' : '[ON]   ') : '[OFF]  ';
        const toolsPreview = ts.tools.length > 0 ? ts.tools.slice(0, 6).join(', ') : 'none';
        return `  ${badge} ${ts.label.padEnd(20)} (${ts.tools.length} tools: ${toolsPreview})`;
      });
      return {
        type: 'handled',
        mood: 'focused',
        spokenText: `${toolsets.filter((t) => t.enabled).length} active toolsets on gateway.`,
        output: ['HERMES AGENT // GET /v1/toolsets', ...lines].join('\n'),
      };
    }

    case '/memory': {
      const keyHeader = ctx.capabilities?.sessionKeyHeader || ctx.sessionKeyHeader || 'X-Hermes-Session-Key';
      const hasCaps = Boolean(ctx.capabilities);
      return {
        type: 'handled',
        mood: 'focused',
        spokenText: `Memory isolation scopes per session via ${keyHeader}.`,
        output: [
          'HERMES AGENT // MEMORY SCOPING',
          `  Continuity Header : X-Hermes-Session-Id (active channel: #${ctx.activeChannel})`,
          `  Scoping Header    : ${keyHeader}${hasCaps && !ctx.capabilities?.sessionKeyHeader ? ' (not advertised in /v1/capabilities)' : ''}`,
          '  REST Endpoint     : Hermes API Server has no /v1/memory endpoint; persistent memory is managed inside each session.',
        ].join('\n'),
      };
    }

    case '/cron': {
      if (!ctx.gatewayConnected && cronJobs.length === 0) {
        return {
          type: 'handled',
          mood: 'alert',
          spokenText: 'Hermes Gateway is offline. Cannot query /api/jobs.',
          output: `HERMES AGENT // GET /api/jobs\n  Gateway offline (${ctx.gatewayError || ctx.gatewayUrl || 'http://127.0.0.1:8642'}).`,
        };
      }
      if (cronJobs.length === 0) {
        return {
          type: 'handled',
          mood: 'neutral',
          spokenText: 'Zero scheduled jobs configured on gateway.',
          output: 'HERMES AGENT // GET /api/jobs\n  No scheduled jobs on gateway.',
        };
      }
      const lines = cronJobs.map(
        (c) =>
          `  [${c.schedule.padEnd(15)}] ${(c.status || c.mode).padEnd(8)} (${c.id}) · ${c.summary}`
      );
      return {
        type: 'handled',
        mood: 'focused',
        spokenText: `${cronJobs.length} scheduled jobs returned by /api/jobs.`,
        output: ['HERMES AGENT // GET /api/jobs', ...lines].join('\n'),
      };
    }

    case '/channels':
    case '/sessions':
    case '/lanes': {
      if (availableChannels.length === 0) {
        return {
          type: 'handled',
          mood: ctx.gatewayConnected ? 'neutral' : 'alert',
          spokenText: ctx.gatewayConnected
            ? 'No active gateway sessions found.'
            : 'Gateway offline; no sessions synced.',
          output: ctx.gatewayConnected
            ? 'HERMES AGENT // GET /api/sessions\n  No sessions yet. Use /new <prompt> to create one.'
            : `HERMES AGENT // GET /api/sessions\n  Gateway offline (${ctx.gatewayError || ctx.gatewayUrl || 'http://127.0.0.1:8642'}).`,
        };
      }
      const lines = availableChannels.map(
        (id) => `  #${id.padEnd(18)} ${id === ctx.activeChannel ? '[ACTIVE]' : ''}`
      );
      return {
        type: 'handled',
        mood: 'neutral',
        spokenText: `${availableChannels.length} synced Hermes sessions. Active lane is #${ctx.activeChannel}.`,
        output: [
          `ACTIVE SESSION LANE: #${ctx.activeChannel} (${ctx.sessionKeyHeader || 'X-Hermes-Session-Key'})`,
          ...lines,
        ].join('\n'),
      };
    }

    case '/gateway':
    case '/doctor': {
      const url = ctx.gatewayUrl || 'http://127.0.0.1:8642';
      const connected = Boolean(ctx.gatewayConnected);
      const model = ctx.activeModel || 'hermes-agent';
      const f = ctx.capabilities?.features;
      const capFlags = f
        ? [
            f.run_submission ? 'runs' : null,
            f.run_approval ? 'approvals' : null,
            f.session_chat_stream ? 'session-stream' : null,
            f.reasoning_streaming ? 'reasoning' : null,
          ]
            .filter(Boolean)
            .join(', ') || 'basic completions'
        : 'unavailable';
      return {
        type: 'handled',
        mood: connected ? 'happy' : 'alert',
        spokenText: connected
          ? `Hermes Gateway connected using ${ctx.resolvedTransport || 'chat_completions'} transport.`
          : 'Hermes Gateway is offline. Start hermes gateway with API_SERVER_ENABLED=true.',
        output: [
          'HERMES AGENT DOCTOR // GATEWAY CHECK',
          `  Gateway URL       : ${url}${ctx.routePrefix || ''} (${connected ? 'CONNECTED' : 'OFFLINE'})`,
          `  Profile           : ${ctx.gatewayProfile || 'default'} · Health (/health): ${ctx.healthStatus || (connected ? 'ok' : 'unreachable')}`,
          `  Capabilities      : ${capFlags} (GET /v1/capabilities)`,
          `  Active Model      : ${model} (GET /api/model/options)`,
          `  Transport Mode    : ${ctx.transportMode || 'auto'} -> resolved: ${ctx.resolvedTransport || 'chat_completions'}`,
          `  Session Headers   : X-Hermes-Session-Id + ${ctx.sessionKeyHeader || 'X-Hermes-Session-Key'}`,
          ...(ctx.gatewayError && !connected ? [`  Error             : ${ctx.gatewayError}`] : []),
        ].join('\n'),
      };
    }

    case '/transport': {
      const arg = argStr.toLowerCase();
      if (!arg) {
        return {
          type: 'handled',
          mood: 'focused',
          spokenText: `Active transport mode is ${ctx.transportMode || 'auto'}.`,
          output: `TRANSPORT MODE // current=${ctx.transportMode || 'auto'}, resolved=${ctx.resolvedTransport || 'chat_completions'}\n  Usage: /transport <auto|runs|session-stream|completions>`,
        };
      }
      const mapped = VALID_TRANSPORTS[arg];
      if (!mapped) {
        return {
          type: 'handled',
          mood: 'alert',
          spokenText: 'Invalid transport mode.',
          output: 'Usage: /transport <auto|runs|session-stream|completions>',
        };
      }
      return {
        type: 'handled',
        mood: 'happy',
        setTransportMode: mapped,
        spokenText: `Gateway transport preference set to ${mapped}.`,
        output: `TRANSPORT MODE // Preference updated to "${mapped}".`,
      };
    }

    case '/switch':
    case '/resume': {
      const normalized = argStr.replace(/^#/, '').replace(/^(hermes|duet)-/, '').toLowerCase() as ChannelId;
      if (availableChannels.length === 0) {
        return {
          type: 'handled',
          mood: 'alert',
          spokenText: 'No synced sessions available yet.',
          output: 'No sessions synced from gateway yet. Use /new <prompt> to start one.',
        };
      }
      if (!normalized || !availableChannels.includes(normalized)) {
        return {
          type: 'handled',
          mood: 'alert',
          spokenText: 'Unknown Hermes session.',
          output: `Unknown session "${argStr}". Synced sessions: ${availableChannels.join(', ')}.`,
        };
      }
      return {
        type: 'handled',
        mood: 'happy',
        switchChannel: normalized,
        spokenText: `Switched to session #${normalized}.`,
        output: `Resumed session #${normalized}.`,
      };
    }

    case '/approve': {
      const firstPending = ctx.approvals.find((c) => c.status === 'pending');
      const rawChoice = args[args.length - 1]?.toLowerCase();
      let choice: 'once' | 'always' | 'deny' = 'once';
      let targetArg = argStr;
      if (rawChoice === 'once' || rawChoice === 'always' || rawChoice === 'deny') {
        choice = rawChoice;
        targetArg = args.slice(0, -1).join(' ').trim();
      }
      const targetId = targetArg || firstPending?.id || '';
      const card = ctx.approvals.find((c) => c.id === targetId || c.requestId === targetId);
      if (!card) {
        return {
          type: 'handled',
          mood: 'alert',
          spokenText: 'No pending run approval found.',
          output: `No pending approval found matching "${targetArg || 'pending'}".`,
        };
      }
      if (card.status !== 'pending') {
        return {
          type: 'handled',
          mood: 'neutral',
          spokenText: `Approval ${card.id} is already ${card.status}.`,
          output: `Approval ${card.id} is already ${card.status}.`,
        };
      }
      return {
        type: 'handled',
        mood: choice === 'deny' ? 'neutral' : 'happy',
        approveCardId: card.id,
        approveChoice: choice,
        spokenText:
          choice === 'deny'
            ? `Denied ${card.title}.`
            : `Approved ${card.title} (${choice}).`,
        output: `RUN APPROVAL (${choice.toUpperCase()}) [${card.id}]: ${card.title}`,
      };
    }

    case '/reject': {
      const targetId = argStr || ctx.approvals.find((c) => c.status === 'pending')?.id || '';
      const card = ctx.approvals.find((c) => c.id === targetId || c.requestId === targetId);
      if (!card) {
        return {
          type: 'handled',
          mood: 'alert',
          spokenText: 'No matching approval found.',
          output: `No pending approval found matching "${argStr || 'pending'}".`,
        };
      }
      return {
        type: 'handled',
        mood: 'neutral',
        rejectCardId: card.id,
        approveChoice: 'deny',
        spokenText: `Denied ${card.title}.`,
        output: `DENIED [${card.id}]: ${card.title}`,
      };
    }

    case '/stop': {
      return {
        type: 'handled',
        mood: 'neutral',
        cancelActiveRun: true,
        spokenText: 'Stopping active run.',
        output: `STOP // Requesting cancellation for active run in #${ctx.activeChannel}.`,
      };
    }

    case '/mood': {
      const nextMood = argStr.toLowerCase() as ExpressionMood;
      if (!VALID_MOODS.includes(nextMood)) {
        return {
          type: 'handled',
          mood: 'alert',
          spokenText: 'Unsupported mood state.',
          output: `Valid moods: ${VALID_MOODS.join(', ')}.`,
        };
      }
      return {
        type: 'handled',
        mood: nextMood,
        setMood: nextMood,
        spokenText: `Expression matrix shifted to ${nextMood}.`,
        output: `Braille portrait expression set to "${nextMood}".`,
      };
    }

    case '/style': {
      const mode = argStr.toLowerCase();
      if (mode && mode !== 'braille') {
        return {
          type: 'handled',
          mood: 'alert',
          spokenText: 'Only braille halftone portrait mode is active.',
          output: 'Valid halftone portrait style: braille.',
        };
      }
      return {
        type: 'handled',
        mood: 'focused',
        setDitherStyle: 'braille',
        spokenText: 'Braille halftone portrait mode is active.',
        output: 'Monochrome halftone portrait mode: "BRAILLE".',
      };
    }

    case '/shader': {
      const mode = argStr.toLowerCase();
      if (mode !== 'solid' && mode !== 'wire' && mode !== 'depth' && mode !== 'holo' && mode !== 'norm') {
        return {
          type: 'handled',
          mood: 'alert',
          spokenText: 'Invalid shader. Choose solid, wire, depth, holo, or norm.',
          output: 'Valid 3D ASCII shaders: solid, wire, depth, holo, norm.',
        };
      }
      return {
        type: 'handled',
        mood: 'focused',
        setShader: mode,
        spokenText: `3D ASCII shader switched to ${mode} mode.`,
        output: `3D SDF ASCII shader mode set to "${mode.toUpperCase()}".`,
      };
    }

    case '/mute': {
      return {
        type: 'handled',
        mood: 'neutral',
        toggleMute: true,
        spokenText: ctx.isMuted ? 'Audio output unmuted.' : '',
        output: ctx.isMuted ? 'Audio output UNMUTED.' : 'Audio output MUTED.',
      };
    }

    case '/status': {
      const pendingCount = ctx.approvals.filter((a) => a.status === 'pending').length;
      const runningCount = ctx.tasks.filter((t) => t.status === 'running').length;
      return {
        type: 'handled',
        mood: ctx.gatewayConnected ? 'focused' : 'alert',
        spokenText: ctx.gatewayConnected
          ? `Connected to Hermes Gateway. ${pendingCount} pending approvals, ${runningCount} active tasks.`
          : 'Hermes Gateway is offline.',
        output: [
          `SESSION LANE: #${ctx.activeChannel}`,
          `GATEWAY: ${ctx.gatewayConnected ? `CONNECTED (${ctx.gatewayUrl || 'http://127.0.0.1:8642'}${ctx.routePrefix || ''})` : `OFFLINE (${ctx.gatewayError || ctx.gatewayUrl || 'http://127.0.0.1:8642'})`}`,
          `TRANSPORT: ${ctx.transportMode || 'auto'} -> ${ctx.resolvedTransport || 'chat_completions'}`,
          `PENDING APPROVALS: ${pendingCount}`,
          `RUNNING TASKS: ${runningCount}`,
          `VOICE AUDIO: ${ctx.isMuted ? 'MUTED' : 'ACTIVE'}`,
        ].join('\n'),
      };
    }

    case '/customize': {
      const maybePatch = argStr
        ? parseNaturalUiIntent(`make ui ${argStr}`, DEFAULT_UI_CONFIG) ?? { openCustomizer: true }
        : { openCustomizer: true };
      return {
        type: 'handled',
        mood: 'happy',
        uiPatch: maybePatch,
        spokenText: argStr
          ? 'Updated UI customization and opened the live studio.'
          : 'UI Customization Studio open. Adjust any theme, layout, or stage element live.',
        output: 'UI CUSTOMIZATION STUDIO // Opening client theme, layout, character & backdrop controls.',
      };
    }

    case '/ui': {
      const lowerArg = argStr.toLowerCase();
      if (!lowerArg || lowerArg === 'studio' || lowerArg === 'open') {
        return {
          type: 'handled',
          mood: 'happy',
          uiPatch: { openCustomizer: true },
          spokenText: 'UI Studio opened. Every theme, layout, and stage token is live.',
          output: 'UI STUDIO // Opened customization drawer. Try /ui cyber-violet, /ui reset, or natural language.',
        };
      }
      if (lowerArg === 'reset' || lowerArg === 'default' || lowerArg === 'restore') {
        return {
          type: 'handled',
          mood: 'happy',
          uiPatch: { resetAll: true },
          spokenText: 'Restored default Slate Monochrome layout and backdrop.',
          output: 'UI RESET // Restored default Slate Monochrome theme, layout, character & backdrop.',
        };
      }
      const preset = getCuratedPreset(lowerArg);
      if (preset) {
        return {
          type: 'handled',
          mood: preset.mood ?? 'happy',
          uiPatch: { presetId: preset.id },
          spokenText: `Applied ${preset.name} UI preset across the workspace.`,
          output: `UI PRESET // Applied "${preset.name}" (${preset.subtitle}).`,
        };
      }
      const parsed =
        parseNaturalUiIntent(argStr, DEFAULT_UI_CONFIG) ??
        parseNaturalUiIntent(`make ui ${argStr}`, DEFAULT_UI_CONFIG);
      if (parsed) {
        return {
          type: 'handled',
          mood: 'happy',
          uiPatch: parsed,
          spokenText: 'Applied your requested UI customization across the workspace.',
          output: `UI UPDATED // ${Object.keys(parsed).join(', ')}`,
        };
      }
      const presetIds = CURATED_UI_PRESETS.map((p) => p.id).join(', ');
      return {
        type: 'handled',
        mood: 'alert',
        spokenText: 'Specify a UI preset, reset, or customization instructions.',
        output: `Usage: /ui <preset|reset|instructions>\nPresets: ${presetIds}\nExample: /ui violet mono sharp right sidebar braille rain`,
      };
    }

    case '/clear': {
      return {
        type: 'handled',
        mood: 'neutral',
        clearTerminal: true,
        spokenText: '',
        output: 'Terminal buffer cleared.',
      };
    }

    default: {
      return {
        type: 'handled',
        mood: 'alert',
        spokenText: `Unrecognized command ${cmd}. Type slash help for Hermes Agent commands.`,
        output: `Unknown command "${cmd}". Type /help to list available Hermes Agent commands.`,
      };
    }
  }
}
