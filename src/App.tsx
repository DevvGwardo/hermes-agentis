import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { ShieldCheck } from 'lucide-react';
import {
  loadBackgroundSettings,
  publishBackgroundSettings,
  subscribeBackgroundSettings,
} from './backgrounds/persistence';
import type { BackgroundSettings } from './backgrounds/types';
import {
  ONBOARDING_COMPLETED_EVENT,
  loadOnboardingCompleted,
} from './onboarding/trigger';
import { QuickJumpModal } from './components/QuickJumpModal';
import { SidebarWorkspace } from './components/SidebarWorkspace';
import { UiCustomizerStudio, type StudioTab } from './components/UiCustomizerStudio';
import { VoiceWorkspacePane, type WorkspacePanelView } from './components/VoiceWorkspacePane';
import {
  buildQuickJumpItems,
  resolveWorkspaceShortcut,
  type QuickJumpItem,
} from './utils/navigationModel';
import { useHermesAgent, hermesItemToEntry } from './hooks/useHermesAgent';
import { useVoiceAgent } from './hooks/useVoiceAgent';
import type {
  ApprovalCard,
  BackgroundTask,
  BriefDecision,
  Channel,
  ChannelId,
  ExpressionMood,
  TerminalEntry,
} from './types/hermes';
import type { CreateDotSessionPayload } from './components/AddSessionShapeModal';
import {
  arrangeSessionsFormation,
  autoArrangeSessions,
  createHermesSessionId,
  findOpenCanvasSpot,
  normalizeCanvasPins,
  normalizeCanvasSessions,
  pruneAndMigrateCanvasSessions,
  slugifySessionName,
  type CanvasFormationPreset,
  type CanvasPin,
  type CanvasSession,
} from './utils/canvasSessions';
import { classifyGatewaySessionOrigin } from './utils/hermesProtocol';
import { executeCliInput } from './utils/cliEngine';
import {
  deriveSessionFromPrompt,
  pickNextShapeAndTint,
  shouldAutoEnterThreadOnSubmit,
  shouldStartNewHomeSession,
} from './utils/homeSession';
import { mergeTasksWithSessionActivity } from './utils/taskInsights';
import {
  hasCustomizedPreferences,
  resolveBootHydrationSource,
  type AppUpdateStatus,
  type PreferenceBackupSummary,
  type UpdateStepResult,
  type UserPreferencesSnapshot,
} from './utils/settingsSnapshot';
import {
  acknowledgeUpdateReceipt,
  applyAppUpdateWithSnapshot,
  applySnapshotToBrowserStorage,
  collectSnapshotFromBrowser,
  createPreferencesBackup,
  fetchAppUpdateStatus,
  fetchPreferencesVault,
  readPreferencesCapturedAt,
  restorePreferencesBackup,
  savePreferencesToVault,
  writePreferencesCapturedAt,
} from './utils/settingsVaultClient';
import {
  UI_PATCH_EVENT,
  applyUiPatch,
  hydrateUiConfigOnBoot,
  parseNaturalUiIntent,
  publishUiConfig,
  subscribeUiCustomizerOpen,
  type UiCustomizationConfig,
  type UiCustomizationPatch,
} from './utils/uiCustomization';

const INITIAL_UI_CONFIG = hydrateUiConfigOnBoot();
const MOOD_CYCLE: ExpressionMood[] = ['neutral', 'happy', 'focused', 'thinking', 'alert'];

function nowTime(): string {
  const d = new Date();
  return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
}

const CANVAS_STORAGE_KEY = 'duet.canvasSessions.v2';
const CANVAS_PINS_STORAGE_KEY = 'duet.canvasPins.v1';

const CANVAS_DISMISSED_KEY = 'duet.dismissedCanvasIds.v1';

