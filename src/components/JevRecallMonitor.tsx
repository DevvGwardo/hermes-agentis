import { useCallback, useEffect, useMemo, useState } from 'react';
import {
  Activity,
  Check,
  Copy,
  Download,
  FileCode2,
  FolderDown,
  Plus,
  RefreshCw,
  X,
} from 'lucide-react';
import type { TerminalEntry } from '../types/hermes';
import {
  JEV_CONTEXT_LIMITS,
  UNIICHAT_RECALL_BENCHMARKS,
  buildJevRecallProjection,
  type AccuracyProjectionPoint,
  type JevLiveEvaluation,
  type UniichatRetrievalMode,
} from '../utils/jevRecallEngine';

interface JevSkillInfo {
  skillName: string;
  repoPath: string;
  installedPath: string;
  isInstalledLocally: boolean;
  liveJevConfigured: boolean;
  jevModel: string;
  endpoint: string;
  markdown: string;
}

interface JevRecallMonitorProps {
  sessionId: string;
  entries: TerminalEntry[];
  onClose: () => void;
  onStartNewSession?: () => void;
  compact?: boolean;
}

const RETRIEVAL_MODES: readonly UniichatRetrievalMode[] = [
  'coverage-rerank',
  'keyword-first-pass',
  'bm25-only',
];

const THRESHOLD_OPTIONS = [0.7, 0.78, 0.85] as const;
const DEPTH_SIM_OPTIONS = [0, 10, 20, 30] as const;

function formatTokens(n: number): string {
  if (n >= 1000) return `${(n / 1000).toFixed(1)}k`;
  return String(n);
}

