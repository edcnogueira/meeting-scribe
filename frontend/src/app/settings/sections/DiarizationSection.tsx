'use client';

import React, { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { toast } from 'sonner';
import { SpeakerChip } from '@/components/SpeakerChip';
import {
  DiarizationSettings as DiarizationSettingsType,
  DEFAULT_DIARIZATION_SETTINGS,
  loadDiarizationSettings,
  saveDiarizationSettings,
  syncDiarizationSettingsToBackend,
} from '@/lib/diarizationSettings';

interface SpeakerIdentityInfo {
  id: string;
  name: string;
  sample_count: number;
  is_self: boolean;
  has_embedding: boolean;
}

/** Filled sample cells (out of 10) — scaled so ~30 samples fills the bar. */
function filledCells(sampleCount: number): number {
  if (sampleCount <= 0) return 0;
  return Math.min(10, Math.max(1, Math.round(sampleCount / 3)));
}

/**
 * Diarization section (task R4): feature toggles (persisted + pushed to the
 * backend), the embeddings-model download row, and the enrolled voice registry
 * with rename / merge / destructive biometric-wipe.
 */
export function DiarizationSection() {
  const [settings, setSettings] = useState<DiarizationSettingsType>(DEFAULT_DIARIZATION_SETTINGS);

  // Embeddings model state.
  const [modelAvailable, setModelAvailable] = useState<boolean | null>(null);
  const [downloading, setDownloading] = useState(false);
  const [downloadProgress, setDownloadProgress] = useState(0);

  // Voice registry.
  const [identities, setIdentities] = useState<SpeakerIdentityInfo[]>([]);
  const [editing, setEditing] = useState<string | null>(null);
  const [editValue, setEditValue] = useState('');
  const [mergeSource, setMergeSource] = useState<string | null>(null);
  const [pendingDelete, setPendingDelete] = useState<SpeakerIdentityInfo | null>(null);

  useEffect(() => {
    setSettings(loadDiarizationSettings());
    syncDiarizationSettingsToBackend();
  }, []);

  const update = useCallback((patch: Partial<DiarizationSettingsType>) => {
    setSettings((prev) => {
      const next = { ...prev, ...patch };
      saveDiarizationSettings(next);
      syncDiarizationSettingsToBackend(next);
      return next;
    });
  }, []);

  const refreshModelStatus = useCallback(async () => {
    try {
      setModelAvailable(await invoke<boolean>('diarization_is_model_available'));
    } catch {
      setModelAvailable(false);
    }
  }, []);

  const loadIdentities = useCallback(async () => {
    try {
      setIdentities(await invoke<SpeakerIdentityInfo[]>('api_list_speaker_identities'));
    } catch (err) {
      console.error('Failed to load identities:', err);
    }
  }, []);

  useEffect(() => {
    refreshModelStatus();
    loadIdentities();
  }, [refreshModelStatus, loadIdentities]);

  // Embeddings download events.
  const unlistenersRef = useRef<UnlistenFn[]>([]);
  useEffect(() => {
    let active = true;
    (async () => {
      const un1 = await listen<{ progress: number }>('diarization-model-download-progress', (e) => {
        if (!active) return;
        setDownloading(true);
        setDownloadProgress(Math.round(e.payload.progress ?? 0));
      });
      const un2 = await listen('diarization-model-download-complete', async () => {
        if (!active) return;
        setDownloading(false);
        setDownloadProgress(100);
        toast.success('Modelo de embeddings baixado');
        await refreshModelStatus();
      });
      const un3 = await listen<{ error: string }>('diarization-model-download-error', (e) => {
        if (!active) return;
        setDownloading(false);
        toast.error('Falha no download do modelo', { description: e.payload.error });
      });
      unlistenersRef.current = [un1, un2, un3];
    })();
    return () => {
      active = false;
      unlistenersRef.current.forEach((fn) => fn());
      unlistenersRef.current = [];
    };
  }, [refreshModelStatus]);

  const downloadModel = useCallback(async () => {
    setDownloading(true);
    setDownloadProgress(0);
    try {
      await invoke('diarization_init');
      await invoke('diarization_download_model');
    } catch (err) {
      setDownloading(false);
      toast.error('Não foi possível iniciar o download', { description: String(err) });
    }
  }, []);

  const cancelModelDownload = useCallback(async () => {
    try {
      await invoke('diarization_cancel_download');
      setDownloading(false);
      toast.info('Download cancelado');
    } catch (err) {
      console.warn('Cancel failed:', err);
    }
  }, []);

  const saveRename = useCallback(
    async (speakerId: string) => {
      const name = editValue.trim();
      if (!name) {
        setEditing(null);
        return;
      }
      try {
        await invoke('api_rename_speaker_identity', { speakerId, name });
        toast.success('Pessoa renomeada');
        setEditing(null);
        await loadIdentities();
      } catch (err) {
        toast.error('Falha ao renomear', { description: String(err) });
      }
    },
    [editValue, loadIdentities]
  );

  const doMerge = useCallback(
    async (sourceId: string, targetId: string) => {
      try {
        await invoke('api_merge_speaker_identities', { targetId, sourceId });
        toast.success('Vozes mescladas');
        setMergeSource(null);
        await loadIdentities();
      } catch (err) {
        toast.error('Falha ao mesclar', { description: String(err) });
      }
    },
    [loadIdentities]
  );

  const confirmDelete = useCallback(async () => {
    if (!pendingDelete) return;
    const target = pendingDelete;
    setPendingDelete(null);
    try {
      await invoke('api_delete_speaker_identity', { speakerId: target.id });
      toast.success(`Voz de ${target.name} excluída`);
      await loadIdentities();
    } catch (err) {
      toast.error('Falha ao excluir', { description: String(err) });
    }
  }, [pendingDelete, loadIdentities]);

  return (
    <section className="sect on set-col" id="s-diar">
      <h1>Diarização</h1>
      <p className="lede">Separa quem disse o quê e aprende as vozes que você nomear. Tudo no dispositivo.</p>

      <div className="card">
        <div className="card-row">
          <div className="info">
            <b>Identificar falantes</b>
            <span>Roda automaticamente ao fim de cada gravação</span>
          </div>
          <input
            type="checkbox"
            className="switch"
            checked={settings.autoDiarize && settings.enabled}
            onChange={(e) => update({ enabled: e.target.checked, autoDiarize: e.target.checked })}
            aria-label="Ativar diarização"
          />
        </div>
        <div className="card-row">
          <div className="info">
            <b>Modelo de embeddings de voz</b>
            <span>wespeaker-voxceleb · 79 MB</span>
          </div>
          {downloading ? (
            <>
              <span className="mono-meta">{downloadProgress}%</span>
              <div style={{ width: 130 }}>
                <div className="progress"><i style={{ width: `${downloadProgress}%` }} /></div>
              </div>
              <button className="btn small ghost" onClick={cancelModelDownload}>Cancelar</button>
            </>
          ) : modelAvailable ? (
            <span className="badge ok">✓ baixado</span>
          ) : modelAvailable === false ? (
            <button className="btn small" onClick={downloadModel}>Baixar</button>
          ) : (
            <span className="mono-meta">verificando…</span>
          )}
        </div>
        <div className="card-row">
          <div className="info">
            <b>Nomes de falantes nos resumos</b>
            <span>Quando ativo, os nomes entram no prompt do provider de resumo — inclusive nos de nuvem</span>
          </div>
          <input
            type="checkbox"
            className="switch"
            checked={settings.summarizeWithSpeakers}
            onChange={(e) => update({ summarizeWithSpeakers: e.target.checked })}
            aria-label="Incluir nomes nos resumos"
          />
        </div>
      </div>

      <div>
        <h1 style={{ fontSize: 15, marginBottom: 2 }}>Registro de vozes</h1>
        <p className="lede" style={{ marginTop: 0 }}>
          Vozes conhecidas são reconhecidas automaticamente na próxima reunião.
        </p>
      </div>

      <div className="card">
        {identities.length === 0 && (
          <div className="card-row">
            <div className="info">
              <span>Nenhuma voz registrada ainda. Renomeie um falante detectado numa reunião para registrá-lo.</span>
            </div>
          </div>
        )}
        {identities.map((id) => {
          const filled = filledCells(id.sample_count);
          return (
            <div className="card-row reg-row" key={id.id}>
              {editing === id.id ? (
                <input
                  className="input"
                  autoFocus
                  value={editValue}
                  onChange={(e) => setEditValue(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter') saveRename(id.id);
                    if (e.key === 'Escape') setEditing(null);
                  }}
                  onBlur={() => saveRename(id.id)}
                  style={{ maxWidth: 200 }}
                  aria-label="Novo nome"
                />
              ) : (
                <SpeakerChip speaker={id.name} isYou={id.is_self} />
              )}
              <div className="info">
                <span>{id.sample_count} amostra{id.sample_count === 1 ? '' : 's'} de voz</span>
                <div className="samples-bar">
                  {Array.from({ length: 10 }).map((_, i) => (
                    <i key={i} className={i < filled ? '' : 'dim'} />
                  ))}
                </div>
              </div>
              <div className="actions">
                {mergeSource === id.id ? (
                  <select
                    className="input"
                    autoFocus
                    defaultValue=""
                    onChange={(e) => e.target.value && doMerge(id.id, e.target.value)}
                    onBlur={() => setMergeSource(null)}
                    style={{ maxWidth: 160, height: 28, padding: '2px 6px', fontSize: 12 }}
                  >
                    <option value="" disabled>Mesclar em…</option>
                    {identities.filter((o) => o.id !== id.id).map((o) => (
                      <option key={o.id} value={o.id}>{o.name}</option>
                    ))}
                  </select>
                ) : (
                  <>
                    <button
                      className="btn small ghost"
                      onClick={() => {
                        setEditing(id.id);
                        setEditValue(id.name);
                      }}
                    >
                      Renomear
                    </button>
                    {identities.length >= 2 && (
                      <button className="btn small ghost" onClick={() => setMergeSource(id.id)}>Mesclar…</button>
                    )}
                    <button
                      className="btn small ghost"
                      style={{ color: 'var(--danger)' }}
                      onClick={() => setPendingDelete(id)}
                    >
                      Excluir
                    </button>
                  </>
                )}
              </div>
            </div>
          );
        })}
      </div>

      {/* delete-voice modal: biometric wipe must be unmistakable */}
      <div className={`overlay${pendingDelete ? ' open' : ''}`}>
        <div className="modal" role="dialog" aria-label="Excluir voz do registro">
          <div className="modal-head">
            <h3>Excluir a voz de {pendingDelete?.name ?? '—'}?</h3>
            <p>
              Isto apaga permanentemente os <strong>dados biométricos de voz</strong> desta pessoa
              (embeddings e amostras). Ela deixa de ser reconhecida automaticamente nas próximas
              reuniões. Transcrições existentes não são alteradas.
            </p>
          </div>
          <div className="modal-foot">
            <button className="btn ghost" onClick={() => setPendingDelete(null)}>Cancelar</button>
            <button className="btn danger" onClick={confirmDelete}>Excluir dados de voz</button>
          </div>
        </div>
      </div>
    </section>
  );
}
