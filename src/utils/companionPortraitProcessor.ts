import type { ExpressionMood, MouthViseme } from '../types/hermes';
import type { PortraitSourceField } from './asciiPortraitEngine';
import { BLUE_NOISE_16X16_THRESH } from './brailleCanvasRaster';
import type {
  BrailleShapeFrame,
  BrailleShapeKind,
  BrailleShapeMotionState,
} from './brailleShapesEngine';

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
  const t = clamp01((x - edge0) / (edge1 - edge0));
  return t * t * (3 - 2 * t);
}

function sdEllipse(px: number, py: number, cx: number, cy: number, rx: number, ry: number): number {
  const dx = (px - cx) / rx;
  const dy = (py - cy) / ry;
  const r = Math.hypot(dx, dy);
  return (r - 1) * Math.min(rx, ry);
}

function sdCapsule2(
  px: number,
  py: number,
  ax: number,
  ay: number,
  bx: number,
  by: number,
  radius: number
): number {
  const pax = px - ax;
  const pay = py - ay;
  const bax = bx - ax;
  const bay = by - ay;
  const denom = bax * bax + bay * bay;
  const h = denom > 1e-8 ? clamp01((pax * bax + pay * bay) / denom) : 0;
  return Math.hypot(pax - bax * h, pay - bay * h) - radius;
}

const BRAILLE_CHAR_TABLE: string[] = Array.from({ length: 256 }, (_, i) =>
  i === 0 ? ' ' : String.fromCharCode(0x2800 + i)
);

type PortraitFacing = 'left' | 'center' | 'right';

export interface CompanionEyeAnchor {
  cx: number;
  cy: number;
  rx: number;
  ry: number;
  skinTone: number;
  /** True when the base illustration already draws this eye closed/winking (e.g. Twintails left eye). */
  winking?: boolean;
}

export interface CompanionMouthAnchor {
  cx: number;
  cy: number;
  rx: number;
  ry: number;
  chinY: number;
  /** True when base artwork has an already-open happy smile (e.g. Twintails). */
  alreadyOpen?: boolean;
}

export interface CompanionPortraitProfile {
  facing: PortraitFacing;
  hollowSeeds?: ReadonlyArray<{ ux: number; uy: number }>;
  boostGarmentTrim?: boolean;
  /** Grounded neck/collar pivot in normalized portrait UV (0..1) so head articulates without floating */
  pivotX: number;
  pivotY: number;
  /** Vertical UV where shoulders/collar anchor gently into grounded posture */
  shoulderLockY: number;
  /** Subtle poised breath & conversational head cadence (zero floating balloon bob) */
  breathRate: number;
  tiltRate: number;
  tiltAmp: number;
  /** Calibrated anime facial anatomy from grid measurement */
  leftEye: CompanionEyeAnchor;
  rightEye: CompanionEyeAnchor;
  browY: number;
  mouth: CompanionMouthAnchor;
}

export const COMPANION_PORTRAIT_PROFILES: Record<BrailleShapeKind, CompanionPortraitProfile> = {
  clover: {
    facing: 'left',
    pivotX: 0.48,
    pivotY: 0.80,
    shoulderLockY: 0.78,
    breathRate: 1.55,
    tiltRate: 1.05,
    tiltAmp: 0.024,
    leftEye: { cx: 0.215, cy: 0.456, rx: 0.044, ry: 0.036, skinTone: 0.72 },
    rightEye: { cx: 0.388, cy: 0.448, rx: 0.068, ry: 0.040, skinTone: 0.68 },
    browY: 0.378,
    mouth: { cx: 0.330, cy: 0.650, rx: 0.056, ry: 0.044, chinY: 0.775 },
  },
  star: {
    facing: 'right',
    pivotX: 0.52,
    pivotY: 0.82,
    shoulderLockY: 0.79,
    breathRate: 1.85,
    tiltRate: 1.38,
    tiltAmp: 0.036,
    leftEye: { cx: 0.488, cy: 0.480, rx: 0.054, ry: 0.036, skinTone: 0.72, winking: true },
    rightEye: { cx: 0.676, cy: 0.464, rx: 0.064, ry: 0.060, skinTone: 0.70 },
    browY: 0.376,
    mouth: { cx: 0.625, cy: 0.658, rx: 0.060, ry: 0.046, chinY: 0.775, alreadyOpen: true },
  },
  ghost: {
    facing: 'center',
    pivotX: 0.46,
    pivotY: 0.84,
    shoulderLockY: 0.82,
    breathRate: 1.70,
    tiltRate: 1.25,
    tiltAmp: 0.034,
    leftEye: { cx: 0.352, cy: 0.522, rx: 0.058, ry: 0.046, skinTone: 0.72 },
    rightEye: { cx: 0.545, cy: 0.525, rx: 0.060, ry: 0.046, skinTone: 0.72 },
    browY: 0.435,
    mouth: { cx: 0.448, cy: 0.695, rx: 0.054, ry: 0.042, chinY: 0.808 },
  },
  heart: {
    facing: 'right',
    pivotX: 0.52,
    pivotY: 0.83,
    shoulderLockY: 0.80,
    breathRate: 1.50,
    tiltRate: 0.96,
    tiltAmp: 0.026,
    leftEye: { cx: 0.515, cy: 0.522, rx: 0.056, ry: 0.040, skinTone: 0.72 },
    rightEye: { cx: 0.650, cy: 0.515, rx: 0.044, ry: 0.036, skinTone: 0.68 },
    browY: 0.450,
    mouth: { cx: 0.608, cy: 0.665, rx: 0.050, ry: 0.040, chinY: 0.785 },
  },
  cloud: {
    facing: 'left',
    pivotX: 0.52,
    pivotY: 0.84,
    shoulderLockY: 0.82,
    breathRate: 1.78,
    tiltRate: 1.22,
    tiltAmp: 0.030,
    leftEye: { cx: 0.360, cy: 0.562, rx: 0.042, ry: 0.050, skinTone: 0.72 },
    rightEye: { cx: 0.512, cy: 0.568, rx: 0.054, ry: 0.052, skinTone: 0.70 },
    browY: 0.482,
    mouth: { cx: 0.448, cy: 0.695, rx: 0.048, ry: 0.038, chinY: 0.808 },
  },
  triangle: {
    facing: 'right',
    pivotX: 0.54,
    pivotY: 0.80,
    shoulderLockY: 0.78,
    breathRate: 1.45,
    tiltRate: 0.92,
    tiltAmp: 0.020,
    leftEye: { cx: 0.592, cy: 0.466, rx: 0.060, ry: 0.036, skinTone: 0.70 },
    rightEye: { cx: 0.735, cy: 0.468, rx: 0.044, ry: 0.034, skinTone: 0.66 },
    browY: 0.405,
    mouth: { cx: 0.650, cy: 0.645, rx: 0.052, ry: 0.040, chinY: 0.768 },
  },
  pebble: {
    facing: 'left',
    boostGarmentTrim: true,
    pivotX: 0.52,
    pivotY: 0.76,
    shoulderLockY: 0.74,
    breathRate: 1.25,
    tiltRate: 0.78,
    tiltAmp: 0.022,
    leftEye: { cx: 0.318, cy: 0.445, rx: 0.045, ry: 0.040, skinTone: 0.72 },
    rightEye: { cx: 0.488, cy: 0.452, rx: 0.050, ry: 0.042, skinTone: 0.70 },
    browY: 0.365,
    mouth: { cx: 0.402, cy: 0.576, rx: 0.036, ry: 0.025, chinY: 0.646 },
  },
  trapezoid: {
    facing: 'center',
    hollowSeeds: [{ ux: 0.50, uy: 0.066 }],
    boostGarmentTrim: true,
    pivotX: 0.50,
    pivotY: 0.76,
    shoulderLockY: 0.74,
    breathRate: 1.38,
    tiltRate: 0.84,
    tiltAmp: 0.022,
    leftEye: { cx: 0.402, cy: 0.478, rx: 0.054, ry: 0.034, skinTone: 0.72 },
    rightEye: { cx: 0.598, cy: 0.478, rx: 0.054, ry: 0.034, skinTone: 0.72 },
    browY: 0.412,
    mouth: { cx: 0.500, cy: 0.604, rx: 0.048, ry: 0.036, chinY: 0.708 },
  },
  blob: {
    facing: 'right',
    pivotX: 0.50,
    pivotY: 0.80,
    shoulderLockY: 0.78,
    breathRate: 1.65,
    tiltRate: 1.28,
    tiltAmp: 0.032,
    leftEye: { cx: 0.515, cy: 0.458, rx: 0.058, ry: 0.038, skinTone: 0.72 },
    rightEye: { cx: 0.680, cy: 0.458, rx: 0.046, ry: 0.036, skinTone: 0.68 },
    browY: 0.398,
    mouth: { cx: 0.618, cy: 0.648, rx: 0.056, ry: 0.042, chinY: 0.758 },
  },
};

