//! Diarization accuracy evaluation harness (integration test).
//!
//! Generates ground-truth fixtures with macOS `say`, runs the real
//! [`DiarizationEngine`] over each, and scores Frame_Accuracy / detected speaker
//! count / RTF against a committed baseline plus absolute targets.
//!
//! The harness is designed to keep `cargo test` green on machines that cannot
//! run it: it *skips* (printing an explicit message and passing) when the
//! diarization ONNX models are absent or when `say` is unavailable. Only the
//! scorer unit tests at the bottom of this file always run — they need neither
//! models nor TTS.
//!
//! Environment:
//! - `MEETILY_DIARIZATION_MODELS_DIR` overrides the models root (default:
//!   `<repo>/frontend/models`, resolved from `CARGO_MANIFEST_DIR`).
//! - `DIARIZATION_EVAL_RECORD_BASELINE=1` records `tests/data/diarization/
//!   baseline.json` instead of asserting against it.

mod support;

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use app_lib::diarization_engine::{
    cos_sim, DiarizationEngine, SpeakerTurn, IDENTIFICATION_COSINE_SIMILARITY,
};

use support::fixtures::{build_fixture, Degradation, Fixture, FixtureSpec, SkipReason, Utterance, SAMPLE_RATE};
use support::scoring::{score, EvalResult};

// ---------------------------------------------------------------------------
// Fixture scripts. Scripted placement is the ground truth; the clean and
// degraded variants share the same script so their timelines are identical.
// ---------------------------------------------------------------------------

/// Three distinct macOS voices (verified present on the dev/CI machines).
const VOICES3: &[&str] = &["Daniel", "Samantha", "Fred"];
const VOICES2: &[&str] = &["Daniel", "Samantha"];

/// Seed shared by the degraded fixtures (deterministic degradation).
const DEGRADE_SEED: u64 = 0xD1A5_2026;

fn degradation() -> Degradation {
    Degradation {
        snr_db: 10.0,
        rt60_secs: 0.3,
        seed: DEGRADE_SEED,
    }
}

/// Sequential 3-speaker script: utterances spaced ~5 s so they do not overlap
/// (~45 s total). Speakers cycle 0 -> 1 -> 2.
const SEQ3_SCRIPT: &[Utterance] = &[
    Utterance { speaker: 0, text: "Good morning everyone, thanks for joining the weekly sync.", start_secs: 0.0 },
    Utterance { speaker: 1, text: "Happy to be here. I finished the reporting dashboard yesterday.", start_secs: 5.0 },
    Utterance { speaker: 2, text: "Nice work. On my side the migration scripts are ready for review.", start_secs: 10.0 },
    Utterance { speaker: 0, text: "Great. Let us walk through the open blockers one by one.", start_secs: 15.0 },
    Utterance { speaker: 1, text: "The only blocker for me is the staging database credentials.", start_secs: 20.0 },
    Utterance { speaker: 2, text: "I can share those right after this call, no problem at all.", start_secs: 25.0 },
    Utterance { speaker: 0, text: "Perfect. Then we should be able to ship by the end of the week.", start_secs: 30.0 },
    Utterance { speaker: 1, text: "Sounds good to me. I will prepare the release notes tomorrow.", start_secs: 35.0 },
    Utterance { speaker: 2, text: "And I will run the final round of integration tests.", start_secs: 40.0 },
];

/// Overlapping 3-speaker script: tighter ~3 s spacing so consecutive utterances
/// partially overlap (~42 s total).
const OVERLAP3_SCRIPT: &[Utterance] = &[
    Utterance { speaker: 0, text: "I really think we should reconsider the whole approach here.", start_secs: 0.0 },
    Utterance { speaker: 1, text: "Wait, but the current design already handles that case fine.", start_secs: 3.0 },
    Utterance { speaker: 2, text: "Actually I agree with the first point, it is cleaner overall.", start_secs: 6.0 },
    Utterance { speaker: 0, text: "Exactly, and it would save us a lot of maintenance later on.", start_secs: 9.0 },
    Utterance { speaker: 1, text: "Fine, but who is going to rewrite all of the existing tests?", start_secs: 12.0 },
    Utterance { speaker: 2, text: "I can take that on if we split the work across two sprints.", start_secs: 15.0 },
    Utterance { speaker: 0, text: "Two sprints feels reasonable given everything else in the backlog.", start_secs: 18.0 },
    Utterance { speaker: 1, text: "Okay, you have convinced me, let us write it up as a proposal.", start_secs: 21.0 },
    Utterance { speaker: 2, text: "I will start a document and share it with the whole team today.", start_secs: 24.0 },
    Utterance { speaker: 0, text: "Wonderful, thank you both for being flexible about this change.", start_secs: 27.0 },
    Utterance { speaker: 1, text: "No problem, it is clearly the better long term decision for us.", start_secs: 30.0 },
    Utterance { speaker: 2, text: "Agreed, let us reconvene once the proposal draft is finished.", start_secs: 33.0 },
    Utterance { speaker: 0, text: "Perfect, I will put a follow up meeting on the calendar for us.", start_secs: 36.0 },
];

