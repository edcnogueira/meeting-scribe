# C3 — UI de settings + validação end-to-end com falantes

**Objetivo:** expor o provedor CLI Agent no modal de settings, documentar, e validar o fluxo completo com reunião **diarizada** — o resumo deve usar os nomes dos falantes (renomeados ou "Speaker N").

**Depende de:** C2. | **Bloqueia:** nada.

## Escopo

### UI

- [x] `services/configService.ts`: `'cli-agent'` no union `ModelConfig['provider']` + métodos get/save/test da config.
- [x] `components/ModelSettingsModal.tsx`: `<SelectItem value="cli-agent">CLI Agent (Codex, Claude Code, Gemini)</SelectItem>`; fora de `requiresApiKey`; bloco condicional novo com seletor de preset (badge "instalado/não encontrado" via `api_test_cli_agent_connection`), campos de comando/args (modo custom), botão **Test**; ramos em `handleSave` e no `onValueChange`.
- [x] `components/SummaryModelSettings.tsx`: excluir `cli-agent` da busca de API key e carregar a config específica (espelho do custom-openai).
- [x] `ConfigContext.tsx` e onboarding: **sem mudanças de lógica** (nenhuma chave sensível; default continua `builtin-ai`). Única alteração: completar o literal `modelOptions: Record<ModelConfig['provider'], string[]>` com a chave `'cli-agent': []` — obrigatório para o `Record` fechar após a extensão do union (senão o `pnpm build` quebra). Sem mudança comportamental.
- [x] Falantes no resumo: conferir que o toggle `summarizeWithSpeakers` (settings de diarização) está descobrível a partir do fluxo de resumo — feito via `Alert` de ajuda no bloco cli-agent do modal, apontando o toggle "Include speakers in summary".

### Docs

- [x] README (do fork): seção na lista de provedores + **nota de privacidade** — o transcript (incluindo nomes de falantes atribuídos) é enviado à CLI escolhida, que fala com o provedor da assinatura; deixar explícito, já que o app se posiciona como privacy-first. (Promovido de "Roadmap" para a tabela "What this fork adds", com a nota de privacidade inline.)

### Validação end-to-end (manual)

> **Pendente: validação manual do usuário (reunião real diarizada).** As caixas abaixo exigem uma reunião real gravada e diarizada + julgamento humano sobre a qualidade do resumo; ficam desmarcadas de propósito para o usuário executar.

- [ ] Reunião real → diarizar → renomear pelo menos um falante (ex.: "Speaker 1 → João"), deixar outro como "Speaker N" → gerar resumo via `codex` **e** via `claude`, com `summarizeWithSpeakers` ligado.
- [ ] Confirmar no resumo: decisões/action items **atribuídos por nome** e nenhuma confusão entre falantes; "Speaker N" tratado como participante legítimo.
- [ ] Repetir com template `daily_standup` e um template custom, em PT (valida os passes de tradução/normalização herdados).
- [ ] Testar "Test connection" com CLI inexistente no PATH e com sessão deslogada (erro acionável na UI).

## Critérios de aceite

- Selecionar preset, testar e salvar funciona no modal; config persiste e sobrevive a restart.
- Resumo end-to-end via CLI usa os nomes da diarização (renomeados e "Speaker N") nos action items.
- Erros de CLI ausente/deslogada aparecem na UI como mensagem acionável, não como resumo vazio.
- README atualizado com o provedor e a nota de privacidade.

## Referências

- Prefixo de falante no transcript do resumo: `frontend/src/hooks/meeting-details/useSummaryGeneration.ts:448-459`; toggle em `frontend/src/lib/diarizationSettings.ts` + `components/DiarizationSettings.tsx` (D5).
- Padrão de UI de provedor sem API key: `builtin-ai` e `custom-openai` em `ModelSettingsModal.tsx` / `SummaryModelSettings.tsx`.
