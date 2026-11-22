import type { ExpressionMood, EyeState, MouthViseme } from '../types/hermes';
import { BLUE_NOISE_16X16_THRESH } from './brailleCanvasRaster';

export type DitherStyle = 'braille';

export interface PortraitSourceField {
  width: number;
  height: number;
  /** Precomputed base chiaroscuro luminance (0..1) from reference silhouette */
  baseLum: Float32Array;
  /** Ink opacity mask (0..1) so animations know where skin vs hair/lines sit */
  inkMask: Float32Array;
}

/**
 * Continuous interpolated animation state for 60fps fluid character motion
 * (2.5D head yaw/pitch turning, smooth eye blinks, eyebrow micro-expressions,
 * damped lip-sync aperture, eye gaze saccades, and overlapping hair/shoulder follow-through).
 */
export interface PortraitMotionState {
  blink: number;       // 0 (open) .. 1 (closed)
  squint: number;      // 0 .. 1 happy/focused cheek squint
  wide: number;        // 0 .. 1 alert eye expansion
  browLift: number;    // -0.45 (focused) .. +1.0 (raised emphasis)
  mouthOpen: number;   // 0 .. 1 vertical aperture
  mouthWidth: number;  // 0.60 .. 1.38 horizontal stretch
  mouthRound: number;  // 0 .. 1 roundness for O/oo vowels
  smile: number;       // 0 .. 1 upturned smile corners
  jawDrop: number;     // 0 .. 1 downward chin/jaw articulation
  teethTop: number;    // 0 .. 1 upper teeth bar prominence
  teethBottom: number; // 0 .. 1 lower teeth / smile bar (Wide / Ee)
  tongueShow: number;  // 0 .. 1 tongue visibility inside open cavity
  tongueLift: number;  // 0 .. 1 raised tongue tip behind upper teeth (L / Th)
  lipPress: number;    // 0 .. 1 bilabial pressed seal (M / B / P)
  lipBite: number;     // 0 .. 1 labiodental upper-teeth-on-lower-lip (F / V)
  puckerRing: number;  // 0 .. 1 pursed O/Oo lip highlight ring
  restPart: number;    // 0 .. 1 relaxed natural/rest slight parting
  gazeX: number;       // -1 .. 1 eye saccade X
  gazeY: number;       // -1 .. 1 eye saccade Y
  headYaw: number;     // -0.35 .. +0.35 radians 2.5D left/right head turn
  headPitch: number;   // -0.22 .. +0.22 radians 2.5D up/down head pitch
  headTilt: number;    // radians planar head tilt
  headNodY: number;    // normalized UV vertical head/breath shift
  headSwayX: number;   // normalized UV horizontal sway
  hairLagX: number;    // normalized UV secondary follow-through on lower curls
  hairLagY: number;    // normalized UV secondary bounce on lower curls
  shoulderY: number;   // normalized UV decoupled collar/shoulder breathing
  listenPulse: number; // 0 .. 1 active listening/speaking aura
  speakEnvelope?: number; // smoothed 0..1 speaking cadence envelope
  smoothEnergy?: number;  // smoothed 0..1 audio energy envelope
}

export interface PortraitParams {
  cols: number;
  rows: number;
  time: number; // seconds
  mood: ExpressionMood;
  viseme: MouthViseme;
  eyeState: EyeState;
  energy: number; // 0..1 audio level
  isSpeaking: boolean;
  isListening: boolean;
  style?: DitherStyle;
  sourceField?: PortraitSourceField | null;
  spriteBank?: PortraitSourceField[] | null;
  motion?: PortraitMotionState | null;
}

export interface PortraitFrame {
  cols: number;
  rows: number;
  lines: string[];
  /** Per-row intensity values (0..255) matching each character */
  cells: Uint8Array;
  /** Per-subpixel (2*cols x 4*rows) luminance 0..255 for exact per-dot tonal shading */
  subLum?: Uint8Array;
}

// Deterministic 16x16 toroidal void-and-cluster blue-noise halftone thresholds in [0.055, 0.940].
// 4x larger tiling period than 8x8 with 256 tonal steps for silky skin gradients and zero dot-boiling.
const BLUE_THRESH_16X16 = BLUE_NOISE_16X16_THRESH;

// Standard 2x4 Unicode Braille lookup table (U+2800..U+28FF)
const BRAILLE_CHAR_TABLE: string[] = Array.from({ length: 256 }, (_, i) =>
  i === 0 ? ' ' : String.fromCharCode(0x2800 + i)
);

const EXP_LUT_MAX = 8;
const EXP_LUT_SIZE = 1024;
const EXP_LUT_SCALE = EXP_LUT_SIZE / EXP_LUT_MAX;
const EXP_NEG_LUT = new Float32Array(EXP_LUT_SIZE + 1);
for (let i = 0; i <= EXP_LUT_SIZE; i++) {
  EXP_NEG_LUT[i] = Math.exp(-(i * EXP_LUT_MAX) / EXP_LUT_SIZE);
}

function fastExpNeg(x: number): number {
  if (x <= 0) return 1;
  if (x >= EXP_LUT_MAX) return 0;
  const f = x * EXP_LUT_SCALE;
  const idx = f | 0;
  const v0 = EXP_NEG_LUT[idx];
  return v0 + (EXP_NEG_LUT[idx + 1] - v0) * (f - idx);
}

function clamp(v: number, min: number, max: number): number {
  return v < min ? min : v > max ? max : v;
}

function clamp01(v: number): number {
  return v < 0 ? 0 : v > 1 ? 1 : v;
}

function lerp(a: number, b: number, t: number): number {
  return a + (b - a) * t;
}

function expApproach(current: number, target: number, rate: number, dt: number): number {
  const factor = 1 - Math.exp(-rate * Math.max(0.001, dt));
  return current + (target - current) * factor;
}

function smoothstep(edge0: number, edge1: number, x: number): number {
  const t = clamp01((x - edge0) / (edge1 - edge0 || 1));
  return t * t * (3 - 2 * t);
}

function sdEllipse(x: number, y: number, cx: number, cy: number, rx: number, ry: number): number {
  const dx = (x - cx) / rx;
  const dy = (y - cy) / ry;
  return (Math.sqrt(dx * dx + dy * dy) - 1) * (rx < ry ? rx : ry);
}

function sdCapsule2(
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
  const h = clamp01((pax * bax + pay * bay) / denom);
  const dx = pax - bax * h;
  const dy = pay - bay * h;
  return Math.sqrt(dx * dx + dy * dy) - r;
}

function getMouthShape(viseme: MouthViseme, energy: number): { rx: number; ry: number; open: number } {
  const e = energy * 0.025;
  switch (viseme) {
    case 'closed':
      return { rx: 0.044, ry: 0.008, open: 0 };
    case 'rest':
      return { rx: 0.045, ry: 0.011, open: 0.14 };
    case 'mbp':
      return { rx: 0.050, ry: 0.007, open: 0 };
    case 'fv':
      return { rx: 0.048, ry: 0.015 + e * 0.3, open: 0.34 };
    case 'lth':
      return { rx: 0.052, ry: 0.028 + e * 0.6, open: 0.68 };
    case 'smile':
      return { rx: 0.056, ry: 0.011, open: 0.15 };
    case 'small':
      return { rx: 0.046, ry: 0.021 + e * 0.65, open: 0.52 };
    case 'mid':
      return { rx: 0.052, ry: 0.031 + e * 0.85, open: 0.82 };
    case 'round':
      return { rx: 0.034, ry: 0.035 + e * 0.9, open: 0.92 };
    case 'ee':
      return { rx: 0.068, ry: 0.028 + e * 0.65, open: 0.78 };
    case 'wide':
      return { rx: 0.060, ry: 0.044 + e, open: 1.0 };
  }
}

export interface LookTarget {
  /** Normalized pointer horizontal offset from portrait center (-1 left .. +1 right) */
  x: number;
  /** Normalized pointer vertical offset from portrait center (-1 up .. +1 down) */
  y: number;
}

/**
 * Evaluates a biologically-grounded asymmetric blink envelope in [0, 1]:
 * - Quasi-periodic intervals (~2.8s–4.4s) with organic jitter and occasional double-blinks
 * - Fast snappy lid closure (~26% of blink), brief full-closed dwell (~20%), graceful ease-out opening (~54%)
 */
export function evaluateNaturalBlinkCurve(
  time: number,
  phaseOffset = 0,
  doubleBlinkModulo = 4
): number {
  const t = time + phaseOffset;
  const blinkWarp = t * 0.28 + 0.15 * Math.sin(t * 0.41 + phaseOffset * 0.6) + 0.06 * Math.sin(t * 0.19);
  const cycleIndex = Math.floor(blinkWarp);
  const cyclePos = blinkWarp - cycleIndex;

  const shapeBlink = (u: number): number => {
    if (u <= 0 || u >= 1) return 0;
    // 0.00..0.26: snappy lid drop
    if (u < 0.26) {
      const k = u / 0.26;
      return 1 - (1 - k) * (1 - k);
    }
    // 0.26..0.46: brief full-closed dwell so closed lashes register crisply
    if (u < 0.46) {
      return 1;
    }
    // 0.46..1.00: graceful slower lid lift
    const k = (u - 0.46) / 0.54;
    const eased = k * k * (3 - 2 * k);
    return 1 - eased;
  };

  // Primary blink (~215ms window in cycle)
  if (cyclePos > 0.935) {
    return shapeBlink((cyclePos - 0.935) / 0.065);
  }
  // Occasional crisp double-blink slightly before primary blink
  const hasDouble = ((cycleIndex % doubleBlinkModulo) + doubleBlinkModulo) % doubleBlinkModulo === 1;
  if (hasDouble && cyclePos > 0.835 && cyclePos < 0.892) {
    return shapeBlink((cyclePos - 0.835) / 0.057) * 0.94;
  }
  return 0;
}

