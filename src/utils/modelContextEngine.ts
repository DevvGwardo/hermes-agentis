import type { TerminalEntry } from '../types/hermes';

export type ModelMetadataSource =
  | 'gateway'
  | 'openrouter-live'
  | 'hermes-config'
  | 'catalog';

export interface ModelPricingInfo {
  /** USD per 1M input tokens */
  inputPerMillion: number;
  /** USD per 1M output tokens */
  outputPerMillion: number;
}

export interface ModelInfoSpec {
  id: string;
  displayName: string;
  provider: string;
  contextWindow: number;
  maxOutputTokens: number;
  modality: string;
  pricing?: ModelPricingInfo;
  metadataSource: ModelMetadataSource;
  fetchedAtMs?: number;
}

export interface ProviderTokenUsage {
  promptTokens: number;
  completionTokens: number;
  totalTokens: number;
  cachedTokens?: number;
  updatedAtMs: number;
  model?: string;
}

export interface ModelInfoResponse {
  activeModel: ModelInfoSpec;
  availableModels: ModelInfoSpec[];
  gatewayConnected: boolean;
  liveCatalogFetched: boolean;
  compactionThresholdRatio: number;
  lastReportedUsage?: ProviderTokenUsage | null;
  fetchedAtMs: number;
}

export type ContextWindowHealth = 'nominal' | 'elevated' | 'warning' | 'critical';

export interface ContextBreakdownSegment {
  id: 'system' | 'user' | 'assistant' | 'traces';
  label: string;
  tokens: number;
  percentOfWindow: number;
  color: string;
}

export interface ContextWindowUsageReport {
  model: ModelInfoSpec;
  usedTokens: number;
  contextWindow: number;
  remainingTokens: number;
  usedPct: number; // 0..100
  remainingPct: number; // 0..100
  compactionThresholdRatio: number; // e.g. 0.80
  compactionThresholdTokens: number;
  tokensUntilCompaction: number;
  status: ContextWindowHealth;
  statusLabel: string;
  measurementSource: 'provider-usage' | 'tokenized-estimate';
  turnCount: number;
  avgTokensPerTurn: number;
  estimatedTurnsRemaining: number;
  estimatedSessionCostUsd: number | null;
  segments: ContextBreakdownSegment[];
  lastReportedUsage?: ProviderTokenUsage | null;
}

export type ContextInputEntry = Pick<TerminalEntry, 'id' | 'role' | 'text'> & {
  reasoningDetail?: string;
  timestamp?: string;
};

/**
 * Verified fallback model specifications covering Hermes, Anthropic, OpenAI,
 * Google Gemini, DeepSeek, Meta Llama, Qwen, xAI, and Mistral families.
 * Used when neither the live Hermes Gateway nor the public OpenRouter catalog
 * supplies an explicit context_length for the active model.
 */
