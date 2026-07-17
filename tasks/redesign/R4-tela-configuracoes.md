# R4 — Tela de Configurações (transcrição · resumo · diarização · gravação)

**Objetivo:** substituir o modal/abas atuais de settings pela tela conforme `settings.html`: navegação lateral própria com 4 seções, ligada às configs reais (Whisper, providers, diarização/registro de vozes, pasta de gravação).

**Depende de:** R1. | **Bloqueia:** nada (∥ R2, R3).

## Escopo

### Estrutura

- [ ] Rota de settings com nav lateral (Transcrição / Modelo de resumo / Diarização / Gravação) e coluna de conteúdo 640px; header com badge "sem telemetria · sem conta · sem nuvem".

### Transcrição (modelos Whisper)

- [ ] Card com uma linha por modelo: nome + descrição/tamanho; estados — **ativo** (badge ok + badge "Metal · GPU"), **baixando** (% mono + barra + Cancelar), **disponível** (botão Baixar). Cancelar → volta a Baixar; concluir → "✓ baixado".
- [ ] Banner informativo de aceleração detectada (Metal/GPU) conforme o hardware real.

### Modelo de resumo (providers)

- [ ] Card-lista com radio por provider: IA integrada (badge "recomendado"), Ollama (badge "local"), Claude/OpenAI/Groq/OpenRouter (badge `warn` "nuvem", detalhe expansível com campo de API key `password` + hint "Guardada no Keychain do macOS"), Endpoint personalizado (campo URL), CLI Agent.
- [ ] Detalhe do CLI Agent: banner `warn` de privacidade (transcrito sai da máquina, inclusive nomes de falantes se ativo na Diarização); presets Codex / Claude Code / Gemini com comando mono e badge **"✓ instalado" / "não encontrado"** via detecção real (`api_test_cli_agent_connection` — C2/C3); comando personalizado; botão "Testar conexão" com estados executando (spinner + comando) e sucesso ("Resposta recebida em N s — o CLI está autenticado e pronto") / erro acionável.

### Diarização + registro de vozes

- [ ] Toggles: "Identificar falantes" (automático pós-gravação), "Nomes de falantes nos resumos" (aviso de que entram no prompt inclusive em providers de nuvem); linha do modelo de embeddings com badge de download.
- [ ] Registro de vozes: uma linha por pessoa — chip colorido (VOCÊ no próprio), contagem de amostras + barra de amostras (10 células), ações Renomear / Mesclar… / Excluir (vermelho).
- [ ] Modal destrutivo de exclusão: "Excluir a voz de X?" com aviso explícito de dados **biométricos** (embeddings e amostras apagados; transcrições existentes não mudam) e botão `danger` "Excluir dados de voz".

### Gravação

- [ ] Pasta das reuniões: chip mono com o caminho + "Alterar…" (a sidebar espelha esta pasta — O1); linha "Estrutura de cada reunião" (`audio.m4a · transcript.json · summary.md · meta.json`) + "Revelar no Finder".
- [ ] Banner: mover/renomear no Finder é seguro — usar **Sincronizar** na sidebar.

## Critérios de aceite

- Todas as seções operam sobre estado real (download de modelo com progresso/cancelamento, teste de CLI, toggles persistidos, registro de vozes editável) — sem placeholders.
- Excluir voz exige o modal e remove de fato os dados; Mesclar mantido funcional como hoje (D4/D5).
- Nenhuma regressão nas configs existentes que a tela substitui (mapear tudo que `SettingTabs`/`ModelSettingsModal` cobre hoje e migrar ou justificar a remoção).
- Visual idêntico a `settings.html` nos dois temas.

## Referências

- Design: `docs/design/redesign-2026-07/settings.html`.
- Código atual: `frontend/src/app/settings/`, `components/SettingTabs.tsx`, `components/WhisperModelManager.tsx` + `BuiltInModelManager.tsx`, `components/ModelSettingsModal.tsx`, `components/SummaryModelSettings.tsx`, `components/DiarizationSettings.tsx`, `components/SpeakerIdentityManager.tsx`, `components/RecordingSettings.tsx`; provider CLI C2/C3; pastas O1.
