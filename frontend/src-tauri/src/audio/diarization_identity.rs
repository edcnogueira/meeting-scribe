//! Speaker identity registry: matching + enrollment (task D4).
//!
//! Turns anonymous per-meeting clusters into named people. Three concerns live
//! here:
//!   1. **Pure matching / averaging helpers** — cosine best-match against the
//!      local registry, incremental (capped) mean enrollment, and weighted merge.
//!      All unit-tested with synthetic embeddings, no ONNX model involved.
//!   2. **Enrollment by rename** — `api_rename_meeting_speaker` folds a cluster's
//!      embedding into a person's profile and re-labels that meeting's segments.
//!   3. **Registry management commands** — list / rename / merge / delete people.
//!
//! Privacy: voice embeddings live only in the local SQLite database. They are
//! never logged, emitted, or sent anywhere.

use crate::database::repositories::speakers::{SpeakerIdentity, SpeakersRepository};
use crate::diarization_engine::cos_sim;
use crate::state::AppState;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use tauri::{AppHandle, Manager, Runtime};

/// Cosine-similarity floor to accept a cluster as an enrolled person (D1: same
/// person 0.975–0.993, different people <= 0.53; 0.65 is conservative).
pub const IDENTIFICATION_THRESHOLD: f32 = crate::diarization_engine::IDENTIFICATION_COSINE_SIMILARITY;

/// Maximum number of cluster embeddings folded into one person's reference
/// vector. Past this the running mean keeps adapting but never freezes (the
/// oldest samples decay), so a person's voice profile stays current without a
/// single loud meeting dominating it forever.
pub const MAX_ENROLLMENT_SAMPLES: u32 = 10;

/// Env override for the machine owner's reserved profile name. Default "Eu".
const SELF_NAME_ENV: &str = "MEETILY_SELF_NAME";

/// Reserved display name for the machine owner ("Eu" unless overridden).
pub fn self_name() -> String {
    match std::env::var(SELF_NAME_ENV) {
        Ok(v) if !v.trim().is_empty() => v.trim().to_string(),
        _ => "Eu".to_string(),
    }
}

// ----------------------------------------------------------------------------
// Pure helpers (unit-tested without any model).
// ----------------------------------------------------------------------------

/// A candidate enrolled identity considered during matching.
#[derive(Debug, Clone)]
pub struct IdentityCandidate {
    pub id: String,
    pub name: String,
    pub embedding: Vec<f32>,
}

impl From<&SpeakerIdentity> for IdentityCandidate {
    fn from(s: &SpeakerIdentity) -> Self {
        IdentityCandidate {
            id: s.id.clone(),
            name: s.name.clone(),
            embedding: s.embedding.clone(),
        }
    }
}

