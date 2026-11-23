import { useEffect, useMemo, useState } from 'react';
import {
  AlertCircle,
  BookOpen,
  Brain,
  Check,
  CheckCircle2,
  ChevronDown,
  Copy,
  CornerDownRight,
  FileCode2,
  Globe,
  Loader2,
  MessageSquareText,
  MoreHorizontal,
  Search,
  Terminal,
  Users,
  Wrench,
} from 'lucide-react';
import {
  coalesceConsecutiveSteps,
  type ClassifiedTraceStep,
  type TraceToolKind,
} from '../utils/threadTrace';
import { summarizeTraceHeader } from '../utils/threadSearch';
import { AnimatedCollapsible, StaggerItem, StatusPulse } from './ui';

interface ThreadTraceTreeProps {
  agentLabel?: string;
  sessionId?: string;
  items: ClassifiedTraceStep[];
  isLive: boolean;
  hideLiveFooter?: boolean;
  defaultExpanded?: boolean;
}

const DEFAULT_VISIBLE_LIMIT = 8;
const STATUS_ONLY_DETAIL_RE =
  /^(completed|complete|done|finished|running|started|pending|failed|error|cancelled|interrupted)$/i;

export function ThreadTraceTree({
  agentLabel = 'Activity',
  sessionId,
  items,
  isLive,
  hideLiveFooter = false,
  defaultExpanded = true,
}: ThreadTraceTreeProps) {
  const [collapsed, setCollapsed] = useState(!defaultExpanded);
  const [showAllSteps, setShowAllSteps] = useState(false);
  const [copiedStepId, setCopiedStepId] = useState<string | null>(null);

  // Respect user's Settings toggle if they change default expansion behavior
  useEffect(() => {
    setCollapsed(!defaultExpanded);
  }, [defaultExpanded]);

  const coalescedItems = useMemo(() => coalesceConsecutiveSteps(items), [items]);

  const hiddenCount =
    !showAllSteps && coalescedItems.length > DEFAULT_VISIBLE_LIMIT
      ? coalescedItems.length - DEFAULT_VISIBLE_LIMIT
      : 0;
  const visibleItems =
    hiddenCount > 0 ? coalescedItems.slice(-DEFAULT_VISIBLE_LIMIT) : coalescedItems;

  const stepSummary = summarizeTraceHeader(items, isLive);
  const latestStep = coalescedItems[coalescedItems.length - 1];
  const collapsedPreview = useMemo(() => {
    if (latestStep) {
      const detail =
        latestStep.detail && !STATUS_ONLY_DETAIL_RE.test(latestStep.detail.trim())
          ? latestStep.detail.trim()
          : '';
      return detail ? `${latestStep.label} · ${detail}` : latestStep.label;
    }
    return stepSummary.replace(/^\d+\s+steps?\s*·\s*/i, '');
  }, [latestStep, stepSummary]);

  const totalStepCount = items.length;
  const stepCountLabel =
    totalStepCount === 0
      ? isLive
        ? 'Starting'
        : ''
      : `${totalStepCount} ${totalStepCount === 1 ? 'step' : 'steps'}`;

  const hasFailedStep = items.some((s) => s.status === 'failed');

  const renderStepIcon = (step: ClassifiedTraceStep) => {
    const kind: TraceToolKind =
      step.toolKind ??
      (step.icon === 'reasoning'
        ? 'reasoning'
        : step.icon === 'commentary'
        ? 'commentary'
        : step.icon === 'spawn'
        ? 'agent'
        : step.icon === 'link'
        ? 'web'
        : step.icon === 'tool'
        ? 'tool'
        : 'status');

    const tint =
      step.status === 'failed'
        ? 'text-rose-300'
        : step.status === 'running'
        ? 'text-amber-300'
        : kind === 'terminal'
        ? 'text-[var(--ui-accent-soft)]'
        : kind === 'skill'
        ? 'text-violet-300/90'
        : kind === 'search' || kind === 'web'
        ? 'text-sky-300/90'
        : kind === 'file'
        ? 'text-emerald-300/85'
        : 'text-zinc-400';

    switch (kind) {
      case 'terminal':
        return <Terminal className={`w-3.5 h-3.5 ${tint}`} />;
      case 'skill':
        return <BookOpen className={`w-3.5 h-3.5 ${tint}`} />;
      case 'file':
        return <FileCode2 className={`w-3.5 h-3.5 ${tint}`} />;
      case 'search':
        return <Search className={`w-3.5 h-3.5 ${tint}`} />;
      case 'web':
        return <Globe className={`w-3.5 h-3.5 ${tint}`} />;
      case 'agent':
        return <Users className={`w-3.5 h-3.5 ${tint}`} />;
      case 'reasoning':
        return <Brain className={`w-3.5 h-3.5 ${tint}`} />;
      case 'commentary':
        return <MessageSquareText className={`w-3.5 h-3.5 ${tint}`} />;
      case 'tool':
        return <Wrench className={`w-3.5 h-3.5 ${tint}`} />;
      default:
        return <CornerDownRight className={`w-3.5 h-3.5 ${tint}`} />;
    }
  };

  const renderStatusBadge = (step: ClassifiedTraceStep) => {
    if (step.status === 'running') {
      return (
        <Loader2
          aria-label="Running"
          className="w-3 h-3 text-amber-300 motion-safe:animate-spin motion-safe-only shrink-0"
        />
      );
    }
    if (step.status === 'failed') {
      return (
        <AlertCircle aria-label="Failed" className="w-3 h-3 text-rose-300 shrink-0" />
      );
    }
    return (
      <Check
        aria-label="Completed"
        className="w-3 h-3 text-emerald-400/80 shrink-0"
      />
    );
  };

  const handleCopyStep = (stepId: string, content: string) => {
    if (typeof navigator !== 'undefined' && navigator.clipboard?.writeText) {
      void navigator.clipboard.writeText(content).catch(() => {});
    }
    setCopiedStepId(stepId);
    window.setTimeout(() => {
      setCopiedStepId((curr) => (curr === stepId ? null : curr));
    }, 1400);
  };

  return (
    <div
      className="max-w-[66ch] py-0.5"
      data-testid="thread-trace-tree"
      data-expanded={!collapsed ? 'true' : 'false'}
      title={sessionId}
    >
      <div
        className={`rounded-xl border backdrop-blur-md overflow-hidden transition-colors ${
          isLive
            ? 'border-amber-400/24 bg-[#0E1117]/90 shadow-[0_8px_28px_rgba(0,0,0,0.38)]'
            : hasFailedStep
            ? 'border-rose-400/22 bg-[#0E1117]/88 shadow-[0_6px_22px_rgba(0,0,0,0.30)]'
            : 'border-white/[0.11] bg-[#0E1117]/88 shadow-[0_6px_22px_rgba(0,0,0,0.30)]'
        }`}
      >
        {/* Integrated Single-Row Header */}
        <button
          type="button"
          data-testid="thread-trace-toggle"
          onClick={() => setCollapsed((v) => !v)}
          aria-expanded={!collapsed}
          className={`w-full flex items-center justify-between gap-2.5 px-3 py-2 text-left group cursor-pointer transition-colors hover:bg-white/[0.03] ${
            !collapsed ? 'border-b border-white/[0.07]' : ''
          }`}
        >
          <div className="flex items-center gap-2 min-w-0">
            {isLive ? (
              <StatusPulse size="sm" state="thinking" />
            ) : hasFailedStep ? (
              <AlertCircle className="w-3.5 h-3.5 text-rose-300 shrink-0" aria-hidden="true" />
            ) : (
              <CheckCircle2
                className="w-3.5 h-3.5 text-emerald-400/80 shrink-0"
                aria-hidden="true"
              />
            )}

            <span className="text-[12px] font-semibold text-zinc-100 group-hover:text-white transition-colors shrink-0">
              {agentLabel}
            </span>

            {stepCountLabel && (
              <span className="font-mono text-[10.5px] tabular-nums text-zinc-400 shrink-0">
                {stepCountLabel}
              </span>
            )}

            {collapsed && collapsedPreview && (
              <span
                className={`font-mono text-[11px] tabular-nums truncate ${
                  isLive ? 'text-amber-300/90' : 'text-zinc-400'
                }`}
              >
                · {collapsedPreview}
              </span>
            )}
          </div>

          <div className="flex items-center gap-2 shrink-0">
            <span
              className={`font-mono text-[10px] uppercase tracking-wider tabular-nums ${
                isLive
                  ? 'text-amber-300/90'
                  : hasFailedStep
                  ? 'text-rose-300/90'
                  : 'text-zinc-500'
              }`}
            >
              {isLive ? 'Live' : hasFailedStep ? 'Attention' : 'Done'}
            </span>
            <ChevronDown
              className={`w-3.5 h-3.5 text-zinc-400 group-hover:text-white shrink-0 transition-transform duration-200 ease-[cubic-bezier(0.16,1,0.3,1)] ${
                collapsed ? '-rotate-90' : 'rotate-0'
              }`}
              aria-hidden="true"
            />
          </div>
        </button>

        {/* Expanded Unified Step Timeline */}
        <AnimatedCollapsible open={!collapsed}>
          <div className="divide-y divide-white/[0.05]">
            {hiddenCount > 0 && (
              <button
                type="button"
                onClick={() => setShowAllSteps(true)}
                className="w-full flex items-center gap-2 px-3 py-1.5 text-xs text-zinc-400 hover:text-white hover:bg-white/[0.03] transition-colors cursor-pointer"
              >
                <MoreHorizontal className="w-3.5 h-3.5 text-zinc-500 shrink-0" aria-hidden="true" />
                <span className="font-mono text-[11px] tabular-nums">
                  Show {hiddenCount} earlier {hiddenCount === 1 ? 'step' : 'steps'}
                </span>
              </button>
            )}

            {visibleItems.map((step, idx) => {
              const isReasoningStep = Boolean(step.isReasoning || step.icon === 'reasoning');
              const isStatusOnlyDetail = Boolean(
                step.detail && STATUS_ONLY_DETAIL_RE.test(step.detail.trim())
              );
              const meaningfulDetail = isStatusOnlyDetail ? '' : step.detail;
              const copyPayload = meaningfulDetail
                ? `${step.label}: ${meaningfulDetail}`
                : step.label;

              return (
                <StaggerItem
                  key={step.id}
                  index={idx}
                  className={`group/step px-3 py-2 transition-colors hover:bg-white/[0.025] ${
                    isReasoningStep ? 'space-y-1.5' : 'flex items-center justify-between gap-3'
                  }`}
                >
                  {isReasoningStep ? (
                    <>
                      <div className="flex items-center justify-between gap-3">
                        <div className="flex items-center gap-2 min-w-0">
                          <span aria-hidden="true" className="shrink-0 flex items-center">
                            {renderStepIcon(step)}
                          </span>
                          <span className="font-mono text-[11.5px] font-medium text-zinc-200 shrink-0">
                            {step.label}
                          </span>
                        </div>
                        <div className="flex items-center gap-2 shrink-0">
                          {step.detail && (
                            <button
                              type="button"
                              onClick={() => handleCopyStep(step.id, step.detail)}
                              aria-label="Copy reasoning note"
                              className="inline-flex items-center gap-1 font-mono text-[10px] text-zinc-500 hover:text-zinc-200 transition-colors cursor-pointer"
                            >
                              {copiedStepId === step.id ? (
                                <>
                                  <Check className="w-2.5 h-2.5 text-[var(--ui-accent-soft)]" />
                                  <span className="text-[var(--ui-accent-soft)]">Copied</span>
                                </>
                              ) : (
                                <>
                                  <Copy className="w-2.5 h-2.5" />
                                  <span>Copy</span>
                                </>
                              )}
                            </button>
                          )}
                          <span className="font-mono text-[10.5px] tabular-nums text-zinc-500">
                            {step.timestamp}
                          </span>
                        </div>
                      </div>
                      {step.detail && (
                        <p
                          data-testid="reasoning-trace-detail"
                          className="ml-5.5 text-[12px] leading-relaxed text-zinc-300/90 whitespace-pre-wrap break-words rounded-lg bg-black/35 border border-white/[0.06] px-2.5 py-1.5"
                        >
                          {step.detail}
                        </p>
                      )}
                    </>
                  ) : (
                    <>
                      <div className="flex items-center gap-2 min-w-0 flex-1">
                        <span aria-hidden="true" className="shrink-0 flex items-center">
                          {renderStepIcon(step)}
                        </span>
                        <span
                          title={step.label}
                          className="font-mono text-[11.5px] font-medium text-zinc-100 shrink-0 max-w-[44%] truncate"
                        >
                          {step.label}
                        </span>
                        {meaningfulDetail ? (
                          <span
                            title={meaningfulDetail}
                            className="font-mono text-[11px] text-zinc-400 truncate min-w-0"
                          >
                            {meaningfulDetail}
                          </span>
                        ) : (
                          <span className="font-mono text-[10.5px] text-zinc-500 shrink-0">
                            {step.status === 'running'
                              ? 'running'
                              : step.status === 'failed'
                              ? 'failed'
                              : 'completed'}
                          </span>
                        )}
                        {step.repeatCount > 1 && (
                          <span className="font-mono text-[10px] px-1.5 py-0.5 rounded bg-white/[0.06] border border-white/[0.08] text-zinc-300 tabular-nums shrink-0">
                            ×{step.repeatCount}
                          </span>
                        )}
                      </div>

                      <div className="flex items-center gap-2 shrink-0">
                        {meaningfulDetail && (
                          <button
                            type="button"
                            onClick={() => handleCopyStep(step.id, copyPayload)}
                            aria-label={`Copy ${step.label} detail`}
                            className="opacity-0 group-hover/step:opacity-100 focus:opacity-100 inline-flex items-center gap-1 font-mono text-[10px] text-zinc-500 hover:text-zinc-200 transition-opacity cursor-pointer"
                          >
                            {copiedStepId === step.id ? (
                              <Check className="w-2.5 h-2.5 text-[var(--ui-accent-soft)]" />
                            ) : (
                              <Copy className="w-2.5 h-2.5" />
                            )}
                          </button>
                        )}
                        {renderStatusBadge(step)}
                        {step.timestamp && (
                          <span className="font-mono text-[10.5px] tabular-nums text-zinc-500">
                            {step.timestamp}
                          </span>
                        )}
                      </div>
                    </>
                  )}
                </StaggerItem>
              );
            })}

            {isLive && !hideLiveFooter && (
              <div
                data-testid="thread-trace-live-footer"
                className="flex items-center justify-between gap-2 px-3 py-1.5 bg-amber-400/[0.03] text-[11.5px]"
              >
                <div className="flex items-center gap-2 text-amber-200/90">
                  <Loader2 className="w-3 h-3 text-amber-300 motion-safe:animate-spin motion-safe-only shrink-0" />
                  <span className="font-medium">Thinking…</span>
                </div>
                <span className="font-mono text-[10px] text-zinc-500">active run</span>
              </div>
            )}
          </div>
        </AnimatedCollapsible>
      </div>
    </div>
  );
}
