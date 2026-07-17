-- Migration: Speaker identity enrollment fields (task D4)
--
-- Extends the `speakers` (enrolled identity) table so cross-session matching and
-- enrollment-by-use can maintain a running reference embedding per person:
--   sample_count — how many cluster embeddings have been folded into the mean
--                  (capped during enrollment; see audio::diarization_identity).
--   is_self      — marks the machine owner's profile ("Eu"), fed automatically
--                  from the microphone track. At most one row should carry this.
--
-- Voice embeddings never leave the local SQLite database.

ALTER TABLE speakers ADD COLUMN sample_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE speakers ADD COLUMN is_self INTEGER NOT NULL DEFAULT 0;

CREATE INDEX IF NOT EXISTS idx_speakers_is_self ON speakers (is_self);
