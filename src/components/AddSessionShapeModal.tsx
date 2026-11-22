import { useEffect, useState } from 'react';
import { ChevronDown, Plus, SlidersHorizontal, X } from 'lucide-react';
import type { ExpressionMood, HermesProfileSummary, MouthViseme } from '../types/hermes';
import { amplitudeToViseme } from '../utils/asciiCharacter';
import type { BrailleDotProfile } from '../utils/brailleCanvasRaster';
import {
  BRAILLE_SHAPE_CATALOG,
  BRAILLE_TINT_PALETTE,
  type BrailleShapeKind,
  type BrailleShapeTint,
} from '../utils/brailleShapesEngine';
import {
  AVAILABLE_DOT_SKILLS,
  DOT_STARTER_TEMPLATES,
  formatAutonomyLabel,
  formatSandboxLabel,
  type DotAutonomyMode,
  type DotSandboxEnv,
} from '../utils/canvasSessions';
import { BrailleShapeCanvas } from './BrailleShapeCanvas';
import { ModalShell } from './ui';

export interface CreateDotSessionPayload {
  name: string;
  topic: string;
  persona: string;
  shape: BrailleShapeKind;
  tint: BrailleShapeTint;
  eyeStyle: 'open' | 'sleepy';
  autonomyMode?: DotAutonomyMode;
  scheduleLabel?: string;
  sandboxEnv?: DotSandboxEnv;
  skills?: string[];
  profile?: string;
}

interface AddSessionShapeModalProps {
  open: boolean;
  dotProfile?: BrailleDotProfile;
  bloom?: number;
  profiles?: HermesProfileSummary[];
  defaultProfile?: string;
  onClose: () => void;
  onCreate: (payload: CreateDotSessionPayload) => void;
}

const TINT_KEYS = Object.keys(BRAILLE_TINT_PALETTE) as BrailleShapeTint[];

const AUDITION_MOODS: ReadonlyArray<{ id: 'idle' | 'speak' | 'think' | 'happy'; label: string; mood: ExpressionMood }> = [
  { id: 'happy', label: 'Happy', mood: 'happy' },
  { id: 'speak', label: 'Speak', mood: 'speaking' },
  { id: 'think', label: 'Think', mood: 'thinking' },
  { id: 'idle', label: 'Calm', mood: 'neutral' },
];

