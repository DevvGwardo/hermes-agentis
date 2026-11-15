import "dotenv/config";
import express from "express";
import path from "node:path";
import { readFileSync, rmSync, writeFileSync } from "node:fs";
import { Readable } from "node:stream";
import { fileURLToPath } from "node:url";
import {
  DEFAULT_UI_CONFIG,
  applyUiPatch,
  parseNaturalUiIntent,
  sanitizeUiPatch,
  type UiCustomizationConfig,
  type UiCustomizationPatch,
} from "./src/utils/uiCustomization.ts";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const PORT = Number(process.env.PORT || 3000);

const HERMES_API_URL = (process.env.HERMES_API_URL || "http://127.0.0.1:8642").replace(/\/+$/, "");
const HERMES_API_KEY = process.env.HERMES_API_KEY || "";
const HERMES_PROFILE = process.env.HERMES_PROFILE || "default";

// ---------------------------------------------------------------------------
// Hermes Duet adapter: this UI stays, but all state/runs/approvals come from
// the real Hermes Duet server. We pair as a device (same flow as the Duet web
// client): internal pair-code -> /api/pair -> duet_device cookie, stored locally.
const DUET_BASE_URL = (process.env.DUET_BASE_URL || "http://127.0.0.1:7456").replace(/\/+$/, "");
const DUET_INTERNAL_URL = (process.env.DUET_INTERNAL_URL || "http://127.0.0.1:7457").replace(/\/+$/, "");
const DUET_DEVICE_NAME = process.env.DUET_DEVICE_NAME || "hermes-agentis";
const DUET_DEVICE_FILE = path.join(__dirname, ".duet-device");
const DUET_INTERNAL_TOKEN =
  process.env.DUET_INTERNAL_TOKEN ||
  (() => {
    const candidates = [
      process.env.DUET_TOKEN_FILE,
      path.join(__dirname, "..", "hermes-duet", "data", "internal.token"),
    ].filter(Boolean) as string[];
    for (const file of candidates) {
      try {
        const t = readFileSync(file, "utf8").trim();
        if (t) return t;
      } catch {
        // try next candidate
      }
    }
    return "";
  })();

let duetDeviceToken: string | null = null;

async function loadDuetDeviceToken(): Promise<string | null> {
  if (duetDeviceToken) return duetDeviceToken;
  try {
    duetDeviceToken = readFileSync(DUET_DEVICE_FILE, "utf8").trim() || null;
  } catch {
    duetDeviceToken = null;
  }
  return duetDeviceToken;
}

async function pairWithDuet(): Promise<string | null> {
  if (!DUET_INTERNAL_TOKEN) return null;
  try {
    const pairRes = await fetch(`${DUET_INTERNAL_URL}/internal/pair`, {
      method: "POST",
      headers: { Authorization: `Bearer ${DUET_INTERNAL_TOKEN}` },
      signal: AbortSignal.timeout(5000),
    });
    if (!pairRes.ok) return null;
    const pairData = (await pairRes.json()) as { code?: string };
    if (!pairData.code) return null;

    const redeemRes = await fetch(`${DUET_BASE_URL}/api/pair`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ code: pairData.code, device_name: DUET_DEVICE_NAME }),
      signal: AbortSignal.timeout(5000),
    });
    if (!redeemRes.ok) return null;
    const setCookie = redeemRes.headers.get("set-cookie") || "";
    const cookiePair = setCookie.split(";")[0];
    if (!cookiePair.startsWith("duet_device=")) return null;
    duetDeviceToken = cookiePair.slice("duet_device=".length);
    try {
      writeFileSync(DUET_DEVICE_FILE, duetDeviceToken, { encoding: "utf8", mode: 0o600 });
    } catch {
      // non-fatal: next start re-pairs
    }
    console.log(`[duet] paired as device "${DUET_DEVICE_NAME}"`);
    return duetDeviceToken;
  } catch {
    return null;
  }
}

/** Authenticated call into the Duet server; re-pairs once on 401. */
async function duetFetch(pathname: string, init: RequestInit = {}, retried = false): Promise<Response> {
  let token = await loadDuetDeviceToken();
  if (!token) {
    token = await pairWithDuet();
    if (!token) throw new Error("duet-not-paired");
  }
  const res = await fetch(`${DUET_BASE_URL}${pathname}`, {
    ...init,
    signal: init.signal ?? AbortSignal.timeout(2500),
    headers: { ...(init.headers as Record<string, string> | undefined), cookie: `duet_device=${token}` },
  });
  if (res.status === 401 && !retried) {
    duetDeviceToken = null;
    try {
      rmSync(DUET_DEVICE_FILE);
    } catch {
      // ignore
    }
    return duetFetch(pathname, init, true);
  }
  return res;
}

// ---------------------------------------------------------------------------
// Channel identity bridge: the canvas dots and standalone lanes key channels by
// slug ("ops"), while the real Duet server uses "chan-<slug>" ids. Resolve the
// slug before proxying, and create the real lane on first use so every canvas
// shape behaves as a genuine isolated hermes-agent lane (persona becomes the
// channel's standing instructions).
interface DuetUpstreamChannel {
  id: string;
  slug?: string;
  name?: string;
  session_id?: string;
  topic?: string | null;
  instructions?: string | null;
  archived?: boolean;
}

const duetChannelCache = new Map<string, string>(); // normalized slug/id -> real channel id
const duetChannelMetaCache = new Map<string, DuetUpstreamChannel>(); // real channel id -> upstream channel
let duetChannelCacheAt = 0;

function normalizeChannelKey(raw?: string | null): string {
  return String(raw ?? "")
    .replace(/^chan-/i, "")
    .toLowerCase()
    .trim()
    .replace(/[^a-z0-9-_]+/g, "-")
    .replace(/^-+|-+$/g, "");
}

