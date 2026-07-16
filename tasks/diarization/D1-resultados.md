# D1 — Resultados: spike de modelos de diarização (ONNX)

Spike executado num protótipo Rust standalone rodando na crate `ort` (mesma do
app), fora do workspace do repo: `scratch/diarization-spike/`. Todos os números
abaixo vêm de execuções reais no M1 (CPU, `CPUExecutionProvider`), não de estimativa.

## TL;DR

- **Par de modelos escolhido:** `pyannote/segmentation-3.0` (segmentação/VAD, powerset)
  + `wespeaker_en_voxceleb_resnet34` (embedding de falante, 256-d) — ambos exports
  ONNX **públicos e não-gated** redistribuídos pelo projeto sherpa-onnx.
- **Roda em `ort` 2.0.0-rc.10** exatamente no mesmo padrão do `parakeet_engine/`
  (sessão CPU, `inputs![...]`, `try_extract_array`). Sem lib C++ do sherpa.
- **Tempo:** RTF (tempo / duração do áudio) **0.009–0.022**, ou seja **≤ 2.2%** da
  duração — muito abaixo do alvo de 15%. ~45 s de áudio processados em ~1 s.
- **Qualidade:** 99.5% de acerto no mix mono sequencial (3 falantes), 94.2% no mono
  com sobreposição, 100% nas trilhas separadas.
- **Trilha separada > mono mixado** quando há fala sobreposta (ver seção dedicada).
- **Identificação cross-sessão funciona:** mesma pessoa em gravações diferentes casa
  com similaridade de cosseno 0.975–0.993, contra ≤ 0.53 para pessoas diferentes.

## Modelos escolhidos + URLs de download

Baixados por `scratch/diarization-spike/download_models.sh` (não versionados no git).

| Papel | Modelo | Arquivo | I/O ONNX |
|---|---|---|---|
| Segmentação / VAD | pyannote-segmentation-3.0 | `sherpa-onnx-pyannote-segmentation-3-0/model.onnx` (5.9 MB; há `model.int8.onnx` de 1.5 MB) | in `x`[N,1,T] waveform 16k; out `y`[N,frames,7] powerset. `num_speakers=3`, `powerset_max_classes=2`, `window_size=160000` (10 s), `receptive_field_shift=270` |
| Embedding de falante | wespeaker en voxceleb resnet34 | `wespeaker_en_voxceleb_resnet34.onnx` (26.5 MB) | in `feats`[B,T,80] fbank kaldi; out `embs`[B,256]. `sample_rate=16000`, `normalize_samples=0` |

URLs (GitHub Releases do k2-fsa/sherpa-onnx, não-gated):

```
# Segmentação (tar.bz2 contém model.onnx + model.int8.onnx + LICENSE)
https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-segmentation-models/sherpa-onnx-pyannote-segmentation-3-0.tar.bz2

# Embedding de falante
https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/wespeaker_en_voxceleb_resnet34.onnx
```

Alternativas testáveis pelo mesmo pipeline (mesma release de embedding, só trocar o
arquivo): `3dspeaker_speech_eres2net_base_sv_zh-cn_3dspeaker_16k.onnx`,
`wespeaker_en_voxceleb_CAM++.onnx`, `nemo_en_titanet_small.onnx`. Todas verificadas
como URLs válidas (HTTP 200). Ficou-se no wespeaker resnet34 por ser inglês,
robusto e com margem de separação excelente já na 1ª tentativa.

### Licença dos pesos

- **pyannote/segmentation-3.0**: MIT no export ONNX do sherpa (arquivo `LICENSE` no
  tar). O checkpoint original no HF é *gated* (exige aceitar termos + token), mas o
  **export ONNX redistribuído pelo sherpa-onnx é público e não exige token HF** —
  foi esse que se usou. Uso pessoal: OK.
- **wespeaker voxceleb resnet34**: licença Apache-2.0 (WeSpeaker). Uso livre.

