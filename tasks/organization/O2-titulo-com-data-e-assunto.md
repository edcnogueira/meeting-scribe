# O2 — Título personalizado: data + assunto específico da reunião

**Objetivo:** ao gerar o resumo, trocar o título genérico da reunião (`Meeting DD_MM_YY_HH_MM_SS`) por um título útil no formato **`YYYY-MM-DD - <assunto específico>`** (ex.: `2026-07-17 - Alinhamento do provedor CLI`), extraído do próprio resumo.

**Depende de:** nada. | **Bloqueia:** nada. (O1 ∥ O2)

## Por que é barato

O template de resumo **já pede um título ao LLM**: o esqueleto markdown começa com `# <Add Title here>` (`Template::to_markdown_structure`, `summary/templates/types.rs`) e o LLM preenche o H1. Hoje esse H1 é simplesmente ignorado — o título da reunião continua o timestamp gerado no início da gravação (`frontend/src/hooks/useRecordingStart.ts:41`). A task é aproveitar o que já existe.

## Escopo

- [x] **Prompt**: reforçar a instrução do título no prompt de geração (`summary/processor.rs:154`): o H1 deve ser um título curto e específico do assunto tratado (decisão/tema central), no idioma do transcript — nunca genérico tipo "Meeting Summary" ou o nome do template. Vale para todos os provedores (cli-agent incluso, nada específico dele).
- [x] **Extração pós-geração** (backend, no fluxo que persiste o resumo): parsear o primeiro `# H1` do markdown final (após `clean_llm_markdown_output`); fallback se ausente/vazio: manter título atual.
- [x] **Montagem**: `YYYY-MM-DD - <título>` usando a **data de criação da reunião** (`meetings.created_at`), não a data de geração do resumo; truncar título a ~80 chars.
- [x] **Guarda de edição manual**: só renomear se o título atual ainda casa com o padrão auto-gerado (`Meeting \d{2}_\d{2}_\d{2}.*` e variantes do `generateMeetingTitle`) **ou** já está no formato `YYYY-MM-DD - ...` (re-geração de resumo pode atualizar o assunto). Título editado à mão pelo usuário nunca é sobrescrito.
- [x] **Persistência + UI**: atualizar `meetings.title` pelo repositório existente e emitir o evento que a sidebar já usa para refletir rename (verificar o fluxo atual de rename manual e reusar).
- [x] **Fora de escopo (v1)**: renomear a pasta da reunião no disco (o vínculo é `folder_path`; título do DB é livre — evita conflito com O1); passo de LLM dedicado só para título; toggle em settings.
- [x] Testes unitários: extração do H1 (com ruído, sem H1, H1 genérico), montagem com data correta, guarda de título manual.

## Critérios de aceite

- Reunião com título auto-gerado + resumo gerado → título vira `YYYY-MM-DD - <assunto>` com a data da reunião e assunto específico do transcript.
- Reunião renomeada manualmente → gerar resumo **não** altera o título.
- Re-gerar resumo atualiza o assunto mantendo o formato.
- Funciona com template built-in e custom, em PT, com qualquer provedor.
- `cargo test`/`clippy` e `pnpm build` limpos.

## Referências

- Esqueleto com H1: `summary/templates/types.rs` (`to_markdown_structure`); prompt: `summary/processor.rs:150-170`.
- Título auto-gerado no start: `frontend/src/hooks/useRecordingStart.ts:22,41`.
- Limpeza do markdown: `summary/processor.rs` (`clean_llm_markdown_output`).
- Persistência do resumo (ponto de integração da extração): `summary/service.rs` (fluxo `process_transcript_background`).