/**
 * Converts a black-ink-on-white (or transparent) manga portrait RGBA buffer into a
 * sculpted facing-aware chiaroscuro luminance field matching the main Hermes character's
 * dark-mode halftone aesthetic, with leak-proof neck/collar floor protection and hollow
 * halo cutout for Seraph.
 */
export function buildCompanionPortraitSourceFromRgba(
  rgba: Uint8ClampedArray,
  width: number,
  height: number,
  kind: BrailleShapeKind
): PortraitSourceField {
  const total = width * height;
  const outside = new Uint8Array(total);
  const queue = new Int32Array(total);
  let qHead = 0;
  let qTail = 0;

  const profile = COMPANION_PORTRAIT_PROFILES[kind] ?? COMPANION_PORTRAIT_PROFILES.clover;

  const isBgPixel = (idx: number): boolean => {
    const off = idx * 4;
    const a = rgba[off + 3];
    if (a < 42) return true;
    const r = rgba[off];
    const g = rgba[off + 1];
    const b = rgba[off + 2];
    return r > 232 && g > 232 && b > 232;
  };

  const pushOutside = (x: number, y: number) => {
    const idx = y * width + x;
    if (!outside[idx] && isBgPixel(idx)) {
      outside[idx] = 1;
      queue[qTail++] = idx;
    }
  };

  for (let x = 0; x < width; x++) {
    pushOutside(x, 0);
    if (x < width * 0.22 || x > width * 0.90) {
      pushOutside(x, height - 1);
    }
  }
  for (let y = 0; y < height; y++) {
    pushOutside(0, y);
    pushOutside(width - 1, y);
  }

  if (profile.hollowSeeds) {
    for (const seed of profile.hollowSeeds) {
      const sx = Math.round(width * seed.ux);
      const sy = Math.round(height * seed.uy);
      if (sx >= 0 && sx < width && sy >= 0 && sy < height) {
        pushOutside(sx, sy);
      }
    }
  }

  const floorGuardY = Math.floor(height * 0.87);
  const floorGuardX0 = Math.floor(width * 0.28);
  const floorGuardX1 = Math.floor(width * 0.88);

  while (qHead < qTail) {
    const p = queue[qHead++];
    const x = p % width;
    const y = (p / width) | 0;
    if (x > 0) pushOutside(x - 1, y);
    if (x + 1 < width) pushOutside(x + 1, y);
    if (y + 1 < height) pushOutside(x, y + 1);
    if (y > 0) {
      if (!(y >= floorGuardY && x >= floorGuardX0 && x <= floorGuardX1)) {
        pushOutside(x, y - 1);
      }
    }
  }

  const baseLum = new Float32Array(total);
  const inkMask = new Float32Array(total);
  const facing = profile.facing;
  const boostTrim = Boolean(profile.boostGarmentTrim);

  for (let y = 0; y < height; y++) {
    const uy = y / Math.max(1, height - 1);
    const bottomFade = 1 - smoothstep(0.88, 0.995, uy);

    for (let x = 0; x < width; x++) {
      const ux = x / Math.max(1, width - 1);
      const idx = y * width + x;

      if (outside[idx]) {
        baseLum[idx] = 0;
        inkMask[idx] = 0;
        continue;
      }

      const off = idx * 4;
      const alpha = rgba[off + 3] / 255;
      const r = rgba[off];
      const g = rgba[off + 1];
      const b = rgba[off + 2];
      const rgbBright = (0.299 * r + 0.587 * g + 0.114 * b) / 255;
      const ink = alpha * (1 - rgbBright * 0.88);
      inkMask[idx] = ink;

      const nearOut =
        (x > 0 && outside[idx - 1]) ||
        (x + 1 < width && outside[idx + 1]) ||
        (y > 0 && outside[idx - width]) ||
        (y + 1 < height && outside[idx + width]) ||
        (x > 1 && outside[idx - 2]) ||
        (x + 2 < width && outside[idx + 2]) ||
        (y > 1 && outside[idx - width * 2]);

      if (ink > 0.31) {
        const crownSheen = Math.exp(-(((uy - 0.18) * 4.6) ** 2)) * 0.15;
        const waveAxis = facing === 'right' ? 1 - ux : ux;
        const strandWave =
          ux > 0.14 && ux < 0.86 && uy > 0.18 && uy < 0.90
            ? Math.max(0, Math.sin((waveAxis - uy * 0.20) * 32)) *
              0.12 *
              (1 - smoothstep(0.78, 0.95, uy))
            : 0;
        const curlSheen =
          uy > 0.50
            ? Math.exp(-(((ux - 0.74) * 4.2) ** 2)) * 0.10 +
              Math.exp(-(((ux - 0.24) * 4.2) ** 2)) * 0.10
            : 0;
        const rimGlow = nearOut ? 0.62 : 0;
        baseLum[idx] = clamp01((0.052 + crownSheen + strandWave + curlSheen + rimGlow) * bottomFade);
      } else {
        const isTopAccessory = uy < 0.29;
        const isSideWingOrFin = (ux > 0.68 || ux < 0.20) && uy < 0.58;
        const isGarmentTrim = boostTrim && uy > 0.67;

        if (isTopAccessory || isSideWingOrFin || isGarmentTrim) {
          const sheen = 0.85 + Math.exp(-(((ux - 0.50) * 3.4) ** 2)) * 0.11;
          baseLum[idx] = clamp01(sheen * (1 - ink * 0.94) * bottomFade);
        } else if (facing === 'center') {
          const isEyeZone = ux > 0.26 && ux < 0.74 && uy > 0.34 && uy < 0.54;
          const cheekSpec =
            (Math.exp(-(((ux - 0.38) * 7.2) ** 2 + ((uy - 0.54) * 7.0) ** 2)) +
              Math.exp(-(((ux - 0.62) * 7.2) ** 2 + ((uy - 0.54) * 7.0) ** 2))) *
            0.10;
          const noseBridgeSpec =
            Math.exp(-(((ux - 0.50) * 11.5) ** 2 + ((uy - 0.48) * 7.5) ** 2)) * 0.12;
          const templeWrapShadow = smoothstep(0.18, 0.34, Math.abs(ux - 0.50)) * 0.20;
          const bangShadow = (1 - smoothstep(0.30, 0.36, uy)) * 0.10;
          const chinNeckShadow = smoothstep(0.68, 0.92, uy) * 0.18;
          const eyeBoost = isEyeZone ? 0.13 : 0;
          const faceLum =
            0.66 + cheekSpec + noseBridgeSpec + eyeBoost - templeWrapShadow - bangShadow - chinNeckShadow;
          baseLum[idx] = clamp01(Math.min(0.90, faceLum) * (1 - ink * 0.96) * bottomFade);
        } else {
          const litX = facing === 'right' ? 1 - ux : ux;
          const isEyeZone = litX > 0.18 && litX < 0.58 && uy > 0.34 && uy < 0.54;
          const cheekSpec = Math.exp(-(((litX - 0.29) * 6.5) ** 2 + ((uy - 0.54) * 7.0) ** 2)) * 0.13;
          const noseBridgeSpec =
            Math.exp(-(((litX - 0.26) * 12.5) ** 2 + ((uy - 0.49) * 8.0) ** 2)) * 0.13;
          const cheekShadow = smoothstep(0.36, 0.62, litX) * 0.25;
          const bangShadow = (1 - smoothstep(0.31, 0.37, uy)) * 0.11;
          const chinNeckShadow = smoothstep(0.69, 0.92, uy) * 0.19;
          const eyeBoost = isEyeZone ? 0.14 : 0;
          const faceLum =
            0.66 + cheekSpec + noseBridgeSpec + eyeBoost - cheekShadow - bangShadow - chinNeckShadow;
          baseLum[idx] = clamp01(Math.min(0.90, faceLum) * (1 - ink * 0.96) * bottomFade);
        }
      }
    }
  }

  return { width, height, baseLum, inkMask };
}