function getTargetMotionFromParams(
  params: PortraitParams,
  lookTarget?: LookTarget | null,
  speakEnvelopeOverride?: number,
  smoothEnergyOverride?: number
): PortraitMotionState {
  const { time, mood, viseme, eyeState, energy, isSpeaking, isListening } = params;
  const effEnergy = smoothEnergyOverride ?? energy;
  const speakEnv = speakEnvelopeOverride ?? (isSpeaking ? 1 : 0);

  // Natural rhythmic breathing & overlapping secondary action (organic figure-8 sway)
  const breath = Math.sin(time * 2.35) + Math.sin(time * 4.7 + 0.35) * 0.16;
  const breathLag = Math.sin(time * 2.35 - 0.82) + Math.sin(time * 4.7 - 0.48) * 0.16;
  const speakCadence = Math.sin(time * 7.6) * speakEnv;
  const speakBounce = speakEnv * Math.abs(Math.sin(time * 7.6)) * (0.010 + effEnergy * 0.015);
  const speakNod = speakEnv * Math.sin(time * 4.4) * (0.022 + effEnergy * 0.036);

  // Expressive autonomous 2.5D head yaw & pitch choreography per mood
  let headYaw =
    Math.sin(time * 1.18) * 0.225 +
    Math.sin(time * 0.54 + 0.65) * 0.088;
  let headPitch =
    Math.sin(time * 1.48) * 0.102 +
    Math.sin(time * 0.68 + 0.4) * 0.034;

  if (mood === 'thinking') {
    headYaw = -0.19 + Math.sin(time * 1.05) * 0.092 + Math.sin(time * 0.46) * 0.032;
    headPitch = -0.095 + Math.cos(time * 0.92) * 0.044;
  } else if (mood === 'listening') {
    headYaw = 0.13 + Math.sin(time * 1.38) * 0.095;
    headPitch = 0.032 + Math.sin(time * 2.35) * (0.044 + effEnergy * 0.048);
  } else if (mood === 'alert') {
    headYaw = 0.16 + Math.sin(time * 2.8) * 0.048;
    headPitch = -0.045 + Math.sin(time * 2.0) * 0.024;
  } else if (mood === 'focused') {
    headYaw = 0.05 + Math.sin(time * 0.88) * 0.088;
    headPitch = 0.03 + Math.sin(time * 1.38) * 0.040;
  } else if (mood === 'happy') {
    headYaw = 0.06 + Math.sin(time * 1.28) * 0.205 + Math.sin(time * 0.56 + 1.05) * 0.078;
    headPitch = -0.016 + Math.sin(time * 2.15) * 0.086 + Math.cos(time * 0.85) * 0.034;
  }

  if (speakEnv > 0.005) {
    // Conversational emphasis head turns toward viewer & syllable nods
    headYaw += speakEnv * (0.075 + Math.sin(time * 1.55) * (0.09 + effEnergy * 0.062));
    headPitch += speakEnv * Math.sin(time * 3.6) * (0.036 + effEnergy * 0.046);
  }

  // Blend subtle pointer-tracking head turn when user hovers/moves cursor near portrait
  if (lookTarget) {
    const lx = clamp(lookTarget.x, -1, 1);
    const ly = clamp(lookTarget.y, -1, 1);
    headYaw = lerp(headYaw, lx * 0.30, 0.66);
    headPitch = lerp(headPitch, ly * 0.18, 0.62);
  }

  headYaw = clamp(headYaw, -0.34, 0.34);
  headPitch = clamp(headPitch, -0.21, 0.21);

  const baseTilt =
    mood === 'thinking'
      ? -0.068 + Math.sin(time * 1.55) * 0.036
      : mood === 'happy'
      ? 0.028 + Math.sin(time * 2.05) * 0.048 + Math.sin(time * 0.78 + 0.4) * 0.020
      : mood === 'listening'
      ? 0.042 + Math.sin(time * 2.45) * 0.034
      : mood === 'alert'
      ? -0.024 + Math.sin(time * 2.5) * 0.018
      : mood === 'focused'
      ? -0.014 + Math.sin(time * 1.48) * 0.026
      : Math.sin(time * 1.65) * 0.048 + Math.sin(time * 0.72 + 0.5) * 0.020;
  const headTilt = baseTilt + speakNod - headYaw * 0.14;

  const autoBlink = evaluateNaturalBlinkCurve(time, 0, 4);

  const blink =
    eyeState === 'blink' ? 1 : eyeState === 'half' ? Math.max(0.46, autoBlink) : autoBlink;
  const squint =
    eyeState === 'happy' || mood === 'happy'
      ? 0.34 + Math.max(0, speakCadence) * effEnergy * 0.12
      : eyeState === 'half' || mood === 'focused'
      ? 0.56
      : viseme === 'ee'
      ? 0.22
      : speakEnv * effEnergy * 0.14;
  const wide = eyeState === 'wide' || mood === 'alert' ? 0.88 : 0;

  // Dynamic eyebrow micro-expressions (emphasis raises, focused narrows, subtle blink micro-dip)
  let browLift = 0.18 * Math.sin(time * 1.48) + 0.06 * Math.sin(time * 0.68 + 0.3);
  if (mood === 'alert' || eyeState === 'wide') browLift = 0.72;
  else if (mood === 'thinking') browLift = 0.38 + Math.sin(time * 1.6) * 0.14;
  else if (mood === 'focused' || eyeState === 'half') browLift = -0.34;
  else if (mood === 'happy' || eyeState === 'happy') {
    browLift = 0.26 + Math.sin(time * 1.95) * 0.13 + Math.sin(time * 0.65 + 0.5) * 0.05;
  }
  if (speakEnv > 0.005) {
    browLift += speakEnv * Math.max(0, Math.sin(time * 3.8)) * (0.10 + effEnergy * 0.40);
  }
  browLift -= blink * 0.18;
  browLift = clamp(browLift, -0.45, 1.0);

  // Mouth targets mapped to the 10-viseme Talking Reference sheet
  let mouthOpen = 0;
  let mouthWidth = 1;
  let mouthRound = 0;
  let smile = mood === 'happy' ? 0.68 : 0.08;
  let jawDrop = 0;
  let teethTop = 0;
  let teethBottom = 0;
  let tongueShow = 0;
  let tongueLift = 0;
  let lipPress = 0;
  let lipBite = 0;
  let puckerRing = 0;
  let restPart = 0;

  switch (viseme) {
    case 'closed':
      mouthOpen = 0;
      mouthWidth = 0.92;
      break;
    case 'rest': // Natural/Rest subtle relaxed parting
      mouthOpen = 0.10;
      mouthWidth = 0.94;
      teethTop = 0.48;
      jawDrop = 0.05;
      restPart = 1;
      break;
    case 'smile':
      mouthOpen = 0;
      mouthWidth = 1.12;
      smile = 1;
      break;
    case 'mbp': // M / B / P bilabial pressed seal
      mouthOpen = 0;
      mouthWidth = 1.10;
      smile = 0.02;
      lipPress = 1;
      break;
    case 'fv': // F / V labiodental upper teeth resting on raised lower lip
      mouthOpen = 0.24 + energy * 0.06;
      mouthWidth = 1.02;
      teethTop = 0.98;
      lipBite = 1;
      jawDrop = 0.12;
      break;
    case 'lth': // L / Th medium open with raised tongue tip behind upper teeth
      mouthOpen = 0.56 + energy * 0.12;
      mouthWidth = 1.02;
      teethTop = 0.96;
      tongueShow = 1;
      tongueLift = 0.96;
      jawDrop = 0.45;
      break;
    case 'small': // Slightly Open (Ah/Uh) horizontal slit with crisp upper teeth bar
      mouthOpen = 0.30 + energy * 0.10;
      mouthWidth = 1.04;
      teethTop = 0.98;
      jawDrop = 0.22;
      break;
    case 'mid': // conversational open
      mouthOpen = 0.62 + energy * 0.14;
      mouthWidth = 1.06;
      mouthRound = 0.10;
      teethTop = 0.94;
      tongueShow = 0.58;
      jawDrop = 0.58;
      break;
    case 'round': { // Rounded / Oh (O/Oo) & tighter Rouh / Oo at lower energy
      const tightOo = energy < 0.45 ? 1 - smoothstep(0.18, 0.45, energy) : 0;
      mouthOpen = lerp(0.76 + energy * 0.14, 0.38, tightOo);
      mouthWidth = lerp(0.74, 0.56, tightOo);
      mouthRound = 0.98;
      puckerRing = lerp(0.92, 1.0, tightOo);
      jawDrop = lerp(0.62, 0.28, tightOo);
      break;
    }
    case 'ee': // Wide / Ee (E/I) stretched grin with crisp upper & lower teeth bars
      mouthOpen = 0.54 + energy * 0.12;
      mouthWidth = 1.36;
      smile = Math.max(smile, 0.96);
      teethTop = 1;
      teethBottom = 0.98;
      jawDrop = 0.36;
      break;
    case 'wide': // Wide Open (A/Ha) deep jaw drop + upper teeth + arched tongue
      mouthOpen = 0.92 + energy * 0.08;
      mouthWidth = 1.14;
      teethTop = 1;
      tongueShow = 0.96;
      tongueLift = 0.08;
      jawDrop = 1.0;
      break;
  }

  // Natural micro-saccades with realistic VOR fixation holds & crisp glance shifts
  const fixWindow = Math.floor(time * 0.85);
  const fixDriftX = Math.sin(time * 1.1) * 0.032;
  const fixDriftY = Math.cos(time * 0.95) * 0.015;
  const saccadeX =
    ((fixWindow * 7) % 5 === 0 ? 0.28 : (fixWindow * 3) % 4 === 1 ? -0.25 : 0.06) *
    Math.sin(fixWindow * 2.1 + 0.5);
  const saccadeY = (fixWindow * 5) % 4 === 0 ? -0.14 : (fixWindow * 3) % 5 === 2 ? 0.08 : 0.02;

  let gazeX =
    mood === 'thinking'
      ? -0.40 + Math.sin(Math.floor(time * 1.15) * 1.7) * 0.34 + fixDriftX
      : eyeState === 'side'
      ? 0.72
      : mood === 'listening'
      ? Math.sin(Math.floor(time * 0.85) * 1.9) * 0.26 + fixDriftX
      : saccadeX + fixDriftX + clamp((headYaw - 0.04) * 0.20, -0.09, 0.09);
  let gazeY =
    mood === 'thinking'
      ? -0.46 + Math.cos(Math.floor(time * 1.15) * 1.3) * 0.16 + fixDriftY
      : mood === 'alert'
      ? -0.10
      : saccadeY + fixDriftY + clamp(headPitch * 0.16, -0.045, 0.045);

  if (lookTarget) {
    gazeX = lerp(gazeX, clamp(lookTarget.x * 1.15, -1, 1), 0.75);
    gazeY = lerp(gazeY, clamp(lookTarget.y * 0.9, -1, 1), 0.75);
  }

  const listenPulse =
    speakEnv > 0.01
      ? clamp01((0.55 + effEnergy * 0.45) * speakEnv + (isListening ? (0.34 + effEnergy * 0.45) * (1 - speakEnv) : 0))
      : isListening
      ? clamp01(0.36 + effEnergy * 0.45)
      : 0;

  return {
    blink,
    squint,
    wide,
    browLift,
    mouthOpen: clamp01(mouthOpen),
    mouthWidth,
    mouthRound,
    smile: clamp01(smile),
    jawDrop: clamp01(jawDrop),
    teethTop: clamp01(teethTop),
    teethBottom: clamp01(teethBottom),
    tongueShow: clamp01(tongueShow),
    tongueLift: clamp01(tongueLift),
    lipPress: clamp01(lipPress),
    lipBite: clamp01(lipBite),
    puckerRing: clamp01(puckerRing),
    restPart: clamp01(restPart),
    gazeX,
    gazeY,
    headYaw,
    headPitch,
    headTilt,
    headNodY: breath * 0.026 - speakBounce,
    headSwayX:
      Math.sin(time * 1.25) * 0.022 +
      Math.sin(time * 0.56 + 0.7) * 0.008 +
      speakEnv * Math.sin(time * 3.8) * 0.005 * effEnergy,
    hairLagX:
      breathLag * 0.052 +
      speakEnv * Math.sin(time * 4.4 - 0.72) * 0.024 * effEnergy,
    hairLagY:
      Math.cos(time * 2.35 - 0.6) * 0.022 +
      speakEnv * Math.sin(time * 7.6 - 0.65) * 0.012 * effEnergy,
    shoulderY: breath * 0.040,
    listenPulse,
    smoothEnergy: effEnergy,
    speakEnvelope: speakEnv,
  };
}

/**
 * Stateful damped-spring / exponential animator for 60fps Canvas playback.
 * Drives smooth 2.5D head turns, blinks, eyebrow raises, lip-sync, and secondary hair elasticity.
 */
export function createPortraitAnimator() {
  const state: PortraitMotionState = {
    blink: 0,
    squint: 0.4,
    wide: 0,
    browLift: 0,
    mouthOpen: 0,
    mouthWidth: 1,
    mouthRound: 0,
    smile: 0.4,
    jawDrop: 0,
    teethTop: 0,
    teethBottom: 0,
    tongueShow: 0,
    tongueLift: 0,
    lipPress: 0,
    lipBite: 0,
    puckerRing: 0,
    restPart: 0,
    gazeX: 0,
    gazeY: 0,
    headYaw: 0,
    headPitch: 0,
    headTilt: 0,
    headNodY: 0,
    headSwayX: 0,
    hairLagX: 0,
    hairLagY: 0,
    shoulderY: 0,
    listenPulse: 0,
    smoothEnergy: 0.08,
    speakEnvelope: 0,
  };
  let hairVelX = 0;
  let hairVelY = 0;
  let mouthVel = 0;
  let jawVel = 0;
  let browVel = 0;
  let impulseNodVel = 0;
  let impulseNodOffset = 0;
  let impulsePitchOffset = 0;
  let impulsePitchVel = 0;
  let smoothEnergy = 0.08;
  let speakEnvelope = 0;
  let prevHeadTilt = 0;
  let prevHeadYaw = 0;
  let prevHeadNodY = 0;
  let prevMood: ExpressionMood | null = null;
  let prevSpeaking = false;

  const triggerImpulse = (kind: 'mood' | 'speak' | 'tap' = 'tap') => {
    if (kind === 'mood' || kind === 'tap') {
      impulseNodVel = clamp(impulseNodVel + 0.12, -0.22, 0.22);
      impulsePitchVel = clamp(impulsePitchVel + 0.68, -1.15, 1.15);
      browVel = clamp(browVel + 1.85, -3.0, 3.0);
      hairVelX = clamp(hairVelX + 0.14, -0.26, 0.26);
      hairVelY = clamp(hairVelY + 0.095, -0.17, 0.17);
    } else if (kind === 'speak') {
      impulseNodVel = clamp(impulseNodVel + 0.065, -0.14, 0.14);
      browVel = clamp(browVel + 1.10, -2.0, 2.0);
      hairVelY = clamp(hairVelY + 0.048, -0.11, 0.11);
    }
  };

  return {
    triggerImpulse,
    step(
      params: Omit<PortraitParams, 'cols' | 'rows' | 'style' | 'sourceField' | 'motion'>,
      dt: number,
      reducedMotion = false,
      lookTarget?: LookTarget | null
    ): PortraitMotionState {
      const clampedDt = Math.min(0.06, Math.max(0.004, dt));
      if (!reducedMotion) {
        if (!prevSpeaking && params.isSpeaking) {
          triggerImpulse('speak');
        } else if (prevMood !== null && prevMood !== params.mood && params.mood !== 'speaking') {
          triggerImpulse('mood');
        }
      }
      prevMood = params.mood;
      prevSpeaking = params.isSpeaking;

      speakEnvelope = expApproach(speakEnvelope, params.isSpeaking ? 1 : 0, 8.2, clampedDt);
      smoothEnergy = expApproach(smoothEnergy, clamp01(params.energy), params.energy > smoothEnergy ? 8.5 : 6.4, clampedDt);
      state.speakEnvelope = speakEnvelope;
      state.smoothEnergy = smoothEnergy;

      const target = getTargetMotionFromParams(
        params as PortraitParams,
        lookTarget,
        speakEnvelope,
        smoothEnergy
      );

      // Asymmetric crisp blink: fast snap-close (rate 52), graceful lid lift (rate 22)
      const blinkRate = target.blink > state.blink ? 52 : 22;
      state.blink = expApproach(state.blink, target.blink, blinkRate, clampedDt);
      state.squint = expApproach(state.squint, target.squint, 10, clampedDt);
      state.wide = expApproach(state.wide, target.wide, 14, clampedDt);

      // Smoothly damped brow micro-expression spring for expressive emphasis
      browVel += ((target.browLift - state.browLift) * 125 - browVel * 20) * clampedDt;
      state.browLift = clamp(state.browLift + browVel * clampedDt, -0.48, 1.0);

      // Snappy critically-damped 2nd-order filter for lip-sync mouth & jaw (crisp syllable hits, zero teleport pops)
      const mouthStiffness = target.mouthOpen > state.mouthOpen ? 162 : 138;
      mouthVel += ((target.mouthOpen - state.mouthOpen) * mouthStiffness - mouthVel * 23.2) * clampedDt;
      state.mouthOpen = clamp01(state.mouthOpen + mouthVel * clampedDt);

      jawVel += ((target.jawDrop - state.jawDrop) * 156 - jawVel * 22.8) * clampedDt;
      state.jawDrop = clamp01(state.jawDrop + jawVel * clampedDt);

      state.mouthWidth = expApproach(state.mouthWidth, target.mouthWidth, 19.5, clampedDt);
      state.mouthRound = expApproach(state.mouthRound, target.mouthRound, 20.0, clampedDt);
      state.smile = expApproach(state.smile, target.smile, 13.5, clampedDt);
      state.teethTop = expApproach(state.teethTop, target.teethTop, 21.0, clampedDt);
      state.teethBottom = expApproach(state.teethBottom, target.teethBottom, 20.0, clampedDt);
      state.tongueShow = expApproach(state.tongueShow, target.tongueShow, 19.5, clampedDt);
      state.tongueLift = expApproach(state.tongueLift, target.tongueLift, 19.5, clampedDt);
      state.lipPress = expApproach(state.lipPress, target.lipPress, 22.0, clampedDt);
      state.lipBite = expApproach(state.lipBite, target.lipBite, 21.0, clampedDt);
      state.puckerRing = expApproach(state.puckerRing, target.puckerRing, 20.0, clampedDt);
      state.restPart = expApproach(state.restPart, target.restPart, 18.0, clampedDt);

      // Crisp eye saccades between fixation points
      state.gazeX = expApproach(state.gazeX, target.gazeX, 18, clampedDt);
      state.gazeY = expApproach(state.gazeY, target.gazeY, 18, clampedDt);

      if (reducedMotion) {
        state.headYaw = expApproach(state.headYaw, target.headYaw * 0.2, 5, clampedDt);
        state.headPitch = expApproach(state.headPitch, target.headPitch * 0.2, 5, clampedDt);
        state.headTilt = expApproach(state.headTilt, target.headTilt * 0.25, 6, clampedDt);
        state.headNodY = expApproach(state.headNodY, target.headNodY * 0.2, 6, clampedDt);
        state.headSwayX = 0;
        state.hairLagX = 0;
        state.hairLagY = 0;
        state.shoulderY = 0;
        impulseNodVel = 0;
        impulseNodOffset = 0;
        impulsePitchOffset = 0;
        impulsePitchVel = 0;
      } else {
        impulseNodVel += (-impulseNodOffset * 135 - impulseNodVel * 17) * clampedDt;
        impulseNodOffset = clamp(impulseNodOffset + impulseNodVel * clampedDt, -0.022, 0.022);

        impulsePitchVel += (-impulsePitchOffset * 120 - impulsePitchVel * 16) * clampedDt;
        impulsePitchOffset = clamp(impulsePitchOffset + impulsePitchVel * clampedDt, -0.085, 0.085);

        state.headYaw = expApproach(state.headYaw, target.headYaw, 6.2, clampedDt);
        state.headPitch = clamp(
          expApproach(state.headPitch, target.headPitch, 6.6, clampedDt) + impulsePitchOffset,
          -0.24,
          0.24
        );
        state.headTilt = expApproach(state.headTilt, target.headTilt, 7.2, clampedDt);
        state.headNodY =
          expApproach(state.headNodY, target.headNodY, 8.4, clampedDt) +
          impulseNodOffset -
          Math.max(0, jawVel) * 0.0012;
        state.headSwayX = expApproach(state.headSwayX, target.headSwayX, 7.4, clampedDt);
        state.shoulderY = expApproach(state.shoulderY, target.shoulderY, 5.5, clampedDt);

        // Spring-damper secondary follow-through on lower hair curls reacting smoothly to head motion
        const tiltVel = clamp((state.headTilt - prevHeadTilt) / clampedDt, -1.2, 1.2);
        const yawVel = clamp((state.headYaw - prevHeadYaw) / clampedDt, -1.4, 1.4);
        const nodVel = clamp((state.headNodY - prevHeadNodY) / clampedDt, -0.8, 0.8);
        const desiredLagX = target.hairLagX - tiltVel * 0.044 - yawVel * 0.056;
        const desiredLagY = target.hairLagY - nodVel * 0.036;

        const stiffness = 54;
        const damping = 10.6;
        hairVelX = clamp(
          hairVelX + ((desiredLagX - state.hairLagX) * stiffness - hairVelX * damping) * clampedDt,
          -0.36,
          0.36
        );
        hairVelY = clamp(
          hairVelY + ((desiredLagY - state.hairLagY) * stiffness - hairVelY * damping) * clampedDt,
          -0.24,
          0.24
        );
        state.hairLagX = clamp(state.hairLagX + hairVelX * clampedDt, -0.092, 0.092);
        state.hairLagY = clamp(state.hairLagY + hairVelY * clampedDt, -0.060, 0.060);
      }

      state.listenPulse = expApproach(state.listenPulse, target.listenPulse, 9, clampedDt);
      prevHeadTilt = state.headTilt;
      prevHeadYaw = state.headYaw;
      prevHeadNodY = state.headNodY;

      return { ...state };
    },
  };
}

