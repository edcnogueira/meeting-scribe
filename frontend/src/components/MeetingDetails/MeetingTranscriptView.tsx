'use client';

import { useEffect, useMemo, useRef } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import { TranscriptSegmentData } from '@/types';
import { getSpeakerColorIndex } from '@/lib/speakerColors';

export interface SpeakerMeta {
  score: number | null;
  samples: number;
  isYou: boolean;
}

interface MeetingTranscriptViewProps {
  segments: TranscriptSegmentData[];
  /** name → confidence/samples/self, for colored chips + confidence tooltips. */
  speakerMeta?: Map<string, SpeakerMeta>;
  hasMore?: boolean;
  isLoadingMore?: boolean;
  totalCount?: number;
  loadedCount?: number;
  onLoadMore?: () => void;
}

const VIRTUALIZATION_THRESHOLD = 30;

function fmt(t: number | undefined): string {
  const total = Math.floor(t ?? 0);
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${pad(h)}:${pad(m)}:${pad(s)}`;
}

function confTip(meta: SpeakerMeta | undefined): string | undefined {
  if (!meta) return undefined;
  if (meta.samples > 0 && meta.score !== null) {
    return `Correspondência de voz: ${Math.round(meta.score * 100)}%\n${meta.samples} amostras no registro`;
  }
  return 'Voz nova — sem correspondência no registro.\nRenomeie para começar a ensiná-la.';
}

function Segment({
  segment,
  speakerMeta,
}: {
  segment: TranscriptSegmentData;
  speakerMeta?: Map<string, SpeakerMeta>;
}) {
  const speaker = segment.speaker?.trim();
  const meta = speaker ? speakerMeta?.get(speaker) : undefined;
  const tip = confTip(meta);

  return (
    <div className={`seg${speaker ? '' : ' no-speaker'}`}>
      <span className="ts">{fmt(segment.timestamp)}</span>
      {speaker && (
        <span className="s-head">
          <span className="spk" data-c={getSpeakerColorIndex(speaker)} data-tip={tip}>
            {speaker}
            {meta?.isYou && <span className="you">VOCÊ</span>}
          </span>
        </span>
      )}
      <p className="s-text">{segment.text}</p>
    </div>
  );
}

/**
 * Virtualized transcript for the redesigned meeting screen (task R3): mono
 * timestamps + colored speaker chips (`.seg` / `.spk`) with confidence tooltips,
 * the `no-speaker` variant for undiarized meetings, and the existing infinite
 * scroll for long meetings preserved.
 */
export function MeetingTranscriptView({
  segments,
  speakerMeta,
  hasMore = false,
  isLoadingMore = false,
  totalCount = 0,
  loadedCount = 0,
  onLoadMore,
}: MeetingTranscriptViewProps) {
  const scrollRef = useRef<HTMLDivElement>(null);
  const loadMoreTriggerRef = useRef<HTMLDivElement>(null);

  const useVirtualization = segments.length >= VIRTUALIZATION_THRESHOLD;

  const virtualizer = useVirtualizer({
    count: segments.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => 72,
    overscan: 12,
  });

  // Infinite scroll: load more as the sentinel nears the viewport.
  useEffect(() => {
    if (!onLoadMore || !hasMore || isLoadingMore || segments.length === 0) return;
    const el = loadMoreTriggerRef.current;
    if (!el) return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries[0].isIntersecting && hasMore && !isLoadingMore) onLoadMore();
      },
      { root: scrollRef.current, rootMargin: '150px', threshold: 0 }
    );
    observer.observe(el);
    return () => observer.disconnect();
  }, [hasMore, isLoadingMore, onLoadMore, segments.length]);

  const loadMoreFooter = useMemo(() => {
    if (!hasMore && !isLoadingMore) return null;
    return (
      <div ref={loadMoreTriggerRef} style={{ padding: '14px 0', textAlign: 'center' }} className="faint">
        {isLoadingMore ? (
          <span style={{ display: 'inline-flex', alignItems: 'center', gap: 8 }}>
            <span className="spinner" /> Carregando mais…
          </span>
        ) : totalCount > 0 ? (
          <span style={{ fontSize: 12 }}>
            Mostrando {loadedCount} de {totalCount} trechos
          </span>
        ) : null}
      </div>
    );
  }, [hasMore, isLoadingMore, totalCount, loadedCount]);

  if (segments.length === 0) {
    return (
      <div className="panel-body transcript-body">
        <p className="faint" style={{ fontSize: 12.5 }}>
          Nenhum trecho de transcrição ainda.
        </p>
      </div>
    );
  }

  if (!useVirtualization) {
    return (
      <div ref={scrollRef} className="panel-body transcript-body">
        {segments.map((seg) => (
          <Segment key={seg.id} segment={seg} speakerMeta={speakerMeta} />
        ))}
        {loadMoreFooter}
      </div>
    );
  }

  return (
    <div ref={scrollRef} className="panel-body transcript-body">
      <div style={{ height: virtualizer.getTotalSize(), width: '100%', position: 'relative' }}>
        {virtualizer.getVirtualItems().map((row) => {
          const seg = segments[row.index];
          return (
            <div
              key={seg.id}
              data-index={row.index}
              ref={virtualizer.measureElement}
              style={{ position: 'absolute', top: 0, left: 0, width: '100%', transform: `translateY(${row.start}px)` }}
            >
              <Segment segment={seg} speakerMeta={speakerMeta} />
            </div>
          );
        })}
      </div>
      {loadMoreFooter}
    </div>
  );
}
