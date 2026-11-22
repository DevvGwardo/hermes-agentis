import {
  PanelLeft,
  Search,
  Settings,
  Volume2,
  VolumeX,
} from 'lucide-react';
import type {
  BackgroundTask,
  ChannelId,
  ExpressionMood,
  HermesAgentSession,
  MouthViseme,
} from '../types/hermes';
import { formatCanvasDotLabel, type CanvasSession } from '../utils/canvasSessions';
import { formatDisplaySessionTitle } from '../utils/sidebarSessions';
import { AsciiPortraitCanvas } from './AsciiPortraitCanvas';
import type { WorkspacePanelView } from './VoiceWorkspacePane';

interface MobileWorkspaceBarProps {
  activeChannel: ChannelId;
  activeSessionId: string;
  turnCount: number;
  mood: ExpressionMood;
  viseme: MouthViseme;
  isSpeaking: boolean;
  isListening: boolean;
  isThinking?: boolean;
  isMuted: boolean;
  panelView: WorkspacePanelView;
  pendingCount: number;
  showThinking: boolean;
  canvasSessions: CanvasSession[];
  hermesSessions?: HermesAgentSession[];
  thinkingByChannel: Record<string, boolean>;
  tasks?: BackgroundTask[];
  homeSessionDraft?: boolean;
  onChangePanelView: (view: WorkspacePanelView) => void;
  onSelectChannel: (channelId: ChannelId) => void;
  onOpenMobileSidebar?: () => void;
  onOpenQuickJump?: () => void;
  onResetSession: (channelId: ChannelId) => void;
  onCycleMood: () => void;
  onToggleMute: () => void;
  onToggleThinking: () => void;
  onOpenStudio?: () => void;
  updateAvailable?: boolean;
}

