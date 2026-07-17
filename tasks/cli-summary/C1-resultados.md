# C1 — Resultados: spike das CLIs de IA (codex / claude / gemini)

Spike executado no macOS (M-series), invocando cada CLI como **subprocesso não-interativo**
com transcript sintético diarizado passado por **stdin**. Todos os números de `codex` e
`claude` vêm de execuções reais; `gemini` **não foi validado** (CLI não instalada) e está
documentado só a partir da doc oficial.

## TL;DR

- **Todas as três CLIs lêem o prompt via stdin e escrevem markdown limpo no stdout.** Os
  logs/preâmbulo do `codex` vão para **stderr**, então o stdout já sai limpo sem flag extra.
- **`codex` e `claude` validados de verdade.** `gemini` **não validado — CLI não instalada**;
  preset abaixo veio da doc oficial (`google-gemini/gemini-cli`).
- **Awareness de falantes confirmado nos dois validados:** os resumos atribuem action items
  por falante pelo nome (`Maria:`, `Speaker 2:`, `Speaker 3:`, `João:`) exatamente como
  aparecem no transcript prefixado.
- **stdin grande (>100k chars) sem truncar:** `claude` processou 118 KB em 22 s e `codex` em
  24 s, ambos referenciando a decisão colocada **no fim** do transcript (`Jabuticaba-9`,
  `30 de setembro`).
- **Latência medida:** 20–27 s para transcript de ~13 KB; 22–24 s para ~118 KB. **Timeout
  default recomendado: 600 s** (folga enorme sobre o p50 medido; cobre cold start, reasoning
  alto e transcripts de reunião longa).

## Tabela de presets

| Item | codex (validado) | claude (validado) | gemini (**não validado — CLI não instalada**) |
|---|---|---|---|
| Binário | `codex` (`codex-cli 0.144.1`) | `claude` (`2.1.212`, Claude Code) | `gemini` |
| Subcomando / modo | `codex exec` | `claude -p` / `--print` | `gemini -p` / `--prompt` (headless) |
| Args recomendados | `codex exec --color never -s read-only --skip-git-repo-check -` | `claude -p --output-format text` | `gemini -p - --output-format text` |
| Prompt via stdin | Sim. `-` (ou nenhum prompt) → lê instruções do stdin. Se houver prompt E stdin, o stdin vira bloco `<stdin>` anexado | Sim. Com `-p` e stdin pipado, o stdin é o prompt (`--input-format text`, default) | Sim. Doc: em headless, se input é pipado/redirecionado, entra como prompt; combina com `-p "instrução"` |
| Saída limpa (stdout) | **Já limpo** — só a mensagem final vai ao stdout; versão/model/session/logs vão ao **stderr**. Opcional: `-o <arquivo>` grava só a última mensagem | **Já limpo** — `--output-format text` imprime só o texto final; stderr vazio | `--output-format text` (default) imprime só a resposta; `json`/`stream-json` para parsing |
| Seleção de modelo | `-m <model>` / `--model` (default observado: `gpt-5.5`) | `--model <model>` (usa o configurado por padrão) | `-m` / `--model <model>` |
| Refino de system prompt | via `-c` (config) / instrução no próprio prompt | `--append-system-prompt <txt>` (e `--system-prompt`) — **existe e funciona** | `--append-system-prompt` (doc); refino também via `GEMINI.md` |
| Comportamento sem auth | Probe: `codex login status` → exit 0 + "Logged in using ChatGPT" quando OK. Sessão expirada esperada: exit ≠ 0 no `exec` + mensagem no stderr (**não reproduzido — não fizemos logout**) | Falha de auth esperada: exit ≠ 0 + mensagem no stderr (**não reproduzido — não fizemos logout**) | Idem — detectar por exit ≠ 0 + stderr (**não reproduzido; CLI não instalada**) |
| Latência medida (~13 KB) | 27 s | 20 s | n/d |
| Latência medida (~118 KB) | 24 s | 22 s | n/d |

## Detalhes de saída limpa

`codex exec` já separa canais: o **stdout** contém somente o markdown final; todo o
preâmbulo (`OpenAI Codex v...`, `workdir`, `model`, `session id`, e o eco do prompt) e os
logs vão para o **stderr**. Ou seja, o provedor C2 pode capturar `stdout` diretamente. Como
alternativa mais explícita, `-o <arquivo>` grava exatamente a última mensagem do agente num
arquivo (conteúdo idêntico ao stdout no teste).

`claude -p --output-format text` imprime apenas o texto da resposta no stdout (stderr vazio
no teste). Nenhum pós-processamento foi necessário além do que o app já faz
(`clean_llm_markdown_output` em `frontend/src-tauri/src/summary/processor.rs`, que remove
`<think>` e cercas de código).

