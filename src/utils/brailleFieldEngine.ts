import { BLUE_NOISE_8X8_RANKS } from './brailleCanvasRaster';

export type BrailleHorizonStyle = 'silk' | 'peaks' | 'pulse' | 'ribbon';

export interface HorizonMotionState {
  energy: number;
  pointerX: number;
  pointerY: number;
  bands: number[];
  pointerVelX?: number;
  wakePhase?: number;
}

export interface HorizonAnimatorInput {
  energy: number;
  pointerX: number;
  pointerY: number;
  bands?: number[];
}

export interface BrailleHorizonParams {
  cols: number;
  rows: number;
  time: number;
  energy: number;
  pointerX: number; // -1..1
  pointerY: number; // -1..1
  bands?: number[];
  motion?: HorizonMotionState | null;
  style?: BrailleHorizonStyle;
}

export interface BrailleHorizonFrame {
  cols: number;
  rows: number;
  lines: string[];
  /** Per-cell normalized intensity 0..255 */
  cells: Uint8Array;
  /** Per-subpixel (2*cols x 4*rows) normalized intensity 0..255 */
  subLum?: Uint8Array;
}

const BRAILLE_CHAR_TABLE: string[] = Array.from({ length: 256 }, (_, i) =>
  i === 0 ? ' ' : String.fromCharCode(0x2800 + i)
);

const FIELD_DITHER_THRESH = Float32Array.from(
  BLUE_NOISE_8X8_RANKS,
  (rank) => 0.11 + ((rank + 0.5) / 64) * 0.72
);

const FIELD_EXP_MAX = 8;
const FIELD_EXP_SIZE = 512;
const FIELD_EXP_SCALE = FIELD_EXP_SIZE / FIELD_EXP_MAX;
const FIELD_EXP_LUT = new Float32Array(FIELD_EXP_SIZE + 1);
for (let i = 0; i <= FIELD_EXP_SIZE; i++) {
  FIELD_EXP_LUT[i] = Math.exp(-(i * FIELD_EXP_MAX) / FIELD_EXP_SIZE);
}

function fastFieldExpNeg(x: number): number {
  if (x <= 0) return 1;
  if (x >= FIELD_EXP_MAX) return 0;
  const f = x * FIELD_EXP_SCALE;
  const idx = f | 0;
  const v0 = FIELD_EXP_LUT[idx];
  return v0 + (FIELD_EXP_LUT[idx + 1] - v0) * (f - idx);
}

function clamp01(v: number): number {
  return v < 0 ? 0 : v > 1 ? 1 : v;
}

function clamp(v: number, min: number, max: number): number {
  return v < min ? min : v > max ? max : v;
}

function smoothstep(edge0: number, edge1: number, x: number): number {
  const t = clamp01((x - edge0) / (edge1 - edge0 || 1));
  return t * t * (3 - 2 * t);
}

/**
 * Stateful spring-damped filter for acoustic Braille horizon energy, 12-band FFT levels,
 * and pointer interaction with directional wake tracking.
 */
