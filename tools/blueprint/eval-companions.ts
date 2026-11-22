import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import type { MouthViseme } from '../../src/types/hermes';
import {
  BRAILLE_TINT_PALETTE,
  BRAILLE_SHAPE_CATALOG,
  type BrailleShapeKind,
  type BrailleShapeMotionState,
} from '../../src/utils/brailleShapesEngine';
import { NEUTRAL_IDLE_POSE } from '../../src/utils/brailleIdleLoops';
import {
  buildCompanionPortraitSourceFromRgba,
  renderCompanionPortraitFrame,
  type CompanionPortraitRenderParams,
} from '../../src/utils/companionPortraitProcessor';
import type { PortraitSourceField } from '../../src/utils/asciiPortraitEngine';

const IMAGE_FILES: Record<BrailleShapeKind, string> = {
  clover: 'src/assets/images/char_v2_cyber_bob_1795316021855.jpg',
  star: 'src/assets/images/char_v2_twintails_1795316023562.jpg',
  ghost: 'src/assets/images/char_v2_neko_1795316024406.jpg',
  heart: 'src/assets/images/char_v2_kitsune_1795316022069.jpg',
  cloud: 'src/assets/images/char_v2_usagi_1795316024186.jpg',
  triangle: 'src/assets/images/char_v2_mecha_1795316022949.jpg',
  pebble: 'src/assets/images/char_v2_yume_1795316022834.jpg',
  trapezoid: 'src/assets/images/char_v2_seraph_1795316124290.jpg',
  blob: 'src/assets/images/char_v2_wolfcut_1795316023424.jpg',
};

// Measured eye & mouth bounding boxes (in portrait UV 0..1) from grid calibration
const ANATOMY_BOXES: Record<
  BrailleShapeKind,
  {
    eyeBox: [number, number, number, number];
    mouthBox: [number, number, number, number];
  }
> = {
  clover: { eyeBox: [0.16, 0.40, 0.48, 0.50], mouthBox: [0.25, 0.59, 0.41, 0.72] },
  star: { eyeBox: [0.58, 0.39, 0.76, 0.54], mouthBox: [0.54, 0.60, 0.71, 0.73] },
  ghost: { eyeBox: [0.27, 0.46, 0.63, 0.58], mouthBox: [0.37, 0.64, 0.53, 0.76] },
  heart: { eyeBox: [0.44, 0.46, 0.71, 0.57], mouthBox: [0.54, 0.61, 0.68, 0.73] },
  cloud: { eyeBox: [0.30, 0.50, 0.59, 0.63], mouthBox: [0.38, 0.64, 0.52, 0.76] },
  triangle: { eyeBox: [0.52, 0.42, 0.79, 0.51], mouthBox: [0.58, 0.59, 0.72, 0.71] },
  pebble: { eyeBox: [0.26, 0.39, 0.56, 0.51], mouthBox: [0.34, 0.55, 0.47, 0.64] },
  trapezoid: { eyeBox: [0.33, 0.43, 0.67, 0.53], mouthBox: [0.43, 0.56, 0.57, 0.66] },
  blob: { eyeBox: [0.44, 0.41, 0.74, 0.51], mouthBox: [0.54, 0.59, 0.70, 0.72] },
};

const SUBPIXEL_BITS: ReadonlyArray<readonly [number, number, number]> = [
  [0, 0, 0x01],
  [1, 0, 0x08],
  [0, 1, 0x02],
  [1, 1, 0x10],
  [0, 2, 0x04],
  [1, 2, 0x20],
  [0, 3, 0x40],
  [1, 3, 0x80],
];

function decodePortrait(imgPath: string, kind: BrailleShapeKind, size = 160): PortraitSourceField {
  const raw = execFileSync(
    'ffmpeg',
    ['-v', 'error', '-i', imgPath, '-vf', `scale=${size}:${size}`, '-f', 'rawvideo', '-pix_fmt', 'rgba', '-'],
    { maxBuffer: 16 * 1024 * 1024 }
  );
  return buildCompanionPortraitSourceFromRgba(new Uint8ClampedArray(raw), size, size, kind);
}