/// L2-normalize a vector in place (no-op on the zero vector).
fn l2_normalize(v: &mut [f32]) {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 1e-9 {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
}

/// Best enrolled identity whose cosine similarity to `cluster_embedding` is at
/// or above `threshold`. Returns `(id, name, score)`; `None` when nobody clears
/// the bar. On ties the first-seen (name-sorted) candidate wins deterministically.
pub fn best_identity_match(
    cluster_embedding: &[f32],
    candidates: &[IdentityCandidate],
    threshold: f32,
) -> Option<(String, String, f32)> {
    if cluster_embedding.is_empty() {
        return None;
    }
    let mut best: Option<(String, String, f32)> = None;
    for c in candidates {
        if c.embedding.len() != cluster_embedding.len() || c.embedding.is_empty() {
            continue;
        }
        let score = cos_sim(cluster_embedding, &c.embedding);
        if score >= threshold {
            let better = match &best {
                Some((_, _, bs)) => score > *bs,
                None => true,
            };
            if better {
                best = Some((c.id.clone(), c.name.clone(), score));
            }
        }
    }
    best
}

/// Fold a new cluster embedding into a person's running reference vector.
///
/// Returns `(new_reference_normalized, new_sample_count)`. Behavior:
///   - Empty/zero-count current profile → adopt the sample (count 1).
///   - Otherwise a running mean weighted by `min(current_count, cap)`, so once
///     `cap` samples are reached the profile keeps a `cap : 1` blend (never
///     frozen, never dominated by a single sample). Count saturates at `cap`.
///   - Length mismatch → keep the current profile unchanged (defensive).
pub fn incremental_average(
    current: &[f32],
    current_count: u32,
    sample: &[f32],
    cap: u32,
) -> (Vec<f32>, u32) {
    if sample.is_empty() {
        return (current.to_vec(), current_count);
    }
    if current.is_empty() || current_count == 0 {
        let mut v = sample.to_vec();
        l2_normalize(&mut v);
        return (v, 1);
    }
    if current.len() != sample.len() {
        return (current.to_vec(), current_count);
    }
    let cap = cap.max(1);
    let n = current_count.min(cap) as f32;
    let mut out: Vec<f32> = current
        .iter()
        .zip(sample)
        .map(|(c, s)| (c * n + s) / (n + 1.0))
        .collect();
    l2_normalize(&mut out);
    let new_count = (current_count + 1).min(cap);
    (out, new_count)
}

/// Weighted merge of two identity profiles (used when fusing two people).
///
/// Returns `(reference_normalized, sample_count)`; the surviving vector is the
/// sample-count-weighted mean, and the count is `a + b` capped at `cap`. Handles
/// empty profiles and length mismatch (keeps the higher-count side).
pub fn merge_profiles(
    a: &[f32],
    a_count: u32,
    b: &[f32],
    b_count: u32,
    cap: u32,
) -> (Vec<f32>, u32) {
    let cap = cap.max(1);
    let empty_a = a.is_empty();
    let empty_b = b.is_empty();
    if empty_a && empty_b {
        return (Vec::new(), 0);
    }
    if empty_a {
        let mut v = b.to_vec();
        l2_normalize(&mut v);
        return (v, b_count.max(1).min(cap));
    }
    if empty_b || a.len() != b.len() {
        let mut v = a.to_vec();
        l2_normalize(&mut v);
        return (v, (a_count + b_count).max(1).min(cap));
    }
    let wa = a_count.max(1) as f32;
    let wb = b_count.max(1) as f32;
    let mut out: Vec<f32> = a
        .iter()
        .zip(b)
        .map(|(x, y)| (x * wa + y * wb) / (wa + wb))
        .collect();
    l2_normalize(&mut out);
    let new_count = (a_count + b_count).max(1).min(cap);
    (out, new_count)
}

/// Resolution of one per-meeting cluster against the registry.
#[derive(Debug, Clone, PartialEq)]
pub struct ClusterResolution {
    /// Per-meeting cluster label, e.g. "Speaker 1".
    pub cluster_label: String,
    /// Matched identity id (None → stays anonymous).
    pub speaker_id: Option<String>,
    /// Display label for transcripts: the person's name when matched, else the
    /// cluster label unchanged.
    pub display_label: String,
    /// Match score when matched.
    pub score: Option<f32>,
}

/// Match every cluster centroid against the registry, producing a display label
/// per cluster (the person's name when >= threshold, otherwise the cluster label).
pub fn resolve_clusters(
    centroids: &BTreeMap<String, Vec<f32>>,
    candidates: &[IdentityCandidate],
    threshold: f32,
) -> Vec<ClusterResolution> {
    centroids
        .iter()
        .map(|(cluster_label, centroid)| {
            match best_identity_match(centroid, candidates, threshold) {
                Some((id, name, score)) => ClusterResolution {
                    cluster_label: cluster_label.clone(),
                    speaker_id: Some(id),
                    display_label: name,
                    score: Some(score),
                },
                None => ClusterResolution {
                    cluster_label: cluster_label.clone(),
                    speaker_id: None,
                    display_label: cluster_label.clone(),
                    score: None,
                },
            }
        })
        .collect()
}

// ----------------------------------------------------------------------------
// Enrollment helper shared with the diarization pipeline.
// ----------------------------------------------------------------------------

/// Fold a cluster embedding into a person by name, creating them if needed, and
/// return the identity id. `is_self` marks/creates the owner profile. No-op-safe
/// on an empty embedding (still ensures the identity exists).
pub async fn enroll_embedding_by_name(
    pool: &sqlx::SqlitePool,
    name: &str,
    embedding: &[f32],
    is_self: bool,
) -> Result<String, sqlx::Error> {
    let existing = if is_self {
        SpeakersRepository::get_self_identity(pool).await?
    } else {
        SpeakersRepository::get_speaker_identity_by_name(pool, name).await?
    };

    match existing {
        Some(identity) => {
            if !embedding.is_empty() {
                let (merged, count) = incremental_average(
                    &identity.embedding,
                    identity.sample_count,
                    embedding,
                    MAX_ENROLLMENT_SAMPLES,
                );
                SpeakersRepository::update_speaker_embedding(pool, &identity.id, &merged, count)
                    .await?;
            }
            Ok(identity.id)
        }
        None => {
            let emb = if embedding.is_empty() {
                None
            } else {
                Some(embedding)
            };
            let count = if embedding.is_empty() { 0 } else { 1 };
            SpeakersRepository::create_speaker_identity(pool, name, emb, count, is_self).await
        }
    }
}

// ----------------------------------------------------------------------------
// Tauri commands
// ----------------------------------------------------------------------------

/// Public (embedding-free) view of an enrolled identity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeakerIdentityInfo {
    pub id: String,
    pub name: String,
    pub sample_count: u32,
    pub is_self: bool,
    pub has_embedding: bool,
}

