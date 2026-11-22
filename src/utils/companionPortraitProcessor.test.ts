import test from 'node:test';
import assert from 'node:assert/strict';
import { createBrailleShapeAnimator } from './brailleShapesEngine';
import {
  buildCompanionPortraitSourceFromRgba,
  renderCompanionPortraitFrame,
} from './companionPortraitProcessor';

function makeSyntheticMangaPortrait(w: number, h: number, openBottomCollar = false): Uint8ClampedArray {
  const rgba = new Uint8ClampedArray(w * h * 4);
  // Start with pure opaque white background
  rgba.fill(255);

  const setBlack = (x: number, y: number) => {
    if (x < 0 || x >= w || y < 0 || y >= h) return;
    const off = (y * w + x) * 4;
    rgba[off] = 0;
    rgba[off + 1] = 0;
    rgba[off + 2] = 0;
    rgba[off + 3] = 255;
  };

  // Draw closed circular hair/head outline centered at (0.5, 0.5) with radius 0.34
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      const dx = x / w - 0.5;
      const dy = y / h - 0.5;
      const d = Math.hypot(dx, dy);
      const isHeadRing = d >= 0.24 && d <= 0.34;
      const isCollar = openBottomCollar && y > h * 0.88 && Math.abs(dx) < 0.14;
      if (isHeadRing && !isCollar) {
        setBlack(x, y);
      }
    }
  }
  return rgba;
}

test('buildCompanionPortraitSourceFromRgba strips opaque white background while keeping white anime face luminous', () => {
  const w = 64;
  const h = 64;
  const rgba = makeSyntheticMangaPortrait(w, h, false);
  const field = buildCompanionPortraitSourceFromRgba(rgba, w, h, 'ghost');

  // Outer corner must be stripped to 0
  assert.equal(field.baseLum[0], 0);
  assert.equal(field.baseLum[w - 1], 0);

  // Center white face inside black hair ring must stay luminous
  const centerIdx = Math.floor(h * 0.5) * w + Math.floor(w * 0.44);
  assert.ok(field.baseLum[centerIdx] > 0.35, `Expected bright face center, got ${field.baseLum[centerIdx]}`);
});

test('buildCompanionPortraitSourceFromRgba prevents open bottom collar leak into neck and face', () => {
  const w = 64;
  const h = 64;
  const rgba = makeSyntheticMangaPortrait(w, h, false);
  // Draw vertical black neck/shoulder walls from head bottom (y=0.72) down to y=0.92,
  // leaving the bottom center open to the bottom edge (just like open manga collar lines).
  const xLeft = Math.floor(w * 0.36);
  const xRight = Math.floor(w * 0.64);
  for (let y = Math.floor(h * 0.72); y < h; y++) {
    for (let x = xLeft + 2; x < xRight - 2; x++) {
      const off = (y * w + x) * 4;
      rgba[off] = 255;
      rgba[off + 1] = 255;
      rgba[off + 2] = 255;
      rgba[off + 3] = 255;
    }
    if (y < Math.floor(h * 0.92)) {
      for (const wallX of [xLeft, xLeft + 1, xRight - 1, xRight]) {
        const off = (y * w + wallX) * 4;
        rgba[off] = 0;
        rgba[off + 1] = 0;
        rgba[off + 2] = 0;
        rgba[off + 3] = 255;
      }
    }
  }

  const field = buildCompanionPortraitSourceFromRgba(rgba, w, h, 'blob');
  const centerIdx = Math.floor(h * 0.5) * w + Math.floor(w * 0.46);
  assert.ok(
    field.baseLum[centerIdx] > 0.3,
    `Expected collar floor guard to protect face center, got ${field.baseLum[centerIdx]}`
  );
});

test('buildCompanionPortraitSourceFromRgba clears hollow halo interior for Seraph (trapezoid)', () => {
  const w = 64;
  const h = 64;
  const rgba = makeSyntheticMangaPortrait(w, h, false);
  // Draw a thin black halo ellipse at top around (0.48, 0.06)
  for (let y = 1; y <= 8; y++) {
    for (let x = 16; x <= 46; x++) {
      const nx = (x - 31) / 14;
      const ny = (y - 4) / 3;
      const d = Math.hypot(nx, ny);
      if (d >= 0.78 && d <= 1.05) {
        const off = (y * w + x) * 4;
        rgba[off] = 0;
        rgba[off + 1] = 0;
        rgba[off + 2] = 0;
        rgba[off + 3] = 255;
      }
    }
  }

  const field = buildCompanionPortraitSourceFromRgba(rgba, w, h, 'trapezoid');
  const haloHoleIdx = Math.round(h * 0.056) * w + Math.round(w * 0.48);
  assert.equal(field.baseLum[haloHoleIdx], 0, 'Expected hollow center inside Seraph halo to remain dark');
});

test('renderCompanionPortraitFrame outputs exact dimensions, subpixel luminance, and responds to thinking/speaking', () => {
  const w = 64;
  const h = 64;
  const rgba = makeSyntheticMangaPortrait(w, h, false);
  const sourceField = buildCompanionPortraitSourceFromRgba(rgba, w, h, 'clover');

  const idle = renderCompanionPortraitFrame({
    sourceField,
    cols: 36,
    rows: 20,
    time: 1.0,
    energy: 0.08,
    isSpeaking: false,
    isThinking: false,
  });

  assert.equal(idle.cols, 36);
  assert.equal(idle.rows, 20);
  assert.equal(idle.lines.length, 20);
  assert.ok(idle.subLum && idle.subLum.length === 36 * 2 * 20 * 4);
  const nonEmptyChars = idle.lines.join('').replace(/ /g, '').length;
  assert.ok(nonEmptyChars > 40, `Expected rich Braille output, got ${nonEmptyChars} chars`);

  const speaking = renderCompanionPortraitFrame({
    sourceField,
    cols: 36,
    rows: 20,
    time: 1.0,
    energy: 0.85,
    isSpeaking: true,
    isThinking: true,
  });
  assert.notEqual(
    speaking.lines.join('\n'),
    idle.lines.join('\n'),
    'Expected speaking/thinking auras to visibly modulate rendered Braille frame'
  );
});

test('renderCompanionPortraitFrame renders cleanly with live createBrailleShapeAnimator motion state', () => {
  const w = 64;
  const h = 64;
  const rgba = makeSyntheticMangaPortrait(w, h, false);
  const sourceField = buildCompanionPortraitSourceFromRgba(rgba, w, h, 'ghost');
  const animator = createBrailleShapeAnimator();
  const motion = animator.step(
    {
      time: 1.4,
      shape: 'ghost',
      mood: 'happy',
      eyeStyle: 'open',
      viseme: 'closed',
      energy: 0.12,
      isSpeaking: false,
      isThinking: false,
      isDragging: false,
      isHovered: true,
    },
    0.032,
    false
  );

  const frame = renderCompanionPortraitFrame({
    sourceField,
    cols: 66,
    rows: 36,
    time: 1.4,
    energy: 0.12,
    isSpeaking: false,
    isThinking: false,
    motion,
  });

  const nonEmptyChars = frame.lines.join('').replace(/ /g, '').length;
  assert.ok(
    nonEmptyChars > 180,
    `Expected rich non-empty creature frame with live motion state, got ${nonEmptyChars} chars`
  );
});
