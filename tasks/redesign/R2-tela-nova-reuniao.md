# R2 — Tela "Nova reunião" (setup → gravando → processando)

**Objetivo:** reconstruir a tela de gravação conforme `recording.html`: coluna única calma com máquina de estados setup → ao vivo → processamento, ligada aos comandos/eventos Tauri reais de gravação e transcrição.

**Depende de:** R1. | **Bloqueia:** nada (∥ R3, R4).

## Escopo

### Estado 1 — Setup ("Pronto para gravar")

- [ ] Título + promessa de privacidade ("Áudio e transcrição ficam nesta máquina…").
- [ ] Card de dispositivos: select de microfone (dispositivos reais) e de áudio do sistema ("Capturar áudio do sistema (ScreenCaptureKit)" / "Não capturar").
- [ ] Aviso Bluetooth (banner `warn`): aparece quando o microfone escolhido é fone Bluetooth — texto do perfil SCO 8 kHz, recomendação de usar mic interno (reaproveitar a detecção do `BluetoothPlaybackWarning` atual).
- [ ] Linha de permissões: badges "✓ Microfone permitido" / "✓ Gravação de tela permitida" + tooltip "necessária para o áudio do sistema · por quê?" (nenhuma imagem é gravada). Estados negativos devem oferecer a ação de permitir.
- [ ] Botão primário "Iniciar gravação" + atalho ⌘R (funcional, com `kbd` no hint).

### Estado 2 — Gravando

- [ ] Cabeçalho ao vivo: `rec-pill` pulsando, cronômetro mono `mm:ss`, `meter` de nível real (7 barras), rótulo dos dispositivos em uso, botão "■ Parar".
- [ ] Badge "● REC" no header da janela enquanto grava.
- [ ] Transcrição ao vivo: feed com timestamps, animação de entrada por segmento, indicador "digitando" (3 pontos), auto-scroll; nota fixa "falantes são identificados após a gravação".

### Estado 3 — Processando

- [ ] Card centrado: spinner, "Finalizando a transcrição…", subtexto com o modelo Whisper em uso e aviso de que dá para fechar a tela (processamento continua), barra de progresso real.
- [ ] Badge "Processando" no header. Ao concluir: título "Transcrição concluída", subtexto com a pasta de destino, botão "Abrir reunião" → tela da reunião (R3).

## Critérios de aceite

- Fluxo real completo: selecionar dispositivos → gravar (⌘R ou botão) → transcrição ao vivo aparecendo → parar → processamento → abrir a reunião criada.
- Aviso Bluetooth aparece/some conforme o dispositivo; permissões refletem o estado real do macOS.
- Visual idêntico a `recording.html` nos dois temas.

## Referências

- Design: `docs/design/redesign-2026-07/recording.html`.
- Código atual: `frontend/src/app/page.tsx`, `components/RecordingControls.tsx`, `components/DeviceSelection.tsx`, `components/AudioLevelMeter.tsx`, `components/BluetoothPlaybackWarning.tsx`, `components/PermissionWarning.tsx`, eventos `transcript-update`.
