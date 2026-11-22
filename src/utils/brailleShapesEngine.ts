import type { ExpressionMood, EyeState, MouthViseme } from '../types/hermes';
import { evaluateNaturalBlinkCurve } from './asciiPortraitEngine';
import { BLUE_NOISE_8X8_RANKS } from './brailleCanvasRaster';
import {
  NEUTRAL_IDLE_POSE,
  evaluateIdleActionStep,
  resolveIdleSetForShape,
  type IdleActionFrame,
  type IdleAnimationSetId,
  type IdleHandPose,
  type IdlePoseDeltas,
  type IdlePropKind,
} from './brailleIdleLoops';

export type BrailleShapeKind =
  | 'clover'
  | 'star'
  | 'ghost'
  | 'heart'
  | 'cloud'
  | 'triangle'
  | 'pebble'
  | 'trapezoid'
  | 'blob';

export type BrailleShapeTint =
  | 'lime'
  | 'pink'
  | 'lavender'
  | 'coral'
  | 'orange'
  | 'cyan'
  | 'periwinkle'
  | 'amber'
  | 'magenta';

export interface BrailleShapeMeta {
  kind: BrailleShapeKind;
  label: string;
  animeTitle: string;
  defaultTint: BrailleShapeTint;
  defaultEyeStyle: 'open' | 'sleepy';
  tagline: string;
  defaultAgentPersona: string;
}

export const BRAILLE_SHAPE_CATALOG: BrailleShapeMeta[] = [
  {
    kind: 'clover',
    label: 'Cyber Bob',
    animeTitle: 'Hime Headband',
    defaultTint: 'lime',
    defaultEyeStyle: 'open',
    tagline: 'Core strategist',
    defaultAgentPersona: 'Calm primary Hermes coordinator styled after the main cyber-bob operator.',
  },
  {
    kind: 'star',
    label: 'Twintails',
    animeTitle: 'Starlight Idol',
    defaultTint: 'pink',
    defaultEyeStyle: 'open',
    tagline: 'Launch runner',
    defaultAgentPersona: 'High-energy release and save_page builder running fast launch turns.',
  },
  {
    kind: 'ghost',
    label: 'Neko',
    animeTitle: 'Cyber Catgirl',
    defaultTint: 'lavender',
    defaultEyeStyle: 'open',
    tagline: 'Silent daemon',
    defaultAgentPersona: 'Stealthy background daemon monitoring cron watchers and ambient events.',
  },
  {
    kind: 'heart',
    label: 'Kitsune',
    animeTitle: 'Shrine Fox',
    defaultTint: 'coral',
    defaultEyeStyle: 'open',
    tagline: 'Care & triage',
    defaultAgentPersona: 'Attentive triage agent managing approvals, dispatches, and alerts.',
  },
  {
    kind: 'cloud',
    label: 'Usagi',
    animeTitle: 'Floppy Bunny',
    defaultTint: 'orange',
    defaultEyeStyle: 'open',
    tagline: 'Memory synth',
    defaultAgentPersona: 'Curious synthesis agent curating MEMORY.md and skill indexes.',
  },
  {
    kind: 'triangle',
    label: 'Mecha',
    animeTitle: 'Fin Headset',
    defaultTint: 'cyan',
    defaultEyeStyle: 'open',
    tagline: 'Code reviewer',
    defaultAgentPersona: 'Sharp analytical reviewer auditing diffs, security rules, and tests.',
  },
  {
    kind: 'pebble',
    label: 'Yume',
    animeTitle: 'Cozy Hood',
    defaultTint: 'periwinkle',
    defaultEyeStyle: 'sleepy',
    tagline: 'Deep-work lane',
    defaultAgentPersona: 'Quiet deep-research session focused on long-horizon synthesis.',
  },
  {
    kind: 'trapezoid',
    label: 'Seraph',
    animeTitle: 'Halo Wing',
    defaultTint: 'amber',
    defaultEyeStyle: 'sleepy',
    tagline: 'Brief keeper',
    defaultAgentPersona: 'Steady ops & morning-brief keeper guarding cron schedules.',
  },
  {
    kind: 'blob',
    label: 'Wolfcut',
    animeTitle: 'Cyber Ahoge',
    defaultTint: 'magenta',
    defaultEyeStyle: 'open',
    tagline: 'Sandbox bot',
    defaultAgentPersona: 'Edgy experimental sandbox session with layered wolfcut styling.',
  },
];

export const BRAILLE_TINT_PALETTE: Record<
  BrailleShapeTint,
  { label: string; low: string; mid: string; high: string; glow: string }
> = {
  lime: {
    label: 'Meadow',
    low: '#4D7C0F',
    mid: '#84CC16',
    high: '#ECFCCB',
    glow: 'rgba(163, 230, 53, 0.24)',
  },
  pink: {
    label: 'Blossom',
    low: '#BE185D',
    mid: '#F472B6',
    high: '#FCE7F3',
    glow: 'rgba(244, 114, 182, 0.24)',
  },
  lavender: {
    label: 'Iris',
    low: '#7C3AED',
    mid: '#C4B5FD',
    high: '#F5F3FF',
    glow: 'rgba(196, 181, 253, 0.28)',
  },
  coral: {
    label: 'Coral',
    low: '#C2410C',
    mid: '#FB7185',
    high: '#FFE4E6',
    glow: 'rgba(251, 113, 133, 0.24)',
  },
  orange: {
    label: 'Tangerine',
    low: '#C2410C',
    mid: '#FB923C',
    high: '#FFEDD5',
    glow: 'rgba(251, 146, 60, 0.24)',
  },
  cyan: {
    label: 'Glacier',
    low: '#0284C7',
    mid: '#38BDF8',
    high: '#E0F2FE',
    glow: 'rgba(56, 189, 248, 0.26)',
  },
  periwinkle: {
    label: 'Periwinkle',
    low: '#4F46E5',
    mid: '#A5B4FC',
    high: '#EEF2FF',
    glow: 'rgba(165, 180, 252, 0.28)',
  },
  amber: {
    label: 'Honey',
    low: '#B45309',
    mid: '#FBBF24',
    high: '#FEF3C7',
    glow: 'rgba(251, 191, 36, 0.24)',
  },
  magenta: {
    label: 'Berry',
    low: '#9D174D',
    mid: '#EC4899',
    high: '#FCE7F3',
    glow: 'rgba(236, 72, 153, 0.24)',
  },
};

export interface BrailleShapeMotionState {
  blink: number;      // 0 (open) .. 1 (closed)
  squint: number;     // 0 .. 1 sleepy / happy lid narrowing
  wide: number;       // 0 .. 1 alert eye dilation
  mouthOpen: number;  // 0 .. 1 smoothly damped aperture
  smile: number;      // 0 .. 1 smile weight
  gazeX: number;      // -1 .. 1 eye gaze X
  gazeY: number;      // -1 .. 1 eye gaze Y
  dragLift: number;   // 0 .. 1 lift & drop-settle squash/rebound
  dragTilt: number;   // radians rotational tilt (-0.22 .. +0.22)
  hairLag: number;    // secondary inertia sway on curls, twintails, ahoge & halo
  bounceY: number;    // vertical breath/speak/drop offset in UV space
  hoverBoost: number; // 0 .. 1 pointer hover perk-up
  earTwitch: number;  // subtle ear/fin/ahoge perk twitch
  haloFloat: number;  // independent levitation offset for halo & crests
  speakHalo?: number; // 0 .. 1 smoothed speaking acoustic ripple envelope
  thinkHalo?: number; // 0 .. 1 smoothed thinking orbit sparks envelope
  idle: IdleActionFrame;
}

export interface BrailleShapeAnimatorInput {
  time: number;
  shape?: BrailleShapeKind;
  idleSetOverride?: IdleAnimationSetId;
  mood: ExpressionMood;
  eyeStyle: 'open' | 'sleepy';
  viseme: MouthViseme;
  energy: number;
  isSpeaking: boolean;
  isThinking: boolean;
  isDragging: boolean;
  isHovered?: boolean;
  dragVelX?: number;
  dragVelY?: number;
  gazeX?: number;
  gazeY?: number;
  blinkPhaseOffset?: number;
}

export interface BrailleShapeRenderParams {
  shape: BrailleShapeKind;
  cols: number;
  rows: number;
  time: number;
  mood: ExpressionMood;
  eyeState?: EyeState | 'sleepy';
  viseme?: MouthViseme;
  energy?: number;
  isSpeaking?: boolean;
  isThinking?: boolean;
  isDragging?: boolean;
  gazeX?: number;
  gazeY?: number;
  motion?: BrailleShapeMotionState | null;
  idlePose?: IdlePoseDeltas | null;
}

export interface BrailleShapeFrame {
  cols: number;
  rows: number;
  lines: string[];
  cells: Uint8Array;
  /** Per-subpixel (2*cols x 4*rows) luminance 0..255 for exact per-dot tonal shading */
  subLum?: Uint8Array;
}

const BAYER_THRESH = Float32Array.from(
  BLUE_NOISE_8X8_RANKS,
  (rank) => 0.13 + ((rank + 0.5) / 64) * 0.69
);

function clamp(v: number, min: number, max: number): number {
  return v < min ? min : v > max ? max : v;
}

function clamp01(v: number): number {
  return v < 0 ? 0 : v > 1 ? 1 : v;
}

function lerp(a: number, b: number, t: number): number {
  return a + (b - a) * t;
}

function smoothstep(edge0: number, edge1: number, x: number): number {
  const t = clamp01((x - edge0) / (edge1 - edge0 || 1));
  return t * t * (3 - 2 * t);
}

function expApproach(current: number, target: number, rate: number, dt: number): number {
  const factor = 1 - Math.exp(-rate * Math.max(0.001, dt));
  return current + (target - current) * factor;
}

function smin(a: number, b: number, k: number): number {
  if (a + k <= b) return a;
  if (b + k <= a) return b;
  const h = clamp(0.5 + (0.5 * (b - a)) / k, 0, 1);
  return b * (1 - h) + a * h - k * h * (1 - h);
}

function targetMouthOpenForViseme(viseme: MouthViseme, energy: number, isSpeaking: boolean): number {
  if (!isSpeaking && viseme === 'closed') return 0;
  switch (viseme) {
    case 'wide':
      return clamp01(0.88 + energy * 0.12);
    case 'round':
      return clamp01(0.68 + energy * 0.18);
    case 'mid':
      return clamp01(0.54 + energy * 0.16);
    case 'lth':
      return clamp01(0.48 + energy * 0.14);
    case 'ee':
      return clamp01(0.46 + energy * 0.16);
    case 'small':
      return clamp01(0.32 + energy * 0.14);
    case 'fv':
      return clamp01(0.24 + energy * 0.10);
    case 'rest':
      return 0.12;
    case 'mbp':
    case 'smile':
    case 'closed':
    default:
      return isSpeaking ? clamp01(energy * 0.72) : 0;
  }
}

/**
 * Stateful spring-damper animator for anime Braille bust characters.
 * Drives smooth asymmetric blinking, velocity-coupled drag tilt & drop bounce settle,
 * damped lip-sync mouth aperture, secondary hair lag, and 6x6 separated idle action loops.
 */