export interface CompanionMouthTargets {
  open: number;
  width: number;
  round: number;
  jawDrop: number;
  teethTop: number;
  teethBottom: number;
  tongueShow: number;
  tongueLift: number;
  lipPress: number;
  lipBite: number;
  puckerRing: number;
  smile: number;
}

export function resolveCompanionMouthTargets(
  viseme: MouthViseme,
  energy: number,
  isSpeaking: boolean,
  mood: ExpressionMood
): CompanionMouthTargets {
  const e = clamp01(energy);
  const smileBase = mood === 'happy' ? 0.85 : 0.15;
  if (!isSpeaking && viseme === 'closed') {
    return {
      open: 0,
      width: 0.95,
      round: 0,
      jawDrop: 0,
      teethTop: 0,
      teethBottom: 0,
      tongueShow: 0,
      tongueLift: 0,
      lipPress: 0,
      lipBite: 0,
      puckerRing: 0,
      smile: smileBase,
    };
  }
  switch (viseme) {
    case 'closed':
      return {
        open: 0,
        width: 0.92,
        round: 0,
        jawDrop: 0,
        teethTop: 0,
        teethBottom: 0,
        tongueShow: 0,
        tongueLift: 0,
        lipPress: 0,
        lipBite: 0,
        puckerRing: 0,
        smile: 0.08,
      };
    case 'smile':
      return {
        open: 0.02,
        width: 1.12,
        round: 0,
        jawDrop: 0,
        teethTop: 0,
        teethBottom: 0,
        tongueShow: 0,
        tongueLift: 0,
        lipPress: 0,
        lipBite: 0,
        puckerRing: 0,
        smile: 1,
      };
    case 'rest':
      return {
        open: 0.16,
        width: 0.92,
        round: 0,
        jawDrop: 0.08,
        teethTop: 0.55,
        teethBottom: 0,
        tongueShow: 0,
        tongueLift: 0,
        lipPress: 0,
        lipBite: 0,
        puckerRing: 0,
        smile: 0.15,
      };
    case 'mbp':
      return {
        open: 0,
        width: 1.12,
        round: 0,
        jawDrop: 0,
        teethTop: 0,
        teethBottom: 0,
        tongueShow: 0,
        tongueLift: 0,
        lipPress: 1,
        lipBite: 0,
        puckerRing: 0,
        smile: 0,
      };
    case 'fv':
      return {
        open: 0.30 + e * 0.08,
        width: 1.04,
        round: 0,
        jawDrop: 0.16,
        teethTop: 1,
        teethBottom: 0,
        tongueShow: 0,
        tongueLift: 0,
        lipPress: 0,
        lipBite: 1,
        puckerRing: 0,
        smile: 0.1,
      };
    case 'lth':
      return {
        open: 0.56 + e * 0.14,
        width: 1.06,
        round: 0,
        jawDrop: 0.48,
        teethTop: 0.96,
        teethBottom: 0,
        tongueShow: 1,
        tongueLift: 0.96,
        lipPress: 0,
        lipBite: 0,
        puckerRing: 0,
        smile: 0.15,
      };
    case 'small':
      return {
        open: 0.34 + e * 0.12,
        width: 1.02,
        round: 0,
        jawDrop: 0.24,
        teethTop: 0.95,
        teethBottom: 0,
        tongueShow: 0.2,
        tongueLift: 0,
        lipPress: 0,
        lipBite: 0,
        puckerRing: 0,
        smile: 0.15,
      };
    case 'mid':
      return {
        open: 0.64 + e * 0.14,
        width: 1.06,
        round: 0.12,
        jawDrop: 0.56,
        teethTop: 0.94,
        teethBottom: 0,
        tongueShow: 0.55,
        tongueLift: 0,
        lipPress: 0,
        lipBite: 0,
        puckerRing: 0,
        smile: 0.2,
      };
    case 'round': {
      const tightOo = e < 0.45 ? 1 - smoothstep(0.18, 0.45, e) : 0;
      return {
        open: lerp(0.76 + e * 0.14, 0.42, tightOo),
        width: lerp(0.70, 0.54, tightOo),
        round: 1,
        jawDrop: lerp(0.58, 0.28, tightOo),
        teethTop: 0,
        teethBottom: 0,
        tongueShow: 0,
        tongueLift: 0,
        lipPress: 0,
        lipBite: 0,
        puckerRing: 0.96,
        smile: 0,
      };
    }
    case 'ee':
      return {
        open: 0.52 + e * 0.12,
        width: 1.32,
        round: 0,
        jawDrop: 0.34,
        teethTop: 1,
        teethBottom: 0.96,
        tongueShow: 0,
        tongueLift: 0,
        lipPress: 0,
        lipBite: 0,
        puckerRing: 0,
        smile: 0.95,
      };
    case 'wide':
      return {
        open: 0.92 + e * 0.08,
        width: 1.14,
        round: 0,
        jawDrop: 1.0,
        teethTop: 1,
        teethBottom: 0,
        tongueShow: 0.92,
        tongueLift: 0.08,
        lipPress: 0,
        lipBite: 0,
        puckerRing: 0,
        smile: 0.22,
      };
  }
}

