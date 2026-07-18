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

- [x] [C1 — Spike: validar as CLIs de IA (codex/claude/gemini)](cli-summary/C1-spike-clis.md)
- [x] [C2 — Backend: provedor `cli-agent` (Rust)](cli-summary/C2-backend-provedor-cli.md)
- [x] [C3 — UI de settings + validação end-to-end com falantes](cli-summary/C3-ui-settings-e-validacao.md) — implementado; pendente validação manual E2E (reunião real diarizada)

Ordem: C1 → C2 → C3 (sequencial). Executado em 2026-07-17 (PRs #1–#3, squash na `main`).

## Organização no Meeting Notes

Pastas de reuniões espelhando o disco (cada pasta do app = diretório real, gestão pelo app) e título automático `YYYY-MM-DD - <assunto>` extraído do H1 que o LLM já gera no resumo.

- [x] [O1 — Pastas de reuniões no app (espelho real do Finder)](organization/O1-pastas-de-reunioes.md)
- [x] [O2 — Título personalizado: data + assunto específico](organization/O2-titulo-com-data-e-assunto.md)

Ordem: O1 ∥ O2 (paralelo). Executado em 2026-07-17 (PRs #7–#9, squash na `main`).

## Redesign da UI (Claude Design, 2026-07)

Export do design em `docs/design/redesign-2026-07/` (telas HTML + `css/tokens.css` + `js/app.js` — tratar como contrato visual, ver `DESIGN-HANDOFF.md`). Tema claro/escuro por `data-theme`, tokens oklch, 12 cores estáveis de falante, sidebar em árvore espelhando o disco.

- [x] [R1 — Fundação: tokens, componentes compartilhados e shell/sidebar](redesign/R1-fundacao-tokens-componentes-shell.md)
- [x] [R2 — Tela "Nova reunião" (setup → gravando → processando)](redesign/R2-tela-nova-reuniao.md)
- [x] [R3 — Tela de detalhes da reunião (todos os estados)](redesign/R3-tela-detalhes-da-reuniao.md)
- [x] [R4 — Tela de Configurações](redesign/R4-tela-configuracoes.md)

Ordem: R1 primeiro; depois R2 ∥ R3 ∥ R4 (paralelo). Executado em 2026-07-17 (PRs #12–#16, squash na `main`).
