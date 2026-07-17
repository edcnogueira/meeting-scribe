//! Per-meeting diarization post-processing (task D3).
//!
//! Mirrors `audio::retranscription`: decode a saved meeting's audio, run the
//! diarization engine, attribute a speaker label to every transcribed segment by
//! timestamp overlap, and persist the result. Runs as a cancellable background
//! job and emits `diarization-progress` events.
//!
//! Two modes:
//!   - Separate tracks (D2 present): `mic.mp4` -> VAD only -> "Eu" turns (no
//!     model); `system.mp4` -> `diarize()` -> "Speaker N" clusters. The two
//!     timelines are merged by timestamp.
//!   - Fallback (mono only): diarize the whole mixed `audio.mp4`.

use crate::audio::decoder::decode_audio_file;
use crate::audio::vad::get_speech_chunks_with_progress;
use crate::database::repositories::speakers::SpeakersRepository;
use crate::diarization_engine::SpeakerTurn;
use crate::state::AppState;
use anyhow::{anyhow, Result};
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter, Manager, Runtime};

/// Guards against concurrent diarization jobs.
static DIARIZATION_IN_PROGRESS: AtomicBool = AtomicBool::new(false);
/// Cooperative cancellation signal for the running job.
static DIARIZATION_CANCELLED: AtomicBool = AtomicBool::new(false);

/// Env toggle for auto-diarizing on meeting save. Default: enabled. Set
/// `MEETILY_AUTO_DIARIZE=0` (or `false`/`off`) to disable. (UI toggle lands in D5.)
const AUTO_DIARIZE_ENV: &str = "MEETILY_AUTO_DIARIZE";

/// VAD redemption time for the mic track (matches batch retranscription).
const VAD_REDEMPTION_TIME_MS: u32 = 2000;

/// RAII guard clearing `DIARIZATION_IN_PROGRESS` on drop.
struct DiarizationGuard;

impl DiarizationGuard {
    fn acquire() -> Result<Self, String> {
        if DIARIZATION_IN_PROGRESS
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err("Diarization already in progress".to_string());
        }
        Ok(DiarizationGuard)
    }
}

impl Drop for DiarizationGuard {
    fn drop(&mut self) {
        DIARIZATION_IN_PROGRESS.store(false, Ordering::SeqCst);
    }
}

/// Progress update emitted during diarization (mirrors `RetranscriptionProgress`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiarizationProgress {
    pub meeting_id: String,
    pub stage: String, // decoding | segmenting | embedding | clustering | saving
    pub progress_percentage: u32,
    pub message: String,
}

/// Result of a diarization run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiarizationResult {
    pub meeting_id: String,
    pub segments_labeled: usize,
    pub speaker_count: usize,
    pub used_separate_tracks: bool,
}

/// A speaker-labeled span on the recording timeline (seconds from start).
#[derive(Debug, Clone, PartialEq)]
pub struct LabeledTurn {
    pub start_secs: f64,
    pub end_secs: f64,
    pub label: String,
}

/// A transcript row to attribute (id + recording-relative bounds in seconds).
#[derive(Debug, Clone)]
pub struct TranscriptBounds {
    pub id: String,
    pub start_secs: f64,
    pub end_secs: f64,
}

pub fn is_diarization_in_progress() -> bool {
    DIARIZATION_IN_PROGRESS.load(Ordering::SeqCst)
}

pub fn cancel_diarization() {
    DIARIZATION_CANCELLED.store(true, Ordering::SeqCst);
}

// ----------------------------------------------------------------------------
// Pure helpers (unit-tested without any model).
// ----------------------------------------------------------------------------

