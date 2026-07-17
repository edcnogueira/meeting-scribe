# O1 — Pastas de reuniões no app (espelho real do Finder)

**Objetivo:** permitir organizar reuniões em pastas por projeto/empresa dentro do Meeting Notes, onde **cada pasta do app é um diretório real no disco** e vice-versa — o app mostra a mesma organização que o Finder, mas tudo é gerenciável pelo app.

**Depende de:** nada. | **Bloqueia:** nada. (O1 ∥ O2)

## Decisões de design

- **Filesystem é a fonte de verdade da árvore.** Pasta de organização = diretório real sob a base de gravações (`appDataDir`). Nenhuma tabela nova na v1.
- **`meetings.folder_path` (já existe, migration `20251006000000_add_audio_sync_fields.sql`) é o vínculo** reunião ↔ localização. Mover reunião = `fs::rename` do diretório da reunião + `UPDATE folder_path`.
- **Distinção pasta-de-organização vs. pasta-de-reunião:** diretório referenciado por algum `folder_path` no DB (ou contendo artefatos `audio.mp4`/`transcripts.json`) é reunião; o resto é organização. Dotfiles (`.checkpoints`) ignorados.
- Ordem das operações: **fs primeiro, DB depois**; se o UPDATE falhar, desfazer o rename (best-effort) e reportar erro — nunca deixar DB apontando para pasta inexistente.

## Escopo

### Backend (Rust)

- [ ] Comandos Tauri (registrar no `generate_handler!`):
  - `api_list_meeting_folder_tree` — escaneia a base recursivamente e devolve a árvore (pastas de organização + reuniões vinculadas por `folder_path`; reuniões na raiz = grupo "Unfiled").
  - `api_create_meeting_folder(parent_path?, name)` — aninhamento permitido; sanitizar com a mesma `sanitize_filename` de `audio/audio_processing.rs`; erro em colisão.
  - `api_rename_meeting_folder(path, new_name)` — `fs::rename` + UPDATE de **prefixo** em `folder_path` de todas as reuniões abaixo (uma transação).
  - `api_delete_meeting_folder(path)` — **só pasta vazia** na v1 (sem reuniões nem subpastas); erro claro caso contrário.
  - `api_move_meeting_to_folder(meeting_id, target_folder_path?)` — `None`/raiz = Unfiled; validar colisão de nome no destino.
- [ ] Segurança de caminho: toda operação valida que o path resolvido está **dentro da base de gravações** (canonicalize + starts_with) — nunca aceitar path arbitrário do JS.
- [ ] Mudanças externas (Finder): re-scan sob demanda (o tree é lido do disco a cada `api_list_meeting_folder_tree`); reunião cujo diretório sumiu → item marcado `missing` na resposta (não crashar, não deletar do DB).
- [ ] Reuniões legadas (`folder_path` NULL ou na raiz): aparecem em "Unfiled", sem migração automática.
- [ ] Testes unitários com diretório temporário: criar/renomear/mover/deletar, colisão, pasta não-vazia, prefixo de `folder_path` atualizado, path fora da base rejeitado.

### Frontend

- [ ] `SidebarProvider.tsx` (~linha 116): substituir o nó único plano "Meeting Notes" pela árvore do `api_list_meeting_folder_tree` — pastas expansíveis/colapsáveis (persistir estado expandido em localStorage), reuniões como folhas, "Unfiled" no final.
- [ ] Gestão por menu de contexto: Nova pasta (raiz e em pasta), Renomear, Excluir (desabilitado se não-vazia, com tooltip), e "Mover para pasta..." na reunião (dialog com a árvore). Drag & drop fica **fora da v1**.
- [ ] Botão/ação de refresh da árvore (cobre mudanças feitas no Finder).
- [ ] Item `missing` com indicação visual e sem navegação quebrada.

## Critérios de aceite

- Criar pasta "Setare/ProjetoX" no app → diretórios reais aparecem no Finder.
- Mover uma reunião para a pasta → o diretório da reunião muda de lugar no disco, `folder_path` atualiza, playback/transcript/diarização continuam funcionando (paths internos são relativos à pasta da reunião).
- Renomear pasta com N reuniões dentro → todas continuam abrindo (folder_path re-prefixado).
- Criar/mover pasta pelo Finder → após refresh o app mostra a mesma árvore.
- Reuniões antigas seguem acessíveis em "Unfiled".
- `cargo test`/`clippy`/`check` e `pnpm build` limpos.

## Referências

- Criação da pasta de reunião: `frontend/src-tauri/src/audio/audio_processing.rs:35` (`create_meeting_folder`, `sanitize_filename`).
- Coluna `folder_path`: `migrations/20251006000000_add_audio_sync_fields.sql`; modelo em `database/models.rs`, repositório de meetings em `database/repositories/`.
- Sidebar plano atual: `frontend/src/components/Sidebar/SidebarProvider.tsx:116-125`.
- Precedente de comando com validação de path e progress: `audio/retranscription.rs`.
