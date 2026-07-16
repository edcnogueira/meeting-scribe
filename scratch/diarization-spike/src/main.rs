// Diarization model spike (task D1).
//
// Pipeline (pure Rust on `ort`, mirroring frontend/src-tauri/src/parakeet_engine):
//   wav 16kHz mono -> pyannote segmentation (powerset) -> speech regions
//     -> kaldi-fbank -> wespeaker embedding -> agglomerative cosine clustering.
//
// Runs several scenarios (sequential mix, overlapped mono, and a "separate track"
// pair) and prints calibration stats + timing so the report numbers come from a
// real run. See tasks/diarization/D1-resultados.md.

use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use ndarray::{Array3, ArrayD};
use ort::execution_providers::CPUExecutionProvider;
use ort::inputs;
use ort::session::builder::GraphOptimizationLevel;
use ort::session::Session;
use ort::value::TensorRef;
use serde::Serialize;

const SR: usize = 16000;
const SEG_WINDOW: usize = 160_000; // 10 s, from segmentation model metadata window_size
const N_MELS: usize = 80;
const FFT_N: usize = 512;
const FRAME_LEN: usize = 400; // 25 ms
const FRAME_SHIFT: usize = 160; // 10 ms
const PREEMPH: f32 = 0.97;

// pyannote-segmentation-3.0 powerset (num_speakers=3, max_classes=2): 7 classes.
// order = combinations by increasing cardinality (pyannote Powerset convention).
const POWERSET: [&[usize]; 7] = [
    &[],          // 0: silence
    &[0],         // 1
    &[1],         // 2
    &[2],         // 3
    &[0, 1],      // 4
    &[0, 2],      // 5
    &[1, 2],      // 6
];

// ---------- audio io ----------

fn read_wav_16k_mono(path: &str) -> Vec<f32> {
    let mut reader = hound::WavReader::open(path).expect("open wav");
    let spec = reader.spec();
    assert_eq!(spec.sample_rate, SR as u32, "expected 16kHz wav");
    assert_eq!(spec.channels, 1, "expected mono wav");
    match spec.sample_format {
        hound::SampleFormat::Int => reader
            .samples::<i16>()
            .map(|s| s.unwrap() as f32 / 32768.0)
            .collect(),
        hound::SampleFormat::Float => reader.samples::<f32>().map(|s| s.unwrap()).collect(),
    }
}

// ---------- kaldi-compatible fbank (matches torchaudio.compliance.kaldi.fbank
// defaults used by wespeaker: povey window, preemph 0.97, dc removal, 80 mel,
// power spectrum, log, then per-utterance CMN over time). ----------

struct Fbank {
    povey: Vec<f32>,
    mel_banks: Vec<(usize, Vec<f32>)>, // per mel bin: (fft_start_idx, weights)
    fft: std::sync::Arc<dyn rustfft::Fft<f32>>,
}

impl Fbank {
    fn new() -> Self {
        // Povey window: (0.5 - 0.5 cos(2*pi*n/(N-1)))^0.85
        let povey: Vec<f32> = (0..FRAME_LEN)
            .map(|n| {
                let w = 0.5 - 0.5 * (2.0 * std::f32::consts::PI * n as f32 / (FRAME_LEN as f32 - 1.0)).cos();
                w.powf(0.85)
            })
            .collect();

        // Mel filterbank over [20, 8000] Hz, kaldi triangular in mel domain.
        let low = 20.0f32;
        let high = (SR as f32) / 2.0;
        let mel = |f: f32| 1127.0 * (1.0 + f / 700.0).ln();
        let inv_mel = |m: f32| 700.0 * ((m / 1127.0).exp() - 1.0);
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
                } else if start.is_some() && weights.last().map(|x| *x).unwrap_or(0.0) == 0.0 {
                    // stop once past the triangle
                }
            }
            let _ = inv_mel; // (kept for reference)
            mel_banks.push((start.unwrap_or(0), weights));
        }

        let mut planner = rustfft::FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(FFT_N);
        Fbank { povey, mel_banks, fft }
    }

    /// Compute [num_frames, 80] fbank with CMN.
    fn compute(&self, samples: &[f32]) -> Vec<[f32; N_MELS]> {
        if samples.len() < FRAME_LEN {
            return Vec::new();
        }
        let num_frames = 1 + (samples.len() - FRAME_LEN) / FRAME_SHIFT;
        let mut out: Vec<[f32; N_MELS]> = Vec::with_capacity(num_frames);
        let mut buf = vec![rustfft::num_complex::Complex::<f32>::new(0.0, 0.0); FFT_N];

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
            // FFT
            for c in buf.iter_mut() {
                *c = rustfft::num_complex::Complex::new(0.0, 0.0);
            }
            for i in 0..FRAME_LEN {
                buf[i].re = frame[i];
            }
            self.fft.process(&mut buf);
            // power spectrum
            let mut power = [0.0f32; FFT_N / 2 + 1];
            for (i, p) in power.iter_mut().enumerate() {
                *p = buf[i].re * buf[i].re + buf[i].im * buf[i].im;
            }
            // mel + log
            let mut row = [0.0f32; N_MELS];
            for (m, (start, weights)) in self.mel_banks.iter().enumerate() {
                let mut e = 0.0f32;
                for (k, w) in weights.iter().enumerate() {
                    e += w * power[start + k];
                }
                row[m] = e.max(1e-10).ln();
            }
            out.push(row);
        }

        // CMN: subtract per-dim mean over time.
        let n = out.len() as f32;
        let mut mean = [0.0f32; N_MELS];
        for row in &out {
            for m in 0..N_MELS {
                mean[m] += row[m];
            }
        }
        for m in 0..N_MELS {
            mean[m] /= n;
        }
        for row in out.iter_mut() {
            for m in 0..N_MELS {
                row[m] -= mean[m];
            }
        }
        out
    }
}

