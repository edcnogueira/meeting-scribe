//! ONNX model wrappers for speaker diarization.
//!
//! Two models run on `ort` (same runtime as `parakeet_engine`):
//!   - pyannote segmentation-3.0 (powerset VAD): raw 16 kHz waveform -> per-frame
//!     active-speaker logits.
//!   - wespeaker voxceleb resnet34: kaldi fbank features -> 256-d speaker embedding.
//!
//! The inference/fbank/powerset logic is ported from the validated D1 spike
//! (`scratch/diarization-spike/src/main.rs`), swapping `rustfft` for the crate
//! already vendored in the app (`realfft`).

use ndarray::{Array3, ArrayD};
use ort::execution_providers::CPUExecutionProvider;
use ort::inputs;
use ort::session::builder::GraphOptimizationLevel;
use ort::session::Session;
use ort::value::TensorRef;
use realfft::num_complex::Complex32;
use realfft::RealFftPlanner;

use std::path::Path;

/// Diarization sample rate (both models expect 16 kHz mono).
pub const SR: usize = 16000;
/// Segmentation window length in samples (10 s), from the pyannote model metadata.
pub const SEG_WINDOW: usize = 160_000;
/// Number of powerset classes for `num_speakers=3, max_classes=2`.
pub const POWERSET_CLASSES: usize = 7;
/// Number of local speakers the segmentation model resolves per window.
pub const LOCAL_SPEAKERS: usize = 3;

const N_MELS: usize = 80;
const FFT_N: usize = 512;
const FRAME_LEN: usize = 400; // 25 ms
const FRAME_SHIFT: usize = 160; // 10 ms
const PREEMPH: f32 = 0.97;

/// pyannote-segmentation-3.0 powerset mapping (increasing cardinality order).
pub const POWERSET: [&[usize]; POWERSET_CLASSES] = [
    &[],     // 0: silence
    &[0],    // 1
    &[1],    // 2
    &[2],    // 3
    &[0, 1], // 4
    &[0, 2], // 5
    &[1, 2], // 6
];

/// Model filenames on disk (written by the download manager).
pub const SEGMENTATION_FILE: &str = "segmentation.onnx";
pub const EMBEDDING_FILE: &str = "embedding.onnx";

