import { useLayoutEffect, useRef, useState, type RefObject } from 'react';
import {
  ArrowUp,
  AudioLines,
  Clock,
  Cpu,
  FolderGit2,
  Maximize2,
  MessageSquare,
  Pause,
  Play,
  Plus,
  RotateCcw,
  SlidersHorizontal,
  Sparkles,
  Trash2,
  X,
} from 'lucide-react';
import type {
  ExpressionMood,
  HermesAgentSession,
  HermesProfileSummary,
  MouthViseme,
  TerminalEntry,
} from '../types/hermes';
import type { BrailleDotProfile } from '../utils/brailleCanvasRaster';
import { BRAILLE_TINT_PALETTE } from '../utils/brailleShapesEngine';
import {
  AVAILABLE_DOT_SKILLS,
  formatAutonomyLabel,
  formatCanvasDotLabel,
  formatSandboxLabel,
  getShapeMeta,
  type CanvasSession,
  type DotAutonomyMode,
} from '../utils/canvasSessions';
import { computeCanvasPopoverAlignment } from '../utils/responsiveLayout';
import { BrailleShapeCanvas } from './BrailleShapeCanvas';
import { DecodedText } from './DecodedText';
import { ScrollToBottomButton, StatusPulse } from './ui';

interface CanvasSessionNodeProps {
  session: CanvasSession;
  hermesSession?: HermesAgentSession;
  index: number;
  livePos: { xPct: number; yPct: number };
  isSelected: boolean;
  isChatOpen: boolean;
  isDragged: boolean;
  isThinking: boolean;
  isSpeaking: boolean;
  isListeningHere: boolean;
  pendingApprovalCount?: number;
  viseme: MouthViseme;
  energy: number;
  entries: TerminalEntry[];
  turnCount: number;
  canRemove: boolean;
  compact?: boolean;
  dotProfile?: BrailleDotProfile;
  bloom?: number;
  speedScale?: number;
  reducedMotionOverride?: boolean;
  profiles?: HermesProfileSummary[];
  defaultProfile?: string;
  gazeRef: RefObject<{ clientX: number; clientY: number } | null>;
  onPointerDown: (e: React.PointerEvent<HTMLButtonElement>) => void;
  onPointerUp: (e: React.PointerEvent<HTMLButtonElement>) => void;
  onKeyDown: (e: React.KeyboardEvent<HTMLButtonElement>) => void;
  onSelect: () => void;
  onToggleChat: () => void;
  onSendMessage: (text: string) => void;
  onToggleVoice: () => void;
  onCycleMood: () => void;
  onResetSession: () => void;
  onOpenThread: () => void;
  onRemoveSession: () => void;
  onUpdateDotConfig?: (
    id: string,
    patch: Partial<
      Pick<CanvasSession, 'autonomyMode' | 'scheduleLabel' | 'autonomyPaused' | 'skills' | 'profile'>
    >
  ) => void;
}

