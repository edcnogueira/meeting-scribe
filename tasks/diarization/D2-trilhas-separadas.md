# D2 — Trilhas separadas mic/system na gravação

**Objetivo:** persistir, além do `audio.mp4` mono mixado atual (que continua existindo para playback e retranscrição), **duas trilhas separadas** por reunião: `mic.mp4` (= "Eu", 1 falante determinístico) e `system.mp4` (= remotos, único áudio que precisa de clustering). Isso eleva a assertividade da diarização: metade do problema (quem é "Eu") é resolvida sem modelo.

**Depende de:** nada. | **Bloqueia:** D3.

## Escopo

- [ ] Ponto de interceptação: no `AudioPipeline` (`audio/pipeline.rs`), o `AudioMixerRingBuffer` mantém `mic_buffer` e `system_buffer` separados até a mixagem — as janelas de 600ms extraídas por device (`extract_window`) devem ser roteadas também para gravação por trilha, **antes** do `ProfessionalAudioMixer::mix_window()`. Não alterar o caminho mixado existente (gravação e transcrição ao vivo seguem idênticos).
- [ ] Persistência: duas instâncias adicionais do padrão `IncrementalAudioSaver` (`audio/incremental_saver.rs`) — checkpoints de 30s + finalize via FFmpeg — gerando `mic.mp4` e `system.mp4` na mesma pasta da reunião, alinhados no tempo com o mix (mesmo relógio de `timestamp` relativo ao início da gravação; janelas com zero-padding preservam o alinhamento).
- [ ] Toggle em settings ("Save separate tracks", default ligado no fork) para poder desativar e economizar disco.
- [ ] `RecordingSaver`/evento `recording-saved`: incluir os caminhos das trilhas no payload/`transcripts.json` metadata (aditivo, `Option`).
- [ ] Reuniões antigas e áudio importado não têm trilhas — nada a fazer aqui (D3 trata o fallback para mono).

## Critérios de aceite

- Gravar uma reunião de teste → pasta contém `audio.mp4` + `mic.mp4` + `system.mp4`, mesma duração (±1 janela), reproduzíveis.
- Transcrição ao vivo, mixagem e playback seguem funcionando exatamente como antes (zero regressão no caminho existente).
- Trilhas sobrevivem a parada abrupta tão bem quanto o mix (checkpoints incrementais).
- Diff concentrado: idealmente só `pipeline.rs` (roteamento) + saver novo + settings — minimizar toque em arquivos quentes do upstream.

## Referências

- `frontend/src-tauri/src/audio/pipeline.rs` — `AudioMixerRingBuffer` (buffers por device), `mix_window()`, ponto onde `device_type` é descartado (~linha 849).
- `frontend/src-tauri/src/audio/{incremental_saver.rs, recording_saver.rs, recording_state.rs}` (`AudioChunk.device_type`).
- Decisão de design herdada do Meeting Scribe: mic = "Eu" determinístico; só o system audio passa por diarização.