/**
 * Converts RGBA pixels of the transparent black-ink character reference artwork
 * into a sculpted chiaroscuro luminance field for dark-mode halftone dithering.
 */
export function buildPortraitSourceFromRgba(
  rgba: Uint8ClampedArray,
  width: number,
  height: number
): PortraitSourceField {
  const total = width * height;
  const outside = new Uint8Array(total);
  const queue = new Int32Array(total);
  let qHead = 0;
  let qTail = 0;

  const alphaAtIdx = (idx: number) => rgba[idx * 4 + 3];
  const pushOutside = (x: number, y: number) => {
    const idx = y * width + x;
    if (!outside[idx] && alphaAtIdx(idx) < 42) {
      outside[idx] = 1;
      queue[qTail++] = idx;
    }
  };

  for (let x = 0; x < width; x++) {
    pushOutside(x, 0);
    if (x < width * 0.26 || x > width * 0.72) {
      pushOutside(x, height - 1);
    }
  }
  for (let y = 0; y < height; y++) {
    pushOutside(0, y);
    pushOutside(width - 1, y);
  }

  while (qHead < qTail) {
    const p = queue[qHead++];
    const x = p % width;
    const y = (p / width) | 0;
    if (x > 0) pushOutside(x - 1, y);
    if (x + 1 < width) pushOutside(x + 1, y);
    if (y > 0) pushOutside(x, y - 1);
    if (y + 1 < height) pushOutside(x, y + 1);
  }

  const baseLum = new Float32Array(total);
  const inkMask = new Float32Array(total);

  for (let y = 0; y < height; y++) {
    const uy = y / Math.max(1, height - 1);
    const bottomFade = 1 - smoothstep(0.84, 0.99, uy);

    for (let x = 0; x < width; x++) {
      const ux = x / Math.max(1, width - 1);
      const idx = y * width + x;

      if (outside[idx]) {
        baseLum[idx] = 0;
        inkMask[idx] = 0;
        continue;
      }

      const alpha = rgba[idx * 4 + 3] / 255;
      const r = rgba[idx * 4];
      const g = rgba[idx * 4 + 1];
      const b = rgba[idx * 4 + 2];
      const rgbBright = (0.299 * r + 0.587 * g + 0.114 * b) / 255;
      const ink = alpha * (1 - rgbBright * 0.85);
      inkMask[idx] = ink;

      const nearOut =
        (x > 0 && outside[idx - 1]) ||
        (x + 1 < width && outside[idx + 1]) ||
        (y > 0 && outside[idx - width]) ||
        (y + 1 < height && outside[idx + width]) ||
        (x > 1 && outside[idx - 2]) ||
        (x + 2 < width && outside[idx + 2]);

      if (ink > 0.35) {
        const crownSheen = Math.exp(-(((uy - 0.14) * 5.0) ** 2)) * 0.16;
        const strandWave =
          ux > 0.44 && ux < 0.86 && uy > 0.24 && uy < 0.90
            ? Math.max(0, Math.sin((ux - uy * 0.22) * 34)) * 0.135 * (1 - smoothstep(0.76, 0.94, uy))
            : 0;
        const curlSheen =
          uy > 0.54
            ? (Math.exp(-(((ux - 0.72) * 4.2) ** 2)) * 0.12 +
                Math.exp(-(((ux - 0.16) * 5.2) ** 2)) * 0.11)
            : 0;
        const rimGlow = nearOut ? 0.52 : 0;
        baseLum[idx] = clamp01((0.055 + crownSheen + strandWave + curlSheen + rimGlow) * bottomFade);
      } else {
        const isHeadbandOrShine = uy < 0.36;
        if (isHeadbandOrShine) {
          const bandSheen = 0.85 + Math.exp(-(((ux - 0.52) * 4.0) ** 2)) * 0.12;
          baseLum[idx] = clamp01(bandSheen * bottomFade);
        } else {
          const isEyeZone = ux > 0.16 && ux < 0.45 && uy > 0.39 && uy < 0.475;
          const cheekSpec = Math.exp(-(((ux - 0.24) * 7.2) ** 2 + ((uy - 0.535) * 7.8) ** 2)) * 0.12;
          const noseBridgeSpec = Math.exp(-(((ux - 0.21) * 15.0) ** 2 + ((uy - 0.495) * 8.5) ** 2)) * 0.14;
          const cheekShadow = smoothstep(0.285, 0.50, ux) * 0.36;
          const bangShadow = (1 - smoothstep(0.35, 0.39, uy)) * 0.16;
          const chinNeckShadow = smoothstep(0.62, 0.87, uy) * 0.24;
          const eyeBoost = isEyeZone ? 0.15 : 0;
          const faceLum = 0.62 + cheekSpec + noseBridgeSpec + eyeBoost - cheekShadow - bangShadow - chinNeckShadow;
          baseLum[idx] = clamp01(Math.min(0.86, faceLum) * (1 - ink * 0.94) * bottomFade);
        }
      }
    }
  }

  return { width, height, baseLum, inkMask };
}

export interface ReferencePoseBlend {
  mouthIndexA: number;
  mouthIndexB: number;
  mouthBlend: number;
  eyeIndex: number;
  eyeBlend: number;
  eyeHalfIndex: number;
  eyeHalfWeight: number;
  eyeClosedIndex: number;
  eyeClosedWeight: number;
  blinkAmount: number;
}

/**
 * Maps continuous facial motion state + phoneme viseme onto smooth crossfade indices
 * across the 12-panel reference sprite sheet (panels 01..12 mapped to indices 0..11),
 * decoupling mouth articulation from continuous 3-stage blinking (open -> half-lid -> closed).
 */
export function resolveReferencePoseBlend(
  motion: PortraitMotionState,
  viseme: MouthViseme,
  mood: ExpressionMood,
  isSpeaking: boolean
): ReferencePoseBlend {
  const open = clamp01(motion.mouthOpen);
  const roundAmt = clamp01(motion.mouthRound);
  const smileAmt = clamp01(motion.smile);
  const blinkAmt = clamp01(motion.blink);

  let mouthIndexA = 0;
  let mouthIndexB = 0;
  let mouthBlend = 0;

  if (!isSpeaking && open < 0.08 && motion.lipPress < 0.2 && motion.lipBite < 0.2) {
    // Smooth idle crossfade between neutral alert (01 -> idx 0) and warm subtle smile (12 -> idx 11)
    mouthIndexA = 0;
    mouthIndexB = 11;
    mouthBlend = smoothstep(0.15, 0.85, smileAmt);
  } else if (viseme === 'round' || roundAmt > 0.45) {
    // O / Oo sequence: slight part (02) -> tight O pucker (08, idx 7) <-> rounded mid vowel (07, idx 6)
    if (open < 0.48) {
      mouthIndexA = 9; // narrow round onset (10)
      mouthIndexB = 7; // tight O pucker (08)
      mouthBlend = smoothstep(0.12, 0.48, open);
    } else {
      mouthIndexA = 6; // rounded open (07)
      mouthIndexB = 7; // tight O pucker (08)
      mouthBlend = 1 - smoothstep(0.48, 0.88, open);
    }
  } else if (viseme === 'fv' || motion.lipBite > 0.42) {
    mouthIndexA = 1; // gentle part (02)
    mouthIndexB = 9; // narrow labiodental (10)
    mouthBlend = clamp01(motion.lipBite);
  } else if (viseme === 'lth' || motion.tongueLift > 0.42) {
    mouthIndexA = 8; // oblique 3/4 lingual/mid (09)
    mouthIndexB = 2; // crisp upper teeth (03)
    mouthBlend = 0.45;
  } else if (viseme === 'ee' || (motion.mouthWidth > 1.15 && open > 0.24)) {
    mouthIndexA = 2; // crisp open teeth (03)
    mouthIndexB = 8; // 3/4 oblique teeth/jaw (09)
    mouthBlend = clamp01((open - 0.24) * 1.15);
  } else if (motion.lipPress > 0.45 && open < 0.14) {
    mouthIndexA = 0; // clean sealed lips (01)
    mouthIndexB = 9; // pressed tension (10)
    mouthBlend = clamp01(motion.lipPress * 0.65);
  } else if (open < 0.12) {
    mouthIndexA = smileAmt > 0.45 ? 11 : 0; // 12 or 01
    mouthIndexB = 1; // gentle parting (02)
    mouthBlend = smoothstep(0.02, 0.12, open);
  } else if (open < 0.34) {
    mouthIndexA = 1; // 02 gentle part
    mouthIndexB = 2; // 03 crisp open + upper teeth
    mouthBlend = (open - 0.12) / 0.22;
  } else if (open < 0.66) {
    mouthIndexA = 2; // 03 crisp open
    mouthIndexB = mood === 'thinking' ? 8 : 3; // 09 oblique or 04 peak open
    mouthBlend = (open - 0.34) / 0.32;
  } else {
    mouthIndexA = 2; // 03 conversational open
    mouthIndexB = 3; // 04 wide jaw drop
    mouthBlend = clamp01((open - 0.56) / 0.36);
  }

  // Continuous 3-stage eye blink & half-lid expression overlay from panels 05, 06, 07, 11
  const eyeHalfIndex = open > 0.52 ? 4 : 6;
  const eyeClosedIndex = smileAmt > 0.45 || mood === 'happy' ? 10 : 5;

  // Focused squint gently engages half-lid only when genuinely focused/half-lidded (never default happy)
  const focusedSquint = mood === 'focused' ? clamp01((motion.squint - 0.36) * 2.2) : 0;
  const effBlink = Math.max(blinkAmt, focusedSquint * 0.52);

  // Smooth tent + S-curve weights so open -> half -> closed has zero jump at any threshold
  const eyeHalfWeight =
    effBlink <= 0.5
      ? smoothstep(0.06, 0.48, effBlink)
      : 1 - smoothstep(0.46, 0.86, effBlink);
  const eyeClosedWeight = smoothstep(0.42, 0.88, effBlink);

  let eyeIndex = mouthIndexB;
  let eyeBlend = clamp01(eyeHalfWeight + eyeClosedWeight);
  if (eyeClosedWeight >= eyeHalfWeight && eyeClosedWeight > 0.04) {
    eyeIndex = eyeClosedIndex;
  } else if (eyeHalfWeight > 0.04) {
    eyeIndex = eyeHalfIndex;
  } else if (isSpeaking && open > 0.50) {
    // Smoothly ramped engaged wide eyes during vocal emphasis (04 -> idx 3)
    eyeIndex = 3;
    eyeBlend = smoothstep(0.50, 0.85, open) * 0.58;
  }

  return {
    mouthIndexA,
    mouthIndexB,
    mouthBlend: clamp01(mouthBlend),
    eyeIndex,
    eyeBlend: clamp01(eyeBlend),
    eyeHalfIndex,
    eyeHalfWeight: clamp01(eyeHalfWeight),
    eyeClosedIndex,
    eyeClosedWeight: clamp01(eyeClosedWeight),
    blinkAmount: blinkAmt,
  };
}

function detectAxisSegments(
  isDividerLine: (pos: number) => boolean,
  total: number,
  expectedCount: number
): Array<[number, number]> {
  const dividers: Array<[number, number]> = [];
  let inDiv = false;
  let divStart = 0;

  // Scan interior dividers (ignoring outermost 4% borders)
  const minScan = Math.floor(total * 0.08);
  const maxScan = Math.ceil(total * 0.92);
  for (let p = minScan; p < maxScan; p++) {
    const hit = isDividerLine(p);
    if (hit && !inDiv) {
      inDiv = true;
      divStart = p;
    } else if (!hit && inDiv) {
      inDiv = false;
      dividers.push([divStart, p - 1]);
    }
  }

  // Pick evenly spaced divider bands matching expectedCount - 1 splits
  if (dividers.length >= expectedCount - 1) {
    const selected: Array<[number, number]> = [];
    for (let k = 1; k < expectedCount; k++) {
      const target = (total * k) / expectedCount;
      let best = dividers[0];
      let bestDist = Infinity;
      for (const d of dividers) {
        const mid = (d[0] + d[1]) * 0.5;
        const dist = Math.abs(mid - target);
        if (dist < bestDist) {
          bestDist = dist;
          best = d;
        }
      }
      if (bestDist < total * 0.08) {
        selected.push(best);
      }
    }
    if (selected.length === expectedCount - 1) {
      const segments: Array<[number, number]> = [];
      let cur = 2;
      for (const [d0, d1] of selected) {
        segments.push([cur, Math.max(cur + 8, d0 - 2)]);
        cur = d1 + 2;
      }
      segments.push([cur, Math.max(cur + 8, total - 3)]);
      return segments;
    }
  }

  // Fallback equal splits
  const segments: Array<[number, number]> = [];
  for (let k = 0; k < expectedCount; k++) {
    const s = Math.floor((total * k) / expectedCount) + 2;
    const e = Math.floor((total * (k + 1)) / expectedCount) - 2;
    segments.push([s, Math.max(s + 8, e)]);
  }
  return segments;
}