function applyCompanionEye(
  ax: number,
  ay: number,
  lum: number,
  eye: CompanionEyeAnchor,
  blinkAmt: number,
  gazeX: number,
  gazeY: number,
  isHappySquint: boolean
): number {
  if (eye.winking) return lum;

  const dx = ax - eye.cx;
  const dy = ay - eye.cy;
  if (Math.abs(dx) > eye.rx * 1.45 || Math.abs(dy) > eye.ry * 1.65) {
    return lum;
  }

  const topY = eye.cy - eye.ry * 1.02;
  const botY = eye.cy + eye.ry * 1.02;

  if (blinkAmt > 0.04) {
    const lidY = lerp(topY, eye.cy + eye.ry * 0.24, blinkAmt);
    const inX = 1 - smoothstep(eye.rx * 0.88, eye.rx * 1.18, Math.abs(dx));

    // Smooth upper eyelid skin sweep over the iris (extends across full socket when near closed)
    const wipeBot = blinkAmt > 0.60 ? lerp(lidY, botY + eye.ry * 0.18, smoothstep(0.60, 0.88, blinkAmt)) : lidY;
    if (inX > 0.02 && ay > topY - eye.ry * 0.14 && ay < wipeBot + 0.006) {
      const vMask =
        smoothstep(topY - eye.ry * 0.14, topY + eye.ry * 0.10, ay) *
        (1 - smoothstep(wipeBot - 0.006, wipeBot + 0.006, ay));
      const mask = inX * vMask * smoothstep(0.04, 0.22, blinkAmt);
      lum = lerp(lum, eye.skinTone, mask * 0.96);
    }

    // Crisp curved anime closed eyelash line (gentle happy upward curve or classic downward closed arc)
    const xNorm = dx / Math.max(0.01, eye.rx);
    const arcDir = isHappySquint ? 1 : -1;
    const arcCurve = xNorm * xNorm * eye.ry * 0.22 * arcDir * smoothstep(0.35, 0.95, blinkAmt);
    const lashY = lerp(lidY, eye.cy + eye.ry * 0.16, smoothstep(0.65, 1.0, blinkAmt));
    const dLash = sdCapsule2(
      ax,
      ay + arcCurve,
      eye.cx - eye.rx * 0.98,
      lashY,
      eye.cx + eye.rx * 0.98,
      lashY,
      Math.min(0.0062, Math.max(0.0042, eye.ry * 0.12))
    );
    if (dLash < 0.009) {
      const lashMask = (1 - smoothstep(-0.002, 0.0065, dLash)) * smoothstep(0.14, 0.48, blinkAmt);
      lum *= 1 - lashMask * 0.96;
    }

    // Delicate upper eyelid crease line
    const dCrease = sdCapsule2(
      ax,
      ay,
      eye.cx - eye.rx * 0.72,
      lashY - eye.ry * 0.42,
      eye.cx + eye.rx * 0.72,
      lashY - eye.ry * 0.42,
      0.0020
    );
    if (dCrease < 0.005) {
      const cMask =
        (1 - smoothstep(-0.001, 0.004, dCrease)) *
        smoothstep(0.16, 0.48, blinkAmt) *
        (1 - smoothstep(0.68, 0.98, blinkAmt) * 0.65);
      lum *= 1 - cMask * 0.26;
    }
  }

  // Animated bright specular eye catchlight that shifts with gaze/saccades when eye is open
  if (blinkAmt < 0.56) {
    const openVis = 1 - smoothstep(0.18, 0.56, blinkAmt);
    const gx = clamp(gazeX, -1, 1) * eye.rx * 0.22;
    const gy = clamp(gazeY, -1, 1) * eye.ry * 0.16;
    const catchR = Math.max(0.0075, Math.min(eye.rx, eye.ry) * 0.21);
    const dCatch = sdEllipse(
      ax,
      ay,
      eye.cx - eye.rx * 0.18 + gx,
      eye.cy - eye.ry * 0.16 + gy,
      catchR,
      catchR
    );
    if (dCatch < 0.005) {
      const cMask = (1 - smoothstep(-0.002, 0.0038, dCatch)) * openVis;
      lum = Math.max(lum, cMask * 0.98);
    }
  }

  return lum;
}