function makeMotion(overrides: Partial<BrailleShapeMotionState> = {}): BrailleShapeMotionState {
  return {
    blink: 0,
    squint: 0,
    wide: 0,
    mouthOpen: 0,
    smile: 0.35,
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
    idle: {
      setId: 'tactician',
      actionIndex: 0,
      actionId: 'rest',
      actionLabel: 'Rest',
      isResting: true,
      envelope: 0,
      phase: 0,
      pose: NEUTRAL_IDLE_POSE,
    },
    ...overrides,
  };
}

function frameToDotGrid(lines: string[], cols: number, rows: number): Uint8Array {
  const subW = cols * 2;
  const subH = rows * 4;
  const grid = new Uint8Array(subW * subH);
  for (let r = 0; r < rows; r++) {
    const line = lines[r];
    for (let c = 0; c < cols; c++) {
      const code = line.charCodeAt(c);
      if (code <= 0x2800 || code > 0x28ff) continue;
      const bits = code - 0x2800;
      for (const [dx, dy, mask] of SUBPIXEL_BITS) {
        if (bits & mask) {
          grid[(r * 4 + dy) * subW + (c * 2 + dx)] = 1;
        }
      }
    }
  }
  return grid;
}

function boxDiffRatio(
  a: Uint8Array,
  b: Uint8Array,
  subW: number,
  subH: number,
  box: [number, number, number, number]
): { diffDots: number; totalDots: number; ratio: number } {
  const x0 = Math.max(0, Math.floor(box[0] * subW));
  const y0 = Math.max(0, Math.floor(box[1] * subH));
  const x1 = Math.min(subW - 1, Math.ceil(box[2] * subW));
  const y1 = Math.min(subH - 1, Math.ceil(box[3] * subH));
  let diffDots = 0;
  let totalDots = 0;
  for (let y = y0; y <= y1; y++) {
    for (let x = x0; x <= x1; x++) {
      totalDots++;
      if (a[y * subW + x] !== b[y * subW + x]) diffDots++;
    }
  }
  return { diffDots, totalDots, ratio: totalDots > 0 ? diffDots / totalDots : 0 };
}

function centroidYInBand(
  grid: Uint8Array,
  subW: number,
  subH: number,
  yMinFrac: number,
  yMaxFrac: number
): number {
  const y0 = Math.floor(subH * yMinFrac);
  const y1 = Math.min(subH - 1, Math.ceil(subH * yMaxFrac));
  let sumY = 0;
  let count = 0;
  for (let y = y0; y <= y1; y++) {
    for (let x = 0; x < subW; x++) {
      if (grid[y * subW + x]) {
        sumY += y;
        count++;
      }
    }
  }
  return count > 0 ? sumY / count / subH : 0.5;
}