// ---------- onnx sessions ----------

fn build_session(path: &Path) -> Session {
    Session::builder()
        .unwrap()
        .with_optimization_level(GraphOptimizationLevel::Level3)
        .unwrap()
        .with_execution_providers(vec![CPUExecutionProvider::default().build()])
        .unwrap()
        .commit_from_file(path)
        .unwrap()
}

/// Run segmentation on one 10s window -> [num_frames, 7] logits.
fn segment_window(seg: &mut Session, window: &[f32]) -> ArrayD<f32> {
    let mut buf = vec![0.0f32; SEG_WINDOW];
    let n = window.len().min(SEG_WINDOW);
    buf[..n].copy_from_slice(&window[..n]);
    let x = Array3::from_shape_vec((1, 1, SEG_WINDOW), buf).unwrap();
    let outputs = seg
        .run(inputs!["x" => TensorRef::from_array_view(x.view()).unwrap()])
        .unwrap();
    outputs
        .get("y")
        .unwrap()
        .try_extract_array::<f32>()
        .unwrap()
        .to_owned()
        .into_dyn()
}

/// Embedding for a mono chunk -> L2-normalized 256-d vector (or None if too short).
fn embed(emb: &mut Session, fbank: &Fbank, samples: &[f32]) -> Option<Vec<f32>> {
    let feats = fbank.compute(samples);
    if feats.len() < 10 {
        return None;
    }
    let t = feats.len();
    let flat: Vec<f32> = feats.iter().flat_map(|r| r.iter().copied()).collect();
    let arr = Array3::from_shape_vec((1, t, N_MELS), flat).unwrap();
    let outputs = emb
        .run(inputs!["feats" => TensorRef::from_array_view(arr.view()).unwrap()])
        .unwrap();
    let v = outputs
        .get("embs")
        .unwrap()
        .try_extract_array::<f32>()
        .unwrap()
        .to_owned();
    let mut e: Vec<f32> = v.iter().copied().collect();
    let norm = e.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-9);
    for x in e.iter_mut() {
        *x /= norm;
    }
    Some(e)
}

