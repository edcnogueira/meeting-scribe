# C3 — UI de settings + validação end-to-end com falantes

**Objetivo:** expor o provedor CLI Agent no modal de settings, documentar, e validar o fluxo completo com reunião **diarizada** — o resumo deve usar os nomes dos falantes (renomeados ou "Speaker N").

**Depende de:** C2. | **Bloqueia:** nada.

## Escopo

### UI

- [ ] `services/configService.ts`: `'cli-agent'` no union `ModelConfig['provider']` + métodos get/save/test da config.
- [ ] `components/ModelSettingsModal.tsx`: `<SelectItem value="cli-agent">CLI Agent (Codex, Claude Code, Gemini)</SelectItem>`; fora de `requiresApiKey`; bloco condicional novo com seletor de preset (badge "instalado/não encontrado" via `api_test_cli_agent_connection`), campos de comando/args (modo custom), botão **Test**; ramos em `handleSave` e no `onValueChange`.
- [ ] `components/SummaryModelSettings.tsx`: excluir `cli-agent` da busca de API key e carregar a config específica (espelho do custom-openai).
- [ ] `ConfigContext.tsx` e onboarding: **sem mudanças** (não há chave sensível; default continua `builtin-ai`).
- [ ] Falantes no resumo: conferir que o toggle `summarizeWithSpeakers` (settings de diarização) está descobrível a partir do fluxo de resumo — se a reunião tem diarização e o toggle está off, o usuário precisa entender por que os nomes não aparecem (mínimo: texto de ajuda; nada de UI nova além disso).

### Docs

- [ ] README (do fork): seção na lista de provedores + **nota de privacidade** — o transcript (incluindo nomes de falantes atribuídos) é enviado à CLI escolhida, que fala com o provedor da assinatura; deixar explícito, já que o app se posiciona como privacy-first.

### Validação end-to-end (manual)

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
