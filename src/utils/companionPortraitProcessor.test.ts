import test from 'node:test';
import assert from 'node:assert/strict';
import type { PortraitSourceField } from './asciiPortraitEngine';
import { BRAILLE_SHAPE_CATALOG, type BrailleShapeMotionState } from './brailleShapesEngine';
import { NEUTRAL_IDLE_POSE } from './brailleIdleLoops';
import {
  COMPANION_PORTRAIT_PROFILES,
  renderCompanionPortraitFrame,
  resolveCompanionMouthTargets,
} from './companionPortraitProcessor';

function makeSyntheticPortraitField(size = 96): PortraitSourceField {
  const baseLum = new Float32Array(size * size);
  const inkMask = new Float32Array(size * size);
  for (let y = 0; y < size; y++) {
    const uy = y / (size - 1);
    for (let x = 0; x < size; x++) {
      const ux = x / (size - 1);
      const idx = y * size + x;
      const inBust = Math.hypot((ux - 0.5) * 1.15, (uy - 0.54) * 1.05) < 0.44;
      if (!inBust) continue;
      // Bright face skin with dark eye irises and closed mouth line
      const inLeftIris = Math.hypot((ux - 0.352) / 0.045, (uy - 0.522) / 0.036) < 1;
      const inRightIris = Math.hypot((ux - 0.545) / 0.045, (uy - 0.525) / 0.036) < 1;
      const onMouthLine = Math.abs(ux - 0.448) < 0.045 && Math.abs(uy - 0.695) < 0.012;
      if (inLeftIris || inRightIris || onMouthLine) {
        baseLum[idx] = 0.06;
        inkMask[idx] = 0.92;
      } else {
        baseLum[idx] = 0.72;
        inkMask[idx] = 0.04;
      }
    }
  }
  return { width: size, height: size, baseLum, inkMask };
}

function makeMotion(overrides: Partial<BrailleShapeMotionState> = {}): BrailleShapeMotionState {
  return {
    blink: 0,
    squint: 0,
    wide: 0,
    mouthOpen: 0,
    smile: 0.25,
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

test('COMPANION_PORTRAIT_PROFILES defines calibrated eye, mouth, and neck-pivot anatomy for all 9 companions', () => {
  assert.equal(Object.keys(COMPANION_PORTRAIT_PROFILES).length, 9);
  for (const meta of BRAILLE_SHAPE_CATALOG) {
    const p = COMPANION_PORTRAIT_PROFILES[meta.kind];
    assert.ok(p, `Missing profile for ${meta.kind}`);
    assert.ok(p.pivotY >= 0.72 && p.pivotY <= 0.88, `${meta.kind} pivotY should anchor near neck/shoulders`);
    assert.ok(p.mouth.cy > p.leftEye.cy, `${meta.kind} mouth must sit below eyes`);
    assert.ok(p.mouth.chinY > p.mouth.cy, `${meta.kind} chin must sit below mouth`);
    assert.ok(p.browY < p.leftEye.cy, `${meta.kind} brow must sit above eyes`);
  }
});

test('renderCompanionPortraitFrame animates real eye blinks and distinct talking visemes', () => {
  const sourceField = makeSyntheticPortraitField(96);
  const cols = 48;
  const rows = 28;

  const fOpen = renderCompanionPortraitFrame({
    sourceField,
    kind: 'ghost',
    cols,
    rows,
    time: 0,
    energy: 0.7,
    isSpeaking: true,
    isThinking: false,
    viseme: 'closed',
    motion: makeMotion({ blink: 0 }),
  });

  const fBlink = renderCompanionPortraitFrame({
    sourceField,
    kind: 'ghost',
    cols,
    rows,
    time: 0,
    energy: 0.7,
    isSpeaking: true,
    isThinking: false,
    viseme: 'closed',
    motion: makeMotion({ blink: 1 }),
  });

  const fWide = renderCompanionPortraitFrame({
    sourceField,
    kind: 'ghost',
    cols,
    rows,
    time: 0,
    energy: 0.7,
    isSpeaking: true,
    isThinking: false,
    viseme: 'wide',
    motion: makeMotion({ blink: 0, mouthOpen: 0.96 }),
  });

  const fRound = renderCompanionPortraitFrame({
    sourceField,
    kind: 'ghost',
    cols,
    rows,
    time: 0,
    energy: 0.7,
    isSpeaking: true,
    isThinking: false,
    viseme: 'round',
    motion: makeMotion({ blink: 0, mouthOpen: 0.72 }),
  });

  const fEe = renderCompanionPortraitFrame({
    sourceField,
    kind: 'ghost',
    cols,
    rows,
    time: 0,
    energy: 0.7,
    isSpeaking: true,
    isThinking: false,
    viseme: 'ee',
    motion: makeMotion({ blink: 0, mouthOpen: 0.56 }),
  });

  const countLineDiffs = (a: string[], b: string[]) => {
    let diffs = 0;
    for (let r = 0; r < a.length; r++) {
      for (let c = 0; c < a[r].length; c++) {
        if (a[r][c] !== b[r][c]) diffs++;
      }
    }
    return diffs;
  };

  assert.ok(countLineDiffs(fOpen.lines, fBlink.lines) >= 8, 'Blink must visibly alter eye Braille cells');
  assert.ok(countLineDiffs(fOpen.lines, fWide.lines) >= 6, 'Wide talking viseme must visibly open mouth');
  assert.ok(countLineDiffs(fWide.lines, fRound.lines) >= 4, 'Round O/Oo must differ visually from Wide A/Ha');
  assert.ok(countLineDiffs(fWide.lines, fEe.lines) >= 4, 'Wide Ee must differ visually from Wide A/Ha');

  const mtWide = resolveCompanionMouthTargets('wide', 0.85, true, 'speaking');
  const mtMbp = resolveCompanionMouthTargets('mbp', 0.35, true, 'speaking');
  assert.ok(mtWide.open > 0.9 && mtWide.jawDrop > 0.9);
  assert.equal(mtMbp.open, 0);
  assert.equal(mtMbp.lipPress, 1);
});
