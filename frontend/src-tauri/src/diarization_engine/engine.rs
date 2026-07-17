//! Diarization engine: model lifecycle, HuggingFace/GitHub download manager, and
//! the central `diarize()` API (segmentation -> embedding -> agglomerative
//! cosine clustering). Mirrors the structure of `parakeet_engine`.

use crate::diarization_engine::model::{
    DiarizationModel, DiarizationModelError, EMBEDDING_FILE, POWERSET, SEGMENTATION_FILE,
    SEG_WINDOW, SR,
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

/// Extract single-speaker speech regions with embeddings by sliding the 10 s
/// segmentation window and embedding the exclusive (single-active-speaker) audio
/// of each contiguous run. Ported from the D1 spike.
fn extract_regions(
    model: &mut DiarizationModel,
    audio: &[f32],
) -> Result<Vec<Region>, DiarizationModelError> {
    let mut regions = Vec::new();
    let n_windows = audio.len().div_ceil(SEG_WINDOW);

    for w in 0..n_windows {
        let w0 = w * SEG_WINDOW;
        let w1 = (w0 + SEG_WINDOW).min(audio.len());
        let window = &audio[w0..w1];
        let logits = model.segment_window(window)?;
        let frames = logits.shape()[1];
        if frames == 0 {
            continue;
        }
        let step = SEG_WINDOW as f32 / frames as f32 / SR as f32; // seconds/frame
        let win_start = w0 as f32 / SR as f32;
        let samples_per_frame = SEG_WINDOW / frames;

        // per-frame active local speakers + exclusivity
        let mut active = vec![[false; 3]; frames];
        let mut exclusive = vec![usize::MAX; frames];
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
            let set = POWERSET[best];
            for &s in set {
                active[f][s] = true;
            }
            if set.len() == 1 {
                exclusive[f] = set[0];
            }
        }

        let bridge = (0.25 / step).round() as usize; // bridge gaps <= 250 ms
        let min_frames = (0.4 / step).round() as usize; // drop < 400 ms

        for spk in 0..3 {
            let mut f = 0usize;
            while f < frames {
                if !active[f][spk] {
                    f += 1;
                    continue;
                }
                let start = f;
                let mut end = f;
                let mut gap = 0usize;
                let mut g = f + 1;
                while g < frames {
                    if active[g][spk] {
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

                // gather exclusive samples for a clean embedding
                let mut clean: Vec<f32> = Vec::new();
                for fr in start..=end {
                    if exclusive[fr] == spk {
                        let s0 = w0 + fr * samples_per_frame;
                        let s1 = (s0 + samples_per_frame).min(audio.len());
                        if s0 < audio.len() {
                            clean.extend_from_slice(&audio[s0..s1]);
                        }
                    }
                }
                // fallback: whole run if not enough exclusive audio
                let seg_samples = if clean.len() >= SR / 3 {
                    clean
                } else {
                    let s0 = (w0 + start * samples_per_frame).min(audio.len());
                    let s1 = (w0 + (end + 1) * samples_per_frame).min(audio.len());
                    audio[s0..s1].to_vec()
                };

                if let Some(e) = model.embed(&seg_samples)? {
                    regions.push(Region {
                        start: win_start + start as f32 * step,
                        end: win_start + (end + 1) as f32 * step,
                        embedding: e,
                    });
                }
            }
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
}