/// Merge two independent timelines (e.g. mic "Eu" turns and system "Speaker N"
/// turns) into one list sorted by start time. Both are already on the same
/// recording clock, so this is a sorted concatenation.
pub fn merge_timelines(mic: Vec<LabeledTurn>, system: Vec<LabeledTurn>) -> Vec<LabeledTurn> {
    let mut all: Vec<LabeledTurn> = mic.into_iter().chain(system).collect();
    all.sort_by(|a, b| {
        a.start_secs
            .partial_cmp(&b.start_secs)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    all
}

/// Overlap (in seconds) of two intervals; 0 when disjoint.
fn overlap(a_start: f64, a_end: f64, b_start: f64, b_end: f64) -> f64 {
    (a_end.min(b_end) - a_start.max(b_start)).max(0.0)
}

/// Assign each transcript segment the speaker label with the greatest temporal
/// overlap. Segments overlapping no turn get `None`.
pub fn assign_speakers(
    segments: &[TranscriptBounds],
    turns: &[LabeledTurn],
) -> Vec<(String, Option<String>)> {
    segments
        .iter()
        .map(|seg| {
            let mut per_label: BTreeMap<String, f64> = BTreeMap::new();
            for turn in turns {
                let ov = overlap(seg.start_secs, seg.end_secs, turn.start_secs, turn.end_secs);
                if ov > 0.0 {
                    *per_label.entry(turn.label.clone()).or_insert(0.0) += ov;
                }
            }
            let best = per_label
                .into_iter()
                .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(label, _)| label);
            (seg.id.clone(), best)
        })
        .collect()
}

/// Turn diarization cluster ids into "Speaker N" labels (1-based).
fn cluster_label(cluster_id: usize) -> String {
    format!("Speaker {}", cluster_id + 1)
}

/// Convert engine `SpeakerTurn`s into labeled turns ("Speaker N").
fn system_turns_to_labeled(turns: &[SpeakerTurn]) -> Vec<LabeledTurn> {
    turns
        .iter()
        .map(|t| LabeledTurn {
            start_secs: t.start_secs as f64,
            end_secs: t.end_secs as f64,
            label: cluster_label(t.cluster_id),
        })
        .collect()
}

/// Mean, L2-normalized embedding per cluster id from the system turns.
fn cluster_centroids(turns: &[SpeakerTurn]) -> BTreeMap<usize, Vec<f32>> {
    let mut sums: BTreeMap<usize, (Vec<f32>, usize)> = BTreeMap::new();
    for t in turns {
        let entry = sums
            .entry(t.cluster_id)
            .or_insert_with(|| (vec![0.0; t.embedding.len()], 0));
        if entry.0.len() == t.embedding.len() {
            for (i, x) in t.embedding.iter().enumerate() {
                entry.0[i] += x;
            }
            entry.1 += 1;
        }
    }
    sums.into_iter()
        .map(|(k, (mut v, n))| {
            if n > 0 {
                for x in v.iter_mut() {
                    *x /= n as f32;
                }
                let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-9);
                for x in v.iter_mut() {
                    *x /= norm;
                }
            }
            (k, v)
        })
        .collect()
}

// ----------------------------------------------------------------------------
// Audio helpers
// ----------------------------------------------------------------------------

/// Locate the mixed audio file for fallback mode.
fn find_mixed_audio(folder: &Path) -> Result<PathBuf> {
    let candidates = [
        "audio.mp4", "audio.m4a", "audio.wav", "audio.mp3", "audio.flac", "audio.ogg",
        "recording.mp4",
    ];
    for name in candidates {
        let path = folder.join(name);
        if path.exists() {
            return Ok(path);
        }
    }
    Err(anyhow!("No mixed audio file found in {}", folder.display()))
}

/// Decode any audio file to 16 kHz mono f32 on a blocking thread.
async fn decode_16k_mono(path: PathBuf) -> Result<Vec<f32>> {
    let decoded = tokio::task::spawn_blocking(move || decode_audio_file(&path))
        .await
        .map_err(|e| anyhow!("Decode task panicked: {}", e))??;
    let samples = tokio::task::spawn_blocking(move || decoded.to_whisper_format())
        .await
        .map_err(|e| anyhow!("Resample task panicked: {}", e))?;
    Ok(samples)
}

/// Run VAD on the mic track and return "Eu" turns (no model involved).
async fn mic_vad_turns(samples: Vec<f32>) -> Result<Vec<LabeledTurn>> {
    let segments = tokio::task::spawn_blocking(move || {
        get_speech_chunks_with_progress(&samples, VAD_REDEMPTION_TIME_MS, |_p, _f| {
            !DIARIZATION_CANCELLED.load(Ordering::SeqCst)
        })
    })
    .await
    .map_err(|e| anyhow!("VAD task panicked: {}", e))?
    .map_err(|e| anyhow!("VAD failed: {}", e))?;

    Ok(segments
        .into_iter()
        .map(|s| LabeledTurn {
            start_secs: s.start_timestamp_ms / 1000.0,
            end_secs: s.end_timestamp_ms / 1000.0,
            label: "Eu".to_string(),
        })
        .collect())
}

// ----------------------------------------------------------------------------
// Orchestration
// ----------------------------------------------------------------------------

