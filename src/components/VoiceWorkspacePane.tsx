import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import {
  PanelLeft,
  Mic,
  MicOff,
  Square,
  Volume2,
  VolumeX,
  ArrowUp,
  Brain,
  Check,
  X,
  RotateCcw,
  Terminal,
  ListChecks,
  LayoutGrid,
  RefreshCw,
  Settings,
  Plus,
  Copy,
  MoreHorizontal,
  ArrowUpRight,
  Slash,
  Search,
  ChevronUp,
  ChevronDown,
  Minimize2,
  Maximize2,
  Pencil,
  CornerDownRight,
  Home,
  MessageSquare,
  Activity,
} from 'lucide-react';
import type {
  ApprovalCard,
  BackgroundTask,
  BriefDecision,
  Channel,
  ChannelId,
  ExpressionMood,
  EyeState,
  HermesAgentSession,
  HermesCronJob,
  HermesGatewayStatus,
  HermesProfileMode,
  HermesProfileSummary,
  HermesSkill,
  HermesToolset,
  HermesTransportMode,
  TerminalEntry,
  VoiceState,
} from '../types/hermes';
import { amplitudeToViseme } from '../utils/asciiCharacter';
import type { LookTarget } from '../utils/asciiPortraitEngine';
import type { PortraitAssetStatus } from '../utils/portraitLoadingEngine';
import {
  formatCanvasDotLabel,
  type CanvasFormationPreset,
  type CanvasPin,
  type CanvasSession,
} from '../utils/canvasSessions';
import {
  CURATED_HOME_STARTERS,
  resolveComposerPlaceholder,
} from '../utils/homeSession';
import {
  filterSlashCommands,
  resolvePaletteSelection,
  shouldShowSlashPalette,
  type SlashCommandSpec,
} from '../utils/slashPalette';
import { groupThreadTimeline, isTerminalDiagnosticActivity } from '../utils/threadTrace';
import {
  determineActiveMinimapTurn,
  extractThreadMinimapTurns,
} from '../utils/threadMinimap';
import {
  findMatchingThreadEntryIds,
  findPrecedingUserPrompt,
  formatThreadTranscriptMarkdown,
  stepMatchIndex,
} from '../utils/threadSearch';
import { computeThreadScrollPin } from '../utils/threadScroll';
import { shouldShowScrollTopFade } from '../utils/motionChoreography';
import { partitionChannelTasks } from '../utils/taskInsights';
import {
  resolveStageHeaderSize,
  resolveThreadTasksLayoutMode,
} from '../utils/responsiveLayout';
import { DEFAULT_UI_CONFIG, type UiCustomizationConfig } from '../utils/uiCustomization';
import type { CreateDotSessionPayload } from './AddSessionShapeModal';
import { AsciiPortraitCanvas } from './AsciiPortraitCanvas';
import { BrailleHorizonCanvas } from './BrailleHorizonCanvas';
import { BrailleSessionsCanvas } from './BrailleSessionsCanvas';
import { ContextWindowMonitor } from './ContextWindowMonitor';
import { DecodedText } from './DecodedText';
import { HermesOperationsPanel } from './HermesOperationsPanel';
import { InlineHermesPopover } from './InlineHermesPopover';
import { JevRecallMonitor } from './JevRecallMonitor';
import { useModelContextInfo } from '../hooks/useModelContextInfo';
import { formatCompactTokens } from '../utils/modelContextEngine';
import { CommandTerminal } from './CommandTerminal';
import { MobileWorkspaceBar } from './MobileWorkspaceBar';
import { SlashCommandPalette } from './SlashCommandPalette';
import { ThreadMessageContent, renderHighlightedText } from './ThreadMessageContent';
import { ThreadTaskBanner } from './ThreadTaskBanner';
import { ThreadTraceTree } from './ThreadTraceTree';
import { ThreadTurnMinimap } from './ThreadTurnMinimap';
import {
  MorphTabs,
  ScrollToBottomButton,
  StatusPulse,
  type MorphTabItem,
  type StatusPulseState,
} from './ui';

export type WorkspacePanelView = 'home' | 'canvas' | 'voice' | 'queue' | 'cli';

interface VoiceWorkspacePaneProps {
  channels: Channel[];
  activeChannel: ChannelId;
  onSelectChannel: (id: ChannelId) => void;
  mood: ExpressionMood;
  onCycleMood: () => void;
  panelView: WorkspacePanelView;
  onChangePanelView: (view: WorkspacePanelView) => void;
  onOpenMobileSidebar?: () => void;
  onToggleSidebar?: () => void;
  onOpenQuickJump?: () => void;
  voiceState: VoiceState;
  lastSpokenLine: string;
  animateSpeechLine: boolean;
  entries: TerminalEntry[];
  approvals: ApprovalCard[];
  brief: BriefDecision[];
  tasks: BackgroundTask[];
  gatewayStatus?: HermesGatewayStatus | null;
  onRefreshGateway?: () => Promise<unknown> | void;
  skills?: HermesSkill[];
  toolsets?: HermesToolset[];
  cronJobs?: HermesCronJob[];
  transportMode?: HermesTransportMode;
  onChangeTransportMode?: (mode: HermesTransportMode) => void;
  onCreateJob?: (payload: { prompt: string; schedule: string; name?: string }) => Promise<unknown>;
  onPauseJob?: (jobId: string) => Promise<void>;
  onResumeJob?: (jobId: string) => Promise<void>;
  onRunJob?: (jobId: string) => Promise<void>;
  onDeleteJob?: (jobId: string) => Promise<void>;
  onApproveCard: (id: string, choice?: 'once' | 'always' | 'deny') => void;
  onRejectCard: (id: string) => void;
  onClarifyCard?: (id: string, answer: string) => void;
  onRunBriefCommand: (command: string, briefId: string) => void;
  onToggleTaskStatus: (id: string) => void;
  onConfirmTask?: (id: string) => void;
  onReopenTask?: (id: string) => void;
  onCancelTask?: (id: string) => void;
  onClearCompletedTasks?: (channelId: ChannelId) => void;
  onSteerTask?: (id: string, text: string) => Promise<void> | void;
  onToggleListening: () => void;
  onToggleMute: () => void;
  onStopSpeaking: () => void;
  onSubmitInput: (value: string, source: 'voice' | 'cli') => void;
  onSubmitSessionInput: (channelId: ChannelId, value: string, source: 'voice' | 'cli') => void;
  /** Whether the agent's live activity trace is shown in the transcript. */
  showThinking: boolean;
  onToggleThinking: () => void;
  canvasSessions: CanvasSession[];
  canvasPins: CanvasPin[];
  hermesSessions: HermesAgentSession[];
  thinkingByChannel: Record<string, boolean>;
  speakingChannel: ChannelId | null;
  onSelectCanvasSession: (session: CanvasSession) => void;
  onMoveCanvasSession: (id: string, xPct: number, yPct: number) => void;
  onAddCanvasSession: (payload: CreateDotSessionPayload) => void;
  onUpdateDotConfig?: (
    id: string,
    patch: Partial<
      Pick<CanvasSession, 'autonomyMode' | 'scheduleLabel' | 'autonomyPaused' | 'skills' | 'profile'>
    >
  ) => void;
  onAddCanvasPin: (pin: Omit<CanvasPin, 'id'>) => void;
  onMoveCanvasPin: (id: string, xPct: number, yPct: number) => void;
  onRemoveCanvasPin: (id: string) => void;
  onAutoArrangeCanvas: (formation?: CanvasFormationPreset) => void;
  onCycleSessionMood: (id: string) => void;
  onResetSession: (channelId: string) => void;
  onRemoveCanvasSession: (id: string) => void;
  onOpenSessionThread: (session: CanvasSession) => void;
  threadVisitNonce?: number;
  homeSessionDraft?: boolean;
  homeCommandOutput?: { command: string; output: string } | null;
  onDismissHomeCommandOutput?: () => void;
  onStartNewSession?: () => void;
  profiles?: HermesProfileSummary[];
  draftProfile?: string;
  onChangeDraftProfile?: (profileId: string) => void;
  onChangeSessionProfile?: (channelId: ChannelId, profileId: string) => void;
  onSaveProfile?: (payload: {
    id: string;
    label?: string;
    mode: HermesProfileMode;
    baseUrl?: string;
    apiKey?: string;
    description?: string;
  }) => Promise<unknown>;
  onDeleteProfile?: (profileId: string) => Promise<unknown>;
  uiConfig?: UiCustomizationConfig;
  onOpenStudio?: () => void;
  onToggleJevRecall?: () => void;
  updateAvailable?: boolean;
}

