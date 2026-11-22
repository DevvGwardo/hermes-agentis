import { useEffect, useRef, useState, type RefObject } from 'react';
import type { ExpressionMood, EyeState, MouthViseme } from '../types/hermes';
import {
  buildPortraitBankFromSpriteSheet,
  buildPortraitSourceFromRgba,
  createPortraitAnimator,
  evaluateNaturalBlinkCurve,
  renderHalftonePortrait,
  type LookTarget,
  type PortraitSourceField,
} from '../utils/asciiPortraitEngine';
import {
  blendLoadingToPortraitFrame,
  computeMaterializeProgress,
  renderPortraitLoadingFrame,
  type PortraitAssetStatus,
} from '../utils/portraitLoadingEngine';
import {
  createRasterSurface,
  packSubpixelDotsToTonalBuckets,
  rasterizeTonalDotBucketsToSurface,
  resolvePortraitTonePalette,
  type BrailleDotProfile,
  type TonalDotBucketSpec,
} from '../utils/brailleCanvasRaster';

const REFERENCE_PORTRAIT_URL = '/assets/aistudio/image_9fdb5ec4.png';
const REFERENCE_SPRITE_SHEET_URL = '/assets/aistudio/image_f0b5dc2e.png';

let cachedPortraitSource: PortraitSourceField | null = null;
let portraitLoadPromise: Promise<PortraitSourceField | null> | null = null;
let cachedSpriteBank: PortraitSourceField[] | null = null;
let spriteBankLoadPromise: Promise<PortraitSourceField[] | null> | null = null;
let portraitAssetStatus: PortraitAssetStatus = 'loading';
let portraitLoadGeneration = 0;

const assetListeners = new Set<() => void>();

function notifyAssetListeners() {
  for (const listener of assetListeners) {
    listener();
  }
}

function loadReferencePortraitField(): Promise<PortraitSourceField | null> {
  if (cachedPortraitSource) return Promise.resolve(cachedPortraitSource);
  if (portraitLoadPromise) return portraitLoadPromise;

  const gen = portraitLoadGeneration;
  portraitLoadPromise = new Promise((resolve) => {
    const img = new Image();
    img.onload = () => {
      if (gen !== portraitLoadGeneration) {
        resolve(null);
        return;
      }
      try {
        const size = 224;
        const offscreen = document.createElement('canvas');
        offscreen.width = size;
        offscreen.height = size;
        const octx = offscreen.getContext('2d', { willReadFrequently: true }) || offscreen.getContext('2d');
        if (!octx) {
          resolve(null);
          return;
        }
        octx.drawImage(img, 0, 0, size, size);
        const rgba = octx.getImageData(0, 0, size, size).data;
        cachedPortraitSource = buildPortraitSourceFromRgba(rgba, size, size);
        portraitAssetStatus = 'ready';
        notifyAssetListeners();
        resolve(cachedPortraitSource);
      } catch {
        resolve(null);
      }
    };
    img.onerror = () => {
      if (gen === portraitLoadGeneration) {
        portraitLoadPromise = null;
      }
      resolve(null);
    };
    img.src = REFERENCE_PORTRAIT_URL;
  });

  return portraitLoadPromise;
}

function loadReferenceSpriteBank(): Promise<PortraitSourceField[] | null> {
  if (cachedSpriteBank) return Promise.resolve(cachedSpriteBank);
  if (spriteBankLoadPromise) return spriteBankLoadPromise;

  const gen = portraitLoadGeneration;
  spriteBankLoadPromise = new Promise((resolve) => {
    const img = new Image();
    img.onload = () => {
      if (gen !== portraitLoadGeneration) {
        resolve(null);
        return;
      }
      try {
        const w = img.naturalWidth || img.width;
        const h = img.naturalHeight || img.height;
        if (!w || !h) {
          resolve(null);
          return;
        }
        const offscreen = document.createElement('canvas');
        offscreen.width = w;
        offscreen.height = h;
        const octx = offscreen.getContext('2d', { willReadFrequently: true }) || offscreen.getContext('2d');
        if (!octx) {
          resolve(null);
          return;
        }
        octx.drawImage(img, 0, 0, w, h);
        const rgba = octx.getImageData(0, 0, w, h).data;
        const bank = buildPortraitBankFromSpriteSheet(rgba, w, h, 224);
        if (bank.length >= 12) {
          cachedSpriteBank = bank;
          portraitAssetStatus = 'ready';
          notifyAssetListeners();
          resolve(bank);
        } else {
          resolve(null);
        }
      } catch {
        resolve(null);
      }
    };
    img.onerror = () => {
      if (gen === portraitLoadGeneration) {
        spriteBankLoadPromise = null;
      }
      resolve(null);
    };
    img.src = REFERENCE_SPRITE_SHEET_URL;
  });

  return spriteBankLoadPromise;
}

