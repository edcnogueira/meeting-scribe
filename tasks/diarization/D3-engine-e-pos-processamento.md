# D3 — Engine de diarização + pós-processamento por reunião

**Objetivo:** módulo `diarization_engine/` (ONNX via `ort`, modelos do D1) e o comando `api_diarize_meeting` que diariza uma reunião salva e grava o falante em cada segmento do transcript.

**Depende de:** D1 (modelos/limiares), D2 (trilhas). | **Bloqueia:** D4.

## Escopo

- [ ] `diarization_engine/` espelhando o `parakeet_engine/`: `model.rs` (sessões `ort`), `engine.rs` (download manager HF multi-arquivo com progress/`ModelStatus`), `commands.rs` (`static DIARIZATION_ENGINE: Mutex<Option<Arc<...>>>` + comandos init/status/download registrados no `generate_handler!` de `lib.rs`).
- [ ] API central: `diarize(samples_16k, num_speakers: Option<usize>) -> Vec<SpeakerTurn { start, end, cluster_id, embedding }>` — clustering agglomerative por cosseno; `num_speakers` fixa o corte.
- [ ] Comando `api_diarize_meeting(meeting_id, num_remote_speakers?)` no padrão do `audio/retranscription.rs`:
  - **Com trilhas (D2)**: `mic.mp4` → VAD apenas → turnos rotulados "Eu" (sem modelo); `system.mp4` → `diarize()` → clusters "Speaker N". Merge das duas timelines por timestamp.
  - **Fallback (reuniões antigas/importadas)**: diarizar o `audio.mp4` mono mixado inteiro.
  - Atribuição: casar turnos com os segmentos transcritos por overlap de `audio_start_time`/`audio_end_time`.
- [ ] Persistência: cabear a coluna `transcripts.speaker` (já existe no schema, órfã — adicionar ao struct `Transcript`, INSERT/SELECT no `TranscriptsRepository`); migration nova com tabelas `speakers` (id, name, embeddings BLOB) e `meeting_speakers` (meeting_id, cluster, speaker_id?, score); `SpeakersRepository`.
- [ ] Evento `diarization-progress` (stages: decoding/segmenting/embedding/clustering/matching/saving) espelhando `RetranscriptionProgress`; job em background, nunca bloquear a UI.
- [ ] Gatilho automático ao salvar a reunião (toggle em settings) + invocável manualmente (UI vem em D5; testar via comando Tauri direto).
- [ ] Nomenclatura: tipos novos como `Diarization*`/`SpeakerIdentity` — "speaker" sozinho já significa alto-falante no codebase (`devices/speakers.rs`).

## Critérios de aceite

- Reunião gravada com trilhas → todos os segmentos do transcript ganham `speaker` ("Eu" ou "Speaker N"); trocas de falante batem com o áudio na conferência manual.
- Reunião antiga (só `audio.mp4`) → fallback mono funciona e popula `speaker`.
- `num_remote_speakers` informado altera o resultado (fixa clusters).
- Progresso visível via evento; cancelável; reunião longa não trava a UI.
- `cargo test` com testes unitários do clustering/atribuição por overlap (fixtures sintéticas, sem modelo real no CI).

## Referências

- `frontend/src-tauri/src/audio/retranscription.rs` (padrão decode→lote→salvar+progress), `parakeet_engine/` (padrão ort+download), `database/{models.rs, repositories/transcript.rs}`, `migrations/20251110000001_add_speaker_field.sql` (coluna órfã), `audio/decoder.rs`.
