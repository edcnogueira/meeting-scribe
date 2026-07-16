#!/usr/bin/env python3
"""Generate synthetic multi-speaker test audio with known ground truth.

Uses macOS `say` with distinct voices, then ffmpeg to normalize to 16kHz mono.
Produces:
  audio/mix_seq.wav      - sequential 3-speaker conversation (no overlap)
  audio/mono_overlap.wav - 2 speakers with an overlapping region (single mixed track)
  audio/track_mic.wav    - overlap scenario, only the "local/mic" speaker (A)
  audio/track_system.wav - overlap scenario, only the "remote/system" speakers (B,C)
  audio/ground_truth.json - reference timelines for scoring

We KNOW the truth because we placed each utterance ourselves.
"""
import json
import os
import subprocess
import sys
import wave

import numpy as np

SR = 16000
HERE = os.path.dirname(os.path.abspath(__file__))
AUDIO = os.path.join(HERE, "audio")
TMP = os.path.join(AUDIO, "_tmp")
os.makedirs(TMP, exist_ok=True)

# Distinct voices: A = deep male (en_GB), B = female (pt_BR), C = synthetic male (very distinct)
VOICES = {"A": "Daniel", "B": "Luciana", "C": "Fred"}

TEXTS = {
    "A": [
        "Good morning everyone, let us start today's engineering sync and review the quarterly roadmap together.",
        "I think the diarization feature is the most important item for us to ship this month for our users.",
        "Let me summarize the action items so that everybody knows exactly what to do before the next meeting.",
    ],
    "B": [
        "Olá pessoal, eu concordo com o planejamento mas precisamos revisar os prazos com bastante cuidado.",
        "A parte de transcrição já está funcionando muito bem, agora falta apenas separar quem está falando.",
    ],
    "C": [
        "From my side the backend is ready and the models are already downloaded and cached on disk locally.",
        "I will run the benchmarks tonight and share the timing numbers with the whole team tomorrow morning.",
    ],
}


def say_to_wav(voice, text, out_path):
    aiff = out_path + ".aiff"
    subprocess.run(["say", "-v", voice, "-o", aiff, text], check=True)
    # normalize to 16kHz mono s16 wav
    subprocess.run(
        ["ffmpeg", "-y", "-loglevel", "error", "-i", aiff,
         "-ar", str(SR), "-ac", "1", "-c:a", "pcm_s16le", out_path],
        check=True,
    )
    os.remove(aiff)


def load_wav(path):
    with wave.open(path, "rb") as w:
        n = w.getnframes()
        data = np.frombuffer(w.readframes(n), dtype=np.int16).astype(np.float32) / 32768.0
    return data


def save_wav(path, samples):
    s = np.clip(samples, -1.0, 1.0)
    pcm = (s * 32767.0).astype(np.int16)
    with wave.open(path, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(SR)
        w.writeframes(pcm.tobytes())


def silence(seconds):
    return np.zeros(int(seconds * SR), dtype=np.float32)


# --- generate all utterance clips ---
clips = {}  # (speaker, idx) -> samples
for spk, texts in TEXTS.items():
    for i, t in enumerate(texts):
        p = os.path.join(TMP, f"{spk}_{i}.wav")
        say_to_wav(VOICES[spk], t, p)
        clips[(spk, i)] = load_wav(p)
        print(f"  {spk}_{i} ({VOICES[spk]}): {len(clips[(spk,i)])/SR:.2f}s", file=sys.stderr)

gt = {}

# --- Scenario 1: sequential 3-speaker conversation (no overlap) ---
# order chosen so the same speaker returns (tests re-identification / clustering)
order = [("A", 0), ("B", 0), ("C", 0), ("A", 1), ("B", 1), ("C", 1), ("A", 2)]
gap = 0.4
seq = []
seq_gt = []
t = 0.0
for spk, i in order:
    c = clips[(spk, i)]
    start = t
    seq.append(c)
    dur = len(c) / SR
    seq_gt.append({"speaker": spk, "start": round(start, 3), "end": round(start + dur, 3)})
    t += dur
    seq.append(silence(gap))
    t += gap
save_wav(os.path.join(AUDIO, "mix_seq.wav"), np.concatenate(seq))
gt["mix_seq"] = seq_gt
print(f"mix_seq: {t:.2f}s, {len(seq_gt)} segments", file=sys.stderr)

# --- Scenario 2: overlap. A is the local/mic speaker, B & C are remote/system. ---
# Timeline (seconds): place utterances, deliberately overlapping A with B.
# We build three canvases of equal length: full-mix, mic-only(A), system-only(B,C).
placements = [
    # (speaker, clip_idx, start_time)
    ("A", 0, 0.0),
    ("B", 0, 6.0),   # B starts while A may still be talking -> overlap with A
    ("C", 0, 13.0),
    ("A", 1, 19.0),
    ("B", 1, 24.5),  # overlaps tail of A(1)
    ("A", 2, 31.0),
    ("C", 1, 36.0),
]
total_len = 0
resolved = []
for spk, i, start in placements:
    c = clips[(spk, i)]
    end = start + len(c) / SR
    total_len = max(total_len, end)
    resolved.append((spk, c, start, end))

N = int((total_len + 1.0) * SR)
mono = np.zeros(N, dtype=np.float32)
mic = np.zeros(N, dtype=np.float32)     # only A
system = np.zeros(N, dtype=np.float32)  # only B, C
ov_gt = []
for spk, c, start, end in resolved:
    s0 = int(start * SR)
    seg = c
    mono[s0:s0 + len(seg)] += seg
    if spk == "A":
        mic[s0:s0 + len(seg)] += seg
    else:
        system[s0:s0 + len(seg)] += seg
    ov_gt.append({"speaker": spk, "start": round(start, 3), "end": round(end, 3)})

save_wav(os.path.join(AUDIO, "mono_overlap.wav"), mono)
save_wav(os.path.join(AUDIO, "track_mic.wav"), mic)
save_wav(os.path.join(AUDIO, "track_system.wav"), system)
gt["mono_overlap"] = ov_gt
# system-only ground truth = only B and C segments
gt["track_system"] = [g for g in ov_gt if g["speaker"] != "A"]
gt["track_mic"] = [g for g in ov_gt if g["speaker"] == "A"]
print(f"overlap scenario: {total_len:.2f}s, {len(ov_gt)} segments", file=sys.stderr)

gt["_meta"] = {"sample_rate": SR, "voices": VOICES}
with open(os.path.join(AUDIO, "ground_truth.json"), "w") as f:
    json.dump(gt, f, indent=2)
print("wrote ground_truth.json", file=sys.stderr)
