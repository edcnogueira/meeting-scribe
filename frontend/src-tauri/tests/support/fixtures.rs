//! Ground-truth diarization fixture generator (test support).
//!
//! Synthesizes multi-speaker audio with a known speaker timeline by placing
//! clips rendered with the macOS `say` command at scripted start times into a
//! single 16 kHz mono buffer. Ground truth falls out of the *measured* duration
//! of each rendered clip, so TTS timing variation can never desynchronize the
//! truth from the audio.
//!
//! Optional degradation (seeded low-passed noise mixed at a target SNR plus a
//! synthetic room impulse response) is fully deterministic for a fixed seed.
//!
//! Generated fixtures are cached under `tests/data/diarization/cache/`, keyed by
//! a hash of the spec plus `GENERATOR_VERSION`. A cache hit loads instead of
//! re-synthesizing; a corrupt or unreadable cache is regenerated and
//! overwritten.

use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Command;

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use realfft::num_complex::Complex;
use realfft::RealFftPlanner;
use serde::{Deserialize, Serialize};

/// Fixed target sample rate for every fixture (mono).
pub const SAMPLE_RATE: u32 = 16_000;

/// Bump when the generation logic changes in a way that invalidates cached
/// fixtures or ground-truth timelines (part of the cache key — Requirement 1.9).
pub const GENERATOR_VERSION: u32 = 2;

/// A single scripted utterance. `speaker` indexes into [`FixtureSpec::voices`].
#[derive(Clone, Copy, Debug)]
pub struct Utterance {
    pub speaker: usize,
    pub text: &'static str,
    pub start_secs: f32,
}

/// Deterministic degradation parameters. Same values + same input => same output.
#[derive(Clone, Copy, Debug)]
pub struct Degradation {
    /// Target speech-to-noise ratio in dB (measured pre-reverb).
    pub snr_db: f32,
    /// RT60 of the synthetic room impulse response, in seconds.
    pub rt60_secs: f32,
    /// Seed for the noise and RIR-tail RNGs.
    pub seed: u64,
}

/// Declarative description of a fixture. Scripted placement *is* the ground truth.
pub struct FixtureSpec {
    pub name: &'static str,
    /// One macOS `say` voice per speaker index.
    pub voices: &'static [&'static str],
    pub script: &'static [Utterance],
    /// `None` = clean audio; `Some` = apply degradation after mixing.
    pub degrade: Option<Degradation>,
}

/// A synthesized fixture: the mixed 16 kHz mono buffer plus its ground truth.
pub struct Fixture {
    pub samples_16k: Vec<f32>,
    /// Per speaker, the list of `(start, end)` spans actually rendered, in
    /// seconds. Overlaps across speakers are allowed.
    pub truth: Vec<Vec<(f32, f32)>>,
}

/// Why [`build_fixture`] could not produce a fixture. Callers treat this as a
/// test *skip*, never a failure (Requirement 1.8).
#[derive(Debug)]
pub enum SkipReason {
    /// The `say` command is not available on this host.
    SayUnavailable(String),
    /// Synthesis or decoding of a clip failed.
    SynthesisFailed(String),
}

impl std::fmt::Display for SkipReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SkipReason::SayUnavailable(m) => write!(f, "`say` unavailable: {m}"),
            SkipReason::SynthesisFailed(m) => write!(f, "fixture synthesis failed: {m}"),
        }
    }
}

/// On-disk ground-truth sidecar written next to the cached WAV.
#[derive(Serialize, Deserialize)]
struct GroundTruth {
    generator_version: u32,
    spec_hash: u64,
    truth: Vec<Vec<(f32, f32)>>,
}

/// Synthesize (or load from cache) a fixture.
///
/// Returns `Err(SkipReason::SayUnavailable)` when the host cannot run `say`, so
/// the caller can skip rather than fail on machines without macOS TTS.
pub fn build_fixture(spec: &FixtureSpec, cache_dir: &Path) -> Result<Fixture, SkipReason> {
    if !say_available() {
        return Err(SkipReason::SayUnavailable(
            "`which say` failed or command unavailable".to_string(),
        ));
    }

    let hash = spec_hash(spec);
    let wav_path = cache_dir.join(format!("{}_{:016x}.wav", spec.name, hash));
    let json_path = cache_dir.join(format!("{}_{:016x}.json", spec.name, hash));

    if let Some(fixture) = load_cache(&wav_path, &json_path, hash) {
        return Ok(fixture);
    }

    let fixture = synthesize(spec)?;

    // Best-effort cache write; a failure here never fails the fixture.
    if let Err(e) = write_cache(&wav_path, &json_path, hash, &fixture) {
        eprintln!("fixture cache write failed (continuing uncached): {e}");
    }

    Ok(fixture)
}

