# Tasks — Fork pessoal do Meetily

Fila de tarefas do fork. Cada arquivo é uma tarefa auto-contida (objetivo, escopo, critérios de aceite, dependências). Executar em ordem, uma branch por tarefa quando fizer sentido, integrando em `feature/diarization`.

Contexto e decisões de design: vault Obsidian → `WIKI/personal/Meetily/Planos/Plano - Diarização e Identificação de Falantes.md`.

## Diarização e Identificação de Falantes

- [x] [D1 — Spike: modelos de diarização (ONNX)](diarization/D1-spike-modelos.md)
- [ ] [D2 — Trilhas separadas mic/system na gravação](diarization/D2-trilhas-separadas.md)
- [ ] [D3 — Engine de diarização + pós-processamento por reunião](diarization/D3-engine-e-pos-processamento.md)
- [ ] [D4 — Identificação: cadastro de pessoas com perfil de voz](diarization/D4-cadastro-de-pessoas.md)
- [ ] [D5 — UI de falantes](diarization/D5-ui-de-falantes.md)

Ordem: D1 e D2 são independentes entre si (podem andar em paralelo); D3 depende das duas; D4 depende de D3; D5 depende de D4.
