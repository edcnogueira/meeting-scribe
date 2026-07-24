//! Diarization engine: model lifecycle, HuggingFace/GitHub download manager, and
//! the central `diarize()` API (segmentation -> embedding -> agglomerative
//! cosine clustering). Mirrors the structure of `parakeet_engine`.

use crate::diarization_engine::model::{
    DiarizationModel, DiarizationModelError, EMBEDDING_FILE, LOCAL_SPEAKERS, POWERSET,
    SEGMENTATION_FILE, SEG_WINDOW, SR,
};
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::fs;
use tokio::io::{AsyncWriteExt, BufWriter};
use tokio::sync::RwLock;
use tokio::time::timeout;

/// Canonical model identifier for the bundled diarization model pair.
pub const DIARIZATION_MODEL_NAME: &str = "diarization-default";

/// Clustering cut on cosine *distance* (1 - cosine similarity). Calibrated in D1
/// (stable range 0.35-0.60; 0.50 recommended).
pub const CLUSTER_COSINE_DISTANCE_CUT: f32 = 0.50;

/// Cross-session identification threshold on cosine *similarity* (D1). Used by D4;
/// exposed here so the calibrated value lives with the engine.
pub const IDENTIFICATION_COSINE_SIMILARITY: f32 = 0.65;

/// Segmentation model download (sherpa-onnx export, non-gated). tar.bz2 archive
/// containing `model.onnx` (+ int8 variant + LICENSE). From D1-resultados.md.
const SEGMENTATION_URL: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-segmentation-models/sherpa-onnx-pyannote-segmentation-3-0.tar.bz2";
/// wespeaker en voxceleb resnet34 speaker embedding (direct .onnx). From D1.
const EMBEDDING_URL: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/wespeaker_en_voxceleb_resnet34.onnx";

// Approximate download sizes (bytes) for weighted progress.
const SEGMENTATION_APPROX_BYTES: u64 = 2_400_000; // ~2.3 MB compressed tarball
const EMBEDDING_APPROX_BYTES: u64 = 27_800_000; // ~26.5 MB

/// Model availability status (mirrors `parakeet_engine::ModelStatus`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DiarizationModelStatus {
    Available,
    Missing,
    Downloading { progress: u8 },
    Error(String),
}

/// Detailed download progress (MB-based with speed).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiarizationDownloadProgress {
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub downloaded_mb: f64,
    pub total_mb: f64,
    pub speed_mbps: f64,
    pub percent: u8,
}

impl DiarizationDownloadProgress {
    fn new(downloaded: u64, total: u64, speed_mbps: f64) -> Self {
        let percent = if total > 0 {
            ((downloaded as f64 / total as f64) * 100.0).min(100.0) as u8
        } else {
            0
        };
        Self {
            downloaded_bytes: downloaded,
            total_bytes: total,
            downloaded_mb: downloaded as f64 / (1024.0 * 1024.0),
            total_mb: total as f64 / (1024.0 * 1024.0),
            speed_mbps,
            percent,
        }
    }
}

/// Model metadata reported to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiarizationModelInfo {
    pub name: String,
    pub path: PathBuf,
    pub size_mb: u32,
    pub status: DiarizationModelStatus,
    pub description: String,
}

/// A contiguous single-speaker span produced by `diarize()`.
///
/// `cluster_id` is a per-meeting local label (0-based); cross-session identity
/// matching against enrolled speakers is D4's responsibility.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeakerTurn {
    pub start_secs: f32,
    pub end_secs: f32,
    pub cluster_id: usize,
    pub embedding: Vec<f32>,
}

/// Cosine similarity of two equal-length L2-normalized vectors.
pub fn cos_sim(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// Agglomerative average-linkage clustering on cosine distance. Ported from the
/// validated D1 spike.
///
/// - `threshold`: merge while the closest pair is within this cosine distance.
/// - `fixed_k`: when `Some(k)`, stop at exactly `k` clusters (overrides threshold)
///   as long as there are at least `k` items.
///
/// Returns a label per input embedding (labels are contiguous from 0).
pub fn cluster_agglomerative(
    embeddings: &[Vec<f32>],
    threshold: Option<f32>,
    fixed_k: Option<usize>,
) -> Vec<usize> {
    let n = embeddings.len();
    if n == 0 {
        return vec![];
    }
    if n == 1 {
        return vec![0];
    }

    let mut members: Vec<Vec<usize>> = (0..n).map(|i| vec![i]).collect();
    let dist = |a: &[usize], b: &[usize]| -> f32 {
        let mut s = 0.0;
        for &i in a {
            for &j in b {
                s += 1.0 - cos_sim(&embeddings[i], &embeddings[j]);
            }
        }
        s / (a.len() * b.len()) as f32
    };

    loop {
        if members.len() <= 1 {
            break;
        }
        if let Some(k) = fixed_k {
            if members.len() <= k.max(1) {
                break;
            }
        }

        // find the closest pair
        let mut best = (0usize, 1usize);
        let mut best_d = f32::INFINITY;
        for i in 0..members.len() {
            for j in (i + 1)..members.len() {
                let d = dist(&members[i], &members[j]);
                if d < best_d {
                    best_d = d;
                    best = (i, j);
                }
            }
        }

        if let Some(th) = threshold {
            if fixed_k.is_none() && best_d > th {
                break;
            }
        }

        let (i, j) = best;
        let mut merged = members[i].clone();
        merged.extend(members[j].clone());
        members.remove(j);
        members[i] = merged;
    }

    // Relabel members in order of first-appearing index for deterministic output.
    let mut order: Vec<(usize, usize)> = members
        .iter()
        .enumerate()
        .map(|(ci, mem)| (*mem.iter().min().unwrap(), ci))
        .collect();
    order.sort_by_key(|(first_idx, _)| *first_idx);

    let mut labels = vec![0usize; n];
    for (new_label, (_, ci)) in order.iter().enumerate() {
        for &idx in &members[*ci] {
            labels[idx] = new_label;
        }
    }
    labels
}

/// Internal speech region carrying its embedding before clustering.
struct Region {
    start: f32,
    end: f32,
    embedding: Vec<f32>,
}

/// Whole-recording per-frame speaker activity after stitching overlapping
/// segmentation windows into global speaker tracks.
struct StitchedTimeline {
    /// Seconds per segmentation frame (~17 ms).
    step: f32,
    /// Per frame: bitmask over global track ids of active speakers (tracks
    /// >= 64 are not representable in the mask and are dropped — see
    /// `stitch_activity`; real recordings stay far below that bound).
    active: Vec<u64>,
    /// Per frame: the single exclusive global track id, if exactly one speaker
    /// is active in the winning window's decision for that frame.
    exclusive: Vec<Option<usize>>,
    /// Number of global tracks allocated.
    n_tracks: usize,
}

/// A contiguous speech run of a single global track over the stitched timeline.
/// `end_f` is inclusive.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Run {
    track: usize,
    start_f: usize,
    end_f: usize,
}