/**
 * Extracts, cleans, aligns, and builds a 12-pose PortraitSourceField bank
 * from a 4x3 character talking sprite sheet (such as image_f0b5dc2e.png).
 */
export function buildPortraitBankFromSpriteSheet(
  rgba: Uint8ClampedArray,
  width: number,
  height: number,
  targetSize: number = 160
): PortraitSourceField[] {
  const lumAt = (x: number, y: number) => {
    const i = (y * width + x) * 4;
    return (0.299 * rgba[i] + 0.587 * rgba[i + 1] + 0.114 * rgba[i + 2]) / 255;
  };

  const rowSegments = detectAxisSegments(
    (y) => {
      let dark = 0;
      for (let x = 0; x < width; x += 2) {
        if (lumAt(x, y) < 0.92) dark++;
      }
      return dark / Math.ceil(width / 2) > 0.72;
    },
    height,
    3
  );

  const colSegments = detectAxisSegments(
    (x) => {
      let dark = 0;
      for (let y = 0; y < height; y += 2) {
        if (lumAt(x, y) < 0.92) dark++;
      }
      return dark / Math.ceil(height / 2) > 0.72;
    },
    width,
    4
  );

  const bank: PortraitSourceField[] = [];

  for (const [y0, y1] of rowSegments) {
    for (const [x0, x1] of colSegments) {
      const cw = x1 - x0 + 1;
      const ch = y1 - y0 + 1;
      const cTotal = cw * ch;

      // Copy cell RGB & mark candidate dark ink
      const cellRgb = new Uint8ClampedArray(cTotal * 3);
      for (let ry = 0; ry < ch; ry++) {
        for (let rx = 0; rx < cw; rx++) {
          const srcIdx = ((y0 + ry) * width + (x0 + rx)) * 4;
          const dstIdx = (ry * cw + rx) * 3;
          const a = rgba[srcIdx + 3] / 255;
          cellRgb[dstIdx] = Math.round(rgba[srcIdx] * a + 255 * (1 - a));
          cellRgb[dstIdx + 1] = Math.round(rgba[srcIdx + 1] * a + 255 * (1 - a));
          cellRgb[dstIdx + 2] = Math.round(rgba[srcIdx + 2] * a + 255 * (1 - a));
        }
      }

      // 1) Remove disconnected panel number labels ("01".."12") strictly inside top-left corner
      const visited = new Uint8Array(cTotal);
      const queue = new Int32Array(cTotal);
      const labelMaxX = Math.floor(cw * 0.24);
      const labelMaxY = Math.floor(ch * 0.20);
      const boundMaxX = Math.floor(cw * 0.27);
      const boundMaxY = Math.floor(ch * 0.23);

      const isCellDark = (idx: number) =>
        (cellRgb[idx * 3] + cellRgb[idx * 3 + 1] + cellRgb[idx * 3 + 2]) / 3 < 195;
      // Match flood-fill ceiling (>234) so anti-aliased digit edge pixels are also stripped
      const isLabelCandidate = (idx: number) =>
        cellRgb[idx * 3] <= 236 || cellRgb[idx * 3 + 1] <= 236 || cellRgb[idx * 3 + 2] <= 236;

      for (let sy = 0; sy < labelMaxY; sy++) {
        for (let sx = 0; sx < labelMaxX; sx++) {
          const seed = sy * cw + sx;
          if (visited[seed] || !isLabelCandidate(seed)) continue;
          let qH = 0;
          let qT = 0;
          visited[seed] = 1;
          queue[qT++] = seed;
          let compMaxX = sx;
          let compMaxY = sy;

          while (qH < qT) {
            const p = queue[qH++];
            const px = p % cw;
            const py = (p / cw) | 0;
            if (px > compMaxX) compMaxX = px;
            if (py > compMaxY) compMaxY = py;
            const nbs = [
              px > 0 ? p - 1 : -1,
              px + 1 < cw ? p + 1 : -1,
              py > 0 ? p - cw : -1,
              py + 1 < ch ? p + cw : -1,
            ];
            for (const nb of nbs) {
              if (nb >= 0 && !visited[nb] && isLabelCandidate(nb)) {
                visited[nb] = 1;
                queue[qT++] = nb;
              }
            }
          }

          if (compMaxX < boundMaxX && compMaxY < boundMaxY) {
            for (let k = 0; k < qT; k++) {
              const p = queue[k];
              const px = p % cw;
              const py = (p / cw) | 0;
              for (let dy = -1; dy <= 1; dy++) {
                const ny = py + dy;
                if (ny < 0 || ny >= ch) continue;
                for (let dx = -1; dx <= 1; dx++) {
                  const nx = px + dx;
                  if (nx < 0 || nx >= cw) continue;
                  const np = ny * cw + nx;
                  cellRgb[np * 3] = 255;
                  cellRgb[np * 3 + 1] = 255;
                  cellRgb[np * 3 + 2] = 255;
                }
              }
            }
          }
        }
      }

      // 2) Exterior flood-fill from bright outer background -> transparent alpha 0
      const outside = new Uint8Array(cTotal);
      let oH = 0;
      let oT = 0;
      const isBrightBg = (idx: number) =>
        cellRgb[idx * 3] > 234 && cellRgb[idx * 3 + 1] > 234 && cellRgb[idx * 3 + 2] > 234;
      const pushOut = (rx: number, ry: number) => {
        const idx = ry * cw + rx;
        if (!outside[idx] && isBrightBg(idx)) {
          outside[idx] = 1;
          queue[oT++] = idx;
        }
      };

      for (let rx = 0; rx < cw; rx++) {
        pushOut(rx, 0);
        if (rx < cw * 0.25 || rx > cw * 0.74) {
          pushOut(rx, ch - 1);
        }
      }
      for (let ry = 0; ry < ch; ry++) {
        pushOut(0, ry);
        pushOut(cw - 1, ry);
      }

      while (oH < oT) {
        const p = queue[oH++];
        const px = p % cw;
        const py = (p / cw) | 0;
        if (px > 0) pushOut(px - 1, py);
        if (px + 1 < cw) pushOut(px + 1, py);
        if (py > 0) pushOut(px, py - 1);
        if (py + 1 < ch) pushOut(px, py + 1);
      }

      // 3) Measure stable alignment anchor (crown top Y & upper-hair horizontal centroid)
      let crownY = Math.floor(ch * 0.06);
      let foundCrown = false;
      let sumX = 0;
      let cntX = 0;
      for (let ry = 0; ry < ch; ry++) {
        for (let rx = 0; rx < cw; rx++) {
          const idx = ry * cw + rx;
          if (!outside[idx] && isCellDark(idx)) {
            if (!foundCrown && rx > cw * 0.22 && rx < cw * 0.78) {
              crownY = ry;
              foundCrown = true;
            }
            if (ry >= ch * 0.10 && ry <= ch * 0.48) {
              sumX += rx;
              cntX++;
            }
          }
        }
      }
      const anchorCx = cntX > 0 ? sumX / cntX : cw * 0.5;
      const anchorCrownY = foundCrown ? crownY : ch * 0.06;

      // 4) Resample aligned panel into canonical targetSize x targetSize RGBA buffer with clean bilinear interpolation
      const aligned = new Uint8ClampedArray(targetSize * targetSize * 4);
      const scale = Math.min(cw, ch) * 0.94;
      const dstCx = targetSize * 0.505;
      const dstCrownY = targetSize * 0.055;

      for (let ty = 0; ty < targetSize; ty++) {
        const srcY = anchorCrownY + ((ty - dstCrownY) / targetSize) * scale;
        if (srcY < 0 || srcY >= ch - 1) continue;
        const y0i = srcY | 0;
        const y1i = y0i + 1 < ch ? y0i + 1 : y0i;
        const fty = srcY - y0i;
        for (let tx = 0; tx < targetSize; tx++) {
          const srcX = anchorCx + ((tx - dstCx) / targetSize) * scale;
          const outIdx = (ty * targetSize + tx) * 4;
          if (srcX < 0 || srcX >= cw - 1) {
            aligned[outIdx + 3] = 0;
            continue;
          }
          const x0i = srcX | 0;
          const x1i = x0i + 1 < cw ? x0i + 1 : x0i;
          const ftx = srcX - x0i;
          const i00 = y0i * cw + x0i;
          const i10 = y0i * cw + x1i;
          const i01 = y1i * cw + x0i;
          const i11 = y1i * cw + x1i;

          const a00 = outside[i00] ? 0 : 1;
          const a10 = outside[i10] ? 0 : 1;
          const a01 = outside[i01] ? 0 : 1;
          const a11 = outside[i11] ? 0 : 1;
          const w00 = (1 - ftx) * (1 - fty) * a00;
          const w10 = ftx * (1 - fty) * a10;
          const w01 = (1 - ftx) * fty * a01;
          const w11 = ftx * fty * a11;
          const wSum = w00 + w10 + w01 + w11;
          if (wSum < 0.48) {
            aligned[outIdx + 3] = 0;
            continue;
          }
          const invW = 1 / wSum;
          aligned[outIdx] = Math.round(
            (cellRgb[i00 * 3] * w00 + cellRgb[i10 * 3] * w10 + cellRgb[i01 * 3] * w01 + cellRgb[i11 * 3] * w11) * invW
          );
          aligned[outIdx + 1] = Math.round(
            (cellRgb[i00 * 3 + 1] * w00 +
              cellRgb[i10 * 3 + 1] * w10 +
              cellRgb[i01 * 3 + 1] * w01 +
              cellRgb[i11 * 3 + 1] * w11) *
              invW
          );
          aligned[outIdx + 2] = Math.round(
            (cellRgb[i00 * 3 + 2] * w00 +
              cellRgb[i10 * 3 + 2] * w10 +
              cellRgb[i01 * 3 + 2] * w01 +
              cellRgb[i11 * 3 + 2] * w11) *
              invW
          );
          aligned[outIdx + 3] = 255;
        }
      }

      bank.push(buildPortraitSourceFromRgba(aligned, targetSize, targetSize));
    }
  }

  return bank;
}

interface SourceFieldBounds {
  minUx: number;
  maxUx: number;
  minUy: number;
  maxUy: number;
}

const sourceBoundsCache = new WeakMap<PortraitSourceField, SourceFieldBounds>();

function getSourceFieldBounds(source: PortraitSourceField): SourceFieldBounds {
  const cached = sourceBoundsCache.get(source);
  if (cached) return cached;
  const { width, height, baseLum, inkMask } = source;
  let minX = width;
  let maxX = -1;
  let minY = height;
  let maxY = -1;
  for (let y = 0; y < height; y++) {
    const off = y * width;
    for (let x = 0; x < width; x++) {
      const idx = off + x;
      if (baseLum[idx] > 0.015 || inkMask[idx] > 0.015) {
        if (x < minX) minX = x;
        if (x > maxX) maxX = x;
        if (y < minY) minY = y;
        if (y > maxY) maxY = y;
      }
    }
  }
  const invW = 1 / Math.max(1, width - 1);
  const invH = 1 / Math.max(1, height - 1);
  const bounds: SourceFieldBounds =
    maxX >= minX && maxY >= minY
      ? {
          minUx: minX * invW,
          maxUx: maxX * invW,
          minUy: minY * invH,
          maxUy: maxY * invH,
        }
      : { minUx: 0, maxUx: 1, minUy: 0, maxUy: 1 };
  sourceBoundsCache.set(source, bounds);
  return bounds;
}

interface PrecomputedReferenceFrame {
  time: number;
  energy: number;
  isSpeaking: boolean;
  isListening: boolean;
  hasTurn: boolean;
  hasWaveform: boolean;
  hasHeadYawShading: boolean;
  hasBrowLift: boolean;
  motion: PortraitMotionState;
  ct: number;
  st: number;
  rigidShiftX: number;
  rigidShiftY: number;
  glintSweep: number;
  sinT38: number;
  cosT38: number;
  sinT24: number;
  cosT24: number;
  sinT45: number;
  cosT45: number;
  waveGain: number;
  waveAmp: number;
}

function buildReferenceFrameConstants(params: PortraitParams): PrecomputedReferenceFrame {
  const motion = params.motion ?? getTargetMotionFromParams(params);
  const { time, energy, isSpeaking, isListening } = params;
  const effEnergy = motion.smoothEnergy ?? energy;
  const speakEnv = motion.speakEnvelope ?? (isSpeaking ? 1 : 0);
  const hasTurn =
    motion.headYaw > 0.004 ||
    motion.headYaw < -0.004 ||
    motion.headPitch > 0.004 ||
    motion.headPitch < -0.004;

  const waveAmpScale = isListening ? Math.max(speakEnv, 1) : speakEnv;
  const waveGain = clamp01(effEnergy * 1.05 * waveAmpScale);

  // Poised, rigid-body portrait kinematics (zero rubber-sheet hair wave or differential shear)
  const rigidTilt = clamp(motion.headTilt * 0.34, -0.045, 0.045);
  const rigidShiftX = clamp(motion.headSwayX * 0.34 - motion.headYaw * 0.042, -0.028, 0.028);
  const rigidShiftY = clamp(motion.headNodY * 0.42 - motion.headPitch * 0.032, -0.022, 0.022);

  return {
    time,
    energy: effEnergy,
    isSpeaking,
    isListening,
    hasTurn,
    hasWaveform: waveGain > 0.08 && effEnergy > 0.14,
    hasHeadYawShading: motion.headYaw > 0.02 || motion.headYaw < -0.02,
    hasBrowLift: motion.browLift > 0.08 || motion.browLift < -0.08,
    motion,
    ct: Math.cos(rigidTilt),
    st: Math.sin(rigidTilt),
    rigidShiftX,
    rigidShiftY,
    glintSweep: (time * 0.52) % 1.15,
    sinT38: Math.sin(time * 3.8),
    cosT38: Math.cos(time * 3.8),
    sinT24: Math.sin(time * 2.4),
    cosT24: Math.cos(time * 2.4),
    sinT45: Math.sin(time * 4.5),
    cosT45: Math.cos(time * 4.5),
    waveGain,
    waveAmp: 0.011 + effEnergy * 0.012,
  };
}

/**
 * Samples the reference silhouette field at normalized UV (0..1, 0..1) using:
 * - Safe crown/curl framing
 * - 2.5D cylindrical/ellipsoidal depth parallax for headYaw & headPitch turning
 * - Overlapping multi-zone hair, bang, and shoulder breathing kinematics
 * - Sculpted eyelids, eyebrows, happy squints, dual iris catchlights, and audio-synced mouth visemes
 */