function cacheUpstreamChannel(ch: DuetUpstreamChannel) {
  if (!ch?.id) return;
  duetChannelMetaCache.set(ch.id, ch);
  if (ch.archived) return;
  const keys = [
    normalizeChannelKey(ch.id),
    normalizeChannelKey(ch.slug),
    normalizeChannelKey(ch.name),
  ].filter(Boolean);
  for (const k of keys) {
    duetChannelCache.set(k, ch.id);
  }
}

async function refreshDuetChannelCache(force = false): Promise<boolean> {
  if (!force && duetChannelCache.size > 0 && Date.now() - duetChannelCacheAt < 15_000) return true;
  try {
    const r = await duetFetch("/api/state", { signal: AbortSignal.timeout(4000) });
    if (!r.ok) return false;
    const snap = (await r.json().catch(() => null)) as {
      channels?: DuetUpstreamChannel[];
    } | null;
    if (!snap?.channels) return false;
    duetChannelCache.clear();
    duetChannelMetaCache.clear();
    for (const ch of snap.channels) {
      cacheUpstreamChannel(ch);
    }
    duetChannelCacheAt = Date.now();
    return true;
  } catch {
    return false;
  }
}

function pickFreshChannelSlug(desiredSlug: string, extraTaken: Iterable<string> = []): string {
  const taken = new Set<string>([
    "general",
    ...duetChannelCache.keys(),
    ...Array.from(extraTaken, (s) => normalizeChannelKey(s)).filter(Boolean),
  ]);
  const rawBase = normalizeChannelKey(desiredSlug) || "session";
  const base =
    (rawBase === "general" ? "general-session" : rawBase)
      .slice(0, 24)
      .replace(/-+$/g, "") || "session";
  if (!taken.has(base)) return base;
  let n = 2;
  while (true) {
    const suffix = `-${n}`;
    const head = base.slice(0, Math.max(1, 24 - suffix.length)).replace(/-+$/g, "") || "s";
    const candidate = `${head}${suffix}`;
    if (!taken.has(candidate)) return candidate;
    n++;
  }
}

/**
 * Resolve or create the real Duet channel record for a local slug, optionally
 * guaranteeing a brand-new non-colliding channel when `requireFresh` is true.
 */
async function ensureDuetChannelRecord(
  localId: string,
  meta?: { name?: string; topic?: string; persona?: string },
  opts?: { requireFresh?: boolean }
): Promise<{ channel: DuetUpstreamChannel; created: boolean } | null> {
  const baseSlug = normalizeChannelKey(localId) || "general";
  try {
    const refreshed = await refreshDuetChannelCache(Boolean(opts?.requireFresh));
    if (!opts?.requireFresh) {
      const hitId = duetChannelCache.get(baseSlug);
      if (hitId) {
        const hitMeta = duetChannelMetaCache.get(hitId) ?? { id: hitId, slug: baseSlug };
        return { channel: hitMeta, created: false };
      }
    }

    if (!refreshed && !(await loadDuetDeviceToken()) && !DUET_INTERNAL_TOKEN) {
      return null;
    }

    const standalone = standaloneChannels.find((c) => c.id === baseSlug);
    const targetSlug = opts?.requireFresh ? pickFreshChannelSlug(baseSlug) : baseSlug;
    const rawName = (opts?.requireFresh ? targetSlug : meta?.name || standalone?.name || targetSlug).replace(/^#/, "");
    const topic = meta?.topic ?? standalone?.topic ?? undefined;
    const persona = meta?.persona ?? standalone?.persona ?? undefined;

    const postCreate = async (slugName: string) => {
      const createRes = await duetFetch("/api/channels", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        signal: AbortSignal.timeout(4500),
        body: JSON.stringify({
          name: slugName,
          ...(topic ? { topic } : {}),
          ...(persona ? { instructions: persona } : {}),
        }),
      });
      if (!createRes.ok) return null;
      const created = (await createRes.json().catch(() => null)) as {
        channel?: DuetUpstreamChannel & { last_at?: number | null };
      } | null;
      return created?.channel?.id ? created.channel : null;
    };

    let createdChannel = await postCreate(rawName);
    // If hermes-duet un-archived an old channel (has prior last_at) while requireFresh=true,
    // mint a guaranteed fresh slug with a short time suffix so the new Home session stays clean.
    if (createdChannel && opts?.requireFresh && createdChannel.last_at != null) {
      const freshTag = Date.now().toString(36).slice(-4);
      const uniqueName = `${targetSlug.slice(0, 19).replace(/-+$/g, "") || "s"}-${freshTag}`;
      const retried = await postCreate(uniqueName);
      if (retried) createdChannel = retried;
    }

    if (createdChannel?.id) {
      cacheUpstreamChannel(createdChannel);
      const resolvedKey = normalizeChannelKey(createdChannel.slug || targetSlug) || targetSlug;
      duetChannelCache.set(resolvedKey, createdChannel.id);
      duetChannelCacheAt = Date.now();
      return { channel: createdChannel, created: true };
    } else if (!opts?.requireFresh) {
      // Creation refused (duplicate name, "already a general channel"): refresh
      // cache and match existing upstream lane.
      duetChannelCacheAt = 0;
      if (await refreshDuetChannelCache(true)) {
        const hitId = duetChannelCache.get(targetSlug) ?? duetChannelCache.get(baseSlug);
        if (hitId) {
          return {
            channel: duetChannelMetaCache.get(hitId) ?? { id: hitId, slug: baseSlug },
            created: false,
          };
        }
      }
    }
  } catch {
    // Duet unreachable: caller falls back to standalone handling.
  }
  return null;
}