impl From<SpeakerIdentity> for SpeakerIdentityInfo {
    fn from(s: SpeakerIdentity) -> Self {
        SpeakerIdentityInfo {
            id: s.id,
            name: s.name,
            sample_count: s.sample_count,
            is_self: s.is_self,
            has_embedding: !s.embedding.is_empty(),
        }
    }
}

fn pool_from<R: Runtime>(app: &AppHandle<R>) -> Result<sqlx::SqlitePool, String> {
    let state = app
        .try_state::<AppState>()
        .ok_or_else(|| "App state not available".to_string())?;
    Ok(state.db_manager.pool().clone())
}

/// List enrolled identities (never exposes raw embeddings).
#[tauri::command]
pub async fn api_list_speaker_identities<R: Runtime>(
    app: AppHandle<R>,
) -> Result<Vec<SpeakerIdentityInfo>, String> {
    let pool = pool_from(&app)?;
    let identities = SpeakersRepository::list_speaker_identities(&pool)
        .await
        .map_err(|e| format!("Failed to list identities: {}", e))?;
    Ok(identities.into_iter().map(Into::into).collect())
}

/// Rename an enrolled identity.
#[tauri::command]
pub async fn api_rename_speaker_identity<R: Runtime>(
    app: AppHandle<R>,
    speaker_id: String,
    name: String,
) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Name must not be empty".to_string());
    }
    let pool = pool_from(&app)?;
    SpeakersRepository::rename_speaker_identity(&pool, &speaker_id, name)
        .await
        .map_err(|e| format!("Failed to rename identity: {}", e))
}

