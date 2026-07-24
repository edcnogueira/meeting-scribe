//! Unit tests for the diarization fixture generator support module.
//!
//! Degradation determinism and SNR tests use a synthetic speech signal and do
//! not require macOS TTS. The cache/ground-truth determinism test invokes `say`
//! and skips gracefully (prints a message and returns) when it is unavailable.

mod support;

use support::fixtures::{
    add_noise, build_fixture, degrade, rms, Degradation, FixtureSpec, Utterance, SAMPLE_RATE,
};

/// A deterministic band-limited "speech-like" signal (no TTS needed).
fn synthetic_speech(secs: f32) -> Vec<f32> {
    let n = (secs * SAMPLE_RATE as f32) as usize;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f32 / SAMPLE_RATE as f32;
        // A couple of formant-ish tones with a slow amplitude envelope.
        let env = 0.5 * (1.0 + (2.0 * std::f32::consts::PI * 3.0 * t).sin());
        let s = 0.6 * (2.0 * std::f32::consts::PI * 180.0 * t).sin()
            + 0.3 * (2.0 * std::f32::consts::PI * 320.0 * t).sin();
        out.push(env * s);
    }
    out
}

#[test]
fn degradation_is_bit_identical_for_same_seed() {
    let speech = synthetic_speech(2.0);
    let d = Degradation {
        snr_db: 10.0,
        rt60_secs: 0.3,
        seed: 42,
    };

    let a = degrade(&speech, &d);
    let b = degrade(&speech, &d);

    assert_eq!(a.len(), speech.len(), "output length must match input");
    assert_eq!(a, b, "same seed must yield bit-identical degraded samples");
}

#[test]
fn degradation_differs_for_different_seed() {
    let speech = synthetic_speech(2.0);
    let base = Degradation {
        snr_db: 10.0,
        rt60_secs: 0.3,
        seed: 1,
    };
    let other = Degradation { seed: 2, ..base };

    let a = degrade(&speech, &base);
    let b = degrade(&speech, &other);
    assert_ne!(a, b, "different seeds should produce different noise");
}

#[test]
fn achieved_snr_is_within_half_db_of_target() {
    let speech = synthetic_speech(3.0);
    let target = 10.0f32;

    let (_noisy, scaled_noise) = add_noise(&speech, target, 7);
    let rms_speech = rms(&speech);
    let rms_noise = rms(&scaled_noise);
    let achieved = 20.0 * (rms_speech / rms_noise).log10();

    assert!(
        (achieved - target).abs() <= 0.5,
        "achieved SNR {achieved:.3} dB not within +/-0.5 dB of {target} dB",
    );
}

/// Same spec version must produce the same ground-truth timeline, and the
/// second build must be served from cache (Requirement 1.9).
#[test]
fn cache_yields_identical_ground_truth_timeline() {
    const SPEC: FixtureSpec = FixtureSpec {
        name: "seq2_cache_test",
        voices: &["Alex", "Samantha"],
        script: &[
            Utterance {
                speaker: 0,
                text: "Good morning everyone, let us begin the meeting.",
                start_secs: 0.0,
            },
            Utterance {
                speaker: 1,
                text: "Thanks. I have a quick update on the roadmap.",
                start_secs: 3.0,
            },
        ],
        degrade: None,
    };

    let tmp = tempfile::tempdir().expect("tempdir");
    let cache_dir = tmp.path();

    let first = match build_fixture(&SPEC, cache_dir) {
        Ok(f) => f,
        Err(reason) => {
            eprintln!("SKIP cache_yields_identical_ground_truth_timeline: {reason}");
            return;
        }
    };

    // A cache file should now exist for this spec.
    let has_cache = std::fs::read_dir(cache_dir)
        .map(|rd| {
            rd.filter_map(Result::ok)
                .any(|e| e.path().extension().is_some_and(|x| x == "json"))
        })
        .unwrap_or(false);
    assert!(has_cache, "expected a cached ground_truth json after first build");

    let second = build_fixture(&SPEC, cache_dir).expect("cache hit should succeed");

    assert_eq!(
        first.truth, second.truth,
        "ground-truth timeline must be identical across builds of the same spec",
    );
    assert_eq!(
        first.samples_16k.len(),
        second.samples_16k.len(),
        "cached audio length must match freshly synthesized length",
    );

    // Ground truth must be non-empty and internally consistent.
    assert_eq!(first.truth.len(), SPEC.voices.len());
    for (spk, spans) in first.truth.iter().enumerate() {
        assert!(!spans.is_empty(), "speaker {spk} has no spans");
        for (start, end) in spans {
            assert!(end > start, "span end must follow start: ({start}, {end})");
        }
    }
    // Second utterance is scripted at 3.0 s and must start there.
    let (s1_start, _s1_end) = first.truth[1][0];
    assert!(
        (s1_start - 3.0).abs() < 0.05,
        "speaker 1 should start near 3.0 s, got {s1_start}",
    );
}
