//! Speaker diarization engine (task D3).
//!
//! ONNX diarization on `ort` (same runtime as `parakeet_engine`): pyannote
//! segmentation-3.0 for voice-activity/overlap and wespeaker resnet34 for
//! speaker embeddings, followed by agglomerative cosine clustering. Model choice
//! and thresholds come from the D1 spike (`tasks/diarization/D1-resultados.md`).
//!
//! # Module structure
//! - `model`: ONNX session wrappers + kaldi fbank featurizer.
//! - `engine`: model lifecycle, download manager, and the `diarize()` API.
//! - `commands`: Tauri command surface (init/status/download).
//!
//! Meeting-level post-processing (`api_diarize_meeting`) lives in
//! `crate::audio::diarization`, next to the retranscription pipeline it mirrors.

pub mod commands;
pub mod engine;
pub mod model;

pub use engine::{
    cluster_agglomerative, cos_sim, DiarizationDownloadProgress, DiarizationEngine,
    DiarizationModelInfo, DiarizationModelStatus, SpeakerTurn, CLUSTER_COSINE_DISTANCE_CUT,
    DIARIZATION_MODEL_NAME, IDENTIFICATION_COSINE_SIMILARITY,
};
pub use model::{DiarizationModel, DiarizationModelError};
