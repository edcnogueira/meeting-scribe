# R1 — Fundação do redesign: tokens, componentes compartilhados e shell (sidebar)

**Objetivo:** portar o sistema visual do design (`docs/design/redesign-2026-07/`) para o app — tokens oklch com tema claro/escuro, biblioteca de componentes base e o shell compartilhado (sidebar em árvore de pastas) — para que R2/R3/R4 componham telas apenas com peças prontas.

**Depende de:** nada. | **Bloqueia:** R2, R3, R4.

**Fonte da verdade:** `css/tokens.css`, `css/app.css`, `js/app.js` e `components.html` (folha de espécimes nos dois temas). O handoff manda tratar o export como contrato visual: igualar pixels e comportamento primeiro, refatorar depois.

## Escopo

### Tokens (`css/tokens.css` → globals.css/Tailwind)

- [ ] Portar todas as variáveis: `--bg/--surface/--surface-2/--hover/--fg/--muted/--faint/--border/--border-strong`, acento (`--accent*`), gravação (`--rec*`), `--warn*`, `--ok*`, `--danger*`, sombras, `--scrim`, raios (`--r-s/m/l`), fontes (ui/display/mono), `--sidebar-w: 264px`, `--read-width: 68ch`.
- [ ] Tema por atributo `data-theme="light|dark"` no root (não `prefers-color-scheme` apenas), com persistência da escolha e toggle. Substituir o theming atual do app por este.
- [ ] Paleta de falantes: 12 matizes estáveis via `data-c="1..12"` + tokens `--spk-bg/fg-l/c` por tema. Integrar ao `SpeakerChip.tsx` existente (cor estável por falante, reuso em chips, dots e acentos).

### Componentes base (espécimes em `components.html`)

- [ ] Botões `btn` (default/primary/ghost/danger/small/disabled), `icon-btn`, `kbd`, `spinner`.
- [ ] `badge` (ok/warn/danger/accent/`rec` ● REC) e `rec-pill` com pulso.
- [ ] `banner` (informativa, `warn` privacidade, `danger` erro acionável, `accent`) com `b-actions`.
- [ ] Progresso: barra determinada/indeterminada, `stages` (etapas com done/now), `meter` de nível de áudio (7 barras).
- [ ] Chip de falante: cor por `data-c`, estado `unknown`, selo `VOCÊ`, tooltip de confiança (`data-tip` multilinha).
- [ ] Inputs: `input`, `field` + `label` + `hint`, switch, radio com `accent-color`.
- [ ] Menu de contexto `ctx` — incluindo item desabilitado com explicação inline (`.why`).
- [ ] Modal/overlay (fechar por ✕/Esc/clique no scrim) e o padrão de modal destrutivo (botão `danger`).
- [ ] Estilos de Markdown `article.md`: `h1-meta`, listas, `task` com checkbox e `owner` destacado.
- [ ] Estado vazio (`empty`: ícone, título, texto, ação primária).

### Shell / sidebar (`js/app.js` + `css/app.css`)

- [ ] Sidebar 264px com titlebar (traffic lights), botão **Nova reunião** (→ tela de gravação), rótulo "Reuniões" com botão **Sincronizar** (refresh, espelha o disco — O1), árvore de pastas com estado aberto/fechado persistido, item ativo, e rodapé (engrenagem → settings, toggle de tema, recolher).
- [ ] Reunião sem título definitivo: nome provisório em itálico + `pending-dot` com tooltip "Aguardando primeiro resumo" (integra com O2).
- [ ] Colapso da sidebar ⇄ botão de mostrar no header da tela (padrão `js-show-sidebar` de todas as telas).
- [ ] Menu de contexto da árvore — pasta: Nova pasta / Renomear / Excluir (desabilitado com explicação quando não-vazia); reunião: Renomear / Mover para pasta… / Revelar no Finder / Excluir.
- [ ] Renomear inline (Enter confirma, Esc cancela, blur confirma) e modal "Mover para pasta" com árvore de destino (move no disco — O1).
- [ ] Header padrão das telas: breadcrumbs `pasta / título`, ações à direita (ex.: "Revelar no Finder").

## Critérios de aceite

- `components.html` reproduzido no app: cada espécime idêntico nos dois temas (comparação lado a lado).
- Toggle de tema persiste entre sessões e troca todas as telas sem flash incorreto.
- Sidebar espelha as pastas reais (O1), com contexto/rename/move funcionais e colapso restaurável.
- Nenhum componente novo usa cor/tipografia fora dos tokens; sem regressão de layout nos viewports do manifesto (1024×768 → 1920×1080; sem overflow horizontal).

## Referências

- Design: `docs/design/redesign-2026-07/{css/tokens.css,css/app.css,js/app.js,components.html}` + `DESIGN-HANDOFF.md`.
- Código atual: `frontend/src/app/globals.css`, `frontend/src/components/Sidebar/`, `frontend/src/components/SpeakerChip.tsx`, `frontend/src/components/ui/`.
- Pastas/título automático já implementados: tarefas O1 e O2 (`tasks/organization/`).
