import type { ExpressionMood, EyeState, MouthViseme } from '../types/hermes';

export const ASCII_WIDTH = 46;

export interface AsciiRenderOptions {
  mood: ExpressionMood;
  viseme: MouthViseme;
  eyeState: EyeState;
  energy: number; // 0..1 audio amplitude
  tick: number;   // animation frame counter
}

const EYE_PATTERNS: Record<EyeState, { left: string; right: string; browL: string; browR: string }> = {
  open:  { browL: '___', browR: '___', left: '(◉)', right: '(◉)' },
  blink: { browL: '___', browR: '___', left: '═══', right: '═══' },
  half:  { browL: '\\__', browR: '__/', left: '(─)', right: '(─)' },
  wide:  { browL: '/¯¯', browR: '¯¯\\', left: '(◎)', right: '(◎)' },
  happy: { browL: '⌒ ', browR: ' ⌒', left: '^ ^', right: '^ ^' },
  side:  { browL: '___', browR: '¯¯¯', left: '(◉ ', right: ' ◉)' },
};

const MOUTH_PATTERNS: Record<MouthViseme, string> = {
  closed: '  ───  ',
  rest:   '  ─·─  ',
  smile:  '  ╰─╯  ',
  mbp:    '  ═══  ',
  fv:     '  ▀─▀  ',
  lth:    '  ╰╦╯  ',
  small:  '  ╰o╯  ',
  mid:    '  (◯)  ',
  round:  '  (O)  ',
  ee:     ' ╰───╯ ',
  wide:   ' ╰═══╯ ',
};

const TEXT_VISEME_MAP: Array<[RegExp, MouthViseme]> = [
  [/^(ph)/i, 'fv'],
  [/^(wh|oo|ou|oh|ow|oa)/i, 'round'],
  [/^(ch|sh|ee|ea|ie|ey)/i, 'ee'],
  [/^(th)/i, 'lth'],
  [/^(ah|ha|aa|ar|au|aw)/i, 'wide'],
  [/^(ai|ay)/i, 'mid'],
  [/^[mbp]/i, 'mbp'],
  [/^[fv]/i, 'fv'],
  [/^[ldtn]/i, 'lth'],
  [/^[iyj]/i, 'ee'],
  [/^[owq]/i, 'round'],
  [/^[a]/i, 'wide'],
  [/^(uh|eh|er|e|u|r|s|z|k|g|c|h|x)/i, 'small'],
];

let lastTimelineText = '';
let lastTimelineResult: MouthViseme[] = ['small', 'mid', 'wide', 'round'];

/**
 * Extracts a compact phoneme-viseme sequence from spoken text so talking lip-sync
 * hits bilabial (M/B/P), labiodental (F/V), lingual (L/Th), E/I, O/Oo, and A/Ha shapes.
 */
export function textToVisemeTimeline(text: string): MouthViseme[] {
  const cleaned = text.trim();
  if (!cleaned) return ['small', 'mid', 'wide', 'round'];
  if (cleaned === lastTimelineText) return lastTimelineResult;

  const out: MouthViseme[] = [];
  let i = 0;
  while (i < cleaned.length) {
    const ch = cleaned[i];
    if (/[\s,.;:!?—-]/.test(ch)) {
      if (out.length > 0 && out[out.length - 1] !== 'rest') {
        out.push('rest');
      }
      i += 1;
      continue;
    }
    const slice = cleaned.slice(i, i + 2);
    let matched: MouthViseme | null = null;
    for (const [re, viseme] of TEXT_VISEME_MAP) {
      const m = slice.match(re);
      if (m) {
        matched = viseme;
        i += m[0].length;
        break;
      }
    }
    if (!matched) {
      i += 1;
      continue;
    }
    if (out[out.length - 1] !== matched) {
      out.push(matched);
    }
  }

  const res: MouthViseme[] = out.length > 0 ? out : ['small', 'mid', 'wide', 'round'];
  lastTimelineText = cleaned;
  lastTimelineResult = res;
  return res;
}

/**
 * Maps instantaneous audio RMS amplitude (0..1), current mood, frame tick, and optional
 * FFT bands / spoken text into a realistic animated MouthViseme for real-time lip synchronization.
 */