function sampleWaveformHalo(
  ax: number,
  ay: number,
  fc: PrecomputedReferenceFrame
): number {
  const adx = ax - 0.47;
  const ady = (ay - 0.43) * 0.92;
  const adistSq = adx * adx + ady * ady;
  if (adistSq <= 0.172225 || adistSq >= 0.245025) return 0;
  const adist = Math.sqrt(adistSq);
  const invR = 1 / adist;
  const u = adx * invR;
  const v = ady * invR;
  const u2 = u * u - v * v;
  const v2 = 2 * u * v;
  const u3 = u * (4 * u * u - 3);
  const v3 = v * (3 - 4 * v * v);
  const u4 = u2 * u2 - v2 * v2;
  const v4 = 2 * v2 * u2;
  const u5 = u2 * u3 - v2 * v3;
  const v5 = v2 * u3 + u2 * v3;
  const waveR =
    0.445 +
    (v5 * fc.cosT38 + u5 * fc.sinT38) * fc.waveAmp +
    (u3 * fc.cosT24 + v3 * fc.sinT24) * 0.008;
  const ringBand = Math.abs(adist - waveR);
  if (ringBand >= 0.014) return 0;
  const arcPulse = 0.5 + 0.5 * (v4 * fc.cosT45 - u4 * fc.sinT45);
  const sideWeight = 0.35 + 0.65 * (u < 0 ? -u : u);
  const env =
    (1 - smoothstep(0.002, 0.014, ringBand)) *
    sideWeight *
    (0.28 + arcPulse * 0.48) *
    fc.waveGain;
  return clamp01(env * (ay > 0.74 ? 1 - smoothstep(0.74, 0.86, ay) : 1));
}

function applyReferenceEyeExpression(
  ax: number,
  ay: number,
  lum: number,
  motion: PortraitMotionState
): number {
  const blink = motion.blink;
  if (blink > 0.05) {
    const upperDropL = lerp(0.414, 0.453, blink);
    const upperDropR = lerp(0.402, 0.445, blink);
    const lowerRise = smoothstep(0.55, 1.0, blink) * 0.010;
    const botL = 0.465 - lowerRise;
    const botR = 0.468 - lowerRise;

    const inEyeXL = 1 - smoothstep(0.018, 0.025, Math.abs(ax - 0.223));
    const inEyeXR = 1 - smoothstep(0.049, 0.064, Math.abs(ax - 0.380));
    const skinToneL = 0.70;
    const skinToneR = clamp01(0.67 - smoothstep(0.32, 0.46, ax) * 0.24);

    const wipeBotL = blink > 0.64 ? lerp(upperDropL, 0.476, smoothstep(0.64, 0.88, blink)) : upperDropL;
    const wipeBotR = blink > 0.64 ? lerp(upperDropR, 0.478, smoothstep(0.64, 0.88, blink)) : upperDropR;

    if (inEyeXL > 0.02 && ay > 0.398 && ay < wipeBotL + 0.004) {
      const vMask = smoothstep(0.398, 0.408, ay) * (1 - smoothstep(wipeBotL - 0.004, wipeBotL + 0.004, ay));
      const mask = inEyeXL * vMask * smoothstep(0.04, 0.20, blink);
      lum = lerp(lum, skinToneL, mask * 0.98);
    }
    if (inEyeXR > 0.02 && ay > 0.384 && ay < wipeBotR + 0.004) {
      const vMask = smoothstep(0.384, 0.395, ay) * (1 - smoothstep(wipeBotR - 0.004, wipeBotR + 0.004, ay));
      const mask = inEyeXR * vMask * smoothstep(0.04, 0.20, blink);
      lum = lerp(lum, skinToneR, mask * 0.98);
    }

    if (blink > 0.70) {
      const lowCleanL = inEyeXL * smoothstep(botL - 0.004, botL + 0.005, ay) * (1 - smoothstep(botL + 0.005, 0.476, ay));
      const lowCleanR = inEyeXR * smoothstep(botR - 0.004, botR + 0.005, ay) * (1 - smoothstep(botR + 0.005, 0.478, ay));
      const fullClean = smoothstep(0.70, 0.92, blink);
      if (lowCleanL > 0.02) lum = lerp(lum, skinToneL, lowCleanL * fullClean * 0.92);
      if (lowCleanR > 0.02) lum = lerp(lum, skinToneR, lowCleanR * fullClean * 0.92);
    }

    const clNorm = (ax - 0.223) / 0.024;
    const crNorm = (ax - 0.380) / 0.056;
    const creaseCurveL = clNorm * clNorm * 0.0025;
    const creaseCurveR = crNorm * crNorm * 0.004;
    const dCreaseL = sdCapsule2(ax, ay + creaseCurveL, 0.202, upperDropL - 0.015, 0.242, upperDropL - 0.016, 0.0016);
    const dCreaseR = sdCapsule2(ax, ay + creaseCurveR, 0.334, upperDropR - 0.016, 0.426, upperDropR - 0.018, 0.0018);
    const dCrease = dCreaseL < dCreaseR ? dCreaseL : dCreaseR;
    if (dCrease < 0.004) {
      const creaseEnvelope = smoothstep(0.18, 0.48, blink) * (1 - smoothstep(0.66, 0.96, blink) * 0.72);
      const cMask = (1 - smoothstep(-0.001, 0.003, dCrease)) * creaseEnvelope;
      lum *= 1 - cMask * 0.22;
    }

    const lashBaseL = lerp(upperDropL, 0.453, smoothstep(0.65, 1.0, blink));
    const lashBaseR = lerp(upperDropR, 0.447, smoothstep(0.65, 1.0, blink));
    const alNorm = (ax - 0.222) / 0.024;
    const arNorm = (ax - 0.380) / 0.054;
    const arcShapeL = -alNorm * alNorm * lerp(-0.0022, 0.0052, blink);
    const arcShapeR = -arNorm * arNorm * lerp(-0.003, 0.0072, blink);
    const dLashMainL = sdCapsule2(ax, ay + arcShapeL, 0.198, lashBaseL, 0.245, lashBaseL - 0.002, 0.0038);
    const dLashMainR = sdCapsule2(ax, ay + arcShapeR, 0.328, lashBaseR + 0.001, 0.433, lashBaseR - 0.003, 0.0047);
    const dWingL = sdCapsule2(ax, ay, 0.198, lashBaseL - 0.002, 0.191, lashBaseL - 0.006, 0.0024);
    const dWingR = sdCapsule2(ax, ay, 0.431, lashBaseR - 0.003, 0.443, lashBaseR - 0.009, 0.0030);
    const dLM = dLashMainL < dLashMainR ? dLashMainL : dLashMainR;
    const dW = dWingL < dWingR ? dWingL : dWingR;
    const dLashes = dLM < dW ? dLM : dW;
    if (dLashes < 0.007) {
      const lashMask = (1 - smoothstep(-0.002, 0.0046, dLashes)) * smoothstep(0.14, 0.52, blink);
      lum *= 1 - lashMask * 0.96;
    }

    if (blink < 0.62) {
      const gx = motion.gazeX * 0.008;
      const gy = motion.gazeY * 0.005;
      const halfCatch = 1 - smoothstep(0.15, 0.58, blink);
      const dHalfCatch = sdEllipse(ax, ay, 0.364 + gx, 0.436 + gy, 0.0068, 0.0065);
      if (dHalfCatch < 0.004 && ay > lashBaseR + 0.004) {
        const cMask = (1 - smoothstep(-0.002, 0.003, dHalfCatch)) * halfCatch;
        lum = Math.max(lum, cMask * 0.94);
      }
    }
  } else {
    if (motion.squint > 0.18) {
      const lift = motion.squint * 0.006;
      const cheekWarmL = sdEllipse(ax, ay, 0.218, 0.482 - lift, 0.036, 0.012);
      const cheekWarmR = sdEllipse(ax, ay, 0.378, 0.482 - lift, 0.052, 0.013);
      const dSquint = cheekWarmL < cheekWarmR ? cheekWarmL : cheekWarmR;
      if (dSquint < 0.008) {
        const sMask = (1 - smoothstep(-0.003, 0.006, dSquint)) * motion.squint * 0.65;
        const cheekTone = clamp01(0.70 - smoothstep(0.28, 0.48, ax) * 0.20);
        lum = lerp(lum, cheekTone, sMask);
      }
    }

    const gx = motion.gazeX * 0.010;
    const gy = motion.gazeY * 0.006;
    const catchScale = 1 + motion.wide * 0.2 + motion.squint * 0.08;
    const catchR = 0.0072 * catchScale;
    const dCatchL = sdEllipse(ax, ay, 0.214 + gx, 0.428 + gy, catchR * 0.85, catchR);
    const dCatchR = sdEllipse(ax, ay, 0.360 + gx, 0.422 + gy, catchR, catchR);
    const dSubCatchR = sdEllipse(ax, ay, 0.396 + gx * 0.65, 0.440 + gy * 0.65, catchR * 0.52, catchR * 0.52);
    const dCR = dCatchR < dSubCatchR ? dCatchR : dSubCatchR;
    const dCatch = dCatchL < dCR ? dCatchL : dCR;
    if (dCatch < 0.004) {
      const cMask = 1 - smoothstep(-0.002, 0.003, dCatch);
      lum = Math.max(lum, cMask * 0.98);
    }
  }
  return lum;
}

