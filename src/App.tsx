import { useCallback, useEffect, useRef, useState } from 'react';
import {
  loadBackgroundSettings,
  publishBackgroundSettings,
} from './backgrounds/persistence';
import type { BackgroundSettings } from './backgrounds/types';
import { SidebarWorkspace } from './components/SidebarWorkspace';
import { UiCustomizerStudio, type StudioTab } from './components/UiCustomizerStudio';
import { VoiceWorkspacePane, type WorkspacePanelView } from './components/VoiceWorkspacePane';
import { useDuet, duetItemToEntry } from './hooks/useDuet';
import { useVoiceAgent } from './hooks/useVoiceAgent';
import type {
  ApprovalCard,
  BackgroundTask,
  BriefDecision,
  Channel,
  ChannelId,
  ExpressionMood,
  HermesGatewayStatus,
  TerminalEntry,
} from './types/hermes';
import type { BrailleShapeKind, BrailleShapeTint } from './utils/brailleShapesEngine';
import {
  autoArrangeSessions,
  createHermesSessionId,
  createSeedCanvasSessions,
  findOpenCanvasSpot,
  normalizeCanvasSessions,
  slugifySessionName,
  type CanvasSession,
} from './utils/canvasSessions';
import { INITIAL_CHANNELS, executeCliInput } from './utils/cliEngine';
import {
  deriveSessionFromPrompt,
  pickNextShapeAndTint,
  shouldStartNewHomeSession,
} from './utils/homeSession';
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

function buildLocalFallbackReply(
  prompt: string,
  approvals: ApprovalCard[],
  tasks: BackgroundTask[],
  brief: BriefDecision[]
): { reply: string; mood: ExpressionMood } {
  const lower = prompt.toLowerCase();
  if (lower.includes('brief')) {
    const open = brief.filter((b) => !b.resolved);
    return {
      reply: `Your morning brief has ${open.length} open items. Top priority: ${
        open[0]?.title ?? 'all items resolved'
      }.`,
      mood: 'focused',
    };
  }
  if (lower.includes('task') || lower.includes('status')) {
    const running = tasks.filter((t) => t.status === 'running').length;
    return {
      reply: `Currently tracking ${tasks.length} background lanes, with ${running} actively running and zero faults.`,
      mood: 'focused',
    };
  }
  if (lower.includes('approve')) {
    const nextPending = approvals.find((a) => a.status === 'pending');
    if (nextPending) {
      return {
        reply: `${nextPending.title} is queued for your sign-off. Run /approve ${nextPending.id} or tap Approve to dispatch.`,
        mood: 'happy',
      };
    }
    return {
      reply: 'All approval gates in this lane are currently clear.',
      mood: 'happy',
    };
  }
  return {
    reply: `Hermes link active. Received: "${prompt}". Use /brief, /tasks, or /approve to drive your Duet lanes.`,
    mood: 'happy',
  };
}

const CANVAS_STORAGE_KEY = 'duet.canvasSessions.v2';

