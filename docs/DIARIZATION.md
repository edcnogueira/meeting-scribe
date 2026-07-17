# Speaker Diarization

How Meeting Scribe answers "who said what" — fully on-device. This document covers the architecture, the models, the identity registry, configuration, and privacy characteristics of the diarization feature (implemented in tasks [D1–D5](../tasks/README.md)).

## Design at a glance

| Decision | Choice | Why |
|----------|--------|-----|
| When | **Post-recording** (auto on save + manual re-run), not real-time | Global clustering over the whole meeting is significantly more accurate than incremental decisions, and keeps the live pipeline untouched |
| Input | **Separate tracks**: `mic.mp4` (you) + `system.mp4` (remote participants), with mixed-mono fallback | The mic track is one known speaker — half the problem solved without any model. Fewer voices per track → cleaner clusters (spike measured 100% vs 94.2% accuracy on overlapping speech) |
| Models | pyannote **segmentation-3.0** + WeSpeaker **ResNet34 (VoxCeleb)** embeddings, ONNX on `ort` | Non-gated exports, MIT/Apache-2.0, no HF token needed, ~2% of audio duration to process on an M1 |
| Identification | **Local voice-profile registry** with cosine matching | People you've named once are recognized automatically in later meetings |

## Pipeline

```
api_diarize_meeting(meeting_id, num_remote_speakers?)
│
├─ separate-track mode (default for new recordings)
│   ├─ mic.mp4    → decode 16kHz → Silero VAD          → turns labeled as the self profile ("Eu")
│   └─ system.mp4 → decode 16kHz → segmentation model  → speech regions
│                     → embedding model (per region)   → 256-d voice embeddings
│                     → agglomerative clustering        → clusters (cosine distance cut 0.50;
│                       (num_remote_speakers fixes k)      "expected participants" hint fixes k)
│                     → registry matching               → names (similarity ≥ 0.65) or "Speaker N"
│   └─ merge both timelines by timestamp
│
├─ mono fallback (older meetings, imported audio): same chain over audio.mp4;
│   the self profile competes in matching like any other identity
│
└─ attribution: diarized turns ↔ transcript segments by overlap of
    audio_start_time/audio_end_time → transcripts.speaker (SQLite) → UI labels
```

Progress is streamed to the UI via the `diarization-progress` Tauri event (`decoding → segmenting → embedding → clustering → saving`); the job runs in the background and is cancellable.

## Models

Downloaded on first use into the app's models directory (same manager pattern as the Parakeet engine — progress, resume, integrity checks):

| Role | Model | License | Source |
|------|-------|---------|--------|
| Segmentation | pyannote segmentation-3.0 (ONNX export) | MIT | [sherpa-onnx releases](https://github.com/k2-fsa/sherpa-onnx) |
| Speaker embeddings | WeSpeaker ResNet34, VoxCeleb (ONNX) | Apache-2.0 | sherpa-onnx releases |

Exact URLs and the calibration measurements (thresholds, timing, mono-vs-track comparison) are recorded in [`tasks/diarization/D1-resultados.md`](../tasks/diarization/D1-resultados.md).

## The identity registry

- **Tables**: `speakers` (id, name, averaged embedding, `sample_count`, `is_self`) and `meeting_speakers` (per-meeting cluster ↔ identity, match score, cluster embedding). Segment attribution lives in `transcripts.speaker`.
- **Enrollment by renaming**: rename "Speaker 1" → "João" in the meeting panel; the cluster's embedding is folded into João's profile (incremental average, capped at 10 samples). Every correction makes future matching better.
- **The self profile**: the mic track automatically feeds an `is_self` identity (display name configurable; default "Eu"). In separate-track mode it never competes for system-audio clusters; in mono fallback it participates normally.
- **Management**: list, rename, merge, and delete identities from Settings. Deleting removes the voice embeddings (biometric data); historical transcript labels remain as plain text.

## Configuration

Settings → Speaker diarization:

| Setting | Default | Notes |
|---------|---------|-------|
| Enable diarization | on | Master switch |
| Auto-diarize after saving | on | Also exposed as `MEETILY_AUTO_DIARIZE` env fallback |
| Save separate tracks | on | Also `MEETILY_SAVE_SEPARATE_TRACKS`; disable to save disk |
| Speaker-prefixed summary transcript | off | Opt-in: prefixes each utterance with the speaker name in the text sent to the LLM |
| Self profile name | "Eu" | Also `MEETILY_SELF_NAME` |

Per-meeting: the speakers panel accepts an optional **number of remote participants**; when provided it fixes the cluster count, which measurably improves accuracy (it removes the hardest part of the problem — guessing how many people are talking).

## Privacy

Voice embeddings are biometric data. They are stored **only** in the local SQLite database, are never logged, never leave the machine, and are erased when an identity is deleted. The diarization models run entirely offline after the one-time download.

## Key code locations

| Area | Path |
|------|------|
| ONNX engine (sessions, download, clustering) | `frontend/src-tauri/src/diarization_engine/` |
| Post-processing orchestration | `frontend/src-tauri/src/audio/diarization.rs` |
| Identity registry & matching | `frontend/src-tauri/src/audio/diarization_identity.rs`, `frontend/src-tauri/src/database/repositories/speakers.rs` |
| Separate-track recording | `frontend/src-tauri/src/audio/track_saver.rs` |
| Runtime settings | `frontend/src-tauri/src/audio/diarization_settings.rs` |
| UI | `frontend/src/components/{SpeakerChip,DiarizationSettings,SpeakerIdentityManager}.tsx`, `frontend/src/components/MeetingDetails/SpeakersPanel.tsx` |
| Standalone validation prototype (D1 spike) | `scratch/diarization-spike/` |