export function amplitudeToViseme(
  amplitude: number,
  mood: ExpressionMood,
  tick: number,
  isSpeaking: boolean,
  bands?: number[],
  spokenText?: string
): MouthViseme {
  if (!isSpeaking) {
    if (mood === 'happy') return 'smile';
    if (mood === 'alert') return 'small';
    if (mood === 'listening') return 'rest';
    return 'closed';
  }

  const clamped = Math.max(0, Math.min(1, amplitude));
  const syllableWave = Math.sin(tick * 0.85);
  const syllableMod = (syllableWave + 1) * 0.11;
  const level = Math.min(1, clamped + syllableMod);

  if (level < 0.07) return 'closed';
  if (level < 0.22) return 'small';

  // When live FFT bands or spoken text are supplied, drive syllable-paced 10-shape phoneme articulation
  if (spokenText && spokenText.trim().length > 0) {
    const timeline = textToVisemeTimeline(spokenText);
    const syllableStep = Math.floor(Math.max(0, tick) / 3);
    const phoneme = timeline[syllableStep % timeline.length];

    // Keep bilabial M/B/P closures crisp on syllable onset, then release into voiced opening
    if (phoneme === 'mbp' && (tick % 3 === 0 || clamped < 0.45)) {
      return 'mbp';
    }
    if (phoneme === 'mbp') {
      return clamped > 0.68 ? 'mid' : 'small';
    }

    // Modulate phoneme by instantaneous audio energy so quiet moments tighten and peaks open
    if (clamped < 0.14 && phoneme !== 'fv') {
      return syllableStep % 3 === 0 ? 'rest' : 'small';
    }
    if (bands && bands.length >= 10) {
      const low = ((bands[1] ?? 0) + (bands[2] ?? 0) + (bands[3] ?? 0)) / 3;
      const mid = ((bands[4] ?? 0) + (bands[5] ?? 0) + (bands[6] ?? 0)) / 3;
      const high = ((bands[7] ?? 0) + (bands[8] ?? 0) + (bands[9] ?? 0)) / 3;

      // Strong high-frequency fricative/sibilant energy boosts F/V, L/Th, or Wide/Ee
      if (high > mid * 1.18 && high > 0.28) {
        if (phoneme === 'fv' || phoneme === 'lth' || phoneme === 'ee') return phoneme;
        return syllableStep % 2 === 0 ? 'fv' : 'ee';
      }
      // Strong low vowel resonance biases Rounded O/Oo or Wide Open A/Ha
      if (low > high * 1.25 && clamped > 0.52) {
        return phoneme === 'round' ? 'round' : 'wide';
      }
    }
    if (clamped > 0.78 && (phoneme === 'small' || phoneme === 'rest')) {
      return 'wide';
    }
    return phoneme;
  }

  if (bands && bands.length >= 10) {
    const low = ((bands[1] ?? 0) + (bands[2] ?? 0)) * 0.5;
    const high = ((bands[7] ?? 0) + (bands[8] ?? 0)) * 0.5;
    if (clamped < 0.28 && tick % 5 === 0) return 'mbp';
    if (high > low * 1.2 && high > 0.3) {
      return tick % 3 === 0 ? 'fv' : tick % 3 === 1 ? 'lth' : 'ee';
    }
  }

  if (level < 0.44) {
    if (tick % 4 === 0) return 'round';
    if (tick % 3 === 0) return 'small';
    return 'mid';
  }
  if (level < 0.72) {
    if (tick % 5 === 0) return 'round';
    if (tick % 3 === 1) return 'wide';
    return 'mid';
  }
  return tick % 6 === 0 ? 'round' : 'wide';
}

/**
 * Resolves the character's eye state from mood, speaking state, and animation tick.
 */
export function resolveEyeState(mood: ExpressionMood, tick: number, isSpeaking: boolean): EyeState {
  // 4-step blinking rhythm: open -> half -> blink -> half -> open
  const blinkCycle = tick % 64;
  if (blinkCycle === 60 || blinkCycle === 63) {
    return 'half';
  }
  if (blinkCycle === 61 || blinkCycle === 62) {
    return 'blink';
  }

  if (mood === 'happy') return 'happy';
  if (mood === 'alert') return 'wide';
  if (mood === 'thinking') return tick % 24 < 12 ? 'side' : 'half';
  if (mood === 'focused') return 'half';
  if (mood === 'listening' && tick % 18 === 0) return 'wide';
  if (isSpeaking && tick % 29 === 0) return 'happy';
  return 'open';
}

