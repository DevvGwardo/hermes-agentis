import type { BackgroundPresetKind } from '../backgrounds/types';
import type { ExpressionMood } from '../types/hermes';
import type { BrailleDotProfile } from './brailleCanvasRaster';
import type { BrailleHorizonStyle } from './brailleFieldEngine';

export type UiAccentPreset =
  | 'slate'
  | 'emerald'
  | 'cyan'
  | 'violet'
  | 'amber'
  | 'rose'
  | 'crimson'
  | 'mono'
  | 'custom';

export type UiCornerRadius = 'sharp' | 'rounded' | 'pill';
export type UiFontPreset = 'jakarta' | 'mono' | 'display' | 'system';
export type UiSidebarPosition = 'left' | 'right';
export type UiComposerStyle = 'docked' | 'floating' | 'minimal';
export type UiDensity = 'compact' | 'comfortable' | 'airy';
export type UiPanelView = 'home' | 'canvas' | 'voice' | 'queue' | 'cli';

export interface UiCustomizationConfig {
  // Palette & surface styling
  accentPreset: UiAccentPreset;
  accentHex: string;
  accentSoftHex: string;
  accentBrightHex: string;
  surfaceHex: string;
  textHex: string;
  panelAlpha: number;
  borderOpacity: number;
  glowIntensity: number;

  // Geometry & typography
  cornerRadius: UiCornerRadius;
  density: UiDensity;
  fontPreset: UiFontPreset;
  fontScale: number;

  // Layout & navigation chrome
  sidebarVisible: boolean;
  sidebarPosition: UiSidebarPosition;
  sidebarWidthPx: number;
  sidebarQueueVisible: boolean;
  sidebarChannelsVisible: boolean;
  showTopNav: boolean;
  composerStyle: UiComposerStyle;
  workspaceTitle: string;

  // Character, wave horizon & motion
  portraitVisible: boolean;
  portraitScale: number;
  portraitTintHex: string | null;
  brailleDotProfile: BrailleDotProfile;
  brailleBloom: number;
  horizonVisible: boolean;
  horizonOpacity: number;
  horizonStyle: BrailleHorizonStyle;
  statusReadoutVisible: boolean;
  showJevRecall: boolean;
  animationSpeed: number;
  reducedMotion: boolean;
}

export interface UiCustomizationPatch extends Partial<UiCustomizationConfig> {
  presetId?: string;
  backgroundPresetId?: string;
  backgroundOpacity?: number;
  glassBlurPx?: number;
  activeView?: UiPanelView;
  mood?: ExpressionMood;
  showThinking?: boolean;
  voiceMuted?: boolean;
  openCustomizer?: boolean;
  closeCustomizer?: boolean;
  canvasAutoArrange?: boolean;
  canvasMoodAll?: ExpressionMood;
  resetAll?: boolean;
}

export interface AccentSwatchDefinition {
  id: Exclude<UiAccentPreset, 'custom'>;
  label: string;
  accentHex: string;
  accentSoftHex: string;
  accentBrightHex: string;
  surfaceHex: string;
}

export const ACCENT_SWATCHES: readonly AccentSwatchDefinition[] = [
  {
    id: 'slate',
    label: 'Slate',
    accentHex: '#94A3B8',
    accentSoftHex: '#CBD5E1',
    accentBrightHex: '#E2E8F0',
    surfaceHex: '#11141B',
  },
  {
    id: 'emerald',
    label: 'Emerald',
    accentHex: '#34D399',
    accentSoftHex: '#6EE7B7',
    accentBrightHex: '#4ADE80',
    surfaceHex: '#0A1613',
  },
  {
    id: 'cyan',
    label: 'Glacier Cyan',
    accentHex: '#38BDF8',
    accentSoftHex: '#7DD3FC',
    accentBrightHex: '#22D3EE',
    surfaceHex: '#08141D',
  },
  {
    id: 'violet',
    label: 'Cyber Violet',
    accentHex: '#A855F7',
    accentSoftHex: '#D8B4FE',
    accentBrightHex: '#C084FC',
    surfaceHex: '#120B1E',
  },
  {
    id: 'amber',
    label: 'Solaris Amber',
    accentHex: '#F59E0B',
    accentSoftHex: '#FCD34D',
    accentBrightHex: '#FBBF24',
    surfaceHex: '#161108',
  },
  {
    id: 'rose',
    label: 'Sakura Rose',
    accentHex: '#FB7185',
    accentSoftHex: '#FDA4AF',
    accentBrightHex: '#F472B6',
    surfaceHex: '#190B12',
  },
  {
    id: 'crimson',
    label: 'Crimson',
    accentHex: '#EF4444',
    accentSoftHex: '#FCA5A5',
    accentBrightHex: '#F87171',
    surfaceHex: '#19090B',
  },
  {
    id: 'mono',
    label: 'Monograph',
    accentHex: '#E2E8F0',
    accentSoftHex: '#F8FAFC',
    accentBrightHex: '#CBD5E1',
    surfaceHex: '#090B0E',
  },
];

export interface CuratedUiPreset {
  id: string;
  name: string;
  subtitle: string;
  config: Partial<UiCustomizationConfig>;
  backgroundPresetId: BackgroundPresetKind;
  backgroundOpacity: number;
  glassBlurPx: number;
  mood?: ExpressionMood;
}