function ensurePortraitAssetsLoading(): void {
  if (cachedSpriteBank || cachedPortraitSource) {
    portraitAssetStatus = 'ready';
    return;
  }
  portraitAssetStatus = 'loading';
  const spritePromise = loadReferenceSpriteBank();
  const basePromise = loadReferencePortraitField();
  void Promise.all([spritePromise, basePromise]).then(([bank, base]) => {
    if (!bank && !base && !cachedSpriteBank && !cachedPortraitSource) {
      portraitAssetStatus = 'error';
      notifyAssetListeners();
    } else if (bank || base) {
      portraitAssetStatus = 'ready';
      notifyAssetListeners();
    }
  });
}

function retryPortraitAssetsLoading(): void {
  portraitLoadGeneration += 1;
  portraitLoadPromise = null;
  spriteBankLoadPromise = null;
  portraitAssetStatus = 'loading';
  notifyAssetListeners();
  ensurePortraitAssetsLoading();
}

interface AsciiPortraitCanvasProps {
  variant?: 'hero' | 'compact' | 'thumb' | 'sidebar' | 'stage';
  mood: ExpressionMood;
  viseme: MouthViseme;
  eyeState: EyeState;
  energy: number;
  isSpeaking: boolean;
  isListening: boolean;
  lookTarget?: LookTarget | null;
  lookTargetRef?: RefObject<LookTarget | null>;
  tintHex?: string | null;
  dotProfile?: BrailleDotProfile;
  bloom?: number;
  speedScale?: number;
  reducedMotionOverride?: boolean;
  showLoadingLabel?: boolean;
  onStatusChange?: (status: PortraitAssetStatus) => void;
  onClick?: () => void;
}

