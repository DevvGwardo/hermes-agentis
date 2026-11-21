import { useEffect, useMemo, useRef, useState } from 'react';
import {
  ArrowLeft,
  ChevronRight,
  LayoutGrid,
  MessageSquarePlus,
  PanelLeftClose,
  Pin,
  Plus,
  Search,
  Settings,
  X,
} from 'lucide-react';
import type {
  ApprovalCard,
  BackgroundTask,
  BriefDecision,
  Channel,
  ChannelId,
  ExpressionMood,
  HermesAgentSession,
  HermesGatewayStatus,
} from '../types/hermes';
import type { CanvasSession } from '../utils/canvasSessions';
import type { LookTarget } from '../utils/asciiPortraitEngine';
import type { BrailleDotProfile } from '../utils/brailleCanvasRaster';
import {
  computeSidebarPointerLookTarget,
  deriveSidebarCharacterState,
} from '../utils/sidebarCharacterState';
import {
  buildSidebarSessionEntries,
  filterSidebarSessionEntries,
  type SidebarScopeTab,
} from '../utils/sidebarSessions';
import {
  formatElapsedDuration,
  summarizeChannelTaskActivity,
} from '../utils/taskInsights';
import { AsciiPortraitCanvas } from './AsciiPortraitCanvas';
import { StaggerItem, StatusPulse, getStaggerVars } from './ui';

interface SidebarWorkspaceProps {
  channels: Channel[];
  activeChannel: ChannelId;
  onSelectChannel: (id: ChannelId) => void;
  canvasSessions?: CanvasSession[];
  hermesSessions?: HermesAgentSession[];
  thinkingByChannel?: Record<string, boolean>;
  onSelectSession?: (session: CanvasSession) => void;
  onPinToCanvas?: (session: CanvasSession) => void;
  approvals: ApprovalCard[];
  brief: BriefDecision[];
  tasks: BackgroundTask[];
  lastSpokenLine: string;
  mood: ExpressionMood;
  frequencyBands: number[];
  isSpeaking: boolean;
  isListening: boolean;
  inputLevel?: number;
  outputLevel?: number;
  onCycleMood?: () => void;
  portraitDotProfile?: BrailleDotProfile;
  portraitBloom?: number;
  animationSpeed?: number;
  reducedMotion?: boolean;
  portraitTintHex?: string | null;
  selectedItemId: string | null;
  onSelectApproval: (card: ApprovalCard) => void;
  onSelectBrief: (item: BriefDecision) => void;
  onNewTask: () => void;
  onOpenQueue: () => void;
  onOpenCanvas?: () => void;
  onOpenQuickJump?: () => void;
  onOpenStudio?: () => void;
  onCollapseSidebar?: () => void;
  onCloseMobile?: () => void;
  workspaceTitle?: string;
  gatewayStatus?: HermesGatewayStatus | null;
  homeSessionDraft?: boolean;
  queueVisible?: boolean;
  channelsVisible?: boolean;
  sidebarPosition?: 'left' | 'right';
}