export default function App() {
  const [activeChannel, setActiveChannel] = useState<ChannelId>('general');
  const [approvals, setApprovals] = useState<ApprovalCard[]>([]);
  const [brief, setBrief] = useState<BriefDecision[]>([]);
  const [tasks, setTasks] = useState<BackgroundTask[]>([]);
  const [channels, setChannels] = useState<Channel[]>([]);
  const [canvasSessions, setCanvasSessions] = useState<CanvasSession[]>(() => {
    try {
      const raw = localStorage.getItem(CANVAS_STORAGE_KEY);
      if (raw) {
        const parsed = JSON.parse(raw) as Partial<CanvasSession>[];
        if (Array.isArray(parsed) && parsed.length > 0) {
          const normalized = normalizeCanvasSessions(parsed);
          if (normalized.length > 0) return normalized;
        }
      }
    } catch {
      // fall back to seeded sessions
    }
    return createSeedCanvasSessions(INITIAL_CHANNELS);
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
  const [gatewayStatus, setGatewayStatus] = useState<HermesGatewayStatus | null>(null);

  const [thinkingByChannel, setThinkingByChannel] = useState<Record<string, boolean>>({});
  const [speakingChannel, setSpeakingChannel] = useState<ChannelId | null>(null);
  const [lastSpokenByChannel, setLastSpokenByChannel] = useState<Record<string, string>>({});
  const [lastSpokenLine, setLastSpokenLineState] = useState<string>(
    'Linking to Hermes Duet…'
  );

  const [entries, setEntries] = useState<TerminalEntry[]>([]);
  const [uiConfig, setUiConfig] = useState<UiCustomizationConfig>(INITIAL_UI_CONFIG);
  const [customizerOpen, setCustomizerOpen] = useState(false);
  const [customizerTab, setCustomizerTab] = useState<StudioTab>('theme');
  const [canUndoUi, setCanUndoUi] = useState(false);
  const uiHistoryRef = useRef<Array<{ uiConfig: UiCustomizationConfig; bg: BackgroundSettings }>>([]);
  const lastUndoPushAtRef = useRef(0);
  const lastPatchSigRef = useRef<{ sig: string; at: number } | null>(null);

  const stateRef = useRef({
    activeChannel,
    channels,
    panelView,
    homeSessionDraft,
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
        next[idx] = entry;
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
          localStorage.setItem('duet.showThinking', nextThink ? '1' : '0');
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

  // The real engine: live Duet state, runs, approvals, and SSE updates.
  const duet = useDuet({
    upsertEntry,
    patchEntryText,
    onUiPatch: (patch) => {
      applyAgentUiPatch(patch, 'hermes');
    },
    onRunSettled: (channelId) => {
      setChannelThinking(channelId, false);
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
    connected: duetConnected,
    linked: duetLinked,
    sendMessage: duetSendMessage,
    createHermesSession,
    loadChannelHistory,
    resetHermesSession,
    deleteHermesSession,
    actOnCard: duetActOnCard,
  } = duet;

  useEffect(() => {
    if (!duetConnected || !activeChannel) return;
    if (homeSessionDraft && panelView === 'home') return;
    void loadChannelHistory(activeChannel);
  }, [duetConnected, activeChannel, homeSessionDraft, panelView, loadChannelHistory]);

  // Persist Braille Canvas sessions locally
  useEffect(() => {
    try {
      localStorage.setItem(CANVAS_STORAGE_KEY, JSON.stringify(canvasSessions));
    } catch {
      // ignore storage quota errors
    }
  }, [canvasSessions]);

  // Sync any local custom canvas sessions to the server hermes-agent registry on link.
  // Seeded core dots are skipped while linked: they become real Duet lanes lazily
  // on first message instead of mass-creating channels at link time.
  const syncedSessionsRef = useRef<Set<string>>(new Set());
  useEffect(() => {
    if (!duetConnected) return;
    for (const cs of canvasSessions) {
      if (duetLinked && cs.isCore !== false) continue;
      if (syncedSessionsRef.current.has(cs.channelId)) continue;
      syncedSessionsRef.current.add(cs.channelId);
      if (!duet.sessions.some((s) => s.channelId === cs.channelId)) {
        void createHermesSession({
          channelId: cs.channelId,
          name: cs.name,
          sessionId: cs.hermesSessionId,
          topic: cs.topic,
          persona: cs.agentPersona,
        }).catch(() => {});
      }
    }
  }, [duetConnected, duetLinked, canvasSessions, duet.sessions, createHermesSession]);

  // Mirror Duet state locally + merge custom canvas sessions so new Braille sessions act as real lanes.
  useEffect(() => {
    const baseChannels = duet.channels.length > 0 ? duet.channels : INITIAL_CHANNELS;
    const merged: Channel[] = [...baseChannels];
    for (const cs of canvasSessions) {
      if (!merged.some((c) => c.id === cs.channelId)) {
        merged.push({
          id: cs.channelId,
          name: cs.name,
          sessionId: cs.hermesSessionId,
          description: cs.topic,
          persona: cs.agentPersona,
          unreadCount: 0,
        });
      }
    }
    setChannels(merged);
    if (merged.length > 0) {
      setActiveChannel((prev) => (merged.some((c) => c.id === prev) ? prev : merged[0].id));
    }
  }, [duet.channels, canvasSessions]);
  useEffect(() => setApprovals(duet.approvals), [duet.approvals]);
  useEffect(() => setTasks(duet.tasks), [duet.tasks]);
  useEffect(() => setBrief(duet.brief), [duet.brief]);

  useEffect(() => {
    if (!duetConnected) return;
    upsertEntry({
      id: 'duet-link',
      channelId: 'general',
      role: 'system',
      source: 'ui',
      text: 'HERMES DUET LINK ACTIVE // isolated hermes-agent sessions & approval gates wired.',
      timestamp: nowTime(),
    });
  }, [duetConnected, upsertEntry]);

  useEffect(() => {
    if (homeSessionDraft && panelView === 'home') {
      setLastSpokenLineState(
        duet.initialLoading
          ? 'Linking to Hermes Duet…'
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
    } else if (!duet.initialLoading) {
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
    duet.initialLoading,
    homeSessionDraft,
    panelView,
  ]);

  const [showThinking, setShowThinking] = useState<boolean>(() => {
    try {
      return localStorage.getItem('duet.showThinking') !== '0';
    } catch {
      return true;
    }
  });
  const handleToggleThinking = useCallback(() => {
    setShowThinking((v) => {
      try {
        localStorage.setItem('duet.showThinking', v ? '0' : '1');
      } catch {
        // ignore
      }
      return !v;
    });
  }, []);

  useEffect(() => {
    let active = true;
    fetch('/api/hermes/gateway')
      .then((r) => (r.ok ? r.json() : null))
      .then((data: HermesGatewayStatus | null) => {
        if (active && data) setGatewayStatus(data);
      })
      .catch(() => {
        // Standalone fallback remains active cleanly
      });
    return () => {
      active = false;
    };
  }, []);

  const handleApproveCard = useCallback(
    (id: string, editedDraft?: string) => {
      let approvedTitle = '';
      setApprovals((prev) =>
        prev.map((c) => {
          if (c.id === id) {
            approvedTitle = c.title;
            return {
              ...c,
              status: 'approved',
              draft: editedDraft ?? c.draft,
            };
          }
          return c;
        })
      );
      setBrief((prev) =>
        prev.map((b) =>
          b.suggestedCommand.includes(id) ? { ...b, resolved: true } : b
        )
      );
      if (approvedTitle) {
        void duetActOnCard(id, 'approve', editedDraft).catch(() => {});

        const msg = `Approved ${approvedTitle}. Resuming Hermes run with updated payload.`;
        setMood('happy');
        updateSpokenLine(msg);
        appendEntry({
          channelId: stateRef.current.activeChannel,
          role: 'hermes',
          source: 'ui',
          mood: 'happy',
          text: msg,
        });
        voiceRef.current?.speakText(msg, 'happy');
      }
    },
    [appendEntry, updateSpokenLine, duetActOnCard]
  );

  const handleRejectCard = useCallback(
    (id: string) => {
      let rejectedTitle = '';
      setApprovals((prev) =>
        prev.map((c) => {
          if (c.id === id) {
            rejectedTitle = c.title;
            return { ...c, status: 'rejected' };
          }
          return c;
        })
      );
      if (rejectedTitle) {
        void duetActOnCard(id, 'decline').catch(() => {});

        const msg = `Held ${rejectedTitle}. No outbound MCP action was sent.`;
        setMood('neutral');
        updateSpokenLine(msg);
        appendEntry({
          channelId: stateRef.current.activeChannel,
          role: 'hermes',
          source: 'ui',
          mood: 'neutral',
          text: msg,
        });
        voiceRef.current?.speakText(msg, 'neutral');
      }
    },
    [appendEntry, updateSpokenLine, duetActOnCard]
  );

  const handleStartNewHomeSession = useCallback(() => {
    voiceRef.current?.stopSpeaking();
    setSpeakingChannel(null);
    setSelectedItemId(null);
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
      meta?: { name?: string; topic?: string; sessionId?: string; persona?: string },
      echoUserIfOffline = true
    ) => {
      const { approvals: appList, tasks: taskList, brief: briefList } = stateRef.current;
      const targetShape = stateRef.current.canvasSessions.find((s) => s.channelId === ch);
      const targetHermesSessionId =
        meta?.sessionId ?? targetShape?.hermesSessionId ?? createHermesSessionId(ch);
      const targetPersona = meta?.persona ?? targetShape?.agentPersona;
      const targetTopic = meta?.topic ?? targetShape?.topic;
      const targetName = meta?.name ?? targetShape?.name ?? ch;

      setChannelThinking(ch, true);
      if (ch === stateRef.current.activeChannel) {
        setMood('thinking');
      }

      if (duetConnected) {
        try {
          const { item } = await duetSendMessage(promptText, ch, {
            name: targetName,
            topic: targetTopic,
            sessionId: targetHermesSessionId,
            persona: targetPersona,
            uiConfig: stateRef.current.uiConfig,
          });
          if (item) {
            if (item.ui_patch) {
              applyAgentUiPatch(item.ui_patch, 'hermes');
            }
            const entry = duetItemToEntry(item);
            if (entry) upsertEntry(entry);
          }
          return;
        } catch {
          if (echoUserIfOffline) {
            appendEntry({
              channelId: ch,
              role: 'user',
              source,
              text: promptText,
            });
          }
        }
      }

      // Offline fallback: direct gateway chat isolated per-channel so multiple sessions run concurrently.
      chatAbortsRef.current.get(ch)?.abort();
      const controller = new AbortController();
      chatAbortsRef.current.set(ch, controller);
      const timeoutId = window.setTimeout(() => controller.abort(), 300_000);

      setChannelThinking(ch, true);
      if (ch === stateRef.current.activeChannel) {
        setMood('thinking');
      }

      try {
        const res = await fetch('/api/hermes/chat', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          signal: controller.signal,
          body: JSON.stringify({
            message: promptText,
            channelId: ch,
            sessionId: targetHermesSessionId,
            persona: targetPersona,
            uiConfig: stateRef.current.uiConfig,
            history: stateRef.current.entries
              .filter(
                (e) =>
                  e.channelId === ch && (e.role === 'user' || e.role === 'hermes')
              )
              .slice(-6)
              .map((e) => ({ role: e.role, text: e.text })),
            pendingApprovals: appList
              .filter((a) => a.status === 'pending')
              .map((a) => ({ id: a.id, title: a.title, destination: a.destination, mcpTool: a.mcpTool })),
            tasks: taskList
              .filter((t) => t.channelId === ch)
              .map((t) => ({ id: t.id, title: t.title, status: t.status, progress: t.progress })),
          }),
        });

        window.clearTimeout(timeoutId);

        if (!res.ok) {
          throw new Error(`HTTP ${res.status}`);
        }

        const data = (await res.json()) as {
          reply?: string;
          mood?: ExpressionMood;
          suggestedCommand?: string;
          uiPatch?: UiCustomizationPatch | null;
        };

        if (data.uiPatch) {
          applyAgentUiPatch(data.uiPatch, 'hermes');
        }

        const reply = data.reply || 'Signal acknowledged.';
        const nextMood = data.mood || 'speaking';
        setChannelThinking(ch, false);
        updateSpokenLine(reply, ch);

        appendEntry({
          channelId: ch,
          role: 'hermes',
          source,
          mood: nextMood,
          text: data.suggestedCommand ? `${reply}\n[Suggested: ${data.suggestedCommand}]` : reply,
        });

        if (ch === stateRef.current.activeChannel) {
          setMood(nextMood);
          setSpeakingChannel(ch);
          await voiceRef.current?.speakText(reply, nextMood);
        }
      } catch (err) {
        window.clearTimeout(timeoutId);
        setChannelThinking(ch, false);
        if (err instanceof DOMException && err.name === 'AbortError' && chatAbortsRef.current.get(ch) !== controller) {
          return;
        }
        const localIntentPatch = parseNaturalUiIntent(promptText, stateRef.current.uiConfig);
        if (localIntentPatch) {
          const outcome = applyAgentUiPatch(localIntentPatch, 'hermes');
          const msg = `Updated UI directly: ${outcome.summary}.`;
          updateSpokenLine(msg, ch);
          appendEntry({
            channelId: ch,
            role: 'hermes',
            source,
            mood: 'happy',
            text: msg,
          });
          if (ch === stateRef.current.activeChannel) {
            setMood('happy');
            setSpeakingChannel(ch);
            await voiceRef.current?.speakText(msg, 'happy');
          }
          return;
        }
        const fallback = buildLocalFallbackReply(promptText, appList, taskList, briefList);
        updateSpokenLine(fallback.reply, ch);
        appendEntry({
          channelId: ch,
          role: 'hermes',
          source,
          mood: fallback.mood,
          text: fallback.reply,
        });
        if (ch === stateRef.current.activeChannel) {
          setMood(fallback.mood);
          setSpeakingChannel(ch);
          await voiceRef.current?.speakText(fallback.reply, fallback.mood);
        }
      } finally {
        if (chatAbortsRef.current.get(ch) === controller) {
          chatAbortsRef.current.delete(ch);
        }
      }
    },
    [
      appendEntry,
      applyAgentUiPatch,
      duetConnected,
      duetSendMessage,
      setChannelThinking,
      updateSpokenLine,
      upsertEntry,
    ]
  );

  const startHomeSessionAndSend = useCallback(
    async (promptText: string, source: 'voice' | 'cli') => {
      const currentSessions = stateRef.current.canvasSessions;
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
      };

      syncedSessionsRef.current.add(derived.slug);
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
                description: derived.topic,
                persona: derived.persona,
                unreadCount: 0,
              },
            ]
      );
      setActiveChannel(derived.slug);
      setMood('thinking');
      setChannelThinking(derived.slug, true);
      setLastSpokenLineState(`Starting ${derived.hermesSessionId}…`);

      let finalChannelId = derived.slug;
      let finalSessionId = derived.hermesSessionId;

      if (duetConnected) {
        try {
          const created = await createHermesSession({
            channelId: derived.slug,
            name: derived.slug,
            sessionId: derived.hermesSessionId,
            topic: derived.topic,
            persona: derived.persona,
            skipWelcome: true,
            requireFresh: true,
          });
          if (created.resolvedChannelId && created.resolvedChannelId !== derived.slug) {
            finalChannelId = created.resolvedChannelId;
            syncedSessionsRef.current.add(finalChannelId);
          }
          if (created.upstream_session_id || created.session?.sessionId) {
            finalSessionId =
              created.upstream_session_id || created.session?.sessionId || finalSessionId;
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
          // Proceed with local slug; duetSendMessage will retry or fall back cleanly
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
        },
        duetConnected
      );
    },
    [
      appendEntry,
      createHermesSession,
      duetConnected,
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

      if (!isCommandInput && duetConnected) {
        // No local echo on the Duet path: the real item (POST response or SSE)
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
        gatewayConnected: gatewayStatus?.connected,
        gatewayUrl: gatewayStatus?.gatewayUrl,
      });

      if (result.type === 'handled') {
        if (result.startNewSession) {
          if (result.newSessionPrompt) {
            setPanelView('home');
            setMobileSidebarOpen(false);
            await startHomeSessionAndSend(result.newSessionPrompt, source);
          } else {
            handleStartNewHomeSession();
          }
          return;
        }
        if (result.uiPatch) {
          applyAgentUiPatch(result.uiPatch, 'cli');
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
          handleApproveCard(result.approveCardId);
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
      appendEntry,
      applyAgentUiPatch,
      duetConnected,
      gatewayStatus,
      handleApproveCard,
      handleRejectCard,
      handleStartNewHomeSession,
      sendDirectChannelPrompt,
      startHomeSessionAndSend,
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
    document.title = 'Hermes Duet // Cyber Comms';
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
    setTasks((prev) =>
      prev.map((t) => {
        if (t.id !== id || t.status === 'completed') return t;
        const nextStatus = t.status === 'running' ? 'paused' : 'running';
        return {
          ...t,
          status: nextStatus,
          lastStep: nextStatus === 'running' ? 'Lane resumed on host worker' : 'Paused by operator',
          updatedAt: nowTime(),
        };
      })
    );
  }, []);

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
      if (duetConnected) {
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
    [appendEntry, duetConnected, loadChannelHistory]
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

      if (duetConnected) {
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
    [appendEntry, duetConnected, resetHermesSession, setChannelThinking, updateSpokenLine]
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
    (payload: {
      name: string;
      topic: string;
      persona: string;
      shape: BrailleShapeKind;
      tint: BrailleShapeTint;
      eyeStyle: 'open' | 'sleepy';
    }) => {
      const currentSessions = stateRef.current.canvasSessions;
      const slug = slugifySessionName(
        payload.name,
        currentSessions.map((s) => s.channelId)
      );
      const hermesSessionId = createHermesSessionId(slug);
      const spot = findOpenCanvasSpot(currentSessions);
      const nextSession: CanvasSession = {
        id: `session-${slug}-${Date.now().toString(36)}`,
        channelId: slug,
        hermesSessionId,
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
      };
      syncedSessionsRef.current.add(slug);
      setHomeSessionDraft(false);
      setCanvasSessions((prev) => [...prev, nextSession]);
      setActiveChannel(slug);
      setMood('happy');

      const msg = `Isolated session ${hermesSessionId} online (#${slug}). ${payload.topic}`;
      updateSpokenLine(msg, slug);

      if (duetConnected) {
        void createHermesSession({
          channelId: slug,
          name: slug,
          sessionId: hermesSessionId,
          topic: payload.topic,
          persona: payload.persona,
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
    [appendEntry, createHermesSession, duetConnected, updateSpokenLine]
  );

  const handleAutoArrangeCanvas = useCallback(() => {
    setCanvasSessions((prev) => autoArrangeSessions(prev));
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

  const handleRemoveCanvasSession = useCallback(
    (id: string) => {
      const target = stateRef.current.canvasSessions.find((s) => s.id === id);
      setCanvasSessions((prev) => {
        if (prev.length <= 1) return prev;
        const next = prev.filter((s) => s.id !== id);
        if (target && target.channelId === stateRef.current.activeChannel && next[0]) {
          setActiveChannel(next[0].channelId);
        }
        return next;
      });
      if (target) {
        setEntries((prev) => prev.filter((e) => e.channelId !== target.channelId));
        if (duetConnected) {
          void deleteHermesSession(target.channelId).catch(() => {});
        }
      }
    },
    [deleteHermesSession, duetConnected]
  );

  const handleOpenSessionThread = useCallback(
    (session: CanvasSession) => {
      setHomeSessionDraft(false);
      setActiveChannel(session.channelId);
      setMood(session.mood);
      setPanelView('voice');
      setThreadVisitNonce((n) => n + 1);
      setMobileSidebarOpen(false);
      if (duetConnected) {
        void loadChannelHistory(session.channelId);
      }
    },
    [duetConnected, loadChannelHistory]
  );

  const rootDensityPadding =
    uiConfig.density === 'compact'
      ? 'p-1 sm:p-2'
      : uiConfig.density === 'airy'
      ? 'p-2.5 sm:p-5 xl:p-6'
      : 'p-1.5 sm:p-3.5 xl:p-4';

  const mainGridClass = !uiConfig.sidebarVisible
    ? 'grid-cols-1'
    : uiConfig.sidebarPosition === 'right'
    ? 'grid-cols-1 lg:grid-cols-[1fr_var(--ui-sidebar-w,296px)]'
    : 'grid-cols-1 lg:grid-cols-[var(--ui-sidebar-w,296px)_1fr]';

  const gridGapClass =
    uiConfig.density === 'compact' ? 'gap-2' : uiConfig.density === 'airy' ? 'gap-4' : 'gap-3';

  return (
    <div
      className={`h-[100dvh] min-h-[500px] emerald-ambient-bg text-[#ECF3F0] ${rootDensityPadding} flex flex-col overflow-hidden`}
    >
      <main id="top" className={`flex-1 min-h-0 w-full mx-auto grid ${mainGridClass} ${gridGapClass}`}>
        {/* Sidebar Window: Tasks, Approvals, Halftone Thumb & Lanes */}
        {uiConfig.sidebarVisible && (
          <div
            className={`${mobileSidebarOpen ? 'flex' : 'hidden'} lg:flex ${
              uiConfig.sidebarPosition === 'right' ? 'lg:order-2' : 'lg:order-1'
            } flex-col min-h-0 h-full`}
          >
            <SidebarWorkspace
              channels={channels}
              activeChannel={activeChannel}
              onSelectChannel={(id) => {
                handleSelectChannel(id, { openThread: true });
                setMobileSidebarOpen(false);
              }}
              canvasSessions={canvasSessions}
              thinkingByChannel={thinkingByChannel}
              onSelectSession={(session) => {
                setSelectedItemId(session.id);
                handleOpenSessionThread(session);
              }}
              approvals={approvals}
              brief={brief}
              tasks={tasks}
              lastSpokenLine={lastSpokenLine}
              mood={mood}
              frequencyBands={voice.voiceState.frequencyBands}
              isSpeaking={voice.voiceState.isSpeaking}
              isListening={voice.voiceState.isListening}
              selectedItemId={selectedItemId}
              onSelectTask={(task) => {
                setSelectedItemId(task.id);
                setHomeSessionDraft(false);
                setActiveChannel(task.channelId);
                setPanelView('voice');
                setThreadVisitNonce((n) => n + 1);
                setMobileSidebarOpen(false);
                if (duetConnected) {
                  void loadChannelHistory(task.channelId);
                }
              }}
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
              onCloseMobile={() => setMobileSidebarOpen(false)}
              workspaceTitle={uiConfig.workspaceTitle}
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
          } flex-col min-h-0 h-full`}
        >
          <VoiceWorkspacePane
            channels={channels}
            activeChannel={activeChannel}
            onSelectChannel={handleSelectChannel}
            mood={mood}
            onCycleMood={handleCycleMood}
            panelView={panelView}
            onChangePanelView={(view) => {
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
            }}
            threadVisitNonce={threadVisitNonce}
            onOpenMobileSidebar={
              uiConfig.sidebarVisible ? () => setMobileSidebarOpen(true) : undefined
            }
            voiceState={voice.voiceState}
            lastSpokenLine={lastSpokenLine}
            animateSpeechLine={animateSpeechLine}
            entries={entries}
            approvals={approvals}
            brief={brief}
            tasks={tasks}
            gatewayStatus={gatewayStatus}
            onApproveCard={handleApproveCard}
            onRejectCard={handleRejectCard}
            onRunBriefCommand={handleRunBriefCommand}
            onToggleTaskStatus={handleToggleTaskStatus}
            onToggleListening={voice.toggleListening}
            onToggleMute={voice.toggleMute}
            onStopSpeaking={() => {
              const current = chatAbortsRef.current.get(activeChannel);
              chatAbortsRef.current.delete(activeChannel);
              current?.abort();
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
            hermesSessions={duet.sessions}
            thinkingByChannel={thinkingByChannel}
            speakingChannel={voice.voiceState.isSpeaking ? speakingChannel : null}
            onSelectCanvasSession={handleSelectCanvasSession}
            onMoveCanvasSession={handleMoveCanvasSession}
            onAddCanvasSession={handleAddCanvasSession}
            onAutoArrangeCanvas={handleAutoArrangeCanvas}
            onCycleSessionMood={handleCycleSessionMood}
            onResetSession={handleResetCanvasSession}
            onRemoveCanvasSession={handleRemoveCanvasSession}
            onOpenSessionThread={handleOpenSessionThread}
            homeSessionDraft={homeSessionDraft}
            onStartNewSession={handleStartNewHomeSession}
            uiConfig={uiConfig}
            onOpenStudio={() => setCustomizerOpen(true)}
          />
        </div>
      </main>

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
      />
    </div>
  );
}
