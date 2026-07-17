'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { toast } from 'sonner';
import { ChevronDown, ChevronRight, Users, RefreshCw, X, Check, Pencil } from 'lucide-react';
import { SpeakerChip } from '@/components/SpeakerChip';

interface MeetingSpeakerInfo {
  cluster_label: string;
  display_name: string;
  speaker_id: string | null;
  score: number | null;
  is_self: boolean;
  has_identity: boolean;
}

interface SpeakerIdentityInfo {
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

interface SpeakersPanelProps {
  meetingId: string;
  /** Reload transcripts after diarization / rename so labels refresh. */
  onRefetchTranscripts?: () => Promise<void>;
}

/**
 * Meeting-details speaker panel (task D5): lists detected speakers, renames /
 * enrolls them (feeding the D4 registry), takes an optional remote-participant
 * hint, and triggers (re-)diarization with live progress + cancel + errors.
 */
export function SpeakersPanel({ meetingId, onRefetchTranscripts }: SpeakersPanelProps) {
  const [collapsed, setCollapsed] = useState(false);
  const [speakers, setSpeakers] = useState<MeetingSpeakerInfo[]>([]);
  const [identities, setIdentities] = useState<SpeakerIdentityInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [numRemote, setNumRemote] = useState<string>('');
  const [editing, setEditing] = useState<string | null>(null);
  const [editValue, setEditValue] = useState('');

  const [diarizing, setDiarizing] = useState(false);
  const [progress, setProgress] = useState<DiarizationProgress | null>(null);
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

  // Diarization progress / completion / error events for this meeting.
  const unlistenersRef = useRef<UnlistenFn[]>([]);
  useEffect(() => {
    let active = true;
    const setup = async () => {
      const un1 = await listen<DiarizationProgress>('diarization-progress', (e) => {
        if (!active || e.payload.meeting_id !== meetingId) return;
        setProgress(e.payload);
      });
      const un2 = await listen<{ meeting_id: string }>('diarization-complete', async (e) => {
        if (!active || e.payload.meeting_id !== meetingId) return;
        setDiarizing(false);
        setProgress(null);
        toast.success('Diarization complete');
        await loadSpeakers();
        await loadIdentities();
        if (onRefetchTranscripts) await onRefetchTranscripts();
      });
      const un3 = await listen<{ meeting_id: string; error: string }>('diarization-error', (e) => {
        if (!active || e.payload.meeting_id !== meetingId) return;
        setDiarizing(false);
        setProgress(null);
        setError(e.payload.error);
        toast.error('Diarization failed', { description: e.payload.error });
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
    setDiarizing(true);
    setProgress({ meeting_id: meetingId, stage: 'starting', progress_percentage: 0, message: 'Starting...' });
    try {
      const parsed = parseInt(numRemote, 10);
      const numRemoteSpeakers = Number.isFinite(parsed) && parsed > 0 ? parsed : null;
      await invoke('api_diarize_meeting', { meetingId, numRemoteSpeakers });
    } catch (err) {
      setDiarizing(false);
      setProgress(null);
      const msg = String(err);
      setError(msg);
      toast.error('Could not start diarization', { description: msg });
    }
  }, [meetingId, numRemote]);

  const cancelDiarize = useCallback(async () => {
    try {
      await invoke('cancel_diarization_command');
      toast.info('Cancelling diarization...');
    } catch (err) {
      console.warn('Cancel failed:', err);
    }
  }, []);

  const beginEdit = (s: MeetingSpeakerInfo) => {
    setEditing(s.cluster_label);
    setEditValue(s.display_name);
  };

  const saveEdit = useCallback(
    async (clusterLabel: string) => {
      const name = editValue.trim();
      if (!name) {
        setEditing(null);
        return;
      }
      try {
        await invoke('api_rename_meeting_speaker', { meetingId, clusterLabel, name });
        toast.success(`Speaker renamed to ${name}`);
        setEditing(null);
        await loadSpeakers();
        await loadIdentities();
        if (onRefetchTranscripts) await onRefetchTranscripts();
      } catch (err) {
        toast.error('Rename failed', { description: String(err) });
      }
    },
    [meetingId, editValue, loadSpeakers, loadIdentities, onRefetchTranscripts]
  );

  const hasSpeakers = speakers.length > 0;

  return (
    <div className="border-b border-gray-200 bg-gray-50/60">
      <button
        type="button"
        onClick={() => setCollapsed((c) => !c)}
        className="w-full flex items-center justify-between px-4 py-2 text-sm font-medium text-gray-700 hover:bg-gray-100"
      >
        <span className="flex items-center gap-2">
          <Users className="w-4 h-4 text-gray-500" />
          Speakers
          {hasSpeakers && <span className="text-xs text-gray-400">({speakers.length})</span>}
        </span>
        {collapsed ? <ChevronRight className="w-4 h-4" /> : <ChevronDown className="w-4 h-4" />}
      </button>

      {!collapsed && (
        <div className="px-4 pb-3 space-y-3">
          {loading ? (
            <p className="text-xs text-gray-400">Loading speakers...</p>
          ) : !hasSpeakers ? (
            <p className="text-xs text-gray-500">
              This meeting has not been diarized yet. Run diarization to detect speakers.
            </p>
          ) : (
            <ul className="space-y-1.5">
              {speakers.map((s) => (
                <li key={s.cluster_label} className="flex items-center gap-2">
                  {editing === s.cluster_label ? (
                    <div className="flex items-center gap-1 flex-1 min-w-0">
                      <input
                        list="speaker-identity-names"
                        autoFocus
                        value={editValue}
                        onChange={(e) => setEditValue(e.target.value)}
                        onKeyDown={(e) => {
                          if (e.key === 'Enter') saveEdit(s.cluster_label);
                          if (e.key === 'Escape') setEditing(null);
                        }}
                        placeholder="Name"
                        className="flex-1 min-w-0 px-2 py-1 text-sm border border-gray-300 rounded focus:outline-none focus:ring-1 focus:ring-blue-500"
                      />
                      <button
                        type="button"
                        onClick={() => saveEdit(s.cluster_label)}
                        className="p-1 text-green-600 hover:bg-green-50 rounded"
                        title="Save"
                      >
                        <Check className="w-4 h-4" />
                      </button>
                      <button
                        type="button"
                        onClick={() => setEditing(null)}
                        className="p-1 text-gray-400 hover:bg-gray-100 rounded"
                        title="Cancel"
                      >
                        <X className="w-4 h-4" />
                      </button>
                    </div>
                  ) : (
                    <>
                      <SpeakerChip speaker={s.display_name} score={s.score ?? undefined} />
                      {s.is_self && <span className="text-[10px] text-gray-400">you</span>}
                      {s.score !== null && s.has_identity && (
                        <span className="text-[10px] text-gray-400">
                          {Math.round((s.score ?? 0) * 100)}%
                        </span>
                      )}
                      <button
                        type="button"
                        onClick={() => beginEdit(s)}
                        className="ml-auto p-1 text-gray-400 hover:text-gray-600 hover:bg-gray-100 rounded"
                        title="Rename / assign person"
                      >
                        <Pencil className="w-3.5 h-3.5" />
                      </button>
                    </>
                  )}
                </li>
              ))}
            </ul>
          )}

          {/* Autocomplete source: names already in the registry. */}
          <datalist id="speaker-identity-names">
            {identities.map((i) => (
              <option key={i.id} value={i.name} />
            ))}
          </datalist>

          {/* Diarization controls */}
          <div className="space-y-2 pt-1">
            <div className="flex items-center gap-2">
              <label className="text-xs text-gray-500 whitespace-nowrap" htmlFor="num-remote">
                Remote participants
              </label>
              <input
                id="num-remote"
                type="number"
                min={1}
                value={numRemote}
                onChange={(e) => setNumRemote(e.target.value)}
                placeholder="auto"
                disabled={diarizing}
                className="w-16 px-2 py-1 text-sm border border-gray-300 rounded focus:outline-none focus:ring-1 focus:ring-blue-500 disabled:opacity-50"
              />
            </div>

            {diarizing ? (
              <div className="space-y-1.5">
                <div className="flex items-center justify-between text-xs text-gray-600">
                  <span>{progress?.message ?? 'Diarizing...'}</span>
                  <button
                    type="button"
                    onClick={cancelDiarize}
                    className="text-red-500 hover:text-red-600"
                  >
                    Cancel
                  </button>
                </div>
                <div className="w-full h-1.5 bg-gray-200 rounded-full overflow-hidden">
                  <div
                    className="h-full bg-gradient-to-r from-blue-500 to-blue-600 rounded-full transition-all"
                    style={{ width: `${progress?.progress_percentage ?? 0}%` }}
                  />
                </div>
              </div>
            ) : (
              <button
                type="button"
                onClick={startDiarize}
                className="inline-flex items-center gap-1.5 px-3 py-1.5 text-sm rounded-md bg-blue-600 text-white hover:bg-blue-700"
              >
                <RefreshCw className="w-3.5 h-3.5" />
                {hasSpeakers ? 'Re-diarize' : 'Diarize'}
              </button>
            )}

            {error && !diarizing && (
              <p className="text-xs text-red-500">Error: {error}</p>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