function padCenter(content: string, width: number = ASCII_WIDTH): string {
  if (content.length === width) return content;
  if (content.length > width) return content.slice(0, width);
  const totalPad = width - content.length;
  const left = Math.floor(totalPad / 2);
  const right = totalPad - left;
  return ' '.repeat(left) + content + ' '.repeat(right);
}

/**
 * Renders the Hermes Agent "Nous Girl" cyber-messenger ASCII character portrait.
 * All lines are strictly normalized to ASCII_WIDTH columns to guarantee zero jitter.
 */
export function renderNousGirlAscii(opts: AsciiRenderOptions): string[] {
  const { mood, viseme, eyeState, energy, tick } = opts;
  const eyes = EYE_PATTERNS[eyeState];
  const mouth = MOUTH_PATTERNS[viseme];

  const activeEnergy = Math.max(0, Math.min(1, energy));
  const phase = tick % 4;
  const sparkA = activeEnergy > 0.35 ? (phase % 2 === 0 ? '✦' : '·') : phase === 0 ? '·' : ' ';
  const sparkB = activeEnergy > 0.55 ? (phase % 2 === 1 ? '✧' : '*') : ' ';
  const wingTipL = activeEnergy > 0.25 && phase < 2 ? '≪' : '«';
  const wingTipR = activeEnergy > 0.25 && phase < 2 ? '≫' : '»';

  const cheekL = mood === 'happy' ? '〃' : mood === 'listening' ? '≈' : ' ';
  const cheekR = mood === 'happy' ? '〃' : mood === 'listening' ? '≈' : ' ';
  const visorMark = mood === 'thinking' ? '◈' : mood === 'alert' ? '!' : '◊';
  const chinTag = mood === 'speaking' ? ' ·───· ' : '       ';
  const pulseBar =
    activeEnergy > 0.65
      ? '▓▓▓'
      : activeEnergy > 0.35
      ? '▒▒▒'
      : activeEnergy > 0.12
      ? '░░░'
      : '───';

  // Symmetrically aligned winged messenger crest, face, eyes, nose, viseme mouth, & collar
  const rawLines = [
    `      ${sparkA}       ▄▄████████▄▄       ${sparkB}      `,
    `    ${wingTipL}══╗   ▄██▀▀      ▀▀██▄   ╔══${wingTipR}    `,
    `   /███╗ ▄██▀  ▄██${visorMark}██▄  ▀██▄ ╔███\\   `,
    `  /████╚▄█▀  ▄██████████▄  ▀█▄╝████\\  `,
    `  \\█████▀   ███▀  ██  ▀███   ▀█████/  `,
    `   \\██▀    ██▀  ${eyes.browL}    ${eyes.browR}  ▀██    ▀██/   `,
    `    ▀│    ██▌   ${eyes.left}    ${eyes.right}   ▐██    │▀    `,
    `     │    ██  ${cheekL}     ╷╷     ${cheekR}  ██    │     `,
    `   ${sparkB} │    ██▄    ${mouth}    ▄██    │ ${sparkA}   `,
    `     │     ▀██▄  ${chinTag}  ▄██▀     │     `,
    `     ╰──    ▀███▄  ──  ▄███▀    ──╯     `,
    `             ▀███▄▄▄▄▄▄███▀             `,
    `           ▄█▀  ║${pulseBar}║  ▀█▄           `,
    `         ▄██▀  ▄╚════╝▄  ▀██▄         `,
    `       ▄██▀  ▄█▀  ${visorMark}   ▀█▄  ▀██▄       `,
    `      ███   ██   /  \\   ██   ███      `,
    `     ▐██▌   ██  /    \\  ██   ▐██▌     `,
    `     ███    ▀█▄/──────\\▄█▀    ███     `,
  ];

  return rawLines.map((line) => padCenter(line, ASCII_WIDTH));
}