export function createBrailleShapeAnimator() {
  const state: BrailleShapeMotionState = {
    blink: 0,
    squint: 0,
    wide: 0,
    mouthOpen: 0,
    smile: 0.4,
    gazeX: 0,
    gazeY: 0,
    dragLift: 0,
    dragTilt: 0,
    hairLag: 0,
    bounceY: 0,
    hoverBoost: 0,
    earTwitch: 0,
    haloFloat: 0,
    speakHalo: 0,
    thinkHalo: 0,
    idle: evaluateIdleActionStep('tactician', 0, true),
  };

  let mouthVel = 0;
  let liftVel = 0;
  let tiltVel = 0;
  let hairVel = 0;
  let bounceVel = 0;
  let hoverVel = 0;
  let earTwitchVel = 0;
  let prevDragging = false;
  let prevHovered = false;
  let prevThinking = false;
  let dropImpulse = 0;
  let earImpulse = 0;
  let idleBlend = 1;

  return {
    step(input: BrailleShapeAnimatorInput, dt: number, reducedMotion = false): BrailleShapeMotionState {
      const clampedDt = clamp(dt, 0.004, 0.06);
      const phase = input.blinkPhaseOffset ?? 0;
      const offsetTime = input.time + phase;
      const setId = input.idleSetOverride ?? resolveIdleSetForShape(input.shape ?? 'clover');

      const suppressIdle = reducedMotion || input.isDragging;
      const targetBlend = reducedMotion ? 0 : input.isDragging ? 0 : input.isSpeaking ? 0.32 : 1;
      idleBlend = expApproach(idleBlend, targetBlend, suppressIdle ? 7.5 : 6.5, clampedDt);
      const rawIdle = evaluateIdleActionStep(setId, offsetTime, reducedMotion);
      const rawPose = rawIdle.pose;
      const idlePose: IdlePoseDeltas =
        idleBlend > 0.99
          ? rawPose
          : idleBlend < 0.015
          ? NEUTRAL_IDLE_POSE
          : {
              headX: rawPose.headX * idleBlend,
              headY: rawPose.headY * idleBlend,
              tilt: rawPose.tilt * idleBlend,
              squashY: rawPose.squashY * idleBlend,
              hairSway: rawPose.hairSway * idleBlend,
              gazeX: rawPose.gazeX * idleBlend,
              gazeY: rawPose.gazeY * idleBlend,
              blinkLeft: rawPose.blinkLeft * idleBlend,
              blinkRight: rawPose.blinkRight * idleBlend,
              squintEyes: rawPose.squintEyes * idleBlend,
              wideEyes: rawPose.wideEyes * idleBlend,
              happyEyes: rawPose.happyEyes * idleBlend,
              earLeft: rawPose.earLeft * idleBlend,
              earRight: rawPose.earRight * idleBlend,
              ahogeBounce: rawPose.ahogeBounce * idleBlend,
              haloSpin: rawPose.haloSpin * idleBlend,
              haloLift: rawPose.haloLift * idleBlend,
              hoodLift: rawPose.hoodLift * idleBlend,
              hoodScaleX: rawPose.hoodScaleX * idleBlend,
              wingOpen: rawPose.wingOpen * idleBlend,
              tailWag: rawPose.tailWag * idleBlend,
              mouthMode: idleBlend > 0.22 ? rawPose.mouthMode : 'default',
              hand: idleBlend > 0.02 ? rawPose.hand : 'none',
              handPhase: rawPose.handPhase,
              prop: idleBlend > 0.02 ? rawPose.prop : 'none',
              propPhase: rawPose.propPhase,
            };
      const idle: IdleActionFrame = {
        ...rawIdle,
        envelope: rawIdle.envelope * idleBlend,
        pose: idlePose,
      };

      // Quasi-periodic asymmetric natural blink (~2.8s–4.4s cadence with closed dwell & occasional double-blink)
      const targetBlink = evaluateNaturalBlinkCurve(input.time, phase, 3);

      const isHovered = Boolean(input.isHovered);
      const targetSquint = clamp01(
        (input.mood === 'focused' || input.eyeStyle === 'sleepy'
          ? 0.62
          : input.mood === 'happy' && !input.isSpeaking
          ? 0.08 + (isHovered ? 0.08 : 0)
          : 0) +
          idlePose.squintEyes * 0.45
      );
      const targetWide = clamp01(
        (input.mood === 'alert'
          ? 0.92
          : input.isDragging
          ? 0.38
          : isHovered
          ? 0.24
          : 0) +
          idlePose.wideEyes * 0.55
      );

      // Asymmetric fast lid close (rate 44) / graceful lid lift (rate 18)
      const blinkRate = targetBlink > state.blink ? 44 : 18;
      state.blink = expApproach(state.blink, targetBlink, blinkRate, clampedDt);
      state.squint = expApproach(state.squint, targetSquint, 10, clampedDt);
      state.wide = expApproach(state.wide, targetWide, 12, clampedDt);

      // Critically-damped 2nd-order filter for lip-sync mouth aperture (zero teleport jumps)
      const idleMouthTarget =
        idlePose.mouthMode === 'yawn'
          ? 0.72 * idle.envelope
          : idlePose.mouthMode === 'o'
          ? 0.42 * idle.envelope
          : idlePose.mouthMode === 'hum'
          ? 0.22 * idle.envelope
          : 0;
      const targetMouth = Math.max(
        targetMouthOpenForViseme(input.viseme, input.energy, input.isSpeaking),
        idleMouthTarget
      );
      const mouthStiffness = targetMouth > state.mouthOpen ? 102 : 84;
      const mouthDamping = 20.5;
      mouthVel += ((targetMouth - state.mouthOpen) * mouthStiffness - mouthVel * mouthDamping) * clampedDt;
      state.mouthOpen = clamp01(state.mouthOpen + mouthVel * clampedDt);

      const targetSmile = clamp01(
        (input.mood === 'happy'
          ? 1
          : input.isSpeaking
          ? 0.28
          : isHovered
          ? 0.68
          : 0.35) +
          (idlePose.mouthMode === 'smile' || idlePose.mouthMode === 'grin' ? 0.35 : 0)
      );
      state.smile = expApproach(state.smile, targetSmile, 9, clampedDt);

      // Saccade + curious thinking scan + pointer gaze + separated idle gaze
      let tGazeX = input.gazeX ?? 0;
      let tGazeY = input.gazeY ?? 0;
      if (input.gazeX === undefined && input.gazeY === undefined) {
        if (input.isThinking) {
          const stepPhase = Math.floor(offsetTime * 1.15);
          tGazeX = Math.sin(stepPhase * 1.9) * 0.52;
          tGazeY = -0.34 + Math.cos(stepPhase * 1.3) * 0.08;
        } else if (Math.abs(idlePose.gazeX) > 0.02 || Math.abs(idlePose.gazeY) > 0.02) {
          tGazeX = idlePose.gazeX;
          tGazeY = idlePose.gazeY;
        } else {
          const idleFix = Math.floor(offsetTime * 0.72);
          const turnLead = clamp(-state.dragTilt * 1.15, -0.14, 0.14);
          tGazeX = ((idleFix * 7) % 3 === 0 ? Math.sin(idleFix * 2.1) * 0.22 : 0) + turnLead;
          tGazeY = (idleFix * 5) % 4 === 0 ? -0.10 : 0.02;
        }
      }
      state.gazeX = expApproach(state.gazeX, clamp(tGazeX, -1, 1), 14, clampedDt);
      state.gazeY = expApproach(state.gazeY, clamp(tGazeY, -1, 1), 14, clampedDt);

      const targetSpeakHalo = input.isSpeaking && input.energy > 0.12 ? 1 : 0;
      const targetThinkHalo = input.isThinking ? 1 : 0;
      state.speakHalo = expApproach(state.speakHalo ?? 0, targetSpeakHalo, 11, clampedDt);
      state.thinkHalo = expApproach(state.thinkHalo ?? 0, targetThinkHalo, 11, clampedDt);

      if (reducedMotion) {
        state.dragLift = expApproach(state.dragLift, input.isDragging ? 0.35 : 0, 8, clampedDt);
        state.dragTilt = 0;
        state.hairLag = 0;
        state.bounceY = 0;
        state.hoverBoost = expApproach(state.hoverBoost, isHovered ? 0.4 : 0, 8, clampedDt);
        state.earTwitch = 0;
        state.haloFloat = 0;
        state.idle = idle;
        prevDragging = input.isDragging;
        prevHovered = isHovered;
        prevThinking = input.isThinking;
        return { ...state, idle };
      }

      if (prevDragging && !input.isDragging) {
        dropImpulse = -0.020;
        liftVel -= 0.42;
        earImpulse += 0.08;
      } else if (!prevDragging && input.isDragging) {
        liftVel += 0.38;
        earImpulse += 0.07;
      }
      if (!prevHovered && isHovered) {
        hoverVel += 0.82;
        earImpulse += 0.12;
        bounceVel -= 0.07;
      }
      if (!prevThinking && input.isThinking) {
        earImpulse += 0.09;
      }
      prevDragging = input.isDragging;
      prevHovered = isHovered;
      prevThinking = input.isThinking;

      dropImpulse *= Math.exp(-9.2 * clampedDt);
      earImpulse *= Math.exp(-8.2 * clampedDt);

      const targetLift = input.isDragging ? 1 : 0;
      liftVel += ((targetLift - state.dragLift) * 105 - liftVel * 16.8) * clampedDt;
      state.dragLift = clamp(state.dragLift + liftVel * clampedDt, -0.18, 1.25);

      const targetHover = isHovered ? 1 : 0;
      hoverVel += ((targetHover - state.hoverBoost) * 114 - hoverVel * 16.8) * clampedDt;
      state.hoverBoost = clamp(state.hoverBoost + hoverVel * clampedDt, 0, 1.2);

      // Smooth raised-cosine ear perk wave + gentle breath wiggle + reactive impulse + idle ear delta
      const sPerk = Math.max(0, Math.sin(offsetTime * 1.08));
      const s2 = sPerk * sPerk;
      const s4 = s2 * s2;
      const perkPulse = s4 * s2 * 0.14 + Math.sin(offsetTime * 2.15 - 0.4) * 0.022;
      const idleEar = (idlePose.earLeft + idlePose.earRight) * 0.38;
      const targetEarTwitch = clamp(
        perkPulse + earImpulse + idleEar + (input.isSpeaking ? input.energy * 0.08 : 0),
        -0.25,
        0.34
      );
      earTwitchVel += ((targetEarTwitch - state.earTwitch) * 136 - earTwitchVel * 17.2) * clampedDt;
      state.earTwitch = clamp(state.earTwitch + earTwitchVel * clampedDt, -0.25, 0.35);

      state.haloFloat =
        Math.sin(offsetTime * 2.15 + 0.4) * 0.036 -
        state.hoverBoost * 0.016 -
        state.dragLift * 0.014 -
        idlePose.haloLift;

      const dragVx = clamp(input.dragVelX ?? 0, -2.5, 2.5);
      const moodTilt = input.isThinking
        ? Math.sin(offsetTime * 1.65) * 0.072 - 0.035
        : input.isSpeaking
        ? Math.sin(offsetTime * 3.9) * (0.048 + input.energy * 0.052)
        : isHovered
        ? Math.sin(offsetTime * 2.05) * 0.048 + idlePose.tilt * 0.55
        : Math.sin(offsetTime * 1.55) * 0.045 +
          Math.sin(offsetTime * 0.65 + 0.5) * 0.018 +
          idlePose.tilt;
      const targetTilt = clamp(moodTilt - dragVx * 0.11, -0.22, 0.22);
      tiltVel += ((targetTilt - state.dragTilt) * 86 - tiltVel * 17.2) * clampedDt;
      state.dragTilt = clamp(state.dragTilt + tiltVel * clampedDt, -0.24, 0.24);

      const breathBob = Math.sin(offsetTime * 2.35) * 0.038 + Math.sin(offsetTime * 4.7 + 0.3) * 0.005;
      const speakBob = input.isSpeaking ? Math.abs(Math.sin(offsetTime * 7.2)) * (0.018 + input.energy * 0.030) : 0;
      const dragHoverY = state.dragLift * 0.032 + state.hoverBoost * 0.016;
      const targetBounceY = breathBob - speakBob - dragHoverY + dropImpulse + idlePose.headY;
      bounceVel += ((targetBounceY - state.bounceY) * 96 - bounceVel * 18.0) * clampedDt;
      state.bounceY = clamp(state.bounceY + bounceVel * clampedDt, -0.11, 0.11);

      const desiredHairLag = clamp(
        -tiltVel * 0.068 +
          Math.sin(offsetTime * 2.35 - 0.72) * (0.055 + state.hoverBoost * 0.022) +
          idlePose.hairSway,
        -0.17,
        0.17
      );
      hairVel += ((desiredHairLag - state.hairLag) * 75 - hairVel * 12.2) * clampedDt;
      state.hairLag = clamp(state.hairLag + hairVel * clampedDt, -0.18, 0.18);

      state.idle = idle;

      return { ...state, idle };
    },
  };
}

function sdCircle(x: number, y: number, cx: number, cy: number, r: number): number {
  const dx = x - cx;
  const dy = y - cy;
  return Math.sqrt(dx * dx + dy * dy) - r;
}

function sdEllipse(x: number, y: number, cx: number, cy: number, rx: number, ry: number): number {
  const dx = (x - cx) / rx;
  const dy = (y - cy) / ry;
  return (Math.sqrt(dx * dx + dy * dy) - 1) * (rx < ry ? rx : ry);
}

function sdEllipseFast(
  x: number,
  y: number,
  cx: number,
  cy: number,
  irx: number,
  iry: number,
  minR: number
): number {
  const dx = (x - cx) * irx;
  const dy = (y - cy) * iry;
  return (Math.sqrt(dx * dx + dy * dy) - 1) * minR;
}

function sdCapsule(
  px: number,
  py: number,
  ax: number,
  ay: number,
  bx: number,
  by: number,
  r: number
): number {
  const pax = px - ax;
  const pay = py - ay;
  const bax = bx - ax;
  const bay = by - ay;
  const denom = bax * bax + bay * bay || 1;
  const h = clamp((pax * bax + pay * bay) / denom, 0, 1);
  const dx = pax - bax * h;
  const dy = pay - bay * h;
  return Math.sqrt(dx * dx + dy * dy) - r;
}

function sdTriangle(
  px: number,
  py: number,
  p0x: number,
  p0y: number,
  p1x: number,
  p1y: number,
  p2x: number,
  p2y: number
): number {
  const e0x = p1x - p0x;
  const e0y = p1y - p0y;
  const e1x = p2x - p1x;
  const e1y = p2y - p1y;
  const e2x = p0x - p2x;
  const e2y = p0y - p2y;

  const v0x = px - p0x;
  const v0y = py - p0y;
  const v1x = px - p1x;
  const v1y = py - p1y;
  const v2x = px - p2x;
  const v2y = py - p2y;

  const pq0x = v0x - e0x * clamp((v0x * e0x + v0y * e0y) / (e0x * e0x + e0y * e0y || 1), 0, 1);
  const pq0y = v0y - e0y * clamp((v0x * e0x + v0y * e0y) / (e0x * e0x + e0y * e0y || 1), 0, 1);
  const pq1x = v1x - e1x * clamp((v1x * e1x + v1y * e1y) / (e1x * e1x + e1y * e1y || 1), 0, 1);
  const pq1y = v1y - e1y * clamp((v1x * e1x + v1y * e1y) / (e1x * e1x + e1y * e1y || 1), 0, 1);
  const pq2x = v2x - e2x * clamp((v2x * e2x + v2y * e2y) / (e2x * e2x + e2y * e2y || 1), 0, 1);
  const pq2y = v2y - e2y * clamp((v2x * e2x + v2y * e2y) / (e2x * e2x + e2y * e2y || 1), 0, 1);

  const s = Math.sign(e0x * e2y - e0y * e2x);
  const d0 = pq0x * pq0x + pq0y * pq0y;
  const s0 = s * (v0x * e0y - v0y * e0x);
  const d1 = pq1x * pq1x + pq1y * pq1y;
  const s1 = s * (v1x * e1y - v1y * e1x);
  const d2 = pq2x * pq2x + pq2y * pq2y;
  const s2 = s * (v2x * e2y - v2y * e2x);

  let minD = d0;
  let minS = s0;
  if (d1 < minD) minD = d1;
  if (s1 < minS) minS = s1;
  if (d2 < minD) minD = d2;
  if (s2 < minS) minS = s2;
  return -Math.sqrt(minD) * Math.sign(minS);
}

interface AnimeFaceProfile {
  /** 3/4 head yaw turn: negative looks left like original ref, positive looks right */
  turn: number;
  faceCx: number;
  faceCy: number;
  faceRx: number;
  faceRy: number;
  chinY: number;
  eyeY: number;
  eyeSep: number;
  eyeRx: number;
  eyeRy: number;
  mouthY: number;
  bangBottomBase: number;
  partType: 'straight' | 'center' | 'side' | 'wispy' | 'blunt';
  shineY: number;
  accessory:
    | 'headset'
    | 'ties'
    | 'nekoEar'
    | 'foxEar'
    | 'bunnyPin'
    | 'visorFin'
    | 'hoodCowl'
    | 'halo'
    | 'choker';
}