/// The 6 permutations of three local speaker slots.
const SLOT_PERMS: [[usize; LOCAL_SPEAKERS]; 6] = [
    [0, 1, 2],
    [0, 2, 1],
    [1, 0, 2],
    [1, 2, 0],
    [2, 0, 1],
    [2, 1, 0],
];

/// Minimum activity-pattern IoU for a next slot to inherit a previous slot's
/// global track. The same physical speaker occupies nearly the same absolute
/// frames in both overlapping windows (IoU near 1); temporally adjacent speakers
/// (one ending as the next begins) share the hand-off frames and score lower.
/// A high floor keeps the stitch conservative: an uncertain slot gets a *fresh*
/// track (agglomerative clustering later re-merges same-voice tracks by
/// embedding, which is cheap to undo) rather than being welded onto a different
/// speaker's track (an impure run, which clustering cannot undo). Empirically
/// the eval accuracy peaks on a 0.85-0.90 plateau.
const ALIGN_MATCH_IOU: f32 = 0.9;

/// Choose the permutation of the next window's local speakers that maximizes
/// activity-pattern agreement (intersection-over-union) with the previous window
/// over their overlap region.
///
/// Returns, for each next-window local slot, the previous-window local slot it
/// maps to, or `usize::MAX` when the slot has no agreement with any previous
/// slot (a new or re-appearing voice that the caller assigns a fresh global
/// track). The mapping is relative to the previous window's slots; the caller
/// composes it with the previous window's local->global map.
///
/// IoU (rather than raw co-activation count) is what keeps a long-talking
/// previous speaker from absorbing every next slot it happens to overlap.
fn align_local_speakers(
    prev_overlap: &[[bool; LOCAL_SPEAKERS]],
    next_overlap: &[[bool; LOCAL_SPEAKERS]],
) -> [usize; LOCAL_SPEAKERS] {
    let n = prev_overlap.len().min(next_overlap.len());
    // inter[i][j] = frames both active; act_next[i]/act_prev[j] = per-slot counts.
    let mut inter = [[0u32; LOCAL_SPEAKERS]; LOCAL_SPEAKERS];
    let mut act_next = [0u32; LOCAL_SPEAKERS];
    let mut act_prev = [0u32; LOCAL_SPEAKERS];
    for f in 0..n {
        for j in 0..LOCAL_SPEAKERS {
            if prev_overlap[f][j] {
                act_prev[j] += 1;
            }
        }
        for i in 0..LOCAL_SPEAKERS {
            if !next_overlap[f][i] {
                continue;
            }
            act_next[i] += 1;
            for j in 0..LOCAL_SPEAKERS {
                if prev_overlap[f][j] {
                    inter[i][j] += 1;
                }
            }
        }
    }

    let iou = |i: usize, j: usize| -> f32 {
        let union = act_next[i] + act_prev[j] - inter[i][j];
        if union == 0 {
            0.0
        } else {
            inter[i][j] as f32 / union as f32
        }
    };

    // Pick the permutation maximizing total IoU over matched pairs.
    let mut best_perm = SLOT_PERMS[0];
    let mut best_score = -1.0f32;
    for perm in SLOT_PERMS.iter() {
        let s: f32 = (0..LOCAL_SPEAKERS).map(|i| iou(i, perm[i])).sum();
        if s > best_score {
            best_score = s;
            best_perm = *perm;
        }
    }

    // A next slot only inherits a previous track when its matched pattern is
    // similar enough (same voice); otherwise it is a fresh voice.
    let mut out = [usize::MAX; LOCAL_SPEAKERS];
    for i in 0..LOCAL_SPEAKERS {
        let j = best_perm[i];
        if iou(i, j) >= ALIGN_MATCH_IOU {
            out[i] = j;
        }
    }
    out
}