export const CURATED_UI_PRESETS: readonly CuratedUiPreset[] = [
  {
    id: 'slate-terminal',
    name: 'Slate Monochrome',
    subtitle: 'Modern graphite glass & neutral telemetry',
    backgroundPresetId: 'aurora-drift',
    backgroundOpacity: 0.58,
    glassBlurPx: 14,
    mood: 'focused',
    config: {
      accentPreset: 'slate',
      accentHex: '#94A3B8',
      accentSoftHex: '#CBD5E1',
      accentBrightHex: '#E2E8F0',
      surfaceHex: '#11141B',
      textHex: '#F1F5F9',
      panelAlpha: 0.80,
      borderOpacity: 0.22,
      glowIntensity: 0.30,
      cornerRadius: 'rounded',
      density: 'comfortable',
      fontPreset: 'jakarta',
      fontScale: 1.0,
      sidebarVisible: true,
      sidebarPosition: 'left',
      sidebarWidthPx: 296,
      sidebarQueueVisible: true,
      sidebarChannelsVisible: true,
      showTopNav: true,
      composerStyle: 'docked',
      workspaceTitle: 'Hermes',
      portraitVisible: true,
      portraitScale: 1.0,
      portraitTintHex: null,
      brailleDotProfile: 'balanced',
      brailleBloom: 0.75,
      horizonVisible: true,
      horizonOpacity: 0.8,
      horizonStyle: 'silk',
      statusReadoutVisible: true,
      animationSpeed: 1.0,
      reducedMotion: false,
    },
  },
  {
    id: 'emerald-terminal',
    name: 'Emerald Terminal',
    subtitle: 'Signature smoked-emerald Hermes cockpit',
    backgroundPresetId: 'aurora-drift',
    backgroundOpacity: 0.64,
    glassBlurPx: 12,
    mood: 'happy',
    config: {
      accentPreset: 'emerald',
      accentHex: '#34D399',
      accentSoftHex: '#6EE7B7',
      accentBrightHex: '#4ADE80',
      surfaceHex: '#0A1613',
      textHex: '#ECF3F0',
      panelAlpha: 0.76,
      borderOpacity: 0.42,
      glowIntensity: 1.0,
      cornerRadius: 'rounded',
      density: 'comfortable',
      fontPreset: 'jakarta',
      fontScale: 1.0,
      sidebarVisible: true,
      sidebarPosition: 'left',
      sidebarWidthPx: 296,
      sidebarQueueVisible: true,
      sidebarChannelsVisible: true,
      showTopNav: true,
      composerStyle: 'docked',
      workspaceTitle: 'Hermes',
      portraitVisible: true,
      portraitScale: 1.0,
      portraitTintHex: null,
      brailleDotProfile: 'balanced',
      brailleBloom: 0.75,
      horizonVisible: true,
      horizonOpacity: 0.8,
      horizonStyle: 'silk',
      statusReadoutVisible: true,
      animationSpeed: 1.0,
      reducedMotion: false,
    },
  },
  {
    id: 'cyber-violet',
    name: 'Cyber Violet',
    subtitle: 'Neon plasma nebula & ultraviolet glass',
    backgroundPresetId: 'nebula',
    backgroundOpacity: 0.72,
    glassBlurPx: 16,
    mood: 'focused',
    config: {
      accentPreset: 'violet',
      accentHex: '#A855F7',
      accentSoftHex: '#D8B4FE',
      accentBrightHex: '#C084FC',
      surfaceHex: '#120B1E',
      textHex: '#F3EEFA',
      panelAlpha: 0.74,
      borderOpacity: 0.54,
      glowIntensity: 1.35,
      cornerRadius: 'rounded',
      density: 'comfortable',
      fontPreset: 'display',
      fontScale: 1.0,
      portraitTintHex: '#E9D5FF',
      brailleDotProfile: 'phosphor',
      brailleBloom: 1.1,
      horizonVisible: true,
      horizonOpacity: 0.85,
      horizonStyle: 'pulse',
    },
  },
  {
    id: 'solaris-amber',
    name: 'Solaris Amber',
    subtitle: 'Retro phosphor CRT monospace telemetry',
    backgroundPresetId: 'scanline-shimmer',
    backgroundOpacity: 0.78,
    glassBlurPx: 8,
    mood: 'focused',
    config: {
      accentPreset: 'amber',
      accentHex: '#F59E0B',
      accentSoftHex: '#FCD34D',
      accentBrightHex: '#FBBF24',
      surfaceHex: '#141007',
      textHex: '#FEF3C7',
      panelAlpha: 0.82,
      borderOpacity: 0.58,
      glowIntensity: 1.25,
      cornerRadius: 'sharp',
      density: 'compact',
      fontPreset: 'mono',
      fontScale: 0.98,
      composerStyle: 'docked',
      portraitTintHex: '#FDE68A',
      brailleDotProfile: 'crisp',
      brailleBloom: 0.85,
      horizonVisible: true,
      horizonOpacity: 0.72,
      horizonStyle: 'peaks',
    },
  },
  {
    id: 'glacier-cyan',
    name: 'Glacier Ice',
    subtitle: 'Frosted Arctic blue glass & orbital nodes',
    backgroundPresetId: 'constellation-drift',
    backgroundOpacity: 0.68,
    glassBlurPx: 20,
    mood: 'neutral',
    config: {
      accentPreset: 'cyan',
      accentHex: '#38BDF8',
      accentSoftHex: '#7DD3FC',
      accentBrightHex: '#22D3EE',
      surfaceHex: '#07131C',
      textHex: '#E0F2FE',
      panelAlpha: 0.68,
      borderOpacity: 0.48,
      glowIntensity: 1.15,
      cornerRadius: 'rounded',
      density: 'airy',
      fontPreset: 'jakarta',
      fontScale: 1.02,
      portraitTintHex: '#BAE6FD',
      horizonVisible: true,
      horizonOpacity: 0.78,
    },
  },
  {
    id: 'sakura-rose',
    name: 'Sakura Nocturne',
    subtitle: 'Soft petal pills & bioluminescent tide',
    backgroundPresetId: 'horizon-tide',
    backgroundOpacity: 0.7,
    glassBlurPx: 14,
    mood: 'happy',
    config: {
      accentPreset: 'rose',
      accentHex: '#FB7185',
      accentSoftHex: '#FDA4AF',
      accentBrightHex: '#F472B6',
      surfaceHex: '#180B12',
      textHex: '#FFF1F2',
      panelAlpha: 0.74,
      borderOpacity: 0.5,
      glowIntensity: 1.2,
      cornerRadius: 'pill',
      density: 'comfortable',
      fontPreset: 'jakarta',
      fontScale: 1.0,
      portraitScale: 1.1,
      portraitTintHex: '#FECDD3',
      horizonVisible: true,
      horizonOpacity: 0.88,
    },
  },
  {
    id: 'zen-mono',
    name: 'Stealth Monograph',
    subtitle: 'Zero-clutter minimalist focus shell',
    backgroundPresetId: 'braille-rain',
    backgroundOpacity: 0.42,
    glassBlurPx: 10,
    mood: 'neutral',
    config: {
      accentPreset: 'mono',
      accentHex: '#E2E8F0',
      accentSoftHex: '#F8FAFC',
      accentBrightHex: '#CBD5E1',
      surfaceHex: '#090B0E',
      textHex: '#F1F5F9',
      panelAlpha: 0.84,
      borderOpacity: 0.24,
      glowIntensity: 0.35,
      cornerRadius: 'sharp',
      density: 'compact',
      fontPreset: 'mono',
      fontScale: 0.98,
      sidebarVisible: false,
      showTopNav: true,
      composerStyle: 'minimal',
      portraitVisible: true,
      portraitScale: 0.96,
      portraitTintHex: '#F1F5F9',
      horizonVisible: false,
      horizonOpacity: 0.35,
      statusReadoutVisible: true,
    },
  },
];

export const DEFAULT_UI_CONFIG: UiCustomizationConfig = {
  accentPreset: 'slate',
  accentHex: '#94A3B8',
  accentSoftHex: '#CBD5E1',
  accentBrightHex: '#E2E8F0',
  surfaceHex: '#11141B',
  textHex: '#F1F5F9',
  panelAlpha: 0.80,
  borderOpacity: 0.22,
  glowIntensity: 0.30,
  cornerRadius: 'rounded',
  density: 'comfortable',
  fontPreset: 'jakarta',
  fontScale: 1.0,
  sidebarVisible: true,
  sidebarPosition: 'left',
  sidebarWidthPx: 296,
  sidebarQueueVisible: true,
  sidebarChannelsVisible: true,
  showTopNav: true,
  composerStyle: 'docked',
  workspaceTitle: 'Hermes',
  portraitVisible: true,
  portraitScale: 1.0,
  portraitTintHex: null,
  brailleDotProfile: 'balanced',
  brailleBloom: 0.75,
  horizonVisible: true,
  horizonOpacity: 0.8,
  horizonStyle: 'silk',
  statusReadoutVisible: true,
  showJevRecall: false,
  animationSpeed: 1.0,
  reducedMotion: false,
};