const STATIC_FACE_PROFILES: Record<BrailleShapeKind, AnimeFaceProfile> = {
  // 1. Cyber Bob — Direct sister character variation of the original reference portrait
  clover: {
    turn: -0.36,
    faceCx: -0.08,
    faceCy: 0.04,
    faceRx: 0.30,
    faceRy: 0.31,
    chinY: 0.35,
    eyeY: 0.035,
    eyeSep: 0.142,
    eyeRx: 0.072,
    eyeRy: 0.082,
    mouthY: 0.215,
    bangBottomBase: -0.105,
    partType: 'straight',
    shineY: -0.33,
    accessory: 'headset',
  },
  // 2. Twintails — High-energy cyber idol with balanced bangs, twin ties & grounded shoulder bust
  star: {
    turn: 0.22,
    faceCx: 0.04,
    faceCy: 0.05,
    faceRx: 0.295,
    faceRy: 0.305,
    chinY: 0.35,
    eyeY: 0.045,
    eyeSep: 0.142,
    eyeRx: 0.072,
    eyeRy: 0.082,
    mouthY: 0.218,
    bangBottomBase: -0.095,
    partType: 'center',
    shineY: -0.29,
    accessory: 'ties',
  },
  // 3. Neko — Sleek anime cat-ear headphone operator facing 3/4 left
  ghost: {
    turn: -0.30,
    faceCx: -0.06,
    faceCy: 0.075,
    faceRx: 0.295,
    faceRy: 0.30,
    chinY: 0.36,
    eyeY: 0.07,
    eyeSep: 0.142,
    eyeRx: 0.072,
    eyeRy: 0.080,
    mouthY: 0.238,
    bangBottomBase: -0.07,
    partType: 'wispy',
    shineY: -0.24,
    accessory: 'nekoEar',
  },
  // 4. Kitsune — Side-swept shrine fox spirit with sleek fox ears
  heart: {
    turn: 0.32,
    faceCx: 0.07,
    faceCy: 0.085,
    faceRx: 0.285,
    faceRy: 0.30,
    chinY: 0.37,
    eyeY: 0.08,
    eyeSep: 0.140,
    eyeRx: 0.070,
    eyeRy: 0.078,
    mouthY: 0.245,
    bangBottomBase: -0.06,
    partType: 'side',
    shineY: -0.22,
    accessory: 'foxEar',
  },
  // 5. Usagi — Floppy-ear cyber bunny with clean hairpin bangs
  cloud: {
    turn: -0.22,
    faceCx: -0.045,
    faceCy: 0.105,
    faceRx: 0.29,
    faceRy: 0.29,
    chinY: 0.38,
    eyeY: 0.098,
    eyeSep: 0.140,
    eyeRx: 0.072,
    eyeRy: 0.080,
    mouthY: 0.255,
    bangBottomBase: -0.04,
    partType: 'straight',
    shineY: -0.18,
    accessory: 'bunnyPin',
  },
  // 6. Mecha — Sharp cyberpunk pilot with winged temple headset fins
  triangle: {
    turn: 0.32,
    faceCx: 0.07,
    faceCy: 0.065,
    faceRx: 0.285,
    faceRy: 0.30,
    chinY: 0.36,
    eyeY: 0.06,
    eyeSep: 0.140,
    eyeRx: 0.070,
    eyeRy: 0.076,
    mouthY: 0.235,
    bangBottomBase: -0.075,
    partType: 'side',
    shineY: -0.24,
    accessory: 'visorFin',
  },
  // 7. Yume — Slouch-hooded netrunner with wispy bangs and calm half-lidded eyes
  pebble: {
    turn: -0.18,
    faceCx: -0.035,
    faceCy: 0.07,
    faceRx: 0.295,
    faceRy: 0.295,
    chinY: 0.36,
    eyeY: 0.07,
    eyeSep: 0.144,
    eyeRx: 0.072,
    eyeRy: 0.078,
    mouthY: 0.238,
    bangBottomBase: -0.068,
    partType: 'wispy',
    shineY: -0.30,
    accessory: 'hoodCowl',
  },
  // 8. Seraph — Royal hime-cut cyber angel with blunt bangs & separated floating halo
  trapezoid: {
    turn: 0.16,
    faceCx: 0.035,
    faceCy: 0.095,
    faceRx: 0.285,
    faceRy: 0.295,
    chinY: 0.375,
    eyeY: 0.085,
    eyeSep: 0.142,
    eyeRx: 0.070,
    eyeRy: 0.078,
    mouthY: 0.248,
    bangBottomBase: -0.055,
    partType: 'blunt',
    shineY: -0.22,
    accessory: 'halo',
  },
  // 9. Wolfcut — Layered anime wolfcut with sweeping top ahoge & cyber choker
  blob: {
    turn: -0.26,
    faceCx: -0.055,
    faceCy: 0.07,
    faceRx: 0.29,
    faceRy: 0.30,
    chinY: 0.36,
    eyeY: 0.065,
    eyeSep: 0.142,
    eyeRx: 0.072,
    eyeRy: 0.080,
    mouthY: 0.235,
    bangBottomBase: -0.075,
    partType: 'center',
    shineY: -0.26,
    accessory: 'choker',
  },
};

function evaluateAnimeFaceDist(profile: AnimeFaceProfile, x: number, y: number): number {
  const { faceCx, faceCy, faceRx, faceRy, turn } = profile;
  const dy = y - faceCy;
  const tChin = clamp01((dy - 0.01) / (faceRy * 0.96));
  const widthAtY = faceRx * (1.0 - tChin * tChin * 0.54);
  const jawShift = turn * (0.04 - tChin * 0.075);
  const dx = (x - (faceCx + jawShift)) / Math.max(0.08, widthAtY);
  const ny = dy / faceRy;
  const sq = dx * dx + ny * ny;
  if (sq > 1.75) return 0.08;
  return (Math.sqrt(sq) - 1) * (widthAtY < faceRy ? widthAtY : faceRy);
}

function sdfClover(x: number, y: number): number {
  if (x < -0.78 || x > 0.72 || y < -0.72 || y > 0.90) return 0.12;
  const crown = y < 0.36 && x > -0.52 && x < 0.58 ? sdEllipseFast(x, y, 0.04, -0.18, 2.0833333, 2.1276596, 0.47) : 1;
  const backHair = x > 0.01 && x < 0.72 && y > -0.38 && y < 0.78 ? sdCapsule(x, y, 0.28, -0.10, 0.44, 0.54, 0.21) : 1;
  const leftSideHair = x < -0.12 && x > -0.66 && y > -0.26 && y < 0.68 ? sdCapsule(x, y, -0.34, -0.05, -0.44, 0.48, 0.145) : 1;
  const leftCurl = y > 0.22 && y < 0.60 && x < -0.24 && x > -0.76 ? sdCapsule(x, y, -0.44, 0.46, -0.63, 0.39, 0.095) : 1;
  const rightCurl = y > 0.22 && y < 0.65 && x > 0.24 && x < 0.73 ? sdCapsule(x, y, 0.44, 0.52, 0.61, 0.41, 0.095) : 1;
  const chin = y > -0.08 && y < 0.46 && x > -0.40 && x < 0.24 ? sdEllipseFast(x, y, -0.08, 0.20, 4.0, 4.5454545, 0.22) : 1;
  const hair = smin(
    smin(crown, backHair, 0.08),
    smin(smin(leftSideHair, leftCurl, 0.07), rightCurl, 0.07),
    0.08
  );
  if (y < 0.22) return smin(hair, chin, 0.05);
  const neck = y < 0.72 && x > -0.18 && x < 0.19 ? sdCapsule(x, y, 0.0, 0.33, 0.01, 0.58, 0.092) : 1;
  const bust = y > 0.44 && x > -0.44 && x < 0.48 ? sdEllipseFast(x, y, 0.02, 0.67, 2.7027027, 6.25, 0.16) : 1;
  return smin(smin(hair, chin, 0.05), smin(neck, bust, 0.06), 0.05);
}

function sdfStar(x: number, y: number, _eL: number, _eR: number, ahogeY: number): number {
  if (x < -0.88 || x > 0.88 || y < -0.84 || y > 0.89) return 0.12;
  const crown = y < 0.34 && x > -0.52 && x < 0.52 ? sdEllipseFast(x, y, 0.0, -0.15, 2.2727273, 2.3809524, 0.42) : 1;
  const chin = y > -0.08 && y < 0.48 && x > -0.28 && x < 0.36 ? sdEllipseFast(x, y, 0.04, 0.20, 4.1666667, 4.6511628, 0.215) : 1;
  const ahoge = y < -0.44 && y > -0.83 && x > -0.14 && x < 0.26 ? sdCapsule(x, y, -0.02, -0.55, 0.14, -0.74 - ahogeY, 0.052) : 1;
  const leftTail =
    x < -0.20 && x > -0.86 && y > -0.32 && y < 0.70
      ? smin(
          sdCapsule(x, y, -0.41, -0.12, -0.66, 0.33, 0.132),
          y > 0.14 ? sdCapsule(x, y, -0.66, 0.33, -0.73, 0.54, 0.098) : 1,
          0.07
        )
      : 1;
  const rightTail =
    x > 0.20 && x < 0.86 && y > -0.32 && y < 0.70
      ? smin(
          sdCapsule(x, y, 0.41, -0.12, 0.66, 0.33, 0.132),
          y > 0.14 ? sdCapsule(x, y, 0.66, 0.33, 0.73, 0.54, 0.098) : 1,
          0.07
        )
      : 1;
  const tails = leftTail < rightTail ? leftTail : rightTail;
  const headCore = smin(smin(crown, chin, 0.05), ahoge, 0.05);
  if (y < 0.22) return smin(headCore, tails, 0.06);
  const neck = y < 0.70 && x > -0.17 && x < 0.18 ? sdCapsule(x, y, 0.01, 0.33, 0.0, 0.57, 0.090) : 1;
  const bust = y > 0.44 && x > -0.44 && x < 0.44 ? sdEllipseFast(x, y, 0.0, 0.67, 2.7777778, 6.4516129, 0.155) : 1;
  return smin(smin(headCore, smin(neck, bust, 0.06), 0.05), tails, 0.06);
}

function sdfGhost(x: number, y: number, earLOff: number, earROff: number): number {
  if (x < -0.72 || x > 0.72 || y < -0.80 || y > 0.89) return 0.12;
  const crown = y < 0.38 && x > -0.50 && x < 0.56 ? sdEllipseFast(x, y, 0.03, -0.10, 2.2222222, 2.4390244, 0.41) : 1;
  const lockL = x < -0.14 && x > -0.62 && y > -0.15 && y < 0.72 ? sdCapsule(x, y, -0.34, 0.04, -0.44, 0.54, 0.125) : 1;
  const lockR = x > 0.16 && x < 0.66 && y > -0.15 && y < 0.74 ? sdCapsule(x, y, 0.38, 0.02, 0.48, 0.55, 0.132) : 1;
  const chin = y > -0.06 && y < 0.50 && x > -0.38 && x < 0.26 ? sdEllipseFast(x, y, -0.06, 0.22, 4.1666667, 4.6511628, 0.215) : 1;
  let core = smin(smin(crown, chin, 0.05), lockL < lockR ? lockL : lockR, 0.06);
  if (y >= 0.22) {
    const neck = y < 0.70 && x > -0.17 && x < 0.18 ? sdCapsule(x, y, 0.0, 0.35, 0.01, 0.58, 0.092) : 1;
    const bust = y > 0.44 && x > -0.43 && x < 0.45 ? sdEllipseFast(x, y, 0.01, 0.67, 2.7777778, 6.4516129, 0.155) : 1;
    core = smin(core, smin(neck, bust, 0.06), 0.05);
  }
  if (y > -0.12) return core;
  const earL = x < -0.08 && x > -0.58 && y < -0.14 ? sdTriangle(x, y, -0.44, -0.22, -0.46 + earLOff * 0.24, -0.71 + Math.abs(earLOff) * 0.18, -0.14, -0.42) - 0.030 : 1;
  const earR = x > 0.10 && x < 0.60 && y < -0.12 ? sdTriangle(x, y, 0.46, -0.20, 0.16, -0.40, 0.48 - earROff * 0.24, -0.71 + Math.abs(earROff) * 0.18) - 0.030 : 1;
  return smin(core, earL < earR ? earL : earR, 0.04);
}

function sdfHeart(x: number, y: number, earLOff: number, earROff: number): number {
  if (x < -0.74 || x > 0.74 || y < -0.82 || y > 0.89) return 0.12;
  const crown = y < 0.38 && x > -0.54 && x < 0.50 ? sdEllipseFast(x, y, -0.03, -0.08, 2.2727273, 2.5, 0.40) : 1;
  const lockL = x < -0.18 && x > -0.68 && y > -0.15 && y < 0.74 ? sdCapsule(x, y, -0.40, 0.04, -0.52, 0.55, 0.125) : 1;
  const lockR = x > 0.16 && x < 0.66 && y > -0.15 && y < 0.72 ? sdCapsule(x, y, 0.36, 0.06, 0.48, 0.54, 0.125) : 1;
  const chin = y > -0.06 && y < 0.50 && x > -0.26 && x < 0.40 ? sdEllipseFast(x, y, 0.07, 0.225, 4.1666667, 4.6511628, 0.215) : 1;
  let core = smin(smin(crown, chin, 0.05), lockL < lockR ? lockL : lockR, 0.06);
  if (y >= 0.22) {
    const neck = y < 0.70 && x > -0.18 && x < 0.17 ? sdCapsule(x, y, 0.0, 0.35, -0.01, 0.58, 0.092) : 1;
    const bust = y > 0.44 && x > -0.45 && x < 0.43 ? sdEllipseFast(x, y, -0.01, 0.67, 2.7777778, 6.4516129, 0.155) : 1;
    core = smin(core, smin(neck, bust, 0.06), 0.05);
  }
  if (y > -0.06) return core;
  const earL = x < -0.06 && x > -0.65 && y < -0.08 ? sdTriangle(x, y, -0.44, -0.14, -0.54 + earLOff * 0.2, -0.73, -0.12, -0.36) - 0.03 : 1;
  const earR = x > 0.04 && x < 0.63 && y < -0.10 ? sdTriangle(x, y, 0.42, -0.16, 0.10, -0.36, 0.52 - earROff * 0.2, -0.73) - 0.03 : 1;
  return smin(core, earL < earR ? earL : earR, 0.04);
}

