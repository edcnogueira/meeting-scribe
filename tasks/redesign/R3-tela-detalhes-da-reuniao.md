# R3 — Tela de detalhes da reunião (transcrição · falantes · resumo, com todos os estados)

**Objetivo:** reconstruir a tela central conforme `meeting.html` **e** `meeting-empty.html` — são a mesma tela em estados diferentes (populada vs. recém-gravada): transcrição com chips, painel de falantes com confiança/rename, e painel de resumo com estados vazio/gerando/erro de CLI/sucesso.

**Depende de:** R1. | **Bloqueia:** nada (∥ R2, R4).

## Escopo

### Layout e header

- [ ] Grid `transcrição (1fr) + rail (384px)`; rail = painel Falantes (altura fixa) + painel Resumo (flex).
- [ ] Breadcrumbs `pasta / título`; título provisório ("Gravação de …") em itálico + badge `warn` "aguardando resumo" com tooltip "o título definitivo vem do H1 do primeiro resumo" (O2). Botão "Revelar no Finder".

### Painel de transcrição

- [ ] Meta fixa: duração, nº de falantes (ou "falantes não identificados"), data/hora de gravação, botão "Copiar transcrição" (formato `[hh:mm:ss] Nome: texto`, feedback "Copiado ✓").
- [ ] Segmentos: timestamp mono + chip de falante colorido com tooltip de confiança ("Correspondência de voz: N% · M amostras" / "Voz nova — renomeie para ensiná-la") + texto. Variante `no-speaker` (sem diarização). Manter a virtualização existente para reuniões longas.

### Painel de falantes

- [ ] Linhas: chip (`data-c`, selo VOCÊ), input de renomear inline (renomear ensina o registro — atualiza chips da transcrição e confiança na hora), % de confiança (`low` em warn quando < 60).
- [ ] Rodapé: campo "Participantes remotos" (numérico, tooltip explicando o agrupamento do áudio do sistema) + hint "renomear ensina o registro de vozes" / "roda 100% no dispositivo".
- [ ] Estado vazio: "Ninguém foi identificado ainda" + explicação + botão "Identificar falantes".
- [ ] Progresso de (re)diarização in-panel: 6 etapas (Decodificando áudio → … → Salvando) com done/now, barra de progresso e "Cancelar" — ligado ao progresso real da engine (D3/D5).
- [ ] Resultado degenerado: banner `warn` "190 falantes detectados — resultado improvável" com explicação e botão "Informar participantes" (foca e destaca o campo remoto); lista com scroll interno que nunca quebra o layout; meta da transcrição vira "N falantes (?)".

### Painel de resumo

- [ ] Cabeçalho: copiar como Markdown, exportar `.md`. Toolbar: select de template (padrão / Daily Standup / personalizados) + "Regenerar" (ou "Gerar resumo" no vazio).
- [ ] Corpo em `article.md`: H1 + `h1-meta` (provider, data/hora, nota do título), seções, ações como `task` com checkbox e `owner` destacado.
- [ ] Campo fixo "Contexto para a IA" (persiste por reunião, entra no prompt).
- [ ] Estados: **vazio** ("O primeiro resumo também dá o título definitivo à reunião") → **gerando** (spinner + rótulo do provider, barra indeterminada) → **erro de CLI acionável** (banner `danger`: "A sessão do Codex CLI expirou.", code-hint `$ codex login`, botões "Tentar novamente" e "Trocar provider" → settings; cobrir também CLI não instalada e timeout — C3) → **sucesso** (título do H1 aplicado ao header, à sidebar e ao arquivo — remove badge/pending-dot).

## Critérios de aceite

- Fluxo real: reunião recém-gravada abre no estado vazio → diarizar (etapas reais, cancelável) → renomear falante reflete em transcrição+registro → gerar resumo → título definitivo propagado (header, sidebar, disco).
- Erro de sessão expirada do CLI aparece como banner acionável com o comando literal, e o retry funciona.
- Caso degenerado (dezenas/centenas de falantes) não quebra o layout e guia o usuário ao campo de participantes remotos.
- Visual idêntico a `meeting.html`/`meeting-empty.html` nos dois temas.

## Referências

- Design: `docs/design/redesign-2026-07/meeting.html` e `meeting-empty.html`.
- Código atual: `frontend/src/app/meeting-details/`, `components/TranscriptView.tsx` + `VirtualizedTranscriptView.tsx`, `components/SpeakerChip.tsx`, `components/AISummary/`, `components/EmptyStateSummary.tsx`, `hooks/meeting-details/useSummaryGeneration.ts`; diarização D3–D5; título O2; erros de CLI C2/C3.