export function JevRecallMonitor({
  sessionId,
  entries,
  onClose,
  onStartNewSession,
  compact = false,
}: JevRecallMonitorProps) {
  const [activeTab, setActiveTab] = useState<'projection' | 'skill'>('projection');
  const [mode, setMode] = useState<UniichatRetrievalMode>('coverage-rerank');
  const [queryExpansion, setQueryExpansion] = useState(true);
  const [resetThreshold, setResetThreshold] = useState<number>(0.78);
  const [showHermesBaseline, setShowHermesBaseline] = useState(true);
  const [simulatedTurnOffset, setSimulatedTurnOffset] = useState<number>(0);

  const [liveEvaluation, setLiveEvaluation] = useState<JevLiveEvaluation | null>(null);
  const [evaluating, setEvaluating] = useState(false);
  const [hoveredIdx, setHoveredIdx] = useState<number | null>(null);

  const [skillInfo, setSkillInfo] = useState<JevSkillInfo | null>(null);
  const [skillLoading, setSkillLoading] = useState(false);
  const [installingSkill, setInstallingSkill] = useState(false);
  const [installFeedback, setInstallFeedback] = useState<string | null>(null);
  const [copiedSkill, setCopiedSkill] = useState(false);

  const projection = useMemo(
    () =>
      buildJevRecallProjection({
        entries,
        mode,
        queryExpansion,
        resetThreshold,
        liveEvaluation,
        simulatedTurnOffset,
      }),
    [entries, mode, queryExpansion, resetThreshold, liveEvaluation, simulatedTurnOffset]
  );

  // Load skill metadata & SKILL.md from backend once on mount
  useEffect(() => {
    let active = true;
    setSkillLoading(true);
    fetch('/api/hermes/jev/skill')
      .then((r) => (r.ok ? r.json() : null))
      .then((data: JevSkillInfo | null) => {
        if (active && data) {
          setSkillInfo(data);
        }
      })
      .catch(() => {})
      .finally(() => {
        if (active) setSkillLoading(false);
      });
    return () => {
      active = false;
    };
  }, []);

  // Reset live evaluation when user changes tuning controls so displayed primitives stay synchronized
  useEffect(() => {
    setLiveEvaluation(null);
  }, [sessionId, mode, queryExpansion, resetThreshold, simulatedTurnOffset, entries.length]);

  const handleEvaluateNow = useCallback(async () => {
    setEvaluating(true);
    try {
      const res = await fetch('/api/hermes/jev/evaluate', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          sessionId,
          mode,
          queryExpansion,
          resetThreshold,
          simulatedTurnOffset,
          entries: entries.slice(-60).map((e) => ({
            id: e.id,
            role: e.role,
            text: e.text,
            timestamp: e.timestamp,
          })),
        }),
      });
      if (res.ok) {
        const data = (await res.json()) as { evaluation?: JevLiveEvaluation };
        if (data.evaluation) {
          setLiveEvaluation(data.evaluation);
        }
      }
    } catch {
      // Keep local calibrated curve if server is temporarily unavailable
    } finally {
      setEvaluating(false);
    }
  }, [sessionId, mode, queryExpansion, resetThreshold, simulatedTurnOffset, entries]);

  const handleInstallSkill = useCallback(async () => {
    setInstallingSkill(true);
    setInstallFeedback(null);
    try {
      const res = await fetch('/api/hermes/jev/skill/install', { method: 'POST' });
      const data = (await res.json().catch(() => ({}))) as {
        installed?: boolean;
        installedPath?: string;
        error?: string;
      };
      if (res.ok && data.installed) {
        setSkillInfo((prev) =>
          prev
            ? {
                ...prev,
                isInstalledLocally: true,
                installedPath: data.installedPath || prev.installedPath,
              }
            : prev
        );
        setInstallFeedback(`Installed to ${data.installedPath || '~/.hermes/skills/memory/uniichat-recall-jev/SKILL.md'}`);
      } else {
        setInstallFeedback(data.error || 'Could not install SKILL.md locally.');
      }
    } catch {
      setInstallFeedback('Server unavailable while installing SKILL.md.');
    } finally {
      setInstallingSkill(false);
    }
  }, []);

  const handleCopySkill = useCallback(() => {
    if (!skillInfo?.markdown) return;
    if (typeof navigator !== 'undefined' && navigator.clipboard?.writeText) {
      void navigator.clipboard.writeText(skillInfo.markdown).catch(() => {});
    }
    setCopiedSkill(true);
    window.setTimeout(() => setCopiedSkill(false), 1600);
  }, [skillInfo]);

  const handleDownloadSkill = useCallback(() => {
    if (!skillInfo?.markdown) return;
    const blob = new Blob([skillInfo.markdown], { type: 'text/markdown;charset=utf-8' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = 'SKILL.md';
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    URL.revokeObjectURL(url);
  }, [skillInfo]);

  // SVG Chart Geometry
  const svgW = 640;
  const svgH = compact ? 156 : 186;
  const padL = 38;
  const padR = 16;
  const padT = 14;
  const padB = 26;
  const plotW = svgW - padL - padR;
  const plotH = svgH - padT - padB;
  const yMin = 0.35;
  const yMax = 1.0;

  const pts = projection.points;
  const maxTurn = Math.max(1, pts[pts.length - 1]?.turn ?? 24);
  const currentTurn = projection.snapshot.turnCount;

  const xOf = (turn: number) => padL + (turn / maxTurn) * plotW;
  const yOf = (acc: number) =>
    padT + ((yMax - Math.max(yMin, Math.min(yMax, acc))) / (yMax - yMin)) * plotH;

  const observedPts = pts.filter((p) => !p.isProjected);
  const projectedPts = pts.filter((p) => p.turn >= currentTurn);

  const pathFrom = (arr: AccuracyProjectionPoint[], getter: (p: AccuracyProjectionPoint) => number) =>
    arr
      .map((p, idx) => `${idx === 0 ? 'M' : 'L'} ${xOf(p.turn).toFixed(1)} ${yOf(getter(p)).toFixed(1)}`)
      .join(' ');

  const observedPath = pathFrom(observedPts, (p) => p.accuracy);
  const projectedPath = pathFrom(projectedPts, (p) => p.accuracy);
  const hermesBaselinePath = pathFrom(pts, (p) => p.originalHermesAccuracy);

  // Shaded RLCD calibrated confidence band polygon
  const bandPolygon = useMemo(() => {
    if (pts.length < 2) return '';
    const upper = pts.map((p) => `${xOf(p.turn).toFixed(1)},${yOf(p.upperBound).toFixed(1)}`);
    const lower = [...pts]
      .reverse()
      .map((p) => `${xOf(p.turn).toFixed(1)},${yOf(p.lowerBound).toFixed(1)}`);
    return `${upper.join(' ')} ${lower.join(' ')}`;
  }, [pts, maxTurn, plotW, plotH]);

  const thresholdY = yOf(projection.resetThreshold);
  const nowX = xOf(currentTurn);
  const nowY = yOf(projection.currentAccuracy);
  const resetTurnClamped = Math.min(maxTurn, projection.recommendedResetTurn);
  const resetInPlot = projection.recommendedResetTurn <= maxTurn;
  const resetX = xOf(resetTurnClamped);

  const inspectedPoint =
    hoveredIdx !== null && pts[hoveredIdx]
      ? pts[hoveredIdx]
      : pts[Math.min(currentTurn, pts.length - 1)];

  const healthColorClass =
    projection.healthStatus === 'reset-now'
      ? 'text-rose-300'
      : projection.healthStatus === 'watch'
      ? 'text-amber-300'
      : 'text-emerald-300';

  const curveStrokeColor =
    projection.healthStatus === 'reset-now'
      ? '#FB7185'
      : projection.healthStatus === 'watch'
      ? '#FBBF24'
      : 'var(--ui-accent-bright, #6EE7B7)';

  const yTicks = [1.0, 0.85, 0.7, 0.55, 0.4];
  const xTickStep = maxTurn <= 24 ? 4 : maxTurn <= 40 ? 8 : 10;
  const xTicks: number[] = [];
  for (let t = 0; t <= maxTurn; t += xTickStep) {
    xTicks.push(t);
  }
  if (xTicks[xTicks.length - 1] !== maxTurn) {
    xTicks.push(maxTurn);
  }

  return (
    <section
      data-testid="jev-recall-monitor"
      aria-label="Jev uniichat-recall accuracy projection"
      className="w-full rounded-xl border border-white/14 bg-[#0E1117]/96 backdrop-blur-xl text-left shadow-[0_20px_54px_rgba(0,0,0,0.68)] overflow-hidden"
    >
      {/* Header Row — strictly single-row on desktop */}
      <div className="flex items-center justify-between gap-2 px-4 py-2.5 border-b border-white/[0.08]">
        <div className="flex items-center gap-2 min-w-0">
          <Activity className="w-3.5 h-3.5 text-[var(--ui-accent-soft)] shrink-0" />
          <h3 className="text-xs font-semibold text-white whitespace-nowrap shrink-0">
            Jev Recall Accuracy
          </h3>
          <span aria-hidden="true" className="hidden sm:inline text-zinc-600 shrink-0">·</span>
          <span className="hidden sm:inline font-mono text-[11px] text-zinc-400 truncate tabular-nums">
            uniichat-recall {(projection.benchmark.hitAt1 * 100).toFixed(1)}%
            <span className="hidden md:inline">
              {' '}·{' '}
              {projection.evaluation.source === 'typesafe-jev-live'
                ? `${projection.evaluation.model} live`
                : 'calibrated'}
            </span>
          </span>
        </div>

        <div className="flex items-center gap-1.5 shrink-0">
          {/* Segmented View Switch */}
          <div
            role="tablist"
            aria-label="Jev recall monitor sections"
            className="flex items-center gap-0.5 p-0.5 rounded-lg bg-white/[0.04] border border-white/[0.08]"
          >
            <button
              type="button"
              role="tab"
              aria-selected={activeTab === 'projection'}
              data-testid="jev-tab-projection"
              onClick={() => setActiveTab('projection')}
              className={`px-2.5 py-1 rounded-md text-[11px] font-medium transition-colors whitespace-nowrap cursor-pointer ${
                activeTab === 'projection'
                  ? 'bg-white/[0.12] text-white'
                  : 'text-zinc-400 hover:text-white'
              }`}
            >
              Projection
            </button>
            <button
              type="button"
              role="tab"
              aria-selected={activeTab === 'skill'}
              data-testid="jev-tab-skill"
              onClick={() => setActiveTab('skill')}
              className={`px-2.5 py-1 rounded-md text-[11px] font-medium transition-colors whitespace-nowrap inline-flex items-center gap-1 cursor-pointer ${
                activeTab === 'skill'
                  ? 'bg-white/[0.12] text-white'
                  : 'text-zinc-400 hover:text-white'
              }`}
            >
              <FileCode2 className="w-3 h-3" />
              <span>SKILL.md</span>
            </button>
          </div>

          <button
            type="button"
            data-testid="jev-evaluate-btn"
            onClick={() => void handleEvaluateNow()}
            disabled={evaluating}
            title="Run TypeSafe Jev System One decision evaluation on active session"
            className="h-7 px-2.5 rounded-lg border border-white/12 bg-white/[0.04] hover:bg-white/[0.09] text-[11px] font-medium text-zinc-200 hover:text-white inline-flex items-center gap-1.5 transition-colors whitespace-nowrap cursor-pointer disabled:opacity-50"
          >
            <RefreshCw className={`w-3 h-3 ${evaluating ? 'animate-spin' : ''}`} />
            <span>{evaluating ? 'Scoring…' : 'Evaluate'}</span>
          </button>

          {onStartNewSession && (
            <button
              type="button"
              data-testid="jev-start-new-session-btn"
              onClick={onStartNewSession}
              className={`h-7 px-2.5 rounded-lg text-[11px] font-medium inline-flex items-center gap-1 transition-colors whitespace-nowrap cursor-pointer ${
                projection.healthStatus === 'reset-now' || projection.healthStatus === 'watch'
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
            data-testid="jev-close-btn"
            onClick={onClose}
            aria-label="Hide Jev Recall Accuracy monitor"
            className="w-7 h-7 rounded-lg text-zinc-400 hover:text-white hover:bg-white/[0.07] inline-flex items-center justify-center transition-colors cursor-pointer"
          >
            <X className="w-3.5 h-3.5" />
          </button>
        </div>
      </div>

      {activeTab === 'projection' ? (
        <div className="p-4 space-y-3.5">
          {/* 4-Column Tabular KPI Strip (Hairline Divided, Zero Nested Cards) */}
          <div className="grid grid-cols-2 sm:grid-cols-4 gap-y-2.5 sm:gap-y-0 sm:divide-x sm:divide-white/[0.08] pb-3 border-b border-white/[0.07]">
            <div className="pr-3">
              <div className="text-[11px] text-zinc-400">Calibrated Accuracy</div>
              <div className="mt-0.5 flex items-baseline gap-2">
                <span
                  data-testid="jev-current-accuracy"
                  className="font-mono text-base font-semibold text-white tabular-nums"
                >
                  {(projection.currentAccuracy * 100).toFixed(1)}%
                </span>
                <span className={`text-[11px] font-medium ${healthColorClass}`}>
                  {projection.healthLabel}
                </span>
              </div>
            </div>

            <div className="sm:px-3">
              <div className="text-[11px] text-zinc-400">Start New Session</div>
              <div className="mt-0.5 flex items-baseline gap-1.5">
                <span
                  data-testid="jev-turns-remaining"
                  className="font-mono text-base font-semibold text-white tabular-nums"
                >
                  {projection.turnsRemaining === 0
                    ? 'Now'
                    : `~${projection.turnsRemaining} ${projection.turnsRemaining === 1 ? 'turn' : 'turns'}`}
                </span>
                <span className="font-mono text-[10.5px] text-zinc-500 tabular-nums">
                  T{projection.recommendedResetTurn}
                </span>
              </div>
            </div>

            <div className="pr-3 sm:px-3">
              <div className="text-[11px] text-zinc-400">Jev Context Window</div>
              <div className="mt-0.5 flex items-baseline gap-1.5 font-mono tabular-nums">
                <span className="text-base font-semibold text-white">
                  {formatTokens(projection.snapshot.estimatedTokens)}
                </span>
                <span className="text-[10.5px] text-zinc-500">
                  / 32k ({projection.jevQuestionBudgetPct}%)
                </span>
              </div>
            </div>

            <div className="sm:pl-3">
              <div className="text-[11px] text-zinc-400">Jev Decision Primitives</div>
              <div className="mt-0.5 font-mono text-[11px] text-zinc-200 tabular-nums leading-snug">
                <span>noul {(projection.evaluation.recallGroundedNoul * 100).toFixed(0)}%</span>
                <span className="text-zinc-600"> · </span>
                <span className="text-zinc-300">{projection.evaluation.sessionAction.replace(/_/g, ' ')}</span>
              </div>
              <div className="font-mono text-[10px] text-zinc-500 tabular-nums">
                abstention {(projection.evaluation.abstentionSafeNoul * 100).toFixed(0)}% · {projection.evaluation.contextCoherence}
              </div>
            </div>
          </div>

          {/* Interactive SVG Projection Graph */}
          <div className="space-y-1.5">
            <div className="flex flex-wrap items-center justify-between gap-2 text-[11px] text-zinc-400">
              <div className="flex flex-wrap items-center gap-3 font-mono tabular-nums">
                <span className="text-zinc-200">
                  Turn {inspectedPoint.turn}
                  {inspectedPoint.turn === currentTurn ? ' (Now)' : inspectedPoint.isProjected ? ' (Projected)' : ''}
                </span>
                <span>
                  Recall{' '}
                  <strong className="text-white font-semibold">
                    {(inspectedPoint.accuracy * 100).toFixed(1)}%
                  </strong>{' '}
                  <span className="text-zinc-500">
                    ({(inspectedPoint.lowerBound * 100).toFixed(0)}–{(inspectedPoint.upperBound * 100).toFixed(0)}%)
                  </span>
                </span>
                <span>{formatTokens(inspectedPoint.tokens)} tok</span>
                {showHermesBaseline && (
                  <span className="text-zinc-500">
                    Hermes 2.2k: {(inspectedPoint.originalHermesAccuracy * 100).toFixed(0)}%
                  </span>
                )}
              </div>

              <button
                type="button"
                onClick={() => setShowHermesBaseline((v) => !v)}
                className="text-[11px] text-zinc-400 hover:text-white transition-colors whitespace-nowrap cursor-pointer"
              >
                {showHermesBaseline ? 'Hide' : 'Show'} Hermes 2.2k baseline
              </button>
            </div>

            <div className="relative w-full overflow-hidden rounded-lg bg-black/35 border border-white/[0.06]">
              <svg
                viewBox={`0 0 ${svgW} ${svgH}`}
                role="img"
                aria-label="Projected uniichat-recall accuracy curve over session turns with new-session reset threshold"
                data-testid="jev-projection-svg"
                className="w-full h-auto block select-none"
                onMouseLeave={() => setHoveredIdx(null)}
                onMouseMove={(e) => {
                  const rect = e.currentTarget.getBoundingClientRect();
                  const relX = ((e.clientX - rect.left) / Math.max(1, rect.width)) * svgW;
                  const frac = Math.max(0, Math.min(1, (relX - padL) / plotW));
                  const idx = Math.round(frac * (pts.length - 1));
                  setHoveredIdx(idx);
                }}
              >
                {/* Horizontal gridlines + Y labels */}
                {yTicks.map((yt) => {
                  const y = yOf(yt);
                  return (
                    <g key={yt}>
                      <line
                        x1={padL}
                        x2={svgW - padR}
                        y1={y}
                        y2={y}
                        stroke="rgba(255,255,255,0.06)"
                        strokeWidth="1"
                      />
                      <text
                        x={padL - 6}
                        y={y + 3.5}
                        textAnchor="end"
                        fill="rgba(161,161,170,0.8)"
                        fontSize="9.5"
                        fontFamily="ui-monospace, SFMono-Regular, Menlo, monospace"
                      >
                        {Math.round(yt * 100)}%
                      </text>
                    </g>
                  );
                })}

                {/* X-axis Turn Labels */}
                {xTicks.map((xt) => {
                  const x = xOf(xt);
                  return (
                    <text
                      key={xt}
                      x={x}
                      y={svgH - 7}
                      textAnchor="middle"
                      fill="rgba(161,161,170,0.75)"
                      fontSize="9.5"
                      fontFamily="ui-monospace, SFMono-Regular, Menlo, monospace"
                    >
                      T{xt}
                    </text>
                  );
                })}

                {/* Danger zone shading below reset threshold */}
                <rect
                  x={padL}
                  y={thresholdY}
                  width={plotW}
                  height={Math.max(0, padT + plotH - thresholdY)}
                  fill="rgba(244, 63, 94, 0.06)"
                />

                {/* Reset Threshold Horizontal Line */}
                <line
                  x1={padL}
                  x2={svgW - padR}
                  y1={thresholdY}
                  y2={thresholdY}
                  stroke="rgba(251, 113, 133, 0.65)"
                  strokeWidth="1.2"
                  strokeDasharray="4 4"
                />
                <text
                  x={svgW - padR - 4}
                  y={thresholdY - 4}
                  textAnchor="end"
                  fill="rgba(253, 164, 175, 0.9)"
                  fontSize="9.5"
                  fontFamily="ui-monospace, SFMono-Regular, Menlo, monospace"
                >
                  New session floor {Math.round(projection.resetThreshold * 100)}%
                </text>

                {/* RLCD Calibrated Confidence Band */}
                {bandPolygon && (
                  <polygon points={bandPolygon} fill="rgba(110, 231, 183, 0.10)" />
                )}

                {/* Original Hermes 2,200-char baseline comparison curve */}
                {showHermesBaseline && (
                  <path
                    d={hermesBaselinePath}
                    fill="none"
                    stroke="rgba(148, 163, 184, 0.42)"
                    strokeWidth="1.35"
                    strokeDasharray="2.5 3.5"
                  />
                )}

                {/* Projected Future Curve */}
                <path
                  d={projectedPath}
                  fill="none"
                  stroke={curveStrokeColor}
                  strokeWidth="2"
                  strokeDasharray="5 3.5"
                  strokeOpacity="0.88"
                />

                {/* Observed Session Curve */}
                {observedPts.length > 1 && (
                  <path
                    d={observedPath}
                    fill="none"
                    stroke={curveStrokeColor}
                    strokeWidth="2.4"
                  />
                )}

                {/* Recommended New Session Crossing Vertical Line */}
                {resetInPlot && (
                  <g>
                    <line
                      x1={resetX}
                      x2={resetX}
                      y1={padT}
                      y2={padT + plotH}
                      stroke="rgba(251, 191, 36, 0.55)"
                      strokeWidth="1.15"
                      strokeDasharray="3 3"
                    />
                    <circle
                      cx={resetX}
                      cy={thresholdY}
                      r="3.8"
                      fill="#0E1117"
                      stroke="#FBBF24"
                      strokeWidth="1.8"
                    />
                    <text
                      x={Math.min(svgW - padR - 4, Math.max(padL + 42, resetX))}
                      y={padT + 10}
                      textAnchor="middle"
                      fill="#FCD34D"
                      fontSize="9.5"
                      fontFamily="ui-monospace, SFMono-Regular, Menlo, monospace"
                    >
                      Reset T{projection.recommendedResetTurn}
                    </text>
                  </g>
                )}

                {/* Current Turn "Now" Marker */}
                <line
                  x1={nowX}
                  x2={nowX}
                  y1={padT}
                  y2={padT + plotH}
                  stroke="rgba(255,255,255,0.22)"
                  strokeWidth="1"
                />
                <circle
                  cx={nowX}
                  cy={nowY}
                  r="4.2"
                  fill="#0E1117"
                  stroke={curveStrokeColor}
                  strokeWidth="2.2"
                />

                {/* Interactive Hover Crosshair */}
                {hoveredIdx !== null && pts[hoveredIdx] && (
                  <g>
                    <line
                      x1={xOf(pts[hoveredIdx].turn)}
                      x2={xOf(pts[hoveredIdx].turn)}
                      y1={padT}
                      y2={padT + plotH}
                      stroke="rgba(255,255,255,0.34)"
                      strokeWidth="1"
                    />
                    <circle
                      cx={xOf(pts[hoveredIdx].turn)}
                      cy={yOf(pts[hoveredIdx].accuracy)}
                      r="3.6"
                      fill="#FFFFFF"
                    />
                  </g>
                )}
              </svg>
            </div>
          </div>

          {/* Recommendation + Controls Row */}
          <div className="flex flex-col gap-2.5 pt-0.5">
            <p className="text-xs text-zinc-300 leading-relaxed">
              {projection.recommendationText}{' '}
              {projection.evaluation.note && (
                <span className="text-zinc-500">({projection.evaluation.note})</span>
              )}
            </p>

            <div className="flex flex-wrap items-center justify-between gap-2 text-[11px]">
              {/* Retrieval Variant Selector */}
              <div className="flex flex-wrap items-center gap-1.5">
                <span className="text-zinc-500">Ranking:</span>
                <div className="inline-flex items-center gap-0.5 p-0.5 rounded-lg bg-white/[0.04] border border-white/[0.08]">
                  {RETRIEVAL_MODES.map((m) => {
                    const b = UNIICHAT_RECALL_BENCHMARKS[m];
                    const active = mode === m;
                    return (
                      <button
                        key={m}
                        type="button"
                        data-testid={`jev-mode-${m}`}
                        onClick={() => setMode(m)}
                        className={`px-2 py-0.5 rounded-md transition-colors whitespace-nowrap cursor-pointer ${
                          active
                            ? 'bg-white/[0.12] text-white font-medium'
                            : 'text-zinc-400 hover:text-white'
                        }`}
                      >
                        {b.shortLabel}
                      </button>
                    );
                  })}
                </div>

                <button
                  type="button"
                  data-testid="jev-query-expansion-toggle"
                  aria-pressed={queryExpansion}
                  onClick={() => setQueryExpansion((v) => !v)}
                  className={`px-2 py-1 rounded-lg border transition-colors whitespace-nowrap cursor-pointer ${
                    queryExpansion
                      ? 'border-[rgba(var(--ui-accent-rgb),0.45)] bg-[rgba(var(--ui-accent-rgb),0.14)] text-[var(--ui-accent-soft)]'
                      : 'border-white/10 bg-white/[0.03] text-zinc-400 hover:text-white'
                  }`}
                >
                  Paraphrase: {queryExpansion ? 'On' : 'Off'}
                </button>
              </div>

              {/* Reset Threshold & What-If Depth Preview */}
              <div className="flex flex-wrap items-center gap-2">
                <div className="inline-flex items-center gap-1">
                  <span className="text-zinc-500">Floor:</span>
                  <div className="inline-flex items-center gap-0.5 p-0.5 rounded-lg bg-white/[0.04] border border-white/[0.08] font-mono tabular-nums">
                    {THRESHOLD_OPTIONS.map((th) => (
                      <button
                        key={th}
                        type="button"
                        data-testid={`jev-threshold-${Math.round(th * 100)}`}
                        onClick={() => setResetThreshold(th)}
                        className={`px-1.5 py-0.5 rounded-md transition-colors cursor-pointer ${
                          resetThreshold === th
                            ? 'bg-white/[0.12] text-white font-medium'
                            : 'text-zinc-400 hover:text-white'
                        }`}
                      >
                        {Math.round(th * 100)}%
                      </button>
                    ))}
                  </div>
                </div>

                <div className="inline-flex items-center gap-1">
                  <span className="text-zinc-500">Simulate:</span>
                  <div className="inline-flex items-center gap-0.5 p-0.5 rounded-lg bg-white/[0.04] border border-white/[0.08] font-mono tabular-nums">
                    {DEPTH_SIM_OPTIONS.map((offset) => (
                      <button
                        key={offset}
                        type="button"
                        data-testid={`jev-sim-${offset}`}
                        onClick={() => setSimulatedTurnOffset(offset)}
                        className={`px-1.5 py-0.5 rounded-md transition-colors cursor-pointer ${
                          simulatedTurnOffset === offset
                            ? 'bg-white/[0.12] text-white font-medium'
                            : 'text-zinc-400 hover:text-white'
                        }`}
                      >
                        {offset === 0 ? 'Live' : `+${offset}t`}
                      </button>
                    ))}
                  </div>
                </div>
              </div>
            </div>
          </div>
        </div>
      ) : (
        /* Hermes SKILL.md Viewer & Local Installer */
        <div data-testid="jev-skill-panel" className="p-4 space-y-3">
          <div className="flex flex-wrap items-center justify-between gap-2">
            <div className="min-w-0 space-y-0.5">
              <div className="text-xs font-semibold text-white">
                Hermes Agent Skill · uniichat-recall-jev
              </div>
              <div className="font-mono text-[11px] text-zinc-400 truncate">
                {skillInfo?.isInstalledLocally
                  ? `Installed at ${skillInfo.installedPath}`
                  : `Bundle: ${skillInfo?.repoPath || 'skills/hermes/uniichat-recall-jev/SKILL.md'}`}{' '}
                · {skillInfo?.liveJevConfigured ? `TYPESAFE_API_KEY configured (${skillInfo.jevModel})` : `Endpoint: ${JEV_CONTEXT_LIMITS.endpoint}`}
              </div>
            </div>

            <div className="flex items-center gap-1.5 shrink-0">
              <button
                type="button"
                data-testid="jev-copy-skill-btn"
                onClick={handleCopySkill}
                disabled={!skillInfo?.markdown}
                className="h-7 px-2.5 rounded-lg border border-white/12 bg-white/[0.04] hover:bg-white/[0.09] text-[11px] font-medium text-zinc-200 hover:text-white inline-flex items-center gap-1.5 transition-colors whitespace-nowrap cursor-pointer disabled:opacity-45"
              >
                {copiedSkill ? <Check className="w-3 h-3 text-emerald-300" /> : <Copy className="w-3 h-3" />}
                <span>{copiedSkill ? 'Copied' : 'Copy SKILL.md'}</span>
              </button>

              <button
                type="button"
                data-testid="jev-download-skill-btn"
                onClick={handleDownloadSkill}
                disabled={!skillInfo?.markdown}
                className="h-7 px-2.5 rounded-lg border border-white/12 bg-white/[0.04] hover:bg-white/[0.09] text-[11px] font-medium text-zinc-200 hover:text-white inline-flex items-center gap-1.5 transition-colors whitespace-nowrap cursor-pointer disabled:opacity-45"
              >
                <Download className="w-3 h-3" />
                <span>Download</span>
              </button>

              <button
                type="button"
                data-testid="jev-install-skill-btn"
                onClick={() => void handleInstallSkill()}
                disabled={installingSkill || !skillInfo?.markdown}
                className="ui-accent-btn h-7 px-2.5 rounded-lg text-[11px] font-semibold inline-flex items-center gap-1.5 hover:opacity-90 transition-opacity whitespace-nowrap cursor-pointer disabled:opacity-50"
              >
                <FolderDown className="w-3 h-3" />
                <span>
                  {installingSkill
                    ? 'Installing…'
                    : skillInfo?.isInstalledLocally
                    ? 'Reinstall to ~/.hermes/skills'
                    : 'Install to ~/.hermes/skills'}
                </span>
              </button>
            </div>
          </div>

          {installFeedback && (
            <div
              data-testid="jev-install-feedback"
              className="font-mono text-[11px] text-emerald-300"
            >
              {installFeedback}
            </div>
          )}

          <pre
            data-testid="jev-skill-markdown"
            className="rounded-lg bg-black/45 border border-white/[0.08] p-3 font-mono text-[11px] leading-relaxed text-zinc-200 whitespace-pre-wrap break-words max-h-[260px] overflow-y-auto cyber-scroll"
          >
            {skillLoading ? 'Loading uniichat-recall-jev SKILL.md…' : skillInfo?.markdown || 'SKILL.md unavailable.'}
          </pre>
        </div>
      )}
    </section>
  );
}