/// Two-speaker remote-track style script: well separated, ~5 s spacing (~40 s).
const TRACK2_SCRIPT: &[Utterance] = &[
    Utterance { speaker: 0, text: "Thanks for taking the time to talk through the proposal with me.", start_secs: 0.0 },
    Utterance { speaker: 1, text: "Of course, I read it last night and I have a few questions for you.", start_secs: 5.0 },
    Utterance { speaker: 0, text: "Please go ahead, I would love to hear your honest feedback on it.", start_secs: 10.0 },
    Utterance { speaker: 1, text: "My main concern is around the timeline for the second milestone.", start_secs: 15.0 },
    Utterance { speaker: 0, text: "That is fair, the second milestone is definitely the most ambitious.", start_secs: 20.0 },
    Utterance { speaker: 1, text: "Could we perhaps move some of that scope into a later phase instead?", start_secs: 25.0 },
    Utterance { speaker: 0, text: "Yes, I think deferring the analytics work would relieve the pressure.", start_secs: 30.0 },
    Utterance { speaker: 1, text: "That works for me, let us update the plan and circulate it again.", start_secs: 35.0 },
];

/// The five evaluation fixtures (Requirements 1.2, 1.3).
fn fixture_specs() -> Vec<FixtureSpec> {
    vec![
        FixtureSpec { name: "seq3", voices: VOICES3, script: SEQ3_SCRIPT, degrade: None },
        FixtureSpec { name: "overlap3", voices: VOICES3, script: OVERLAP3_SCRIPT, degrade: None },
        FixtureSpec { name: "track2", voices: VOICES2, script: TRACK2_SCRIPT, degrade: None },
        FixtureSpec { name: "seq3_degraded", voices: VOICES3, script: SEQ3_SCRIPT, degrade: Some(degradation()) },
        FixtureSpec { name: "overlap3_degraded", voices: VOICES3, script: OVERLAP3_SCRIPT, degrade: Some(degradation()) },
    ]
}

/// Absolute Frame_Accuracy target per fixture (Requirements 2.1-2.5).
fn absolute_target(name: &str) -> f64 {
    match name {
        "seq3" => 0.99,
        "overlap3" => 0.97,
        "track2" => 0.995,
        "seq3_degraded" => 0.95,
        "overlap3_degraded" => 0.90,
        other => panic!("no absolute target defined for fixture {other}"),
    }
}

/// Maximum acceptable real-time factor (Requirement 7.1).
const RTF_MAX: f64 = 0.15;

// ---------------------------------------------------------------------------
// Models directory resolution + presence check.
// ---------------------------------------------------------------------------

/// Root passed to [`DiarizationEngine::new_with_models_dir`]. The engine stores
/// the model pair under `<root>/diarization/diarization-default/`.
fn models_root() -> PathBuf {
    if let Ok(dir) = std::env::var("MEETILY_DIARIZATION_MODELS_DIR") {
        PathBuf::from(dir)
    } else {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../models")
    }
}

/// Whether the diarization ONNX pair is present on disk. Mirrors the engine's
/// own `is_on_disk` layout (`<root>/diarization/diarization-default/*.onnx`) and
/// is side-effect free, so a skip never creates directories.
fn models_present(root: &Path) -> bool {
    let dir = root.join("diarization").join("diarization-default");
    dir.join("segmentation.onnx").exists() && dir.join("embedding.onnx").exists()
}

