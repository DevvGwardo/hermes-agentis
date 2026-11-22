import { useEffect, useRef, useState, type RefObject } from 'react';
import type { ExpressionMood, MouthViseme } from '../types/hermes';
import {
  evaluateNaturalBlinkCurve,
  type PortraitSourceField,
} from '../utils/asciiPortraitEngine';
import {
  resolveIdleSetForShape,
  type IdleAnimationSetId,
} from '../utils/brailleIdleLoops';
import {
  BRAILLE_TINT_PALETTE,
  createBrailleShapeAnimator,
  renderBrailleShape,
  type BrailleShapeKind,
  type BrailleShapeTint,
} from '../utils/brailleShapesEngine';
import {
  createRasterSurface,
  packSubpixelDotsToTonalBuckets,
  rasterizeTonalDotBucketsToSurface,
  resolveShapeTonePalette,
  type BrailleDotProfile,
  type TonalDotBucketSpec,
} from '../utils/brailleCanvasRaster';
import { renderCompanionPortraitFrame } from '../utils/companionPortraitProcessor';
import {
  getCachedCompanionPortrait,
  loadCompanionPortrait,
} from './companionPortraitAssets';

interface BrailleShapeCanvasProps {
  shape: BrailleShapeKind;
  tint: BrailleShapeTint;
  mood: ExpressionMood;
  eyeStyle?: 'open' | 'sleepy';
  idleSet?: IdleAnimationSetId;
  viseme?: MouthViseme;
  energy?: number;
  isSpeaking?: boolean;
  isThinking?: boolean;
  isDragging?: boolean;
  isHovered?: boolean;
  speedScale?: number;
  reducedMotionOverride?: boolean;
  dotProfile?: BrailleDotProfile;
  bloom?: number;
  variant?: 'creature' | 'preview';
  blinkPhaseOffset?: number;
  gazeRef?: RefObject<{ clientX: number; clientY: number } | null>;
  className?: string;
}