/// Root of the fixture cache relative to the crate manifest dir.
pub fn default_cache_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/diarization/cache")
}

// ---------------------------------------------------------------------------
// Synthesis
// ---------------------------------------------------------------------------

fn synthesize(spec: &FixtureSpec) -> Result<Fixture, SkipReason> {
    let tmp = tempfile::Builder::new()
        .prefix("meetily-fixture-")
        .tempdir()
        .map_err(|e| SkipReason::SynthesisFailed(format!("tempdir: {e}")))?;

    let n_speakers = spec.voices.len();
    let mut truth: Vec<Vec<(f32, f32)>> = vec![Vec::new(); n_speakers];

    // (start_sample, clip_samples) per utterance, in script order.
    let mut placed: Vec<(usize, Vec<f32>)> = Vec::with_capacity(spec.script.len());
    let mut total_samples = 0usize;

    for (i, utt) in spec.script.iter().enumerate() {
        let voice = spec.voices.get(utt.speaker).ok_or_else(|| {
            SkipReason::SynthesisFailed(format!(
                "utterance {i} references speaker {} but only {} voices provided",
                utt.speaker, n_speakers
            ))
        })?;

        let clip_path = tmp.path().join(format!("clip_{i}.wav"));
        say_to_wav(voice, utt.text, &clip_path)?;
        let clip = decode_wav_16k_mono(&clip_path)?;

        let start_sample = (utt.start_secs * SAMPLE_RATE as f32).round() as usize;
        let end_sample = start_sample + clip.len();
        total_samples = total_samples.max(end_sample);

        let start_secs = start_sample as f32 / SAMPLE_RATE as f32;
        let end_secs = end_sample as f32 / SAMPLE_RATE as f32;
        truth[utt.speaker].push((start_secs, end_secs));

        placed.push((start_sample, clip));
    }

    // Mix clips into one buffer; overlapping speakers sum (overlaps allowed).
    let mut samples = vec![0.0f32; total_samples];
    for (start, clip) in &placed {
        for (j, s) in clip.iter().enumerate() {
            samples[start + j] += *s;
        }
    }

    // Degradation is applied AFTER mixing; ground truth is unaffected.
    if let Some(d) = &spec.degrade {
        samples = degrade(&samples, d);
    }

    Ok(Fixture {
        samples_16k: samples,
        truth,
    })
}