export default function App() {
  const [activeChannel, setActiveChannel] = useState<ChannelId>('');
  const [approvals, setApprovals] = useState<ApprovalCard[]>([]);
  const [brief, setBrief] = useState<BriefDecision[]>([]);
  const [tasks, setTasks] = useState<BackgroundTask[]>([]);
  const [channels, setChannels] = useState<Channel[]>([]);
  const dismissedChannelIdsRef = useRef<Set<string>>(
    (() => {
      try {
        const raw = localStorage.getItem(CANVAS_DISMISSED_KEY);
        if (raw) {
          const parsed = JSON.parse(raw) as unknown;
          if (Array.isArray(parsed)) {
            return new Set(parsed.filter((v): v is string => typeof v === 'string'));
          }
        }
      } catch {
        // ignore storage errors
      }
      return new Set<string>();
    })()
  );
  const [canvasSessions, setCanvasSessions] = useState<CanvasSession[]>(() => {
    try {
      const raw = localStorage.getItem(CANVAS_STORAGE_KEY);
      if (raw) {
        const parsed = JSON.parse(raw) as Partial<CanvasSession>[];
        if (Array.isArray(parsed) && parsed.length > 0) {
          const normalized = normalizeCanvasSessions(parsed);
          return pruneAndMigrateCanvasSessions(normalized, [], {
            migrateLegacySeeds: true,
          }).sessions;
        }
      }
    } catch {
      // ignore storage errors
    }
    return [];
  });
  const [canvasPins, setCanvasPins] = useState<CanvasPin[]>(() => {
    try {
      const raw = localStorage.getItem(CANVAS_PINS_STORAGE_KEY);
      if (raw) {
        const parsed = JSON.parse(raw) as Partial<CanvasPin>[];
        if (Array.isArray(parsed)) {
          return normalizeCanvasPins(parsed);
        }
      }
    } catch {
      // ignore storage errors
    }
    return [];
  });
  const [mood, setMood] = useState<ExpressionMood>('happy');
  const [panelView, setPanelView] = useState<WorkspacePanelView>(() => {
    if (typeof window !== 'undefined') {
      if (window.location.search.includes('view=canvas')) return 'canvas';
      if (window.location.search.includes('autostart=1')) return 'voice';
    }
    return 'home';
  });
  const [homeSessionDraft, setHomeSessionDraft] = useState<boolean>(() => {
    if (typeof window !== 'undefined') {
      if (window.location.search.includes('view=canvas') || window.location.search.includes('autostart=1')) {
        return false;
      }
    }
    return true;
  });
  const [mobileSidebarOpen, setMobileSidebarOpen] = useState(false);
  const [selectedItemId, setSelectedItemId] = useState<string | null>(null);
  const [threadVisitNonce, setThreadVisitNonce] = useState(0);
  const [animateSpeechLine, setAnimateSpeechLine] = useState(false);

  const [thinkingByChannel, setThinkingByChannel] = useState<Record<string, boolean>>({});
  const [confirmedSyntheticTaskIds, setConfirmedSyntheticTaskIds] = useState<Set<string>>(
    () => new Set()
  );
  const [dismissedTaskIds, setDismissedTaskIds] = useState<Set<string>>(() => new Set());
  const [speakingChannel, setSpeakingChannel] = useState<ChannelId | null>(null);
  const [lastSpokenByChannel, setLastSpokenByChannel] = useState<Record<string, string>>({});
  const [lastSpokenLine, setLastSpokenLineState] = useState<string>(
    'Connecting to Hermes Agent…'
  );

  const [entries, setEntries] = useState<TerminalEntry[]>([]);
  const [draftProfile, setDraftProfile] = useState<string>('default');
  const [homeCommandOutput, setHomeCommandOutput] = useState<{
    command: string;
    output: string;
  } | null>(null);
  const [uiConfig, setUiConfig] = useState<UiCustomizationConfig>(INITIAL_UI_CONFIG);
  const [customizerOpen, setCustomizerOpen] = useState(false);
  const [quickJumpOpen, setQuickJumpOpen] = useState(false);
  const [customizerTab, setCustomizerTab] = useState<StudioTab>('theme');
  const [canUndoUi, setCanUndoUi] = useState(false);
  const uiHistoryRef = useRef<Array<{ uiConfig: UiCustomizationConfig; bg: BackgroundSettings }>>([]);
  const lastUndoPushAtRef = useRef(0);
  const lastPatchSigRef = useRef<{ sig: string; at: number } | null>(null);

  // Durable preferences vault + click-to-update state
  const [onboardingCompletedState, setOnboardingCompletedState] = useState<boolean>(() =>
    loadOnboardingCompleted()
  );
  const [bgSyncNonce, setBgSyncNonce] = useState(0);
  const [vaultHydrated, setVaultHydrated] = useState(false);
  const [vaultStorageLabel, setVaultStorageLabel] = useState('Server preferences vault');
  const [vaultLastSyncedAt, setVaultLastSyncedAt] = useState<number | null>(null);
  const [vaultBackups, setVaultBackups] = useState<PreferenceBackupSummary[]>([]);
  const [updateStatus, setUpdateStatus] = useState<AppUpdateStatus | null>(null);
  const [checkingUpdate, setCheckingUpdate] = useState(false);
  const [applyingUpdate, setApplyingUpdate] = useState(false);
  const [updateSteps, setUpdateSteps] = useState<UpdateStepResult[]>([]);
  const [updateError, setUpdateError] = useState<string | null>(null);
  const [busyBackup, setBusyBackup] = useState(false);
  const vaultHydratingRef = useRef(false);

  const stateRef = useRef({
    activeChannel,
    channels,
    panelView,
    homeSessionDraft,
    draftProfile,
    approvals,
    tasks,
    brief,
    entries,
    canvasSessions,
    uiConfig,
  });
  stateRef.current = {
    activeChannel,
    channels,
    panelView,
    homeSessionDraft,
    draftProfile,
    approvals,
    tasks,
    brief,
    entries,
    canvasSessions,
    uiConfig,
  };

  const chatAbortsRef = useRef<Map<string, AbortController>>(new Map());

  const voiceRef = useRef<{
    isMuted: boolean;
    speakText: (text: string, mood: ExpressionMood) => Promise<void>;
    setThinking: (thinking: boolean) => void;
    toggleMute: () => void;
    stopSpeaking: () => void;
  } | null>(null);

  const updateSpokenLine = useCallback((line: string, channelId?: ChannelId) => {
    const ch = channelId ?? stateRef.current.activeChannel;
    setAnimateSpeechLine(true);
    setLastSpokenByChannel((prev) => ({ ...prev, [ch]: line }));
    if (
      ch === stateRef.current.activeChannel &&
      !(stateRef.current.homeSessionDraft && stateRef.current.panelView === 'home')
    ) {
      setLastSpokenLineState(line);
    }
  }, []);

  const upsertEntry = useCallback((entry: TerminalEntry) => {
    setEntries((prev) => {
      const idx = prev.findIndex((e) => e.id === entry.id);
      if (idx >= 0) {
        const next = [...prev];
        const prevCreatedAt = next[idx].createdAtMs;
        next[idx] = entry;
        if (entry.createdAtMs !== undefined && prevCreatedAt !== entry.createdAtMs) {
          next.sort((a, b) => (a.createdAtMs ?? Infinity) - (b.createdAtMs ?? Infinity));
        }
        return next;
      }
      if (entry.createdAtMs !== undefined) {
        const next = [...prev, entry];
        next.sort((a, b) => (a.createdAtMs ?? Infinity) - (b.createdAtMs ?? Infinity));
        return next;
      }
      return [...prev, entry];
    });
  }, []);

  const patchEntryText = useCallback((id: string, text: string) => {
    setEntries((prev) => prev.map((e) => (e.id === id ? { ...e, text } : e)));
  }, []);

  const appendEntry = useCallback(
    (entry: Omit<TerminalEntry, 'id' | 'timestamp'>) => {
      setEntries((prev) => [
        ...prev,
        {
          ...entry,
          id: `${Date.now()}-${Math.random().toString(36).slice(2, 7)}`,
          timestamp: nowTime(),
          createdAtMs: Date.now(),
        },
      ]);
    },
    []
  );

  const setChannelThinking = useCallback((channelId: ChannelId, isThinking: boolean) => {
    setThinkingByChannel((prev) => {
      if (Boolean(prev[channelId]) === isThinking) return prev;
      return { ...prev, [channelId]: isThinking };
    });
    if (channelId === stateRef.current.activeChannel) {
      voiceRef.current?.setThinking(isThinking);
    }
  }, []);

  const applyAgentUiPatch = useCallback(
    (rawPatch: UiCustomizationPatch, sourceLabel = 'hermes') => {
      const current = stateRef.current.uiConfig;
      const { next, sanitized, changedKeys, summary } = applyUiPatch(current, rawPatch);
      const sig = JSON.stringify(sanitized);
      const now = Date.now();
      if (
        sourceLabel === 'hermes' &&
        lastPatchSigRef.current &&
        lastPatchSigRef.current.sig === sig &&
        now - lastPatchSigRef.current.at < 1500
      ) {
        return { changed: false, summary };
      }
      lastPatchSigRef.current = { sig, at: now };

      const prevBg = loadBackgroundSettings();

      if (sanitized.openCustomizer) {
        setCustomizerOpen(true);
      }
      if (sanitized.closeCustomizer) {
        setCustomizerOpen(false);
      }
      if (sanitized.activeView) {
        setPanelView(sanitized.activeView);
        setMobileSidebarOpen(false);
      }
      if (sanitized.mood) {
        setMood(sanitized.mood);
      }
      if (sanitized.showThinking !== undefined) {
        const nextThink = sanitized.showThinking;
        setShowThinking(nextThink);
        try {
          localStorage.setItem('duet.showInlineTrace', nextThink ? '1' : '0');
        } catch {
          // ignore
        }
      }
      if (
        sanitized.voiceMuted !== undefined &&
        voiceRef.current &&
        voiceRef.current.isMuted !== sanitized.voiceMuted
      ) {
        voiceRef.current.toggleMute();
      }
      if (sanitized.canvasAutoArrange) {
        setCanvasSessions((prev) => autoArrangeSessions(prev));
      }
      if (sanitized.canvasMoodAll) {
        const targetMood = sanitized.canvasMoodAll;
        setMood(targetMood);
        setCanvasSessions((prev) => prev.map((s) => ({ ...s, mood: targetMood })));
      }

      const bgChanged =
        sanitized.backgroundPresetId !== undefined ||
        sanitized.backgroundOpacity !== undefined ||
        sanitized.glassBlurPx !== undefined ||
        Boolean(sanitized.resetAll);

      if (bgChanged) {
        publishBackgroundSettings({
          ...prevBg,
          activeId: sanitized.backgroundPresetId ?? prevBg.activeId,
          opacity: sanitized.backgroundOpacity ?? prevBg.opacity,
          blurPx: sanitized.glassBlurPx ?? prevBg.blurPx,
        });
      }

      if (changedKeys.length > 0) {
        const shouldPushUndo =
          sourceLabel !== 'studio' || now - lastUndoPushAtRef.current > 700;
        if (shouldPushUndo) {
          uiHistoryRef.current = [
            ...uiHistoryRef.current.slice(-19),
            { uiConfig: current, bg: prevBg },
          ];
          lastUndoPushAtRef.current = now;
          setCanUndoUi(true);
        }
        stateRef.current.uiConfig = next;
        setUiConfig(next);
        publishUiConfig(next);
      }

      return { changed: changedKeys.length > 0, summary };
    },
    []
  );

  const handleUndoUi = useCallback(() => {
    const prev = uiHistoryRef.current.pop();
    if (!prev) return;
    setCanUndoUi(uiHistoryRef.current.length > 0);
    stateRef.current.uiConfig = prev.uiConfig;
    setUiConfig(prev.uiConfig);
    publishUiConfig(prev.uiConfig);
    publishBackgroundSettings(prev.bg);
  }, []);

  useEffect(() => {
    const unsubOpen = subscribeUiCustomizerOpen((tab) => {
      if (tab) setCustomizerTab(tab);
      setCustomizerOpen(true);
    });
    const onExternalPatch = (ev: Event) => {
      const customEv = ev as CustomEvent<UiCustomizationPatch>;
      if (customEv.detail) {
        applyAgentUiPatch(customEv.detail, 'external');
      }
    };
    window.addEventListener(UI_PATCH_EVENT, onExternalPatch);
    return () => {
      unsubOpen();
      window.removeEventListener(UI_PATCH_EVENT, onExternalPatch);
    };
  }, [applyAgentUiPatch]);

  // Live Hermes Agent gateway state, streaming runs, approvals, and SSE updates.
  const hermes = useHermesAgent({
    upsertEntry,
    patchEntryText,
    onUiPatch: (patch) => {
      applyAgentUiPatch(patch, 'hermes');
    },
    onRunActive: (channelId) => {
      setChannelThinking(channelId, true);
    },
    onRunSettled: (channelId) => {
      setChannelThinking(channelId, false);
    },
    onSessionStatus: (channelId, status) => {
      const busy = status === 'thinking' || status === 'speaking' || status === 'running';
      setChannelThinking(channelId, busy);
    },
    onAssistantComplete: (text, channelId) => {
      setChannelThinking(channelId, false);
      updateSpokenLine(text, channelId);
      const onHomeDraft =
        stateRef.current.homeSessionDraft && stateRef.current.panelView === 'home';
      if (channelId === stateRef.current.activeChannel && !onHomeDraft) {
        setMood('happy');
        setSpeakingChannel(channelId);
        void voiceRef.current?.speakText(text, 'happy');
      }
    },
    onSessionReset: (channelId) => {
      setChannelThinking(channelId, false);
      setEntries((prev) => prev.filter((e) => e.channelId !== channelId));
    },
  });
  const {
    connected: hermesConnected,
    gatewayStatus,
    profiles,
    refreshGateway,
    saveProfile,
    deleteProfile,
    skills,
    toolsets,
    cronJobs,
    transportMode,
    setTransportMode,
    sendMessage: hermesSendMessage,
    abortRun,
    createHermesSession,
    loadChannelHistory,
    resetHermesSession,
    deleteHermesSession,
    actOnRunApproval,
    clarifyRun,
    createJob,
    pauseJob,
    resumeJob,
    runJob,
    deleteJob,
    confirmTask: hermesConfirmTask,
    reopenTask: hermesReopenTask,
    cancelTask: hermesCancelTask,
    steerTask: hermesSteerTask,
  } = hermes;

  useEffect(() => {
    if (!hermesConnected || !activeChannel) return;
    if (homeSessionDraft && panelView === 'home') return;
    void loadChannelHistory(activeChannel);
  }, [hermesConnected, activeChannel, homeSessionDraft, panelView, loadChannelHistory]);

  // Persist Braille Canvas sessions & workspace pins locally
  useEffect(() => {
    try {
      localStorage.setItem(CANVAS_STORAGE_KEY, JSON.stringify(canvasSessions));
    } catch {
      // ignore storage quota errors
    }
  }, [canvasSessions]);

  useEffect(() => {
    try {
      localStorage.setItem(CANVAS_PINS_STORAGE_KEY, JSON.stringify(canvasPins));
    } catch {
      // ignore storage quota errors
    }
  }, [canvasPins]);

  // Sync ONLY workspace-owned gateway sessions into canvasSessions and prune legacy/external dots
  useEffect(() => {
    if (hermes.channels.length === 0 && hermes.sessions.length === 0) return;
    setCanvasSessions((prev) => {
      const pruned = pruneAndMigrateCanvasSessions(prev, hermes.sessions, {
        migrateLegacySeeds: true,
      });
      const baseList = pruned.sessions;
      const chById = new Map(hermes.channels.map((c) => [c.id, c]));
      let changed = pruned.changed;

      const updated = baseList.map((cs) => {
        const ch = chById.get(cs.channelId);
        if (!ch) return cs;
        const nextName = ch.name || cs.name;
        const nextTopic = ch.description || cs.topic;
        const nextSid = ch.sessionId || cs.hermesSessionId;
        const nextProfile = ch.profile ?? cs.profile;
        if (
          nextName !== cs.name ||
          nextTopic !== cs.topic ||
          nextSid !== cs.hermesSessionId ||
          nextProfile !== cs.profile
        ) {
          changed = true;
          return {
            ...cs,
            name: nextName,
            topic: nextTopic,
            hermesSessionId: nextSid,
            profile: nextProfile,
          };
        }
        return cs;
      });

      const existingIds = new Set(updated.map((s) => s.channelId));
      const existingRowIds = new Set(updated.map((s) => s.id));
      const additions: CanvasSession[] = [];

      for (const ch of hermes.channels) {
        if (existingIds.has(ch.id)) continue;
        if (dismissedChannelIdsRef.current.has(ch.id)) continue;
        const origin =
          ch.origin ??
          classifyGatewaySessionOrigin({
            sessionId: ch.sessionId,
            sessionKey: ch.sessionKey,
            channelId: ch.id,
          });
        if (origin !== 'workspace') continue;

        existingIds.add(ch.id);
        const visual = pickNextShapeAndTint([...updated, ...additions]);
        const spot = findOpenCanvasSpot([...updated, ...additions]);
        let rowId = `session-${ch.id}`;
        if (existingRowIds.has(rowId)) {
          rowId = `session-${ch.id}-${additions.length + 1}`;
        }
        existingRowIds.add(rowId);
        additions.push({
          id: rowId,
          channelId: ch.id,
          hermesSessionId: ch.sessionId || createHermesSessionId(ch.id),
          profile: ch.profile,
          agentPersona: ch.persona || 'Hermes Agent session synced from gateway.',
          name: ch.name || ch.id,
          topic: ch.description || ch.name || ch.id,
          shape: visual.shape,
          tint: visual.tint,
          xPct: spot.xPct,
          yPct: spot.yPct,
          mood: 'happy',
          eyeStyle: visual.eyeStyle,
          isCore: false,
          origin: 'workspace',
          autonomyMode: 'manual',
          scheduleLabel: 'On-demand',
          sandboxEnv: 'cloud-sandbox',
          workspaceDir: `~/hermes/dots/${ch.id}`,
          skills: [],
        });
      }

      if (additions.length > 0) {
        if (updated.length === 0) {
          return autoArrangeSessions(additions);
        }
        return [...updated, ...additions];
      }
      return changed ? updated : prev;
    });
  }, [hermes.channels, hermes.sessions]);

  // Sync user-created workspace canvas sessions to the server hermes-agent registry.
  const syncedSessionsRef = useRef<Set<string>>(new Set());
  useEffect(() => {
    if (!hermesConnected) return;
    for (const cs of canvasSessions) {
      if (cs.isCore !== false || cs.origin !== 'workspace') continue;
      if (syncedSessionsRef.current.has(cs.channelId)) continue;
      syncedSessionsRef.current.add(cs.channelId);
      if (!hermes.sessions.some((s) => s.channelId === cs.channelId)) {
        void createHermesSession({
          channelId: cs.channelId,
          name: cs.name,
          sessionId: cs.hermesSessionId,
          topic: cs.topic,
          persona: cs.agentPersona,
          profile: cs.profile,
        }).catch(() => {});
      }
    }
  }, [hermesConnected, canvasSessions, hermes.sessions, createHermesSession]);

  // Mirror real Hermes Agent sessions/channels from gateway without hardcoded fallbacks
  useEffect(() => {
    const merged: Channel[] = [...hermes.channels];
    for (const cs of canvasSessions) {
      if (!merged.some((c) => c.id === cs.channelId)) {
        merged.push({
          id: cs.channelId,
          name: cs.name,
          sessionId: cs.hermesSessionId,
          profile: cs.profile,
          description: cs.topic,
          persona: cs.agentPersona,
          origin: cs.origin === 'imported' ? 'external' : 'workspace',
          unreadCount: 0,
        });
      }
    }
    setChannels(merged);
    if (merged.length > 0) {
      const firstWorkspace = merged.find((c) => c.origin === 'workspace');
      setActiveChannel((prev) =>
        prev && merged.some((c) => c.id === prev) ? prev : firstWorkspace?.id ?? ''
      );
    } else {
      setActiveChannel('');
    }
  }, [hermes.channels, canvasSessions]);
  useEffect(() => setApprovals(hermes.approvals), [hermes.approvals]);
  useEffect(() => setTasks(hermes.tasks), [hermes.tasks]);
  useEffect(() => setBrief(hermes.brief), [hermes.brief]);

  const effectiveTasks = useMemo(
    () =>
      mergeTasksWithSessionActivity({
        tasks: tasks.filter((t) => !dismissedTaskIds.has(t.id)),
        entries,
        sessions: hermes.sessions,
        thinkingByChannel,
        channels,
        confirmedTaskIds: confirmedSyntheticTaskIds,
      }).filter((t) => !dismissedTaskIds.has(t.id)),
    [tasks, entries, hermes.sessions, thinkingByChannel, channels, confirmedSyntheticTaskIds, dismissedTaskIds]
  );
  stateRef.current.tasks = effectiveTasks;

  useEffect(() => {
    if (homeSessionDraft && panelView === 'home') {
      setLastSpokenLineState(
        hermes.initialLoading
          ? 'Connecting to Hermes Agent…'
          : 'New Hermes Agent session ready — type or speak a prompt to begin.'
      );
      return;
    }
    const scopedLatest = [...entries]
      .reverse()
      .find((e) => e.channelId === activeChannel && e.role === 'hermes' && e.text.trim().length > 0);
    const cached = lastSpokenByChannel[activeChannel];
    if (cached) {
      setLastSpokenLineState(cached);
    } else if (scopedLatest) {
      setLastSpokenLineState(scopedLatest.text.slice(0, 140));
    } else if (!hermes.initialLoading) {
      const cs = canvasSessions.find((s) => s.channelId === activeChannel);
      if (cs) {
        setLastSpokenLineState(`${cs.hermesSessionId} ready · ${cs.topic}`);
      }
    }
  }, [
    activeChannel,
    entries,
    lastSpokenByChannel,
    canvasSessions,
    hermes.initialLoading,
    homeSessionDraft,
    panelView,
  ]);

  const [showThinking, setShowThinking] = useState<boolean>(() => {
    try {
      return localStorage.getItem('duet.showInlineTrace') !== '0';
    } catch {
      return true;
    }
  });
  const handleToggleThinking = useCallback(() => {
    setShowThinking((v) => {
      try {
        localStorage.setItem('duet.showInlineTrace', v ? '0' : '1');
      } catch {
        // ignore
      }
      return !v;
    });
  }, []);

  const handleApproveCard = useCallback(
    async (id: string, choice: 'once' | 'always' | 'deny' = 'once') => {
      const card = stateRef.current.approvals.find((a) => a.id === id || a.requestId === id);
      if (!card) return;
      const targetChannel = card.channelId || stateRef.current.activeChannel || 'general';
      try {
        await actOnRunApproval(card, choice);
        const actionWord =
          choice === 'deny' ? 'Denied' : choice === 'always' ? 'Always allowed' : 'Approved once';
        const msg = `${actionWord}: ${card.title}${card.runId ? ` (run ${card.runId})` : ''}.`;
        setMood(choice === 'deny' ? 'neutral' : 'happy');
        updateSpokenLine(msg, targetChannel);
        appendEntry({
          channelId: targetChannel,
          role: 'system',
          source: 'ui',
          text: msg,
        });
      } catch (err) {
        const errMsg = `Run approval failed for ${card.title}: ${
          err instanceof Error ? err.message : String(err)
        }`;
        setMood('alert');
        appendEntry({
          channelId: targetChannel,
          role: 'activity',
          source: 'ui',
          activityStatus: 'error',
          errorCode: 'approval_failed',
          terminalOutcome: 'failed',
          text: errMsg,
        });
      }
    },
    [actOnRunApproval, appendEntry, updateSpokenLine]
  );

  const handleRejectCard = useCallback(
    (id: string) => {
      void handleApproveCard(id, 'deny');
    },
    [handleApproveCard]
  );

  const handleClarifyCard = useCallback(
    async (id: string, answer: string) => {
      const card = stateRef.current.approvals.find((a) => a.id === id || a.requestId === id);
      if (!card || !answer.trim()) return;
      const targetChannel = card.channelId || stateRef.current.activeChannel || 'general';
      try {
        await clarifyRun(card, answer.trim());
        const msg = `Submitted clarification for ${card.title}.`;
        setMood('happy');
        updateSpokenLine(msg, targetChannel);
        appendEntry({
          channelId: targetChannel,
          role: 'system',
          source: 'ui',
          text: msg,
        });
      } catch (err) {
        const errMsg = `Clarification failed for ${card.title}: ${
          err instanceof Error ? err.message : String(err)
        }`;
        setMood('alert');
        appendEntry({
          channelId: targetChannel,
          role: 'activity',
          source: 'ui',
          activityStatus: 'error',
          errorCode: 'clarify_failed',
          terminalOutcome: 'failed',
          text: errMsg,
        });
      }
    },
    [appendEntry, clarifyRun, updateSpokenLine]
  );

  const handleStartNewHomeSession = useCallback(() => {
    voiceRef.current?.stopSpeaking();
    setSpeakingChannel(null);
    setSelectedItemId(null);
    setHomeCommandOutput(null);
    setHomeSessionDraft(true);
    setPanelView('home');
    setMobileSidebarOpen(false);
    setMood('happy');
    setLastSpokenLineState('New Hermes Agent session ready — type or speak a prompt to begin.');
    window.setTimeout(() => {
      document.getElementById('hermes-voice-input')?.focus();
    }, 40);
  }, []);

  const sendDirectChannelPrompt = useCallback(
    async (
      ch: ChannelId,
      promptText: string,
      source: 'voice' | 'cli',
      meta?: { name?: string; topic?: string; sessionId?: string; persona?: string; profile?: string },
      echoUserIfOffline = true
    ) => {
      const targetShape = stateRef.current.canvasSessions.find((s) => s.channelId === ch);
      const targetChannel = stateRef.current.channels.find((c) => c.id === ch);
      const targetHermesSessionId =
        meta?.sessionId ?? targetShape?.hermesSessionId ?? createHermesSessionId(ch);
      const targetPersona = meta?.persona ?? targetShape?.agentPersona;
      const targetTopic = meta?.topic ?? targetShape?.topic;
      const targetName = meta?.name ?? targetShape?.name ?? ch;
      const targetProfile =
        meta?.profile ?? targetShape?.profile ?? targetChannel?.profile ?? stateRef.current.draftProfile;

      // Client-side UI commands phrased in natural language still work without inventing an agent reply.
      const localIntentPatch = parseNaturalUiIntent(promptText, stateRef.current.uiConfig);

      setChannelThinking(ch, true);
      if (ch === stateRef.current.activeChannel) {
        setMood('thinking');
      }

      if (hermesConnected) {
        try {
          const { item } = await hermesSendMessage(promptText, ch, {
            name: targetName,
            topic: targetTopic,
            sessionId: targetHermesSessionId,
            persona: targetPersona,
            profile: targetProfile,
            transportMode,
          });
          if (item) {
            if (item.ui_patch) {
              applyAgentUiPatch(item.ui_patch, 'hermes');
            }
            const entry = hermesItemToEntry(item);
            if (entry) upsertEntry(entry);
          }
          return;
        } catch (err) {
          setChannelThinking(ch, false);
          if (echoUserIfOffline) {
            appendEntry({
              channelId: ch,
              role: 'user',
              source,
              text: promptText,
            });
          }
          if (localIntentPatch) {
            const outcome = applyAgentUiPatch(localIntentPatch, 'hermes');
            appendEntry({
              channelId: ch,
              role: 'system',
              source,
              text: `Applied client UI change (${outcome.summary}). Gateway request failed: ${
                err instanceof Error ? err.message : String(err)
              }`,
            });
          } else {
            const errMsg = `Hermes Gateway request failed: ${
              err instanceof Error ? err.message : String(err)
            }`;
            setMood('alert');
            updateSpokenLine(errMsg, ch);
            appendEntry({
              channelId: ch,
              role: 'activity',
              source,
              activityStatus: 'error',
              errorCode: 'gateway_offline',
              terminalOutcome: 'failed',
              text: errMsg,
            });
          }
          return;
        }
      }

      // Express backend offline / disconnected: apply pure UI intent if present, never synthesize an agent reply.
      setChannelThinking(ch, false);
      if (localIntentPatch) {
        const outcome = applyAgentUiPatch(localIntentPatch, 'hermes');
        appendEntry({
          channelId: ch,
          role: 'system',
          source,
          text: `Applied client UI change (${outcome.summary}). Backend server is offline — no agent reply generated.`,
        });
        return;
      }

      const offlineMsg = `Hermes backend unreachable (${gatewayStatus?.gatewayUrl || 'http://127.0.0.1:8642'}). Start the server and gateway to send turns.`;
      setMood('alert');
      updateSpokenLine(offlineMsg, ch);
      appendEntry({
        channelId: ch,
        role: 'activity',
        source,
        activityStatus: 'error',
        errorCode: 'gateway_offline',
        terminalOutcome: 'failed',
        text: offlineMsg,
      });
    },
    [
      appendEntry,
      applyAgentUiPatch,
      gatewayStatus?.gatewayUrl,
      hermesConnected,
      hermesSendMessage,
      setChannelThinking,
      transportMode,
      updateSpokenLine,
      upsertEntry,
    ]
  );

  const startHomeSessionAndSend = useCallback(
    async (promptText: string, source: 'voice' | 'cli') => {
      const currentSessions = stateRef.current.canvasSessions;
      const selectedProfile = stateRef.current.draftProfile || 'default';
      const existingIds = new Set<string>([
        ...currentSessions.map((s) => s.channelId),
        ...stateRef.current.channels.map((c) => c.id),
      ]);
      const derived = deriveSessionFromPrompt(promptText, existingIds);
      const visual = pickNextShapeAndTint(currentSessions);
      const spot = findOpenCanvasSpot(currentSessions);

      const provisionalSession: CanvasSession = {
        id: `session-${derived.slug}-${Date.now().toString(36)}`,
        channelId: derived.slug,
        hermesSessionId: derived.hermesSessionId,
        profile: selectedProfile,
        agentPersona: derived.persona,
        name: derived.slug,
        topic: derived.topic,
        shape: visual.shape,
        tint: visual.tint,
        xPct: spot.xPct,
        yPct: spot.yPct,
        mood: 'thinking',
        eyeStyle: visual.eyeStyle,
        isCore: false,
        origin: 'workspace',
        autonomyMode: 'manual',
        scheduleLabel: 'On-demand',
        sandboxEnv: 'cloud-sandbox',
        workspaceDir: `~/hermes-dots/${derived.slug}`,
        skills: [],
      };

      dismissedChannelIdsRef.current.delete(derived.slug);
      try {
        localStorage.setItem(
          CANVAS_DISMISSED_KEY,
          JSON.stringify(Array.from(dismissedChannelIdsRef.current))
        );
      } catch {
        // ignore
      }
      syncedSessionsRef.current.add(derived.slug);
      setHomeCommandOutput(null);
      setHomeSessionDraft(false);
      setCanvasSessions((prev) => [...prev, provisionalSession]);
      setChannels((prev) =>
        prev.some((c) => c.id === derived.slug)
          ? prev
          : [
              ...prev,
              {
                id: derived.slug,
                name: derived.slug,
                sessionId: derived.hermesSessionId,
                profile: selectedProfile,
                description: derived.topic,
                persona: derived.persona,
                origin: 'workspace',
                unreadCount: 0,
              },
            ]
      );
      setActiveChannel(derived.slug);
      setSelectedItemId(provisionalSession.id);
      setPanelView('voice');
      setThreadVisitNonce((n) => n + 1);
      setMobileSidebarOpen(false);
      setMood('thinking');
      setChannelThinking(derived.slug, true);
      setLastSpokenLineState(`Starting ${derived.hermesSessionId}…`);

      let finalChannelId = derived.slug;
      let finalSessionId = derived.hermesSessionId;

      if (hermesConnected) {
        try {
          const created = await createHermesSession({
            channelId: derived.slug,
            name: derived.slug,
            sessionId: derived.hermesSessionId,
            topic: derived.topic,
            persona: derived.persona,
            profile: selectedProfile,
            skipWelcome: true,
            requireFresh: true,
          });
          if (created.resolvedChannelId && created.resolvedChannelId !== derived.slug) {
            finalChannelId = created.resolvedChannelId;
            syncedSessionsRef.current.add(finalChannelId);
          }
          if (created.session?.sessionId) {
            finalSessionId = created.session.sessionId;
          }
          if (finalChannelId !== derived.slug || finalSessionId !== derived.hermesSessionId) {
            setCanvasSessions((prev) =>
              prev.map((s) =>
                s.id === provisionalSession.id
                  ? {
                      ...s,
                      channelId: finalChannelId,
                      name: finalChannelId,
                      hermesSessionId: finalSessionId,
                    }
                  : s
              )
            );
            setChannels((prev) =>
              prev.map((c) =>
                c.id === derived.slug
                  ? { ...c, id: finalChannelId, name: finalChannelId, sessionId: finalSessionId }
                  : c
              )
            );
            setActiveChannel(finalChannelId);
            if (finalChannelId !== derived.slug) {
              setChannelThinking(derived.slug, false);
              setChannelThinking(finalChannelId, true);
            }
          }
        } catch {
          // Proceed with local slug; sendDirectChannelPrompt will surface any gateway errors honestly
        }
      } else {
        appendEntry({
          channelId: finalChannelId,
          role: 'user',
          source,
          text: promptText,
        });
      }

      await sendDirectChannelPrompt(
        finalChannelId,
        promptText,
        source,
        {
          name: finalChannelId,
          topic: derived.topic,
          sessionId: finalSessionId,
          persona: derived.persona,
          profile: selectedProfile,
        },
        hermesConnected
      );
    },
    [
      appendEntry,
      createHermesSession,
      hermesConnected,
      sendDirectChannelPrompt,
      setChannelThinking,
    ]
  );

  const processUserInput = useCallback(
    async (rawText: string, source: 'voice' | 'cli', targetChannelId?: ChannelId) => {
      const trimmed = rawText.trim();
      if (!trimmed) return;

      const {
        activeChannel: currentActive,
        approvals: appList,
        tasks: taskList,
        brief: briefList,
      } = stateRef.current;
      const ch = targetChannelId ?? currentActive;
      const isCommandInput = trimmed.startsWith('/');

      if (
        !targetChannelId &&
        shouldStartNewHomeSession({
          homeSessionDraft: stateRef.current.homeSessionDraft,
          panelView: stateRef.current.panelView,
          rawText: trimmed,
        })
      ) {
        await startHomeSessionAndSend(trimmed, source);
        return;
      }

      if (
        shouldAutoEnterThreadOnSubmit({
          panelView: stateRef.current.panelView,
          rawText: trimmed,
          targetChannelId,
        })
      ) {
        setHomeCommandOutput(null);
        setHomeSessionDraft(false);
        setPanelView('voice');
        setThreadVisitNonce((n) => n + 1);
        setMobileSidebarOpen(false);
      }

      if (!isCommandInput && hermesConnected) {
        // No local echo on the live SSE path: the real item (POST response or SSE)
        // is the single display copy, so transcript order stays chronological.
      } else {
        appendEntry({
          channelId: ch,
          role: isCommandInput ? 'command' : 'user',
          source,
          text: trimmed,
        });
      }

      const result = executeCliInput(trimmed, {
        activeChannel: ch,
        availableChannels: stateRef.current.channels.map((c) => c.id),
        approvals: appList,
        tasks: taskList,
        brief: briefList,
        isMuted: voiceRef.current?.isMuted ?? false,
        skills,
        toolsets,
        cronJobs,
        gatewayConnected: gatewayStatus?.connected,
        gatewayUrl: gatewayStatus?.gatewayUrl,
        activeModel: gatewayStatus?.activeModel,
        gatewayProfile: gatewayStatus?.profile,
        profiles,
        draftProfile: stateRef.current.draftProfile,
        routePrefix: gatewayStatus?.routePrefix,
        healthStatus: gatewayStatus?.healthStatus,
        sessionKeyHeader: gatewayStatus?.sessionKeyHeader,
        capabilities: gatewayStatus?.capabilities,
        transportMode,
        resolvedTransport: gatewayStatus?.resolvedTransport,
        gatewayError: gatewayStatus?.error,
      });

      if (result.type === 'handled') {
        if (result.startNewSession) {
          if (result.newSessionPrompt) {
            await startHomeSessionAndSend(result.newSessionPrompt, source);
          } else {
            handleStartNewHomeSession();
          }
          return;
        }
        if (result.switchProfile) {
          setDraftProfile(result.switchProfile);
        }
        if (result.uiPatch) {
          applyAgentUiPatch(result.uiPatch, 'cli');
        }
        if (result.setTransportMode) {
          setTransportMode(result.setTransportMode);
        }
        if (result.cancelActiveRun) {
          const current = chatAbortsRef.current.get(ch);
          chatAbortsRef.current.delete(ch);
          current?.abort();
          void abortRun(ch);
          setChannelThinking(ch, false);
          if (ch === stateRef.current.activeChannel) {
            setSpeakingChannel(null);
            voiceRef.current?.stopSpeaking();
          }
        }
        if (result.clearTerminal) {
          setEntries((prev) => prev.filter((e) => e.channelId !== ch));
          setMood('neutral');
          return;
        }
        if (result.switchChannel) {
          setHomeSessionDraft(false);
          setActiveChannel(result.switchChannel);
        }
        if (result.approveCardId) {
          void handleApproveCard(result.approveCardId, result.approveChoice ?? 'once');
          return;
        }
        if (result.rejectCardId) {
          handleRejectCard(result.rejectCardId);
          return;
        }
        if (result.toggleMute) {
          voiceRef.current?.toggleMute();
        }
        if (result.setMood) {
          setMood(result.setMood);
        } else if (ch === stateRef.current.activeChannel) {
          setMood(result.mood);
        }

        appendEntry({
          channelId: result.switchChannel ?? ch,
          role: 'system',
          source: 'cli',
          text: result.output,
        });

        if (stateRef.current.panelView === 'home' && !result.uiPatch?.openCustomizer) {
          setHomeCommandOutput({ command: trimmed, output: result.output });
        }

        if (result.switchChannel) {
          setHomeCommandOutput(null);
          setPanelView('voice');
          setThreadVisitNonce((n) => n + 1);
          setMobileSidebarOpen(false);
        }

        if (result.spokenText) {
          updateSpokenLine(result.spokenText, result.switchChannel ?? ch);
          appendEntry({
            channelId: result.switchChannel ?? ch,
            role: 'hermes',
            source: 'cli',
            mood: result.mood,
            text: result.spokenText,
          });
          if ((result.switchChannel ?? ch) === stateRef.current.activeChannel) {
            voiceRef.current?.speakText(result.spokenText, result.mood);
          }
        } else {
          updateSpokenLine(result.output, result.switchChannel ?? ch);
        }
        return;
      }

      await sendDirectChannelPrompt(ch, result.prompt, source, undefined, true);
    },
    [
      abortRun,
      appendEntry,
      applyAgentUiPatch,
      cronJobs,
      gatewayStatus,
      handleApproveCard,
      handleRejectCard,
      handleStartNewHomeSession,
      hermesConnected,
      profiles,
      sendDirectChannelPrompt,
      setChannelThinking,
      setTransportMode,
      skills,
      startHomeSessionAndSend,
      toolsets,
      transportMode,
      updateSpokenLine,
    ]
  );



  const handleVoiceTranscript = useCallback(
    (transcript: string) => {
      processUserInput(transcript, 'voice');
    },
    [processUserInput]
  );

  const voice = useVoiceAgent(handleVoiceTranscript);
  voiceRef.current = {
    isMuted: voice.voiceState.isMuted,
    speakText: voice.speakText,
    setThinking: voice.setThinking,
    toggleMute: voice.toggleMute,
    stopSpeaking: voice.stopSpeaking,
  };

  useEffect(() => {
    document.title = 'Hermes Agent // Cyber Comms';
  }, []);

  const buildLivePreferencesSnapshot = useCallback(
    (reason: UserPreferencesSnapshot['reason'] = 'sync'): UserPreferencesSnapshot =>
      collectSnapshotFromBrowser(
        {
          appVersion: updateStatus?.currentVersion || '0.1.0',
          uiConfig,
          workspace: {
            showThinking,
            voiceMuted: voice.voiceState.isMuted,
            mood,
            draftProfile,
            transportMode,
            onboardingCompleted: onboardingCompletedState || loadOnboardingCompleted(),
          },
          canvas: {
            sessions: canvasSessions,
            pins: canvasPins,
            dismissedIds: Array.from(dismissedChannelIdsRef.current),
          },
        },
        reason
      ),
    [
      canvasPins,
      canvasSessions,
      draftProfile,
      mood,
      onboardingCompletedState,
      showThinking,
      transportMode,
      uiConfig,
      updateStatus?.currentVersion,
      voice.voiceState.isMuted,
    ]
  );

  const applyRestoredPreferencesSnapshot = useCallback(
    (snapshot: UserPreferencesSnapshot) => {
      vaultHydratingRef.current = true;
      applySnapshotToBrowserStorage(snapshot);
      stateRef.current.uiConfig = snapshot.uiConfig;
      setUiConfig(snapshot.uiConfig);
      setShowThinking(snapshot.workspace.showThinking);
      voice.setMuted(snapshot.workspace.voiceMuted);
      setMood(snapshot.workspace.mood);
      setDraftProfile(snapshot.workspace.draftProfile);
      if (snapshot.workspace.transportMode) {
        setTransportMode(snapshot.workspace.transportMode);
      }
      setOnboardingCompletedState(snapshot.workspace.onboardingCompleted);
      if (snapshot.canvas.sessions.length > 0 || snapshot.canvas.dismissedIds.length > 0) {
        setCanvasSessions(snapshot.canvas.sessions);
      }
      if (snapshot.canvas.pins.length > 0) {
        setCanvasPins(snapshot.canvas.pins);
      }
      dismissedChannelIdsRef.current = new Set(snapshot.canvas.dismissedIds);
      setBgSyncNonce((n) => n + 1);
      window.setTimeout(() => {
        vaultHydratingRef.current = false;
      }, 80);
    },
    [setTransportMode, voice]
  );

  // Track external background customization & onboarding completion for vault sync
  useEffect(() => {
    const unsubBg = subscribeBackgroundSettings(() => {
      if (!vaultHydratingRef.current) {
        setBgSyncNonce((n) => n + 1);
      }
    });
    const onOnboardingDone = () => {
      setOnboardingCompletedState(true);
    };
    window.addEventListener(ONBOARDING_COMPLETED_EVENT, onOnboardingDone);
    return () => {
      unsubBg();
      window.removeEventListener(ONBOARDING_COMPLETED_EVENT, onOnboardingDone);
    };
  }, []);

  // Rehydrate settings & themes from durable server vault on boot (survives updates & port shifts)
  useEffect(() => {
    let cancelled = false;
    void (async () => {
      const [vaultResp, updStatus] = await Promise.all([
        fetchPreferencesVault(),
        fetchAppUpdateStatus(false),
      ]);
      if (cancelled) return;

      if (updStatus) {
        setUpdateStatus(updStatus);
      }
      if (vaultResp) {
        setVaultStorageLabel(vaultResp.storageLabel);
        setVaultBackups(vaultResp.backups);
        const localBase = collectSnapshotFromBrowser(
          {
            appVersion: updStatus?.currentVersion || '0.1.0',
            uiConfig: stateRef.current.uiConfig,
            workspace: {
              showThinking,
              voiceMuted: voiceRef.current?.isMuted ?? false,
              mood,
              draftProfile: stateRef.current.draftProfile,
              transportMode,
              onboardingCompleted: loadOnboardingCompleted(),
            },
            canvas: {
              sessions: stateRef.current.canvasSessions,
              pins: canvasPins,
              dismissedIds: Array.from(dismissedChannelIdsRef.current),
            },
          },
          'sync'
        );
        localBase.capturedAt = readPreferencesCapturedAt();

        const source = resolveBootHydrationSource(localBase, vaultResp.vault);
        if (source === 'vault' && vaultResp.vault) {
          applyRestoredPreferencesSnapshot(vaultResp.vault);
          setVaultLastSyncedAt(vaultResp.vault.capturedAt);
        } else if (vaultResp.vault || hasCustomizedPreferences(localBase)) {
          const synced = await savePreferencesToVault({
            ...localBase,
            capturedAt: Date.now(),
          });
          if (!cancelled && synced.ok) {
            setVaultLastSyncedAt(synced.vault?.capturedAt ?? Date.now());
            writePreferencesCapturedAt(synced.vault?.capturedAt ?? Date.now());
          }
        }
      }
      if (!cancelled) {
        setVaultHydrated(true);
      }
    })();
    return () => {
      cancelled = true;
    };
    // Run once on initial boot
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Debounce-sync customized themes, backdrops & settings to durable vault
  useEffect(() => {
    if (!vaultHydrated || vaultHydratingRef.current) return;
    const timer = window.setTimeout(() => {
      const snap = buildLivePreferencesSnapshot('sync');
      void savePreferencesToVault(snap).then((res) => {
        if (res.ok) {
          const ts = res.vault?.capturedAt ?? snap.capturedAt;
          writePreferencesCapturedAt(ts);
          setVaultLastSyncedAt(ts);
        }
      });
    }, 350);
    return () => window.clearTimeout(timer);
  }, [vaultHydrated, bgSyncNonce, buildLivePreferencesSnapshot]);

  const handleCheckUpdates = useCallback(async () => {
    setCheckingUpdate(true);
    setUpdateError(null);
    const next = await fetchAppUpdateStatus(true);
    if (next) {
      setUpdateStatus(next);
    } else {
      setUpdateError('Unable to reach update service right now.');
    }
    setCheckingUpdate(false);
  }, []);

  const handleApplyUpdate = useCallback(async () => {
    if (applyingUpdate) return;
    setApplyingUpdate(true);
    setUpdateError(null);
    setUpdateSteps([
      {
        id: 'snapshot-prep',
        label: 'Snapshotting active themes, wallpapers & layout settings',
        status: 'ok',
      },
    ]);
    try {
      const snap = buildLivePreferencesSnapshot('pre-update');
      applySnapshotToBrowserStorage(snap);
      await savePreferencesToVault(snap);
      const outcome = await applyAppUpdateWithSnapshot(snap);
      if (outcome.steps.length > 0) {
        setUpdateSteps(outcome.steps);
      }
      if (outcome.status) {
        setUpdateStatus(outcome.status);
      }
      if (!outcome.ok) {
        setUpdateError(outcome.error || 'Update could not complete.');
        setApplyingUpdate(false);
        return;
      }
      const vaultResp = await fetchPreferencesVault();
      if (vaultResp) {
        setVaultBackups(vaultResp.backups);
      }
      setVaultLastSyncedAt(Date.now());
      if (outcome.needsReload) {
        window.setTimeout(() => {
          window.location.reload();
        }, 450);
        return;
      }
    } catch (err) {
      setUpdateError(err instanceof Error ? err.message : 'Update failed cleanly.');
    }
    setApplyingUpdate(false);
  }, [applyingUpdate, buildLivePreferencesSnapshot]);

  const handleCreateBackup = useCallback(async () => {
    setBusyBackup(true);
    const snap = buildLivePreferencesSnapshot('manual');
    const created = await createPreferencesBackup(snap, 'manual');
    if (created) {
      setVaultBackups(created.backups);
      setVaultLastSyncedAt(Date.now());
    }
    setBusyBackup(false);
  }, [buildLivePreferencesSnapshot]);

  const handleRestoreBackup = useCallback(
    async (backupId: string) => {
      setBusyBackup(true);
      const restored = await restorePreferencesBackup(backupId);
      if (restored) {
        applyRestoredPreferencesSnapshot(restored.snapshot);
        setVaultBackups(restored.backups);
        setVaultLastSyncedAt(restored.snapshot.capturedAt);
      }
      setBusyBackup(false);
    },
    [applyRestoredPreferencesSnapshot]
  );

  const handleImportSnapshot = useCallback(
    async (imported: UserPreferencesSnapshot) => {
      setBusyBackup(true);
      applyRestoredPreferencesSnapshot(imported);
      const saved = await savePreferencesToVault(imported);
      if (saved.ok) {
        setVaultLastSyncedAt(saved.vault?.capturedAt ?? Date.now());
      }
      setBusyBackup(false);
    },
    [applyRestoredPreferencesSnapshot]
  );

  const handleDismissUpdateReceipt = useCallback(() => {
    setUpdateStatus((prev) => (prev ? { ...prev, pendingReceipt: null } : prev));
    void acknowledgeUpdateReceipt();
  }, []);

  const handleCycleMood = useCallback(() => {
    setMood((current) => {
      const idx = MOOD_CYCLE.indexOf(current);
      const next = MOOD_CYCLE[(idx + 1) % MOOD_CYCLE.length];
      updateSpokenLine(`Expression shifted to ${next}.`);
      return next;
    });
  }, [updateSpokenLine]);

  const handleToggleTaskStatus = useCallback((id: string) => {
    const updatedMs = Date.now();
    setTasks((prev) =>
      prev.map((t) => {
        if (t.id !== id || t.status === 'completed') return t;
        const nextStatus = t.status === 'running' ? 'paused' : 'running';
        return {
          ...t,
          status: nextStatus,
          rawStatus: nextStatus === 'running' ? 'running' : 'waiting',
          lastStep: nextStatus === 'running' ? 'Lane resumed on host worker' : 'Paused by operator',
          updatedAtMs: updatedMs,
          updatedAt: nowTime(),
        };
      })
    );
  }, []);

  const handleConfirmTask = useCallback(
    (id: string) => {
      if (id.startsWith('live-turn-') || id.startsWith('trace-turn-')) {
        setConfirmedSyntheticTaskIds((prev) => {
          const next = new Set(prev);
          next.add(id);
          return next;
        });
        return;
      }
      const now = Date.now();
      setTasks((prev) =>
        prev.map((t) =>
          t.id === id
            ? {
                ...t,
                status: 'completed',
                rawStatus: 'done',
                agentDoneAtMs: t.agentDoneAtMs ?? now,
                confirmedAtMs: now,
                updatedAtMs: now,
                updatedAt: nowTime(),
              }
            : t
        )
      );
      if (hermesConnected) {
        void hermesConfirmTask(id).catch(() => {});
      }
    },
    [hermesConfirmTask, hermesConnected]
  );

  const handleReopenTask = useCallback(
    (id: string) => {
      if (id.startsWith('live-turn-') || id.startsWith('trace-turn-')) {
        setConfirmedSyntheticTaskIds((prev) => {
          const next = new Set(prev);
          next.delete(id);
          return next;
        });
        return;
      }
      const now = Date.now();
      setTasks((prev) =>
        prev.map((t) =>
          t.id === id
            ? {
                ...t,
                confirmedAtMs: undefined,
                status: t.agentDoneAtMs ? 'completed' : 'paused',
                rawStatus: t.agentDoneAtMs ? 'done' : 'waiting',
                updatedAtMs: now,
                updatedAt: nowTime(),
              }
            : t
        )
      );
      if (hermesConnected) {
        void hermesReopenTask(id).catch(() => {});
      }
    },
    [hermesConnected, hermesReopenTask]
  );

  const handleCancelTask = useCallback(
    (id: string) => {
      if (id.startsWith('live-turn-')) {
        const ch = id.slice('live-turn-'.length);
        const current = chatAbortsRef.current.get(ch);
        chatAbortsRef.current.delete(ch);
        current?.abort();
        void abortRun(ch);
        setChannelThinking(ch, false);
        if (ch === stateRef.current.activeChannel) {
          setSpeakingChannel(null);
          voiceRef.current?.stopSpeaking();
        }
        return;
      }
      if (id.startsWith('trace-turn-')) {
        setDismissedTaskIds((prev) => new Set([...prev, id]));
        return;
      }
      const targetTask = stateRef.current.tasks.find((t) => t.id === id);
      if (id.startsWith('task-turn-') && targetTask) {
        const ch = targetTask.channelId;
        const current = chatAbortsRef.current.get(ch);
        chatAbortsRef.current.delete(ch);
        current?.abort();
        setChannelThinking(ch, false);
        if (ch === stateRef.current.activeChannel) {
          setSpeakingChannel(null);
          voiceRef.current?.stopSpeaking();
        }
      }
      const now = Date.now();
      setTasks((prev) =>
        prev.map((t) =>
          t.id === id
            ? {
                ...t,
                status: 'completed',
                rawStatus: 'cancelled',
                lastStep: 'Cancelled by operator',
                updatedAtMs: now,
                updatedAt: nowTime(),
              }
            : t
        )
      );
      if (hermesConnected) {
        void hermesCancelTask(id).catch(() => {});
      }
    },
    [abortRun, hermesCancelTask, hermesConnected, setChannelThinking]
  );

  const handleClearCompletedTasks = useCallback(
    (channelId: ChannelId) => {
      const toRemove = stateRef.current.tasks
        .filter((t) => t.channelId === channelId && t.status === 'completed' && Boolean(t.confirmedAtMs))
        .map((t) => t.id);
      if (toRemove.length === 0) return;

      const idSet = new Set(toRemove);
      setDismissedTaskIds((prev) => new Set([...prev, ...toRemove]));
      setTasks((prev) => prev.filter((t) => !idSet.has(t.id)));
      if (hermesConnected) {
        void fetch('/api/hermes/tasks/clear', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ channelId }),
        }).catch(() => {});
      }
    },
    [hermesConnected]
  );

  const handleSteerTask = useCallback(
    async (id: string, text: string) => {
      const clean = text.trim();
      if (!clean) return;
      if (id.startsWith('live-turn-') || id.startsWith('trace-turn-')) {
        const ch = id.replace(/^(?:live-turn-|trace-turn-)/, '');
        await sendDirectChannelPrompt(ch, clean, 'cli');
        return;
      }
      const now = Date.now();
      setTasks((prev) =>
        prev.map((t) => {
          if (t.id !== id) return t;
          const nextSteps = [...(t.steps ?? []), { text: `Steer: ${clean}`, done: false }];
          const doneCount = nextSteps.filter((s) => s.done).length;
          return {
            ...t,
            steps: nextSteps,
            progressLabel: `${doneCount}/${nextSteps.length}`,
            progress: Math.round((doneCount / Math.max(1, nextSteps.length)) * 100),
            updatedAtMs: now,
            updatedAt: nowTime(),
          };
        })
      );
      if (hermesConnected) {
        await hermesSteerTask(id, clean).catch(() => {});
      }
    },
    [hermesConnected, hermesSteerTask, sendDirectChannelPrompt]
  );

  const handleRunBriefCommand = useCallback(
    (command: string, briefId: string) => {
      setBrief((prev) =>
        prev.map((b) => (b.id === briefId ? { ...b, resolved: true } : b))
      );
      processUserInput(command, 'cli');
    },
    [processUserInput]
  );

  const handleSelectChannel = useCallback(
    (id: ChannelId, opts?: { openThread?: boolean }) => {
      setHomeSessionDraft(false);
      setActiveChannel(id);
      if (opts?.openThread) {
        setPanelView((prev) => (prev === 'cli' ? 'cli' : 'voice'));
        setThreadVisitNonce((n) => n + 1);
      }
      if (hermesConnected) {
        void loadChannelHistory(id);
      }
      const label = `Tuned active Hermes lane to #${id}.`;
      const scoped = stateRef.current.entries.filter((e) => e.channelId === id);
      if (scoped[scoped.length - 1]?.text !== label) {
        appendEntry({
          channelId: id,
          role: 'system',
          source: 'ui',
          text: label,
        });
      }
    },
    [appendEntry, hermesConnected, loadChannelHistory]
  );

  const handleResetCanvasSession = useCallback(
    async (channelId: string) => {
      voiceRef.current?.stopSpeaking();
      setChannelThinking(channelId, false);
      const target = stateRef.current.canvasSessions.find((s) => s.channelId === channelId);
      const sid = target?.hermesSessionId ?? createHermesSessionId(channelId);
      setEntries((prev) => prev.filter((e) => e.channelId !== channelId));
      const greeting = `Fresh context opened for ${sid} (#${channelId}). Prior messages in this shape were cleared.`;
      updateSpokenLine(greeting, channelId);

      if (hermesConnected) {
        try {
          await resetHermesSession(channelId);
          return;
        } catch {
          // Fall through to local reset message
        }
      }
      appendEntry({
        channelId,
        role: 'hermes',
        source: 'ui',
        mood: 'happy',
        text: greeting,
      });
    },
    [appendEntry, hermesConnected, resetHermesSession, setChannelThinking, updateSpokenLine]
  );

  const handleNewTask = useCallback(() => {
    handleStartNewHomeSession();
  }, [handleStartNewHomeSession]);

  const handleSelectCanvasSession = useCallback(
    (session: CanvasSession) => {
      setHomeSessionDraft(false);
      setActiveChannel(session.channelId);
      setMood(session.mood);
      const latestHermes = [...stateRef.current.entries]
        .reverse()
        .find((e) => e.channelId === session.channelId && e.role === 'hermes' && e.text.trim());
      updateSpokenLine(
        latestHermes
          ? latestHermes.text
          : `${session.hermesSessionId} (${session.name}): ${session.topic}`,
        session.channelId
      );
    },
    [updateSpokenLine]
  );

  const handleMoveCanvasSession = useCallback((id: string, xPct: number, yPct: number) => {
    setCanvasSessions((prev) =>
      prev.map((s) => (s.id === id ? { ...s, xPct, yPct } : s))
    );
  }, []);

  const handleAddCanvasSession = useCallback(
    (payload: CreateDotSessionPayload) => {
      const currentSessions = stateRef.current.canvasSessions;
      const slug = slugifySessionName(
        payload.name,
        currentSessions.map((s) => s.channelId)
      );
      const hermesSessionId = createHermesSessionId(slug);
      const spot = findOpenCanvasSpot(currentSessions);
      const autonomyMode = payload.autonomyMode ?? 'manual';
      const scheduleLabel =
        payload.scheduleLabel ??
        (autonomyMode === 'scheduled'
          ? 'Every 30m'
          : autonomyMode === 'watch'
          ? 'Continuous watch'
          : 'On-demand');
      const targetProfile = payload.profile || stateRef.current.draftProfile || 'default';
      const nextSession: CanvasSession = {
        id: `session-${slug}-${Date.now().toString(36)}`,
        channelId: slug,
        hermesSessionId,
        profile: targetProfile,
        agentPersona: payload.persona,
        name: slug,
        topic: payload.topic,
        shape: payload.shape,
        tint: payload.tint,
        xPct: spot.xPct,
        yPct: spot.yPct,
        mood: 'happy',
        eyeStyle: payload.eyeStyle,
        isCore: false,
        origin: 'workspace',
        autonomyMode,
        scheduleLabel,
        sandboxEnv: payload.sandboxEnv ?? 'cloud-sandbox',
        workspaceDir: `~/hermes-dots/${slug}`,
        skills: payload.skills && payload.skills.length > 0 ? payload.skills : ['browser', 'files'],
      };
      dismissedChannelIdsRef.current.delete(slug);
      try {
        localStorage.setItem(
          CANVAS_DISMISSED_KEY,
          JSON.stringify(Array.from(dismissedChannelIdsRef.current))
        );
      } catch {
        // ignore
      }
      syncedSessionsRef.current.add(slug);
      setHomeSessionDraft(false);
      setCanvasSessions((prev) => [...prev, nextSession]);
      setActiveChannel(slug);
      setMood('happy');

      const msg = `Isolated Dot ${hermesSessionId} online (#${slug}${targetProfile !== 'default' ? ` · @${targetProfile}` : ''} · ${scheduleLabel}). ${payload.topic}`;
      updateSpokenLine(msg, slug);

      if (hermesConnected) {
        void createHermesSession({
          channelId: slug,
          name: slug,
          sessionId: hermesSessionId,
          topic: payload.topic,
          persona: payload.persona,
          profile: targetProfile,
        }).catch(() => {
          appendEntry({
            channelId: slug,
            role: 'hermes',
            source: 'ui',
            mood: 'happy',
            text: msg,
          });
        });
      } else {
        appendEntry({
          channelId: slug,
          role: 'hermes',
          source: 'ui',
          mood: 'happy',
          text: msg,
        });
      }
    },
    [appendEntry, createHermesSession, hermesConnected, updateSpokenLine]
  );

  const handleUpdateDotConfig = useCallback(
    (
      id: string,
      patch: Partial<
        Pick<CanvasSession, 'autonomyMode' | 'scheduleLabel' | 'autonomyPaused' | 'skills' | 'profile'>
      >
    ) => {
      const target = stateRef.current.canvasSessions.find((s) => s.id === id);
      setCanvasSessions((prev) =>
        prev.map((s) => (s.id === id ? { ...s, ...patch } : s))
      );
      if (target && patch.profile !== undefined) {
        setChannels((prev) =>
          prev.map((c) => (c.id === target.channelId ? { ...c, profile: patch.profile } : c))
        );
        if (hermesConnected) {
          void createHermesSession({
            channelId: target.channelId,
            name: target.name,
            sessionId: target.hermesSessionId,
            topic: target.topic,
            persona: target.agentPersona,
            profile: patch.profile,
          }).catch(() => {});
        }
      }
    },
    [createHermesSession, hermesConnected]
  );

  const handleChangeSessionProfile = useCallback(
    (channelId: ChannelId, profileId: string) => {
      const target = stateRef.current.canvasSessions.find((s) => s.channelId === channelId);
      if (target) {
        handleUpdateDotConfig(target.id, { profile: profileId });
        return;
      }
      setChannels((prev) =>
        prev.map((c) => (c.id === channelId ? { ...c, profile: profileId } : c))
      );
      if (hermesConnected) {
        const ch = stateRef.current.channels.find((c) => c.id === channelId);
        void createHermesSession({
          channelId,
          name: ch?.name ?? channelId,
          sessionId: ch?.sessionId ?? createHermesSessionId(channelId),
          topic: ch?.description ?? '',
          persona: ch?.persona ?? '',
          profile: profileId,
        }).catch(() => {});
      }
    },
    [createHermesSession, handleUpdateDotConfig, hermesConnected]
  );

  const handleAddCanvasPin = useCallback((pin: Omit<CanvasPin, 'id'>) => {
    const id = `pin-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`;
    setCanvasPins((prev) => [...prev, { ...pin, id }]);
  }, []);

  const handleMoveCanvasPin = useCallback((id: string, xPct: number, yPct: number) => {
    setCanvasPins((prev) => prev.map((p) => (p.id === id ? { ...p, xPct, yPct } : p)));
  }, []);

  const handleRemoveCanvasPin = useCallback((id: string) => {
    setCanvasPins((prev) => prev.filter((p) => p.id !== id));
  }, []);

  const handleAutoArrangeCanvas = useCallback((formation?: CanvasFormationPreset) => {
    setCanvasSessions((prev) => arrangeSessionsFormation(prev, formation ?? 'constellation'));
  }, []);

  const handleCycleSessionMood = useCallback(
    (id: string) => {
      setCanvasSessions((prev) =>
        prev.map((s) => {
          if (s.id !== id) return s;
          const idx = MOOD_CYCLE.indexOf(s.mood);
          const next = MOOD_CYCLE[(idx + 1) % MOOD_CYCLE.length];
          if (s.channelId === stateRef.current.activeChannel) {
            setMood(next);
            updateSpokenLine(`#${s.name} expression shifted to ${next}.`, s.channelId);
          }
          return { ...s, mood: next };
        })
      );
    },
    [updateSpokenLine]
  );

  const handlePinToCanvas = useCallback(
    (session: CanvasSession) => {
      dismissedChannelIdsRef.current.delete(session.channelId);
      try {
        localStorage.setItem(
          CANVAS_DISMISSED_KEY,
          JSON.stringify(Array.from(dismissedChannelIdsRef.current))
        );
      } catch {
        // ignore
      }
      setCanvasSessions((prev) => {
        if (prev.some((s) => s.channelId === session.channelId)) {
          return prev.map((s) =>
            s.channelId === session.channelId ? { ...s, origin: s.origin ?? 'imported' } : s
          );
        }
        const visual = pickNextShapeAndTint(prev);
        const spot = findOpenCanvasSpot(prev);
        const pinned: CanvasSession = {
          ...session,
          id: `session-${session.channelId}-${Date.now().toString(36)}`,
          shape: visual.shape,
          tint: visual.tint,
          eyeStyle: visual.eyeStyle,
          xPct: spot.xPct,
          yPct: spot.yPct,
          isCore: false,
          origin: 'imported',
        };
        return prev.length === 0 ? autoArrangeSessions([pinned]) : [...prev, pinned];
      });
      setHomeSessionDraft(false);
      setActiveChannel(session.channelId);
      setSelectedItemId(session.id);
      setPanelView('canvas');
      setMobileSidebarOpen(false);
      if (hermesConnected) {
        void loadChannelHistory(session.channelId);
      }
    },
    [hermesConnected, loadChannelHistory]
  );

  const handleRemoveCanvasSession = useCallback(
    (id: string) => {
      const target = stateRef.current.canvasSessions.find((s) => s.id === id);
      if (target) {
        dismissedChannelIdsRef.current.add(target.channelId);
        try {
          localStorage.setItem(
            CANVAS_DISMISSED_KEY,
            JSON.stringify(Array.from(dismissedChannelIdsRef.current))
          );
        } catch {
          // ignore
        }
      }
      setCanvasSessions((prev) => {
        const next = prev.filter((s) => s.id !== id);
        if (target && target.channelId === stateRef.current.activeChannel) {
          setActiveChannel(next[0]?.channelId ?? '');
        }
        return next;
      });
      if (target) {
        setEntries((prev) => prev.filter((e) => e.channelId !== target.channelId));
        if (hermesConnected && target.origin !== 'imported') {
          void deleteHermesSession(target.channelId).catch(() => {});
        }
      }
    },
    [deleteHermesSession, hermesConnected]
  );

  const handleOpenSessionThread = useCallback(
    (session: CanvasSession) => {
      setHomeSessionDraft(false);
      setActiveChannel(session.channelId);
      setMood(session.mood);
      setPanelView('voice');
      setThreadVisitNonce((n) => n + 1);
      setMobileSidebarOpen(false);
      if (hermesConnected) {
        void loadChannelHistory(session.channelId);
      }
    },
    [hermesConnected, loadChannelHistory]
  );

  const handleChangePanelView = useCallback((view: WorkspacePanelView) => {
    setHomeCommandOutput(null);
    if (view === 'home') {
      setSelectedItemId(null);
      setHomeSessionDraft(true);
      setLastSpokenLineState(
        'New Hermes Agent session ready — type or speak a prompt to begin.'
      );
    } else if (view === 'voice') {
      setHomeSessionDraft(false);
      setThreadVisitNonce((n) => n + 1);
    }
    setPanelView(view);
    setMobileSidebarOpen(false);
  }, []);

  const handleToggleSidebar = useCallback(() => {
    applyAgentUiPatch({ sidebarVisible: !stateRef.current.uiConfig.sidebarVisible }, 'studio');
  }, [applyAgentUiPatch]);

  const quickJumpItems = useMemo(() => {
    const pendingCount = approvals.filter((a) => a.status === 'pending').length;
    const activeRunningCount = effectiveTasks.filter(
      (t) => t.channelId === activeChannel && t.status === 'running'
    ).length;
    return buildQuickJumpItems({
      activeView: panelView,
      activeChannel,
      sidebarVisible: uiConfig.sidebarVisible,
      isMuted: voice.voiceState.isMuted,
      gatewayConnected: Boolean(gatewayStatus?.connected),
      pendingApprovalCount: pendingCount,
      activeRunningTaskCount: activeRunningCount,
      sessions: canvasSessions,
      thinkingByChannel,
    });
  }, [
    activeChannel,
    approvals,
    canvasSessions,
    effectiveTasks,
    gatewayStatus?.connected,
    panelView,
    thinkingByChannel,
    uiConfig.sidebarVisible,
    voice.voiceState.isMuted,
  ]);

  const handleExecuteQuickJump = useCallback(
    (item: QuickJumpItem) => {
      const { action } = item;
      if (action.type === 'view') {
        handleChangePanelView(action.view);
        return;
      }
      if (action.type === 'session') {
        const target = canvasSessions.find((s) => s.id === action.sessionId);
        if (target) {
          setSelectedItemId(target.id);
          handleOpenSessionThread(target);
        } else {
          handleSelectChannel(action.channelId, { openThread: true });
        }
        return;
      }
      switch (action.command) {
        case 'new-session':
          handleStartNewHomeSession();
          break;
        case 'toggle-sidebar':
          handleToggleSidebar();
          break;
        case 'open-settings':
          setCustomizerOpen(true);
          break;
        case 'toggle-mute':
          voice.toggleMute();
          break;
        case 'retry-gateway':
          void refreshGateway();
          break;
      }
    },
    [
      canvasSessions,
      handleChangePanelView,
      handleOpenSessionThread,
      handleSelectChannel,
      handleStartNewHomeSession,
      handleToggleSidebar,
      refreshGateway,
      voice,
    ]
  );

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement | null;
      const tag = target?.tagName?.toLowerCase();
      const isEditableTarget =
        Boolean(target?.isContentEditable) ||
        tag === 'input' ||
        tag === 'textarea' ||
        tag === 'select';
      const intent = resolveWorkspaceShortcut({
        key: e.key,
        metaKey: e.metaKey,
        ctrlKey: e.ctrlKey,
        altKey: e.altKey,
        shiftKey: e.shiftKey,
        isEditableTarget,
      });
      if (!intent) return;
      e.preventDefault();
      if (intent.type === 'toggle-quick-jump') {
        setCustomizerOpen(false);
        setQuickJumpOpen((prev) => !prev);
      } else if (intent.type === 'toggle-sidebar') {
        handleToggleSidebar();
      } else if (intent.type === 'open-settings') {
        setQuickJumpOpen(false);
        setCustomizerOpen((prev) => !prev);
      } else if (intent.type === 'switch-view') {
        setQuickJumpOpen(false);
        handleChangePanelView(intent.view);
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [handleChangePanelView, handleToggleSidebar]);

  const rootDensityPadding =
    uiConfig.density === 'compact'
      ? 'p-1 sm:p-2'
      : uiConfig.density === 'airy'
      ? 'p-2.5 sm:p-5 xl:p-6'
      : 'p-1.5 sm:p-3.5 xl:p-4';

  const mainGridClass = !uiConfig.sidebarVisible
    ? 'grid-cols-1'
    : uiConfig.sidebarPosition === 'right'
    ? 'grid-cols-1 lg:grid-cols-[minmax(0,1fr)_min(var(--ui-sidebar-w,296px),260px)] xl:grid-cols-[minmax(0,1fr)_var(--ui-sidebar-w,296px)]'
    : 'grid-cols-1 lg:grid-cols-[min(var(--ui-sidebar-w,296px),260px)_minmax(0,1fr)] xl:grid-cols-[var(--ui-sidebar-w,296px)_minmax(0,1fr)]';

  const gridGapClass =
    uiConfig.density === 'compact' ? 'gap-2' : uiConfig.density === 'airy' ? 'gap-4' : 'gap-3';

  return (
    <div
      className={`h-[100dvh] min-h-[360px] emerald-ambient-bg text-[#ECF3F0] ${rootDensityPadding} flex flex-col overflow-hidden`}
    >
      <main id="top" className={`flex-1 min-h-0 min-w-0 w-full mx-auto grid ${mainGridClass} ${gridGapClass}`}>
        {/* Sidebar Window: Tasks, Approvals, Halftone Thumb & Lanes */}
        {uiConfig.sidebarVisible && (
          <div
            className={`${mobileSidebarOpen ? 'flex' : 'hidden'} lg:flex ${
              uiConfig.sidebarPosition === 'right' ? 'lg:order-2' : 'lg:order-1'
            } flex-col min-h-0 min-w-0 h-full`}
          >
            <SidebarWorkspace
              channels={channels}
              activeChannel={activeChannel}
              onSelectChannel={(id) => {
                handleSelectChannel(id, { openThread: true });
                setMobileSidebarOpen(false);
              }}
              canvasSessions={canvasSessions}
              hermesSessions={hermes.sessions}
              thinkingByChannel={thinkingByChannel}
              onSelectSession={(session) => {
                setSelectedItemId(session.id);
                handleOpenSessionThread(session);
              }}
              onPinToCanvas={handlePinToCanvas}
              approvals={approvals}
              brief={brief}
              tasks={effectiveTasks}
              lastSpokenLine={lastSpokenLine}
              mood={mood}
              frequencyBands={voice.voiceState.frequencyBands}
              isSpeaking={voice.voiceState.isSpeaking}
              isListening={voice.voiceState.isListening}
              inputLevel={voice.voiceState.inputLevel}
              outputLevel={voice.voiceState.outputLevel}
              onCycleMood={handleCycleMood}
              portraitDotProfile={uiConfig.brailleDotProfile}
              portraitBloom={uiConfig.brailleBloom}
              animationSpeed={uiConfig.animationSpeed}
              reducedMotion={uiConfig.reducedMotion}
              portraitTintHex={
                uiConfig.portraitTintHex ??
                (uiConfig.accentPreset === 'slate' ? null : uiConfig.accentSoftHex)
              }
              selectedItemId={selectedItemId}
              onSelectApproval={(card) => {
                setSelectedItemId(card.id);
                setHomeSessionDraft(false);
                setActiveChannel(card.channelId);
                setPanelView('queue');
                setMobileSidebarOpen(false);
              }}
              onSelectBrief={(item) => {
                setSelectedItemId(item.id);
                setHomeSessionDraft(false);
                setPanelView('voice');
                setThreadVisitNonce((n) => n + 1);
                setMobileSidebarOpen(false);
                handleRunBriefCommand(item.suggestedCommand, item.id);
              }}
              onNewTask={handleNewTask}
              onOpenQueue={() => {
                setPanelView((prev) => (prev === 'queue' ? 'home' : 'queue'));
                setMobileSidebarOpen(false);
              }}
              onOpenCanvas={() => handleChangePanelView('canvas')}
              onOpenQuickJump={() => {
                setMobileSidebarOpen(false);
                setQuickJumpOpen(true);
              }}
              onOpenStudio={() => {
                setMobileSidebarOpen(false);
                setCustomizerOpen(true);
              }}
              onCollapseSidebar={handleToggleSidebar}
              onCloseMobile={() => setMobileSidebarOpen(false)}
              workspaceTitle={uiConfig.workspaceTitle}
              gatewayStatus={gatewayStatus}
              homeSessionDraft={homeSessionDraft}
              queueVisible={uiConfig.sidebarQueueVisible}
              channelsVisible={uiConfig.sidebarChannelsVisible}
              sidebarPosition={uiConfig.sidebarPosition}
            />
          </div>
        )}

        {/* Main Workspace Stage */}
        <div
          className={`${mobileSidebarOpen && uiConfig.sidebarVisible ? 'hidden' : 'flex'} lg:flex ${
            uiConfig.sidebarPosition === 'right' ? 'lg:order-1' : 'lg:order-2'
          } flex-col min-h-0 min-w-0 h-full`}
        >
          <VoiceWorkspacePane
            channels={channels}
            activeChannel={activeChannel}
            onSelectChannel={handleSelectChannel}
            mood={mood}
            onCycleMood={handleCycleMood}
            panelView={panelView}
            onChangePanelView={handleChangePanelView}
            threadVisitNonce={threadVisitNonce}
            onOpenMobileSidebar={
              uiConfig.sidebarVisible ? () => setMobileSidebarOpen(true) : undefined
            }
            onToggleSidebar={handleToggleSidebar}
            onOpenQuickJump={() => setQuickJumpOpen(true)}
            voiceState={voice.voiceState}
            lastSpokenLine={lastSpokenLine}
            animateSpeechLine={animateSpeechLine}
            entries={entries}
            approvals={approvals}
            brief={brief}
            tasks={effectiveTasks}
            gatewayStatus={gatewayStatus}
            onRefreshGateway={refreshGateway}
            skills={skills}
            toolsets={toolsets}
            cronJobs={cronJobs}
            transportMode={transportMode}
            onChangeTransportMode={setTransportMode}
            onCreateJob={createJob}
            onPauseJob={pauseJob}
            onResumeJob={resumeJob}
            onRunJob={runJob}
            onDeleteJob={deleteJob}
            onApproveCard={(id, choice) => {
              void handleApproveCard(id, choice ?? 'once');
            }}
            onRejectCard={handleRejectCard}
            onClarifyCard={(id, answer) => {
              void handleClarifyCard(id, answer);
            }}
            onRunBriefCommand={handleRunBriefCommand}
            onToggleTaskStatus={handleToggleTaskStatus}
            onConfirmTask={handleConfirmTask}
            onReopenTask={handleReopenTask}
            onCancelTask={handleCancelTask}
            onClearCompletedTasks={handleClearCompletedTasks}
            onSteerTask={handleSteerTask}
            onToggleListening={voice.toggleListening}
            onToggleMute={voice.toggleMute}
            onStopSpeaking={() => {
              const current = chatAbortsRef.current.get(activeChannel);
              chatAbortsRef.current.delete(activeChannel);
              current?.abort();
              void abortRun(activeChannel);
              setChannelThinking(activeChannel, false);
              setSpeakingChannel(null);
              voice.stopSpeaking();
            }}
            onSubmitInput={(val, src) => processUserInput(val, src)}
            onSubmitSessionInput={(channelId, val, src) =>
              processUserInput(val, src, channelId)
            }
            showThinking={showThinking}
            onToggleThinking={handleToggleThinking}
            canvasSessions={canvasSessions}
            canvasPins={canvasPins}
            hermesSessions={hermes.sessions}
            thinkingByChannel={thinkingByChannel}
            speakingChannel={voice.voiceState.isSpeaking ? speakingChannel : null}
            onSelectCanvasSession={handleSelectCanvasSession}
            onMoveCanvasSession={handleMoveCanvasSession}
            onAddCanvasSession={handleAddCanvasSession}
            onUpdateDotConfig={handleUpdateDotConfig}
            onAddCanvasPin={handleAddCanvasPin}
            onMoveCanvasPin={handleMoveCanvasPin}
            onRemoveCanvasPin={handleRemoveCanvasPin}
            onAutoArrangeCanvas={handleAutoArrangeCanvas}
            onCycleSessionMood={handleCycleSessionMood}
            onResetSession={handleResetCanvasSession}
            onRemoveCanvasSession={handleRemoveCanvasSession}
            onOpenSessionThread={handleOpenSessionThread}
            homeSessionDraft={homeSessionDraft}
            homeCommandOutput={homeCommandOutput}
            onDismissHomeCommandOutput={() => setHomeCommandOutput(null)}
            onStartNewSession={handleStartNewHomeSession}
            profiles={profiles}
            draftProfile={draftProfile}
            onChangeDraftProfile={setDraftProfile}
            onChangeSessionProfile={handleChangeSessionProfile}
            onSaveProfile={saveProfile}
            onDeleteProfile={deleteProfile}
            uiConfig={uiConfig}
            onOpenStudio={() => setCustomizerOpen(true)}
            updateAvailable={Boolean(updateStatus?.updateAvailable)}
          />
        </div>
      </main>

      {updateStatus?.pendingReceipt && !customizerOpen && (
        <div
          role="status"
          data-testid="post-update-preserved-toast"
          className="fixed bottom-4 right-4 z-40 max-w-[360px] rounded-xl border border-white/15 bg-[#14171F]/95 backdrop-blur-xl px-3.5 py-2.5 shadow-2xl flex items-center gap-3 ui-card-enter"
        >
          <div className="w-7 h-7 rounded-lg bg-white/[0.06] border border-white/12 flex items-center justify-center shrink-0">
            <ShieldCheck className="w-4 h-4 text-zinc-200" />
          </div>
          <div className="min-w-0 flex-1">
            <div className="text-xs font-semibold text-white">
              Updated cleanly · settings preserved
            </div>
            <div className="text-[10.5px] font-mono text-zinc-400 truncate">
              v{updateStatus.pendingReceipt.newVersion} · themes &amp; layout restored
            </div>
          </div>
          <div className="flex items-center gap-1.5 shrink-0">
            <button
              type="button"
              onClick={() => {
                setCustomizerTab('updates');
                setCustomizerOpen(true);
              }}
              className="text-[11px] font-medium text-[var(--ui-accent-soft)] hover:text-white px-2 py-1 rounded-md hover:bg-white/[0.06] cursor-pointer"
            >
              Details
            </button>
            <button
              type="button"
              onClick={handleDismissUpdateReceipt}
              aria-label="Dismiss update notification"
              className="text-[11px] text-zinc-400 hover:text-white px-1.5 py-1 rounded-md hover:bg-white/[0.06] cursor-pointer"
            >
              ✕
            </button>
          </div>
        </div>
      )}

      <QuickJumpModal
        open={quickJumpOpen}
        onClose={() => setQuickJumpOpen(false)}
        items={quickJumpItems}
        onExecute={handleExecuteQuickJump}
        isMuted={voice.voiceState.isMuted}
      />

      <UiCustomizerStudio
        open={customizerOpen}
        activeTab={customizerTab}
        onChangeTab={setCustomizerTab}
        onClose={() => setCustomizerOpen(false)}
        config={uiConfig}
        mood={mood}
        showThinking={showThinking}
        voiceMuted={voice.voiceState.isMuted}
        canUndo={canUndoUi}
        onApplyPatch={applyAgentUiPatch}
        onUndo={handleUndoUi}
        onAskAgent={(prompt) => {
          void processUserInput(prompt, 'cli');
        }}
        snapshot={buildLivePreferencesSnapshot('sync')}
        vaultStorageLabel={vaultStorageLabel}
        lastSyncedAt={vaultLastSyncedAt}
        updateStatus={updateStatus}
        checkingUpdate={checkingUpdate}
        applyingUpdate={applyingUpdate}
        updateSteps={updateSteps}
        updateError={updateError}
        backups={vaultBackups}
        busyBackup={busyBackup}
        onCheckUpdates={() => void handleCheckUpdates()}
        onApplyUpdate={() => void handleApplyUpdate()}
        onCreateBackup={() => void handleCreateBackup()}
        onRestoreBackup={(id) => void handleRestoreBackup(id)}
        onImportSnapshot={(snap) => void handleImportSnapshot(snap)}
        onDismissUpdateReceipt={handleDismissUpdateReceipt}
      />
    </div>
  );
}
