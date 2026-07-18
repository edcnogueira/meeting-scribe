'use client';

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { toast } from 'sonner';

export interface MeetingSpeakerInfo {
  cluster_label: string;
  display_name: string;
  speaker_id: string | null;
  score: number | null;
  is_self: boolean;
  has_identity: boolean;
}

export interface SpeakerIdentityInfo {
  id: string;
  name: string;
  sample_count: number;
  is_self: boolean;
  has_embedding: boolean;
}

interface DiarizationProgress {
  meeting_id: string;
  stage: string;
  progress_percentage: number;
  message: string;
}

/**
 * The design surfaces six diarization stages. The engine (D3/D5) only emits five
 * (registry matching happens silently between clustering and saving), so the
 * missing "Associando ao registro" step is inferred: it flips to done once the
 * `saving` event arrives. See task R3 handoff.
 */
export const DIARIZATION_STAGES = [
  'Decodificando áudio',
  'Segmentando fala',
  'Extraindo embeddings',
  'Agrupando vozes',
  'Associando ao registro',
  'Salvando',
] as const;

/** Map a real engine stage string to the index of the *current* design stage. */
function stageToIndex(stage: string): number {
  const s = (stage || '').toLowerCase();
  if (s.includes('complete') || s.includes('done')) return DIARIZATION_STAGES.length;
  if (s.includes('sav')) return 5; // saving → marks "Associando ao registro" done
  if (s.includes('cluster') || s.includes('agrup')) return 3;
  if (s.includes('embed')) return 2;
  if (s.includes('segment')) return 1;
  if (s.includes('decod') || s.includes('decoding') || s.includes('start')) return 0;
  return 0;
}

/** A degenerate clustering run yields far more clusters than a real meeting has. */
export const DEGENERATE_SPEAKER_THRESHOLD = 15;

export interface SpeakerRow extends MeetingSpeakerInfo {
  /** Voice-match confidence as a whole percentage, or null when unmatched. */
  confPercent: number | null;
  /** Registry sample count joined from the identity, when known. */
  sampleCount: number;
  /** Palette hue index (1..12) for the chip color. */
}

export interface UseDiarizationResult {
  speakers: MeetingSpeakerInfo[];
  identities: SpeakerIdentityInfo[];
  loading: boolean;
  diarizing: boolean;
  /** Current design stage index (0..6) while diarizing. */
  stageIndex: number;
  progressPercent: number;
  error: string | null;
  numRemote: string;
  setNumRemote: (v: string) => void;
  startDiarize: () => Promise<void>;
  cancelDiarize: () => Promise<void>;
  renameSpeaker: (clusterLabel: string, name: string) => Promise<void>;
  /** True when the run produced an implausible number of clusters. */
  isDegenerate: boolean;
  /** Number of samples in the registry for a given meeting speaker. */
  sampleCountFor: (speaker: MeetingSpeakerInfo) => number;
}

/**
 * Meeting-details diarization state (task R3): loads detected speakers and the
 * voice registry, drives (re-)diarization with live staged progress + cancel +
 * errors, and renames/enrolls speakers (feeding the D4 registry). Lifted out of
 * the old inline SpeakersPanel so the meeting screen can share speaker data
 * between the transcript meta bar and the speakers rail.
 */