export const VERIFIED_MODEL_SPECS: readonly Omit<ModelInfoSpec, 'metadataSource'>[] = [
  {
    id: 'nousresearch/hermes-4-70b',
    displayName: 'Hermes 4 70B',
    provider: 'Nous Research',
    contextWindow: 131_072,
    maxOutputTokens: 16_384,
    modality: 'text->text',
    pricing: { inputPerMillion: 0.4, outputPerMillion: 0.4 },
  },
  {
    id: 'nousresearch/hermes-4-405b',
    displayName: 'Hermes 4 405B',
    provider: 'Nous Research',
    contextWindow: 131_072,
    maxOutputTokens: 16_384,
    modality: 'text->text',
    pricing: { inputPerMillion: 1.0, outputPerMillion: 3.0 },
  },
  {
    id: 'nousresearch/hermes-3-llama-3.1-405b',
    displayName: 'Hermes 3 405B',
    provider: 'Nous Research',
    contextWindow: 131_072,
    maxOutputTokens: 8_192,
    modality: 'text->text',
    pricing: { inputPerMillion: 0.8, outputPerMillion: 0.8 },
  },
  {
    id: 'anthropic/claude-sonnet-4',
    displayName: 'Claude Sonnet 4',
    provider: 'Anthropic',
    contextWindow: 200_000,
    maxOutputTokens: 64_000,
    modality: 'text+image->text',
    pricing: { inputPerMillion: 3.0, outputPerMillion: 15.0 },
  },
  {
    id: 'anthropic/claude-opus-4',
    displayName: 'Claude Opus 4',
    provider: 'Anthropic',
    contextWindow: 200_000,
    maxOutputTokens: 32_000,
    modality: 'text+image->text',
    pricing: { inputPerMillion: 15.0, outputPerMillion: 75.0 },
  },
  {
    id: 'anthropic/claude-3.7-sonnet',
    displayName: 'Claude 3.7 Sonnet',
    provider: 'Anthropic',
    contextWindow: 200_000,
    maxOutputTokens: 64_000,
    modality: 'text+image->text',
    pricing: { inputPerMillion: 3.0, outputPerMillion: 15.0 },
  },
  {
    id: 'anthropic/claude-3.5-sonnet',
    displayName: 'Claude 3.5 Sonnet',
    provider: 'Anthropic',
    contextWindow: 200_000,
    maxOutputTokens: 8_192,
    modality: 'text+image->text',
    pricing: { inputPerMillion: 3.0, outputPerMillion: 15.0 },
  },
  {
    id: 'openai/gpt-4.1',
    displayName: 'GPT-4.1',
    provider: 'OpenAI',
    contextWindow: 1_047_576,
    maxOutputTokens: 32_768,
    modality: 'text+image->text',
    pricing: { inputPerMillion: 2.0, outputPerMillion: 8.0 },
  },
  {
    id: 'openai/gpt-4o',
    displayName: 'GPT-4o',
    provider: 'OpenAI',
    contextWindow: 128_000,
    maxOutputTokens: 16_384,
    modality: 'text+image->text',
    pricing: { inputPerMillion: 2.5, outputPerMillion: 10.0 },
  },
  {
    id: 'openai/o3',
    displayName: 'OpenAI o3',
    provider: 'OpenAI',
    contextWindow: 200_000,
    maxOutputTokens: 100_000,
    modality: 'text+image->text',
    pricing: { inputPerMillion: 2.0, outputPerMillion: 8.0 },
  },
  {
    id: 'openai/o4-mini',
    displayName: 'OpenAI o4-mini',
    provider: 'OpenAI',
    contextWindow: 200_000,
    maxOutputTokens: 100_000,
    modality: 'text+image->text',
    pricing: { inputPerMillion: 1.1, outputPerMillion: 4.4 },
  },
  {
    id: 'openai/gpt-5',
    displayName: 'GPT-5',
    provider: 'OpenAI',
    contextWindow: 400_000,
    maxOutputTokens: 128_000,
    modality: 'text+image->text',
    pricing: { inputPerMillion: 1.25, outputPerMillion: 10.0 },
  },
  {
    id: 'google/gemini-2.5-pro',
    displayName: 'Gemini 2.5 Pro',
    provider: 'Google',
    contextWindow: 1_048_576,
    maxOutputTokens: 65_536,
    modality: 'text+image+audio->text',
    pricing: { inputPerMillion: 1.25, outputPerMillion: 10.0 },
  },
  {
    id: 'google/gemini-2.5-flash',
    displayName: 'Gemini 2.5 Flash',
    provider: 'Google',
    contextWindow: 1_048_576,
    maxOutputTokens: 65_536,
    modality: 'text+image+audio->text',
    pricing: { inputPerMillion: 0.15, outputPerMillion: 0.6 },
  },
  {
    id: 'deepseek/deepseek-r1',
    displayName: 'DeepSeek R1',
    provider: 'DeepSeek',
    contextWindow: 163_840,
    maxOutputTokens: 32_768,
    modality: 'text->text',
    pricing: { inputPerMillion: 0.55, outputPerMillion: 2.19 },
  },
  {
    id: 'deepseek/deepseek-chat-v3',
    displayName: 'DeepSeek V3',
    provider: 'DeepSeek',
    contextWindow: 163_840,
    maxOutputTokens: 16_384,
    modality: 'text->text',
    pricing: { inputPerMillion: 0.27, outputPerMillion: 1.1 },
  },
  {
    id: 'qwen/qwen3-235b-a22b',
    displayName: 'Qwen3 235B',
    provider: 'Qwen',
    contextWindow: 131_072,
    maxOutputTokens: 32_768,
    modality: 'text->text',
    pricing: { inputPerMillion: 0.2, outputPerMillion: 0.6 },
  },
  {
    id: 'meta-llama/llama-4-maverick',
    displayName: 'Llama 4 Maverick',
    provider: 'Meta',
    contextWindow: 1_048_576,
    maxOutputTokens: 32_768,
    modality: 'text+image->text',
    pricing: { inputPerMillion: 0.2, outputPerMillion: 0.6 },
  },
  {
    id: 'x-ai/grok-3',
    displayName: 'Grok 3',
    provider: 'xAI',
    contextWindow: 131_072,
    maxOutputTokens: 32_768,
    modality: 'text->text',
    pricing: { inputPerMillion: 3.0, outputPerMillion: 15.0 },
  },
  {
    id: 'hermes-agent',
    displayName: 'Hermes Agent',
    provider: 'Hermes Gateway',
    contextWindow: 131_072,
    maxOutputTokens: 16_384,
    modality: 'text+tools->text',
  },
];

