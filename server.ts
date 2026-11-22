import dotenv from "dotenv";
import express from "express";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { createServer as createHttpServer } from "node:http";
import { fileURLToPath } from "node:url";
import {
  applyStreamingDeltaToSteps,
  applyToolProgressToSteps,
  deriveTurnTaskTitle,
  finalizeTurnTaskSteps,
} from "./src/utils/taskInsights.ts";
import {
  ChatCompletionsStreamParser,
  RunsEventStreamParser,
  SessionChatStreamParser,
  buildGatewayUrl,
  chooseGatewayTransport,
  classifyGatewaySessionOrigin,
  createIdempotencyKey,
  extractModelOptions,
  fetchWithConcurrentRetry,
  mapGatewayJobs,
  mapGatewaySessionMessages,
  mapGatewaySessions,
  mapGatewaySkills,
  mapGatewayToolsets,
  parseCapabilitiesPayload,
  parseChatCompletionsJson,
  resolveProfileGatewayConfig,
  sanitizeSessionKeyHeader,
  type TerminalTurnOutcome,
} from "./src/utils/hermesProtocol.ts";
import {
  buildProfileCatalog,
  isValidCustomProfileSlug,
  isValidHttpUrl,
  normalizeBaseUrl,
  normalizeProfileId,
  toRedactedProfileSummary,
  type HermesProfileDefinition,
  type HermesProfileRuntime,
} from "./src/utils/profileRegistry.ts";
import type {
  HermesCapabilities,
  HermesGatewayStatus,
  HermesProfileMode,
  HermesSessionOrigin,
  HermesToolset,
  HermesTransportMode,
} from "./src/types/hermes.ts";
import {
  applyGitPullUpdate,
  buildAppUpdateStatus,
  createPreferencesVaultStore,
  fetchUpstreamReleaseInfo,
  isCommitAncestorOfHead,
  readLocalBuildMetadata,
  readPackageVersion,
  resolveVoxcellDataDir,
  type UpstreamReleaseInfo,
} from "./src/utils/appUpdater.ts";
import {
  normalizePreferencesSnapshot,
  summarizePreservedCategories,
  type UpdateStepResult,
} from "./src/utils/settingsSnapshot.ts";
import {
  JEV_CONTEXT_LIMITS,
  analyzeSessionEntries,
  buildCalibratedBaselineEvaluation,
  buildTypeSafeJevRequestPayload,
  parseTypeSafeJevResponse,
  type UniichatRetrievalMode,
} from "./src/utils/jevRecallEngine.ts";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
dotenv.config({ path: path.join(__dirname, ".env") });
const VOXCELL_DATA_DIR = resolveVoxcellDataDir(__dirname, process.env);
const preferencesStore = createPreferencesVaultStore(
  VOXCELL_DATA_DIR,
  readPackageVersion(__dirname)
);
let cachedUpstreamRelease: { info: UpstreamReleaseInfo; fetchedAt: number } | null = null;
const UPSTREAM_CACHE_TTL_MS = 30_000;

function parseArgFlag(name: string): string | undefined {
  const args = process.argv.slice(2);
  for (let i = 0; i < args.length; i++) {
    const arg = args[i];
    if (arg === `--${name}` && args[i + 1]) return args[i + 1];
    if (arg.startsWith(`--${name}=`)) return arg.slice(name.length + 3);
  }
  return undefined;
}

const PORT = Number(parseArgFlag("port") || process.env.PORT || 3000);
const HOST = parseArgFlag("host") || "0.0.0.0";
// Identifies THIS process in /api/health so the boot self-probe can tell a
// shadowing listener (Docker forward, an old instance) from this server.
const BOOT_ID = Math.random().toString(36).slice(2, 8);
const HERMES_CHAT_TIMEOUT_MS = Number(process.env.HERMES_CHAT_TIMEOUT_MS || 300_000);

function getGatewayConfig(profileId?: string | null) {
  if (!profileId || !profileId.trim()) {
    return resolveProfileGatewayConfig(process.env);
  }
  const clean = normalizeProfileId(profileId);
  const runtimes = listAllProfileRuntimes();
  const match = runtimes.find((r) => r.id === clean);
  if (match) {
    return {
      baseUrl: match.baseUrl,
      profile: match.id,
      apiKey: match.apiKey,
      routePrefix: match.routePrefix,
    };
  }
  return resolveProfileGatewayConfig({
    ...process.env,
    HERMES_PROFILE: clean,
  });
}

// ---------------------------------------------------------------------------
// Live Hermes Agent Gateway state (zero simulated channels/tasks/cards)
// ---------------------------------------------------------------------------
interface HermesCard {
  type: "approval" | "clarify";
  title?: string;
  body?: string;
  status: "pending" | "approved" | "declined";
  draft?: string;
  action?: string;
  destination?: string;
  requestId?: string;
  executionGeneration?: number;
  choices?: string[];
  command?: string;
}

interface HermesItem {
  id: string;
  kind: "user" | "assistant" | "card" | "activity" | "system";
  created_at: number;
  updated_at?: number;
  channel_id?: string;
  text?: string;
  streaming?: boolean;
  run_id?: string | null;
  error_code?: string;
  terminal_outcome?: TerminalTurnOutcome;
  card?: HermesCard;
  activity?: {
    run_id: string;
    status: string;
    kind?: "reasoning" | "commentary" | "tool" | "subagent" | "status";
    summary?: string;
    detail?: string;
    error?: string;
    steps?: { name: string; label?: string; status: string }[];
  };
}

interface HermesTask {
  id: string;
  title: string;
  status: string;
  error_code?: string;
  kind?: string;
  background?: boolean;
  handover_id?: string;
  session_id?: string;
  profile?: string;
  run_id?: string;
  progress?: string;
  steps?: { text: string; done?: boolean; active?: boolean }[];
  result?: string;
  channel_id?: string;
  agent_done_at?: number;
  confirmed_at?: number;
  created_at: number;
  updated_at?: number;
}

interface HermesChannel {
  id: string;
  name: string;
  session_id: string;
  session_key: string;
  profile?: string | null;
  topic?: string | null;
  persona?: string | null;
  source?: string | null;
  model?: string | null;
  origin?: HermesSessionOrigin;
  message_count?: number | null;
  last_message?: string | null;
  archived?: boolean;
  last_at?: number | null;
}

type SessionRuntimeStatus = "idle" | "thinking" | "speaking" | "running";

const channels: HermesChannel[] = [];
const pendingCards: HermesItem[] = [];
const tasks: HermesTask[] = [];
const items: HermesItem[] = [];

const sessionStatusMap = new Map<string, SessionRuntimeStatus>();
const activeRunAborts = new Map<string, AbortController>();
const activeGatewayRunIds = new Map<string, string>();
const runProfileMap = new Map<string, string>();
const workspaceSessionIds = new Set<string>();
const workspaceChannelIds = new Set<string>();

const customProfileDefs = new Map<string, HermesProfileDefinition>();
const cachedCapabilitiesByProfile = new Map<string, HermesCapabilities | null>();
const cachedGatewayStatusByProfile = new Map<string, HermesGatewayStatus>();

const PROFILE_MARKER_FILES = [
  "config.yaml",
  "profile.yaml",
  ".env",
  "SOUL.md",
  "auth.json",
  "state.db",
];

function discoverLocalHermesProfiles(): HermesProfileDefinition[] {
  const out: HermesProfileDefinition[] = [];
  try {
    const hermesHomeRoot = process.env.HERMES_HOME_ROOT || path.join(os.homedir(), ".hermes");
    const profilesDir = path.join(hermesHomeRoot, "profiles");
    if (!fs.existsSync(profilesDir)) return out;
    const entries = fs.readdirSync(profilesDir, { withFileTypes: true });
    for (const ent of entries) {
      if (!ent.isDirectory()) continue;
      const slug = normalizeProfileId(ent.name);
      if (!slug || slug === "default") continue;
      const dirPath = path.join(profilesDir, ent.name);
      const hasMarker = PROFILE_MARKER_FILES.some((f) => fs.existsSync(path.join(dirPath, f)));
      if (!hasMarker) continue;
      out.push({
        id: slug,
        label: ent.name,
        mode: "multiplexed",
        source: "discovered",
        description: `~/.hermes/profiles/${ent.name}`,
      });
    }
  } catch {
    // ignore inaccessible home directory in sandboxed containers
  }
  return out;
}

function listAllProfileRuntimes(): HermesProfileRuntime[] {
  return buildProfileCatalog({
    env: process.env,
    discoveredProfiles: discoverLocalHermesProfiles(),
    customProfiles: Array.from(customProfileDefs.values()),
  }).runtimes;
}

function normalizeChannelKey(raw?: string | null): string {
  return String(raw ?? "")
    .replace(/^(chan|hermes|duet)-/i, "")
    .toLowerCase()
    .trim()
    .replace(/[^a-z0-9-_]+/g, "-")
    .replace(/^-+|-+$/g, "");
}

function defaultSessionKeyForChannel(channelId: string): string {
  return sanitizeSessionKeyHeader(`voxcell-${channelId}`) || `voxcell-session`;
}

function resolveChannelOrigin(opts: {
  sessionId?: string | null;
  sessionKey?: string | null;
  channelId?: string | null;
  source?: string | null;
  existingOrigin?: HermesSessionOrigin;
  explicitOrigin?: HermesSessionOrigin;
}): HermesSessionOrigin {
  if (opts.explicitOrigin) return opts.explicitOrigin;
  const createdInWorkspace =
    (opts.sessionId ? workspaceSessionIds.has(opts.sessionId) : false) ||
    (opts.channelId ? workspaceChannelIds.has(opts.channelId) : false);
  if (createdInWorkspace) return "workspace";
  if (opts.existingOrigin) return opts.existingOrigin;
  return classifyGatewaySessionOrigin({
    sessionId: opts.sessionId,
    sessionKey: opts.sessionKey,
    channelId: opts.channelId,
    source: opts.source,
    createdInWorkspace: false,
  });
}

