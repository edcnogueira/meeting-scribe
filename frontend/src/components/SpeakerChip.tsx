'use client';

import { getSpeakerColorIndex } from '@/lib/speakerColors';

interface SpeakerChipProps {
  speaker: string;
  /** Optional match score (0..1) shown in the tooltip when available. */
  score?: number;
  /** Marks the current user — renders the "VOCÊ" badge from the design. */
  isYou?: boolean;
  /** Renders the dashed "unknown speaker" state regardless of the label. */
  unknown?: boolean;
  className?: string;
}

/**
 * Small colored label identifying the speaker of a transcript segment (D5).
 *
 * Redesign (task R1): the color is one of 12 stable palette hues resolved from
 * the token-driven `.spk` styles via a deterministic `data-c` index, so the same
 * speaker always renders with the same hue in both light and dark themes.
 * Renders nothing when there is no speaker, so undiarized segments look
 * exactly as before.
 */
export function SpeakerChip({ speaker, score, isYou, unknown, className }: SpeakerChipProps) {
  const name = (speaker ?? '').trim();
  if (!name) return null;

  const hasScore = score !== undefined && Number.isFinite(score);
  // Multiline confidence tooltip via the design's `data-tip` pattern.
  const tip = hasScore
    ? `Correspondência de voz: ${Math.round((score as number) * 100)}%`
    : undefined;

  if (unknown) {
    return (
      <span className={`spk unknown ${className ?? ''}`} data-tip={tip} title={hasScore ? tip : undefined}>
        {name}
      </span>
    );
  }

  const c = getSpeakerColorIndex(name);

  return (
    <span
      className={`spk ${className ?? ''}`}
      data-c={c}
      data-tip={tip}
      title={hasScore ? tip : undefined}
    >
      {name}
      {isYou && <span className="you">VOCÊ</span>}
    </span>
  );
}