function applyCompanionMouth(
  ax: number,
  ay: number,
  lum: number,
  profile: CompanionPortraitProfile,
  mt: CompanionMouthTargets,
  isSpeaking: boolean
): number {
  const m = profile.mouth;
  const mx = m.cx;
  const my = m.cy;

  // At rest (not speaking and closed), leave the character's original manga mouth untouched
  const needsArticulation =
    isSpeaking ||
    mt.open > 0.04 ||
    mt.lipPress > 0.12 ||
    mt.lipBite > 0.12;
  if (!needsArticulation) {
    return lum;
  }

  // Only clean-patch and sculpt inside local mouth bounding box
  const boxRx = m.rx * 1.85;
  const boxRy = m.ry * 1.85;
  if (Math.abs(ax - mx) > boxRx || Math.abs(ay - my) > boxRy) {
    return lum;
  }

  const activeBlend = clamp01(
    Math.max(
      smoothstep(0.02, 0.22, mt.open),
      smoothstep(0.08, 0.45, mt.lipPress),
      smoothstep(0.08, 0.45, mt.lipBite),
      isSpeaking ? 0.95 : 0
    )
  );

  // Facing-aware skin patch to cleanly erase the static drawn mouth before carving live viseme
  const skinBase = profile.facing === 'center' ? 0.72 : 0.70;
  const patchRx = m.rx * (m.alreadyOpen ? 1.24 : 1.14);
  const patchRy = m.ry * (m.alreadyOpen ? 1.15 : 0.96);
  const dPatch = sdEllipse(ax, ay, mx, my + (m.alreadyOpen ? m.ry * 0.12 : 0), patchRx, patchRy);
  if (dPatch < 0.008) {
    const pMask = (1 - smoothstep(-0.004, 0.007, dPatch)) * activeBlend;
    lum = lerp(lum, skinBase, pMask * 0.96);
  }

  // Bilabial M / B / P or speaking closed pause -> crisp pressed or closed anime lip line
  if (mt.open < 0.08) {
    const pressW = clamp01(mt.lipPress);
    const halfSpan = m.rx * lerp(0.82, 1.04, pressW);
    const lineR = lerp(0.0036, 0.0048, pressW);
    const bow = ((ax - mx) / Math.max(0.01, halfSpan)) ** 2 * 0.0024 * (pressW > 0.3 ? -1 : 1);
    const dLine = sdCapsule2(ax, ay - bow, mx - halfSpan, my, mx + halfSpan, my, lineR);
    if (dLine < 0.006) {
      const lMask = (1 - smoothstep(-0.002, 0.0045, dLine)) * activeBlend;
      lum = lerp(lum, 0.012, lMask * 0.96);
    }
    const dUnderLip = sdEllipse(ax, ay, mx, my + 0.012, halfSpan * 0.55, 0.0048);
    if (dUnderLip < 0.005) {
      const uMask = (1 - smoothstep(-0.002, 0.004, dUnderLip)) * activeBlend;
      lum = Math.max(lum, uMask * lerp(0.80, 0.88, pressW));
    }
    return lum;
  }

  // Active open visemes (small, mid, wide, round, ee, fv, lth)
  const roundW = smoothstep(0.25, 0.72, mt.round);
  const notRound = 1 - roundW;
  const eeW = notRound * smoothstep(0.25, 0.65, mt.teethBottom);
  const fvW = notRound * smoothstep(0.25, 0.65, mt.lipBite);

  const rx =
    m.rx *
    0.88 *
    lerp(
      lerp(lerp(0.68 + mt.open * 0.36, 0.84, fvW), 1.18, eeW),
      lerp(0.42, 0.60, mt.open),
      roundW
    );
  const ry =
    m.ry *
    0.88 *
    lerp(
      lerp(lerp(0.32 + mt.open * 0.74, 0.44, fvW), 0.54 + mt.open * 0.16, eeW),
      lerp(0.54, 0.82, mt.open),
      roundW
    );
  const cy = my + lerp(0.001, m.ry * 0.34, mt.jawDrop);

  const lipRimThickness = Math.max(0.0042, m.rx * 0.085);
  const dOuter = sdEllipse(ax, ay, mx, cy, rx + lipRimThickness, ry + lipRimThickness);
  const dCavity = sdEllipse(ax, ay, mx, cy, rx, ry);

  if (dOuter < 0.006) {
    const rimMask = (1 - smoothstep(-0.002, 0.0042, dOuter)) * activeBlend;
    lum *= 1 - rimMask * 0.84;
  }

  if (dCavity < 0.005) {
    const cMask = (1 - smoothstep(-0.002, 0.0036, dCavity)) * activeBlend;
    lum = lerp(lum, 0.010, cMask * 0.99);

    const normX = (ax - mx) / Math.max(0.008, rx);

    // Crisp bright upper teeth bar
    if (mt.teethTop > 0.18 && roundW < 0.85) {
      const teethCut = lerp(cy - ry * 0.24, cy - ry * 0.36, smoothstep(0.45, 0.85, mt.open));
      if (ay < teethCut && Math.abs(normX) < 0.86) {
        const tEdge = 1 - smoothstep(teethCut - 0.0026, teethCut + 0.0005, ay);
        lum = Math.max(lum, 0.96 * mt.teethTop * tEdge * cMask * (1 - roundW));
      }
    }

    // Lower teeth bar for wide Ee grin
    if (mt.teethBottom > 0.22 && eeW > 0.1) {
      const lowTop = cy + ry * 0.18;
      if (ay > lowTop && Math.abs(normX) < 0.82) {
        const ltEdge = smoothstep(lowTop, lowTop + 0.0028, ay);
        lum = Math.max(lum, 0.94 * mt.teethBottom * ltEdge * cMask * eeW);
      }
    }

    // Tongue inside open cavity (raised center tip for L/Th, soft floor tongue for Wide/Mid)
    if (mt.tongueShow > 0.20 && notRound > 0.1 && fvW < 0.5) {
      const liftW = smoothstep(0.25, 0.65, mt.tongueLift);
      if (liftW > 0.05) {
        const dTip = sdEllipse(ax, ay, mx, cy + ry * 0.20, rx * 0.34, ry * 0.44);
        if (dTip < 0.004) {
          const tgMask = (1 - smoothstep(-0.002, 0.0028, dTip)) * mt.tongueShow * liftW * cMask;
          lum = Math.max(lum, 0.84 * tgMask);
        }
      }
      if (liftW < 0.95) {
        const dFloor = sdEllipse(ax, ay, mx, cy + ry * 0.64, rx * 0.58, ry * 0.32);
        if (dFloor < 0.005) {
          const tgMask = (1 - smoothstep(-0.002, 0.0032, dFloor)) * mt.tongueShow * (1 - liftW) * cMask;
          lum = Math.max(lum, 0.46 * tgMask);
        }
      }
    }

    // Raised lower lip cushion for F/V
    if (fvW > 0.2) {
      const dBite = sdEllipse(ax, ay, mx, cy + ry * 0.42, rx * 0.82, ry * 0.46);
      if (dBite < 0.005 && ay > cy + 0.001) {
        const bMask = (1 - smoothstep(-0.002, 0.003, dBite)) * fvW * cMask;
        lum = Math.max(lum, 0.94 * bMask);
      }
    }
  }

  // Pursed highlight ring on Rounded O / Oo
  if (mt.puckerRing > 0.25) {
    const dRing = Math.abs(sdEllipse(ax, ay, mx, cy, rx + 0.0072, ry + 0.0072)) - 0.0038;
    if (dRing < 0.005) {
      const rMask = (1 - smoothstep(-0.002, 0.0038, dRing)) * mt.puckerRing * activeBlend;
      lum = Math.max(lum, rMask * 0.88);
    }
  }

  // Bright lower-lip catchlight under open mouth
  const dLowerRim = sdEllipse(ax, ay, mx, cy + ry + 0.0068, rx * 0.78, 0.0044);
  if (dLowerRim < 0.005) {
    const lMask = (1 - smoothstep(-0.002, 0.0038, dLowerRim)) * activeBlend;
    lum = Math.max(lum, lMask * 0.88);
  }

  return lum;
}