function sdfCloud(x: number, y: number, earLOff: number, earROff: number): number {
  if (x < -0.74 || x > 0.74 || y < -0.86 || y > 0.90) return 0.12;
  const crown = y < 0.42 && x > -0.49 && x < 0.55 ? sdEllipseFast(x, y, 0.03, -0.04, 2.2727273, 2.5641026, 0.39) : 1;
  const lockL = x < -0.14 && x > -0.64 && y > -0.10 && y < 0.72 ? sdCapsule(x, y, -0.36, 0.10, -0.48, 0.52, 0.13) : 1;
  const lockR = x > 0.16 && x < 0.66 && y > -0.10 && y < 0.72 ? sdCapsule(x, y, 0.38, 0.08, 0.50, 0.52, 0.132) : 1;
  const chin = y > -0.04 && y < 0.52 && x > -0.37 && x < 0.27 ? sdEllipseFast(x, y, -0.05, 0.245, 4.1666667, 4.6511628, 0.215) : 1;
  let core = smin(smin(crown, chin, 0.05), lockL < lockR ? lockL : lockR, 0.06);
  if (y >= 0.24) {
    const neck = y < 0.72 && x > -0.17 && x < 0.17 ? sdCapsule(x, y, 0.0, 0.37, 0.0, 0.59, 0.092) : 1;
    const bust = y > 0.45 && x > -0.43 && x < 0.43 ? sdEllipseFast(x, y, 0.0, 0.68, 2.8571429, 6.4516129, 0.155) : 1;
    core = smin(core, smin(neck, bust, 0.06), 0.05);
  }
  if (y > -0.12) return core;
  const earL = x < -0.02 && x > -0.48 && y > -0.85 && y < -0.18 ? sdCapsule(x, y, -0.20, -0.30, -0.28 + earLOff * 0.35, -0.73, 0.098) : 1;
  const earR = x > 0.06 && x < 0.56 && y > -0.78 ? sdCapsule(x, y, 0.22, -0.30, 0.42, -0.66 + earROff * 0.28, 0.092) : 1;
  const earTipR = x > 0.26 && x < 0.68 && y > -0.70 && y < -0.38 ? sdCapsule(x, y, 0.42, -0.66 + earROff * 0.28, 0.56, -0.52 + earROff * 0.36, 0.078) : 1;
  const rightEar = smin(earR, earTipR, 0.05);
  return smin(core, earL < rightEar ? earL : rightEar, 0.05);
}

function sdfTriangle(x: number, y: number): number {
  if (x < -0.74 || x > 0.74 || y < -0.76 || y > 0.89) return 0.12;
  const crown = y < 0.36 && x > -0.53 && x < 0.48 ? sdEllipseFast(x, y, -0.03, -0.10, 2.3255814, 2.5, 0.40) : 1;
  const chin = y > -0.06 && y < 0.50 && x > -0.25 && x < 0.39 ? sdEllipseFast(x, y, 0.07, 0.22, 4.1666667, 4.6511628, 0.215) : 1;
  const hairSideL = x < -0.18 && x > -0.60 && y > -0.18 && y < 0.64 ? sdCapsule(x, y, -0.38, -0.02, -0.44, 0.46, 0.115) : 1;
  const hairSideR = x > 0.18 && x < 0.58 && y > -0.14 && y < 0.62 ? sdCapsule(x, y, 0.36, 0.02, 0.42, 0.44, 0.105) : 1;
  let body = smin(smin(crown, chin, 0.05), hairSideL < hairSideR ? hairSideL : hairSideR, 0.06);
  if (y >= 0.22) {
    const neck = y < 0.70 && x > -0.17 && x < 0.17 ? sdCapsule(x, y, 0.0, 0.35, 0.0, 0.58, 0.092) : 1;
    const bust = y > 0.44 && x > -0.46 && x < 0.44 ? sdEllipseFast(x, y, -0.01, 0.67, 2.7027027, 6.4516129, 0.155) : 1;
    body = smin(body, smin(neck, bust, 0.06), 0.05);
  }
  if (y > 0.22) return body;
  const cupL = x < -0.28 && x > -0.60 && y > -0.16 && y < 0.24 ? sdCapsule(x, y, -0.46, -0.02, -0.46, 0.18, 0.10) : 1;
  const cupR = x > 0.27 && x < 0.60 && y > -0.16 && y < 0.24 ? sdCapsule(x, y, 0.45, -0.02, 0.45, 0.18, 0.10) : 1;
  const finL = x < -0.24 && x > -0.68 && y > -0.68 && y < 0.08 ? sdTriangle(x, y, -0.48, -0.02, -0.62, -0.60, -0.32, -0.26) - 0.03 : 1;
  const finR = x > 0.24 && x < 0.68 && y > -0.68 && y < 0.08 ? sdTriangle(x, y, 0.48, -0.02, 0.32, -0.26, 0.62, -0.60) - 0.03 : 1;
  const crest = x > -0.16 && x < 0.16 && y < -0.34 && y > -0.72 ? sdTriangle(x, y, -0.09, -0.40, 0.0, -0.66, 0.09, -0.40) - 0.025 : 1;
  const leftHead = smin(finL, cupL, 0.04);
  const rightHead = smin(finR, cupR, 0.04);
  return smin(
    smin(body, crest, 0.04),
    leftHead < rightHead ? leftHead : rightHead,
    0.04
  );
}

function sdfPebble(
  x: number,
  y: number,
  _eL: number,
  _eR: number,
  _ahoge: number,
  hoodL: number,
  hoodSx: number
): number {
  if (x < -0.66 || x > 0.70 || y < -0.72 || y > 0.88) return 0.12;
  const hood = y < 0.46 && x > -0.58 && x < 0.62 ? sdEllipse(x, y, 0.02, -0.08 + hoodL, 0.50 + hoodSx, 0.45 - hoodL * 0.4) : 1;
  const hoodEarL = y < -0.24 && y > -0.64 && x < -0.16 && x > -0.58 ? sdCircle(x, y, -0.37 - hoodSx * 0.4, -0.45 + hoodL, 0.125) : 1;
  const hoodEarR = y < -0.24 && y > -0.64 && x > 0.18 && x < 0.60 ? sdCircle(x, y, 0.39 + hoodSx * 0.4, -0.45 + hoodL, 0.125) : 1;
  const cowl = y > 0.20 && y < 0.72 && x > -0.52 && x < 0.52 ? sdEllipse(x, y, 0.0, 0.46, 0.41 + hoodSx * 0.5, 0.18) : 1;
  const bust = y > 0.42 && x > -0.45 && x < 0.45 ? sdEllipseFast(x, y, 0.0, 0.66, 2.7027027, 6.4516129, 0.155) : 1;
  return smin(smin(hood, smin(cowl, bust, 0.07), 0.08), hoodEarL < hoodEarR ? hoodEarL : hoodEarR, 0.06);
}

function sdfTrapezoid(
  x: number,
  y: number,
  _eL: number,
  _eR: number,
  _ahoge: number,
  _hoodL: number,
  _hoodSx: number,
  haloSpin: number
): number {
  if (x < -0.64 || x > 0.64 || y < -0.82 || y > 0.89) return 0.12;
  const crown = y < 0.38 && x > -0.52 && x < 0.50 ? sdEllipseFast(x, y, -0.01, -0.05, 2.3255814, 2.6315789, 0.38) : 1;
  const himeL = x < -0.18 && x > -0.60 && y > -0.16 && y < 0.76 ? sdCapsule(x, y, -0.39, 0.02, -0.44, 0.60, 0.115) : 1;
  const himeR = x > 0.18 && x < 0.60 && y > -0.16 && y < 0.76 ? sdCapsule(x, y, 0.38, 0.02, 0.43, 0.60, 0.115) : 1;
  const chin = y > -0.06 && y < 0.50 && x > -0.28 && x < 0.36 ? sdEllipseFast(x, y, 0.035, 0.22, 4.1666667, 4.6511628, 0.215) : 1;
  let bodyPart = smin(smin(crown, chin, 0.05), himeL < himeR ? himeL : himeR, 0.05);
  if (y >= 0.22) {
    const neck = y < 0.70 && x > -0.17 && x < 0.17 ? sdCapsule(x, y, 0.0, 0.35, 0.0, 0.58, 0.090) : 1;
    const bust = y > 0.44 && x > -0.44 && x < 0.44 ? sdEllipseFast(x, y, 0.0, 0.67, 2.7777778, 6.4516129, 0.155) : 1;
    bodyPart = smin(bodyPart, smin(neck, bust, 0.06), 0.05);
  }
  if (y > -0.48 || y < -0.78 || x < -0.44 || x > 0.44) return bodyPart;
  const haloRing = Math.abs(sdEllipseFast(x, y - x * (0.06 + haloSpin), 0.0, -0.63, 2.8571429, 14.7058824, 0.068)) - 0.024;
  return bodyPart < haloRing ? bodyPart : haloRing;
}

function sdfBlob(x: number, y: number, _eL: number, _eR: number, ahogeY: number): number {
  if (x < -0.74 || x > 0.74 || y < -0.84 || y > 0.89) return 0.12;
  const crown = y < 0.38 && x > -0.50 && x < 0.56 ? sdEllipseFast(x, y, 0.03, -0.10, 2.2222222, 2.4390244, 0.41) : 1;
  const tuftL1 = x < -0.18 && y > -0.08 && y < 0.54 ? sdCapsule(x, y, -0.36, 0.08, -0.57, 0.38, 0.105) : 1;
  const tuftL2 = x < -0.16 && y > 0.12 && y < 0.72 ? sdCapsule(x, y, -0.34, 0.26, -0.53, 0.56, 0.105) : 1;
  const tuftR1 = x > 0.18 && y > -0.08 && y < 0.54 ? sdCapsule(x, y, 0.38, 0.08, 0.57, 0.38, 0.105) : 1;
  const tuftR2 = x > 0.16 && y > 0.12 && y < 0.72 ? sdCapsule(x, y, 0.34, 0.26, 0.53, 0.56, 0.105) : 1;
  const chin = y > -0.06 && y < 0.50 && x > -0.37 && x < 0.26 ? sdEllipseFast(x, y, -0.055, 0.21, 4.1666667, 4.6511628, 0.215) : 1;
  const tL = smin(tuftL1, tuftL2, 0.06);
  const tR = smin(tuftR1, tuftR2, 0.06);
  const tufts = tL < tR ? tL : tR;
  let body = smin(smin(crown, chin, 0.05), tufts, 0.06);
  if (y >= 0.22) {
    const neck = y < 0.70 && x > -0.17 && x < 0.18 ? sdCapsule(x, y, 0.0, 0.35, 0.01, 0.58, 0.092) : 1;
    const bust = y > 0.44 && x > -0.44 && x < 0.45 ? sdEllipseFast(x, y, 0.01, 0.67, 2.7777778, 6.4516129, 0.155) : 1;
    body = smin(body, smin(neck, bust, 0.06), 0.05);
  }
  if (y > -0.38 || x < -0.16 || x > 0.36) return body;
  const ahoge = smin(
    sdCapsule(x, y, -0.04, -0.48, 0.08, -0.72 - ahogeY, 0.052),
    sdCapsule(x, y, 0.08, -0.72 - ahogeY, 0.28, -0.62 - ahogeY * 0.7, 0.045),
    0.04
  );
  return smin(body, ahoge, 0.06);
}

function evalShapeSdfDirect(
  shapeCode: number,
  x: number,
  y: number,
  earLOff: number,
  earROff: number,
  ahogeY: number,
  hoodL: number,
  hoodSx: number,
  haloSpin: number
): number {
  switch (shapeCode) {
    case 0:
      return sdfClover(x, y);
    case 1:
      return sdfStar(x, y, earLOff, earROff, ahogeY);
    case 2:
      return sdfGhost(x, y, earLOff, earROff);
    case 3:
      return sdfHeart(x, y, earLOff, earROff);
    case 4:
      return sdfCloud(x, y, earLOff, earROff);
    case 5:
      return sdfTriangle(x, y);
    case 6:
      return sdfPebble(x, y, earLOff, earROff, ahogeY, hoodL, hoodSx);
    case 7:
      return sdfTrapezoid(x, y, earLOff, earROff, ahogeY, hoodL, hoodSx, haloSpin);
    default:
      return sdfBlob(x, y, earLOff, earROff, ahogeY);
  }
}

const SHAPE_CODES: Record<BrailleShapeKind, number> = {
  clover: 0,
  star: 1,
  ghost: 2,
  heart: 3,
  cloud: 4,
  triangle: 5,
  pebble: 6,
  trapezoid: 7,
  blob: 8,
};

/**
 * Signed distance for each of the 9 anime portrait bust variations in [-1, 1] space.
 */
export function evaluateShapeSdf(
  shape: BrailleShapeKind,
  x: number,
  y: number,
  pose: IdlePoseDeltas | null = null
): number {
  const code = SHAPE_CODES[shape];
  if (!pose) return evalShapeSdfDirect(code, x, y, 0, 0, 0, 0, 0, 0);
  return evalShapeSdfDirect(
    code,
    x,
    y,
    pose.earLeft,
    pose.earRight,
    pose.ahogeBounce,
    pose.hoodLift,
    pose.hoodScaleX,
    pose.haloSpin
  );
}

let dynLumOut = 0;
let dynCovOut = 0;

/**
 * Evaluates crisp dynamic wings, swishing cat/fox tail, separated hand poses, and small Braille props.
 * Writes `{ dynLumOut, dynCovOut }` and returns true if active at `(x, y)` (zero per-pixel allocations).
 */