export function useDiarization(
  meetingId: string,
  onRefetchTranscripts?: () => Promise<void>
): UseDiarizationResult {
  const [speakers, setSpeakers] = useState<MeetingSpeakerInfo[]>([]);
  const [identities, setIdentities] = useState<SpeakerIdentityInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [numRemote, setNumRemote] = useState<string>('');
  const [diarizing, setDiarizing] = useState(false);
  const [stageIndex, setStageIndex] = useState(0);
  const [progressPercent, setProgressPercent] = useState(0);
  const [error, setError] = useState<string | null>(null);

  const loadSpeakers = useCallback(async () => {
    try {
      const list = await invoke<MeetingSpeakerInfo[]>('api_get_meeting_speakers', { meetingId });
      setSpeakers(list);
    } catch (err) {
      console.error('Failed to load meeting speakers:', err);
    } finally {
      setLoading(false);
    }
  }, [meetingId]);

  const loadIdentities = useCallback(async () => {
    try {
      const list = await invoke<SpeakerIdentityInfo[]>('api_list_speaker_identities');
      setIdentities(list);
    } catch (err) {
      console.error('Failed to load speaker identities:', err);
    }
  }, []);

  useEffect(() => {
    setLoading(true);
    loadSpeakers();
    loadIdentities();
  }, [loadSpeakers, loadIdentities]);

  // Diarization progress / completion / error events, filtered by meeting id.
  const unlistenersRef = useRef<UnlistenFn[]>([]);
  useEffect(() => {
    let active = true;
    const setup = async () => {
      const un1 = await listen<DiarizationProgress>('diarization-progress', (e) => {
        if (!active || e.payload.meeting_id !== meetingId) return;
        setStageIndex(stageToIndex(e.payload.stage));
        setProgressPercent(e.payload.progress_percentage ?? 0);
      });
      const un2 = await listen<{ meeting_id: string }>('diarization-complete', async (e) => {
        if (!active || e.payload.meeting_id !== meetingId) return;
        setStageIndex(DIARIZATION_STAGES.length);
        setProgressPercent(100);
        setDiarizing(false);
        toast.success('Diarização concluída');
        await loadSpeakers();
        await loadIdentities();
        if (onRefetchTranscripts) await onRefetchTranscripts();
      });
      const un3 = await listen<{ meeting_id: string; error: string }>('diarization-error', (e) => {
        if (!active || e.payload.meeting_id !== meetingId) return;
        setDiarizing(false);
        const msg = e.payload.error || '';
        // A user-initiated cancel surfaces as an error event; treat it quietly.
        if (/cancel/i.test(msg)) {
          setError(null);
          toast.info('Diarização cancelada');
          return;
        }
        setError(msg);
        toast.error('Falha na diarização', { description: msg });
      });
      unlistenersRef.current = [un1, un2, un3];
    };
    setup();
    return () => {
      active = false;
      unlistenersRef.current.forEach((fn) => fn());
      unlistenersRef.current = [];
    };
  }, [meetingId, loadSpeakers, loadIdentities, onRefetchTranscripts]);

  const startDiarize = useCallback(async () => {
    setError(null);
    setStageIndex(0);
    setProgressPercent(0);
    setDiarizing(true);
    try {
      const parsed = parseInt(numRemote, 10);
      const numRemoteSpeakers = Number.isFinite(parsed) && parsed > 0 ? parsed : null;
      await invoke('api_diarize_meeting', { meetingId, numRemoteSpeakers });
    } catch (err) {
      setDiarizing(false);
      const msg = String(err);
      setError(msg);
      toast.error('Não foi possível iniciar a diarização', { description: msg });
    }
  }, [meetingId, numRemote]);

  const cancelDiarize = useCallback(async () => {
    try {
      await invoke('cancel_diarization_command');
    } catch (err) {
      console.warn('Cancel failed:', err);
    }
  }, []);

  const renameSpeaker = useCallback(
    async (clusterLabel: string, rawName: string) => {
      const name = rawName.trim();
      if (!name) return;
      try {
        await invoke('api_rename_meeting_speaker', { meetingId, clusterLabel, name });
        toast.success(`Falante renomeado para ${name}`);
        await loadSpeakers();
        await loadIdentities();
        if (onRefetchTranscripts) await onRefetchTranscripts();
      } catch (err) {
        toast.error('Falha ao renomear', { description: String(err) });
      }
    },
    [meetingId, loadSpeakers, loadIdentities, onRefetchTranscripts]
  );

  const sampleCountFor = useCallback(
    (speaker: MeetingSpeakerInfo): number => {
      if (!speaker.speaker_id) return 0;
      const identity = identities.find((i) => i.id === speaker.speaker_id);
      return identity?.sample_count ?? 0;
    },
    [identities]
  );

  const isDegenerate = speakers.length > DEGENERATE_SPEAKER_THRESHOLD;

  return useMemo(
    () => ({
      speakers,
      identities,
      loading,
      diarizing,
      stageIndex,
      progressPercent,
      error,
      numRemote,
      setNumRemote,
      startDiarize,
      cancelDiarize,
      renameSpeaker,
      isDegenerate,
      sampleCountFor,
    }),
    [
      speakers,
      identities,
      loading,
      diarizing,
      stageIndex,
      progressPercent,
      error,
      numRemote,
      startDiarize,
      cancelDiarize,
      renameSpeaker,
      isDegenerate,
      sampleCountFor,
    ]
  );
}