fn emit_progress<R: Runtime>(
    app: &AppHandle<R>,
    meeting_id: &str,
    stage: &str,
    progress: u32,
    message: &str,
) {
    let _ = app.emit(
        "diarization-progress",
        DiarizationProgress {
            meeting_id: meeting_id.to_string(),
            stage: stage.to_string(),
            progress_percentage: progress,
            message: message.to_string(),
        },
    );
}

/// Resolve a meeting's folder path from the database.
async fn meeting_folder<R: Runtime>(app: &AppHandle<R>, meeting_id: &str) -> Result<PathBuf> {
    let state = app
        .try_state::<AppState>()
        .ok_or_else(|| anyhow!("App state not available"))?;
    let meeting = crate::database::repositories::meeting::MeetingsRepository::get_meeting_metadata(
        state.db_manager.pool(),
        meeting_id,
    )
    .await
    .map_err(|e| anyhow!("Failed to load meeting: {}", e))?
    .ok_or_else(|| anyhow!("Meeting {} not found", meeting_id))?;

    meeting
        .folder_path
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("Meeting {} has no recording folder", meeting_id))
}

/// Load transcript bounds (segments with recording-relative timestamps).
async fn load_transcript_bounds<R: Runtime>(
    app: &AppHandle<R>,
    meeting_id: &str,
) -> Result<Vec<TranscriptBounds>> {
    let state = app
        .try_state::<AppState>()
        .ok_or_else(|| anyhow!("App state not available"))?;
    let rows = sqlx::query_as::<_, (String, Option<f64>, Option<f64>)>(
        "SELECT id, audio_start_time, audio_end_time FROM transcripts WHERE meeting_id = ?",
    )
    .bind(meeting_id)
    .fetch_all(state.db_manager.pool())
    .await
    .map_err(|e| anyhow!("Failed to load transcripts: {}", e))?;

    Ok(rows
        .into_iter()
        .filter_map(|(id, start, end)| match (start, end) {
            (Some(s), Some(e)) => Some(TranscriptBounds {
                id,
                start_secs: s,
                end_secs: e,
            }),
            _ => None,
        })
        .collect())
}

/// Persist speaker labels on transcripts and cluster rows on `meeting_speakers`.
async fn persist_results<R: Runtime>(
    app: &AppHandle<R>,
    meeting_id: &str,
    assignments: &[(String, Option<String>)],
    system_turns: &[SpeakerTurn],
    has_mic: bool,
) -> Result<usize> {
    let state = app
        .try_state::<AppState>()
        .ok_or_else(|| anyhow!("App state not available"))?;
    let pool = state.db_manager.pool();

    let mut conn = pool.acquire().await.map_err(|e| anyhow!("DB error: {}", e))?;
    let mut tx = sqlx::Connection::begin(&mut *conn)
        .await
        .map_err(|e| anyhow!("Failed to begin transaction: {}", e))?;

    let mut labeled = 0usize;
    for (id, label) in assignments {
        if let Some(label) = label {
            sqlx::query("UPDATE transcripts SET speaker = ? WHERE id = ?")
                .bind(label)
                .bind(id)
                .execute(&mut *tx)
                .await
                .map_err(|e| anyhow!("Failed to update transcript speaker: {}", e))?;
            labeled += 1;
        }
    }

    tx.commit()
        .await
        .map_err(|e| anyhow!("Failed to commit transcript labels: {}", e))?;

    // Replace this meeting's cluster rows with the fresh clustering.
    SpeakersRepository::delete_meeting_speakers(pool, meeting_id)
        .await
        .map_err(|e| anyhow!("Failed to clear meeting speakers: {}", e))?;

    if has_mic {
        // Local speaker has no clustered embedding (VAD-only).
        SpeakersRepository::insert_meeting_speaker(pool, meeting_id, "Eu", &[], None, None)
            .await
            .map_err(|e| anyhow!("Failed to store local speaker: {}", e))?;
    }

    let centroids = cluster_centroids(system_turns);
    for (cluster_id, embedding) in centroids {
        SpeakersRepository::insert_meeting_speaker(
            pool,
            meeting_id,
            &cluster_label(cluster_id),
            &embedding,
            None,
            None,
        )
        .await
        .map_err(|e| anyhow!("Failed to store cluster speaker: {}", e))?;
    }

    Ok(labeled)
}