export function SidebarWorkspace({
  channels,
  activeChannel,
  onSelectChannel,
  canvasSessions = [],
  hermesSessions = [],
  thinkingByChannel = {},
  onSelectSession,
  onPinToCanvas,
  approvals,
  brief,
  tasks,
  lastSpokenLine,
  mood,
  frequencyBands,
  isSpeaking,
  isListening,
  inputLevel = 0,
  outputLevel = 0,
  onCycleMood,
  portraitDotProfile = 'balanced',
  portraitBloom = 0.75,
  animationSpeed = 1,
  reducedMotion = false,
  portraitTintHex = null,
  selectedItemId,
  onSelectApproval,
  onSelectBrief,
  onNewTask,
  onOpenQueue,
  onOpenCanvas,
  onOpenQuickJump,
  onOpenStudio,
  onCollapseSidebar,
  onCloseMobile,
  workspaceTitle = 'Hermes',
  gatewayStatus,
  homeSessionDraft = false,
  queueVisible = true,
}: SidebarWorkspaceProps) {
  const pendingApprovals = approvals.filter((a) => a.status === 'pending');
  const openBrief = brief.filter((b) => !b.resolved).slice(0, 2);
  const activeChannelBtnRef = useRef<HTMLButtonElement | null>(null);
  const characterCardRef = useRef<HTMLDivElement | null>(null);
  const lookTargetRef = useRef<LookTarget | null>(null);

  const [scopeTab, setScopeTab] = useState<SidebarScopeTab>('workspace');
  const [searchQuery, setSearchQuery] = useState('');
  const [nowMs, setNowMs] = useState(() => Date.now());
  const [speakTick, setSpeakTick] = useState(0);

  useEffect(() => {
    if (!isSpeaking) return;
    const id = window.setInterval(() => {
      setSpeakTick((t) => (t + 1) % 10000);
    }, 42);
    return () => window.clearInterval(id);
  }, [isSpeaking]);

  const charState = useMemo(
    () =>
      deriveSidebarCharacterState({
        tasks,
        approvals,
        thinkingByChannel,
        hermesSessions,
        activeChannel,
        homeSessionDraft,
        baseMood: mood,
        isSpeaking,
        isListening,
        inputLevel,
        outputLevel,
        frequencyBands,
        tick: speakTick,
        gatewayStatus,
        lastSpokenLine,
      }),
    [
      tasks,
      approvals,
      thinkingByChannel,
      hermesSessions,
      activeChannel,
      homeSessionDraft,
      mood,
      isSpeaking,
      isListening,
      inputLevel,
      outputLevel,
      frequencyBands,
      speakTick,
      gatewayStatus,
      lastSpokenLine,
    ]
  );

  const hasActiveClock = Boolean(charState.activeTask?.createdAtMs);
  useEffect(() => {
    setNowMs(Date.now());
    const id = window.setInterval(
      () => setNowMs(Date.now()),
      hasActiveClock ? 1000 : 60_000
    );
    return () => window.clearInterval(id);
  }, [hasActiveClock]);

  const activityByChannel = useMemo(
    () => summarizeChannelTaskActivity(tasks, channels, thinkingByChannel),
    [tasks, channels, thinkingByChannel]
  );

  const sidebarEntries = useMemo(
    () =>
      buildSidebarSessionEntries({
        canvasSessions,
        hermesSessions,
        activityByChannel,
        nowMs,
      }),
    [canvasSessions, hermesSessions, activityByChannel, nowMs]
  );

  const allCounts = useMemo(
    () =>
      filterSidebarSessionEntries(sidebarEntries, {
        tab: 'all',
        query: '',
      }),
    [sidebarEntries]
  );

  const effectiveScopeTab: SidebarScopeTab =
    scopeTab === 'external' && allCounts.externalCount === 0 ? 'workspace' : scopeTab;

  const {
    visible: visibleSessions,
    workspaceCount,
    externalCount,
    totalCount: scopedCount,
  } = useMemo(
    () =>
      filterSidebarSessionEntries(sidebarEntries, {
        tab: 'all',
        query: searchQuery,
        originFilter: effectiveScopeTab,
      }),
    [sidebarEntries, effectiveScopeTab, searchQuery]
  );

  useEffect(() => {
    activeChannelBtnRef.current?.scrollIntoView({ block: 'nearest', inline: 'nearest' });
  }, [activeChannel, channels.length]);

  const showScopeTabs = externalCount > 0;
  const showSearchBar = scopedCount >= 8 || searchQuery.trim().length > 0;

  const handleCardPointerMove = (e: React.PointerEvent<HTMLDivElement>) => {
    const el = characterCardRef.current ?? e.currentTarget;
    const rect = el.getBoundingClientRect();
    lookTargetRef.current = computeSidebarPointerLookTarget(
      rect,
      e.clientX,
      e.clientY,
      46
    );
  };

  const handleCardPointerLeave = () => {
    lookTargetRef.current = null;
  };

  const cardSurfaceClass =
    charState.mode === 'speaking' || charState.mode === 'listening'
      ? 'border-[rgba(var(--ui-accent-rgb),0.40)] bg-[rgba(var(--ui-accent-rgb),0.06)]'
      : charState.mode === 'working'
      ? 'border-amber-400/35 bg-amber-500/[0.05]'
      : charState.mode === 'attention'
      ? 'border-amber-300/40 bg-amber-400/[0.05]'
      : 'border-white/[0.09] bg-white/[0.025] hover:border-white/[0.18]';

  const statusToneClass =
    charState.tone === 'amber'
      ? 'text-amber-300'
      : charState.tone === 'rose'
      ? 'text-rose-300/90'
      : 'text-[var(--ui-accent-soft)]';

  const avatarHaloClass =
    charState.mode === 'speaking' || charState.mode === 'listening' || charState.mode === 'working'
      ? 'ui-halo-pulse opacity-100'
      : charState.mode === 'offline'
      ? 'opacity-45'
      : 'ui-halo-idle opacity-85';

  const avatarHaloGradient =
    charState.tone === 'amber'
      ? 'radial-gradient(circle at 50% 48%, rgba(245, 158, 11, 0.28) 0%, rgba(245, 158, 11, 0.08) 54%, transparent 78%)'
      : charState.tone === 'rose'
      ? 'radial-gradient(circle at 50% 48%, rgba(244, 63, 94, 0.20) 0%, rgba(244, 63, 94, 0.05) 54%, transparent 78%)'
      : 'radial-gradient(circle at 50% 48%, rgba(var(--ui-accent-rgb), 0.24) 0%, rgba(var(--ui-accent-rgb), 0.06) 54%, transparent 78%)';

  const effectivePortraitTint =
    portraitTintHex ?? (charState.mode === 'offline' ? '#94A3B8' : null);

  return (
    <aside
      aria-label="Hermes Sessions and Status"
      data-testid="sidebar-workspace"
      className="ui-sidebar-shell flex flex-col h-full rounded-2xl border overflow-hidden select-none"
    >
      {/* Clean Minimal Header */}
      <div className="flex items-center justify-between px-4 pt-3.5 pb-2.5">
        <div className="flex items-center gap-2 min-w-0">
          {onCloseMobile && (
            <button
              type="button"
              onClick={onCloseMobile}
              aria-label="Back to workspace"
              className="lg:hidden min-w-[36px] min-h-[36px] flex items-center justify-center -ml-1 rounded-lg text-zinc-300 hover:text-white hover:bg-white/8 transition-colors cursor-pointer"
            >
              <ArrowLeft className="w-4 h-4" />
            </button>
          )}
          <span
            data-testid="sidebar-workspace-title"
            className="font-mono text-xs font-semibold tracking-tight text-white/90 truncate"
          >
            {workspaceTitle}
          </span>
        </div>

        <div className="flex items-center gap-1 shrink-0">
          <button
            type="button"
            onClick={onNewTask}
            aria-pressed={homeSessionDraft}
            className={`ui-tactile flex items-center gap-1 text-xs font-medium transition-colors cursor-pointer whitespace-nowrap px-2 py-1 rounded-md shrink-0 ${
              homeSessionDraft
                ? 'bg-[rgba(var(--ui-accent-rgb),0.16)] text-white border border-[rgba(var(--ui-accent-rgb),0.36)]'
                : 'text-zinc-200 hover:text-white hover:bg-white/[0.06]'
            }`}
          >
            <Plus className="w-3.5 h-3.5" />
            <span>New</span>
          </button>

          {onCollapseSidebar && (
            <button
              type="button"
              onClick={onCollapseSidebar}
              data-testid="sidebar-collapse-btn"
              aria-label="Hide sidebar"
              title="Hide sidebar (⌘B)"
              className="hidden lg:inline-flex w-7 h-7 items-center justify-center rounded-md text-zinc-400 hover:text-white hover:bg-white/[0.06] transition-colors cursor-pointer shrink-0"
            >
              <PanelLeftClose className="w-3.5 h-3.5" />
            </button>
          )}
        </div>
      </div>

      {/* Reactive Agent Companion & Task Status Card */}
      <div className="px-3">
        <div
          ref={characterCardRef}
          data-testid="sidebar-character-card"
          data-character-mode={charState.mode}
          data-running-tasks={String(charState.runningTaskCount)}
          onPointerMove={handleCardPointerMove}
          onPointerLeave={handleCardPointerLeave}
          className={`rounded-xl border p-2.5 transition-colors duration-200 overflow-hidden ${cardSurfaceClass}`}
        >
          <div className="flex items-center gap-2">
            <div className="relative rounded-lg shrink-0">
              <div
                aria-hidden="true"
                className={`pointer-events-none absolute -inset-2.5 rounded-full ${avatarHaloClass}`}
                style={{ background: avatarHaloGradient }}
              />
              <div className={`relative z-10 transition-opacity duration-200 ${charState.mode === 'offline' ? 'opacity-75' : 'opacity-100'}`}>
                <AsciiPortraitCanvas
                  variant="sidebar"
                  mood={charState.mood}
                  viseme={charState.viseme}
                  eyeState={charState.eyeState}
                  energy={charState.energy}
                  isSpeaking={isSpeaking}
                  isListening={isListening}
                  lookTargetRef={lookTargetRef}
                  tintHex={effectivePortraitTint}
                  dotProfile={portraitDotProfile}
                  bloom={portraitBloom}
                  speedScale={animationSpeed}
                  reducedMotionOverride={reducedMotion}
                  onClick={onCycleMood}
                />
              </div>
            </div>

            <button
              type="button"
              onClick={onOpenQueue}
              title="Open Hermes Agent Operations"
              className="ui-tactile min-w-0 flex-1 text-left rounded-lg pl-0.5 pr-1 py-1 hover:bg-white/[0.04] transition-colors cursor-pointer"
            >
              <div className="flex items-center justify-between gap-1.5 min-w-0">
                <span className="text-[12.5px] font-semibold text-white truncate min-w-0">
                  Hermes Agent
                </span>
                <span
                  data-testid="sidebar-active-lane"
                  className={`inline-flex items-center gap-1 font-mono text-[10px] tabular-nums whitespace-nowrap shrink-0 ${statusToneClass}`}
                >
                  <StatusPulse state={charState.pulseState} size="sm" />
                  <span>{charState.statusLabel}</span>
                </span>
              </div>

              <div className="flex items-center justify-between gap-1 mt-0.5 min-w-0">
                <p className="text-[11px] text-zinc-400 truncate">
                  {charState.gatewayLine
                    .replace('Gateway offline · Open Ops', 'Open Queue & Gateway')
                    .replace('Open Ops', 'Open Queue')}
                </p>
                <ChevronRight className="w-3 h-3 text-zinc-500 shrink-0" aria-hidden="true" />
              </div>

              {(isSpeaking || isListening || (charState.mode === 'working' && charState.activeTask)) && (
                <div className="mt-1 flex items-center justify-between gap-2 min-w-0">
                  {isSpeaking || isListening ? (
                    <>
                      <span className="font-mono text-[10px] text-[var(--ui-accent-soft)] truncate">
                        {isListening ? 'mic active' : 'voice live'}
                        {charState.runningTaskCount > 0 ? ` · ${charState.runningTaskCount} active` : ''}
                      </span>
                      <span className="flex items-end gap-0.5 h-2.5 shrink-0" aria-hidden="true">
                        {frequencyBands.slice(0, 6).map((band, idx) => (
                          <span
                            key={idx}
                            className="w-0.5 h-full origin-bottom rounded-[1px] bg-[var(--ui-accent-bright)] transition-transform duration-75"
                            style={{ transform: `scaleY(${Math.max(0.24, Math.min(1, band))})` }}
                          />
                        ))}
                      </span>
                    </>
                  ) : (
                    <span className="font-mono text-[10px] text-amber-200/85 tabular-nums truncate">
                      {charState.activeTask?.createdAtMs
                        ? `${formatElapsedDuration(charState.activeTask.createdAtMs, nowMs)} elapsed`
                        : 'active execution'}
                      {charState.runningTaskCount > 1
                        ? ` · ${charState.runningTaskCount} active`
                        : ''}
                    </span>
                  )}
                </div>
              )}
            </button>
          </div>

          {charState.activeTask ? (
            <button
              type="button"
              data-testid="sidebar-active-task-strip"
              onClick={() => onSelectChannel(charState.activeTask!.channelId)}
              title={`Open ${charState.activeTask.title}`}
              className="ui-tactile w-full text-left mt-2 rounded-lg border border-amber-400/18 bg-amber-500/[0.06] px-2.5 py-2 hover:border-amber-400/32 transition-colors cursor-pointer"
            >
              <div className="flex items-center justify-between gap-2">
                <span className="text-[11.5px] font-medium text-white/95 truncate">
                  {charState.activeTask.title}
                </span>
                <span className="font-mono text-[10px] text-amber-300 tabular-nums shrink-0">
                  {charState.activeTask.ratioLabel}
                </span>
              </div>
              <p className="font-mono text-[10px] text-amber-200/82 truncate mt-0.5">
                {charState.activeTask.stepText}
              </p>
            </button>
          ) : (
            charState.topApproval && (
              <button
                type="button"
                data-testid="sidebar-approval-strip"
                onClick={onOpenQueue}
                title={`Review ${charState.topApproval.title}`}
                className="ui-tactile w-full text-left mt-2 rounded-lg border border-amber-400/22 bg-amber-400/[0.06] px-2.5 py-1.5 hover:border-amber-400/38 transition-colors cursor-pointer"
              >
                <div className="flex items-center justify-between gap-2 font-mono text-[10px] text-amber-200/90 tabular-nums">
                  <span className="truncate">
                    {queueVisible
                      ? `Review in Queue · #${charState.topApproval.channelId}`
                      : charState.topApproval.title}
                  </span>
                  <span className="text-amber-300 shrink-0">Open →</span>
                </div>
              </button>
            )
          )}
        </div>
      </div>

      {/* Needs Attention (Pinned above sessions so approval gates are never buried) */}
      {queueVisible && (pendingApprovals.length > 0 || openBrief.length > 0) && (
        <div className="px-3 pt-3 pb-1 space-y-1 border-b border-white/[0.06]">
          <div className="px-2 pb-0.5 flex items-center justify-between font-mono text-[10px] uppercase tracking-[0.14em] text-amber-200/75 tabular-nums">
            <span>Needs attention</span>
            <span>{pendingApprovals.length + openBrief.length}</span>
          </div>

          {pendingApprovals.map((card, idx) => {
            const active = selectedItemId === card.id;
            return (
              <button
                key={card.id}
                type="button"
                style={getStaggerVars(idx)}
                onClick={() => onSelectApproval(card)}
                className={`ui-stagger-enter ui-tactile w-full text-left flex items-start gap-2.5 px-2.5 py-2 rounded-xl border transition-colors duration-150 cursor-pointer ${
                  active
                    ? 'bg-white/[0.08] border-[rgba(var(--ui-accent-rgb),0.36)]'
                    : 'border-transparent hover:bg-white/[0.04]'
                }`}
              >
                <StatusPulse state="waiting" className="mt-1.5" />
                <div className="min-w-0 flex-1">
                  <div className="text-[12.5px] font-medium text-white truncate">{card.title}</div>
                  <div className="text-[10.5px] font-mono text-amber-200/75 truncate mt-0.5">
                    Approval · #{card.channelId}
                  </div>
                </div>
              </button>
            );
          })}

          {openBrief.map((item, idx) => {
            const active = selectedItemId === item.id;
            return (
              <button
                key={item.id}
                type="button"
                style={getStaggerVars(idx + pendingApprovals.length)}
                onClick={() => onSelectBrief(item)}
                className={`ui-stagger-enter ui-tactile w-full text-left flex items-start gap-2.5 px-2.5 py-2 rounded-xl border transition-colors duration-150 cursor-pointer ${
                  active
                    ? 'bg-white/[0.08] border-[rgba(var(--ui-accent-rgb),0.36)]'
                    : 'border-transparent hover:bg-white/[0.04]'
                }`}
              >
                <StatusPulse state="idle" className="mt-1.5" />
                <div className="min-w-0 flex-1">
                  <div className="text-[12.5px] text-zinc-200 truncate">{item.title}</div>
                  <div className="text-[10.5px] font-mono text-zinc-400 truncate mt-0.5">Brief</div>
                </div>
              </button>
            );
          })}
        </div>
      )}

      {/* Sessions Header: Workspace Sessions vs optional Gateway History */}
      {queueVisible && (workspaceCount > 0 || externalCount > 0) && (
        <div className="px-3 pt-2.5 pb-1 space-y-2 shrink-0">
          {showScopeTabs ? (
            <div
              role="tablist"
              aria-label="Session scope"
              className="flex items-center justify-between gap-2 px-1 border-b border-white/[0.07]"
            >
              <div className="flex items-center gap-3">
                {(
                  [
                    { id: 'workspace', label: 'Sessions', count: workspaceCount },
                    { id: 'external', label: 'Gateway', count: externalCount },
                  ] as const
                ).map((tab) => {
                  const selected = effectiveScopeTab === tab.id;
                  return (
                    <button
                      key={tab.id}
                      type="button"
                      role="tab"
                      aria-selected={selected}
                      data-testid={`sidebar-tab-${tab.id}`}
                      onClick={() => {
                        setScopeTab(tab.id);
                        setSearchQuery('');
                      }}
                      className={`pb-1.5 text-[11px] font-medium transition-colors cursor-pointer inline-flex items-baseline gap-1 border-b-2 -mb-px ${
                        selected
                          ? 'text-white border-[var(--ui-accent)]'
                          : 'text-zinc-400 border-transparent hover:text-zinc-200'
                      }`}
                    >
                      <span>{tab.label}</span>
                      <span
                        className={`font-mono text-[10px] tabular-nums ${
                          selected ? 'text-[var(--ui-accent-soft)]' : 'text-zinc-500'
                        }`}
                      >
                        {tab.count}
                      </span>
                    </button>
                  );
                })}
              </div>
            </div>
          ) : (
            <div className="px-2 flex items-center justify-between font-mono text-[10px] uppercase tracking-[0.14em] text-zinc-400 tabular-nums">
              <span>Sessions</span>
              <span>{workspaceCount}</span>
            </div>
          )}

          {showSearchBar && (
            <div className="relative">
              <Search
                aria-hidden="true"
                className="w-3.5 h-3.5 text-zinc-500 pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2"
              />
              <input
                type="text"
                role="searchbox"
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                placeholder={
                  effectiveScopeTab === 'external'
                    ? 'Search gateway history…'
                    : 'Filter sessions…'
                }
                aria-label="Filter sessions"
                data-testid="sidebar-session-search"
                className="w-full rounded-lg bg-white/[0.03] border border-white/[0.08] focus:border-[var(--ui-accent)] focus:outline-none pl-8 pr-7 py-1.5 text-[11.5px] text-white/90 placeholder:text-zinc-500 transition-colors"
              />
              {searchQuery && (
                <button
                  type="button"
                  onClick={() => setSearchQuery('')}
                  aria-label="Clear session filter"
                  className="absolute right-1.5 top-1/2 -translate-y-1/2 p-1 rounded text-zinc-400 hover:text-white transition-colors cursor-pointer"
                >
                  <X className="w-3 h-3" />
                </button>
              )}
            </div>
          )}
        </div>
      )}

      {/* Scannable Session List */}
      <div className="flex-1 overflow-y-auto cyber-scroll px-3 py-1.5 space-y-1">
        {!queueVisible ? (
          <div className="px-2.5 py-6 text-center text-[11px] text-zinc-500">
            Queue hidden in UI layout
          </div>
        ) : effectiveScopeTab === 'workspace' && workspaceCount === 0 && pendingApprovals.length === 0 && openBrief.length === 0 ? (
          <div className="px-2 py-4 text-left space-y-3">
            <div className="space-y-1 px-0.5">
              <p className="text-[12px] font-medium text-zinc-200">No sessions yet</p>
              <p className="text-[11px] text-zinc-400 leading-relaxed">
                {externalCount > 0
                  ? 'Start a prompt below or browse external Gateway runs.'
                  : 'Start a conversation or map parallel agents on Canvas.'}
              </p>
            </div>
            <div className="space-y-1.5">
              <button
                type="button"
                data-testid="sidebar-empty-new-prompt"
                onClick={onNewTask}
                className="ui-tactile w-full flex items-center gap-2 px-2.5 py-2 rounded-lg border border-white/[0.09] bg-white/[0.03] hover:bg-white/[0.06] hover:border-white/20 text-xs font-medium text-zinc-100 transition-colors cursor-pointer"
              >
                <MessageSquarePlus className="w-3.5 h-3.5 text-[var(--ui-accent-soft)] shrink-0" />
                <span className="truncate">Start on Home</span>
              </button>
              {onOpenCanvas && (
                <button
                  type="button"
                  data-testid="sidebar-empty-open-canvas"
                  onClick={onOpenCanvas}
                  className="ui-tactile w-full flex items-center gap-2 px-2.5 py-2 rounded-lg border border-white/[0.08] bg-transparent hover:bg-white/[0.05] hover:border-white/18 text-xs font-medium text-zinc-300 hover:text-white transition-colors cursor-pointer"
                >
                  <LayoutGrid className="w-3.5 h-3.5 text-zinc-400 shrink-0" />
                  <span className="truncate">Open Canvas</span>
                </button>
              )}
              {externalCount > 0 && (
                <button
                  type="button"
                  onClick={() => setScopeTab('external')}
                  className="px-1 pt-0.5 text-[11px] font-mono text-[var(--ui-accent-soft)] hover:text-white underline underline-offset-4 cursor-pointer"
                >
                  Browse {externalCount} gateway {externalCount === 1 ? 'session' : 'sessions'}
                </button>
              )}
            </div>
          </div>
        ) : visibleSessions.length === 0 ? (
          <div className="px-2.5 py-5 text-left space-y-2">
            <p className="text-[12px] font-medium text-zinc-300">
              {searchQuery.trim()
                ? 'No matching sessions'
                : 'No external gateway sessions'}
            </p>
            {searchQuery.trim() && (
              <button
                type="button"
                onClick={() => setSearchQuery('')}
                className="text-[11px] font-mono text-zinc-400 hover:text-white underline underline-offset-4 cursor-pointer"
              >
                Clear filter
              </button>
            )}
          </div>
        ) : (
          <>
            {effectiveScopeTab === 'external' && (
              <div className="px-2 pb-1 text-[10.5px] text-zinc-400">
                External CLI &amp; API history · click to read or pin to Canvas
              </div>
            )}
            {visibleSessions.map((entry, idx) => {
              const { session } = entry;
              const active =
                !homeSessionDraft &&
                (selectedItemId === session.id || activeChannel === session.channelId);

              const rowContent = (
                <div
                  className={`group/row flex items-center gap-1 rounded-xl border transition-colors duration-150 ${
                    active
                      ? 'bg-[rgba(var(--ui-accent-rgb),0.11)] border-[rgba(var(--ui-accent-rgb),0.36)]'
                      : 'border-transparent hover:bg-white/[0.04]'
                  }`}
                >
                  <button
                    type="button"
                    ref={active ? activeChannelBtnRef : undefined}
                    data-testid={`sidebar-session-${session.channelId}`}
                    data-has-task={String(entry.hasActiveWork)}
                    data-origin={entry.origin}
                    title={entry.tooltipTitle}
                    onClick={() =>
                      onSelectSession
                        ? onSelectSession(session)
                        : onSelectChannel(session.channelId)
                    }
                    className="ui-tactile flex-1 min-w-0 text-left flex items-start gap-2.5 px-2.5 py-2 cursor-pointer"
                  >
                    <StatusPulse
                      state={entry.hasActiveWork ? 'thinking' : active ? 'live' : 'idle'}
                      size="sm"
                      className="mt-1.5"
                    />
                    <div className="min-w-0 flex-1">
                      <div className="flex items-center justify-between gap-2 min-w-0">
                        <span
                          className={`text-[12.5px] font-medium truncate min-w-0 flex-1 ${
                            active
                              ? 'text-white'
                              : entry.isOpaqueId
                              ? 'text-zinc-300'
                              : 'text-zinc-100'
                          }`}
                        >
                          {entry.displayTitle}
                        </span>
                        {(entry.hasActiveWork || entry.relativeTime) && (
                          <span
                            className={`font-mono text-[10px] tabular-nums shrink-0 ${
                              entry.hasActiveWork
                                ? 'text-amber-300/90 font-medium'
                                : active
                                ? 'text-[var(--ui-accent-soft)]'
                                : 'text-zinc-400'
                            }`}
                          >
                            {entry.hasActiveWork
                              ? entry.runningCount > 1
                                ? `${entry.runningCount} active`
                                : 'working'
                              : entry.relativeTime}
                          </span>
                        )}
                      </div>
                      <div
                        className={`font-mono text-[10.5px] truncate mt-0.5 ${
                          entry.hasActiveWork
                            ? 'text-amber-200/75'
                            : active
                            ? 'text-zinc-300'
                            : 'text-zinc-400'
                        }`}
                      >
                        {entry.metadataLine}
                      </div>
                    </div>
                  </button>

                  {effectiveScopeTab === 'external' && !entry.onCanvas && onPinToCanvas && (
                    <button
                      type="button"
                      data-testid={`sidebar-pin-${session.channelId}`}
                      onClick={() => onPinToCanvas(session)}
                      aria-label={`Pin ${entry.displayTitle} to Canvas`}
                      title="Pin to Canvas"
                      className="mr-1.5 p-1.5 rounded-lg text-zinc-400 hover:text-[var(--ui-accent-soft)] hover:bg-white/[0.08] opacity-70 sm:opacity-0 sm:group-hover/row:opacity-100 focus:opacity-100 transition-opacity cursor-pointer shrink-0"
                    >
                      <Pin className="w-3.5 h-3.5" />
                    </button>
                  )}
                </div>
              );

              if (idx < 10) {
                return (
                  <StaggerItem key={session.id} index={idx}>
                    {rowContent}
                  </StaggerItem>
                );
              }
              return <div key={session.id}>{rowContent}</div>;
            })}
          </>
        )}
      </div>

      {/* Quiet Navigation Footer: Quick Jump & Settings */}
      {(onOpenQuickJump || onOpenStudio) && (
        <div className="px-3 py-2.5 border-t border-white/[0.07] flex items-center gap-1.5 shrink-0">
          {onOpenQuickJump && (
            <button
              type="button"
              data-testid="sidebar-quick-jump-btn"
              onClick={onOpenQuickJump}
              className="ui-tactile flex-1 min-w-0 h-8 px-2.5 rounded-lg border border-white/[0.08] bg-white/[0.025] hover:bg-white/[0.06] hover:border-white/20 flex items-center justify-between gap-2 text-xs text-zinc-300 hover:text-white transition-colors cursor-pointer"
            >
              <span className="inline-flex items-center gap-1.5 min-w-0 truncate">
                <Search className="w-3.5 h-3.5 text-zinc-400 shrink-0" />
                <span className="truncate">Quick jump</span>
              </span>
              <kbd className="font-mono text-[10px] text-zinc-400 shrink-0">⌘K</kbd>
            </button>
          )}
          {onOpenStudio && (
            <button
              type="button"
              onClick={onOpenStudio}
              aria-label="Open Settings"
              title="Settings (⌘,)"
              className="ui-tactile w-8 h-8 rounded-lg border border-white/[0.08] bg-white/[0.025] hover:bg-white/[0.06] hover:border-white/20 text-zinc-300 hover:text-white flex items-center justify-center transition-colors cursor-pointer shrink-0"
            >
              <Settings className="w-3.5 h-3.5" />
            </button>
          )}
        </div>
      )}
    </aside>
  );
}