function hexToRgb(hex: string): [number, number, number] {
  const n = parseInt(hex.slice(1), 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

function rasterizeCellToSheet(
  sheet: Uint8Array,
  sheetW: number,
  sheetH: number,
  offX: number,
  offY: number,
  tileW: number,
  tileH: number,
  lines: string[],
  subLum: Uint8Array | undefined,
  cols: number,
  rows: number,
  lowHex: string,
  midHex: string,
  highHex: string
) {
  const subW = cols * 2;
  const subH = rows * 4;
  const [lr, lg, lb] = hexToRgb(lowHex);
  const [mr, mg, mb] = hexToRgb(midHex);
  const [hr, hg, hb] = hexToRgb(highHex);

  const scaleX = tileW / subW;
  const scaleY = tileH / subH;

  for (let r = 0; r < rows; r++) {
    const line = lines[r];
    for (let c = 0; c < cols; c++) {
      const code = line.charCodeAt(c);
      if (code <= 0x2800 || code > 0x28ff) continue;
      const bits = code - 0x2800;
      for (const [dx, dy, mask] of SUBPIXEL_BITS) {
        if (!(bits & mask)) continue;
        const sx = c * 2 + dx;
        const sy = r * 4 + dy;
        const lum = subLum ? subLum[sy * subW + sx] / 255 : 0.75;
        let rr: number;
        let gg: number;
        let bb: number;
        if (lum < 0.5) {
          const t = lum * 2;
          rr = Math.round(lr + (mr - lr) * t);
          gg = Math.round(lg + (mg - lg) * t);
          bb = Math.round(lb + (mb - lb) * t);
        } else {
          const t = (lum - 0.5) * 2;
          rr = Math.round(mr + (hr - mr) * t);
          gg = Math.round(mg + (hg - mg) * t);
          bb = Math.round(mb + (hb - mb) * t);
        }
        const px = Math.floor(offX + sx * scaleX);
        const py = Math.floor(offY + sy * scaleY);
        for (let oy = 0; oy < 2; oy++) {
          for (let ox = 0; ox < 2; ox++) {
            const x = px + ox;
            const y = py + oy;
            if (x < 0 || y < 0 || x >= sheetW || y >= sheetH) continue;
            const idx = (y * sheetW + x) * 4;
            sheet[idx] = rr;
            sheet[idx + 1] = gg;
            sheet[idx + 2] = bb;
            sheet[idx + 3] = 255;
          }
        }
      }
    }
  }
}

function saveRgbaPng(rgba: Uint8Array, w: number, h: number, outPath: string) {
  fs.mkdirSync(path.dirname(outPath), { recursive: true });
  execFileSync(
    'ffmpeg',
    [
      '-y',
      '-v',
      'error',
      '-f',
      'rawvideo',
      '-pix_fmt',
      'rgba',
      '-s',
      `${w}x${h}`,
      '-i',
      '-',
      outPath,
    ],
    { input: Buffer.from(rgba) }
  );
}

const args = process.argv.slice(2);
const labelIdx = args.indexOf('--label');
const label = labelIdx >= 0 && args[labelIdx + 1] ? args[labelIdx + 1] : 'current';

const cols = 66;
const rows = 36;
const subW = cols * 2;
const subH = rows * 4;

const tileW = 240;
const tileH = 252;
const pad = 8;
const numCols = 5;
const numRows = BRAILLE_SHAPE_CATALOG.length;
const sheetW = numCols * (tileW + pad) + pad;
const sheetH = numRows * (tileH + pad) + pad;
const sheet = new Uint8Array(sheetW * sheetH * 4);
for (let i = 0; i < sheet.length; i += 4) {
  sheet[i] = 10;
  sheet[i + 1] = 12;
  sheet[i + 2] = 18;
  sheet[i + 3] = 255;
}

interface CompanionScore {
  kind: BrailleShapeKind;
  label: string;
  blinkEyeRatio: number;
  talkWideMouthRatio: number;
  talkRoundVsWideRatio: number;
  shoulderFloatDrift: number;
  headVsShoulderRatio: number;
}

const scores: CompanionScore[] = [];

for (let rIdx = 0; rIdx < BRAILLE_SHAPE_CATALOG.length; rIdx++) {
  const meta = BRAILLE_SHAPE_CATALOG[rIdx];
  const kind = meta.kind;
  const imgPath = path.resolve(IMAGE_FILES[kind]);
  const sourceField = decodePortrait(imgPath, kind, 160);
  const palette = BRAILLE_TINT_PALETTE[meta.defaultTint];
  const boxes = ANATOMY_BOXES[kind];

  const renderState = (
    overrides: Partial<CompanionPortraitRenderParams> & { viseme?: MouthViseme }
  ) =>
    renderCompanionPortraitFrame({
      sourceField,
      kind,
      cols,
      rows,
      time: 0.4,
      energy: 0.08,
      isSpeaking: false,
      isThinking: false,
      gazeX: 0,
      gazeY: 0,
      motion: makeMotion(),
      ...overrides,
    });

  // 1. Neutral Idle (t=0.0) & Peak Breath Idle (t=1.2)
  const fIdle0 = renderState({ time: 0.0, motion: makeMotion({ bounceY: -0.025, dragTilt: -0.04 }) });
  const fIdle1 = renderState({ time: 1.4, motion: makeMotion({ bounceY: 0.025, dragTilt: 0.04 }) });
  const fNeutral = renderState({ time: 0.0, motion: makeMotion() });

  // 2. Full Blink
  const fBlink = renderState({ time: 0.0, motion: makeMotion({ blink: 1.0 }) });

  // 3. Controlled speaking comparison at identical energy (0.72) so ONLY viseme articulation changes
  const fClosedSpeak = renderState({
    time: 0.0,
    isSpeaking: true,
    energy: 0.72,
    viseme: 'closed',
    motion: makeMotion({ mouthOpen: 0.0, speakHalo: 0.7 }),
  });

  // 4. Talk Wide (A/Ha)
  const fWide = renderState({
    time: 0.0,
    isSpeaking: true,
    energy: 0.72,
    viseme: 'wide',
    motion: makeMotion({ mouthOpen: 0.96, speakHalo: 0.7 }),
  });

  // 5. Talk Round (O/Oo)
  const fRound = renderState({
    time: 0.0,
    isSpeaking: true,
    energy: 0.72,
    viseme: 'round',
    motion: makeMotion({ mouthOpen: 0.68, speakHalo: 0.7 }),
  });

  // 6. Talk Ee + Expressive Glance/Turn
  const fEeTurn = renderState({
    time: 0.0,
    isSpeaking: true,
    energy: 0.72,
    viseme: 'ee',
    gazeX: 0.85,
    gazeY: -0.25,
    motion: makeMotion({ mouthOpen: 0.56, smile: 0.95, gazeX: 0.85, gazeY: -0.25, dragTilt: 0.055, speakHalo: 0.7 }),
  });

  const gNeutral = frameToDotGrid(fNeutral.lines, cols, rows);
  const gIdle0 = frameToDotGrid(fIdle0.lines, cols, rows);
  const gIdle1 = frameToDotGrid(fIdle1.lines, cols, rows);
  const gBlink = frameToDotGrid(fBlink.lines, cols, rows);
  const gClosedSpeak = frameToDotGrid(fClosedSpeak.lines, cols, rows);
  const gWide = frameToDotGrid(fWide.lines, cols, rows);
  const gRound = frameToDotGrid(fRound.lines, cols, rows);

  // Subtract non-eye background delta so global dimming cannot fake a real eye blink
  const blinkEyeRaw = boxDiffRatio(gNeutral, gBlink, subW, subH, boxes.eyeBox).ratio;
  const blinkControl = boxDiffRatio(gNeutral, gBlink, subW, subH, [0.22, 0.72, 0.78, 0.86]).ratio;
  const blinkEyeNet = Math.max(0, blinkEyeRaw - blinkControl);

  const wideDiff = boxDiffRatio(gClosedSpeak, gWide, subW, subH, boxes.mouthBox);
  const roundVsWideDiff = boxDiffRatio(gWide, gRound, subW, subH, boxes.mouthBox);

  // Measure vertical float of shoulders (bottom 18%) vs head motion (top 55%)
  const shY0 = centroidYInBand(gIdle0, subW, subH, 0.78, 0.96);
  const shY1 = centroidYInBand(gIdle1, subW, subH, 0.78, 0.96);
  const shoulderFloatDrift = Math.abs(shY1 - shY0);

  const headDiff = boxDiffRatio(gIdle0, gIdle1, subW, subH, [0.15, 0.12, 0.85, 0.68]).ratio;
  const shoulderDiff = boxDiffRatio(gIdle0, gIdle1, subW, subH, [0.15, 0.80, 0.85, 0.96]).ratio;
  const headVsShoulderRatio = headDiff / Math.max(0.015, shoulderDiff);

  scores.push({
    kind,
    label: meta.label,
    blinkEyeRatio: blinkEyeNet,
    talkWideMouthRatio: wideDiff.ratio,
    talkRoundVsWideRatio: roundVsWideDiff.ratio,
    shoulderFloatDrift,
    headVsShoulderRatio,
  });

  const framesForRow = [fNeutral, fBlink, fWide, fRound, fEeTurn];
  for (let cIdx = 0; cIdx < framesForRow.length; cIdx++) {
    const fr = framesForRow[cIdx];
    const ox = pad + cIdx * (tileW + pad);
    const oy = pad + rIdx * (tileH + pad);
    rasterizeCellToSheet(
      sheet,
      sheetW,
      sheetH,
      ox,
      oy,
      tileW,
      tileH,
      fr.lines,
      fr.subLum,
      cols,
      rows,
      palette.low,
      palette.mid,
      palette.high
    );
  }
}

const outSheet = path.resolve(`blueprint-out/companions/${label}/sheet_companions.png`);
saveRgbaPng(sheet, sheetW, sheetH, outSheet);

// Compute aggregate uphill score out of 100
const avgBlink = scores.reduce((s, x) => s + x.blinkEyeRatio, 0) / scores.length;
const avgWideMouth = scores.reduce((s, x) => s + x.talkWideMouthRatio, 0) / scores.length;
const avgRoundVsWide = scores.reduce((s, x) => s + x.talkRoundVsWideRatio, 0) / scores.length;
const avgShoulderFloat = scores.reduce((s, x) => s + x.shoulderFloatDrift, 0) / scores.length;
const avgHeadVsShoulder = scores.reduce((s, x) => s + x.headVsShoulderRatio, 0) / scores.length;

const blinkScore = Math.min(25, (avgBlink / 0.14) * 25);
const talkMouthScore = Math.min(25, (avgWideMouth / 0.16) * 25);
const visemeDistinctScore = Math.min(20, (avgRoundVsWide / 0.08) * 20);
const antiFloatScore = Math.min(15, Math.max(0, (1 - avgShoulderFloat / 0.018) * 15));
const neckPivotScore = Math.min(15, Math.min(1, (avgHeadVsShoulder - 0.8) / 1.4) * 15);
const totalScore = Math.round(
  blinkScore + talkMouthScore + visemeDistinctScore + antiFloatScore + neckPivotScore
);

console.log(`\n=== Companion Animation Uphill Evaluation [${label}] ===`);
console.log(`Total Score: ${totalScore} / 100`);
console.log(`  Eye Blink Articulation : ${blinkScore.toFixed(1)} / 25  (avg eye-box delta ${(avgBlink * 100).toFixed(1)}%)`);
console.log(`  Talking Mouth Openness : ${talkMouthScore.toFixed(1)} / 25  (avg mouth-box delta ${(avgWideMouth * 100).toFixed(1)}%)`);
console.log(`  Viseme Distinctness    : ${visemeDistinctScore.toFixed(1)} / 20  (round vs wide delta ${(avgRoundVsWide * 100).toFixed(1)}%)`);
console.log(`  Grounded Anti-Float    : ${antiFloatScore.toFixed(1)} / 15  (avg shoulder drift ${(avgShoulderFloat * 100).toFixed(2)}%)`);
console.log(`  Neck-Pivot Kinematics  : ${neckPivotScore.toFixed(1)} / 15  (head/shoulder motion ratio ${avgHeadVsShoulder.toFixed(2)}x)`);
console.log(`\nPer-Companion Breakdown:`);
for (const s of scores) {
  console.log(
    `  ${s.label.padEnd(10)} | blink=${(s.blinkEyeRatio * 100).toFixed(1).padStart(5)}% | wide=${(s.talkWideMouthRatio * 100).toFixed(1).padStart(5)}% | rnd/wd=${(s.talkRoundVsWideRatio * 100).toFixed(1).padStart(5)}% | shDrift=${(s.shoulderFloatDrift * 100).toFixed(2)}% | head/sh=${s.headVsShoulderRatio.toFixed(2)}x`
  );
}
console.log(`\nSaved contact sheet: ${outSheet}`);
