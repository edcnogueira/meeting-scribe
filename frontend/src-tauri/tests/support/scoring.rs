//! Frame-accuracy scorer for the diarization evaluation harness.
//!
//! Ports the D1 spike's scoring approach into Rust test-support code: the
//! diarization result and the fixture ground truth are both quantized into
//! 100 ms frames, and the two are compared under the best cluster-to-speaker
//! mapping found by exhaustive permutation (fixtures use <= 3 speakers, so the
//! search is trivially small).
//!
//! Silence handling (kept consistent with the D1 spike): a frame in which the
//! ground truth is silent *and* the prediction is silent counts as a correct
//! frame — silence agreement is scored, not ignored. A frame where exactly one
//! side is silent is always wrong.
//!
//! Overlap handling: a ground-truth frame carries the *set* of speakers active
//! at its center. A predicted (mapped) speaker scores the frame correct when it
//! is a member of that set, so partial overlap is not double-penalized.

use app_lib::diarization_engine::SpeakerTurn;

/// Frame width used for scoring, in seconds (100 ms).
pub const FRAME_SECS: f32 = 0.1;

/// Result of scoring one fixture. `rtf` is filled in by the runner (the scorer
/// itself does not time diarization) and defaults to `0.0`.
#[derive(Debug, Clone)]
pub struct EvalResult {
    pub name: String,
    pub frame_accuracy: f64,
    pub detected: usize,
    pub expected: usize,
    pub rtf: f64,
}

/// Score `turns` against the ground-truth `truth` spans over `duration` seconds.
///
/// - `truth[s]` is the list of `(start, end)` spans (seconds) for truth speaker
///   `s`; `expected` is the number of truth speakers that actually speak.
/// - `detected` is the number of distinct `cluster_id`s present in `turns`.
/// - `frame_accuracy` is the fraction of 100 ms frames that are correct under
///   the best cluster-to-speaker mapping (see module docs).
///
/// `rtf` is left at `0.0`; the caller sets it after timing `diarize`.
pub fn score(
    name: &str,
    turns: &[SpeakerTurn],
    truth: &[Vec<(f32, f32)>],
    duration: f32,
) -> EvalResult {
    let n_frames = ((duration / FRAME_SECS).ceil() as usize).max(1);

    // Per-frame ground-truth speaker set and predicted cluster.
    let mut truth_frames: Vec<Vec<usize>> = Vec::with_capacity(n_frames);
    let mut pred_frames: Vec<Option<usize>> = Vec::with_capacity(n_frames);

    for i in 0..n_frames {
        let center = (i as f32 + 0.5) * FRAME_SECS;

        let mut active: Vec<usize> = Vec::new();
        for (spk, spans) in truth.iter().enumerate() {
            if spans.iter().any(|&(a, b)| center >= a && center < b) {
                active.push(spk);
            }
        }
        truth_frames.push(active);

        // Prediction: the cluster of the (first) turn covering the frame center.
        let pred = turns
            .iter()
            .find(|t| center >= t.start_secs && center < t.end_secs)
            .map(|t| t.cluster_id);
        pred_frames.push(pred);
    }

    // Distinct detected clusters (relabeled to a dense 0..detected index space).
    let mut clusters: Vec<usize> = turns.iter().map(|t| t.cluster_id).collect();
    clusters.sort_unstable();
    clusters.dedup();
    let detected = clusters.len();

    let expected = truth.iter().filter(|spans| !spans.is_empty()).count();
    let n_truth_speakers = truth.len();

    // Map each predicted cluster_id -> dense index for the permutation search.
    let cluster_index = |cid: usize| -> usize {
        clusters.iter().position(|&c| c == cid).expect("known cluster")
    };
    let pred_dense: Vec<Option<usize>> = pred_frames
        .iter()
        .map(|p| p.map(cluster_index))
        .collect();

    let best_correct = best_mapping_correct(
        &truth_frames,
        &pred_dense,
        detected,
        n_truth_speakers,
    );

    let frame_accuracy = best_correct as f64 / n_frames as f64;

    EvalResult {
        name: name.to_string(),
        frame_accuracy,
        detected,
        expected,
        rtf: 0.0,
    }
}

/// Count correct frames under the best injective cluster-to-speaker mapping.
///
/// Each of the `detected` clusters is mapped to a distinct truth speaker or to
/// nothing (an "unmapped extra" cluster). Silence frames (empty truth) are
/// correct exactly when the prediction is silent. The search is exhaustive over
/// all injective partial mappings — fine for <= 3 clusters / speakers.
fn best_mapping_correct(
    truth_frames: &[Vec<usize>],
    pred_dense: &[Option<usize>],
    detected: usize,
    n_truth_speakers: usize,
) -> usize {
    // Silence-only agreement is independent of the mapping: count it once and
    // add the best speech-frame agreement on top.
    let mut mapping = vec![None; detected];
    let mut used = vec![false; n_truth_speakers];
    let mut best = 0usize;
    search(
        0,
        detected,
        n_truth_speakers,
        &mut mapping,
        &mut used,
        truth_frames,
        pred_dense,
        &mut best,
    );
    best
}

#[allow(clippy::too_many_arguments)]
fn search(
    cluster: usize,
    detected: usize,
    n_truth_speakers: usize,
    mapping: &mut [Option<usize>],
    used: &mut [bool],
    truth_frames: &[Vec<usize>],
    pred_dense: &[Option<usize>],
    best: &mut usize,
) {
    if cluster == detected {
        let correct = count_correct(mapping, truth_frames, pred_dense);
        if correct > *best {
            *best = correct;
        }
        return;
    }

    // Option A: leave this cluster unmapped (an extra detected speaker).
    mapping[cluster] = None;
    search(
        cluster + 1,
        detected,
        n_truth_speakers,
        mapping,
        used,
        truth_frames,
        pred_dense,
        best,
    );

    // Option B: map this cluster to any not-yet-used truth speaker.
    for spk in 0..n_truth_speakers {
        if used[spk] {
            continue;
        }
        used[spk] = true;
        mapping[cluster] = Some(spk);
        search(
            cluster + 1,
            detected,
            n_truth_speakers,
            mapping,
            used,
            truth_frames,
            pred_dense,
            best,
        );
        used[spk] = false;
    }
    mapping[cluster] = None;
}

/// Count correct frames for a fixed mapping (dense cluster idx -> truth speaker).
fn count_correct(
    mapping: &[Option<usize>],
    truth_frames: &[Vec<usize>],
    pred_dense: &[Option<usize>],
) -> usize {
    let mut correct = 0usize;
    for (truth_set, pred) in truth_frames.iter().zip(pred_dense.iter()) {
        let ok = match (truth_set.is_empty(), pred) {
            // Silence agreement.
            (true, None) => true,
            // Predicted silence over speech, or speech over silence: wrong.
            (true, Some(_)) | (false, None) => false,
            // Speech: correct when the mapped speaker is active in this frame.
            (false, Some(c)) => match mapping[*c] {
                Some(spk) => truth_set.contains(&spk),
                None => false,
            },
        };
        if ok {
            correct += 1;
        }
    }
    correct
}
