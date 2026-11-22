import { Activity, Gauge, Plus, RefreshCw, X } from 'lucide-react';
import {
  formatCompactTokens,
  type ContextWindowUsageReport,
  type ModelInfoSpec,
  type ModelMetadataSource,
} from '../utils/modelContextEngine';

interface ContextWindowMonitorProps {
  usage: ContextWindowUsageReport;
  availableModels: ModelInfoSpec[];
  selectedModelId: string | null;
  activeModelId: string;
  liveCatalogFetched: boolean;
  gatewayConnected: boolean;
  refreshing: boolean;
  onSelectModel: (modelId: string | null) => void;
  onRefresh: () => void;
  onClose: () => void;
  onStartNewSession?: () => void;
  onOpenRecallMonitor?: () => void;
}

function formatSourceLabel(source: ModelMetadataSource): string {
  switch (source) {
    case 'gateway':
      return 'Gateway';
    case 'openrouter-live':
      return 'Live catalog';
    case 'hermes-config':
      return 'Hermes config';
    default:
      return 'Catalog';
  }
}

export function ContextWindowMonitor({
  usage,
  availableModels,
  selectedModelId,
  activeModelId,
  liveCatalogFetched,
  gatewayConnected,
  refreshing,
  onSelectModel,
  onRefresh,
  onClose,
  onStartNewSession,
  onOpenRecallMonitor,
}: ContextWindowMonitorProps) {
  const isComparingModel = Boolean(selectedModelId && selectedModelId !== activeModelId);

  const statusColorClass =
    usage.status === 'critical'
      ? 'text-rose-300'
      : usage.status === 'warning'
      ? 'text-amber-300'
      : usage.status === 'elevated'
      ? 'text-sky-300'
      : 'text-emerald-300';

  const thresholdPct = Math.round(usage.compactionThresholdRatio * 100);

  return (
    <section
      data-testid="context-window-monitor"
      aria-label="Model information and context window usage"
      className="w-full rounded-xl border border-white/14 bg-[#0E1117]/96 backdrop-blur-xl text-left shadow-[0_20px_54px_rgba(0,0,0,0.68)] overflow-hidden"
    >
      {/* Header Row — strictly single-row on desktop */}
      <div className="flex items-center justify-between gap-2 px-4 py-2.5 border-b border-white/[0.08]">
        <div className="flex items-center gap-2 min-w-0">
          <Gauge className="w-3.5 h-3.5 text-[var(--ui-accent-soft)] shrink-0" />
          <h3 className="text-xs font-semibold text-white whitespace-nowrap shrink-0">
            Context Window
          </h3>
          <span aria-hidden="true" className="hidden sm:inline text-zinc-600 shrink-0">·</span>
          <span
            data-testid="ctx-header-model"
            className="hidden sm:inline font-mono text-[11px] text-zinc-400 truncate tabular-nums"
          >
            {usage.model.displayName} ({formatCompactTokens(usage.contextWindow)})
          </span>
        </div>

        <div className="flex items-center gap-1.5 shrink-0">
          <button
            type="button"
            data-testid="ctx-refresh-btn"
            onClick={onRefresh}
            disabled={refreshing}
            title="Fetch latest model specs and provider usage"
            className="h-7 px-2.5 rounded-lg border border-white/12 bg-white/[0.04] hover:bg-white/[0.09] text-[11px] font-medium text-zinc-200 hover:text-white inline-flex items-center gap-1.5 transition-colors whitespace-nowrap cursor-pointer disabled:opacity-50"
          >
            <RefreshCw className={`w-3 h-3 ${refreshing ? 'animate-spin' : ''}`} />
            <span>{refreshing ? 'Syncing…' : 'Fetch info'}</span>
          </button>

          {onStartNewSession && (
            <button
              type="button"
              data-testid="ctx-new-session-btn"
              onClick={onStartNewSession}
              className={`h-7 px-2.5 rounded-lg text-[11px] font-medium inline-flex items-center gap-1 transition-colors whitespace-nowrap cursor-pointer ${
                usage.status === 'critical' || usage.status === 'warning'
                  ? 'ui-accent-btn font-semibold hover:opacity-90'
                  : 'border border-white/12 bg-white/[0.04] text-zinc-200 hover:text-white hover:bg-white/[0.09]'
              }`}
            >
              <Plus className="w-3 h-3" />
              <span>New session</span>
            </button>
          )}

          <button
            type="button"
            data-testid="ctx-close-btn"
            onClick={onClose}
            aria-label="Close context window monitor"
            className="w-7 h-7 rounded-lg text-zinc-400 hover:text-white hover:bg-white/[0.07] inline-flex items-center justify-center transition-colors cursor-pointer"
          >
            <X className="w-3.5 h-3.5" />
          </button>
        </div>
      </div>

      <div className="p-4 space-y-3.5">
        {/* 4-Column Tabular KPI Strip (Hairline Divided) */}
        <div className="grid grid-cols-2 sm:grid-cols-4 gap-y-2.5 sm:gap-y-0 sm:divide-x sm:divide-white/[0.08] pb-3 border-b border-white/[0.07]">
          <div className="pr-3">
            <div className="text-[11px] text-zinc-400">Context Used</div>
            <div className="mt-0.5 flex items-baseline gap-2">
              <span
                data-testid="ctx-used-pct"
                className="font-mono text-base font-semibold text-white tabular-nums"
              >
                {usage.usedPct.toFixed(1)}%
              </span>
              <span className={`text-[11px] font-medium ${statusColorClass}`}>
                {usage.statusLabel}
              </span>
            </div>
          </div>

          <div className="sm:px-3">
            <div className="text-[11px] text-zinc-400">Remaining Headroom</div>
            <div className="mt-0.5 flex items-baseline gap-1.5 font-mono tabular-nums">
              <span
                data-testid="ctx-remaining-tokens"
                className="text-base font-semibold text-white"
              >
                {formatCompactTokens(usage.remainingTokens)}
              </span>
              <span className="text-[10.5px] text-zinc-500">
                {usage.estimatedTurnsRemaining === 0
                  ? 'at floor'
                  : `~${usage.estimatedTurnsRemaining} ${
                      usage.estimatedTurnsRemaining === 1 ? 'turn' : 'turns'
                    }`}
              </span>
            </div>
          </div>

          <div className="pr-3 sm:px-3">
            <div className="text-[11px] text-zinc-400">Max Output & Cost</div>
            <div className="mt-0.5 flex items-baseline gap-1.5 font-mono tabular-nums">
              <span className="text-base font-semibold text-white">
                {formatCompactTokens(usage.model.maxOutputTokens)}
              </span>
              <span className="text-[10.5px] text-zinc-500 truncate">
                {usage.model.pricing
                  ? `$${usage.model.pricing.inputPerMillion}/$${usage.model.pricing.outputPerMillion}M`
                  : usage.model.modality}
              </span>
            </div>
          </div>

          <div className="sm:pl-3">
            <div className="text-[11px] text-zinc-400">Model & Source</div>
            <div className="mt-0.5 font-mono text-[11px] text-zinc-200 truncate">
              {usage.model.provider}
            </div>
            <div className="font-mono text-[10px] text-zinc-500 whitespace-nowrap tabular-nums">
              {formatSourceLabel(usage.model.metadataSource)} ·{' '}
              {usage.measurementSource === 'provider-usage' ? 'live usage' : 'tokenized'}
            </div>
          </div>
        </div>

        {/* High-Precision Segmented Context Bar */}
        <div className="space-y-2">
          <div className="flex flex-wrap items-center justify-between gap-2 font-mono text-[11px] tabular-nums">
            <div className="flex items-center gap-2 text-zinc-200">
              <span data-testid="ctx-token-ratio" className="font-semibold text-white">
                {usage.usedTokens.toLocaleString()} / {usage.contextWindow.toLocaleString()} tokens
              </span>
              {usage.lastReportedUsage?.cachedTokens ? (
                <span className="text-zinc-500">
                  ({formatCompactTokens(usage.lastReportedUsage.cachedTokens)} cached)
                </span>
              ) : null}
            </div>
            <div className="text-zinc-400">
              Compact floor {thresholdPct}% ({formatCompactTokens(usage.compactionThresholdTokens)})
            </div>
          </div>

          <div
            data-testid="ctx-segmented-bar"
            role="progressbar"
            aria-valuenow={Math.round(usage.usedPct)}
            aria-valuemin={0}
            aria-valuemax={100}
            aria-label={`Context window ${usage.usedPct.toFixed(1)} percent used`}
            className="relative h-2.5 w-full rounded-md bg-white/[0.06] border border-white/[0.07] overflow-hidden"
          >
            <div className="flex h-full w-full">
              {usage.segments.map((seg) => {
                if (seg.tokens <= 0) return null;
                // Keep non-zero segments subtly visible (>=0.5%) while staying proportional
                const widthPct = Math.max(0.5, seg.percentOfWindow);
                return (
                  <div
                    key={seg.id}
                    title={`${seg.label}: ${seg.tokens.toLocaleString()} tokens (${seg.percentOfWindow.toFixed(2)}%)`}
                    style={{
                      width: `${widthPct}%`,
                      backgroundColor: seg.color,
                    }}
                    className="h-full transition-all duration-300 first:rounded-l-[5px]"
                  />
                );
              })}
            </div>

            {/* Compaction Threshold Hairline Marker */}
            <div
              aria-hidden="true"
              title={`Compaction threshold (${thresholdPct}%)`}
              style={{ left: `${thresholdPct}%` }}
              className="absolute inset-y-0 w-px bg-amber-300/75 pointer-events-none"
            />
          </div>
        </div>

        {/* Hairline-Divided Token Breakdown Table */}
        <div className="rounded-lg border border-white/[0.07] bg-black/30 divide-y divide-white/[0.06]">
          {usage.segments.map((seg) => (
            <div
              key={seg.id}
              className="flex items-center justify-between gap-3 px-3 py-1.5 text-[11px]"
            >
              <div className="flex items-center gap-2 min-w-0">
                <span
                  aria-hidden="true"
                  style={{ backgroundColor: seg.color }}
                  className="w-2 h-2 rounded-[2px] shrink-0"
                />
                <span className="text-zinc-300 truncate">{seg.label}</span>
              </div>
              <div className="flex items-center gap-3 font-mono tabular-nums shrink-0">
                <span className="text-zinc-200">{seg.tokens.toLocaleString()} tok</span>
                <span className="w-12 text-right text-zinc-500">
                  {seg.percentOfWindow.toFixed(1)}%
                </span>
              </div>
            </div>
          ))}
        </div>

        {/* Bottom Row: Model Spec Switcher / Inspector & Jev Recall Shortcut */}
        <div className="flex flex-wrap items-center justify-between gap-2 pt-0.5 text-[11px]">
          <div className="flex items-center gap-2 min-w-0 flex-1">
            <label
              htmlFor="ctx-model-select"
              className="text-zinc-500 whitespace-nowrap shrink-0"
            >
              Model spec:
            </label>
            <select
              id="ctx-model-select"
              data-testid="ctx-model-select"
              value={selectedModelId ?? usage.model.id}
              onChange={(e) => {
                const next = e.target.value;
                onSelectModel(next === activeModelId ? null : next);
              }}
              className="h-7 min-w-0 max-w-[280px] rounded-lg border border-white/12 bg-[#141821] px-2 font-mono text-[11px] text-zinc-200 focus:outline-none focus:border-[var(--ui-accent)] cursor-pointer"
            >
              {availableModels.map((m) => (
                <option key={m.id} value={m.id}>
                  {m.displayName} · {formatCompactTokens(m.contextWindow)}
                  {m.id === activeModelId ? ' (active)' : ''}
                </option>
              ))}
            </select>
            {isComparingModel && (
              <button
                type="button"
                onClick={() => onSelectModel(null)}
                className="text-[11px] text-[var(--ui-accent-soft)] hover:text-white whitespace-nowrap cursor-pointer"
              >
                Reset to active
              </button>
            )}
          </div>

          <div className="flex items-center gap-3 shrink-0 font-mono text-[10.5px] text-zinc-500 tabular-nums">
            <span>
              {gatewayConnected
                ? 'Gateway linked'
                : liveCatalogFetched
                ? 'OpenRouter live specs'
                : 'Local specs'}
            </span>
            {usage.estimatedSessionCostUsd !== null && (
              <span>Est. ~${usage.estimatedSessionCostUsd.toFixed(4)}</span>
            )}
            {onOpenRecallMonitor && (
              <button
                type="button"
                data-testid="ctx-open-recall-btn"
                onClick={onOpenRecallMonitor}
                className="font-sans text-[11px] text-[var(--ui-accent-soft)] hover:text-white inline-flex items-center gap-1 transition-colors cursor-pointer"
              >
                <Activity className="w-3 h-3" />
                <span>Recall curve</span>
              </button>
            )}
          </div>
        </div>
      </div>
    </section>
  );
}