/// Stitch per-window local speaker activity into a global timeline.
///
/// Pure over synthetic activity matrices (no model inference): every window is a
/// `[frames][LOCAL_SPEAKERS]` bool matrix, all windows share the same frame count
/// (the segmentation model zero-pads to a fixed window length). `offsets[w]` is
/// the global frame index where window `w` begins (monotonically increasing).
/// A global frame covered by two windows is decided by the window where it sits
/// farther from an edge ("center-most wins"); first/last windows keep their edge
/// decisions.
fn stitch_activity(
    windows: &[Vec<[bool; LOCAL_SPEAKERS]>],
    offsets: &[usize],
    step: f32,
) -> StitchedTimeline {
    let n_win = windows.len();
    if n_win == 0 || windows[0].is_empty() {
        return StitchedTimeline {
            step,
            active: Vec::new(),
            exclusive: Vec::new(),
            n_tracks: 0,
        };
    }
    let f_per = windows[0].len();
    let offset = |w: usize| -> usize { offsets[w] };
    let total = offset(n_win - 1) + f_per;

    let slot_active = |w: usize, s: usize| -> bool { windows[w].iter().any(|fr| fr[s]) };

    // Alignment chain: local slot -> global track id, window by window.
    let mut maps: Vec<[usize; LOCAL_SPEAKERS]> = Vec::with_capacity(n_win);
    let mut next_track = 0usize;

    // Window 0: allocate a fresh track for each locally-active slot.
    let mut m0 = [usize::MAX; LOCAL_SPEAKERS];
    for (s, slot) in m0.iter_mut().enumerate() {
        if slot_active(0, s) {
            *slot = next_track;
            next_track += 1;
        }
    }
    maps.push(m0);

    for w in 1..n_win {
        let ov_start = offset(w);
        let ov_end = offset(w - 1) + f_per; // exclusive; < offset(w) + f_per
        let mut prev_ov: Vec<[bool; LOCAL_SPEAKERS]> = Vec::new();
        let mut next_ov: Vec<[bool; LOCAL_SPEAKERS]> = Vec::new();
        for g in ov_start..ov_end {
            let pf = g - offset(w - 1);
            let nf = g - offset(w);
            if pf < f_per && nf < f_per {
                prev_ov.push(windows[w - 1][pf]);
                next_ov.push(windows[w][nf]);
            }
        }
        let rel = align_local_speakers(&prev_ov, &next_ov);
        let prev_map = maps[w - 1];
        let mut m = [usize::MAX; LOCAL_SPEAKERS];
        for s in 0..LOCAL_SPEAKERS {
            if rel[s] != usize::MAX && prev_map[rel[s]] != usize::MAX {
                m[s] = prev_map[rel[s]];
            } else if slot_active(w, s) {
                m[s] = next_track;
                next_track += 1;
            }
        }
        maps.push(m);
    }
    let n_tracks = next_track;

    // Assign each global frame to a covering window. Among the windows covering
    // a frame we trust the one that resolves the *most* local speakers there
    // (a window that missed speech or smeared two voices into one slot resolves
    // fewer), and break ties by "center-most wins" — the window where the frame
    // sits farther from an edge has more receptive-field context. First/last
    // windows keep their edge decisions (nothing else covers those frames).
    let mut winner_w = vec![0usize; total];
    let mut winner_f = vec![0usize; total];
    let mut winner_key = vec![(-1i64, -1i64); total]; // (active_count, dist)
    for w in 0..n_win {
        let base = offset(w);
        for f in 0..f_per {
            let g = base + f;
            if g >= total {
                break;
            }
            let count = windows[w][f].iter().filter(|&&a| a).count() as i64;
            let dist = f.min(f_per - 1 - f) as i64;
            let key = (count, dist);
            if key > winner_key[g] {
                winner_key[g] = key;
                winner_w[g] = w;
                winner_f[g] = f;
            }
        }
    }

    let mut active = vec![0u64; total];
    let mut exclusive = vec![None; total];
    for g in 0..total {
        let w = winner_w[g];
        let f = winner_f[g];
        let mut mask = 0u64;
        let mut count = 0usize;
        let mut last = 0usize;
        for s in 0..LOCAL_SPEAKERS {
            if windows[w][f][s] {
                let tr = maps[w][s];
                if tr == usize::MAX {
                    continue;
                }
                if tr < 64 {
                    mask |= 1u64 << tr;
                }
                count += 1;
                last = tr;
            }
        }
        active[g] = mask;
        exclusive[g] = if count == 1 { Some(last) } else { None };
    }

    StitchedTimeline {
        step,
        active,
        exclusive,
        n_tracks,
    }
}

/// Run segmentation over sliding windows (`SEG_WINDOW`, hop `SEG_WINDOW / 2`) and
/// stitch the per-window local speaker slots into a global timeline.
fn stitch_windows(
    model: &mut DiarizationModel,
    audio: &[f32],
) -> Result<StitchedTimeline, DiarizationModelError> {
    let len = audio.len();
    if len == 0 {
        return Ok(StitchedTimeline {
            step: 0.0,
            active: Vec::new(),
            exclusive: Vec::new(),
            n_tracks: 0,
        });
    }

    let hop = SEG_WINDOW / 2;
    let mut starts: Vec<usize> = Vec::new();
    let mut k = 0usize;
    while k * hop < len {
        starts.push(k * hop);
        k += 1;
    }
    if starts.is_empty() {
        starts.push(0);
    }

    let mut windows: Vec<Vec<[bool; LOCAL_SPEAKERS]>> = Vec::with_capacity(starts.len());
    let mut f_per = 0usize;
    for &w0 in &starts {
        let w1 = (w0 + SEG_WINDOW).min(len);
        let window = &audio[w0..w1];
        let logits = model.segment_window(window)?;
        let frames = logits.shape()[1];
        f_per = frames;
        let mut act = vec![[false; LOCAL_SPEAKERS]; frames];
        for f in 0..frames {
            let mut best = 0usize;
            let mut best_v = f32::NEG_INFINITY;
            for c in 0..POWERSET.len() {
                let v = logits[[0, f, c]];
                if v > best_v {
                    best_v = v;
                    best = c;
                }
            }
            for &s in POWERSET[best] {
                act[f][s] = true;
            }
        }
        windows.push(act);
    }

    if f_per == 0 {
        return Ok(StitchedTimeline {
            step: 0.0,
            active: Vec::new(),
            exclusive: Vec::new(),
            n_tracks: 0,
        });
    }

    let step = SEG_WINDOW as f32 / f_per as f32 / SR as f32; // seconds/frame
    // Global frame index of each window's start sample (true float grid, so
    // there is no cumulative drift between windows).
    let offsets: Vec<usize> = starts
        .iter()
        .map(|&w0| (w0 as f64 * f_per as f64 / SEG_WINDOW as f64).round() as usize)
        .collect();
    let mut timeline = stitch_activity(&windows, &offsets, step);

    // Trim padding-only tail frames beyond the real audio duration.
    let real_frames = sample_to_frame(len, f_per);
    if timeline.active.len() > real_frames {
        timeline.active.truncate(real_frames);
        timeline.exclusive.truncate(real_frames);
    }

    Ok(timeline)
}