Nenhum modelo gated foi necessário.

## Pipeline implementado (Rust puro, `scratch/diarization-spike/src/main.rs`)

1. WAV 16 kHz mono → `hound`.
2. Segmentação em janelas de 10 s (160 000 amostras), passo não-sobreposto, última
   janela com zero-padding. Saída powerset [frames,7] → argmax por frame → conjunto
   de falantes locais ativos (mapeamento powerset pyannote: `{}`, `{0}`, `{1}`, `{2}`,
   `{0,1}`, `{0,2}`, `{1,2}`).
3. Runs contíguos por falante local (bridge de gaps ≤ 250 ms, descarte < 400 ms) →
   regiões de fala.
4. Por região: fbank kaldi 80-d implementado à mão (janela povey, pré-ênfase 0.97,
   remoção de DC, potência, log, CMN) — compatível com o pré-processamento wespeaker
   → modelo de embedding → vetor 256-d L2-normalizado. Embeddings extraídos só das
   frames **exclusivas** (um único falante ativo) para não contaminar com overlap.
5. Clustering **agglomerative average-linkage por distância de cosseno**, implementado
   à mão, em duas variantes: (a) corte por limiar; (b) nº de clusters fixo (quando se
   conhece a contagem de falantes).
6. Scoring frame-a-frame (100 ms) contra ground truth, com melhor mapeamento
   cluster→falante (permutação exaustiva, ≤ 3 falantes).

O áudio de teste é **sintético e com verdade conhecida** (`make_audio.py`): não havia
reunião real gravada em `~/Library/Application Support/Meetily/` (só `notifications.json`),
então gerou-se conversa multi-falante com o `say` do macOS usando 3 vozes distintas
(Daniel = masc en_GB, Luciana = fem pt_BR, Fred = masc en_US sintético). Como nós
mesmos posicionamos cada fala, sabemos exatamente as trocas de falante.

## Limiares calibrados

Varredura real de limiar (coluna "auto" = clustering por limiar; "fixedk" = nº fixo):

| Limiar (dist. cosseno) | mix_seq (3 falantes) | mono_overlap (3) | track_system (2) |
|---|---|---|---|
| 0.35 | 3 clusters, 99.5% | 3, 94.2% | 2, 100% |
| 0.45 | 3, 99.5% | 3, 94.2% | 2, 100% |
| **0.50** | **3, 99.5%** | **3, 94.2%** | **2, 100%** |
| 0.60 | 3, 99.5% | 3, 94.2% | 2, 100% |
| 0.70 | 2, 73.0% (funde falantes) | 2, 69.7% | 2, 100% |

- **Corte de clustering (recomendado): distância de cosseno = 0.50** (equivale a fundir
  regiões com similaridade de cosseno ≥ 0.50). **Faixa válida estável: 0.35–0.60.**
  Abaixo de ~0.35 arrisca dividir um falante em dois clusters (menor sim. intra-falante
  observada ≈ 0.654 → dist. 0.346); acima de ~0.60 começa a fundir falantes distintos.
- **Limiar de identificação (mesma pessoa entre sessões): similaridade de cosseno ≥ 0.65.**
  Observado: mesma pessoa 0.975–0.993; pessoas diferentes ≤ 0.53. Margem enorme; 0.65 é
  conservador e seguro.

Estatísticas de separabilidade dos embeddings (rotuladas pela verdade):

| Cenário | sim. intra (média/mín) | sim. inter (média/máx) |
|---|---|---|
| mix_seq | 0.881 / 0.795 | 0.134 / 0.426 |
| mono_overlap | 0.850 / 0.671 | 0.170 / **0.533** |
| track_system | 0.835 / 0.713 | 0.029 / **0.099** |

## Tempo de processamento (M1, CPU)

