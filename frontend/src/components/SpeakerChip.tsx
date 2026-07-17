'use client';

import { getSpeakerColor } from '@/lib/speakerColors';

interface SpeakerChipProps {
  speaker: string;
  /** Optional match score (0..1) shown in the tooltip when available. */
  score?: number;
  className?: string;
}

/**
 * Small colored label identifying the speaker of a transcript segment (D5).
 * Color is deterministic per speaker name. Renders nothing when there is no
 * speaker, so undiarized segments look exactly as before.
 */
export function SpeakerChip({ speaker, score, className }: SpeakerChipProps) {
  const name = (speaker ?? '').trim();
  if (!name) return null;

  const c = getSpeakerColor(name);
  const title =
    score !== undefined && Number.isFinite(score)
      ? `${name} · match ${Math.round(score * 100)}%`
      : name;

  return (
    <span
      title={title}
      className={`inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-xs font-medium leading-none flex-shrink-0 ${className ?? ''}`}
      style={{ color: c.color, backgroundColor: c.background, border: `1px solid ${c.border}` }}
    >
      <span
        className="inline-block w-1.5 h-1.5 rounded-full"
        style={{ backgroundColor: c.color }}
        aria-hidden="true"
      />
      {name}
    </span>
  );
}