/**
 * Resolve a local (slug) channel id to the real Duet channel id, creating the
 * upstream lane when it does not exist yet.
 */
async function ensureDuetChannel(
  localId: string,
  meta?: { name?: string; topic?: string; persona?: string }
): Promise<string | null> {
  const resolved = await ensureDuetChannelRecord(localId, meta);
  return resolved?.channel.id ?? null;
}

// ---------------------------------------------------------------------------
// Standalone in-memory Duet fallback when no external hermes-duet daemon is
// reachable (e.g. AI Studio container). Keeps real Duet protocol shapes intact.
interface StandaloneDuetCard {
  type: "approval" | "payment" | "brief";
  title?: string;
  body?: string;
  status: "pending" | "approved" | "declined";
  draft?: string;
  action?: string;
  destination?: string;
  entries?: { title?: string; summary?: string }[];
}

interface StandaloneDuetItem {
  id: string;
  kind: "user" | "assistant" | "card" | "activity" | "system";
  created_at: number;
  updated_at?: number;
  channel_id?: string;
  text?: string;
  streaming?: boolean;
  run_id?: string | null;
  card?: StandaloneDuetCard;
  ui_patch?: UiCustomizationPatch;
}

interface StandaloneDuetTask {
  id: string;
  title: string;
  status: string;
  progress?: string;
  steps?: { text: string; done?: boolean }[];
  result?: string;
  channel_id?: string;
  created_at: number;
}

interface StandaloneDuetChannel {
  id: string;
  name: string;
  session_id: string;
  topic?: string | null;
  persona?: string | null;
  archived?: boolean;
  last_at?: number | null;
}

type StandaloneSessionStatus = "idle" | "thinking" | "running";

const standaloneBootTime = Date.now();

const standaloneChannels: StandaloneDuetChannel[] = [
  {
    id: "general",
    name: "general",
    session_id: "hermes-general",
    topic: "Primary Hermes Agent voice thread, daily check-ins, and morning brief",
    persona: "Calm primary Hermes coordinator styled after the main cyber-bob operator.",
  },
  {
    id: "ops",
    name: "ops",
    session_id: "hermes-ops",
    topic: "hermes gateway (:8642), cronjob_manage schedules, and payment gates",
    persona: "Steady ops & morning-brief keeper guarding cron schedules.",
  },
  {
    id: "launch",
    name: "launch",
    session_id: "hermes-launch",
    topic: "~/.hermes/skills/ workflows, save_page rebuilds, and QA checks",
    persona: "High-energy release and save_page builder running fast launch turns.",
  },
  {
    id: "research",
    name: "research",
    session_id: "hermes-research",
    topic: "Isolated background runs and persistent MEMORY.md synthesis",
    persona: "Quiet deep-research session focused on long-horizon synthesis.",
  },
  {
    id: "daemon",
    name: "daemon",
    session_id: "hermes-daemon",
    topic: "Background cron watcher & ambient signal daemon",
    persona: "Stealthy background daemon monitoring cron watchers and ambient events.",
  },
];

const standalonePending: StandaloneDuetItem[] = [
  {
    id: "card-1",
    kind: "card",
    created_at: standaloneBootTime - 300_000,
    channel_id: "general",
    run_id: "run-duet-781",
    card: {
      type: "approval",
      title: "Send Release Dispatch via Mail MCP",
      body: "Dispatches the v2.4 Hermes Agent voice link & Duet MCP changelog to 184 recipients.",
      status: "pending",
      destination: "mail://beta-cohort (184 recipients)",
      draft: "Hermes Duet v2.4 is live: direct Hermes Gateway (/v1/runs) voice link, mcp__duet approval gating, and live Braille decoding are now active.",
    },
  },
  {
    id: "card-2",
    kind: "card",
    created_at: standaloneBootTime - 180_000,
    channel_id: "ops",
    run_id: "run-duet-784",
    card: {
      type: "payment",
      action: "pay",
      title: "Authorize Inference Burst Virtual Cap",
      body: "Itemized receipt: $45.00 overflow ceiling (under $500.00 DUET_MAX_TRANSACTION_LIMIT).",
      status: "pending",
      destination: "nous://inference-gateway (Single-use cap)",
      draft: "Authorize $45.00 single-use virtual card for local Hermes inference overflow.",
    },
  },
  {
    id: "brief-1",
    kind: "card",
    created_at: standaloneBootTime - 120_000,
    channel_id: "general",
    card: {
      type: "brief",
      title: "Morning Brief Decisions",
      status: "pending",
      entries: [
        {
          title: "Approve Mail MCP release dispatch via mcp__duet__request_approval",
          summary: "Hermes Agent prepared the v2.4 Duet release note for 184 subscribers in lane duet-general.",
        },
      ],
    },
  },
];

const standaloneTasks: StandaloneDuetTask[] = [
  {
    id: "task-101",
    title: "Rebuild standing morning-brief.html (save_page)",
    status: "running",
    progress: "72",
    channel_id: "launch",
    created_at: standaloneBootTime - 240_000,
    steps: [{ text: "Streaming update_task delta across Playwright parity lane", done: false }],
  },
  {
    id: "task-102",
    title: "Index ~/.hermes/skills/ & MCP connectors",
    status: "done",
    progress: "100",
    channel_id: "research",
    created_at: standaloneBootTime - 600_000,
    steps: [{ text: "Synced 4 skills and mcp_servers.duet capabilities to #research", done: true }],
  },
  {
    id: "task-103",
    title: "Audit ~/Downloads watcher rule (cronjob_manage)",
    status: "waiting",
    progress: "40",
    channel_id: "ops",
    created_at: standaloneBootTime - 420_000,
    steps: [{ text: "Waiting on confirmation to route invoice PDFs into #ops", done: false }],
  },
];