export function VoiceWorkspacePane({
  channels,
  activeChannel,
  onSelectChannel,
  mood,
  onCycleMood,
  panelView,
  onChangePanelView,
  onOpenMobileSidebar,
  onToggleSidebar,
  onOpenQuickJump,
  voiceState,
  lastSpokenLine,
  animateSpeechLine,
  entries,
  approvals,
  brief,
  tasks,
  gatewayStatus,
  onRefreshGateway,
  skills,
  toolsets,
  cronJobs,
  transportMode,
  onChangeTransportMode,
  onCreateJob,
  onPauseJob,
  onResumeJob,
  onRunJob,
  onDeleteJob,
  onApproveCard,
  onRejectCard,
  onClarifyCard,
  onRunBriefCommand,
  onToggleTaskStatus,
  onConfirmTask,
  onReopenTask,
  onCancelTask,
  onClearCompletedTasks,
  onSteerTask,
  onToggleListening,
  onToggleMute,
  onStopSpeaking,
  onSubmitInput,
  onSubmitSessionInput,
  showThinking,
  onToggleThinking,
  canvasSessions,
  canvasPins,
  hermesSessions,
  thinkingByChannel,
  speakingChannel,
  onSelectCanvasSession,
  onMoveCanvasSession,
  onAddCanvasSession,
  onUpdateDotConfig,
  onAddCanvasPin,
  onMoveCanvasPin,
  onRemoveCanvasPin,
  onAutoArrangeCanvas,
  onCycleSessionMood,
  onResetSession,
  onRemoveCanvasSession,
  onOpenSessionThread,
  threadVisitNonce = 0,
  homeSessionDraft = false,
  homeCommandOutput = null,
  onDismissHomeCommandOutput,
  onStartNewSession,
  profiles = [],
  draftProfile = 'default',
  onChangeDraftProfile,
  onChangeSessionProfile,
  onSaveProfile,
  onDeleteProfile,
  uiConfig = DEFAULT_UI_CONFIG,
  onOpenStudio,
  onToggleJevRecall,
  updateAvailable = false,
}: VoiceWorkspacePaneProps) {
  const [tick, setTick] = useState(0);
  const [input, setInput] = useState('');
  const [slashPaletteForced, setSlashPaletteForced] = useState(false);
  const [slashSelectedIndex, setSlashSelectedIndex] = useState(0);
  const [hermesInspectorOpen, setHermesInspectorOpen] = useState(false);
  const [contextMonitorOpen, setContextMonitorOpen] = useState(false);
  const [compactThreadHeader, setCompactThreadHeader] = useState(false);
  const [threadMoreMenuOpen, setThreadMoreMenuOpen] = useState(false);
  const [threadSearchOpen, setThreadSearchOpen] = useState(false);
  const [threadSearchQuery, setThreadSearchQuery] = useState('');
  const [threadSearchMatchIdx, setThreadSearchMatchIdx] = useState(0);
  const [copiedEntryId, setCopiedEntryId] = useState<string | null>(null);
  const [copiedSessionKey, setCopiedSessionKey] = useState(false);
  const [copiedTranscript, setCopiedTranscript] = useState(false);
  const [clarifyDraft, setClarifyDraft] = useState<Record<string, string>>({});
  const [historyIdx, setHistoryIdx] = useState(-1);
  const [workingSeconds, setWorkingSeconds] = useState(0);
  const [portraitStatus, setPortraitStatus] = useState<PortraitAssetStatus>('loading');
  const [isAwayFromBottom, setIsAwayFromBottom] = useState(false);
  const [showTopFade, setShowTopFade] = useState(false);
  const composerInputRef = useRef<HTMLTextAreaElement | null>(null);
  const threadSearchInputRef = useRef<HTMLInputElement | null>(null);
  const entryDomRefs = useRef<Map<string, HTMLDivElement>>(new Map());
  const [threadTasksOverrideByChannel, setThreadTasksOverrideByChannel] = useState<
    Record<string, boolean>
  >({});
  const [isDesktopLg, setIsDesktopLg] = useState<boolean>(() =>
    typeof window !== 'undefined' ? window.matchMedia('(min-width: 1024px)').matches : true
  );
  const [stageWidth, setStageWidth] = useState<number>(() =>
    typeof window !== 'undefined' ? Math.max(360, window.innerWidth - 340) : 1100
  );
  const [viewportHeight, setViewportHeight] = useState<number>(() =>
    typeof window !== 'undefined' ? window.innerHeight : 900
  );
  const stageRef = useRef<HTMLElement | null>(null);
  const [activeMinimapTurnId, setActiveMinimapTurnId] = useState<string | null>(null);
  const [highlightedTurnId, setHighlightedTurnId] = useState<string | null>(null);
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const threadListRef = useRef<HTMLDivElement | null>(null);
  const bottomAnchorRef = useRef<HTMLDivElement | null>(null);
  const turnAnchorRefs = useRef<Map<string, HTMLDivElement>>(new Map());
  const jumpHighlightTimerRef = useRef<number | null>(null);
  const isPinnedToBottomRef = useRef(true);
  const programmaticTargetTopRef = useRef<number | null>(null);
  const lastUserScrollAtRef = useRef(0);
  const lastScrollTopRef = useRef(0);
  const lastScrollVisitRef = useRef('');
  const portraitWrapRef = useRef<HTMLDivElement | null>(null);
  const lookTargetRef = useRef<LookTarget | null>(null);

  useEffect(() => {
    if (typeof window === 'undefined' || typeof window.matchMedia !== 'function') return;
    const mq = window.matchMedia('(min-width: 1024px)');
    const sync = () => {
      setIsDesktopLg(mq.matches);
      setViewportHeight(window.innerHeight);
    };
    sync();
    mq.addEventListener('change', sync);
    window.addEventListener('resize', sync);
    return () => {
      mq.removeEventListener('change', sync);
      window.removeEventListener('resize', sync);
    };
  }, []);

  useEffect(() => {
    const el = stageRef.current;
    if (!el || typeof ResizeObserver === 'undefined') return;
    const updateWidth = () => {
      const next = Math.round(el.getBoundingClientRect().width);
      if (next > 0) setStageWidth(next);
    };
    updateWidth();
    const ro = new ResizeObserver(updateWidth);
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const isHomeOverview = panelView === 'home';
  const isHomeNewDraft = isHomeOverview && homeSessionDraft;

  const activeCanvasSession = useMemo(
    () => canvasSessions.find((s) => s.channelId === activeChannel) ?? null,
    [canvasSessions, activeChannel]
  );
  const activeChannelObj = useMemo(
    () => channels.find((c) => c.id === activeChannel) ?? null,
    [channels, activeChannel]
  );
  const activeProfile =
    activeCanvasSession?.profile || activeChannelObj?.profile || gatewayStatus?.profile || 'default';
  const sessionCountByProfile = useMemo(() => {
    const counts: Record<string, number> = {};
    for (const ch of channels) {
      const pid = ch.profile || 'default';
      counts[pid] = (counts[pid] ?? 0) + 1;
    }
    return counts;
  }, [channels]);
  const activeHermesSession = useMemo(
    () => hermesSessions.find((s) => s.channelId === activeChannel) ?? null,
    [hermesSessions, activeChannel]
  );
  const activeChannelTasksPartition = useMemo(
    () => partitionChannelTasks(tasks, activeChannel),
    [tasks, activeChannel]
  );
  const activeHermesSessionId =
    activeCanvasSession?.hermesSessionId ?? (activeChannel ? `hermes-${activeChannel}` : 'new session');
  const rawIsThinking =
    Boolean(thinkingByChannel[activeChannel]) ||
    activeHermesSession?.status === 'thinking' ||
    activeHermesSession?.status === 'running' ||
    activeChannelTasksPartition.running.length > 0;
  const rawIsSpeaking =
    voiceState.isSpeaking && (speakingChannel ?? activeChannel) === activeChannel;
  const activeIsThinking = isHomeNewDraft ? false : rawIsThinking;
  const activeIsSpeaking = isHomeNewDraft ? false : rawIsSpeaking;

  const activeDotLabel = useMemo(() => {
    if (!activeChannel) return null;
    return formatCanvasDotLabel(
      activeCanvasSession ?? {
        channelId: activeChannel,
        hermesSessionId: activeHermesSessionId,
        name: activeChannel,
        topic: activeHermesSession?.topic ?? '',
      },
      activeHermesSession ?? undefined,
      0
    );
  }, [activeChannel, activeCanvasSession, activeHermesSession, activeHermesSessionId]);


  const threadTasksDockMode = resolveThreadTasksLayoutMode(stageWidth, isDesktopLg);
  const stageHeaderSize = resolveStageHeaderSize({
    panelView,
    viewportHeightPx: viewportHeight,
  });
  const threadTasksOpen =
    threadTasksOverrideByChannel[activeChannel] ??
    (threadTasksDockMode === 'split'
      ? activeChannelTasksPartition.all.length > 0 || activeIsThinking
      : false);

  const toggleThreadTasksOpen = () => {
    const current = threadTasksOpen;
    const nextOpen = !current;
    setThreadTasksOverrideByChannel((prev) => ({
      ...prev,
      [activeChannel]: nextOpen,
    }));
  };

  const hideThreadTasks = () => {
    setThreadTasksOverrideByChannel((prev) => ({
      ...prev,
      [activeChannel]: false,
    }));
  };

  useEffect(() => {
    if (!activeIsSpeaking) return;
    setTick(0);
    const id = window.setInterval(() => {
      setTick((t) => (t + 1) % 10000);
    }, 42);
    return () => window.clearInterval(id);
  }, [activeIsSpeaking]);

  useEffect(() => {
    if (!activeIsThinking && !activeIsSpeaking) {
      setWorkingSeconds(0);
      return;
    }
    const id = window.setInterval(() => {
      setWorkingSeconds((s) => s + 1);
    }, 1000);
    return () => window.clearInterval(id);
  }, [activeIsThinking, activeIsSpeaking]);

  const scopedEntries = useMemo(
    () => entries.filter((e) => e.channelId === activeChannel),
    [entries, activeChannel]
  );

  const modelContext = useModelContextInfo({
    activeModelId: gatewayStatus?.activeModel,
    activeProvider: gatewayStatus?.activeProvider,
    activeProfile,
    activeChannel,
    entries: scopedEntries,
    skillsCount: skills?.length ?? gatewayStatus?.skillsCount ?? 0,
    toolsetsCount: toolsets?.length ?? gatewayStatus?.toolsets?.length ?? 0,
  });

  useEffect(() => {
    if (uiConfig.showJevRecall) {
      setContextMonitorOpen(false);
    }
  }, [uiConfig.showJevRecall]);
  const lastScopedTextLen = scopedEntries[scopedEntries.length - 1]?.text.length ?? 0;

  const snapThreadToBottom = (behavior: ScrollBehavior = 'auto') => {
    const el = scrollRef.current;
    if (!el) return;
    const maxTop = Math.max(0, el.scrollHeight - el.clientHeight);
    if (behavior === 'smooth') {
      programmaticTargetTopRef.current = maxTop;
      el.scrollTo({ top: maxTop, behavior: 'smooth' });
      setShowTopFade(shouldShowScrollTopFade(maxTop));
    } else {
      el.scrollTop = el.scrollHeight;
      programmaticTargetTopRef.current = el.scrollTop;
      lastScrollTopRef.current = el.scrollTop;
      setShowTopFade(shouldShowScrollTopFade(el.scrollTop));
    }
  };

  useLayoutEffect(() => {
    const visitKey = `${panelView}:${activeChannel}:${threadVisitNonce}`;
    const isNewVisit = lastScrollVisitRef.current !== visitKey;
    lastScrollVisitRef.current = visitKey;

    if (panelView !== 'voice') {
      isPinnedToBottomRef.current = true;
      setIsAwayFromBottom(false);
      setShowTopFade(false);
      return;
    }

    const el = scrollRef.current;
    if (!el) return;

    if (isNewVisit) {
      isPinnedToBottomRef.current = true;
      setIsAwayFromBottom(false);
      lastScrollTopRef.current = el.scrollHeight;
    }
    if (!isPinnedToBottomRef.current) return;

    snapThreadToBottom('auto');
    const rafId = requestAnimationFrame(() => {
      if (isPinnedToBottomRef.current) snapThreadToBottom('auto');
    });
    const timerId = window.setTimeout(() => {
      if (isPinnedToBottomRef.current) snapThreadToBottom('auto');
    }, 60);
    return () => {
      cancelAnimationFrame(rafId);
      window.clearTimeout(timerId);
    };
  }, [
    panelView,
    activeChannel,
    threadVisitNonce,
    scopedEntries.length,
    lastScopedTextLen,
    voiceState.interimTranscript,
    activeIsThinking,
  ]);

  useEffect(() => {
    if (panelView !== 'voice') return;
    const container = scrollRef.current;
    const list = threadListRef.current;
    const anchor = bottomAnchorRef.current;
    if (!container) return;

    let rafId = 0;
    const snapIfPinned = () => {
      if (!isPinnedToBottomRef.current) return;
      if (performance.now() - lastUserScrollAtRef.current < 200) return;
      snapThreadToBottom('auto');
    };

    let resizeObserver: ResizeObserver | null = null;
    if (typeof ResizeObserver !== 'undefined') {
      resizeObserver = new ResizeObserver(() => {
        snapIfPinned();
        cancelAnimationFrame(rafId);
        rafId = requestAnimationFrame(() => {
          snapIfPinned();
        });
      });
      if (list) resizeObserver.observe(list);
      if (anchor) resizeObserver.observe(anchor);
      resizeObserver.observe(container);
    }

    let mutationObserver: MutationObserver | null = null;
    if (list && typeof MutationObserver !== 'undefined') {
      mutationObserver = new MutationObserver(() => {
        snapIfPinned();
      });
      mutationObserver.observe(list, {
        childList: true,
        subtree: true,
        characterData: true,
      });
    }

    return () => {
      cancelAnimationFrame(rafId);
      resizeObserver?.disconnect();
      mutationObserver?.disconnect();
    };
  }, [panelView, activeChannel, threadVisitNonce]);

  const effectiveMood: ExpressionMood = activeIsThinking
    ? 'thinking'
    : voiceState.isListening && !activeIsSpeaking
    ? 'listening'
    : mood;

  const viseme = useMemo(
    () =>
      amplitudeToViseme(
        voiceState.outputLevel,
        effectiveMood,
        tick,
        activeIsSpeaking,
        voiceState.frequencyBands,
        lastSpokenLine
      ),
    [voiceState.outputLevel, effectiveMood, tick, activeIsSpeaking, voiceState.frequencyBands, lastSpokenLine]
  );

  const portraitEyeState: EyeState =
    effectiveMood === 'alert'
      ? 'wide'
      : effectiveMood === 'thinking'
      ? 'side'
      : effectiveMood === 'focused'
      ? 'half'
      : effectiveMood === 'happy'
      ? 'happy'
      : 'open';

  const energy = activeIsSpeaking
    ? voiceState.outputLevel
    : voiceState.isListening
    ? Math.max(0.18, voiceState.inputLevel)
    : 0.08;

  const commandHistory = useMemo(
    () => scopedEntries.filter((e) => e.role === 'user' || e.role === 'command').map((e) => e.text),
    [scopedEntries]
  );

  const visibleMessages = useMemo(
    () =>
      scopedEntries.filter(
        (e) =>
          e.role === 'user' ||
          e.role === 'hermes' ||
          e.role === 'command' ||
          isTerminalDiagnosticActivity(e) ||
          (showThinking && e.role === 'activity')
      ),
    [scopedEntries, showThinking]
  );

  const pendingApprovals = useMemo(
    () => approvals.filter((a) => a.status === 'pending'),
    [approvals]
  );

  const channelPendingApprovals = useMemo(
    () => pendingApprovals.filter((a) => a.channelId === activeChannel),
    [pendingApprovals, activeChannel]
  );

  const activeTurnCount = useMemo(
    () =>
      scopedEntries.filter((e) => e.role === 'user' || e.role === 'command').length,
    [scopedEntries]
  );

  const timelineBlocks = useMemo(
    () => groupThreadTimeline(visibleMessages, activeIsThinking, showThinking),
    [visibleMessages, activeIsThinking, showThinking]
  );

  const minimapTurns = useMemo(
    () => extractThreadMinimapTurns(timelineBlocks),
    [timelineBlocks]
  );

  const minimapAnchorSet = useMemo(
    () => new Set(minimapTurns.map((t) => t.anchorId)),
    [minimapTurns]
  );

  const syncActiveMinimapTurn = () => {
    const scrollEl = scrollRef.current;
    if (!scrollEl || minimapTurns.length === 0) {
      setActiveMinimapTurnId(null);
      return;
    }
    const containerRect = scrollEl.getBoundingClientRect();
    const scrollTop = scrollEl.scrollTop;
    const anchors = minimapTurns
      .map((turn) => {
        const el = turnAnchorRefs.current.get(turn.anchorId);
        if (!el) return null;
        const rect = el.getBoundingClientRect();
        return {
          id: turn.anchorId,
          top: rect.top - containerRect.top + scrollTop,
          height: rect.height,
        };
      })
      .filter((a): a is { id: string; top: number; height: number } => a !== null);

    const maxScrollTop = Math.max(0, scrollEl.scrollHeight - scrollEl.clientHeight);
    const nextId = determineActiveMinimapTurn(
      anchors,
      scrollTop,
      scrollEl.clientHeight,
      maxScrollTop
    );
    setActiveMinimapTurnId((prev) => (prev === nextId ? prev : nextId));
  };

  useEffect(() => {
    if (panelView !== 'voice' || minimapTurns.length === 0) {
      setActiveMinimapTurnId(null);
      return;
    }
    const rafId = requestAnimationFrame(() => syncActiveMinimapTurn());
    return () => cancelAnimationFrame(rafId);
  }, [panelView, activeChannel, minimapTurns]);

  useEffect(() => {
    return () => {
      if (jumpHighlightTimerRef.current !== null) {
        window.clearTimeout(jumpHighlightTimerRef.current);
      }
    };
  }, []);

  const handleJumpToMinimapTurn = (anchorId: string) => {
    const scrollEl = scrollRef.current;
    const targetEl = turnAnchorRefs.current.get(anchorId);
    if (!scrollEl || !targetEl) return;

    const containerRect = scrollEl.getBoundingClientRect();
    const targetRect = targetEl.getBoundingClientRect();
    const rawTarget = targetRect.top - containerRect.top + scrollEl.scrollTop - 16;
    const maxTop = Math.max(0, scrollEl.scrollHeight - scrollEl.clientHeight);
    const targetTop = Math.max(0, Math.min(maxTop, rawTarget));

    isPinnedToBottomRef.current = maxTop - targetTop <= 28;
    lastUserScrollAtRef.current = performance.now();
    programmaticTargetTopRef.current = targetTop;
    setIsAwayFromBottom(maxTop - targetTop > 96);
    setActiveMinimapTurnId(anchorId);
    setShowTopFade(shouldShowScrollTopFade(targetTop));

    scrollEl.scrollTo({
      top: targetTop,
      behavior: uiConfig.reducedMotion ? 'auto' : 'smooth',
    });

    if (jumpHighlightTimerRef.current !== null) {
      window.clearTimeout(jumpHighlightTimerRef.current);
    }
    setHighlightedTurnId(anchorId);
    jumpHighlightTimerRef.current = window.setTimeout(() => {
      setHighlightedTurnId(null);
      jumpHighlightTimerRef.current = null;
    }, 950);
  };

  useEffect(() => {
    setHermesInspectorOpen(false);
    setSlashPaletteForced(false);
    setThreadMoreMenuOpen(false);
    setThreadSearchOpen(false);
    setThreadSearchQuery('');
    setThreadSearchMatchIdx(0);
  }, [panelView, activeChannel]);

  useEffect(() => {
    if (panelView !== 'voice') return;
    const onGlobalKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'f') {
        e.preventDefault();
        setThreadMoreMenuOpen(false);
        setThreadSearchOpen(true);
        window.setTimeout(() => threadSearchInputRef.current?.focus(), 20);
      } else if (e.key === 'Escape' && threadMoreMenuOpen) {
        setThreadMoreMenuOpen(false);
      }
    };
    window.addEventListener('keydown', onGlobalKey);
    return () => window.removeEventListener('keydown', onGlobalKey);
  }, [panelView, threadMoreMenuOpen]);

  useEffect(() => {
    const el = composerInputRef.current;
    if (!el) return;
    el.style.height = 'auto';
    el.style.height = `${Math.min(132, Math.max(32, el.scrollHeight))}px`;
  }, [input, panelView]);

  const threadSearchMatches = useMemo(
    () =>
      threadSearchOpen ? findMatchingThreadEntryIds(scopedEntries, threadSearchQuery) : [],
    [threadSearchOpen, scopedEntries, threadSearchQuery]
  );

  const activeSearchEntryId =
    threadSearchMatches.length > 0
      ? threadSearchMatches[Math.min(threadSearchMatchIdx, threadSearchMatches.length - 1)] ?? null
      : null;

  useEffect(() => {
    setThreadSearchMatchIdx(0);
    if (threadSearchOpen && threadSearchQuery.trim() && threadSearchMatches.length > 0) {
      const firstId = threadSearchMatches[0];
      if (firstId) {
        const rafId = requestAnimationFrame(() => scrollToThreadEntryId(firstId));
        return () => cancelAnimationFrame(rafId);
      }
    }
    return undefined;
  }, [threadSearchQuery, activeChannel, threadSearchOpen]);

  const scrollToThreadEntryId = (entryId: string) => {
    const scrollEl = scrollRef.current;
    const targetEl = entryDomRefs.current.get(entryId) ?? turnAnchorRefs.current.get(entryId);
    if (!scrollEl || !targetEl) return;
    const scrollRect = scrollEl.getBoundingClientRect();
    const targetRect = targetEl.getBoundingClientRect();
    const targetTop = Math.max(0, scrollEl.scrollTop + (targetRect.top - scrollRect.top) - 24);
    isPinnedToBottomRef.current = false;
    programmaticTargetTopRef.current = targetTop;
    scrollEl.scrollTo({
      top: targetTop,
      behavior: uiConfig.reducedMotion ? 'auto' : 'smooth',
    });
    if (jumpHighlightTimerRef.current !== null) {
      window.clearTimeout(jumpHighlightTimerRef.current);
    }
    setHighlightedTurnId(entryId);
    jumpHighlightTimerRef.current = window.setTimeout(() => {
      setHighlightedTurnId(null);
      jumpHighlightTimerRef.current = null;
    }, 950);
  };

  const handleStepThreadSearch = (direction: 'next' | 'prev') => {
    if (threadSearchMatches.length === 0) return;
    const nextIdx = stepMatchIndex(threadSearchMatchIdx, threadSearchMatches.length, direction);
    setThreadSearchMatchIdx(nextIdx);
    const targetId = threadSearchMatches[nextIdx];
    if (targetId) {
      scrollToThreadEntryId(targetId);
    }
  };

  const handleCopySessionKey = () => {
    if (typeof navigator !== 'undefined' && navigator.clipboard?.writeText) {
      void navigator.clipboard.writeText(activeHermesSessionId).catch(() => {});
    }
    setCopiedSessionKey(true);
    window.setTimeout(() => setCopiedSessionKey(false), 1400);
  };

  const handleCopyTranscript = () => {
    const md = formatThreadTranscriptMarkdown({
      title: activeDotLabel?.primaryLabel ?? activeHermesSessionId,
      sessionId: activeHermesSessionId,
      profile: activeProfile,
      model: gatewayStatus?.activeModel || 'hermes-agent',
      entries: scopedEntries,
    });
    if (typeof navigator !== 'undefined' && navigator.clipboard?.writeText) {
      void navigator.clipboard.writeText(md).catch(() => {});
    }
    setCopiedTranscript(true);
    window.setTimeout(() => setCopiedTranscript(false), 1600);
  };

  const handleEditPromptInComposer = (text: string) => {
    setInput(text);
    setSlashPaletteForced(false);
    window.setTimeout(() => {
      const el = composerInputRef.current;
      if (el) {
        el.focus();
        el.setSelectionRange(text.length, text.length);
      }
    }, 10);
  };

  const slashPaletteOpen = useMemo(
    () => panelView !== 'canvas' && shouldShowSlashPalette(input, slashPaletteForced),
    [panelView, input, slashPaletteForced]
  );

  const slashPaletteItems = useMemo(
    () =>
      slashPaletteOpen
        ? filterSlashCommands(input, { pendingApprovalCount: pendingApprovals.length })
        : [],
    [slashPaletteOpen, input, pendingApprovals.length]
  );

  useEffect(() => {
    setSlashSelectedIndex(0);
  }, [input, slashPaletteOpen]);

  const handleSelectSlashSpec = (spec: SlashCommandSpec) => {
    const resolved = resolvePaletteSelection(spec);
    setSlashPaletteForced(false);
    if (resolved.mode === 'run') {
      setInput('');
      setHistoryIdx(-1);
      onSubmitInput(resolved.value, 'cli');
      return;
    }
    setInput(resolved.value);
    window.setTimeout(() => {
      composerInputRef.current?.focus();
    }, 0);
  };

  const handleCopyEntryText = (entryId: string, text: string) => {
    if (typeof navigator !== 'undefined' && navigator.clipboard?.writeText) {
      void navigator.clipboard.writeText(text).catch(() => {});
    }
    setCopiedEntryId(entryId);
    window.setTimeout(() => {
      setCopiedEntryId((curr) => (curr === entryId ? null : curr));
    }, 1500);
  };

  const lastUserPromptInThread = useMemo(() => {
    for (let i = scopedEntries.length - 1; i >= 0; i -= 1) {
      if (scopedEntries[i].role === 'user' && scopedEntries[i].text.trim()) {
        return scopedEntries[i].text.trim();
      }
    }
    return '';
  }, [scopedEntries]);

  const composerPlaceholder = useMemo(
    () =>
      resolveComposerPlaceholder({
        isHomeNewDraft,
        activeChannel,
        promptTarget: activeChannel,
        isThinking: activeIsThinking,
        isSpeaking: activeIsSpeaking,
        isListening: voiceState.isListening,
        compact: !isDesktopLg,
      }),
    [
      isHomeNewDraft,
      activeChannel,
      activeIsThinking,
      activeIsSpeaking,
      voiceState.isListening,
      isDesktopLg,
    ]
  );

  const submitCurrentInput = () => {
    const trimmed = input.trim();
    if (!trimmed) return;
    setInput('');
    setHistoryIdx(-1);
    setSlashPaletteForced(false);
    setHermesInspectorOpen(false);
    onSubmitInput(trimmed, trimmed.startsWith('/') ? 'cli' : 'voice');
  };

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    submitCurrentInput();
  };

  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (slashPaletteOpen && slashPaletteItems.length > 0) {
      if (e.key === 'ArrowDown') {
        e.preventDefault();
        setSlashSelectedIndex((idx) => (idx + 1) % slashPaletteItems.length);
        return;
      }
      if (e.key === 'ArrowUp') {
        e.preventDefault();
        setSlashSelectedIndex(
          (idx) => (idx - 1 + slashPaletteItems.length) % slashPaletteItems.length
        );
        return;
      }
      if (e.key === 'Tab') {
        e.preventDefault();
        const picked = slashPaletteItems[slashSelectedIndex] ?? slashPaletteItems[0];
        if (picked) {
          setInput(`${picked.command} `);
          setSlashPaletteForced(false);
        }
        return;
      }
      if (e.key === 'Enter' && !e.shiftKey) {
        const picked = slashPaletteItems[slashSelectedIndex];
        const exactTyped = input.trim().toLowerCase();
        if (picked && exactTyped !== picked.command) {
          e.preventDefault();
          handleSelectSlashSpec(picked);
          return;
        }
      }
      if (e.key === 'Escape') {
        e.preventDefault();
        setSlashPaletteForced(false);
        if (input.trim() === '/') {
          setInput('');
        }
        return;
      }
    } else if (e.key === 'Escape' && (slashPaletteForced || hermesInspectorOpen)) {
      e.preventDefault();
      setSlashPaletteForced(false);
      setHermesInspectorOpen(false);
      return;
    }

    if (e.key === 'Enter' && !e.shiftKey && !e.nativeEvent.isComposing) {
      e.preventDefault();
      submitCurrentInput();
      return;
    }

    const isMultiline = input.includes('\n');
    if (e.key === 'ArrowUp' && !isMultiline) {
      e.preventDefault();
      if (commandHistory.length === 0) return;
      const nextIdx =
        historyIdx === -1 ? commandHistory.length - 1 : Math.max(0, historyIdx - 1);
      setHistoryIdx(nextIdx);
      setInput(commandHistory[nextIdx]);
    } else if (e.key === 'ArrowDown' && !isMultiline) {
      e.preventDefault();
      if (historyIdx === -1) return;
      if (historyIdx >= commandHistory.length - 1) {
        setHistoryIdx(-1);
        setInput('');
      } else {
        const nextIdx = historyIdx + 1;
        setHistoryIdx(nextIdx);
        setInput(commandHistory[nextIdx]);
      }
    }
  };

  const statusText = voiceState.isMuted
    ? 'Mute'
    : portraitStatus === 'loading'
    ? 'Calibrating'
    : portraitStatus === 'error'
    ? 'Signal offline'
    : activeIsThinking
    ? 'Thinking'
    : activeIsSpeaking
    ? 'Speaking'
    : voiceState.isListening
    ? 'Listening'
    : 'Standby';

  const statusPulseState: StatusPulseState = voiceState.isMuted
    ? 'muted'
    : portraitStatus === 'error'
    ? 'error'
    : activeIsThinking
    ? 'thinking'
    : activeIsSpeaking
    ? 'speaking'
    : voiceState.isListening
    ? 'listening'
    : 'idle';

  const activeChannelRunningTaskCount =
    activeChannelTasksPartition.running.length > 0
      ? activeChannelTasksPartition.running.length
      : activeIsThinking
      ? 1
      : 0;

  const navTabItems: MorphTabItem<WorkspacePanelView>[] = useMemo(
    () => [
      { id: 'home', label: 'Home', icon: <Home className="w-3.5 h-3.5 shrink-0 opacity-80" /> },
      { id: 'canvas', label: 'Canvas', icon: <LayoutGrid className="w-3.5 h-3.5 shrink-0 opacity-80" /> },
      {
        id: 'voice',
        label:
          activeChannelRunningTaskCount > 0
            ? `Thread (${activeChannelRunningTaskCount})`
            : 'Thread',
        icon: <MessageSquare className="w-3.5 h-3.5 shrink-0 opacity-80" />,
      },
      {
        id: 'queue',
        label: pendingApprovals.length > 0 ? `Queue (${pendingApprovals.length})` : 'Queue',
        icon: <ListChecks className="w-3.5 h-3.5 shrink-0 opacity-80" />,
      },
      { id: 'cli', label: 'CLI', icon: <Terminal className="w-3.5 h-3.5 shrink-0 opacity-80" /> },
    ],
    [activeChannelRunningTaskCount, pendingApprovals.length]
  );

  const handlePointerMove = (e: React.PointerEvent<HTMLElement>) => {
    const anchor = portraitWrapRef.current ?? e.currentTarget;
    const rect = anchor.getBoundingClientRect();
    const cx = rect.left + rect.width * 0.5;
    const cy = rect.top + rect.height * 0.48;
    const normW = Math.max(160, rect.width * 1.4);
    const normH = Math.max(160, rect.height * 1.4);
    const nx = Math.max(-1, Math.min(1, (e.clientX - cx) / normW));
    const ny = Math.max(-1, Math.min(1, (e.clientY - cy) / normH));
    lookTargetRef.current = { x: nx, y: ny };
  };

  const handlePointerLeave = () => {
    lookTargetRef.current = null;
  };

  const effectivePortraitTint =
    uiConfig.portraitTintHex ??
    (uiConfig.accentPreset === 'slate' ? null : uiConfig.accentSoftHex);

  const isThreadSplitLayout = panelView === 'voice' && threadTasksDockMode === 'split';
  const showDesktopThreadSidebar = panelView === 'voice' && isDesktopLg;

  const composerDock =
    panelView !== 'canvas' ? (
      <div
        className={`relative z-30 shrink-0 ${
          uiConfig.composerStyle === 'floating'
            ? 'px-4 sm:px-8 lg:px-10 pb-3.5 lg:pb-5 pt-1.5'
            : uiConfig.composerStyle === 'minimal'
            ? 'px-3 sm:px-6 lg:px-8 pb-2 lg:pb-3 pt-1'
            : 'px-2.5 sm:px-6 lg:px-8 pb-3 lg:pb-4 pt-1.5 lg:pt-2'
        }`}
      >
        <SlashCommandPalette
          open={slashPaletteOpen}
          items={slashPaletteItems}
          selectedIndex={slashSelectedIndex}
          onHoverIndex={setSlashSelectedIndex}
          onSelectItem={handleSelectSlashSpec}
          onClose={() => {
            setSlashPaletteForced(false);
            if (input.trim() === '/') setInput('');
          }}
        />

        <form
          onSubmit={handleSubmit}
          className={`lg:max-w-[740px] lg:mx-auto w-full flex flex-col lg:flex-row lg:items-end gap-1.5 lg:gap-2 rounded-[22px] lg:rounded-xl border px-3 py-2.5 lg:px-3 lg:py-2 transition-colors duration-150 ${
            uiConfig.composerStyle === 'minimal'
              ? 'border-white/10 bg-[#11141B]/85 shadow-none'
              : uiConfig.composerStyle === 'floating'
              ? 'border-white/16 bg-[#11141B]/94 shadow-[0_14px_38px_rgba(0,0,0,0.52)]'
              : 'shadow-[0_-6px_28px_rgba(0,0,0,0.42)] lg:shadow-none'
          } ${
            voiceState.isListening
              ? 'border-[var(--ui-accent)] bg-[#131722]/96'
              : uiConfig.composerStyle === 'docked'
              ? 'ui-composer-docked'
              : 'focus-within:border-[var(--ui-accent)]'
          }`}
        >
          {/* Row 1 (Mobile) / Flex Input (Desktop) */}
          <div className="flex items-end gap-2 flex-1 min-w-0">
            <button
              type="button"
              data-testid="slash-palette-trigger"
              onClick={() => {
                setSlashPaletteForced((prev) => !prev);
                composerInputRef.current?.focus();
              }}
              aria-expanded={slashPaletteOpen}
              aria-label="Browse Hermes slash commands"
              title="Hermes slash commands (/)"
              className={`hidden lg:flex w-8 h-8 rounded-lg border items-center justify-center font-mono text-xs transition-colors shrink-0 cursor-pointer ${
                slashPaletteOpen
                  ? 'border-[rgba(var(--ui-accent-rgb),0.55)] bg-[rgba(var(--ui-accent-rgb),0.18)] text-white'
                  : 'border-white/[0.08] bg-white/[0.03] text-zinc-400 hover:text-white hover:border-white/20'
              }`}
            >
              <Slash className="w-3.5 h-3.5" />
            </button>

            <label htmlFor="hermes-voice-input" className="sr-only">
              Voice transcript or command input
            </label>
            <textarea
              ref={composerInputRef}
              id="hermes-voice-input"
              rows={1}
              value={input}
              onChange={(e) => {
                const next = e.target.value;
                setInput(next);
                if (!next.startsWith('/')) {
                  setSlashPaletteForced(false);
                }
              }}
              onKeyDown={handleKeyDown}
              placeholder={composerPlaceholder}
              className="flex-1 min-w-0 bg-transparent text-[14px] lg:text-sm text-white placeholder:text-zinc-500 focus:outline-none py-1.5 px-1 lg:px-0 resize-none leading-[20px] min-h-[32px] max-h-[132px] overflow-y-auto cyber-scroll"
            />
          </div>

          {/* Mobile 2nd Row Toolbar: Thumb-Zone Utility Pills + Voice/Send */}
          <div className="flex items-center justify-between gap-2 lg:justify-end shrink-0">
            {/* Left Tool Cluster (Mobile only: clean Commands trigger since views live in top bar) */}
            <div className="flex lg:hidden items-center gap-1.5">
              <button
                type="button"
                data-testid="mobile-slash-trigger"
                onClick={() => {
                  setSlashPaletteForced((prev) => !prev);
                  composerInputRef.current?.focus();
                }}
                aria-expanded={slashPaletteOpen}
                aria-label="Browse Hermes slash commands"
                title="Commands (/)"
                className={`h-9 px-2.5 rounded-full border flex items-center gap-1.5 text-xs font-medium transition-colors cursor-pointer ${
                  slashPaletteOpen
                    ? 'bg-[rgba(var(--ui-accent-rgb),0.20)] border-[rgba(var(--ui-accent-rgb),0.55)] text-[var(--ui-accent-soft)]'
                    : 'bg-white/[0.06] border-white/10 text-zinc-300 hover:text-white'
                }`}
              >
                <Slash className="w-3.5 h-3.5" />
                <span>Commands</span>
              </button>
            </div>

            {/* Right Action Cluster: Stop / Mic / Send */}
            <div className="flex items-center gap-1.5">
              {(activeIsThinking || activeIsSpeaking) && (
                <button
                  type="button"
                  onClick={onStopSpeaking}
                  aria-label="Stop active run"
                  title="Stop"
                  className="ui-tactile w-9 h-9 lg:w-auto lg:h-7 lg:px-2.5 rounded-full lg:rounded-lg border border-rose-400/40 bg-rose-500/14 text-rose-200 hover:bg-rose-500/24 flex items-center justify-center gap-1.5 text-xs font-medium transition-colors cursor-pointer shrink-0"
                >
                  <Square className="w-3.5 h-3.5 lg:w-2.5 lg:h-2.5 fill-current" />
                  <span className="hidden lg:inline">Stop</span>
                </button>
              )}

              <button
                type="button"
                onClick={onToggleListening}
                aria-pressed={voiceState.isListening}
                aria-label={voiceState.isListening ? 'Stop voice input' : 'Start voice input'}
                className={`w-9 h-9 lg:w-8 lg:h-8 rounded-full lg:rounded-lg flex items-center justify-center transition-colors cursor-pointer shrink-0 border lg:border-0 ${
                  voiceState.isListening
                    ? 'bg-[rgba(var(--ui-accent-rgb),0.22)] text-[var(--ui-accent-bright)] border-[var(--ui-accent)]'
                    : 'bg-white/[0.06] lg:bg-transparent border-white/10 text-[var(--ui-accent-soft)] hover:text-white hover:bg-white/10'
                }`}
              >
                {voiceState.isListening ? (
                  <Mic className="w-4 h-4" />
                ) : (
                  <MicOff className="w-4 h-4" />
                )}
              </button>

              <button
                type="submit"
                disabled={!input.trim()}
                data-testid="composer-send-btn"
                aria-label="Send message or command"
                title={input.trim() ? 'Send' : 'Type a prompt or /command'}
                className={`w-9 h-9 lg:w-8 lg:h-8 rounded-full lg:rounded-lg flex items-center justify-center transition-all duration-150 shrink-0 ${
                  input.trim()
                    ? 'ui-accent-btn hover:opacity-90 active:scale-95 cursor-pointer'
                    : 'bg-white/[0.05] border border-white/10 text-zinc-500 cursor-not-allowed'
                }`}
              >
                <ArrowUp className="w-4 h-4" />
              </button>
            </div>
          </div>
        </form>
      </div>
    ) : null;

  return (
    <section
      ref={stageRef}
      aria-label="Hermes Voice Conversation Workspace"
      data-testid="ascii-stage"
      data-engine="halftone-portrait-ascii"
      data-dither="braille"
      data-view={panelView}
      data-viseme={viseme}
      data-mood={effectiveMood}
      data-speaking={String(voiceState.isSpeaking)}
      onPointerMove={handlePointerMove}
      onPointerLeave={handlePointerLeave}
      className="@container ui-stage-shell relative flex flex-col h-full min-w-0 rounded-2xl border overflow-hidden"
    >
      {/* Mobile Reference-Inspired Dual-Pill Header & Parallel Session Avatar Strip */}
      <MobileWorkspaceBar
        activeChannel={activeChannel}
        activeSessionId={activeHermesSessionId}
        turnCount={activeTurnCount}
        mood={effectiveMood}
        viseme={viseme}
        isSpeaking={activeIsSpeaking}
        isListening={voiceState.isListening}
        isThinking={activeIsThinking}
        isMuted={voiceState.isMuted}
        panelView={panelView}
        pendingCount={pendingApprovals.length}
        showThinking={showThinking}
        canvasSessions={canvasSessions}
        hermesSessions={hermesSessions}
        thinkingByChannel={thinkingByChannel}
        tasks={tasks}
        homeSessionDraft={homeSessionDraft}
        onChangePanelView={onChangePanelView}
        onSelectChannel={onSelectChannel}
        onOpenMobileSidebar={onOpenMobileSidebar}
        onOpenQuickJump={onOpenQuickJump}
        onResetSession={onResetSession}
        onCycleMood={onCycleMood}
        onToggleMute={onToggleMute}
        onToggleThinking={onToggleThinking}
        onOpenStudio={onOpenStudio}
        updateAvailable={updateAvailable}
      />

      {/* Unified Clean Top Bar (Desktop lg+) */}
      {uiConfig.showTopNav ? (
        <div className="hidden lg:flex items-center justify-between gap-3 xl:gap-6 px-4 xl:px-6 pt-3 pb-2 border-b border-white/6 z-30 shrink-0">
          <div className="flex items-center gap-3 min-w-0 overflow-x-auto cyber-scroll">
            {!uiConfig.sidebarVisible && onToggleSidebar && (
              <button
                type="button"
                onClick={onToggleSidebar}
                data-testid="topbar-expand-sidebar"
                aria-label="Show sidebar"
                title="Show sidebar (⌘B)"
                className="ui-tactile w-8 h-8 rounded-lg border border-white/10 bg-white/[0.03] text-zinc-300 hover:text-white hover:border-white/25 inline-flex items-center justify-center transition-colors shrink-0 cursor-pointer"
              >
                <PanelLeft className="w-3.5 h-3.5" />
              </button>
            )}

            <MorphTabs
              items={navTabItems}
              value={panelView}
              onChange={onChangePanelView}
              ariaLabel="Workspace views"
              variant="underline"
            />
          </div>

          <div className="flex items-center gap-1.5 shrink-0">
            {onOpenQuickJump && (
              <button
                type="button"
                data-testid="topbar-quick-jump"
                onClick={onOpenQuickJump}
                aria-label="Quick jump navigation"
                title="Quick jump to any view, session, or action (⌘K)"
                className="ui-tactile h-8 px-2.5 rounded-lg border border-white/10 bg-white/[0.03] text-xs font-medium text-zinc-300 hover:text-white hover:border-white/25 inline-flex items-center gap-2 transition-colors whitespace-nowrap shrink-0 cursor-pointer"
              >
                <Search className="w-3.5 h-3.5 text-zinc-400" />
                <span className="hidden xl:inline">Jump</span>
                <kbd className="font-mono text-[10px] text-zinc-400">⌘K</kbd>
              </button>
            )}

            <button
              type="button"
              data-testid="inline-hermes-trigger"
              aria-expanded={
                (panelView === 'home' || panelView === 'voice') ? hermesInspectorOpen : undefined
              }
              onClick={() => {
                if (panelView === 'home' || panelView === 'voice') {
                  setHermesInspectorOpen((v) => !v);
                } else {
                  onChangePanelView('queue');
                }
              }}
              title={
                panelView === 'home' || panelView === 'voice'
                  ? 'Inline Hermes Agent controls (profiles, skills, toolsets, transport & cron)'
                  : 'Open Queue & Gateway status'
              }
              className={`ui-tactile h-8 px-2.5 rounded-lg border text-xs font-medium flex items-center gap-1.5 transition-colors whitespace-nowrap shrink-0 cursor-pointer ${
                (panelView === 'home' || panelView === 'voice') && hermesInspectorOpen
                  ? 'border-[rgba(var(--ui-accent-rgb),0.5)] bg-[rgba(var(--ui-accent-rgb),0.16)] text-white'
                  : 'border-white/10 bg-white/[0.03] text-zinc-300 hover:text-white hover:border-white/25'
              }`}
            >
              <StatusPulse
                state={gatewayStatus?.connected ? 'live' : 'error'}
                size="sm"
              />
              <span>Hermes</span>
              {(skills?.length ?? 0) + (toolsets?.length ?? 0) > 0 && (
                <span className="font-mono text-[10px] text-zinc-400 tabular-nums">
                  {(skills?.length ?? 0) + (toolsets?.length ?? 0)}
                </span>
              )}
            </button>

            {uiConfig.showContextMeter && (
              <button
                type="button"
                data-testid="topbar-context-meter"
                aria-pressed={contextMonitorOpen}
                aria-label={`Context window ${modelContext.usage.usedPct.toFixed(1)}% used of ${formatCompactTokens(modelContext.usage.contextWindow)} on ${modelContext.usage.model.displayName}`}
                title={`${modelContext.usage.model.displayName} · ${modelContext.usage.usedTokens.toLocaleString()} / ${modelContext.usage.contextWindow.toLocaleString()} tokens (${modelContext.usage.usedPct.toFixed(1)}%)`}
                onClick={() => {
                  const nextOpen = !contextMonitorOpen;
                  setContextMonitorOpen(nextOpen);
                  if (nextOpen) {
                    setHermesInspectorOpen(false);
                    if (uiConfig.showJevRecall && onToggleJevRecall) {
                      onToggleJevRecall();
                    }
                  }
                }}
                className={`ui-tactile h-8 px-2.5 rounded-lg border text-xs font-medium flex items-center gap-2 transition-colors whitespace-nowrap shrink-0 cursor-pointer ${
                  contextMonitorOpen
                    ? 'border-[rgba(var(--ui-accent-rgb),0.55)] bg-[rgba(var(--ui-accent-rgb),0.18)] text-white'
                    : 'border-white/10 bg-white/[0.03] text-zinc-300 hover:text-white hover:border-white/25'
                }`}
              >
                <span
                  aria-hidden="true"
                  className="w-6 h-1.5 rounded-full bg-white/12 overflow-hidden shrink-0"
                >
                  <span
                    className={`block h-full rounded-full transition-all duration-300 ${
                      modelContext.usage.status === 'critical'
                        ? 'bg-rose-400'
                        : modelContext.usage.status === 'warning'
                        ? 'bg-amber-400'
                        : modelContext.usage.status === 'elevated'
                        ? 'bg-sky-400'
                        : 'bg-[var(--ui-accent-bright)]'
                    }`}
                    style={{ width: `${Math.max(4, Math.min(100, modelContext.usage.usedPct))}%` }}
                  />
                </span>
                <span className="font-mono text-[11px] tabular-nums text-zinc-200">
                  {modelContext.usage.usedPct < 10
                    ? `${modelContext.usage.usedPct.toFixed(1)}%`
                    : `${Math.round(modelContext.usage.usedPct)}%`}
                </span>
                <span className="hidden xl:inline font-mono text-[10.5px] text-zinc-500 tabular-nums">
                  {formatCompactTokens(modelContext.usage.contextWindow)}
                </span>
              </button>
            )}

            {onToggleJevRecall && (
              <button
                type="button"
                data-testid="topbar-jev-recall-toggle"
                aria-pressed={uiConfig.showJevRecall}
                onClick={() => {
                  if (!uiConfig.showJevRecall) {
                    setContextMonitorOpen(false);
                  }
                  onToggleJevRecall();
                }}
                title="Toggle Jev × uniichat-recall accuracy projection graph & Hermes SKILL.md"
                className={`ui-tactile h-8 px-2.5 rounded-lg border text-xs font-medium flex items-center gap-1.5 transition-colors whitespace-nowrap shrink-0 cursor-pointer ${
                  uiConfig.showJevRecall
                    ? 'border-[rgba(var(--ui-accent-rgb),0.55)] bg-[rgba(var(--ui-accent-rgb),0.18)] text-white'
                    : 'border-white/10 bg-white/[0.03] text-zinc-300 hover:text-white hover:border-white/25'
                }`}
              >
                <Activity className="w-3.5 h-3.5 text-[var(--ui-accent-soft)] shrink-0" />
                <span>Recall</span>
              </button>
            )}

            <button
              type="button"
              onClick={onToggleMute}
              aria-label={voiceState.isMuted ? 'Unmute Hermes audio' : 'Mute Hermes audio'}
              title={voiceState.isMuted ? 'Unmute audio' : 'Mute audio'}
              className={`w-8 h-8 rounded-lg flex items-center justify-center transition-colors cursor-pointer ${
                voiceState.isMuted
                  ? 'bg-rose-500/15 text-rose-200'
                  : 'text-zinc-400 hover:text-white hover:bg-white/[0.06]'
              }`}
            >
              {voiceState.isMuted ? <VolumeX className="w-3.5 h-3.5" /> : <Volume2 className="w-3.5 h-3.5" />}
            </button>

            {onOpenStudio && (
              <button
                type="button"
                onClick={onOpenStudio}
                data-testid="ui-studio-trigger"
                aria-label="Open Settings"
                title={updateAvailable ? 'Settings (Update available)' : 'Settings'}
                className="ui-tactile h-8 px-2.5 rounded-lg border border-white/10 bg-white/[0.03] text-xs font-medium text-zinc-300 hover:text-white hover:border-white/25 flex items-center gap-1.5 transition-colors whitespace-nowrap shrink-0 cursor-pointer"
              >
                <Settings className="w-3.5 h-3.5 text-[var(--ui-accent-soft)]" />
                <span>Settings</span>
                {updateAvailable && (
                  <span
                    aria-label="Update available"
                    className="w-1.5 h-1.5 rounded-full bg-amber-400 shrink-0"
                  />
                )}
              </button>
            )}
          </div>
        </div>
      ) : (
        onOpenStudio && (
          <div className="hidden lg:flex items-center gap-1.5 absolute top-3.5 right-4 z-30">
            <button
              type="button"
              onClick={onOpenStudio}
              data-testid="ui-studio-trigger"
              aria-label="Open Settings"
              className="h-8 px-2.5 rounded-lg border border-white/12 bg-[#11141B]/92 text-xs font-medium text-zinc-300 hover:text-white flex items-center gap-1.5 transition-colors whitespace-nowrap cursor-pointer"
            >
              <Settings className="w-3.5 h-3.5 text-[var(--ui-accent-soft)]" />
              <span>Settings</span>
              {updateAvailable && (
                <span
                  aria-label="Update available"
                  className="w-1.5 h-1.5 rounded-full bg-amber-400 shrink-0"
                />
              )}
            </button>
          </div>
        )
      )}

      {/* Toggleable Model Info & Context Window Usage Overlay */}
      {contextMonitorOpen && !uiConfig.showJevRecall && (
        <div
          className={`absolute left-1/2 -translate-x-1/2 z-30 w-[min(780px,calc(100%-1.5rem))] max-h-[calc(100%-8.5rem)] overflow-y-auto cyber-scroll ${
            uiConfig.showTopNav ? 'top-14 lg:top-[58px]' : 'top-3'
          }`}
        >
          <ContextWindowMonitor
            usage={modelContext.usage}
            availableModels={modelContext.availableModels}
            selectedModelId={modelContext.selectedModelId}
            activeModelId={modelContext.modelInfo?.activeModel.id ?? modelContext.inspectedModel.id}
            liveCatalogFetched={Boolean(modelContext.modelInfo?.liveCatalogFetched)}
            gatewayConnected={Boolean(gatewayStatus?.connected || modelContext.modelInfo?.gatewayConnected)}
            refreshing={modelContext.refreshing}
            onSelectModel={modelContext.setSelectedModelId}
            onRefresh={() => void modelContext.refresh(true)}
            onClose={() => setContextMonitorOpen(false)}
            onStartNewSession={
              onStartNewSession
                ? () => {
                    setContextMonitorOpen(false);
                    onStartNewSession();
                  }
                : activeChannel
                ? () => {
                    setContextMonitorOpen(false);
                    onResetSession(activeChannel);
                  }
                : undefined
            }
            onOpenRecallMonitor={
              onToggleJevRecall
                ? () => {
                    setContextMonitorOpen(false);
                    if (!uiConfig.showJevRecall) onToggleJevRecall();
                  }
                : undefined
            }
          />
        </div>
      )}

      {/* Toggleable TypeSafe Jev × uniichat-recall Accuracy Projection & Hermes SKILL.md Overlay */}
      {uiConfig.showJevRecall && (
        <div
          className={`absolute left-1/2 -translate-x-1/2 z-30 w-[min(780px,calc(100%-1.5rem))] max-h-[calc(100%-8.5rem)] overflow-y-auto cyber-scroll ${
            uiConfig.showTopNav ? 'top-14 lg:top-[58px]' : 'top-3'
          }`}
        >
          <JevRecallMonitor
            sessionId={activeHermesSessionId}
            entries={scopedEntries}
            compact={panelView === 'voice' || panelView === 'canvas'}
            onClose={() => onToggleJevRecall?.()}
            onStartNewSession={
              onStartNewSession
                ? () => {
                    onStartNewSession();
                  }
                : activeChannel
                ? () => onResetSession(activeChannel)
                : undefined
            }
          />
        </div>
      )}

      {/* Center Stage: Simplified Character Home View OR Compact Header + Thread/Queue/CLI */}
      {isHomeOverview ? (
        <div
          key="view-home"
          ref={portraitWrapRef}
          data-testid="home-character-overview"
          className="ui-view-enter relative z-10 flex-1 min-h-0 overflow-y-auto cyber-scroll flex flex-col items-center px-5 sm:px-10 pt-2 pb-32 sm:pb-36 lg:pb-28"
        >
          <div className="my-auto w-full flex flex-col items-center">
          {uiConfig.portraitVisible && (
            <div
              className="relative flex flex-col items-center justify-center transition-transform duration-200"
              style={{ transform: 'scale(var(--ui-portrait-scale, 1))' }}
            >
              <div
                aria-hidden="true"
                className={`absolute inset-0 -m-12 rounded-full pointer-events-none transition-opacity duration-300 ${
                  activeIsSpeaking || voiceState.isListening
                    ? 'ui-halo-pulse opacity-100'
                    : 'ui-halo-idle opacity-75'
                }`}
                style={{
                  background:
                    effectiveMood === 'thinking'
                      ? 'radial-gradient(circle at 50% 48%, rgba(245, 158, 11, 0.17) 0%, rgba(245, 158, 11, 0.05) 44%, transparent 72%)'
                      : effectiveMood === 'alert'
                      ? 'radial-gradient(circle at 50% 48%, rgba(244, 63, 94, 0.16) 0%, rgba(244, 63, 94, 0.05) 44%, transparent 72%)'
                      : 'radial-gradient(circle at 50% 48%, rgba(var(--ui-accent-rgb), 0.16) 0%, rgba(var(--ui-accent-rgb), 0.05) 44%, transparent 72%)',
                }}
              />
              <div className="relative z-10">
                <AsciiPortraitCanvas
                  variant="stage"
                  mood={effectiveMood}
                  viseme={viseme}
                  eyeState={portraitEyeState}
                  energy={energy}
                  isSpeaking={activeIsSpeaking}
                  isListening={voiceState.isListening}
                  lookTargetRef={lookTargetRef}
                  tintHex={effectivePortraitTint}
                  dotProfile={uiConfig.brailleDotProfile}
                  bloom={uiConfig.brailleBloom}
                  speedScale={uiConfig.animationSpeed}
                  reducedMotionOverride={uiConfig.reducedMotion}
                  onStatusChange={setPortraitStatus}
                  onClick={onCycleMood}
                />
              </div>
            </div>
          )}

          {uiConfig.statusReadoutVisible && (
            <>
              <div
                data-testid={gatewayStatus && !gatewayStatus.connected ? 'gateway-offline-banner' : undefined}
                className="relative z-10 flex flex-wrap items-center justify-center gap-2 mt-2.5 text-xs text-zinc-300"
              >
                <StatusPulse
                  state={gatewayStatus && !gatewayStatus.connected ? 'error' : statusPulseState}
                  size="sm"
                />
                <span
                  className={
                    gatewayStatus && !gatewayStatus.connected
                      ? 'font-mono text-rose-200/90'
                      : 'text-zinc-200'
                  }
                >
                  {gatewayStatus && !gatewayStatus.connected ? 'Gateway offline' : statusText}
                </span>
                <span aria-hidden="true" className="text-zinc-600">·</span>
                {isHomeNewDraft && profiles.length > 1 && onChangeDraftProfile ? (
                  <select
                    data-testid="home-profile-select"
                    aria-label="Hermes profile for new session"
                    value={draftProfile}
                    onChange={(e) => onChangeDraftProfile(e.target.value)}
                    className="rounded-md bg-black/40 border border-white/15 px-1.5 py-0.5 font-mono text-[11px] text-[var(--ui-accent-soft)] focus:outline-none focus:border-[var(--ui-accent)]"
                  >
                    {profiles.map((p) => (
                      <option key={p.id} value={p.id}>
                        @{p.id}
                      </option>
                    ))}
                  </select>
                ) : (
                  <button
                    type="button"
                    data-testid="home-session-badge"
                    onClick={() => setHermesInspectorOpen((v) => !v)}
                    title="Inspect Hermes Agent profiles, skills, toolsets, transport & cron"
                    className="font-mono text-[11px] text-[var(--ui-accent-soft)] hover:text-white transition-colors cursor-pointer"
                  >
                    {isHomeNewDraft || !activeChannel
                      ? `@${draftProfile}`
                      : `${activeDotLabel?.primaryLabel ?? `#${activeChannel}`} · @${activeProfile}`}
                  </button>
                )}
                {gatewayStatus && !gatewayStatus.connected && onRefreshGateway && (
                  <>
                    <span aria-hidden="true" className="text-zinc-600">·</span>
                    <button
                      type="button"
                      onClick={() => void onRefreshGateway()}
                      className="ui-tactile font-mono text-[11px] text-zinc-300 hover:text-white underline underline-offset-4 decoration-white/25 transition-colors cursor-pointer"
                    >
                      Retry
                    </button>
                  </>
                )}
              </div>

              <div className="relative z-10 mt-3 w-full max-w-[640px] text-center space-y-3.5">
                <p
                  data-testid="home-decoded-line"
                  className="max-w-[50ch] mx-auto text-sm sm:text-[15px] leading-relaxed text-white/92"
                >
                  {activeIsThinking ? (
                    <DecodedText
                      text={`Routing turn inside ${activeDotLabel?.promptTarget ?? activeHermesSessionId}...`}
                      animate={!uiConfig.reducedMotion}
                      speedMultiplier={uiConfig.animationSpeed}
                    />
                  ) : (
                    <DecodedText
                      text={lastSpokenLine}
                      animate={animateSpeechLine && !uiConfig.reducedMotion}
                      speedMultiplier={uiConfig.animationSpeed}
                    />
                  )}
                </p>

                {hermesInspectorOpen && (
                  <div className="pt-1">
                    <InlineHermesPopover
                      open={hermesInspectorOpen}
                      onClose={() => setHermesInspectorOpen(false)}
                      activeChannel={activeChannel}
                      activeSessionId={activeHermesSessionId}
                      activeProfile={isHomeNewDraft ? draftProfile : activeProfile}
                      isHomeDraft={isHomeNewDraft}
                      gatewayStatus={gatewayStatus}
                      skills={skills}
                      toolsets={toolsets}
                      cronJobs={cronJobs}
                      approvals={approvals}
                      profiles={profiles}
                      transportMode={transportMode}
                      onRefreshGateway={onRefreshGateway}
                      onChangeTransportMode={onChangeTransportMode}
                      onSelectProfile={(pid) => {
                        if (isHomeNewDraft) {
                          onChangeDraftProfile?.(pid);
                        } else if (activeChannel) {
                          onChangeSessionProfile?.(activeChannel, pid);
                        }
                      }}
                      onApproveCard={onApproveCard}
                      onRejectCard={onRejectCard}
                      onPauseJob={onPauseJob}
                      onResumeJob={onResumeJob}
                      onRunJob={onRunJob}
                      onOpenOperations={() => onChangePanelView('queue')}
                      onRunPromptOrCommand={(text) =>
                        onSubmitInput(text, text.startsWith('/') ? 'cli' : 'voice')
                      }
                    />
                  </div>
                )}

                {!hermesInspectorOpen && homeCommandOutput && (
                  <div
                    data-testid="home-command-output"
                    className="ui-card-enter text-left rounded-xl border border-white/14 bg-[#11141B]/92 backdrop-blur-md p-3 space-y-2 shadow-[0_12px_32px_rgba(0,0,0,0.45)]"
                  >
                    <div className="flex items-center justify-between gap-2">
                      <span className="font-mono text-[11px] text-[var(--ui-accent-soft)] truncate">
                        {homeCommandOutput.command}
                      </span>
                      <div className="flex items-center gap-2 shrink-0">
                        <button
                          type="button"
                          onClick={() => onChangePanelView('cli')}
                          className="font-mono text-[10.5px] text-zinc-400 hover:text-white cursor-pointer"
                        >
                          Open CLI
                        </button>
                        <button
                          type="button"
                          onClick={() => onDismissHomeCommandOutput?.()}
                          aria-label="Dismiss command output"
                          className="text-zinc-400 hover:text-white cursor-pointer"
                        >
                          <X className="w-3.5 h-3.5" />
                        </button>
                      </div>
                    </div>
                    <pre className="font-mono text-[11px] leading-relaxed text-zinc-200 whitespace-pre-wrap break-words max-h-[180px] overflow-y-auto cyber-scroll">
                      {homeCommandOutput.output}
                    </pre>
                  </div>
                )}

                {!hermesInspectorOpen && (
                  <div
                    data-testid="home-quick-starters"
                    className="flex flex-wrap items-center justify-center gap-2 pt-0.5"
                  >
                    {CURATED_HOME_STARTERS.slice(0, 3).map((starter) => (
                      <button
                        key={starter.id}
                        type="button"
                        data-testid={`home-starter-${starter.id}`}
                        onClick={() =>
                          onSubmitInput(
                            starter.prompt,
                            starter.kind === 'command' ? 'cli' : 'voice'
                          )
                        }
                        className="group ui-tactile inline-flex items-center gap-1.5 rounded-lg border border-white/[0.09] bg-[#11141B]/72 hover:border-white/22 hover:bg-[#151923]/86 px-3 py-1.5 text-xs text-zinc-200 hover:text-white transition-colors cursor-pointer whitespace-nowrap"
                      >
                        <span>{starter.title}</span>
                        <ArrowUpRight className="w-3 h-3 text-zinc-500 group-hover:text-[var(--ui-accent-soft)] shrink-0 transition-colors" />
                      </button>
                    ))}
                  </div>
                )}

                {(activeIsThinking ||
                  activeIsSpeaking ||
                  (!isHomeNewDraft && visibleMessages.length > 0) ||
                  (!isHomeNewDraft && onStartNewSession) ||
                  pendingApprovals.length > 0) && (
                  <div className="flex flex-wrap items-center justify-center gap-5 text-xs text-zinc-400">
                    {(activeIsThinking || activeIsSpeaking) && (
                      <button
                        type="button"
                        onClick={onStopSpeaking}
                        className="ui-tactile inline-flex items-center gap-1.5 px-2.5 py-1 rounded-md bg-white/[0.06] border border-white/15 text-zinc-100 hover:bg-white/12 transition-colors cursor-pointer"
                      >
                        <Square className="w-2.5 h-2.5 fill-current" />
                        <span>Stop</span>
                      </button>
                    )}
                    {!isHomeNewDraft && visibleMessages.length > 0 && (
                      <button
                        type="button"
                        onClick={() => onChangePanelView('voice')}
                        className="ui-tactile text-[var(--ui-accent-soft)] hover:text-white underline underline-offset-4 decoration-white/25 transition-colors cursor-pointer"
                      >
                        Open thread ({visibleMessages.length})
                      </button>
                    )}
                    {!isHomeNewDraft && onStartNewSession && (
                      <button
                        type="button"
                        data-testid="home-new-session-btn"
                        onClick={onStartNewSession}
                        className="ui-tactile inline-flex items-center gap-1 hover:text-white underline underline-offset-4 decoration-white/20 transition-colors cursor-pointer"
                      >
                        <Plus className="w-3 h-3" />
                        <span>New session</span>
                      </button>
                    )}
                    {pendingApprovals.length > 0 && (
                      <button
                        type="button"
                        onClick={() => onChangePanelView('queue')}
                        className="ui-tactile text-amber-300/90 hover:text-amber-200 underline underline-offset-4 decoration-amber-400/35 transition-colors cursor-pointer"
                      >
                        Review {pendingApprovals.length} pending {pendingApprovals.length === 1 ? 'approval' : 'approvals'}
                      </button>
                    )}
                  </div>
                )}
              </div>
            </>
          )}
          </div>
        </div>
      ) : panelView === 'canvas' ? (
        <BrailleSessionsCanvas
          sessions={canvasSessions}
          pins={canvasPins}
          hermesSessions={hermesSessions}
          approvals={approvals}
          thinkingByChannel={thinkingByChannel}
          speakingChannel={speakingChannel}
          activeChannel={activeChannel}
          entries={entries}
          voiceState={voiceState}
          viseme={viseme}
          dotProfile={uiConfig.brailleDotProfile}
          bloom={uiConfig.brailleBloom}
          horizonStyle={uiConfig.horizonStyle}
          speedScale={uiConfig.animationSpeed}
          reducedMotionOverride={uiConfig.reducedMotion}
          profiles={profiles}
          defaultProfile={draftProfile}
          onSelectSession={onSelectCanvasSession}
          onMoveSession={onMoveCanvasSession}
          onAddSession={onAddCanvasSession}
          onUpdateDotConfig={onUpdateDotConfig}
          onAddPin={onAddCanvasPin}
          onMovePin={onMoveCanvasPin}
          onRemovePin={onRemoveCanvasPin}
          onSubmitSessionMessage={onSubmitSessionInput}
          onToggleListening={onToggleListening}
          onArrangeFormation={(formation) => onAutoArrangeCanvas(formation)}
          onCycleSessionMood={onCycleSessionMood}
          onResetSession={onResetSession}
          onRemoveSession={onRemoveCanvasSession}
          onOpenSessionThread={onOpenSessionThread}
        />
      ) : (
        <div className="relative flex-1 min-h-0 min-w-0 flex flex-row overflow-hidden">
          <div className="flex-1 min-w-0 flex flex-col min-h-0">
            {/* Desktop Thread Character Stage — adaptive full portrait or compact reading bar (Thread view only) */}
            {panelView === 'voice' && uiConfig.portraitVisible && stageHeaderSize !== 'strip' && !compactThreadHeader ? (
              <div
                ref={portraitWrapRef}
                data-testid="desktop-thread-stage"
                className="relative hidden lg:flex flex-col items-center justify-center pt-1.5 pb-1.5 shrink-0"
              >
                <div className="relative flex items-center justify-center">
                  <div
                    aria-hidden="true"
                    className={`absolute inset-0 -m-8 rounded-full pointer-events-none transition-opacity duration-300 ${
                      activeIsSpeaking || voiceState.isListening
                        ? 'opacity-100'
                        : 'opacity-75'
                    }`}
                    style={{
                      background:
                        effectiveMood === 'thinking'
                          ? 'radial-gradient(ellipse 65% 58% at 50% 48%, rgba(245, 158, 11, 0.16) 0%, rgba(245, 158, 11, 0.04) 48%, transparent 74%)'
                          : effectiveMood === 'alert'
                          ? 'radial-gradient(ellipse 65% 58% at 50% 48%, rgba(244, 63, 94, 0.16) 0%, rgba(244, 63, 94, 0.04) 48%, transparent 74%)'
                          : 'radial-gradient(ellipse 65% 58% at 50% 48%, rgba(var(--ui-accent-rgb), 0.14) 0%, rgba(var(--ui-accent-rgb), 0.04) 48%, transparent 74%)',
                    }}
                  />
                  <div className="relative z-10">
                    <AsciiPortraitCanvas
                      variant="hero"
                      mood={effectiveMood}
                      viseme={viseme}
                      eyeState={portraitEyeState}
                      energy={energy}
                      isSpeaking={activeIsSpeaking}
                      isListening={voiceState.isListening}
                      lookTargetRef={lookTargetRef}
                      tintHex={effectivePortraitTint}
                      dotProfile={uiConfig.brailleDotProfile}
                      bloom={uiConfig.brailleBloom}
                      speedScale={uiConfig.animationSpeed}
                      reducedMotionOverride={uiConfig.reducedMotion}
                      onStatusChange={setPortraitStatus}
                      onClick={onCycleMood}
                    />
                  </div>
                </div>

                {panelView === 'voice' ? (
                  <div className="relative z-10 w-full max-w-[740px] mx-auto px-8 mt-0.5 flex items-center justify-between gap-3 text-[11.5px] font-medium text-zinc-300">
                    <div className="flex items-center gap-1.5 min-w-0">
                      <StatusPulse
                        size="sm"
                        state={gatewayStatus && !gatewayStatus.connected ? 'error' : statusPulseState}
                      />
                      <span
                        className={
                          gatewayStatus && !gatewayStatus.connected
                            ? 'font-mono text-rose-200/90 shrink-0'
                            : 'text-white/95 shrink-0'
                        }
                      >
                        {gatewayStatus && !gatewayStatus.connected ? 'Gateway offline' : statusText}
                      </span>
                      <span aria-hidden="true" className="text-zinc-600 shrink-0">·</span>
                      <button
                        type="button"
                        onClick={handleCopySessionKey}
                        title={`Copy Hermes session key (${activeHermesSessionId})`}
                        data-testid="active-hermes-session-id"
                        className="inline-flex items-center gap-1 min-w-0 max-w-[165px] xl:max-w-[230px] text-left font-medium text-zinc-100 hover:text-white transition-colors cursor-pointer"
                      >
                        <span className="truncate">
                          {activeDotLabel?.primaryLabel ?? activeHermesSessionId}
                        </span>
                        {copiedSessionKey && (
                          <Check className="w-3 h-3 text-[var(--ui-accent-soft)] shrink-0" />
                        )}
                      </button>
                      <span aria-hidden="true" className="text-zinc-600 shrink-0">·</span>
                      <button
                        type="button"
                        data-testid="active-thread-profile"
                        onClick={() => setHermesInspectorOpen((v) => !v)}
                        title="Switch Hermes profile, model, transport, skills, or cron inline"
                        className="font-mono text-[11px] text-[var(--ui-accent-soft)] hover:text-white transition-colors shrink-0 cursor-pointer"
                      >
                        @{activeProfile}
                      </button>
                      {activeTurnCount > 0 && (
                        <span className="hidden xl:inline font-mono text-[10.5px] text-zinc-500 tabular-nums shrink-0">
                          · {activeTurnCount}t
                        </span>
                      )}
                    </div>

                    <div className="flex items-center gap-2.5 shrink-0 text-[11px] text-zinc-400 whitespace-nowrap">
                      <ThreadTaskBanner
                        variant="thread-trigger"
                        sidebarMode={isDesktopLg}
                        activeChannel={activeChannel}
                        activeSessionId={activeHermesSessionId}
                        tasks={tasks}
                        activeIsThinking={activeIsThinking}
                        workingSeconds={workingSeconds}
                        isOpen={threadTasksOpen}
                        onToggleOpen={toggleThreadTasksOpen}
                        onHide={hideThreadTasks}
                      />
                      {visibleMessages.length > 0 && (
                        <button
                          type="button"
                          data-testid="thread-search-toggle"
                          aria-pressed={threadSearchOpen}
                          onClick={() => {
                            setThreadMoreMenuOpen(false);
                            setThreadSearchOpen((v) => {
                              const next = !v;
                              if (next) {
                                window.setTimeout(() => threadSearchInputRef.current?.focus(), 20);
                              }
                              return next;
                            });
                          }}
                          title="Find in thread (Ctrl/Cmd+F)"
                          className={`inline-flex items-center gap-1 transition-colors cursor-pointer ${
                            threadSearchOpen ? 'text-white' : 'hover:text-white'
                          }`}
                        >
                          <Search className="w-3 h-3" />
                          <span>Find</span>
                        </button>
                      )}
                      <div className="relative">
                        <button
                          type="button"
                          data-testid="thread-more-menu-trigger"
                          aria-expanded={threadMoreMenuOpen}
                          aria-label="Thread options"
                          title="Thread options"
                          onClick={() => setThreadMoreMenuOpen((v) => !v)}
                          className={`w-6 h-6 rounded-md flex items-center justify-center transition-colors cursor-pointer ${
                            threadMoreMenuOpen
                              ? 'bg-white/[0.1] text-white'
                              : 'text-zinc-400 hover:text-white hover:bg-white/[0.06]'
                          }`}
                        >
                          <MoreHorizontal className="w-3.5 h-3.5" />
                        </button>
                        {threadMoreMenuOpen && (
                          <>
                            <div
                              aria-hidden="true"
                              onClick={() => setThreadMoreMenuOpen(false)}
                              className="fixed inset-0 z-30"
                            />
                            <div
                              role="menu"
                              data-testid="thread-more-menu"
                              className="ui-menu-pop absolute right-0 top-[calc(100%+6px)] w-48 rounded-xl border border-white/14 bg-[#12151E]/96 backdrop-blur-xl shadow-[0_16px_36px_rgba(0,0,0,0.62)] p-1 z-40 text-left text-xs"
                            >
                              <button
                                type="button"
                                role="menuitem"
                                data-testid="thread-compact-toggle"
                                onClick={() => {
                                  setCompactThreadHeader(true);
                                  setThreadMoreMenuOpen(false);
                                }}
                                className="w-full px-2.5 py-1.5 rounded-lg text-zinc-200 hover:bg-white/[0.07] hover:text-white flex items-center gap-2 cursor-pointer"
                              >
                                <Minimize2 className="w-3.5 h-3.5 text-zinc-400" />
                                <span>Compact header</span>
                              </button>
                              <button
                                type="button"
                                role="menuitem"
                                onClick={() => {
                                  onToggleThinking();
                                  setThreadMoreMenuOpen(false);
                                }}
                                className="w-full px-2.5 py-1.5 rounded-lg text-zinc-200 hover:bg-white/[0.07] hover:text-white flex items-center justify-between gap-2 cursor-pointer"
                              >
                                <span className="flex items-center gap-2">
                                  <Brain className="w-3.5 h-3.5 text-zinc-400" />
                                  <span>Activity trace</span>
                                </span>
                                <span className="font-mono text-[10px] text-zinc-400">
                                  {showThinking ? 'On' : 'Off'}
                                </span>
                              </button>
                              {onToggleJevRecall && (
                                <button
                                  type="button"
                                  role="menuitem"
                                  data-testid="thread-jev-recall-toggle"
                                  onClick={() => {
                                    onToggleJevRecall();
                                    setThreadMoreMenuOpen(false);
                                  }}
                                  className="w-full px-2.5 py-1.5 rounded-lg text-zinc-200 hover:bg-white/[0.07] hover:text-white flex items-center justify-between gap-2 cursor-pointer"
                                >
                                  <span className="flex items-center gap-2">
                                    <Activity className="w-3.5 h-3.5 text-zinc-400" />
                                    <span>Jev recall graph</span>
                                  </span>
                                  <span className="font-mono text-[10px] text-zinc-400">
                                    {uiConfig.showJevRecall ? 'On' : 'Off'}
                                  </span>
                                </button>
                              )}
                              {visibleMessages.length > 0 && (
                                <button
                                  type="button"
                                  role="menuitem"
                                  data-testid="thread-copy-transcript"
                                  onClick={() => {
                                    handleCopyTranscript();
                                    setThreadMoreMenuOpen(false);
                                  }}
                                  className="w-full px-2.5 py-1.5 rounded-lg text-zinc-200 hover:bg-white/[0.07] hover:text-white flex items-center gap-2 cursor-pointer"
                                >
                                  {copiedTranscript ? (
                                    <Check className="w-3.5 h-3.5 text-[var(--ui-accent-soft)]" />
                                  ) : (
                                    <Copy className="w-3.5 h-3.5 text-zinc-400" />
                                  )}
                                  <span>{copiedTranscript ? 'Copied Markdown' : 'Export Markdown'}</span>
                                </button>
                              )}
                              <div className="my-1 border-t border-white/[0.07]" />
                              <button
                                type="button"
                                role="menuitem"
                                onClick={() => {
                                  onResetSession(activeChannel);
                                  setThreadMoreMenuOpen(false);
                                }}
                                className="w-full px-2.5 py-1.5 rounded-lg text-zinc-300 hover:bg-white/[0.07] hover:text-white flex items-center gap-2 cursor-pointer"
                              >
                                <RotateCcw className="w-3.5 h-3.5 text-zinc-400" />
                                <span>Reset session</span>
                              </button>
                            </div>
                          </>
                        )}
                      </div>
                    </div>
                  </div>
                ) : (
                  <div className="relative z-10 flex flex-wrap items-center justify-center gap-2 mt-0.5 px-4 text-[11.5px] font-medium text-zinc-300">
                    <StatusPulse size="sm" state={statusPulseState} />
                    <span className="text-white/90">{statusText}</span>
                    <span aria-hidden="true" className="text-zinc-600">·</span>
                    <span
                      className="font-mono text-[11px] text-[var(--ui-accent-soft)] max-w-[230px] truncate"
                      title={activeHermesSessionId}
                      data-testid="active-hermes-session-id"
                    >
                      {activeHermesSessionId}
                    </span>
                    {activeProfile && activeProfile !== 'default' && (
                      <>
                        <span aria-hidden="true" className="text-zinc-600">·</span>
                        <span
                          data-testid="active-thread-profile"
                          className="font-mono text-[11px] text-zinc-400"
                        >
                          @{activeProfile}
                        </span>
                      </>
                    )}
                  </div>
                )}

                {panelView === 'voice' && uiConfig.horizonVisible ? (
                  <div
                    aria-hidden="true"
                    className="w-full max-w-[740px] px-8 mt-1.5 h-[22px] overflow-hidden"
                    style={{
                      opacity:
                        activeIsSpeaking || voiceState.isListening || activeIsThinking
                          ? Math.max(0.74, uiConfig.horizonOpacity)
                          : Math.min(0.68, Math.max(0.50, uiConfig.horizonOpacity * 0.85)),
                    }}
                  >
                    <BrailleHorizonCanvas
                      energy={activeIsThinking && !activeIsSpeaking ? 0.28 : energy}
                      isActive={activeIsSpeaking || voiceState.isListening || activeIsThinking}
                      frequencyBands={voiceState.frequencyBands}
                      lookTargetRef={lookTargetRef}
                      tintHex={activeIsThinking && !activeIsSpeaking ? '#F59E0B' : null}
                      dotProfile={uiConfig.brailleDotProfile}
                      bloom={uiConfig.brailleBloom}
                      horizonStyle="ribbon"
                      speedScale={uiConfig.animationSpeed}
                      reducedMotionOverride={uiConfig.reducedMotion}
                    />
                  </div>
                ) : (
                  <div
                    aria-hidden="true"
                    className="w-full max-w-[740px] px-8 mt-2.5"
                  >
                    <div className="h-px w-full bg-gradient-to-r from-transparent via-white/[0.08] to-transparent" />
                  </div>
                )}
              </div>
            ) : (
              panelView === 'voice' && (
                <div
                  data-testid="desktop-thread-compact-bar"
                  className="hidden lg:block shrink-0 border-b border-white/[0.06]"
                >
                  <div className="max-w-[740px] mx-auto w-full px-8 py-2 flex items-center justify-between gap-3 text-xs text-zinc-300">
                    <div className="flex items-center gap-2.5 min-w-0">
                      {uiConfig.portraitVisible && (
                        <div className="relative w-9 h-9 rounded-lg border border-white/[0.09] bg-[#0B0D12]/80 flex items-center justify-center overflow-hidden shrink-0">
                          <div className="scale-[0.42] origin-center">
                            <AsciiPortraitCanvas
                              variant="compact"
                              mood={effectiveMood}
                              viseme={viseme}
                              eyeState={portraitEyeState}
                              energy={energy}
                              isSpeaking={activeIsSpeaking}
                              isListening={voiceState.isListening}
                              lookTargetRef={lookTargetRef}
                              tintHex={effectivePortraitTint}
                              dotProfile={uiConfig.brailleDotProfile}
                              bloom={uiConfig.brailleBloom}
                              speedScale={uiConfig.animationSpeed}
                              reducedMotionOverride={uiConfig.reducedMotion}
                              onStatusChange={setPortraitStatus}
                              onClick={onCycleMood}
                            />
                          </div>
                        </div>
                      )}
                      <div className="min-w-0">
                        <div className="flex items-center gap-2 min-w-0">
                          <StatusPulse size="sm" state={statusPulseState} />
                          <button
                            type="button"
                            onClick={handleCopySessionKey}
                            data-testid="active-hermes-session-id"
                            title={`Copy Hermes session key (${activeHermesSessionId})`}
                            className="font-medium text-white truncate max-w-[240px] hover:text-[var(--ui-accent-soft)] transition-colors cursor-pointer"
                          >
                            {activeDotLabel?.primaryLabel ?? activeHermesSessionId}
                          </button>
                          {copiedSessionKey && (
                            <Check className="w-3 h-3 text-[var(--ui-accent-soft)] shrink-0" />
                          )}
                        </div>
                        <div className="flex items-center gap-1.5 font-mono text-[10.5px] text-zinc-400">
                          <span>{statusText}</span>
                          <span aria-hidden="true" className="text-zinc-600">·</span>
                          <button
                            type="button"
                            data-testid="active-thread-profile"
                            onClick={() => setHermesInspectorOpen((v) => !v)}
                            className="text-[var(--ui-accent-soft)] hover:text-white cursor-pointer"
                          >
                            @{activeProfile}
                          </button>
                          <span aria-hidden="true" className="text-zinc-600">·</span>
                          <button
                            type="button"
                            data-testid="thread-context-pill"
                            onClick={() => {
                              setContextMonitorOpen((v) => !v);
                              if (uiConfig.showJevRecall && onToggleJevRecall) {
                                onToggleJevRecall();
                              }
                            }}
                            title={`Inspect ${modelContext.usage.model.displayName} context window (${modelContext.usage.usedTokens.toLocaleString()} / ${modelContext.usage.contextWindow.toLocaleString()} tokens)`}
                            className="inline-flex items-center gap-1.5 text-zinc-400 hover:text-white transition-colors truncate cursor-pointer"
                          >
                            <span className="truncate">{modelContext.usage.model.displayName}</span>
                            <span aria-hidden="true" className="text-zinc-600">·</span>
                            <span className="tabular-nums text-zinc-300">
                              {modelContext.usage.usedPct.toFixed(1)}% / {formatCompactTokens(modelContext.usage.contextWindow)}
                            </span>
                          </button>
                        </div>
                      </div>
                    </div>

                    <div className="flex items-center gap-2.5 shrink-0 text-[11px] text-zinc-400">
                      <ThreadTaskBanner
                        variant="thread-trigger"
                        sidebarMode={isDesktopLg}
                        activeChannel={activeChannel}
                        activeSessionId={activeHermesSessionId}
                        tasks={tasks}
                        activeIsThinking={activeIsThinking}
                        workingSeconds={workingSeconds}
                        isOpen={threadTasksOpen}
                        onToggleOpen={toggleThreadTasksOpen}
                        onHide={hideThreadTasks}
                      />
                      {visibleMessages.length > 0 && (
                        <button
                          type="button"
                          data-testid="thread-search-toggle"
                          aria-pressed={threadSearchOpen}
                          onClick={() => {
                            setThreadMoreMenuOpen(false);
                            setThreadSearchOpen((v) => {
                              const next = !v;
                              if (next) {
                                window.setTimeout(() => threadSearchInputRef.current?.focus(), 20);
                              }
                              return next;
                            });
                          }}
                          title="Find in thread (Ctrl/Cmd+F)"
                          className={`inline-flex items-center gap-1 transition-colors cursor-pointer ${
                            threadSearchOpen ? 'text-white' : 'hover:text-white'
                          }`}
                        >
                          <Search className="w-3 h-3" />
                          <span>Find</span>
                        </button>
                      )}
                      <div className="relative">
                        <button
                          type="button"
                          data-testid="thread-more-menu-trigger"
                          aria-expanded={threadMoreMenuOpen}
                          aria-label="Thread options"
                          title="Thread options"
                          onClick={() => setThreadMoreMenuOpen((v) => !v)}
                          className={`w-6 h-6 rounded-md flex items-center justify-center transition-colors cursor-pointer ${
                            threadMoreMenuOpen
                              ? 'bg-white/[0.1] text-white'
                              : 'text-zinc-400 hover:text-white hover:bg-white/[0.06]'
                          }`}
                        >
                          <MoreHorizontal className="w-3.5 h-3.5" />
                        </button>
                        {threadMoreMenuOpen && (
                          <>
                            <div
                              aria-hidden="true"
                              onClick={() => setThreadMoreMenuOpen(false)}
                              className="fixed inset-0 z-30"
                            />
                            <div
                              role="menu"
                              data-testid="thread-more-menu"
                              className="ui-menu-pop absolute right-0 top-[calc(100%+6px)] w-48 rounded-xl border border-white/14 bg-[#12151E]/96 backdrop-blur-xl shadow-[0_16px_36px_rgba(0,0,0,0.62)] p-1 z-40 text-left text-xs"
                            >
                              {uiConfig.portraitVisible && stageHeaderSize !== 'strip' && (
                                <button
                                  type="button"
                                  role="menuitem"
                                  data-testid="thread-expand-stage-toggle"
                                  onClick={() => {
                                    setCompactThreadHeader(false);
                                    setThreadMoreMenuOpen(false);
                                  }}
                                  className="w-full px-2.5 py-1.5 rounded-lg text-zinc-200 hover:bg-white/[0.07] hover:text-white flex items-center gap-2 cursor-pointer"
                                >
                                  <Maximize2 className="w-3.5 h-3.5 text-zinc-400" />
                                  <span>Show full stage</span>
                                </button>
                              )}
                              <button
                                type="button"
                                role="menuitem"
                                onClick={() => {
                                  onToggleThinking();
                                  setThreadMoreMenuOpen(false);
                                }}
                                className="w-full px-2.5 py-1.5 rounded-lg text-zinc-200 hover:bg-white/[0.07] hover:text-white flex items-center justify-between gap-2 cursor-pointer"
                              >
                                <span className="flex items-center gap-2">
                                  <Brain className="w-3.5 h-3.5 text-zinc-400" />
                                  <span>Activity trace</span>
                                </span>
                                <span className="font-mono text-[10px] text-zinc-400">
                                  {showThinking ? 'On' : 'Off'}
                                </span>
                              </button>
                              {visibleMessages.length > 0 && (
                                <button
                                  type="button"
                                  role="menuitem"
                                  data-testid="thread-copy-transcript"
                                  onClick={() => {
                                    handleCopyTranscript();
                                    setThreadMoreMenuOpen(false);
                                  }}
                                  className="w-full px-2.5 py-1.5 rounded-lg text-zinc-200 hover:bg-white/[0.07] hover:text-white flex items-center gap-2 cursor-pointer"
                                >
                                  {copiedTranscript ? (
                                    <Check className="w-3.5 h-3.5 text-[var(--ui-accent-soft)]" />
                                  ) : (
                                    <Copy className="w-3.5 h-3.5 text-zinc-400" />
                                  )}
                                  <span>{copiedTranscript ? 'Copied Markdown' : 'Export Markdown'}</span>
                                </button>
                              )}
                              <div className="my-1 border-t border-white/[0.07]" />
                              <button
                                type="button"
                                role="menuitem"
                                onClick={() => {
                                  onResetSession(activeChannel);
                                  setThreadMoreMenuOpen(false);
                                }}
                                className="w-full px-2.5 py-1.5 rounded-lg text-zinc-300 hover:bg-white/[0.07] hover:text-white flex items-center gap-2 cursor-pointer"
                              >
                                <RotateCcw className="w-3.5 h-3.5 text-zinc-400" />
                                <span>Reset session</span>
                              </button>
                            </div>
                          </>
                        )}
                      </div>
                    </div>
                  </div>
                  {uiConfig.horizonVisible && (
                    <div
                      aria-hidden="true"
                      className="w-full max-w-[740px] mx-auto px-8 pb-1 h-[14px] overflow-hidden"
                      style={{ opacity: Math.min(0.6, uiConfig.horizonOpacity * 0.8) }}
                    >
                      <BrailleHorizonCanvas
                        energy={activeIsThinking && !activeIsSpeaking ? 0.28 : energy}
                        isActive={activeIsSpeaking || voiceState.isListening || activeIsThinking}
                        frequencyBands={voiceState.frequencyBands}
                        lookTargetRef={lookTargetRef}
                        tintHex={activeIsThinking && !activeIsSpeaking ? '#F59E0B' : null}
                        dotProfile={uiConfig.brailleDotProfile}
                        bloom={uiConfig.brailleBloom}
                        horizonStyle="ribbon"
                        speedScale={uiConfig.animationSpeed}
                        reducedMotionOverride={uiConfig.reducedMotion}
                      />
                    </div>
                  )}
                </div>
              )
            )}

            {/* Mobile Slim Thread Control Strip & Flowing Acoustic Wave Divider (< lg) */}
            {panelView === 'voice' ? (
              <>
                <div
                  data-testid="mobile-braille-stage"
                  className="px-3.5 pt-0.5 pb-1 flex lg:hidden items-center justify-between gap-2 shrink-0"
                >
                  <div className="flex items-center gap-1.5 min-w-0 text-[11px] text-zinc-400">
                    <StatusPulse state={statusPulseState} size="sm" />
                    <span className="text-zinc-200 font-medium shrink-0">{statusText}</span>
                    <span aria-hidden="true" className="text-zinc-600 shrink-0">·</span>
                    <button
                      type="button"
                      onClick={() => setHermesInspectorOpen((v) => !v)}
                      aria-expanded={hermesInspectorOpen}
                      aria-label="Open inline Hermes Agent controls"
                      className="font-mono text-[11px] text-[var(--ui-accent-soft)] hover:text-white truncate cursor-pointer"
                    >
                      @{activeProfile}
                    </button>
                  </div>

                  <div className="flex items-center gap-1.5 shrink-0">
                    <ThreadTaskBanner
                      variant="mobile-trigger"
                      activeChannel={activeChannel}
                      activeSessionId={activeHermesSessionId}
                      tasks={tasks}
                      activeIsThinking={activeIsThinking}
                      workingSeconds={workingSeconds}
                      isOpen={threadTasksOpen}
                      onToggleOpen={toggleThreadTasksOpen}
                      onHide={hideThreadTasks}
                    />
                    <button
                      type="button"
                      onClick={() => setHermesInspectorOpen((v) => !v)}
                      aria-expanded={hermesInspectorOpen}
                      aria-label="Open Hermes runtime controls"
                      className={`shrink-0 px-2 py-1 rounded-lg text-[10.5px] font-mono border transition-colors cursor-pointer ${
                        hermesInspectorOpen
                          ? 'bg-[rgba(var(--ui-accent-rgb),0.16)] border-[rgba(var(--ui-accent-rgb),0.36)] text-white'
                          : 'bg-white/[0.04] border-white/[0.08] text-zinc-300 hover:text-white'
                      }`}
                    >
                      Hermes
                    </button>
                    {visibleMessages.length > 0 && (
                      <button
                        type="button"
                        onClick={() => {
                          setThreadSearchOpen((v) => {
                            const next = !v;
                            if (next) {
                              window.setTimeout(() => threadSearchInputRef.current?.focus(), 20);
                            }
                            return next;
                          });
                        }}
                        aria-pressed={threadSearchOpen}
                        aria-label="Find in thread"
                        title="Find in thread"
                        className={`shrink-0 p-1.5 rounded-lg border transition-colors cursor-pointer ${
                          threadSearchOpen
                            ? 'bg-[rgba(var(--ui-accent-rgb),0.16)] border-[rgba(var(--ui-accent-rgb),0.36)] text-white'
                            : 'bg-white/[0.04] border-white/[0.08] text-zinc-400 hover:text-white'
                        }`}
                      >
                        <Search className="w-3 h-3" />
                      </button>
                    )}
                  </div>
                </div>

                {uiConfig.horizonVisible && (
                  <div
                    aria-hidden="true"
                    className="lg:hidden w-full px-3 h-[18px] overflow-hidden shrink-0"
                    style={{
                      opacity:
                        activeIsSpeaking || voiceState.isListening || activeIsThinking
                          ? Math.max(0.76, uiConfig.horizonOpacity)
                          : Math.min(0.68, Math.max(0.50, uiConfig.horizonOpacity * 0.85)),
                    }}
                  >
                    <BrailleHorizonCanvas
                      energy={activeIsThinking && !activeIsSpeaking ? 0.28 : energy}
                      isActive={activeIsSpeaking || voiceState.isListening || activeIsThinking}
                      frequencyBands={voiceState.frequencyBands}
                      lookTargetRef={lookTargetRef}
                      tintHex={activeIsThinking && !activeIsSpeaking ? '#F59E0B' : null}
                      dotProfile={uiConfig.brailleDotProfile}
                      bloom={uiConfig.brailleBloom}
                      horizonStyle="ribbon"
                      speedScale={uiConfig.animationSpeed}
                      reducedMotionOverride={uiConfig.reducedMotion}
                    />
                  </div>
                )}

                {!isDesktopLg && threadTasksOpen && (
                  <div className="lg:hidden max-h-[42dvh] overflow-y-auto cyber-scroll px-3.5 py-2.5 pb-3 border-b border-white/[0.09] bg-[#111318]/94 shrink-0">
                    <ThreadTaskBanner
                      variant="thread-inline"
                      activeChannel={activeChannel}
                      activeSessionId={activeHermesSessionId}
                      tasks={tasks}
                      activeIsThinking={activeIsThinking}
                      workingSeconds={workingSeconds}
                      isOpen={threadTasksOpen}
                      onToggleOpen={toggleThreadTasksOpen}
                      onHide={hideThreadTasks}
                      onToggleTaskStatus={onToggleTaskStatus}
                      onConfirmTask={onConfirmTask}
                      onReopenTask={onReopenTask}
                      onCancelTask={onCancelTask}
                      onClearCompleted={onClearCompletedTasks}
                      onSteerTask={onSteerTask}
                      onStopSpeaking={onStopSpeaking}
                    />
                  </div>
                )}
              </>
            ) : (
              uiConfig.horizonVisible && (
                <div
                  aria-hidden="true"
                  className="lg:hidden w-full px-3 h-[18px] overflow-hidden shrink-0"
                  style={{
                    opacity:
                      activeIsSpeaking || voiceState.isListening || activeIsThinking
                        ? Math.max(0.55, uiConfig.horizonOpacity)
                        : Math.min(0.40, uiConfig.horizonOpacity * 0.65),
                  }}
                >
                  <BrailleHorizonCanvas
                    energy={activeIsThinking && !activeIsSpeaking ? 0.26 : energy}
                    isActive={activeIsSpeaking || voiceState.isListening || activeIsThinking}
                    frequencyBands={voiceState.frequencyBands}
                    lookTargetRef={lookTargetRef}
                    tintHex={activeIsThinking && !activeIsSpeaking ? '#F59E0B' : null}
                    dotProfile={uiConfig.brailleDotProfile}
                    bloom={uiConfig.brailleBloom}
                    horizonStyle="ribbon"
                    speedScale={uiConfig.animationSpeed}
                    reducedMotionOverride={uiConfig.reducedMotion}
                  />
                </div>
              )
            )}

            {panelView === 'queue' ? (
              <div key="view-queue" className="ui-view-enter flex-1 min-h-0 px-3 sm:px-8 py-3 sm:py-5 overflow-y-auto cyber-scroll">
                <div className="max-w-[780px] mx-auto w-full pb-4">
                  <HermesOperationsPanel
                    channels={channels}
                    activeChannel={activeChannel}
                    onSelectChannel={onSelectChannel}
                    approvals={approvals}
                    onApproveCard={onApproveCard}
                    onRejectCard={onRejectCard}
                    onClarifyCard={onClarifyCard}
                    brief={brief}
                    onRunBriefCommand={(cmd, id) => {
                      onRunBriefCommand(cmd, id);
                      onChangePanelView('voice');
                    }}
                    gatewayStatus={gatewayStatus}
                    onRefreshGateway={onRefreshGateway}
                    skills={skills}
                    showJevRecall={uiConfig.showJevRecall}
                    onToggleJevRecall={onToggleJevRecall}
                    toolsets={toolsets}
                    cronJobs={cronJobs}
                    transportMode={transportMode}
                    onChangeTransportMode={onChangeTransportMode}
                    onCreateJob={onCreateJob}
                    onPauseJob={onPauseJob}
                    onResumeJob={onResumeJob}
                    onRunJob={onRunJob}
                    onDeleteJob={onDeleteJob}
                    profiles={profiles}
                    selectedProfileId={draftProfile}
                    onSelectDefaultProfile={onChangeDraftProfile}
                    onSaveProfile={onSaveProfile}
                    onDeleteProfile={onDeleteProfile}
                    sessionCountByProfile={sessionCountByProfile}
                  />
                </div>
              </div>
            ) : panelView === 'cli' ? (
              <div key="view-cli" className="ui-view-enter flex-1 min-h-0 px-3 sm:px-8 py-3 sm:py-5 overflow-hidden">
                <CommandTerminal
                  activeChannel={activeChannel}
                  activeSessionId={activeHermesSessionId}
                  entries={entries}
                  isThinking={activeIsThinking}
                  onSubmitInput={(val) => onSubmitInput(val, 'cli')}
                />
              </div>
            ) : (
              <div className="relative flex-1 min-h-0 flex flex-col">
                {hermesInspectorOpen && (
                  <>
                    <div
                      aria-hidden="true"
                      onClick={() => setHermesInspectorOpen(false)}
                      className="absolute inset-0 z-20 bg-black/38 backdrop-blur-[2px]"
                    />
                    <div className="absolute inset-x-0 top-2 px-3.5 sm:px-8 z-30 pointer-events-none">
                      <div className="pointer-events-auto">
                        <InlineHermesPopover
                          open={hermesInspectorOpen}
                          onClose={() => setHermesInspectorOpen(false)}
                          activeChannel={activeChannel}
                          activeSessionId={activeHermesSessionId}
                          activeProfile={activeProfile}
                          isHomeDraft={false}
                          gatewayStatus={gatewayStatus}
                          skills={skills}
                          toolsets={toolsets}
                          cronJobs={cronJobs}
                          approvals={approvals}
                          profiles={profiles}
                          transportMode={transportMode}
                          onRefreshGateway={onRefreshGateway}
                          onChangeTransportMode={onChangeTransportMode}
                          onSelectProfile={(pid) => {
                            if (activeChannel) {
                              onChangeSessionProfile?.(activeChannel, pid);
                            }
                          }}
                          onApproveCard={onApproveCard}
                          onRejectCard={onRejectCard}
                          onPauseJob={onPauseJob}
                          onResumeJob={onResumeJob}
                          onRunJob={onRunJob}
                          onOpenOperations={() => onChangePanelView('queue')}
                          onRunPromptOrCommand={(text) =>
                            onSubmitInput(text, text.startsWith('/') ? 'cli' : 'voice')
                          }
                        />
                      </div>
                    </div>
                  </>
                )}

                {threadSearchOpen && (
                  <div
                    data-testid="thread-search-bar"
                    className="px-3.5 sm:px-8 pt-2 pb-1.5 z-20 shrink-0"
                  >
                    <div className="max-w-[740px] mx-auto w-full flex items-center gap-2 rounded-xl border border-white/14 bg-[#12151E]/95 px-3 py-1.5 shadow-[0_10px_28px_rgba(0,0,0,0.45)]">
                      <Search className="w-3.5 h-3.5 text-zinc-400 shrink-0" aria-hidden="true" />
                      <label htmlFor="thread-search-input" className="sr-only">
                        Find in thread
                      </label>
                      <input
                        id="thread-search-input"
                        ref={threadSearchInputRef}
                        type="text"
                        value={threadSearchQuery}
                        onChange={(e) => setThreadSearchQuery(e.target.value)}
                        onKeyDown={(e) => {
                          if (e.key === 'Escape') {
                            setThreadSearchOpen(false);
                            setThreadSearchQuery('');
                          } else if (e.key === 'Enter') {
                            e.preventDefault();
                            handleStepThreadSearch(e.shiftKey ? 'prev' : 'next');
                          }
                        }}
                        placeholder="Find in thread…"
                        className="flex-1 min-w-0 bg-transparent text-xs text-white placeholder:text-zinc-500 focus:outline-none"
                      />
                      <span className="font-mono text-[10.5px] text-zinc-400 tabular-nums shrink-0">
                        {threadSearchQuery.trim()
                          ? threadSearchMatches.length > 0
                            ? `${Math.min(threadSearchMatchIdx + 1, threadSearchMatches.length)} / ${threadSearchMatches.length}`
                            : '0 matches'
                          : `${visibleMessages.length} msgs`}
                      </span>
                      <div className="flex items-center gap-0.5 shrink-0">
                        <button
                          type="button"
                          disabled={threadSearchMatches.length === 0}
                          onClick={() => handleStepThreadSearch('prev')}
                          aria-label="Previous match"
                          className="w-6 h-6 rounded-md text-zinc-400 hover:text-white hover:bg-white/[0.08] disabled:opacity-35 flex items-center justify-center cursor-pointer"
                        >
                          <ChevronUp className="w-3.5 h-3.5" />
                        </button>
                        <button
                          type="button"
                          disabled={threadSearchMatches.length === 0}
                          onClick={() => handleStepThreadSearch('next')}
                          aria-label="Next match"
                          className="w-6 h-6 rounded-md text-zinc-400 hover:text-white hover:bg-white/[0.08] disabled:opacity-35 flex items-center justify-center cursor-pointer"
                        >
                          <ChevronDown className="w-3.5 h-3.5" />
                        </button>
                        <button
                          type="button"
                          onClick={() => {
                            setThreadSearchOpen(false);
                            setThreadSearchQuery('');
                          }}
                          aria-label="Close thread search"
                          className="w-6 h-6 rounded-md text-zinc-400 hover:text-white hover:bg-white/[0.08] flex items-center justify-center cursor-pointer"
                        >
                          <X className="w-3.5 h-3.5" />
                        </button>
                      </div>
                    </div>
                  </div>
                )}

                <ThreadTurnMinimap
                  turns={minimapTurns}
                  activeTurnId={activeMinimapTurnId}
                  onJumpToTurn={handleJumpToMinimapTurn}
                />

                <div
                  key={`view-voice-${activeChannel}`}
                  ref={scrollRef}
                  onWheel={() => {
                    lastUserScrollAtRef.current = performance.now();
                  }}
                  onTouchMove={() => {
                    lastUserScrollAtRef.current = performance.now();
                  }}
                  onPointerDown={() => {
                    lastUserScrollAtRef.current = performance.now();
                  }}
                  onScroll={() => {
                    const el = scrollRef.current;
                    if (!el) return;
                    const nextTop = el.scrollTop;
                    const target = programmaticTargetTopRef.current;
                    const isNearTarget =
                      target !== null && Math.abs(nextTop - target) <= 8;
                    const isNearEnd =
                      el.scrollHeight - nextTop - el.clientHeight <= 24;
                    const isProgrammatic =
                      target !== null && (isNearTarget || isNearEnd);
                    if (!isProgrammatic || isNearTarget) {
                      programmaticTargetTopRef.current = null;
                    }
                    const hasUserScrollIntent =
                      performance.now() - lastUserScrollAtRef.current < 450;
                    const result = computeThreadScrollPin({
                      prevScrollTop: lastScrollTopRef.current,
                      nextScrollTop: nextTop,
                      scrollHeight: el.scrollHeight,
                      clientHeight: el.clientHeight,
                      isNewVisit: false,
                      isProgrammatic,
                      prevPinned: isPinnedToBottomRef.current,
                      hasUserScrollIntent,
                    });
                    lastScrollTopRef.current = nextTop;
                    isPinnedToBottomRef.current = result.pinned;
                    setIsAwayFromBottom(result.away);
                    setShowTopFade(shouldShowScrollTopFade(nextTop));
                    syncActiveMinimapTurn();
                  }}
                  data-testid="voice-transcript-stream"
                  data-pinned={String(!isAwayFromBottom)}
                  style={{ overflowAnchor: 'none' }}
                  className={`ui-view-enter transcript-fade-mask ${
                    showTopFade ? 'ui-transcript-fade-active' : ''
                  } flex-1 min-h-0 overflow-y-auto cyber-scroll pt-2.5 pb-6 ${
                    minimapTurns.length > 0
                      ? 'pl-3.5 sm:pl-12 lg:pl-14 pr-3.5 sm:pr-8'
                      : 'px-3.5 sm:px-8'
                  }`}
                >
                  <div
                    ref={threadListRef}
                    className="max-w-[740px] mx-auto w-full flex flex-col justify-start space-y-4 sm:space-y-5"
                  >
                    {timelineBlocks.length === 0 && !activeIsThinking && !voiceState.interimTranscript && (
                      <div
                        data-testid="thread-empty-guide"
                        className="my-auto pt-6 sm:pt-10 pb-6 flex flex-col items-center text-center space-y-5"
                      >
                        <div className="space-y-1.5">
                          <h2 className="text-[15px] font-medium text-white/95">
                            {activeChannel
                              ? `Thread · ${activeDotLabel?.primaryLabel ?? activeHermesSessionId}`
                              : 'Ready for a new thread'}
                          </h2>
                          <p className="text-xs text-zinc-400">
                            Messages, live activity traces, and run tasks appear here. Type below or press <span className="font-mono text-zinc-200">/</span> for commands.
                          </p>
                        </div>

                        <div className="flex flex-wrap items-center justify-center gap-2">
                          {CURATED_HOME_STARTERS.slice(0, 2).map((starter) => (
                            <button
                              key={starter.id}
                              type="button"
                              onClick={() => onSubmitInput(starter.prompt, 'voice')}
                              className="group ui-tactile inline-flex items-center gap-1.5 rounded-lg border border-white/[0.09] bg-white/[0.03] hover:border-white/22 hover:bg-white/[0.06] px-3 py-1.5 text-xs text-zinc-200 hover:text-white transition-colors cursor-pointer"
                            >
                              <span>{starter.title}</span>
                              <ArrowUpRight className="w-3 h-3 text-zinc-500 group-hover:text-[var(--ui-accent-soft)] shrink-0" />
                            </button>
                          ))}
                        </div>

                        <div className="flex flex-wrap items-center justify-center gap-3 font-mono text-[11px] text-zinc-500">
                          <button
                            type="button"
                            onClick={() => onSubmitInput('/skills', 'cli')}
                            className="hover:text-zinc-200 transition-colors cursor-pointer"
                          >
                            /skills
                          </button>
                          <span aria-hidden="true">·</span>
                          <button
                            type="button"
                            onClick={() => onSubmitInput('/toolsets', 'cli')}
                            className="hover:text-zinc-200 transition-colors cursor-pointer"
                          >
                            /toolsets
                          </button>
                          <span aria-hidden="true">·</span>
                          <button
                            type="button"
                            onClick={() => onSubmitInput('/gateway', 'cli')}
                            className="hover:text-zinc-200 transition-colors cursor-pointer"
                          >
                            /gateway
                          </button>
                          <span aria-hidden="true">·</span>
                          <button
                            type="button"
                            onClick={() => setHermesInspectorOpen(true)}
                            className="text-[var(--ui-accent-soft)] hover:text-white transition-colors cursor-pointer"
                          >
                            Runtime
                          </button>
                        </div>
                      </div>
                    )}

                    {timelineBlocks.map((block, idx) => {
                      if (block.kind === 'trace') {
                        const hasVisibleTaskBlock =
                          threadTasksOpen &&
                          (activeChannelTasksPartition.running.length > 0 || activeIsThinking);
                        if (block.isLive && block.items.length === 0 && hasVisibleTaskBlock) {
                          return null;
                        }
                        const isMinimapAnchor = minimapAnchorSet.has(block.id);
                        return (
                          <div
                            key={block.id}
                            ref={
                              isMinimapAnchor
                                ? (el) => {
                                    if (el) turnAnchorRefs.current.set(block.id, el);
                                    else turnAnchorRefs.current.delete(block.id);
                                  }
                                : undefined
                            }
                            data-turn-anchor={isMinimapAnchor ? block.id : undefined}
                            className={highlightedTurnId === block.id ? 'ui-turn-jump rounded-xl' : undefined}
                          >
                            <ThreadTraceTree
                              sessionId={activeHermesSessionId}
                              items={block.items}
                              isLive={block.isLive}
                              hideLiveFooter={hasVisibleTaskBlock}
                              defaultExpanded={uiConfig.activityTraceExpanded}
                            />
                          </div>
                        );
                      }

                      const entry = block.entry;
                      const isUser = entry.role === 'user' || entry.role === 'command';
                      const isCommandTurn = entry.role === 'command';
                      const isMinimapAnchor = minimapAnchorSet.has(entry.id);
                      const isJumpHighlighted =
                        highlightedTurnId === entry.id || activeSearchEntryId === entry.id;
                      const setEntryRef = (el: HTMLDivElement | null) => {
                        if (el) {
                          entryDomRefs.current.set(entry.id, el);
                          if (isMinimapAnchor) turnAnchorRefs.current.set(entry.id, el);
                        } else {
                          entryDomRefs.current.delete(entry.id);
                          if (isMinimapAnchor) turnAnchorRefs.current.delete(entry.id);
                        }
                      };
                      const activeHighlightQuery = threadSearchOpen ? threadSearchQuery : '';

                      if (isUser) {
                        return (
                          <div
                            key={entry.id}
                            ref={setEntryRef}
                            data-turn-anchor={isMinimapAnchor ? entry.id : undefined}
                            data-testid={isCommandTurn ? 'thread-command-entry' : 'thread-user-entry'}
                            className="group/user ml-auto max-w-[88%] sm:max-w-[74%] flex flex-col items-end gap-1"
                          >
                            <div
                              className={`ui-msg-enter w-fit max-w-full rounded-2xl border px-3.5 sm:px-4 py-2 sm:py-2.5 leading-relaxed text-white ${
                                isCommandTurn
                                  ? 'bg-[#141822]/90 border-[rgba(var(--ui-accent-rgb),0.32)] font-mono text-[12.5px]'
                                  : 'bg-white/[0.07] border-white/[0.14] text-[13.5px]'
                              } ${isJumpHighlighted ? 'ui-turn-jump' : ''}`}
                            >
                              {isCommandTurn ? (
                                <div className="flex items-center gap-2">
                                  <Terminal className="w-3.5 h-3.5 text-[var(--ui-accent-soft)] shrink-0" />
                                  <span className="whitespace-pre-wrap break-words">
                                    {renderHighlightedText(entry.text, activeHighlightQuery)}
                                  </span>
                                </div>
                              ) : (
                                <p className="whitespace-pre-wrap break-words">
                                  {renderHighlightedText(entry.text, activeHighlightQuery)}
                                </p>
                              )}
                            </div>

                            <div className="flex items-center gap-2.5 px-1 font-mono text-[10px] text-zinc-500 tabular-nums opacity-75 sm:opacity-0 sm:group-hover/user:opacity-100 focus-within:opacity-100 transition-opacity">
                              {entry.timestamp && <span>{entry.timestamp}</span>}
                              <button
                                type="button"
                                onClick={() => handleEditPromptInComposer(entry.text)}
                                aria-label="Edit prompt in composer"
                                className="inline-flex items-center gap-1 hover:text-zinc-200 transition-colors cursor-pointer"
                              >
                                <Pencil className="w-2.5 h-2.5" />
                                <span>Edit</span>
                              </button>
                              <button
                                type="button"
                                onClick={() =>
                                  onSubmitInput(entry.text, isCommandTurn ? 'cli' : 'voice')
                                }
                                aria-label="Re-run prompt"
                                className="inline-flex items-center gap-1 hover:text-zinc-200 transition-colors cursor-pointer"
                              >
                                <RotateCcw className="w-2.5 h-2.5" />
                                <span>Re-run</span>
                              </button>
                              <button
                                type="button"
                                onClick={() => handleCopyEntryText(entry.id, entry.text)}
                                aria-label="Copy prompt"
                                className="inline-flex items-center gap-1 hover:text-zinc-200 transition-colors cursor-pointer"
                              >
                                {copiedEntryId === entry.id ? (
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
                            </div>
                          </div>
                        );
                      }
                      const outcomeLabel =
                        entry.terminalOutcome === 'iteration_budget_exhausted'
                          ? 'Budget limit'
                          : entry.terminalOutcome === 'cancelled'
                          ? 'Cancelled'
                          : entry.terminalOutcome === 'interrupted'
                          ? 'Interrupted'
                          : entry.terminalOutcome === 'admission_conflict'
                          ? 'Conflict'
                          : entry.errorCode === 'gateway_offline'
                          ? 'Gateway offline'
                          : entry.terminalOutcome === 'failed'
                          ? 'Failed'
                          : null;
                      const isErrorDiagnostic =
                        entry.errorCode === 'gateway_offline' ||
                        entry.terminalOutcome === 'failed' ||
                        entry.terminalOutcome === 'admission_conflict';
                      const turnRetryPrompt = findPrecedingUserPrompt(scopedEntries, entry.id);
                      return (
                        <div
                          key={entry.id}
                          ref={setEntryRef}
                          data-turn-anchor={isMinimapAnchor ? entry.id : undefined}
                          data-testid="thread-assistant-entry"
                          className={`group/msg ui-msg-enter max-w-[68ch] ${
                            isErrorDiagnostic
                              ? 'rounded-xl bg-rose-500/[0.05] border border-rose-400/20 px-3.5 py-2.5 space-y-2'
                              : 'space-y-1.5'
                          } ${isJumpHighlighted ? 'ui-turn-jump rounded-xl' : ''}`}
                        >
                          <ThreadMessageContent
                            text={entry.text}
                            animateDecode={!entry.id.startsWith('init-') && !entry.streaming}
                            isErrorDiagnostic={isErrorDiagnostic}
                            highlightQuery={activeHighlightQuery}
                          />
                          {isErrorDiagnostic && (
                            <div className="flex flex-wrap items-center gap-2 pt-0.5">
                              {(turnRetryPrompt || lastUserPromptInThread) && (
                                <button
                                  type="button"
                                  onClick={() =>
                                    onSubmitInput(turnRetryPrompt || lastUserPromptInThread, 'voice')
                                  }
                                  className="ui-tactile px-2.5 py-1 rounded-md border border-white/14 bg-white/[0.06] text-[11px] font-mono text-white hover:bg-white/12 transition-colors cursor-pointer"
                                >
                                  Retry prompt
                                </button>
                              )}
                              {onRefreshGateway && (
                                <button
                                  type="button"
                                  onClick={() => void onRefreshGateway()}
                                  className="ui-tactile px-2.5 py-1 rounded-md border border-white/12 bg-white/[0.04] text-[11px] font-mono text-zinc-200 hover:text-white transition-colors cursor-pointer"
                                >
                                  Probe gateway
                                </button>
                              )}
                              <button
                                type="button"
                                onClick={() => onChangePanelView('queue')}
                                className="ui-tactile px-2.5 py-1 rounded-md border border-white/10 text-[11px] font-mono text-zinc-300 hover:text-white transition-colors cursor-pointer"
                              >
                                Gateway &amp; Tools
                              </button>
                            </div>
                          )}
                          {(idx > 0 || outcomeLabel || !isErrorDiagnostic) && (
                            <div className="flex flex-wrap items-center gap-2.5 font-mono text-[10.5px] text-zinc-500 tabular-nums">
                              {idx > 0 && entry.timestamp && <span>{entry.timestamp}</span>}
                              {outcomeLabel && (
                                <span
                                  data-testid="entry-outcome-badge"
                                  className={`px-1.5 py-0.5 rounded border text-[10px] ${
                                    entry.terminalOutcome === 'iteration_budget_exhausted'
                                      ? 'border-amber-400/35 bg-amber-500/12 text-amber-200'
                                      : entry.terminalOutcome === 'cancelled' || entry.terminalOutcome === 'interrupted'
                                      ? 'border-white/20 bg-white/[0.06] text-zinc-300'
                                      : 'border-rose-400/35 bg-rose-500/12 text-rose-200'
                                  }`}
                                >
                                  {outcomeLabel}
                                </span>
                              )}
                              {!isErrorDiagnostic && !entry.streaming && entry.text.trim() && (
                                <div className="inline-flex items-center gap-2.5 opacity-75 sm:opacity-0 sm:group-hover/msg:opacity-100 focus-within:opacity-100 transition-opacity">
                                  <button
                                    type="button"
                                    onClick={() => handleCopyEntryText(entry.id, entry.text)}
                                    aria-label="Copy response"
                                    className="inline-flex items-center gap-1 text-zinc-500 hover:text-zinc-200 transition-colors cursor-pointer"
                                  >
                                    {copiedEntryId === entry.id ? (
                                      <>
                                        <Check className="w-3 h-3 text-[var(--ui-accent-soft)]" />
                                        <span className="text-[var(--ui-accent-soft)]">Copied</span>
                                      </>
                                    ) : (
                                      <>
                                        <Copy className="w-3 h-3" />
                                        <span>Copy</span>
                                      </>
                                    )}
                                  </button>
                                  {turnRetryPrompt && (
                                    <button
                                      type="button"
                                      onClick={() => onSubmitInput(turnRetryPrompt, 'voice')}
                                      aria-label="Retry this turn"
                                      className="inline-flex items-center gap-1 text-zinc-500 hover:text-zinc-200 transition-colors cursor-pointer"
                                    >
                                      <RotateCcw className="w-2.5 h-2.5" />
                                      <span>Retry</span>
                                    </button>
                                  )}
                                  <button
                                    type="button"
                                    onClick={() => composerInputRef.current?.focus()}
                                    aria-label="Reply in thread"
                                    className="inline-flex items-center gap-1 text-zinc-500 hover:text-zinc-200 transition-colors cursor-pointer"
                                  >
                                    <CornerDownRight className="w-2.5 h-2.5" />
                                    <span>Follow up</span>
                                  </button>
                                </div>
                              )}
                            </div>
                          )}
                        </div>
                      );
                    })}

                    {voiceState.interimTranscript && (
                      <div className="ml-auto max-w-[85%] sm:max-w-[72%] rounded-2xl bg-[rgba(var(--ui-accent-rgb),0.14)] border border-[rgba(var(--ui-accent-rgb),0.36)] px-3.5 sm:px-4 py-2 sm:py-2.5 text-[13.5px] leading-relaxed text-zinc-100">
                        <p className="whitespace-pre-wrap break-words">{voiceState.interimTranscript}</p>
                      </div>
                    )}

                    {/* Inline Pending Run Approval / Clarification Card */}
                    {channelPendingApprovals.length > 0 && (() => {
                      const card = channelPendingApprovals[0];
                      const isClarify = card.kind === 'clarify';
                      const draftVal = clarifyDraft[card.id] ?? '';
                      return (
                        <div
                          data-testid={`approval-card-${card.id}`}
                          className="max-w-[66ch] rounded-xl bg-amber-500/[0.06] border border-amber-400/30 p-3 space-y-2.5"
                        >
                          <div className="flex items-start justify-between gap-2.5">
                            <button
                              type="button"
                              onClick={() => onChangePanelView('queue')}
                              className="flex items-center gap-2 min-w-0 text-left hover:text-[var(--ui-accent-soft)] transition-colors cursor-pointer"
                            >
                              <RefreshCw className="w-3.5 h-3.5 text-amber-300 shrink-0" aria-hidden="true" />
                              <span className="text-xs sm:text-[13px] font-medium text-white truncate">
                                {card.title}
                              </span>
                            </button>
                            <span className="font-mono text-[10px] text-amber-200/80 shrink-0">
                              {card.mcpTool || 'approval'}
                            </span>
                          </div>

                          {(card.command || card.summary) && (
                            <pre className="rounded-lg bg-black/40 border border-white/10 px-2.5 py-1.5 font-mono text-[11px] text-[var(--ui-accent-soft)] whitespace-pre-wrap break-all">
                              {card.command || card.summary}
                            </pre>
                          )}

                          {isClarify && onClarifyCard ? (
                            <div className="flex items-center gap-2">
                              <input
                                type="text"
                                value={draftVal}
                                onChange={(e) =>
                                  setClarifyDraft((prev) => ({ ...prev, [card.id]: e.target.value }))
                                }
                                placeholder="Provide answer to continue run..."
                                className="flex-1 min-w-0 rounded-lg bg-black/40 border border-white/15 px-2.5 py-1.5 font-mono text-xs text-white focus:outline-none focus:border-[var(--ui-accent)]"
                              />
                              <button
                                type="button"
                                disabled={!draftVal.trim()}
                                onClick={() => onClarifyCard(card.id, draftVal)}
                                className="ui-accent-btn h-8 px-3 rounded-lg font-semibold text-xs hover:opacity-90 disabled:opacity-45 cursor-pointer"
                              >
                                Send
                              </button>
                            </div>
                          ) : (
                            <div className="flex flex-wrap items-center gap-2">
                              <button
                                type="button"
                                data-testid={`thread-approve-once-${card.id}`}
                                onClick={() => onApproveCard(card.id, 'once')}
                                className="ui-accent-btn h-8 px-3 rounded-lg font-semibold text-xs inline-flex items-center gap-1 hover:opacity-90 transition-opacity cursor-pointer"
                              >
                                <Check className="w-3.5 h-3.5" />
                                <span>Once</span>
                              </button>
                              <button
                                type="button"
                                data-testid={`thread-approve-always-${card.id}`}
                                onClick={() => onApproveCard(card.id, 'always')}
                                className="h-8 px-3 rounded-lg bg-[rgba(var(--ui-accent-rgb),0.16)] border border-[rgba(var(--ui-accent-rgb),0.36)] text-[var(--ui-accent-soft)] font-semibold text-xs inline-flex items-center gap-1 hover:bg-[rgba(var(--ui-accent-rgb),0.24)] transition-colors cursor-pointer"
                              >
                                <span>Always</span>
                              </button>
                              <button
                                type="button"
                                data-testid={`thread-approve-deny-${card.id}`}
                                onClick={() => onRejectCard(card.id)}
                                className="h-8 px-3 rounded-lg bg-rose-500/12 border border-rose-400/30 text-rose-200 hover:bg-rose-500/22 text-xs inline-flex items-center gap-1 cursor-pointer"
                              >
                                <X className="w-3.5 h-3.5" />
                                <span>Deny</span>
                              </button>
                            </div>
                          )}
                        </div>
                      );
                    })()}

                    <div ref={bottomAnchorRef} aria-hidden="true" className="h-px shrink-0" />
                  </div>
                </div>

                <ScrollToBottomButton
                  visible={isAwayFromBottom}
                  onClick={() => {
                    isPinnedToBottomRef.current = true;
                    setIsAwayFromBottom(false);
                    snapThreadToBottom('auto');
                  }}
                  className="absolute bottom-3 right-4 sm:right-10 z-20"
                />
              </div>
            )}

            {isThreadSplitLayout && composerDock}
          </div>

          {/* Dedicated Right Tasks Sidebar (Split on >=900px stage, Overlay drawer on narrower desktop stages) */}
          {showDesktopThreadSidebar && (
            <ThreadTaskBanner
              variant="thread-sidebar"
              overlayMode={threadTasksDockMode === 'overlay'}
              activeChannel={activeChannel}
              activeSessionId={activeHermesSessionId}
              tasks={tasks}
              activeIsThinking={activeIsThinking}
              workingSeconds={workingSeconds}
              isOpen={threadTasksOpen}
              onToggleOpen={toggleThreadTasksOpen}
              onHide={hideThreadTasks}
              onToggleTaskStatus={onToggleTaskStatus}
              onConfirmTask={onConfirmTask}
              onReopenTask={onReopenTask}
              onCancelTask={onCancelTask}
              onClearCompleted={onClearCompletedTasks}
              onSteerTask={onSteerTask}
              onStopSpeaking={onStopSpeaking}
            />
          )}
        </div>
      )}

      {/* Fluid Full-Bottom Braille Wave Horizon inside the Main Window (Home Overview) */}
      {isHomeOverview && uiConfig.horizonVisible && (
        <div
          data-testid="horizon-wave-container"
          className="pointer-events-none absolute inset-x-0 bottom-0 h-[242px] sm:h-[272px] lg:h-[296px] z-0 overflow-hidden"
          style={{ opacity: uiConfig.horizonOpacity }}
        >
          <BrailleHorizonCanvas
            energy={energy}
            isActive={activeIsSpeaking || voiceState.isListening}
            frequencyBands={voiceState.frequencyBands}
            lookTargetRef={lookTargetRef}
            dotProfile={uiConfig.brailleDotProfile}
            bloom={uiConfig.brailleBloom}
            horizonStyle={uiConfig.horizonStyle}
            speedScale={uiConfig.animationSpeed}
            reducedMotionOverride={uiConfig.reducedMotion}
          />
        </div>
      )}

      {/* Floating Glass Composer Dock for Home, Queue, CLI, and non-split Mobile Thread */}
      {!isThreadSplitLayout && composerDock}
    </section>
  );
}