function clamp(v: number, min: number, max: number): number {
  return v < min ? min : v > max ? max : v;
}

function normalizeModelKey(id: string): string {
  return id
    .trim()
    .toLowerCase()
    .replace(/:(free|extended|thinking|beta)$/i, '')
    .replace(/\s+/g, '-');
}

function stripProviderPrefix(id: string): string {
  const idx = id.indexOf('/');
  return idx >= 0 ? id.slice(idx + 1) : id;
}

function inferProviderFromId(id: string): string {
  const lower = id.toLowerCase();
  if (lower.startsWith('anthropic/') || lower.includes('claude')) return 'Anthropic';
  if (lower.startsWith('openai/') || /^(gpt-|o[134]-|o[134]$)/.test(stripProviderPrefix(lower))) return 'OpenAI';
  if (lower.startsWith('google/') || lower.includes('gemini')) return 'Google';
  if (lower.startsWith('nousresearch/') || lower.includes('hermes')) return 'Nous Research';
  if (lower.startsWith('deepseek/') || lower.includes('deepseek')) return 'DeepSeek';
  if (lower.startsWith('qwen/') || lower.includes('qwen')) return 'Qwen';
  if (lower.startsWith('meta-llama/') || lower.includes('llama')) return 'Meta';
  if (lower.startsWith('x-ai/') || lower.includes('grok')) return 'xAI';
  if (lower.startsWith('mistralai/') || lower.includes('mistral')) return 'Mistral';
  const slash = id.indexOf('/');
  if (slash > 0) {
    const raw = id.slice(0, slash);
    return raw.charAt(0).toUpperCase() + raw.slice(1);
  }
  return 'Hermes Gateway';
}

function prettifyModelName(id: string): string {
  const base = stripProviderPrefix(id).trim();
  if (!base || base === 'hermes-agent') return 'Hermes Agent';
  return base
    .split(/[-_]+/)
    .filter(Boolean)
    .map((part) => {
      if (/^(gpt|o\d|r\d|v\d|xl|pro|mini|nano)$/i.test(part)) return part.toUpperCase();
      if (/^\d/.test(part)) return part;
      return part.charAt(0).toUpperCase() + part.slice(1);
    })
    .join(' ');
}

function parseFinitePositive(v: unknown): number | null {
  if (typeof v === 'number' && Number.isFinite(v) && v > 0) return v;
  if (typeof v === 'string' && v.trim()) {
    const n = Number(v);
    if (Number.isFinite(n) && n > 0) return n;
  }
  return null;
}

/**
 * Parses model metadata records from OpenRouter's /api/v1/models or any
 * OpenAI-compatible /v1/models / /api/model/options payload.
 */