function applyReferenceMouthExpression(
  ax: number,
  ay: number,
  lum: number,
  motion: PortraitMotionState
): number {
  const mxBase = 0.274;
  const my = 0.614;
  const openAmt = motion.mouthOpen;
  const smileAmt = motion.smile;
  const lipPress = motion.lipPress;
  const lipBite = motion.lipBite;
  const restPart = motion.restPart;
  const roundAmt = clamp01(motion.mouthRound);
  const wideStretch = clamp01((motion.mouthWidth - 0.94) / 0.40) * (1 - roundAmt);
  const isPureClosed =
    openAmt <= 0.03 && lipPress < 0.08 && lipBite < 0.08 && restPart < 0.08 && smileAmt <= 0.32;

  if (isPureClosed) {
    if (ax >= 0.232 && ax <= 0.318 && ay >= 0.592 && ay <= 0.646) {
      const dPatch = sdEllipse(ax, ay, 0.273, 0.616, 0.035, 0.020);
      if (dPatch < 0.006) {
        const pMask = 1 - smoothstep(-0.003, 0.005, dPatch);
        const skinGrad = clamp01(0.73 - smoothstep(0.245, 0.335, ax) * 0.16);
        lum = lerp(lum, skinGrad, pMask * 0.92);
      }
      const bowX = (ax - 0.272) * 26;
      const bowCurve = bowX * bowX * 0.0022;
      const dClosedLine = sdCapsule2(ax, ay - bowCurve, 0.246, 0.615, 0.300, 0.613, 0.0026);
      if (dClosedLine < 0.005 && ax >= 0.232) {
        const cMask = 1 - smoothstep(-0.002, 0.0034, dClosedLine);
        lum = lerp(lum, 0.015, cMask * 0.92);
      }
      const dUnderLip = sdEllipse(ax, ay, 0.272, 0.625, 0.014, 0.0032);
      if (dUnderLip < 0.004) {
        const uMask = 1 - smoothstep(-0.002, 0.003, dUnderLip);
        lum = Math.max(lum, uMask * 0.78);
      }
    }
    return lum;
  }

  const isSmileOnly =
    openAmt <= 0.05 && lipPress < 0.15 && lipBite < 0.15 && restPart < 0.15 && smileAmt > 0.32;
  if (isSmileOnly) {
    const dOuterCornerPatch = sdEllipse(ax, ay, 0.294, 0.616, 0.024, 0.016);
    if (dOuterCornerPatch < 0.006 && ax >= 0.232) {
      const pMask = (1 - smoothstep(-0.003, 0.005, dOuterCornerPatch)) * smileAmt * 0.65;
      lum = lerp(lum, 0.68, pMask);
    }
    const smX = (ax - 0.272) * 24;
    const smileLift = smX * smX * (0.0034 + smileAmt * 0.0018);
    const dSmile = sdCapsule2(ax, ay + smileLift, 0.245, 0.615, 0.306, 0.612, 0.0030);
    if (dSmile < 0.005 && ax >= 0.232) {
      const sMask = (1 - smoothstep(-0.002, 0.0038, dSmile)) * Math.min(1, smileAmt * 0.95);
      lum = lerp(lum, 0.015, sMask * 0.88);
    }
    return lum;
  }

  const roundW = smoothstep(0.30, 0.72, roundAmt);
  const notRound = 1 - roundW;
  const eeW = notRound * smoothstep(0.22, 0.65, Math.max(motion.teethBottom, wideStretch));
  const fvW = notRound * smoothstep(0.25, 0.65, lipBite);
  const restW = notRound * smoothstep(0.25, 0.65, restPart) * (1 - smoothstep(0.08, 0.22, openAmt));

  const mx = lerp(mxBase, 0.283, eeW);
  const activeBlend = clamp01(
    Math.max(
      smoothstep(0.015, 0.28, openAmt),
      smoothstep(0.08, 0.65, lipPress),
      smoothstep(0.08, 0.65, lipBite),
      smoothstep(0.08, 0.65, restPart)
    )
  );

  const cleanW = lerp(0.038, 0.052, Math.max(openAmt, wideStretch, lipPress * 0.72));
  const cleanH = lerp(0.024, 0.038, Math.max(openAmt, lipBite * 0.5));
  if (ax >= 0.228 && ax <= 0.340 && ay >= 0.578 && ay <= 0.672) {
    const dPatch = sdEllipse(ax, ay, 0.278, 0.616, cleanW, cleanH);
    if (dPatch < 0.008) {
      const pMask = (1 - smoothstep(-0.004, 0.006, dPatch)) * activeBlend;
      const skinGrad = clamp01(0.73 - smoothstep(0.245, 0.345, ax) * 0.17);
      lum = lerp(lum, skinGrad, pMask * 0.97);
    }
  }

  if (lipPress > 0.22 && openAmt < 0.20) {
    const pressBlend = smoothstep(0.22, 0.75, lipPress) * (1 - smoothstep(0.08, 0.20, openAmt));
    const cornerX = (ax - 0.274) * 28;
    const cornerCurve = cornerX * cornerX * 0.0018;
    // Crisp inward-tucked bilabial seal (wider and firmer than relaxed closed line)
    const dSeal = sdCapsule2(ax, ay - cornerCurve, 0.236, 0.616, 0.312, 0.613, 0.0040);
    if (dSeal < 0.0055 && ax >= 0.226) {
      const sMask = (1 - smoothstep(-0.002, 0.0038, dSeal)) * pressBlend;
      lum = lerp(lum, 0.010, sMask * 0.98);
    }
    const dTickL = sdCapsule2(ax, ay, 0.236, 0.611, 0.238, 0.620, 0.0022);
    const dTickR = sdCapsule2(ax, ay, 0.311, 0.609, 0.312, 0.618, 0.0022);
    const dTicks = dTickL < dTickR ? dTickL : dTickR;
    if (dTicks < 0.0045 && ax >= 0.226) {
      const tMask = (1 - smoothstep(-0.002, 0.0032, dTicks)) * pressBlend;
      lum = lerp(lum, 0.015, tMask * 0.92);
    }
    const dUpperPillow = sdEllipse(ax, ay, 0.274, 0.607, 0.022, 0.0038);
    const dPressGlow = sdEllipse(ax, ay, 0.274, 0.626, 0.022, 0.0046);
    const dCushion = dUpperPillow < dPressGlow ? dUpperPillow : dPressGlow;
    if (dCushion < 0.005 && ax >= 0.226) {
      const gMask = (1 - smoothstep(-0.002, 0.0035, dCushion)) * pressBlend;
      lum = Math.max(lum, gMask * 0.88);
    }
    if (pressBlend > 0.88) return lum;
  }

  const liftWPre = notRound * (1 - fvW) * smoothstep(0.25, 0.65, motion.tongueLift);
  const openHigh = smoothstep(0.35, 0.55, openAmt);
  const baseRx =
    lerp(
      lerp(0.028, 0.032, clamp01(openAmt / 0.45)),
      lerp(0.032, 0.037, clamp01((openAmt - 0.45) / 0.55)),
      openHigh
    ) +
    liftWPre * 0.010;
  const rx = lerp(
    lerp(lerp(lerp(baseRx, 0.022, restW), 0.030, fvW), lerp(0.034, 0.038, Math.max(openAmt, wideStretch)), eeW),
    lerp(0.014, 0.022, openAmt),
    roundW
  );

  const baseRy =
    lerp(
      lerp(0.012, 0.019, clamp01(openAmt / 0.45)),
      lerp(0.021, 0.036, clamp01((openAmt - 0.45) / 0.55)),
      openHigh
    ) +
    liftWPre * 0.004;
  const ry = lerp(
    lerp(lerp(lerp(baseRy, 0.0062, restW), 0.0125, fvW), lerp(0.0165, 0.020, openAmt), eeW),
    lerp(0.016, 0.026, openAmt),
    roundW
  );

  const cy = my + lerp(0.000, 0.013, motion.jawDrop);
  const slantScale = lerp(lerp(0, -0.018, fvW), -0.032, eeW);
  const slantY = (ax - mx) * slantScale;
  const mouthLocalY = ay - slantY;

  const normX = (ax - mx) / (rx || 0.01);
  const cornerLift = eeW * Math.max(0, normX * normX - 0.20) * -0.0028;
  const archW = Math.max(0, 1 - normX * normX);
  const topFlatten = eeW * (archW * 0.0024 + cornerLift) + fvW * (archW * 0.0018);

  const lipBorder = lerp(lerp(0.0036, 0.0024, Math.max(restW, fvW)), 0.0034, eeW);
  const dOuterLip = sdEllipse(ax, mouthLocalY + topFlatten * 0.5, mx, cy, rx + lipBorder, ry + lipBorder);
  const dCavity = sdEllipse(ax, mouthLocalY + topFlatten, mx, cy, rx, ry);

  if (dOuterLip < 0.0055 && ax >= 0.222) {
    const lipRim = (1 - smoothstep(-0.002, 0.0034, dOuterLip)) * activeBlend;
    const rimDarken = lerp(0.80, 0.58, restW);
    const eeTarget = lerp(lum, 0.02, lipRim * 0.90);
    const stdTarget = lum * (1 - lipRim * rimDarken);
    lum = lerp(stdTarget, eeTarget, eeW);
  }

  if (dCavity < 0.005 && ax >= 0.222) {
    const cMask = (1 - smoothstep(-0.002, 0.0030, dCavity)) * activeBlend;
    lum = lerp(lum, 0.010, cMask * 0.99);

    if (motion.teethTop > 0.12 && roundW < 0.92) {
      // Keep a crisp dark oral slit beneath upper teeth on Ah/Uh (small) as well as wide vowels
      const openCutoff = lerp(cy - ry * 0.26, cy - ry * 0.42, openHigh);
      const teethCutoff = lerp(
        lerp(lerp(openCutoff, cy + ry * 0.06, restW), cy - 0.0020, eeW),
        cy + 0.0008,
        fvW
      );
      const teethSpan = lerp(lerp(0.85, 0.88, eeW), 0.86, fvW);
      if (mouthLocalY < teethCutoff && Math.abs(normX) < teethSpan) {
        const tEdge = 1 - smoothstep(teethCutoff - 0.0018, teethCutoff + 0.0002, mouthLocalY);
        const tBright = lerp(lerp(0.88, 0.98, motion.teethTop), 0.72, restW);
        lum = Math.max(lum, tBright * tEdge * cMask * (1 - roundW));
      }
    }

    const nonRoundFv = notRound * (1 - fvW);
    if (motion.teethBottom > 0.2 && nonRoundFv > 0.05) {
      const lowTeethTop = cy + 0.0022;
      if (mouthLocalY > lowTeethTop && Math.abs(normX) < 0.84) {
        const ltEdge = smoothstep(lowTeethTop, lowTeethTop + 0.0022, mouthLocalY);
        lum = Math.max(lum, 0.96 * motion.teethBottom * ltEdge * cMask * nonRoundFv);
      }
    }

    if (motion.tongueShow > 0.18 && nonRoundFv > 0.05) {
      const liftW = smoothstep(0.18, 0.58, motion.tongueLift);
      if (liftW > 0.02) {
        // Centered raised tongue tip aligned between crisp left & right dark oral pockets
        const dTongueTip = sdEllipse(ax, mouthLocalY, mx + 0.008, cy + ry * 0.18, rx * 0.24, ry * 0.42);
        if (dTongueTip < 0.004) {
          const tgMask = (1 - smoothstep(-0.002, 0.0020, dTongueTip)) * motion.tongueShow * cMask * nonRoundFv * liftW;
          lum = Math.max(lum, 0.88 * tgMask);
        }
      }
      if (liftW < 0.98) {
        const dFloorTongue = sdEllipse(ax, mouthLocalY, mx + 0.002, cy + ry * 0.66, rx * 0.56, ry * 0.30);
        if (dFloorTongue < 0.005) {
          const tgMask = (1 - smoothstep(-0.002, 0.003, dFloorTongue)) * motion.tongueShow * cMask * nonRoundFv * (1 - liftW);
          lum = Math.max(lum, 0.43 * tgMask);
        }
      }
    }

    if (lipBite > 0.18) {
      const biteW = smoothstep(0.18, 0.52, lipBite);
      const dTuckLip = sdEllipse(ax, mouthLocalY, mx, cy + ry * 0.44, rx * 0.84, ry * 0.48);
      if (dTuckLip < 0.005 && mouthLocalY > cy + 0.0020) {
        const bMask = (1 - smoothstep(-0.002, 0.0026, dTuckLip)) * biteW * cMask;
        lum = Math.max(lum, 0.96 * bMask);
      }
    }
  }

  if (motion.puckerRing > 0.20) {
    const dRing = Math.abs(sdEllipse(ax, mouthLocalY, mx + 0.001, cy + 0.001, rx + 0.0066, ry + 0.0066)) - 0.0038;
    if (dRing < 0.005 && ax >= 0.228) {
      const rMask = (1 - smoothstep(-0.002, 0.0035, dRing)) * motion.puckerRing * activeBlend;
      lum = Math.max(lum, rMask * 0.88);
    }
  }

  const lowerOffset = lerp(ry + 0.0056, ry * 0.68 + 0.0024, fvW);
  const dLowerLipRim = sdEllipse(ax, mouthLocalY, mx + 0.001, cy + lowerOffset, rx * lerp(0.80, 0.72, roundW), 0.0038);
  if (dLowerLipRim < 0.005 && ax >= 0.228) {
    const lMask = (1 - smoothstep(-0.002, 0.0035, dLowerLipRim)) * activeBlend;
    lum = Math.max(lum, lMask * 0.86);
  }
  return lum;
}

/**
 * Samples the reference silhouette field at normalized UV (0..1, 0..1) using:
 * - Safe crown/curl framing
 * - 2.5D cylindrical/ellipsoidal depth parallax for headYaw & headPitch turning
 * - Overlapping multi-zone hair, bang, and shoulder breathing kinematics
 * - Sculpted eyelids, eyebrows, happy squints, dual iris catchlights, and audio-synced mouth visemes
 */
function sampleReferenceWithExpression(
  ux: number,
  uy: number,
  fc: PrecomputedReferenceFrame,
  source: PortraitSourceField
): number {
  const { motion, ct, st, energy } = fc;

  // 1. Safe viewport framing inset so crown arc & bob curls never clip edges
  const framedX = (ux - 0.50) * 1.115 + 0.49;
  const framedY = (uy - 0.56) * 1.12 + 0.51;

  if (framedX < 0.01 || framedX > 0.99 || framedY < 0.005 || framedY > 0.995) {
    return 0;
  }

  // 2. Clean rigid-body rotation + translation around neck pivot (zero liquid shear/wave)
  const pivotX = 0.38;
  const pivotY = 0.74;
  const dx = framedX - pivotX - fc.rigidShiftX;
  const dy = framedY - pivotY - fc.rigidShiftY;

  const ax = pivotX + dx * ct - dy * st;
  let ay = pivotY + dx * st + dy * ct;

  // 3. Natural physical chin & jaw-drop articulation during open vowels
  const jawAmt = clamp01(motion.jawDrop + motion.mouthOpen * 0.12);
  if (jawAmt > 0.06 && ay > 0.620 && ay < 0.885 && ax > 0.170 && ax < 0.490) {
    const latW = smoothstep(0.170, 0.220, ax) * (1 - smoothstep(0.365, 0.490, ax));
    const vertW = smoothstep(0.620, 0.665, ay) * (1 - smoothstep(0.815, 0.885, ay));
    ay -= jawAmt * 0.036 * latW * vertW;
  }

  if (ax < 0 || ax > 1 || ay < 0 || ay > 1) {
    return 0;
  }

  // Zero-alloc inline bilinear source sample
  const w = source.width;
  const maxX = w - 1;
  const maxY = source.height - 1;
  const fx = ax * maxX;
  const fy = ay * maxY;
  const x0 = fx | 0;
  const y0 = fy | 0;
  const x1 = x0 < maxX ? x0 + 1 : maxX;
  const y1 = y0 < maxY ? y0 + 1 : maxY;
  const tx = fx - x0;
  const ty = fy - y0;

  const i00 = y0 * w + x0;
  const i10 = y0 * w + x1;
  const i01 = y1 * w + x0;
  const i11 = y1 * w + x1;

  const w00 = (1 - tx) * (1 - ty);
  const w10 = tx * (1 - ty);
  const w01 = (1 - tx) * ty;
  const w11 = tx * ty;

  const baseLumArr = source.baseLum;
  const inkMaskArr = source.inkMask;
  let lum =
    baseLumArr[i00] * w00 +
    baseLumArr[i10] * w10 +
    baseLumArr[i01] * w01 +
    baseLumArr[i11] * w11;
  const ink =
    inkMaskArr[i00] * w00 +
    inkMaskArr[i10] * w10 +
    inkMaskArr[i01] * w01 +
    inkMaskArr[i11] * w11;

  // Transparent pixel outside character silhouette: jump straight to acoustic waveform check
  if (lum <= 0.001 && ink <= 0.001) {
    return fc.hasWaveform && ay < 0.86 ? sampleWaveformHalo(ax, ay, fc) : 0;
  }

  if (ink < 0.35 && (fc.hasHeadYawShading || motion.headPitch !== 0) && ay > 0.36 && ay < 0.76 && ax > 0.17 && ax < 0.48) {
    const cheekArea = smoothstep(0.18, 0.46, ax) * smoothstep(0.38, 0.65, ay) * (1 - smoothstep(0.65, 0.76, ay));
    lum = clamp01(lum + (motion.headYaw * 0.14 - motion.headPitch * 0.06) * cheekArea);
  }

  if (fc.hasBrowLift && ink < 0.55 && ay > 0.35 && ay < 0.42 && ax > 0.18 && ax < 0.44) {
    const bOffset = -motion.browLift * 0.014;
    const dBrowL = sdCapsule2(ax, ay, 0.195, 0.392 + bOffset, 0.240, 0.386 + bOffset * 0.85, 0.0042);
    const dBrowR = sdCapsule2(ax, ay, 0.330, 0.376 + bOffset * 0.9, 0.415, 0.372 + bOffset * 0.75, 0.0048);
    const dBrows = dBrowL < dBrowR ? dBrowL : dBrowR;
    if (dBrows < 0.006) {
      const bMask = (1 - smoothstep(-0.002, 0.0042, dBrows)) * clamp01(Math.abs(motion.browLift));
      lum *= 1 - bMask * 0.74;
    }
  }

  if (ax >= 0.16 && ax <= 0.47 && ay >= 0.37 && ay <= 0.51) {
    lum = applyReferenceEyeExpression(ax, ay, lum, motion);
  }

  if (ax >= 0.222 && ax <= 0.355 && ay >= 0.575 && ay <= 0.675) {
    lum = applyReferenceMouthExpression(ax, ay, lum, motion);
  }

  const aura = motion.listenPulse;
  if (aura > 0.06 && ink < 0.35 && ay < 0.28 && lum > 0.4) {
    const gx = (ax - fc.glintSweep) * 7.5;
    const gSq = gx * gx;
    if (gSq < 8) {
      lum = clamp01(lum + fastExpNeg(gSq) * aura * (0.14 + energy * 0.14));
    }
  }

  // 9. Soft bust bottom & outer edge feathering (skip smoothstep on ~85% interior pixels)
  if (ay > 0.83) lum *= 1 - smoothstep(0.83, 0.98, ay);
  if (ax > 0.86) lum *= 1 - smoothstep(0.86, 0.985, ax);
  if (ax < 0.085) lum *= smoothstep(0.015, 0.085, ax);

  // 10. Subtle animated Braille acoustic waveform halo outside silhouette when speaking or listening
  if (fc.hasWaveform && lum < 0.04 && ay < 0.86) {
    const wf = sampleWaveformHalo(ax, ay, fc);
    if (wf > lum) lum = wf;
  }

  return clamp01(lum);
}

/**
 * Samples the 12-pose aligned sprite bank (`image_f0b5dc2e.png`) using:
 * - 2.5D head yaw/pitch depth parallax & overlapping curl/shoulder breathing
 * - Crossfaded mouth articulation across reference panels 01..12
 * - Decoupled co-articulated eye-band blending for smooth natural blinks & half-lids
 * - Subtle saccade catchlights & headband shimmer
 */
