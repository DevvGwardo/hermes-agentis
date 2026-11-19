#!/usr/bin/env node
/**
 * gen-vectors.mjs — Cross-language golden JSON vector generator (RUNE Desktop spec §9, ticket P0-04).
 *
 * Imports the PURE web reference utilities (src/utils/*.ts, read-only) and exports
 * deterministic golden vectors to voxcell-desktop/tests/vectors/<module>/<function>.json
 * so the Rust ports in voxcell-core / voxcell-hermes can be locked against TypeScript behavior.
 *
 * Run from the REPO ROOT (tsx lives in the web node_modules):
 *   npx tsx voxcell-desktop/tools/gen-vectors.mjs
 *
 * Determinism contract (also recorded in tests/vectors/manifest.json):
 *   - process.env.TZ pinned to 'UTC' before any module import (formatStartClock uses local getters).
 *   - Date.now() pinned to FIXED_NOW_MS.
 *   - Math.random replaced with a seeded mulberry32 PRNG.
 *   - crypto.randomUUID replaced with a seeded counter-based UUIDv4-shaped generator
 *     (only createIdempotencyKey consumes it; the UUID suffix is shim-determined, the
 *     prefix/sanitization/length logic under test is real).
 *   - All inputs are fixed literals below; no network, no filesystem reads, no clock.
 *   - Output JSON is canonicalized: object keys sorted recursively, array order preserved,
 *     -0 normalized to 0, 2-space indent, trailing newline.
 *
 * Vector file schema:
 *   { "module", "function", "description", "cases": [ { "name", "input", "expected" } ] }
 * `input` is a named-argument object (JSON-native; Sets are represented as arrays and
 * converted by the runner). `expected` is the real output of the web implementation.
 *
 * If a module cannot be imported headlessly (browser-only dependency at import time),
 * it is skipped and the reason is recorded in manifest.json instead of failing the run.
 */