export function parseExternalModelCatalog(
  raw: unknown,
  source: ModelMetadataSource,
  fetchedAtMs = Date.now()
): ModelInfoSpec[] {
  if (!raw || typeof raw !== 'object') return [];
  const obj = raw as Record<string, unknown>;
  const list: unknown[] = [];

  if (Array.isArray(obj.data)) list.push(...obj.data);
  if (Array.isArray(obj.models)) list.push(...obj.models);
  if (Array.isArray(obj.curated_models)) list.push(...obj.curated_models);
  if (Array.isArray(obj.providers)) {
    for (const p of obj.providers) {
      if (p && typeof p === 'object' && Array.isArray((p as Record<string, unknown>).models)) {
        list.push(...((p as Record<string, unknown>).models as unknown[]));
      }
    }
  }

  const byId = new Map<string, ModelInfoSpec>();
  for (const item of list) {
    if (!item || typeof item !== 'object') continue;
    const rec = item as Record<string, unknown>;
    const id =
      (typeof rec.id === 'string' && rec.id.trim()) ||
      (typeof rec.model === 'string' && rec.model.trim()) ||
      (typeof rec.slug === 'string' && rec.slug.trim()) ||
      '';
    if (!id) continue;

    const topProvider =
      rec.top_provider && typeof rec.top_provider === 'object'
        ? (rec.top_provider as Record<string, unknown>)
        : null;
    const arch =
      rec.architecture && typeof rec.architecture === 'object'
        ? (rec.architecture as Record<string, unknown>)
        : null;
    const pricingObj =
      rec.pricing && typeof rec.pricing === 'object'
        ? (rec.pricing as Record<string, unknown>)
        : null;

    const contextWindow =
      parseFinitePositive(rec.context_length) ??
      parseFinitePositive(rec.context_window) ??
      parseFinitePositive(rec.max_context_tokens) ??
      parseFinitePositive(rec.max_input_tokens) ??
      parseFinitePositive(topProvider?.context_length);

    if (!contextWindow || contextWindow < 1024) continue;

    const maxOutputTokens =
      parseFinitePositive(topProvider?.max_completion_tokens) ??
      parseFinitePositive(rec.max_output_tokens) ??
      parseFinitePositive(rec.max_completion_tokens) ??
      Math.min(32_768, Math.max(4_096, Math.round(contextWindow * 0.125)));

    const promptPricePerToken = parseFinitePositive(pricingObj?.prompt);
    const completionPricePerToken = parseFinitePositive(pricingObj?.completion);
    const pricing: ModelPricingInfo | undefined =
      promptPricePerToken !== null && completionPricePerToken !== null
        ? {
            inputPerMillion: Number((promptPricePerToken * 1_000_000).toFixed(3)),
            outputPerMillion: Number((completionPricePerToken * 1_000_000).toFixed(3)),
          }
        : undefined;

    const rawName = typeof rec.name === 'string' && rec.name.trim() ? rec.name.trim() : '';
    const displayName = rawName.includes(': ')
      ? rawName.split(': ').slice(1).join(': ').trim()
      : rawName || prettifyModelName(id);

    const provider =
      (typeof rec.provider === 'string' && rec.provider.trim()) ||
      (rawName.includes(': ') ? rawName.split(': ')[0].trim() : '') ||
      inferProviderFromId(id);

    const modality =
      (typeof arch?.modality === 'string' && arch.modality.trim()) ||
      (typeof rec.modality === 'string' && rec.modality.trim()) ||
      'text->text';

    byId.set(normalizeModelKey(id), {
      id,
      displayName,
      provider,
      contextWindow: Math.round(contextWindow),
      maxOutputTokens: Math.round(maxOutputTokens),
      modality,
      ...(pricing ? { pricing } : {}),
      metadataSource: source,
      fetchedAtMs,
    });
  }

  return Array.from(byId.values());
}

/**
 * Matches a model identifier (exact, provider-stripped, or prefix-matched)
 * across prioritized metadata sources: Gateway > Live Catalog > Verified Fallback.
 */
