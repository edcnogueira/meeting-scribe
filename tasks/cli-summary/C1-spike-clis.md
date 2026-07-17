# C1 — Spike: validar as CLIs de IA (codex/claude/gemini)

**Objetivo:** validar na prática o modo não-interativo exato de cada CLI candidata e produzir a tabela preset → comando/args/stdin/limpeza de stdout que vira o registry de presets no código. Nada entra no app antes disso.

**Depende de:** nada. | **Bloqueia:** C2.

## Escopo

- [x] **Codex**: `codex exec` — stdout já sai limpo (só a resposta final; preâmbulo/logs no stderr); prompt via stdin com `-`; modelo via `-m`. Opcional `-o <arquivo>` grava só a última mensagem. Ver `C1-resultados.md`.
- [x] **Claude Code**: `claude -p --output-format text` — prompt via stdin, stdout limpo; `--append-system-prompt` existe e funciona.
- [~] **Gemini CLI**: `gemini -p` — **não validado (CLI não instalada)**. Preset documentado da doc oficial (`gemini -p -`, `--output-format text`, `-m/--model`) em `C1-resultados.md`.
- [x] Testar com **transcript diarizado**: `[~]` transcript **sintético** (não reunião real) prefixado `[MM:SS] Nome: fala`, misturando renomeados e "Speaker N". Confirmado: codex e claude atribuem action items por falante pelo nome.
- [x] Medir latência e definir timeout default: 20–27 s (13 KB) / 22–24 s (118 KB). **Timeout default: 600s.**
- [x] Prompt grande via stdin: 118 KB (>100k) — claude 22 s, codex 24 s, sem truncar; ambos referenciam a decisão colocada no fim.
- [x] Comportamento sem login/sessão expirada: `[~]` **não reproduzido (sem logout)**. Probe `codex login status` (exit 0 = ok); fallback = detectar exit ≠ 0 + stderr.

## Critérios de aceite

- Tabela registrada neste diretório (`C1-resultados.md`): por preset — binário, args, forma de passar prompt, flags de saída limpa, seleção de modelo, comportamento sem auth, latência medida.
- Timeout default definido com base nas medições.
- Confirmação explícita de que o resumo gerado por cada CLI referencia os falantes pelo nome quando o transcript vem prefixado.

## Referências

- Plano: vault Obsidian → `WIKI/personal/Meetily/Planos/Plano - Provedor CLI para Resumos.md` (§ Fase 0, § Riscos).
- Prefixo de falante no transcript enviado ao LLM: `frontend/src/hooks/meeting-details/useSummaryGeneration.ts:448-459` (D5, opt-in via `summarizeWithSpeakers` em `frontend/src/lib/diarizationSettings.ts`).
- Contrato de saída do LLM é markdown puro; pós-processamento só limpa `<think>`/cercas: `frontend/src-tauri/src/summary/processor.rs` (`clean_llm_markdown_output`).
