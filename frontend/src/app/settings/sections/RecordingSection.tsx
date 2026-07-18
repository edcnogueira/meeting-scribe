'use client';

import React, { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';

interface RecordingPreferences {
  save_folder: string;
  auto_save: boolean;
  file_format: string;
  preferred_mic_device: string | null;
  preferred_system_device: string | null;
}

/**
 * Recording section (task R4): the meetings folder chip (mirrored by the
 * sidebar — O1) with a real folder picker, the per-meeting file-structure row
 * with "Reveal in Finder", and the Finder-safety sync banner.
 */
export function RecordingSection() {
  const [prefs, setPrefs] = useState<RecordingPreferences | null>(null);

  const load = useCallback(async () => {
    try {
      const p = await invoke<RecordingPreferences>('get_recording_preferences');
      setPrefs(p);
    } catch {
      try {
        const def = await invoke<string>('get_default_recordings_folder_path');
        setPrefs({
          save_folder: def,
          auto_save: true,
          file_format: 'mp4',
          preferred_mic_device: null,
          preferred_system_device: null,
        });
      } catch (err) {
        console.error('Failed to load recording preferences:', err);
      }
    }
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  const change = useCallback(async () => {
    try {
      const picked = await invoke<string | null>('select_recording_folder');
      if (!picked || !prefs) return;
      const next = { ...prefs, save_folder: picked };
      await invoke('set_recording_preferences', { preferences: next });
      setPrefs(next);
      toast.success('Pasta das reuniões atualizada');
    } catch (err) {
      toast.error('Não foi possível alterar a pasta', { description: String(err) });
    }
  }, [prefs]);

  const reveal = useCallback(async () => {
    try {
      await invoke('open_recordings_folder');
    } catch (err) {
      console.error('Failed to open recordings folder:', err);
    }
  }, []);

  return (
    <section className="sect on set-col" id="s-rec">
      <h1>Gravação</h1>
      <p className="lede">Onde suas reuniões vivem — como arquivos de verdade, visíveis no Finder.</p>

      <div className="card">
        <div className="card-row">
          <div className="info">
            <b>Pasta das reuniões</b>
            <span>A barra lateral espelha exatamente esta pasta</span>
          </div>
          <span className="path-chip">{prefs?.save_folder || '…'}</span>
          <button className="btn small" onClick={change}>Alterar…</button>
        </div>
        <div className="card-row">
          <div className="info">
            <b>Estrutura de cada reunião</b>
            <span className="mono" style={{ fontSize: 11.5 }}>audio.m4a · transcript.json · summary.md · meta.json</span>
          </div>
          <button className="btn small ghost" onClick={reveal}>Revelar no Finder</button>
        </div>
      </div>

      <div className="banner accent">
        <span className="b-ico">🔒</span>
        <div className="b-body">
          Mover ou renomear pastas no Finder é seguro — use <strong>Sincronizar</strong> na barra lateral para refletir as mudanças aqui.
        </div>
      </div>
    </section>
  );
}