export function resolveModelInfoSpec(params: {
  modelId: string;
  providerHint?: string;
  gatewaySpecs?: readonly ModelInfoSpec[];
  liveCatalogSpecs?: readonly ModelInfoSpec[];
  configContextWindow?: number | null;
  fetchedAtMs?: number;
}): ModelInfoSpec {
  const rawId = params.modelId.trim() || 'hermes-agent';
  const norm = normalizeModelKey(rawId);
  const stripped = stripProviderPrefix(norm);
  const fetchedAtMs = params.fetchedAtMs ?? Date.now();

  const findInList = (list: readonly ModelInfoSpec[] | undefined): ModelInfoSpec | null => {
    if (!list || list.length === 0) return null;
    // 1. Exact normalized match
    for (const item of list) {
      if (normalizeModelKey(item.id) === norm) return item;
    }
    // 2. Provider-stripped exact match
    for (const item of list) {
      if (stripProviderPrefix(normalizeModelKey(item.id)) === stripped) return item;
    }
    // 3. Longest prefix / containment match (e.g., claude-sonnet-4-20250514 -> anthropic/claude-sonnet-4)
    let best: ModelInfoSpec | null = null;
    let bestScore = 0;
    for (const item of list) {
      const candidate = stripProviderPrefix(normalizeModelKey(item.id));
      const boundaryMatch =
        (stripped.startsWith(candidate) &&
          (stripped.length === candidate.length || /[-._:]/.test(stripped[candidate.length]))) ||
        (candidate.startsWith(stripped) &&
          (candidate.length === stripped.length || /[-._:]/.test(candidate[stripped.length])));
      if (boundaryMatch) {
        const score = Math.min(stripped.length, candidate.length);
        if (score >= 5 && score > bestScore) {
          bestScore = score;
          best = item;
        }
      }
    }
    return best;
  };

  const fromGateway = findInList(params.gatewaySpecs);
  if (fromGateway) {
    return {
      ...fromGateway,
      id: rawId,
      provider: params.providerHint || fromGateway.provider,
    };
  }

  const fromLive = findInList(params.liveCatalogSpecs);
  if (fromLive) {
    return {
      ...fromLive,
      id: rawId,
      provider: params.providerHint || fromLive.provider,
    };
  }

  const catalogWrapped: ModelInfoSpec[] = VERIFIED_MODEL_SPECS.map((s) => ({
    ...s,
    metadataSource: 'catalog',
    fetchedAtMs,
  }));
  const fromCatalog = findInList(catalogWrapped);
  if (fromCatalog) {
    if (params.configContextWindow && params.configContextWindow >= 1024) {
      return {
        ...fromCatalog,
        id: rawId,
        provider: params.providerHint || fromCatalog.provider,
        contextWindow: Math.round(params.configContextWindow),
        metadataSource: 'hermes-config',
      };
    }
    return {
      ...fromCatalog,
      id: rawId,
      provider: params.providerHint || fromCatalog.provider,
    };
  }

  return {
    id: rawId,
    displayName: prettifyModelName(rawId),
    provider: params.providerHint || inferProviderFromId(rawId),
    contextWindow:
      params.configContextWindow && params.configContextWindow >= 1024
        ? Math.round(params.configContextWindow)
        : 131_072,
    maxOutputTokens: 16_384,
    modality: 'text->text',
    metadataSource:
      params.configContextWindow && params.configContextWindow >= 1024
        ? 'hermes-config'
        : 'catalog',
    fetchedAtMs,
  };
}

/**
 * BPE-calibrated token estimator aligned with cl100k_base / o200k_base behavior:
 * accounts for code/JSON punctuation density, numeric chunks, CJK characters,
 * whitespace, and English subword runs rather than a crude chars/4 division.
 */
export function estimateAccurateTokens(text: string): number {
  if (!text) return 0;
  const trimmed = text.trim();
  if (!trimmed) return 0;

  let cjkChars = 0;
  let codePunct = 0;
  let digits = 0;
  let whitespaceRuns = 0;
  let inSpace = false;

  for (let i = 0; i < trimmed.length; i++) {
    const code = trimmed.charCodeAt(i);
    if (code === 32 || code === 9 || code === 10 || code === 13) {
      if (!inSpace) {
        whitespaceRuns++;
        inSpace = true;
      }
      continue;
    }
    inSpace = false;

    if (
      (code >= 0x4e00 && code <= 0x9fff) ||
      (code >= 0x3040 && code <= 0x30ff) ||
      (code >= 0xac00 && code <= 0xd7af)
    ) {
      cjkChars++;
    } else if (code >= 48 && code <= 57) {
      digits++;
    } else if (
      (code >= 33 && code <= 47) ||
      (code >= 58 && code <= 64) ||
      (code >= 91 && code <= 96) ||
      (code >= 123 && code <= 126)
    ) {
      codePunct++;
    }
  }

  const proseChars = Math.max(0, trimmed.length - cjkChars - codePunct - digits - whitespaceRuns);
  // Empirically calibrated ratios:
  // - English/Latin prose: ~4.05 chars/token
  // - Code & JSON punctuation: ~0.62 tokens/symbol
  // - Digit runs: ~2.6 digits/token
  // - CJK glyphs: ~1.35 tokens/char
  // - Newlines/indentation overhead: ~0.12 tokens/whitespace run
  const rawTokens =
    proseChars / 4.05 +
    codePunct * 0.62 +
    digits / 2.6 +
    cjkChars * 1.35 +
    whitespaceRuns * 0.12;

  return Math.max(1, Math.ceil(rawTokens));
}

/**
 * Extracts OpenAI / Anthropic / Hermes Gateway token usage payloads from a JSON response or SSE chunk.
 */