fn cos_sim(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

// ---------- speech regions from segmentation ----------

#[derive(Clone)]
struct Region {
    start: f32,
    end: f32,
    embedding: Vec<f32>,
    gt_speaker: String, // filled by scoring
    cluster: usize,     // filled by clustering
}

fn extract_regions(
    seg: &mut Session,
    emb: &mut Session,
    fbank: &Fbank,
    audio: &[f32],
) -> Vec<Region> {
    let mut regions = Vec::new();
    let n_windows = (audio.len() + SEG_WINDOW - 1) / SEG_WINDOW;
    for w in 0..n_windows {
        let w0 = w * SEG_WINDOW;
        let w1 = (w0 + SEG_WINDOW).min(audio.len());
        let window = &audio[w0..w1];
        let logits = segment_window(seg, window);
        let frames = logits.shape()[1];
        let step = SEG_WINDOW as f32 / frames as f32 / SR as f32; // seconds per frame
        let win_start = w0 as f32 / SR as f32;

        // per-frame active local speakers + exclusivity
        let mut active = vec![[false; 3]; frames];
        let mut exclusive = vec![usize::MAX; frames]; // local speaker if exactly one active
        for f in 0..frames {
            let mut best = 0usize;
            let mut best_v = f32::NEG_INFINITY;
            for c in 0..7 {
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

        // contiguous runs per local speaker, bridging small gaps
        let bridge = (0.25 / step).round() as usize; // bridge gaps <=250ms
        let min_frames = (0.4 / step).round() as usize; // drop <400ms
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
                        let s0 = w0 + fr * (SEG_WINDOW / frames);
                        let s1 = (s0 + SEG_WINDOW / frames).min(audio.len());
                        if s0 < audio.len() {
                            clean.extend_from_slice(&audio[s0..s1]);
                        }
                    }
                }
                // fallback: whole run if not enough exclusive audio
                let seg_samples = if clean.len() >= SR / 3 {
                    clean
                } else {
                    let s0 = w0 + start * (SEG_WINDOW / frames);
                    let s1 = (w0 + (end + 1) * (SEG_WINDOW / frames)).min(audio.len());
                    audio[s0.min(audio.len())..s1].to_vec()
                };
                if let Some(e) = embed(emb, fbank, &seg_samples) {
                    regions.push(Region {
                        start: win_start + start as f32 * step,
                        end: win_start + (end + 1) as f32 * step,
                        embedding: e,
                        gt_speaker: String::new(),
                        cluster: 0,
                    });
                }
            }
        }
    }
    regions.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap());
    regions
}

// ---------- agglomerative clustering (average linkage, cosine distance) ----------

fn cluster(embeddings: &[Vec<f32>], threshold: Option<f32>, fixed_k: Option<usize>) -> Vec<usize> {
    let n = embeddings.len();
    if n == 0 {
        return vec![];
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
            if members.len() <= k {
                break;
            }
        }
        // find closest pair
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
    let mut labels = vec![0usize; n];
    for (c, mem) in members.iter().enumerate() {
        for &idx in mem {
            labels[idx] = c;
        }
    }
    labels
}

// ---------- ground truth + scoring ----------

#[derive(serde::Deserialize)]
struct GtSeg {
    speaker: String,
    start: f32,
    end: f32,
}

fn load_gt(path: &str, key: &str) -> Vec<GtSeg> {
    let raw = std::fs::read_to_string(path).unwrap();
    let all: HashMap<String, serde_json::Value> = serde_json::from_str(&raw).unwrap();
    serde_json::from_value(all[key].clone()).unwrap()
}

fn gt_at(gt: &[GtSeg], t: f32) -> Option<&str> {
    gt.iter()
        .find(|g| t >= g.start && t < g.end)
        .map(|g| g.speaker.as_str())
}

/// Frame-level accuracy over speech regions with best cluster->speaker mapping.
fn score(regions: &[Region], gt: &[GtSeg], duration: f32) -> (f32, usize, usize) {
    // predicted speaker at time t = cluster of the region covering t (last wins)
    let pred_at = |t: f32| -> Option<usize> {
        regions
            .iter()
            .find(|r| t >= r.start && t < r.end)
            .map(|r| r.cluster)
    };
    let n_clusters = regions.iter().map(|r| r.cluster).max().map(|m| m + 1).unwrap_or(0);
    let true_speakers: Vec<String> = {
        let mut v: Vec<String> = gt.iter().map(|g| g.speaker.clone()).collect();
        v.sort();
        v.dedup();
        v
    };
    // brute-force best assignment cluster->true speaker (<=4 speakers)
    let dt = 0.1f32;
    let steps = (duration / dt) as usize;
    let mut samples: Vec<(usize, usize)> = Vec::new(); // (true_idx, pred_cluster)
    let sp_idx: HashMap<&str, usize> =
        true_speakers.iter().enumerate().map(|(i, s)| (s.as_str(), i)).collect();
    for s in 0..steps {
        let t = s as f32 * dt;
        if let Some(tg) = gt_at(gt, t) {
            if let Some(pc) = pred_at(t) {
                samples.push((sp_idx[tg], pc));
            } else {
                samples.push((sp_idx[tg], usize::MAX)); // missed speech
            }
        }
    }
    let total = samples.len().max(1);
    // try all permutations mapping clusters to true speakers
    let perms = permutations((0..n_clusters).collect());
    let mut best_acc = 0.0f32;
    for p in &perms {
        // p[cluster] = true speaker index (only for first len(true) clusters)
        let mut correct = 0usize;
        for &(tg, pc) in &samples {
            if pc != usize::MAX && pc < p.len() && p[pc] == tg {
                correct += 1;
            }
        }
        let acc = correct as f32 / total as f32;
        if acc > best_acc {
            best_acc = acc;
        }
    }
    (best_acc, n_clusters, true_speakers.len())
}