import { mkdirSync, rmSync, writeFileSync, readdirSync, statSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

// ---------------------------------------------------------------------------
// Determinism harness — MUST be installed before the web modules are imported.
// ---------------------------------------------------------------------------
process.env.TZ = 'UTC';

const FIXED_NOW_MS = 1795089600000; // 2026-11-19T12:00:00.000Z
const PRNG_SEED = 0x5eed1234;

function mulberry32(seed) {
  let s = seed | 0;
  return () => {
    s = (s + 0x6d2b79f5) | 0;
    let t = Math.imul(s ^ (s >>> 15), 1 | s);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}
const rng = mulberry32(PRNG_SEED);
Date.now = () => FIXED_NOW_MS;
Math.random = rng;

const realCrypto = globalThis.crypto;
let uuidCounter = 0;
Object.defineProperty(globalThis, 'crypto', {
  configurable: true,
  value: {
    randomUUID: () => {
      uuidCounter += 1;
      const hex = uuidCounter.toString(16).padStart(8, '0');
      return `${hex}-0000-4000-8000-000000000000`;
    },
    getRandomValues: realCrypto?.getRandomValues?.bind(realCrypto),
  },
});

// ---------------------------------------------------------------------------
// Paths & module loading (skip-with-reason for browser-only modules)
// ---------------------------------------------------------------------------
const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(SCRIPT_DIR, '..', '..');
const VECTORS_DIR = join(REPO_ROOT, 'voxcell-desktop', 'tests', 'vectors');

const MODULES = [
  'hermesProtocol',
  'profileRegistry',
  'settingsSnapshot',
  'responsiveLayout',
  'taskInsights',
  'sidebarCharacterState',
  'threadTrace',
];

const loaded = {};
const skipped = [];
for (const name of MODULES) {
  try {
    loaded[name] = await import(pathToFileURL(join(REPO_ROOT, 'src', 'utils', `${name}.ts`)).href);
  } catch (err) {
    skipped.push({
      module: name,
      source: `src/utils/${name}.ts`,
      status: 'skipped',
      reason: `Import failed under headless Node (browser-only dependency?): ${err && err.message ? err.message : String(err)}`,
    });
    console.warn(`[skip] ${name}: ${err && err.message ? err.message : err}`);
  }
}

// ---------------------------------------------------------------------------
// Canonical JSON + vector collection
// ---------------------------------------------------------------------------
function canonical(value) {
  if (Array.isArray(value)) return value.map(canonical);
  if (value && typeof value === 'object') {
    const out = {};
    for (const key of Object.keys(value).sort()) {
      const v = value[key];
      if (v === undefined) continue;
      out[key] = canonical(v);
    }
    return out;
  }
  if (typeof value === 'number') {
    if (Object.is(value, -0)) return 0;
    if (!Number.isFinite(value)) return null;
    return value;
  }
  return value;
}

const vectors = []; // { module, fn, description, cases }
function vec(module, fn, description, cases) {
  // Multiple vec() calls for the same function (e.g. stream-parser scenario
  // groups) merge into ONE file so no cases are silently overwritten.
  const existing = vectors.find((v) => v.module === module && v.fn === fn);
  if (existing) {
    existing.cases.push(...cases);
    if (!existing.description.includes(description)) {
      existing.description += ' ' + description;
    }
    return;
  }
  vectors.push({ module, fn, description, cases });
}
function caseOf(name, input, expected) {
  return { name, input: canonical(input), expected: canonical(expected) };
}
// Human-safe labels for raw-string case tables ('' and null need explicit names).
const rawLabel = (raw) => (raw === '' ? '(empty)' : raw === null ? '(null)' : String(raw));

// ---------------------------------------------------------------------------
// Shared fixtures (fixed literals only)
// ---------------------------------------------------------------------------
const T0 = FIXED_NOW_MS;
const MIN = 60_000;

const taskRunningSteps = {
  id: 'task-build', channelId: 'general', sessionId: 'hermes-general', profile: 'default',
  kind: 'Background', background: true, title: 'Refactor the halftone emitter',
  status: 'running', rawStatus: 'running', progress: 40, progressLabel: '1/3',
  lastStep: 'terminal: cargo check',
  steps: [
    { text: 'Read raster code', done: true },
    { text: 'terminal: cargo check', active: true },
    { text: 'Write GPU emitter' },
  ],
  createdAtMs: T0 - 125_000, updatedAtMs: T0 - 5_000, updatedAt: '11:57',
};
const taskRunningRatio = {
  id: 'task-research', channelId: 'general', title: 'Survey RUNE APIs',
  status: 'running', rawStatus: 'running', progress: 33, progressLabel: '2/6',
  lastStep: 'Running web_search', createdAtMs: T0 - 90_000, updatedAtMs: T0 - 20_000, updatedAt: '11:58',
};
const taskRunningZero = {
  id: 'task-fresh', channelId: 'design', title: 'Draft the phone widget',
  status: 'running', rawStatus: 'running', progress: 0,
  lastStep: 'Working in hermes-design', createdAtMs: T0 - 2_000, updatedAtMs: T0 - 1_000, updatedAt: '11:59',
};
const taskQueued = {
  id: 'task-queue', channelId: 'general', title: 'Nightly backup',
  status: 'running', rawStatus: 'queued', progress: 0,
  lastStep: 'Queued', createdAtMs: T0 - 30_000, updatedAtMs: T0 - 30_000, updatedAt: '11:59',
};
const taskPausedApproval = {
  id: 'task-approval', channelId: 'general', title: 'Deploy staging build',
  status: 'paused', rawStatus: 'waiting_for_approval', progress: 60,
  lastStep: 'Waiting for approval', createdAtMs: T0 - 4 * MIN, updatedAtMs: T0 - 2 * MIN, updatedAt: '11:58',
};
const taskCompletedUnconfirmed = {
  id: 'task-done', channelId: 'general', title: 'Summarize the changelog',
  status: 'completed', rawStatus: 'done', progress: 100,
  lastStep: 'Completed response',
  steps: [{ text: 'Read CHANGELOG.md', done: true }, { text: 'Completed response', done: true }],
  result: 'Changelog summarized: 12 entries across 3 releases.',
  agentDoneAtMs: T0 - 8 * MIN, createdAtMs: T0 - 10 * MIN, updatedAtMs: T0 - 8 * MIN, updatedAt: '11:52',
};
const taskCompletedConfirmed = {
  ...taskCompletedUnconfirmed,
  id: 'task-confirmed', title: 'Clean the dist folder',
  confirmedAtMs: T0 - 7 * MIN, updatedAtMs: T0 - 7 * MIN,
};
const taskFailed = {
  id: 'task-failed', channelId: 'ops', title: 'Run integration suite',
  status: 'completed', rawStatus: 'failed', errorCode: 'tool_error', progress: 80,
  lastStep: 'terminal: cargo test', result: 'error: test failed',
  createdAtMs: T0 - 12 * MIN, updatedAtMs: T0 - 9 * MIN, updatedAt: '11:51',
};
const taskCancelled = {
  id: 'task-cancelled', channelId: 'ops', title: 'Long crawl',
  status: 'completed', rawStatus: 'cancelled', progress: 10,
  lastStep: 'Running web_extract', createdAtMs: T0 - 15 * MIN, updatedAtMs: T0 - 14 * MIN, updatedAt: '11:46',
};
const taskBudget = {
  id: 'task-budget', channelId: 'general', title: 'Deep research loop',
  status: 'completed', rawStatus: 'iteration_budget_exhausted', progress: 100,
  lastStep: 'Iteration budget reached (partial answer)',
  createdAtMs: T0 - 20 * MIN, updatedAtMs: T0 - 18 * MIN, updatedAt: '11:42',
};
const taskVague = {
  id: 'task-vague', channelId: 'general', title: 'go',
  status: 'running', rawStatus: 'running', progress: 50,
  lastStep: 'Running terminal',
  steps: [{ text: 'Running terminal' }, { text: 'terminal: gh pr checks' }],
  createdAtMs: T0 - MIN, updatedAtMs: T0 - 30_000, updatedAt: '11:59',
};

const entry = (id, role, text, extra = {}) => ({
  id, channelId: 'general', role, source: 'ui', text,
  timestamp: '11:57:0' + (id.charCodeAt(id.length - 1) % 10), createdAtMs: T0 - 60_000, ...extra,
});

const userEntry = entry('u1', 'user', 'Refactor the halftone emitter please', { createdAtMs: T0 - 120_000 });
const reasoningEntry = entry('a1', 'activity', 'Reasoning: weighing GPU instancing options', {
  activityKind: 'reasoning', activityStatus: 'done', reasoningDetail: 'weighing GPU instancing options',
});
const commentaryEntry = entry('a2', 'activity', 'Commentary: checking the vendor patches first', {
  activityKind: 'commentary', activityStatus: 'done',
});
const toolRunningEntry = entry('a3', 'activity', 'Running terminal', {
  activityKind: 'tool', activityStatus: 'running',
});
const toolDetailEntry = entry('a4', 'activity', 'terminal: cargo check', {
  activityKind: 'tool', activityStatus: 'running',
});
const toolDoneEntry = entry('a5', 'activity', 'terminal completed', {
  activityKind: 'tool', activityStatus: 'done',
});
const subagentEntry = entry('a6', 'activity', 'Spawned subagent for RUNE research', {
  activityKind: 'subagent', activityStatus: 'done',
});
const linkEntry = entry('a7', 'activity', 'Opened gateway docs — https://example.dev/rune', {
  activityKind: 'status', activityStatus: 'done',
});
const diagnosticEntry = entry('a8', 'activity', 'Turn failed', {
  activityKind: 'status', activityStatus: 'error', terminalOutcome: 'failed', errorCode: 'tool_error',
});
const replyEntry = entry('r1', 'hermes', 'Here is the refactor plan.', { createdAtMs: T0 - 30_000 });
const lateToolEntry = entry('a9', 'activity', 'terminal: cargo test', {
  activityKind: 'tool', activityStatus: 'done', createdAtMs: T0 - 10_000,
});

const gatewayStatusConnected = {
  connected: true, mode: 'hermes-gateway', gatewayUrl: 'http://127.0.0.1:8642',
  profile: 'default', activeModel: 'hermes-4', models: ['hermes-4'],
  sessionHeader: 'X-Hermes-Session-Id', skillsCount: 12, healthStatus: 'ok',
};
const gatewayStatusOffline = { ...gatewayStatusConnected, connected: false, activeModel: '', healthStatus: 'error', error: 'ECONNREFUSED' };

const bands12 = [0.62, 0.55, 0.48, 0.4, 0.33, 0.27, 0.2, 0.16, 0.12, 0.09, 0.06, 0.04];

const sseFrame = (event, data) =>
  (event ? `event: ${event}\n` : '') +
  `data: ${typeof data === 'string' ? data : JSON.stringify(data)}\n\n`;
const chunked = (wire, size) => {
  const out = [];
  for (let i = 0; i < wire.length; i += size) out.push(wire.slice(i, i + size));
  return out;
};

// ---------------------------------------------------------------------------
// hermesProtocol
// ---------------------------------------------------------------------------
if (loaded.hermesProtocol) {
  const m = loaded.hermesProtocol;
  const mod = 'hermesProtocol';

  vec(mod, 'resolveProfileGatewayConfig', 'Gateway base URL / profile routing / key precedence from env.', [
    caseOf('empty-env-defaults', { env: {} }, m.resolveProfileGatewayConfig({})),
    caseOf('trailing-slash-url', { env: { HERMES_API_URL: 'http://127.0.0.1:8642///' } },
      m.resolveProfileGatewayConfig({ HERMES_API_URL: 'http://127.0.0.1:8642///' })),
    caseOf('named-profile-shared-key', { env: { HERMES_PROFILE: 'coder', HERMES_API_KEY: 'default-key', HERMES_PROFILE_API_KEY: 'shared-key' } },
      m.resolveProfileGatewayConfig({ HERMES_PROFILE: 'coder', HERMES_API_KEY: 'default-key', HERMES_PROFILE_API_KEY: 'shared-key' })),
    caseOf('named-profile-specific-key-wins', { env: { HERMES_PROFILE: 'researcher', HERMES_API_KEY: 'default-key', HERMES_PROFILE_API_KEY: 'shared-key', HERMES_API_KEY_RESEARCHER: 'scoped-key' } },
      m.resolveProfileGatewayConfig({ HERMES_PROFILE: 'researcher', HERMES_API_KEY: 'default-key', HERMES_PROFILE_API_KEY: 'shared-key', HERMES_API_KEY_RESEARCHER: 'scoped-key' })),
    caseOf('default-profile-ignores-shared-key', { env: { HERMES_PROFILE: 'default', HERMES_API_KEY: 'default-key', HERMES_PROFILE_API_KEY: 'shared-key' } },
      m.resolveProfileGatewayConfig({ HERMES_PROFILE: 'default', HERMES_API_KEY: 'default-key', HERMES_PROFILE_API_KEY: 'shared-key' })),
    caseOf('profile-slug-normalization', { env: { HERMES_PROFILE: 'Side Bot!', HERMES_API_KEY_SIDE_BOT_: 'scoped' } },
      m.resolveProfileGatewayConfig({ HERMES_PROFILE: 'Side Bot!', HERMES_API_KEY_SIDE_BOT_: 'scoped' })),
  ]);

  vec(mod, 'buildGatewayUrl', 'URL joining with /p/<profile> prefix and unprefixed escape hatch.', [
    caseOf('prefixed-path', { config: { baseUrl: 'http://127.0.0.1:8642', routePrefix: '/p/coder' }, pathname: '/v1/runs' },
      m.buildGatewayUrl({ baseUrl: 'http://127.0.0.1:8642', routePrefix: '/p/coder' }, '/v1/runs')),
    caseOf('unprefixed-health', { config: { baseUrl: 'http://127.0.0.1:8642', routePrefix: '/p/coder' }, pathname: 'health', options: { unprefixed: true } },
      m.buildGatewayUrl({ baseUrl: 'http://127.0.0.1:8642', routePrefix: '/p/coder' }, 'health', { unprefixed: true })),
    caseOf('missing-leading-slash', { config: { baseUrl: 'http://127.0.0.1:8642', routePrefix: '' }, pathname: 'v1/capabilities' },
      m.buildGatewayUrl({ baseUrl: 'http://127.0.0.1:8642', routePrefix: '' }, 'v1/capabilities')),
  ]);

  vec(mod, 'sanitizeSessionKeyHeader', 'X-Hermes-Session-Key sanitization: CRLF/NUL strip, trim, 256 cap.', [
    caseOf('plain', { raw: 'voxcell-main' }, m.sanitizeSessionKeyHeader('voxcell-main')),
    caseOf('control-chars', { raw: 'vox\r\ncell\x00-key  ' }, m.sanitizeSessionKeyHeader('vox\r\ncell\x00-key  ')),
    caseOf('empty-after-strip', { raw: '\r\n\x00' }, m.sanitizeSessionKeyHeader('\r\n\x00')),
    caseOf('null', { raw: null }, m.sanitizeSessionKeyHeader(null)),
    caseOf('over-256', { raw: 'k'.repeat(300) }, m.sanitizeSessionKeyHeader('k'.repeat(300))),
  ]);

  vec(mod, 'createIdempotencyKey', 'Idempotency-Key prefix sanitization + length cap. UUID suffix is shim-seeded (see manifest determinism notes).', [
    caseOf('default-prefix', { prefix: undefined }, m.createIdempotencyKey()),
    caseOf('custom-prefix', { prefix: 'turn-42' }, m.createIdempotencyKey('turn-42')),
    caseOf('invisible-chars-sanitized', { prefix: 'bad\x01prefix\x7f' }, m.createIdempotencyKey('bad\x01prefix\x7f')),
    caseOf('long-prefix-capped', { prefix: 'p'.repeat(60) }, m.createIdempotencyKey('p'.repeat(60))),
  ]);

  const capsFull = {
    object: 'hermes.api_server.capabilities', platform: 'hermes-agent', model: 'hermes-4',
    session_key_header: 'X-Hermes-Session-Key',
    features: { run_submission: true, run_events_sse: true, run_approval: true, run_stop: true, session_chat_stream: true, chat_completions: true, reasoning_streaming: true },
    endpoints: { runs: '/v1/runs', health: '/health' },
  };
  vec(mod, 'parseCapabilitiesPayload', '/v1/capabilities normalization with defaults and feature coercion.', [
    caseOf('full', { raw: capsFull }, m.parseCapabilitiesPayload(capsFull)),
    caseOf('minimal', { raw: { features: { run_submission: 1, run_events_sse: 0 } } }, m.parseCapabilitiesPayload({ features: { run_submission: 1, run_events_sse: 0 } })),
    caseOf('non-string-endpoints-dropped', { raw: { endpoints: { runs: '/v1/runs', bad: 5 } } }, m.parseCapabilitiesPayload({ endpoints: { runs: '/v1/runs', bad: 5 } })),
    caseOf('null', { raw: null }, m.parseCapabilitiesPayload(null)),
    caseOf('scalar', { raw: 42 }, m.parseCapabilitiesPayload(42)),
  ]);

  const capsRunsOnly = { features: { run_submission: true, run_events_sse: true, chat_completions: false } };
  const capsChatReasoning = { features: { reasoning_streaming: true, chat_completions: true } };
  const capsSessionOnly = { features: { session_chat_stream: true, chat_completions: false } };
  const capsRunsReasoningNoControls = { features: { run_submission: true, run_events_sse: true, reasoning_streaming: true } };
  const transportCases = [];
  for (const preferred of ['auto', 'runs', 'session_stream', 'chat_completions']) {
    for (const [label, caps] of [['null', null], ['full', capsFull], ['runs-only', capsRunsOnly], ['chat-reasoning', capsChatReasoning], ['session-only', capsSessionOnly], ['runs-reasoning-no-controls', capsRunsReasoningNoControls]]) {
      transportCases.push(caseOf(`${preferred}+${label}`, { preferred, capabilities: caps }, m.chooseGatewayTransport(preferred, caps)));
    }
  }
  vec(mod, 'chooseGatewayTransport', 'Transport selection matrix: preferred mode x advertised capabilities.', transportCases);

  vec(mod, 'extractModelOptions', 'Model/provider catalog extraction from heterogeneous gateway payloads.', [
    caseOf('models-and-providers', {
      raw: {
        models: ['hermes-4', ' hermes-4 ', { id: 'hermes-4-mini' }, { model: 'hermes-3' }, { name: 'legacy' }],
        current_model: 'hermes-4', current_provider: 'nous',
        providers: ['openai', { id: 'nous', label: 'Nous Research', authenticated: true, models: ['hermes-4'] }, { slug: 'anthropic', configured: false }],
      },
    }, m.extractModelOptions({
      models: ['hermes-4', ' hermes-4 ', { id: 'hermes-4-mini' }, { model: 'hermes-3' }, { name: 'legacy' }],
      current_model: 'hermes-4', current_provider: 'nous',
      providers: ['openai', { id: 'nous', label: 'Nous Research', authenticated: true, models: ['hermes-4'] }, { slug: 'anthropic', configured: false }],
    })),
    caseOf('data-and-selected-fallback', { raw: { data: [{ id: 'm1' }, 'm2'], selected: { model: 'm9', provider: 'p9' }, curated_models: ['m3'] } },
      m.extractModelOptions({ data: [{ id: 'm1' }, 'm2'], selected: { model: 'm9', provider: 'p9' }, curated_models: ['m3'] })),
    caseOf('empty', { raw: null }, m.extractModelOptions(null)),
  ]);

  vec(mod, 'mapGatewaySkills', 'Skill list normalization (array / {skills} / {data} envelopes).', [
    caseOf('mixed-envelope', {
      raw: { skills: [
        { name: ' pdf ', description: 'Read and merge PDFs', category: 'docs', enabled: false },
        { name: 'maps', summary: 'Geocode and route', path: '/skills/maps' },
        { name: 'bare' },
        { description: 'nameless skipped' },
        'not-an-object',
      ] },
    }, m.mapGatewaySkills({ skills: [
      { name: ' pdf ', description: 'Read and merge PDFs', category: 'docs', enabled: false },
      { name: 'maps', summary: 'Geocode and route', path: '/skills/maps' },
      { name: 'bare' },
      { description: 'nameless skipped' },
      'not-an-object',
    ] })),
    caseOf('data-array', { raw: { data: [{ name: 'x', source: 'custom' }] } }, m.mapGatewaySkills({ data: [{ name: 'x', source: 'custom' }] })),
    caseOf('garbage', { raw: 'nope' }, m.mapGatewaySkills('nope')),
  ]);

  vec(mod, 'mapGatewayToolsets', 'Toolset normalization incl. bare-string toolsets.', [
    caseOf('mixed', { raw: { toolsets: ['core', { name: 'web', label: 'Web tools', description: 'Search & extract', tools: ['web_search', 'web_extract', 7], enabled: false, configured: false }, { name: '  ' }] } },
      m.mapGatewayToolsets({ toolsets: ['core', { name: 'web', label: 'Web tools', description: 'Search & extract', tools: ['web_search', 'web_extract', 7], enabled: false, configured: false }, { name: '  ' }] })),
    caseOf('empty', { raw: [] }, m.mapGatewayToolsets([])),
  ]);

  vec(mod, 'mapGatewayJobs', 'Cron job normalization: id/schedule/channel fallbacks, paused derivation, agent vs no-agent.', [
    caseOf('mixed-jobs', { raw: { jobs: [
      { id: 'job-1', name: 'Morning brief', cron: '0 8 * * *', channel_id: 'general', prompt: 'Summarize overnight news', last_run_at: '2026-11-19T08:00:00Z', next_run_at: '2026-11-20T08:00:00Z', last_status: 'ok' },
      { job_id: 'job-2', schedule_display: 'every 5m', delivery_target: 'ops', command: 'backup.sh', status: 'paused' },
      { name: 'job-3', enabled: false },
      { prompt: 'no id skipped' },
    ] } }, m.mapGatewayJobs({ jobs: [
      { id: 'job-1', name: 'Morning brief', cron: '0 8 * * *', channel_id: 'general', prompt: 'Summarize overnight news', last_run_at: '2026-11-19T08:00:00Z', next_run_at: '2026-11-20T08:00:00Z', last_status: 'ok' },
      { job_id: 'job-2', schedule_display: 'every 5m', delivery_target: 'ops', command: 'backup.sh', status: 'paused' },
      { name: 'job-3', enabled: false },
      { prompt: 'no id skipped' },
    ] })),
  ]);

  vec(mod, 'isOpaqueSessionIdentifier', 'Opaque gateway session id detection (local/run/cron/uuid/raw-hex).', [
    ...[
      'local-abc123def', '#hermes-run-0123456789ab', 'cron-morning-brief',
      '123e4567-e89b-42d3-a456-426614174000', 'a'.repeat(24), 'hermes-general',
      'local-short', '', null, '#hermes-local-abcdef',
    ].map((raw) => caseOf(rawLabel(raw), { raw }, m.isOpaqueSessionIdentifier(raw))),
  ]);

  vec(mod, 'classifyGatewaySessionOrigin', 'Workspace vs external session provenance.', [
    caseOf('voxcell-key', { params: { sessionKey: 'voxcell-main', source: 'cli' } }, m.classifyGatewaySessionOrigin({ sessionKey: 'voxcell-main', source: 'cli' })),
    caseOf('web-source', { params: { source: 'web' } }, m.classifyGatewaySessionOrigin({ source: 'web' })),
    caseOf('cli-source', { params: { source: 'cli', sessionId: 'hermes-general' } }, m.classifyGatewaySessionOrigin({ source: 'cli', sessionId: 'hermes-general' })),
    caseOf('hermes-slug-workspace', { params: { sessionId: 'hermes-general', channelId: 'general' } }, m.classifyGatewaySessionOrigin({ sessionId: 'hermes-general', channelId: 'general' })),
    caseOf('opaque-id-external', { params: { sessionId: 'hermes-local-abc123def' } }, m.classifyGatewaySessionOrigin({ sessionId: 'hermes-local-abc123def' })),
    caseOf('created-in-workspace-flag', { params: { createdInWorkspace: true, source: 'telegram' } }, m.classifyGatewaySessionOrigin({ createdInWorkspace: true, source: 'telegram' })),
    caseOf('unknown-external', { params: { sessionId: 'random-session' } }, m.classifyGatewaySessionOrigin({ sessionId: 'random-session' })),
  ]);

  vec(mod, 'mapGatewaySessions', 'Session record mapping: epoch seconds vs ms vs ISO strings, preview fallbacks.', [
    caseOf('mixed-timestamps', { raw: { sessions: [
      { session_id: 'hermes-general', session_key: 'voxcell-main', title: 'General', source: 'web', model: 'hermes-4', message_count: 8, last_message: 'hello', created_at: 1795089000, updated_at: 1795089300 },
      { id: 'hermes-ops', preview: 'backup done', started_at: '2026-11-19T11:50:00Z', last_active_at: 1795089400000 },
      { session_id: '  ', title: 'skipped' },
      { session_id: 'hermes-cron', first_prompt: 'nightly brief', created_at: '1795089000' },
    ] } }, m.mapGatewaySessions({ sessions: [
      { session_id: 'hermes-general', session_key: 'voxcell-main', title: 'General', source: 'web', model: 'hermes-4', message_count: 8, last_message: 'hello', created_at: 1795089000, updated_at: 1795089300 },
      { id: 'hermes-ops', preview: 'backup done', started_at: '2026-11-19T11:50:00Z', last_active_at: 1795089400000 },
      { session_id: '  ', title: 'skipped' },
      { session_id: 'hermes-cron', first_prompt: 'nightly brief', created_at: '1795089000' },
    ] })),
    caseOf('garbage', { raw: 7 }, m.mapGatewaySessions(7)),
  ]);

  vec(mod, 'mapGatewaySessionMessages', 'History mapping: role filtering, content parts, reasoning, synthetic timestamps (Date.now pinned).', [
    caseOf('mixed-messages', { raw: { messages: [
      { id: 'm1', role: 'user', content: 'Hello', created_at: 1795089000 },
      { role: 'assistant', content: [{ type: 'text', text: 'Hi there' }, { type: 'input_image' }], reasoning_content: 'greeting the user' },
      { role: 'tool', content: 'tool output skipped' },
      { role: 'developer', content: 'dev skipped' },
      { role: 'system', text: 'You are Hermes.' },
      { role: 'assistant', content: '   ' },
    ] }, sessionId: 'hermes-general' }, m.mapGatewaySessionMessages({ messages: [
      { id: 'm1', role: 'user', content: 'Hello', created_at: 1795089000 },
      { role: 'assistant', content: [{ type: 'text', text: 'Hi there' }, { type: 'input_image' }], reasoning_content: 'greeting the user' },
      { role: 'tool', content: 'tool output skipped' },
      { role: 'developer', content: 'dev skipped' },
      { role: 'system', text: 'You are Hermes.' },
      { role: 'assistant', content: '   ' },
    ] }, 'hermes-general')),
  ]);

  const decoderWire =
    ': keepalive\n\n' +
    'event: hermes.tool.progress\r\ndata: {"tool":"terminal","status":"running"}\r\n\r\n' +
    'data: line-one\ndata: line-two\n\n' +
    'data: {"partial":';
  const decoder = new m.SseLineDecoder();
  const decoderFrames = [];
  for (const chunk of chunked(decoderWire, 7)) decoderFrames.push(...decoder.push(chunk));
  const decoderFlush = decoder.flush();
  vec(mod, 'SseLineDecoder', 'SSE framing: chunk splits, CRLF, keepalive comments, multi-line data, flush tail.', [
    caseOf('chunked-wire', { chunks: chunked(decoderWire, 7) }, { frames: decoderFrames, flush: decoderFlush }),
    caseOf('empty', { chunks: ['', ': ping\n\n'] }, (() => { const d = new m.SseLineDecoder(); return { frames: d.push(''), ping: d.push(': ping\n\n'), flush: d.flush() }; })()),
  ]);

  vec(mod, 'classifyFinishReason', 'Terminal outcome classification from finish_reason + error metadata.', [
    ...[
      ['stop', undefined, undefined], ['cancelled', undefined, undefined], ['error', undefined, undefined],
      ['length', 'iteration_budget_exhausted', undefined], ['length', undefined, undefined],
      [null, 'admission_conflict', undefined], [null, undefined, 'admission_conflict'],
      [null, undefined, 'Something else'],
    ].map(([fr, code, msg]) => caseOf(`${fr}|${code}|${msg}`, { finishReason: fr, errorCode: code, errorMessage: msg }, m.classifyFinishReason(fr, code, msg))),
  ]);

  vec(mod, 'parseChatCompletionsJson', 'Non-streaming /v1/chat/completions response parsing.', [
    caseOf('full-response', { raw: {
      session_id: 'hermes-general', session_key: 'voxcell-main',
      choices: [{ finish_reason: 'stop', message: { content: 'Done.', reasoning_content: 'thought about it' } }],
      hermes: { error_code: '' },
    } }, m.parseChatCompletionsJson({
      session_id: 'hermes-general', session_key: 'voxcell-main',
      choices: [{ finish_reason: 'stop', message: { content: 'Done.', reasoning_content: 'thought about it' } }],
      hermes: { error_code: '' },
    })),
    caseOf('admission-conflict', { raw: { choices: [{ finish_reason: 'error' }], error: { code: 'admission_conflict', message: 'admission_conflict' } } },
      m.parseChatCompletionsJson({ choices: [{ finish_reason: 'error' }], error: { code: 'admission_conflict', message: 'admission_conflict' } })),
    caseOf('empty', { raw: {} }, m.parseChatCompletionsJson({})),
    caseOf('null', { raw: null }, m.parseChatCompletionsJson(null)),
  ]);

  {
    const wire =
      sseFrame(null, { session_id: 'hermes-general', session_key: 'voxcell-main', choices: [{ delta: { reasoning_content: 'Planning. ' } }] }) +
      sseFrame(null, { choices: [{ delta: { content: 'Hello' } }] }) +
      sseFrame('hermes.tool.progress', { tool: 'terminal', status: 'running', summary: 'cargo check' }) +
      sseFrame('hermes.tool.progress', { tool: 'terminal', status: 'done', summary: 'cargo check ok' }) +
      sseFrame(null, { choices: [{ delta: { content: ' world' }, finish_reason: null }] }) +
      sseFrame(null, { choices: [{ delta: {}, finish_reason: 'stop' }] }) +
      'data: [DONE]\n\n';
    const events = { reasoning: [], content: [], tools: [] };
    const parser = new m.ChatCompletionsStreamParser();
    const cb = {
      onReasoningDelta: (accumulated, chunk) => events.reasoning.push({ accumulated, chunk }),
      onContentDelta: (accumulated, chunk) => events.content.push({ accumulated, chunk }),
      onToolProgress: (ev) => events.tools.push(ev),
    };
    for (const chunk of chunked(wire, 11)) parser.push(chunk, cb);
    const result = parser.finish(cb);
    vec(mod, 'ChatCompletionsStreamParser', 'Streaming chat completions: reasoning/content deltas, hermes.tool.progress frames, finish + [DONE].', [
      caseOf('full-turn', { chunks: chunked(wire, 11) }, { result, events }),
    ]);
  }
  {
    const wire = sseFrame(null, { error: { code: 'admission_conflict', message: 'admission_conflict' }, choices: [{ delta: { content: 'partial' } }] });
    const parser = new m.ChatCompletionsStreamParser();
    parser.push(wire);
    vec(mod, 'ChatCompletionsStreamParser', 'Admission-conflict error frame classification.', [
      caseOf('admission-conflict-stream', { chunks: [wire] }, { result: parser.finish(), events: { reasoning: [], content: [], tools: [] } }),
    ]);
  }

  {
    const wire =
      sseFrame(null, { type: 'assistant.delta', delta: 'Hello' }) +
      sseFrame(null, { event: 'assistant.delta', text: ' world' }) +
      sseFrame(null, { type: 'assistant.commentary', message_id: 'c1', text: 'Checking vendor patches', already_streamed: false }) +
      sseFrame(null, { type: 'tool.started', tool: 'terminal', args_preview: 'cargo check' }) +
      sseFrame(null, { type: 'tool.completed', tool: 'terminal', result: 'ok', duration: 2.5 }) +
      sseFrame(null, { type: 'tool.failed', tool: 'web', error: 'timeout' }) +
      sseFrame(null, { type: 'assistant.completed', partial: true, interrupted: true }) +
      'data: [DONE]\n\n';
    const events = { deltas: [], commentary: [], tools: [] };
    const parser = new m.SessionChatStreamParser();
    const cb = {
      onDelta: (accumulated, chunk) => events.deltas.push({ accumulated, chunk }),
      onCommentary: (c) => events.commentary.push(c),
      onToolEvent: (ev) => events.tools.push(ev),
    };
    for (const chunk of chunked(wire, 13)) parser.push(chunk, cb);
    const result = parser.finish(cb);
    vec(mod, 'SessionChatStreamParser', 'Session chat stream: deltas, commentary, tool lifecycle, interrupted/partial finalization.', [
      caseOf('interrupted-turn', { chunks: chunked(wire, 13) }, { result, events }),
    ]);
  }
  {
    const wire =
      sseFrame(null, { event: 'assistant.completed', content: 'Full answer from finalization.' }) +
      sseFrame(null, { event: 'run.completed', output: 'Authoritative output.' });
    const parser = new m.SessionChatStreamParser();
    parser.push(wire);
    vec(mod, 'SessionChatStreamParser', 'run.completed output fallback and completed outcome.', [
      caseOf('run-completed', { chunks: [wire] }, { result: parser.finish(), events: { deltas: [], commentary: [], tools: [] } }),
    ]);
  }
  {
    const wire = sseFrame(null, { event: 'run.failed', error: 'gateway exploded' });
    const parser = new m.SessionChatStreamParser();
    parser.push(wire);
    vec(mod, 'SessionChatStreamParser', 'run.failed outcome + error propagation.', [
      caseOf('run-failed', { chunks: [wire] }, { result: parser.finish(), events: { deltas: [], commentary: [], tools: [] } }),
    ]);
  }

  {
    const frames = [
      { event: 'message.reasoning', delta: 'Considering ' },
      { event: 'message.delta', reasoning_content: 'options. ', delta: 'Hello' },
      { event: 'message.delta', delta: ' world' },
      { event: 'message.interim', text: 'Hello world', already_streamed: true },
      { event: 'tool.started', tool: 'terminal', preview: 'cargo check' },
      { event: 'tool.completed', tool: 'terminal', result: 'ok', duration: 1.5 },
      { event: 'tool.failed', tool: 'web', error: 'timeout' },
      { event: 'approval.request', request_id: 'req-1', execution_generation: 2, tool: 'terminal', command: 'rm -rf build', title: 'Approve terminal', description: 'Delete build dir', destination: 'terminal', choices: ['once', 'deny'] },
      { event: 'clarify.request', prompt_id: 'req-2', draft: 'Which branch?' },
      { event: 'subagent.start', task: 'Research RUNE APIs', child_session_id: 'hermes-child', delegation_id: 'del-1' },
      { event: 'subagent.complete', status: 'completed', summary: 'Found 3 APIs', child_session_id: 'hermes-child', delegation_id: 'del-1' },
      { event: 'run.steered', text: 'focus on layout' },
      { event: 'run.completed', output: '', runtime: { model: 'hermes-4' } },
    ];
    const wire = frames.map((f) => sseFrame(null, f)).join('') + 'data: [DONE]\n\n';
    const events = { deltas: [], reasoning: [], interim: [], tools: [], approvals: [], subagentStarts: [], subagentCompletes: [], steered: [] };
    const parser = new m.RunsEventStreamParser();
    const cb = {
      onDelta: (accumulated, chunk) => events.deltas.push({ accumulated, chunk }),
      onReasoningDelta: (accumulated, chunk) => events.reasoning.push({ accumulated, chunk }),
      onInterim: (i) => events.interim.push(i),
      onToolEvent: (ev) => events.tools.push(ev),
      onApprovalRequest: (req) => events.approvals.push(req),
      onSubagentStart: (info) => events.subagentStarts.push(info),
      onSubagentComplete: (info) => events.subagentCompletes.push(info),
      onSteered: (text) => events.steered.push(text),
    };
    for (const chunk of chunked(wire, 17)) parser.push(chunk, cb);
    const result = parser.finish(cb);
    vec(mod, 'RunsEventStreamParser', 'Full /v1/runs turn: reasoning/delta/interim, tool lifecycle, approval+clarify, subagents, steer; empty terminal output must not erase streamed text.', [
      caseOf('full-turn-empty-final-output', { chunks: chunked(wire, 17) }, { result, events }),
    ]);
  }
  {
    const variants = [
      ['authoritative-output', [{ event: 'message.delta', delta: 'draft' }, { event: 'run.completed', output: 'Final answer.', reasoning_content: 'fallback reasoning', runtime: { model: 'hermes-4' } }]],
      ['budget-exhausted', [{ event: 'run.completed', output: 'Partial.', error_code: 'iteration_budget_exhausted' }]],
      ['run-failed', [{ event: 'run.failed', error: 'boom', error_code: 'tool_error' }]],
      ['run-cancelled', [{ event: 'run.cancelled', error: 'user stopped' }]],
      ['run-interrupted', [{ event: 'run.interrupted' }]],
      ['approval-defaults', [{ event: 'approval.request', request_id: 'req-9', tool: 'terminal' }]],
    ];
    const cases = variants.map(([name, frames]) => {
      const wire = frames.map((f) => sseFrame(null, f)).join('');
      const events = { deltas: [], reasoning: [], interim: [], tools: [], approvals: [], subagentStarts: [], subagentCompletes: [], steered: [] };
      const parser = new m.RunsEventStreamParser();
      const cb = {
        onDelta: (a, c) => events.deltas.push({ accumulated: a, chunk: c }),
        onReasoningDelta: (a, c) => events.reasoning.push({ accumulated: a, chunk: c }),
        onInterim: (i) => events.interim.push(i),
        onToolEvent: (ev) => events.tools.push(ev),
        onApprovalRequest: (req) => events.approvals.push(req),
        onSubagentStart: (info) => events.subagentStarts.push(info),
        onSubagentComplete: (info) => events.subagentCompletes.push(info),
        onSteered: (text) => events.steered.push(text),
      };
      parser.push(wire, cb);
      return caseOf(name, { chunks: [wire] }, { result: parser.finish(cb), events });
    });
    vec(mod, 'RunsEventStreamParser', 'Terminal event variants: authoritative output, budget, failed/cancelled/interrupted, approval defaults.', cases);
  }

  {
    // Offline 429-retry vectors: makeRequest/sleep are injected, no network is touched.
    const runRetry = async (responses, opts) => {
      const queue = responses.map((r) => new Response(r.body ?? '', {
        status: r.status,
        headers: r.retryAfter !== undefined ? { 'retry-after': r.retryAfter } : {},
      }));
      let attempts = 0;
      const waitsMs = [];
      const res = await m.fetchWithConcurrentRetry(
        async () => { attempts += 1; return queue[Math.min(attempts - 1, queue.length - 1)]; },
        { ...opts, sleep: async (ms) => { waitsMs.push(ms); } },
      );
      return { finalStatus: res.status, attempts, waitsMs };
    };
    const cases = [];
    cases.push(caseOf('retry-after-then-exp-backoff', {
      responses: [{ status: 429, retryAfter: '2' }, { status: 429 }, { status: 200, body: 'ok' }],
      opts: { maxRetries: 3, baseDelayMs: 350, maxDelayMs: 5000 },
    }, await runRetry([{ status: 429, retryAfter: '2' }, { status: 429 }, { status: 200, body: 'ok' }], { maxRetries: 3, baseDelayMs: 350, maxDelayMs: 5000 })));
    cases.push(caseOf('max-retries-exhausted', {
      responses: [{ status: 429 }, { status: 429 }, { status: 429 }],
      opts: { maxRetries: 1, baseDelayMs: 350, maxDelayMs: 5000 },
    }, await runRetry([{ status: 429 }, { status: 429 }, { status: 429 }], { maxRetries: 1, baseDelayMs: 350, maxDelayMs: 5000 })));
    cases.push(caseOf('retry-after-http-date', {
      responses: [{ status: 429, retryAfter: 'Thu, 19 Nov 2026 12:00:10 GMT' }, { status: 200 }],
      opts: { maxRetries: 3, baseDelayMs: 350, maxDelayMs: 30000 },
    }, await runRetry([{ status: 429, retryAfter: 'Thu, 19 Nov 2026 12:00:10 GMT' }, { status: 200 }], { maxRetries: 3, baseDelayMs: 350, maxDelayMs: 30000 })));
    cases.push(caseOf('immediate-success', {
      responses: [{ status: 200 }], opts: {},
    }, await runRetry([{ status: 200 }], {})));
    vec(mod, 'fetchWithConcurrentRetry', '429 concurrent-run retry with injected makeRequest/sleep (offline). Retry-After seconds, HTTP-date (Date.now pinned), exponential backoff, retry cap.', cases);
  }
}

// ---------------------------------------------------------------------------
// profileRegistry
// ---------------------------------------------------------------------------
if (loaded.profileRegistry) {
  const m = loaded.profileRegistry;
  const mod = 'profileRegistry';

  vec(mod, 'normalizeProfileId', 'Slug normalization + reserved-slug guard (api/v1/health/p/sessions/runs -> default).', [
    ...['  Coder Bot! ', 'api', 'v1', 'health', 'p', 'sessions', 'runs', '', null, 'UPPER_case-9', 'x'.repeat(60), '--trimmed--'].map((raw) =>
      caseOf(rawLabel(raw), { raw }, m.normalizeProfileId(raw))),
  ]);

  vec(mod, 'isValidCustomProfileSlug', 'Custom slug validity rules.', [
    ...['coder', 'default', 'api', 'a', '9lives', '-x', 'x-', 'x_y', 'has space', ''].map((raw) =>
      caseOf(rawLabel(raw), { raw }, m.isValidCustomProfileSlug(raw))),
  ]);

  vec(mod, 'normalizeBaseUrl', 'Trailing-slash trimming.', [
    ...['http://127.0.0.1:8642///', ' http://x.dev ', '', null].map((raw) =>
      caseOf(rawLabel(raw), { raw }, m.normalizeBaseUrl(raw))),
  ]);

  vec(mod, 'isValidHttpUrl', 'HTTP(S)-only URL validation.', [
    ...['http://127.0.0.1:8642', 'https://gw.example.dev/', 'ftp://x.dev', 'not a url', ''].map((raw) =>
      caseOf(rawLabel(raw), { raw }, m.isValidHttpUrl(raw))),
  ]);

  const envJson = JSON.stringify([
    { id: 'coder', label: ' Coder ', mode: 'standalone', baseUrl: 'http://127.0.0.1:8643/', apiKey: 'coder-key', description: 'Coding side session' },
    { name: 'researcher', baseUrl: 'http://127.0.0.1:8644' },
    { id: 'coder', label: 'duplicate dropped' },
    { id: 'api', label: 'reserved becomes default' },
    { id: 'badurl', baseUrl: 'ftp://nope' },
    'skipped',
  ]);
  vec(mod, 'parseHermesProfilesEnv', 'HERMES_PROFILES parsing: JSON array, CSV shorthand, malformed-JSON fallthrough, dedupe.', [
    caseOf('json-array', { env: { HERMES_PROFILES: envJson } }, m.parseHermesProfilesEnv({ HERMES_PROFILES: envJson })),
    caseOf('csv-mixed', { env: { HERMES_PROFILES: 'coder=http://127.0.0.1:8643, researcher , ,api' } }, m.parseHermesProfilesEnv({ HERMES_PROFILES: 'coder=http://127.0.0.1:8643, researcher , ,api' })),
    caseOf('malformed-json-falls-through', { env: { HERMES_PROFILES: '[broken json' } }, m.parseHermesProfilesEnv({ HERMES_PROFILES: '[broken json' })),
    caseOf('empty', { env: {} }, m.parseHermesProfilesEnv({})),
  ]);

  const baseEnv = { HERMES_API_URL: 'http://127.0.0.1:8642', HERMES_API_KEY: 'default-key' };
  vec(mod, 'resolveProfileRuntimeConfig', 'Per-profile routing: standalone vs /p/<profile>, key precedence explicit > profile-env > shared > default.', [
    caseOf('default-profile', { def: { id: 'default', mode: 'standalone', source: 'env' }, env: baseEnv },
      m.resolveProfileRuntimeConfig({ id: 'default', mode: 'standalone', source: 'env' }, baseEnv)),
    caseOf('standalone-with-url', { def: { id: 'coder', mode: 'standalone', baseUrl: 'http://127.0.0.1:8643/', source: 'env', apiKey: 'explicit' }, env: baseEnv },
      m.resolveProfileRuntimeConfig({ id: 'coder', mode: 'standalone', baseUrl: 'http://127.0.0.1:8643/', source: 'env', apiKey: 'explicit' }, baseEnv)),
    caseOf('standalone-without-url-downgrades', { def: { id: 'coder', mode: 'standalone', source: 'discovered' }, env: baseEnv },
      m.resolveProfileRuntimeConfig({ id: 'coder', mode: 'standalone', source: 'discovered' }, baseEnv)),
    caseOf('multiplexed-shared-key', { def: { id: 'researcher', mode: 'multiplexed', source: 'env' }, env: { ...baseEnv, HERMES_PROFILE_API_KEY: 'shared' } },
      m.resolveProfileRuntimeConfig({ id: 'researcher', mode: 'multiplexed', source: 'env' }, { ...baseEnv, HERMES_PROFILE_API_KEY: 'shared' })),
    caseOf('profile-env-key-wins-over-shared', { def: { id: 'researcher', mode: 'multiplexed', source: 'env' }, env: { ...baseEnv, HERMES_PROFILE_API_KEY: 'shared', HERMES_API_KEY_RESEARCHER: 'scoped' } },
      m.resolveProfileRuntimeConfig({ id: 'researcher', mode: 'multiplexed', source: 'env' }, { ...baseEnv, HERMES_PROFILE_API_KEY: 'shared', HERMES_API_KEY_RESEARCHER: 'scoped' })),
  ]);

  const catalogEnv = {
    HERMES_API_URL: 'http://127.0.0.1:8642',
    HERMES_API_KEY: 'default-key',
    HERMES_PROFILES: JSON.stringify([{ id: 'coder', mode: 'standalone', baseUrl: 'http://127.0.0.1:8643' }]),
  };
  const discovered = [
    { id: 'researcher', mode: 'multiplexed', source: 'discovered', description: 'Found in ~/.hermes/profiles' },
    { id: 'coder', mode: 'multiplexed', source: 'discovered', label: 'Discovered coder label' },
  ];
  const custom = [
    { id: 'coder', mode: 'standalone', baseUrl: 'http://127.0.0.1:9999', source: 'custom', apiKey: 'custom-key' },
    { id: 'lab', mode: 'multiplexed', source: 'custom' },
  ];
  vec(mod, 'buildProfileCatalog', 'Catalog merge precedence custom > discovered > env, default-first + alphabetical sort.', [
    caseOf('full-merge', { env: catalogEnv, discoveredProfiles: discovered, customProfiles: custom },
      m.buildProfileCatalog({ env: catalogEnv, discoveredProfiles: discovered, customProfiles: custom })),
    caseOf('env-only', { env: catalogEnv }, m.buildProfileCatalog({ env: catalogEnv })),
    caseOf('named-primary', { env: { ...catalogEnv, HERMES_PROFILE: 'coder', HERMES_API_KEY_CODER: 'coder-key' } },
      m.buildProfileCatalog({ env: { ...catalogEnv, HERMES_PROFILE: 'coder', HERMES_API_KEY_CODER: 'coder-key' } })),
  ]);

  {
    const runtime = m.resolveProfileRuntimeConfig({ id: 'coder', mode: 'standalone', baseUrl: 'http://127.0.0.1:8643', source: 'custom', apiKey: 'secret-key', description: 'Coder profile' }, baseEnv);
    vec(mod, 'toRedactedProfileSummary', 'API keys fully redacted for UI summaries; gateway status merged when present.', [
      caseOf('without-status', { runtime }, m.toRedactedProfileSummary(runtime)),
      caseOf('with-status', { runtime, status: gatewayStatusConnected }, m.toRedactedProfileSummary(runtime, gatewayStatusConnected)),
      caseOf('offline-status-error', { runtime, status: gatewayStatusOffline }, m.toRedactedProfileSummary(runtime, gatewayStatusOffline)),
    ]);
  }
}

// ---------------------------------------------------------------------------
// settingsSnapshot
// ---------------------------------------------------------------------------
if (loaded.settingsSnapshot) {
  const m = loaded.settingsSnapshot;
  const mod = 'settingsSnapshot';

  const legacyEmeraldUi = {
    accentPreset: 'emerald', accentHex: '#34D399', surfaceHex: '#0A1613', textHex: '#ECF3F0',
    panelAlpha: 0.76, borderOpacity: 0.42, glowIntensity: 1,
  };
  const minimalRaw = { capturedAt: T0 - MIN, appVersion: '0.1.0', reason: 'sync' };
  const legacyRaw = { ...minimalRaw, uiConfig: legacyEmeraldUi };
  const customizedEmeraldRaw = { ...minimalRaw, uiConfig: { ...legacyEmeraldUi, panelAlpha: 0.9 } };
  const slateRaw = { ...minimalRaw, themeEpoch: 'slate-v1', snapshotId: 'snap-1', sourceOrigin: 'http://127.0.0.1:5173', reason: 'manual', uiConfig: { accentPreset: 'violet', workspaceTitle: 'Lab' } };
  const canvasRaw = {
    ...minimalRaw,
    workspace: { showThinking: false, voiceMuted: true, mood: 'focused', draftProfile: ' coder ', transportMode: 'runs', onboardingCompleted: true },
    canvas: {
      sessions: [
        { id: 'dot-1', channelId: 'general', hermesSessionId: 'hermes-general', name: 'General', shape: 'star', xPct: 10, yPct: 20, mood: 'thinking' },
        { name: '#Second Dot', xPct: 500, yPct: -40 },
        { channelId: 'general', name: 'duplicate channel dropped' },
        'garbage',
      ],
      pins: [{ id: 'pin-1', kind: 'note', title: 'Remember this', xPct: 30, yPct: 40 }, { title: 'auto id' }, { id: 'pin-1', title: 'dup dropped' }],
      dismissedIds: ['ok-1', '  ', 7, 'ok-2'],
    },
    background: { activeId: 'aurora-drift', opacity: 0.5, blurPx: 10, customSlots: [] },
  };

  vec(mod, 'normalizePreferencesSnapshot', 'Snapshot normalization: defaults, legacy untouched-emerald auto-migration to slate-v1, customized palettes preserved, canvas/workspace sanitization.', [
    caseOf('minimal-defaults', { raw: minimalRaw }, m.normalizePreferencesSnapshot(minimalRaw)),
    caseOf('legacy-untouched-emerald-migrates', { raw: legacyRaw }, m.normalizePreferencesSnapshot(legacyRaw)),
    caseOf('customized-emerald-preserved', { raw: customizedEmeraldRaw }, m.normalizePreferencesSnapshot(customizedEmeraldRaw)),
    caseOf('explicit-slate-with-custom-ui', { raw: slateRaw }, m.normalizePreferencesSnapshot(slateRaw)),
    caseOf('full-canvas-workspace', { raw: canvasRaw }, m.normalizePreferencesSnapshot(canvasRaw)),
    caseOf('missing-capturedAt-uses-pinned-now', { raw: { appVersion: '0.1.0' } }, m.normalizePreferencesSnapshot({ appVersion: '0.1.0' })),
    caseOf('invalid-mood-and-version-fallback', { raw: { capturedAt: T0, appVersion: '  ', workspace: { mood: 'angry', draftProfile: '   ' } }, fallbackVersion: '9.9.9' },
      m.normalizePreferencesSnapshot({ capturedAt: T0, appVersion: '  ', workspace: { mood: 'angry', draftProfile: '   ' } }, '9.9.9')),
    caseOf('null', { raw: null }, m.normalizePreferencesSnapshot(null)),
    caseOf('scalar', { raw: 'nope' }, m.normalizePreferencesSnapshot('nope')),
  ]);

  const defaultSnap = m.normalizePreferencesSnapshot(minimalRaw);
  const customSnap = m.normalizePreferencesSnapshot(slateRaw);
  vec(mod, 'isUiConfigDefault', 'UI config equality against slate-v1 defaults.', [
    caseOf('default', { config: defaultSnap.uiConfig }, m.isUiConfigDefault(defaultSnap.uiConfig)),
    caseOf('customized', { config: customSnap.uiConfig }, m.isUiConfigDefault(customSnap.uiConfig)),
  ]);
  vec(mod, 'isBackgroundDefault', 'Background settings equality against defaults.', [
    caseOf('default', { bg: defaultSnap.background }, m.isBackgroundDefault(defaultSnap.background)),
    caseOf('customized', { bg: canvasRaw.background }, m.isBackgroundDefault(m.normalizePreferencesSnapshot(canvasRaw).background)),
  ]);
  vec(mod, 'hasCustomizedPreferences', 'Detects any user customization across ui/background/workspace/canvas.', [
    caseOf('null', { snapshot: null }, m.hasCustomizedPreferences(null)),
    caseOf('default-snapshot', { snapshot: defaultSnap }, m.hasCustomizedPreferences(defaultSnap)),
    caseOf('custom-ui', { snapshot: customSnap }, m.hasCustomizedPreferences(customSnap)),
    caseOf('canvas-only-customization', { snapshot: m.normalizePreferencesSnapshot(canvasRaw) }, m.hasCustomizedPreferences(m.normalizePreferencesSnapshot(canvasRaw))),
  ]);

  const vaultCustom = m.normalizePreferencesSnapshot({ ...slateRaw, capturedAt: T0 - 2 * MIN });
  const vaultPreUpdate = m.normalizePreferencesSnapshot({ ...minimalRaw, reason: 'pre-update', capturedAt: T0 - 30_000 });
  const vaultRestoreOlder = m.normalizePreferencesSnapshot({ ...minimalRaw, reason: 'restore', capturedAt: T0 - 10 * MIN });
  vec(mod, 'resolveBootHydrationSource', 'Boot hydration precedence: vault wins for untouched-local + customized-vault, pre-update/restore recency, zero local timestamp.', [
    caseOf('no-vault', { localSnapshot: defaultSnap, vaultSnapshot: null }, m.resolveBootHydrationSource(defaultSnap, null)),
    caseOf('local-default-vault-custom', { localSnapshot: defaultSnap, vaultSnapshot: vaultCustom }, m.resolveBootHydrationSource(defaultSnap, vaultCustom)),
    caseOf('both-custom-local-wins', { localSnapshot: customSnap, vaultSnapshot: vaultCustom }, m.resolveBootHydrationSource(customSnap, vaultCustom)),
    caseOf('pre-update-newer-than-local', { localSnapshot: defaultSnap, vaultSnapshot: vaultPreUpdate }, m.resolveBootHydrationSource(defaultSnap, vaultPreUpdate)),
    caseOf('restore-older-than-local', { localSnapshot: defaultSnap, vaultSnapshot: vaultRestoreOlder }, m.resolveBootHydrationSource(defaultSnap, vaultRestoreOlder)),
    caseOf('local-zero-timestamp', { localSnapshot: { ...defaultSnap, capturedAt: 0 }, vaultSnapshot: vaultPreUpdate }, m.resolveBootHydrationSource({ ...defaultSnap, capturedAt: 0 }, vaultPreUpdate)),
  ]);

  vec(mod, 'summarizePreservedCategories', 'Preserved-category diffs for update receipts.', [
    caseOf('default-snapshot', { snapshot: defaultSnap }, m.summarizePreservedCategories(defaultSnap)),
    caseOf('customized-snapshot', { snapshot: m.normalizePreferencesSnapshot(canvasRaw) }, m.summarizePreservedCategories(m.normalizePreferencesSnapshot(canvasRaw))),
  ]);

  vec(mod, 'summarizeBackupRecord', 'Rolling-backup metadata summary.', [
    caseOf('manual-backup', { id: 'manual-2026-11-19', snapshot: customSnap }, m.summarizeBackupRecord('manual-2026-11-19', customSnap)),
    caseOf('pre-update-backup', { id: 'pre-update-abc', snapshot: m.normalizePreferencesSnapshot({ ...minimalRaw, reason: 'pre-update' }) },
      m.summarizeBackupRecord('pre-update-abc', m.normalizePreferencesSnapshot({ ...minimalRaw, reason: 'pre-update' }))),
  ]);

  vec(mod, 'compareSemver', 'Semver comparison with v-prefix, prerelease suffix, short forms.', [
    ...[
      ['0.1.0', '0.2.0'], ['0.2.0', '0.1.0'], ['1.0.0', '1.0.0'], ['v1.2.3', '1.2.4'],
      ['1.2.3-beta.1', '1.2.3'], ['1.2', '1.2.0'], ['2.0.0', '10.0.0'], ['garbage', '0.0.1'],
    ].map(([a, b]) => caseOf(`${a}_vs_${b}`, { currentRaw: a, latestRaw: b }, m.compareSemver(a, b))),
  ]);

  vec(mod, 'parseImportedPreferencesJson', 'JSON import parser: re-stamps capturedAt (Date.now pinned) + reason=import; invalid text -> null.', [
    caseOf('valid-import', { rawText: JSON.stringify(slateRaw) }, m.parseImportedPreferencesJson(JSON.stringify(slateRaw))),
    caseOf('invalid-json', { rawText: '{broken' }, m.parseImportedPreferencesJson('{broken')),
    caseOf('non-object-json', { rawText: '42' }, m.parseImportedPreferencesJson('42')),
  ]);
}

// ---------------------------------------------------------------------------
// responsiveLayout
// ---------------------------------------------------------------------------
if (loaded.responsiveLayout) {
  const m = loaded.responsiveLayout;
  const mod = 'responsiveLayout';

  vec(mod, 'resolveThreadTasksLayoutMode', 'split vs overlay task drawer: >=900px stage on desktop-lg, overlay otherwise/unknown width.', [
    ...[
      [900, true], [899, true], [901, true], [1440, true], [900, false],
      [null, true], [undefined, true], [0, true], [-5, true], [NaN, true], [Infinity, true],
    ].map(([w, lg]) => caseOf(`w=${w},lg=${lg}`, { stageWidthPx: w, isDesktopLg: lg }, m.resolveThreadTasksLayoutMode(w, lg))),
    caseOf('threshold-constant', { note: 'THREAD_SPLIT_MIN_STAGE_WIDTH' }, m.THREAD_SPLIT_MIN_STAGE_WIDTH),
  ]);

  vec(mod, 'computeCanvasPopoverAlignment', 'Edge-safe popover alignment: left <24%, right >76%, center otherwise; non-finite -> center.', [
    ...[0, 23.9, 24, 50, 76, 76.1, 100, NaN, Infinity].map((x) =>
      caseOf(`x=${x}`, { xPct: x }, m.computeCanvasPopoverAlignment(x))),
  ]);

  vec(mod, 'resolveStageHeaderSize', 'Portrait header tiers: home=full, canvas=strip, queue/cli compact>=860, voice(thread) compact>=700.', [
    ...['home', 'canvas', 'voice', 'queue', 'cli'].flatMap((panelView) =>
      [699, 700, 768, 859, 860, 1080].map((h) =>
        caseOf(`${panelView}@${h}`, { params: { panelView, viewportHeightPx: h } }, m.resolveStageHeaderSize({ panelView, viewportHeightPx: h })))),
  ]);
}

// ---------------------------------------------------------------------------
// taskInsights
// ---------------------------------------------------------------------------
if (loaded.taskInsights) {
  const m = loaded.taskInsights;
  const mod = 'taskInsights';

  vec(mod, 'formatElapsedDuration', 'Compact elapsed formatting: seconds, m+ss, h+mm; invalid inputs -> 0s.', [
    ...[
      [T0 - 42_000, T0, true], [T0 - 60_000, T0, true], [T0 - 248_000, T0, true],
      [T0 - 248_000, T0, false], [T0 - 4_020_000, T0, true], [T0 + 5_000, T0, true],
      [undefined, T0, true], [NaN, T0, true], [T0, NaN, true],
    ].map(([created, now, showSeconds]) =>
      caseOf(`created=${created},showSeconds=${showSeconds}`, { createdAtMs: created, nowMs: now, showSeconds }, m.formatElapsedDuration(created, now, showSeconds))),
  ]);

  vec(mod, 'formatStartClock', 'HH:MM 24h clock. TZ pinned to UTC by the generator (see manifest); 0/invalid -> empty string.', [
    ...[0, undefined, NaN, T0, Date.UTC(2026, 10, 19, 0, 5, 0), Date.UTC(2026, 10, 19, 23, 59, 59)].map((ms) =>
      caseOf(String(ms), { createdAtMs: ms }, m.formatStartClock(ms))),
  ]);

  vec(mod, 'getTaskKindLabel', 'In-chat vs background kind labels.', [
    caseOf('in-chat', { task: { ...taskRunningSteps, background: false } }, m.getTaskKindLabel({ ...taskRunningSteps, background: false })),
    caseOf('background-default', { task: { ...taskRunningSteps, kind: undefined } }, m.getTaskKindLabel({ ...taskRunningSteps, kind: undefined })),
    caseOf('custom-kind', { task: { ...taskRunningSteps, kind: 'Cron' } }, m.getTaskKindLabel({ ...taskRunningSteps, kind: 'Cron' })),
  ]);

  const confirmTasks = [
    ['running-open', taskRunningSteps], ['completed-needs-confirm', taskCompletedUnconfirmed],
    ['confirmed', taskCompletedConfirmed], ['failed-open', taskFailed], ['cancelled-open', taskCancelled],
  ];
  vec(mod, 'getTaskConfirmationState', 'Confirmation state machine.', confirmTasks.map(([name, task]) => caseOf(name, { task }, m.getTaskConfirmationState(task))));
  vec(mod, 'getTaskConfirmationLabel', 'Confirmation labels.', confirmTasks.map(([name, task]) => caseOf(name, { task }, m.getTaskConfirmationLabel(task))));

  vec(mod, 'getTaskStatusDisplay', 'Status label/pulse/tone across raw statuses.', [
    ...[
      ['running', taskRunningSteps], ['queued', taskQueued], ['waiting-approval', taskPausedApproval],
      ['paused-generic', { ...taskPausedApproval, rawStatus: 'paused' }], ['done', taskCompletedUnconfirmed],
      ['failed', taskFailed], ['cancelled', taskCancelled],
      ['interrupted', { ...taskCancelled, rawStatus: 'interrupted' }], ['budget', taskBudget],
      ['budget-error-code', { ...taskCompletedUnconfirmed, errorCode: 'iteration_budget_exhausted' }],
    ].map(([name, task]) => caseOf(name, { task }, m.getTaskStatusDisplay(task))),
  ]);

  vec(mod, 'resolveTaskStepBreakdown', 'Step checklist resolution: explicit steps, completed override, fraction labels, fallback single step.', [
    ...[
      ['running-with-steps', taskRunningSteps], ['running-ratio-label', taskRunningRatio],
      ['running-zero-fallback', taskRunningZero], ['completed-all-done', taskCompletedUnconfirmed],
      ['failed-no-active', { ...taskRunningSteps, status: 'completed', rawStatus: 'failed' }],
      ['cancelled-no-active', { ...taskRunningSteps, status: 'completed', rawStatus: 'cancelled' }],
      ['paused-pending', { ...taskRunningSteps, status: 'paused', rawStatus: 'waiting_for_approval' }],
    ].map(([name, task]) => caseOf(name, { task }, m.resolveTaskStepBreakdown(task))),
  ]);

  vec(mod, 'resolveTaskProgress', 'Progress pct/ratio/indeterminate derivation.', [
    ...[
      ['running-steps', taskRunningSteps], ['running-ratio', taskRunningRatio],
      ['running-zero-indeterminate', taskRunningZero], ['completed', taskCompletedUnconfirmed],
      ['failed-not-100', taskFailed], ['progress-clamped', { ...taskRunningZero, progress: 250 }],
    ].map(([name, task]) => caseOf(name, { task }, m.resolveTaskProgress(task))),
  ]);

  const partitionTasks = [
    { ...taskRunningSteps, id: 'run-old', updatedAtMs: T0 - 6 * MIN },
    { ...taskRunningSteps, id: 'run-new', updatedAtMs: T0 - 1 * MIN },
    { ...taskPausedApproval, id: 'pause-1' },
    { ...taskCompletedUnconfirmed, id: 'done-unconfirmed-old', updatedAtMs: T0 - 20 * MIN },
    { ...taskCompletedUnconfirmed, id: 'done-unconfirmed-new', updatedAtMs: T0 - 2 * MIN },
    { ...taskCompletedConfirmed, id: 'done-confirmed', updatedAtMs: T0 - 1 * MIN },
    { ...taskRunningSteps, id: 'other-channel', channelId: 'design' },
  ];
  vec(mod, 'partitionChannelTasks', 'Thread-scoped partition: running first (recency), paused, completed (needs-confirm before confirmed, then recency).', [
    caseOf('general-partition', { tasks: partitionTasks, channelId: 'general' }, m.partitionChannelTasks(partitionTasks, 'general')),
    caseOf('empty-channel', { tasks: partitionTasks, channelId: 'nowhere' }, m.partitionChannelTasks(partitionTasks, 'nowhere')),
  ]);

  {
    const partition = m.partitionChannelTasks(partitionTasks, 'general');
    const emptyPartition = m.partitionChannelTasks([], 'general');
    vec(mod, 'summarizeThreadTasks', 'Header badge + counts, synthetic thinking task when nothing running.', [
      caseOf('mixed', { partition, activeIsThinking: false }, m.summarizeThreadTasks(partition, false)),
      caseOf('thinking-only', { partition: emptyPartition, activeIsThinking: true }, m.summarizeThreadTasks(emptyPartition, true)),
      caseOf('empty', { partition: emptyPartition, activeIsThinking: false }, m.summarizeThreadTasks(emptyPartition, false)),
      caseOf('paused-only', { partition: m.partitionChannelTasks([taskPausedApproval], 'general'), activeIsThinking: false },
        m.summarizeThreadTasks(m.partitionChannelTasks([taskPausedApproval], 'general'), false)),
      caseOf('needs-confirm-only', { partition: m.partitionChannelTasks([taskCompletedUnconfirmed], 'general'), activeIsThinking: false },
        m.summarizeThreadTasks(m.partitionChannelTasks([taskCompletedUnconfirmed], 'general'), false)),
    ]);
    vec(mod, 'summarizeThreadTaskAggregate', 'Aggregate completion strip across a thread partition.', [
      caseOf('mixed', { partition, activeIsThinking: true }, m.summarizeThreadTaskAggregate(partition, true)),
      caseOf('empty-thinking', { partition: emptyPartition, activeIsThinking: true }, m.summarizeThreadTaskAggregate(emptyPartition, true)),
      caseOf('empty', { partition: emptyPartition, activeIsThinking: false }, m.summarizeThreadTaskAggregate(emptyPartition, false)),
    ]);
  }

  vec(mod, 'formatTaskOutcomeSnippet', 'Outcome snippet: whitespace collapse, redundancy guard, word-boundary truncation.', [
    caseOf('short-result', { task: taskCompletedUnconfirmed }, m.formatTaskOutcomeSnippet(taskCompletedUnconfirmed)),
    caseOf('no-result', { task: taskRunningSteps }, m.formatTaskOutcomeSnippet(taskRunningSteps)),
    caseOf('equals-lastStep', { task: { ...taskCompletedUnconfirmed, result: 'Completed response' } }, m.formatTaskOutcomeSnippet({ ...taskCompletedUnconfirmed, result: 'Completed response' })),
    caseOf('long-result-truncated', { task: { ...taskCompletedUnconfirmed, result: 'word '.repeat(60) }, maxChars: 80 }, m.formatTaskOutcomeSnippet({ ...taskCompletedUnconfirmed, result: 'word '.repeat(60) }, 80)),
  ]);

  vec(mod, 'isTaskInitiallyExpanded', 'Only the lead running task expands inside the sidebar.', [
    caseOf('lead-in-sidebar', { task: taskRunningSteps, leadRunningTaskId: 'task-build', inSidebar: true }, m.isTaskInitiallyExpanded(taskRunningSteps, 'task-build', true)),
    caseOf('non-lead', { task: taskRunningSteps, leadRunningTaskId: 'other', inSidebar: true }, m.isTaskInitiallyExpanded(taskRunningSteps, 'other', true)),
    caseOf('outside-sidebar', { task: taskRunningSteps, leadRunningTaskId: 'task-build', inSidebar: false }, m.isTaskInitiallyExpanded(taskRunningSteps, 'task-build', false)),
    caseOf('completed-lead', { task: taskCompletedUnconfirmed, leadRunningTaskId: 'task-done', inSidebar: true }, m.isTaskInitiallyExpanded(taskCompletedUnconfirmed, 'task-done', true)),
  ]);

  vec(mod, 'summarizeChannelTaskActivity', 'Per-channel active-task badges incl. thinking-only channels.', [
    caseOf('multi-channel', {
      tasks: [taskRunningSteps, taskRunningZero, taskPausedApproval, taskCompletedUnconfirmed],
      channels: [{ id: 'general' }, { id: 'design' }, { id: 'ops' }],
      thinkingByChannel: { ops: true, 'extra-think': true },
    }, m.summarizeChannelTaskActivity(
      [taskRunningSteps, taskRunningZero, taskPausedApproval, taskCompletedUnconfirmed],
      [{ id: 'general' }, { id: 'design' }, { id: 'ops' }],
      { ops: true, 'extra-think': true },
    )),
  ]);

  vec(mod, 'deriveTurnTaskTitle', 'Prompt-derived titles: URL stripping, whitespace collapse, 64-char word-boundary clip, fallback.', [
    caseOf('short', { prompt: 'Fix the halftone emitter' }, m.deriveTurnTaskTitle('Fix the halftone emitter')),
    caseOf('url-stripped', { prompt: 'Check https://www.github.com/anomalyco/rune now' }, m.deriveTurnTaskTitle('Check https://www.github.com/anomalyco/rune now')),
    caseOf('long-clipped', { prompt: 'Please refactor the entire halftone pipeline to use GPU instancing instead of CPU rasterization' },
      m.deriveTurnTaskTitle('Please refactor the entire halftone pipeline to use GPU instancing instead of CPU rasterization')),
    caseOf('empty-fallback', { prompt: '   ', fallbackLabel: '#general' }, m.deriveTurnTaskTitle('   ', '#general')),
    caseOf('empty-no-fallback', { prompt: '' }, m.deriveTurnTaskTitle('')),
  ]);

  vec(mod, 'resolveTaskDisplayTitle', 'Vague nudge titles promoted from concrete steps; descriptive titles kept.', [
    caseOf('descriptive-kept', { task: taskRunningSteps }, m.resolveTaskDisplayTitle(taskRunningSteps)),
    caseOf('vague-promoted-from-step', { task: taskVague }, m.resolveTaskDisplayTitle(taskVague)),
    caseOf('vague-bootstrap-skipped', { task: { ...taskVague, steps: [{ text: 'Working in hermes-general' }, { text: 'Composing response' }], lastStep: 'Running terminal' } },
      m.resolveTaskDisplayTitle({ ...taskVague, steps: [{ text: 'Working in hermes-general' }, { text: 'Composing response' }], lastStep: 'Running terminal' })),
    caseOf('vague-lastStep-fallback', { task: { ...taskVague, steps: [], lastStep: 'terminal: gh pr checks' } },
      m.resolveTaskDisplayTitle({ ...taskVague, steps: [], lastStep: 'terminal: gh pr checks' })),
    caseOf('empty-title', { task: { ...taskVague, title: '  ' } }, m.resolveTaskDisplayTitle({ ...taskVague, title: '  ' })),
  ]);

  {
    const events = [
      { toolName: 'terminal', status: 'running', summary: '' },
      { toolName: 'terminal', status: 'running', summary: 'terminal: cargo check' },
      { toolName: 'web_search', status: 'running', summary: 'web_search: rune instancing' },
      { toolName: 'terminal', status: 'done', summary: 'terminal: cargo check ok' },
      { toolName: 'web_search', status: 'failed', summary: 'web_search: timeout' },
    ];
    let steps = [{ text: 'Working in hermes-general', done: false, active: true }];
    const trace = [];
    for (const ev of events) {
      const r = m.applyToolProgressToSteps(steps, ev);
      steps = r.steps;
      trace.push({ event: ev, steps: r.steps, progress: r.progress });
    }
    vec(mod, 'applyToolProgressToSteps', 'Tool-progress event fold: placeholder upgrade in-place, prior-step settling, terminal close, failure keeps step open.', [
      caseOf('event-sequence', { prevSteps: [{ text: 'Working in hermes-general', done: false, active: true }], events }, { final: { steps, progress: trace[trace.length - 1].progress }, trace }),
      caseOf('done-without-open-step', { prevSteps: [], events: [{ toolName: 'terminal', status: 'done', summary: 'terminal: ls' }] },
        m.applyToolProgressToSteps([], { toolName: 'terminal', status: 'done', summary: 'terminal: ls' })),
    ]);
  }

  vec(mod, 'applyStreamingDeltaToSteps', 'Transition into Composing response; idempotent when already composing.', [
    caseOf('from-tool-steps', { prevSteps: [{ text: 'terminal: cargo check', done: true }, { text: 'web_search: rune', done: false, active: true }] },
      m.applyStreamingDeltaToSteps([{ text: 'terminal: cargo check', done: true }, { text: 'web_search: rune', done: false, active: true }])),
    caseOf('already-composing', { prevSteps: [{ text: 'Composing response', done: false, active: true }] },
      m.applyStreamingDeltaToSteps([{ text: 'Composing response', done: false, active: true }])),
    caseOf('empty', { prevSteps: [] }, m.applyStreamingDeltaToSteps([])),
  ]);

  vec(mod, 'finalizeTurnTaskSteps', 'Turn settlement for done/failed/cancelled/budget outcomes.', [
    ...['done', 'failed', 'cancelled', 'iteration_budget_exhausted'].flatMap((outcome) => [
      caseOf(`${outcome}-with-steps`, { prevSteps: [{ text: 'Working in hermes-general', done: false, active: true }, { text: 'terminal: cargo check', done: true }], outcome, finalLabel: 'Refactor complete' },
        m.finalizeTurnTaskSteps([{ text: 'Working in hermes-general', done: false, active: true }, { text: 'terminal: cargo check', done: true }], outcome, 'Refactor complete')),
      caseOf(`${outcome}-empty`, { prevSteps: [], outcome }, m.finalizeTurnTaskSteps([], outcome)),
    ]),
  ]);

  vec(mod, 'isActivityEntryRunning', 'In-flight activity detection via status or Running-text heuristic.', [
    ...[
      ['running-status', toolRunningEntry], ['done-status', toolDoneEntry],
      ['running-text-no-status', { ...toolRunningEntry, activityStatus: undefined }],
      ['queued', { ...toolRunningEntry, activityStatus: 'queued' }],
      ['failed', { ...toolRunningEntry, activityStatus: 'failed' }],
      ['non-activity', userEntry],
    ].map(([name, e]) => caseOf(name, { entry: e }, m.isActivityEntryRunning(e))),
  ]);

  {
    const entries = [userEntry, reasoningEntry, toolRunningEntry, replyEntry, lateToolEntry];
    vec(mod, 'extractTrailingTurnContext', 'Trailing-turn detection after settled replies; latest prompt tracking; activity batches.', [
      caseOf('late-tool-after-reply', { entries }, m.extractTrailingTurnContext(entries)),
      caseOf('empty', { entries: [] }, m.extractTrailingTurnContext([])),
      caseOf('slash-command-not-prompt', { entries: [entry('c1', 'command', '/gateway', { createdAtMs: T0 - 50_000 }), toolRunningEntry] },
        m.extractTrailingTurnContext([entry('c1', 'command', '/gateway', { createdAtMs: T0 - 50_000 }), toolRunningEntry])),
    ]);
  }

  vec(mod, 'dedupeActivityEntries', 'Running-placeholder upgrades and duplicate suppression beside concrete steps; reasoning untouched.', [
    caseOf('upgrade-placeholder', { activities: [toolRunningEntry, toolDetailEntry] }, m.dedupeActivityEntries([toolRunningEntry, toolDetailEntry])),
    caseOf('suppress-trailing-generic', { activities: [toolDetailEntry, toolRunningEntry] }, m.dedupeActivityEntries([toolDetailEntry, toolRunningEntry])),
    caseOf('done-not-suppressed', { activities: [toolDoneEntry, toolRunningEntry] }, m.dedupeActivityEntries([toolDoneEntry, toolRunningEntry])),
    caseOf('reasoning-barrier', { activities: [toolRunningEntry, reasoningEntry, toolDetailEntry] }, m.dedupeActivityEntries([toolRunningEntry, reasoningEntry, toolDetailEntry])),
  ]);

  {
    const designEntries = [
      entry('du1', 'user', 'Draft the phone widget layout', { channelId: 'design', createdAtMs: T0 - 40_000 }),
      entry('da1', 'activity', 'Running terminal', { channelId: 'design', activityKind: 'tool', activityStatus: 'running', createdAtMs: T0 - 20_000 }),
    ];
    const opsEntries = [
      entry('ou1', 'user', 'Run the backup', { channelId: 'ops', createdAtMs: T0 - 9 * MIN }),
      entry('oa1', 'activity', 'terminal: backup.sh', { channelId: 'ops', activityKind: 'tool', activityStatus: 'done', createdAtMs: T0 - 8 * MIN }),
    ];
    const sessions = [{
      sessionId: 'hermes-design', channelId: 'design', name: 'Design', persona: 'Hermes', topic: 'Phone widget',
      status: 'thinking', turnCount: 2, lastMessage: 'Draft the phone widget layout', updatedAtMs: T0 - 15_000,
    }];
    const channels = [{ id: 'design', name: 'Design', sessionId: 'hermes-design' }, { id: 'ops', name: 'Ops' }];
    const params = {
      tasks: [taskRunningSteps],
      entries: [...designEntries, ...opsEntries],
      sessions,
      thinkingByChannel: { design: true },
      channels,
      confirmedTaskIds: ['trace-turn-ops'],
    };
    vec(mod, 'mergeTasksWithSessionActivity', 'Synthetic live-turn tasks for busy threads without server tasks; completed trace-turn tasks for tool-only threads; confirmedTaskIds input is an array (Set in TS).', [
      caseOf('synthetic-live-and-trace', params, m.mergeTasksWithSessionActivity({ ...params, confirmedTaskIds: new Set(params.confirmedTaskIds) })),
      caseOf('no-synthetic-when-running-task-exists', { tasks: [taskRunningSteps, { ...taskRunningZero, channelId: 'design' }], entries: designEntries, thinkingByChannel: { design: true } },
        m.mergeTasksWithSessionActivity({ tasks: [taskRunningSteps, { ...taskRunningZero, channelId: 'design' }], entries: designEntries, thinkingByChannel: { design: true } })),
      caseOf('empty', { tasks: [], entries: [] }, m.mergeTasksWithSessionActivity({ tasks: [], entries: [] })),
    ]);
  }
}

// ---------------------------------------------------------------------------
// sidebarCharacterState
// ---------------------------------------------------------------------------
if (loaded.sidebarCharacterState) {
  const m = loaded.sidebarCharacterState;
  const mod = 'sidebarCharacterState';

  const approvalPending = {
    id: 'ap-1', channelId: 'general', title: 'Approve deploy', actionType: 'deploy',
    destination: 'terminal', summary: 'Deploy staging', draft: 'deploy.sh', status: 'pending',
  };
  const approvalResolved = { ...approvalPending, id: 'ap-2', status: 'approved' };
  const baseInput = {
    tasks: [], approvals: [], activeChannel: 'general', baseMood: 'happy',
    isSpeaking: false, isListening: false, lastSpokenLine: '',
  };
  const scenarios = [
    ['speaking-with-bands', {
      ...baseInput, isSpeaking: true, outputLevel: 0.72, frequencyBands: bands12, tick: 7,
      lastSpokenLine: 'Hello from Hermes', gatewayStatus: gatewayStatusConnected,
      tasks: [taskRunningSteps], approvals: [approvalPending],
    }],
    ['speaking-quiet', { ...baseInput, isSpeaking: true, outputLevel: 0.1, tick: 2, lastSpokenLine: 'hm' }],
    ['listening', { ...baseInput, isListening: true, inputLevel: 0.5, frequencyBands: bands12, gatewayStatus: gatewayStatusConnected }],
    ['working-multi-channel', {
      ...baseInput, tasks: [taskRunningSteps, taskRunningRatio, taskRunningZero],
      thinkingByChannel: { ops: true },
      hermesSessions: [{ sessionId: 'hermes-lab', channelId: 'lab', name: 'Lab', persona: 'Hermes', topic: '', status: 'running', turnCount: 1, updatedAtMs: T0 }],
      gatewayStatus: gatewayStatusConnected,
    }],
    ['working-single', { ...baseInput, tasks: [taskRunningSteps] }],
    ['attention-approvals', { ...baseInput, approvals: [approvalPending, { ...approvalPending, id: 'ap-3' }, approvalResolved] }],
    ['offline', { ...baseInput, gatewayStatus: gatewayStatusOffline }],
    ['offline-thinking-base', { ...baseInput, baseMood: 'thinking', gatewayStatus: gatewayStatusOffline }],
    ['idle-online', { ...baseInput, gatewayStatus: gatewayStatusConnected }],
    ['idle-new-session', { ...baseInput, homeSessionDraft: true }],
    ['idle-channel', { ...baseInput, activeChannel: 'design' }],
    ['idle-moods', { ...baseInput, baseMood: 'alert' }],
  ];
  vec(mod, 'deriveSidebarCharacterState', 'Synchronized sidebar character: mode/mood/eyes/viseme/energy/tone + task & approval telemetry across all six modes.',
    scenarios.map(([name, input]) => caseOf(name, { input }, m.deriveSidebarCharacterState(input))));

  const cardRect = { left: 0, top: 0, width: 280, height: 66 };
  const lookPoints = [
    ['portrait-center', 44, 33], ['far-right', 279, 33], ['far-left', -200, 33],
    ['above', 44, -300], ['below', 44, 400], ['default-offset', 100, 50],
  ];
  vec(mod, 'computeSidebarPointerLookTarget', 'Normalized gaze vector with clamping to [-1,1] and portrait-center offset.', [
    ...lookPoints.map(([name, x, y]) => caseOf(name, { cardRect, clientX: x, clientY: y }, m.computeSidebarPointerLookTarget(cardRect, x, y))),
    caseOf('custom-offset', { cardRect, clientX: 100, clientY: 50, portraitCenterOffsetPx: 20 }, m.computeSidebarPointerLookTarget(cardRect, 100, 50, 20)),
    caseOf('tiny-card', { cardRect: { left: 10, top: 10, width: 40, height: 20 }, clientX: 30, clientY: 20 },
      m.computeSidebarPointerLookTarget({ left: 10, top: 10, width: 40, height: 20 }, 30, 20)),
  ]);
}

// ---------------------------------------------------------------------------
// threadTrace
// ---------------------------------------------------------------------------
if (loaded.threadTrace) {
  const m = loaded.threadTrace;
  const mod = 'threadTrace';

  const classifyEntries = [
    ['reasoning-kind', reasoningEntry],
    ['reasoning-prefix-text', entry('x1', 'activity', 'Reasoning: comparing shader paths', { activityKind: 'status' })],
    ['commentary', commentaryEntry],
    ['subagent', subagentEntry],
    ['tool-kind', toolDoneEntry],
    ['link-heuristic', linkEntry],
    ['tool-heuristic', entry('x2', 'activity', 'Read the Cargo manifest', { activityKind: 'status' })],
    ['spawn-heuristic', entry('x3', 'activity', 'New agent session attached', { activityKind: 'status' })],
    ['status-suffix-split', entry('x4', 'activity', 'terminal completed', { activityKind: 'status' })],
    ['colon-label-split', entry('x5', 'activity', 'terminal: cargo check', { activityKind: 'status' })],
    ['emdash-label-split', entry('x6', 'activity', 'Gateway probe — all green', { activityKind: 'status' })],
    ['plain-status', entry('x7', 'activity', 'Idle', { activityKind: 'status' })],
  ];
  vec(mod, 'classifyTraceStep', 'Trace icon classification + label/detail splitting for every branch.',
    classifyEntries.map(([name, e]) => caseOf(name, { entry: e }, m.classifyTraceStep(e))));

  vec(mod, 'isTerminalDiagnosticActivity', 'Turn-boundary diagnostics: failed/cancelled/admission_conflict/error status; tool/reasoning kinds excluded.', [
    caseOf('failed-outcome', { entry: diagnosticEntry }, m.isTerminalDiagnosticActivity(diagnosticEntry)),
    caseOf('cancelled', { entry: { ...diagnosticEntry, terminalOutcome: 'cancelled', errorCode: undefined, activityStatus: undefined } },
      m.isTerminalDiagnosticActivity({ ...diagnosticEntry, terminalOutcome: 'cancelled', errorCode: undefined, activityStatus: undefined })),
    caseOf('admission-conflict', { entry: { ...diagnosticEntry, terminalOutcome: 'admission_conflict', errorCode: undefined, activityStatus: undefined } },
      m.isTerminalDiagnosticActivity({ ...diagnosticEntry, terminalOutcome: 'admission_conflict', errorCode: undefined, activityStatus: undefined })),
    caseOf('tool-kind-excluded', { entry: { ...diagnosticEntry, activityKind: 'tool' } }, m.isTerminalDiagnosticActivity({ ...diagnosticEntry, activityKind: 'tool' })),
    caseOf('non-activity', { entry: userEntry }, m.isTerminalDiagnosticActivity(userEntry)),
    caseOf('clean-status', { entry: linkEntry }, m.isTerminalDiagnosticActivity(linkEntry)),
  ]);

  {
    const steps = [toolDoneEntry, { ...toolDoneEntry, id: 'a5b' }, { ...toolDoneEntry, id: 'a5c' }, reasoningEntry, toolDetailEntry, commentaryEntry, { ...commentaryEntry, id: 'a2b' }].map(m.classifyTraceStep);
    vec(mod, 'coalesceConsecutiveSteps', 'Consecutive identical completed steps merge with repeatCount + latest timestamp; reasoning/commentary never merge.', [
      caseOf('mixed-run', { steps }, m.coalesceConsecutiveSteps(steps)),
      caseOf('empty', { steps: [] }, m.coalesceConsecutiveSteps([])),
    ]);
  }

  {
    const turn1 = [userEntry, reasoningEntry, toolRunningEntry, toolDoneEntry, replyEntry, lateToolEntry];
    const user2 = entry('u2', 'user', 'Now add the tests', { createdAtMs: T0 - 5_000 });
    const turn2 = [user2, { ...reasoningEntry, id: 'b1', channelId: 'general' }, { ...toolRunningEntry, id: 'b2' }];
    vec(mod, 'groupThreadTimeline', 'Turn segmentation, activity/reply interleaving, late tools below replies, live thinking attachment, diagnostic boundaries.', [
      caseOf('two-turns-thinking', { entries: [...turn1, ...turn2], isThinking: true }, m.groupThreadTimeline([...turn1, ...turn2], true)),
      caseOf('two-turns-settled', { entries: [...turn1, ...turn2], isThinking: false }, m.groupThreadTimeline([...turn1, ...turn2], false)),
      caseOf('reply-settled-no-placeholder', { entries: turn1, isThinking: true }, m.groupThreadTimeline(turn1, true)),
      caseOf('empty-thinking-placeholder', { entries: [], isThinking: true }, m.groupThreadTimeline([], true)),
      caseOf('empty-thinking-no-placeholder', { entries: [], isThinking: true, includeLivePlaceholder: false }, m.groupThreadTimeline([], true, false)),
      caseOf('diagnostic-boundary', { entries: [userEntry, toolDoneEntry, diagnosticEntry, { ...replyEntry, id: 'r2' }], isThinking: false },
        m.groupThreadTimeline([userEntry, toolDoneEntry, diagnosticEntry, { ...replyEntry, id: 'r2' }], false)),
      caseOf('dedupe-inside-trace', { entries: [userEntry, toolRunningEntry, toolDetailEntry, toolDoneEntry], isThinking: false },
        m.groupThreadTimeline([userEntry, toolRunningEntry, toolDetailEntry, toolDoneEntry], false)),
    ]);
  }
}

// ---------------------------------------------------------------------------
// Write files + manifest
// ---------------------------------------------------------------------------
mkdirSync(VECTORS_DIR, { recursive: true });
for (const name of MODULES) {
  rmSync(join(VECTORS_DIR, name), { recursive: true, force: true });
}

const byModule = {};
let totalCases = 0;
for (const v of vectors) {
  const dir = join(VECTORS_DIR, v.module);
  mkdirSync(dir, { recursive: true });
  const file = `${v.fn}.json`;
  const payload = {
    module: v.module,
    function: v.fn,
    description: v.description,
    cases: v.cases,
  };
  writeFileSync(join(dir, file), JSON.stringify(canonical(payload), null, 2) + '\n', 'utf8');
  byModule[v.module] ??= { files: [], caseCount: 0 };
  byModule[v.module].files.push(file);
  byModule[v.module].caseCount += v.cases.length;
  totalCases += v.cases.length;
}

const modulesManifest = MODULES.map((name) => {
  const skip = skipped.find((s) => s.module === name);
  if (skip) return skip;
  const info = byModule[name] ?? { files: [], caseCount: 0 };
  return {
    module: name,
    source: `src/utils/${name}.ts`,
    status: 'ok',
    fileCount: info.files.length,
    caseCount: info.caseCount,
    files: info.files.sort(),
  };
});

const manifest = {
  schemaVersion: 1,
  generator: 'voxcell-desktop/tools/gen-vectors.mjs',
  command: 'npx tsx voxcell-desktop/tools/gen-vectors.mjs',
  spec: 'docs/RUNE_DESKTOP_SPEC.md §9 (ticket P0-04)',
  determinism: {
    tz: 'UTC',
    fixedNowMs: FIXED_NOW_MS,
    fixedNowIso: new Date(FIXED_NOW_MS).toISOString(),
    prngSeed: `0x${PRNG_SEED.toString(16)}`,
    shims: [
      'Date.now() pinned to fixedNowMs',
      'Math.random replaced with seeded mulberry32 PRNG',
      'crypto.randomUUID replaced with counter-based UUIDv4-shaped generator (affects createIdempotencyKey suffix only)',
    ],
    canonicalJson: 'Object keys sorted recursively; array order preserved; -0 normalized to 0; 2-space indent; trailing newline.',
    notes: [
      'formatStartClock uses local-time getters; vectors are only valid with TZ=UTC (set by the generator and by CI).',
      'fetchWithConcurrentRetry vectors use injected makeRequest/sleep mocks; no network access.',
      'mergeTasksWithSessionActivity.confirmedTaskIds is serialized as an array; the TS API takes a ReadonlySet.',
      'All browser-gated functions (localStorage/DOM helpers in uiCustomization/backgrounds persistence) are never invoked; only pure exports are exercised.',
    ],
  },
  modules: modulesManifest,
  totals: {
    modulesOk: modulesManifest.filter((x) => x.status === 'ok').length,
    modulesSkipped: skipped.length,
    vectorFiles: vectors.length,
    cases: totalCases,
  },
};
writeFileSync(join(VECTORS_DIR, 'manifest.json'), JSON.stringify(canonical(manifest), null, 2) + '\n', 'utf8');

// ---------------------------------------------------------------------------
// Summary
// ---------------------------------------------------------------------------
console.log(`Vectors written to ${VECTORS_DIR}`);
for (const info of modulesManifest) {
  if (info.status === 'ok') {
    console.log(`  [ok]   ${info.module}: ${info.fileCount} files, ${info.caseCount} cases`);
  } else {
    console.log(`  [skip] ${info.module}: ${info.reason}`);
  }
}
console.log(`Total: ${vectors.length} vector files, ${totalCases} cases, ${skipped.length} skipped modules.`);
if (vectors.length === 0) {
  console.error('No vectors generated — all modules failed to import.');
  process.exit(1);
}