#[derive(thiserror::Error, Debug)]
pub enum DiarizationModelError {
    #[error("ORT error: {0}")]
    Ort(#[from] ort::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("ndarray shape error: {0}")]
    Shape(#[from] ndarray::ShapeError),
    #[error("model output not found: {0}")]
    OutputNotFound(String),
    #[error("model file not found: {0}")]
    ModelFileNotFound(String),
}

/// Kaldi-compatible 80-d log-mel fbank (povey window, preemphasis 0.97, DC
/// removal, power spectrum, log, per-utterance CMN) — matches the wespeaker
/// preprocessing. Ported from the D1 spike.
pub struct Fbank {
    povey: Vec<f32>,
    mel_banks: Vec<(usize, Vec<f32>)>, // per mel bin: (fft_start_idx, weights)
    r2c: std::sync::Arc<dyn realfft::RealToComplex<f32>>,
}

impl Fbank {
    pub fn new() -> Self {
        // Povey window: (0.5 - 0.5 cos(2*pi*n/(N-1)))^0.85
        let povey: Vec<f32> = (0..FRAME_LEN)
            .map(|n| {
                let w = 0.5
                    - 0.5
                        * (2.0 * std::f32::consts::PI * n as f32 / (FRAME_LEN as f32 - 1.0)).cos();
                w.powf(0.85)
            })
            .collect();

        // Triangular mel filterbank over [20, 8000] Hz (kaldi convention).
        let low = 20.0f32;
        let high = (SR as f32) / 2.0;
        let mel = |f: f32| 1127.0 * (1.0 + f / 700.0).ln();
        let mel_low = mel(low);
        let mel_high = mel(high);
        let n_fft_bins = FFT_N / 2 + 1; // 257
        let fft_bin_hz = |i: usize| i as f32 * SR as f32 / FFT_N as f32;
        let delta = (mel_high - mel_low) / (N_MELS as f32 + 1.0);

        let mut mel_banks = Vec::with_capacity(N_MELS);
        for m in 0..N_MELS {
            let left = mel_low + m as f32 * delta;
            let center = left + delta;
            let right = left + 2.0 * delta;
            let mut start = None;
            let mut weights = Vec::new();
            for i in 0..n_fft_bins {
                let mz = mel(fft_bin_hz(i));
                let w = if mz > left && mz < right {
                    if mz <= center {
                        (mz - left) / (center - left)
                    } else {
                        (right - mz) / (right - center)
                    }
                } else {
                    0.0
                };
                if w > 0.0 {
                    if start.is_none() {
                        start = Some(i);
                    }
                    weights.push(w);
                }
            }
            mel_banks.push((start.unwrap_or(0), weights));
        }

        let mut planner = RealFftPlanner::<f32>::new();
        let r2c = planner.plan_fft_forward(FFT_N);

        Fbank {
            povey,
            mel_banks,
            r2c,
        }
    }

    /// Compute `[num_frames, 80]` fbank with CMN. Returns an empty vec for audio
    /// shorter than one frame.
    pub fn compute(&self, samples: &[f32]) -> Vec<[f32; N_MELS]> {
        if samples.len() < FRAME_LEN {
            return Vec::new();
        }
        let num_frames = 1 + (samples.len() - FRAME_LEN) / FRAME_SHIFT;
        let mut out: Vec<[f32; N_MELS]> = Vec::with_capacity(num_frames);
        let mut indata: Vec<f32> = self.r2c.make_input_vec();
        let mut spectrum: Vec<Complex32> = self.r2c.make_output_vec();

        for f in 0..num_frames {
            let off = f * FRAME_SHIFT;
            let mut frame: Vec<f32> = samples[off..off + FRAME_LEN].to_vec();

            // remove DC offset
            let mean = frame.iter().sum::<f32>() / FRAME_LEN as f32;
            for x in frame.iter_mut() {
                *x -= mean;
            }
            // preemphasis (kaldi: back to front, x[0] uses x[0])
            let first = frame[0];
            for i in (1..FRAME_LEN).rev() {
                frame[i] -= PREEMPH * frame[i - 1];
            }
            frame[0] -= PREEMPH * first;
            // povey window
            for i in 0..FRAME_LEN {
                frame[i] *= self.povey[i];
            }

            // zero-padded real FFT
            for v in indata.iter_mut() {
                *v = 0.0;
            }
            indata[..FRAME_LEN].copy_from_slice(&frame);
            // process() is infallible for correctly-sized buffers.
            let _ = self.r2c.process(&mut indata, &mut spectrum);

            // power spectrum -> mel -> log
            let mut row = [0.0f32; N_MELS];
            for (m, (start, weights)) in self.mel_banks.iter().enumerate() {
                let mut e = 0.0f32;
                for (k, w) in weights.iter().enumerate() {
                    let c = spectrum[start + k];
                    let power = c.re * c.re + c.im * c.im;
                    e += w * power;
                }
                row[m] = e.max(1e-10).ln();
            }
            out.push(row);
        }

        // CMN: subtract per-dim mean over time.
        let n = out.len() as f32;
        let mut cmn = [0.0f32; N_MELS];
        for row in &out {
            for m in 0..N_MELS {
                cmn[m] += row[m];
            }
        }
        for m in 0..N_MELS {
            cmn[m] /= n;
        }
        for row in out.iter_mut() {
            for m in 0..N_MELS {
                row[m] -= cmn[m];
            }
        }
        out
    }
}

impl Default for Fbank {
    fn default() -> Self {
        Self::new()
    }
}

/// Loaded diarization models (segmentation + embedding) plus the fbank featurizer.
pub struct DiarizationModel {
    segmentation: Session,
    embedding: Session,
    fbank: Fbank,
}

impl DiarizationModel {
    /// Load both ONNX models from a directory containing `segmentation.onnx`
    /// and `embedding.onnx`.
    pub fn new<P: AsRef<Path>>(model_dir: P) -> Result<Self, DiarizationModelError> {
        let dir = model_dir.as_ref();
        let seg_path = dir.join(SEGMENTATION_FILE);
        let emb_path = dir.join(EMBEDDING_FILE);

        if !seg_path.exists() {
            return Err(DiarizationModelError::ModelFileNotFound(
                seg_path.display().to_string(),
            ));
        }
        if !emb_path.exists() {
            return Err(DiarizationModelError::ModelFileNotFound(
                emb_path.display().to_string(),
            ));
        }

        let segmentation = Self::build_session(&seg_path)?;
        let embedding = Self::build_session(&emb_path)?;

        log::info!(
            "Loaded diarization models: segmentation={}, embedding={}",
            seg_path.display(),
            emb_path.display()
        );

        Ok(Self {
            segmentation,
            embedding,
            fbank: Fbank::new(),
        })
    }

    fn build_session(path: &Path) -> Result<Session, DiarizationModelError> {
        let session = Session::builder()?
            .with_optimization_level(GraphOptimizationLevel::Level3)?
            .with_execution_providers(vec![CPUExecutionProvider::default().build()])?
            .commit_from_file(path)?;
        Ok(session)
    }

    /// Run segmentation on one window (<= `SEG_WINDOW` samples, zero-padded) and
    /// return the `[frames, POWERSET_CLASSES]` logits.
    pub fn segment_window(&mut self, window: &[f32]) -> Result<ArrayD<f32>, DiarizationModelError> {
        let mut buf = vec![0.0f32; SEG_WINDOW];
        let n = window.len().min(SEG_WINDOW);
        buf[..n].copy_from_slice(&window[..n]);
        let x = Array3::from_shape_vec((1, 1, SEG_WINDOW), buf)?;

        let outputs = self
            .segmentation
            .run(inputs!["x" => TensorRef::from_array_view(x.view())?])?;

        let y = outputs
            .get("y")
            .ok_or_else(|| DiarizationModelError::OutputNotFound("y".to_string()))?
            .try_extract_array::<f32>()?
            .to_owned()
            .into_dyn();
        Ok(y)
    }

    /// Compute an L2-normalized 256-d speaker embedding for a mono chunk. Returns
    /// `None` when the chunk is too short to yield stable features.
    pub fn embed(&mut self, samples: &[f32]) -> Result<Option<Vec<f32>>, DiarizationModelError> {
        let feats = self.fbank.compute(samples);
        if feats.len() < 10 {
            return Ok(None);
        }
        let t = feats.len();
        let flat: Vec<f32> = feats.iter().flat_map(|r| r.iter().copied()).collect();
        let arr = Array3::from_shape_vec((1, t, N_MELS), flat)?;

        let outputs = self
            .embedding
            .run(inputs!["feats" => TensorRef::from_array_view(arr.view())?])?;

        let v = outputs
            .get("embs")
            .ok_or_else(|| DiarizationModelError::OutputNotFound("embs".to_string()))?
            .try_extract_array::<f32>()?
            .to_owned();

        let mut e: Vec<f32> = v.iter().copied().collect();
        let norm = e.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-9);
        for x in e.iter_mut() {
            *x /= norm;
        }
        Ok(Some(e))
    }
}