export interface CompanionPortraitRenderParams {
  sourceField: PortraitSourceField;
  kind?: BrailleShapeKind;
  cols: number;
  rows: number;
  time: number;
  energy: number;
  isSpeaking: boolean;
  isThinking: boolean;
  mood?: ExpressionMood;
  eyeStyle?: 'open' | 'sleepy';
  viseme?: MouthViseme;
  mouthTargets?: CompanionMouthTargets | null;
  gazeX?: number;
  gazeY?: number;
  motion?: BrailleShapeMotionState | null;
}

let cachedCompanionSubLen = 0;
let companionRawBuf = new Float32Array(0);
let companionWorkBuf = new Float32Array(0);
let companionEdgeBuf = new Float32Array(0);
let companionRowChars: string[] = [];

function ensureCompanionBuffers(len: number, cols: number): void {
  if (cachedCompanionSubLen < len) {
    cachedCompanionSubLen = len;
    companionRawBuf = new Float32Array(len);
    companionWorkBuf = new Float32Array(len);
    companionEdgeBuf = new Float32Array(len);
  }
  if (companionRowChars.length !== cols) {
    companionRowChars = Array.from({ length: cols }, () => ' ');
  }
}

/**
 * Renders a high-detail anime companion PortraitSourceField into a crisp 16x16 edge-aware
 * blue-noise Unicode Braille halftone frame with grounded neck-pivot head kinematics,
 * real blinking eyelids, saccading eye catchlights, dynamic brow lifts, and live
 * phoneme-driven talking mouth articulation with physical jaw drop.
 */