export const UI_STORAGE_KEY = 'duet.uiCustomization.v1';
export const UI_CONFIG_EVENT = 'duet:ui-config-changed';
export const UI_PATCH_EVENT = 'duet:ui-patch-requested';
export const UI_CUSTOMIZER_OPEN_EVENT = 'duet:ui-customizer-open';

export function clampNumber(value: unknown, min: number, max: number, fallback: number, precision = 2): number {
  if (typeof value !== 'number' || !Number.isFinite(value)) return fallback;
  const clamped = Math.max(min, Math.min(max, value));
  const factor = 10 ** precision;
  return Math.round(clamped * factor) / factor;
}

export function normalizeHex(input: unknown): string | null {
  if (typeof input !== 'string') return null;
  let clean = input.trim();
  if (!clean) return null;
  if (!clean.startsWith('#')) clean = `#${clean}`;
  if (/^#[0-9a-fA-F]{3}$/.test(clean)) {
    const [, r, g, b] = clean;
    return `#${r}${r}${g}${g}${b}${b}`.toUpperCase();
  }
  if (/^#[0-9a-fA-F]{6}$/.test(clean)) {
    return clean.toUpperCase();
  }
  return null;
}

export function hexToRgbTuple(hex: string): [number, number, number] {
  const norm = normalizeHex(hex) ?? '#94A3B8';
  const r = parseInt(norm.slice(1, 3), 16);
  const g = parseInt(norm.slice(3, 5), 16);
  const b = parseInt(norm.slice(5, 7), 16);
  return [r, g, b];
}

export function blendHex(hexA: string, hexB: string, t: number): string {
  const [r1, g1, b1] = hexToRgbTuple(hexA);
  const [r2, g2, b2] = hexToRgbTuple(hexB);
  const w = Math.max(0, Math.min(1, t));
  const r = Math.round(r1 + (r2 - r1) * w);
  const g = Math.round(g1 + (g2 - g1) * w);
  const b = Math.round(b1 + (b2 - b1) * w);
  return `#${r.toString(16).padStart(2, '0')}${g.toString(16).padStart(2, '0')}${b.toString(16).padStart(2, '0')}`.toUpperCase();
}

export function getAccentSwatch(id: string): AccentSwatchDefinition | undefined {
  return ACCENT_SWATCHES.find((s) => s.id === id);
}

export function getCuratedPreset(id: string): CuratedUiPreset | undefined {
  return CURATED_UI_PRESETS.find((p) => p.id === id);
}

const VALID_ACCENT_PRESETS = new Set<UiAccentPreset>([
  'slate',
  'emerald',
  'cyan',
  'violet',
  'amber',
  'rose',
  'crimson',
  'mono',
  'custom',
]);
const VALID_CORNER_RADIUS = new Set<UiCornerRadius>(['sharp', 'rounded', 'pill']);
const VALID_FONT_PRESETS = new Set<UiFontPreset>(['jakarta', 'mono', 'display', 'system']);
const VALID_SIDEBAR_POSITIONS = new Set<UiSidebarPosition>(['left', 'right']);
const VALID_COMPOSER_STYLES = new Set<UiComposerStyle>(['docked', 'floating', 'minimal']);
const VALID_DENSITIES = new Set<UiDensity>(['compact', 'comfortable', 'airy']);
const VALID_VIEWS = new Set<UiPanelView>(['home', 'canvas', 'voice', 'queue', 'cli']);
const VALID_BRAILLE_PROFILES = new Set<BrailleDotProfile>(['crisp', 'balanced', 'phosphor']);
const VALID_HORIZON_STYLES = new Set<BrailleHorizonStyle>(['silk', 'peaks', 'pulse']);
const VALID_MOODS = new Set<ExpressionMood>([
  'neutral',
  'listening',
  'thinking',
  'speaking',
  'happy',
  'focused',
  'alert',
]);
const VALID_BG_PRESETS = new Set<string>([
  'aurora-drift',
  'mesh-gradient',
  'braille-rain',
  'scanline-shimmer',
  'nebula',
  'constellation-drift',
  'horizon-tide',
]);

/**
 * Validates and clamps any UI customization patch (from UI sliders, slash commands, or LLM output).
 */
