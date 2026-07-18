'use client';

import { useEffect, useMemo, useState } from 'react';
import { useRouter } from 'next/navigation';
import { Summary, TranscriptSegmentData } from '@/types';
import { useShell } from '@/components/shell/ShellContext';
import { PanelIcon } from '@/components/shell/icons';
import Analytics from '@/lib/analytics';

import { TranscriptPanel } from '@/components/MeetingDetails/TranscriptPanel';
import { SpeakersPanel } from '@/components/MeetingDetails/SpeakersPanel';
import { SummaryPanel } from '@/components/MeetingDetails/SummaryPanel';
import { SpeakerMeta } from '@/components/MeetingDetails/MeetingTranscriptView';
import { summaryToMarkdown } from '@/components/MeetingDetails/summaryMarkdown';

import { useMeetingData } from '@/hooks/meeting-details/useMeetingData';
import { useSummaryGeneration } from '@/hooks/meeting-details/useSummaryGeneration';
import { useTemplates } from '@/hooks/meeting-details/useTemplates';
import { useCopyOperations } from '@/hooks/meeting-details/useCopyOperations';
import { useMeetingOperations } from '@/hooks/meeting-details/useMeetingOperations';
import { useDiarization } from '@/hooks/meeting-details/useDiarization';
import { useConfig } from '@/contexts/ConfigContext';

interface PageContentProps {
  meeting: any;
  summaryData: Summary | null;
  shouldAutoGenerate?: boolean;
  onAutoGenerateComplete?: () => void;
  onMeetingUpdated?: () => Promise<void>;
  onRefetchTranscripts?: () => Promise<void>;
  segments?: TranscriptSegmentData[];
  hasMore?: boolean;
  isLoadingMore?: boolean;
  totalCount?: number;
  loadedCount?: number;
  onLoadMore?: () => void;
}

const AI_CONTEXT_KEY = (id: string) => `ms-ai-context-${id}`;

function formatDuration(seconds: number): string {
  const total = Math.floor(seconds);
  if (total <= 0) return '0min';
  const h = Math.floor(total / 3600);
  const m = Math.round((total % 3600) / 60);
  if (h > 0) return `${h}h ${m}min`;
  return `${m}min`;
}

function formatRecordedAt(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return '';
  const date = d.toLocaleDateString('pt-BR');
  const time = d.toLocaleTimeString('pt-BR', { hour: '2-digit', minute: '2-digit' });
  return `gravada em ${date} às ${time}`;
}

function formatGeneratedAt(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return '';
  return `${d.toLocaleDateString('pt-BR')} ${d.toLocaleTimeString('pt-BR', { hour: '2-digit', minute: '2-digit' })}`;
}

function folderLabel(path?: string | null): string {
  if (!path) return 'Sem pasta';
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] || 'Sem pasta';
}

