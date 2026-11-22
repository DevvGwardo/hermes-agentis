import test from 'node:test';
import assert from 'node:assert/strict';
import {
  BLUE_NOISE_8X8_RANKS,
  mixHexColors,
  packSubpixelDotsToTonalBuckets,
  parseHexRgb,
  rasterizeDotBucketsToSurface,
  rasterizeTonalDotBucketsToSurface,
  resolvePortraitTonePalette,
  resolveShapeTonePalette,
  type BrailleRasterSurface,
  type TonalDotBucketSpec,
} from './brailleCanvasRaster';

function makeFakeSurface(width: number, height: number): BrailleRasterSurface {
  const bytes = new Uint8ClampedArray(width * height * 4);
  return {
    imageData: { data: bytes, width, height, colorSpace: 'srgb' } as ImageData,
    u32: new Uint32Array(bytes.buffer),
    bytes,
    width,
    height,
  };
}

test('parseHexRgb and mixHexColors decode and blend 6-digit hex colors cleanly', () => {
  assert.deepEqual(parseHexRgb('#34D399'), [52, 211, 153]);
  assert.deepEqual(parseHexRgb('#FFF'), [255, 255, 255]);
  assert.deepEqual(parseHexRgb('invalid'), [230, 240, 236]);
  assert.equal(mixHexColors('#000000', '#FFFFFF', 0), '#000000');
  assert.equal(mixHexColors('#000000', '#FFFFFF', 1), '#ffffff');
  assert.equal(mixHexColors('#000000', '#FFFFFF', 0.5), '#808080');
});

test('rasterizeDotBucketsToSurface clears prior pixels and stamps analytical subpixel coverage', () => {
  const surface = makeFakeSurface(10, 10);
  surface.u32.fill(0xffffffff);

  const xy = new Float32Array([2.0, 3.0]);
  rasterizeDotBucketsToSurface(
    surface,
    [{ xy, len: 2, hex: '#34D399', alpha: 1 }],
    1,
    1.5
  );

  // Pixel (0,0) must be cleared
  assert.equal(surface.bytes[3], 0);

  // Pixel (2,3) has full 1x1 coverage -> alpha 255, RGB (52, 211, 153)
  const p23 = (3 * 10 + 2) * 4;
  assert.equal(surface.bytes[p23], 52);
  assert.equal(surface.bytes[p23 + 1], 211);
  assert.equal(surface.bytes[p23 + 2], 153);
  assert.equal(surface.bytes[p23 + 3], 255);

  // Pixel (3,3) has 0.5 horizontal x 1.0 vertical coverage -> alpha ~128
  const p33 = (3 * 10 + 3) * 4;
  assert.ok(surface.bytes[p33 + 3] >= 126 && surface.bytes[p33 + 3] <= 129);
});

test('rasterizeTonalDotBucketsToSurface modulates dot coverage area by diameterScale with circular AA', () => {
  const surfaceSmall = makeFakeSurface(16, 16);
  const surfaceLarge = makeFakeSurface(16, 16);

  const countAlphaSum = (bytes: Uint8ClampedArray) => {
    let sum = 0;
    for (let i = 3; i < bytes.length; i += 4) sum += bytes[i];
    return sum;
  };

  rasterizeTonalDotBucketsToSurface(
    surfaceSmall,
    [{ xy: new Float32Array([6, 6]), len: 2, hex: '#8FA69E', alpha: 0.9, diameterScale: 0.65 }],
    1,
    3.0
  );
  rasterizeTonalDotBucketsToSurface(
    surfaceLarge,
    [{ xy: new Float32Array([6, 6]), len: 2, hex: '#FFFFFF', alpha: 0.9, diameterScale: 1.3 }],
    1,
    3.0
  );

  const smallCoverage = countAlphaSum(surfaceSmall.bytes);
  const largeCoverage = countAlphaSum(surfaceLarge.bytes);
  assert.ok(smallCoverage > 0 && largeCoverage > smallCoverage * 1.8,
    `Expected larger diameterScale to cover noticeably more alpha mass (${smallCoverage} vs ${largeCoverage})`
  );

  // Center of large dot should be brighter than outer corner pixel due to circular AA kernel
  const centerA = surfaceLarge.bytes[(7 * 16 + 7) * 4 + 3];
  const outerA = surfaceLarge.bytes[(5 * 16 + 5) * 4 + 3];
  assert.ok(centerA > outerA, `Expected circular falloff from center (${centerA}) to edge (${outerA})`);
});