function sampleSpriteBankWithExpression(
  ux: number,
  uy: number,
  fc: PrecomputedReferenceFrame,
  bank: PortraitSourceField[],
  pose: ReferencePoseBlend
): number {
  const { motion, ct, st, energy } = fc;

  const framedX = (ux - 0.50) * 1.088 + 0.50;
  const framedY = (uy - 0.54) * 1.095 + 0.51;
  if (framedX < 0.01 || framedX > 0.99 || framedY < 0.005 || framedY > 0.995) {
    return 0;
  }

  // Clean rigid-body portrait transform around neck pivot (zero hair-wave or rubber-sheet shear)
  const pivotX = 0.42;
  const pivotY = 0.74;
  const dx = framedX - pivotX - fc.rigidShiftX;
  const dy = framedY - pivotY - fc.rigidShiftY;

  const ax = pivotX + dx * ct - dy * st;
  let ay = pivotY + dx * st + dy * ct;

  // Subtle physical chin/jaw-drop hinge during wide vowel speech in sprite-bank mode too
  const jawAmt = clamp01(motion.jawDrop * 0.75 + motion.mouthOpen * 0.08);
  if (jawAmt > 0.08 && ay > 0.625 && ay < 0.840 && ax > 0.195 && ax < 0.480) {
    const latW = smoothstep(0.195, 0.240, ax) * (1 - smoothstep(0.365, 0.480, ax));
    const vertW = smoothstep(0.625, 0.675, ay) * (1 - smoothstep(0.775, 0.840, ay));
    ay -= jawAmt * 0.016 * latW * vertW;
  }

  if (ax < 0 || ax > 1 || ay < 0 || ay > 1) {
    return 0;
  }

  const fieldA = bank[clamp(pose.mouthIndexA, 0, bank.length - 1)];
  const fieldB = bank[clamp(pose.mouthIndexB, 0, bank.length - 1)];

  const w = fieldA.width;
  const maxX = w - 1;
  const maxY = fieldA.height - 1;
  const fx = ax * maxX;
  const fy = ay * maxY;
  const x0 = fx | 0;
  const y0 = fy | 0;
  const x1 = x0 < maxX ? x0 + 1 : maxX;
  const y1 = y0 < maxY ? y0 + 1 : maxY;
  const tx = fx - x0;
  const ty = fy - y0;

  const w00 = (1 - tx) * (1 - ty);
  const w10 = tx * (1 - ty);
  const w01 = (1 - tx) * ty;
  const w11 = tx * ty;

  const i00 = y0 * w + x0;
  const i10 = y0 * w + x1;
  const i01 = y1 * w + x0;
  const i11 = y1 * w + x1;

  const baseA = fieldA.baseLum;
  const baseB = fieldB.baseLum;
  const inkArrA = fieldA.inkMask;
  const inkArrB = fieldB.inkMask;
  const lumA = baseA[i00] * w00 + baseA[i10] * w10 + baseA[i01] * w01 + baseA[i11] * w11;
  const lumB = baseB[i00] * w00 + baseB[i10] * w10 + baseB[i01] * w01 + baseB[i11] * w11;
  const inkA = inkArrA[i00] * w00 + inkArrA[i10] * w10 + inkArrA[i01] * w01 + inkArrA[i11] * w11;
  const inkB = inkArrB[i00] * w00 + inkArrB[i10] * w10 + inkArrB[i01] * w01 + inkArrB[i11] * w11;
  const blendT = smoothstep(0.06, 0.94, pose.mouthBlend);

  let lum = lerp(lumA, lumB, blendT);
  const ink = lerp(inkA, inkB, blendT);

  // Preserve crisp dark oral cavity & lip line contrast during speaking crossfades
  if (
    pose.mouthIndexA !== pose.mouthIndexB &&
    blendT > 0.04 &&
    blendT < 0.96 &&
    ax >= 0.22 &&
    ax <= 0.44 &&
    ay >= 0.56 &&
    ay <= 0.72
  ) {
    if (inkA > 0.50 || inkB > 0.50) {
      lum = lerp(lum, Math.min(lumA, lumB), 0.52);
    }
  }

  // Co-articulated upper eye/brow band: continuous 3-stage blend (open -> half-lid -> closed)
  // aligned cleanly to real sprite-bank eye & lash coordinates (ay ~ 0.42..0.555).
  if ((pose.eyeBlend > 0.015 || pose.blinkAmount > 0.04) && ax >= 0.16 && ax <= 0.53 && ay >= 0.38 && ay <= 0.575) {
    const exMask = smoothstep(0.165, 0.198, ax) * (1 - smoothstep(0.475, 0.525, ax));
    const eyMask = smoothstep(0.38, 0.415, ay) * (1 - smoothstep(0.545, 0.575, ay));
    const socketMask = exMask * eyMask;

    if (socketMask > 0.01) {
      let targetEyeLum = lum;
      if (pose.eyeHalfWeight > 0.01 || pose.eyeClosedWeight > 0.01) {
        const halfField = bank[clamp(pose.eyeHalfIndex, 0, bank.length - 1)];
        const closedField = bank[clamp(pose.eyeClosedIndex, 0, bank.length - 1)];
        const halfBase = halfField.baseLum;
        const closedBase = closedField.baseLum;
        const halfLum = halfBase[i00] * w00 + halfBase[i10] * w10 + halfBase[i01] * w01 + halfBase[i11] * w11;
        const closedLum = closedBase[i00] * w00 + closedBase[i10] * w10 + closedBase[i01] * w01 + closedBase[i11] * w11;

        const totalBlinkW = clamp01(pose.eyeHalfWeight + pose.eyeClosedWeight);
        const stageMix =
          totalBlinkW > 0.001
            ? lerp(halfLum, closedLum, pose.eyeClosedWeight / totalBlinkW)
            : lum;
        targetEyeLum = lerp(lum, stageMix, totalBlinkW);

        // Preserve crisp half-lid & closed eyelash ink contrast during 3-stage crossfades
        if (pose.eyeClosedWeight > 0.35) {
          const closedInkArr = closedField.inkMask;
          const closedInk =
            closedInkArr[i00] * w00 + closedInkArr[i10] * w10 + closedInkArr[i01] * w01 + closedInkArr[i11] * w11;
          if (closedInk > 0.48) {
            targetEyeLum = lerp(targetEyeLum, Math.min(targetEyeLum, closedLum), pose.eyeClosedWeight * 0.55);
          }
        } else if (pose.eyeHalfWeight > 0.35) {
          const halfInkArr = halfField.inkMask;
          const halfInk =
            halfInkArr[i00] * w00 + halfInkArr[i10] * w10 + halfInkArr[i01] * w01 + halfInkArr[i11] * w11;
          if (halfInk > 0.48) {
            targetEyeLum = lerp(targetEyeLum, Math.min(targetEyeLum, halfLum), pose.eyeHalfWeight * 0.45);
          }
        }
      } else if (pose.eyeBlend > 0.01) {
        const eyeBase = bank[clamp(pose.eyeIndex, 0, bank.length - 1)].baseLum;
        targetEyeLum = lerp(
          lum,
          eyeBase[i00] * w00 + eyeBase[i10] * w10 + eyeBase[i01] * w01 + eyeBase[i11] * w11,
          pose.eyeBlend
        );
      }

      lum = lerp(lum, targetEyeLum, socketMask);
    }
  }

  // Subtle studio key-light cheek shading shift during head turn (no pixel warp)
  if (ink < 0.35 && fc.hasHeadYawShading && ay > 0.36 && ay < 0.76 && ax > 0.18 && ax < 0.52) {
    const cheekArea = smoothstep(0.19, 0.48, ax) * smoothstep(0.38, 0.65, ay) * (1 - smoothstep(0.65, 0.76, ay));
    lum = clamp01(lum + motion.headYaw * 0.12 * cheekArea);
  }

  // Traveling headband shimmer during speaking/listening
  const aura = motion.listenPulse;
  if (aura > 0.06 && ink < 0.35 && ay < 0.28 && lum > 0.4) {
    const gx = (ax - fc.glintSweep) * 7.5;
    const gSq = gx * gx;
    if (gSq < 8) {
      lum = clamp01(lum + fastExpNeg(gSq) * aura * (0.15 + energy * 0.16));
    }
  }

  // Soft bust bottom & outer edge feathering into sparse Braille halftone stippling
  const bottomFeather = 1 - smoothstep(0.83, 0.98, ay);
  const rightFeather = 1 - smoothstep(0.86, 0.985, ax);
  const leftFeather = smoothstep(0.015, 0.085, ax);
  lum *= bottomFeather * rightFeather * leftFeather;

  // Subtle animated Braille acoustic waveform halo outside silhouette when speaking or listening
  if (fc.hasWaveform && lum < 0.04 && ay < 0.86) {
    const adx = ax - 0.47;
    const ady = (ay - 0.43) * 0.92;
    const adistSq = adx * adx + ady * ady;
    if (adistSq > 0.172225 && adistSq < 0.245025) {
      const adist = Math.sqrt(adistSq);
      const invR = 1 / adist;
      const u = adx * invR;
      const v = ady * invR;
      const u2 = u * u - v * v;
      const v2 = 2 * u * v;
      const u3 = u * (4 * u * u - 3);
      const v3 = v * (3 - 4 * v * v);
      const u4 = u2 * u2 - v2 * v2;
      const v4 = 2 * v2 * u2;
      const u5 = u2 * u3 - v2 * v3;
      const v5 = v2 * u3 + u2 * v3;
      const waveR =
        0.445 +
        (v5 * fc.cosT38 + u5 * fc.sinT38) * fc.waveAmp +
        (u3 * fc.cosT24 + v3 * fc.sinT24) * 0.008;
      const ringBand = Math.abs(adist - waveR);
      if (ringBand < 0.014) {
        const arcPulse = 0.5 + 0.5 * (v4 * fc.cosT45 - u4 * fc.sinT45);
        const sideWeight = 0.35 + 0.65 * (u < 0 ? -u : u);
        const env =
          (1 - smoothstep(0.002, 0.014, ringBand)) *
          sideWeight *
          (0.28 + arcPulse * 0.48) *
          fc.waveGain;
        lum = Math.max(lum, env * (1 - smoothstep(0.74, 0.86, ay)));
      }
    }
  }

  return clamp01(lum);
}

/**
 * Pure TypeScript mathematical fallback portrait field (used in headless tests
 * and for the first millisecond before the reference PNG loads).
 */
export function samplePortraitLuminance(nx: number, ny: number, params: PortraitParams): number {
  const { time, mood, viseme, eyeState, energy, isSpeaking, isListening } = params;

  const swayX = Math.sin(time * 1.35) * 0.01 + (isSpeaking ? Math.sin(time * 5.2) * 0.008 * energy : 0);
  const swayY = Math.cos(time * 1.9) * 0.009 - (isSpeaking ? Math.abs(Math.sin(time * 6.8)) * 0.012 * energy : 0);
  const tilt = mood === 'thinking' ? 0.055 : mood === 'happy' ? -0.025 : 0.015;
  const ct = Math.cos(tilt);
  const st = Math.sin(tilt);

  const rx0 = nx - swayX;
  const ry0 = ny - swayY;
  const x = rx0 * ct - ry0 * st;
  const y = rx0 * st + ry0 * ct;

  const yawShift = params.motion ? params.motion.headYaw * 0.14 : Math.sin(time * 0.72) * 0.02;
  const fx = x + 0.11 - yawShift;
  const fy = y + 0.02;

  const dCranium = sdEllipse(fx, fy, 0.0, -0.08, 0.34, 0.41);
  const dJaw = sdEllipse(fx, fy, -0.04, 0.19, 0.22, 0.25);
  const dFace = Math.min(dCranium, (dCranium + dJaw) * 0.5);

  const dNeck = sdCapsule2(x, y, 0.01, 0.32, 0.08, 0.84, 0.125);
  const dCollar = sdEllipse(x, y, 0.06, 0.78, 0.42, 0.18);
  const dBust = Math.min(dNeck, dCollar);

  const dHairCrown = sdEllipse(x, y, 0.03, -0.18, 0.5, 0.48);
  const dHairBackR = sdCapsule2(x, y, 0.28, -0.14, 0.46, 0.62, 0.17);
  const dCurlL = sdCapsule2(x, y, -0.35, 0.12, -0.52, 0.58, 0.1);
  const dBangL = sdCapsule2(x, y, -0.02, -0.56, -0.44, -0.14, 0.11);
  const dBangC = sdCapsule2(x, y, 0.04, -0.54, -0.12, -0.12, 0.09);

  const dHair = Math.min(
    Math.min(dHairCrown, dHairBackR),
    Math.min(dCurlL, Math.min(dBangL, dBangC))
  );

  const dHeadband = Math.abs(sdEllipse(x, y, 0.01, -0.02, 0.32, 0.55)) - 0.038;

  const radialFade = 1 - smoothstep(0.72, 1.0, Math.hypot(x * 0.88, y * 0.92));
  const bottomFade = 1 - smoothstep(0.55, 0.94, y);

  let lum = 0;

  if (dHair < 0.065) {
    const hairMask = 1 - smoothstep(-0.02, 0.05, dHair);
    const rim = smoothstep(-0.055, 0.0, dHair) * (1 - smoothstep(0.0, 0.045, dHair));
    const zs = (y + 0.36 + Math.sin(x * 22) * 0.025) * 8;
    const zigSheen = fastExpNeg(zs * zs) * 0.65;
    lum = Math.max(lum, hairMask * (0.05 + rim * 0.38 + zigSheen));
  }

  if (dHeadband < 0.02 && y < -0.08 && x > -0.25 && x < 0.32) {
    const hbMask = 1 - smoothstep(-0.008, 0.015, dHeadband);
    const pulse = isSpeaking || isListening ? 0.08 * energy : 0;
    lum = Math.max(lum, hbMask * (0.94 + pulse));
  }

  if (dBust < 0.045) {
    const bustMask = 1 - smoothstep(-0.015, 0.03, dBust);
    lum = Math.max(lum, bustMask * 0.68);
  }

  if (dFace < 0.035) {
    const faceMask = 1 - smoothstep(-0.018, 0.022, dFace);
    const keyLight = clamp01(0.95 - (fx + 0.05) * 0.85);
    let skinLum = 0.32 + keyLight * 0.68;

    const eyeH =
      eyeState === 'blink' ? 0.006 : eyeState === 'half' ? 0.019 : eyeState === 'wide' ? 0.044 : 0.035;
    const dEyeL = sdEllipse(fx, fy, -0.15, -0.02, 0.046, eyeH);
    const dEyeR = sdEllipse(fx, fy, 0.04, -0.03, 0.064, eyeH);
    const dEyes = Math.min(dEyeL, dEyeR);

    if (dEyes < 0.018) {
      const eMask = 1 - smoothstep(-0.005, 0.014, dEyes);
      skinLum *= 1 - eMask * 0.94;
      if (eyeState !== 'blink') {
        const dCatch = Math.min(
          sdEllipse(fx, fy, -0.158, -0.03, 0.015, 0.014),
          sdEllipse(fx, fy, 0.028, -0.04, 0.018, 0.016)
        );
        skinLum = Math.max(skinLum, (1 - smoothstep(-0.003, 0.008, dCatch)) * 0.98);
      }
    }

    const mouth = getMouthShape(viseme, energy);
    const dMouth = sdEllipse(fx, fy, -0.095, 0.19, mouth.rx * 1.4, mouth.ry * 1.4);
    if (dMouth < 0.02) {
      const mMask = 1 - smoothstep(-0.005, 0.01, dMouth);
      skinLum *= 1 - mMask * (mouth.open > 0.3 ? 0.95 : 0.85);
    }

    lum = lum * (1 - faceMask) + clamp01(skinLum) * faceMask;
  }

  return clamp01(lum * radialFade * bottomFade);
}