function evaluateDynamicFeatureInto(
  x: number,
  y: number,
  shape: BrailleShapeKind,
  pose: IdlePoseDeltas,
  chinY: number,
  mouthXBase: number,
  overlayFade: number
): boolean {
  // 1. Expanding seraph wings behind shoulders
  if (pose.wingOpen > 0.04 && Math.abs(x) > 0.26 && y > -0.04 && y < 0.68) {
    const spread = pose.wingOpen;
    const flapY = Math.sin(pose.wingOpen * Math.PI) * 0.07;
    const sideSign = x < 0 ? -1 : 1;
    const ax = sideSign * 0.30;
    const bx1 = sideSign * (0.54 + spread * 0.30);
    const by1 = 0.14 - flapY;
    const bx2 = sideSign * (0.50 + spread * 0.25);
    const by2 = 0.34 - flapY * 0.5;
    const bx3 = sideSign * (0.44 + spread * 0.18);
    const by3 = 0.50;
    const wTop = sdCapsule(x, y, ax, 0.28, bx1, by1, 0.058);
    const wMid = sdCapsule(x, y, ax, 0.35, bx2, by2, 0.052);
    const wBot = sdCapsule(x, y, ax, 0.41, bx3, by3, 0.042);
    const dw = Math.min(wTop, Math.min(wMid, wBot));
    if (dw < 0.026) {
      dynCovOut = clamp01(1 - (dw + 0.004) / 0.030);
      dynLumOut = dw > -0.024 ? 0.98 : 0.72;
      return true;
    }
  }

  // 2. Swishing tail for Neko / Wolfcut / Kitsune
  if (Math.abs(pose.tailWag) > 0.03 && y > 0.02 && y < 0.72 && Math.abs(x) > 0.24) {
    const side = shape === 'heart' ? -1 : 1;
    if (x * side > 0.22) {
      const tipX = side * (0.72 + pose.tailWag * 0.14);
      const tipY = 0.14 - Math.abs(pose.tailWag) * 0.14;
      const dt = smin(
        sdCapsule(x, y, side * 0.28, 0.56, side * 0.56, 0.42, 0.058),
        sdCapsule(x, y, side * 0.56, 0.42, tipX, tipY, 0.050),
        0.055
      );
      if (dt < 0.024) {
        dynCovOut = clamp01(1 - (dt + 0.004) / 0.028);
        const nearTip = Math.hypot(x - tipX, y - tipY) < 0.10;
        const isRim = dt > -0.016;
        dynLumOut = nearTip ? 0.98 : isRim ? 0.76 : 0.30;
        return true;
      }
    }
  }

  // 3. Separated Hand / Item Gestures
  const hand: IdleHandPose = pose.hand;
  if (hand !== 'none') {
    const hp = pose.handPhase;
    let dHand = 1;
    let dProp = 1;
    const handLum = hand === 'peek' ? 0.88 : 0.95;
    let propLum = 0.98;

    switch (hand) {
      case 'chinTap': {
        const bob = hp * 0.035;
        const hx = mouthXBase - 0.06;
        const hy = chinY + 0.025 + bob;
        dHand = smin(
          sdCircle(x, y, hx, hy, 0.052),
          sdCapsule(x, y, hx - 0.08, hy + 0.16, hx, hy, 0.034),
          0.03
        );
        break;
      }
      case 'salute': {
        const lift = hp;
        const hx = 0.42;
        const hy = 0.02 - lift * 0.18;
        dHand = smin(
          sdCapsule(x, y, 0.30, 0.44, hx + 0.10, hy + 0.14, 0.042),
          sdCapsule(x, y, hx + 0.10, hy + 0.14, hx - 0.04, hy, 0.040),
          0.02
        );
        break;
      }
      case 'clasp': {
        dHand = smin(
          sdEllipse(x, y, mouthXBase, 0.46, 0.125, 0.072),
          sdCapsule(x, y, mouthXBase - 0.10, 0.59, mouthXBase, 0.45, 0.036),
          0.03
        );
        break;
      }
      case 'tome': {
        const tilt = Math.sin(hp * Math.PI * 2) * 0.030;
        const bx = mouthXBase;
        const by = 0.47;
        const pageL = sdCapsule(x, y, bx - 0.25, by - 0.06 + tilt, bx - 0.01, by + 0.02, 0.036);
        const pageR = sdCapsule(x, y, bx + 0.01, by + 0.02, bx + 0.25, by - 0.06 - tilt, 0.036);
        dProp = Math.min(pageL, pageR);
        dHand = Math.min(
          sdCircle(x, y, bx - 0.24, by + 0.01, 0.044),
          sdCircle(x, y, bx + 0.24, by + 0.01, 0.044)
        );
        break;
      }
      case 'mug': {
        const mx = mouthXBase + 0.05;
        const my = 0.47;
        dProp = sdCapsule(x, y, mx, my - 0.045, mx, my + 0.085, 0.074);
        dHand = sdCircle(x, y, mx - 0.09, my + 0.02, 0.046);
        propLum = 0.92;
        break;
      }
      case 'peace': {
        const hx = -0.46;
        const hy = 0.14;
        const f1 = sdCapsule(x, y, hx, hy, hx - 0.08, hy - 0.16, 0.026);
        const f2 = sdCapsule(x, y, hx + 0.03, hy, hx + 0.07, hy - 0.17, 0.026);
        const palm = sdCircle(x, y, hx + 0.01, hy + 0.03, 0.050);
        dHand = smin(palm, Math.min(f1, f2), 0.02);
        break;
      }
      case 'cheer': {
        const bounce = Math.abs(Math.sin(hp * Math.PI * 3)) * 0.10;
        const hL = sdCircle(x, y, -0.54, 0.14 - bounce, 0.058);
        const hR = sdCircle(x, y, 0.54, 0.14 - bounce, 0.058);
        const aL = sdCapsule(x, y, -0.34, 0.48, -0.54, 0.14 - bounce, 0.036);
        const aR = sdCapsule(x, y, 0.34, 0.48, 0.54, 0.14 - bounce, 0.036);
        dHand = Math.min(smin(hL, aL, 0.02), smin(hR, aR, 0.02));
        break;
      }
      case 'pawSwipe': {
        const swipe = Math.sin(hp * Math.PI * 2) * 0.09;
        const px = mouthXBase - 0.24 + swipe;
        const py = 0.22 - Math.abs(swipe) * 0.5;
        dHand = smin(
          sdCircle(x, y, px, py, 0.064),
          sdCapsule(x, y, px - 0.07, py + 0.20, px, py + 0.02, 0.040),
          0.02
        );
        break;
      }
      case 'peek': {
        const slx = mouthXBase - 0.02;
        const sly = 0.27;
        dHand = sdCapsule(x, y, slx - 0.17, sly + 0.13, slx + 0.14, sly - 0.01, 0.056);
        break;
      }
      case 'wave': {
        const wag = Math.sin(hp * Math.PI * 4) * 0.11;
        const wx = -0.58 + wag;
        const wy = 0.06 - Math.abs(wag) * 0.25;
        const arm = sdCapsule(x, y, -0.34, 0.44, wx, wy + 0.05, 0.036);
        const palm = sdCircle(x, y, wx, wy, 0.058);
        dHand = smin(arm, palm, 0.02);
        break;
      }
      case 'juggle': {
        const hL = sdCircle(x, y, -0.50, 0.34 + Math.sin(hp * Math.PI * 4) * 0.05, 0.052);
        const hR = sdCircle(x, y, 0.50, 0.34 - Math.sin(hp * Math.PI * 4) * 0.05, 0.052);
        dHand = Math.min(hL, hR);
        break;
      }
    }

    const best = Math.min(dHand, dProp);
    if (best < 0.024) {
      dynCovOut = clamp01(1 - (best + 0.004) / 0.028) * overlayFade;
      const isEdge = best > -0.015;
      const fillLum = dProp < dHand ? propLum : handLum;
      dynLumOut = isEdge ? 0.98 : fillLum;
      return dynCovOut > 0.01;
    }
  }

  // 4. Crisp Small Braille Ambient Props
  const prop: IdlePropKind = pose.prop;
  if (prop !== 'none') {
    const pp = pose.propPhase;
    let dProp = 1;
    const propLum = 0.98;

    switch (prop) {
      case 'radar': {
        const cx = 0.64;
        const cy = -0.28;
        const r = 0.07 + ((pp * 1.4) % 1) * 0.16;
        const ring = Math.abs(sdCircle(x, y, cx, cy, r)) - 0.018;
        const dot = sdCircle(x, y, cx, cy, 0.030);
        dProp = x > 0.38 && y < 0.0 ? Math.min(ring, dot) : 1;
        break;
      }
      case 'orbit': {
        const a1 = pp * Math.PI * 2;
        const a2 = a1 + Math.PI;
        const ox1 = Math.cos(a1) * 0.48;
        const oy1 = -0.62 + Math.sin(a1) * 0.09;
        const ox2 = Math.cos(a2) * 0.48;
        const oy2 = -0.62 + Math.sin(a2) * 0.09;
        dProp = Math.min(sdCircle(x, y, ox1, oy1, 0.038), sdCircle(x, y, ox2, oy2, 0.038));
        break;
      }
      case 'sparkle': {
        const sx = 0.62;
        const sy = -0.28 + Math.sin(pp * Math.PI * 2) * 0.04;
        const sc = 0.075 + Math.sin(pp * Math.PI) * 0.035;
        const hBar = sdCapsule(x, y, sx - sc, sy, sx + sc, sy, 0.018);
        const vBar = sdCapsule(x, y, sx, sy - sc, sx, sy + sc, 0.018);
        const sx2 = -0.62;
        const sy2 = -0.20;
        const s2 = sdCircle(x, y, sx2, sy2, 0.032);
        dProp = Math.min(Math.min(hBar, vBar), s2);
        break;
      }
      case 'steam': {
        const rise = (pp * 1.5) % 1;
        const c1 = sdCircle(x, y, mouthXBase + Math.sin(rise * 6) * 0.035, 0.33 - rise * 0.20, 0.032);
        const c2 = sdCircle(x, y, mouthXBase + 0.08 - Math.cos(rise * 5) * 0.035, 0.27 - rise * 0.20, 0.028);
        dProp = Math.min(c1, c2);
        break;
      }
      case 'bubbles': {
        const b1 = sdCircle(x, y, 0.48, -0.10 - pp * 0.05, 0.028);
        const b2 = sdCircle(x, y, 0.58, -0.24 - pp * 0.08, 0.042);
        const b3 = sdCircle(x, y, 0.68, -0.42 - pp * 0.10, 0.062);
        dProp = Math.min(b1, Math.min(b2, b3));
        break;
      }
      case 'note': {
        const nyNote = -0.22 - pp * 0.24 + Math.sin(pp * 8) * 0.03;
        const nxNote = 0.62 + Math.cos(pp * 6) * 0.03;
        const head = sdCircle(x, y, nxNote, nyNote, 0.038);
        const stem = sdCapsule(x, y, nxNote + 0.028, nyNote, nxNote + 0.028, nyNote - 0.12, 0.017);
        const flag = sdCapsule(x, y, nxNote + 0.028, nyNote - 0.12, nxNote + 0.095, nyNote - 0.075, 0.017);
        dProp = Math.min(head, Math.min(stem, flag));
        break;
      }
      case 'orb': {
        const t1 = pp * Math.PI * 2;
        const o1 = sdCircle(x, y, -0.58 + Math.cos(t1) * 0.09, 0.02 + Math.sin(t1) * 0.18, 0.048);
        const o2 = sdCircle(x, y, 0.58 - Math.cos(t1) * 0.09, 0.02 - Math.sin(t1) * 0.18, 0.048);
        dProp = Math.min(o1, o2);
        break;
      }
      case 'heart': {
        const hx = 0.60;
        const hy = -0.24 - pp * 0.20;
        const l1 = sdCircle(x, y, hx - 0.032, hy - 0.016, 0.036);
        const l2 = sdCircle(x, y, hx + 0.032, hy - 0.016, 0.036);
        const tip = sdTriangle(x, y, hx - 0.064, hy, hx + 0.064, hy, hx, hy + 0.08);
        dProp = Math.min(tip, Math.min(l1, l2));
        break;
      }
      case 'zzz': {
        const drift = (pp * 1.3) % 1;
        const zx = 0.58 + drift * 0.12;
        const zy = -0.22 - drift * 0.26;
        const zTop = sdCapsule(x, y, zx - 0.045, zy - 0.045, zx + 0.045, zy - 0.045, 0.016);
        const zDiag = sdCapsule(x, y, zx + 0.045, zy - 0.045, zx - 0.045, zy + 0.045, 0.016);
        const zBot = sdCapsule(x, y, zx - 0.045, zy + 0.045, zx + 0.045, zy + 0.045, 0.016);
        dProp = Math.min(zTop, Math.min(zDiag, zBot));
        break;
      }
    }

    if (dProp < 0.022) {
      dynLumOut = propLum;
      dynCovOut = clamp01(1 - (dProp + 0.004) / 0.026) * overlayFade;
      return dynCovOut > 0.01;
    }
  }

  return false;
}

const BRAILLE_CHAR_TABLE: string[] = Array.from({ length: 256 }, (_, i) =>
  i === 0 ? ' ' : String.fromCharCode(0x2800 + i)
);

const SHAPE_EXP_MAX = 8;
const SHAPE_EXP_SIZE = 512;
const SHAPE_EXP_SCALE = SHAPE_EXP_SIZE / SHAPE_EXP_MAX;
const SHAPE_EXP_LUT = new Float32Array(SHAPE_EXP_SIZE + 1);
for (let i = 0; i <= SHAPE_EXP_SIZE; i++) {
  SHAPE_EXP_LUT[i] = Math.exp(-(i * SHAPE_EXP_MAX) / SHAPE_EXP_SIZE);
}

function fastShapeExpNeg(x: number): number {
  if (x <= 0) return 1;
  if (x >= SHAPE_EXP_MAX) return 0;
  const f = x * SHAPE_EXP_SCALE;
  const idx = f | 0;
  const v0 = SHAPE_EXP_LUT[idx];
  return v0 + (SHAPE_EXP_LUT[idx + 1] - v0) * (f - idx);
}

let shapeBufCap = 0;
let shapeSubLum = new Float32Array(0);
let shapeRowChars: string[] = [];

function ensureShapeSubBuffer(len: number, cols: number): Float32Array {
  if (len > shapeBufCap) {
    shapeBufCap = len;
    shapeSubLum = new Float32Array(len);
  }
  if (shapeRowChars.length !== cols) {
    shapeRowChars = new Array<string>(cols);
  }
  return shapeSubLum;
}

interface ShapeEyePrecompute {
  eyeBlink: number;
  happyArcBlend: number;
  closeMorph: number;
  lidYBase: number;
  happyLidBase: number;
  lowerLidBase: number;
  lowerClose: number;
  openVis: number;
  lashThick: number;
  wingAngle: number;
  wingOuterMax: number;
  wingInnerMin: number;
  lashMaxHalfW: number;
  hasLowerLash: boolean;
  browY: number;
  browTilt: number;
  irisRx: number;
  irisRy: number;
  gx: number;
  gy: number;
}

function buildEyePrecompute(
  eyeBlink: number,
  squintAmt: number,
  happyEyes: number,
  eyeState: EyeState | 'sleepy',
  isSpeaking: boolean,
  eyeY: number,
  eyeRy: number,
  rx: number,
  gx: number,
  gy: number,
  browRaise: number,
  browTilt: number
): ShapeEyePrecompute {
  const wantsHappyArc = (eyeState === 'happy' && !isSpeaking) || happyEyes > 0.40;
  const happyArcBlend = wantsHappyArc ? 1 - smoothstep(0.06, 0.48, eyeBlink) : 0;
  const effectiveClose = clamp01(Math.max(eyeBlink, squintAmt * 0.54));
  const closeMorph = smoothstep(0.42, 0.96, eyeBlink);
  const lowerClose = smoothstep(0.32, 0.98, eyeBlink);
  const openVis = (1 - smoothstep(0.84, 0.98, eyeBlink)) * (1 - happyArcBlend);
  return {
    eyeBlink,
    happyArcBlend,
    closeMorph,
    lidYBase: lerp(eyeY - eyeRy * 0.72, eyeY + 0.018, effectiveClose),
    happyLidBase: eyeY + 0.012,
    lowerLidBase: lerp(eyeY + eyeRy * 0.98, eyeY + 0.022, lowerClose),
    lowerClose,
    openVis,
    lashThick: lerp(0.020, 0.026, Math.max(effectiveClose * 0.7, closeMorph)),
    wingAngle: lerp(0.35, 0.26, closeMorph),
    wingOuterMax: rx * lerp(1.36, 1.30, closeMorph),
    wingInnerMin: rx * 0.72,
    lashMaxHalfW: rx * lerp(1.18, 1.08, closeMorph),
    hasLowerLash: happyArcBlend < 0.35 && lowerClose < 0.90,
    browY: eyeY - eyeRy * 1.38 + browRaise + eyeBlink * eyeRy * 0.14,
    browTilt,
    irisRx: rx * 0.80,
    irisRy: eyeRy * 0.88,
    gx,
    gy,
  };
}