export function sanitizeUiPatch(raw: unknown): UiCustomizationPatch {
  if (!raw || typeof raw !== 'object') return {};
  const src = raw as Record<string, unknown>;
  const out: UiCustomizationPatch = {};

  if (src.resetAll === true) {
    out.resetAll = true;
    return out;
  }

  if (typeof src.presetId === 'string') {
    const preset = getCuratedPreset(src.presetId.trim().toLowerCase());
    if (preset) out.presetId = preset.id;
  }

  if (typeof src.accentPreset === 'string' && VALID_ACCENT_PRESETS.has(src.accentPreset as UiAccentPreset)) {
    const ap = src.accentPreset as UiAccentPreset;
    out.accentPreset = ap;
    const swatch = getAccentSwatch(ap);
    if (swatch) {
      out.accentHex = swatch.accentHex;
      out.accentSoftHex = swatch.accentSoftHex;
      out.accentBrightHex = swatch.accentBrightHex;
      if (!src.surfaceHex) {
        out.surfaceHex = swatch.surfaceHex;
      }
    }
  }

  const accentHex = normalizeHex(src.accentHex);
  if (accentHex) {
    out.accentHex = accentHex;
    if (!out.accentPreset) {
      const matched = ACCENT_SWATCHES.find((s) => s.accentHex === accentHex);
      out.accentPreset = matched ? matched.id : 'custom';
    }
    if (!normalizeHex(src.accentSoftHex)) {
      out.accentSoftHex = blendHex(accentHex, '#FFFFFF', 0.42);
    }
    if (!normalizeHex(src.accentBrightHex)) {
      out.accentBrightHex = blendHex(accentHex, '#FFFFFF', 0.22);
    }
    if (!normalizeHex(src.surfaceHex) && out.accentPreset === 'custom') {
      out.surfaceHex = blendHex('#0B0D12', accentHex, 0.08);
    }
  }

  const softHex = normalizeHex(src.accentSoftHex);
  if (softHex) out.accentSoftHex = softHex;

  const brightHex = normalizeHex(src.accentBrightHex);
  if (brightHex) out.accentBrightHex = brightHex;

  const surfaceHex = normalizeHex(src.surfaceHex);
  if (surfaceHex) {
    // Keep surface dark enough for crisp readable contrast
    const [r, g, b] = hexToRgbTuple(surfaceHex);
    const lum = (0.299 * r + 0.587 * g + 0.114 * b) / 255;
    out.surfaceHex = lum > 0.22 ? blendHex('#06090B', surfaceHex, 0.28) : surfaceHex;
  }

  const textHex = normalizeHex(src.textHex);
  if (textHex) out.textHex = textHex;

  if (typeof src.panelAlpha === 'number') out.panelAlpha = clampNumber(src.panelAlpha, 0.32, 0.95, DEFAULT_UI_CONFIG.panelAlpha);
  if (typeof src.borderOpacity === 'number') out.borderOpacity = clampNumber(src.borderOpacity, 0.08, 0.9, DEFAULT_UI_CONFIG.borderOpacity);
  if (typeof src.glowIntensity === 'number') out.glowIntensity = clampNumber(src.glowIntensity, 0, 2.0, DEFAULT_UI_CONFIG.glowIntensity);

  if (typeof src.cornerRadius === 'string' && VALID_CORNER_RADIUS.has(src.cornerRadius as UiCornerRadius)) {
    out.cornerRadius = src.cornerRadius as UiCornerRadius;
  }
  if (typeof src.density === 'string' && VALID_DENSITIES.has(src.density as UiDensity)) {
    out.density = src.density as UiDensity;
  }
  if (typeof src.fontPreset === 'string' && VALID_FONT_PRESETS.has(src.fontPreset as UiFontPreset)) {
    out.fontPreset = src.fontPreset as UiFontPreset;
  }
  if (typeof src.fontScale === 'number') out.fontScale = clampNumber(src.fontScale, 0.85, 1.25, DEFAULT_UI_CONFIG.fontScale);

  if (typeof src.sidebarVisible === 'boolean') out.sidebarVisible = src.sidebarVisible;
  if (typeof src.sidebarPosition === 'string' && VALID_SIDEBAR_POSITIONS.has(src.sidebarPosition as UiSidebarPosition)) {
    out.sidebarPosition = src.sidebarPosition as UiSidebarPosition;
  }
  if (typeof src.sidebarWidthPx === 'number') out.sidebarWidthPx = clampNumber(src.sidebarWidthPx, 236, 420, DEFAULT_UI_CONFIG.sidebarWidthPx, 0);
  if (typeof src.sidebarQueueVisible === 'boolean') out.sidebarQueueVisible = src.sidebarQueueVisible;
  if (typeof src.sidebarChannelsVisible === 'boolean') out.sidebarChannelsVisible = src.sidebarChannelsVisible;
  if (typeof src.showTopNav === 'boolean') out.showTopNav = src.showTopNav;
  if (typeof src.showJevRecall === 'boolean') out.showJevRecall = src.showJevRecall;
  if (typeof src.composerStyle === 'string' && VALID_COMPOSER_STYLES.has(src.composerStyle as UiComposerStyle)) {
    out.composerStyle = src.composerStyle as UiComposerStyle;
  }
  if (typeof src.workspaceTitle === 'string' && src.workspaceTitle.trim()) {
    out.workspaceTitle = src.workspaceTitle.trim().slice(0, 28);
  }

  if (typeof src.portraitVisible === 'boolean') out.portraitVisible = src.portraitVisible;
  if (typeof src.portraitScale === 'number') out.portraitScale = clampNumber(src.portraitScale, 0.6, 1.45, DEFAULT_UI_CONFIG.portraitScale);
  if (src.portraitTintHex === null || src.portraitTintHex === 'auto' || src.portraitTintHex === '') {
    if ('portraitTintHex' in src) out.portraitTintHex = null;
  } else {
    const pt = normalizeHex(src.portraitTintHex);
    if (pt) out.portraitTintHex = pt;
  }

  if (typeof src.brailleDotProfile === 'string' && VALID_BRAILLE_PROFILES.has(src.brailleDotProfile as BrailleDotProfile)) {
    out.brailleDotProfile = src.brailleDotProfile as BrailleDotProfile;
  }
  if (typeof src.brailleBloom === 'number') out.brailleBloom = clampNumber(src.brailleBloom, 0, 1.5, DEFAULT_UI_CONFIG.brailleBloom);
  if (typeof src.horizonVisible === 'boolean') out.horizonVisible = src.horizonVisible;
  if (typeof src.horizonOpacity === 'number') out.horizonOpacity = clampNumber(src.horizonOpacity, 0, 1, DEFAULT_UI_CONFIG.horizonOpacity);
  if (typeof src.horizonStyle === 'string' && VALID_HORIZON_STYLES.has(src.horizonStyle as BrailleHorizonStyle)) {
    out.horizonStyle = src.horizonStyle as BrailleHorizonStyle;
  }
  if (typeof src.statusReadoutVisible === 'boolean') out.statusReadoutVisible = src.statusReadoutVisible;
  if (typeof src.animationSpeed === 'number') out.animationSpeed = clampNumber(src.animationSpeed, 0.3, 1.8, DEFAULT_UI_CONFIG.animationSpeed);
  if (typeof src.reducedMotion === 'boolean') out.reducedMotion = src.reducedMotion;

  if (typeof src.backgroundPresetId === 'string') {
    const bgId = src.backgroundPresetId.trim().toLowerCase();
    if (VALID_BG_PRESETS.has(bgId) || bgId.startsWith('custom-')) {
      out.backgroundPresetId = bgId;
    }
  }
  if (typeof src.backgroundOpacity === 'number') out.backgroundOpacity = clampNumber(src.backgroundOpacity, 0.15, 0.85, 0.64);
  if (typeof src.glassBlurPx === 'number') out.glassBlurPx = clampNumber(src.glassBlurPx, 0, 28, 12, 0);

  if (typeof src.activeView === 'string' && VALID_VIEWS.has(src.activeView as UiPanelView)) {
    out.activeView = src.activeView as UiPanelView;
  }
  if (typeof src.mood === 'string' && VALID_MOODS.has(src.mood as ExpressionMood)) {
    out.mood = src.mood as ExpressionMood;
  }
  if (typeof src.showThinking === 'boolean') out.showThinking = src.showThinking;
  if (typeof src.voiceMuted === 'boolean') out.voiceMuted = src.voiceMuted;
  if (typeof src.openCustomizer === 'boolean') out.openCustomizer = src.openCustomizer;
  if (typeof src.closeCustomizer === 'boolean') out.closeCustomizer = src.closeCustomizer;
  if (typeof src.canvasAutoArrange === 'boolean') out.canvasAutoArrange = src.canvasAutoArrange;
  if (typeof src.canvasMoodAll === 'string' && VALID_MOODS.has(src.canvasMoodAll as ExpressionMood)) {
    out.canvasMoodAll = src.canvasMoodAll as ExpressionMood;
  }

  return out;
}

const CONFIG_KEYS = Object.keys(DEFAULT_UI_CONFIG) as Array<keyof UiCustomizationConfig>;