/// Core diarization routine for a saved meeting.
async fn run_diarization<R: Runtime>(
    app: AppHandle<R>,
    meeting_id: String,
    num_remote_speakers: Option<usize>,
) -> Result<DiarizationResult> {
    let engine = crate::diarization_engine::commands::get_engine()
        .ok_or_else(|| anyhow!("Diarization engine not initialized"))?;
    if !engine.is_available().await {
        return Err(anyhow!(
            "Diarization model not downloaded. Download it before diarizing."
        ));
    }

    let folder = meeting_folder(&app, &meeting_id).await?;
    let mic_path = folder.join("mic.mp4");
    let system_path = folder.join("system.mp4");
    let has_tracks = mic_path.exists() && system_path.exists();

    emit_progress(&app, &meeting_id, "decoding", 5, "Decoding audio...");
    if DIARIZATION_CANCELLED.load(Ordering::SeqCst) {
        return Err(anyhow!("Diarization cancelled"));
    }

    let (mic_turns, system_turns): (Vec<LabeledTurn>, Vec<SpeakerTurn>) = if has_tracks {
        info!("Diarizing with separate tracks for meeting {}", meeting_id);

        // Mic track: VAD only -> "Eu".
        let mic_samples = decode_16k_mono(mic_path).await?;
        emit_progress(&app, &meeting_id, "segmenting", 20, "Detecting local speech...");
        let mic_turns = mic_vad_turns(mic_samples).await?;

        if DIARIZATION_CANCELLED.load(Ordering::SeqCst) {
            return Err(anyhow!("Diarization cancelled"));
        }

        // System track: full diarization -> "Speaker N".
        let system_samples = decode_16k_mono(system_path).await?;
        emit_progress(&app, &meeting_id, "embedding", 45, "Analyzing remote speakers...");
        let system_turns = engine
            .diarize(&system_samples, num_remote_speakers)
            .await
            .map_err(|e| anyhow!("System-track diarization failed: {}", e))?;

        (mic_turns, system_turns)
    } else {
        info!("Diarizing mixed audio (fallback) for meeting {}", meeting_id);
        let audio_path = find_mixed_audio(&folder)?;
        let samples = decode_16k_mono(audio_path).await?;
        emit_progress(&app, &meeting_id, "segmenting", 30, "Analyzing speakers...");
        let turns = engine
            .diarize(&samples, num_remote_speakers)
            .await
            .map_err(|e| anyhow!("Diarization failed: {}", e))?;
        (Vec::new(), turns)
    };

    if DIARIZATION_CANCELLED.load(Ordering::SeqCst) {
        return Err(anyhow!("Diarization cancelled"));
    }

    emit_progress(&app, &meeting_id, "clustering", 75, "Merging speaker timeline...");
    let system_labeled = system_turns_to_labeled(&system_turns);
    let timeline = merge_timelines(mic_turns.clone(), system_labeled);

    // Distinct speakers = system clusters (+ "Eu" if tracks were used).
    let system_cluster_count = system_turns
        .iter()
        .map(|t| t.cluster_id)
        .collect::<std::collections::HashSet<_>>()
        .len();
    let speaker_count = system_cluster_count + usize::from(!mic_turns.is_empty());

    // Attribute speakers to transcript segments by overlap.
    let bounds = load_transcript_bounds(&app, &meeting_id).await?;
    let assignments = assign_speakers(&bounds, &timeline);

    emit_progress(&app, &meeting_id, "saving", 90, "Saving speaker labels...");
    let labeled = persist_results(
        &app,
        &meeting_id,
        &assignments,
        &system_turns,
        !mic_turns.is_empty(),
    )
    .await?;

    emit_progress(&app, &meeting_id, "complete", 100, "Diarization complete");

    Ok(DiarizationResult {
        meeting_id,
        segments_labeled: labeled,
        speaker_count,
        used_separate_tracks: has_tracks,
    })
}

/// Entry point that manages the guard, cancellation, and completion events.
async fn start_diarization<R: Runtime>(
    app: AppHandle<R>,
    meeting_id: String,
    num_remote_speakers: Option<usize>,
) -> Result<DiarizationResult> {
    let _guard = DiarizationGuard::acquire().map_err(|e| anyhow!(e))?;
    DIARIZATION_CANCELLED.store(false, Ordering::SeqCst);

    let result = run_diarization(app.clone(), meeting_id.clone(), num_remote_speakers).await;

    // Free the model after the batch job unless a recording is in progress.
    if !crate::audio::recording_commands::is_recording().await {
        if let Some(engine) = crate::diarization_engine::commands::get_engine() {
            engine.unload_model().await;
        }
    }

    match &result {
        Ok(res) => {
            let _ = app.emit(
                "diarization-complete",
                serde_json::json!({
                    "meeting_id": res.meeting_id,
                    "segments_labeled": res.segments_labeled,
                    "speaker_count": res.speaker_count,
                    "used_separate_tracks": res.used_separate_tracks,
                }),
            );
        }
        Err(e) => {
            let _ = app.emit(
                "diarization-error",
                serde_json::json!({
                    "meeting_id": meeting_id,
                    "error": e.to_string(),
                }),
            );
        }
    }

    result
}