fn say_available() -> bool {
    Command::new("which")
        .arg("say")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn say_to_wav(voice: &str, text: &str, out: &Path) -> Result<(), SkipReason> {
    let output = Command::new("say")
        .arg("-v")
        .arg(voice)
        .arg("-o")
        .arg(out)
        .arg("--data-format=LEI16@16000")
        .arg("--file-format=WAVE")
        .arg(text)
        .output()
        .map_err(|e| SkipReason::SayUnavailable(format!("spawning `say`: {e}")))?;

    if !output.status.success() {
        return Err(SkipReason::SynthesisFailed(format!(
            "`say` exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Degradation (deterministic)
// ---------------------------------------------------------------------------

/// Apply seeded degradation: low-passed white noise mixed at `snr_db` relative
/// to speech RMS, then convolution with a synthetic RIR (`rt60_secs`). Fully
/// deterministic for a fixed seed. Output length equals the input length.
pub fn degrade(speech: &[f32], d: &Degradation) -> Vec<f32> {
    let (noisy, _noise) = add_noise(speech, d.snr_db, d.seed);
    let rir = synth_rir(d.rt60_secs, d.seed);
    let wet = fft_convolve(&noisy, &rir);
    // Keep alignment with the ground-truth timeline: drop the reverb tail so the
    // output has the same length (and sample offsets) as the input.
    let mut out = wet;
    out.truncate(speech.len());
    out
}

/// Mix seeded one-pole low-passed white noise into `speech` at the target SNR.
///
/// Returns `(noisy, scaled_noise)` where `scaled_noise` is exactly the signal
/// added, so a test can measure the achieved SNR (speech RMS vs noise RMS,
/// pre-reverb).
pub fn add_noise(speech: &[f32], snr_db: f32, seed: u64) -> (Vec<f32>, Vec<f32>) {
    let mut rng = StdRng::seed_from_u64(seed);

    // One-pole low-passed white noise.
    const ALPHA: f32 = 0.2;
    let mut noise = vec![0.0f32; speech.len()];
    let mut prev = 0.0f32;
    for n in noise.iter_mut() {
        let white: f32 = rng.gen_range(-1.0..1.0);
        prev = ALPHA * white + (1.0 - ALPHA) * prev;
        *n = prev;
    }

    let rms_speech = rms(speech);
    let rms_noise = rms(&noise).max(1e-12);
    // 10*log10(rms_speech^2 / rms_noise^2) = snr_db  =>  rms_noise = rms_speech / 10^(snr/20)
    let target_rms_noise = rms_speech / 10f32.powf(snr_db / 20.0);
    let gain = target_rms_noise / rms_noise;

    let mut scaled_noise = noise;
    for n in scaled_noise.iter_mut() {
        *n *= gain;
    }
    let noisy: Vec<f32> = speech
        .iter()
        .zip(scaled_noise.iter())
        .map(|(s, n)| s + n)
        .collect();

    (noisy, scaled_noise)
}

/// Synthetic room impulse response: unit impulse (direct path) plus a seeded
/// noise tail shaped by `exp(-6.9 t / rt60)`, then L2-normalized so convolution
/// preserves signal energy scale.
fn synth_rir(rt60_secs: f32, seed: u64) -> Vec<f32> {
    let len = ((rt60_secs * SAMPLE_RATE as f32).round() as usize).max(1);
    // Independent, deterministic stream from the RIR-tail RNG.
    let mut rng = StdRng::seed_from_u64(seed.wrapping_add(0x9E37_79B9_7F4A_7C15));

    let mut rir = vec![0.0f32; len];
    rir[0] = 1.0; // direct path
    for i in 1..len {
        let t = i as f32 / SAMPLE_RATE as f32;
        let decay = (-6.9 * t / rt60_secs).exp();
        let white: f32 = rng.gen_range(-1.0..1.0);
        rir[i] = white * decay;
    }

    let norm = rir.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-12);
    for x in rir.iter_mut() {
        *x /= norm;
    }
    rir
}

/// Linear convolution via FFT. Output length is `sig.len() + kernel.len() - 1`.
fn fft_convolve(sig: &[f32], kernel: &[f32]) -> Vec<f32> {
    if sig.is_empty() || kernel.is_empty() {
        return Vec::new();
    }
    let full_len = sig.len() + kernel.len() - 1;
    let n = full_len.next_power_of_two();

    let mut planner = RealFftPlanner::<f32>::new();
    let fwd = planner.plan_fft_forward(n);
    let inv = planner.plan_fft_inverse(n);

    let mut sig_buf = fwd.make_input_vec();
    sig_buf[..sig.len()].copy_from_slice(sig);
    let mut sig_spec = fwd.make_output_vec();
    fwd.process(&mut sig_buf, &mut sig_spec).expect("forward fft");

    let mut ker_buf = fwd.make_input_vec();
    ker_buf[..kernel.len()].copy_from_slice(kernel);
    let mut ker_spec = fwd.make_output_vec();
    fwd.process(&mut ker_buf, &mut ker_spec).expect("forward fft");

    // Multiply spectra element-wise.
    let mut prod: Vec<Complex<f32>> = sig_spec
        .iter()
        .zip(ker_spec.iter())
        .map(|(a, b)| a * b)
        .collect();
    // The product of two Hermitian spectra is Hermitian; force the DC and
    // Nyquist bins to be purely real so the inverse transform is well-formed.
    if let Some(first) = prod.first_mut() {
        first.im = 0.0;
    }
    if n % 2 == 0 {
        if let Some(last) = prod.last_mut() {
            last.im = 0.0;
        }
    }

    let mut out = inv.make_output_vec();
    inv.process(&mut prod, &mut out).expect("inverse fft");

    let scale = 1.0 / n as f32;
    out.truncate(full_len);
    for x in out.iter_mut() {
        *x *= scale;
    }
    out
}

/// Root-mean-square of a signal.
pub fn rms(x: &[f32]) -> f32 {
    if x.is_empty() {
        return 0.0;
    }
    (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
}

// ---------------------------------------------------------------------------
// Caching
// ---------------------------------------------------------------------------

fn spec_hash(spec: &FixtureSpec) -> u64 {
    let mut h = DefaultHasher::new();
    GENERATOR_VERSION.hash(&mut h);
    SAMPLE_RATE.hash(&mut h);
    spec.name.hash(&mut h);
    for v in spec.voices {
        v.hash(&mut h);
    }
    for u in spec.script {
        u.speaker.hash(&mut h);
        u.text.hash(&mut h);
        u.start_secs.to_bits().hash(&mut h);
    }
    match &spec.degrade {
        Some(d) => {
            1u8.hash(&mut h);
            d.snr_db.to_bits().hash(&mut h);
            d.rt60_secs.to_bits().hash(&mut h);
            d.seed.hash(&mut h);
        }
        None => 0u8.hash(&mut h),
    }
    h.finish()
}

fn load_cache(wav_path: &Path, json_path: &Path, hash: u64) -> Option<Fixture> {
    let json = fs::read(json_path).ok()?;
    let gt: GroundTruth = serde_json::from_slice(&json).ok()?;
    if gt.generator_version != GENERATOR_VERSION || gt.spec_hash != hash {
        return None; // stale/corrupt -> regenerate
    }
    let samples = decode_wav_16k_mono(wav_path).ok()?;
    Some(Fixture {
        samples_16k: samples,
        truth: gt.truth,
    })
}

fn write_cache(
    wav_path: &Path,
    json_path: &Path,
    hash: u64,
    fixture: &Fixture,
) -> std::io::Result<()> {
    if let Some(parent) = wav_path.parent() {
        fs::create_dir_all(parent)?;
    }
    encode_wav_16k_mono(wav_path, &fixture.samples_16k)?;
    let gt = GroundTruth {
        generator_version: GENERATOR_VERSION,
        spec_hash: hash,
        truth: fixture.truth.clone(),
    };
    let json = serde_json::to_vec_pretty(&gt)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    fs::write(json_path, json)
}

// ---------------------------------------------------------------------------
// Minimal WAV codec (16-bit PCM mono). `say` emits standard RIFF/WAVE files
// with extra chunks (JUNK/FLLR), so the reader walks chunks rather than
// assuming fixed offsets.
// ---------------------------------------------------------------------------

fn decode_wav_16k_mono(path: &Path) -> Result<Vec<f32>, SkipReason> {
    let bytes = fs::read(path)
        .map_err(|e| SkipReason::SynthesisFailed(format!("reading {}: {e}", path.display())))?;
    parse_wav_16k_mono(&bytes)
        .ok_or_else(|| SkipReason::SynthesisFailed(format!("undecodable WAV: {}", path.display())))
}

/// Parse a little-endian 16-bit PCM mono WAV, returning samples in `[-1, 1)`.
/// Returns `None` on any structural problem.
fn parse_wav_16k_mono(bytes: &[u8]) -> Option<Vec<f32>> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }

    let mut pos = 12usize;
    let mut fmt_ok = false;
    let mut data: Option<&[u8]> = None;

    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let size = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().ok()?) as usize;
        let body_start = pos + 8;
        let body_end = body_start.checked_add(size)?;
        if body_end > bytes.len() {
            return None;
        }
        let body = &bytes[body_start..body_end];

        if id == b"fmt " {
            if body.len() < 16 {
                return None;
            }
            let audio_format = u16::from_le_bytes(body[0..2].try_into().ok()?);
            let channels = u16::from_le_bytes(body[2..4].try_into().ok()?);
            let bits = u16::from_le_bytes(body[14..16].try_into().ok()?);
            // PCM (1) mono 16-bit only.
            if audio_format != 1 || channels != 1 || bits != 16 {
                return None;
            }
            fmt_ok = true;
        } else if id == b"data" {
            data = Some(body);
        }

        // Chunks are word-aligned: skip a pad byte for odd sizes.
        pos = body_end + (size & 1);
    }

    if !fmt_ok {
        return None;
    }
    let data = data?;
    let mut out = Vec::with_capacity(data.len() / 2);
    for frame in data.chunks_exact(2) {
        let sample = i16::from_le_bytes([frame[0], frame[1]]);
        out.push(sample as f32 / 32768.0);
    }
    Some(out)
}

fn encode_wav_16k_mono(path: &Path, samples: &[f32]) -> std::io::Result<()> {
    let data_len = samples.len() * 2;
    let mut buf = Vec::with_capacity(44 + data_len);

    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&((36 + data_len) as u32).to_le_bytes());
    buf.extend_from_slice(b"WAVE");

    buf.extend_from_slice(b"fmt ");
    buf.extend_from_slice(&16u32.to_le_bytes()); // PCM fmt chunk size
    buf.extend_from_slice(&1u16.to_le_bytes()); // PCM
    buf.extend_from_slice(&1u16.to_le_bytes()); // mono
    buf.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    buf.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes()); // byte rate
    buf.extend_from_slice(&2u16.to_le_bytes()); // block align
    buf.extend_from_slice(&16u16.to_le_bytes()); // bits per sample

    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&(data_len as u32).to_le_bytes());
    for &s in samples {
        let v = (s.clamp(-1.0, 1.0) * 32767.0).round() as i16;
        buf.extend_from_slice(&v.to_le_bytes());
    }

    fs::write(path, buf)
}
