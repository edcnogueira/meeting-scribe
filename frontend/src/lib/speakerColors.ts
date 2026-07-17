/**
 * Deterministic per-speaker colors (task D5).
 *
 * Maps a speaker label ("Eu", "Speaker 2", or a person's name) to a stable
 * color so the same speaker always renders with the same hue across a meeting
 * and across sessions. Pure string hash -> HSL, no persistence needed.
 */

export interface SpeakerColor {
  /** Solid color for the dot / accent (text-on-white safe). */
  color: string;
  /** Tinted background for the chip. */
  background: string;
  /** Border for the chip. */
  border: string;
}

// FNV-1a style hash: small, deterministic, well-distributed for short strings.
function hashString(input: string): number {
  let hash = 0x811c9dc5;
  for (let i = 0; i < input.length; i++) {
    hash ^= input.charCodeAt(i);
    hash = Math.imul(hash, 0x01000193);
  }
  // Force unsigned 32-bit.
  return hash >>> 0;
}

/**
 * Resolve a stable color set for a speaker label. Trims/normalizes so
 * "Speaker 1" and " Speaker 1 " share a color.
 */
export function getSpeakerColor(label: string): SpeakerColor {
  const key = (label ?? '').trim();
  const hue = hashString(key) % 360;
  return {
    color: `hsl(${hue}, 65%, 42%)`,
    background: `hsl(${hue}, 70%, 95%)`,
    border: `hsl(${hue}, 60%, 82%)`,
  };
}

/**
 * Number of stable hues in the redesign speaker palette (see the `data-c`
 * definitions in `src/styles/design-system.css`).
 */
export const SPEAKER_PALETTE_SIZE = 12;

/**
 * Map a speaker label to a stable palette index in the range 1..12 (task R1).
 *
 * The same label always resolves to the same slot across a meeting and across
 * sessions, and the value is meant to be applied as the `data-c` attribute so
 * the token-driven `.spk` styles pick the corresponding hue in either theme.
 */
export function getSpeakerColorIndex(label: string): number {
  const key = (label ?? '').trim();
  if (!key) return 1;
  return (hashString(key) % SPEAKER_PALETTE_SIZE) + 1;
}
