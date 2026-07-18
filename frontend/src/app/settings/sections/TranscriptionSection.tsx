'use client';

import React, { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { toast } from 'sonner';
import {
  WhisperAPI,
  MODEL_CONFIGS,
  type ModelInfo,
  type ModelStatus,
} from '@/lib/whisper';
import { useConfig } from '@/contexts/ConfigContext';

/** pt-BR file-size formatting matching the design ("1,6 GB", "466 MB"). */
function formatSize(sizeMb: number): string {
  if (sizeMb >= 1000) {
    return `${(sizeMb / 1000).toFixed(1).replace('.', ',')} GB`;
  }
  return `${Math.round(sizeMb)} MB`;
}

function isDownloading(status: ModelStatus): number | null {
  if (typeof status === 'object' && status && 'Downloading' in status) {
    return (status as { Downloading: number }).Downloading;
  }
  return null;
}

function isCorrupted(status: ModelStatus): boolean {
  return typeof status === 'object' && status !== null && 'Corrupted' in status;
}

function errorOf(status: ModelStatus): string | null {
  if (typeof status === 'object' && status && 'Error' in status) {
    return (status as { Error: string }).Error;
  }
  return null;
}

/**
 * Transcription section (task R4): Whisper model cards wired to the real
 * whisper-rs backend — active / downloading / available states with live
 * download progress + cancel, plus the Metal-acceleration banner.
 */
export function TranscriptionSection() {
  const { transcriptModelConfig, setTranscriptModelConfig } = useConfig();
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [currentModel, setCurrentModel] = useState<string | null>(null);
  const [isMac, setIsMac] = useState(false);
  const [loading, setLoading] = useState(true);
  const progressThrottle = useRef<Record<string, number>>({});

  const refresh = useCallback(async () => {
    try {
      const list = await WhisperAPI.getAvailableModels();
      setModels(list);
    } catch (err) {
      console.error('Failed to list whisper models:', err);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    WhisperAPI.init().catch(() => {});
    refresh();
    WhisperAPI.getCurrentModel().then(setCurrentModel).catch(() => {});
    (async () => {
      try {
        const { platform } = await import('@tauri-apps/plugin-os');
        setIsMac(platform() === 'macos');
      } catch {
        setIsMac(
          typeof navigator !== 'undefined' &&
            navigator.userAgent.toLowerCase().includes('mac')
        );
      }
    })();
  }, [refresh]);

  // Live download events.
  useEffect(() => {
    let active = true;
    const unlisteners: UnlistenFn[] = [];
    (async () => {
      const patch = (name: string, status: ModelStatus) => {
        if (!active) return;
        setModels((prev) =>
          prev.map((m) => (m.name === name ? { ...m, status } : m))
        );
      };
      unlisteners.push(
        await listen<{ modelName: string; progress: number }>(
          'model-download-progress',
          (e) => {
            const { modelName, progress } = e.payload;
            const now = Date.now();
            const last = progressThrottle.current[modelName] ?? 0;
            if (now - last < 250 && progress < 100) return;
            progressThrottle.current[modelName] = now;
            patch(modelName, { Downloading: progress });
          }
        )
      );
      unlisteners.push(
        await listen<{ modelName: string }>('model-download-complete', (e) => {
          patch(e.payload.modelName, 'Available');
          toast.success(`${e.payload.modelName} baixado`);
          refresh();
        })
      );
      unlisteners.push(
        await listen<{ modelName: string; error: string }>(
          'model-download-error',
          (e) => {
            patch(e.payload.modelName, { Error: e.payload.error });
            toast.error('Falha no download', { description: e.payload.error });
          }
        )
      );
    })();
    return () => {
      active = false;
      unlisteners.forEach((fn) => fn());
    };
  }, [refresh]);

  const download = useCallback(async (name: string) => {
    setModels((prev) =>
      prev.map((m) => (m.name === name ? { ...m, status: { Downloading: 0 } } : m))
    );
    try {
      await WhisperAPI.downloadModel(name);
    } catch (err) {
      setModels((prev) =>
        prev.map((m) => (m.name === name ? { ...m, status: 'Missing' } : m))
      );
      toast.error('Não foi possível iniciar o download', { description: String(err) });
    }
  }, []);

  const cancel = useCallback(async (name: string) => {
    try {
      await WhisperAPI.cancelDownload(name);
      setModels((prev) =>
        prev.map((m) => (m.name === name ? { ...m, status: 'Missing' } : m))
      );
    } catch (err) {
      console.warn('Cancel failed:', err);
    }
  }, []);

  const activate = useCallback(
    async (name: string) => {
      try {
        await invoke('api_save_transcript_config', {
          provider: 'localWhisper',
          model: name,
          apiKey: null,
        });
        setTranscriptModelConfig({ provider: 'localWhisper', model: name, apiKey: null });
        setCurrentModel(name);
        WhisperAPI.loadModel(name).catch(() => {});
        toast.success(`Modelo ativo: ${name}`);
      } catch (err) {
        toast.error('Não foi possível ativar o modelo', { description: String(err) });
      }
    },
    [setTranscriptModelConfig]
  );

  const activeName = currentModel ?? (transcriptModelConfig.provider === 'localWhisper' ? transcriptModelConfig.model : null);

  return (
    <section className="sect on set-col" id="s-trans">
      <h1>Transcrição</h1>
      <p className="lede">Modelos Whisper rodam no seu Mac. Baixe uma vez, use offline para sempre.</p>

      <div className="card">
        {loading && <div className="card-row"><div className="info"><span>Carregando modelos…</span></div></div>}
        {!loading && models.length === 0 && (
          <div className="card-row"><div className="info"><span>Nenhum modelo disponível.</span></div></div>
        )}
        {models.map((m) => {
          const dl = isDownloading(m.status);
          const isActive = activeName === m.name;
          const available = m.status === 'Available';
          const corrupted = isCorrupted(m.status);
          const err = errorOf(m.status);
          const desc = m.description ?? MODEL_CONFIGS[m.name]?.description ?? '';
          const sizeMb = m.size_mb || MODEL_CONFIGS[m.name]?.size_mb || 0;
          return (
            <div className="card-row" key={m.name}>
              <div className="info">
                <b>{m.name}</b>
                <span>{[desc, sizeMb ? formatSize(sizeMb) : ''].filter(Boolean).join(' · ')}</span>
              </div>

              {dl !== null ? (
                <>
                  <span className="mono-meta">{Math.floor(dl)}%</span>
                  <div style={{ width: 130 }}>
                    <div className={`progress${dl <= 0 ? ' indeterminate' : ''}`}>
                      <i style={{ width: `${dl}%` }} />
                    </div>
                  </div>
                  <button className="btn small ghost" onClick={() => cancel(m.name)}>Cancelar</button>
                </>
              ) : err ? (
                <>
                  <span className="badge danger" data-tip={err}>erro</span>
                  <button className="btn small" onClick={() => download(m.name)}>Tentar de novo</button>
                </>
              ) : corrupted ? (
                <>
                  <span className="badge warn">corrompido</span>
                  <button className="btn small" onClick={() => download(m.name)}>Rebaixar</button>
                </>
              ) : isActive ? (
                <>
                  {isMac && <span className="badge accent">Metal · GPU</span>}
                  <span className="badge ok">✓ Ativo</span>
                </>
              ) : available ? (
                <button className="btn small ghost" onClick={() => activate(m.name)}>Ativar</button>
              ) : (
                <button className="btn small" onClick={() => download(m.name)}>Baixar</button>
              )}
            </div>
          );
        })}
      </div>

      {isMac && (
        <div className="banner">
          <span className="b-ico">ⓘ</span>
          <div className="b-body">
            A aceleração <strong>Metal</strong> foi detectada neste Mac — a transcrição usa a GPU automaticamente.
          </div>
        </div>
      )}
    </section>
  );
}
