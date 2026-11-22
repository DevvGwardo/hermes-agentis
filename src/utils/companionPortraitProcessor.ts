import type { PortraitSourceField } from './asciiPortraitEngine';
import { BLUE_NOISE_8X8_RANKS } from './brailleCanvasRaster';
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

function smoothstep(edge0: number, edge1: number, x: number): number {
  const t = clamp01((x - edge0) / (edge1 - edge0));
  return t * t * (3 - 2 * t);
}

const COMPANION_DITHER_THRESH = new Float32Array(64);
for (let i = 0; i < 64; i++) {
  COMPANION_DITHER_THRESH[i] = 0.065 + (BLUE_NOISE_8X8_RANKS[i] / 63) * 0.865;
}

const BRAILLE_CHAR_TABLE: string[] = Array.from({ length: 256 }, (_, i) =>
  i === 0 ? ' ' : String.fromCharCode(0x2800 + i)
);

type PortraitFacing = 'left' | 'center' | 'right';

interface CompanionPortraitProfile {
  facing: PortraitFacing;
  hollowSeeds?: ReadonlyArray<{ ux: number; uy: number }>;
  boostGarmentTrim?: boolean;
  bobRate: number;
  bobAmp: number;
  tiltRate: number;
  tiltAmp: number;
}

export const COMPANION_PORTRAIT_PROFILES: Record<BrailleShapeKind, CompanionPortraitProfile> = {
  clover: {
    facing: 'left',
    bobRate: 2.0,
    bobAmp: 0.012,
    tiltRate: 1.05,
    tiltAmp: 0.020,
  },
  star: {
    facing: 'right',
    bobRate: 2.8,
    bobAmp: 0.019,
    tiltRate: 1.45,
    tiltAmp: 0.032,
  },
  ghost: {
    facing: 'center',
    bobRate: 2.4,
    bobAmp: 0.015,
    tiltRate: 1.35,
    tiltAmp: 0.034,
  },
  heart: {
    facing: 'right',
    bobRate: 1.9,
    bobAmp: 0.013,
    tiltRate: 0.95,
    tiltAmp: 0.024,
  },
  cloud: {
    facing: 'left',
    bobRate: 2.5,
    bobAmp: 0.017,
    tiltRate: 1.25,
    tiltAmp: 0.026,
  },
  triangle: {
    facing: 'right',
    bobRate: 1.8,
    bobAmp: 0.010,
    tiltRate: 0.90,
    tiltAmp: 0.016,
  },
  pebble: {
    facing: 'left',
    boostGarmentTrim: true,
    bobRate: 1.35,
    bobAmp: 0.012,
    tiltRate: 0.72,
    tiltAmp: 0.022,
  },
  trapezoid: {
    facing: 'center',
    hollowSeeds: [{ ux: 0.50, uy: 0.066 }],
    boostGarmentTrim: true,
    bobRate: 1.45,
    bobAmp: 0.022,
    tiltRate: 0.78,
    tiltAmp: 0.018,
  },
  blob: {
    facing: 'right',
    bobRate: 2.35,
    bobAmp: 0.016,
    tiltRate: 1.30,
    tiltAmp: 0.030,
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
    // Pure white exterior background in opaque JPEG manga ink illustrations
    return r > 232 && g > 232 && b > 232;
  };

  const pushOutside = (x: number, y: number) => {
    const idx = y * width + x;
    if (!outside[idx] && isBgPixel(idx)) {
      outside[idx] = 1;
      queue[qTail++] = idx;
    }
  };

  // Seed outer perimeter, skipping the center-bottom neck/shoulder opening
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

  // Seed hollow interior sky pockets (e.g. Seraph's floating double-ring halo)
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
    // Prevent exterior flood-fill from sneaking upward through an open bottom collar/shoulder gap
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
        // Dark ink hair, glasses rims, lashes, hoodie, outlines: sculpted dark sheen + luminous rim
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
        // Bright anime skin, hair sheen highlights, headwear, ears, wings, mask, peace hand, and trim
        const isTopAccessory = uy < 0.29;
        const isSideWingOrFin = (ux > 0.68 || ux < 0.20) && uy < 0.58;
        const isGarmentTrim = boostTrim && uy > 0.67;

        if (isTopAccessory || isSideWingOrFin || isGarmentTrim) {
          // Crisp luminous highlights on halos, ears, goggles, fox mask, wings, mecha fins & hoodie seams
          const sheen = 0.85 + Math.exp(-(((ux - 0.50) * 3.4) ** 2)) * 0.11;
          baseLum[idx] = clamp01(sheen * (1 - ink * 0.94) * bottomFade);
        } else if (facing === 'center') {
          // Symmetrical studio front-key lighting for front-facing characters (Neko, Seraph)
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
          // Facing-aware 3/4 anime chiaroscuro (mirrored cleanly for right-facing vs left-facing)
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

export interface CompanionPortraitRenderParams {
  sourceField: PortraitSourceField;
  kind?: BrailleShapeKind;
  cols: number;
  rows: number;
  time: number;
  energy: number;
  isSpeaking: boolean;
  isThinking: boolean;
  gazeX?: number;
  gazeY?: number;
  motion?: BrailleShapeMotionState | null;
}

let cachedCompanionSubLen = 0;
let companionRawBuf = new Float32Array(0);
let companionWorkBuf = new Float32Array(0);
let companionRowChars: string[] = [];

function ensureCompanionBuffers(len: number, cols: number): void {
  if (cachedCompanionSubLen < len) {
    cachedCompanionSubLen = len;
    companionRawBuf = new Float32Array(len);
    companionWorkBuf = new Float32Array(len);
  }
  if (companionRowChars.length !== cols) {
    companionRowChars = Array.from({ length: cols }, () => ' ');
  }
}

/**
 * Renders a high-detail anime companion PortraitSourceField into a crisp blue-noise
 * Unicode Braille halftone frame with live breathing, tilt, drag lean, hover lift,
 * speaking acoustic arcs, and thinking cognition sparks.
 */
export function renderCompanionPortraitFrame(
  params: CompanionPortraitRenderParams
): BrailleShapeFrame {
  const { sourceField, kind, cols, rows, time, energy, isSpeaking, isThinking, motion } = params;
  const profile = kind ? COMPANION_PORTRAIT_PROFILES[kind] : COMPANION_PORTRAIT_PROFILES.clover;
  const subW = cols * 2;
  const subH = rows * 4;
  const totalSub = subW * subH;
  ensureCompanionBuffers(totalSub, cols);

  const raw = companionRawBuf;
  const work = companionWorkBuf;
  const rowChars = companionRowChars;

  const idlePose = motion?.idle?.pose;
  const ambientBob = Math.sin(time * profile.bobRate) * profile.bobAmp;
  const ambientTilt = Math.sin(time * profile.tiltRate) * profile.tiltAmp;
  const rawBob = motion
    ? motion.bounceY * 0.45 + (idlePose ? idlePose.headY * 0.35 : 0) + ambientBob * 0.35
    : ambientBob;
  const bobY = Number.isFinite(rawBob) ? clamp(rawBob, -0.06, 0.06) : 0;
  const rawTilt = motion
    ? motion.dragTilt * 0.85 + (idlePose ? idlePose.tilt * 0.55 : 0) + ambientTilt * 0.25
    : ambientTilt;
  const tilt = Number.isFinite(rawTilt) ? clamp(rawTilt, -0.26, 0.26) : 0;
  const hoverLift = motion ? clamp(motion.hoverBoost, 0, 1) * 0.024 : 0;
  const idleShiftX = idlePose ? clamp(idlePose.headX, -0.15, 0.15) * 0.35 : 0;
  const dragLeanX = motion ? clamp(motion.hairLag, -0.25, 0.25) * 0.16 + idleShiftX : 0;
  const dragLeanY = motion ? clamp(motion.dragLift, -0.3, 1.0) * 0.022 : 0;
  const idleSquash = idlePose ? clamp(idlePose.squashY, -0.2, 0.2) : 0;
  const liftSquash = motion ? clamp(motion.dragLift, -0.25, 0.6) + idleSquash : 0;
  const gazeX = clamp(motion ? motion.gazeX : (params.gazeX ?? 0), -1, 1);
  const gazeY = clamp(motion ? motion.gazeY : (params.gazeY ?? 0), -1, 1);
  const speakHalo = motion ? (motion.speakHalo ?? (isSpeaking ? 0.7 : 0)) : isSpeaking ? 0.7 : 0;
  const thinkHalo = motion ? (motion.thinkHalo ?? (isThinking ? 0.8 : 0)) : isThinking ? 0.8 : 0;
  const blinkAmt = motion ? clamp01(motion.blink) : 0;

  const cosT = Math.cos(-tilt);
  const sinT = Math.sin(-tilt);
  const scaleX = 1.06 / (1 + liftSquash * 0.06 + (isSpeaking ? energy * 0.032 : 0));
  const scaleY = 1.06 / (1 - liftSquash * 0.05 + (isSpeaking ? energy * 0.032 : 0));
  const shiftX = gazeX * 0.016 + dragLeanX;
  const shiftY = bobY - hoverLift - dragLeanY + gazeY * 0.012;

  const fw = sourceField.width;
  const fh = sourceField.height;
  const baseLum = sourceField.baseLum;
  const invSubW = 1 / subW;
  const invSubH = 1 / subH;
  const lumBoost = 1 + (isSpeaking ? 0.06 + energy * 0.08 : 0) + (motion ? motion.hoverBoost * 0.05 : 0);
  // Subtle micro-shimmer on blinks so companion still feels responsive without distorting anime eyes
  const blinkPulse = blinkAmt > 0.15 ? 1 - blinkAmt * 0.06 : 1;

  let activeMinX = subW;
  let activeMaxX = -1;
  let activeMinY = subH;
  let activeMaxY = -1;

  for (let sy = 0; sy < subH; sy++) {
    const uy = (sy + 0.5) * invSubH;
    const ny = uy - 0.5 + shiftY;
    const rowOff = sy * subW;

    for (let sx = 0; sx < subW; sx++) {
      const ux = (sx + 0.5) * invSubW;
      const nx = ux - 0.5 - shiftX;

      // Rigid rotation + scale around portrait center
      const rx = (nx * cosT - ny * sinT) * scaleX + 0.5;
      const ry = (nx * sinT + ny * cosT) * scaleY + 0.5;

      let val = 0;
      if (rx >= 0 && rx < 1 && ry >= 0 && ry < 1) {
        const fx = rx * (fw - 1);
        const fy = ry * (fh - 1);
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
        const top = baseLum[o00] * (1 - tx) + baseLum[o10] * tx;
        const bot = baseLum[o01] * (1 - tx) + baseLum[o11] * tx;
        val = (top * (1 - ty) + bot * ty) * lumBoost * blinkPulse;
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

      // Delicate speaking acoustic arcs on left & right flanks
      if (speakHalo > 0.05) {
        const dcx = ux - 0.5;
        const dcy = uy - 0.52;
        const dist = Math.hypot(dcx, dcy);
        if (dist > 0.40 && dist < 0.50 && Math.abs(dcy) < 0.22) {
          const wave = Math.sin(dist * 75 - time * 12) * 0.5 + 0.5;
          const arcEnv =
            smoothstep(0.40, 0.43, dist) *
            (1 - smoothstep(0.47, 0.50, dist)) *
            (1 - smoothstep(0.10, 0.22, Math.abs(dcy)));
          const arcLum = wave * arcEnv * speakHalo * (0.55 + energy * 0.45);
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

  // Contour-preserving unsharp contrast boost for razor-sharp eyes, lashes, and hair sheen
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
        const sharpened = c + (c - avg) * 0.50;
        let t = (sharpened - 0.03) * 1.08;
        if (t <= 0) t = 0;
        else if (t >= 1) t = 1;
        else t = t * 0.40 + t * t * (3 - 2 * t) * 0.60;
        work[idx] = t;
      }
    }
  }

  // Deterministic 8x8 blue-noise Braille packing + exact per-subpixel luminance
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
    const bRow0 = (syBase & 7) << 3;
    const bRow1 = ((syBase + 1) & 7) << 3;
    const bRow2 = ((syBase + 2) & 7) << 3;
    const bRow3 = ((syBase + 3) & 7) << 3;

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

      const w00 = work[r0 + c2];
      const w01 = work[r0 + c2 + 1];
      const w10 = work[r1 + c2];
      const w11 = work[r1 + c2 + 1];
      const w20 = work[r2 + c2];
      const w21 = work[r2 + c2 + 1];
      const w30 = work[r3 + c2];
      const w31 = work[r3 + c2 + 1];

      const bx0 = c2 & 7;
      const bx1 = (c2 + 1) & 7;
      let bits = 0;
      if (w00 >= COMPANION_DITHER_THRESH[bRow0 + bx0]) {
        bits |= 0x01;
        subLum[r0 + c2] = (w00 * 255 + 0.5) | 0;
      }
      if (w01 >= COMPANION_DITHER_THRESH[bRow0 + bx1]) {
        bits |= 0x08;
        subLum[r0 + c2 + 1] = (w01 * 255 + 0.5) | 0;
      }
      if (w10 >= COMPANION_DITHER_THRESH[bRow1 + bx0]) {
        bits |= 0x02;
        subLum[r1 + c2] = (w10 * 255 + 0.5) | 0;
      }
      if (w11 >= COMPANION_DITHER_THRESH[bRow1 + bx1]) {
        bits |= 0x10;
        subLum[r1 + c2 + 1] = (w11 * 255 + 0.5) | 0;
      }
      if (w20 >= COMPANION_DITHER_THRESH[bRow2 + bx0]) {
        bits |= 0x04;
        subLum[r2 + c2] = (w20 * 255 + 0.5) | 0;
      }
      if (w21 >= COMPANION_DITHER_THRESH[bRow2 + bx1]) {
        bits |= 0x20;
        subLum[r2 + c2 + 1] = (w21 * 255 + 0.5) | 0;
      }
      if (w30 >= COMPANION_DITHER_THRESH[bRow3 + bx0]) {
        bits |= 0x40;
        subLum[r3 + c2] = (w30 * 255 + 0.5) | 0;
      }
      if (w31 >= COMPANION_DITHER_THRESH[bRow3 + bx1]) {
        bits |= 0x80;
        subLum[r3 + c2 + 1] = (w31 * 255 + 0.5) | 0;
      }

      cells[cellRowOff + c] = (lumSum * 31.875 + 0.5) | 0;
      rowChars[c] = BRAILLE_CHAR_TABLE[bits];
    }
    lines[r] = rowChars.join('');
  }

  return { cols, rows, lines, cells, subLum };
}