function upsertLocalChannel(opts: {
  channelId?: string;
  name?: string;
  sessionId: string;
  sessionKey?: string;
  profile?: string | null;
  topic?: string | null;
  persona?: string | null;
  source?: string | null;
  model?: string | null;
  origin?: HermesSessionOrigin;
  messageCount?: number | null;
  lastMessage?: string | null;
  lastAt?: number | null;
}): HermesChannel {
  const existingBySid = channels.find((c) => c.session_id === opts.sessionId);
  const cleanId =
    existingBySid?.id ||
    normalizeChannelKey(opts.channelId || opts.sessionId) ||
    "session";

  const resolvedProfile =
    opts.profile && opts.profile.trim()
      ? normalizeProfileId(opts.profile)
      : undefined;

  let existing = existingBySid || channels.find((c) => c.id === cleanId);
  if (!existing) {
    const resolvedOrigin = resolveChannelOrigin({
      sessionId: opts.sessionId,
      sessionKey: opts.sessionKey,
      channelId: cleanId,
      source: opts.source,
      explicitOrigin: opts.origin,
    });
    const resolvedKey =
      sanitizeSessionKeyHeader(opts.sessionKey) ||
      (resolvedOrigin === "workspace" ? defaultSessionKeyForChannel(cleanId) : "");
    if (resolvedOrigin === "workspace") {
      workspaceSessionIds.add(opts.sessionId);
      workspaceChannelIds.add(cleanId);
    }
    existing = {
      id: cleanId,
      name: (opts.name || opts.topic || cleanId).replace(/^#/, ""),
      session_id: opts.sessionId,
      session_key: resolvedKey,
      profile: resolvedProfile ?? null,
      topic: opts.topic ?? null,
      persona: opts.persona ?? null,
      source: opts.source ?? null,
      model: opts.model ?? null,
      origin: resolvedOrigin,
      message_count: opts.messageCount ?? null,
      last_message: opts.lastMessage ?? null,
      last_at: opts.lastAt ?? Date.now(),
    };
    channels.push(existing);
  } else {
    if (opts.name) existing.name = opts.name.replace(/^#/, "");
    if (opts.sessionId) existing.session_id = opts.sessionId;
    if (resolvedProfile) existing.profile = resolvedProfile;
    if (opts.topic !== undefined && opts.topic !== null) existing.topic = opts.topic;
    if (opts.persona !== undefined && opts.persona !== null) existing.persona = opts.persona;
    if (opts.source !== undefined) existing.source = opts.source;
    if (opts.model !== undefined) existing.model = opts.model;
    existing.origin = resolveChannelOrigin({
      sessionId: existing.session_id,
      sessionKey: opts.sessionKey ?? existing.session_key,
      channelId: existing.id,
      source: existing.source,
      existingOrigin: existing.origin,
      explicitOrigin: opts.origin,
    });
    if (opts.sessionKey) {
      existing.session_key =
        sanitizeSessionKeyHeader(opts.sessionKey) || existing.session_key;
    } else if (existing.origin === "workspace" && !existing.session_key) {
      existing.session_key = defaultSessionKeyForChannel(existing.id);
    }
    if (existing.origin === "workspace") {
      workspaceSessionIds.add(existing.session_id);
      workspaceChannelIds.add(existing.id);
    }
    if (opts.messageCount !== undefined && opts.messageCount !== null) {
      existing.message_count = opts.messageCount;
    }
    if (opts.lastMessage !== undefined && opts.lastMessage !== null) {
      existing.last_message = opts.lastMessage;
    }
    if (opts.lastAt) existing.last_at = Math.max(existing.last_at ?? 0, opts.lastAt);
  }
  return existing;
}

function buildSessionSnapshot(ch: HermesChannel) {
  const chItems = items.filter((it) => (it.channel_id || ch.id) === ch.id);
  const localTurns = chItems.filter((it) => it.kind === "user").length;
  const turnCount =
    localTurns > 0
      ? localTurns
      : typeof ch.message_count === "number" && ch.message_count > 0
      ? Math.max(1, Math.ceil(ch.message_count / 2))
      : 0;
  const lastMsg = [...chItems]
    .reverse()
    .find((it) => (it.kind === "assistant" || it.kind === "user") && it.text?.trim());
  const latestTimestamp = chItems.reduce(
    (acc, it) => Math.max(acc, it.created_at),
    ch.last_at ?? Date.now()
  );
  const hasRunningTask = tasks.some(
    (t) => t.channel_id === ch.id && (t.status === "running" || t.status === "waiting")
  );
  const explicitStatus = sessionStatusMap.get(ch.id);
  const status = explicitStatus ?? (hasRunningTask ? "running" : "idle");
  const resolvedOrigin = resolveChannelOrigin({
    sessionId: ch.session_id,
    sessionKey: ch.session_key,
    channelId: ch.id,
    source: ch.source,
    existingOrigin: ch.origin,
  });

  return {
    sessionId: ch.session_id,
    sessionKey: ch.session_key,
    channelId: ch.id,
    name: ch.name,
    persona: ch.persona || "",
    topic: ch.topic || ch.name,
    profile: ch.profile || getGatewayConfig().profile,
    status,
    turnCount,
    lastMessage: lastMsg?.text?.trim() || ch.last_message || ch.topic || "",
    updatedAtMs: latestTimestamp,
    source: ch.source ?? undefined,
    model: ch.model ?? undefined,
    origin: resolvedOrigin,
    ownedByApiServer: resolvedOrigin === "workspace",
  };
}

function buildAllSessionSnapshots() {
  return channels.filter((c) => !c.archived).map(buildSessionSnapshot);
}

const sseClients = new Set<express.Response>();

function broadcastEvent(event: Record<string, unknown>) {
  const payload = `data: ${JSON.stringify(event)}\n\n`;
  for (const client of sseClients) {
    try {
      client.write(payload);
    } catch {
      sseClients.delete(client);
    }
  }
}

// ---------------------------------------------------------------------------
// Direct Hermes Agent Gateway HTTP helpers
// ---------------------------------------------------------------------------
async function fetchHermesGateway(
  pathname: string,
  init?: RequestInit & { unprefixed?: boolean },
  timeoutMs = 3000,
  externalSignal?: AbortSignal,
  profileId?: string | null
): Promise<Response> {
  const cfg = getGatewayConfig(profileId);
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  const onExternalAbort = () => controller.abort();
  externalSignal?.addEventListener("abort", onExternalAbort, { once: true });

  const headers: Record<string, string> = {
    "Content-Type": "application/json",
    ...(init?.headers as Record<string, string> | undefined),
  };
  if (cfg.apiKey && !headers.Authorization) {
    headers.Authorization = `Bearer ${cfg.apiKey}`;
  }

  const targetUrl = buildGatewayUrl(cfg, pathname, { unprefixed: init?.unprefixed });
  try {
    return await fetch(targetUrl, {
      ...init,
      headers,
      signal: controller.signal,
    });
  } finally {
    clearTimeout(timer);
    externalSignal?.removeEventListener("abort", onExternalAbort);
  }
}

async function probeHermesGateway(profileId?: string | null): Promise<HermesGatewayStatus> {
  const cfg = getGatewayConfig(profileId);
  const checkedAtMs = Date.now();

  let connected = false;
  let capabilities: HermesCapabilities | null = null;
  let healthStatus: string | undefined;
  let models: string[] = [];
  let activeModel = process.env.API_SERVER_MODEL_NAME || "";
  let activeProvider: string | undefined;
  let providers: Array<{ id: string; label: string; configured?: boolean }> = [];
  let skillsCount = 0;
  let toolsets: HermesToolset[] = [];
  let errorMsg: string | undefined;

  // 1. Primary probe: GET /v1/capabilities
  try {
    const capRes = await fetchHermesGateway(
      "/v1/capabilities",
      { method: "GET" },
      1800,
      undefined,
      cfg.profile
    );
    if (capRes.ok) {
      connected = true;
      const body = await capRes.json().catch(() => null);
      capabilities = parseCapabilitiesPayload(body);
      if (capabilities) {
        cachedCapabilitiesByProfile.set(cfg.profile, capabilities);
        if (!activeModel && capabilities.model) {
          activeModel = capabilities.model;
        }
      }
    } else if (capRes.status === 401 || capRes.status === 403) {
      errorMsg =
        cfg.profile !== "default"
          ? `Gateway responded HTTP ${capRes.status} for profile "${cfg.profile}" (check HERMES_PROFILE_API_KEY)`
          : `Gateway responded HTTP ${capRes.status} (check HERMES_API_KEY)`;
    } else {
      errorMsg = `Capabilities check returned HTTP ${capRes.status}`;
    }
  } catch (err) {
    errorMsg = err instanceof Error ? err.message : String(err);
  }

  // 2. Documented liveness probe: GET /health (never /v1/health)
  try {
    const hRes = await fetchHermesGateway(
      "/health",
      { method: "GET", unprefixed: cfg.routePrefix === "" },
      1200,
      undefined,
      cfg.profile
    );
    if (hRes.ok) {
      const hJson = (await hRes.json().catch(() => null)) as { status?: string } | null;
      healthStatus = hJson?.status || "ok";
      if (!connected && !errorMsg) {
        connected = true;
      }
    } else if (!connected && cfg.routePrefix) {
      const rootHealthRes = await fetchHermesGateway(
        "/health",
        { method: "GET", unprefixed: true },
        1200,
        undefined,
        cfg.profile
      ).catch(() => null);
      if (rootHealthRes?.ok) {
        const hj = (await rootHealthRes.json().catch(() => null)) as { status?: string } | null;
        healthStatus = hj?.status || "ok";
      }
    }
  } catch {
    // ignore secondary health error when capabilities already reported status
  }

  // 3. Model picker inventory: GET /api/model/options primary, /v1/models fallback
  if (connected) {
    try {
      const optRes = await fetchHermesGateway(
        "/api/model/options",
        { method: "GET" },
        2000,
        undefined,
        cfg.profile
      );
      if (optRes.ok) {
        const parsed = extractModelOptions(await optRes.json().catch(() => null));
        models = parsed.models;
        providers = parsed.providers;
        if (!activeModel && parsed.activeModel) activeModel = parsed.activeModel;
        if (parsed.activeProvider) activeProvider = parsed.activeProvider;
      }
    } catch {
      // fall back to /v1/models
    }

    if (models.length === 0) {
      try {
        const mRes = await fetchHermesGateway(
          "/v1/models",
          { method: "GET" },
          1500,
          undefined,
          cfg.profile
        );
        if (mRes.ok) {
          const mj = (await mRes.json().catch(() => null)) as {
            data?: Array<{ id?: string }>;
            models?: Array<{ id?: string } | string>;
          } | null;
          if (Array.isArray(mj?.data)) {
            models = mj.data.map((m) => m.id).filter((id): id is string => Boolean(id));
          } else if (Array.isArray(mj?.models)) {
            models = mj.models
              .map((m) => (typeof m === "string" ? m : m.id))
              .filter((id): id is string => Boolean(id));
          }
          if (!activeModel && models[0]) activeModel = models[0];
        }
      } catch {
        // ignore
      }
    }

    // 4. Deterministic skills & toolsets discovery
    const [skillsRes, toolsetsRes] = await Promise.all([
      fetchHermesGateway("/v1/skills", { method: "GET" }, 1800, undefined, cfg.profile).catch(
        () => null
      ),
      fetchHermesGateway("/v1/toolsets", { method: "GET" }, 1800, undefined, cfg.profile).catch(
        () => null
      ),
    ]);
    if (skillsRes?.ok) {
      const mapped = mapGatewaySkills(await skillsRes.json().catch(() => null));
      skillsCount = mapped.length;
    }
    if (toolsetsRes?.ok) {
      toolsets = mapGatewayToolsets(await toolsetsRes.json().catch(() => null));
    }
  } else {
    cachedCapabilitiesByProfile.set(cfg.profile, null);
  }

  const resolvedModel = activeModel || models[0] || "hermes-agent";
  const resolvedTransport = chooseGatewayTransport("auto", capabilities);

  const status: HermesGatewayStatus = {
    connected,
    mode: connected ? "hermes-gateway" : "local-standby",
    gatewayUrl: cfg.baseUrl,
    routePrefix: cfg.routePrefix,
    profile: cfg.profile,
    activeModel: resolvedModel,
    activeProvider,
    models,
    providers,
    sessionHeader: "X-Hermes-Session-Id",
    sessionKeyHeader: capabilities?.sessionKeyHeader || "X-Hermes-Session-Key",
    skillsCount,
    toolsets,
    capabilities,
    resolvedTransport,
    healthStatus,
    checkedAtMs,
    ...(errorMsg && !connected ? { error: errorMsg } : {}),
  };

  cachedGatewayStatusByProfile.set(cfg.profile, status);
  return status;
}

async function syncSessionsFromGateway(): Promise<void> {
  const runtimes = listAllProfileRuntimes();
  const primaryProfile = getGatewayConfig().profile;
  const profilesToSync = runtimes.filter(
    (r) => r.id === primaryProfile || r.source === "custom" || r.source === "env"
  );

  const syncedIdsByProfile = new Map<string, Set<string>>();

  await Promise.all(
    profilesToSync.map(async (rt) => {
      try {
        const res = await fetchHermesGateway(
          "/api/sessions?limit=100&include_children=false",
          { method: "GET" },
          2000,
          undefined,
          rt.id
        );
        if (!res.ok) return;
        const raw = await res.json().catch(() => null);
        const gwSessions = mapGatewaySessions(raw);
        const idSet = new Set<string>(gwSessions.map((s) => s.id));
        syncedIdsByProfile.set(rt.id, idSet);

        for (const s of gwSessions) {
          upsertLocalChannel({
            sessionId: s.id,
            sessionKey: s.sessionKey,
            profile: rt.id,
            name: s.title || s.id,
            topic: s.title || s.id,
            source: s.source ?? null,
            model: s.model ?? null,
            messageCount: s.messageCount ?? null,
            lastMessage: s.lastMessage ?? null,
            lastAt: s.updatedAtMs ?? s.createdAtMs ?? null,
          });
        }
      } catch {
        // gateway offline or /api/sessions unavailable for this profile
      }
    })
  );

  // Drop stale local entries only for profiles that responded cleanly
  for (let i = channels.length - 1; i >= 0; i--) {
    const ch = channels[i];
    const chProfile = ch.profile || primaryProfile;
    const activeIds = syncedIdsByProfile.get(chProfile);
    if (activeIds && !activeIds.has(ch.session_id) && !activeRunAborts.has(ch.id)) {
      channels.splice(i, 1);
    }
  }
}

async function fetchGatewayContext(profileId?: string | null) {
  const status = await probeHermesGateway(profileId);
  if (!status.connected) {
    return {
      connected: false,
      profile: status.profile,
      skills: [],
      toolsets: [],
      cronJobs: [],
      sessionKeyHeader: status.sessionKeyHeader || "X-Hermes-Session-Key",
      error: status.error || "Gateway unreachable",
    };
  }

  const [skillsRes, toolsetsRes, jobsRes] = await Promise.all([
    fetchHermesGateway("/v1/skills", { method: "GET" }, 2000, undefined, status.profile).catch(
      () => null
    ),
    fetchHermesGateway("/v1/toolsets", { method: "GET" }, 2000, undefined, status.profile).catch(
      () => null
    ),
    fetchHermesGateway("/api/jobs", { method: "GET" }, 2000, undefined, status.profile).catch(
      () => null
    ),
  ]);

  const skills = skillsRes?.ok
    ? mapGatewaySkills(await skillsRes.json().catch(() => null))
    : [];
  const toolsets = toolsetsRes?.ok
    ? mapGatewayToolsets(await toolsetsRes.json().catch(() => null))
    : status.toolsets;
  const cronJobs = jobsRes?.ok
    ? mapGatewayJobs(await jobsRes.json().catch(() => null))
    : [];

  return {
    connected: true,
    profile: status.profile,
    skills,
    toolsets,
    cronJobs,
    sessionKeyHeader: status.sessionKeyHeader || "X-Hermes-Session-Key",
  };
}

// ---------------------------------------------------------------------------
// Gateway Turn Execution (Runs API / Chat Completions / Session Chat Stream)
// ---------------------------------------------------------------------------
interface ExecuteTurnParams {
  message: string;
  channel: HermesChannel;
  runIdLocal: string;
  turnTask: HermesTask;
  transportMode?: HermesTransportMode;
  idempotencyKey: string;
  abortSignal: AbortSignal;
  model?: string;
  provider?: string;
}

interface TurnExecutionOutcome {
  reply: string;
  reasoning: string;
  outcome: TerminalTurnOutcome;
  errorCode?: string;
  error?: string;
  gatewayRunId?: string;
}

function recordReasoningActivity(params: {
  channel: HermesChannel;
  runIdLocal: string;
  accumulatedReasoning: string;
  reasoningItemIdRef: { current: string | null };
  streaming: boolean;
}) {
  const { channel, runIdLocal, accumulatedReasoning, reasoningItemIdRef, streaming } = params;
  const trimmed = accumulatedReasoning.trim();
  if (!trimmed) return;

  if (!reasoningItemIdRef.current) {
    reasoningItemIdRef.current = `reason-${runIdLocal}`;
    const item: HermesItem = {
      id: reasoningItemIdRef.current,
      kind: "activity",
      created_at: Date.now(),
      channel_id: channel.id,
      run_id: runIdLocal,
      streaming,
      text: `Reasoning: ${trimmed}`,
      activity: {
        run_id: runIdLocal,
        status: streaming ? "thinking" : "done",
        kind: "reasoning",
        summary: "Reasoning",
        detail: trimmed,
      },
    };
    items.push(item);
    broadcastEvent({ type: "item", item });
  } else {
    const existing = items.find((it) => it.id === reasoningItemIdRef.current);
    if (existing) {
      existing.text = `Reasoning: ${trimmed}`;
      existing.streaming = streaming;
      existing.updated_at = Date.now();
      existing.activity = {
        run_id: runIdLocal,
        status: streaming ? "thinking" : "done",
        kind: "reasoning",
        summary: "Reasoning",
        detail: trimmed,
      };
      broadcastEvent({ type: "item", item: existing });
    }
  }
}

function recordToolOrSubagentActivity(params: {
  channel: HermesChannel;
  runIdLocal: string;
  turnTask: HermesTask;
  seqRef: { current: number };
  toolName: string;
  label: string;
  status: string;
  summary: string;
  kind?: "tool" | "subagent" | "commentary";
}) {
  const { channel, runIdLocal, turnTask, seqRef, toolName, label, status, summary, kind = "tool" } =
    params;
  const normalizedStatus = (status || "running").toLowerCase();
  const isGenericRunning = /^running\s+/i.test(summary.trim());
  const toolKey = toolName.trim().toLowerCase();

  const existingIdx = items.findIndex(
    (it) =>
      it.kind === "activity" &&
      it.run_id === runIdLocal &&
      it.activity?.kind === kind &&
      it.activity?.steps?.[0]?.name?.toLowerCase() === toolKey &&
      (it.activity?.steps?.[0]?.status === "running" ||
        /^running\s+/i.test((it.text ?? "").trim()))
  );

  if (existingIdx >= 0 && (!isGenericRunning || normalizedStatus !== "running")) {
    const existing = items[existingIdx];
    existing.text = summary;
    existing.updated_at = Date.now();
    existing.activity = {
      run_id: runIdLocal,
      status,
      kind,
      summary,
      steps: [{ name: toolName, label, status }],
    };
    broadcastEvent({ type: "item", item: existing });
  } else {
    seqRef.current += 1;
    const activityItem: HermesItem = {
      id: `act-${runIdLocal}-${seqRef.current}`,
      kind: "activity",
      created_at: Date.now(),
      channel_id: channel.id,
      run_id: runIdLocal,
      text: summary,
      activity: {
        run_id: runIdLocal,
        status,
        kind,
        summary,
        steps: [{ name: toolName, label, status }],
      },
    };
    items.push(activityItem);
    broadcastEvent({ type: "item", item: activityItem });
  }

  if (kind === "tool" || kind === "subagent") {
    const updatedSteps = applyToolProgressToSteps(turnTask.steps ?? [], {
      toolName,
      label,
      status,
      summary,
    });
    turnTask.steps = updatedSteps.steps;
    turnTask.progress = updatedSteps.progress;
    turnTask.updated_at = Date.now();
    if (sessionStatusMap.get(channel.id) !== "running") {
      sessionStatusMap.set(channel.id, "running");
      broadcastEvent({ type: "session", session: buildSessionSnapshot(channel) });
    }
    broadcastEvent({ type: "task", task: turnTask });
  }
}

async function readStreamWithParser(
  body: ReadableStream<Uint8Array>,
  onChunk: (text: string) => void
) {
  const reader = body.getReader();
  const decoder = new TextDecoder();
  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    onChunk(decoder.decode(value, { stream: true }));
  }
  const tail = decoder.decode();
  if (tail) onChunk(tail);
}

async function readGatewayErrorBody(res: Response): Promise<{ code?: string; message: string }> {
  const text = await res.text().catch(() => "");
  if (text) {
    try {
      const parsed = JSON.parse(text) as {
        error?: { code?: string; message?: string } | string;
        code?: string;
        message?: string;
        detail?: string;
      };
      const nestedErr = typeof parsed.error === "object" ? parsed.error : null;
      const code =
        nestedErr?.code ||
        parsed.code ||
        undefined;
      const message =
        nestedErr?.message ||
        (typeof parsed.error === "string" ? parsed.error : "") ||
        parsed.message ||
        parsed.detail ||
        text;
      return { code, message: String(message) };
    } catch {
      return { message: text.slice(0, 300) };
    }
  }
  return { message: `HTTP ${res.status}` };
}

async function executeGatewayTurn(
  params: ExecuteTurnParams,
  assistantId: string
): Promise<TurnExecutionOutcome> {
  const {
    message,
    channel,
    runIdLocal,
    turnTask,
    transportMode = "auto",
    idempotencyKey,
    abortSignal,
    model,
    provider,
  } = params;

  const targetProfile = channel.profile || getGatewayConfig().profile;
  if (!cachedCapabilitiesByProfile.has(targetProfile)) {
    await probeHermesGateway(targetProfile).catch(() => null);
  }
  const profileCapabilities = cachedCapabilitiesByProfile.get(targetProfile) ?? null;
  const profileStatus = cachedGatewayStatusByProfile.get(targetProfile);
  const transport = chooseGatewayTransport(transportMode, profileCapabilities);
  const cfg = getGatewayConfig(targetProfile);
  const resolvedModel =
    model ||
    profileStatus?.activeModel ||
    process.env.API_SERVER_MODEL_NAME ||
    "hermes-agent";
  const resolvedProvider = provider || profileStatus?.activeProvider;

  const reasoningItemIdRef = { current: null as string | null };
  const seqRef = { current: 0 };
  let streamedPlaceholderCreated = false;

  const getLatestActivityTimestampForRun = (): number => {
    let maxTs = 0;
    for (const it of items) {
      if (it.kind === "activity" && it.run_id === runIdLocal) {
        const ts = Math.max(it.created_at || 0, it.updated_at || 0);
        if (ts > maxTs) maxTs = ts;
      }
    }
    return maxTs;
  };

  const onContentDelta = (accumulated: string) => {
    const streamed = applyStreamingDeltaToSteps(turnTask.steps ?? []);
    if (streamed.changed) {
      turnTask.steps = streamed.steps;
      turnTask.progress = streamed.progress;
      turnTask.updated_at = Date.now();
      broadcastEvent({ type: "task", task: turnTask });
    }
    const minReplyTs = getLatestActivityTimestampForRun() + 1;
    if (!streamedPlaceholderCreated) {
      streamedPlaceholderCreated = true;
      const placeholder: HermesItem = {
        id: assistantId,
        kind: "assistant",
        created_at: Math.max(Date.now(), minReplyTs),
        channel_id: channel.id,
        streaming: true,
        run_id: runIdLocal,
        text: accumulated,
      };
      items.push(placeholder);
      sessionStatusMap.set(channel.id, "speaking");
      broadcastEvent({ type: "item", item: placeholder });
      broadcastEvent({ type: "session", session: buildSessionSnapshot(channel) });
    } else {
      const existing = items.find((it) => it.id === assistantId);
      if (existing) {
        existing.text = accumulated;
        if (existing.created_at < minReplyTs) {
          existing.created_at = Math.max(Date.now(), minReplyTs);
          broadcastEvent({ type: "item", item: existing });
          return;
        }
      }
      broadcastEvent({ type: "delta", item_id: assistantId, text: accumulated });
    }
  };

  // -------------------------------------------------------------------------
  // Transport 1: Runs API (POST /v1/runs + GET /v1/runs/{id}/events)
  // -------------------------------------------------------------------------
  if (transport === "runs") {
    const runBody: Record<string, unknown> = {
      input: message,
      session_id: channel.session_id,
      ...(channel.persona ? { instructions: channel.persona } : {}),
      ...(model ? { model } : {}),
      ...(provider ? { provider } : {}),
    };

    const createRes = await fetchWithConcurrentRetry(
      () =>
        fetchHermesGateway(
          "/v1/runs",
          {
            method: "POST",
            headers: {
              "Idempotency-Key": idempotencyKey,
              "X-Hermes-Session-Key": channel.session_key,
            },
            body: JSON.stringify(runBody),
          },
          HERMES_CHAT_TIMEOUT_MS,
          abortSignal,
          targetProfile
        ),
      { signal: abortSignal }
    );

    if (!createRes.ok) {
      const errInfo = await readGatewayErrorBody(createRes);
      return {
        reply: "",
        reasoning: "",
        outcome:
          errInfo.code === "admission_conflict" ? "admission_conflict" : "failed",
        errorCode: errInfo.code,
        error: `Gateway ${createRes.status}: ${errInfo.message}`,
      };
    }

    const createdJson = (await createRes.json().catch(() => null)) as {
      run_id?: string;
      status?: string;
    } | null;
    const gwRunId = createdJson?.run_id;
    if (!gwRunId) {
      return {
        reply: "",
        reasoning: "",
        outcome: "failed",
        error: "Gateway /v1/runs did not return a run_id",
      };
    }

    activeGatewayRunIds.set(channel.id, gwRunId);
    runProfileMap.set(gwRunId, targetProfile);
    turnTask.run_id = gwRunId;
    broadcastEvent({ type: "task", task: turnTask });

    try {
      const eventsRes = await fetchHermesGateway(
        `/v1/runs/${encodeURIComponent(gwRunId)}/events`,
        {
          method: "GET",
          headers: { Accept: "text/event-stream" },
        },
        HERMES_CHAT_TIMEOUT_MS,
        abortSignal,
        targetProfile
      );

      if (!eventsRes.ok || !eventsRes.body) {
        const errInfo = await readGatewayErrorBody(eventsRes);
        return {
          reply: "",
          reasoning: "",
          outcome: "failed",
          errorCode: errInfo.code,
          error: `Run events HTTP ${eventsRes.status}: ${errInfo.message}`,
          gatewayRunId: gwRunId,
        };
      }

      const parser = new RunsEventStreamParser();
      const callbacks = {
        onReasoningDelta: (acc: string) => {
          recordReasoningActivity({
            channel,
            runIdLocal,
            accumulatedReasoning: acc,
            reasoningItemIdRef,
            streaming: true,
          });
        },
        onInterim: (interim: { text: string; alreadyStreamed: boolean }) => {
          if (interim.alreadyStreamed) return;
          recordToolOrSubagentActivity({
            channel,
            runIdLocal,
            turnTask,
            seqRef,
            toolName: "commentary",
            label: "Commentary",
            status: "done",
            summary: interim.text,
            kind: "commentary",
          });
        },
        onToolEvent: (ev: {
          toolName: string;
          label: string;
          status: string;
          summary: string;
        }) => {
          recordToolOrSubagentActivity({
            channel,
            runIdLocal,
            turnTask,
            seqRef,
            toolName: ev.toolName,
            label: ev.label,
            status: ev.status,
            summary: ev.summary,
            kind: "tool",
          });
        },
        onSubagentStart: (info: { summary: string }) => {
          recordToolOrSubagentActivity({
            channel,
            runIdLocal,
            turnTask,
            seqRef,
            toolName: "subagent",
            label: "Subagent spawned",
            status: "running",
            summary: `Agent spawned: ${info.summary}`,
            kind: "subagent",
          });
        },
        onSubagentComplete: (info: { status: string; summary: string }) => {
          recordToolOrSubagentActivity({
            channel,
            runIdLocal,
            turnTask,
            seqRef,
            toolName: "subagent",
            label: `Subagent ${info.status}`,
            status: info.status === "failed" ? "failed" : "done",
            summary: `Subagent ${info.status}: ${info.summary}`,
            kind: "subagent",
          });
        },
        onApprovalRequest: async (req: {
          requestId: string;
          executionGeneration: number;
          kind: "approval" | "clarify";
          title: string;
          toolName?: string;
          command?: string;
          summary: string;
          destination: string;
          choices: string[];
        }) => {
          // Confirm canonical pending_controls identity via GET /v1/runs/{id} when available
          let resolvedReqId = req.requestId;
          let resolvedGen = req.executionGeneration;
          try {
            const runPoll = await fetchHermesGateway(
              `/v1/runs/${encodeURIComponent(gwRunId)}`,
              { method: "GET" },
              1500,
              undefined,
              targetProfile
            );
            if (runPoll.ok) {
              const runState = (await runPoll.json().catch(() => null)) as {
                pending_controls?: {
                  prompt_id?: string;
                  request_id?: string;
                  execution_generation?: number;
                };
              } | null;
              if (runState?.pending_controls) {
                resolvedReqId =
                  runState.pending_controls.prompt_id ||
                  runState.pending_controls.request_id ||
                  resolvedReqId;
                if (typeof runState.pending_controls.execution_generation === "number") {
                  resolvedGen = runState.pending_controls.execution_generation;
                }
              }
            }
          } catch {
            // proceed with event identity
          }

          turnTask.status = "waiting";
          turnTask.updated_at = Date.now();
          broadcastEvent({ type: "task", task: turnTask });

          const cardId = `approval-${gwRunId}-${resolvedReqId}`;
          const existingIdx = pendingCards.findIndex((c) => c.id === cardId);
          const cardItem: HermesItem = {
            id: cardId,
            kind: "card",
            created_at: Date.now(),
            channel_id: channel.id,
            run_id: gwRunId,
            card: {
              type: req.kind,
              title: req.title,
              body: req.summary,
              status: "pending",
              action: req.toolName,
              destination: req.destination,
              draft: req.command || req.summary,
              command: req.command,
              requestId: resolvedReqId,
              executionGeneration: resolvedGen,
              choices: req.choices,
            },
          };
          if (existingIdx >= 0) pendingCards[existingIdx] = cardItem;
          else pendingCards.push(cardItem);
          broadcastEvent({ type: "item", item: cardItem });
        },
        onSteered: (text: string) => {
          recordToolOrSubagentActivity({
            channel,
            runIdLocal,
            turnTask,
            seqRef,
            toolName: "steer",
            label: "Steer",
            status: "done",
            summary: `Steer: ${text}`,
            kind: "commentary",
          });
        },
        onDelta: onContentDelta,
      };

      await readStreamWithParser(eventsRes.body, (chunk) => parser.push(chunk, callbacks));
      const result = parser.finish(callbacks);

      if (result.reasoning.trim()) {
        recordReasoningActivity({
          channel,
          runIdLocal,
          accumulatedReasoning: result.reasoning,
          reasoningItemIdRef,
          streaming: false,
        });
      }

      // Clear any unsettled approval card tied to this run once settled
      for (const pc of pendingCards) {
        if (pc.run_id === gwRunId && pc.card?.status === "pending") {
          pc.card.status = result.outcome === "cancelled" ? "declined" : "approved";
          pc.updated_at = Date.now();
          broadcastEvent({ type: "item", item: pc });
        }
      }

      return {
        reply: result.output.trim(),
        reasoning: result.reasoning.trim(),
        outcome: result.outcome,
        errorCode: result.errorCode,
        error: result.error,
        gatewayRunId: gwRunId,
      };
    } finally {
      if (activeGatewayRunIds.get(channel.id) === gwRunId) {
        activeGatewayRunIds.delete(channel.id);
      }
    }
  }

  // -------------------------------------------------------------------------
  // Transport 2: Session Chat Stream (POST /api/sessions/{id}/chat/stream)
  // -------------------------------------------------------------------------
  if (transport === "session_stream") {
    const streamRes = await fetchWithConcurrentRetry(
      () =>
        fetchHermesGateway(
          `/api/sessions/${encodeURIComponent(channel.session_id)}/chat/stream`,
          {
            method: "POST",
            headers: {
              "Idempotency-Key": idempotencyKey,
              "X-Hermes-Session-Key": channel.session_key,
              Accept: "text/event-stream",
            },
            body: JSON.stringify({
              input: message,
              ...(model ? { model } : {}),
              ...(provider ? { provider } : {}),
            }),
          },
          HERMES_CHAT_TIMEOUT_MS,
          abortSignal,
          targetProfile
        ),
      { signal: abortSignal }
    );

    if (!streamRes.ok || !streamRes.body) {
      const errInfo = await readGatewayErrorBody(streamRes);
      return {
        reply: "",
        reasoning: "",
        outcome: errInfo.code === "admission_conflict" ? "admission_conflict" : "failed",
        errorCode: errInfo.code,
        error: `Gateway ${streamRes.status}: ${errInfo.message}`,
      };
    }

    const parser = new SessionChatStreamParser();
    const callbacks = {
      onCommentary: (c: { text: string; alreadyStreamed: boolean }) => {
        if (c.alreadyStreamed) return;
        recordToolOrSubagentActivity({
          channel,
          runIdLocal,
          turnTask,
          seqRef,
          toolName: "commentary",
          label: "Commentary",
          status: "done",
          summary: c.text,
          kind: "commentary",
        });
      },
      onToolEvent: (ev: {
        toolName: string;
        label: string;
        status: string;
        summary: string;
      }) => {
        recordToolOrSubagentActivity({
          channel,
          runIdLocal,
          turnTask,
          seqRef,
          toolName: ev.toolName,
          label: ev.label,
          status: ev.status,
          summary: ev.summary,
          kind: "tool",
        });
      },
      onDelta: onContentDelta,
    };

    await readStreamWithParser(streamRes.body, (chunk) => parser.push(chunk, callbacks));
    const result = parser.finish(callbacks);
    return {
      reply: result.content.trim(),
      reasoning: "",
      outcome: result.outcome,
      error: result.error,
    };
  }

  // -------------------------------------------------------------------------
  // Transport 3: Chat Completions (POST /v1/chat/completions)
  // Sends X-Hermes-Session-Id for full server-side history (no .slice(-6)!)
  // and X-Hermes-Session-Key strictly for Honcho/long-term memory scoping.
  // -------------------------------------------------------------------------
  const chatBody: Record<string, unknown> = {
    model: resolvedModel,
    ...(resolvedProvider ? { provider: resolvedProvider } : {}),
    messages: [{ role: "user", content: message }],
    user: channel.session_key,
    stream: true,
  };

  const gwRes = await fetchWithConcurrentRetry(
    () =>
      fetchHermesGateway(
        "/v1/chat/completions",
        {
          method: "POST",
          headers: {
            "Idempotency-Key": idempotencyKey,
            "X-Hermes-Session-Id": channel.session_id,
            "X-Hermes-Session-Key": channel.session_key,
            Accept: "text/event-stream, application/json",
          },
          body: JSON.stringify(chatBody),
        },
        HERMES_CHAT_TIMEOUT_MS,
        abortSignal,
        targetProfile
      ),
    { signal: abortSignal }
  );

  if (!gwRes.ok) {
    const errInfo = await readGatewayErrorBody(gwRes);
    return {
      reply: "",
      reasoning: "",
      outcome:
        errInfo.code === "admission_conflict" || gwRes.status === 409
          ? "admission_conflict"
          : "failed",
      errorCode: errInfo.code,
      error: `Gateway HTTP ${gwRes.status}: ${errInfo.message}`,
    };
  }

  const contentType = gwRes.headers.get("content-type") || "";
  if (contentType.includes("text/event-stream") && gwRes.body) {
    const parser = new ChatCompletionsStreamParser();
    const callbacks = {
      onReasoningDelta: (acc: string) => {
        recordReasoningActivity({
          channel,
          runIdLocal,
          accumulatedReasoning: acc,
          reasoningItemIdRef,
          streaming: true,
        });
      },
      onToolProgress: (ev: {
        toolName: string;
        label: string;
        status: string;
        summary: string;
      }) => {
        recordToolOrSubagentActivity({
          channel,
          runIdLocal,
          turnTask,
          seqRef,
          toolName: ev.toolName,
          label: ev.label,
          status: ev.status,
          summary: ev.summary,
          kind: "tool",
        });
      },
      onContentDelta,
    };

    await readStreamWithParser(gwRes.body, (chunk) => parser.push(chunk, callbacks));
    const parsed = parser.finish(callbacks);

    if (parsed.reasoning.trim()) {
      recordReasoningActivity({
        channel,
        runIdLocal,
        accumulatedReasoning: parsed.reasoning,
        reasoningItemIdRef,
        streaming: false,
      });
    }

    if (parsed.sessionId && parsed.sessionId !== channel.session_id) {
      channel.session_id = parsed.sessionId;
    }

    return {
      reply: parsed.content.trim(),
      reasoning: parsed.reasoning.trim(),
      outcome: parsed.outcome,
      errorCode: parsed.errorCode,
      error: parsed.errorMessage,
    };
  }

  const nonStream = parseChatCompletionsJson(await gwRes.json().catch(() => null));
  if (!nonStream) {
    return {
      reply: "",
      reasoning: "",
      outcome: "failed",
      error: `Invalid response payload from ${cfg.baseUrl}/v1/chat/completions`,
    };
  }
  if (nonStream.reasoning.trim()) {
    recordReasoningActivity({
      channel,
      runIdLocal,
      accumulatedReasoning: nonStream.reasoning,
      reasoningItemIdRef,
      streaming: false,
    });
  }
  return {
    reply: nonStream.content.trim(),
    reasoning: nonStream.reasoning.trim(),
    outcome: nonStream.outcome,
    errorCode: nonStream.errorCode,
    error: nonStream.errorMessage,
  };
}

async function createServer() {
  const app = express();
  app.use(express.json({ limit: "8mb" }));

  app.get("/api/health", (_req, res) => {
    const cfg = getGatewayConfig();
    const hasDeepgramKey = Boolean(process.env.DEEPGRAM_API_KEY);
    res.json({
      ok: true,
      bootId: BOOT_ID,
      appVersion: readPackageVersion(__dirname),
      hasDeepgramKey,
      hermesApiUrl: cfg.baseUrl,
      hermesProfile: cfg.profile,
      routePrefix: cfg.routePrefix,
      ttsProvider: hasDeepgramKey ? "deepgram" : "browser",
    });
  });

  app.get("/api/hermes/gateway", async (req, res) => {
    const profileParam = typeof req.query.profile === "string" ? req.query.profile : undefined;
    const status = await probeHermesGateway(profileParam);
    res.json(status);
  });

  app.get("/api/hermes/profiles", async (req, res) => {
    const refresh = req.query.refresh === "1" || req.query.refresh === "true";
    const runtimes = listAllProfileRuntimes();
    const defaultProfileId = getGatewayConfig().profile;

    if (refresh) {
      await Promise.all(runtimes.map((rt) => probeHermesGateway(rt.id).catch(() => null)));
    } else if (!cachedGatewayStatusByProfile.has(defaultProfileId)) {
      await probeHermesGateway(defaultProfileId).catch(() => null);
    }

    const profiles = runtimes.map((rt) =>
      toRedactedProfileSummary(rt, cachedGatewayStatusByProfile.get(rt.id))
    );
    res.json({ defaultProfileId, profiles });
  });

  app.post("/api/hermes/profiles", async (req, res) => {
    const { id: rawId, label, mode, baseUrl, apiKey, description } = (req.body ?? {}) as {
      id?: string;
      label?: string;
      mode?: HermesProfileMode;
      baseUrl?: string;
      apiKey?: string;
      description?: string;
    };

    if (!isValidCustomProfileSlug(rawId)) {
      res.status(400).json({
        error:
          'Enter a valid profile slug (lowercase letters, digits, "-" or "_", not "default" or reserved path).',
      });
      return;
    }

    const cleanId = normalizeProfileId(rawId);
    const resolvedMode: HermesProfileMode = mode === "standalone" ? "standalone" : "multiplexed";
    if (resolvedMode === "standalone" && !isValidHttpUrl(baseUrl)) {
      res.status(400).json({
        error: "Standalone profile requires a valid http:// or https:// gateway URL (e.g. http://127.0.0.1:8643).",
      });
      return;
    }

    const existing = customProfileDefs.get(cleanId);
    customProfileDefs.set(cleanId, {
      id: cleanId,
      label: label?.trim() || cleanId,
      mode: resolvedMode,
      baseUrl: resolvedMode === "standalone" ? normalizeBaseUrl(baseUrl) : undefined,
      apiKey: typeof apiKey === "string" && apiKey.trim() ? apiKey.trim() : existing?.apiKey,
      source: "custom",
      description: description?.trim() || existing?.description,
    });

    const status = await probeHermesGateway(cleanId);
    const runtimes = listAllProfileRuntimes();
    const defaultProfileId = getGatewayConfig().profile;
    const profiles = runtimes.map((rt) =>
      toRedactedProfileSummary(rt, cachedGatewayStatusByProfile.get(rt.id))
    );
    const saved = profiles.find((p) => p.id === cleanId);

    res.status(200).json({
      ok: true,
      defaultProfileId,
      profile: saved,
      status,
      profiles,
    });
  });

  app.delete("/api/hermes/profiles/:profileId", (req, res) => {
    const targetId = normalizeProfileId(req.params.profileId);
    const primaryProfile = getGatewayConfig().profile;
    if (targetId === "default" || targetId === primaryProfile) {
      res.status(400).json({ error: `Cannot delete primary profile "${targetId}".` });
      return;
    }
    const existed = customProfileDefs.delete(targetId);
    cachedCapabilitiesByProfile.delete(targetId);
    cachedGatewayStatusByProfile.delete(targetId);

    const runtimes = listAllProfileRuntimes();
    const profiles = runtimes.map((rt) =>
      toRedactedProfileSummary(rt, cachedGatewayStatusByProfile.get(rt.id))
    );
    res.status(200).json({
      ok: true,
      removed: existed,
      defaultProfileId: primaryProfile,
      profiles,
    });
  });

  app.get("/api/hermes/context", async (req, res) => {
    const profileParam = typeof req.query.profile === "string" ? req.query.profile : undefined;
    const ctx = await fetchGatewayContext(profileParam);
    res.json(ctx);
  });

  app.get("/api/hermes/skills", async (req, res) => {
    const profileParam = typeof req.query.profile === "string" ? req.query.profile : undefined;
    try {
      const r = await fetchHermesGateway("/v1/skills", { method: "GET" }, 2500, undefined, profileParam);
      if (!r.ok) {
        const err = await readGatewayErrorBody(r);
        res.status(r.status).json({ error: err.message, skills: [] });
        return;
      }
      res.json({ skills: mapGatewaySkills(await r.json().catch(() => null)) });
    } catch (err) {
      res.status(503).json({
        error: err instanceof Error ? err.message : "Gateway unreachable",
        skills: [],
      });
    }
  });

  // -------------------------------------------------------------------------
  // TypeSafe AI Jev × DevvGwardo/uniichat-recall Hermes Skill & Proxy Routes
  // -------------------------------------------------------------------------
  const REPO_JEV_SKILL_PATH = path.join(
    __dirname,
    "skills",
    "hermes",
    "uniichat-recall-jev",
    "SKILL.md"
  );

  function resolveLocalHermesSkillInstallPath(): string {
    const hermesHomeRoot = process.env.HERMES_HOME_ROOT || path.join(os.homedir(), ".hermes");
    return path.join(hermesHomeRoot, "skills", "memory", "uniichat-recall-jev", "SKILL.md");
  }

  app.get("/api/hermes/jev/skill", (_req, res) => {
    try {
      const markdown = fs.existsSync(REPO_JEV_SKILL_PATH)
        ? fs.readFileSync(REPO_JEV_SKILL_PATH, "utf8")
        : "";
      const installedPath = resolveLocalHermesSkillInstallPath();
      const isInstalledLocally = fs.existsSync(installedPath);
      const liveJevConfigured = Boolean(process.env.TYPESAFE_API_KEY?.trim());
      const jevModel = process.env.TYPESAFE_JEV_MODEL?.trim() || JEV_CONTEXT_LIMITS.defaultModel;
      res.json({
        skillName: "uniichat-recall-jev",
        repoPath: "skills/hermes/uniichat-recall-jev/SKILL.md",
        installedPath,
        isInstalledLocally,
        liveJevConfigured,
        jevModel,
        endpoint: process.env.TYPESAFE_API_URL?.trim() || JEV_CONTEXT_LIMITS.endpoint,
        markdown,
      });
    } catch (err) {
      res.status(500).json({
        error: err instanceof Error ? err.message : "Failed to load Hermes skill file",
      });
    }
  });

  app.post("/api/hermes/jev/skill/install", (_req, res) => {
    try {
      if (!fs.existsSync(REPO_JEV_SKILL_PATH)) {
        res.status(404).json({ error: "Bundled uniichat-recall-jev SKILL.md not found." });
        return;
      }
      const markdown = fs.readFileSync(REPO_JEV_SKILL_PATH, "utf8");
      const targetPath = resolveLocalHermesSkillInstallPath();
      fs.mkdirSync(path.dirname(targetPath), { recursive: true });
      fs.writeFileSync(targetPath, markdown, "utf8");
      res.json({
        installed: true,
        skillName: "uniichat-recall-jev",
        installedPath: targetPath,
      });
    } catch (err) {
      res.status(500).json({
        installed: false,
        error: err instanceof Error ? err.message : "Unable to write skill into ~/.hermes/skills",
      });
    }
  });

  app.post("/api/hermes/jev/evaluate", async (req, res) => {
    const startedAt = Date.now();
    try {
      const body = (req.body && typeof req.body === "object" ? req.body : {}) as Record<string, unknown>;
      const sessionId =
        typeof body.sessionId === "string" && body.sessionId.trim()
          ? body.sessionId.trim().slice(0, 120)
          : "active-session";
      const rawMode = typeof body.mode === "string" ? body.mode : "coverage-rerank";
      const mode: UniichatRetrievalMode =
        rawMode === "coverage-rerank" ||
        rawMode === "keyword-first-pass" ||
        rawMode === "bm25-only" ||
        rawMode === "original-hermes"
          ? rawMode
          : "coverage-rerank";
      const queryExpansion = body.queryExpansion !== false;
      const resetThreshold =
        typeof body.resetThreshold === "number" && Number.isFinite(body.resetThreshold)
          ? Math.max(0.55, Math.min(0.92, body.resetThreshold))
          : 0.78;
      const simulatedTurnOffset =
        typeof body.simulatedTurnOffset === "number" && Number.isFinite(body.simulatedTurnOffset)
          ? Math.max(0, Math.min(40, Math.round(body.simulatedTurnOffset)))
          : 0;

      const rawEntries = Array.isArray(body.entries) ? body.entries : [];
      const sanitizedEntries = rawEntries.slice(-80).map((item, idx) => {
        const rec = item && typeof item === "object" ? (item as Record<string, unknown>) : {};
        const role =
          rec.role === "user" || rec.role === "hermes" || rec.role === "command" || rec.role === "activity"
            ? rec.role
            : "user";
        return {
          id: typeof rec.id === "string" ? rec.id : `e-${idx}`,
          role,
          text: typeof rec.text === "string" ? rec.text.slice(0, 2400) : "",
          timestamp: typeof rec.timestamp === "string" ? rec.timestamp : "",
        };
      });

      const baseSnapshot = analyzeSessionEntries(sanitizedEntries);
      const snapshot =
        simulatedTurnOffset > 0
          ? {
              ...baseSnapshot,
              turnCount: baseSnapshot.turnCount + simulatedTurnOffset,
              estimatedTokens:
                baseSnapshot.estimatedTokens + simulatedTurnOffset * baseSnapshot.avgTokensPerTurn,
              topicDriftRatio: Math.min(0.88, Math.max(0.12, baseSnapshot.topicDriftRatio + simulatedTurnOffset * 0.016)),
            }
          : baseSnapshot;

      const apiKey = process.env.TYPESAFE_API_KEY?.trim();
      const apiUrl = process.env.TYPESAFE_API_URL?.trim() || JEV_CONTEXT_LIMITS.endpoint;
      const jevModel = process.env.TYPESAFE_JEV_MODEL?.trim() || JEV_CONTEXT_LIMITS.defaultModel;

      if (apiKey) {
        const payload = buildTypeSafeJevRequestPayload({
          sessionId,
          mode,
          queryExpansion,
          resetThreshold,
          snapshot,
          model: jevModel,
        });
        const controller = new AbortController();
        const timer = setTimeout(() => controller.abort(), 8000);
        try {
          const upstream = await fetch(apiUrl, {
            method: "POST",
            headers: {
              Authorization: `Bearer ${apiKey}`,
              "Content-Type": "application/json",
            },
            body: JSON.stringify(payload),
            signal: controller.signal,
          });
          clearTimeout(timer);
          if (upstream.ok) {
            const json = await upstream.json().catch(() => null);
            const parsed = parseTypeSafeJevResponse(json, Date.now() - startedAt, jevModel);
            if (parsed) {
              res.json({ evaluation: parsed, liveJevConfigured: true });
              return;
            }
          }
          const errText = await upstream.text().catch(() => "");
          const fallback = buildCalibratedBaselineEvaluation({
            mode,
            queryExpansion,
            resetThreshold,
            snapshot,
          });
          fallback.note = `TypeSafe Jev API returned HTTP ${upstream.status}${errText ? ` (${errText.slice(0, 80)})` : ""} · fell back to benchmark curve`;
          res.json({ evaluation: fallback, liveJevConfigured: true });
          return;
        } catch (err) {
          clearTimeout(timer);
          const fallback = buildCalibratedBaselineEvaluation({
            mode,
            queryExpansion,
            resetThreshold,
            snapshot,
          });
          fallback.note = `TypeSafe Jev unreachable (${err instanceof Error ? err.message : "network error"}) · using benchmark curve`;
          res.json({ evaluation: fallback, liveJevConfigured: true });
          return;
        }
      }

      const evaluation = buildCalibratedBaselineEvaluation({
        mode,
        queryExpansion,
        resetThreshold,
        snapshot,
      });
      res.json({ evaluation, liveJevConfigured: false });
    } catch (err) {
      res.status(500).json({
        error: err instanceof Error ? err.message : "Evaluation failed",
      });
    }
  });

  app.get("/api/hermes/toolsets", async (req, res) => {
    const profileParam = typeof req.query.profile === "string" ? req.query.profile : undefined;
    try {
      const r = await fetchHermesGateway("/v1/toolsets", { method: "GET" }, 2500, undefined, profileParam);
      if (!r.ok) {
        const err = await readGatewayErrorBody(r);
        res.status(r.status).json({ error: err.message, toolsets: [] });
        return;
      }
      res.json({ toolsets: mapGatewayToolsets(await r.json().catch(() => null)) });
    } catch (err) {
      res.status(503).json({
        error: err instanceof Error ? err.message : "Gateway unreachable",
        toolsets: [],
      });
    }
  });

  // -------------------------------------------------------------------------
  // Jobs API (/api/jobs -> /api/hermes/jobs*)
  // -------------------------------------------------------------------------
  app.get("/api/hermes/jobs", async (req, res) => {
    const profileParam = typeof req.query.profile === "string" ? req.query.profile : undefined;
    try {
      const r = await fetchHermesGateway("/api/jobs", { method: "GET" }, 2500, undefined, profileParam);
      if (!r.ok) {
        const err = await readGatewayErrorBody(r);
        res.status(r.status).json({ error: err.message, jobs: [] });
        return;
      }
      res.json({ jobs: mapGatewayJobs(await r.json().catch(() => null)) });
    } catch (err) {
      res.status(503).json({
        error: err instanceof Error ? err.message : "Gateway unreachable",
        jobs: [],
      });
    }
  });

  app.post("/api/hermes/jobs", async (req, res) => {
    const profileParam = typeof req.query.profile === "string" ? req.query.profile : undefined;
    try {
      const r = await fetchHermesGateway(
        "/api/jobs",
        { method: "POST", body: JSON.stringify(req.body ?? {}) },
        4000,
        undefined,
        profileParam
      );
      const body = await r.json().catch(() => ({}));
      res.status(r.status).json(body);
    } catch (err) {
      res.status(503).json({ error: err instanceof Error ? err.message : "Gateway unreachable" });
    }
  });

  app.patch("/api/hermes/jobs/:jobId", async (req, res) => {
    const profileParam = typeof req.query.profile === "string" ? req.query.profile : undefined;
    try {
      const r = await fetchHermesGateway(
        `/api/jobs/${encodeURIComponent(req.params.jobId)}`,
        { method: "PATCH", body: JSON.stringify(req.body ?? {}) },
        4000,
        undefined,
        profileParam
      );
      const body = await r.json().catch(() => ({}));
      res.status(r.status).json(body);
    } catch (err) {
      res.status(503).json({ error: err instanceof Error ? err.message : "Gateway unreachable" });
    }
  });

  app.delete("/api/hermes/jobs/:jobId", async (req, res) => {
    const profileParam = typeof req.query.profile === "string" ? req.query.profile : undefined;
    try {
      const r = await fetchHermesGateway(
        `/api/jobs/${encodeURIComponent(req.params.jobId)}`,
        { method: "DELETE" },
        4000,
        undefined,
        profileParam
      );
      const body = await r.json().catch(() => ({ ok: r.ok }));
      res.status(r.status).json(body);
    } catch (err) {
      res.status(503).json({ error: err instanceof Error ? err.message : "Gateway unreachable" });
    }
  });

  for (const action of ["pause", "resume", "run"] as const) {
    app.post(`/api/hermes/jobs/:jobId/${action}`, async (req, res) => {
      const profileParam = typeof req.query.profile === "string" ? req.query.profile : undefined;
      try {
        const r = await fetchHermesGateway(
          `/api/jobs/${encodeURIComponent(req.params.jobId)}/${action}`,
          { method: "POST" },
          4000,
          undefined,
          profileParam
        );
        const body = await r.json().catch(() => ({ ok: r.ok }));
        res.status(r.status).json(body);
      } catch (err) {
        res.status(503).json({ error: err instanceof Error ? err.message : "Gateway unreachable" });
      }
    });
  }

  // -------------------------------------------------------------------------
  // State Snapshot & Channel History (backed by real GET /api/sessions)
  // -------------------------------------------------------------------------
  app.get("/api/hermes/state", async (_req, res) => {
    await syncSessionsFromGateway();
    res.status(200).json({
      items,
      pending: pendingCards,
      tasks,
      channels,
      sessions: buildAllSessionSnapshots(),
    });
  });

  app.get("/api/hermes/channels/:channelId/items", async (req, res) => {
    const slug = normalizeChannelKey(req.params.channelId) || "general";
    const channel = channels.find((c) => c.id === slug);
    const localItems = items.filter((it) => (it.channel_id || slug) === slug);

    if (channel && localItems.length === 0) {
      try {
        const msgRes = await fetchHermesGateway(
          `/api/sessions/${encodeURIComponent(channel.session_id)}/messages?inline_images=false`,
          { method: "GET" },
          2500,
          undefined,
          channel.profile
        );
        if (msgRes.ok) {
          const gwMessages = mapGatewaySessionMessages(
            await msgRes.json().catch(() => null),
            channel.session_id
          );
          for (const msg of gwMessages) {
            if (msg.reasoning) {
              const reasonItem: HermesItem = {
                id: `${msg.id}-reasoning`,
                kind: "activity",
                created_at: Math.max(0, msg.createdAtMs - 1),
                channel_id: slug,
                text: `Reasoning: ${msg.reasoning}`,
                activity: {
                  run_id: msg.id,
                  status: "done",
                  kind: "reasoning",
                  summary: "Reasoning",
                  detail: msg.reasoning,
                },
              };
              if (!items.some((it) => it.id === reasonItem.id)) items.push(reasonItem);
            }
            const entryItem: HermesItem = {
              id: msg.id,
              kind: msg.role === "user" ? "user" : msg.role === "system" ? "system" : "assistant",
              created_at: msg.createdAtMs,
              channel_id: slug,
              text: msg.text,
            };
            if (!items.some((it) => it.id === entryItem.id)) items.push(entryItem);
          }
        }
      } catch {
        // gateway offline or messages unavailable
      }
    }

    const chItems = items
      .filter((it) => (it.channel_id || slug) === slug)
      .sort((a, b) => a.created_at - b.created_at);
    res.status(200).json({ items: chItems, has_more: false });
  });

  app.post("/api/hermes/channels/:channelId/seen", (_req, res) => {
    res.status(200).json({ ok: true });
  });

  // -------------------------------------------------------------------------
  // Run Submission & Turn Execution
  // -------------------------------------------------------------------------
  app.post("/api/hermes/messages", async (req, res) => {
    const {
      text,
      channel_id = "general",
      name,
      topic,
      session_id,
      session_key,
      persona,
      profile,
      transport_mode,
      idempotency_key,
      model,
      provider,
    } = (req.body ?? {}) as {
      text?: string;
      channel_id?: string;
      name?: string;
      topic?: string;
      session_id?: string;
      session_key?: string;
      persona?: string;
      profile?: string;
      transport_mode?: HermesTransportMode;
      idempotency_key?: string;
      model?: string;
      provider?: string;
    };

    if (!text || typeof text !== "string" || !text.trim()) {
      res.status(400).json({ error: "text is required" });
      return;
    }

    const cleanId = normalizeChannelKey(channel_id) || "session";
    const resolvedSessionId = session_id?.trim() || `hermes-${cleanId}`;
    workspaceSessionIds.add(resolvedSessionId);
    workspaceChannelIds.add(cleanId);
    const channel = upsertLocalChannel({
      channelId: cleanId,
      name,
      sessionId: resolvedSessionId,
      sessionKey: session_key,
      profile,
      topic,
      persona,
      origin: "workspace",
      lastAt: Date.now(),
    });

    const resolvedIdemKey =
      sanitizeSessionKeyHeader(idempotency_key || req.header("idempotency-key")) ||
      createIdempotencyKey("turn");

    const userItem: HermesItem = {
      id: `user-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`,
      kind: "user",
      created_at: Date.now(),
      channel_id: channel.id,
      text,
    };
    items.push(userItem);
    sessionStatusMap.set(channel.id, "thinking");

    // Abort any prior local turn controller on this lane
    activeRunAborts.get(channel.id)?.abort();
    const priorGwRun = activeGatewayRunIds.get(channel.id);
    if (priorGwRun) {
      activeGatewayRunIds.delete(channel.id);
      void fetchHermesGateway(
        `/v1/runs/${encodeURIComponent(priorGwRun)}/stop`,
        { method: "POST" },
        1500,
        undefined,
        channel.profile
      ).catch(() => {});
    }

    for (const prevTask of tasks) {
      if (
        prevTask.channel_id === channel.id &&
        prevTask.id.startsWith("task-turn-") &&
        (prevTask.status === "running" || prevTask.status === "queued" || prevTask.status === "waiting")
      ) {
        const finalizedPrev = finalizeTurnTaskSteps(
          prevTask.steps ?? [],
          "cancelled",
          "Superseded by newer turn"
        );
        prevTask.steps = finalizedPrev.steps;
        prevTask.progress = finalizedPrev.progress;
        prevTask.status = "cancelled";
        prevTask.updated_at = Date.now();
        broadcastEvent({ type: "task", task: prevTask });
      }
    }

    const runController = new AbortController();
    activeRunAborts.set(channel.id, runController);

    const runIdLocal = `run-${Date.now().toString(36)}`;
    const turnStartMs = Date.now();
    const turnTask: HermesTask = {
      id: `task-turn-${runIdLocal}`,
      title: deriveTurnTaskTitle(text, `#${channel.name}`),
      status: "running",
      kind: "In chat",
      background: false,
      session_id: channel.session_id,
      profile: channel.profile || getGatewayConfig().profile,
      channel_id: channel.id,
      progress: "0/1",
      steps: [],
      created_at: turnStartMs,
      updated_at: turnStartMs,
    };
    tasks.push(turnTask);
    if (tasks.length > 200) {
      const evictIdx = tasks.findIndex(
        (t) =>
          t.id.startsWith("task-turn-") &&
          t.status !== "running" &&
          t.status !== "queued" &&
          t.status !== "waiting"
      );
      if (evictIdx >= 0) tasks.splice(evictIdx, 1);
    }

    broadcastEvent({ type: "item", item: userItem });
    broadcastEvent({ type: "task", task: turnTask });
    broadcastEvent({ type: "session", session: buildSessionSnapshot(channel) });
    res.status(200).json({
      item: userItem,
      session: buildSessionSnapshot(channel),
      task: turnTask,
      idempotencyKey: resolvedIdemKey,
    });

    const assistantId = `hermes-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`;

    void (async () => {
      try {
        const outcome = await executeGatewayTurn(
          {
            message: text,
            channel,
            runIdLocal,
            turnTask,
            transportMode: transport_mode,
            idempotencyKey: resolvedIdemKey,
            abortSignal: runController.signal,
            model,
            provider,
          },
          assistantId
        );

        if (runController.signal.aborted && outcome.outcome !== "cancelled") {
          return;
        }

        const settledAt = Date.now();

        if (outcome.outcome === "admission_conflict") {
          const msg =
            outcome.error || "Idempotency key admission conflict (409 admission_conflict).";
          const finalized = finalizeTurnTaskSteps(turnTask.steps ?? [], "failed", msg);
          turnTask.steps = finalized.steps;
          turnTask.progress = finalized.progress;
          turnTask.status = "failed";
          turnTask.error_code = "admission_conflict";
          turnTask.updated_at = settledAt;
          sessionStatusMap.set(channel.id, "idle");
          const errItem: HermesItem = {
            id: assistantId,
            kind: "activity",
            created_at: settledAt,
            channel_id: channel.id,
            run_id: runIdLocal,
            error_code: "admission_conflict",
            terminal_outcome: "admission_conflict",
            text: msg,
            activity: { run_id: runIdLocal, status: "error", kind: "status", error: msg },
          };
          items.push(errItem);
          broadcastEvent({ type: "task", task: turnTask });
          broadcastEvent({ type: "item", item: errItem });
          broadcastEvent({ type: "session", session: buildSessionSnapshot(channel) });
          return;
        }

        if (outcome.outcome === "cancelled" || outcome.outcome === "interrupted") {
          const label =
            outcome.outcome === "interrupted"
              ? outcome.error || "Gateway shutdown interrupted the run."
              : "Turn cancelled";
          const finalized = finalizeTurnTaskSteps(turnTask.steps ?? [], "cancelled", label);
          turnTask.steps = finalized.steps;
          turnTask.progress = finalized.progress;
          turnTask.status = "cancelled";
          turnTask.error_code = outcome.errorCode || outcome.outcome;
          turnTask.result = outcome.reply ? outcome.reply.slice(0, 200) : label;
          turnTask.updated_at = settledAt;
          sessionStatusMap.set(channel.id, "idle");

          const existing = items.find((it) => it.id === assistantId);
          if (existing) {
            existing.streaming = false;
            existing.terminal_outcome = outcome.outcome;
            existing.text = outcome.reply
              ? `${outcome.reply}\n\n[${label}]`
              : `[${label}]`;
            existing.updated_at = settledAt;
            broadcastEvent({ type: "item", item: existing });
          } else {
            const actItem: HermesItem = {
              id: assistantId,
              kind: "activity",
              created_at: settledAt,
              channel_id: channel.id,
              run_id: runIdLocal,
              terminal_outcome: outcome.outcome,
              text: label,
              activity: { run_id: runIdLocal, status: "cancelled", kind: "status", summary: label },
            };
            items.push(actItem);
            broadcastEvent({ type: "item", item: actItem });
          }
          broadcastEvent({ type: "task", task: turnTask });
          broadcastEvent({ type: "session", session: buildSessionSnapshot(channel) });
          return;
        }

        if (outcome.outcome === "failed") {
          const errText = outcome.error || "Gateway turn failed";
          const finalized = finalizeTurnTaskSteps(
            turnTask.steps ?? [],
            "failed",
            errText.slice(0, 140)
          );
          turnTask.steps = finalized.steps;
          turnTask.progress = finalized.progress;
          turnTask.status = "failed";
          turnTask.error_code = outcome.errorCode;
          turnTask.updated_at = settledAt;
          sessionStatusMap.set(channel.id, "idle");

          const existing = items.find((it) => it.id === assistantId);
          if (existing) {
            existing.streaming = false;
            existing.terminal_outcome = "failed";
            existing.updated_at = settledAt;
            broadcastEvent({ type: "item", item: existing });
          }
          const errItem: HermesItem = {
            id: `${assistantId}-err`,
            kind: "activity",
            created_at: settledAt,
            channel_id: channel.id,
            run_id: runIdLocal,
            error_code: outcome.errorCode,
            terminal_outcome: "failed",
            text: errText,
            activity: { run_id: runIdLocal, status: "error", kind: "status", error: errText },
          };
          items.push(errItem);
          broadcastEvent({ type: "task", task: turnTask });
          broadcastEvent({ type: "item", item: errItem });
          broadcastEvent({ type: "session", session: buildSessionSnapshot(channel) });
          return;
        }

        // Completed or iteration_budget_exhausted (partial usable wrap-up answer)
        const isBudgetExhausted = outcome.outcome === "iteration_budget_exhausted";
        const replyText = outcome.reply;

        if (replyText) {
          let maxRunActivityTs = 0;
          for (const it of items) {
            if (it.kind === "activity" && it.run_id === runIdLocal) {
              const ts = Math.max(it.created_at || 0, it.updated_at || 0);
              if (ts > maxRunActivityTs) maxRunActivityTs = ts;
            }
          }
          const existingPlaceholder = items.find((it) => it.id === assistantId);
          const finalAssistant: HermesItem = existingPlaceholder ?? {
            id: assistantId,
            kind: "assistant",
            created_at: Math.max(settledAt, maxRunActivityTs + 1),
            channel_id: channel.id,
            run_id: runIdLocal,
          };
          if (finalAssistant.created_at <= maxRunActivityTs) {
            finalAssistant.created_at = Math.max(settledAt, maxRunActivityTs + 1);
          }
          finalAssistant.streaming = false;
          finalAssistant.text = replyText;
          finalAssistant.error_code = outcome.errorCode;
          finalAssistant.terminal_outcome = outcome.outcome;
          finalAssistant.updated_at = settledAt;
          if (!existingPlaceholder) {
            items.push(finalAssistant);
          }
          broadcastEvent({ type: "item", item: finalAssistant });
        } else {
          const noteItem: HermesItem = {
            id: assistantId,
            kind: "activity",
            created_at: settledAt,
            channel_id: channel.id,
            run_id: runIdLocal,
            terminal_outcome: outcome.outcome,
            text: "Run completed without text output",
            activity: {
              run_id: runIdLocal,
              status: "done",
              kind: "status",
              summary: "Run completed without text output",
            },
          };
          items.push(noteItem);
          broadcastEvent({ type: "item", item: noteItem });
        }

        const finalized = finalizeTurnTaskSteps(
          turnTask.steps ?? [],
          isBudgetExhausted ? "iteration_budget_exhausted" : "done",
          isBudgetExhausted ? "Iteration budget reached" : "Completed response"
        );
        turnTask.steps = finalized.steps;
        turnTask.progress = finalized.progress;
        turnTask.status = "done";
        turnTask.error_code = isBudgetExhausted ? "iteration_budget_exhausted" : undefined;
        turnTask.result = replyText.slice(0, 200);
        turnTask.agent_done_at = settledAt;
        turnTask.updated_at = settledAt;

        sessionStatusMap.set(channel.id, "idle");
        broadcastEvent({ type: "task", task: turnTask });
        broadcastEvent({ type: "session", session: buildSessionSnapshot(channel) });
      } catch (err) {
        sessionStatusMap.set(channel.id, "idle");
        const settledAt = Date.now();
        if (runController.signal.aborted) {
          const finalized = finalizeTurnTaskSteps(
            turnTask.steps ?? [],
            "cancelled",
            "Cancelled by operator"
          );
          turnTask.steps = finalized.steps;
          turnTask.progress = finalized.progress;
          turnTask.status = "cancelled";
          turnTask.updated_at = settledAt;
          broadcastEvent({ type: "task", task: turnTask });

          const existing = items.find((it) => it.id === assistantId);
          if (existing) {
            existing.streaming = false;
            existing.terminal_outcome = "cancelled";
            existing.text = existing.text ? `${existing.text}\n\n[Turn cancelled]` : "[Turn cancelled]";
            broadcastEvent({ type: "item", item: existing });
          } else {
            const stoppedItem: HermesItem = {
              id: assistantId,
              kind: "activity",
              created_at: settledAt,
              channel_id: channel.id,
              run_id: runIdLocal,
              terminal_outcome: "cancelled",
              text: "Turn cancelled",
              activity: {
                run_id: runIdLocal,
                status: "cancelled",
                kind: "status",
                summary: "Turn cancelled",
              },
            };
            items.push(stoppedItem);
            broadcastEvent({ type: "item", item: stoppedItem });
          }
        } else {
          const cfg = getGatewayConfig(channel.profile);
          const errText = `Hermes Gateway offline (${cfg.profile}) at ${cfg.baseUrl}${cfg.routePrefix}: ${
            err instanceof Error ? err.message : String(err)
          }`;
          const finalized = finalizeTurnTaskSteps(
            turnTask.steps ?? [],
            "failed",
            "Gateway unreachable"
          );
          turnTask.steps = finalized.steps;
          turnTask.progress = finalized.progress;
          turnTask.status = "failed";
          turnTask.error_code = "gateway_offline";
          turnTask.updated_at = settledAt;
          broadcastEvent({ type: "task", task: turnTask });

          const errItem: HermesItem = {
            id: assistantId,
            kind: "activity",
            created_at: settledAt,
            channel_id: channel.id,
            run_id: runIdLocal,
            error_code: "gateway_offline",
            terminal_outcome: "failed",
            text: errText,
            activity: { run_id: runIdLocal, status: "error", kind: "status", error: errText },
          };
          items.push(errItem);
          broadcastEvent({ type: "item", item: errItem });
        }
        broadcastEvent({ type: "session", session: buildSessionSnapshot(channel) });
      } finally {
        if (activeRunAborts.get(channel.id) === runController) {
          activeRunAborts.delete(channel.id);
        }
      }
    })();
  });

  // Honest direct chat endpoint (no synthesized offline replies)
  app.post("/api/hermes/chat", async (req, res) => {
    const {
      message,
      channelId = "general",
      sessionId,
      sessionKey,
      profile,
      idempotencyKey,
    } = (req.body ?? {}) as {
      message?: string;
      channelId?: string;
      sessionId?: string;
      sessionKey?: string;
      profile?: string;
      idempotencyKey?: string;
    };
    if (!message || typeof message !== "string") {
      res.status(400).json({ error: "Message is required." });
      return;
    }

    const cfg = getGatewayConfig(profile);
    const resolvedSessionId = sessionId?.trim() || `hermes-${normalizeChannelKey(channelId) || "general"}`;
    const resolvedSessionKey =
      sanitizeSessionKeyHeader(sessionKey) ||
      defaultSessionKeyForChannel(normalizeChannelKey(channelId) || "general");
    const resolvedIdem =
      sanitizeSessionKeyHeader(idempotencyKey || req.header("idempotency-key")) ||
      createIdempotencyKey("chat");

    try {
      const profileStatus = cachedGatewayStatusByProfile.get(cfg.profile);
      const gwRes = await fetchWithConcurrentRetry(() =>
        fetchHermesGateway(
          "/v1/chat/completions",
          {
            method: "POST",
            headers: {
              "Idempotency-Key": resolvedIdem,
              "X-Hermes-Session-Id": resolvedSessionId,
              "X-Hermes-Session-Key": resolvedSessionKey,
              Accept: "application/json",
            },
            body: JSON.stringify({
              model: profileStatus?.activeModel || process.env.API_SERVER_MODEL_NAME || "hermes-agent",
              messages: [{ role: "user", content: message }],
              stream: false,
            }),
          },
          HERMES_CHAT_TIMEOUT_MS,
          undefined,
          cfg.profile
        )
      );

      if (!gwRes.ok) {
        const errInfo = await readGatewayErrorBody(gwRes);
        res.status(gwRes.status).json({
          sourceMode: "gateway-error",
          error: errInfo.message,
          errorCode: errInfo.code,
          sessionId: resolvedSessionId,
        });
        return;
      }

      const parsed = parseChatCompletionsJson(await gwRes.json().catch(() => null));
      if (!parsed || parsed.outcome === "admission_conflict") {
        res.status(parsed?.admissionConflict ? 409 : 502).json({
          sourceMode: "gateway-error",
          error: parsed?.errorMessage || "Invalid gateway response",
          errorCode: parsed?.errorCode,
          sessionId: resolvedSessionId,
        });
        return;
      }

      res.status(200).json({
        reply: parsed.content,
        reasoning: parsed.reasoning,
        outcome: parsed.outcome,
        errorCode: parsed.errorCode,
        sourceMode: "hermes-gateway",
        profile: cfg.profile,
        sessionId: parsed.sessionId || resolvedSessionId,
      });
    } catch (err) {
      res.status(503).json({
        sourceMode: "gateway-offline",
        sessionId: resolvedSessionId,
        error: `Hermes Gateway unreachable (${cfg.profile}) at ${cfg.baseUrl}${cfg.routePrefix}: ${
          err instanceof Error ? err.message : String(err)
        }`,
      });
    }
  });

  // -------------------------------------------------------------------------
  // Run Controls: Stop, Approval, Clarify, Steer, Resolve-Unknown
  // -------------------------------------------------------------------------
  app.post("/api/hermes/sessions/:channelId/abort", async (req, res) => {
    const slug = normalizeChannelKey(req.params.channelId) || "general";
    const ch = channels.find((c) => c.id === slug);
    const gwRunId = activeGatewayRunIds.get(slug);
    let stoppedOnGateway = false;

    if (gwRunId) {
      try {
        const stopRes = await fetchHermesGateway(
          `/v1/runs/${encodeURIComponent(gwRunId)}/stop`,
          { method: "POST" },
          2500,
          undefined,
          ch?.profile || runProfileMap.get(gwRunId)
        );
        stoppedOnGateway = stopRes.ok;
      } catch {
        // fall through to local abort
      }
    }

    const controller = activeRunAborts.get(slug);
    if (controller && !stoppedOnGateway) {
      controller.abort();
      activeRunAborts.delete(slug);
      sessionStatusMap.set(slug, "idle");
      for (const task of tasks) {
        if (
          task.channel_id === slug &&
          task.id.startsWith("task-turn-") &&
          (task.status === "running" || task.status === "queued" || task.status === "waiting")
        ) {
          const finalized = finalizeTurnTaskSteps(
            task.steps ?? [],
            "cancelled",
            "Cancelled by operator"
          );
          task.steps = finalized.steps;
          task.progress = finalized.progress;
          task.status = "cancelled";
          task.updated_at = Date.now();
          broadcastEvent({ type: "task", task });
        }
      }
      const ch = channels.find((c) => c.id === slug);
      if (ch) {
        broadcastEvent({ type: "session", session: buildSessionSnapshot(ch) });
      }
    }
    res.status(200).json({
      ok: true,
      aborted: Boolean(controller || stoppedOnGateway),
      stoppedOnGateway,
      channelId: slug,
    });
  });

  app.post("/api/hermes/runs/:runId/approval", async (req, res) => {
    const { request_id, execution_generation, choice, card_id } = (req.body ?? {}) as {
      request_id?: string;
      execution_generation?: number;
      choice?: "once" | "always" | "deny";
      card_id?: string;
    };
    if (!request_id || typeof execution_generation !== "number" || !choice) {
      res.status(400).json({
        error: "request_id, execution_generation, and choice (once|always|deny) are required",
      });
      return;
    }

    try {
      const targetProfile = runProfileMap.get(req.params.runId);
      const r = await fetchHermesGateway(
        `/v1/runs/${encodeURIComponent(req.params.runId)}/approval`,
        {
          method: "POST",
          body: JSON.stringify({ request_id, execution_generation, choice }),
        },
        4000,
        undefined,
        targetProfile
      );
      const body = await r.json().catch(() => ({}));
      if (!r.ok) {
        res.status(r.status).json(body);
        return;
      }

      const card = pendingCards.find(
        (p) =>
          p.id === card_id ||
          (p.run_id === req.params.runId && p.card?.requestId === request_id)
      );
      if (card?.card) {
        card.card.status = choice === "deny" ? "declined" : "approved";
        card.updated_at = Date.now();
        broadcastEvent({ type: "item", item: card });
      }

      const relatedTask = tasks.find((t) => t.run_id === req.params.runId && t.status === "waiting");
      if (relatedTask && choice !== "deny") {
        relatedTask.status = "running";
        relatedTask.updated_at = Date.now();
        broadcastEvent({ type: "task", task: relatedTask });
      }

      res.status(200).json({ ok: true, result: body, item: card });
    } catch (err) {
      res.status(503).json({
        error: err instanceof Error ? err.message : "Gateway unreachable",
      });
    }
  });

  app.post("/api/hermes/runs/:runId/clarify", async (req, res) => {
    const { request_id, execution_generation, answer, card_id } = (req.body ?? {}) as {
      request_id?: string;
      execution_generation?: number;
      answer?: string;
      card_id?: string;
    };
    if (!request_id || typeof execution_generation !== "number" || typeof answer !== "string") {
      res.status(400).json({
        error: "request_id, execution_generation, and answer are required",
      });
      return;
    }
    try {
      const targetProfile = runProfileMap.get(req.params.runId);
      const r = await fetchHermesGateway(
        `/v1/runs/${encodeURIComponent(req.params.runId)}/clarify`,
        {
          method: "POST",
          body: JSON.stringify({ request_id, execution_generation, answer }),
        },
        4000,
        undefined,
        targetProfile
      );
      const body = await r.json().catch(() => ({}));
      if (!r.ok) {
        res.status(r.status).json(body);
        return;
      }
      const card = pendingCards.find((p) => p.id === card_id || p.run_id === req.params.runId);
      if (card?.card) {
        card.card.status = "approved";
        card.card.draft = answer;
        card.updated_at = Date.now();
        broadcastEvent({ type: "item", item: card });
      }
      res.status(200).json({ ok: true, result: body, item: card });
    } catch (err) {
      res.status(503).json({ error: err instanceof Error ? err.message : "Gateway unreachable" });
    }
  });

  app.post("/api/hermes/runs/:runId/steer", async (req, res) => {
    const { text } = (req.body ?? {}) as { text?: string };
    if (!text || !text.trim()) {
      res.status(400).json({ error: "text is required" });
      return;
    }
    try {
      const targetProfile = runProfileMap.get(req.params.runId);
      const r = await fetchHermesGateway(
        `/v1/runs/${encodeURIComponent(req.params.runId)}/steer`,
        { method: "POST", body: JSON.stringify({ text: text.trim() }) },
        3000,
        undefined,
        targetProfile
      );
      const body = await r.json().catch(() => ({}));
      res.status(r.status).json(body);
    } catch (err) {
      res.status(503).json({ error: err instanceof Error ? err.message : "Gateway unreachable" });
    }
  });

  app.post("/api/hermes/runs/:runId/stop", async (req, res) => {
    try {
      const targetProfile = runProfileMap.get(req.params.runId);
      const r = await fetchHermesGateway(
        `/v1/runs/${encodeURIComponent(req.params.runId)}/stop`,
        { method: "POST" },
        3000,
        undefined,
        targetProfile
      );
      const body = await r.json().catch(() => ({}));
      res.status(r.status).json(body);
    } catch (err) {
      res.status(503).json({ error: err instanceof Error ? err.message : "Gateway unreachable" });
    }
  });

  app.post("/api/hermes/runs/:runId/resolve-unknown", async (req, res) => {
    const { admission_id, execution_generation } = (req.body ?? {}) as {
      admission_id?: string;
      execution_generation?: number;
    };
    if (!admission_id || typeof execution_generation !== "number") {
      res.status(400).json({ error: "admission_id and execution_generation are required" });
      return;
    }
    try {
      const targetProfile = runProfileMap.get(req.params.runId);
      const r = await fetchHermesGateway(
        `/v1/runs/${encodeURIComponent(req.params.runId)}/resolve-unknown`,
        {
          method: "POST",
          body: JSON.stringify({ admission_id, execution_generation }),
        },
        3000,
        undefined,
        targetProfile
      );
      const body = await r.json().catch(() => ({}));
      res.status(r.status).json(body);
    } catch (err) {
      res.status(503).json({ error: err instanceof Error ? err.message : "Gateway unreachable" });
    }
  });

  // -------------------------------------------------------------------------
  // Sessions CRUD synced to Gateway /api/sessions*
  // -------------------------------------------------------------------------
  app.get("/api/hermes/sessions", async (_req, res) => {
    await syncSessionsFromGateway();
    res.status(200).json({ sessions: buildAllSessionSnapshots() });
  });

  app.post("/api/hermes/sessions", async (req, res) => {
    const { channelId, name, sessionId, sessionKey, topic, persona, profile, requireFresh } =
      (req.body ?? {}) as {
        channelId?: string;
        name?: string;
        sessionId?: string;
        sessionKey?: string;
        topic?: string;
        persona?: string;
        profile?: string;
        requireFresh?: boolean;
      };
    const rawKey = channelId || name;
    if (!rawKey || typeof rawKey !== "string") {
      res.status(400).json({ error: "channelId or name is required." });
      return;
    }

    const baseKey = normalizeChannelKey(rawKey) || "session";
    const resolvedKey = requireFresh
      ? (() => {
          const taken = new Set(channels.map((c) => c.id));
          if (!taken.has(baseKey)) return baseKey;
          let n = 2;
          while (taken.has(`${baseKey}-${n}`)) n++;
          return `${baseKey}-${n}`;
        })()
      : baseKey;

    const resolvedProfile =
      profile && profile.trim() ? normalizeProfileId(profile) : getGatewayConfig().profile;

    let resolvedSessionId = sessionId?.trim() || `hermes-${resolvedKey}`;
    const resolvedSessionKey =
      sanitizeSessionKeyHeader(sessionKey) || defaultSessionKeyForChannel(resolvedKey);
    try {
      const createRes = await fetchHermesGateway(
        "/api/sessions",
        {
          method: "POST",
          body: JSON.stringify({
            session_id: resolvedSessionId,
            session_key: resolvedSessionKey,
            title: topic || name || resolvedKey,
          }),
        },
        2500,
        undefined,
        resolvedProfile
      );
      if (createRes.ok) {
        const created = (await createRes.json().catch(() => null)) as {
          session_id?: string;
          id?: string;
        } | null;
        if (created?.session_id) resolvedSessionId = created.session_id;
        else if (created?.id) resolvedSessionId = created.id;
      }
    } catch {
      // keep local session identity if gateway unreachable
    }

    workspaceSessionIds.add(resolvedSessionId);
    workspaceChannelIds.add(resolvedKey);

    const hadChannel = channels.some((c) => c.id === resolvedKey);
    const channel = upsertLocalChannel({
      channelId: resolvedKey,
      name: name || resolvedKey,
      sessionId: resolvedSessionId,
      sessionKey: resolvedSessionKey,
      profile: resolvedProfile,
      topic: topic || name || resolvedKey,
      persona,
      origin: "workspace",
      lastAt: Date.now(),
    });

    const snapshot = buildSessionSnapshot(channel);
    if (!hadChannel) {
      broadcastEvent({ type: "channel", channel });
    }
    broadcastEvent({ type: "session", session: snapshot });
    res.status(200).json({
      resolvedChannelId: channel.id,
      session: snapshot,
      channel,
      item: null,
    });
  });

  app.post("/api/hermes/sessions/:channelId/reset", async (req, res) => {
    const slug = normalizeChannelKey(req.params.channelId) || "general";
    const existing = channels.find((c) => c.id === slug);
    const targetProfile = existing?.profile || getGatewayConfig().profile;
    activeRunAborts.get(slug)?.abort();
    const gwRun = activeGatewayRunIds.get(slug);
    if (gwRun) {
      activeGatewayRunIds.delete(slug);
      void fetchHermesGateway(
        `/v1/runs/${encodeURIComponent(gwRun)}/stop`,
        { method: "POST" },
        1500,
        undefined,
        targetProfile
      ).catch(() => {});
    }

    const oldSessionId = existing?.session_id || `hermes-${slug}`;
    let nextSessionId = `${oldSessionId}-r${Date.now().toString(36).slice(-4)}`;

    // Rotate X-Hermes-Session-Id via POST /api/sessions while keeping channel.session_key stable
    try {
      const createRes = await fetchHermesGateway(
        "/api/sessions",
        {
          method: "POST",
          body: JSON.stringify({ title: existing?.topic || existing?.name || slug }),
        },
        2500,
        undefined,
        targetProfile
      );
      if (createRes.ok) {
        const created = (await createRes.json().catch(() => null)) as {
          session_id?: string;
          id?: string;
        } | null;
        if (created?.session_id) nextSessionId = created.session_id;
        else if (created?.id) nextSessionId = created.id;
      }
    } catch {
      // keep generated rotated session ID offline
    }

    workspaceSessionIds.add(nextSessionId);
    workspaceChannelIds.add(slug);

    const ch = upsertLocalChannel({
      channelId: slug,
      name: existing?.name || slug,
      sessionId: nextSessionId,
      sessionKey: existing?.session_key || defaultSessionKeyForChannel(slug),
      profile: targetProfile,
      topic: existing?.topic,
      persona: existing?.persona,
      origin: "workspace",
    });

    for (let i = items.length - 1; i >= 0; i--) {
      if ((items[i].channel_id || "general") === ch.id) {
        items.splice(i, 1);
      }
    }
    for (let i = tasks.length - 1; i >= 0; i--) {
      if ((tasks[i].channel_id || "general") === ch.id && tasks[i].id.startsWith("task-turn-")) {
        tasks.splice(i, 1);
      }
    }
    sessionStatusMap.set(ch.id, "idle");
    const snapshot = buildSessionSnapshot(ch);
    broadcastEvent({
      type: "session_reset",
      channelId: ch.id,
      session: snapshot,
      item: null,
    });
    res.status(200).json({ ok: true, session: snapshot, item: null });
  });

  app.delete("/api/hermes/sessions/:channelId", async (req, res) => {
    const channelId = normalizeChannelKey(req.params.channelId);
    const idx = channels.findIndex((c) => c.id === channelId);
    const targetChannel = idx >= 0 ? channels[idx] : undefined;
    const targetProfile = targetChannel?.profile || getGatewayConfig().profile;

    activeRunAborts.get(channelId)?.abort();
    const gwRun = activeGatewayRunIds.get(channelId);
    if (gwRun) {
      activeGatewayRunIds.delete(channelId);
      void fetchHermesGateway(
        `/v1/runs/${encodeURIComponent(gwRun)}/stop`,
        { method: "POST" },
        1500,
        undefined,
        targetProfile
      ).catch(() => {});
    }

    const targetSessionId = targetChannel?.session_id;
    if (targetSessionId) {
      await fetchHermesGateway(
        `/api/sessions/${encodeURIComponent(targetSessionId)}`,
        { method: "DELETE" },
        2500,
        undefined,
        targetProfile
      ).catch(() => {});
    }

    if (idx >= 0) channels.splice(idx, 1);
    for (let i = items.length - 1; i >= 0; i--) {
      if ((items[i].channel_id || "") === channelId) {
        items.splice(i, 1);
      }
    }
    for (let i = tasks.length - 1; i >= 0; i--) {
      if ((tasks[i].channel_id || "") === channelId) {
        tasks.splice(i, 1);
      }
    }
    sessionStatusMap.delete(channelId);
    broadcastEvent({ type: "session_deleted", channelId });
    res.status(200).json({ ok: true, channelId });
  });

  // -------------------------------------------------------------------------
  // Turn Task Confirmation / Cancellation / Steering
  // -------------------------------------------------------------------------
  app.post("/api/hermes/tasks/:taskId/confirm", (req, res) => {
    const task = tasks.find((t) => t.id === req.params.taskId);
    if (!task) {
      res.status(404).json({ error: "task not found" });
      return;
    }
    const now = Date.now();
    task.status = "done";
    task.agent_done_at = task.agent_done_at ?? now;
    task.confirmed_at = now;
    task.updated_at = now;
    broadcastEvent({ type: "task", task });
    res.status(200).json({ ok: true, task });
  });

  app.post("/api/hermes/tasks/:taskId/reopen", (req, res) => {
    const task = tasks.find((t) => t.id === req.params.taskId);
    if (!task) {
      res.status(404).json({ error: "task not found" });
      return;
    }
    delete task.confirmed_at;
    task.status = task.agent_done_at ? "done" : "waiting";
    task.updated_at = Date.now();
    broadcastEvent({ type: "task", task });
    res.status(200).json({ ok: true, task });
  });

  app.post("/api/hermes/tasks/:taskId/cancel", async (req, res) => {
    const task = tasks.find((t) => t.id === req.params.taskId);
    if (!task) {
      res.status(404).json({ error: "task not found" });
      return;
    }
    const chSlug = normalizeChannelKey(task.channel_id || "general") || "general";
    const ch = channels.find((c) => c.id === chSlug);
    const gwRunId = task.run_id || activeGatewayRunIds.get(chSlug);
    const targetProfile = task.profile || ch?.profile || (gwRunId ? runProfileMap.get(gwRunId) : undefined);
    if (gwRunId) {
      await fetchHermesGateway(
        `/v1/runs/${encodeURIComponent(gwRunId)}/stop`,
        { method: "POST" },
        2500,
        undefined,
        targetProfile
      ).catch(() => {});
    }

    const controller = activeRunAborts.get(chSlug);
    if (controller && !gwRunId) {
      controller.abort();
      activeRunAborts.delete(chSlug);
    }

    if (!gwRunId) {
      sessionStatusMap.set(chSlug, "idle");
      const finalized = finalizeTurnTaskSteps(
        task.steps ?? [],
        "cancelled",
        "Cancelled by operator"
      );
      task.steps = finalized.steps;
      task.progress = finalized.progress;
      task.status = "cancelled";
      task.updated_at = Date.now();
      const ch = channels.find((c) => c.id === chSlug);
      if (ch) {
        broadcastEvent({ type: "session", session: buildSessionSnapshot(ch) });
      }
      broadcastEvent({ type: "task", task });
    }
    res.status(200).json({ ok: true, task });
  });

  app.post("/api/hermes/tasks/clear", (req, res) => {
    const { channelId } = (req.body ?? {}) as { channelId?: string };
    const chSlug = channelId ? normalizeChannelKey(channelId) : "";
    let removed = 0;
    for (let i = tasks.length - 1; i >= 0; i--) {
      const t = tasks[i];
      if (chSlug && (t.channel_id || "") !== chSlug) continue;
      if (t.confirmed_at || t.status === "cancelled") {
        tasks.splice(i, 1);
        removed += 1;
      }
    }
    res.status(200).json({ ok: true, removed });
  });

  app.post("/api/hermes/tasks/:taskId/message", async (req, res) => {
    const { text } = (req.body ?? {}) as { text?: string };
    if (!text || typeof text !== "string" || !text.trim()) {
      res.status(400).json({ error: "text is required" });
      return;
    }
    const task = tasks.find((t) => t.id === req.params.taskId);
    if (!task) {
      res.status(404).json({ error: "task not found" });
      return;
    }
    const chSlug = normalizeChannelKey(task.channel_id || "general") || "general";
    const ch = channels.find((c) => c.id === chSlug);
    const gwRunId = task.run_id || activeGatewayRunIds.get(chSlug);
    const targetProfile = task.profile || ch?.profile || (gwRunId ? runProfileMap.get(gwRunId) : undefined);
    if (gwRunId) {
      try {
        const steerRes = await fetchHermesGateway(
          `/v1/runs/${encodeURIComponent(gwRunId)}/steer`,
          { method: "POST", body: JSON.stringify({ text: text.trim() }) },
          3000,
          undefined,
          targetProfile
        );
        if (!steerRes.ok) {
          const err = await readGatewayErrorBody(steerRes);
          res.status(steerRes.status).json({ error: err.message });
          return;
        }
      } catch (err) {
        res.status(503).json({
          error: err instanceof Error ? err.message : "Gateway unreachable",
        });
        return;
      }
    }
    const clean = text.trim();
    task.steps = [...(task.steps ?? []), { text: `Steer: ${clean}`, done: false }];
    const doneCount = task.steps.filter((s) => s.done).length;
    task.progress = `${doneCount}/${task.steps.length}`;
    task.updated_at = Date.now();
    broadcastEvent({ type: "task", task });
    res.status(200).json({ ok: true, task });
  });

  // -------------------------------------------------------------------------
  // SSE Event Stream to Browser UI
  // -------------------------------------------------------------------------
  app.get("/api/hermes/events", (req, res) => {
    res.writeHead(200, {
      "Content-Type": "text/event-stream",
      "Cache-Control": "no-cache, no-transform",
      Connection: "keep-alive",
      "X-Accel-Buffering": "no",
    });
    res.write(": hermes-agent-connected\n\n");
    sseClients.add(res);
    const keepalive = setInterval(() => {
      try {
        res.write(": ping\n\n");
      } catch {
        clearInterval(keepalive);
        sseClients.delete(res);
      }
    }, 25000);
    req.on("close", () => {
      clearInterval(keepalive);
      sseClients.delete(res);
    });
  });

  app.post("/api/hermes/tts", async (req, res) => {
    try {
      const { text, mood = "neutral" } = req.body ?? {};
      if (!text || typeof text !== "string") {
        res.status(400).json({ error: "Text is required for speech synthesis." });
        return;
      }

      const cleanText = text.replace(/[\r\n]+/g, " ").trim().slice(0, 1200);
      if (!cleanText) {
        res.status(200).json({ audioBase64: null, fallback: "speechSynthesis", provider: "browser" });
        return;
      }

      const deepgramKey = process.env.DEEPGRAM_API_KEY;
      if (deepgramKey && deepgramKey !== "MY_DEEPGRAM_API_KEY") {
        const voiceByMood: Record<string, string> = {
          happy: "aura-2-thalia-en",
          focused: "aura-2-andromeda-en",
          thinking: "aura-2-helena-en",
          alert: "aura-2-stella-en",
          neutral: "aura-2-thalia-en",
          speaking: "aura-2-thalia-en",
          listening: "aura-2-thalia-en",
        };
        const model = voiceByMood[String(mood)] || "aura-2-thalia-en";

        try {
          const controller = new AbortController();
          const timer = setTimeout(() => controller.abort(), 7000);
          const dgRes = await fetch(
            `https://api.deepgram.com/v1/speak?model=${encodeURIComponent(model)}&encoding=linear16&container=wav&sample_rate=24000`,
            {
              method: "POST",
              headers: {
                Authorization: `Token ${deepgramKey}`,
                "Content-Type": "application/json",
              },
              body: JSON.stringify({ text: cleanText }),
              signal: controller.signal,
            }
          );
          clearTimeout(timer);

          if (dgRes.ok) {
            const wavBuf = Buffer.from(await dgRes.arrayBuffer());
            if (wavBuf.length > 44) {
              res.json({
                audioBase64: wavBuf.toString("base64"),
                mimeType: "audio/wav",
                provider: "deepgram",
                model,
              });
              return;
            }
          }
        } catch {
          // Fall through to browser fallback cleanly
        }
      }

      res.status(200).json({
        audioBase64: null,
        fallback: "speechSynthesis",
        provider: "browser",
      });
    } catch {
      res.status(200).json({
        audioBase64: null,
        fallback: "speechSynthesis",
        provider: "browser",
      });
    }
  });

  // -------------------------------------------------------------------------
  // Durable Preferences Vault & Click-to-Update Engine (survives updates/ports)
  // -------------------------------------------------------------------------
  app.get("/api/preferences", (_req, res) => {
    res.json({
      ok: true,
      vault: preferencesStore.readVault(),
      backups: preferencesStore.listBackups(),
      storageLabel: preferencesStore.vaultPath.startsWith(os.homedir())
        ? "~/.voxcell/preferences-vault.json"
        : "Server preferences vault",
    });
  });

  app.put("/api/preferences", (req, res) => {
    const { snapshot: rawSnapshot, reason } = (req.body ?? {}) as {
      snapshot?: unknown;
      reason?: "sync" | "manual" | "pre-update" | "restore" | "import";
    };
    const normalized = normalizePreferencesSnapshot(
      rawSnapshot,
      readPackageVersion(__dirname)
    );
    if (!normalized) {
      res.status(400).json({ ok: false, error: "Invalid preferences snapshot payload" });
      return;
    }
    const saved = preferencesStore.writeVault(normalized, reason);
    res.json({ ok: true, vault: saved });
  });

  app.post("/api/preferences/backup", (req, res) => {
    const { snapshot: rawSnapshot, reason } = (req.body ?? {}) as {
      snapshot?: unknown;
      reason?: "manual" | "pre-update";
    };
    const candidate = rawSnapshot
      ? normalizePreferencesSnapshot(rawSnapshot, readPackageVersion(__dirname))
      : preferencesStore.readVault();
    if (!candidate) {
      res.status(400).json({ ok: false, error: "No valid preferences snapshot to back up" });
      return;
    }
    const result = preferencesStore.createBackup(
      candidate,
      reason === "pre-update" ? "pre-update" : "manual"
    );
    res.json({ ok: true, backup: result.backup, backups: result.backups });
  });

  app.post("/api/preferences/restore", (req, res) => {
    const { backupId } = (req.body ?? {}) as { backupId?: string };
    if (!backupId || typeof backupId !== "string") {
      res.status(400).json({ ok: false, error: "backupId is required" });
      return;
    }
    const restored = preferencesStore.restoreBackup(backupId);
    if (!restored) {
      res.status(404).json({ ok: false, error: "Restore point not found" });
      return;
    }
    res.json({ ok: true, snapshot: restored.snapshot, backups: restored.backups });
  });

  async function resolveUpdateStatus(forceRefresh = false) {
    const [local, upstream] = await Promise.all([
      readLocalBuildMetadata(__dirname),
      (async () => {
        const now = Date.now();
        if (
          !forceRefresh &&
          cachedUpstreamRelease &&
          now - cachedUpstreamRelease.fetchedAt < UPSTREAM_CACHE_TTL_MS
        ) {
          return cachedUpstreamRelease.info;
        }
        const info = await fetchUpstreamReleaseInfo();
        cachedUpstreamRelease = { info, fetchedAt: now };
        return info;
      })(),
    ]);
    const upstreamAncestorOfLocal =
      local.hasGitRepo && local.commit && upstream?.latestCommit
        ? await isCommitAncestorOfHead(__dirname, upstream.latestCommit)
        : null;
    return buildAppUpdateStatus({
      local,
      upstream,
      receipt: preferencesStore.readReceipt(),
      upstreamAncestorOfLocal,
    });
  }

  app.get("/api/app/update/status", async (req, res) => {
    const forceRefresh = req.query.refresh === "1" || req.query.refresh === "true";
    const status = await resolveUpdateStatus(forceRefresh);
    res.json(status);
  });

  app.post("/api/app/update/apply", async (req, res) => {
    const { snapshot: rawSnapshot } = (req.body ?? {}) as { snapshot?: unknown };
    const currentVersion = readPackageVersion(__dirname);
    const snapshot =
      normalizePreferencesSnapshot(rawSnapshot, currentVersion) ?? preferencesStore.readVault();

    if (!snapshot) {
      res.status(400).json({
        ok: false,
        steps: [],
        needsReload: false,
        restarting: false,
        error: "Unable to snapshot user preferences before update.",
      });
      return;
    }

    // 1. Lock user themes, custom backdrops, and settings into durable vault + restore point
    const steps: UpdateStepResult[] = [];
    const { backup } = preferencesStore.createBackup(snapshot, "pre-update");
    steps.push({
      id: "vault-backup",
      label: `Locked themes & settings (${backup.id})`,
      status: "ok",
    });

    const local = await readLocalBuildMetadata(__dirname);
    const preservedCategories = summarizePreservedCategories(snapshot)
      .filter((c) => c.customized)
      .map((c) => c.label);
    const effectiveCategories =
      preservedCategories.length > 0
        ? preservedCategories
        : ["Theme & Colors", "Layout & Typography", "Stage, Voice & Trace"];

    if (local.hasGitRepo) {
      if (local.dirtyWorktree) {
        steps.push({
          id: "git-check",
          label: "Checked repository cleanliness",
          status: "failed",
          detail: local.dirtyReason || "Local uncommitted modifications detected",
        });
        const status = await resolveUpdateStatus(false);
        res.status(409).json({
          ok: false,
          status,
          steps,
          needsReload: false,
          restarting: false,
          error:
            "Pre-update snapshot saved safely, but git update was paused because local files have uncommitted edits.",
        });
        return;
      }

      const gitOutcome = await applyGitPullUpdate(__dirname);
      steps.push(...gitOutcome.steps);
      if (!gitOutcome.ok) {
        const status = await resolveUpdateStatus(false);
        res.status(500).json({
          ok: false,
          status,
          steps,
          needsReload: false,
          restarting: false,
          error: gitOutcome.error || "Update failed (your themes & settings backup is intact).",
        });
        return;
      }

      const receipt = {
        updatedAt: Date.now(),
        previousVersion: local.version,
        newVersion: gitOutcome.newVersion,
        previousCommit: local.commit,
        newCommit: gitOutcome.newCommit,
        backupId: backup.id,
        mode: "git-pull" as const,
        preservedCategories: effectiveCategories,
      };
      preferencesStore.writeReceipt(receipt);
      cachedUpstreamRelease = null;
      const status = await resolveUpdateStatus(true);
      res.json({
        ok: true,
        status,
        receipt,
        steps,
        needsReload: true,
        restarting: false,
      });
      return;
    }

    // Managed runtime / non-git environment: lock durable vault, refresh upstream check, write receipt
    cachedUpstreamRelease = null;
    steps.push({
      id: "vault-verify",
      label: "Verified durable preferences vault on disk",
      status: "ok",
      detail: `${effectiveCategories.length} setting groups protected`,
    });
    const receipt = {
      updatedAt: Date.now(),
      previousVersion: local.version,
      newVersion: local.version,
      previousCommit: null,
      newCommit: null,
      backupId: backup.id,
      mode: "managed-refresh" as const,
      preservedCategories: effectiveCategories,
    };
    preferencesStore.writeReceipt(receipt);
    steps.push({
      id: "runtime-sync",
      label: "Synchronized update receipt for post-reload restore",
      status: "ok",
    });
    const status = await resolveUpdateStatus(true);
    res.json({
      ok: true,
      status,
      receipt,
      steps,
      needsReload: true,
      restarting: false,
    });
  });

  app.post("/api/app/update/ack", (_req, res) => {
    preferencesStore.clearReceipt();
    res.json({ ok: true });
  });

  const httpServer = createHttpServer(app);

  // Unknown /api/* paths must fail as JSON 404s. Without this the SPA fallback
  // (and vite's middleware chain in dev) answers them with index.html, so a
  // client bug — typo'd route, stale client against a newer server — silently
  // parses markup as data instead of failing loudly.
  app.use("/api", (_req, res) => {
    res.status(404).json({ error: "not_found", path: `/api${_req.path}` });
  });

  let vite: { close: () => Promise<void>; middlewares: express.RequestHandler } | null = null;
  if (process.env.NODE_ENV !== "production") {
    const { createServer: createViteServer } = await import("vite");
    const disableHmr = process.env.DISABLE_HMR === "true";
    const hmrConfig = disableHmr ? false : { server: httpServer };
    vite = await createViteServer({
      root: __dirname,
      server: {
        middlewareMode: true,
        hmr: hmrConfig,
        ...(disableHmr ? { watch: null } : {}),
      },
      appType: "spa",
    });
    app.use(vite.middlewares);
  } else {
    const distDir = path.join(__dirname, "dist");
    app.use(express.static(distDir));
    app.get("*", (_req, res) => {
      res.sendFile(path.join(distDir, "index.html"));
    });
  }

  httpServer.on("error", (err: NodeJS.ErrnoException) => {
    if (err.code === "EADDRINUSE") {
      console.error(
        `Port ${PORT} is already in use on this machine — another app or an old ` +
          `instance of this server owns it. Start on a free port instead: ` +
          `npm run dev -- --port 3100`
      );
      process.exit(1);
    }
    throw err;
  });

  httpServer.listen(PORT, HOST, () => {
    console.log(`Hermes Agent server running on http://${HOST}:${PORT}`);
    // Windows lets two processes bind the same port (SO_REUSEADDR), so a
    // "successful" listen can still be shadowed — e.g. Docker's loopback
    // forward owns 127.0.0.1:3000 and browsers hit it instead of us. Probe
    // the user-facing address and warn when something else answers.
    if (HOST === "0.0.0.0" || HOST === "127.0.0.1" || HOST === "localhost") {
      fetch(`http://127.0.0.1:${PORT}/api/health`)
        .then(async (res) => {
          const body = (await res.json().catch(() => null)) as { bootId?: string } | null;
          if (body?.bootId !== BOOT_ID) {
            console.warn(
              `WARNING: another process is answering on 127.0.0.1:${PORT} — NOT this ` +
                `server (a Docker forward or an old instance). Browsers opening ` +
                `http://localhost:${PORT} will hit the wrong app. Restart with ` +
                `--port 3100.`
            );
          }
        })
        .catch(() => {
          // Probe transport failed; the listen callback above already ran.
        });
    }
  });

  const shutdown = () => {
    httpServer.close(() => {
      void vite?.close().finally(() => process.exit(0));
    });
    httpServer.closeAllConnections?.();
    setTimeout(() => process.exit(0), 1500).unref();
  };
  process.on("SIGTERM", shutdown);
  process.on("SIGINT", shutdown);
}

createServer().catch((err) => {
  console.error("Server failed to start:", err);
  process.exit(1);
});