test('resolvePortraitTonePalette and resolveShapeTonePalette shift tones dynamically across states', () => {
  const idle = resolvePortraitTonePalette({
    mood: 'happy',
    isSpeaking: false,
    isListening: false,
    isLoading: false,
    energy: 0.08,
  });
  const thinking = resolvePortraitTonePalette({
    mood: 'thinking',
    isSpeaking: false,
    isListening: false,
    isLoading: false,
    energy: 0.12,
  });
  const speaking = resolvePortraitTonePalette({
    mood: 'happy',
    isSpeaking: true,
    isListening: false,
    isLoading: false,
    energy: 0.85,
  });

  assert.notEqual(idle.hexes[0], thinking.hexes[0]);
  assert.equal(thinking.hexes[0], '#B48448');
  assert.equal(speaking.hexes[0], '#BAE6FD');
  assert.ok(speaking.alphas[0] > idle.alphas[0]);

  const shapeIdle = resolveShapeTonePalette({
    low: '#4D7C0F',
    mid: '#84CC16',
    high: '#ECFCCB',
    isThinking: false,
    isSpeaking: false,
    energy: 0.08,
  });
  const shapeThink = resolveShapeTonePalette({
    low: '#4D7C0F',
    mid: '#84CC16',
    high: '#ECFCCB',
    isThinking: true,
    isSpeaking: false,
    energy: 0.25,
  });
  assert.notEqual(shapeIdle.hexes[2], shapeThink.hexes[2]);
});

test('rasterizeTonalDotBucketsToSurface uses source-over compositing so bright foreground dots stay luminous over dim halos', () => {
  const surface = makeFakeSurface(12, 12);

  rasterizeTonalDotBucketsToSurface(
    surface,
    [
      // Dim dark-slate halo underneath covering center
      { xy: new Float32Array([4.0, 4.0]), len: 2, hex: '#101418', alpha: 0.35, diameterScale: 1.5 },
      // Bright highlight dot core overlapping on top
      { xy: new Float32Array([4.2, 4.2]), len: 2, hex: '#FFFFFF', alpha: 0.95, diameterScale: 0.95 },
    ],
    1,
    3.0
  );

  // Sample center pixel (5, 5)
  const idx = (5 * 12 + 5) * 4;
  const r = surface.bytes[idx];
  const g = surface.bytes[idx + 1];
  const b = surface.bytes[idx + 2];
  const a = surface.bytes[idx + 3];

  assert.ok(a > 230, `Expected high combined alpha at overlapped dot core, got ${a}`);
  assert.ok(r > 235 && g > 235 && b > 235, `Expected bright white core to dominate over dim halo, got rgb(${r},${g},${b})`);
});

test('BLUE_NOISE_8X8_RANKS is a complete 64-element permutation of 0..63', () => {
  assert.equal(BLUE_NOISE_8X8_RANKS.length, 64);
  const sorted = [...BLUE_NOISE_8X8_RANKS].sort((a, b) => a - b);
  for (let i = 0; i < 64; i++) {
    assert.equal(sorted[i], i, `Missing or duplicate rank at index ${i}`);
  }
});

test('packSubpixelDotsToTonalBuckets separates per-dot luminance tiers within a single Braille cell', () => {
  const makeBuckets = (): [
    TonalDotBucketSpec,
    TonalDotBucketSpec,
    TonalDotBucketSpec,
    TonalDotBucketSpec,
    TonalDotBucketSpec,
  ] => [
    { xy: new Float32Array(32), len: 0, hex: '#444444', alpha: 0.5 },
    { xy: new Float32Array(32), len: 0, hex: '#777777', alpha: 0.7 },
    { xy: new Float32Array(32), len: 0, hex: '#aaaaaa', alpha: 0.85 },
    { xy: new Float32Array(32), len: 0, hex: '#dddddd', alpha: 0.95 },
    { xy: new Float32Array(32), len: 0, hex: '#ffffff', alpha: 1.0 },
  ];

  // Single Braille cell with top-left dot (0x01) + top-right dot (0x08) active -> U+2809
  const char = String.fromCharCode(0x2800 + 0x09);
  const subLum = new Uint8Array(2 * 4);
  subLum[0] = 30;  // top-left dot -> tier 0 (shadow)
  subLum[1] = 240; // top-right dot -> tier 4 (specular highlight)

  const buckets = makeBuckets();
  packSubpixelDotsToTonalBuckets(
    {
      cols: 1,
      rows: 1,
      lines: [char],
      cells: new Uint8Array([120]),
      subLum,
    },
    buckets,
    [60, 110, 160, 210],
    8,
    16,
    1
  );

  assert.equal(buckets[0].len, 2, 'Shadow dot should land in tier 0');
  assert.equal(buckets[4].len, 2, 'Highlight dot should land in tier 4');
  assert.equal(buckets[2].len, 0, 'Coarse cell average (120) must not override per-dot subLum');

  // Without subLum, falls back cleanly to coarse cell average (120 -> tier 2)
  const fallbackBuckets = makeBuckets();
  packSubpixelDotsToTonalBuckets(
    {
      cols: 1,
      rows: 1,
      lines: [char],
      cells: new Uint8Array([120]),
    },
    fallbackBuckets,
    [60, 110, 160, 210],
    8,
    16,
    1
  );
  assert.equal(fallbackBuckets[2].len, 4, 'Both dots should fall back to cell tier 2');
});