const standaloneItems: StandaloneDuetItem[] = [
  {
    id: "init-welcome-general",
    kind: "assistant",
    created_at: standaloneBootTime - 60_000,
    channel_id: "general",
    text: "Session hermes-general online (Cyber Bob). Two mcp__duet approval gates are waiting—tap Voice or type /help.",
  },
  {
    id: "init-welcome-ops",
    kind: "assistant",
    created_at: standaloneBootTime - 58_000,
    channel_id: "ops",
    text: "Session hermes-ops online (Seraph). Monitoring :8642 gateway, cronjob_manage schedules, and $45.00 cap approval.",
  },
  {
    id: "init-welcome-launch",
    kind: "assistant",
    created_at: standaloneBootTime - 56_000,
    channel_id: "launch",
    text: "Session hermes-launch online (Twintails). Tracking save_page rebuilds and QA parity checks cleanly.",
  },
  {
    id: "init-welcome-research",
    kind: "assistant",
    created_at: standaloneBootTime - 54_000,
    channel_id: "research",
    text: "Session hermes-research online (Yume). Isolated MEMORY.md synthesis and ~/.hermes/skills/ index ready.",
  },
  {
    id: "init-welcome-daemon",
    kind: "assistant",
    created_at: standaloneBootTime - 52_000,
    channel_id: "daemon",
    text: "Session hermes-daemon online (Neko). Silent watcher lane listening for cron triggers and ambient events.",
  },
];

const standaloneSessionStatus = new Map<string, StandaloneSessionStatus>();