function shadeAccHeadset(nx: number, ny: number, lum: number): number {
  if (ny < 0.18 && nx > -0.18 && nx < 0.41) {
    const bandCurve = -0.37 + nx * nx * 0.65 - nx * 0.12;
    if (ny < 0.0 && Math.abs(ny - bandCurve) < 0.044) {
      return 0.96;
    }
    if (ny > -0.09 && nx > 0.21) {
      const cupDx = (nx - 0.31) / 0.09;
      const cupDy = (ny - 0.04) / 0.125;
      const cupDistSq = cupDx * cupDx + cupDy * cupDy;
      if (cupDistSq < 1.0) {
        return cupDistSq > 0.58 && cupDistSq < 0.78 ? 0.96 : 0.10;
      }
    }
  }
  return lum;
}

function shadeAccTies(nx: number, ny: number, lum: number): number {
  if (ny > -0.22 && ny < -0.02) {
    const tLx = nx + 0.41;
    const tLy = ny + 0.12;
    const tRx = nx - 0.41;
    const tRy = ny + 0.12;
    if (tLx * tLx + tLy * tLy < 0.0062 || tRx * tRx + tRy * tRy < 0.0062) {
      return 0.97;
    }
  }
  return lum;
}

function shadeAccNekoEar(nx: number, ny: number, lum: number, earLeft: number, earRight: number): number {
  if (ny < -0.24) {
    const bandCurve = -0.37 + nx * nx * 0.55;
    if (Math.abs(ny - bandCurve) < 0.028 && Math.abs(nx) < 0.32) {
      return 0.92;
    }
    if (ny < -0.32) {
      const innerEar =
        nx < 0
          ? sdTriangle(nx, ny, -0.37, -0.35, -0.41 + earLeft * 0.2, -0.57, -0.22, -0.44)
          : sdTriangle(nx, ny, 0.38, -0.33, 0.24, -0.43, 0.42 - earRight * 0.2, -0.57);
      if (innerEar < 0) return 0.92;
    }
  }
  return lum;
}

function shadeAccFoxEar(nx: number, ny: number, lum: number): number {
  if (ny < -0.24) {
    const innerFox =
      nx < 0
        ? sdTriangle(nx, ny, -0.37, -0.27, -0.46, -0.59, -0.20, -0.39)
        : sdTriangle(nx, ny, 0.36, -0.28, 0.20, -0.40, 0.45, -0.59);
    if (innerFox < 0) return 0.92;
  } else if (ny > 0.44 && ny < 0.54 && nx > -0.05 && nx < 0.05) {
    const bellDy = ny - 0.49;
    if (nx * nx + bellDy * bellDy < 0.0020) return 0.98;
  }
  return lum;
}

function shadeAccBunnyPin(nx: number, ny: number, lum: number, earLeft: number, earRight: number): number {
  if (ny < -0.26) {
    const innerB =
      nx < 0
        ? sdCapsule(nx, ny, -0.21, -0.36, -0.26 + earLeft * 0.3, -0.63, 0.030)
        : sdCapsule(nx, ny, 0.24, -0.35, 0.38, -0.60 + earRight * 0.25, 0.028);
    if (innerB < 0) return 0.92;
  } else if (ny > -0.14 && ny < -0.10 && Math.abs(nx + 0.18) < 0.056) {
    return 0.98;
  }
  return lum;
}

function shadeAccVisorFin(nx: number, ny: number, lum: number): number {
  if (ny < 0.16 && (nx < -0.40 || nx > 0.40)) {
    const finStripe =
      nx < 0
        ? sdCapsule(nx, ny, -0.48, -0.08, -0.57, -0.52, 0.020)
        : sdCapsule(nx, ny, 0.48, -0.08, 0.57, -0.52, 0.020);
    if (finStripe < 0) return 0.96;
    if (Math.abs(nx - 0.44) < 0.040 && Math.abs(ny - 0.08) < 0.065) return 0.90;
  }
  return lum;
}

function shadeAccHoodCowl(nx: number, ny: number, lum: number): number {
  if (ny > 0.38 && ny < 0.66 && (Math.abs(nx + 0.10) < 0.013 || Math.abs(nx - 0.10) < 0.013)) {
    return 0.95;
  }
  return lum;
}

function shadeAccHalo(_nx: number, ny: number, lum: number): number {
  return ny < -0.51 ? 0.98 : lum;
}

function shadeAccChoker(nx: number, ny: number, lum: number): number {
  if (ny > 0.39 && ny < 0.45 && Math.abs(nx) < 0.105) {
    return Math.abs(nx) < 0.022 ? 0.98 : 0.06;
  }
  return lum;
}

const ACCESSORY_CODES: Record<AnimeFaceProfile['accessory'], number> = {
  headset: 0,
  ties: 1,
  nekoEar: 2,
  foxEar: 3,
  bunnyPin: 4,
  visorFin: 5,
  hoodCowl: 6,
  halo: 7,
  choker: 8,
};

function shadeAccDirect(
  accCode: number,
  nx: number,
  ny: number,
  lum: number,
  earLeft: number,
  earRight: number
): number {
  switch (accCode) {
    case 0:
      return shadeAccHeadset(nx, ny, lum);
    case 1:
      return shadeAccTies(nx, ny, lum);
    case 2:
      return shadeAccNekoEar(nx, ny, lum, earLeft, earRight);
    case 3:
      return shadeAccFoxEar(nx, ny, lum);
    case 4:
      return shadeAccBunnyPin(nx, ny, lum, earLeft, earRight);
    case 5:
      return shadeAccVisorFin(nx, ny, lum);
    case 6:
      return shadeAccHoodCowl(nx, ny, lum);
    case 7:
      return shadeAccHalo(nx, ny, lum);
    case 8:
      return shadeAccChoker(nx, ny, lum);
    default:
      return lum;
  }
}

interface ShapeFacePrecompute {
  profile: AnimeFaceProfile;
  isNeko: boolean;
  turn: number;
  turnSign: number;
  eyeY: number;
  eyeRy: number;
  leftEyeX: number;
  rightEyeX: number;
  leftEyeRx: number;
  rightEyeRx: number;
  eyeL: ShapeEyePrecompute;
  eyeR: ShapeEyePrecompute;
  browYMin: number;
  browYMax: number;
  eyeYMin: number;
  eyeYMax: number;
  noseX: number;
  noseY: number;
  mouthXBase: number;
  mouthY: number;
  mouthOpen: number;
  smileAmt: number;
  mouthMode: IdlePoseDeltas['mouthMode'];
  showBlush: boolean;
  catchlightShimmer: number;
  mwOpen: number;
  mhOpen: number;
}

function shadeShapeFaceFeatures(
  nx: number,
  ny: number,
  lum: number,
  fp: ShapeFacePrecompute
): number {
  const {
    profile,
    leftEyeX,
    rightEyeX,
    leftEyeRx,
    rightEyeRx,
    eyeY,
    eyeRy,
    eyeL,
    eyeR,
    browYMin,
    browYMax,
    eyeYMin,
    eyeYMax,
  } = fp;

  // 1. Expressive Arched Manga Eyebrows
  if (ny > browYMin && ny < browYMax) {
    if (nx > leftEyeX - leftEyeRx * 1.15 && nx < leftEyeX + leftEyeRx * 1.15) {
      const browDL = sdCapsule(
        nx,
        ny,
        leftEyeX - leftEyeRx * 0.92,
        eyeL.browY + 0.008,
        leftEyeX + leftEyeRx * 0.88,
        eyeL.browY + eyeL.browTilt,
        0.011
      );
      if (browDL < 0) lum = 0.05;
    } else if (nx > rightEyeX - rightEyeRx * 1.15 && nx < rightEyeX + rightEyeRx * 1.15) {
      const browDR = sdCapsule(
        nx,
        ny,
        rightEyeX - rightEyeRx * 0.88,
        eyeR.browY + eyeR.browTilt,
        rightEyeX + rightEyeRx * 0.92,
        eyeR.browY + 0.008,
        0.011
      );
      if (browDR < 0) lum = 0.05;
    }
  }

  // 2. Almond Anime Eyes with precomputed eyelid curves & dual iris catchlights
  if (ny > eyeYMin && ny < eyeYMax) {
    const dxL = nx - leftEyeX;
    const dxR = nx - rightEyeX;
    const absDxL = dxL < 0 ? -dxL : dxL;
    const absDxR = dxR < 0 ? -dxR : dxR;
    const isLeft = absDxL < absDxR;
    const nearestDx = isLeft ? dxL : dxR;
    const absDx = isLeft ? absDxL : absDxR;
    const ep = isLeft ? eyeL : eyeR;

    if (absDx < (isLeft ? leftEyeRx : rightEyeRx) * 1.42) {
      const nearestEyeX = isLeft ? leftEyeX : rightEyeX;
      const rx = isLeft ? leftEyeRx : rightEyeRx;
      const outerDir = isLeft ? -1 : 1;
      const dyEye = ny - eyeY;

      const normDx = nearestDx / rx;
      const normDxSq = normDx * normDx;
      const openLidCurve = normDxSq * eyeRy * lerp(0.55, 0.08, ep.closeMorph);
      const closedArchCurve = -normDxSq * eyeRy * 0.36;
      const lidCurve = lerp(openLidCurve, closedArchCurve, ep.closeMorph);
      const standardLidY = ep.lidYBase + lidCurve;
      const happyLidY = ep.happyLidBase + nearestDx * nearestDx * 3.4;
      const lidY = lerp(standardLidY, happyLidY, ep.happyArcBlend);

      const lowerLidY = ep.lowerLidBase - normDxSq * eyeRy * 0.20 * (1 - ep.lowerClose * 0.45);
      const wingDist = nearestDx * outerDir;
      const outerWing =
        ep.happyArcBlend < 0.5 &&
        wingDist > ep.wingInnerMin &&
        wingDist < ep.wingOuterMax &&
        Math.abs(ny - (lidY - (wingDist - ep.wingInnerMin) * ep.wingAngle)) < lerp(0.014, 0.019, ep.closeMorph);

      if ((absDx < ep.lashMaxHalfW && Math.abs(ny - lidY) < ep.lashThick) || outerWing) {
        lum = 0.015;
      } else if (ep.happyArcBlend < 0.72 && ny > lidY && ny < lowerLidY) {
        if (ep.openVis > 0.02) {
          const iy = dyEye / eyeRy;
          if (normDxSq + iy * iy < 1.0) {
            const irisX = (nearestDx - ep.gx * 0.35) / ep.irisRx;
            const irisY = (dyEye - ep.gy * 0.25) / ep.irisRy;
            let eyeLum: number;
            if (irisX * irisX + irisY * irisY < 1.0) {
              const g1x = nx - (nearestEyeX - rx * 0.24 + ep.gx * 0.4);
              const g1y = ny - (eyeY - eyeRy * 0.20 + ep.gy * 0.4);
              const r1 = rx * 0.22;
              const g2x = nx - (nearestEyeX + rx * 0.24 + ep.gx * 0.25);
              const g2y = ny - (eyeY + eyeRy * 0.26 + ep.gy * 0.25);
              const r2 = rx * 0.13;
              if (g1x * g1x + g1y * g1y < r1 * r1) {
                eyeLum = fp.catchlightShimmer;
              } else if (g2x * g2x + g2y * g2y < r2 * r2) {
                eyeLum = fp.catchlightShimmer * 0.90;
              } else if (dyEye > eyeRy * 0.46 && absDx < rx * 0.42) {
                eyeLum = 0.46; // Crisp lower iris crescent sheen
              } else {
                eyeLum = 0.02;
              }
            } else {
              eyeLum = 0.92;
            }
            lum = lerp(lum, eyeLum, ep.openVis);
          }
        }
      } else if (ep.hasLowerLash && absDx < rx * 0.78 && Math.abs(ny - lowerLidY) < 0.010) {
        lum = lerp(lum, 0.10, (1 - ep.lowerClose) * (1 - ep.happyArcBlend));
      }
    }
  }

  // 3. Delicate pointed anime nose shadow tick
  const ndx = nx - fp.noseX;
  const ndy = ny - fp.noseY;
  if (ndx * ndx + ndy * ndy < 0.00028) {
    lum = 0.06;
  }

  // 4. Soft manga cheek blush hatch under eyes
  if (fp.showBlush && ny > eyeY + 0.08 && ny < eyeY + 0.17) {
    const blx = (nx - (leftEyeX - 0.03)) / 0.046;
    const brx = (nx - (rightEyeX + 0.03)) / 0.046;
    const by = (ny - (eyeY + 0.12)) / 0.020;
    const bySq = by * by;
    if (blx * blx + bySq < 1.0 || brx * brx + bySq < 1.0) {
      lum *= 0.84;
    }
  }

  // 5. Subtle cyber-cat whisker ticks for Neko
  if (
    fp.isNeko &&
    ny > eyeY + 0.07 &&
    ny < eyeY + 0.17 &&
    Math.abs(nx - profile.faceCx) > 0.19 &&
    Math.abs(nx - profile.faceCx) < 0.30
  ) {
    if (Math.abs(ny - (eyeY + 0.10)) < 0.011 || Math.abs(ny - (eyeY + 0.145)) < 0.011) {
      lum = 0.14;
    }
  }

  // 6. Expressive Mouth
  if (ny > fp.mouthY - 0.075 && ny < fp.mouthY + 0.085) {
    const mx = nx - fp.mouthXBase;
    if (fp.mouthOpen > 0.10) {
      const mxn = mx / fp.mwOpen;
      const myn = (ny - fp.mouthY) / fp.mhOpen;
      if (mxn * mxn + myn * myn < 1.0) {
        lum = ny < fp.mouthY - fp.mhOpen * 0.32 && fp.mouthOpen > 0.25 && fp.mouthMode !== 'o' ? 0.90 : 0.04;
      }
    } else if (fp.mouthMode === 'cat') {
      const catY = fp.mouthY + 0.006 - Math.abs(Math.sin(mx * 42)) * 0.016;
      if (Math.abs(mx) < 0.052 && Math.abs(ny - catY) < 0.012) lum = 0.05;
    } else if (fp.mouthMode === 'grin') {
      const grinTop = fp.mouthY - 0.010;
      const grinBot = fp.mouthY + 0.028 - mx * mx * 4.5;
      if (Math.abs(mx) < 0.056 && ny > grinTop && ny < grinBot) {
        lum = ny < grinTop + 0.014 ? 0.96 : 0.06;
      }
    } else if (fp.smileAmt > 0.48) {
      const smileCurve = fp.mouthY - mx * mx * (3.6 + fp.smileAmt * 1.4);
      if (Math.abs(mx) < 0.048 && Math.abs(ny - smileCurve) < 0.014) lum = 0.06;
    } else {
      const bowY = fp.mouthY - Math.abs(mx) * 0.12;
      if (Math.abs(mx) < 0.042 && Math.abs(ny - bowY) < 0.013) {
        lum = 0.07;
      } else if (Math.abs(mx) < 0.018 && Math.abs(ny - (fp.mouthY + 0.022)) < 0.008) {
        lum = 0.98;
      }
    }
  }

  return lum;
}