/// Fire-and-forget the diarization job on the async runtime.
fn spawn_diarization_job<R: Runtime>(
    app: AppHandle<R>,
    meeting_id: String,
    num_remote_speakers: Option<usize>,
) {
    tauri::async_runtime::spawn(async move {
        if let Err(e) = start_diarization(app, meeting_id, num_remote_speakers).await {
            error!("Diarization job failed: {}", e);
        }
    });
}

/// Whether auto-diarization is enabled (default on; `MEETILY_AUTO_DIARIZE=0`
/// / `false` / `off` disables it).
pub fn auto_diarize_enabled() -> bool {
    match std::env::var(AUTO_DIARIZE_ENV) {
        Ok(v) => !matches!(v.trim().to_lowercase().as_str(), "0" | "false" | "off" | "no"),
        Err(_) => true,
    }
}

/// Optionally kick off diarization right after a meeting is saved. No-op when
/// the toggle is off, the engine/model is unavailable, or a job is already
/// running. Never blocks the caller.
pub fn maybe_auto_diarize<R: Runtime>(app: &AppHandle<R>, meeting_id: String) {
    if !auto_diarize_enabled() {
        return;
    }
    if is_diarization_in_progress() {
        info!("Auto-diarize skipped: another diarization job is running");
        return;
    }

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let engine = match crate::diarization_engine::commands::get_engine() {
            Some(e) => e,
            None => return,
        };
        if !engine.is_available().await {
            info!("Auto-diarize skipped: diarization model not downloaded");
            return;
        }
        if let Err(e) = start_diarization(app, meeting_id, None).await {
            warn!("Auto-diarization failed: {}", e);
        }
    });
}

// ----------------------------------------------------------------------------
// Tauri commands
// ----------------------------------------------------------------------------

/// Response when diarization is started.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiarizationStarted {
    pub meeting_id: String,
    pub message: String,
}

/// Start diarization for a saved meeting as a background job.
#[tauri::command]
pub async fn api_diarize_meeting<R: Runtime>(
    app: AppHandle<R>,
    meeting_id: String,
    num_remote_speakers: Option<usize>,
) -> Result<DiarizationStarted, String> {
    if is_diarization_in_progress() {
        return Err("Diarization already in progress".to_string());
    }
    spawn_diarization_job(app, meeting_id.clone(), num_remote_speakers);
    Ok(DiarizationStarted {
        meeting_id,
        message: "Diarization started".to_string(),
    })
}

#[tauri::command]
pub async fn cancel_diarization_command() -> Result<(), String> {
    if !is_diarization_in_progress() {
        return Err("No diarization in progress".to_string());
    }
    cancel_diarization();
    Ok(())
}