/// Global frame index nearest a sample position on the true segmentation grid
/// (`SEG_WINDOW` samples span `f_per` frames). Used consistently for offsets and
/// for run/embedding sample boundaries so no integer rounding drifts.
fn sample_to_frame(sample: usize, f_per: usize) -> usize {
    (sample as f64 * f_per as f64 / SEG_WINDOW as f64).round() as usize
}

/// First audio sample of global frame `g` on the true segmentation grid.
fn frame_to_sample(g: usize, f_per: usize) -> usize {
    (g as f64 * SEG_WINDOW as f64 / f_per as f64).round() as usize
}

/// Extract per-track speech runs from the stitched timeline: bridge gaps
/// <= 250 ms, drop runs < 400 ms — both measured on the whole-recording timeline
/// so a run straddling a window boundary survives as a single run.
fn extract_runs(timeline: &StitchedTimeline) -> Vec<Run> {
    let frames = timeline.active.len();
    if frames == 0 || timeline.step <= 0.0 {
        return Vec::new();
    }
    let bridge = (0.25 / timeline.step).round() as usize; // bridge gaps <= 250 ms
    let min_frames = (0.4 / timeline.step).round() as usize; // drop < 400 ms

    let mut runs = Vec::new();
    for track in 0..timeline.n_tracks.min(64) {
        let bit = 1u64 << track;
        let is_active = |f: usize| (timeline.active[f] & bit) != 0;
        let mut f = 0usize;
        while f < frames {
            if !is_active(f) {
                f += 1;
                continue;
            }
            let start = f;
            let mut end = f;
            let mut gap = 0usize;
            let mut g = f + 1;
            while g < frames {
                if is_active(g) {
                    end = g;
                    gap = 0;
                } else {
                    gap += 1;
                    if gap > bridge {
                        break;
                    }
                }
                g += 1;
            }
            f = end + 1;
            if end - start + 1 < min_frames {
                continue;
            }
            runs.push(Run {
                track,
                start_f: start,
                end_f: end,
            });
        }
    }
    runs
}

/// Extract single-speaker speech regions with embeddings by stitching the
/// overlapping segmentation windows into a global timeline, extracting per-track
/// runs globally, and embedding the exclusive (single-active-speaker) audio of
/// each run. Embedding logic is unchanged from the D1 spike (exclusive samples,
/// whole-run fallback below `SR / 3`); chunked averaging is a later task.
fn extract_regions(
    model: &mut DiarizationModel,
    audio: &[f32],
) -> Result<Vec<Region>, DiarizationModelError> {
    let timeline = stitch_windows(model, audio)?;
    let step = timeline.step;
    if step <= 0.0 {
        return Ok(Vec::new());
    }
    // Recover the frames-per-window from the step to map global frames to samples
    // on the same true grid the offsets used (no cumulative drift).
    let f_per = (SEG_WINDOW as f64 / (step as f64 * SR as f64)).round() as usize;
    if f_per == 0 {
        return Ok(Vec::new());
    }

    let runs = extract_runs(&timeline);
    let mut regions = Vec::new();
    for run in runs {
        // gather exclusive samples for a clean embedding
        let mut clean: Vec<f32> = Vec::new();
        for fr in run.start_f..=run.end_f {
            if timeline.exclusive[fr] == Some(run.track) {
                let s0 = frame_to_sample(fr, f_per);
                let s1 = frame_to_sample(fr + 1, f_per).min(audio.len());
                if s0 < audio.len() {
                    clean.extend_from_slice(&audio[s0..s1]);
                }
            }
        }
        // fallback: whole run if not enough exclusive audio
        let seg_samples = if clean.len() >= SR / 3 {
            clean
        } else {
            let s0 = frame_to_sample(run.start_f, f_per).min(audio.len());
            let s1 = frame_to_sample(run.end_f + 1, f_per).min(audio.len());
            audio[s0..s1].to_vec()
        };

        if let Some(e) = model.embed(&seg_samples)? {
            regions.push(Region {
                start: run.start_f as f32 * step,
                end: (run.end_f + 1) as f32 * step,
                embedding: e,
            });
        }
    }

    regions.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap_or(std::cmp::Ordering::Equal));
    Ok(regions)
}

/// The diarization engine: owns the loaded model and download state.
pub struct DiarizationEngine {
    models_dir: PathBuf,
    current_model: Arc<RwLock<Option<DiarizationModel>>>,
    available: Arc<RwLock<Option<DiarizationModelInfo>>>,
    active_downloads: Arc<RwLock<HashSet<String>>>,
    cancel_flag: Arc<RwLock<bool>>,
}

impl DiarizationEngine {
    /// Create an engine, storing models under `<models_dir>/diarization`.
    pub fn new_with_models_dir(models_dir: Option<PathBuf>) -> Result<Self> {
        let models_dir = if let Some(dir) = models_dir {
            dir.join("diarization")
        } else {
            let current_dir = std::env::current_dir()
                .map_err(|e| anyhow!("Failed to get current directory: {}", e))?;
            if cfg!(debug_assertions) {
                current_dir.join("models").join("diarization")
            } else {
                dirs::data_dir()
                    .or_else(|| dirs::home_dir())
                    .ok_or_else(|| anyhow!("Could not find system data directory"))?
                    .join("Meetily")
                    .join("models")
                    .join("diarization")
            }
        };

        log::info!(
            "DiarizationEngine using models directory: {}",
            models_dir.display()
        );

        if !models_dir.exists() {
            std::fs::create_dir_all(&models_dir)?;
        }

        Ok(Self {
            models_dir,
            current_model: Arc::new(RwLock::new(None)),
            available: Arc::new(RwLock::new(None)),
            active_downloads: Arc::new(RwLock::new(HashSet::new())),
            cancel_flag: Arc::new(RwLock::new(false)),
        })
    }

