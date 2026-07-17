# Tasks — Fork pessoal do Meetily

Fila de tarefas do fork. Cada arquivo é uma tarefa auto-contida (objetivo, escopo, critérios de aceite, dependências). Executar em ordem, uma branch por tarefa quando fizer sentido.

Contexto e decisões de design: vault Obsidian → `WIKI/personal/Meetily/Planos/`.

## Diarização e Identificação de Falantes

- [x] [D1 — Spike: modelos de diarização (ONNX)](diarization/D1-spike-modelos.md)
- [x] [D2 — Trilhas separadas mic/system na gravação](diarization/D2-trilhas-separadas.md)
- [x] [D3 — Engine de diarização + pós-processamento por reunião](diarization/D3-engine-e-pos-processamento.md)
- [x] [D4 — Identificação: cadastro de pessoas com perfil de voz](diarization/D4-cadastro-de-pessoas.md)
- [x] [D5 — UI de falantes](diarization/D5-ui-de-falantes.md)

Ordem: D1 e D2 são independentes entre si (podem andar em paralelo); D3 depende das duas; D4 depende de D3; D5 depende de D4.

## Provedor CLI para Resumos

Plano: `WIKI/personal/Meetily/Planos/Plano - Provedor CLI para Resumos.md`. Resumo via CLI de IA já instalada (`codex exec`, `claude -p`, `gemini -p`) em vez de API key ou modelo local. O transcript enviado ao LLM já chega com os falantes da diarização prefixados (D5, opt-in `summarizeWithSpeakers`) — os resumos devem usar esses nomes ("Speaker N" ou renomeado). A Fase 4 do plano (contribuição upstream) foi cancelada: o fork está destacado, sem PR/issue para o upstream.

- [ ] [C1 — Spike: validar as CLIs de IA (codex/claude/gemini)](cli-summary/C1-spike-clis.md)
- [ ] [C2 — Backend: provedor `cli-agent` (Rust)](cli-summary/C2-backend-provedor-cli.md)
- [ ] [C3 — UI de settings + validação end-to-end com falantes](cli-summary/C3-ui-settings-e-validacao.md)

Ordem: C1 → C2 → C3 (sequencial; branch `enhance/cli-summary-provider` a partir de `main`).
