# D5 — UI de falantes

**Objetivo:** expor a diarização na interface: falantes visíveis no transcript, correção/renomeação fácil (que alimenta o enrollment do D4) e configurações.

**Depende de:** D4.

## Escopo

- [ ] **Transcript**: label + cor por falante em cada segmento (`TranscriptView.tsx`, `VirtualizedTranscriptView.tsx`, painéis de meeting details); tooltip com o score do match. Campo `speaker` chega via tipos existentes (`TranscriptUpdate`/`Transcript` em `types/index.ts`) como opcional — segmentos sem falante renderizam como hoje.
- [ ] **Painel da reunião**: lista de falantes detectados; renomear (→ `api_rename_meeting_speaker`, D4); reatribuir cluster errado; campo "nº de participantes remotos" (dica de clusters); botão "Diarizar/Re-diarizar" com barra de progresso (evento `diarization-progress`).
- [ ] **Settings** (`TranscriptSettings.tsx`): toggle "Speaker diarization" (+ auto ao salvar), toggle "Save separate tracks" (D2), download/status do modelo (padrão do `ParakeetModelManager`), gestão do cadastro de pessoas (listar/renomear/mesclar/apagar).
- [ ] **Resumo**: prefixar cada fala com o nome do falante no transcript enviado ao LLM (opt-in) — melhora action items por pessoa.

## Critérios de aceite

- Fluxo completo sem tocar em terminal: gravar → diarizar (auto ou botão) → ver falantes coloridos → renomear "Speaker 1 → João" → próxima reunião já mostra "João".
- Reunião sem diarização renderiza exatamente como hoje (zero regressão visual).
- Progresso e erros da diarização visíveis (toast/estado no painel), com cancelamento.

## Referências

- `frontend/src/components/{TranscriptView.tsx, VirtualizedTranscriptView.tsx, TranscriptSettings.tsx, MeetingDetails/}`, `frontend/src/types/index.ts`, `frontend/src/contexts/TranscriptContext.tsx`.
- Padrão de UI de gestão de modelo local: `frontend/src/components/BuiltInModelManager.tsx` / `ParakeetModelManager`.
