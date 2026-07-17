# Meeting Scribe

**Privacy-first AI meeting assistant with local speaker diarization.**

Records your microphone and system audio, transcribes locally with Whisper, tells you **who said what** (speaker diarization with a voice-profile registry — fully on-device), and generates structured summaries with local or cloud LLMs. Nothing leaves your machine unless you explicitly configure a cloud provider.

> **Personal project.** This is a heavily customized derivative of
> [Meetily (Zackriya-Solutions/meeting-minutes)](https://github.com/Zackriya-Solutions/meeting-minutes),
> maintained by [@edcnogueira](https://github.com/edcnogueira) for personal use under the MIT license.
> It is shaped around one person's workflow — anything may change without notice. You are welcome to
> use, fork, or borrow from it. For a general-purpose, supported product, use the original Meetily.

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE.md)
![Platform](https://img.shields.io/badge/platform-macOS%20(Apple%20Silicon)-lightgrey)
![Stack](https://img.shields.io/badge/stack-Tauri%202%20%C2%B7%20Rust%20%C2%B7%20Next.js-orange)

---

## What this fork adds on top of Meetily

| Feature | Description |
|---------|-------------|
| **Separate audio tracks** | Besides the mixed mono recording, each meeting saves `mic.mp4` (you) and `system.mp4` (remote participants) as time-aligned tracks — the foundation for accurate diarization. |
| **Local speaker diarization** | Post-recording pipeline: pyannote segmentation + WeSpeaker embeddings running on ONNX Runtime (`ort`), agglomerative clustering, timestamp-overlap attribution. The mic track is labeled deterministically as *you* — no model guessing involved. |
| **Speaker identity registry** | A local registry of people with voice profiles (embeddings in SQLite). Rename "Speaker 1" to "João" once — next meeting, João is recognized automatically (cosine similarity ≥ 0.65). Deleting a person wipes their biometric data. |
| **Speaker-aware UI** | Colored speaker labels in transcripts, a per-meeting speakers panel (rename, re-diarize, expected-participants hint), diarization settings with model download, and an opt-in speaker-prefixed transcript for LLM summaries. |
| **No phone-home** | Upstream auto-updater removed — builds are produced and installed locally. |

Everything else — recording pipeline, Whisper/Parakeet transcription, VAD, summary templates, LLM providers (built-in llama.cpp sidecar, Ollama, Claude, OpenAI, Groq, OpenRouter, custom endpoint) — comes from upstream Meetily v0.4.0 and works as documented there.

See [docs/DIARIZATION.md](docs/DIARIZATION.md) for how the diarization pipeline works, and [tasks/](tasks/README.md) for the task-by-task implementation history (D1–D5).

## How it works

```
Recording:  mic ─┐                       ┌─→ audio.mp4 (mixed, playback)
                 ├─→ audio pipeline ─────┼─→ mic.mp4    (separate track)
            sys ─┘   (mix + VAD)         └─→ system.mp4 (separate track)
                          │
Transcription:            └─→ Silero VAD → Whisper (Metal/CoreML) → transcript (SQLite)

Diarization (post-recording):
    mic.mp4    → VAD → "you" turns              ┐
    system.mp4 → segmentation → embeddings      ├─→ merge by timestamp → speaker per segment
                 → clustering → registry match  ┘    → colored labels in UI

Summary:  transcript (+ optional speaker prefixes) → template → LLM of your choice
```

## Getting started (macOS, Apple Silicon)

Prerequisites: Xcode Command Line Tools, Rust ≥ 1.77, Node ≥ 18, pnpm ≥ 8, CMake.

```bash
git clone git@github.com:edcnogueira/meeting-scribe.git
cd meeting-scribe

# 1. Build the llama-helper sidecar (required by the app build)
cargo build --release -p llama-helper
cp target/release/llama-helper frontend/src-tauri/binaries/llama-helper-aarch64-apple-darwin

# 2. Build the app (Metal + CoreML enabled automatically; first build takes ~30-40 min)
cd frontend
pnpm install
pnpm run tauri:build

# 3. Install
cp -R src-tauri/target/release/bundle/macos/meetily.app /Applications/
```

For development: `pnpm run tauri:dev`. Full build details (Windows/Linux, GPU flags) in [docs/BUILDING.md](docs/BUILDING.md) — inherited from upstream and still accurate.

Notes:
- The app bundle is still named `meetily.app` / `com.meetily.ai` on purpose — it keeps app data (`~/Library/Application Support/Meetily/`) compatible with upstream installs.
- System audio capture on macOS needs the screen-recording permission; a virtual device such as [BlackHole](https://existential.audio/blackhole/) is recommended for routing.
- Diarization models (~30 MB total) are downloaded on first use from the sources listed below.

## Roadmap

- **CLI summary provider** — generate summaries through locally installed subscription CLIs (`codex exec`, `claude -p`, `gemini -p`): no API keys, no local LLM load.
- Markdown export of meetings (with speakers) into an Obsidian vault.
- Cleanups: dead-code removal, PostHog telemetry audit, `cargo test`/`clippy` in CI.

## Credits

This project stands on excellent open work:

- **[Meetily / meeting-minutes](https://github.com/Zackriya-Solutions/meeting-minutes)** by Zackriya Solutions (MIT) — the entire application foundation. This repository preserves their [license](LICENSE.md) and full history (fork point: tag `upstream-v0.4.0`).
- **[whisper.cpp](https://github.com/ggerganov/whisper.cpp)** via [whisper-rs](https://github.com/tazz4843/whisper-rs) (MIT) — local transcription.
- **[llama.cpp](https://github.com/ggerganov/llama.cpp)** via [llama-cpp-2](https://github.com/utilityai/llama-cpp-rs) (MIT) — built-in summarization models.
- **[pyannote segmentation-3.0](https://huggingface.co/pyannote/segmentation-3.0)** (MIT) — speaker segmentation, using the non-gated ONNX export distributed by [sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx).
- **[WeSpeaker](https://github.com/wenet-e2e/wespeaker)** ResNet34 VoxCeleb model (Apache-2.0) — speaker embeddings, ONNX export via sherpa-onnx.
- **[Silero VAD](https://github.com/snakers4/silero-vad)** (MIT) via [silero-rs](https://github.com/emotechlab/silero-rs) — voice activity detection.
- **[NVIDIA Parakeet](https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx)** ONNX conversion by @istupakov — alternative English transcription engine (inherited from upstream).
- **[Tauri](https://tauri.app/)**, **[Next.js](https://nextjs.org/)**, and **[ONNX Runtime](https://onnxruntime.ai/)** via [ort](https://github.com/pykeio/ort).

## License

[MIT](LICENSE.md) — original copyright © Zackriya Solutions; modifications © [@edcnogueira](https://github.com/edcnogueira).