export function AddSessionShapeModal({
  open,
  dotProfile = 'balanced',
  bloom = 0.75,
  profiles = [],
  defaultProfile = 'default',
  onClose,
  onCreate,
}: AddSessionShapeModalProps) {
  const defaultMeta = BRAILLE_SHAPE_CATALOG[2]; // Neko default
  const [selectedTemplateId, setSelectedTemplateId] = useState<string>('custom');
  const [selectedShape, setSelectedShape] = useState<BrailleShapeKind>(defaultMeta.kind);
  const [selectedTint, setSelectedTint] = useState<BrailleShapeTint>(defaultMeta.defaultTint);
  const [eyeStyle, setEyeStyle] = useState<'open' | 'sleepy'>(defaultMeta.defaultEyeStyle);
  const [auditionState, setAuditionState] = useState<'idle' | 'speak' | 'think' | 'happy'>('happy');
  const [autonomyMode, setAutonomyMode] = useState<DotAutonomyMode>('manual');
  const [scheduleLabel, setScheduleLabel] = useState('On-demand');
  const [sandboxEnv, setSandboxEnv] = useState<DotSandboxEnv>('cloud-sandbox');
  const [skills, setSkills] = useState<string[]>(['browser', 'files']);
  const [profile, setProfile] = useState<string>(defaultProfile);
  const [name, setName] = useState('');
  const [topic, setTopic] = useState('');
  const [persona, setPersona] = useState(defaultMeta.defaultAgentPersona);
  const [personaEdited, setPersonaEdited] = useState(false);
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [error, setError] = useState('');
  const [auditionTick, setAuditionTick] = useState(0);

  useEffect(() => {
    if (open) {
      setProfile(defaultProfile);
      setShowAdvanced(false);
    }
  }, [open, defaultProfile]);

  useEffect(() => {
    if (!open || auditionState !== 'speak') {
      setAuditionTick(0);
      return;
    }
    const id = window.setInterval(() => {
      setAuditionTick((t) => (t + 1) % 10000);
    }, 52);
    return () => window.clearInterval(id);
  }, [open, auditionState]);

  if (!open) return null;

  const activeMeta =
    BRAILLE_SHAPE_CATALOG.find((s) => s.kind === selectedShape) ?? BRAILLE_SHAPE_CATALOG[0];
  const activePalette = BRAILLE_TINT_PALETTE[selectedTint];

  const handleApplyTemplate = (templateId: string) => {
    const tpl = DOT_STARTER_TEMPLATES.find((t) => t.id === templateId);
    if (!tpl) return;
    setSelectedTemplateId(tpl.id);
    setSelectedShape(tpl.shape);
    setSelectedTint(tpl.tint);
    setAutonomyMode(tpl.autonomyMode);
    setScheduleLabel(tpl.scheduleLabel);
    setSandboxEnv(tpl.sandboxEnv);
    setSkills([...tpl.skills]);
    if (!personaEdited) {
      setPersona(tpl.persona);
    }
  };

  const handleSelectShape = (kind: BrailleShapeKind) => {
    setSelectedShape(kind);
    const meta = BRAILLE_SHAPE_CATALOG.find((s) => s.kind === kind);
    if (meta) {
      setSelectedTint(meta.defaultTint);
      setEyeStyle(meta.defaultEyeStyle);
      if (!personaEdited) {
        setPersona(meta.defaultAgentPersona);
      }
    }
  };

  const handleToggleSkill = (skillId: string) => {
    setSkills((prev) =>
      prev.includes(skillId) ? prev.filter((s) => s !== skillId) : [...prev, skillId]
    );
  };

  const handleSetAutonomy = (mode: DotAutonomyMode) => {
    setAutonomyMode(mode);
    setScheduleLabel(
      mode === 'manual' ? 'On-demand' : mode === 'watch' ? 'Continuous watch' : 'Every 15m'
    );
  };

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    const cleaned = name.trim();
    if (!cleaned) {
      setError('Enter a session name so hermes-agent can isolate its thread.');
      return;
    }
    onCreate({
      name: cleaned,
      topic: topic.trim() || activeMeta.tagline,
      persona: persona.trim() || activeMeta.defaultAgentPersona,
      shape: selectedShape,
      tint: selectedTint,
      eyeStyle,
      autonomyMode,
      scheduleLabel,
      sandboxEnv,
      skills,
      profile,
    });
    setName('');
    setTopic('');
    setPersona(activeMeta.defaultAgentPersona);
    setPersonaEdited(false);
    setError('');
    onClose();
  };

  const currentAudition = AUDITION_MOODS.find((a) => a.id === auditionState) ?? AUDITION_MOODS[0];
  const auditionEnergy =
    auditionState === 'speak'
      ? Math.max(
          0.18,
          Math.min(
            0.94,
            0.56 +
              Math.sin(auditionTick * 0.52) * 0.24 +
              Math.sin(auditionTick * 1.18 + 0.7) * 0.14
          )
        )
      : auditionState === 'think'
      ? 0.25
      : 0.08;
  const auditionViseme: MouthViseme =
    auditionState === 'speak'
      ? amplitudeToViseme(
          auditionEnergy,
          'speaking',
          auditionTick,
          true,
          undefined,
          `Hello! I am ${activeMeta.label}, synced and ready to help build, monitor, and ship your tasks.`
        )
      : auditionState === 'happy'
      ? 'smile'
      : 'closed';

  return (
    <ModalShell
      open={open}
      onClose={onClose}
      ariaLabelledBy="add-braille-session-title"
      maxWidthClass="w-[min(860px,96vw)]"
    >
      <div className="flex items-center justify-between px-5 py-3.5 border-b border-white/10 shrink-0">
        <div>
          <h2
            id="add-braille-session-title"
            className="text-base font-semibold text-white"
          >
            New Canvas Dot
          </h2>
          <p className="text-xs text-zinc-400 mt-0.5">
            Name your agent session and pick a live Braille companion silhouette
          </p>
        </div>

        <button
          type="button"
          onClick={onClose}
          aria-label="Close add session dialog"
          className="ui-tactile w-8 h-8 rounded-lg border border-white/12 bg-white/[0.04] text-zinc-300 hover:text-white hover:border-white/25 flex items-center justify-center cursor-pointer"
        >
          <X className="w-4 h-4" />
        </button>
      </div>

      <form onSubmit={handleSubmit} className="flex flex-col flex-1 min-h-0">
        <div className="flex-1 overflow-y-auto p-3.5 sm:p-5 cyber-scroll">
          <div className="grid grid-cols-1 md:grid-cols-[260px_minmax(0,1fr)] gap-4 sm:gap-5 items-start">
            {/* Left Column: Large Interactive Character Audition Stage + Tint & Eyes */}
            <div className="min-w-0 rounded-xl border border-white/10 bg-[#0B0D12]/85 p-3.5 sm:p-4 flex flex-col items-center space-y-3.5">
              <div className="w-full flex items-center justify-between text-[11px]">
                <span className="font-mono font-medium text-white">{activeMeta.label}</span>
                <span className="font-mono text-zinc-400">{activeMeta.animeTitle}</span>
              </div>

              <div className="relative py-2 flex items-center justify-center">
                <div
                  aria-hidden="true"
                  className="pointer-events-none absolute -inset-4 rounded-full opacity-75"
                  style={{
                    background: `radial-gradient(circle, ${activePalette.glow} 0%, transparent 70%)`,
                  }}
                />
                <BrailleShapeCanvas
                  variant="creature"
                  shape={selectedShape}
                  tint={selectedTint}
                  mood={currentAudition.mood}
                  eyeStyle={eyeStyle}
                  isSpeaking={auditionState === 'speak'}
                  isThinking={auditionState === 'think'}
                  viseme={auditionViseme}
                  energy={auditionEnergy}
                  dotProfile={dotProfile}
                  bloom={bloom}
                />
              </div>

              {/* Audition Live Motion States */}
              <div className="w-full space-y-1.5">
                <span className="block text-[11px] font-medium text-zinc-300">
                  Audition State
                </span>
                <div className="grid grid-cols-4 gap-1 p-1 rounded-lg bg-white/[0.04] border border-white/10">
                  {AUDITION_MOODS.map((opt) => {
                    const active = auditionState === opt.id;
                    return (
                      <button
                        key={opt.id}
                        type="button"
                        onClick={() => setAuditionState(opt.id)}
                        className={`py-1 rounded-md text-[11px] font-medium cursor-pointer whitespace-nowrap ${
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

              {/* Phosphor Tint */}
              <div className="w-full space-y-1.5">
                <span className="block text-[11px] font-medium text-zinc-300">
                  Phosphor Tint · {activePalette.label}
                </span>
                <div className="grid grid-cols-9 gap-1.5 py-0.5 w-full place-items-center">
                  {TINT_KEYS.map((tintKey) => {
                    const palette = BRAILLE_TINT_PALETTE[tintKey];
                    const active = tintKey === selectedTint;
                    return (
                      <button
                        key={tintKey}
                        type="button"
                        onClick={() => setSelectedTint(tintKey)}
                        aria-label={`Tint ${palette.label}`}
                        aria-pressed={active}
                        title={palette.label}
                        className={`w-5 h-5 rounded-full border-2 transition-transform cursor-pointer ${
                          active ? 'border-white scale-110' : 'border-white/15 hover:scale-105'
                        }`}
                        style={{ backgroundColor: palette.mid }}
                      />
                    );
                  })}
                </div>
              </div>

              {/* Resting Eyes */}
              <div className="w-full space-y-1.5">
                <span className="block text-[11px] font-medium text-zinc-300">
                  Resting Eyes
                </span>
                <div className="flex items-center gap-1 p-1 rounded-lg bg-white/[0.04] border border-white/10">
                  <button
                    type="button"
                    onClick={() => setEyeStyle('open')}
                    className={`flex-1 py-1 px-2 rounded-md text-[11px] font-medium transition-colors cursor-pointer whitespace-nowrap ${
                      eyeStyle === 'open'
                        ? 'ui-accent-btn font-semibold'
                        : 'text-zinc-400 hover:text-white'
                    }`}
                  >
                    Bright Eyes
                  </button>
                  <button
                    type="button"
                    onClick={() => setEyeStyle('sleepy')}
                    className={`flex-1 py-1 px-2 rounded-md text-[11px] font-medium transition-colors cursor-pointer whitespace-nowrap ${
                      eyeStyle === 'sleepy'
                        ? 'ui-accent-btn font-semibold'
                        : 'text-zinc-400 hover:text-white'
                    }`}
                  >
                    Calm Sleepy
                  </button>
                </div>
              </div>

              <div className="w-full pt-2 border-t border-white/8 text-[11px] font-mono text-zinc-400 space-y-0.5 tabular-nums">
                <div>{formatSandboxLabel(sandboxEnv)}</div>
                <div>
                  {formatAutonomyLabel(autonomyMode, scheduleLabel)} · {skills.length} skills
                </div>
              </div>
            </div>

            {/* Right Column: Primary Identity + Silhouette Picker + Progressive Advanced Options */}
            <div className="min-w-0 space-y-3.5">
              {/* Step 1: Session Identity (at top so autoFocus never scrolls preview off-screen) */}
              <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                <div className="space-y-1">
                  <label htmlFor="canvas-session-name" className="block text-xs font-medium text-zinc-200">
                    Dot Name
                  </label>
                  <input
                    id="canvas-session-name"
                    type="text"
                    value={name}
                    onChange={(e) => {
                      setName(e.target.value);
                      if (error) setError('');
                    }}
                    placeholder="e.g. kitsune-lab"
                    autoFocus
                    className="w-full rounded-lg bg-[#0D1016] border border-white/15 focus:border-[var(--ui-accent)] px-3 py-1.5 text-sm text-white placeholder:text-zinc-500 focus:outline-none"
                  />
                </div>

                <div className="space-y-1">
                  <label htmlFor="canvas-session-topic" className="block text-xs font-medium text-zinc-200">
                    Focus or Role (optional)
                  </label>
                  <input
                    id="canvas-session-topic"
                    type="text"
                    value={topic}
                    onChange={(e) => setTopic(e.target.value)}
                    placeholder={activeMeta.tagline}
                    className="w-full rounded-lg bg-[#0D1016] border border-white/15 focus:border-[var(--ui-accent)] px-3 py-1.5 text-sm text-white placeholder:text-zinc-500 focus:outline-none"
                  />
                </div>
              </div>

              {error && (
                <p role="alert" className="text-xs text-rose-300">
                  {error}
                </p>
              )}

              {/* Step 2: 9 Anime Character Braille Picker */}
              <div className="space-y-1.5">
                <span className="block text-xs font-medium text-zinc-200">
                  Braille Companion Silhouette
                </span>
                <div className="grid grid-cols-2 sm:grid-cols-3 gap-2">
                  {BRAILLE_SHAPE_CATALOG.map((meta) => {
                    const isSelected = meta.kind === selectedShape;
                    const displayTint = isSelected ? selectedTint : meta.defaultTint;
                    return (
                      <button
                        key={meta.kind}
                        type="button"
                        onClick={() => handleSelectShape(meta.kind)}
                        aria-pressed={isSelected}
                        aria-label={`Select ${meta.label} (${meta.animeTitle})`}
                        className={`min-w-0 flex flex-col items-center justify-center gap-0.5 rounded-xl border px-2 py-1.5 cursor-pointer ${
                          isSelected
                            ? 'ui-active-card'
                            : 'border-white/10 bg-white/[0.03] hover:border-white/25 hover:bg-white/[0.06]'
                        }`}
                      >
                        <BrailleShapeCanvas
                          variant="preview"
                          shape={meta.kind}
                          tint={displayTint}
                          mood="happy"
                          eyeStyle={isSelected ? eyeStyle : meta.defaultEyeStyle}
                          dotProfile={dotProfile}
                          bloom={bloom}
                          reducedMotionOverride={!isSelected}
                        />
                        <div className="w-full text-center leading-tight">
                          <div className="text-xs font-semibold text-white truncate">{meta.label}</div>
                          <div className="text-[10.5px] text-zinc-400 leading-snug line-clamp-1">
                            {meta.tagline}
                          </div>
                        </div>
                      </button>
                    );
                  })}
                </div>
              </div>

              {/* Step 3: Progressive Disclosure for Templates, Autonomy, Skills & Profile */}
              <div className="rounded-xl border border-white/10 bg-white/[0.02] overflow-hidden">
                <button
                  type="button"
                  data-testid="add-dot-advanced-toggle"
                  aria-expanded={showAdvanced}
                  onClick={() => setShowAdvanced((v) => !v)}
                  className="w-full flex items-center justify-between gap-2 px-3 py-2 text-left hover:bg-white/[0.03] transition-colors cursor-pointer"
                >
                  <span className="inline-flex items-center gap-2 text-xs font-medium text-zinc-200 min-w-0">
                    <SlidersHorizontal className="w-3.5 h-3.5 text-zinc-400 shrink-0" />
                    <span>Templates &amp; advanced options</span>
                  </span>
                  <span className="inline-flex items-center gap-2 shrink-0">
                    <span className="font-mono text-[10.5px] text-zinc-400 hidden sm:inline">
                      {formatAutonomyLabel(autonomyMode, scheduleLabel)} · {skills.length} skills · @{profile}
                    </span>
                    <ChevronDown
                      className={`w-3.5 h-3.5 text-zinc-400 transition-transform duration-150 ${
                        showAdvanced ? 'rotate-180' : ''
                      }`}
                    />
                  </span>
                </button>

                {showAdvanced && (
                  <div className="px-3 pb-3 pt-2 border-t border-white/8 space-y-3">
                    {/* Starter Templates */}
                    <div className="space-y-1.5">
                      <span className="block text-[11px] font-medium text-zinc-300">
                        Starter Templates
                      </span>
                      <div className="grid grid-cols-1 sm:grid-cols-2 gap-1.5">
                        {DOT_STARTER_TEMPLATES.map((tpl) => {
                          const active = selectedTemplateId === tpl.id;
                          return (
                            <button
                              key={tpl.id}
                              type="button"
                              onClick={() => handleApplyTemplate(tpl.id)}
                              className={`min-w-0 px-2.5 py-1.5 rounded-lg border text-left cursor-pointer ${
                                active
                                  ? 'ui-active-card'
                                  : 'border-white/10 bg-white/[0.03] hover:border-white/25'
                              }`}
                            >
                              <div className="text-[11px] font-semibold text-white truncate">
                                {tpl.label}
                              </div>
                              <div className="text-[10px] text-zinc-400 truncate">
                                {tpl.scheduleLabel} · {tpl.subtitle}
                              </div>
                            </button>
                          );
                        })}
                      </div>
                    </div>

                    {/* Autonomy Cadence & Portable Skills */}
                    <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                      <div className="space-y-1">
                        <span className="block text-[11px] font-medium text-zinc-300">
                          Autonomy &amp; Sandbox
                        </span>
                        <div className="flex items-center gap-1 p-1 rounded-lg bg-white/[0.04] border border-white/10">
                          {(
                            [
                              { id: 'manual', label: 'Manual' },
                              { id: 'scheduled', label: 'Scheduled' },
                              { id: 'watch', label: 'Watch' },
                            ] as const
                          ).map((opt) => (
                            <button
                              key={opt.id}
                              type="button"
                              onClick={() => handleSetAutonomy(opt.id)}
                              className={`flex-1 py-1 px-2 rounded-md text-[11px] font-medium transition-colors cursor-pointer whitespace-nowrap ${
                                autonomyMode === opt.id
                                  ? 'ui-accent-btn font-semibold'
                                  : 'text-zinc-400 hover:text-white'
                              }`}
                            >
                              {opt.label}
                            </button>
                          ))}
                        </div>
                      </div>

                      <div className="space-y-1">
                        <span className="block text-[11px] font-medium text-zinc-300">
                          Portable Skills
                        </span>
                        <div className="flex flex-wrap gap-1">
                          {AVAILABLE_DOT_SKILLS.map((sk) => {
                            const active = skills.includes(sk.id);
                            return (
                              <button
                                key={sk.id}
                                type="button"
                                onClick={() => handleToggleSkill(sk.id)}
                                aria-pressed={active}
                                className={`px-2 py-1 rounded-md border text-[10.5px] transition-colors cursor-pointer whitespace-nowrap ${
                                  active
                                    ? 'ui-active-tab'
                                    : 'border-white/10 bg-white/[0.03] text-zinc-400 hover:text-white'
                                }`}
                              >
                                {sk.label}
                              </button>
                            );
                          })}
                        </div>
                      </div>
                    </div>

                    <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                      <div className="space-y-1">
                        <label htmlFor="canvas-session-profile" className="block text-[11px] font-medium text-zinc-300">
                          Hermes Profile
                        </label>
                        <select
                          id="canvas-session-profile"
                          data-testid="add-dot-profile-select"
                          value={profile}
                          onChange={(e) => setProfile(e.target.value)}
                          className="w-full rounded-lg bg-[#0D1016] border border-white/15 focus:border-[var(--ui-accent)] px-3 py-1.5 text-xs font-mono text-white focus:outline-none"
                        >
                          {(profiles.length > 0
                            ? profiles
                            : [{ id: defaultProfile || 'default', label: defaultProfile || 'default' }]
                          ).map((p) => (
                            <option key={p.id} value={p.id}>
                              @{p.id}
                            </option>
                          ))}
                        </select>
                      </div>

                      <div className="space-y-1">
                        <label htmlFor="canvas-session-persona" className="block text-[11px] font-medium text-zinc-300">
                          System Persona
                        </label>
                        <input
                          id="canvas-session-persona"
                          type="text"
                          value={persona}
                          onChange={(e) => {
                            setPersona(e.target.value);
                            setPersonaEdited(true);
                          }}
                          placeholder={activeMeta.defaultAgentPersona}
                          className="w-full rounded-lg bg-[#0D1016] border border-white/15 focus:border-[var(--ui-accent)] px-3 py-1.5 text-xs font-mono text-zinc-100 placeholder:text-zinc-500 focus:outline-none"
                        />
                      </div>
                    </div>
                  </div>
                )}
              </div>
            </div>
          </div>
        </div>

        <div className="flex flex-wrap items-center justify-end gap-2.5 px-4 sm:px-5 py-3.5 border-t border-white/10 bg-[#11141B] shrink-0">
          <button
            type="button"
            onClick={onClose}
            className="px-4 py-2 rounded-lg border border-white/12 bg-white/[0.04] text-xs font-medium text-zinc-300 hover:text-white transition-colors cursor-pointer whitespace-nowrap"
          >
            Cancel
          </button>
          <button
            type="submit"
            className="ui-accent-btn ui-tactile px-4 py-2 rounded-lg text-xs font-semibold flex items-center gap-1.5 hover:opacity-90 transition-opacity cursor-pointer whitespace-nowrap"
          >
            <Plus className="w-3.5 h-3.5" />
            <span>Spawn Dot</span>
          </button>
        </div>
      </form>
    </ModalShell>
  );
}