    fn model_dir(&self) -> PathBuf {
        self.models_dir.join(DIARIZATION_MODEL_NAME)
    }

    fn is_on_disk(&self) -> bool {
        let dir = self.model_dir();
        dir.join(SEGMENTATION_FILE).exists() && dir.join(EMBEDDING_FILE).exists()
    }

    /// Report the single bundled model's current status.
    pub async fn discover_model(&self) -> DiarizationModelInfo {
        let downloading = self
            .active_downloads
            .read()
            .await
            .contains(DIARIZATION_MODEL_NAME);

        let status = if downloading {
            DiarizationModelStatus::Downloading { progress: 0 }
        } else if self.is_on_disk() {
            DiarizationModelStatus::Available
        } else {
            DiarizationModelStatus::Missing
        };

        let info = DiarizationModelInfo {
            name: DIARIZATION_MODEL_NAME.to_string(),
            path: self.model_dir(),
            size_mb: 33,
            status,
            description:
                "pyannote segmentation-3.0 + wespeaker resnet34 (speaker diarization)".to_string(),
        };

        *self.available.write().await = Some(info.clone());
        info
    }

    /// Whether the model pair is present on disk and ready to load.
    pub async fn is_available(&self) -> bool {
        self.is_on_disk()
    }

    /// Whether a model is currently loaded in memory.
    pub async fn is_model_loaded(&self) -> bool {
        self.current_model.read().await.is_some()
    }

    /// Ensure the model is loaded into memory (loads from disk on first call).
    pub async fn ensure_loaded(&self) -> Result<()> {
        if self.current_model.read().await.is_some() {
            return Ok(());
        }
        if !self.is_on_disk() {
            return Err(anyhow!(
                "Diarization model is not downloaded. Download it before diarizing."
            ));
        }
        let dir = self.model_dir();
        // Model loading is CPU/IO heavy; run off the async reactor.
        let model = tokio::task::spawn_blocking(move || DiarizationModel::new(&dir))
            .await
            .map_err(|e| anyhow!("Model load task panicked: {}", e))?
            .map_err(|e| anyhow!("Failed to load diarization model: {}", e))?;
        *self.current_model.write().await = Some(model);
        log::info!("Diarization model loaded into memory");
        Ok(())
    }

    /// Unload the in-memory model (frees the ONNX sessions).
    pub async fn unload_model(&self) -> bool {
        self.current_model.write().await.take().is_some()
    }

    pub async fn get_models_directory(&self) -> PathBuf {
        self.models_dir.clone()
    }

    /// Diarize 16 kHz mono samples into per-speaker turns.
    ///
    /// - `num_speakers`: when `Some(k)`, fixes the cluster count to exactly `k`;
    ///   otherwise uses the calibrated cosine-distance cut.
    ///
    /// Requires the model to be loaded (`ensure_loaded`). The heavy inference runs
    /// on a blocking thread.
    pub async fn diarize(
        &self,
        samples_16k: &[f32],
        num_speakers: Option<usize>,
    ) -> Result<Vec<SpeakerTurn>> {
        self.ensure_loaded().await?;

        let samples = samples_16k.to_vec();
        let model_arc = self.current_model.clone();

        // Run segmentation + embedding on a blocking thread while holding the
        // write lock (diarization is single-flight per engine instance).
        let regions = tokio::task::spawn_blocking(move || -> Result<Vec<Region>> {
            let mut guard = model_arc.blocking_write();
            let model = guard
                .as_mut()
                .ok_or_else(|| anyhow!("Diarization model not loaded"))?;
            extract_regions(model, &samples)
                .map_err(|e| anyhow!("Region extraction failed: {}", e))
        })
        .await
        .map_err(|e| anyhow!("Diarization task panicked: {}", e))??;

        if regions.is_empty() {
            return Ok(Vec::new());
        }

        let embeddings: Vec<Vec<f32>> = regions.iter().map(|r| r.embedding.clone()).collect();
        let labels = cluster_agglomerative(
            &embeddings,
            Some(CLUSTER_COSINE_DISTANCE_CUT),
            num_speakers,
        );

        let turns = regions
            .into_iter()
            .zip(labels)
            .map(|(r, cluster_id)| SpeakerTurn {
                start_secs: r.start,
                end_secs: r.end,
                cluster_id,
                embedding: r.embedding,
            })
            .collect();

        Ok(turns)
    }

    /// Set/clear the cancellation flag for an in-flight download.
    pub async fn cancel_download(&self) {
        *self.cancel_flag.write().await = true;
        self.active_downloads
            .write()
            .await
            .remove(DIARIZATION_MODEL_NAME);
    }

    /// Download and install the diarization model pair, reporting weighted
    /// progress. Idempotent: returns early if already present.
    pub async fn download_model(
        &self,
        progress_callback: Option<Box<dyn Fn(DiarizationDownloadProgress) + Send + Sync>>,
    ) -> Result<()> {
        if self.is_on_disk() {
            log::info!("Diarization model already present; skipping download");
            if let Some(cb) = &progress_callback {
                cb(DiarizationDownloadProgress::new(1, 1, 0.0));
            }
            return Ok(());
        }

        {
            let mut active = self.active_downloads.write().await;
            if active.contains(DIARIZATION_MODEL_NAME) {
                return Err(anyhow!("Diarization model download already in progress"));
            }
            active.insert(DIARIZATION_MODEL_NAME.to_string());
        }
        *self.cancel_flag.write().await = false;

        let result = self.download_inner(progress_callback).await;

        self.active_downloads
            .write()
            .await
            .remove(DIARIZATION_MODEL_NAME);

        result
    }