export default function PageContent({
  meeting,
  summaryData,
  shouldAutoGenerate = false,
  onAutoGenerateComplete,
  onMeetingUpdated,
  onRefetchTranscripts,
  segments = [],
  hasMore,
  isLoadingMore,
  totalCount,
  loadedCount,
  onLoadMore,
}: PageContentProps) {
  const router = useRouter();
  const { sidebarHidden, showSidebar } = useShell();
  const { modelConfig } = useConfig();

  const meetingData = useMeetingData({ meeting, summaryData, onMeetingUpdated });
  const templates = useTemplates();
  const diar = useDiarization(meeting.id, onRefetchTranscripts);

  const [aiContext, setAiContext] = useState('');

  // "Contexto para a IA" persists per meeting and feeds the generation prompt.
  useEffect(() => {
    try {
      setAiContext(window.localStorage.getItem(AI_CONTEXT_KEY(meeting.id)) ?? '');
    } catch {
      setAiContext('');
    }
  }, [meeting.id]);

  const handleAiContextChange = (v: string) => {
    setAiContext(v);
    try {
      window.localStorage.setItem(AI_CONTEXT_KEY(meeting.id), v);
    } catch {
      /* ignore */
    }
  };

  const summaryGeneration = useSummaryGeneration({
    meeting,
    transcripts: meetingData.transcripts,
    modelConfig,
    isModelConfigLoading: false,
    selectedTemplate: templates.selectedTemplate,
    onMeetingUpdated,
    updateMeetingTitle: meetingData.updateMeetingTitle,
    setAiSummary: meetingData.setAiSummary,
  });

  const copyOperations = useCopyOperations({
    meeting,
    transcripts: meetingData.transcripts,
    meetingTitle: meetingData.meetingTitle,
    aiSummary: meetingData.aiSummary,
    blockNoteSummaryRef: meetingData.blockNoteSummaryRef,
  });

  const meetingOperations = useMeetingOperations({ meeting });

  useEffect(() => {
    Analytics.trackPageView('meeting_details');
  }, []);

  // Auto-generate summary when navigated straight from recording.
  useEffect(() => {
    let cancelled = false;
    const run = async () => {
      if (shouldAutoGenerate && meetingData.transcripts.length > 0 && !cancelled) {
        await summaryGeneration.handleGenerateSummary(aiContext);
        if (onAutoGenerateComplete && !cancelled) onAutoGenerateComplete();
      }
    };
    run();
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [shouldAutoGenerate, meeting.id]);

  // ---- derived transcript meta ----
  const durationLabel = useMemo(() => {
    const last = segments.reduce((max, s) => Math.max(max, s.endTime ?? s.timestamp ?? 0), 0);
    return formatDuration(last);
  }, [segments]);

  const speakerCountLabel = useMemo(() => {
    if (diar.speakers.length === 0) return 'falantes não identificados';
    if (diar.isDegenerate) return `${diar.speakers.length} falantes (?)`;
    const n = diar.speakers.length;
    return `${n} ${n === 1 ? 'falante' : 'falantes'}`;
  }, [diar.speakers.length, diar.isDegenerate]);

  const recordedAtLabel = useMemo(() => formatRecordedAt(meeting.created_at), [meeting.created_at]);

  const speakerMeta = useMemo(() => {
    const map = new Map<string, SpeakerMeta>();
    for (const s of diar.speakers) {
      map.set(s.display_name, {
        score: s.score,
        samples: diar.sampleCountFor(s),
        isYou: s.is_self,
      });
    }
    return map;
  }, [diar.speakers, diar.sampleCountFor]);

  // ---- summary ----
  const markdown = useMemo(
    () => summaryToMarkdown(meetingData.aiSummary as any, meetingData.meetingTitle),
    [meetingData.aiSummary, meetingData.meetingTitle]
  );
  const hasSummary = !!meetingData.aiSummary && markdown.trim().length > 0;
  const generatedAtLabel = useMemo(
    () => formatGeneratedAt(meeting.updated_at || meeting.created_at),
    [meeting.updated_at, meeting.created_at]
  );

  const handleExportMarkdown = () => {
    const md = markdown;
    if (!md.trim()) return;
    const blob = new Blob([md], { type: 'text/markdown;charset=utf-8' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `${(meetingData.meetingTitle || 'resumo').replace(/[\\/:*?"<>|]/g, '-')}.md`;
    document.body.appendChild(a);
    a.click();
    a.remove();
    URL.revokeObjectURL(url);
  };

  // ---- header ----
  const pending = !hasSummary;
  const titleProvisional = pending;

  return (
    <>
      <header className="header">
        {sidebarHidden && (
          <button
            type="button"
            className="icon-btn js-show-sidebar"
            onClick={showSidebar}
            data-tip="Mostrar barra lateral"
            aria-label="Show sidebar"
          >
            <PanelIcon />
          </button>
        )}
        <div className="crumbs">
          <span>{folderLabel(meeting.folder_path)}</span>
          <span className="sep" aria-hidden="true">
            /
          </span>
          <span
            className="here"
            style={titleProvisional ? { fontStyle: 'italic', fontWeight: 500, color: 'var(--muted)' } : undefined}
          >
            {meetingData.meetingTitle}
          </span>
          {pending && (
            <span className="badge warn" data-tip="O título definitivo vem do H1 do primeiro resumo">
              aguardando resumo
            </span>
          )}
        </div>
        <div className="grow" />
        <button
          type="button"
          className="btn small ghost"
          data-tip="Abrir a pasta desta reunião no Finder"
          onClick={meetingOperations.handleOpenMeetingFolder}
        >
          Revelar no Finder
        </button>
      </header>

      <div className="zones">
        <TranscriptPanel
          segments={segments}
          speakerMeta={speakerMeta}
          durationLabel={durationLabel}
          speakerCountLabel={speakerCountLabel}
          recordedAtLabel={recordedAtLabel}
          onCopyTranscript={copyOperations.handleCopyTranscript}
          hasMore={hasMore}
          isLoadingMore={isLoadingMore}
          totalCount={totalCount}
          loadedCount={loadedCount}
          onLoadMore={onLoadMore}
        />

        <div className="rail">
          <SpeakersPanel diar={diar} />
          <SummaryPanel
            markdown={markdown}
            hasSummary={hasSummary}
            summaryStatus={summaryGeneration.summaryStatus}
            summaryError={summaryGeneration.summaryError}
            hasTranscripts={segments.length > 0 || meetingData.transcripts.length > 0}
            modelConfig={modelConfig}
            generatedAtLabel={generatedAtLabel}
            templates={templates.availableTemplates}
            selectedTemplate={templates.selectedTemplate}
            onTemplateSelect={templates.handleTemplateSelection}
            onGenerate={() => summaryGeneration.handleGenerateSummary(aiContext)}
            onRegenerate={summaryGeneration.handleRegenerateSummary}
            onCopySummary={copyOperations.handleCopySummary}
            onExportMarkdown={handleExportMarkdown}
            aiContext={aiContext}
            onAiContextChange={handleAiContextChange}
            onOpenSettings={() => router.push('/settings')}
          />
        </div>
      </div>
    </>
  );
}
