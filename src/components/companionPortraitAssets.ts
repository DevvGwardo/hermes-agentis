import type { PortraitSourceField } from '../utils/asciiPortraitEngine';
import type { BrailleShapeKind } from '../utils/brailleShapesEngine';
import { buildCompanionPortraitSourceFromRgba } from '../utils/companionPortraitProcessor';

import cyberBobImg from '../assets/images/char_v2_cyber_bob_1795316021855.jpg';
import twintailsImg from '../assets/images/char_v2_twintails_1795316023562.jpg';
import nekoImg from '../assets/images/char_v2_neko_1795316024406.jpg';
import kitsuneImg from '../assets/images/char_v2_kitsune_1795316022069.jpg';
import usagiImg from '../assets/images/char_v2_usagi_1795316024186.jpg';
import mechaImg from '../assets/images/char_v2_mecha_1795316022949.jpg';
import yumeImg from '../assets/images/char_v2_yume_1795316022834.jpg';
import seraphImg from '../assets/images/char_v2_seraph_1795316124290.jpg';
import wolfcutImg from '../assets/images/char_v2_wolfcut_1795316023424.jpg';

const COMPANION_PORTRAIT_URLS: Record<BrailleShapeKind, string> = {
  clover: cyberBobImg,
  star: twintailsImg,
  ghost: nekoImg,
  heart: kitsuneImg,
  cloud: usagiImg,
  triangle: mechaImg,
  pebble: yumeImg,
  trapezoid: seraphImg,
  blob: wolfcutImg,
};

const fieldCache = new Map<BrailleShapeKind, PortraitSourceField>();
const inflightLoads = new Map<BrailleShapeKind, Promise<PortraitSourceField | null>>();

export function getCachedCompanionPortrait(kind: BrailleShapeKind): PortraitSourceField | null {
  return fieldCache.get(kind) ?? null;
}

/**
 * Asynchronously decodes and caches a 224x224 monochrome halftone source field for the
 * given Canvas companion character kind. Safe to call repeatedly across many cards/nodes.
 */
export function loadCompanionPortrait(kind: BrailleShapeKind): Promise<PortraitSourceField | null> {
  const existing = fieldCache.get(kind);
  if (existing) return Promise.resolve(existing);

  const pending = inflightLoads.get(kind);
  if (pending) return pending;

  if (typeof document === 'undefined' || typeof Image === 'undefined') {
    return Promise.resolve(null);
  }

  const url = COMPANION_PORTRAIT_URLS[kind];
  const promise = new Promise<PortraitSourceField | null>((resolve) => {
    const img = new Image();
    img.crossOrigin = 'anonymous';
    img.onload = () => {
      try {
        const size = 224;
        const offscreen = document.createElement('canvas');
        offscreen.width = size;
        offscreen.height = size;
        const octx =
          offscreen.getContext('2d', { willReadFrequently: true }) || offscreen.getContext('2d');
        if (!octx) {
          resolve(null);
          return;
        }
        octx.fillStyle = '#FFFFFF';
        octx.fillRect(0, 0, size, size);
        octx.drawImage(img, 0, 0, size, size);
        const rgba = octx.getImageData(0, 0, size, size).data;
        const field = buildCompanionPortraitSourceFromRgba(rgba, size, size, kind);
        fieldCache.set(kind, field);
        resolve(field);
      } catch {
        resolve(null);
      }
    };
    img.onerror = () => {
      inflightLoads.delete(kind);
      resolve(null);
    };
    img.src = url;
  });

  inflightLoads.set(kind, promise);
  return promise;
}
