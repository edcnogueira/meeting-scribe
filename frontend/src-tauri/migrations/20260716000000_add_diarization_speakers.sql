-- Migration: Diarization speaker persistence (task D3)
--
-- Two tables:
--   speakers          — enrolled speaker identities (name + reference embedding).
--                       Populated/matched by D4; created here so clusters can
--                       optionally reference an identity.
--   meeting_speakers  — one row per clustered speaker per meeting, storing the
--                       per-meeting cluster label, its mean embedding, and an
--                       optional resolved identity + match score.

CREATE TABLE IF NOT EXISTS speakers (
    id           TEXT PRIMARY KEY,
    name         TEXT NOT NULL,
    embeddings   BLOB,            -- reference embedding(s) for identification (D4)
    created_at   TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS meeting_speakers (
    id             TEXT PRIMARY KEY,
    meeting_id     TEXT NOT NULL,
    cluster_label  TEXT NOT NULL,        -- e.g. "Eu", "Speaker 1"
    speaker_id     TEXT,                 -- resolved identity (D4), NULL until matched
    score          REAL,                 -- identification similarity (D4)
    embedding      BLOB,                 -- per-meeting cluster mean embedding
    created_at     TEXT NOT NULL,
    FOREIGN KEY (meeting_id) REFERENCES meetings(id) ON DELETE CASCADE,
    FOREIGN KEY (speaker_id) REFERENCES speakers(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_meeting_speakers_meeting_id
    ON meeting_speakers (meeting_id);
