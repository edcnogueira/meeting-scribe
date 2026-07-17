'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { toast } from 'sonner';
import { Download, CheckCircle2 } from 'lucide-react';
import { Switch } from './ui/switch';
import { SpeakerIdentityManager } from './SpeakerIdentityManager';
import {
  DiarizationSettings as DiarizationSettingsType,
  DEFAULT_DIARIZATION_SETTINGS,
  loadDiarizationSettings,
  saveDiarizationSettings,
  syncDiarizationSettingsToBackend,
} from '@/lib/diarizationSettings';

interface ToggleRowProps {
  title: string;
  description: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
  disabled?: boolean;
}

function ToggleRow({ title, description, checked, onChange, disabled }: ToggleRowProps) {
  return (
    <div className="flex items-center justify-between gap-4">
      <div>
        <h4 className="text-sm font-medium text-gray-900">{title}</h4>
        <p className="text-xs text-gray-500">{description}</p>
      </div>
      <Switch checked={checked} onCheckedChange={onChange} disabled={disabled} />
    </div>
  );
}

/**
 * Speaker-diarization settings (task D5): feature toggles (persisted + pushed to
 * the backend), diarization-model download/status, and the enrolled-people
 * registry.
 */
export function DiarizationSettings() {
  const [settings, setSettings] = useState<DiarizationSettingsType>(DEFAULT_DIARIZATION_SETTINGS);

  // Diarization model state.
  const [modelAvailable, setModelAvailable] = useState<boolean | null>(null);
  const [downloading, setDownloading] = useState(false);
  const [downloadProgress, setDownloadProgress] = useState(0);

  useEffect(() => {
    setSettings(loadDiarizationSettings());
    // Ensure the backend reflects the persisted values on mount.
    syncDiarizationSettingsToBackend();
  }, []);

  const update = useCallback(
    (patch: Partial<DiarizationSettingsType>) => {
      setSettings((prev) => {
        const next = { ...prev, ...patch };
        saveDiarizationSettings(next);
        syncDiarizationSettingsToBackend(next);
        return next;
      });
    },
    []
  );

  const refreshModelStatus = useCallback(async () => {
    try {
      const available = await invoke<boolean>('diarization_is_model_available');
      setModelAvailable(available);
    } catch (err) {
      console.error('Failed to check diarization model status:', err);
      setModelAvailable(false);
    }
  }, []);

  useEffect(() => {
    refreshModelStatus();
  }, [refreshModelStatus]);

  // Download progress / completion / error events.
  const unlistenersRef = useRef<UnlistenFn[]>([]);
  useEffect(() => {
    let active = true;
    const setup = async () => {
      const un1 = await listen<{ progress: number }>(
        'diarization-model-download-progress',
        (e) => {
          if (!active) return;
          setDownloading(true);
          setDownloadProgress(Math.round(e.payload.progress ?? 0));
        }
      );
      const un2 = await listen('diarization-model-download-complete', async () => {
        if (!active) return;
        setDownloading(false);
        setDownloadProgress(100);
        toast.success('Diarization model downloaded');
        await refreshModelStatus();
      });
      const un3 = await listen<{ error: string }>('diarization-model-download-error', (e) => {
        if (!active) return;
        setDownloading(false);
        toast.error('Model download failed', { description: e.payload.error });
      });
      unlistenersRef.current = [un1, un2, un3];
    };
    setup();
    return () => {
      active = false;
      unlistenersRef.current.forEach((fn) => fn());
      unlistenersRef.current = [];
    };
  }, [refreshModelStatus]);

  const download = useCallback(async () => {
    setDownloading(true);
    setDownloadProgress(0);
    try {
      await invoke('diarization_init');
      await invoke('diarization_download_model');
    } catch (err) {
      setDownloading(false);
      toast.error('Could not start download', { description: String(err) });
    }
  }, []);

  const cancelDownload = useCallback(async () => {
    try {
      await invoke('diarization_cancel_download');
      setDownloading(false);
      toast.info('Download cancelled');
    } catch (err) {
      console.warn('Cancel failed:', err);
    }
  }, []);

  return (
    <div className="space-y-6 pb-6">
      {/* Feature toggles */}
      <div className="bg-white rounded-lg border border-gray-200 p-6 shadow-sm space-y-5">
        <div>
          <h3 className="text-lg font-semibold text-gray-900">Speaker diarization</h3>
          <p className="text-sm text-gray-600">
            Detect who spoke and label transcript segments per speaker.
          </p>
        </div>

        <ToggleRow
          title="Enable diarization"
          description="Master switch for speaker detection."
          checked={settings.enabled}
          onChange={(v) => update({ enabled: v })}
        />
        <ToggleRow
          title="Auto-diarize on save"
          description="Run diarization automatically when a meeting ends."
          checked={settings.autoDiarize}
          onChange={(v) => update({ autoDiarize: v })}
          disabled={!settings.enabled}
        />
        <ToggleRow
          title="Save separate tracks"
          description="Persist mic and system audio separately for higher diarization quality."
          checked={settings.saveSeparateTracks}
          onChange={(v) => update({ saveSeparateTracks: v })}
        />
        <ToggleRow
          title="Speaker names in summaries"
          description="Prefix each line with the speaker name in the transcript sent to the LLM."
          checked={settings.summarizeWithSpeakers}
          onChange={(v) => update({ summarizeWithSpeakers: v })}
        />
      </div>

      {/* Diarization model */}
      <div className="bg-white rounded-lg border border-gray-200 p-6 shadow-sm space-y-3">
        <div className="flex items-center justify-between gap-4">
          <div>
            <h3 className="text-lg font-semibold text-gray-900">Diarization model</h3>
            <p className="text-sm text-gray-600">
              Local speaker-embedding model used to cluster and identify voices.
            </p>
          </div>
          {modelAvailable && !downloading && (
            <span className="inline-flex items-center gap-1 text-sm text-green-600">
              <CheckCircle2 className="w-4 h-4" /> Installed
            </span>
          )}
        </div>

        {downloading ? (
          <div className="space-y-1.5">
            <div className="flex items-center justify-between text-xs text-gray-600">
              <span>Downloading... {downloadProgress}%</span>
              <button type="button" onClick={cancelDownload} className="text-red-500 hover:text-red-600">
                Cancel
              </button>
            </div>
            <div className="w-full h-2 bg-gray-200 rounded-full overflow-hidden">
              <div
                className="h-full bg-gradient-to-r from-blue-500 to-blue-600 rounded-full transition-all"
                style={{ width: `${downloadProgress}%` }}
              />
            </div>
          </div>
        ) : modelAvailable === false ? (
          <button
            type="button"
            onClick={download}
            className="inline-flex items-center gap-1.5 px-3 py-1.5 text-sm rounded-md bg-blue-600 text-white hover:bg-blue-700"
          >
            <Download className="w-4 h-4" />
            Download model
          </button>
        ) : modelAvailable === null ? (
          <p className="text-xs text-gray-400">Checking model status...</p>
        ) : null}
      </div>

      {/* Enrolled people */}
      <div className="bg-white rounded-lg border border-gray-200 p-6 shadow-sm space-y-3">
        <div>
          <h3 className="text-lg font-semibold text-gray-900">People</h3>
          <p className="text-sm text-gray-600">
            Voice profiles learned from your meetings. Rename, merge, or delete them.
          </p>
        </div>
        <SpeakerIdentityManager />
      </div>
    </div>
  );
}