fn permutations(items: Vec<usize>) -> Vec<Vec<usize>> {
    if items.len() <= 1 {
        return vec![items];
    }
    let mut out = Vec::new();
    for i in 0..items.len() {
        let mut rest = items.clone();
        let x = rest.remove(i);
        for mut p in permutations(rest) {
            p.insert(0, x);
            out.push(p);
        }
    }
    out
}

fn label_gt(regions: &mut [Region], gt: &[GtSeg]) {
    for r in regions.iter_mut() {
        // majority speaker by overlap
        let mut best = String::from("?");
        let mut best_ov = 0.0f32;
        for g in gt {
            let ov = (r.end.min(g.end) - r.start.max(g.start)).max(0.0);
            if ov > best_ov {
                best_ov = ov;
                best = g.speaker.clone();
            }
        }
        r.gt_speaker = best;
    }
}

fn intra_inter_stats(regions: &[Region]) -> (f32, f32, f32, f32) {
    let mut intra = Vec::new();
    let mut inter = Vec::new();
    for i in 0..regions.len() {
        for j in (i + 1)..regions.len() {
            let s = cos_sim(&regions[i].embedding, &regions[j].embedding);
            if regions[i].gt_speaker == regions[j].gt_speaker {
                intra.push(s);
            } else {
                inter.push(s);
            }
        }
    }
    let mean = |v: &[f32]| if v.is_empty() { f32::NAN } else { v.iter().sum::<f32>() / v.len() as f32 };
    let min = |v: &[f32]| v.iter().cloned().fold(f32::INFINITY, f32::min);
    let max = |v: &[f32]| v.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    (mean(&intra), min(&intra), mean(&inter), max(&inter))
}

#[derive(Serialize)]
struct ScenarioResult {
    name: String,
    duration_s: f32,
    n_regions: usize,
    proc_time_s: f32,
    rtf: f32,
    // clustering (auto threshold)
    auto_clusters: usize,
    auto_accuracy: f32,
    true_speakers: usize,
    // fixed-k clustering
    fixedk_clusters: usize,
    fixedk_accuracy: f32,
    // embedding separability (from GT labels)
    intra_mean_sim: f32,
    intra_min_sim: f32,
    inter_mean_sim: f32,
    inter_max_sim: f32,
    centroids: HashMap<String, Vec<f32>>, // per GT speaker mean embedding
}

fn centroids_by_gt(regions: &[Region]) -> HashMap<String, Vec<f32>> {
    let mut sums: HashMap<String, (Vec<f32>, usize)> = HashMap::new();
    for r in regions {
        let e = sums.entry(r.gt_speaker.clone()).or_insert((vec![0.0; r.embedding.len()], 0));
        for (i, x) in r.embedding.iter().enumerate() {
            e.0[i] += x;
        }
        e.1 += 1;
    }
    sums.into_iter()
        .map(|(k, (mut v, n))| {
            for x in v.iter_mut() {
                *x /= n as f32;
            }
            let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-9);
            for x in v.iter_mut() {
                *x /= norm;
            }
            (k, v)
        })
        .collect()
}

