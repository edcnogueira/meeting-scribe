# D1 — Spike: modelos de diarização (ONNX)

**Objetivo:** escolher e validar o par de modelos (segmentação + embedding de falante) rodando em `ort` (ONNX Runtime, já usado pelo `parakeet_engine/`), com limiares calibrados, antes de escrever qualquer código no app.

**Depende de:** nada. | **Bloqueia:** D3.

## Escopo

- [x] Obter modelos ONNX candidatos: `pyannote/segmentation-3.0` + embedding WeSpeaker/ERes2Net (avaliar também os pares já exportados prontos pelo projeto sherpa-onnx — só os arquivos `.onnx`, sem a lib C++). — escolhido pyannote-segmentation-3.0 + wespeaker_en_voxceleb_resnet34 (exports ONNX não-gated do sherpa-onnx). Ver D1-resultados.md.
- [x] Protótipo Rust standalone (binário à parte, fora do app — ex.: `cargo new` em `scratch/` ou exemplo no workspace): decodificar um `audio.mp4` real de reunião (48kHz mono → 16kHz) → VAD/segmentação → embeddings por segmento → clustering agglomerative por cosseno, com e sem nº de falantes fixo. — `scratch/diarization-spike/` (roda em `ort`; clustering nas duas variantes: corte por limiar e nº fixo).
- [x] Testar também o cenário de **trilhas separadas** (ver D2): diarizar só a trilha do system audio e medir se a acurácia melhora vs. o mono mixado. — trilha separada 100% vs. mono com overlap 94.2%; recomendação de separar as trilhas registrada.
- [~] Medir em reunião real em PT: qualidade da separação (conferência manual) e tempo de processamento no M1. — tempo medido no M1 (RTF ≤ 0.022). Sem reunião real disponível; validado com áudio sintético de verdade conhecida (limitação registrada; reconfirmar acurácia em reunião real em D3).
- [x] Validar identificação: gravar 2 reuniões com as mesmas pessoas e medir se os clusters da segunda casam com embeddings médios da primeira (similaridade de cosseno). — cross-sessão (enroll mix_seq → test mono_overlap): mesma pessoa 0.975–0.993, diferentes ≤ 0.53.
- [x] Verificar licença dos pesos escolhidos (pyannote é gated no HF; WeSpeaker/3D-Speaker são alternativas de licença livre). Para uso pessoal qualquer um serve — registrar mesmo assim. — export ONNX do pyannote é MIT e não-gated; wespeaker Apache-2.0. Nenhum token HF necessário.

## Critérios de aceite

- Par de modelos escolhido e documentado, com URLs de download dos `.onnx`.
- Limiares definidos: corte de clustering e limiar de identificação (cosseno).
- Tempo de processamento ≲ 15% da duração do áudio no M1 (referência: whisply fez 51min → ~7min).
- Conclusão registrada sobre mono mixado vs. trilha separada (alimenta D3).

## Referências

- Padrão de sessões/download ONNX: `frontend/src-tauri/src/parakeet_engine/{model.rs, parakeet_engine.rs}` (`ort = 2.0.0-rc.10`).
- Código morto com design pyannote anterior (inspiração de API, não reativar): `frontend/src-tauri/src/audio/stt.rs`.
- Decodificação de áudio existente: `frontend/src-tauri/src/audio/decoder.rs` (`decode_audio_file`, `to_whisper_format`).