export function extractProviderTokenUsage(
  raw: unknown,
  model?: string
): ProviderTokenUsage | null {
  if (!raw || typeof raw !== 'object') return null;
  const obj = raw as Record<string, unknown>;
  const usage =
    obj.usage && typeof obj.usage === 'object'
      ? (obj.usage as Record<string, unknown>)
      : obj;

  const promptTokens =
    parseFinitePositive(usage.prompt_tokens) ??
    parseFinitePositive(usage.input_tokens);
  const completionTokens =
    parseFinitePositive(usage.completion_tokens) ??
    parseFinitePositive(usage.output_tokens) ??
    0;
  const totalTokens =
    parseFinitePositive(usage.total_tokens) ??
    (promptTokens !== null ? promptTokens + completionTokens : null);

  if (promptTokens === null && totalTokens === null) return null;

  const resolvedPrompt = Math.round(promptTokens ?? Math.max(0, (totalTokens ?? 0) - completionTokens));
  const resolvedCompletion = Math.round(completionTokens);
  const resolvedTotal = Math.round(totalTokens ?? resolvedPrompt + resolvedCompletion);

  const promptDetails =
    usage.prompt_tokens_details && typeof usage.prompt_tokens_details === 'object'
      ? (usage.prompt_tokens_details as Record<string, unknown>)
      : null;
  const cachedTokens =
    parseFinitePositive(promptDetails?.cached_tokens) ??
    parseFinitePositive(usage.cache_read_input_tokens) ??
    undefined;

  return {
    promptTokens: resolvedPrompt,
    completionTokens: resolvedCompletion,
    totalTokens: resolvedTotal,
    ...(cachedTokens !== undefined ? { cachedTokens: Math.round(cachedTokens) } : {}),
    updatedAtMs: Date.now(),
    ...(model ? { model } : {}),
  };
}

/**
 * Formats token counts compactly with tabular readability (e.g. 128k, 1.05M, 842).
 */
export function formatCompactTokens(tokens: number): string {
  const n = Math.max(0, Math.round(tokens));
  if (n >= 1_000_000) {
    const m = n / 1_000_000;
    const formatted = m >= 10 ? m.toFixed(0) : m.toFixed(2).replace(/\.?0+$/, '');
    return `${formatted}M`;
  }
  if (n >= 10_000) {
    return `${Math.min(999, Math.round(n / 1_000))}k`;
  }
  if (n >= 1_000) {
    return `${(n / 1_000).toFixed(1)}k`;
  }
  return String(n);
}

/**
 * Computes an accurate, segmented context window usage report for the active session
 * against the resolved model's true context window.
 */
