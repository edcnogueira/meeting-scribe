//! Persistence for diarization speakers (task D3).
//!
//! - `meeting_speakers`: per-meeting clusters (label + mean embedding, plus an
//!   optional resolved identity — filled by D4).
//! - `speakers`: enrolled identities. D3 only creates the table/repo; the
//!   cross-session matching that populates it belongs to D4.

use chrono::Utc;
use sqlx::SqlitePool;
use uuid::Uuid;

pub struct SpeakersRepository;

/// A per-meeting clustered speaker row.
#[derive(Debug, Clone)]
pub struct MeetingSpeaker {
    pub id: String,
    pub meeting_id: String,
    pub cluster_label: String,
    pub speaker_id: Option<String>,
    pub score: Option<f64>,
    pub embedding: Vec<f32>,
}

/// An enrolled speaker identity (a person in the local registry, task D4).
///
/// `embedding` is the running L2-normalized reference vector; `sample_count`
/// tracks how many cluster embeddings have been folded into it (capped during
/// enrollment). `is_self` marks the machine owner ("Eu"), fed from the mic track.
#[derive(Debug, Clone)]
pub struct SpeakerIdentity {
    pub id: String,
    pub name: String,
    pub embedding: Vec<f32>,
    pub sample_count: u32,
    pub is_self: bool,
}

/// Serialize an embedding to a little-endian f32 byte blob.
pub fn embedding_to_blob(embedding: &[f32]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(embedding.len() * 4);
    for x in embedding {
        buf.extend_from_slice(&x.to_le_bytes());
    }
    buf
}