export function createHorizonAnimator() {
  const bands = new Array<number>(12).fill(0);
  const state: HorizonMotionState = {
    energy: 0.08,
    pointerX: 0,
    pointerY: 0,
    bands,
    pointerVelX: 0,
    wakePhase: 0,
  };
  let energyVel = 0;
  let pointerVelX = 0;
  let wakePhase = 0;

  return {
    step(input: HorizonAnimatorInput, dt: number, reducedMotion = false): HorizonMotionState {
      const clampedDt = clamp(dt, 0.004, 0.06);
      if (reducedMotion) {
        state.energy = clamp01(input.energy * 0.4);
        state.pointerX = clamp(input.pointerX, -1, 1);
        state.pointerY = clamp(input.pointerY, -1, 1);
        state.pointerVelX = 0;
        for (let i = 0; i < 12; i++) {
          bands[i] = clamp01((input.bands?.[i] ?? 0) * 0.5);
        }
        return state;
      }

      // Critically-damped 2nd-order filter on energy: fluid takeoff & release with zero derivative spikes
      const targetEnergy = clamp01(input.energy);
      const eStiffness = targetEnergy > state.energy ? 68 : 42;
      const eDamping = targetEnergy > state.energy ? 17.5 : 13.5;
      energyVel += ((targetEnergy - state.energy) * eStiffness - energyVel * eDamping) * clampedDt;
      state.energy = clamp01(state.energy + energyVel * clampedDt);

      // Exponential pointer smoothing + damped horizontal wake velocity
      const prevPtrX = state.pointerX;
      const ptrFactor = 1 - Math.exp(-9.5 * clampedDt);
      state.pointerX += (clamp(input.pointerX, -1, 1) - state.pointerX) * ptrFactor;
      state.pointerY += (clamp(input.pointerY, -1, 1) - state.pointerY) * ptrFactor;

      const rawVelX = clamp((state.pointerX - prevPtrX) / clampedDt, -3.5, 3.5);
      const velFactor = 1 - Math.exp(-7.0 * clampedDt);
      pointerVelX += (rawVelX - pointerVelX) * velFactor;
      wakePhase += clampedDt * (1.2 + Math.abs(pointerVelX) * 2.4 + state.energy * 1.4);

      // Fast-attack / graceful-release band filtering
      const attackFactor = 1 - Math.exp(-9.5 * clampedDt);
      const releaseFactor = 1 - Math.exp(-6.0 * clampedDt);
      for (let i = 0; i < 12; i++) {
        const target = clamp01(input.bands?.[i] ?? 0);
        const f = target > bands[i] ? attackFactor : releaseFactor;
        bands[i] += (target - bands[i]) * f;
      }

      return {
        energy: state.energy,
        pointerX: state.pointerX,
        pointerY: state.pointerY,
        bands: bands.slice(),
        pointerVelX,
        wakePhase,
      };
    },
  };
}

let horizonBufCap = 0;
let horizonSubLum = new Float32Array(0);
let horizonColCap = 0;
let colCrest1 = new Float32Array(0);
let colCrest1Echo = new Float32Array(0);
let colCrest2 = new Float32Array(0);
let colCrest3 = new Float32Array(0);
let colCrest4 = new Float32Array(0);
let colCrest5 = new Float32Array(0);
let colSpectrumTop = new Float32Array(0);
let colShimmer = new Float32Array(0);
let colMist = new Float32Array(0);
let colSinU = new Float32Array(0);
let colCosU = new Float32Array(0);
let colDeepSinU = new Float32Array(0);
let colDeepCosU = new Float32Array(0);
let colPeakTop = new Float32Array(0);
let colBarGate = new Float32Array(0);
let horizonRowChars: string[] = [];

function ensureHorizonBuffers(subLen: number, subW: number, cols: number): Float32Array {
  if (subLen > horizonBufCap) {
    horizonBufCap = subLen;
    horizonSubLum = new Float32Array(subLen);
  }
  if (subW > horizonColCap) {
    horizonColCap = subW;
    colCrest1 = new Float32Array(subW);
    colCrest1Echo = new Float32Array(subW);
    colCrest2 = new Float32Array(subW);
    colCrest3 = new Float32Array(subW);
    colCrest4 = new Float32Array(subW);
    colCrest5 = new Float32Array(subW);
    colSpectrumTop = new Float32Array(subW);
    colShimmer = new Float32Array(subW);
    colMist = new Float32Array(subW);
    colSinU = new Float32Array(subW);
    colCosU = new Float32Array(subW);
    colDeepSinU = new Float32Array(subW);
    colDeepCosU = new Float32Array(subW);
    colPeakTop = new Float32Array(subW);
    colBarGate = new Float32Array(subW);
  }
  if (horizonRowChars.length !== cols) {
    horizonRowChars = new Array<string>(cols);
  }
  return horizonSubLum;
}