| Cenário | Duração | Tempo proc. | RTF |
|---|---|---|---|
| mix_seq | 44.9 s | ~0.9 s | **0.020** |
| mono_overlap | 42.4 s | ~0.8 s | 0.019 |
| track_system | 42.4 s | ~0.6 s | 0.013 |
| track_mic | 42.4 s | ~0.4 s | 0.009 |

RTF ≤ 0.022 = **≤ 2.2% da duração**, contra o alvo de ≲ 15% (whisply: 51 min → ~7 min ≈
14%). Extrapolando linearmente, uma reunião de 51 min diarizaria em ~1 min só de
diarização (soma-se ao tempo de transcrição, que é o gargalo real). Tudo em CPU; sobra
folga enorme. Nota: são modelos pequenos (segmentação 5.9 MB, embedding 26.5 MB).

## Conclusão: mono mixado vs. trilha separada (alimenta D3)

- **Mono mixado, sem sobreposição** (mix_seq): 99.5%. Ótimo.
- **Mono mixado, com sobreposição** (mono_overlap): **94.2%**. A fala sobreposta é onde
  o mono perde: a similaridade inter-falante máxima sobe para **0.533** (contra 0.426 no
  caso limpo) porque frames de overlap contaminam os embeddings, e a segmentação tem de
  resolver 2 falantes simultâneos no powerset.
- **Trilha separada** (só a trilha "system", 2 falantes remotos isolados): **100%**, e a
  similaridade inter-falante máxima **despenca para 0.099** — separação praticamente
  perfeita. A trilha "mic" (falante local) vira trivialmente 1 falante (100%).

**Recomendação para D3:** diarizar a **trilha do system audio separada da trilha do mic**.
Isso (1) elimina o cross-talk mic↔system que degrada o mono, (2) reduz o nº de falantes a
desambiguar por trilha (o falante local já está isolado na trilha do mic), e (3) amplia
drasticamente a margem de separação dos embeddings. O falante do mic pode ser rotulado
diretamente como "Eu/local" sem sequer passar por clustering. O ganho de ~6 p.p. medido
aqui é num overlap sintético moderado; em reuniões reais com mais cross-talk o ganho
tende a ser maior.

## Limitações / o que não foi validado plenamente

- **Sem reunião real longa em PT.** Não havia gravação real do Meetily instalado; usou-se
  áudio sintético (`say`) com verdade conhecida. Vozes sintéticas são mais "limpas" e
  provavelmente mais fáceis de separar que fala humana real com ruído/reverberação —
  os números de acurácia são um teto otimista. A viabilidade (modelos rodam em `ort`,
  RTF baixíssimo, limiares bem definidos) está sólida; a acurácia absoluta precisa de
  reconfirmação numa reunião real em D3.
- **Janelas de segmentação não-sobrepostas.** O pipeline usa janelas de 10 s sem overlap
  (o pyannote de produção usa janelas deslizantes com stitching). Simplificação aceitável
  para o spike; a re-identificação entre janelas é resolvida pelo clustering global de
  embeddings. Em D3 vale usar overlap + costura para robustez nas bordas.
- **fbank kaldi reimplementado à mão.** Compatível o suficiente para dar embeddings bem
  separados (validado pelas estatísticas intra/inter), mas não bit-a-bit igual ao
  torchaudio/kaldi. Em produção, manter esse fbank sob teste ou considerar reutilizar o
  featurizer que o app já tiver.
- **Overlap sintético moderado.** O cenário de sobreposição é construído (2 falantes
  somados); reuniões reais podem ter overlap mais severo.

## Como reproduzir

```bash
cd scratch/diarization-spike
./download_models.sh          # baixa os .onnx (não versionados)
python3 make_audio.py         # gera áudio sintético + ground_truth.json
cargo build --release
./target/release/diarize .    # roda todos os cenários e escreve results.json
# varredura de limiar:
CLUSTER_THRESHOLD=0.5 ./target/release/diarize .
```