## Awareness de falantes (critério de aceite)

O transcript foi prefixado no formato do app — `[MM:SS] Falante: fala` (espelha
`useSummaryGeneration.ts:448-459`) — misturando falantes renomeados (`João`, `Maria`) e
anônimos (`Speaker 2`, `Speaker 3`). Confirmado nos dois CLIs validados:

- **codex** — action items atribuídos: `Maria: Fechar os testes de timeout...`,
  `Speaker 2: Alterar a janela de agregação...`, `Speaker 3: Escrever três variações...`.
- **claude** — idem, inclusive `João:` e a atribuição da decisão final
  (`Maria` no deploy, `Speaker 2` de plantão para rollback).

Ambos usaram os nomes **exatamente** como no transcript, sem inventar falantes.

## Teste de stdin grande (>100k chars)

Transcript de **118 109 chars** (~118 KB, 898 linhas de diálogo + bloco de decisões no fim)
passado por stdin:

| CLI | Latência | Truncou? | Referenciou o fim (`Jabuticaba-9` / `30 de setembro`)? |
|---|---|---|---|
| claude | 22 s | Não | Sim (ambos os marcadores) |
| codex | 24 s | Não | Sim (ambos os marcadores) |

O marcador de decisão foi colocado **deliberadamente no final** do transcript; os dois CLIs
o reproduziram no resumo, confirmando que o stdin foi consumido inteiro (sem truncamento de
buffer). `claude` foi o mais rápido, então foi o CLI primário do teste grande; `codex` foi
rodado em seguida para confirmação cruzada.

## Timeout default

Latências medidas ficaram entre **20 s e 27 s** em todos os cenários (13 KB e 118 KB). Mesmo
assim, recomenda-se **timeout default de 600 s (10 min)**: reuniões reais podem ser bem mais
longas que o transcript sintético, o `codex` roda com `reasoning effort: xhigh` por padrão
(mais lento em casos difíceis), e há custo de cold start no primeiro spawn. 600 s dá margem
sem travar a UI indefinidamente. Toda invocação de CLI neste spike foi limitada por um
wrapper de timeout (`scratch/cli-spike/timeout.sh`, via `perl alarm`, já que o macóS base não
traz o `timeout` do coreutils).

## Detecção de sessão expirada / sem auth (alimenta C2)

- **Não reproduzido** — o spike **não fez logout** de nenhuma sessão (regra do run).
- `codex login status` é um **probe barato**: retorna exit `0` + `Logged in using ChatGPT`
  quando autenticado. C2 pode usá-lo para validação prévia e para o erro acionável
  "run `codex login`".
- Estratégia de fallback para todas as CLIs: **detectar por exit code ≠ 0 + conteúdo do
  stderr**. O provedor C2 deve tratar exit ≠ 0 como falha e propagar o stderr como mensagem
  acionável (ex.: "sessão expirada — rode `codex login` / `claude` / `gemini`").

## Limitações / o que não foi validado

- **Transcript sintético**, não uma reunião real. O conteúdo e a diarização foram gerados
  (`scratch/cli-spike/make_transcripts.py`); vozes/erros reais de transcrição podem mudar a
  qualidade do resumo, mas não o contrato de invocação (stdin/stdout/flags), que é o alvo do
  spike.
- **`gemini` não validado** — a CLI não está instalada nesta máquina. O preset da coluna
  gemini vem da doc oficial (`/google-gemini/gemini-cli`) e precisa ser reconfirmado em C2/C3
  quando a CLI estiver disponível.
- **Sessão expirada não reproduzida** — por decisão de não fazer logout. Só a estratégia de
  detecção (exit code + stderr, e o probe `codex login status`) foi registrada.
- Modelos default variam por ambiente (ex.: `codex` usou `gpt-5.5`); o preset deve permitir
  override de modelo via `-m`/`--model`.

## Como reproduzir

```bash
cd scratch/cli-spike
python3 make_transcripts.py                       # gera transcript_normal.txt (~13 KB) e transcript_large.txt (~118 KB)
cat summary_instruction.txt transcript_normal.txt > prompt_normal.txt

# codex (stdout já limpo; preâmbulo vai pro stderr)
sh timeout.sh 300 codex exec --color never -s read-only --skip-git-repo-check - < prompt_normal.txt

# claude (texto puro no stdout)
sh timeout.sh 300 claude -p --output-format text < prompt_normal.txt

# teste de stdin grande
cat summary_instruction.txt transcript_large.txt > prompt_large.txt
sh timeout.sh 300 claude -p --output-format text < prompt_large.txt
```