// ---------------------------------------------------------------------------
// Baseline file (committed).
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize)]
struct Baseline {
    generator_version: u32,
    recorded_at_commit: String,
    fixtures: std::collections::BTreeMap<String, BaselineEntry>,
}

#[derive(Serialize, Deserialize, Clone, Copy)]
struct BaselineEntry {
    frame_accuracy: f64,
    detected: usize,
}

fn baseline_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/diarization/baseline.json")
}

fn current_commit() -> String {
    Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

// ---------------------------------------------------------------------------
// Harness entry point.
// ---------------------------------------------------------------------------

/// Build every fixture, skipping the whole test when `say` is unavailable.
fn build_all(specs: &[FixtureSpec]) -> Result<Vec<Fixture>, SkipReason> {
    let cache_dir = support::fixtures::default_cache_dir();
    specs.iter().map(|s| build_fixture(s, &cache_dir)).collect()
}

#[test]
fn diarization_accuracy_eval() {
    let specs = fixture_specs();

    // Fixture synthesis first: skip (not fail) if the host has no `say`.
    let fixtures = match build_all(&specs) {
        Ok(f) => f,
        Err(reason) => {
            eprintln!("SKIP diarization_accuracy_eval: {reason}");
            return;
        }
    };

    // Models presence is checked BEFORE the baseline check: a machine without
    // the ONNX models (e.g. models not downloaded yet) skips cleanly.
    let root = models_root();
    if !models_present(&root) {
        eprintln!(
            "SKIP diarization_accuracy_eval: diarization models not found under {} \
             (expected diarization/diarization-default/segmentation.onnx + embedding.onnx). \
             Set MEETILY_DIARIZATION_MODELS_DIR or download the models.",
            root.display()
        );
        return;
    }

    let rt = tokio::runtime::Runtime::new().expect("build tokio runtime");
    let engine = DiarizationEngine::new_with_models_dir(Some(root)).expect("construct engine");

    // Diarize + score every fixture, timing only the diarize call for RTF.
    let mut results: Vec<EvalResult> = Vec::with_capacity(specs.len());
    for (spec, fixture) in specs.iter().zip(fixtures.iter()) {
        let duration = fixture.samples_16k.len() as f32 / SAMPLE_RATE as f32;

        let start = std::time::Instant::now();
        let turns = rt
            .block_on(engine.diarize(&fixture.samples_16k, None))
            .unwrap_or_else(|e| panic!("diarize {} failed: {e}", spec.name));
        let elapsed = start.elapsed().as_secs_f64();

        let mut result = score(spec.name, &turns, &fixture.truth, duration);
        result.rtf = elapsed / duration as f64;

        println!(
            "[eval] {:<18} frame_accuracy={:.4} detected={}/{} rtf={:.4}",
            result.name, result.frame_accuracy, result.detected, result.expected, result.rtf
        );
        results.push(result);
    }

    // Baseline: record or assert.
    if std::env::var("DIARIZATION_EVAL_RECORD_BASELINE").as_deref() == Ok("1") {
        record_baseline(&results);
        return;
    }

    assert_against_baseline(&results);
}

fn record_baseline(results: &[EvalResult]) {
    let fixtures = results
        .iter()
        .map(|r| {
            (
                r.name.clone(),
                BaselineEntry { frame_accuracy: r.frame_accuracy, detected: r.detected },
            )
        })
        .collect();

    let baseline = Baseline {
        generator_version: support::fixtures::GENERATOR_VERSION,
        recorded_at_commit: current_commit(),
        fixtures,
    };

    let path = baseline_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create baseline dir");
    }
    let json = serde_json::to_string_pretty(&baseline).expect("serialize baseline");
    std::fs::write(&path, json).expect("write baseline");
    println!("[eval] recorded baseline to {}", path.display());
}