function ensureStandaloneChannel(opts: {
  channelId: string;
  name?: string;
  sessionId?: string;
  topic?: string;
  persona?: string;
}): StandaloneDuetChannel {
  const cleanId =
    opts.channelId
      .toLowerCase()
      .trim()
      .replace(/[^a-z0-9-_]+/g, "-")
      .replace(/^-+|-+$/g, "") || "session";
  let existing = standaloneChannels.find((c) => c.id === cleanId);
  if (!existing) {
    existing = {
      id: cleanId,
      name: (opts.name || cleanId).replace(/^#/, ""),
      session_id: opts.sessionId?.trim() || `hermes-${cleanId}`,
      topic: opts.topic?.trim() || `Isolated hermes-agent session #${cleanId}`,
      persona: opts.persona?.trim() || "Focused hermes-agent sub-session running an independent context.",
      last_at: Date.now(),
    };
    standaloneChannels.push(existing);
  } else {
    if (opts.name) existing.name = opts.name.replace(/^#/, "");
    if (opts.sessionId?.trim()) existing.session_id = opts.sessionId.trim();
    if (opts.topic?.trim()) existing.topic = opts.topic.trim();
    if (opts.persona?.trim()) existing.persona = opts.persona.trim();
  }
  return existing;
}

function buildSessionSnapshot(ch: StandaloneDuetChannel) {
  const chItems = standaloneItems.filter((it) => (it.channel_id || "general") === ch.id);
  const turnCount = chItems.filter((it) => it.kind === "user").length;
  const lastAssistant = [...chItems]
    .reverse()
    .find((it) => (it.kind === "assistant" || it.kind === "user") && it.text?.trim());
  const latestTimestamp = chItems.reduce((acc, it) => Math.max(acc, it.created_at), ch.last_at ?? standaloneBootTime);
  const hasRunningTask = standaloneTasks.some(
    (t) => (t.channel_id || "general") === ch.id && t.status === "running"
  );
  const explicitStatus = standaloneSessionStatus.get(ch.id);
  const status = explicitStatus ?? (hasRunningTask ? "running" : "idle");

  return {
    sessionId: ch.session_id,
    channelId: ch.id,
    name: ch.name,
    persona: ch.persona || "Hermes Agent standalone session",
    topic: ch.topic || `Hermes session #${ch.name}`,
    status,
    turnCount,
    lastMessage: lastAssistant?.text?.trim() || ch.topic || "",
    updatedAtMs: latestTimestamp,
  };
}

function buildAllSessionSnapshots() {
  return standaloneChannels.filter((c) => !c.archived).map(buildSessionSnapshot);
}

const sseClients = new Set<express.Response>();

function broadcastStandaloneEvent(event: Record<string, unknown>) {
  const payload = `data: ${JSON.stringify(event)}\n\n`;
  for (const client of sseClients) {
    try {
      client.write(payload);
    } catch {
      sseClients.delete(client);
    }
  }
}


async function fetchHermesGateway(pathname: string, init?: RequestInit, timeoutMs = 1500) {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  const headers: Record<string, string> = {
    "Content-Type": "application/json",
    ...(init?.headers as Record<string, string> | undefined),
  };
  if (HERMES_API_KEY) {
    headers.Authorization = `Bearer ${HERMES_API_KEY}`;
  }
  try {
    return await fetch(`${HERMES_API_URL}${pathname}`, {
      ...init,
      headers,
      signal: controller.signal,
    });
  } finally {
    clearTimeout(timer);
  }
}

async function createServer() {
  const app = express();
  app.use(express.json({ limit: "2mb" }));

  app.get("/api/health", (_req, res) => {
    const hasDeepgramKey = Boolean(process.env.DEEPGRAM_API_KEY);
    res.json({
      ok: true,
      hasDeepgramKey,
      hermesApiUrl: HERMES_API_URL,
      hermesProfile: HERMES_PROFILE,
      ttsProvider: hasDeepgramKey ? "deepgram" : "browser",
    });
  });

  app.get("/api/hermes/gateway", async (_req, res) => {
    try {
      const healthRes = await fetchHermesGateway("/v1/health", { method: "GET" }, 1200);
      if (!healthRes.ok) {
        throw new Error(`Gateway status ${healthRes.status}`);
      }
      let skillsCount = 4;
      let toolsets: string[] = ["duet", "skills", "memory", "cronjob_manage"];
      try {
        const [skillsRes, toolsRes] = await Promise.all([
          fetchHermesGateway("/v1/skills", { method: "GET" }, 1200),
          fetchHermesGateway("/v1/toolsets", { method: "GET" }, 1200),
        ]);
        if (skillsRes.ok) {
          const sj = (await skillsRes.json()) as { data?: unknown[]; skills?: unknown[] };
          const list = sj.skills ?? sj.data;
          if (Array.isArray(list)) skillsCount = list.length;
        }
        if (toolsRes.ok) {
          const tj = (await toolsRes.json()) as { data?: string[]; toolsets?: string[] };
          const tlist = tj.toolsets ?? tj.data;
          if (Array.isArray(tlist) && tlist.length > 0) toolsets = tlist.slice(0, 8);
        }
      } catch {
        // Optional capabilities endpoints unavailable
      }
      res.json({
        connected: true,
        mode: "hermes-gateway",
        gatewayUrl: HERMES_API_URL,
        profile: HERMES_PROFILE,
        mcpBridge: "mcp__duet__*",
        skillsCount,
        toolsets,
      });
    } catch {
      res.json({
        connected: false,
        mode: "standalone-duet",
        gatewayUrl: HERMES_API_URL,
        profile: HERMES_PROFILE,
        mcpBridge: "mcp__duet__*",
        skillsCount: 4,
        toolsets: ["mcp__duet__*", "skills_list", "memory", "cronjob_manage"],
      });
    }
  });

  app.post("/api/hermes/runs/approve", async (req, res) => {
    const { runId, decision = "approve", draft } = req.body ?? {};
    let forwardedToGateway = false;
    if (runId && typeof runId === "string" && HERMES_API_KEY) {
      try {
        const gwRes = await fetchHermesGateway(
          `/v1/runs/${encodeURIComponent(runId)}/approval`,
          {
            method: "POST",
            body: JSON.stringify({ decision, draft }),
          },
          2000
        );
        forwardedToGateway = gwRes.ok;
      } catch {
        forwardedToGateway = false;
      }
    }
    res.json({ ok: true, forwardedToGateway, runId: runId ?? null, decision });
  });

  async function generateHermesReply(params: {
    message: string;
    channelId?: string;
    sessionId?: string;
    persona?: string;
    history?: Array<{ role?: string; text?: string }>;
    pendingApprovals?: Array<{ id?: string; title?: string }>;
    tasks?: Array<{ status?: string }>;
    uiConfig?: Partial<UiCustomizationConfig>;
  }) {
    const {
      message,
      channelId = "general",
      sessionId,
      persona,
      history = [],
      pendingApprovals = [],
      tasks = [],
      uiConfig,
    } = params;

    const currentUi: UiCustomizationConfig = applyUiPatch(
      DEFAULT_UI_CONFIG,
      sanitizeUiPatch(uiConfig ?? {})
    ).next;
    const heuristicPatch = parseNaturalUiIntent(message, currentUi);

    const resolvedSessionId =
      typeof sessionId === "string" && sessionId ? sessionId : `hermes-${channelId || "general"}`;
    const resolvedPersona =
      typeof persona === "string" && persona.trim()
        ? persona.trim()
        : "Isolated Hermes Agent session running independent conversation context.";

    // 100% Hermes: the gateway is the only reply source. No Gemini bridge and
    // no canned offline persona — gateway failures surface as visible errors.
    const gwTimeoutMs = Number(process.env.HERMES_CHAT_TIMEOUT_MS || 300_000);
    let gatewayFailure = "HERMES_API_URL is not configured";
    if (process.env.HERMES_API_URL || process.env.HERMES_API_KEY) {
      try {
        const gwMessages = [
          {
            role: "system",
            content: `You are Hermes Agent inside Hermes Duet (isolated session ${resolvedSessionId}, lane #${channelId || "general"}). Persona: ${resolvedPersona}. Respond in 1-3 crisp spoken sentences.`,
          },
          ...(Array.isArray(history)
            ? history.slice(-6).map((h) => ({
                role: h.role === "user" ? "user" : "assistant",
                content: h.text ?? "",
              }))
            : []),
          { role: "user", content: message },
        ];
        const gwRes = await fetchHermesGateway(
          "/v1/chat/completions",
          {
            method: "POST",
            headers: {
              "X-Hermes-Session-Key": resolvedSessionId,
            },
            body: JSON.stringify({
              model: process.env.API_SERVER_MODEL_NAME || "hermes-agent",
              messages: gwMessages,
              user: resolvedSessionId,
            }),
          },
          gwTimeoutMs
        );
        if (gwRes.ok) {
          const data = (await gwRes.json()) as {
            choices?: Array<{ message?: { content?: string } }>;
          };
          const content = data.choices?.[0]?.message?.content?.trim();
          if (content) {
            return {
              reply: content,
              mood: "happy",
              suggestedCommand: "",
              uiPatch: heuristicPatch ? sanitizeUiPatch(heuristicPatch) : undefined,
              sourceMode: "hermes-gateway",
              sessionId: resolvedSessionId,
            };
          }
          gatewayFailure = "gateway response contained no content";
        } else {
          gatewayFailure = `gateway HTTP ${gwRes.status}`;
        }
      } catch (err) {
        gatewayFailure = err instanceof Error ? err.message : String(err);
      }
    }

    return {
      reply: `Hermes agent unavailable (${gatewayFailure}). hermes-agentis replies 100% via the Hermes gateway at ${HERMES_API_URL} — verify it is running, then retry. No fallback model answered this.`,
      mood: "alert",
      suggestedCommand: "",
      uiPatch: undefined,
      sourceMode: "hermes-gateway-error",
      sessionId: resolvedSessionId,
      error: gatewayFailure,
    };
  }

  app.post("/api/hermes/chat", async (req, res) => {
    const {
      message,
      channelId,
      sessionId,
      persona,
      history = [],
      pendingApprovals = [],
      tasks = [],
      uiConfig,
    } = req.body ?? {};
    if (!message || typeof message !== "string") {
      res.status(400).json({ error: "Message is required." });
      return;
    }

    const result = await generateHermesReply({
      message,
      channelId,
      sessionId,
      persona,
      history,
      pendingApprovals,
      tasks,
      uiConfig,
    });
    res.status(200).json(result);
  });

  // Separate hermes-agent session registry endpoints
  app.get("/api/hermes/sessions", (_req, res) => {
    res.status(200).json({ sessions: buildAllSessionSnapshots() });
  });

  app.post("/api/hermes/sessions", async (req, res) => {
    const { channelId, name, sessionId, topic, persona, skipWelcome, requireFresh } = (req.body ?? {}) as {
      channelId?: string;
      name?: string;
      sessionId?: string;
      topic?: string;
      persona?: string;
      skipWelcome?: boolean;
      requireFresh?: boolean;
    };
    const rawKey = channelId || name;
    if (!rawKey || typeof rawKey !== "string") {
      res.status(400).json({ error: "channelId or name is required." });
      return;
    }

    const baseKey = normalizeChannelKey(rawKey) || "session";
    const resolvedStandaloneKey = requireFresh
      ? (() => {
          const taken = new Set(standaloneChannels.map((c) => c.id));
          if (!taken.has(baseKey)) return baseKey;
          let n = 2;
          while (taken.has(`${baseKey}-${n}`)) n++;
          return `${baseKey}-${n}`;
        })()
      : baseKey;

    // Linked mode first: make this lane real on upstream hermes-duet so it gets
    // a genuine isolated hermes-agent session_id & standing instructions.
    const upstreamResult = await ensureDuetChannelRecord(
      resolvedStandaloneKey,
      { name: name || resolvedStandaloneKey, topic, persona },
      { requireFresh: Boolean(requireFresh) }
    );

    if (upstreamResult?.channel) {
      const uc = upstreamResult.channel;
      const canonicalSlug =
        normalizeChannelKey(uc.slug || uc.id) || resolvedStandaloneKey;
      const channel = ensureStandaloneChannel({
        channelId: canonicalSlug,
        name: uc.name || canonicalSlug,
        sessionId: uc.session_id || sessionId || `hermes-${canonicalSlug}`,
        topic: uc.topic ?? topic,
        persona: uc.instructions ?? persona,
      });
      const snapshot = buildSessionSnapshot(channel);
      res.status(200).json({
        linked: true,
        resolvedChannelId: canonicalSlug,
        upstream_channel_id: uc.id,
        upstream_session_id: uc.session_id ?? snapshot.sessionId,
        session: snapshot,
        channel,
        item: null,
      });
      return;
    }

    const hadChannel = standaloneChannels.some((c) => c.id === resolvedStandaloneKey);
    const channel = ensureStandaloneChannel({
      channelId: resolvedStandaloneKey,
      name: name || resolvedStandaloneKey,
      sessionId,
      topic,
      persona,
    });

    let welcomeItem: StandaloneDuetItem | null = null;
    const hasItems = standaloneItems.some((it) => (it.channel_id || "general") === channel.id);
    if (!hasItems && !skipWelcome) {
      welcomeItem = {
        id: `init-welcome-${channel.id}-${Date.now()}`,
        kind: "assistant",
        created_at: Date.now(),
        channel_id: channel.id,
        text: `Isolated session ${channel.session_id} ready (#${channel.name}). ${channel.topic}`,
      };
      standaloneItems.push(welcomeItem);
    }

    const snapshot = buildSessionSnapshot(channel);
    if (!hadChannel) {
      broadcastStandaloneEvent({ type: "channel", channel });
    }
    if (welcomeItem) {
      broadcastStandaloneEvent({ type: "item", item: welcomeItem });
    }
    broadcastStandaloneEvent({ type: "session", session: snapshot });
    res.status(200).json({
      linked: false,
      resolvedChannelId: channel.id,
      session: snapshot,
      channel,
      item: welcomeItem,
      upstream_channel_id: null,
    });
  });

  app.post("/api/hermes/sessions/:channelId/reset", (req, res) => {
    const channelId = req.params.channelId;
    const ch = ensureStandaloneChannel({ channelId });
    for (let i = standaloneItems.length - 1; i >= 0; i--) {
      if ((standaloneItems[i].channel_id || "general") === ch.id) {
        standaloneItems.splice(i, 1);
      }
    }
    standaloneSessionStatus.set(ch.id, "idle");
    const resetItem: StandaloneDuetItem = {
      id: `reset-${ch.id}-${Date.now()}`,
      kind: "assistant",
      created_at: Date.now(),
      channel_id: ch.id,
      text: `Fresh context opened for ${ch.session_id} (#${ch.name}). Prior messages in this shape were cleared.`,
    };
    standaloneItems.push(resetItem);
    const snapshot = buildSessionSnapshot(ch);
    broadcastStandaloneEvent({
      type: "session_reset",
      channelId: ch.id,
      session: snapshot,
      item: resetItem,
    });
    res.status(200).json({ ok: true, session: snapshot, item: resetItem });
  });

  app.delete("/api/hermes/sessions/:channelId", (req, res) => {
    const channelId = req.params.channelId;
    if (channelId === "general") {
      res.status(400).json({ error: "Cannot delete primary general session." });
      return;
    }
    const idx = standaloneChannels.findIndex((c) => c.id === channelId);
    if (idx >= 0) {
      standaloneChannels.splice(idx, 1);
    }
    for (let i = standaloneItems.length - 1; i >= 0; i--) {
      if ((standaloneItems[i].channel_id || "general") === channelId) {
        standaloneItems.splice(i, 1);
      }
    }
    standaloneSessionStatus.delete(channelId);
    broadcastStandaloneEvent({ type: "session_deleted", channelId });
    res.status(200).json({ ok: true, channelId });
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

      // 1. Primary TTS: Deepgram Aura-2 REST API (WAV linear16 for Web Audio AnalyserNode lip-sync)
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

      // 2. Clean graceful fallback so browser Web Speech & real-time viseme envelope animate with zero HTTP 500 errors
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

  // ----- Duet adapter routes: real state, runs, approvals, live events -----
  // Falls back cleanly to in-memory standalone Duet state when external Duet is absent.
  app.get("/api/duet/status", async (_req, res) => {
    try {
      const r = await duetFetch("/api/state");
      res.json({ ok: r.ok, paired: r.ok, mode: r.ok ? "duet-linked" : "standalone-duet", baseUrl: DUET_BASE_URL, device: DUET_DEVICE_NAME });
    } catch {
      res.json({ ok: true, paired: false, mode: "standalone-duet", baseUrl: DUET_BASE_URL, device: DUET_DEVICE_NAME });
    }
  });

  app.get("/api/duet/state", async (_req, res) => {
    try {
      const r = await duetFetch("/api/state");
      if (r.ok) {
        res.status(200).json(await r.json());
        return;
      }
    } catch {
      // External Duet daemon unreachable; serve in-memory standalone snapshot
    }
    res.status(200).json({
      items: standaloneItems,
      pending: standalonePending,
      tasks: standaloneTasks,
      channels: standaloneChannels,
      sessions: buildAllSessionSnapshots(),
    });
  });

  // Per-channel item history & seen passthrough (mirrors hermes-duet openChannel())
  app.get("/api/duet/channels/:channelId/items", async (req, res) => {
    const rawChannelId = req.params.channelId;
    const before = typeof req.query.before === "string" ? req.query.before : "";
    try {
      const resolvedId = (await ensureDuetChannel(rawChannelId)) ?? rawChannelId;
      const qs = before ? `?before=${encodeURIComponent(before)}` : "";
      const r = await duetFetch(`/api/channels/${encodeURIComponent(resolvedId)}/items${qs}`, {
        signal: AbortSignal.timeout(4500),
      });
      if (r.ok) {
        res.status(200).json(await r.json());
        return;
      }
    } catch {
      // Fall through to standalone channel items
    }
    const slug = normalizeChannelKey(rawChannelId) || "general";
    const chItems = standaloneItems
      .filter((it) => (it.channel_id || "general") === slug)
      .sort((a, b) => a.created_at - b.created_at);
    res.status(200).json({ items: chItems, has_more: false });
  });

  app.post("/api/duet/channels/:channelId/seen", async (req, res) => {
    const rawChannelId = req.params.channelId;
    try {
      const resolvedId = (await ensureDuetChannel(rawChannelId)) ?? rawChannelId;
      const r = await duetFetch(`/api/channels/${encodeURIComponent(resolvedId)}/seen`, {
        method: "POST",
        signal: AbortSignal.timeout(2500),
      });
      if (r.ok) {
        res.status(200).json(await r.json());
        return;
      }
    } catch {
      // Non-fatal in standalone mode
    }
    res.status(200).json({ ok: true });
  });

  app.post("/api/duet/messages", async (req, res) => {
    const {
      text,
      channel_id = "general",
      name,
      topic,
      session_id,
      persona,
      ui_config,
    } = (req.body ?? {}) as {
      text?: string;
      channel_id?: string;
      name?: string;
      topic?: string;
      session_id?: string;
      persona?: string;
      ui_config?: Partial<UiCustomizationConfig>;
    };
    if (!text || typeof text !== "string") {
      res.status(400).json({ error: "text is required" });
      return;
    }

    try {
      // Resolve the local slug ("ops") to the real Duet channel id, creating the
      // lane upstream on first use so Home & Canvas lanes run as real agent sessions.
      // The session's topic and persona ride along as standing channel instructions.
      const resolvedMeta =
        name || topic || persona ? { name, topic, persona } : undefined;
      const upstreamChannelId =
        (await ensureDuetChannel(channel_id, resolvedMeta)) ?? channel_id;
      const sendBody = JSON.stringify({ text, channel_id: upstreamChannelId });
      let r = await duetFetch("/api/messages", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        signal: AbortSignal.timeout(10_000),
        body: sendBody,
      });
      if (r.status === 404 && upstreamChannelId === channel_id) {
        // Pre-resolution failed (Duet busy or cache cold): force one resolution
        // pass and retry before falling back to the standalone run.
        duetChannelCacheAt = 0;
        const forced = await ensureDuetChannel(channel_id, resolvedMeta);
        if (forced && forced !== channel_id) {
          r = await duetFetch("/api/messages", {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            signal: AbortSignal.timeout(10_000),
            body: JSON.stringify({ text, channel_id: forced }),
          });
        }
      }
      if (r.ok) {
        res.status(200).json(await r.json());
        return;
      }
    } catch {
      // Fall through to standalone in-memory Duet run
    }

    const channel = ensureStandaloneChannel({
      channelId: channel_id,
      name,
      sessionId: session_id,
      topic,
      persona,
    });
    channel.last_at = Date.now();

    const userItem: StandaloneDuetItem = {
      id: `user-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`,
      kind: "user",
      created_at: Date.now(),
      channel_id: channel.id,
      text,
    };
    standaloneItems.push(userItem);
    standaloneSessionStatus.set(channel.id, "thinking");
    broadcastStandaloneEvent({ type: "item", item: userItem });
    broadcastStandaloneEvent({ type: "session", session: buildSessionSnapshot(channel) });
    res.status(200).json({ item: userItem, session: buildSessionSnapshot(channel) });

    // Generate Hermes reply asynchronously in this shape's isolated session
    void (async () => {
      try {
        const history = standaloneItems
          .filter(
            (it) =>
              (it.kind === "user" || it.kind === "assistant") &&
              (it.channel_id || "general") === channel.id
          )
          .slice(-6)
          .map((it) => ({
            role: it.kind === "user" ? "user" : "hermes",
            text: it.text ?? "",
          }));
        const pendingApprovals = standalonePending
          .filter((p) => p.card && p.card.status === "pending" && p.card.type !== "brief")
          .map((p) => ({ id: p.id, title: p.card?.title }));

        const replyData = await generateHermesReply({
          message: text,
          channelId: channel.id,
          sessionId: channel.session_id,
          persona: channel.persona ?? undefined,
          history,
          pendingApprovals,
          tasks: standaloneTasks.filter((t) => (t.channel_id || "general") === channel.id),
          uiConfig: ui_config,
        });

        const assistantItem: StandaloneDuetItem = {
          id: `hermes-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`,
          kind: "assistant",
          created_at: Date.now(),
          channel_id: channel.id,
          streaming: false,
          ui_patch: replyData.uiPatch,
          text: replyData.suggestedCommand
            ? `${replyData.reply}\n[Suggested: ${replyData.suggestedCommand}]`
            : replyData.reply,
        };
        standaloneItems.push(assistantItem);
        standaloneSessionStatus.set(channel.id, "idle");
        if (replyData.uiPatch) {
          broadcastStandaloneEvent({
            type: "ui_patch",
            channelId: channel.id,
            patch: replyData.uiPatch,
          });
        }
        broadcastStandaloneEvent({ type: "item", item: assistantItem });
        broadcastStandaloneEvent({ type: "session", session: buildSessionSnapshot(channel) });
      } catch {
        standaloneSessionStatus.set(channel.id, "idle");
        broadcastStandaloneEvent({ type: "session", session: buildSessionSnapshot(channel) });
      }
    })();
  });

  app.post("/api/duet/cards/:cardId/act", async (req, res) => {
    const { action, value } = (req.body ?? {}) as { action?: string; value?: string };
    try {
      const r = await duetFetch(`/api/cards/${encodeURIComponent(req.params.cardId)}/act`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ action, value }),
      });
      if (r.ok) {
        res.status(200).json(await r.json());
        return;
      }
    } catch {
      // Fall through to standalone in-memory card update
    }

    const item = standalonePending.find((p) => p.id === req.params.cardId);
    if (!item || !item.card) {
      res.status(404).json({ error: "card not found" });
      return;
    }
    item.card.status = action === "approve" ? "approved" : "declined";
    if (typeof value === "string") {
      item.card.draft = value;
    }
    item.updated_at = Date.now();
    broadcastStandaloneEvent({ type: "item", item });
    res.status(200).json({ ok: true, item });
  });

  // SSE passthrough: live items/tasks/channels from the Duet server (or standalone SSE hub).
  app.get("/api/duet/events", async (req, res) => {
    try {
      // Long-lived stream: the default 2.5s duetFetch timeout must NOT apply.
      // We own the lifecycle and abort upstream only when this client leaves.
      const upstreamController = new AbortController();
      const upstream = await duetFetch("/api/events", {
        headers: { accept: "text/event-stream" },
        signal: upstreamController.signal,
      });
      if (upstream.ok && upstream.body) {
        res.writeHead(200, {
          "Content-Type": "text/event-stream",
          "Cache-Control": "no-cache, no-transform",
          Connection: "keep-alive",
          "X-Accel-Buffering": "no",
        });
        const nodeStream = Readable.fromWeb(upstream.body as Parameters<typeof Readable.fromWeb>[0]);
        // A stream error (upstream abort/restart) must end the response, never crash the server.
        nodeStream.on("error", () => {
          try {
            res.end();
          } catch {
            // response already closed
          }
        });
        nodeStream.pipe(res);
        const teardown = () => {
          upstreamController.abort();
          nodeStream.destroy();
        };
        req.on("close", teardown);
        res.on("close", teardown);
        return;
      }
    } catch {
      // Fall through to standalone SSE stream
    }

    res.writeHead(200, {
      "Content-Type": "text/event-stream",
      "Cache-Control": "no-cache, no-transform",
      Connection: "keep-alive",
      "X-Accel-Buffering": "no",
    });
    res.write(": standalone-duet-connected\n\n");
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

  if (process.env.NODE_ENV !== "production") {
    const { createServer: createViteServer } = await import("vite");
    const vite = await createViteServer({
      server: { middlewareMode: true },
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

  app.listen(PORT, "0.0.0.0", () => {
    console.log(`Hermes Duet server running on http://0.0.0.0:${PORT}`);
  });
}

createServer().catch((err) => {
  console.error("Server failed to start:", err);
  process.exit(1);
});
