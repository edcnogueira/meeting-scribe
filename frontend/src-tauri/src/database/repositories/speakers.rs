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

    /// Create an enrolled speaker identity (used by D4; provided here alongside
    /// the table). Returns the generated id.
    pub async fn create_speaker(
        pool: &SqlitePool,
        name: &str,
        embedding: Option<&[f32]>,
    ) -> Result<String, sqlx::Error> {
        let id = format!("spk-{}", Uuid::new_v4());
        let now = Utc::now().to_rfc3339();
        let blob = embedding.map(embedding_to_blob);

        sqlx::query("INSERT INTO speakers (id, name, embeddings, created_at) VALUES (?, ?, ?, ?)")
            .bind(&id)
            .bind(name)
            .bind(blob)
            .bind(&now)
            .execute(pool)
            .await?;

        Ok(id)
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