    async fn download_inner(
        &self,
        progress_callback: Option<Box<dyn Fn(DiarizationDownloadProgress) + Send + Sync>>,
    ) -> Result<()> {
        let dir = self.model_dir();
        fs::create_dir_all(&dir)
            .await
            .map_err(|e| anyhow!("Failed to create diarization model dir: {}", e))?;

        let total_bytes = SEGMENTATION_APPROX_BYTES + EMBEDDING_APPROX_BYTES;
        let client = reqwest::Client::builder()
            .tcp_nodelay(true)
            .timeout(Duration::from_secs(1800))
            .connect_timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| anyhow!("Failed to create HTTP client: {}", e))?;

        let start = Instant::now();
        let mut base_downloaded: u64 = 0;

        // 1) Segmentation tarball -> extract model.onnx -> segmentation.onnx
        let seg_tarball = dir.join("segmentation.tar.bz2");
        self.download_file(
            &client,
            SEGMENTATION_URL,
            &seg_tarball,
            total_bytes,
            base_downloaded,
            start,
            &progress_callback,
        )
        .await?;
        extract_segmentation_onnx(&seg_tarball, &dir.join(SEGMENTATION_FILE))
            .map_err(|e| anyhow!("Failed to extract segmentation model: {}", e))?;
        let _ = fs::remove_file(&seg_tarball).await;
        base_downloaded += SEGMENTATION_APPROX_BYTES;

        // 2) Embedding .onnx (direct)
        self.download_file(
            &client,
            EMBEDDING_URL,
            &dir.join(EMBEDDING_FILE),
            total_bytes,
            base_downloaded,
            start,
            &progress_callback,
        )
        .await?;

        if let Some(cb) = &progress_callback {
            cb(DiarizationDownloadProgress::new(total_bytes, total_bytes, 0.0));
        }

        log::info!("Diarization model download complete");
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn download_file(
        &self,
        client: &reqwest::Client,
        url: &str,
        dest: &Path,
        total_bytes: u64,
        base_downloaded: u64,
        start: Instant,
        progress_callback: &Option<Box<dyn Fn(DiarizationDownloadProgress) + Send + Sync>>,
    ) -> Result<()> {
        log::info!("Downloading diarization file: {} -> {}", url, dest.display());
        let response = client
            .get(url)
            .send()
            .await
            .map_err(|e| anyhow!("Failed to start download {}: {}", url, e))?;
        if !response.status().is_success() {
            return Err(anyhow!(
                "Download failed for {} with status {}",
                url,
                response.status()
            ));
        }

        let file = fs::File::create(dest)
            .await
            .map_err(|e| anyhow!("Failed to create {}: {}", dest.display(), e))?;
        let mut writer = BufWriter::with_capacity(4 * 1024 * 1024, file);

        use futures_util::StreamExt;
        let mut stream = response.bytes_stream();
        let mut this_file: u64 = 0;
        let mut last_report = Instant::now();

        loop {
            if *self.cancel_flag.read().await {
                let _ = writer.flush().await;
                let _ = fs::remove_file(dest).await;
                return Err(anyhow!("Download cancelled by user"));
            }

            let next = timeout(Duration::from_secs(60), stream.next()).await;
            let chunk = match next {
                Err(_) => return Err(anyhow!("Download timeout (no data for 60s)")),
                Ok(None) => break,
                Ok(Some(Ok(c))) => c,
                Ok(Some(Err(e))) => return Err(anyhow!("Download stream error: {}", e)),
            };

            writer
                .write_all(&chunk)
                .await
                .map_err(|e| anyhow!("Failed to write chunk: {}", e))?;
            this_file += chunk.len() as u64;

            if last_report.elapsed() >= Duration::from_millis(400) {
                if let Some(cb) = progress_callback {
                    let downloaded = base_downloaded + this_file;
                    let elapsed = start.elapsed().as_secs_f64().max(0.001);
                    let speed = (downloaded as f64 / (1024.0 * 1024.0)) / elapsed;
                    cb(DiarizationDownloadProgress::new(downloaded, total_bytes, speed));
                }
                last_report = Instant::now();
            }
        }

        writer
            .flush()
            .await
            .map_err(|e| anyhow!("Failed to flush {}: {}", dest.display(), e))?;
        Ok(())
    }
}