#[tauri::command]
pub async fn is_diarization_in_progress_command() -> bool {
    is_diarization_in_progress()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(start: f64, end: f64, label: &str) -> LabeledTurn {
        LabeledTurn {
            start_secs: start,
            end_secs: end,
            label: label.to_string(),
        }
    }

    fn bounds(id: &str, start: f64, end: f64) -> TranscriptBounds {
        TranscriptBounds {
            id: id.to_string(),
            start_secs: start,
            end_secs: end,
        }
    }

    #[test]
    fn test_merge_timelines_sorts_by_start() {
        let mic = vec![turn(0.0, 2.0, "Eu"), turn(10.0, 12.0, "Eu")];
        let system = vec![turn(2.0, 5.0, "Speaker 1"), turn(6.0, 8.0, "Speaker 2")];
        let merged = merge_timelines(mic, system);
        let starts: Vec<f64> = merged.iter().map(|t| t.start_secs).collect();
        assert_eq!(starts, vec![0.0, 2.0, 6.0, 10.0]);
        assert_eq!(merged[0].label, "Eu");
        assert_eq!(merged[1].label, "Speaker 1");
    }

    #[test]
    fn test_assign_speakers_picks_max_overlap() {
        // Segment 0-4s overlaps "Eu" for 2s and "Speaker 1" for ~2s but "Eu" wins
        // by a hair; make the overlap decisive.
        let turns = vec![turn(0.0, 3.0, "Eu"), turn(3.0, 10.0, "Speaker 1")];
        let segs = vec![
            bounds("a", 0.0, 2.5),  // mostly Eu
            bounds("b", 4.0, 9.0),  // all Speaker 1
            bounds("c", 2.5, 3.5),  // 0.5 Eu vs 0.5 Speaker 1 -> tie broken deterministically
        ];
        let result = assign_speakers(&segs, &turns);
        assert_eq!(result[0], ("a".to_string(), Some("Eu".to_string())));
        assert_eq!(result[1], ("b".to_string(), Some("Speaker 1".to_string())));
        // c is a tie (0.5 each); BTreeMap + max_by keeps the last max -> "Speaker 1".
        assert_eq!(result[2].0, "c");
        assert!(result[2].1.is_some());
    }

    #[test]
    fn test_assign_speakers_no_overlap_is_none() {
        let turns = vec![turn(0.0, 2.0, "Eu")];
        let segs = vec![bounds("x", 5.0, 7.0)];
        let result = assign_speakers(&segs, &turns);
        assert_eq!(result[0], ("x".to_string(), None));
    }

    #[test]
    fn test_assign_speakers_accumulates_split_turns() {
        // "Speaker 1" appears twice around a short "Eu"; combined it should win.
        let turns = vec![
            turn(0.0, 2.0, "Speaker 1"),
            turn(2.0, 2.5, "Eu"),
            turn(2.5, 5.0, "Speaker 1"),
        ];
        let segs = vec![bounds("s", 0.0, 5.0)];
        let result = assign_speakers(&segs, &turns);
        // Speaker 1 total = 4.5s vs Eu 0.5s.
        assert_eq!(result[0], ("s".to_string(), Some("Speaker 1".to_string())));
    }

    #[test]
    fn test_cluster_label_is_one_based() {
        assert_eq!(cluster_label(0), "Speaker 1");
        assert_eq!(cluster_label(3), "Speaker 4");
    }

    #[test]
    fn test_cluster_centroids_are_normalized_means() {
        let turns = vec![
            SpeakerTurn { start_secs: 0.0, end_secs: 1.0, cluster_id: 0, embedding: vec![1.0, 0.0] },
            SpeakerTurn { start_secs: 1.0, end_secs: 2.0, cluster_id: 0, embedding: vec![0.0, 1.0] },
            SpeakerTurn { start_secs: 2.0, end_secs: 3.0, cluster_id: 1, embedding: vec![1.0, 0.0] },
        ];
        let centroids = cluster_centroids(&turns);
        assert_eq!(centroids.len(), 2);
        // Cluster 0 mean = (0.5, 0.5) -> normalized (0.707, 0.707).
        let c0 = &centroids[&0];
        assert!((c0[0] - c0[1]).abs() < 1e-6);
        let norm = (c0[0] * c0[0] + c0[1] * c0[1]).sqrt();
        assert!((norm - 1.0).abs() < 1e-5);
    }

    #[test]
    fn test_system_turns_to_labeled() {
        let turns = vec![
            SpeakerTurn { start_secs: 1.0, end_secs: 2.5, cluster_id: 0, embedding: vec![] },
            SpeakerTurn { start_secs: 3.0, end_secs: 4.0, cluster_id: 1, embedding: vec![] },
        ];
        let labeled = system_turns_to_labeled(&turns);
        assert_eq!(labeled[0].label, "Speaker 1");
        assert_eq!(labeled[1].label, "Speaker 2");
        assert_eq!(labeled[0].start_secs, 1.0);
    }

    #[test]
    fn test_auto_diarize_enabled_default_and_overrides() {
        // Note: exercises parsing logic via a temporary env override.
        std::env::remove_var(AUTO_DIARIZE_ENV);
        assert!(auto_diarize_enabled());
        std::env::set_var(AUTO_DIARIZE_ENV, "0");
        assert!(!auto_diarize_enabled());
        std::env::set_var(AUTO_DIARIZE_ENV, "false");
        assert!(!auto_diarize_enabled());
        std::env::set_var(AUTO_DIARIZE_ENV, "1");
        assert!(auto_diarize_enabled());
        std::env::remove_var(AUTO_DIARIZE_ENV);
    }
}
