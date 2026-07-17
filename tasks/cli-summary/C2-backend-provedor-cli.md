# C2 — Backend: provedor `cli-agent` (Rust)

**Objetivo:** implementar o provedor de resumo **CLI Agent** completo no core Rust — variante do enum ao comando Tauri — espelhando os precedentes BuiltInAI (early-return para subprocesso) e CustomOpenAIConfig (config JSON em coluna de settings). Tudo aditivo: nenhum provedor existente muda, templates intactos.

**Depende de:** C1 (registry de presets). | **Bloqueia:** C3.

Branch: `enhance/cli-summary-provider` a partir de `main` (convenção do fork — não existe mais `devtest`).

## Escopo

- [x] `summary/llm_client.rs`: variante `LLMProvider::CliAgent`, braço `"cli-agent"` no `from_str` (linha ~87), early-return em `generate_summary` **antes** do match HTTP (espelho do BuiltInAI, linha ~135), braço em `provider_name`.
- [x] Novo módulo `summary/cli_agent/`:
  - `client.rs` — `generate_with_cli_agent(config, system_prompt, user_prompt, cancellation_token) -> Result<String>`; system + user prompt concatenados num prompt único com separador claro (preset do Claude pode usar `--append-system-prompt`).
  - `process.rs` — spawn one-shot com `tokio::process::Command` **sem shell** (args como array, `sh -c` proibido), prompt via **stdin**, captura de stdout, `tokio::time::timeout` + `child.kill()` no `CancellationToken` (via `tokio::select!`), `CREATE_NO_WINDOW` no Windows.
  - Registry de presets (`codex`, `claude`, `gemini` + custom) com os dados validados em C1.
  - Sanitização de stdout (strip de logs residuais) antes de devolver — `clean_llm_markdown_output` existente cobre o resto.
  - Sessão expirada/sem login: detectar exit code/mensagem (tabela C1) e devolver erro acionável (ex.: "run `codex login`") em vez de resumo vazio.
- [x] `summary/mod.rs`: `pub mod cli_agent;` + struct `CliAgentConfig { preset, command, args, timeout_secs }` (espelho de `CustomOpenAIConfig`).
- [x] `summary/service.rs`: incluir `CliAgent` no grupo sem api_key; `token_threshold` fixo 100k no braço de single-pass (linha ~394-436); resolver `CliAgentConfig` antes de `generate_meeting_summary`.
- [x] `summary/processor.rs`: garantir que `CliAgent` caia no caminho single-pass. Sem outras mudanças — prompts/templates são genéricos e o prefixo de falante já chega pronto do frontend.
- [x] Migration `add_cli_agent_config.sql` (coluna `cliAgentConfig TEXT`) + campo em `Setting` (`database/models.rs`) + `get/save_cli_agent_config` e braços `"cli-agent" => Ok(())`/`Ok(None)` de api_key em `database/repositories/setting.rs` (espelho de `builtin-ai`, linhas ~88/126/252).
- [x] `api/api.rs`: `api_get_cli_agent_config`, `api_save_cli_agent_config`, `api_test_cli_agent_connection` (roda `<cli> --version` e opcionalmente prompt trivial) — **registrar no `generate_handler!` em `lib.rs`**.
- [x] Segurança: comando configurável só via comando Tauri dedicado — **nunca** execução arbitrária exposta ao JS.
- [x] Testes unitários com **binário fake** (script que ecoa stdin) — nunca depender de `codex`/`claude` reais: sucesso, timeout, exit code ≠ 0, cancelamento, stdout com ruído.

## Critérios de aceite

- Resumo gerado end-to-end via provedor `cli-agent` (configurado à mão no SQLite, sem UI) numa reunião real.
- Anti-exemplo do upstream evitado: nenhum ponto órfão — enum, dispatch, service, processor, migration, repositório e comandos Tauri todos ligados (checklist acima completo).
- `cargo test`, `cargo clippy` e `cargo check` limpos em `frontend/src-tauri` (lembrar do pré-requisito `binaries/llama-helper-<triple>` antes do build).
- Cancelamento mata o processo filho (sem CLI órfã em background).

## Referências

- Precedente de subprocesso: `summary/summary_engine/{sidecar.rs, client.rs}` (BuiltInAI) — padrão de timeout/kill; aqui é one-shot, sem keep-alive.
- Precedente de config JSON: `CustomOpenAIConfig` em `summary/mod.rs` + `setting.rs:277` (`get_custom_openai_config`).
- Decisões de design e riscos: plano no vault (§ Decisões de design, § Riscos).
