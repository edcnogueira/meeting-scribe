'use client';

import { useRef } from 'react';
import { getSpeakerColorIndex } from '@/lib/speakerColors';
import {
  DIARIZATION_STAGES,
  MeetingSpeakerInfo,
  UseDiarizationResult,
} from '@/hooks/meeting-details/useDiarization';

interface SpeakersPanelProps {
  diar: UseDiarizationResult;
}

function confTip(diar: UseDiarizationResult, s: MeetingSpeakerInfo): string {
  const samples = diar.sampleCountFor(s);
  if (samples > 0 && s.score !== null) {
    return `Correspondência de voz: ${Math.round(s.score * 100)}%\n${samples} amostras no registro`;
  }
  return 'Voz nova — sem correspondência no registro.\nRenomeie para começar a ensiná-la.';
}

/**
 * Redesigned meeting speakers rail (task R3): renameable speaker rows with
 * colored chips + confidence, a remote-participants field, empty state, in-panel
 * staged (re-)diarization progress with cancel, and the degenerate "too many
 * speakers" warning that guides the user to the remote-count field. Wired to the
 * real diarization engine through `useDiarization`.
 */
export function SpeakersPanel({ diar }: SpeakersPanelProps) {
  const remoteRef = useRef<HTMLInputElement>(null);
  const hasSpeakers = diar.speakers.length > 0;

  const focusRemote = () => {
    const el = remoteRef.current;
    if (!el) return;
    el.classList.add('attention');
    el.focus();
    setTimeout(() => el.classList.remove('attention'), 2400);
  };

  return (
    <section className="panel" id="panel-speakers" aria-label="Falantes">
      <div className="panel-head">
        <h2>Falantes</h2>
        {hasSpeakers && !diar.diarizing && (
          <button type="button" className="btn small" onClick={diar.startDiarize}>
            Rediarizar
          </button>
        )}
      </div>

      {diar.diarizing ? (
        <div className="diar-progress">
          <div className="stages">
            {DIARIZATION_STAGES.map((label, i) => (
              <div
                key={label}
                className={`stage ${i < diar.stageIndex ? 'done' : i === diar.stageIndex ? 'now' : ''}`}
              >
                <span className="s-ico">✓</span>
                {label}
              </div>
            ))}
          </div>
          <div className="progress">
            <i style={{ width: `${diar.progressPercent}%` }} />
          </div>
          <div className="diar-actions">
            <button type="button" className="btn small ghost" onClick={diar.cancelDiarize}>
              Cancelar
            </button>
          </div>
        </div>
      ) : (
        <>
          {diar.isDegenerate && (
            <div className="pad-banner">
              <div className="banner warn">
                <span className="b-ico">⚠</span>
                <div className="b-body">
                  <strong>{diar.speakers.length} falantes detectados — resultado improvável.</strong>
                  <br />
                  Vozes vindas do áudio do sistema chegam numa faixa única e podem fragmentar o
                  agrupamento. Informe quantos <strong>participantes remotos</strong> havia na chamada e
                  rediarize.
                  <div className="b-actions">
                    <button type="button" className="btn small" onClick={focusRemote}>
                      Informar participantes
                    </button>
                  </div>
                </div>
              </div>
            </div>
          )}

          {hasSpeakers ? (
            <div className="spk-list">
              {diar.speakers.map((s) => {
                const pct = s.score !== null ? Math.round(s.score * 100) : null;
                return (
                  <div className="spk-row" key={s.cluster_label}>
                    <span className="spk" data-c={getSpeakerColorIndex(s.display_name)} data-tip={confTip(diar, s)}>
                      <span className="spk-dot" style={{ background: 'currentColor' }} />
                      {s.display_name}
                      {s.is_self && <span className="you">VOCÊ</span>}
                    </span>
                    <span className="name">
                      <input
                        key={`${s.cluster_label}:${s.display_name}`}
                        defaultValue={s.display_name}
                        aria-label="Renomear falante"
                        onKeyDown={(e) => {
                          if (e.key === 'Enter') (e.target as HTMLInputElement).blur();
                          if (e.key === 'Escape') {
                            (e.target as HTMLInputElement).value = s.display_name;
                            (e.target as HTMLInputElement).blur();
                          }
                        }}
                        onBlur={(e) => {
                          const v = e.target.value.trim();
                          if (v && v !== s.display_name) diar.renameSpeaker(s.cluster_label, v);
                          else e.target.value = s.display_name;
                        }}
                      />
                    </span>
                    <span className={`conf${pct !== null && pct < 60 ? ' low' : ''}`}>
                      {pct !== null ? `${pct}%` : '—'}
                    </span>
                  </div>
                );
              })}
            </div>
          ) : (
            <div className="empty">
              <span className="e-ico">
                <svg width="20" height="20" viewBox="0 0 16 16" fill="none">
                  <rect x="5.6" y="1.8" width="4.8" height="8" rx="2.4" stroke="currentColor" strokeWidth="1.2" />
                  <path d="M3.2 7.6a4.8 4.8 0 009.6 0M8 12.4v2" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" />
                </svg>
              </span>
              <h3>Ninguém foi identificado ainda</h3>
              <p>
                A diarização roda no seu Mac e separa quem disse o quê. Vozes conhecidas do registro são
                reconhecidas automaticamente.
              </p>
              <button type="button" className="btn primary" onClick={diar.startDiarize}>
                Identificar falantes
              </button>
            </div>
          )}

          <div className="spk-foot">
            <span className="remote-hint">
              Participantes remotos
              <input
                ref={remoteRef}
                type="text"
                inputMode="numeric"
                value={diar.numRemote}
                placeholder="?"
                onChange={(e) => diar.setNumRemote(e.target.value.replace(/[^0-9]/g, ''))}
                data-tip={'Quantas pessoas estavam do outro\nlado da chamada (áudio do sistema)'}
              />
            </span>
            <span className="grow" />
            <span className="faint">
              {hasSpeakers ? 'renomear ensina o registro de vozes' : 'roda 100% no dispositivo'}
            </span>
          </div>
        </>
      )}
    </section>
  );
}
