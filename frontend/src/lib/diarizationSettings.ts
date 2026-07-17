/**
 * Speaker-diarization UI settings (task D5).
 *
 * Persisted client-side in localStorage (single JSON blob, mirroring the
 * betaFeatures pattern) and pushed into the Rust backend via
 * `set_diarization_settings` so the recording / auto-diarize paths honor them.
 * Defaults intentionally match the backend's env/compiled defaults, so an
 * un-synced first run behaves identically to D2/D3.
 */

import { invoke } from '@tauri-apps/api/core';

export interface DiarizationSettings {
  /** Master switch for speaker diarization. */
  enabled: boolean;
  /** Run diarization automatically when a meeting is saved. */
  autoDiarize: boolean;
  /** Persist separate mic/system tracks (D2) for better diarization. */
  saveSeparateTracks: boolean;
  /** Prefix each line with the speaker name in the transcript sent to the LLM. */
  summarizeWithSpeakers: boolean;
}

export const DEFAULT_DIARIZATION_SETTINGS: DiarizationSettings = {
  enabled: true,
  autoDiarize: true,
  saveSeparateTracks: true,
  summarizeWithSpeakers: false,
};

const STORAGE_KEY = 'diarizationSettings';

/** Load settings from localStorage, falling back to defaults. SSR-safe. */
export function loadDiarizationSettings(): DiarizationSettings {
  if (typeof window === 'undefined') return { ...DEFAULT_DIARIZATION_SETTINGS };
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (!raw) return { ...DEFAULT_DIARIZATION_SETTINGS };
    const parsed = JSON.parse(raw);
    return { ...DEFAULT_DIARIZATION_SETTINGS, ...parsed };
  } catch {
    return { ...DEFAULT_DIARIZATION_SETTINGS };
  }
}

/** Persist settings to localStorage. */
export function saveDiarizationSettings(settings: DiarizationSettings): void {
  if (typeof window === 'undefined') return;
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(settings));
  } catch (err) {
    console.warn('Failed to persist diarization settings:', err);
  }
}

/** Push the three backend-facing toggles into the Rust runtime. */
export async function syncDiarizationSettingsToBackend(
  settings: DiarizationSettings = loadDiarizationSettings()
): Promise<void> {
  try {
    await invoke('set_diarization_settings', {
      enabled: settings.enabled,
      autoDiarize: settings.autoDiarize,
      saveSeparateTracks: settings.saveSeparateTracks,
    });
  } catch (err) {
    // Non-fatal: backend keeps its env/default values.
    console.warn('Failed to sync diarization settings to backend:', err);
  }
}
