# D4 — Identificação: cadastro de pessoas com perfil de voz

**Objetivo:** transformar clusters anônimos em nomes: cadastro local de pessoas (embeddings de referência no SQLite) com matching automático e enrollment pelo uso.

**Depende de:** D3. | **Bloqueia:** D5.

## Escopo

- [ ] Matching pós-clustering dentro do `api_diarize_meeting`: para cada cluster do system audio, comparar o embedding médio contra o cadastro (`speakers`) por cosseno; acima do limiar (D1) → nome da pessoa; abaixo → "Speaker N". Score persistido em `meeting_speakers`.
- [ ] Enrollment por renomeação: comando `api_rename_meeting_speaker(meeting_id, cluster, name)` — cria/atualiza a pessoa e incorpora os embeddings do cluster ao perfil (média incremental com cap de amostras); reatribui os segmentos da reunião.
- [ ] Perfil "Eu": a trilha do mic alimenta automaticamente o perfil do dono da máquina (primeiro cadastro, sem fricção) — útil no fallback mono, onde "Eu" também vira matching.
- [ ] Gestão via comandos Tauri: listar/renomear/mesclar/apagar pessoas (apagar = remover embeddings/biometria; atribuições históricas viram texto simples).
- [ ] Privacidade: embeddings só no SQLite local; nada em telemetria/logs.

## Critérios de aceite

- Gravar reunião A, renomear "Speaker 1 → João"; gravar reunião B com o João → ele é identificado automaticamente com score acima do limiar.
- Falso positivo controlado: pessoa desconhecida não é rotulada com nome de cadastrado (limiar conservador).
- Mesclar e apagar pessoas funciona sem corromper atribuições antigas.
- Testes unitários do matching/média incremental (embeddings sintéticos).

## Referências

- Tabelas e repositório criados em D3 (`speakers`, `meeting_speakers`, `SpeakersRepository`).
- Limiar de identificação calibrado em D1.