export function AsciiPortraitCanvas({
  variant = 'hero',
  mood,
  viseme,
  eyeState,
  energy,
  isSpeaking,
  isListening,
  lookTarget,
  lookTargetRef,
  tintHex = null,
  dotProfile = 'balanced',
  bloom = 0.75,
  speedScale = 1,
  reducedMotionOverride = false,
  showLoadingLabel,
  onStatusChange,
  onClick,
}: AsciiPortraitCanvasProps) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const [sourceField, setSourceField] = useState<PortraitSourceField | null>(() => cachedPortraitSource);
  const [spriteBank, setSpriteBank] = useState<PortraitSourceField[] | null>(() => cachedSpriteBank);
  const [status, setStatus] = useState<PortraitAssetStatus>(() =>
    cachedSpriteBank || cachedPortraitSource ? 'ready' : portraitAssetStatus
  );

  // Track whether this component actually witnessed loading before becoming ready,
  // so tab switches when already cached render immediately without re-wiping.
  const startedWhileLoadingRef = useRef<boolean>(!(cachedSpriteBank || cachedPortraitSource));
  const readyAtMsRef = useRef<number | null>(
    cachedSpriteBank || cachedPortraitSource ? 0 : null
  );
  const animatorRef = useRef<ReturnType<typeof createPortraitAnimator> | null>(null);
  const isHoveredRef = useRef(false);
  const isBlinkingRef = useRef(false);

  const stateRef = useRef({
    mood,
    viseme,
    eyeState,
    energy,
    isSpeaking,
    isListening,
    lookTarget,
    lookTargetRef,
    sourceField,
    spriteBank,
    status,
    tintHex,
    dotProfile,
    bloom,
    speedScale,
    reducedMotionOverride,
  });
  stateRef.current = {
    mood,
    viseme,
    eyeState,
    energy,
    isSpeaking,
    isListening,
    lookTarget,
    lookTargetRef,
    sourceField,
    spriteBank,
    status,
    tintHex,
    dotProfile,
    bloom,
    speedScale,
    reducedMotionOverride,
  };

  useEffect(() => {
    const sync = () => {
      setSourceField(cachedPortraitSource);
      setSpriteBank(cachedSpriteBank);
      const nextStatus: PortraitAssetStatus =
        cachedSpriteBank || cachedPortraitSource ? 'ready' : portraitAssetStatus;
      if (nextStatus === 'ready' && readyAtMsRef.current === null) {
        readyAtMsRef.current = performance.now();
      } else if (nextStatus === 'loading') {
        startedWhileLoadingRef.current = true;
        readyAtMsRef.current = null;
      }
      setStatus(nextStatus);
    };

    assetListeners.add(sync);
    ensurePortraitAssetsLoading();
    sync();

    return () => {
      assetListeners.delete(sync);
    };
  }, []);

  useEffect(() => {
    onStatusChange?.(status);
  }, [status, onStatusChange]);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    const isThumb = variant === 'thumb';
    const isSidebar = variant === 'sidebar';
    const isCompact = variant === 'compact';
    const isStage = variant === 'stage';
    const cols = isThumb ? 26 : isSidebar ? 44 : isCompact ? 50 : isStage ? 96 : 70;
    const rows = isThumb ? 13 : isSidebar ? 20 : isCompact ? 23 : isStage ? 44 : 32;
    const defaultW = isThumb ? 42 : isSidebar ? 72 : isCompact ? 116 : isStage ? 360 : 216;
    const defaultH = isThumb ? 42 : isSidebar ? 66 : isCompact ? 106 : isStage ? 330 : 198;

    const dpr = Math.min(2, window.devicePixelRatio || 1);
    let width = defaultW;
    let height = defaultH;
    if (isStage) {
      const rect = canvas.getBoundingClientRect();
      if (rect.width >= 60 && rect.height >= 55) {
        width = Math.round(rect.width);
        height = Math.round(rect.height);
      }
    }
    canvas.width = Math.max(1, Math.round(width * dpr));
    canvas.height = Math.max(1, Math.round(height * dpr));
    let surface = createRasterSurface(ctx, canvas.width, canvas.height);

    let cellW = width / cols;
    let cellH = height / rows;
    let dotW = cellW / 2;
    let dotH = cellH / 4;
    let dotRadius = isThumb
      ? 0.40
      : Math.min(dotW, dotH) * (isStage ? 0.41 : isSidebar || isCompact ? 0.42 : 0.41);

    const syncStageSurface = () => {
      if (!isStage) return;
      const rect = canvas.getBoundingClientRect();
      if (rect.width < 60 || rect.height < 55) return;
      const nextW = Math.round(rect.width);
      const nextH = Math.round(rect.height);
      if (Math.abs(nextW - width) < 2 && Math.abs(nextH - height) < 2) return;
      width = nextW;
      height = nextH;
      canvas.width = Math.max(1, Math.round(width * dpr));
      canvas.height = Math.max(1, Math.round(height * dpr));
      surface = createRasterSurface(ctx, canvas.width, canvas.height);
      cellW = width / cols;
      cellH = height / rows;
      dotW = cellW / 2;
      dotH = cellH / 4;
      dotRadius = Math.min(dotW, dotH) * 0.41;
    };

    const resizeObserver =
      isStage && typeof ResizeObserver !== 'undefined'
        ? new ResizeObserver(() => {
            syncStageSurface();
          })
        : null;
    resizeObserver?.observe(canvas);
    const startTime = performance.now();
    const animator = createPortraitAnimator();
    animatorRef.current = animator;
    const systemReduced =
      typeof window !== 'undefined' &&
      window.matchMedia?.('(prefers-reduced-motion: reduce)').matches;

    let smoothEnergy = stateRef.current.energy;
    let rafId = 0;
    let hiddenTimerId = 0;
    let lastDraw = startTime;
    let elapsedPhase = 0;
    let lastReducedKey = '';
    let lastBlinkAttr = '';
    let lastMoodAttr = '';

    // Reusable XY coordinate buffers across 5 tonal tiers for variable-size halftone dots
    const maxDots = cols * rows * 8;
    const b0XY = new Float32Array(maxDots * 2);
    const b1XY = new Float32Array(maxDots * 2);
    const b2XY = new Float32Array(maxDots * 2);
    const b3XY = new Float32Array(maxDots * 2);
    const b4XY = new Float32Array(maxDots * 2);
    const buckets: [
      TonalDotBucketSpec,
      TonalDotBucketSpec,
      TonalDotBucketSpec,
      TonalDotBucketSpec,
      TonalDotBucketSpec,
    ] = [
      { xy: b0XY, len: 0, hex: '#94A3B8', alpha: 0.48, diameterScale: 0.68 },
      { xy: b1XY, len: 0, hex: '#CBD5E1', alpha: 0.68, diameterScale: 0.80 },
      { xy: b2XY, len: 0, hex: '#E2E8F0', alpha: 0.84, diameterScale: 0.91 },
      { xy: b3XY, len: 0, hex: '#F1F5F9', alpha: 0.94, diameterScale: 1.00 },
      { xy: b4XY, len: 0, hex: '#FFFFFF', alpha: 0.99, diameterScale: 1.07 },
    ];

    const drawFrame = (now: number) => {
      const s = stateRef.current;
      const isReduced = Boolean(systemReduced || s.reducedMotionOverride);
      const speed = Math.max(0.25, Math.min(2.0, s.speedScale || 1));
      const rawDt = Math.min(0.16, Math.max(0.008, (now - lastDraw) / 1000));
      const dt = isReduced ? rawDt : rawDt * speed;
      lastDraw = now;
      if (!isReduced) {
        elapsedPhase += dt;
      }

      smoothEnergy += (s.energy - smoothEnergy) * (1 - Math.exp(-16 * dt));
      const elapsed = isReduced ? 0.6 : elapsedPhase;
      const hasRealCharacter = Boolean(
        (s.spriteBank && s.spriteBank.length >= 12) || s.sourceField
      );

      let frame;
      let isLoadingVisual = false;

      if (!hasRealCharacter) {
        // Never render mathematical mock character data while real character artwork loads or errors
        isLoadingVisual = true;
        frame = renderPortraitLoadingFrame({
          cols,
          rows,
          time: elapsed,
          variant,
          status: s.status === 'error' ? 'error' : 'loading',
          reducedMotion: isReduced,
        });
      } else {
        const portraitElapsed = isThumb ? 0.5 : elapsed;
        const activeLook = s.lookTargetRef?.current ?? s.lookTarget ?? null;
        const motion = isThumb
          ? null
          : animator.step(
              {
                time: portraitElapsed,
                mood: s.mood,
                viseme: s.viseme,
                eyeState: s.eyeState,
                energy: smoothEnergy,
                isSpeaking: s.isSpeaking,
                isListening: s.isListening,
              },
              dt,
              isReduced,
              activeLook
            );

        if (!isThumb) {
          isBlinkingRef.current = Boolean(motion && motion.blink > 0.015);
          const nextBlinkAttr = motion ? motion.blink.toFixed(2) : '0.00';
          if (nextBlinkAttr !== lastBlinkAttr) {
            lastBlinkAttr = nextBlinkAttr;
            canvas.dataset.blink = nextBlinkAttr;
          }
          if (s.mood !== lastMoodAttr) {
            lastMoodAttr = s.mood;
            canvas.dataset.mood = s.mood;
          }
        }

        const portraitFrame = renderHalftonePortrait({
          cols,
          rows,
          time: portraitElapsed,
          mood: s.mood,
          viseme: isThumb ? 'closed' : s.viseme,
          eyeState: isThumb ? 'open' : s.eyeState,
          energy: isThumb ? 0.1 : smoothEnergy,
          isSpeaking: isThumb ? false : s.isSpeaking,
          isListening: isThumb ? false : s.isListening,
          style: 'braille',
          sourceField: s.sourceField,
          spriteBank: s.spriteBank,
          motion,
        });

        const readyElapsed =
          startedWhileLoadingRef.current && readyAtMsRef.current !== null && readyAtMsRef.current > 0
            ? (now - readyAtMsRef.current) / 1000
            : 10;
        const matProgress = computeMaterializeProgress(readyElapsed, isReduced);

        if (matProgress < 0.995) {
          const loadingFrame = renderPortraitLoadingFrame({
            cols,
            rows,
            time: elapsed,
            variant,
            status: 'loading',
            reducedMotion: isReduced,
          });
          frame = blendLoadingToPortraitFrame(loadingFrame, portraitFrame, matProgress);
        } else {
          frame = portraitFrame;
        }
      }

      const dotDiameter = Math.max(1.02, dotRadius * 1.92);
      packSubpixelDotsToTonalBuckets(
        frame,
        buckets,
        [62, 114, 166, 212],
        cellW,
        cellH,
        dotRadius
      );

      const tone = resolvePortraitTonePalette({
        mood: s.mood,
        isSpeaking: s.isSpeaking,
        isListening: s.isListening,
        isLoading: isLoadingVisual,
        isThumb,
        energy: smoothEnergy,
        tintHex: s.tintHex,
      });

      for (let i = 0; i < 5; i++) {
        buckets[i].hex = tone.hexes[i];
        buckets[i].alpha = tone.alphas[i];
      }

      rasterizeTonalDotBucketsToSurface(surface, buckets, dpr, dotDiameter, {
        dotProfile: s.dotProfile,
        bloom: isThumb ? 0 : isSidebar ? Math.min(0.85, s.bloom * 0.72) : s.bloom,
      });
      ctx.putImageData(surface.imageData, 0, 0);
    };

    drawFrame(startTime);

    // Thumb stays static once real character finishes materializing,
    // but animates smoothly while loading!
    const thumbMaterialized =
      isThumb &&
      status === 'ready' &&
      (!startedWhileLoadingRef.current || readyAtMsRef.current === 0);
    if (thumbMaterialized) return;

    const scheduleNext = () => {
      rafId = window.requestAnimationFrame(loop);
    };

    const loop = (now: number) => {
      if (
        (typeof document !== 'undefined' && document.hidden) ||
        (canvas.offsetWidth === 0 && canvas.offsetHeight === 0)
      ) {
        lastDraw = now;
        hiddenTimerId = window.setTimeout(scheduleNext, 200);
        return;
      }
      const s = stateRef.current;
      const isReduced = Boolean(systemReduced || s.reducedMotionOverride);
      const activeLook = s.lookTargetRef?.current ?? s.lookTarget ?? null;
      const hasRealCharacter = Boolean(
        (s.spriteBank && s.spriteBank.length >= 12) || s.sourceField
      );
      const readyElapsed =
        startedWhileLoadingRef.current && readyAtMsRef.current !== null && readyAtMsRef.current > 0
          ? (now - readyAtMsRef.current) / 1000
          : 10;
      const isMaterializing = readyElapsed < 1.25;

      if (isReduced && hasRealCharacter && !isMaterializing) {
        const reducedKey = `${s.mood}|${s.viseme}|${s.eyeState}|${s.isSpeaking}|${s.isListening}|${Math.round(
          s.energy * 100
        )}|${s.tintHex ?? ''}|${s.status}|${s.dotProfile}|${s.bloom.toFixed(2)}|${
          activeLook ? `${activeLook.x.toFixed(2)},${activeLook.y.toFixed(2)}` : ''
        }`;
        if (reducedKey !== lastReducedKey) {
          lastReducedKey = reducedKey;
          drawFrame(now);
        }
        hiddenTimerId = window.setTimeout(scheduleNext, 180);
        return;
      }
      lastReducedKey = '';

      const blinkLookahead = evaluateNaturalBlinkCurve(elapsedPhase + 0.08, 0, 4) > 0.01;
      const activeMotion =
        s.status !== 'ready' ||
        isMaterializing ||
        s.isSpeaking ||
        s.isListening ||
        s.mood === 'thinking' ||
        s.mood === 'alert' ||
        isHoveredRef.current ||
        activeLook !== null ||
        isBlinkingRef.current ||
        blinkLookahead;

      const minIntervalMs = isReduced
        ? 220
        : isMaterializing
        ? 18
        : activeMotion
        ? 16
        : isSidebar
        ? 42
        : 28;
      if (now - lastDraw >= minIntervalMs) {
        drawFrame(now);
      }
      if (isThumb && s.status === 'ready') {
        const doneElapsed =
          readyAtMsRef.current && readyAtMsRef.current > 0
            ? (now - readyAtMsRef.current) / 1000
            : 10;
        if (doneElapsed > 0.5) {
          animatorRef.current = null;
          return;
        }
      }
      rafId = window.requestAnimationFrame(loop);
    };

    rafId = window.requestAnimationFrame(loop);
    return () => {
      window.cancelAnimationFrame(rafId);
      if (hiddenTimerId) window.clearTimeout(hiddenTimerId);
      resizeObserver?.disconnect();
      animatorRef.current = null;
    };
  }, [variant, sourceField, spriteBank, status]);

  if (variant === 'thumb') {
    return (
      <canvas
        ref={canvasRef}
        data-portrait-status={status}
        aria-hidden="true"
        className="w-[42px] h-[42px] rounded-md bg-black/35 shrink-0"
      />
    );
  }

  const isSidebar = variant === 'sidebar';
  const isCompact = variant === 'compact';
  const isStage = variant === 'stage';
  const showBadge = showLoadingLabel ?? isStage;
  const isError = status === 'error';
  const isLoading = status === 'loading';

  return (
    <div className="relative flex flex-col items-center justify-center shrink-0">
      <button
        type="button"
        onMouseEnter={() => {
          isHoveredRef.current = true;
        }}
        onMouseLeave={() => {
          isHoveredRef.current = false;
        }}
        onClick={() => {
          if (isError) {
            retryPortraitAssetsLoading();
            return;
          }
          animatorRef.current?.triggerImpulse('tap');
          onClick?.();
        }}
        aria-busy={isLoading}
        aria-label={
          isLoading
            ? 'Loading Hermes character sprite rig'
            : isError
            ? 'Character signal failed to load. Click to retry.'
            : `Animated monochrome braille halftone portrait of Hermes, expression ${mood}, mouth ${viseme}. Click to cycle mood.`
        }
        className={
          isSidebar
            ? 'ui-tactile group relative flex items-center justify-center rounded-lg overflow-hidden bg-transparent focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--ui-accent)] cursor-pointer'
            : 'group relative flex items-center justify-center rounded-xl focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-[var(--ui-accent)] transition-transform duration-200 ease-out active:scale-[0.97] hover:scale-[1.015] cursor-pointer'
        }
      >
        <canvas
          ref={canvasRef}
          data-testid={isSidebar ? 'sidebar-portrait-canvas' : 'ascii-portrait-canvas'}
          data-portrait-status={status}
          className={
            isSidebar
              ? 'w-[72px] h-[66px] block'
              : isCompact
              ? 'w-[116px] h-[106px] block'
              : isStage
              ? 'block w-[clamp(164px,min(44vw,29dvh),480px)] sm:w-[clamp(204px,min(38vw,33dvh),500px)] aspect-[12/11]'
              : 'w-[216px] h-[198px] block'
          }
        />
      </button>

      {isLoading && showBadge && (
        <div
          data-testid="portrait-loading-indicator"
          aria-live="polite"
          className="pointer-events-none -mt-2 mb-1 inline-flex items-center gap-2 rounded-lg border border-white/12 bg-[#0D1016]/92 px-3 py-1 font-mono text-[10.5px] text-zinc-200 shadow-[0_6px_20px_rgba(0,0,0,0.45)]"
        >
          <span
            aria-hidden="true"
            className="inline-block w-1.5 h-1.5 rounded-full bg-slate-300 animate-pulse motion-safe-only"
          />
          <span>Calibrating halftone rig…</span>
        </div>
      )}

      {isError && showBadge && (
        <button
          type="button"
          onClick={retryPortraitAssetsLoading}
          data-testid="portrait-retry-btn"
          className="-mt-1 mb-1 inline-flex items-center gap-1.5 rounded-full border border-rose-400/40 bg-rose-500/12 hover:bg-rose-500/20 px-3 py-1 font-mono text-[10.5px] text-rose-200 transition-colors cursor-pointer"
        >
          <span>Signal unavailable · Retry</span>
        </button>
      )}
    </div>
  );
}