/// Merge two identities: `source` is folded into `target` (weighted mean + all
/// meeting references repointed), then `source` is deleted.
#[tauri::command]
pub async fn api_merge_speaker_identities<R: Runtime>(
    app: AppHandle<R>,
    target_id: String,
    source_id: String,
) -> Result<SpeakerIdentityInfo, String> {
    if target_id == source_id {
        return Err("Cannot merge an identity into itself".to_string());
    }
    let pool = pool_from(&app)?;

    let target = SpeakersRepository::get_speaker_identity(&pool, &target_id)
        .await
        .map_err(|e| format!("Failed to load target identity: {}", e))?
        .ok_or_else(|| "Target identity not found".to_string())?;
    let source = SpeakersRepository::get_speaker_identity(&pool, &source_id)
        .await
        .map_err(|e| format!("Failed to load source identity: {}", e))?
        .ok_or_else(|| "Source identity not found".to_string())?;

    let (merged, count) = merge_profiles(
        &target.embedding,
        target.sample_count,
        &source.embedding,
        source.sample_count,
        MAX_ENROLLMENT_SAMPLES,
    );

    SpeakersRepository::update_speaker_embedding(&pool, &target_id, &merged, count)
        .await
        .map_err(|e| format!("Failed to update merged embedding: {}", e))?;
    // Repoint historical references before deleting the source.
    SpeakersRepository::reassign_meeting_speaker_refs(&pool, &source_id, &target_id)
        .await
        .map_err(|e| format!("Failed to repoint meeting references: {}", e))?;
    SpeakersRepository::delete_speaker_identity(&pool, &source_id)
        .await
        .map_err(|e| format!("Failed to delete source identity: {}", e))?;

    let updated = SpeakersRepository::get_speaker_identity(&pool, &target_id)
        .await
        .map_err(|e| format!("Failed to reload merged identity: {}", e))?
        .ok_or_else(|| "Merged identity vanished".to_string())?;
    Ok(updated.into())
}

/// Delete an identity's biometric profile. Past `transcripts.speaker` text is
/// preserved; `meeting_speakers.speaker_id` references are NULLed by the FK.
#[tauri::command]
pub async fn api_delete_speaker_identity<R: Runtime>(
    app: AppHandle<R>,
    speaker_id: String,
) -> Result<(), String> {
    let pool = pool_from(&app)?;
    SpeakersRepository::delete_speaker_identity(&pool, &speaker_id)
        .await
        .map_err(|e| format!("Failed to delete identity: {}", e))
}

/// Result of an enrollment-by-rename.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenameMeetingSpeakerResult {
    pub meeting_id: String,
    pub cluster_label: String,
    pub speaker_id: String,
    pub name: String,
    pub segments_relabeled: usize,
}