/// Extract the (non-int8) `model.onnx` member from a sherpa pyannote
/// `.tar.bz2` into `dest`. Runs synchronously (small archive).
fn extract_segmentation_onnx(tarball: &Path, dest: &Path) -> Result<()> {
    let file = std::fs::File::open(tarball)?;
    let bz = bzip2::read::BzDecoder::new(file);
    let mut archive = tar::Archive::new(bz);

    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.to_path_buf();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_string();
        // The real weights are `model.onnx`; skip `model.int8.onnx`.
        if name == "model.onnx" {
            let mut out = std::fs::File::create(dest)?;
            std::io::copy(&mut entry, &mut out)?;
            log::info!("Extracted segmentation model to {}", dest.display());
            return Ok(());
        }
    }
    Err(anyhow!("model.onnx not found in segmentation archive"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a deterministic pseudo-embedding around a seed direction. Vectors
    /// from the same `base` cluster tightly (high cosine similarity); different
    /// bases are near-orthogonal.
    fn synth_embedding(base: usize, jitter: usize, dim: usize) -> Vec<f32> {
        let mut v = vec![0.0f32; dim];
        // Dominant energy in the base's band.
        for i in 0..dim {
            let band = i % 8;
            v[i] = if band == base { 1.0 } else { 0.0 };
        }
        // Small deterministic jitter so intra-cluster vectors are not identical.
        let idx = (base + jitter) % dim;
        v[idx] += 0.05 * (jitter as f32 + 1.0);
        // L2 normalize (diarize consumers assume normalized embeddings).
        let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-9);
        for x in v.iter_mut() {
            *x /= norm;
        }
        v
    }

    #[test]
    fn test_cluster_empty_and_single() {
        assert!(cluster_agglomerative(&[], Some(0.5), None).is_empty());
        let one = vec![vec![1.0, 0.0, 0.0]];
        assert_eq!(cluster_agglomerative(&one, Some(0.5), None), vec![0]);
    }

    #[test]
    fn test_cluster_separates_three_speakers() {
        // Three well-separated speakers, 3 samples each.
        let mut embs = Vec::new();
        for base in 0..3 {
            for j in 0..3 {
                embs.push(synth_embedding(base, j, 32));
            }
        }
        let labels = cluster_agglomerative(&embs, Some(CLUSTER_COSINE_DISTANCE_CUT), None);

        // Each contiguous group of 3 must share one label.
        for base in 0..3 {
            let group = &labels[base * 3..base * 3 + 3];
            assert!(
                group.iter().all(|l| *l == group[0]),
                "speaker {} split across clusters: {:?}",
                base,
                group
            );
        }
        // And distinct speakers must land in distinct clusters.
        let distinct: std::collections::HashSet<_> =
            [labels[0], labels[3], labels[6]].into_iter().collect();
        assert_eq!(distinct.len(), 3, "expected 3 clusters, got {:?}", labels);
    }

    #[test]
    fn test_cluster_fixed_k_overrides_threshold() {
        // Two speakers but force k=1: everything collapses to one cluster.
        let embs = vec![
            synth_embedding(0, 0, 32),
            synth_embedding(0, 1, 32),
            synth_embedding(4, 0, 32),
            synth_embedding(4, 1, 32),
        ];
        let labels = cluster_agglomerative(&embs, Some(CLUSTER_COSINE_DISTANCE_CUT), Some(1));
        assert!(labels.iter().all(|l| *l == 0), "fixed_k=1 should yield one cluster: {:?}", labels);

        // Force k=2 on the same data -> exactly two clusters.
        let labels2 = cluster_agglomerative(&embs, None, Some(2));
        let distinct: std::collections::HashSet<_> = labels2.iter().copied().collect();
        assert_eq!(distinct.len(), 2, "fixed_k=2 should yield two clusters: {:?}", labels2);
    }

    #[test]
    fn test_cluster_labels_are_contiguous_from_zero() {
        let embs = vec![
            synth_embedding(1, 0, 16),
            synth_embedding(5, 0, 16),
            synth_embedding(1, 1, 16),
        ];
        let labels = cluster_agglomerative(&embs, Some(CLUSTER_COSINE_DISTANCE_CUT), None);
        let mut distinct: Vec<usize> = labels.clone();
        distinct.sort_unstable();
        distinct.dedup();
        // Labels must be 0..k with no gaps.
        for (i, l) in distinct.iter().enumerate() {
            assert_eq!(i, *l, "labels not contiguous: {:?}", labels);
        }
        // First region always gets label 0 (deterministic relabeling).
        assert_eq!(labels[0], 0);
    }

    #[test]
    fn test_cos_sim_orthogonal_and_identical() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![0.0, 1.0, 0.0];
        assert!((cos_sim(&a, &a) - 1.0).abs() < 1e-6);
        assert!(cos_sim(&a, &b).abs() < 1e-6);
    }

    // ---- Window stitching (task 4) ----------------------------------------

    /// Build a `[frames][3]` activity matrix. `spans[s]` is a list of inclusive
    /// `(start, end)` frame ranges where local slot `s` is active.
    fn activity(frames: usize, spans: [&[(usize, usize)]; LOCAL_SPEAKERS]) -> Vec<[bool; 3]> {
        let mut m = vec![[false; 3]; frames];
        for (s, ranges) in spans.iter().enumerate() {
            for &(a, b) in ranges.iter() {
                for f in a..=b {
                    m[f][s] = true;
                }
            }
        }
        m
    }

    #[test]
    fn test_align_identity_mapping() {
        // Three slots, each active in a distinct third; prev == next.
        let prev = activity(15, [&[(0, 4)], &[(5, 9)], &[(10, 14)]]);
        let next = prev.clone();
        assert_eq!(align_local_speakers(&prev, &next), [0, 1, 2]);
    }

    #[test]
    fn test_align_swapped_speakers() {
        // next slot 0 sits where prev slot 1 spoke, next slot 1 where prev 0.
        let prev = activity(15, [&[(0, 4)], &[(5, 9)], &[(10, 14)]]);
        let next = activity(15, [&[(5, 9)], &[(0, 4)], &[(10, 14)]]);
        assert_eq!(align_local_speakers(&prev, &next), [1, 0, 2]);
    }

    #[test]
    fn test_align_new_speaker_appears() {
        // prev only slot 0 active in [0,4]. next slot 0 matches it; next slot 1
        // is active where nobody spoke before -> new; slot 2 silent -> new.
        let prev = activity(10, [&[(0, 4)], &[], &[]]);
        let next = activity(10, [&[(0, 4)], &[(5, 9)], &[]]);
        let rel = align_local_speakers(&prev, &next);
        assert_eq!(rel[0], 0, "slot 0 should match prev slot 0");
        assert_eq!(rel[1], usize::MAX, "slot 1 is a new voice");
        assert_eq!(rel[2], usize::MAX, "slot 2 is silent -> unmatched");
    }

    /// Build a single-track-per-bit timeline from explicit active spans.
    fn timeline_from(
        step: f32,
        frames: usize,
        n_tracks: usize,
        tracks: &[(usize, &[(usize, usize)])],
    ) -> StitchedTimeline {
        let mut active = vec![0u64; frames];
        for &(t, spans) in tracks {
            for &(a, b) in spans {
                for f in a..=b {
                    active[f] |= 1u64 << t;
                }
            }
        }
        let exclusive = active
            .iter()
            .map(|&m| {
                if m.count_ones() == 1 {
                    Some(m.trailing_zeros() as usize)
                } else {
                    None
                }
            })
            .collect();
        StitchedTimeline {
            step,
            active,
            exclusive,
            n_tracks,
        }
    }

    #[test]
    fn test_extract_runs_boundary_straddle_retained() {
        // step = 50 ms/frame -> min 8 frames (400 ms), bridge 5 frames (250 ms).
        // A 700 ms run (14 frames) that would be two 350 ms fragments under
        // per-window extraction is retained as one global run.
        let step = 0.05;
        let tl = timeline_from(step, 20, 1, &[(0, &[(3, 16)])]);
        let runs = extract_runs(&tl);
        assert_eq!(runs, vec![Run { track: 0, start_f: 3, end_f: 16 }]);
    }

    #[test]
    fn test_extract_runs_gap_bridging_and_min_duration() {
        let step = 0.05; // min 8 frames, bridge 5 frames
        // track 0: [0,4] then 4-frame gap [5,8] (<=5) then [9,13] -> bridged
        //          into a single 14-frame run.
        // track 1: a lone 6-frame run (300 ms) -> dropped (< 400 ms).
        let tl = timeline_from(
            step,
            40,
            2,
            &[
                (0, &[(0, 4), (9, 13)]),
                (1, &[(20, 25)]),
            ],
        );
        let runs = extract_runs(&tl);
        assert_eq!(
            runs,
            vec![Run { track: 0, start_f: 0, end_f: 13 }],
            "track 0 bridged; track 1's 300 ms run dropped"
        );
    }

    #[test]
    fn test_extract_runs_wide_gap_splits() {
        let step = 0.05; // bridge 5 frames
        // A 6-frame gap (> bridge) splits into two runs, both >= 400 ms.
        let tl = timeline_from(step, 40, 1, &[(0, &[(0, 9), (16, 25)])]);
        let runs = extract_runs(&tl);
        assert_eq!(
            runs,
            vec![
                Run { track: 0, start_f: 0, end_f: 9 },
                Run { track: 0, start_f: 16, end_f: 25 },
            ]
        );
    }

    /// Non-empty per-track membership vectors over the timeline frames, sorted —
    /// a canonical form invariant to global-track renaming.
    fn track_signatures(tl: &StitchedTimeline) -> Vec<Vec<bool>> {
        let frames = tl.active.len();
        let mut sigs: Vec<Vec<bool>> = (0..tl.n_tracks.min(64))
            .map(|t| {
                let bit = 1u64 << t;
                (0..frames).map(|f| (tl.active[f] & bit) != 0).collect()
            })
            .filter(|sig: &Vec<bool>| sig.iter().any(|&b| b))
            .collect();
        sigs.sort();
        sigs
    }

    #[test]
    fn test_stitch_permutation_invariant() {
        use rand::rngs::StdRng;
        use rand::seq::SliceRandom;
        use rand::{Rng, SeedableRng};

        let f_per = 40usize;
        let step = SEG_WINDOW as f32 / f_per as f32 / SR as f32;
        let offset = |w: usize| -> usize { ((w as f32) * (f_per as f32) / 2.0).round() as usize };

        for case in 0..200u64 {
            let mut rng = StdRng::seed_from_u64(0xD1A5_0000 + case);
            let n_win = rng.gen_range(2..=5);
            let total = offset(n_win - 1) + f_per;

            // Build a consistent global speaker timeline the way real overlapping
            // segmentation windows see it: up to 3 speakers take turns in
            // contiguous blobs (with silence gaps), so every window observes the
            // same speaker at the same absolute frame. This is what makes
            // cross-window alignment well-posed.
            let mut spk = vec![usize::MAX; total]; // usize::MAX = silence
            {
                let mut g = 0usize;
                let mut who = rng.gen_range(0..3);
                while g < total {
                    if rng.gen_bool(0.2) {
                        // silence gap
                        let gap = rng.gen_range(1..=f_per / 4);
                        g = (g + gap).min(total);
                    } else {
                        let len = rng.gen_range(f_per / 4..=f_per);
                        let end = (g + len).min(total);
                        for cell in spk.iter_mut().take(end).skip(g) {
                            *cell = who;
                        }
                        g = end;
                        who = (who + 1 + rng.gen_range(0..2)) % 3; // advance speaker
                    }
                }
            }

            // Each window observes the active speakers through an arbitrary
            // (per-window) speaker -> local-slot assignment, exactly what the
            // segmentation model produces window to window.
            let mut windows: Vec<Vec<[bool; 3]>> = Vec::with_capacity(n_win);
            for w in 0..n_win {
                let mut slot_of = [0usize, 1, 2];
                slot_of.shuffle(&mut rng); // speaker k -> local slot slot_of[k]
                let mut m = vec![[false; 3]; f_per];
                for (f, frame) in m.iter_mut().enumerate() {
                    let g = offset(w) + f;
                    if g < total && spk[g] != usize::MAX {
                        frame[slot_of[spk[g]]] = true;
                    }
                }
                windows.push(m);
            }

            // Permute the local slots of one randomly chosen window.
            let w_idx = rng.gen_range(0..n_win);
            let perm = SLOT_PERMS[rng.gen_range(0..SLOT_PERMS.len())];
            let mut permuted = windows.clone();
            for frame in permuted[w_idx].iter_mut() {
                let orig = *frame;
                for i in 0..3 {
                    frame[i] = orig[perm[i]];
                }
            }

            let offsets: Vec<usize> = (0..n_win).map(offset).collect();
            let base = stitch_activity(&windows, &offsets, step);
            let alt = stitch_activity(&permuted, &offsets, step);

            assert_eq!(
                track_signatures(&base),
                track_signatures(&alt),
                "case {case}: permuting window {w_idx} by {perm:?} changed the global timeline"
            );
        }
    }
}