export function applyUiPatch(
  current: UiCustomizationConfig,
  rawPatch: UiCustomizationPatch
): {
  next: UiCustomizationConfig;
  sanitized: UiCustomizationPatch;
  changedKeys: string[];
  summary: string;
} {
  const sanitized = sanitizeUiPatch(rawPatch);

  if (sanitized.resetAll) {
    const defBg = CURATED_UI_PRESETS[0];
    return {
      next: { ...DEFAULT_UI_CONFIG },
      sanitized: {
        ...sanitized,
        backgroundPresetId: defBg.backgroundPresetId,
        backgroundOpacity: defBg.backgroundOpacity,
        glassBlurPx: defBg.glassBlurPx,
      },
      changedKeys: ['resetAll'],
      summary: 'Restored default Slate Monochrome UI & backdrop',
    };
  }

  let next: UiCustomizationConfig = { ...current };
  const changedSet = new Set<string>();

  if (sanitized.presetId) {
    const preset = getCuratedPreset(sanitized.presetId);
    if (preset) {
      next = { ...next, ...preset.config };
      changedSet.add(`preset:${preset.name}`);
      if (!sanitized.backgroundPresetId) sanitized.backgroundPresetId = preset.backgroundPresetId;
      if (sanitized.backgroundOpacity === undefined) sanitized.backgroundOpacity = preset.backgroundOpacity;
      if (sanitized.glassBlurPx === undefined) sanitized.glassBlurPx = preset.glassBlurPx;
      if (!sanitized.mood && preset.mood) sanitized.mood = preset.mood;
    }
  }

  for (const key of CONFIG_KEYS) {
    if (key in sanitized && sanitized[key] !== undefined) {
      const val = sanitized[key];
      if (next[key] !== val) {
        (next as unknown as Record<string, unknown>)[key] = val;
        changedSet.add(key);
      }
    }
  }

  if (sanitized.backgroundPresetId) changedSet.add(`background:${sanitized.backgroundPresetId}`);
  if (sanitized.glassBlurPx !== undefined) changedSet.add(`blur:${sanitized.glassBlurPx}px`);
  if (sanitized.backgroundOpacity !== undefined) changedSet.add(`bgOpacity:${Math.round(sanitized.backgroundOpacity * 100)}%`);
  if (sanitized.activeView) changedSet.add(`view:${sanitized.activeView}`);
  if (sanitized.mood) changedSet.add(`mood:${sanitized.mood}`);
  if (sanitized.canvasAutoArrange) changedSet.add('canvas:arrange');

  const changedKeys = Array.from(changedSet);
  const summary =
    changedKeys.length > 0
      ? `Applied ${changedKeys.slice(0, 5).join(', ')}${changedKeys.length > 5 ? ` (+${changedKeys.length - 5} more)` : ''}`
      : 'No UI changes needed';

  return { next, sanitized, changedKeys, summary };
}

/**
 * Deterministic natural-language & slash-arg UI intent parser so common/explicit
 * customization requests execute instantaneously and work offline or online alongside Gemini.
 */