/**
 * Renders a technical 2x4 Unicode Braille oscillographic & telemetry wave field
 * combining high-Q dual-trace carrier harmonics, interference nodes, quantized vertical
 * sample stems, optical graticule crosshairs, lower spectrum needles, and cursor phase-lock.
 */
export function renderBrailleHorizon(params: BrailleHorizonParams): BrailleHorizonFrame {
  const { cols, rows, time, motion, style = 'silk' } = params;
  const energy = motion ? motion.energy : params.energy;
  const pointerX = motion ? motion.pointerX : params.pointerX;
  const pointerY = motion ? motion.pointerY : params.pointerY;
  const pointerVelX = motion?.pointerVelX ?? 0;
  const wakePhase = motion?.wakePhase ?? time * 1.4;
  const bands = motion ? motion.bands : params.bands;

  const subW = cols * 2;
  const subH = rows * 4;
  const lines: string[] = new Array(rows);
  const cells = new Uint8Array(cols * rows);

  const subLum = ensureHorizonBuffers(subW * subH, subW, cols);
  const rowChars = horizonRowChars;
  const pxTarget = (pointerX + 1) * 0.5;
  const pyBoost = (1 - clamp01((pointerY + 1) * 0.5)) * 0.06;
  const voiceAmp = 0.065 + energy * 0.19;
  const bandLen = bands?.length ?? 0;
  const invSubW = 1 / subW;
  const invSubH = 1 / subH;
  const cursorWeight = 0.09 + pyBoost;
  const wakeAmp = clamp(Math.abs(pointerVelX) * 0.048, 0, 0.08);
  const wakeDir = pointerVelX >= 0 ? -1 : 1;
  const isRibbon = style === 'ribbon';
  const c1Amp = isRibbon ? 0.14 + energy * 0.24 : 0.072 + voiceAmp * 0.22;
  const c2Amp = isRibbon ? 0.09 + energy * 0.17 : 0.064 + voiceAmp * 0.18;
  const c3Amp = 0.056 + voiceAmp * 0.16;
  const c4Amp = 0.046 + voiceAmp * 0.14;
  const c5Amp = 0.036 + voiceAmp * 0.11;
  const shimmerScale = 0.24 * (0.35 + energy * 0.65);
  const isPeaks = style === 'peaks';
  const isPulse = style === 'pulse';
  const barCount = Math.max(18, Math.round(cols * 0.28));

  // Adaptive telemetry grid cadence in subpixels (keeps clean spacing across mobile and wide desktop)
  const sampleStep = subW >= 360 ? 12 : 8;
  const gridStepX = sampleStep * 2;
  const gridStepY = subH >= 96 ? 20 : 14;
  const ptrSubX = Math.round(pxTarget * (subW - 1));

  // Precompute column-invariant dual-trace carrier waves, FFT spectrum levels, and cursor wake
  for (let sx = 0; sx < subW; sx++) {
    const ux = (sx + 0.5) * invSubW;
    const dxPtr = ux - pxTarget;
    const ptrArg = dxPtr * dxPtr * 22;
    const cursorWave = ptrArg < 8 ? fastFieldExpNeg(ptrArg) * cursorWeight : 0;

    let wakeWave = 0;
    if (wakeAmp > 0.004) {
      const behind = dxPtr * wakeDir;
      if (behind > -0.04 && behind < 0.42) {
        const wakeEnv = fastFieldExpNeg(behind * behind * 22);
        wakeWave = wakeEnv * Math.sin(behind * 34 - wakePhase * 6.5) * wakeAmp;
      }
    }

    let bandBoost = 0;
    let bandRaw = 0.18 + 0.12 * Math.sin(ux * 19 + time * 2.1);
    if (bandLen > 0) {
      const fIdx = ux * (bandLen - 1);
      const i0 = fIdx | 0;
      const i1 = i0 + 1 < bandLen ? i0 + 1 : bandLen - 1;
      const frac = fIdx - i0;
      const smoothFrac = frac * frac * (3 - 2 * frac);
      const b0 = bands![i0] ?? 0;
      const b1 = bands![i1] ?? b0;
      bandRaw = b0 + (b1 - b0) * smoothFrac;
      bandBoost = bandRaw * (isRibbon ? 0.11 : 0.105);
    }

    const pulseMod = isPulse
      ? Math.sin(Math.abs(dxPtr) * 34 - time * 5.2) * (0.04 + energy * 0.065) * fastFieldExpNeg(dxPtr * dxPtr * 4.2)
      : 0;

    // High-frequency amplitude-modulated carrier ripple for an oscillographic RF feel
    const amEnvelope = 0.55 + 0.45 * Math.cos(ux * 9.5 - time * 1.15);
    const carrierRipple = Math.sin(ux * 44 - time * 3.4) * (0.008 + energy * 0.022) * amEnvelope;

    if (isRibbon) {
      colCrest1[sx] =
        0.50 -
        Math.sin(ux * 6.4 + time * 1.45) * c1Amp -
        Math.cos(ux * 13.5 - time * 1.85) * (0.052 + energy * 0.045) -
        cursorWave * 0.76 -
        wakeWave * 0.85 -
        bandBoost;

      colCrest1Echo[sx] =
        0.50 -
        Math.sin(ux * 6.4 + time * 1.45 - 0.65) * (c1Amp * 0.86) +
        Math.sin(ux * 16.5 + time * 2.15) * (0.032 + energy * 0.025);
      colCrest2[sx] =
        0.52 -
        Math.sin(ux * 4.6 - time * 0.98 + 1.6) * c2Amp -
        Math.sin(ux * 18.0 + time * 2.25) * 0.032 -
        cursorWave * 0.48 -
        wakeWave * 0.55 -
        bandBoost * 0.55;

      colCrest3[sx] = 0.5;
      colCrest4[sx] = 0.5;
      colCrest5[sx] = 0.5;
      colSpectrumTop[sx] = 1.0;
    } else {
      colCrest1[sx] = Math.max(
        0.16,
        0.31 -
          Math.sin(ux * 6.6 + time * 0.88) * c1Amp -
          Math.cos(ux * 15.2 - time * 1.32) * 0.028 -
          carrierRipple -
          cursorWave * 0.76 -
          wakeWave * 0.80 -
          bandBoost * 0.72 -
          pulseMod
      );

      // Phase-lagged dual-trace echo creates crisp interference nodes along the upper carrier
      colCrest1Echo[sx] = Math.max(
        0.17,
        0.34 -
          Math.sin(ux * 6.6 + time * 0.88 - 0.55) * (c1Amp * 0.88) +
          Math.sin(ux * 19.0 + time * 1.75) * 0.020 -
          cursorWave * 0.48 -
          bandBoost * 0.44
      );

      colCrest2[sx] =
        0.47 -
        Math.sin(ux * 5.2 - time * 0.68 + 1.45) * c2Amp -
        Math.sin(ux * 18.4 + time * 1.85) * 0.022 +
        carrierRipple * 0.55 -
        cursorWave * 0.54 -
        wakeWave * 0.48 -
        bandBoost * 0.50 -
        pulseMod * 0.5;

      colCrest3[sx] =
        0.62 -
        Math.cos(ux * 8.4 + time * 1.12 + 0.6) * c3Amp -
        Math.sin(ux * 16.0 - time * 0.98) * 0.018 -
        cursorWave * 0.32 -
        bandBoost * 0.36;

      colCrest4[sx] =
        0.77 -
        Math.sin(ux * 6.0 - time * 0.78 + 2.35) * c4Amp -
        Math.cos(ux * 13.6 + time * 1.36) * 0.018 -
        bandBoost * 0.26;

      colCrest5[sx] = Math.min(
        0.955,
        0.895 -
          Math.cos(ux * 7.4 + time * 0.94 + 1.1) * c5Amp -
          Math.sin(ux * 16.8 - time * 1.16) * 0.016 -
          bandBoost * 0.22
      );

      // Lower-edge quantized FFT micro-needle ceiling (rising subtly from bottom baseline uy=0.98)
      const needleMod = 0.5 + 0.5 * Math.sin(ux * 31.0 - time * 2.8) * Math.cos(ux * 13.0 + time * 1.5);
      const needleHeight = 0.045 + (bandRaw * 0.55 + needleMod * 0.45) * (0.055 + energy * 0.09);
      colSpectrumTop[sx] = 0.985 - needleHeight;
    }

    if (isPeaks) {
      const barPos = ux * barCount;
      const barIdx = barPos | 0;
      const barFrac = barPos - barIdx - 0.5;
      const barCenterU = (barIdx + 0.5) / barCount;
      const barGate = barFrac > -0.34 && barFrac < 0.34 ? 1 : 0;
      let barBand = 0.14 + 0.12 * Math.sin(barCenterU * 11 + time * 2.4);
      if (bandLen > 0) {
        const bi = Math.min(bandLen - 1, Math.floor(barCenterU * bandLen));
        barBand = (bands![bi] ?? 0) * 0.42 + 0.08;
      }
      const ptrBar = fastFieldExpNeg((barCenterU - pxTarget) * (barCenterU - pxTarget) * 20) * (0.12 + energy * 0.14);
      colBarGate[sx] = barGate;
      colPeakTop[sx] = clamp(0.88 - (barBand * (0.55 + energy * 0.75) + ptrBar), 0.18, 0.86);
    }

    const edgeTaper = isRibbon
      ? smoothstep(0.0, 0.075, ux) * (1 - smoothstep(0.925, 1.0, ux))
      : 1;
    const harmonicShimmer = 0.74 * Math.sin(ux * 22 - time * 3.4) + 0.26 * Math.cos(ux * 11 + time * 1.85);
    colShimmer[sx] = (0.94 + shimmerScale * harmonicShimmer) * 0.98 * edgeTaper;
    const sparkleGate = Math.sin(ux * 54 - time * 4.6) * Math.cos(ux * 31 + time * 2.9);
    colMist[sx] =
      !isRibbon && energy > 0.12
        ? clamp01(0.42 + 0.58 * sparkleGate) * energy * 0.42
        : 0;

    const rippleU = isPulse ? Math.abs(dxPtr) * 38 - time * 4.4 : ux * 28 - time * 1.75;
    colSinU[sx] = Math.sin(rippleU);
    colCosU[sx] = Math.cos(rippleU);

    const deepPhaseU = ux * 14 + time * 0.95;
    colDeepSinU[sx] = Math.sin(deepPhaseU);
    colDeepCosU[sx] = Math.cos(deepPhaseU);
  }

  for (let sy = 0; sy < subH; sy++) {
    const uy = (sy + 0.5) * invSubH; // 0 (top) .. 1 (bottom)
    // Guarantee row 0 (sy < 4) stays completely clean while bottom telemetry flows all the way to uy=1.0
    const topFade = isRibbon
      ? smoothstep(0.04, 0.28, uy)
      : sy < 4
      ? 0
      : smoothstep(0.08, 0.38, uy);
    const bottomTaper = isRibbon
      ? 1 - smoothstep(0.76, 0.96, uy)
      : 1 - 0.10 * smoothstep(0.93, 1.0, uy);
    const fadeEnvelope = topFade * bottomTaper;
    const rowOff = sy * subW;

    if (fadeEnvelope <= 0.001) {
      subLum.fill(0, rowOff, rowOff + subW);
      continue;
    }

    const isGridRow = !isRibbon && sy % gridStepY === 0;
    const isCrossArmY = !isRibbon && (sy % gridStepY === 1 || sy % gridStepY === gridStepY - 1);
    const isBaselineRule =
      !isRibbon &&
      (Math.abs(uy - 0.52) < invSubH * 0.55 || Math.abs(uy - 0.80) < invSubH * 0.55);
    const bottomAnchorWeight = isRibbon
      ? 0
      : smoothstep(0.35, 0.75, uy) * (1 - 0.15 * smoothstep(0.92, 1.0, uy)) * (isPulse ? 0.075 : 0.036);
    const peakScanMask = isPeaks && (sy % 3 !== 2) ? 1 : 0.42;

    // High-Q sharp oscilloscope strokes
    const stroke1 = isRibbon ? 46 : 56;
    const strokeEcho = 60;
    const stroke2 = isRibbon ? 38 : 48;

    for (let sx = 0; sx < subW; sx++) {
      const crest1 = colCrest1[sx];
      const crest1Echo = colCrest1Echo[sx];
      const crest2 = colCrest2[sx];
      const crest3 = colCrest3[sx];
      const crest4 = colCrest4[sx];
      const crest5 = colCrest5[sx];

      const d1 = uy > crest1 ? uy - crest1 : crest1 - uy;
      const d2 = uy > crest2 ? uy - crest2 : crest2 - uy;

      const s1 = (uy > crest1 ? d1 * 0.84 : d1) * stroke1;
      const s2 = (uy > crest2 ? d2 * 0.86 : d2) * stroke2;
      const line1 = s1 < 2.83 ? fastFieldExpNeg(s1 * s1) * Math.min(1.16, colShimmer[sx] * 1.14) : 0;

      if (isRibbon) {
        const edgeMask = colShimmer[sx] > 0 ? Math.min(1, colShimmer[sx] / 0.94) : 0;
        const ribbonSecondWeight = 0.72 * edgeMask;
        const dEchoR = uy > crest1Echo ? uy - crest1Echo : crest1Echo - uy;
        const sEchoR = dEchoR * 52;
        const lineEchoR = sEchoR < 2.83 ? fastFieldExpNeg(sEchoR * sEchoR) * 0.48 * edgeMask : 0;
        const line2 = s2 < 2.83 ? fastFieldExpNeg(s2 * s2) * ribbonSecondWeight : 0;
        const tick = sx % sampleStep === 0 && sy === Math.floor(subH * 0.5) ? 0.48 * edgeMask : 0;
        const maxLine = Math.max(line1, line2, lineEchoR, tick);
        subLum[rowOff + sx] = clamp01(maxLine * fadeEnvelope);
        continue;
      }

      const dEcho = uy > crest1Echo ? uy - crest1Echo : crest1Echo - uy;
      const sEcho = dEcho * strokeEcho;
      const lineEcho = sEcho < 2.83 ? fastFieldExpNeg(sEcho * sEcho) * 0.54 : 0;

      // Interference node highlight where primary and echo traces cross
      const crossGap = Math.abs(crest1 - crest1Echo);
      const nodeLock =
        crossGap < 0.018 && d1 < 0.020
          ? (1 - crossGap / 0.018) * (1 - d1 / 0.020) * 0.48
          : 0;

      const d3 = uy > crest3 ? uy - crest3 : crest3 - uy;
      const d4 = uy > crest4 ? uy - crest4 : crest4 - uy;
      const d5 = uy > crest5 ? uy - crest5 : crest5 - uy;
      const s3 = (uy > crest3 ? d3 * 0.88 : d3) * 44;
      const s4 = (uy > crest4 ? d4 * 0.88 : d4) * 40;
      const s5 = (uy > crest5 ? d5 * 0.90 : d5) * 38;

      const line2 = s2 < 2.83 ? fastFieldExpNeg(s2 * s2) * 0.80 : 0;
      const line3 = s3 < 2.83 ? fastFieldExpNeg(s3 * s3) * 0.64 : 0;
      const line4 = s4 < 2.83 ? fastFieldExpNeg(s4 * s4) * 0.54 : 0;
      const line5 = s5 < 2.83 ? fastFieldExpNeg(s5 * s5) * 0.46 : 0;

      // Quantized vertical sample-and-hold stems + crisp crest sample pins every sampleStep columns
      let telemetryStem = 0;
      let samplePin = 0;
      if (sx % sampleStep === 0) {
        const upperSpan = Math.min(crest1, crest2);
        const lowerSpan = Math.max(crest1, crest2);
        if (uy >= upperSpan && uy <= lowerSpan && sy % 4 === 0) {
          telemetryStem = 0.24;
        }
        if (d1 < invSubH * 0.85 || d2 < invSubH * 0.75) {
          samplePin = 0.94;
        }
      }

      // Whisper-quiet precision optical graticule: evenly-spaced '+' crosshairs so waves stay primary
      let graticule = 0;
      const modX = sx % gridStepX;
      if (isGridRow && modX === 0) {
        graticule = 0.28;
      } else if ((isGridRow && (modX === 1 || modX === gridStepX - 1)) || (isCrossArmY && modX === 0)) {
        graticule = 0.16;
      } else if (isBaselineRule && sx % 6 === 0) {
        graticule = 0.14;
      } else if (sx % gridStepX === 0 && sy % 6 === 0) {
        graticule = 0.12;
      }

      // Precision vertical cursor-lock tracking reticle at active pointer column
      let cursorTick = 0;
      if (Math.abs(sx - ptrSubX) <= 0 && sy % 2 === 0 && uy >= Math.max(0.14, crest1 - 0.10) && uy <= crest2 + 0.08) {
        cursorTick = 0.64;
      }

      // Quantized FFT micro-spectrum needles anchored along the bottom floor (uy ~ 0.88..0.985)
      let spectrumNeedle = 0;
      if (sx % 6 === 0 && uy >= colSpectrumTop[sx] && uy <= 0.985) {
        const isTip = uy - colSpectrumTop[sx] < invSubH * 1.4;
        spectrumNeedle = isTip ? 0.78 : sy % 2 === 0 ? 0.42 : 0;
      }

      // Subtle rising mist spray when voice energy spikes
      let mist = 0;
      const mistOffset = crest1 - uy;
      if (mistOffset > 0.008 && mistOffset < 0.12 && colMist[sx] > 0.02) {
        mist = colMist[sx] * fastFieldExpNeg(mistOffset * mistOffset * 185);
      }

      // Tight localized falloff skirts right below main crests so troughs remain pure dark obsidian
      const skirt1 = uy > crest1 && d1 < 0.10 ? fastFieldExpNeg(d1 * d1 * 240) * 0.065 : 0;
      const skirt2 = uy > crest2 && d2 < 0.09 ? fastFieldExpNeg(d2 * d2 * 260) * 0.055 : 0;
      const skirt4 = uy > crest4 && d4 < 0.08 ? fastFieldExpNeg(d4 * d4 * 280) * 0.05 : 0;

      const sinRipple = isPulse ? colSinU[sx] * Math.cos(uy * 14) + colCosU[sx] * Math.sin(uy * 14) : 0;
      const fineRipple = isPulse ? (0.5 + 0.5 * sinRipple) * bottomAnchorWeight : 0;

      let peakBoost = 0;
      if (isPeaks && colBarGate[sx] > 0.5) {
        const peakTop = colPeakTop[sx];
        if (uy >= peakTop && uy < 0.94) {
          const capDist = uy - peakTop;
          const capGlow = capDist < 0.055 ? 0.88 : 0.48 * (1 - smoothstep(peakTop, 0.94, uy) * 0.42);
          peakBoost = capGlow * peakScanMask;
        }
      }

      let maxTrace = line1;
      if (lineEcho > maxTrace) maxTrace = lineEcho;
      if (line2 > maxTrace) maxTrace = line2;
      if (line3 > maxTrace) maxTrace = line3;
      if (line4 > maxTrace) maxTrace = line4;
      if (line5 > maxTrace) maxTrace = line5;
      if (peakBoost > maxTrace) maxTrace = peakBoost;
      if (samplePin > maxTrace) maxTrace = samplePin;
      if (spectrumNeedle > maxTrace) maxTrace = spectrumNeedle;

      const infra = Math.max(graticule, telemetryStem, cursorTick);
      const signalBase = maxTrace > infra ? maxTrace : infra;
      const lum = clamp01((signalBase + nodeLock + skirt1 + skirt2 + skirt4 + fineRipple + mist) * fadeEnvelope);
      subLum[rowOff + sx] = lum;
    }
  }

  const subLumOut = new Uint8Array(subW * subH);

  for (let r = 0; r < rows; r++) {
    const syBase = r * 4;
    const r0 = syBase * subW;
    const r1 = r0 + subW;
    const r2 = r1 + subW;
    const r3 = r2 + subW;
    const bRow0 = (syBase & 7) << 3;
    const bRow1 = ((syBase + 1) & 7) << 3;
    const bRow2 = ((syBase + 2) & 7) << 3;
    const bRow3 = ((syBase + 3) & 7) << 3;
    const cellRowOff = r * cols;

    for (let c = 0; c < cols; c++) {
      const c2 = c * 2;
      const c2_1 = c2 + 1;
      const bx0 = c2 & 7;
      const bx1 = c2_1 & 7;

      const l00 = subLum[r0 + c2];
      const l01 = subLum[r0 + c2_1];
      const l10 = subLum[r1 + c2];
      const l11 = subLum[r1 + c2_1];
      const l20 = subLum[r2 + c2];
      const l21 = subLum[r2 + c2_1];
      const l30 = subLum[r3 + c2];
      const l31 = subLum[r3 + c2_1];

      const lumSum = l00 + l01 + l10 + l11 + l20 + l21 + l30 + l31;
      let maxSub = l00;
      if (l01 > maxSub) maxSub = l01;
      if (l10 > maxSub) maxSub = l10;
      if (l11 > maxSub) maxSub = l11;
      if (l20 > maxSub) maxSub = l20;
      if (l21 > maxSub) maxSub = l21;
      if (l30 > maxSub) maxSub = l30;
      if (l31 > maxSub) maxSub = l31;

      let bits = 0;
      if (l00 > FIELD_DITHER_THRESH[bRow0 + bx0]) {
        bits |= 0x01;
        subLumOut[r0 + c2] = (l00 * 255 + 0.5) | 0;
      }
      if (l01 > FIELD_DITHER_THRESH[bRow0 + bx1]) {
        bits |= 0x08;
        subLumOut[r0 + c2_1] = (l01 * 255 + 0.5) | 0;
      }
      if (l10 > FIELD_DITHER_THRESH[bRow1 + bx0]) {
        bits |= 0x02;
        subLumOut[r1 + c2] = (l10 * 255 + 0.5) | 0;
      }
      if (l11 > FIELD_DITHER_THRESH[bRow1 + bx1]) {
        bits |= 0x10;
        subLumOut[r1 + c2_1] = (l11 * 255 + 0.5) | 0;
      }
      if (l20 > FIELD_DITHER_THRESH[bRow2 + bx0]) {
        bits |= 0x04;
        subLumOut[r2 + c2] = (l20 * 255 + 0.5) | 0;
      }
      if (l21 > FIELD_DITHER_THRESH[bRow2 + bx1]) {
        bits |= 0x20;
        subLumOut[r2 + c2_1] = (l21 * 255 + 0.5) | 0;
      }
      if (l30 > FIELD_DITHER_THRESH[bRow3 + bx0]) {
        bits |= 0x40;
        subLumOut[r3 + c2] = (l30 * 255 + 0.5) | 0;
      }
      if (l31 > FIELD_DITHER_THRESH[bRow3 + bx1]) {
        bits |= 0x80;
        subLumOut[r3 + c2_1] = (l31 * 255 + 0.5) | 0;
      }

      // Peak-weighted cell brightness keeps sharp wave crests luminous while subtler bathymetry stays soft
      const cellIntensity = (lumSum * 0.125) * 0.52 + maxSub * 0.48;
      cells[cellRowOff + c] = Math.min(255, (cellIntensity * 255 + 0.5) | 0);
      rowChars[c] = BRAILLE_CHAR_TABLE[bits];
    }
    lines[r] = rowChars.join('');
  }

  return { cols, rows, lines, cells, subLum: subLumOut };
}
