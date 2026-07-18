'use client';

import '@/styles/recording.css';

import { useState, useEffect, useRef, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { appDataDir } from '@tauri-apps/api/path';
import { listen } from '@tauri-apps/api/event';
import { useRouter } from 'next/navigation';
import { toast } from 'sonner';

import { ScreenHeader } from '@/components/shell/ScreenHeader';
import { MicIcon } from '@/components/shell/icons';

import { useSidebar } from '@/components/Sidebar/SidebarProvider';
import { usePermissionCheck } from '@/hooks/usePermissionCheck';
import { useRecordingState, RecordingStatus } from '@/contexts/RecordingStateContext';
import { useTranscripts } from '@/contexts/TranscriptContext';
import { useConfig } from '@/contexts/ConfigContext';
import { useModalState } from '@/hooks/useModalState';
import { useRecordingStateSync } from '@/hooks/useRecordingStateSync';
import { useRecordingStart } from '@/hooks/useRecordingStart';
import { useRecordingStop } from '@/hooks/useRecordingStop';
import { useTranscriptRecovery } from '@/hooks/useTranscriptRecovery';

import { SettingsModals } from './_components/SettingsModal';
import { TranscriptRecovery } from '@/components/TranscriptRecovery';
import { indexedDBService } from '@/services/indexedDBService';
import { transcriptService } from '@/services/transcriptService';
import type { AudioDevice } from '@/components/DeviceSelection';
import Analytics from '@/lib/analytics';

/** Recording-relative mm:ss for a timestamp expressed in seconds. */
function fmtClock(seconds: number | null | undefined): string {
  const total = Math.max(0, Math.floor(seconds ?? 0));
  const m = Math.floor(total / 60);
  const s = total % 60;
  return `${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`;
}

/** Strip the "(input)"/"(output)" suffix used by the device wiring for display. */
function deviceLabel(value: string | null): string | null {
  if (!value) return null;
  return value.replace(/\s*\((input|output)\)\s*$/i, '');
}

/** Bluetooth headset heuristic (mirrors DeviceSelection.getDeviceMetadata). */
function isBluetoothName(name: string | null): boolean {
  if (!name) return false;
  const n = name.toLowerCase();
  return (
    n.includes('airpods') ||
    n.includes('bluetooth') ||
    n.includes('wireless') ||
    n.includes('wh-') ||
    n.includes('bt ')
  );
}

export default function Home() {
  const router = useRouter();

  // Local recording mirror the recording hooks require as a setter target.
  const [isRecording, setIsRecordingState] = useState(false);
  const [showRecoveryDialog, setShowRecoveryDialog] = useState(false);

  // Global + config state
  const recordingState = useRecordingState();
  const { status } = recordingState;
  const { transcripts, transcriptContainerRef } = useTranscripts();
  const {
    selectedDevices,
    setSelectedDevices,
    transcriptModelConfig,
  } = useConfig();

  // Permissions / shell
  const {
    hasMicrophone,
    hasSystemAudio,
    isChecking: permsChecking,
    requestPermissions,
  } = usePermissionCheck();
  const { setIsMeetingActive, currentMeeting, refetchMeetings } = useSidebar();

  // Modal + recording plumbing (preserved from the previous screen)
  const { modals, messages, showModal, hideModal } = useModalState(transcriptModelConfig);
  const { isRecordingDisabled, setIsRecordingDisabled } = useRecordingStateSync(
    isRecording,
    setIsRecordingState,
    setIsMeetingActive,
  );
  const { handleRecordingStart } = useRecordingStart(isRecording, setIsRecordingState, showModal);
  const { handleRecordingStop, setIsStopping } = useRecordingStop(
    setIsRecordingState,
    setIsRecordingDisabled,
  );

  // Transcript recovery (unchanged plumbing)
  const {
    recoverableMeetings,
    checkForRecoverableTranscripts,
    recoverMeeting,
    loadMeetingTranscripts,
    deleteRecoverableMeeting,
  } = useTranscriptRecovery();

  // ---- Device discovery ---------------------------------------------------
  const [devices, setDevices] = useState<AudioDevice[]>([]);
  const devicesInitRef = useRef(false);

  const fetchDevices = useCallback(async () => {
    try {
      const result = await invoke<AudioDevice[]>('get_audio_devices');
      setDevices(result);
    } catch (err) {
      console.error('Failed to fetch audio devices:', err);
    }
  }, []);

  useEffect(() => {
    fetchDevices();
  }, [fetchDevices]);

  const inputDevices = devices.filter((d) => d.device_type === 'Input');
  const outputDevices = devices.filter((d) => d.device_type === 'Output');

  // Seed default device selection once real devices are known.
  useEffect(() => {
    if (devicesInitRef.current || devices.length === 0) return;
    devicesInitRef.current = true;
    const mic = selectedDevices.micDevice ?? (inputDevices[0] ? `${inputDevices[0].name} (input)` : null);
    const system =
      selectedDevices.systemDevice ?? (outputDevices[0] ? `${outputDevices[0].name} (output)` : null);
    setSelectedDevices({ micDevice: mic, systemDevice: system });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [devices]);

  const selectedMicLabel = deviceLabel(selectedDevices.micDevice) ?? inputDevices[0]?.name ?? null;
  const systemCaptureEnabled = selectedDevices.systemDevice != null;
  const showBluetoothWarning = isBluetoothName(selectedMicLabel);

  const handleMicChange = useCallback(
    (name: string) => {
      setSelectedDevices({ ...selectedDevices, micDevice: `${name} (input)` });
    },
    [selectedDevices, setSelectedDevices],
  );

  const handleSystemChange = useCallback(
    (mode: string) => {
      if (mode === 'capture') {
        const dev = outputDevices[0];
        setSelectedDevices({
          ...selectedDevices,
          systemDevice: dev ? `${dev.name} (output)` : null,
        });
      } else {
        setSelectedDevices({ ...selectedDevices, systemDevice: null });
      }
    },
    [selectedDevices, setSelectedDevices, outputDevices],
  );

  const openSystemSettings = useCallback((pane: string) => {
    invoke('open_system_settings', { preferencePane: pane }).catch((err) =>
      console.error('Failed to open system settings:', err),
    );
  }, []);

  // ---- Derived screen state ----------------------------------------------
  const isLive = recordingState.isRecording || status === RecordingStatus.STARTING;
  const isProcessing =
    !isLive &&
    (status === RecordingStatus.STOPPING ||
      status === RecordingStatus.PROCESSING_TRANSCRIPTS ||
      status === RecordingStatus.SAVING ||
      status === RecordingStatus.COMPLETED);
  const screen: 'setup' | 'live' | 'proc' = isLive ? 'live' : isProcessing ? 'proc' : 'setup';
  const isCompleted = status === RecordingStatus.COMPLETED;

  // ---- Live: elapsed timer -----------------------------------------------
  const [elapsed, setElapsed] = useState(0);
  const pausedRef = useRef(recordingState.isPaused);
  pausedRef.current = recordingState.isPaused;

  useEffect(() => {
    if (screen === 'live') setElapsed(Math.floor(recordingState.recordingDuration ?? 0));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [screen]);

  useEffect(() => {
    if (screen !== 'live') return;
    const id = setInterval(() => {
      if (!pausedRef.current) setElapsed((e) => e + 1);
    }, 1000);
    return () => clearInterval(id);
  }, [screen]);

  // ---- Live: real 7-bar meter driven by the VAD speech-detected event -----
  const [bars, setBars] = useState<number[]>(() => Array(7).fill(18));
  const energyRef = useRef(0.25);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    listen('speech-detected', () => {
      energyRef.current = 1;
    })
      .then((fn) => {
        unlisten = fn;
      })
      .catch(() => {});
    return () => {
      if (unlisten) unlisten();
    };
  }, []);

  useEffect(() => {
    if (screen !== 'live' || recordingState.isPaused) {
      setBars(Array(7).fill(18));
      return;
    }
    const id = setInterval(() => {
      energyRef.current = Math.max(0.18, energyRef.current * 0.88);
      setBars(Array.from({ length: 7 }, () => 18 + Math.random() * 78 * energyRef.current));
    }, 140);
    return () => clearInterval(id);
  }, [screen, recordingState.isPaused]);

  // ---- Processing: real progress from the transcription queue -------------
  const [progress, setProgress] = useState(8);
  const maxChunksRef = useRef(0);

  useEffect(() => {
    if (screen !== 'proc') {
      setProgress(8);
      maxChunksRef.current = 0;
      return;
    }
    if (isCompleted) {
      setProgress(100);
      return;
    }
    const poll = async () => {
      try {
        const s = await transcriptService.getTranscriptionStatus();
        if (s.chunks_in_queue > maxChunksRef.current) maxChunksRef.current = s.chunks_in_queue;
        setProgress((p) => {
          if (status === RecordingStatus.SAVING) return Math.max(p, 96);
          const denom = maxChunksRef.current + 1;
          const target =
            maxChunksRef.current > 0 ? (1 - s.chunks_in_queue / denom) * 92 : p + 4;
          return Math.min(94, Math.max(p, target, 8));
        });
      } catch {
        /* status may be unavailable briefly during teardown */
      }
    };
    poll();
    const id = setInterval(poll, 500);
    return () => clearInterval(id);
  }, [screen, status, isCompleted]);

  useEffect(() => {
    if (isCompleted) setProgress(100);
  }, [isCompleted]);

  // ---- Start / stop -------------------------------------------------------
  const startRecording = useCallback(async () => {
    if (isRecordingDisabled) return;
    Analytics.trackButtonClick('start_recording', 'recording_screen');
    try {
      await handleRecordingStart();
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      console.error('Failed to start recording:', error);
      showModal('errorAlert', message);
    }
  }, [handleRecordingStart, isRecordingDisabled, showModal]);

  const stopRecording = useCallback(async () => {
    if (!recordingState.isRecording) return;
    Analytics.trackButtonClick('stop_recording', 'recording_screen');
    setIsStopping(true);
    try {
      const dataDir = await appDataDir();
      const timestamp = new Date().toISOString().replace(/[:.]/g, '-');
      const savePath = `${dataDir}/recording-${timestamp}.wav`;
      await invoke('stop_recording', { args: { save_path: savePath } });
      Analytics.trackTranscriptionSuccess();
      await handleRecordingStop(true);
    } catch (error) {
      console.error('Failed to stop recording:', error);
      await handleRecordingStop(false);
    }
  }, [recordingState.isRecording, setIsStopping, handleRecordingStop]);

  // ⌘R / Ctrl+R toggles recording from the setup screen.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'r' && !e.repeat) {
        if (screen === 'setup') {
          e.preventDefault();
          startRecording();
        }
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [screen, startRecording]);

  // Bridge the tray / sidebar "start recording" request to this screen.
  useEffect(() => {
    const onSidebarStart = () => {
      if (screen === 'setup') startRecording();
    };
    window.addEventListener('start-recording-from-sidebar', onSidebarStart);
    return () => window.removeEventListener('start-recording-from-sidebar', onSidebarStart);
  }, [screen, startRecording]);

  const openMeeting = useCallback(() => {
    const id = currentMeeting?.id;
    if (id && id !== 'intro-call') {
      router.push(`/meeting-details?id=${id}&source=recording`);
    }
  }, [currentMeeting, router]);

  // ---- Startup housekeeping (unchanged plumbing) --------------------------
  useEffect(() => {
    Analytics.trackPageView('home');
  }, []);

  useEffect(() => {
    const performStartupChecks = async () => {
      if (
        recordingState.isRecording ||
        status === RecordingStatus.STOPPING ||
        status === RecordingStatus.PROCESSING_TRANSCRIPTS ||
        status === RecordingStatus.SAVING
      ) {
        return;
      }
      try {
        await indexedDBService.deleteOldMeetings(7).catch(() => {});
        await indexedDBService.deleteSavedMeetings(24).catch(() => {});
        await checkForRecoverableTranscripts();
      } catch (error) {
        console.error('Failed to perform startup checks:', error);
      }
    };
    performStartupChecks();
  }, [checkForRecoverableTranscripts, recordingState.isRecording, status]);

  useEffect(() => {
    if (recoverableMeetings.length > 0) {
      const shown = sessionStorage.getItem('recovery_dialog_shown');
      if (!shown) {
        setShowRecoveryDialog(true);
        sessionStorage.setItem('recovery_dialog_shown', 'true');
      }
    }
  }, [recoverableMeetings]);

  const handleRecovery = async (meetingId: string) => {
    try {
      const result = await recoverMeeting(meetingId);
      if (result.success) {
        toast.success('Meeting recovered successfully!', {
          description:
            result.audioRecoveryStatus?.status === 'success'
              ? 'Transcripts and audio recovered'
              : 'Transcripts recovered (no audio available)',
          action: result.meetingId
            ? { label: 'View Meeting', onClick: () => router.push(`/meeting-details?id=${result.meetingId}`) }
            : undefined,
          duration: 10000,
        });
        await refetchMeetings();
        if (recoverableMeetings.length === 0) sessionStorage.removeItem('recovery_dialog_shown');
        if (result.meetingId) {
          setTimeout(() => router.push(`/meeting-details?id=${result.meetingId}`), 2000);
        }
      }
    } catch (error) {
      toast.error('Failed to recover meeting', {
        description: error instanceof Error ? error.message : 'Unknown error occurred',
      });
      throw error;
    }
  };

  const handleDialogClose = () => {
    setShowRecoveryDialog(false);
    if (recoverableMeetings.length === 0) sessionStorage.removeItem('recovery_dialog_shown');
  };

  // ---- Header badge -------------------------------------------------------
  const headerBadge =
    screen === 'live' ? (
      <span className="badge rec">● REC</span>
    ) : screen === 'proc' ? (
      <span className="badge accent">Processando</span>
    ) : null;

  // Processing copy
  const modelLabel =
    transcriptModelConfig?.provider === 'localWhisper' && transcriptModelConfig?.model
      ? `Whisper ${transcriptModelConfig.model}`
      : transcriptModelConfig?.model || 'Whisper large-v3-turbo';
  const folderPath =
    typeof window !== 'undefined' ? sessionStorage.getItem('last_recording_folder_path') || '' : '';

  const liveDevicesLabel =
    (selectedMicLabel || 'Microfone padrão') + (systemCaptureEnabled ? ' · áudio do sistema' : '');

  return (
    <>
      <ScreenHeader crumbs={[{ label: 'Nova reunião' }]} actions={headerBadge} />

      <div className="rec-wrap">
        <div className="rec-col">
          {/* ===== state: setup ===== */}
          {screen === 'setup' && (
            <>
              <div>
                <h1 className="rec-title">Pronto para gravar</h1>
                <p className="rec-sub">
                  Áudio e transcrição ficam nesta máquina. Nada é enviado para a nuvem.
                </p>
              </div>

              <div className="device-card">
                <div className="dev-row">
                  <label>
                    <MicIcon />
                    Microfone
                  </label>
                  <select
                    className="input"
                    value={selectedMicLabel ?? ''}
                    onChange={(e) => handleMicChange(e.target.value)}
                    disabled={inputDevices.length === 0}
                  >
                    {inputDevices.length === 0 && <option value="">Nenhum microfone encontrado</option>}
                    {inputDevices.map((d) => (
                      <option key={d.name} value={d.name}>
                        {d.name}
                      </option>
                    ))}
                  </select>
                </div>

                <div className="dev-row">
                  <label>Áudio do sistema</label>
                  <select
                    className="input"
                    value={systemCaptureEnabled ? 'capture' : 'none'}
                    onChange={(e) => handleSystemChange(e.target.value)}
                  >
                    <option value="capture">Capturar áudio do sistema (ScreenCaptureKit)</option>
                    <option value="none">Não capturar</option>
                  </select>
                </div>

                {showBluetoothWarning && (
                  <div className="banner warn">
                    <span className="b-ico">⚠</span>
                    <div className="b-body">
                      <strong>Fones Bluetooth como microfone reduzem a qualidade da captura.</strong>
                      <br />
                      O macOS troca para o perfil SCO (8&nbsp;kHz) quando um fone Bluetooth também
                      grava. Prefira o microfone interno e mantenha o fone apenas como saída.
                    </div>
                  </div>
                )}

                <div className="perm-row">
                  {hasMicrophone ? (
                    <span className="badge ok">✓ Microfone permitido</span>
                  ) : (
                    <button
                      type="button"
                      className="badge danger"
                      onClick={() => {
                        openSystemSettings('Privacy_Microphone');
                        requestPermissions();
                      }}
                      style={{ cursor: 'pointer' }}
                    >
                      Permitir microfone
                    </button>
                  )}

                  {hasSystemAudio ? (
                    <span className="badge ok">✓ Gravação de tela permitida</span>
                  ) : (
                    <button
                      type="button"
                      className="badge danger"
                      onClick={() => {
                        openSystemSettings('Privacy_ScreenCapture');
                        requestPermissions();
                      }}
                      style={{ cursor: 'pointer' }}
                    >
                      Permitir gravação de tela
                    </button>
                  )}

                  <span
                    className="faint"
                    data-tip={
                      'Exigida pelo macOS para capturar o áudio do sistema.\nNenhuma imagem é gravada.'
                    }
                  >
                    necessária para o áudio do sistema · por quê?
                  </span>
                </div>
              </div>

              <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
                <button
                  className="btn primary"
                  onClick={startRecording}
                  disabled={isRecordingDisabled || permsChecking}
                  style={{ padding: '10px 20px', fontSize: '13.5px' }}
                >
                  <span
                    style={{
                      width: 9,
                      height: 9,
                      borderRadius: '50%',
                      background: 'currentColor',
                      opacity: 0.85,
                    }}
                  />
                  Iniciar gravação
                </button>
                <span className="faint" style={{ fontSize: 12 }}>
                  ou pressione <span className="kbd">⌘</span> <span className="kbd">R</span>
                </span>
              </div>
            </>
          )}

          {/* ===== state: recording ===== */}
          {screen === 'live' && (
            <>
              <div className="live-head">
                <span className="rec-pill">
                  <span className="pulse" />
                  {recordingState.isPaused ? 'PAUSADO' : 'GRAVANDO'}
                </span>
                <span className="elapsed mono">{fmtClock(elapsed)}</span>
                <div
                  className={`meter${recordingState.isPaused ? ' idle' : ''}`}
                  aria-label="Nível de áudio"
                >
                  {bars.map((h, i) => (
                    <i key={i} style={{ height: `${h}%` }} />
                  ))}
                </div>
                <div className="grow" />
                <div className="live-devices">{liveDevicesLabel}</div>
                <button className="btn" onClick={stopRecording}>
                  ■ Parar
                </button>
              </div>

              <div className="live-feed">
                <div className="live-feed-head">
                  Transcrição ao vivo{' '}
                  <span
                    className="faint"
                    style={{ letterSpacing: 0, textTransform: 'none', fontWeight: 500 }}
                  >
                    · falantes são identificados após a gravação
                  </span>
                </div>
                <div className="live-feed-body" ref={transcriptContainerRef}>
                  {transcripts.length === 0 && (
                    <div className="live-feed-empty">
                      Ouvindo… a transcrição aparece aqui conforme a fala é reconhecida.
                    </div>
                  )}
                  {transcripts.map((t) => (
                    <div key={t.id} className="seg no-speaker">
                      <span className="ts">{fmtClock(t.audio_start_time)}</span>
                      <p className="s-text">{t.text}</p>
                    </div>
                  ))}
                  <div className="typing">
                    <i />
                    <i />
                    <i />
                  </div>
                </div>
              </div>
            </>
          )}

          {/* ===== state: processing ===== */}
          {screen === 'proc' && (
            <div className="proc-card">
              {!isCompleted && (
                <span className="spinner" style={{ width: 22, height: 22, borderWidth: '2.5px' }} />
              )}
              <h2>{isCompleted ? 'Transcrição concluída' : 'Finalizando a transcrição…'}</h2>
              <p>
                {isCompleted
                  ? `A reunião foi salva em ${
                      folderPath || 'nesta máquina'
                    }. Identifique os falantes e gere o primeiro resumo na tela da reunião.`
                  : `O áudio está sendo transcrito localmente com o modelo ${modelLabel}. Você já pode fechar esta tela — o processamento continua.`}
              </p>
              <div className="progress">
                <i style={{ width: `${Math.round(progress)}%` }} />
              </div>
              {isCompleted && (
                <button className="btn primary" onClick={openMeeting}>
                  Abrir reunião
                </button>
              )}
            </div>
          )}
        </div>
      </div>

      {/* Modals + recovery (preserved plumbing) */}
      <SettingsModals modals={modals} messages={messages} onClose={hideModal} />
      <TranscriptRecovery
        isOpen={showRecoveryDialog}
        onClose={handleDialogClose}
        recoverableMeetings={recoverableMeetings}
        onRecover={handleRecovery}
        onDelete={deleteRecoverableMeeting}
        onLoadPreview={loadMeetingTranscripts}
      />
    </>
  );
}