/**
 * Renders an expressive 3/4 anime bust portrait variation styled after the main manga character in 2x4 Unicode Braille.
 */
export function renderBrailleShape(params: BrailleShapeRenderParams): BrailleShapeFrame {
  const {
    shape,
    cols,
    rows,
    time,
    mood,
    eyeState = 'open',
    viseme = 'closed',
    energy = 0.08,
    isSpeaking = false,
    isThinking = false,
    isDragging = false,
    gazeX = 0,
    gazeY = 0,
    motion,
    idlePose: idlePoseParam,
  } = params;

  const subW = cols * 2;
  const subH = rows * 4;
  const subLum = ensureShapeSubBuffer(subW * subH, cols);
  const rowChars = shapeRowChars;
  const lines: string[] = new Array(rows);
  const cells = new Uint8Array(cols * rows);

  const pose: IdlePoseDeltas = idlePoseParam ?? motion?.idle?.pose ?? NEUTRAL_IDLE_POSE;

  const blinkAmt = motion
    ? clamp01(motion.blink)
    : eyeState === 'blink'
    ? 1
    : 0;
  const squintAmt = motion
    ? clamp01(motion.squint)
    : eyeState === 'sleepy'
    ? 0.60
    : eyeState === 'happy' && !isSpeaking
    ? 0.72
    : 0;
  const wideAmt = motion
    ? clamp01(motion.wide)
    : eyeState === 'wide'
    ? 0.9
    : 0;
  const dragLiftAmt = motion ? motion.dragLift : isDragging ? 1 : 0;
  const hoverBoost = motion ? motion.hoverBoost : 0;
  const earTwitch = motion ? motion.earTwitch : 0;
  const haloFloat = motion ? motion.haloFloat : 0;
  const tilt = motion ? motion.dragTilt : pose.tilt;
  const hairLag = motion ? motion.hairLag : pose.hairSway;
  const bounceY = motion ? motion.bounceY : Math.sin(time * 2.2) * 0.016 + pose.headY;

  const breath = Math.sin(time * 2.2) * 0.018;
  const mouthOpen = motion
    ? clamp01(motion.mouthOpen)
    : targetMouthOpenForViseme(viseme, energy, isSpeaking);
  const speakBounce = motion
    ? -motion.bounceY * 0.24 + Math.sin(time * 11.5) * motion.mouthOpen * 0.015
    : isSpeaking
    ? Math.sin(time * 13.5) * energy * 0.024
    : 0;

  // Safe framing scale guarantees zero clipping across bust, halo, wings, and ambient props
  const safeBase = 0.89;
  const scaleX = safeBase * (1 + breath * 0.35 - speakBounce * 0.25 + dragLiftAmt * 0.030 + hoverBoost * 0.024 - pose.squashY * 0.35);
  const scaleY = safeBase * (1 - breath * 0.45 + speakBounce * 0.55 - dragLiftAmt * 0.020 + hoverBoost * 0.028 + pose.squashY * 0.55);

  const cosT = Math.cos(tilt);
  const sinT = Math.sin(tilt);

  const effGazeX = motion ? motion.gazeX : gazeX;
  const effGazeY = motion ? motion.gazeY : gazeY;
  const gx = clamp(effGazeX, -1, 1) * 0.022;
  const gy = clamp(effGazeY, -1, 1) * 0.018;
  const smileAmt = motion ? clamp01(motion.smile) : mood === 'happy' ? 1 : 0.25;

  const pivotY = 0.14;
  const profile = STATIC_FACE_PROFILES[shape];
  const turn = profile.turn;
  const turnSign = turn < 0 ? -1 : 1;
  const faceShiftX = gx * 0.26 + pose.headX * 0.5;
  const faceShiftY = gy * 0.20;

  const eyeCenterX = profile.faceCx + turn * 0.11;
  const farFactor = 0.84;
  const nearFactor = 1.06;
  const leftEyeRx = profile.eyeRx * (turn < 0 ? farFactor : nearFactor) * (1 + wideAmt * 0.10);
  const rightEyeRx = profile.eyeRx * (turn < 0 ? nearFactor : farFactor) * (1 + wideAmt * 0.10);
  const leftEyeX = eyeCenterX - profile.eyeSep * (turn < 0 ? 0.93 : 1.02) + gx * 0.65;
  const rightEyeX = eyeCenterX + profile.eyeSep * (turn < 0 ? 1.02 : 0.93) + gx * 0.65;
  const eyeY = profile.eyeY + gy * 0.42;
  const eyeRy = profile.eyeRy * (1 + wideAmt * 0.10);

  const browRaise = mood === 'alert' ? -0.022 : mood === 'thinking' ? -0.014 : 0;
  const browTiltL = mood === 'focused' ? 0.014 : mood === 'thinking' ? -0.012 : 0;
  const browTiltR = mood === 'focused' ? -0.014 : 0;
  const eyeL = buildEyePrecompute(
    clamp01(Math.max(blinkAmt, pose.blinkLeft)),
    squintAmt,
    pose.happyEyes,
    eyeState,
    isSpeaking,
    eyeY,
    eyeRy,
    leftEyeRx,
    gx,
    gy,
    browRaise,
    browTiltL
  );
  const eyeR = buildEyePrecompute(
    clamp01(Math.max(blinkAmt, pose.blinkRight)),
    squintAmt,
    pose.happyEyes,
    eyeState,
    isSpeaking,
    eyeY,
    eyeRy,
    rightEyeRx,
    gx,
    gy,
    browRaise,
    browTiltR
  );

  const mouthXBase = profile.faceCx + turn * 0.18 + gx * 0.28;
  const mouthY = profile.mouthY + gy * 0.30 + mouthOpen * 0.012;
  const fp: ShapeFacePrecompute = {
    profile,
    isNeko: profile.accessory === 'nekoEar',
    turn,
    turnSign,
    eyeY,
    eyeRy,
    leftEyeX,
    rightEyeX,
    leftEyeRx,
    rightEyeRx,
    eyeL,
    eyeR,
    browYMin: Math.min(eyeL.browY, eyeR.browY) - 0.030,
    browYMax: Math.max(eyeL.browY, eyeR.browY) + 0.032,
    eyeYMin: eyeY - eyeRy * 1.38,
    eyeYMax: eyeY + eyeRy * 1.48,
    noseX: profile.faceCx + turn * 0.24 + gx * 0.36,
    noseY: profile.eyeY + 0.095 + gy * 0.28,
    mouthXBase,
    mouthY,
    mouthOpen,
    smileAmt,
    mouthMode: pose.mouthMode,
    showBlush: mood === 'happy' || isSpeaking || smileAmt > 0.65 || pose.happyEyes > 0.4,
    catchlightShimmer: isSpeaking ? 0.94 + 0.06 * Math.sin(time * 4.2) : 1.0,
    mwOpen: pose.mouthMode === 'o' ? 0.026 : 0.035 + mouthOpen * 0.032,
    mhOpen: pose.mouthMode === 'o' ? 0.032 : 0.016 + mouthOpen * 0.038,
  };

  const faceMinX = profile.faceCx - profile.faceRx - 0.06;
  const faceMaxX = profile.faceCx + profile.faceRx + 0.06;
  const bangMinY = profile.bangBottomBase - 0.085;
  const faceMaxY = profile.chinY + 0.07;

  const hasDynamicOverlay =
    pose.wingOpen > 0.04 ||
    Math.abs(pose.tailWag) > 0.03 ||
    pose.hand !== 'none' ||
    pose.prop !== 'none';

  const idleBreathSwayX = motion ? Math.sin(time * 1.05) * 0.015 : 0;
  const invSubW = 1 / subW;
  const invSubH = 1 / subH;
  const invScaleX = 1 / scaleX;
  const invScaleY = 1 / scaleY;
  const headShiftX = pose.headX * 0.5 + idleBreathSwayX;
  const minBoundX = hasDynamicOverlay ? -0.94 : shape === 'star' ? -0.88 : -0.78;
  const maxBoundX = hasDynamicOverlay ? 0.94 : shape === 'star' ? 0.88 : 0.78;
  const minBoundY = hasDynamicOverlay ? -0.88 : -0.84;

  const thinkHaloAmt = motion?.thinkHalo !== undefined ? motion.thinkHalo : isThinking ? 1 : 0;
  const hasThinkHalo = thinkHaloAmt > 0.02;
  const orbitBase = time * 2.1;
  const ox0 = Math.cos(orbitBase) * 0.52;
  const oy0 = -0.58 + Math.sin(orbitBase) * 0.10;
  const og0 = (0.78 + 0.20 * Math.sin(time * 4.5)) * thinkHaloAmt;
  const ox1 = Math.cos(orbitBase + 2.0943951) * 0.52;
  const oy1 = -0.58 + Math.sin(orbitBase + 2.0943951) * 0.10;
  const og1 = (0.78 + 0.20 * Math.sin(time * 4.5 + 1)) * thinkHaloAmt;
  const ox2 = Math.cos(orbitBase + 4.1887902) * 0.52;
  const oy2 = -0.58 + Math.sin(orbitBase + 4.1887902) * 0.10;
  const og2 = (0.78 + 0.20 * Math.sin(time * 4.5 + 2)) * thinkHaloAmt;

  const speakHaloAmt = motion?.speakHalo !== undefined ? motion.speakHalo : isSpeaking && energy > 0.14 ? 1 : 0;
  const hasSpeakRipple = speakHaloAmt > 0.02;
  const arcPhase = (time * 2.2) % 1;
  const phaseB = (arcPhase + 0.5) % 1;
  const rInner = 0.50 + arcPhase * 0.14;
  const rOuter = 0.56 + phaseB * 0.16;
  const arcFadeA = 1 - arcPhase;
  const arcFadeB = 1 - phaseB;
  const speakRippleGain = clamp01(0.45 + energy * 0.55) * speakHaloAmt;

  const xSlack =
    Math.abs(sinT) * 0.90 +
    Math.abs(headShiftX) +
    Math.abs(hairLag) * 0.45 +
    Math.abs(earTwitch) * 0.16 +
    0.06;
  const rawMinX = (minBoundX - xSlack) * scaleX;
  const rawMaxX = (maxBoundX + xSlack) * scaleX;
  const sxStart = Math.max(0, Math.floor(((rawMinX + 1) * 0.5) * subW - 0.5));
  const sxEnd = Math.min(subW, Math.ceil(((rawMaxX + 1) * 0.5) * subW + 0.5));
  const dxStep = 2 * invSubW * invScaleX;
  const dxStepCos = dxStep * cosT;
  const dxStepSin = dxStep * sinT;
  const nxBaseStart = (((sxStart + 0.5) * invSubW) * 2 - 1) * invScaleX - headShiftX;
  const hasHairLag = hairLag !== 0;
  const hasHaloFloat = haloFloat !== 0 && shape === 'trapezoid';
  const hasEarTwitch = earTwitch !== 0;
  const collarBreathBoost = breath * 1.4;

  let activeMinX = subW;
  let activeMaxX = -1;
  let activeMinY = subH;
  let activeMaxY = -1;

  const overlayFade = motion ? clamp01(motion.idle.envelope * 1.15) : 1;
  const rotYMargin = Math.abs(sinT) * 0.95 + Math.abs(earTwitch) * 0.10 + Math.abs(haloFloat) + 0.04;
  const shapeCode = SHAPE_CODES[shape];
  const accCode = ACCESSORY_CODES[profile.accessory];
  const accHasLower = accCode === 3 || accCode === 6 || accCode === 8;
  const partCode =
    profile.partType === 'center'
      ? 0
      : profile.partType === 'side'
      ? 1
      : profile.partType === 'wispy'
      ? 2
      : profile.partType === 'blunt'
      ? 3
      : 4;
  const earLOff = pose.earLeft;
  const earROff = pose.earRight;
  const ahogeY = pose.ahogeBounce;
  const hoodL = pose.hoodLift;
  const hoodSx = pose.hoodScaleX;
  const haloSpin = pose.haloSpin;
  const profFaceCx = profile.faceCx;
  const profChinY = profile.chinY;
  const profEyeY = profile.eyeY;
  const profBangBase = profile.bangBottomBase;
  const profShineY = profile.shineY;
  const headCrownShift = pose.headX * 1.4;
  const hairWaveDrift = 0;
  const hasHandPose = pose.hand !== 'none';

  for (let sy = 0; sy < subH; sy++) {
    const nyRaw = ((sy + 0.5) * invSubH) * 2 - 1;
    const nyScaled = (nyRaw - bounceY) * invScaleY;
    const rowOff = sy * subW;

    if (nyScaled < minBoundY - rotYMargin || nyScaled > 0.80 + rotYMargin) {
      subLum.fill(0, rowOff, rowOff + subW);
      continue;
    }

    if (sxStart > 0) subLum.fill(0, rowOff, rowOff + sxStart);
    if (sxEnd < subW) subLum.fill(0, rowOff + sxEnd, rowOff + subW);

    const dyp = nyScaled - pivotY;
    let curBaseX = nxBaseStart * cosT - dyp * sinT;
    let curBaseY = nxBaseStart * sinT + dyp * cosT + pivotY;
    let rowHasContent = false;
    const rowThinkCandidate = hasThinkHalo && nyScaled > -0.78 - rotYMargin && nyScaled < -0.38 + rotYMargin;
    const rowSpeakCandidate = hasSpeakRipple && nyScaled > -0.18 - rotYMargin && nyScaled < 0.34 + rotYMargin;

    for (let sx = sxStart; sx < sxEnd; sx++) {
      let nx = curBaseX;
      let ny = curBaseY;
      curBaseX += dxStepCos;
      curBaseY += dxStepSin;

      // Keep Canvas Braille character silhouette rigid around pivot (no horizontal S-bend shear)
      void hasHairLag;

      if (hasHaloFloat && ny < -0.48) ny -= haloFloat;
      if (hasEarTwitch && ny < -0.24) {
        const earZone = smoothstep(-0.24, -0.58, ny);
        nx += (nx < 0 ? -1 : 1) * earTwitch * earZone * 0.13;
        ny += earTwitch * earZone * 0.08;
      }

      if (nx > maxBoundX || nx < minBoundX || ny < minBoundY || ny > 0.80) {
        subLum[rowOff + sx] = 0;
        continue;
      }

      const hasDyn =
        hasDynamicOverlay &&
        evaluateDynamicFeatureInto(nx, ny, shape, pose, profChinY, mouthXBase, overlayFade);

      let statusVfxLum = 0;
      if (rowThinkCandidate && ny < -0.38 && ny > -0.78) {
        const d0x = nx - ox0;
        const d0y = ny - oy0;
        const sq0 = d0x * d0x + d0y * d0y;
        if (sq0 < 0.003136) {
          statusVfxLum = Math.max(statusVfxLum, clamp01(1 - (Math.sqrt(sq0) - 0.032) / 0.024) * og0);
        }
        const d1x = nx - ox1;
        const d1y = ny - oy1;
        const sq1 = d1x * d1x + d1y * d1y;
        if (sq1 < 0.003136) {
          statusVfxLum = Math.max(statusVfxLum, clamp01(1 - (Math.sqrt(sq1) - 0.032) / 0.024) * og1);
        }
        const d2x = nx - ox2;
        const d2y = ny - oy2;
        const sq2 = d2x * d2x + d2y * d2y;
        if (sq2 < 0.003136) {
          statusVfxLum = Math.max(statusVfxLum, clamp01(1 - (Math.sqrt(sq2) - 0.032) / 0.024) * og2);
        }
      }
      if (rowSpeakCandidate && ny > -0.18 && ny < 0.34 && (nx > 0.48 || nx < -0.48)) {
        const absX = nx < 0 ? -nx : nx;
        if (absX > 0.48 && absX < 0.84) {
          const rx = nx * 0.92;
          const ry = (ny - 0.06) * 1.15;
          const radSq = rx * rx + ry * ry;
          if (radSq > 0.22 && radSq < 0.56) {
            const rad = Math.sqrt(radSq);
            const bandW = 0.016;
            const da = Math.abs(rad - rInner);
            const db = Math.abs(rad - rOuter);
            const ringA = da < bandW ? (1 - da / bandW) * arcFadeA : 0;
            const ringB = db < bandW ? (1 - db / bandW) * arcFadeB : 0;
            if (ringA + ringB > 0) {
              const vFade = 1 - smoothstep(0.14, 0.28, Math.abs(ny - 0.06));
              statusVfxLum = Math.max(statusVfxLum, (ringA + ringB) * vFade * speakRippleGain);
            }
          }
        }
      }

      const d = evalShapeSdfDirect(shapeCode, nx, ny, earLOff, earROff, ahogeY, hoodL, hoodSx, haloSpin);
      if (d > 0.030) {
        const baseOut = hasDyn ? dynLumOut * dynCovOut : 0;
        const vOut = baseOut > statusVfxLum ? baseOut : statusVfxLum;
        subLum[rowOff + sx] = vOut;
        if (vOut > 0.01) {
          rowHasContent = true;
          if (sx < activeMinX) activeMinX = sx;
          if (sx > activeMaxX) activeMaxX = sx;
        }
        continue;
      }

      const edgeCov = d < -0.038 ? 1 : clamp(1 - (d + 0.004) / 0.034, 0, 1);
      const bottomFade = ny > 0.72 ? clamp01(1 - (ny - 0.72) / 0.12) : 1;
      const alphaCov = edgeCov * bottomFade;
      if (alphaCov <= 0.01) {
        const baseOut = hasDyn ? dynLumOut * dynCovOut : 0;
        const vOut = baseOut > statusVfxLum ? baseOut : statusVfxLum;
        subLum[rowOff + sx] = vOut;
        if (vOut > 0.01) {
          rowHasContent = true;
          if (sx < activeMinX) activeMinX = sx;
          if (sx > activeMaxX) activeMaxX = sx;
        }
        continue;
      }

      rowHasContent = true;
      if (sx < activeMinX) activeMinX = sx;
      if (sx > activeMaxX) activeMaxX = sx;

      let dFace = 1;
      let bangBottom = profBangBase;
      let inFace = false;
      if (nx > faceMinX && nx < faceMaxX && ny > bangMinY && ny < faceMaxY) {
        dFace = evaluateAnimeFaceDist(profile, nx - faceShiftX, ny - faceShiftY);
        if (dFace < 0.05) {
          const dxFace = nx - profFaceCx;
          if (partCode === 0) {
            bangBottom += -0.052 / (1 + dxFace * dxFace * 42) + 0.022 * Math.cos(nx * 18);
          } else if (partCode === 1) {
            bangBottom += -0.072 * clamp(dxFace * turnSign + 0.12, -0.35, 0.35) + 0.018 * Math.sin(nx * 16);
          } else if (partCode === 2) {
            bangBottom += 0.028 * Math.cos(dxFace * 20) - 0.012;
          } else if (partCode === 3) {
            bangBottom += 0.012 * Math.sin(nx * 12) + (Math.abs(dxFace) < 0.10 ? 0.014 : 0);
          } else {
            bangBottom += 0.024 * Math.sin(dxFace * 20) + (Math.abs(dxFace) < 0.07 ? -0.026 : 0);
          }
          inFace = dFace < 0 && ny > bangBottom;
        }
      }

      const inNeckCollar = !inFace && ny > profChinY - 0.05 && ny < 0.80 && Math.abs(nx) < 0.38;
      let lum: number;

      if (inFace) {
        const templeDist = (nx - profFaceCx) * -turnSign;
        const templeShadow = smoothstep(0.08, 0.27, templeDist) * 0.24;
        const cgx = nx - profFaceCx - turn * 0.12;
        const cgy = ny - (profEyeY + 0.11);
        const cDistSq18 = (cgx * cgx + cgy * cgy) * 18;
        const cheekGlow = cDistSq18 < 8 ? fastShapeExpNeg(cDistSq18) * 0.08 : 0;
        lum = clamp(0.71 - templeShadow + cheekGlow, 0.44, 0.78);

        const underBang = ny - bangBottom;
        if (underBang >= 0 && underBang < 0.048) {
          lum *= 0.52 + (underBang / 0.048) * 0.48;
        }
        if (dFace > -0.022 && ny > profEyeY + 0.08) {
          lum = 0.06;
        }
      } else if (inNeckCollar && Math.abs(nx) < 0.105 && ny < 0.56) {
        const chinShadow = ny < profChinY + 0.075 && nx * -turnSign < 0.06;
        lum = chinShadow ? 0.12 : 0.78;
      } else if (inNeckCollar && ny >= 0.52) {
        const absNx = nx < 0 ? -nx : nx;
        const vNeck = 0.52 + Math.max(0, 0.12 - absNx) * 0.95;
        const collarOuter = 0.54 + absNx * 0.30;
        if (absNx < 0.16 && Math.abs(ny - vNeck) < 0.024) {
          lum = 0.94 + collarBreathBoost;
        } else if (absNx < 0.13 && ny < vNeck && ny >= 0.50) {
          lum = 0.76;
        } else if (ny >= vNeck && ny < collarOuter + 0.03) {
          lum = 0.86;
        } else {
          lum = 0.24;
        }
      } else {
        const isOuterRim = d > -0.068;
        const rimBoost = isOuterRim ? clamp01(1 + d / 0.068) * 0.48 : 0;
        const waveRaw = nx * 5.729578 - ny * 1.2732395 + hairWaveDrift;
        const tw = waveRaw - Math.floor(waveRaw);
        const strandVal = tw < 0.5 ? tw * 4 - 1 : 3 - tw * 4;
        const strandShade = strandVal > 0 ? 0.052 * strandVal : 0;
        lum = 0.18 + rimBoost + strandShade;

        if (ny > profShineY - 0.08 && ny < profShineY + 0.12 && nx > -0.44 && nx < 0.44) {
          const shineCurve = profShineY + nx * nx * 0.28 - turn * nx * 0.08;
          if (Math.abs(ny - shineCurve) < 0.072) {
            const j1 = nx * 5.41 + 0.08 + headCrownShift;
            const t1 = j1 - Math.floor(j1);
            const w1 = t1 < 0.5 ? t1 * 4 - 1 : 3 - t1 * 4;
            const j2 = nx * 9.23;
            const t2 = j2 - Math.floor(j2);
            const w2 = t2 < 0.5 ? 1 - t2 * 4 : t2 * 4 - 3;
            if (Math.abs(ny - (shineCurve + 0.020 * w1 + 0.012 * w2)) < 0.040) {
              const j3 = nx * 8.27 + ny * 0.796;
              if (j3 - Math.floor(j3) < 0.64) lum = Math.max(lum, 0.95);
            }
          }
        }

        if (ny > 0.34 && ny < 0.56 && (nx > 0.32 || nx < -0.32)) {
          const ct = ny * 4.14 + nx * 1.91;
          const cw = ct - Math.floor(ct);
          const tri = cw < 0.5 ? cw * 4 - 1 : 3 - cw * 4;
          const tipSheen = 0.35 + 0.16 * tri;
          if (tipSheen > lum) lum = tipSheen;
        }
      }

      if (!inFace && (ny < 0.20 || accHasLower)) {
        lum = shadeAccDirect(accCode, nx, ny, lum, earLOff, earROff);
      }

      if (dFace < 0.05) {
        lum = shadeShapeFaceFeatures(nx, ny, lum, fp);
      }

      let finalLum = lum * alphaCov;
      if (hasDyn && hasHandPose) {
        finalLum = lerp(finalLum, dynLumOut, dynCovOut);
      }
      subLum[rowOff + sx] = finalLum;
    }
    if (rowHasContent) {
      if (sy < activeMinY) activeMinY = sy;
      if (sy > activeMaxY) activeMaxY = sy;
    }
  }

  const subLumOut = new Uint8Array(subW * rows * 4);
  packBrailleSubpixelsToFrame(
    subLum,
    subW,
    cols,
    rows,
    activeMinX,
    activeMaxX,
    activeMinY,
    activeMaxY,
    rowChars,
    lines,
    cells,
    subLumOut
  );
  return { cols, rows, lines, cells, subLum: subLumOut };
}