export function CanvasSessionNode({
  session,
  hermesSession,
  index,
  livePos,
  isSelected,
  isChatOpen,
  isDragged,
  isThinking,
  isSpeaking,
  isListeningHere,
  pendingApprovalCount = 0,
  viseme,
  energy,
  entries,
  turnCount,
  canRemove,
  compact = false,
  dotProfile = 'balanced',
  bloom = 0.75,
  speedScale = 1,
  reducedMotionOverride = false,
  profiles = [],
  defaultProfile = 'default',
  gazeRef,
  onPointerDown,
  onPointerUp,
  onKeyDown,
  onSelect,
  onToggleChat,
  onSendMessage,
  onToggleVoice,
  onCycleMood,
  onResetSession,
  onOpenThread,
  onRemoveSession,
  onUpdateDotConfig,
}: CanvasSessionNodeProps) {
  const [draft, setDraft] = useState('');
  const [isHovered, setIsHovered] = useState(false);
  const [isAwayFromBottom, setIsAwayFromBottom] = useState(false);
  const [popoverTab, setPopoverTab] = useState<'chat' | 'config'>('chat');
  const chatScrollRef = useRef<HTMLDivElement | null>(null);
  const inputRef = useRef<HTMLInputElement | null>(null);

  const palette = BRAILLE_TINT_PALETTE[session.tint];
  const meta = getShapeMeta(session.shape);
  const dotLabel = formatCanvasDotLabel(session, hermesSession, turnCount);

  const effectiveMood: ExpressionMood = isThinking
    ? 'thinking'
    : isSpeaking
    ? 'speaking'
    : session.mood;

  const isAutonomousActive = session.autonomyMode !== 'manual' && !session.autonomyPaused;

  const visibleMessages = entries.filter(
    (e) =>
      e.role === 'user' ||
      e.role === 'hermes' ||
      e.role === 'command' ||
      e.role === 'activity'
  );

  const lastMsgLen = visibleMessages[visibleMessages.length - 1]?.text.length ?? 0;

  useLayoutEffect(() => {
    if (!isChatOpen || popoverTab !== 'chat' || !chatScrollRef.current) {
      setIsAwayFromBottom(false);
      return;
    }
    const snap = () => {
      if (chatScrollRef.current) {
        chatScrollRef.current.scrollTop = chatScrollRef.current.scrollHeight;
        setIsAwayFromBottom(false);
      }
    };
    snap();
    const rafId = requestAnimationFrame(snap);
    const timerId = window.setTimeout(snap, 60);
    return () => {
      cancelAnimationFrame(rafId);
      window.clearTimeout(timerId);
    };
  }, [isChatOpen, popoverTab, visibleMessages.length, lastMsgLen, isThinking]);

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    const trimmed = draft.trim();
    if (!trimmed) return;
    setDraft('');
    setPopoverTab('chat');
    onSelect();
    onSendMessage(trimmed);
  };

  const handleSetAutonomyMode = (mode: DotAutonomyMode) => {
    if (!onUpdateDotConfig) return;
    const scheduleLabel =
      mode === 'manual'
        ? 'On-demand'
        : mode === 'watch'
        ? 'Continuous watch'
        : session.autonomyMode === 'scheduled' && session.scheduleLabel !== 'On-demand'
        ? session.scheduleLabel
        : 'Every 15m';
    onUpdateDotConfig(session.id, {
      autonomyMode: mode,
      scheduleLabel,
      autonomyPaused: false,
    });
  };

  const handleToggleSkill = (skillId: string) => {
    if (!onUpdateDotConfig) return;
    const has = session.skills.includes(skillId);
    const nextSkills = has
      ? session.skills.filter((s) => s !== skillId)
      : [...session.skills, skillId];
    onUpdateDotConfig(session.id, { skills: nextSkills });
  };

  const statusTag = isSpeaking
    ? 'speaking'
    : isThinking
    ? 'thinking'
    : isAutonomousActive
    ? session.autonomyMode === 'watch'
      ? 'watching'
      : 'scheduled'
    : null;

  const popoverAlign = computeCanvasPopoverAlignment(livePos.xPct);
  const popoverHorizontalClass =
    popoverAlign === 'left'
      ? 'sm:left-0 sm:translate-x-0'
      : popoverAlign === 'right'
      ? 'sm:right-0 sm:left-auto sm:translate-x-0'
      : 'sm:left-1/2 sm:-translate-x-1/2';

  return (
    <div
      data-testid={`canvas-session-${session.channelId}`}
      data-shape={session.shape}
      data-hermes-session={session.hermesSessionId}
      data-chat-open={String(isChatOpen)}
      data-autonomy={session.autonomyMode}
      data-x={livePos.xPct}
      data-y={livePos.yPct}
      style={
        {
          '--dot-x': `${livePos.xPct}%`,
          '--dot-y': `${livePos.yPct}%`,
          zIndex: isDragged ? 50 : isChatOpen ? 45 : isSelected ? 22 : 12,
        } as React.CSSProperties
      }
      className={`ui-node-enter ${
        !isDragged ? 'ui-canvas-node-glide' : ''
      } relative sm:absolute sm:left-[var(--dot-x)] sm:top-[var(--dot-y)] sm:-translate-x-1/2 sm:-translate-y-1/2 flex flex-col items-center sm:touch-none py-1 sm:py-0 w-full sm:w-auto ${
        isChatOpen ? 'col-span-2 sm:col-span-1' : ''
      }`}
    >
      {/* Draggable / Clickable Anime Braille Character Sprite */}
      <button
        type="button"
        data-testid={`canvas-session-trigger-${session.channelId}`}
        onPointerEnter={() => setIsHovered(true)}
        onPointerLeave={() => setIsHovered(false)}
        onPointerDown={onPointerDown}
        onPointerUp={onPointerUp}
        onClick={(e) => {
          if (e.detail === 0) {
            onToggleChat();
          }
        }}
        onKeyDown={onKeyDown}
        aria-label={`Dot ${dotLabel.primaryLabel} (${session.hermesSessionId}), ${meta.label} anime character, ${formatAutonomyLabel(session.autonomyMode, session.scheduleLabel, session.autonomyPaused)}, ${turnCount} turns. Click to open workspace or drag to move.`}
        aria-expanded={isChatOpen}
        className={`group relative flex flex-col items-center focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-[var(--ui-accent)] rounded-2xl transition-transform duration-200 ease-out ${
          isDragged
            ? 'scale-[1.06] -translate-y-1 cursor-grabbing'
            : isSelected || isChatOpen
            ? 'scale-[1.02] cursor-grab hover:scale-[1.035]'
            : 'cursor-grab hover:scale-[1.02]'
        }`}
      >
        {/* Optical Ambient Corona + Floor Selection Reticle */}
        <div
          aria-hidden="true"
          className={`pointer-events-none absolute -inset-3 rounded-full transition-opacity duration-200 ${
            isSelected || isDragged || isThinking || isChatOpen
              ? 'opacity-100'
              : isAutonomousActive
              ? 'opacity-45 group-hover:opacity-85'
              : 'opacity-0 group-hover:opacity-70'
          }`}
          style={{
            background: `radial-gradient(circle, ${palette.glow} 0%, transparent 72%)`,
          }}
        />

        {(isSelected || isThinking || isSpeaking) && (
          <svg
            aria-hidden="true"
            viewBox="0 0 120 36"
            className="pointer-events-none absolute bottom-[34px] w-28 h-8 opacity-85"
          >
            <ellipse
              cx="60"
              cy="18"
              rx="52"
              ry="12"
              fill="none"
              stroke={palette.mid}
              strokeOpacity={isThinking || isSpeaking ? '0.68' : '0.38'}
              strokeWidth="1.25"
              strokeDasharray="6 5"
              className="ui-constellation-line"
            />
            <ellipse
              cx="60"
              cy="18"
              rx="38"
              ry="8"
              fill="none"
              stroke={palette.high}
              strokeOpacity="0.24"
              strokeWidth="1"
            />
          </svg>
        )}

        <div
          className={`relative z-10 ${
            compact ? 'h-[112px] flex items-end justify-center' : ''
          }`}
        >
          <div
            className={
              compact
                ? 'scale-[0.70] origin-bottom'
                : 'scale-[0.76] sm:scale-[0.86] origin-bottom'
            }
          >
            <BrailleShapeCanvas
              shape={session.shape}
              tint={session.tint}
              mood={effectiveMood}
              eyeStyle={session.eyeStyle}
              viseme={isSpeaking ? viseme : 'closed'}
              energy={isSpeaking ? energy : isThinking ? 0.28 : 0.08}
              isSpeaking={isSpeaking}
              isThinking={isThinking}
              isDragging={isDragged}
              isHovered={isHovered}
              dotProfile={dotProfile}
              bloom={bloom}
              speedScale={speedScale}
              reducedMotionOverride={reducedMotionOverride}
              blinkPhaseOffset={index * 4.35}
              gazeRef={gazeRef}
            />
          </div>
        </div>

        {/* Ground Contact Shadow */}
        <div
          aria-hidden="true"
          className={`pointer-events-none w-18 h-2.5 rounded-full transition-[transform,opacity] duration-150 ease-out -mt-1.5 ${
            isDragged ? 'scale-x-[0.68] scale-y-[0.82] opacity-35' : 'scale-100 opacity-60'
          }`}
          style={{
            background:
              'radial-gradient(ellipse at center, rgba(0, 0, 0, 0.72) 0%, transparent 75%)',
          }}
        />

        {/* Crisp Unboxed Dot Identity & Bounded Metadata Line */}
        <div className="relative z-10 mt-0.5 flex flex-col items-center leading-tight whitespace-nowrap max-w-[184px]">
          <div className="flex items-center gap-1.5 text-xs max-w-full">
            <span
              title={session.name}
              className={`font-mono font-semibold truncate max-w-[148px] ${
                isSelected || isChatOpen
                  ? 'text-white'
                  : 'text-zinc-200 group-hover:text-white'
              }`}
            >
              {dotLabel.primaryLabel}
            </span>
            {statusTag && (
              <span className="font-mono text-[10px] shrink-0" style={{ color: palette.mid }}>
                · {statusTag}
              </span>
            )}
          </div>
          {(dotLabel.secondaryLabel !== 'Web' || pendingApprovalCount > 0) && (
            <span
              title={session.hermesSessionId}
              className="font-mono text-[10px] text-zinc-400 tabular-nums truncate max-w-[172px]"
            >
              {dotLabel.secondaryLabel !== 'Web' ? dotLabel.secondaryLabel : ''}
              {pendingApprovalCount > 0
                ? `${dotLabel.secondaryLabel !== 'Web' ? ' · ' : ''}${pendingApprovalCount} ${
                    pendingApprovalCount === 1 ? 'gate' : 'gates'
                  }`
                : ''}
            </span>
          )}
        </div>
      </button>

      {/* Modular Per-Dot Perspective Workspace Window (Chat | Sandbox & Triggers) */}
      {isChatOpen && (
        <div
          data-testid={`session-chat-${session.channelId}`}
          onPointerDown={(e) => e.stopPropagation()}
          onWheelCapture={(e) => e.stopPropagation()}
          className={`ui-chat-pop relative mt-2 w-full sm:mt-0 sm:w-[min(308px,calc(100cqw-2rem))] sm:absolute ${popoverHorizontalClass} ${
            livePos.yPct > 56 ? 'sm:bottom-[calc(100%+8px)]' : 'sm:top-[calc(100%+8px)]'
          } rounded-xl bg-[#11141B]/96 border border-[rgba(var(--ui-accent-rgb),0.36)] shadow-[0_14px_34px_rgba(0,0,0,0.62)] overflow-hidden flex flex-col z-40`}
        >
          {/* Dot Window Header */}
          <div className="flex items-center justify-between gap-1.5 px-2.5 py-1.5 border-b border-white/8 bg-white/[0.02]">
            <div className="flex items-center gap-1.5 min-w-0">
              <StatusPulse
                state={isThinking ? 'thinking' : isSpeaking ? 'speaking' : 'idle'}
              />
              <span
                title={session.hermesSessionId}
                className="font-mono text-[10.5px] font-medium text-zinc-100 truncate"
              >
                {dotLabel.primaryLabel}
              </span>
              <span className="text-[10px] text-zinc-400 truncate hidden sm:inline shrink-0">
                · {meta.label}
                {session.profile && session.profile !== 'default' ? ` · @${session.profile}` : ''}
              </span>
            </div>

            <div className="flex items-center gap-0.5 shrink-0">
              <button
                type="button"
                onClick={onCycleMood}
                aria-label={`Cycle mood for #${session.name}`}
                title="Cycle expression"
                className="p-1 rounded text-zinc-400 hover:text-white hover:bg-white/8 transition-colors cursor-pointer"
              >
                <Sparkles className="w-3 h-3" />
              </button>
              <button
                type="button"
                onClick={onResetSession}
                aria-label={`Reset session #${session.name}`}
                title="Reset session context"
                className="p-1 rounded text-zinc-400 hover:text-white hover:bg-white/8 transition-colors cursor-pointer"
              >
                <RotateCcw className="w-3 h-3" />
              </button>
              <button
                type="button"
                onClick={onOpenThread}
                aria-label={`Expand #${session.name} to full thread`}
                title="Expand full thread"
                className="p-1 rounded text-zinc-400 hover:text-[var(--ui-accent-soft)] hover:bg-white/8 transition-colors cursor-pointer"
              >
                <Maximize2 className="w-3 h-3" />
              </button>
              {canRemove && (
                <button
                  type="button"
                  onClick={onRemoveSession}
                  aria-label={`Remove session ${dotLabel.primaryLabel}`}
                  title={
                    session.origin === 'imported'
                      ? 'Unpin from Canvas'
                      : 'Remove session'
                  }
                  className="p-1 rounded text-zinc-400 hover:text-rose-300 hover:bg-rose-500/12 transition-colors cursor-pointer"
                >
                  <Trash2 className="w-3 h-3" />
                </button>
              )}
              <button
                type="button"
                onClick={onToggleChat}
                aria-label={`Minimize chat for #${session.name}`}
                title="Minimize workspace"
                className="p-1 rounded text-zinc-400 hover:text-white hover:bg-white/8 transition-colors cursor-pointer"
              >
                <X className="w-3 h-3" />
              </button>
            </div>
          </div>

          {/* Sub-View Switcher: Thread Stream vs OpenAI Dots Sandbox & Autonomy Config */}
          <div className="flex items-center justify-between gap-2 px-2.5 py-1 border-b border-white/[0.06] bg-black/25">
            <div className="flex items-center gap-1" role="tablist" aria-label={`#${session.name} workspace sections`}>
              <button
                type="button"
                role="tab"
                aria-selected={popoverTab === 'chat'}
                onClick={() => setPopoverTab('chat')}
                className={`px-2 py-0.5 rounded text-[10.5px] font-medium transition-colors cursor-pointer whitespace-nowrap ${
                  popoverTab === 'chat'
                    ? 'bg-[rgba(var(--ui-accent-rgb),0.16)] text-[var(--ui-accent-soft)]'
                    : 'text-zinc-400 hover:text-white'
                }`}
              >
                Thread
              </button>
              <button
                type="button"
                role="tab"
                aria-selected={popoverTab === 'config'}
                data-testid={`dot-config-tab-${session.channelId}`}
                onClick={() => setPopoverTab('config')}
                className={`px-2 py-0.5 rounded text-[10.5px] font-medium flex items-center gap-1 transition-colors cursor-pointer whitespace-nowrap ${
                  popoverTab === 'config'
                    ? 'bg-[rgba(var(--ui-accent-rgb),0.16)] text-[var(--ui-accent-soft)]'
                    : 'text-zinc-400 hover:text-white'
                }`}
              >
                <SlidersHorizontal className="w-2.5 h-2.5" />
                <span>Sandbox &amp; Trigger</span>
              </button>
            </div>
            <span className="font-mono text-[9.5px] text-zinc-500 truncate tabular-nums">
              {session.skills.length} skills
            </span>
          </div>

          {popoverTab === 'chat' ? (
            <div className="relative">
              <div
                ref={chatScrollRef}
                onScroll={() => {
                  const el = chatScrollRef.current;
                  if (!el) return;
                  setIsAwayFromBottom(el.scrollHeight - el.scrollTop - el.clientHeight > 22);
                }}
                data-testid={`session-chat-scroll-${session.channelId}`}
                className="max-h-[168px] min-h-[78px] overflow-y-auto cyber-scroll px-2.5 py-2 space-y-2 text-[11.5px] leading-relaxed select-text"
              >
                {visibleMessages.length === 0 ? (
                  <p className="text-zinc-400 text-[11px] leading-snug">
                    {session.topic}
                  </p>
                ) : (
                  visibleMessages.map((entry) => {
                    const isUser = entry.role === 'user' || entry.role === 'command';
                    if (entry.role === 'activity') {
                      return (
                        <div
                          key={entry.id}
                          className="font-mono text-[10px] text-zinc-500 truncate"
                        >
                          ▸ {entry.text}
                        </div>
                      );
                    }
                    if (isUser) {
                      return (
                        <div
                          key={entry.id}
                          className="ui-msg-enter ml-auto max-w-[88%] rounded-lg bg-white/[0.08] border border-white/10 px-2.5 py-1.5 text-white/95 whitespace-pre-wrap break-words"
                        >
                          {entry.text}
                        </div>
                      );
                    }
                    return (
                      <div
                        key={entry.id}
                        className="ui-msg-enter max-w-[94%] text-zinc-200 whitespace-pre-wrap break-words"
                      >
                        <DecodedText
                          text={entry.text}
                          animate={!entry.id.startsWith('init-')}
                          speedMultiplier={1.3}
                        />
                      </div>
                    );
                  })
                )}

                {isThinking && (
                  <div className="flex items-center gap-1.5 font-mono text-[10.5px] text-amber-300/90 truncate">
                    <span aria-hidden="true" className="shrink-0">❖</span>
                    <span className="truncate">Thinking in {dotLabel.promptTarget}…</span>
                  </div>
                )}
              </div>

              <ScrollToBottomButton
                compact
                label="Latest"
                visible={isAwayFromBottom}
                testId={`session-jump-${session.channelId}`}
                className="absolute bottom-1.5 right-2 z-10"
                onClick={() => {
                  const el = chatScrollRef.current;
                  if (el) {
                    el.scrollTop = el.scrollHeight;
                    setIsAwayFromBottom(false);
                  }
                }}
              />
            </div>
          ) : (
            <div
              data-testid={`dot-config-panel-${session.channelId}`}
              className="px-3 py-2.5 space-y-2.5 text-[11px] max-h-[210px] overflow-y-auto cyber-scroll select-text"
            >
              {/* Isolated Environment / Workspace Directory */}
              <div className="flex items-center justify-between gap-2 pb-2 border-b border-white/8">
                <div className="min-w-0">
                  <div className="flex items-center gap-1.5 text-white font-medium">
                    <Cpu className="w-3 h-3 text-[var(--ui-accent-soft)] shrink-0" />
                    <span className="truncate">{formatSandboxLabel(session.sandboxEnv)}</span>
                  </div>
                  <div className="flex items-center gap-1 font-mono text-[10px] text-zinc-400 truncate mt-0.5">
                    <FolderGit2 className="w-2.5 h-2.5 shrink-0" />
                    <span className="truncate">{session.workspaceDir}</span>
                  </div>
                </div>
                <button
                  type="button"
                  onClick={() => {
                    onSelect();
                    onSendMessage(`/status`);
                    setPopoverTab('chat');
                  }}
                  className="px-2 py-1 rounded-md border border-[rgba(var(--ui-accent-rgb),0.36)] bg-[rgba(var(--ui-accent-rgb),0.14)] hover:bg-[rgba(var(--ui-accent-rgb),0.22)] text-[10px] font-semibold text-[var(--ui-accent-soft)] whitespace-nowrap shrink-0 cursor-pointer"
                >
                  Run check
                </button>
              </div>

              {/* Isolated Profile Assignment */}
              {onUpdateDotConfig &&
                (profiles.length > 1 || (session.profile && session.profile !== 'default')) && (
                  <div className="flex items-center justify-between gap-2 pb-2 border-b border-white/8">
                    <label
                      htmlFor={`dot-profile-${session.channelId}`}
                      className="text-[10.5px] font-medium text-zinc-300 shrink-0"
                    >
                      Hermes Profile
                    </label>
                    <select
                      id={`dot-profile-${session.channelId}`}
                      data-testid={`dot-profile-select-${session.channelId}`}
                      value={session.profile || defaultProfile}
                      onChange={(e) =>
                        onUpdateDotConfig(session.id, { profile: e.target.value })
                      }
                      className="rounded-md bg-black/45 border border-white/15 focus:border-[var(--ui-accent)] px-2 py-0.5 font-mono text-[10.5px] text-[var(--ui-accent-soft)] focus:outline-none max-w-[140px]"
                    >
                      {(profiles.length > 0
                        ? profiles
                        : [{ id: defaultProfile || 'default' }]
                      ).map((p) => (
                        <option key={p.id} value={p.id}>
                          @{p.id}
                        </option>
                      ))}
                    </select>
                  </div>
                )}

              {/* Always-On Autonomy & Cadence Controls */}
              <div className="space-y-1.5">
                <div className="flex items-center justify-between gap-2">
                  <span className="flex items-center gap-1 text-[10.5px] font-medium text-zinc-300">
                    <Clock className="w-3 h-3 text-zinc-400" />
                    <span>Trigger Mode</span>
                  </span>
                  {session.autonomyMode !== 'manual' && onUpdateDotConfig && (
                    <button
                      type="button"
                      onClick={() =>
                        onUpdateDotConfig(session.id, {
                          autonomyPaused: !session.autonomyPaused,
                        })
                      }
                      className="inline-flex items-center gap-1 text-[10px] font-mono text-zinc-300 hover:text-white cursor-pointer whitespace-nowrap"
                    >
                      {session.autonomyPaused ? (
                        <>
                          <Play className="w-2.5 h-2.5 text-[var(--ui-accent)]" />
                          <span>Resume</span>
                        </>
                      ) : (
                        <>
                          <Pause className="w-2.5 h-2.5 text-amber-300" />
                          <span>Pause</span>
                        </>
                      )}
                    </button>
                  )}
                </div>

                <div className="grid grid-cols-3 gap-1 p-0.5 rounded-lg bg-white/[0.04] border border-white/10">
                  {(
                    [
                      { id: 'manual', label: 'On-demand' },
                      { id: 'scheduled', label: 'Scheduled' },
                      { id: 'watch', label: 'Watch' },
                    ] as const
                  ).map((opt) => {
                    const active = session.autonomyMode === opt.id;
                    return (
                      <button
                        key={opt.id}
                        type="button"
                        onClick={() => handleSetAutonomyMode(opt.id)}
                        className={`py-1 px-1.5 rounded-md text-[10px] font-medium transition-colors cursor-pointer whitespace-nowrap ${
                          active
                            ? 'ui-accent-btn font-semibold'
                            : 'text-zinc-400 hover:text-white'
                        }`}
                      >
                        {opt.label}
                      </button>
                    );
                  })}
                </div>
              </div>

              {/* Portable Berd-Style Skills */}
              <div className="space-y-1.5 pt-1 border-t border-white/8">
                <div className="text-[10.5px] font-medium text-zinc-300">
                  Attached Skills
                </div>
                <div className="flex flex-wrap gap-1">
                  {AVAILABLE_DOT_SKILLS.map((skill) => {
                    const active = session.skills.includes(skill.id);
                    return (
                      <button
                        key={skill.id}
                        type="button"
                        onClick={() => handleToggleSkill(skill.id)}
                        aria-pressed={active}
                        title={skill.summary}
                        className={`px-2 py-0.5 rounded-md border text-[10px] transition-colors cursor-pointer whitespace-nowrap ${
                          active
                            ? 'ui-active-tab'
                            : 'border-white/10 bg-white/[0.03] text-zinc-400 hover:text-white hover:border-white/20'
                        }`}
                      >
                        {skill.label}
                      </button>
                    );
                  })}
                </div>
              </div>
            </div>
          )}
        </div>
      )}

      {/* Per-Character Quick-Ask Input */}
      <form
        onSubmit={handleSubmit}
        onPointerDown={(e) => e.stopPropagation()}
        data-testid={`session-input-form-${session.channelId}`}
        className={`${compact ? 'mt-1' : 'mt-1.5'} ${
          isChatOpen
            ? 'w-full sm:w-[min(308px,calc(100cqw-2rem))]'
            : compact
            ? 'w-full max-w-[184px] sm:w-[190px] sm:max-w-none'
            : 'w-full max-w-[188px] sm:w-[200px] sm:max-w-none'
        } flex items-center gap-1 rounded-xl border px-1.5 py-1 transition-[border-color,background-color,opacity,width] duration-150 ${
          isChatOpen || isSelected
            ? 'bg-[#11141B]/95 border-[rgba(var(--ui-accent-rgb),0.50)] shadow-[0_6px_18px_rgba(0,0,0,0.36)] opacity-100'
            : 'bg-[#0F1218]/72 border-white/[0.10] opacity-75 hover:opacity-100 focus-within:opacity-100 hover:border-white/20 focus-within:border-[var(--ui-accent)]'
        }`}
      >
        <button
          type="button"
          onClick={() => {
            onToggleChat();
            if (!isChatOpen) {
              window.setTimeout(() => inputRef.current?.focus(), 0);
            }
          }}
          aria-label={
            isChatOpen
              ? `Hide #${session.name} chat window`
              : `Open #${session.name} chat window`
          }
          title={isChatOpen ? 'Hide workspace window' : 'Open workspace window'}
          className={`ui-tactile w-6 h-6 rounded-md flex items-center justify-center transition-colors shrink-0 cursor-pointer ${
            isChatOpen
              ? 'bg-white/10 text-[var(--ui-accent-soft)]'
              : 'text-zinc-400 hover:text-white hover:bg-white/10'
          }`}
        >
          {isChatOpen ? (
            <MessageSquare className="w-3.5 h-3.5" />
          ) : (
            <Plus className="w-3.5 h-3.5" />
          )}
        </button>

        <input
          ref={inputRef}
          type="text"
          value={draft}
          onFocus={onSelect}
          onChange={(e) => setDraft(e.target.value)}
          data-testid={`session-input-${session.channelId}`}
          aria-label={`Prompt for #${session.name} (${session.hermesSessionId})`}
          placeholder={isThinking ? 'Thinking…' : 'Ask…'}
          className="flex-1 min-w-0 bg-transparent text-xs text-white placeholder:text-zinc-500 focus:outline-none px-1 py-0.5"
        />

        {draft.trim() ? (
          <button
            type="submit"
            data-testid={`session-send-${session.channelId}`}
            aria-label={`Send message to #${session.name}`}
            className="ui-pop-in ui-tactile ui-accent-btn w-7 h-7 rounded-lg flex items-center justify-center hover:opacity-90 transition-opacity shrink-0 cursor-pointer"
          >
            <ArrowUp className="w-3.5 h-3.5" />
          </button>
        ) : (
          <button
            type="button"
            onClick={onToggleVoice}
            aria-pressed={isListeningHere}
            aria-label={
              isListeningHere
                ? `Stop voice input for #${session.name}`
                : `Voice input for #${session.name}`
            }
            className={`ui-tactile w-7 h-7 rounded-lg flex items-center justify-center transition-colors shrink-0 cursor-pointer ${
              isListeningHere || isSpeaking
                ? 'bg-[var(--ui-accent-bright)] text-[#0B0D12] shadow-[0_0_12px_rgba(var(--ui-accent-rgb),0.40)]'
                : 'bg-white/[0.08] hover:bg-[var(--ui-accent)] text-zinc-200 hover:text-[#0B0D12]'
            }`}
          >
            <AudioLines className="w-3.5 h-3.5" />
          </button>
        )}
      </form>
    </div>
  );
}