/// Enroll a meeting cluster as a named person: fold its embedding into that
/// person's profile (creating them if new) and re-label the meeting's segments.
#[tauri::command]
pub async fn api_rename_meeting_speaker<R: Runtime>(
    app: AppHandle<R>,
    meeting_id: String,
    cluster_label: String,
    name: String,
) -> Result<RenameMeetingSpeakerResult, String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("Name must not be empty".to_string());
    }
    let pool = pool_from(&app)?;

    // Locate the cluster row for this meeting.
    let cluster = SpeakersRepository::get_meeting_speaker_by_label(&pool, &meeting_id, &cluster_label)
        .await
        .map_err(|e| format!("Failed to load cluster: {}", e))?
        .ok_or_else(|| format!("Cluster '{}' not found for meeting", cluster_label))?;

    // Current display label of this cluster (person name if already matched,
    // else the cluster label) — that's what transcript rows carry today.
    let old_display = match &cluster.speaker_id {
        Some(sid) => SpeakersRepository::get_speaker_identity(&pool, sid)
            .await
            .map_err(|e| format!("Failed to load current identity: {}", e))?
            .map(|s| s.name)
            .unwrap_or_else(|| cluster.cluster_label.clone()),
        None => cluster.cluster_label.clone(),
    };

    // Fold the cluster embedding into the (possibly new) person's profile.
    let is_self = name == self_name();
    let speaker_id = enroll_embedding_by_name(&pool, &name, &cluster.embedding, is_self)
        .await
        .map_err(|e| format!("Failed to enroll identity: {}", e))?;

    // Score of this cluster against the (updated) profile, for the record.
    let score = SpeakersRepository::get_speaker_identity(&pool, &speaker_id)
        .await
        .map_err(|e| format!("Failed to reload identity: {}", e))?
        .and_then(|s| {
            if s.embedding.len() == cluster.embedding.len() && !cluster.embedding.is_empty() {
                Some(cos_sim(&cluster.embedding, &s.embedding) as f64)
            } else {
                None
            }
        });

    // Point the cluster row at the identity.
    SpeakersRepository::set_meeting_speaker_identity(
        &pool,
        &meeting_id,
        &cluster_label,
        Some(&speaker_id),
        score,
    )
    .await
    .map_err(|e| format!("Failed to link cluster to identity: {}", e))?;

    // Re-label this meeting's transcript segments that carried the old label.
    let relabeled = sqlx::query(
        "UPDATE transcripts SET speaker = ? WHERE meeting_id = ? AND speaker = ?",
    )
    .bind(&name)
    .bind(&meeting_id)
    .bind(&old_display)
    .execute(&pool)
    .await
    .map_err(|e| format!("Failed to relabel transcripts: {}", e))?
    .rows_affected() as usize;

    Ok(RenameMeetingSpeakerResult {
        meeting_id,
        cluster_label,
        speaker_id,
        name,
        segments_relabeled: relabeled,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn norm(v: Vec<f32>) -> Vec<f32> {
        let mut v = v;
        l2_normalize(&mut v);
        v
    }

    fn cand(id: &str, name: &str, emb: Vec<f32>) -> IdentityCandidate {
        IdentityCandidate {
            id: id.to_string(),
            name: name.to_string(),
            embedding: norm(emb),
        }
    }

    #[test]
    fn match_above_threshold_returns_person() {
        let emb = norm(vec![1.0, 0.1, 0.0]);
        let candidates = vec![cand("spk-1", "Joao", vec![1.0, 0.1, 0.0])];
        let m = best_identity_match(&emb, &candidates, IDENTIFICATION_THRESHOLD);
        let (id, name, score) = m.expect("should match");
        assert_eq!(id, "spk-1");
        assert_eq!(name, "Joao");
        assert!(score >= IDENTIFICATION_THRESHOLD);
    }

    #[test]
    fn match_below_threshold_returns_none() {
        // Orthogonal vectors → cosine 0, well below 0.65.
        let emb = norm(vec![1.0, 0.0, 0.0]);
        let candidates = vec![cand("spk-1", "Joao", vec![0.0, 1.0, 0.0])];
        assert!(best_identity_match(&emb, &candidates, IDENTIFICATION_THRESHOLD).is_none());
    }

    #[test]
    fn match_picks_best_of_multiple_candidates() {
        let emb = norm(vec![1.0, 0.2, 0.0]);
        let candidates = vec![
            cand("spk-1", "Ana", vec![1.0, 0.9, 0.0]),   // farther
            cand("spk-2", "Bruno", vec![1.0, 0.2, 0.0]), // exact direction
            cand("spk-3", "Carla", vec![0.0, 1.0, 0.0]), // orthogonal-ish
        ];
        let (id, name, _) =
            best_identity_match(&emb, &candidates, IDENTIFICATION_THRESHOLD).expect("match");
        assert_eq!(id, "spk-2");
        assert_eq!(name, "Bruno");
    }

    #[test]
    fn match_skips_mismatched_dimensions() {
        let emb = norm(vec![1.0, 0.0]);
        let candidates = vec![cand("spk-1", "Joao", vec![1.0, 0.0, 0.0])];
        assert!(best_identity_match(&emb, &candidates, 0.0).is_none());
    }

    #[test]
    fn incremental_average_from_empty_adopts_sample() {
        let (out, count) = incremental_average(&[], 0, &[3.0, 0.0], MAX_ENROLLMENT_SAMPLES);
        assert_eq!(count, 1);
        // Normalized unit vector.
        assert!((out[0] - 1.0).abs() < 1e-6);
        assert!(out[1].abs() < 1e-6);
    }

    #[test]
    fn incremental_average_moves_toward_sample_and_counts() {
        let start = norm(vec![1.0, 0.0]);
        let (out, count) = incremental_average(&start, 1, &norm(vec![0.0, 1.0]), MAX_ENROLLMENT_SAMPLES);
        assert_eq!(count, 2);
        // With weight 1:1 the mean sits at 45°; both components equal & positive.
        assert!(out[0] > 0.0 && out[1] > 0.0);
        assert!((out[0] - out[1]).abs() < 1e-6);
    }

    #[test]
    fn incremental_average_caps_sample_count() {
        let mut emb = norm(vec![1.0, 0.0, 0.0]);
        let mut count = 1u32;
        for _ in 0..50 {
            let (e, c) = incremental_average(&emb, count, &norm(vec![1.0, 0.0, 0.0]), MAX_ENROLLMENT_SAMPLES);
            emb = e;
            count = c;
        }
        assert_eq!(count, MAX_ENROLLMENT_SAMPLES);
    }

    #[test]
    fn incremental_average_cap_keeps_adapting() {
        // At the cap, a new sample still nudges the profile (never frozen).
        let base = norm(vec![1.0, 0.0]);
        let (out, count) =
            incremental_average(&base, MAX_ENROLLMENT_SAMPLES, &norm(vec![0.0, 1.0]), MAX_ENROLLMENT_SAMPLES);
        assert_eq!(count, MAX_ENROLLMENT_SAMPLES);
        assert!(out[1] > 0.0, "profile should shift toward the new sample");
        // But only slightly (10:1 blend), so the original direction still dominates.
        assert!(out[0] > out[1]);
    }

    #[test]
    fn incremental_average_ignores_dimension_mismatch() {
        let base = norm(vec![1.0, 0.0, 0.0]);
        let (out, count) = incremental_average(&base, 2, &[1.0, 0.0], MAX_ENROLLMENT_SAMPLES);
        assert_eq!(out, base);
        assert_eq!(count, 2);
    }

    #[test]
    fn merge_profiles_weights_by_count() {
        // a has far more samples → merged vector stays close to a.
        let a = norm(vec![1.0, 0.0]);
        let b = norm(vec![0.0, 1.0]);
        let (out, count) = merge_profiles(&a, 9, &b, 1, MAX_ENROLLMENT_SAMPLES);
        assert_eq!(count, MAX_ENROLLMENT_SAMPLES); // 9+1 capped at 10
        assert!(out[0] > out[1], "should lean toward the heavier profile");
    }

    #[test]
    fn merge_profiles_handles_empty_side() {
        let b = norm(vec![0.0, 1.0]);
        let (out, count) = merge_profiles(&[], 0, &b, 3, MAX_ENROLLMENT_SAMPLES);
        assert_eq!(out, b);
        assert_eq!(count, 3);
    }

    #[test]
    fn resolve_clusters_labels_matched_and_leaves_others() {
        let mut centroids = BTreeMap::new();
        centroids.insert("Speaker 1".to_string(), norm(vec![1.0, 0.0, 0.0]));
        centroids.insert("Speaker 2".to_string(), norm(vec![0.0, 1.0, 0.0]));
        let candidates = vec![cand("spk-1", "Joao", vec![1.0, 0.05, 0.0])];

        let res = resolve_clusters(&centroids, &candidates, IDENTIFICATION_THRESHOLD);
        let s1 = res.iter().find(|r| r.cluster_label == "Speaker 1").unwrap();
        let s2 = res.iter().find(|r| r.cluster_label == "Speaker 2").unwrap();
        assert_eq!(s1.display_label, "Joao");
        assert_eq!(s1.speaker_id.as_deref(), Some("spk-1"));
        // Speaker 2 doesn't match anyone → stays anonymous.
        assert_eq!(s2.display_label, "Speaker 2");
        assert!(s2.speaker_id.is_none());
    }

    #[test]
    fn self_name_defaults_to_eu() {
        std::env::remove_var(SELF_NAME_ENV);
        assert_eq!(self_name(), "Eu");
        std::env::set_var(SELF_NAME_ENV, "Owner");
        assert_eq!(self_name(), "Owner");
        std::env::remove_var(SELF_NAME_ENV);
    }
}