export function computeContextWindowUsage(params: {
  model: ModelInfoSpec;
  entries: readonly ContextInputEntry[];
  skillsCount?: number;
  toolsetsCount?: number;
  compactionThresholdRatio?: number;
  lastReportedUsage?: ProviderTokenUsage | null;
}): ContextWindowUsageReport {
  const {
    model,
    entries,
    skillsCount = 0,
    toolsetsCount = 0,
    compactionThresholdRatio = 0.8,
    lastReportedUsage = null,
  } = params;

  const contextWindow = Math.max(4_096, Math.round(model.contextWindow));
  const thresholdRatio = clamp(compactionThresholdRatio, 0.5, 0.95);
  const compactionThresholdTokens = Math.round(contextWindow * thresholdRatio);

  let userTokens = 0;
  let assistantTokens = 0;
  let traceTokens = 0;
  let userTurnCount = 0;
  let conversationalCount = 0;

  for (const entry of entries) {
    if (!entry.text || !entry.text.trim()) continue;
    if (entry.id.startsWith('init-')) continue;

    if (entry.role === 'user' || entry.role === 'command') {
      userTokens += estimateAccurateTokens(entry.text) + 4;
      userTurnCount++;
      conversationalCount++;
    } else if (entry.role === 'hermes') {
      assistantTokens += estimateAccurateTokens(entry.text) + 4;
      conversationalCount++;
    } else if (entry.role === 'activity') {
      const detail = entry.reasoningDetail
        ? `${entry.text}\n${entry.reasoningDetail}`
        : entry.text;
      traceTokens += estimateAccurateTokens(detail) + 6;
    }
  }

  // Hermes system prompt + loaded skill/toolset schemas occupy baseline context when session is active
  const baseSystemPrompt = conversationalCount > 0 || skillsCount > 0 || toolsetsCount > 0 ? 540 : 320;
  const skillSchemaTokens = Math.min(4_500, skillsCount * 95);
  const toolsetSchemaTokens = Math.min(6_000, toolsetsCount * 220);
  let systemTokens = baseSystemPrompt + skillSchemaTokens + toolsetSchemaTokens;

  const estimatedTotal = systemTokens + userTokens + assistantTokens + traceTokens;

  let measurementSource: 'provider-usage' | 'tokenized-estimate' = 'tokenized-estimate';
  let usedTokens = estimatedTotal;

  // Prefer live provider-reported token usage when available and current
  if (lastReportedUsage && lastReportedUsage.totalTokens > 0) {
    measurementSource = 'provider-usage';
    usedTokens = Math.max(lastReportedUsage.totalTokens, lastReportedUsage.promptTokens);
    // Reconcile the residual difference into system/tool schemas so segments sum accurately to usedTokens
    const nonSystemSum = userTokens + assistantTokens + traceTokens;
    if (usedTokens >= nonSystemSum) {
      systemTokens = usedTokens - nonSystemSum;
    } else if (nonSystemSum > 0) {
      const scale = usedTokens / (nonSystemSum + systemTokens);
      systemTokens = Math.round(systemTokens * scale);
      userTokens = Math.round(userTokens * scale);
      assistantTokens = Math.round(assistantTokens * scale);
      traceTokens = Math.max(0, usedTokens - systemTokens - userTokens - assistantTokens);
    }
  }

  const remainingTokens = Math.max(0, contextWindow - usedTokens);
  const usedPct = Number(clamp((usedTokens / contextWindow) * 100, 0, 100).toFixed(1));
  const remainingPct = Number(Math.max(0, 100 - usedPct).toFixed(1));
  const tokensUntilCompaction = Math.max(0, compactionThresholdTokens - usedTokens);

  const turnCount =
    userTurnCount > 0 ? userTurnCount : conversationalCount > 0 ? Math.ceil(conversationalCount / 2) : 0;
  const conversationalTokens = userTokens + assistantTokens + traceTokens;
  const avgTokensPerTurn =
    turnCount > 0
      ? clamp(Math.round(conversationalTokens / turnCount), 160, 8_000)
      : 680;
  const estimatedTurnsRemaining =
    remainingTokens <= 0 ? 0 : Math.floor(tokensUntilCompaction / Math.max(120, avgTokensPerTurn));

  let status: ContextWindowHealth = 'nominal';
  let statusLabel = 'Healthy';
  if (usedPct >= 92) {
    status = 'critical';
    statusLabel = 'Critical';
  } else if (usedPct >= thresholdRatio * 100) {
    status = 'warning';
    statusLabel = 'Near limit';
  } else if (usedPct >= 60) {
    status = 'elevated';
    statusLabel = 'Filling';
  }

  const estimatedSessionCostUsd = model.pricing
    ? Number(
        (
          ((systemTokens + userTokens + traceTokens) / 1_000_000) * model.pricing.inputPerMillion +
          (assistantTokens / 1_000_000) * model.pricing.outputPerMillion
        ).toFixed(5)
      )
    : null;

  const pctOf = (t: number) => Number(clamp((t / contextWindow) * 100, 0, 100).toFixed(2));

  const segments: ContextBreakdownSegment[] = [
    {
      id: 'system',
      label: 'System & tool schemas',
      tokens: systemTokens,
      percentOfWindow: pctOf(systemTokens),
      color: '#64748B',
    },
    {
      id: 'user',
      label: 'User prompts',
      tokens: userTokens,
      percentOfWindow: pctOf(userTokens),
      color: 'var(--ui-accent-bright, #6EE7B7)',
    },
    {
      id: 'assistant',
      label: 'Assistant replies',
      tokens: assistantTokens,
      percentOfWindow: pctOf(assistantTokens),
      color: '#38BDF8',
    },
    {
      id: 'traces',
      label: 'Tool & reasoning traces',
      tokens: traceTokens,
      percentOfWindow: pctOf(traceTokens),
      color: '#F59E0B',
    },
  ];

  return {
    model,
    usedTokens,
    contextWindow,
    remainingTokens,
    usedPct,
    remainingPct,
    compactionThresholdRatio: thresholdRatio,
    compactionThresholdTokens,
    tokensUntilCompaction,
    status,
    statusLabel,
    measurementSource,
    turnCount,
    avgTokensPerTurn,
    estimatedTurnsRemaining,
    estimatedSessionCostUsd,
    segments,
    lastReportedUsage,
  };
}