// Reusable typed scratch buffers for Braille subpixel dithering to avoid GC pressure at 60fps
let cachedSubLen = 0;
let rawSubBuf = new Float32Array(0);
let workSubBuf = new Float32Array(0);
let edgeSubBuf = new Float32Array(0);
let portraitRowChars: string[] = [];

function ensureSubBuffers(len: number, cols: number) {
  if (len > cachedSubLen) {
    cachedSubLen = len;
    rawSubBuf = new Float32Array(len);
    workSubBuf = new Float32Array(len);
    edgeSubBuf = new Float32Array(len);
  }
  if (portraitRowChars.length !== cols) {
    portraitRowChars = new Array<string>(cols);
  }
}

/**
 * Renders a monochrome Unicode Braille (U+2800..U+28FF) halftone ASCII portrait frame
 * using 2x4 subpixel 16x16 Void-and-Cluster Blue-Noise + Edge-Aware Contour-Lock Thresholding.
 */
export function renderHalftonePortrait(params: PortraitParams): PortraitFrame {
  const { cols, rows, sourceField, spriteBank, viseme, mood, isSpeaking } = params;
  const cells = new Uint8Array(cols * rows);
  const lines: string[] = new Array(rows);
  const aspect = (cols * 0.53) / Math.max(1, rows);
  const hasBank = Boolean(spriteBank && spriteBank.length >= 12);
  const fc = hasBank || sourceField ? buildReferenceFrameConstants(params) : null;
  const poseBlend =
    hasBank && fc ? resolveReferencePoseBlend(fc.motion, viseme, mood, isSpeaking) : null;

  const subW = cols * 2;
  const subH = rows * 4;
  ensureSubBuffers(subW * subH, cols);
  const raw = rawSubBuf;
  const work = workSubBuf;
  const edge = edgeSubBuf;
  const rowChars = portraitRowChars;
  const invSubW = 1 / subW;
  const invSubH = 1 / subH;

  let activeMinX = subW;
  let activeMaxX = -1;
  let activeMinY = subH;
  let activeMaxY = -1;

  // 1. Sample 2x4 subpixel luminance buffer directly from 12-pose sprite bank, single reference, or 2D fallback
  if (spriteBank && poseBlend && fc && hasBank) {
    const sbBounds = getSourceFieldBounds(spriteBank[0]);
    const xMinFramed = fc.hasWaveform ? 0.0 : Math.max(0.01, sbBounds.minUx - 0.11);
    const xMaxFramed = fc.hasWaveform ? 0.99 : Math.min(0.99, sbBounds.maxUx + 0.11);
    const sxStart = Math.max(0, Math.floor((((xMinFramed - 0.50) / 1.088 + 0.50) * subW) - 1));
    const sxEnd = Math.min(subW, Math.ceil((((xMaxFramed - 0.50) / 1.088 + 0.50) * subW) + 1));

    for (let sy = 0; sy < subH; sy++) {
      const uy = (sy + 0.5) * invSubH;
      const rowOff = sy * subW;
      const framedY = (uy - 0.54) * 1.095 + 0.51;
      if (framedY < 0.005 || framedY > 0.995) {
        raw.fill(0, rowOff, rowOff + subW);
        continue;
      }
      if (sxStart > 0) raw.fill(0, rowOff, rowOff + sxStart);
      if (sxEnd < subW) raw.fill(0, rowOff + sxEnd, rowOff + subW);
      for (let sx = sxStart; sx < sxEnd; sx++) {
        const ux = (sx + 0.5) * invSubW;
        const val = sampleSpriteBankWithExpression(ux, uy, fc, spriteBank, poseBlend);
        raw[rowOff + sx] = val;
        if (val > 0.025) {
          if (sx < activeMinX) activeMinX = sx;
          if (sx > activeMaxX) activeMaxX = sx;
          if (sy < activeMinY) activeMinY = sy;
          if (sy > activeMaxY) activeMaxY = sy;
        }
      }
    }
  } else if (sourceField && fc) {
    const sfBounds = getSourceFieldBounds(sourceField);
    const xMinFramed = fc.hasWaveform ? 0.0 : Math.max(0.01, sfBounds.minUx - 0.11);
    const xMaxFramed = fc.hasWaveform ? 0.99 : Math.min(0.99, sfBounds.maxUx + 0.11);
    const sxStart = Math.max(0, Math.floor((((xMinFramed - 0.49) / 1.115 + 0.50) * subW) - 1));
    const sxEnd = Math.min(subW, Math.ceil((((xMaxFramed - 0.49) / 1.115 + 0.50) * subW) + 1));

    for (let sy = 0; sy < subH; sy++) {
      const uy = (sy + 0.5) * invSubH;
      const rowOff = sy * subW;
      const framedY = (uy - 0.56) * 1.12 + 0.51;
      if (framedY < 0.005 || framedY > 0.995) {
        raw.fill(0, rowOff, rowOff + subW);
        continue;
      }
      if (sxStart > 0) raw.fill(0, rowOff, rowOff + sxStart);
      if (sxEnd < subW) raw.fill(0, rowOff + sxEnd, rowOff + subW);
      for (let sx = sxStart; sx < sxEnd; sx++) {
        const ux = (sx + 0.5) * invSubW;
        const val = sampleReferenceWithExpression(ux, uy, fc, sourceField);
        raw[rowOff + sx] = val;
        if (val > 0.025) {
          if (sx < activeMinX) activeMinX = sx;
          if (sx > activeMaxX) activeMaxX = sx;
          if (sy < activeMinY) activeMinY = sy;
          if (sy > activeMaxY) activeMaxY = sy;
        }
      }
    }
  } else {
    for (let sy = 0; sy < subH; sy++) {
      const uy = (sy + 0.5) * invSubH;
      const ny = uy * 2 - 1;
      const rowOff = sy * subW;
      for (let sx = 0; sx < subW; sx++) {
        const ux = (sx + 0.5) * invSubW;
        const nx = (ux * 2 - 1) * aspect;
        const val = samplePortraitLuminance(nx, ny, params);
        raw[rowOff + sx] = val;
        if (val > 0.025) {
          if (sx < activeMinX) activeMinX = sx;
          if (sx > activeMaxX) activeMaxX = sx;
          if (sy < activeMinY) activeMinY = sy;
          if (sy > activeMaxY) activeMaxY = sy;
        }
      }
    }
  }

  work.fill(0, 0, subW * subH);
  edge.fill(0, 0, subW * subH);

  if (activeMaxX >= activeMinX && activeMaxY >= activeMinY) {
    const syLo = activeMinY > 1 ? activeMinY - 1 : 0;
    const syHi = activeMaxY + 1 < subH ? activeMaxY + 1 : subH - 1;
    const sxLo = activeMinX > 1 ? activeMinX - 1 : 0;
    const sxHi = activeMaxX + 1 < subW ? activeMaxX + 1 : subW - 1;

    // 2. Contour-preserving unsharp contrast boost + edge-lock weights for crisp eyes/mouth/teeth
    for (let sy = syLo; sy <= syHi; sy++) {
      const rowOff = sy * subW;
      const upOff = (sy > 0 ? sy - 1 : 0) * subW;
      const dnOff = (sy + 1 < subH ? sy + 1 : subH - 1) * subW;
      const isVertInterior = sy > 0 && sy + 1 < subH && sxLo > 0 && sxHi + 1 < subW;

      if (isVertInterior) {
        for (let sx = sxLo; sx <= sxHi; sx++) {
          const idx = rowOff + sx;
          const c = raw[idx];
          if (c <= 0.03) {
            work[idx] = 0;
            continue;
          }
          if (c >= 0.94) {
            work[idx] = 1;
            continue;
          }
          const avg = (raw[idx - 1] + raw[idx + 1] + raw[upOff + sx] + raw[dnOff + sx]) * 0.25;
          const diff = c - avg;
          const absDiff = diff < 0 ? -diff : diff;
          edge[idx] = absDiff > 0.045 ? Math.min(0.64, (absDiff - 0.045) * 3.2) : 0;
          const sharpened = c + diff * 0.50;
          let t = (sharpened - 0.03) * 1.0752688;
          if (t <= 0) t = 0;
          else if (t >= 1) t = 1;
          else t = t * 0.42 + t * t * (3 - 2 * t) * 0.58;
          work[idx] = t;
        }
      } else {
        for (let sx = sxLo; sx <= sxHi; sx++) {
          const idx = rowOff + sx;
          const c = raw[idx];
          if (c <= 0.03) {
            work[idx] = 0;
            continue;
          }
          if (c >= 0.94) {
            work[idx] = 1;
            continue;
          }
          const l = raw[rowOff + (sx > 0 ? sx - 1 : 0)];
          const r = raw[rowOff + (sx + 1 < subW ? sx + 1 : subW - 1)];
          const u = raw[upOff + sx];
          const d = raw[dnOff + sx];
          const avg = (l + r + u + d) * 0.25;
          const diff = c - avg;
          const absDiff = diff < 0 ? -diff : diff;
          edge[idx] = absDiff > 0.045 ? Math.min(0.64, (absDiff - 0.045) * 3.2) : 0;
          const sharpened = c + diff * 0.50;
          let t = (sharpened - 0.03) * 1.0752688;
          if (t <= 0) t = 0;
          else if (t >= 1) t = 1;
          else t = t * 0.42 + t * t * (3 - 2 * t) * 0.58;
          work[idx] = t;
        }
      }
    }
  }

  // 3. Deterministic Ordered 16x16 Blue-Noise + Edge-Aware Contour-Lock Halftone Packing
  const subLum = new Uint8Array(subW * subH);
  const rLo = activeMaxY >= activeMinY ? Math.max(0, activeMinY >> 2) : rows;
  const rHi = activeMaxY >= activeMinY ? Math.min(rows - 1, (activeMaxY + 1) >> 2) : -1;
  const cLo = activeMaxX >= activeMinX ? Math.max(0, (activeMinX - 1) >> 1) : cols;
  const cHi = activeMaxX >= activeMinX ? Math.min(cols - 1, (activeMaxX + 1) >> 1) : -1;
  const emptyRowStr = ' '.repeat(cols);

  for (let r = 0; r < rows; r++) {
    const cellRowOff = r * cols;
    if (r < rLo || r > rHi || cHi < cLo) {
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
    const bRow0 = (syBase & 15) << 4;
    const bRow1 = ((syBase + 1) & 15) << 4;
    const bRow2 = ((syBase + 2) & 15) << 4;
    const bRow3 = ((syBase + 3) & 15) << 4;

    for (let c = cLo; c <= cHi; c++) {
      const c2 = c * 2;
      const lumSum =
        raw[r0 + c2] +
        raw[r0 + c2 + 1] +
        raw[r1 + c2] +
        raw[r1 + c2 + 1] +
        raw[r2 + c2] +
        raw[r2 + c2 + 1] +
        raw[r3 + c2] +
        raw[r3 + c2 + 1];

      if (lumSum <= 0.04) {
        cells[cellRowOff + c] = 0;
        rowChars[c] = ' ';
        continue;
      }

      const i00 = r0 + c2;
      const i01 = i00 + 1;
      const i10 = r1 + c2;
      const i11 = i10 + 1;
      const i20 = r2 + c2;
      const i21 = i20 + 1;
      const i30 = r3 + c2;
      const i31 = i30 + 1;

      const w00 = work[i00];
      const w01 = work[i01];
      const w10 = work[i10];
      const w11 = work[i11];
      const w20 = work[i20];
      const w21 = work[i21];
      const w30 = work[i30];
      const w31 = work[i31];

      const bx0 = c2 & 15;
      const bx1 = (c2 + 1) & 15;
      let bits = 0;
      const t00 = BLUE_THRESH_16X16[bRow0 + bx0];
      if (w00 >= t00 + (0.5 - t00) * edge[i00]) {
        bits |= 0x01;
        subLum[i00] = (w00 * 255 + 0.5) | 0;
      }
      const t01 = BLUE_THRESH_16X16[bRow0 + bx1];
      if (w01 >= t01 + (0.5 - t01) * edge[i01]) {
        bits |= 0x08;
        subLum[i01] = (w01 * 255 + 0.5) | 0;
      }
      const t10 = BLUE_THRESH_16X16[bRow1 + bx0];
      if (w10 >= t10 + (0.5 - t10) * edge[i10]) {
        bits |= 0x02;
        subLum[i10] = (w10 * 255 + 0.5) | 0;
      }
      const t11 = BLUE_THRESH_16X16[bRow1 + bx1];
      if (w11 >= t11 + (0.5 - t11) * edge[i11]) {
        bits |= 0x10;
        subLum[i11] = (w11 * 255 + 0.5) | 0;
      }
      const t20 = BLUE_THRESH_16X16[bRow2 + bx0];
      if (w20 >= t20 + (0.5 - t20) * edge[i20]) {
        bits |= 0x04;
        subLum[i20] = (w20 * 255 + 0.5) | 0;
      }
      const t21 = BLUE_THRESH_16X16[bRow2 + bx1];
      if (w21 >= t21 + (0.5 - t21) * edge[i21]) {
        bits |= 0x20;
        subLum[i21] = (w21 * 255 + 0.5) | 0;
      }
      const t30 = BLUE_THRESH_16X16[bRow3 + bx0];
      if (w30 >= t30 + (0.5 - t30) * edge[i30]) {
        bits |= 0x40;
        subLum[i30] = (w30 * 255 + 0.5) | 0;
      }
      const t31 = BLUE_THRESH_16X16[bRow3 + bx1];
      if (w31 >= t31 + (0.5 - t31) * edge[i31]) {
        bits |= 0x80;
        subLum[i31] = (w31 * 255 + 0.5) | 0;
      }

      cells[cellRowOff + c] = (lumSum * 31.875 + 0.5) | 0;
      rowChars[c] = BRAILLE_CHAR_TABLE[bits];
    }
    lines[r] = rowChars.join('');
  }

  return { cols, rows, lines, cells, subLum };
}