fn assert_against_baseline(results: &[EvalResult]) {
    let path = baseline_path();
    let bytes = std::fs::read(&path).unwrap_or_else(|_| {
        panic!(
            "baseline file missing at {}. Record it first by running the harness with \
             DIARIZATION_EVAL_RECORD_BASELINE=1 (on the unmodified engine).",
            path.display()
        )
    });
    let baseline: Baseline =
        serde_json::from_slice(&bytes).expect("parse baseline.json");

    // The absolute Frame_Accuracy targets and the exact detected-speaker-count
    // check are only satisfied by the engine AFTER the tasks 4-6 improvements
    // (window stitching, embedding, clustering refinement) land. On the
    // unmodified engine (the LOCAL-3 baseline) some fixtures fall short, so
    // enforcing those targets unconditionally would make `cargo test` red for
    // everyone before those tasks merge. They are therefore gated: they run when
    // `DIARIZATION_EVAL_STRICT=1` is set, OR — per fixture — when the recorded
    // baseline already fully meets that fixture's targets (both the accuracy
    // target and the correct speaker count). The baseline-regression guard and
    // the RTF bound below always run, so accuracy can never silently drop.
    let strict = std::env::var("DIARIZATION_EVAL_STRICT").as_deref() == Ok("1");

    for r in results {
        let base = baseline
            .fixtures
            .get(&r.name)
            .unwrap_or_else(|| panic!("fixture {} missing from baseline.json", r.name));

        let target = absolute_target(&r.name);

        // Always on: never regress below the recorded baseline.
        assert!(
            r.frame_accuracy >= base.frame_accuracy - 1e-9,
            "{}: frame_accuracy {:.4} regressed below baseline {:.4}",
            r.name, r.frame_accuracy, base.frame_accuracy
        );
        // Always on: RTF bound (already satisfied by the unmodified engine).
        assert!(
            r.rtf <= RTF_MAX,
            "{}: RTF {:.4} exceeds bound {:.4}",
            r.name, r.rtf, RTF_MAX
        );

        // Gated absolute targets: enforced under STRICT, or once the baseline
        // itself already meets both the accuracy target and the expected count.
        let baseline_meets_targets =
            base.frame_accuracy >= target && base.detected == r.expected;
        if strict || baseline_meets_targets {
            assert!(
                r.frame_accuracy >= target,
                "{}: frame_accuracy {:.4} below absolute target {:.4}",
                r.name, r.frame_accuracy, target
            );
            assert_eq!(
                r.detected, r.expected,
                "{}: detected {} speakers but expected {}",
                r.name, r.detected, r.expected
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Identity stability (Requirement 6.3).
// ---------------------------------------------------------------------------

const IDENTITY_VOICE: &[&str] = &["Daniel"];

const IDENTITY_SCRIPT_A: &[Utterance] = &[
    Utterance { speaker: 0, text: "The quarterly numbers came in stronger than any of us expected.", start_secs: 0.0 },
    Utterance { speaker: 0, text: "We should celebrate the whole team for the hard work they put in.", start_secs: 4.0 },
];

const IDENTITY_SCRIPT_B: &[Utterance] = &[
    Utterance { speaker: 0, text: "Tomorrow I plan to review the architecture document in detail.", start_secs: 0.0 },
    Utterance { speaker: 0, text: "There are a couple of sections that still need a lot more clarity.", start_secs: 4.0 },
];

/// Average the returned turn embeddings and L2-normalize the mean.
fn mean_embedding(turns: &[SpeakerTurn]) -> Vec<f32> {
    assert!(!turns.is_empty(), "no turns to average embeddings from");
    let dim = turns[0].embedding.len();
    let mut acc = vec![0.0f32; dim];
    for t in turns {
        assert_eq!(t.embedding.len(), dim, "inconsistent embedding dimensions");
        for (a, v) in acc.iter_mut().zip(t.embedding.iter()) {
            *a += *v;
        }
    }
    let norm = acc.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-12);
    for x in acc.iter_mut() {
        *x /= norm;
    }
    acc
}

#[test]
fn identity_is_stable_across_clips() {
    let spec_a = FixtureSpec { name: "identity_a", voices: IDENTITY_VOICE, script: IDENTITY_SCRIPT_A, degrade: None };
    let spec_b = FixtureSpec { name: "identity_b", voices: IDENTITY_VOICE, script: IDENTITY_SCRIPT_B, degrade: None };

    let cache_dir = support::fixtures::default_cache_dir();
    let (fa, fb) = match (build_fixture(&spec_a, &cache_dir), build_fixture(&spec_b, &cache_dir)) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(reason), _) | (_, Err(reason)) => {
            eprintln!("SKIP identity_is_stable_across_clips: {reason}");
            return;
        }
    };

    let root = models_root();
    if !models_present(&root) {
        eprintln!(
            "SKIP identity_is_stable_across_clips: diarization models not found under {}",
            root.display()
        );
        return;
    }

    let rt = tokio::runtime::Runtime::new().expect("build tokio runtime");
    let engine = DiarizationEngine::new_with_models_dir(Some(root)).expect("construct engine");

    let turns_a = rt.block_on(engine.diarize(&fa.samples_16k, None)).expect("diarize clip A");
    let turns_b = rt.block_on(engine.diarize(&fb.samples_16k, None)).expect("diarize clip B");

    let emb_a = mean_embedding(&turns_a);
    let emb_b = mean_embedding(&turns_b);

    let sim = cos_sim(&emb_a, &emb_b);
    println!("[eval] identity cosine similarity = {sim:.4} (threshold {IDENTIFICATION_COSINE_SIMILARITY})");

    assert!(
        sim >= IDENTIFICATION_COSINE_SIMILARITY,
        "same-voice clips matched with cosine similarity {sim:.4}, below threshold {IDENTIFICATION_COSINE_SIMILARITY}",
    );
}

// ---------------------------------------------------------------------------
// Scorer unit tests (Task 2.5) — no models or TTS required, always run.
// ---------------------------------------------------------------------------

fn turn(cluster_id: usize, start_secs: f32, end_secs: f32) -> SpeakerTurn {
    SpeakerTurn { start_secs, end_secs, cluster_id, embedding: Vec::new() }
}

#[test]
fn scorer_perfect_prediction_is_one() {
    let truth = vec![vec![(0.0, 1.0)], vec![(1.0, 2.0)]];
    let turns = vec![turn(0, 0.0, 1.0), turn(1, 1.0, 2.0)];
    let r = score("perfect", &turns, &truth, 2.0);
    assert!((r.frame_accuracy - 1.0).abs() < 1e-9, "got {}", r.frame_accuracy);
    assert_eq!(r.detected, 2);
    assert_eq!(r.expected, 2);
}

#[test]
fn scorer_permuted_cluster_ids_is_one() {
    let truth = vec![vec![(0.0, 1.0)], vec![(1.0, 2.0)]];
    // Cluster ids swapped relative to truth speaker order (and non-contiguous).
    let turns = vec![turn(9, 0.0, 1.0), turn(5, 1.0, 2.0)];
    let r = score("permuted", &turns, &truth, 2.0);
    assert!((r.frame_accuracy - 1.0).abs() < 1e-9, "got {}", r.frame_accuracy);
    assert_eq!(r.detected, 2);
}

#[test]
fn scorer_half_wrong_frames_is_half() {
    let truth = vec![vec![(0.0, 1.0)], vec![(1.0, 2.0)]];
    // A single cluster spanning both speakers: best mapping can only match one.
    let turns = vec![turn(0, 0.0, 2.0)];
    let r = score("half", &turns, &truth, 2.0);
    assert!((r.frame_accuracy - 0.5).abs() < 1e-9, "got {}", r.frame_accuracy);
    assert_eq!(r.detected, 1);
    assert_eq!(r.expected, 2);
}

#[test]
fn scorer_empty_turns_scores_silence_agreement() {
    // Speaker 0 speaks in the first half; the second half is silence.
    let truth = vec![vec![(0.0, 1.0)]];
    let turns: Vec<SpeakerTurn> = Vec::new();
    let r = score("empty", &turns, &truth, 2.0);
    // Speech frames wrong (predicted silence), silence frames correct -> 0.5.
    assert!((r.frame_accuracy - 0.5).abs() < 1e-9, "got {}", r.frame_accuracy);
    assert_eq!(r.detected, 0);
    assert_eq!(r.expected, 1);
}

#[test]
fn scorer_surfaces_detected_and_expected_mismatch() {
    let truth = vec![vec![(0.0, 1.0)], vec![(1.0, 2.0)], vec![(2.0, 3.0)]];
    // Only two clusters detected for three truth speakers.
    let turns = vec![turn(0, 0.0, 1.0), turn(1, 1.0, 2.0)];
    let r = score("mismatch", &turns, &truth, 3.0);
    assert_eq!(r.detected, 2);
    assert_eq!(r.expected, 3);
}