/// Deserialize a little-endian f32 byte blob back into an embedding.
pub fn blob_to_embedding(blob: &[u8]) -> Vec<f32> {
    blob.chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

impl SpeakersRepository {
    /// Remove all clustered speakers for a meeting (idempotent re-diarization).
    pub async fn delete_meeting_speakers(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM meeting_speakers WHERE meeting_id = ?")
            .bind(meeting_id)
            .execute(pool)
            .await?;
        Ok(())
    }

    /// Insert one clustered speaker row for a meeting. Returns the generated id.
    pub async fn insert_meeting_speaker(
        pool: &SqlitePool,
        meeting_id: &str,
        cluster_label: &str,
        embedding: &[f32],
        speaker_id: Option<&str>,
        score: Option<f64>,
    ) -> Result<String, sqlx::Error> {
        let id = format!("mspk-{}", Uuid::new_v4());
        let now = Utc::now().to_rfc3339();
        let blob = embedding_to_blob(embedding);

        sqlx::query(
            "INSERT INTO meeting_speakers
                (id, meeting_id, cluster_label, speaker_id, score, embedding, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(meeting_id)
        .bind(cluster_label)
        .bind(speaker_id)
        .bind(score)
        .bind(blob)
        .bind(&now)
        .execute(pool)
        .await?;

        Ok(id)
    }

    /// Fetch all clustered speakers for a meeting.
    pub async fn get_meeting_speakers(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<Vec<MeetingSpeaker>, sqlx::Error> {
        let rows = sqlx::query_as::<_, (String, String, String, Option<String>, Option<f64>, Option<Vec<u8>>)>(
            "SELECT id, meeting_id, cluster_label, speaker_id, score, embedding
             FROM meeting_speakers WHERE meeting_id = ? ORDER BY cluster_label ASC",
        )
        .bind(meeting_id)
        .fetch_all(pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|(id, meeting_id, cluster_label, speaker_id, score, embedding)| MeetingSpeaker {
                id,
                meeting_id,
                cluster_label,
                speaker_id,
                score,
                embedding: embedding.map(|b| blob_to_embedding(&b)).unwrap_or_default(),
            })
            .collect())
    }

    /// Fetch one meeting cluster row by its per-meeting label.
    pub async fn get_meeting_speaker_by_label(
        pool: &SqlitePool,
        meeting_id: &str,
        cluster_label: &str,
    ) -> Result<Option<MeetingSpeaker>, sqlx::Error> {
        let row = sqlx::query_as::<_, (String, String, String, Option<String>, Option<f64>, Option<Vec<u8>>)>(
            "SELECT id, meeting_id, cluster_label, speaker_id, score, embedding
             FROM meeting_speakers WHERE meeting_id = ? AND cluster_label = ? LIMIT 1",
        )
        .bind(meeting_id)
        .bind(cluster_label)
        .fetch_optional(pool)
        .await?;

        Ok(row.map(|(id, meeting_id, cluster_label, speaker_id, score, embedding)| MeetingSpeaker {
            id,
            meeting_id,
            cluster_label,
            speaker_id,
            score,
            embedding: embedding.map(|b| blob_to_embedding(&b)).unwrap_or_default(),
        }))
    }

    /// Point a meeting cluster row at a resolved identity (nullable) + score.
    pub async fn set_meeting_speaker_identity(
        pool: &SqlitePool,
        meeting_id: &str,
        cluster_label: &str,
        speaker_id: Option<&str>,
        score: Option<f64>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE meeting_speakers SET speaker_id = ?, score = ?
             WHERE meeting_id = ? AND cluster_label = ?",
        )
        .bind(speaker_id)
        .bind(score)
        .bind(meeting_id)
        .bind(cluster_label)
        .execute(pool)
        .await?;
        Ok(())
    }

    // ------------------------------------------------------------------
    // Enrolled identities (task D4). Embeddings never leave this database.
    // ------------------------------------------------------------------

    fn map_identity(
        id: String,
        name: String,
        embeddings: Option<Vec<u8>>,
        sample_count: i64,
        is_self: i64,
    ) -> SpeakerIdentity {
        SpeakerIdentity {
            id,
            name,
            embedding: embeddings.map(|b| blob_to_embedding(&b)).unwrap_or_default(),
            sample_count: sample_count.max(0) as u32,
            is_self: is_self != 0,
        }
    }

    /// List every enrolled identity (with its reference embedding).
    pub async fn list_speaker_identities(
        pool: &SqlitePool,
    ) -> Result<Vec<SpeakerIdentity>, sqlx::Error> {
        let rows = sqlx::query_as::<_, (String, String, Option<Vec<u8>>, i64, i64)>(
            "SELECT id, name, embeddings, sample_count, is_self FROM speakers ORDER BY name ASC",
        )
        .fetch_all(pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(id, name, emb, sc, is_self)| Self::map_identity(id, name, emb, sc, is_self))
            .collect())
    }

    /// Fetch one enrolled identity by id.
    pub async fn get_speaker_identity(
        pool: &SqlitePool,
        id: &str,
    ) -> Result<Option<SpeakerIdentity>, sqlx::Error> {
        let row = sqlx::query_as::<_, (String, String, Option<Vec<u8>>, i64, i64)>(
            "SELECT id, name, embeddings, sample_count, is_self FROM speakers WHERE id = ? LIMIT 1",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;
        Ok(row.map(|(id, name, emb, sc, is_self)| Self::map_identity(id, name, emb, sc, is_self)))
    }

    /// Fetch the machine owner's ("Eu") identity, if one has been enrolled.
    pub async fn get_self_identity(
        pool: &SqlitePool,
    ) -> Result<Option<SpeakerIdentity>, sqlx::Error> {
        let row = sqlx::query_as::<_, (String, String, Option<Vec<u8>>, i64, i64)>(
            "SELECT id, name, embeddings, sample_count, is_self FROM speakers WHERE is_self != 0 LIMIT 1",
        )
        .fetch_optional(pool)
        .await?;
        Ok(row.map(|(id, name, emb, sc, is_self)| Self::map_identity(id, name, emb, sc, is_self)))
    }

    /// Find an identity by exact name (case-sensitive).
    pub async fn get_speaker_identity_by_name(
        pool: &SqlitePool,
        name: &str,
    ) -> Result<Option<SpeakerIdentity>, sqlx::Error> {
        let row = sqlx::query_as::<_, (String, String, Option<Vec<u8>>, i64, i64)>(
            "SELECT id, name, embeddings, sample_count, is_self FROM speakers WHERE name = ? LIMIT 1",
        )
        .bind(name)
        .fetch_optional(pool)
        .await?;
        Ok(row.map(|(id, name, emb, sc, is_self)| Self::map_identity(id, name, emb, sc, is_self)))
    }

    /// Create an enrolled speaker identity. Returns the generated id.
    pub async fn create_speaker_identity(
        pool: &SqlitePool,
        name: &str,
        embedding: Option<&[f32]>,
        sample_count: u32,
        is_self: bool,
    ) -> Result<String, sqlx::Error> {
        let id = format!("spk-{}", Uuid::new_v4());
        let now = Utc::now().to_rfc3339();
        let blob = embedding.map(embedding_to_blob);

        sqlx::query(
            "INSERT INTO speakers (id, name, embeddings, sample_count, is_self, created_at)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(name)
        .bind(blob)
        .bind(sample_count as i64)
        .bind(is_self as i64)
        .bind(&now)
        .execute(pool)
        .await?;

        Ok(id)
    }

    /// Back-compat thin wrapper (kept for callers created in D3).
    pub async fn create_speaker(
        pool: &SqlitePool,
        name: &str,
        embedding: Option<&[f32]>,
    ) -> Result<String, sqlx::Error> {
        Self::create_speaker_identity(pool, name, embedding, embedding.map_or(0, |_| 1), false).await
    }

    /// Replace an identity's reference embedding and sample count.
    pub async fn update_speaker_embedding(
        pool: &SqlitePool,
        id: &str,
        embedding: &[f32],
        sample_count: u32,
    ) -> Result<(), sqlx::Error> {
        let blob = embedding_to_blob(embedding);
        sqlx::query("UPDATE speakers SET embeddings = ?, sample_count = ? WHERE id = ?")
            .bind(blob)
            .bind(sample_count as i64)
            .bind(id)
            .execute(pool)
            .await?;
        Ok(())
    }

    /// Rename an enrolled identity.
    pub async fn rename_speaker_identity(
        pool: &SqlitePool,
        id: &str,
        name: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE speakers SET name = ? WHERE id = ?")
            .bind(name)
            .bind(id)
            .execute(pool)
            .await?;
        Ok(())
    }

    /// Delete an enrolled identity (its biometric embedding). Historical
    /// `meeting_speakers.speaker_id` references are NULLed by the FK
    /// (ON DELETE SET NULL); `transcripts.speaker` text stays untouched.
    pub async fn delete_speaker_identity(
        pool: &SqlitePool,
        id: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM speakers WHERE id = ?")
            .bind(id)
            .execute(pool)
            .await?;
        Ok(())
    }

    /// Repoint every meeting cluster row from one identity to another (merge).
    pub async fn reassign_meeting_speaker_refs(
        pool: &SqlitePool,
        from_id: &str,
        to_id: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE meeting_speakers SET speaker_id = ? WHERE speaker_id = ?")
            .bind(to_id)
            .bind(from_id)
            .execute(pool)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_embedding_blob_roundtrip() {
        let emb = vec![0.5f32, -0.25, 1.0, 0.0, -1.5];
        let blob = embedding_to_blob(&emb);
        assert_eq!(blob.len(), emb.len() * 4);
        let back = blob_to_embedding(&blob);
        assert_eq!(emb, back);
    }

    #[test]
    fn test_blob_to_embedding_ignores_trailing_bytes() {
        // Malformed blob with a trailing partial float is ignored (chunks_exact).
        let mut blob = embedding_to_blob(&[1.0f32, 2.0]);
        blob.push(0xAB);
        let back = blob_to_embedding(&blob);
        assert_eq!(back, vec![1.0f32, 2.0]);
    }
}