export function BrailleShapeCanvas({
  shape,
  tint,
  mood,
  eyeStyle = 'open',
  idleSet,
  viseme = 'closed',
  energy = 0.08,
  isSpeaking = false,
  isThinking = false,
  isDragging = false,
  isHovered = false,
  speedScale = 1,
  reducedMotionOverride = false,
  dotProfile = 'balanced',
  bloom = 0.75,
  variant = 'creature',
  blinkPhaseOffset = 0,
  gazeRef,
  className = '',
}: BrailleShapeCanvasProps) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const resolvedSet = idleSet ?? resolveIdleSetForShape(shape);
  const [portraitField, setPortraitField] = useState<PortraitSourceField | null>(() =>
    getCachedCompanionPortrait(shape)
  );

  useEffect(() => {
    let cancelled = false;
    const cached = getCachedCompanionPortrait(shape);
    setPortraitField(cached);
    if (!cached) {
      void loadCompanionPortrait(shape).then((loaded) => {
        if (!cancelled && loaded) {
          setPortraitField(loaded);
        }
      });
    }
    return () => {
      cancelled = true;
    };
  }, [shape]);

  const stateRef = useRef({
    shape,
    tint,
    mood,
    eyeStyle,
    idleSet: resolvedSet,
    viseme,
    energy,
    isSpeaking,
    isThinking,
    isDragging,
    isHovered,
    speedScale,
    reducedMotionOverride,
    dotProfile,
    bloom,
    blinkPhaseOffset,
    gazeRef,
    portraitField,
  });
  stateRef.current = {
    shape,
    tint,
    mood,
    eyeStyle,
    idleSet: resolvedSet,
    viseme,
    energy,
    isSpeaking,
    isThinking,
    isDragging,
    isHovered,
    speedScale,
    reducedMotionOverride,
    dotProfile,
    bloom,
    blinkPhaseOffset,
    gazeRef,
    portraitField,
  };

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    const isPreview = variant === 'preview';
    const cols = isPreview ? 42 : 66;
    const rows = isPreview ? 23 : 36;
    const width = isPreview ? 82 : 146;
    const height = isPreview ? 84 : 154;

    const dpr = Math.min(2, window.devicePixelRatio || 1);
    canvas.width = width * dpr;
    canvas.height = height * dpr;
    const surface = createRasterSurface(ctx, canvas.width, canvas.height);

    const cellW = width / cols;
    const cellH = height / rows;
    const dotW = cellW / 2;
    const dotH = cellH / 4;
    const dotRadius = Math.min(dotW, dotH) * (isPreview ? 0.44 : 0.42);
    const dotDiameter = Math.max(0.92, dotRadius * 1.92);

    const systemReduced =
      typeof window !== 'undefined' &&
      window.matchMedia?.('(prefers-reduced-motion: reduce)').matches;

    const startTime = performance.now();
    const animator = createBrailleShapeAnimator();
    let lastDraw = startTime;
    let elapsedPhase = 0;
    let prevRectX: number | null = null;
    let prevRectY: number | null = null;
    let prevDragActive = false;
    let settleUntilMs = 0;
    let smoothEnergy = stateRef.current.energy;
    let activeIdleBusy = false;
    let activeBlinkBusy = false;
    let rafId = 0;
    let hiddenTimerId = 0;
    let lastReducedKey = '';

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
      { xy: b0XY, len: 0, hex: '#4D7C0F', alpha: 0.74, diameterScale: 0.72 },
      { xy: b1XY, len: 0, hex: '#84CC16', alpha: 0.86, diameterScale: 0.84 },
      { xy: b2XY, len: 0, hex: '#B9EA74', alpha: 0.93, diameterScale: 0.95 },
      { xy: b3XY, len: 0, hex: '#DCF7B4', alpha: 0.96, diameterScale: 1.04 },
      { xy: b4XY, len: 0, hex: '#ECFCCB', alpha: 0.99, diameterScale: 1.12 },
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

      const elapsed = isReduced ? 0.5 : elapsedPhase;
      smoothEnergy += (s.energy - smoothEnergy) * (1 - Math.exp(-16 * dt));

      let targetGazeX: number | undefined;
      let targetGazeY: number | undefined;
      let dragVelX = 0;
      let dragVelY = 0;

      const ptr = s.gazeRef?.current;
      if ((ptr || s.isDragging) && !isPreview) {
        const rect = canvas.getBoundingClientRect();
        if (rect.width > 0 && rect.height > 0) {
          if (ptr) {
            const cx = rect.left + rect.width * 0.5;
            const cy = rect.top + rect.height * 0.5;
            targetGazeX = Math.max(-1, Math.min(1, (ptr.clientX - cx) / 220));
            targetGazeY = Math.max(-1, Math.min(1, (ptr.clientY - cy) / 220));
          }
          if (s.isDragging && prevRectX !== null && prevRectY !== null) {
            dragVelX = (rect.left - prevRectX) / Math.max(0.016, dt) / 360;
            dragVelY = (rect.top - prevRectY) / Math.max(0.016, dt) / 360;
          }
          prevRectX = rect.left;
          prevRectY = rect.top;
        }
      } else {
        prevRectX = null;
        prevRectY = null;
      }

      const motion = isPreview
        ? null
        : animator.step(
            {
              time: elapsed,
              shape: s.shape,
              idleSetOverride: s.idleSet,
              mood: s.mood,
              eyeStyle: s.eyeStyle,
              viseme: s.viseme,
              energy: smoothEnergy,
              isSpeaking: s.isSpeaking,
              isThinking: s.isThinking,
              isDragging: s.isDragging,
              isHovered: s.isHovered,
              dragVelX,
              dragVelY,
              gazeX: targetGazeX,
              gazeY: targetGazeY,
              blinkPhaseOffset: s.blinkPhaseOffset,
            },
            dt,
            isReduced
          );

      if (motion) {
        activeIdleBusy = !motion.idle.isResting;
        activeBlinkBusy =
          motion.blink > 0.015 ||
          motion.idle.pose.blinkLeft > 0.05 ||
          motion.idle.pose.blinkRight > 0.05;
        canvas.dataset.blink = motion.blink.toFixed(2);
        if (canvas.dataset.animSet !== motion.idle.setId) {
          canvas.dataset.animSet = motion.idle.setId;
        }
        if (canvas.dataset.animAction !== motion.idle.actionId) {
          canvas.dataset.animAction = motion.idle.actionId;
          canvas.dataset.animIndex = String(motion.idle.actionIndex);
        }
      } else {
        activeIdleBusy = false;
        activeBlinkBusy = false;
        canvas.dataset.blink = '0.00';
      }

      const activeSource = s.portraitField ?? getCachedCompanionPortrait(s.shape);
      const frame = activeSource
        ? renderCompanionPortraitFrame({
            sourceField: activeSource,
            cols,
            rows,
            time: elapsed + s.blinkPhaseOffset * 0.7,
            energy: smoothEnergy,
            isSpeaking: s.isSpeaking,
            isThinking: s.isThinking,
            gazeX: motion ? motion.gazeX : 0,
            gazeY: motion ? motion.gazeY : 0,
            motion,
          })
        : renderBrailleShape({
            shape: s.shape,
            cols,
            rows,
            time: elapsed + s.blinkPhaseOffset * 0.7,
            mood: s.mood,
            eyeState: s.eyeStyle,
            viseme: s.viseme,
            energy: smoothEnergy,
            isSpeaking: s.isSpeaking,
            isThinking: s.isThinking,
            isDragging: s.isDragging,
            gazeX: motion ? motion.gazeX : 0,
            gazeY: motion ? motion.gazeY : 0,
            motion,
          });

      const palette = BRAILLE_TINT_PALETTE[s.tint];
      packSubpixelDotsToTonalBuckets(
        frame,
        buckets,
        activeSource ? [58, 108, 158, 204] : [64, 112, 160, 206],
        cellW,
        cellH,
        dotRadius
      );

      const tone = resolveShapeTonePalette({
        low: palette.low,
        mid: palette.mid,
        high: palette.high,
        isThinking: s.isThinking,
        isSpeaking: s.isSpeaking,
        energy: smoothEnergy,
      });

      for (let i = 0; i < 5; i++) {
        buckets[i].hex = tone.hexes[i];
        buckets[i].alpha = tone.alphas[i];
      }

      rasterizeTonalDotBucketsToSurface(surface, buckets, dpr, dotDiameter, {
        dotProfile: s.dotProfile,
        bloom: isPreview ? Math.min(0.4, s.bloom * 0.45) : s.bloom,
      });
      ctx.putImageData(surface.imageData, 0, 0);
    };

    drawFrame(startTime);

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
      if (isReduced) {
        const hasPortrait = Boolean(s.portraitField ?? getCachedCompanionPortrait(s.shape));
        const reducedKey = `${s.shape}|${hasPortrait}|${s.tint}|${s.mood}|${s.eyeStyle}|${s.viseme}|${Math.round(
          s.energy * 100
        )}|${s.isSpeaking}|${s.isThinking}|${s.isDragging}|${s.isHovered}|${s.dotProfile}|${s.bloom.toFixed(2)}`;
        if (reducedKey !== lastReducedKey) {
          lastReducedKey = reducedKey;
          drawFrame(now);
        }
        hiddenTimerId = window.setTimeout(scheduleNext, 200);
        return;
      }
      lastReducedKey = '';

      if (prevDragActive && !s.isDragging) {
        settleUntilMs = now + 900;
      }
      prevDragActive = s.isDragging;

      const blinkLookahead =
        evaluateNaturalBlinkCurve(elapsedPhase + 0.08, s.blinkPhaseOffset, 3) > 0.01;
      const fastMotion =
        s.isSpeaking ||
        s.isDragging ||
        now < settleUntilMs ||
        activeBlinkBusy ||
        blinkLookahead;
      const activeMotion =
        fastMotion ||
        s.isThinking ||
        s.isHovered ||
        activeIdleBusy;
      const minInterval = isPreview
        ? 64
        : fastMotion
        ? 16
        : activeMotion
        ? 22
        : 32;
      if (now - lastDraw >= minInterval) {
        drawFrame(now);
      }
      rafId = window.requestAnimationFrame(loop);
    };

    rafId = window.requestAnimationFrame(loop);
    return () => {
      window.cancelAnimationFrame(rafId);
      if (hiddenTimerId) window.clearTimeout(hiddenTimerId);
    };
  }, [variant, portraitField]);

  return (
    <canvas
      ref={canvasRef}
      aria-hidden="true"
      data-anim-set={resolvedSet}
      data-portrait-loaded={portraitField ? 'true' : 'false'}
      className={
        variant === 'preview'
          ? `block w-[82px] h-[84px] pointer-events-none ${className}`
          : `block w-[146px] h-[154px] pointer-events-none ${className}`
      }
    />
  );
}
