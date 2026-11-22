import type { PortraitSourceField } from '../utils/asciiPortraitEngine';
import type { BrailleShapeKind } from '../utils/brailleShapesEngine';
import { buildCompanionPortraitSourceFromRgba } from '../utils/companionPortraitProcessor';

import cyberBobImg from '../assets/images/char_cyber_bob_1795309995550.jpg';
import twintailsImg from '../assets/images/char_twintails_1795309991142.jpg';
import nekoImg from '../assets/images/char_neko_1795309987928.jpg';
import kitsuneImg from '../assets/images/char_kitsune_1795309990760.jpg';
import usagiImg from '../assets/images/char_usagi_1795309993007.jpg';
import mechaImg from '../assets/images/char_mecha_1795309989704.jpg';
import yumeImg from '../assets/images/char_yume_1795309992770.jpg';
import seraphImg from '../assets/images/char_seraph_1795309991362.jpg';
import wolfcutImg from '../assets/images/char_wolfcut_1795309990718.jpg';

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
