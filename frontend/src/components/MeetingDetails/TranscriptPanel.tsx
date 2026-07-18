'use client';

import { useState } from 'react';
import { TranscriptSegmentData } from '@/types';
import { MeetingTranscriptView, SpeakerMeta } from './MeetingTranscriptView';

interface TranscriptPanelProps {
  segments: TranscriptSegmentData[];
  speakerMeta?: Map<string, SpeakerMeta>;
  /** e.g. "1h 12min" */
  durationLabel: string;
  /** e.g. "4 falantes" | "falantes não identificados" | "190 falantes (?)" */
  speakerCountLabel: string;
  /** e.g. "gravada em 17/07/2026 às 09:30" */
  recordedAtLabel: string;
  onCopyTranscript: () => Promise<void> | void;
  hasMore?: boolean;
  isLoadingMore?: boolean;
  totalCount?: number;
  loadedCount?: number;
  onLoadMore?: () => void;
}

/**
 * Redesigned meeting transcript panel (task R3): a fixed meta bar (duration,
 * speaker count, recorded-at, copy button with "Copiado ✓" feedback) over the
 * virtualized `.seg` transcript.
 */
export function TranscriptPanel({
  segments,
  speakerMeta,
  durationLabel,
  speakerCountLabel,
  recordedAtLabel,
  onCopyTranscript,
  hasMore,
  isLoadingMore,
  totalCount,
  loadedCount,
  onLoadMore,
}: TranscriptPanelProps) {
  const [copied, setCopied] = useState(false);

  const handleCopy = async () => {
    await onCopyTranscript();
    setCopied(true);
    setTimeout(() => setCopied(false), 1600);
  };

  return (
    <section className="panel" aria-label="Transcrição">
      <div className="t-meta">
        <span className="mono">{durationLabel}</span>
        <span>·</span>
        <span>{speakerCountLabel}</span>
        <span>·</span>
        <span>{recordedAtLabel}</span>
        <span className="grow" />
        <button type="button" className="btn small ghost" onClick={handleCopy}>
          {copied ? 'Copiado ✓' : 'Copiar transcrição'}
        </button>
      </div>
      <MeetingTranscriptView
        segments={segments}
        speakerMeta={speakerMeta}
        hasMore={hasMore}
        isLoadingMore={isLoadingMore}
        totalCount={totalCount}
        loadedCount={loadedCount}
        onLoadMore={onLoadMore}
      />
    </section>
  );
}