export function parseNaturalUiIntent(
  rawText: string,
  current: UiCustomizationConfig
): UiCustomizationPatch | null {
  const trimmed = rawText.trim();
  if (!trimmed) return null;
  const lower = trimmed.toLowerCase();

  // Require clear UI customization/navigation intent so ordinary prompts don't misfire
  const hasUiVerb =
    /\b(make|set|change|switch|turn|hide|show|toggle|move|put|resize|scale|rename|style|theme|customize|reset|restore|open|close|go|enable|disable|increase|decrease|boost|reduce|arrange|apply)\b/i.test(
      lower
    );
  const hasUiNoun =
    /\b(ui|interface|theme|preset|color|colour|accent|palette|slate|grey|gray|violet|purple|amber|gold|cyan|blue|rose|pink|crimson|red|emerald|green|mono|monochrome|stealth|zen|sakura|solaris|glacier|sidebar|panel|nav|tabs|header|title|brand|portrait|character|avatar|horizon|wave|footer|composer|input|font|monospace|jakarta|text|corner|corners|sharp|pill|rounded|blur|opacity|glow|border|compact|airy|comfortable|motion|animation|speed|background|backdrop|aurora|braille|dots|bloom|phosphor|peaks|silk|pulse|rain|matrix|scanline|nebula|constellation|mesh|canvas|queue|cli|thread|studio|customizer)\b/i.test(
      lower
    );
  const isDirectPresetOrHex =
    Boolean(getCuratedPreset(lower)) || /#[0-9a-f]{3,6}\b/i.test(lower);

  if (!isDirectPresetOrHex && !(hasUiVerb && hasUiNoun)) {
    return null;
  }

  const patch: UiCustomizationPatch = {};
  let touched = false;
  const mark = () => {
    touched = true;
  };

  // Reset
  if (
    /\b(reset|restore|default)\b.*\b(ui|theme|layout|style|everything|all|settings|interface)\b/i.test(lower) ||
    lower === 'reset'
  ) {
    return { resetAll: true };
  }

  // Open / close customizer studio
  if (/\b(open|show|launch)\b.*\b(studio|customizer|customize|theme picker)\b/i.test(lower)) {
    patch.openCustomizer = true;
    mark();
  }

  // Curated presets
  if (/\b(cyber\s*violet|ultraviolet|violet\s*theme)\b/i.test(lower) || lower === 'cyber-violet') {
    patch.presetId = 'cyber-violet';
    mark();
  } else if (/\b(solaris|amber\s*crt|retro\s*crt|crt\s*theme)\b/i.test(lower) || lower === 'solaris-amber') {
    patch.presetId = 'solaris-amber';
    mark();
  } else if (/\b(glacier|ice\s*cyan|arctic)\b/i.test(lower) || lower === 'glacier-cyan') {
    patch.presetId = 'glacier-cyan';
    mark();
  } else if (/\b(sakura|nocturne|cherry\s*blossom)\b/i.test(lower) || lower === 'sakura-rose') {
    patch.presetId = 'sakura-rose';
    mark();
  } else if (/\b(zen\s*mode|stealth|minimalist\s*focus|monograph)\b/i.test(lower) || lower === 'zen-mono') {
    patch.presetId = 'zen-mono';
    mark();
  } else if (/\b(slate\s*monochrome|slate\s*terminal|slate\s*theme)\b/i.test(lower) || lower === 'slate-terminal') {
    patch.presetId = 'slate-terminal';
    mark();
  } else if (/\b(emerald\s*terminal)\b/i.test(lower) || lower === 'emerald-terminal') {
    patch.presetId = 'emerald-terminal';
    mark();
  }

  // Accent swatches / hex codes
  const hexMatches = trimmed.match(/#[0-9a-fA-F]{3,6}\b/g);
  if (hexMatches && hexMatches.length > 0) {
    const firstHex = normalizeHex(hexMatches[0]);
    if (firstHex) {
      if (/\b(character|portrait|avatar)\b/i.test(lower) && !/\b(ui|accent|theme)\b/i.test(lower)) {
        patch.portraitTintHex = firstHex;
      } else {
        patch.accentHex = firstHex;
      }
      mark();
    }
  } else if (!patch.presetId) {
    if (/\b(slate|grey|gray|graphite|charcoal)\b/i.test(lower)) {
      patch.accentPreset = 'slate';
      mark();
    } else if (/\b(violet|purple|lavender)\b/i.test(lower)) {
      patch.accentPreset = 'violet';
      mark();
    } else if (/\b(cyan|ice|aqua|sky|blue)\b/i.test(lower)) {
      patch.accentPreset = 'cyan';
      mark();
    } else if (/\b(amber|gold|yellow|orange)\b/i.test(lower)) {
      patch.accentPreset = 'amber';
      mark();
    } else if (/\b(rose|pink|magenta|sakura)\b/i.test(lower)) {
      patch.accentPreset = 'rose';
      mark();
    } else if (/\b(crimson|scarlet|red)\b/i.test(lower)) {
      patch.accentPreset = 'crimson';
      mark();
    } else if (/\b(monochrome|monograph|silver|grayscale|greyscale)\b/i.test(lower)) {
      patch.accentPreset = 'mono';
      mark();
    } else if (/\b(emerald|mint|green)\b/i.test(lower)) {
      patch.accentPreset = 'emerald';
      mark();
    }
  }

  // Portrait tint explicit color keywords
  if (/\b(portrait|character|hermes|avatar)\b.*\b(pink|rose|violet|purple|cyan|blue|amber|gold|red|white|silver|green|emerald|auto)\b/i.test(lower)) {
    if (/\bauto\b/i.test(lower)) patch.portraitTintHex = null;
    else if (/\b(pink|rose)\b/i.test(lower)) patch.portraitTintHex = '#F472B6';
    else if (/\b(violet|purple)\b/i.test(lower)) patch.portraitTintHex = '#D8B4FE';
    else if (/\b(cyan|blue)\b/i.test(lower)) patch.portraitTintHex = '#7DD3FC';
    else if (/\b(amber|gold)\b/i.test(lower)) patch.portraitTintHex = '#FCD34D';
    else if (/\b(white|silver)\b/i.test(lower)) patch.portraitTintHex = '#FFFFFF';
    else if (/\b(red|crimson)\b/i.test(lower)) patch.portraitTintHex = '#FCA5A5';
    else if (/\b(green|emerald)\b/i.test(lower)) patch.portraitTintHex = '#6EE7B7';
    mark();
  }

  // Backgrounds
  if (/\b(braille\s*rain|matrix|glyph\s*rain)\b/i.test(lower)) {
    patch.backgroundPresetId = 'braille-rain';
    mark();
  } else if (/\b(aurora|aurora\s*drift)\b/i.test(lower)) {
    patch.backgroundPresetId = 'aurora-drift';
    mark();
  } else if (/\b(mesh|mesh\s*gradient)\b/i.test(lower)) {
    patch.backgroundPresetId = 'mesh-gradient';
    mark();
  } else if (/\b(scanline|scanlines|crt)\b/i.test(lower)) {
    patch.backgroundPresetId = 'scanline-shimmer';
    mark();
  } else if (/\b(nebula|cosmos)\b/i.test(lower)) {
    patch.backgroundPresetId = 'nebula';
    mark();
  } else if (/\b(constellation|stars|orbital)\b/i.test(lower)) {
    patch.backgroundPresetId = 'constellation-drift';
    mark();
  } else if (/\b(horizon\s*tide|tide\s*backdrop)\b/i.test(lower)) {
    patch.backgroundPresetId = 'horizon-tide';
    mark();
  }

  // Blur & Opacity numbers
  const blurMatch = lower.match(/\bblur\b[^0-9]*([0-9]{1,2})/);
  if (blurMatch) {
    patch.glassBlurPx = Number(blurMatch[1]);
    mark();
  } else if (/\b(more blur|heavy blur|extra blur|frostier|frosted)\b/i.test(lower)) {
    patch.glassBlurPx = 22;
    mark();
  } else if (/\b(no blur|remove blur|zero blur|crisp glass)\b/i.test(lower)) {
    patch.glassBlurPx = 0;
    mark();
  }

  const bgOpMatch = lower.match(/\b(?:background\s*opacity|backdrop\s*opacity)\b[^0-9]*([0-9]{1,3})/);
  if (bgOpMatch) {
    const val = Number(bgOpMatch[1]);
    patch.backgroundOpacity = val > 1 ? val / 100 : val;
    mark();
  }

  if (/\b(transparent\s*panels?|see[- ]through|more\s*glass)\b/i.test(lower)) {
    patch.panelAlpha = 0.52;
    mark();
  } else if (/\b(solid\s*panels?|opaque\s*panels?)\b/i.test(lower)) {
    patch.panelAlpha = 0.92;
    mark();
  }

  // Sidebar controls
  if (/\b(hide|remove|close|collapse)\b.*\bsidebar\b/i.test(lower)) {
    patch.sidebarVisible = false;
    mark();
  } else if (/\b(show|open|restore|display)\b.*\bsidebar\b/i.test(lower)) {
    patch.sidebarVisible = true;
    mark();
  }
  if (/\bsidebar\b.*\bright\b|\bright\s*sidebar\b/i.test(lower)) {
    patch.sidebarPosition = 'right';
    patch.sidebarVisible = true;
    mark();
  } else if (/\bsidebar\b.*\bleft\b|\bleft\s*sidebar\b/i.test(lower)) {
    patch.sidebarPosition = 'left';
    patch.sidebarVisible = true;
    mark();
  }
  if (/\b(wider|expand)\b.*\bsidebar\b/i.test(lower)) {
    patch.sidebarWidthPx = Math.min(420, current.sidebarWidthPx + 48);
    mark();
  } else if (/\b(narrow|slimmer|shrink)\b.*\bsidebar\b/i.test(lower)) {
    patch.sidebarWidthPx = Math.max(236, current.sidebarWidthPx - 44);
    mark();
  }

  // Top nav
  if (/\b(hide|remove)\b.*\b(top\s*nav|tabs|navigation|toolbar)\b/i.test(lower)) {
    patch.showTopNav = false;
    mark();
  } else if (/\b(show|restore)\b.*\b(top\s*nav|tabs|navigation|toolbar)\b/i.test(lower)) {
    patch.showTopNav = true;
    mark();
  }

  // Horizon / Wave & Braille Optics
  if (/\b(hide|remove|turn\s*off|disable)\b.*\b(horizon|wave|waves)\b/i.test(lower)) {
    patch.horizonVisible = false;
    mark();
  } else if (/\b(show|enable|turn\s*on)\b.*\b(horizon|wave|waves)\b/i.test(lower)) {
    patch.horizonVisible = true;
    mark();
  }
  if (/\b(spectral\s*peaks?|peaks\s*horizon|bars?\s*horizon)\b/i.test(lower)) {
    patch.horizonStyle = 'peaks';
    patch.horizonVisible = true;
    mark();
  } else if (/\b(pulse\s*tide|interference\s*wave|pulse\s*horizon)\b/i.test(lower)) {
    patch.horizonStyle = 'pulse';
    patch.horizonVisible = true;
    mark();
  } else if (/\b(silk\s*waves?|silk\s*horizon|fluid\s*horizon)\b/i.test(lower)) {
    patch.horizonStyle = 'silk';
    patch.horizonVisible = true;
    mark();
  }
  if (/\b(crisp\s*dots?|crisp\s*braille|sharp\s*dots?)\b/i.test(lower)) {
    patch.brailleDotProfile = 'crisp';
    mark();
  } else if (/\b(phosphor\s*dots?|phosphor\s*braille|phosphor\s*bloom)\b/i.test(lower)) {
    patch.brailleDotProfile = 'phosphor';
    patch.brailleBloom = 1.15;
    mark();
  } else if (/\b(balanced\s*dots?|balanced\s*braille)\b/i.test(lower)) {
    patch.brailleDotProfile = 'balanced';
    mark();
  }
  if (/\b(no\s*bloom|disable\s*bloom|zero\s*bloom)\b/i.test(lower)) {
    patch.brailleBloom = 0;
    mark();
  } else if (/\b(high\s*bloom|max\s*bloom|extra\s*bloom|boost\s*bloom)\b/i.test(lower)) {
    patch.brailleBloom = 1.35;
    mark();
  }

  // Character portrait visibility & scale
  if (/\b(hide|remove|turn\s*off)\b.*\b(character|portrait|avatar|hermes\s*face)\b/i.test(lower)) {
    patch.portraitVisible = false;
    mark();
  } else if (/\b(show|display|restore|turn\s*on)\b.*\b(character|portrait|avatar)\b/i.test(lower)) {
    patch.portraitVisible = true;
    mark();
  }
  if (
    /\b(bigger|larger|huge|giant|scale\s*up|zoom\s*in)\b[^,.]*\b(character|portrait|avatar|hermes)\b|\b(character|portrait|avatar|hermes)\b[^,.]*\b(bigger|larger|huge|giant|scale\s*up|zoom\s*in)\b/i.test(
      lower
    )
  ) {
    patch.portraitScale = 1.3;
    patch.portraitVisible = true;
    mark();
  } else if (
    /\b(smaller|tiny|compact|mini|scale\s*down)\b[^,.]*\b(character|portrait|avatar|hermes)\b|\b(character|portrait|avatar|hermes)\b[^,.]*\b(smaller|tiny|compact|mini|scale\s*down)\b/i.test(
      lower
    )
  ) {
    patch.portraitScale = 0.78;
    patch.portraitVisible = true;
    mark();
  }

  // Status readout
  if (/\b(hide|remove)\b.*\b(status|readout|subtitles)\b/i.test(lower)) {
    patch.statusReadoutVisible = false;
    mark();
  } else if (/\b(show|restore)\b.*\b(status|readout|subtitles)\b/i.test(lower)) {
    patch.statusReadoutVisible = true;
    mark();
  }

  // Corner radius
  if (/\b(sharp|square|boxy|brutalist|angular)\b/i.test(lower)) {
    patch.cornerRadius = 'sharp';
    mark();
  } else if (/\b(pill|super\s*round|extra\s*round|bubbly)\b/i.test(lower)) {
    patch.cornerRadius = 'pill';
    mark();
  } else if (/\b(rounded\s*corners|normal\s*corners)\b/i.test(lower)) {
    patch.cornerRadius = 'rounded';
    mark();
  }

  // Density & Composer style
  if (/\b(compact|dense|tight)\b/i.test(lower)) {
    patch.density = 'compact';
    mark();
  } else if (/\b(airy|spacious|relaxed)\b/i.test(lower)) {
    patch.density = 'airy';
    mark();
  }
  if (/\b(minimal\s*composer|minimal\s*input|clean\s*input)\b/i.test(lower)) {
    patch.composerStyle = 'minimal';
    mark();
  } else if (/\b(floating\s*composer|floating\s*input)\b/i.test(lower)) {
    patch.composerStyle = 'floating';
    mark();
  } else if (/\b(docked\s*composer|docked\s*input)\b/i.test(lower)) {
    patch.composerStyle = 'docked';
    mark();
  }

  // Fonts & text size
  if (/\b(mono|monospace|code\s*font|terminal\s*font|jetbrains)\b/i.test(lower)) {
    patch.fontPreset = 'mono';
    mark();
  } else if (/\b(display\s*font|archivo|bold\s*font)\b/i.test(lower)) {
    patch.fontPreset = 'display';
    mark();
  } else if (/\b(system\s*font)\b/i.test(lower)) {
    patch.fontPreset = 'system';
    mark();
  } else if (/\b(default\s*font|jakarta)\b/i.test(lower)) {
    patch.fontPreset = 'jakarta';
    mark();
  }

  if (/\b(bigger|larger|increase)\b.*\b(font|text|type)\b/i.test(lower)) {
    patch.fontScale = 1.12;
    mark();
  } else if (/\b(smaller|tiny|decrease)\b.*\b(font|text|type)\b/i.test(lower)) {
    patch.fontScale = 0.9;
    mark();
  }

  // Motion & speed
  if (/\b(calm\s*motion|reduce\s*motion|reduced\s*motion|freeze\s*animations|stop\s*moving)\b/i.test(lower)) {
    patch.reducedMotion = true;
    mark();
  } else if (/\b(full\s*motion|enable\s*animations)\b/i.test(lower)) {
    patch.reducedMotion = false;
    mark();
  }
  if (/\b(faster|speed\s*up|energetic)\b.*\b(animat|character|wave|motion)\b/i.test(lower)) {
    patch.animationSpeed = 1.45;
    patch.reducedMotion = false;
    mark();
  } else if (/\b(slower|calm|gentle)\b.*\b(animat|character|wave|motion)\b/i.test(lower)) {
    patch.animationSpeed = 0.6;
    mark();
  }

  // Glow
  if (/\b(high\s*glow|extra\s*glow|neon\s*glow|boost\s*glow)\b/i.test(lower)) {
    patch.glowIntensity = 1.65;
    mark();
  } else if (/\b(no\s*glow|flat|remove\s*glow)\b/i.test(lower)) {
    patch.glowIntensity = 0;
    mark();
  }

  // Rename workspace title
  const renameMatch = trimmed.match(/\b(?:rename|set)\s+(?:workspace|header|brand|title)\s+(?:to\s+)?["']?([a-zA-Z0-9 _>.#/-]{2,26})["']?/i);
  if (renameMatch) {
    patch.workspaceTitle = renameMatch[1].trim();
    mark();
  }

  // Active view navigation
  if (/\b(open|switch\s+to|show|go\s+to)\b.*\bcanvas\b/i.test(lower)) {
    patch.activeView = 'canvas';
    mark();
  } else if (/\b(open|switch\s+to|show|go\s+to)\b.*\b(thread|transcript|chat\s*log)\b/i.test(lower)) {
    patch.activeView = 'voice';
    mark();
  } else if (/\b(open|switch\s+to|show|go\s+to)\b.*\b(queue|approvals)\b/i.test(lower)) {
    patch.activeView = 'queue';
    mark();
  } else if (/\b(open|switch\s+to|show|go\s+to)\b.*\bcli\b/i.test(lower)) {
    patch.activeView = 'cli';
    mark();
  } else if (/\b(go\s*home|open\s*home|home\s*view)\b/i.test(lower)) {
    patch.activeView = 'home';
    mark();
  }

  // Canvas multi-session actions
  if (/\b(auto[- ]?arrange|tidy|organize)\b.*\b(canvas|sessions|characters|shapes)\b/i.test(lower)) {
    patch.canvasAutoArrange = true;
    mark();
  }

  return touched ? patch : null;
}

export const UI_THEME_EPOCH_KEY = 'duet.uiThemeEpoch.v1';
export const UI_THEME_EPOCH_VALUE = 'slate-v1';

export function loadUiConfig(): UiCustomizationConfig {
  if (typeof window === 'undefined') return { ...DEFAULT_UI_CONFIG };
  try {
    const raw = window.localStorage.getItem(UI_STORAGE_KEY);
    const epoch = window.localStorage.getItem(UI_THEME_EPOCH_KEY);
    if (!raw) {
      window.localStorage.setItem(UI_THEME_EPOCH_KEY, UI_THEME_EPOCH_VALUE);
      return { ...DEFAULT_UI_CONFIG };
    }
    const parsed = JSON.parse(raw) as unknown;
    const { next } = applyUiPatch(DEFAULT_UI_CONFIG, sanitizeUiPatch(parsed));
    if (
      epoch !== UI_THEME_EPOCH_VALUE &&
      next.accentPreset === 'emerald' &&
      next.accentHex === '#34D399' &&
      next.surfaceHex === '#0A1613' &&
      next.textHex === '#ECF3F0' &&
      next.panelAlpha === 0.76 &&
      next.borderOpacity === 0.42 &&
      next.glowIntensity === 1 &&
      !next.portraitTintHex
    ) {
      const migrated: UiCustomizationConfig = {
        ...next,
        accentPreset: DEFAULT_UI_CONFIG.accentPreset,
        accentHex: DEFAULT_UI_CONFIG.accentHex,
        accentSoftHex: DEFAULT_UI_CONFIG.accentSoftHex,
        accentBrightHex: DEFAULT_UI_CONFIG.accentBrightHex,
        surfaceHex: DEFAULT_UI_CONFIG.surfaceHex,
        textHex: DEFAULT_UI_CONFIG.textHex,
        panelAlpha: DEFAULT_UI_CONFIG.panelAlpha,
        borderOpacity: DEFAULT_UI_CONFIG.borderOpacity,
        glowIntensity: DEFAULT_UI_CONFIG.glowIntensity,
      };
      saveUiConfig(migrated);
      return migrated;
    }
    if (epoch !== UI_THEME_EPOCH_VALUE) {
      window.localStorage.setItem(UI_THEME_EPOCH_KEY, UI_THEME_EPOCH_VALUE);
    }
    return next;
  } catch {
    return { ...DEFAULT_UI_CONFIG };
  }
}

export function saveUiConfig(config: UiCustomizationConfig): void {
  if (typeof window === 'undefined') return;
  try {
    window.localStorage.setItem(UI_STORAGE_KEY, JSON.stringify(config));
    window.localStorage.setItem(UI_THEME_EPOCH_KEY, UI_THEME_EPOCH_VALUE);
  } catch {
    // Ignore storage quota / privacy mode errors
  }
}

export function applyUiConfigToDom(config: UiCustomizationConfig): void {
  if (typeof document === 'undefined') return;
  const root = document.documentElement;
  const [ar, ag, ab] = hexToRgbTuple(config.accentHex);
  const [sr, sg, sb] = hexToRgbTuple(config.accentSoftHex);
  const [surR, surG, surB] = hexToRgbTuple(config.surfaceHex);

  const radiusCard =
    config.cornerRadius === 'sharp' ? '4px' : config.cornerRadius === 'pill' ? '26px' : '16px';
  const radiusControl =
    config.cornerRadius === 'sharp' ? '4px' : config.cornerRadius === 'pill' ? '18px' : '12px';

  const fontStack =
    config.fontPreset === 'mono'
      ? "var(--font-mono)"
      : config.fontPreset === 'display'
      ? "var(--font-display)"
      : config.fontPreset === 'system'
      ? 'system-ui, -apple-system, sans-serif'
      : "var(--font-sans)";

  root.style.setProperty('--ui-accent', config.accentHex);
  root.style.setProperty('--ui-accent-rgb', `${ar}, ${ag}, ${ab}`);
  root.style.setProperty('--ui-accent-soft', config.accentSoftHex);
  root.style.setProperty('--ui-accent-soft-rgb', `${sr}, ${sg}, ${sb}`);
  root.style.setProperty('--ui-accent-bright', config.accentBrightHex);
  root.style.setProperty('--ui-surface-rgb', `${surR}, ${surG}, ${surB}`);
  root.style.setProperty('--ui-text', config.textHex);
  root.style.setProperty('--glass-panel-alpha', String(config.panelAlpha));
  root.style.setProperty('--ui-panel-alpha', String(config.panelAlpha));
  root.style.setProperty('--ui-border-alpha', String(config.borderOpacity));
  root.style.setProperty('--ui-glow-alpha', String(Number((0.09 * config.glowIntensity).toFixed(3))));
  root.style.setProperty('--ui-radius-card', radiusCard);
  root.style.setProperty('--ui-radius-control', radiusControl);
  root.style.setProperty('--ui-font-scale', String(config.fontScale));
  root.style.setProperty('--ui-font-active', fontStack);
  root.style.setProperty('--ui-anim-speed', String(config.animationSpeed));
  root.style.setProperty('--ui-sidebar-w', `${config.sidebarWidthPx}px`);
  root.style.setProperty('--ui-portrait-scale', String(config.portraitScale));
  root.style.setProperty('--ui-horizon-opacity', String(config.horizonOpacity));

  root.dataset.uiAccent = config.accentPreset;
  root.dataset.uiThemed = config.accentPreset === 'slate' ? 'default' : 'custom';
  root.dataset.uiRadius = config.cornerRadius;
  root.dataset.uiFont = config.fontPreset;
  root.dataset.uiCompact = config.density === 'compact' ? 'true' : 'false';
  root.dataset.uiMotion = config.reducedMotion ? 'reduced' : 'full';
}

export function hydrateUiConfigOnBoot(): UiCustomizationConfig {
  const cfg = loadUiConfig();
  applyUiConfigToDom(cfg);
  return cfg;
}

export function publishUiConfig(config: UiCustomizationConfig): void {
  applyUiConfigToDom(config);
  saveUiConfig(config);
  if (typeof window !== 'undefined') {
    window.dispatchEvent(new CustomEvent<UiCustomizationConfig>(UI_CONFIG_EVENT, { detail: config }));
  }
}

export function subscribeUiConfig(listener: (config: UiCustomizationConfig) => void): () => void {
  if (typeof window === 'undefined') return () => {};
  const handler = (ev: Event) => {
    const customEv = ev as CustomEvent<UiCustomizationConfig>;
    if (customEv.detail) listener(customEv.detail);
  };
  window.addEventListener(UI_CONFIG_EVENT, handler);
  return () => window.removeEventListener(UI_CONFIG_EVENT, handler);
}

export type StudioTabId = 'theme' | 'layout' | 'stage' | 'backdrop' | 'updates';

export function requestUiCustomizerOpen(tab?: StudioTabId): void {
  if (typeof window === 'undefined') return;
  window.dispatchEvent(new CustomEvent(UI_CUSTOMIZER_OPEN_EVENT, { detail: { tab } }));
}

export function subscribeUiCustomizerOpen(
  listener: (tab?: StudioTabId) => void
): () => void {
  if (typeof window === 'undefined') return () => {};
  const handler = (ev: Event) => {
    const customEv = ev as CustomEvent<{ tab?: StudioTabId }>;
    listener(customEv.detail?.tab);
  };
  window.addEventListener(UI_CUSTOMIZER_OPEN_EVENT, handler);
  return () => window.removeEventListener(UI_CUSTOMIZER_OPEN_EVENT, handler);
}

export function dispatchExternalUiPatch(patch: UiCustomizationPatch): void {
  if (typeof window === 'undefined') return;
  window.dispatchEvent(new CustomEvent<UiCustomizationPatch>(UI_PATCH_EVENT, { detail: patch }));
}
