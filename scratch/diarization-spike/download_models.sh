#!/usr/bin/env bash
# Download the ONNX diarization models used by the spike (not committed to git).
# Models are non-gated, redistributed by the sherpa-onnx project (k2-fsa).
set -euo pipefail
cd "$(dirname "$0")"
mkdir -p models
cd models

SEG="https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-segmentation-models"
EMB="https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models"

if [ ! -d sherpa-onnx-pyannote-segmentation-3-0 ]; then
  echo "Downloading pyannote segmentation-3.0 (ONNX)..."
  curl -sSL -o seg.tar.bz2 "$SEG/sherpa-onnx-pyannote-segmentation-3-0.tar.bz2"
  tar xjf seg.tar.bz2
  rm -f seg.tar.bz2
fi

if [ ! -f wespeaker_en_voxceleb_resnet34.onnx ]; then
  echo "Downloading wespeaker en voxceleb resnet34 (ONNX)..."
  curl -sSL -o wespeaker_en_voxceleb_resnet34.onnx "$EMB/wespeaker_en_voxceleb_resnet34.onnx"
fi

echo "Done. Models in $(pwd)"
ls -la