fn run_scenario(
    seg: &mut Session,
    emb: &mut Session,
    fbank: &Fbank,
    name: &str,
    wav: &str,
    gt_path: &str,
    gt_key: &str,
    threshold: f32,
) -> ScenarioResult {
    let audio = read_wav_16k_mono(wav);
    let duration = audio.len() as f32 / SR as f32;
    let gt = load_gt(gt_path, gt_key);

    let t0 = Instant::now();
    let mut regions = extract_regions(seg, emb, fbank, &audio);
    let proc_time = t0.elapsed().as_secs_f32();

    label_gt(&mut regions, &gt);
    let true_n = {
        let mut v: Vec<&str> = gt.iter().map(|g| g.speaker.as_str()).collect();
        v.sort();
        v.dedup();
        v.len()
    };

    // auto threshold clustering
    let labels_auto = cluster(
        &regions.iter().map(|r| r.embedding.clone()).collect::<Vec<_>>(),
        Some(threshold),
        None,
    );
    for (r, l) in regions.iter_mut().zip(&labels_auto) {
        r.cluster = *l;
    }
    let (acc_auto, nc_auto, _) = score(&regions, &gt, duration);

    // fixed-k clustering
    let labels_k = cluster(
        &regions.iter().map(|r| r.embedding.clone()).collect::<Vec<_>>(),
        None,
        Some(true_n),
    );
    for (r, l) in regions.iter_mut().zip(&labels_k) {
        r.cluster = *l;
    }
    let (acc_k, nc_k, _) = score(&regions, &gt, duration);

    let (intra_m, intra_min, inter_m, inter_max) = intra_inter_stats(&regions);
    let cents = centroids_by_gt(&regions);

    println!(
        "[{}] dur={:.1}s regions={} proc={:.2}s rtf={:.3} | auto: {} clusters acc={:.1}% | fixedk({}): acc={:.1}% | intra sim mean={:.3} min={:.3}  inter mean={:.3} max={:.3}",
        name, duration, regions.len(), proc_time, proc_time / duration,
        nc_auto, acc_auto * 100.0, true_n, acc_k * 100.0,
        intra_m, intra_min, inter_m, inter_max
    );

    ScenarioResult {
        name: name.to_string(),
        duration_s: duration,
        n_regions: regions.len(),
        proc_time_s: proc_time,
        rtf: proc_time / duration,
        auto_clusters: nc_auto,
        auto_accuracy: acc_auto,
        true_speakers: true_n,
        fixedk_clusters: nc_k,
        fixedk_accuracy: acc_k,
        intra_mean_sim: intra_m,
        intra_min_sim: intra_min,
        inter_mean_sim: inter_m,
        inter_max_sim: inter_max,
        centroids: cents,
    }
}

fn main() {
    let dir = std::env::args().nth(1).unwrap_or_else(|| ".".to_string());
    let seg_path = format!("{}/models/sherpa-onnx-pyannote-segmentation-3-0/model.onnx", dir);
    let emb_path = format!("{}/models/wespeaker_en_voxceleb_resnet34.onnx", dir);
    let gt_path = format!("{}/audio/ground_truth.json", dir);
    // Clustering cosine-distance cut. 0.55 chosen from wespeaker/pyannote priors;
    // the intra/inter stats printed below let us verify/tune it.
    let threshold = std::env::var("CLUSTER_THRESHOLD").ok().and_then(|s| s.parse().ok()).unwrap_or(0.55f32);

    eprintln!("loading models...");
    let mut seg = build_session(Path::new(&seg_path));
    let mut emb = build_session(Path::new(&emb_path));
    let fbank = Fbank::new();
    eprintln!("threshold(cosine-distance)={}\n", threshold);

    let mut results = Vec::new();
    results.push(run_scenario(&mut seg, &mut emb, &fbank, "mix_seq", &format!("{}/audio/mix_seq.wav", dir), &gt_path, "mix_seq", threshold));
    results.push(run_scenario(&mut seg, &mut emb, &fbank, "mono_overlap", &format!("{}/audio/mono_overlap.wav", dir), &gt_path, "mono_overlap", threshold));
    results.push(run_scenario(&mut seg, &mut emb, &fbank, "track_system", &format!("{}/audio/track_system.wav", dir), &gt_path, "track_system", threshold));
    results.push(run_scenario(&mut seg, &mut emb, &fbank, "track_mic", &format!("{}/audio/track_mic.wav", dir), &gt_path, "track_mic", threshold));

    // ---------- cross-session identification ----------
    // enroll from mix_seq centroids, match mono_overlap centroids by cosine.
    println!("\n=== cross-session identification (enroll=mix_seq, test=mono_overlap) ===");
    let enroll = &results[0].centroids;
    let test = &results[1].centroids;
    for (spk, tvec) in test {
        let mut best = ("?".to_string(), f32::NEG_INFINITY);
        for (espk, evec) in enroll {
            let s = cos_sim(tvec, evec);
            if s > best.1 {
                best = (espk.clone(), s);
            }
        }
        let ok = if &best.0 == spk { "MATCH" } else { "MISMATCH" };
        println!("  test speaker {} -> enrolled {} (sim={:.3}) [{}]", spk, best.0, best.1, ok);
    }

    let out = format!("{}/results.json", dir);
    std::fs::write(&out, serde_json::to_string_pretty(&results).unwrap()).unwrap();
    println!("\nwrote {}", out);
}