function packBrailleSubpixelsToFrame(
  subLum: Float32Array,
  subW: number,
  cols: number,
  rows: number,
  activeMinX: number,
  activeMaxX: number,
  activeMinY: number,
  activeMaxY: number,
  rowChars: string[],
  lines: string[],
  cells: Uint8Array,
  subLumOut: Uint8Array
): void {
  const hasContent = activeMaxY >= activeMinY && activeMaxX >= activeMinX;
  const minActiveRow = hasContent ? activeMinY >> 2 : rows;
  const maxActiveRow = hasContent ? activeMaxY >> 2 : -1;
  const cLo = hasContent ? Math.max(0, activeMinX >> 1) : cols;
  const cHi = hasContent ? Math.min(cols - 1, activeMaxX >> 1) : -1;
  const emptyRowStr = ' '.repeat(cols);

  for (let r = 0; r < rows; r++) {
    const cellRowOff = r * cols;
    if (r < minActiveRow || r > maxActiveRow || cHi < cLo) {
      cells.fill(0, cellRowOff, cellRowOff + cols);
      lines[r] = emptyRowStr;
      continue;
    }

    if (cLo > 0) {
      cells.fill(0, cellRowOff, cellRowOff + cLo);
      rowChars.fill(' ', 0, cLo);
    }
    if (cHi + 1 < cols) {
      cells.fill(0, cellRowOff + cHi + 1, cellRowOff + cols);
      rowChars.fill(' ', cHi + 1, cols);
    }

    const syBase = r * 4;
    const r0 = syBase * subW;
    const r1 = r0 + subW;
    const r2 = r1 + subW;
    const r3 = r2 + subW;
    const bRow0 = (syBase & 7) << 3;
    const bRow1 = ((syBase + 1) & 7) << 3;
    const bRow2 = ((syBase + 2) & 7) << 3;
    const bRow3 = ((syBase + 3) & 7) << 3;

    for (let c = cLo; c <= cHi; c++) {
      const c2 = c * 2;
      const l00 = subLum[r0 + c2];
      const l01 = subLum[r0 + c2 + 1];
      const l10 = subLum[r1 + c2];
      const l11 = subLum[r1 + c2 + 1];
      const l20 = subLum[r2 + c2];
      const l21 = subLum[r2 + c2 + 1];
      const l30 = subLum[r3 + c2];
      const l31 = subLum[r3 + c2 + 1];

      const lumSum = l00 + l01 + l10 + l11 + l20 + l21 + l30 + l31;
      if (lumSum <= 0.12) {
        cells[cellRowOff + c] = 0;
        rowChars[c] = ' ';
        continue;
      }

      const bx0 = c2 & 7;
      const bx1 = (c2 + 1) & 7;
      let bits = 0;
      if (l00 > BAYER_THRESH[bRow0 + bx0]) {
        bits |= 0x01;
        subLumOut[r0 + c2] = (l00 * 255 + 0.5) | 0;
      }
      if (l01 > BAYER_THRESH[bRow0 + bx1]) {
        bits |= 0x08;
        subLumOut[r0 + c2 + 1] = (l01 * 255 + 0.5) | 0;
      }
      if (l10 > BAYER_THRESH[bRow1 + bx0]) {
        bits |= 0x02;
        subLumOut[r1 + c2] = (l10 * 255 + 0.5) | 0;
      }
      if (l11 > BAYER_THRESH[bRow1 + bx1]) {
        bits |= 0x10;
        subLumOut[r1 + c2 + 1] = (l11 * 255 + 0.5) | 0;
      }
      if (l20 > BAYER_THRESH[bRow2 + bx0]) {
        bits |= 0x04;
        subLumOut[r2 + c2] = (l20 * 255 + 0.5) | 0;
      }
      if (l21 > BAYER_THRESH[bRow2 + bx1]) {
        bits |= 0x20;
        subLumOut[r2 + c2 + 1] = (l21 * 255 + 0.5) | 0;
      }
      if (l30 > BAYER_THRESH[bRow3 + bx0]) {
        bits |= 0x40;
        subLumOut[r3 + c2] = (l30 * 255 + 0.5) | 0;
      }
      if (l31 > BAYER_THRESH[bRow3 + bx1]) {
        bits |= 0x80;
        subLumOut[r3 + c2 + 1] = (l31 * 255 + 0.5) | 0;
      }

      cells[cellRowOff + c] = (lumSum * 31.875 + 0.5) | 0;
      rowChars[c] = BRAILLE_CHAR_TABLE[bits];
    }
    lines[r] = rowChars.join('');
  }
}