export function MobileWorkspaceBar({
  activeChannel,
  activeSessionId,
  turnCount,
  mood,
  viseme,
  isSpeaking,
  isListening,
  isMuted,
  panelView,
  pendingCount,
  canvasSessions,
  hermesSessions = [],
  homeSessionDraft = false,
  onChangePanelView,
  onOpenMobileSidebar,
  onOpenQuickJump,
  onCycleMood,
  onToggleMute,
  onOpenStudio,
  updateAvailable = false,
}: MobileWorkspaceBarProps) {
  const hasChannel = Boolean(activeChannel);
  const isHomeNewDraft = (panelView === 'home' && homeSessionDraft) || !hasChannel;
  const activeCanvasSession = hasChannel
    ? canvasSessions.find((s) => s.channelId === activeChannel)
    : undefined;
  const activeHermesSession = hasChannel
    ? hermesSessions.find((s) => s.channelId === activeChannel)
    : undefined;
  const dotLabel = hasChannel
    ? formatCanvasDotLabel(
        activeCanvasSession ?? {
          channelId: activeChannel,
          hermesSessionId: activeSessionId,
          name: activeChannel,
          topic: activeHermesSession?.topic ?? '',
        },
        activeHermesSession,
        turnCount
      )
    : null;
  const activeTitle = !hasChannel
    ? 'New session'
    : dotLabel?.primaryLabel ??
      formatDisplaySessionTitle({
        channelId: activeChannel,
        sessionId: activeSessionId,
      }).title;
  const activeMetaLine = dotLabel?.secondaryLabel ?? `${turnCount}t`;

  const views: { id: WorkspacePanelView; label: string; count?: number }[] = [
    { id: 'home', label: 'Home' },
    { id: 'canvas', label: 'Canvas' },
    { id: 'voice', label: 'Thread' },
    { id: 'queue', label: 'Queue', count: pendingCount > 0 ? pendingCount : undefined },
    { id: 'cli', label: 'CLI' },
  ];

  return (
    <div className="lg:hidden relative z-30 border-b border-white/[0.08] bg-[#11141B]/90 shrink-0">
      {/* Row 1: Compact Session Identity + Direct Utility Actions */}
      <div className="flex items-center justify-between gap-2 px-2.5 pt-2 pb-1.5">
        <div className="flex items-center gap-2 min-w-0 flex-1">
          {onOpenMobileSidebar && (
            <button
              type="button"
              onClick={onOpenMobileSidebar}
              aria-label="Open sessions and status"
              title="Sessions"
              className="ui-tactile w-9 h-9 rounded-xl border border-white/[0.09] bg-white/[0.04] text-zinc-200 hover:text-white flex items-center justify-center shrink-0 cursor-pointer"
            >
              <PanelLeft className="w-4 h-4" />
            </button>
          )}

          <button
            type="button"
            onClick={onCycleMood}
            aria-label={`Cycle Hermes mood (current: ${mood})`}
            className="w-8 h-8 rounded-full overflow-hidden border border-white/15 bg-black/50 flex items-center justify-center shrink-0 cursor-pointer"
          >
            <span className="w-8 h-8 overflow-hidden flex items-center justify-center pointer-events-none">
              <span className="scale-[0.76] origin-center shrink-0">
                <AsciiPortraitCanvas
                  variant="thumb"
                  mood={mood}
                  viseme={viseme}
                  eyeState="open"
                  energy={isSpeaking ? 0.65 : isListening ? 0.35 : 0.1}
                  isSpeaking={isSpeaking}
                  isListening={isListening}
                />
              </span>
            </span>
          </button>

          <button
            type="button"
            onClick={() => onChangePanelView(panelView === 'home' ? 'voice' : 'home')}
            className="flex-1 min-w-0 overflow-hidden text-left cursor-pointer"
          >
            <div className="text-[12.5px] font-semibold text-white leading-tight truncate">
              {isHomeNewDraft ? 'New session' : activeTitle}
            </div>
            <div className="font-mono text-[10px] tracking-tight text-zinc-400 tabular-nums leading-tight mt-0.5 truncate">
              {isHomeNewDraft
                ? canvasSessions.length > 0
                  ? `${canvasSessions.length} ${canvasSessions.length === 1 ? 'session' : 'sessions'}`
                  : 'Ready for prompt'
                : activeMetaLine}
            </div>
          </button>
        </div>

        <div className="flex items-center gap-1 shrink-0">
          {onOpenQuickJump && (
            <button
              type="button"
              onClick={onOpenQuickJump}
              aria-label="Quick jump navigation"
              title="Quick jump"
              className="ui-tactile w-8 h-8 rounded-lg text-zinc-300 hover:text-white hover:bg-white/[0.06] flex items-center justify-center transition-colors cursor-pointer"
            >
              <Search className="w-4 h-4" />
            </button>
          )}

          <button
            type="button"
            onClick={onToggleMute}
            aria-label={isMuted ? 'Unmute Hermes audio' : 'Mute Hermes audio'}
            className={`ui-tactile w-8 h-8 rounded-lg flex items-center justify-center transition-colors cursor-pointer ${
              isMuted
                ? 'bg-rose-500/20 text-rose-200'
                : 'text-zinc-300 hover:text-white hover:bg-white/[0.06]'
            }`}
          >
            {isMuted ? (
              <VolumeX className="w-4 h-4" />
            ) : (
              <Volume2 className="w-4 h-4" />
            )}
          </button>

          {onOpenStudio && (
            <button
              type="button"
              onClick={onOpenStudio}
              aria-label="Open Settings"
              title="Settings"
              className="ui-tactile relative w-8 h-8 rounded-lg text-zinc-300 hover:text-white hover:bg-white/[0.06] flex items-center justify-center transition-colors cursor-pointer"
            >
              <Settings className="w-4 h-4" />
              {updateAvailable && (
                <span
                  aria-label="Update available"
                  className="absolute top-1.5 right-1.5 w-1.5 h-1.5 rounded-full bg-amber-400"
                />
              )}
            </button>
          )}
        </div>
      </div>

      {/* Row 2: Direct 1-Tap Labeled View Tabs (Replaces hidden dropdown & cryptic icon circles) */}
      <nav
        aria-label="Mobile workspace views"
        className="grid grid-cols-5 gap-1 px-2 pb-1.5"
      >
        {views.map((tab) => {
          const active = panelView === tab.id;
          return (
            <button
              key={tab.id}
              type="button"
              aria-current={active ? 'page' : undefined}
              data-testid={`mobile-nav-${tab.id}`}
              onClick={() => onChangePanelView(tab.id)}
              className={`ui-tactile min-h-[34px] px-1 rounded-lg text-[11.5px] font-medium flex items-center justify-center gap-1 transition-colors cursor-pointer whitespace-nowrap ${
                active
                  ? 'bg-[rgba(var(--ui-accent-rgb),0.18)] text-white border border-[rgba(var(--ui-accent-rgb),0.40)] font-semibold'
                  : 'text-zinc-400 hover:text-zinc-200 border border-transparent'
              }`}
            >
              <span className="truncate">{tab.label}</span>
              {tab.count !== undefined && (
                <span className="font-mono text-[9.5px] text-amber-300 tabular-nums shrink-0">
                  {tab.count}
                </span>
              )}
            </button>
          );
        })}
      </nav>
    </div>
  );
}