export function renderCompanionPortraitFrame(
  params: CompanionPortraitRenderParams
): BrailleShapeFrame {
  const {
    sourceField,
    kind,
    cols,
    rows,
    time,
    energy,
    isSpeaking,
    isThinking,
    mood = isSpeaking ? 'speaking' : isThinking ? 'thinking' : 'happy',
    eyeStyle = 'open',
    viseme = 'closed',
    mouthTargets,
    motion,
  } = params;
  const profile = kind ? COMPANION_PORTRAIT_PROFILES[kind] : COMPANION_PORTRAIT_PROFILES.clover;
  const subW = cols * 2;
  const subH = rows * 4;
  const totalSub = subW * subH;
  ensureCompanionBuffers(totalSub, cols);

  const raw = companionRawBuf;
  const work = companionWorkBuf;
  const edge = companionEdgeBuf;
  const rowChars = companionRowChars;

  const mt = mouthTargets ?? resolveCompanionMouthTargets(viseme, energy, isSpeaking, mood);

  const idlePose = motion?.idle?.pose;

  // Grounded, poised head kinematics around the character's neck pivot (ZERO floating sticker bob!)
  const chestBreath = Math.sin(time * profile.breathRate);
  const speakEmphasisNod = isSpeaking
    ? Math.sin(time * 5.2) * (0.004 + energy * 0.008) + mt.jawDrop * 0.005
    : 0;
  const headNodY = clamp(
    chestBreath * 0.0045 +
      (idlePose ? clamp(idlePose.headY, -0.06, 0.06) * 0.14 : 0) +
      speakEmphasisNod,
    -0.018,
    0.018
  );

  // Subtle conversational head tilt + drag inertia tilt around neck pivot (not floating center spin)
  const ambientTilt =
    Math.sin(time * profile.tiltRate) * profile.tiltAmp +
    Math.sin(time * profile.tiltRate * 0.46 + 0.9) * profile.tiltAmp * 0.42;
  const speakTilt = isSpeaking ? Math.sin(time * 2.4) * (0.018 + energy * 0.022) : 0;
  const rawTilt = motion
    ? motion.dragTilt * 0.48 + (idlePose ? idlePose.tilt * 0.38 : 0) + ambientTilt * 0.55 + speakTilt
    : ambientTilt + speakTilt;
  const headTilt = Number.isFinite(rawTilt) ? clamp(rawTilt, -0.14, 0.14) : 0;

  // Natural micro-saccade fixation holds + pointer gaze tracking
  const fixWindow = Math.floor(time * 0.78);
  const autoGazeX =
    mood === 'thinking'
      ? -0.45 + Math.sin(Math.floor(time * 1.1) * 1.7) * 0.32
      : ((fixWindow * 7) % 5 === 0 ? 0.38 : (fixWindow * 3) % 4 === 1 ? -0.35 : 0.04) *
        Math.sin(fixWindow * 1.9 + 0.4);
  const autoGazeY =
    mood === 'thinking'
      ? -0.42
      : (fixWindow * 5) % 4 === 0
      ? -0.18
      : (fixWindow * 3) % 5 === 2
      ? 0.10
      : 0;

  const inputGazeX = motion ? motion.gazeX : (params.gazeX ?? 0);
  const inputGazeY = motion ? motion.gazeY : (params.gazeY ?? 0);
  const hasPointerGaze = Math.abs(inputGazeX) > 0.04 || Math.abs(inputGazeY) > 0.04;
  const gazeX = clamp(hasPointerGaze ? lerp(autoGazeX, inputGazeX, 0.75) : autoGazeX, -1, 1);
  const gazeY = clamp(hasPointerGaze ? lerp(autoGazeY, inputGazeY, 0.75) : autoGazeY, -1, 1);

  // Subtle 2.5D head turn sway (driven by gaze + conversational sway, attenuated cleanly at shoulders)
  const headYaw = clamp(
    gazeX * 0.16 +
      Math.sin(time * 0.92 + 0.6) * 0.055 +
      (isSpeaking ? Math.sin(time * 1.75) * 0.05 : 0),
    -0.24,
    0.24
  );
  const headShiftX = clamp(
    -headYaw * 0.055 +
      (motion ? clamp(motion.hairLag, -0.25, 0.25) * 0.05 : 0) +
      (idlePose ? clamp(idlePose.headX, -0.15, 0.15) * 0.12 : 0),
    -0.022,
    0.022
  );

  // Blink + sleepy half-lid + brow emphasis
  const sleepyBase = eyeStyle === 'sleepy' ? 0.34 : mood === 'thinking' ? 0.14 : 0;
  const rawBlink = motion ? clamp01(motion.blink) : 0;
  const blinkL = clamp01(Math.max(rawBlink, idlePose?.blinkLeft ?? 0, sleepyBase));
  const blinkR = clamp01(Math.max(rawBlink, idlePose?.blinkRight ?? 0, sleepyBase));
  const isHappySquint = mood === 'happy' && rawBlink > 0.5;

  const browLift = clamp(
    (isSpeaking ? Math.max(0, Math.sin(time * 4.2)) * (0.15 + energy * 0.55) : 0) +
      (mood === 'thinking' ? -0.28 : mood === 'happy' ? 0.18 : 0) +
      (motion ? motion.hoverBoost * 0.28 : 0) -
      rawBlink * 0.20,
    -0.4,
    0.85
  );

  const speakHalo = motion ? (motion.speakHalo ?? (isSpeaking ? 0.65 : 0)) : isSpeaking ? 0.65 : 0;
  const thinkHalo = motion ? (motion.thinkHalo ?? (isThinking ? 0.75 : 0)) : isThinking ? 0.75 : 0;

  // Grounded scale (NO floating zoom-bounce on speech!)
  const baseScale = 1.05;
  const fw = sourceField.width;
  const fh = sourceField.height;
  const baseLum = sourceField.baseLum;
  const inkMask = sourceField.inkMask;
  const invSubW = 1 / subW;
  const invSubH = 1 / subH;
  const lumBoost = 1 + (isSpeaking ? 0.03 + energy * 0.04 : 0) + (motion ? motion.hoverBoost * 0.04 : 0);

  const pivotX = profile.pivotX;
  const pivotY = profile.pivotY;
  const lockLo = profile.shoulderLockY - 0.12;
  const lockHi = profile.shoulderLockY + 0.09;
  const jawAmt = clamp01(mt.jawDrop + mt.open * 0.12);
  const mouth = profile.mouth;

  let activeMinX = subW;
  let activeMaxX = -1;
  let activeMinY = subH;
  let activeMaxY = -1;

  for (let sy = 0; sy < subH; sy++) {
    const uy = (sy + 0.5) * invSubH;
    const rowOff = sy * subW;

    // Decoupled neck-pivot articulation: head moves naturally while shoulders/collar stay grounded!
    const headWeight = 1 - smoothstep(lockLo, lockHi, uy);
    const localTilt = -headTilt * headWeight;
    const cosT = Math.cos(localTilt);
    const sinT = Math.sin(localTilt);
    const localShiftX = headShiftX * headWeight;
    const localShiftY = headNodY * headWeight + chestBreath * 0.0022 * (1 - headWeight);

    const framedY = (uy - 0.5) * baseScale + 0.5;
    const dyPivot = framedY - pivotY + localShiftY;

    for (let sx = 0; sx < subW; sx++) {
      const ux = (sx + 0.5) * invSubW;
      const framedX = (ux - 0.5) * baseScale + 0.5;
      const dxPivot = framedX - pivotX - localShiftX;

      const ax = pivotX + dxPivot * cosT - dyPivot * sinT;
      let ay = pivotY + dxPivot * sinT + dyPivot * cosT;

      // Physical chin/jaw-drop hinge during wide spoken vowels
      if (
        jawAmt > 0.06 &&
        ay > mouth.cy - mouth.ry * 0.25 &&
        ay < mouth.chinY + 0.06 &&
        Math.abs(ax - mouth.cx) < mouth.rx * 2.2
      ) {
        const latW = 1 - smoothstep(mouth.rx * 0.9, mouth.rx * 2.2, Math.abs(ax - mouth.cx));
        const vertW =
          smoothstep(mouth.cy - mouth.ry * 0.25, mouth.cy + mouth.ry * 0.55, ay) *
          (1 - smoothstep(mouth.chinY - 0.02, mouth.chinY + 0.06, ay));
        ay -= jawAmt * 0.020 * latW * vertW;
      }

      let val = 0;
      let ink = 0;
      if (ax >= 0 && ax < 1 && ay >= 0 && ay < 1) {
        const fx = ax * (fw - 1);
        const fy = ay * (fh - 1);
        const x0 = fx | 0;
        const y0 = fy | 0;
        const x1 = x0 + 1 < fw ? x0 + 1 : fw - 1;
        const y1 = y0 + 1 < fh ? y0 + 1 : fh - 1;
        const tx = fx - x0;
        const ty = fy - y0;
        const o00 = y0 * fw + x0;
        const o10 = y0 * fw + x1;
        const o01 = y1 * fw + x0;
        const o11 = y1 * fw + x1;
        const w00 = (1 - tx) * (1 - ty);
        const w10 = tx * (1 - ty);
        const w01 = (1 - tx) * ty;
        const w11 = tx * ty;
        val = (baseLum[o00] * w00 + baseLum[o10] * w10 + baseLum[o01] * w01 + baseLum[o11] * w11) * lumBoost;
        ink = inkMask[o00] * w00 + inkMask[o10] * w10 + inkMask[o01] * w01 + inkMask[o11] * w11;

        if (val > 0.002 || ink > 0.002) {
          // 2.5D head-turn cheek key-light chiaroscuro shift
          if (ink < 0.35 && Math.abs(headYaw) > 0.02 && ay > 0.36 && ay < 0.76) {
            const cheekW =
              (1 - smoothstep(0.14, 0.34, Math.abs(ax - mouth.cx))) *
              smoothstep(0.36, 0.56, ay) *
              (1 - smoothstep(0.66, 0.78, ay));
            val = clamp01(val + headYaw * 0.14 * cheekW);
          }

          // Subtle dynamic eyebrow emphasis micro-motion
          if (Math.abs(browLift) > 0.10 && ink < 0.55 && Math.abs(ay - profile.browY) < 0.035) {
            const bOff = -browLift * 0.011;
            const dBL = sdCapsule2(
              ax,
              ay,
              profile.leftEye.cx - profile.leftEye.rx * 0.72,
              profile.browY + bOff,
              profile.leftEye.cx + profile.leftEye.rx * 0.72,
              profile.browY + bOff * 0.85,
              0.0038
            );
            const dBR = sdCapsule2(
              ax,
              ay,
              profile.rightEye.cx - profile.rightEye.rx * 0.72,
              profile.browY + bOff * 0.85,
              profile.rightEye.cx + profile.rightEye.rx * 0.72,
              profile.browY + bOff,
              0.0038
            );
            const dB = dBL < dBR ? dBL : dBR;
            if (dB < 0.006) {
              const bMask = (1 - smoothstep(-0.002, 0.0042, dB)) * Math.min(1, Math.abs(browLift));
              val *= 1 - bMask * 0.56;
            }
          }

          // Real anime blinking eyelids + saccading specular eye catchlights
          val = applyCompanionEye(ax, ay, val, profile.leftEye, blinkL, gazeX, gazeY, isHappySquint);
          val = applyCompanionEye(ax, ay, val, profile.rightEye, blinkR, gazeX, gazeY, isHappySquint);

          // Live phoneme-driven anime talking mouth articulation
          val = applyCompanionMouth(ax, ay, val, profile, mt, isSpeaking);
        }
      }

      // Orbiting cognition sparks when thinking
      if (thinkHalo > 0.05) {
        for (let k = 0; k < 3; k++) {
          const ang = time * 2.4 + k * 2.0944;
          const ox = 0.50 + Math.cos(ang) * 0.43;
          const oy = 0.22 + Math.sin(ang) * 0.14;
          const dx = ux - ox;
          const dy = uy - oy;
          const d2 = dx * dx + dy * dy;
          if (d2 < 0.0014) {
            const spark = (1 - d2 / 0.0014) * thinkHalo * 0.92;
            if (spark > val) val = spark;
          }
        }
      }

      // Delicate speaking acoustic arcs outside silhouette on left & right flanks
      if (speakHalo > 0.05 && val < 0.04) {
        const dcx = ux - 0.5;
        const dcy = uy - 0.52;
        const dist = Math.hypot(dcx, dcy);
        if (dist > 0.41 && dist < 0.495 && Math.abs(dcy) < 0.20) {
          const wave = Math.sin(dist * 72 - time * 12) * 0.5 + 0.5;
          const arcEnv =
            smoothstep(0.41, 0.435, dist) *
            (1 - smoothstep(0.47, 0.495, dist)) *
            (1 - smoothstep(0.10, 0.20, Math.abs(dcy)));
          const arcLum = wave * arcEnv * speakHalo * (0.45 + energy * 0.45);
          if (arcLum > val) val = arcLum;
        }
      }

      val = clamp01(val);
      raw[rowOff + sx] = val;
      if (val > 0.028) {
        if (sx < activeMinX) activeMinX = sx;
        if (sx > activeMaxX) activeMaxX = sx;
        if (sy < activeMinY) activeMinY = sy;
        if (sy > activeMaxY) activeMaxY = sy;
      }
    }
  }

  work.fill(0, 0, totalSub);
  edge.fill(0, 0, totalSub);

  // Contour-preserving unsharp boost + edge-lock weights for razor-sharp eyes, lashes, teeth, and mouth rims
  if (activeMaxX >= activeMinX && activeMaxY >= activeMinY) {
    const syLo = activeMinY > 1 ? activeMinY - 1 : 0;
    const syHi = activeMaxY + 1 < subH ? activeMaxY + 1 : subH - 1;
    const sxLo = activeMinX > 1 ? activeMinX - 1 : 0;
    const sxHi = activeMaxX + 1 < subW ? activeMaxX + 1 : subW - 1;

    for (let sy = syLo; sy <= syHi; sy++) {
      const rowOff = sy * subW;
      const upOff = (sy > 0 ? sy - 1 : 0) * subW;
      const dnOff = (sy + 1 < subH ? sy + 1 : subH - 1) * subW;
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
        const avg = (l + r + raw[upOff + sx] + raw[dnOff + sx]) * 0.25;
        const diff = c - avg;
        const absDiff = diff < 0 ? -diff : diff;
        edge[idx] = absDiff > 0.045 ? Math.min(0.66, (absDiff - 0.045) * 3.3) : 0;
        const sharpened = c + diff * 0.52;
        let t = (sharpened - 0.03) * 1.08;
        if (t <= 0) t = 0;
        else if (t >= 1) t = 1;
        else t = t * 0.40 + t * t * (3 - 2 * t) * 0.60;
        work[idx] = t;
      }
    }
  }

  // Deterministic 16x16 blue-noise + edge-aware contour-lock Braille packing
  const cells = new Uint8Array(cols * rows);
  const subLum = new Uint8Array(totalSub);
  const lines: string[] = Array.from({ length: rows }, () => '');
  const emptyRowStr = ' '.repeat(cols);

  const rLo = activeMaxY >= activeMinY ? Math.max(0, activeMinY >> 2) : rows;
  const rHi = activeMaxY >= activeMinY ? Math.min(rows - 1, (activeMaxY + 1) >> 2) : -1;
  const cLo = activeMaxX >= activeMinX ? Math.max(0, (activeMinX - 1) >> 1) : cols;
  const cHi = activeMaxX >= activeMinX ? Math.min(cols - 1, (activeMaxX + 1) >> 1) : -1;

  for (let r = 0; r < rows; r++) {
    const cellRowOff = r * cols;
    if (r < rLo || r > rHi || cHi < cLo) {
      lines[r] = emptyRowStr;
      continue;
    }

    if (cLo > 0) rowChars.fill(' ', 0, cLo);
    if (cHi + 1 < cols) rowChars.fill(' ', cHi + 1, cols);

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
      const c2 = c << 1;
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

      const t00 = BLUE_NOISE_16X16_THRESH[bRow0 + bx0];
      if (w00 >= t00 + (0.5 - t00) * edge[i00]) {
        bits |= 0x01;
        subLum[i00] = (w00 * 255 + 0.5) | 0;
      }
      const t01 = BLUE_NOISE_16X16_THRESH[bRow0 + bx1];
      if (w01 >= t01 + (0.5 - t01) * edge[i01]) {
        bits |= 0x08;
        subLum[i01] = (w01 * 255 + 0.5) | 0;
      }
      const t10 = BLUE_NOISE_16X16_THRESH[bRow1 + bx0];
      if (w10 >= t10 + (0.5 - t10) * edge[i10]) {
        bits |= 0x02;
        subLum[i10] = (w10 * 255 + 0.5) | 0;
      }
      const t11 = BLUE_NOISE_16X16_THRESH[bRow1 + bx1];
      if (w11 >= t11 + (0.5 - t11) * edge[i11]) {
        bits |= 0x10;
        subLum[i11] = (w11 * 255 + 0.5) | 0;
      }
      const t20 = BLUE_NOISE_16X16_THRESH[bRow2 + bx0];
      if (w20 >= t20 + (0.5 - t20) * edge[i20]) {
        bits |= 0x04;
        subLum[i20] = (w20 * 255 + 0.5) | 0;
      }
      const t21 = BLUE_NOISE_16X16_THRESH[bRow2 + bx1];
      if (w21 >= t21 + (0.5 - t21) * edge[i21]) {
        bits |= 0x20;
        subLum[i21] = (w21 * 255 + 0.5) | 0;
      }
      const t30 = BLUE_NOISE_16X16_THRESH[bRow3 + bx0];
      if (w30 >= t30 + (0.5 - t30) * edge[i30]) {
        bits |= 0x40;
        subLum[i30] = (w30 * 255 + 0.5) | 0;
      }
      const t31 = BLUE_NOISE_16X16_THRESH[bRow3 + bx1];
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
