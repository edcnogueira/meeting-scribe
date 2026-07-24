# Requirements Document

## Introduction

Speaker diarization in Meeting Scribe (pyannote segmentation-3.0 + WeSpeaker ResNet34 embeddings + agglomerative clustering, ported from the D1 spike) misattributes speakers often enough that users notice. The current engine processes audio in non-overlapping 10-second windows with no stitching, drops sub-400 ms fragments per window, and uses thresholds calibrated only on synthetic audio — all documented as accuracy limitations in `tasks/diarization/D1-resultados.md`. This feature raises diarization accuracy measurably and adds an evaluation harness so that accuracy changes are quantified instead of guessed, and future regressions are caught automatically.

## Glossary

- **Diarization_Engine**: The Rust component in `frontend/src-tauri/src/diarization_engine/` that converts a mono 16 kHz audio buffer into Speaker_Turns (segmentation → embedding → clustering).
- **Segmentation_Stage**: The part of the Diarization_Engine that runs the pyannote segmentation model over analysis windows and produces Speech_Regions.
- **Embedding_Stage**: The part of the Diarization_Engine that computes a 256-dimensional voice embedding for each Speech_Region.
- **Clustering_Stage**: The part of the Diarization_Engine that groups Speech_Region embeddings into speaker clusters (agglomerative, cosine distance).
- **Attribution_Stage**: The post-processing step in `frontend/src-tauri/src/audio/diarization.rs` (`assign_speakers`) that assigns a speaker label to each transcript segment by temporal overlap with Speaker_Turns.
- **Identity_Matcher**: The component that matches cluster embeddings against the stored voice-profile registry to resolve persistent speaker names.
- **Evaluation_Harness**: A new automated test facility that diarizes Ground_Truth_Fixtures and reports Frame_Accuracy, detected speaker count, and RTF per fixture.
- **Ground_Truth_Fixture**: A generated multi-speaker audio file whose speaker timeline is known exactly by construction (speech placement is scripted, not manually annotated).
- **Degraded_Fixture**: A Ground_Truth_Fixture whose audio is additionally mixed with background noise at a scripted signal-to-noise ratio of 10 dB and convolved with a synthetic room impulse response, using a fixed random seed.
- **Speech_Region**: A contiguous span of audio attributed to one local speaker by the Segmentation_Stage, carrying one embedding.
- **Speaker_Turn**: A speaker-labeled time span (start/end seconds) in the diarization result.
- **Frame_Accuracy**: Percentage of 100 ms frames whose predicted speaker matches the ground truth, under the best cluster-to-speaker mapping (the metric used in the D1 spike).
- **Exclusive_Audio**: Samples within a Speech_Region where exactly one speaker is active, used for clean embeddings.
- **Speaker_Count_Hint**: The optional user-provided number of remote speakers that fixes the cluster count.
- **Self_Profile**: The stored identity representing the local (microphone) speaker.
- **RTF**: Real-time factor — processing time divided by audio duration.
- **Baseline**: The Frame_Accuracy values recorded by the Evaluation_Harness on the current (pre-change) engine for each Ground_Truth_Fixture.

## Requirements

### Requirement 1: Automated accuracy evaluation

**User Story:** As the app maintainer, I want an automated evaluation harness with ground-truth fixtures, so that any accuracy change is measured objectively and regressions are caught by tests.

#### Acceptance Criteria

1. THE Evaluation_Harness SHALL compute Frame_Accuracy of a diarization result against a Ground_Truth_Fixture using 100 ms frames and the best cluster-to-speaker mapping.
2. THE Evaluation_Harness SHALL include Ground_Truth_Fixtures covering at least: sequential speech with 3 speakers, overlapping speech with 3 speakers, and isolated remote-track audio with 2 speakers.
3. THE Evaluation_Harness SHALL include Degraded_Fixture variants of the sequential-speech and overlapping-speech fixtures.
4. WHEN the Evaluation_Harness runs, THE Evaluation_Harness SHALL report Frame_Accuracy, detected speaker count, and RTF for each Ground_Truth_Fixture.
5. THE Evaluation_Harness SHALL derive ground-truth labels from scripted fixture construction rather than manual annotation.
6. THE Evaluation_Harness SHALL run through the project's Rust test suite.
7. IF the diarization ONNX models are absent from the local models directory, THEN THE Evaluation_Harness SHALL skip with an explicit skip message instead of failing.
8. IF the host platform cannot synthesize the fixture audio, THEN THE Evaluation_Harness SHALL skip with an explicit skip message instead of failing.
9. WHEN the Evaluation_Harness generates a Ground_Truth_Fixture, THE Evaluation_Harness SHALL produce the same speaker timeline for the same generation script version.
10. WHEN the Evaluation_Harness first runs on the unmodified engine, THE Evaluation_Harness SHALL record the resulting Frame_Accuracy values as the Baseline in a versioned file under the test data directory.
11. THE Evaluation_Harness SHALL place fixture speaker transitions at times not aligned with the segmentation analysis grid (at least 400 ms away from any multiple of 5 seconds).

### Requirement 2: Measurable accuracy improvement

**User Story:** As a user, I want speaker labels to be correct more often — especially when people talk over each other — so that the transcript reflects who actually said what.

#### Acceptance Criteria

1. WHEN diarizing the overlapping-speech fixture, THE Diarization_Engine SHALL achieve Frame_Accuracy greater than or equal to 97.0%.
2. WHEN diarizing the sequential-speech fixture, THE Diarization_Engine SHALL achieve Frame_Accuracy greater than or equal to 99.0%.
3. WHEN diarizing the isolated remote-track fixture, THE Diarization_Engine SHALL achieve Frame_Accuracy greater than or equal to 99.5%.
4. WHEN diarizing the degraded sequential-speech fixture, THE Diarization_Engine SHALL achieve Frame_Accuracy greater than or equal to 95.0%.
5. WHEN diarizing the degraded overlapping-speech fixture, THE Diarization_Engine SHALL achieve Frame_Accuracy greater than or equal to 90.0%.
6. WHEN any engine change from this feature is applied, THE Diarization_Engine SHALL keep Frame_Accuracy on every Ground_Truth_Fixture greater than or equal to the Baseline value for that fixture.

### Requirement 3: Continuity across analysis-window boundaries

**User Story:** As a user, I want a person speaking continuously to receive one uninterrupted speaker turn, so that turns are not fragmented or lost at arbitrary points of the recording.

#### Acceptance Criteria

1. WHEN a single speaker's speech run spans the boundary between two adjacent analysis windows, THE Segmentation_Stage SHALL emit one Speech_Region covering the full run.
2. IF a speech run's total duration is greater than or equal to 400 ms but every within-window fragment of the run is shorter than 400 ms, THEN THE Segmentation_Stage SHALL retain the run as a Speech_Region.
3. WHEN identical audio content is prepended with up to 5 seconds of silence, THE Diarization_Engine SHALL produce Speaker_Turns whose start and end times shift by the prepended duration within a tolerance of 250 ms and whose speaker grouping is unchanged.

### Requirement 4: Speaker-count robustness

**User Story:** As a user, I want the engine to find the actual number of speakers — no phantom extra speakers and no merged distinct speakers — so that renaming and identification stay meaningful.

#### Acceptance Criteria

1. WHEN diarizing a Ground_Truth_Fixture without a Speaker_Count_Hint, THE Clustering_Stage SHALL detect a speaker count equal to the fixture's true speaker count.
2. WHERE a Speaker_Count_Hint of k is provided, THE Clustering_Stage SHALL output exactly k clusters.
3. IF the Embedding_Stage cannot compute an embedding for a Speech_Region, THEN THE Diarization_Engine SHALL exclude that Speech_Region from clustering and continue processing the remaining regions.

### Requirement 5: Attribution behavior preserved

**User Story:** As a user, I want transcript lines to keep being labeled by the speaker who overlaps them most, so that accuracy work in the engine does not change how labels reach the transcript.

#### Acceptance Criteria

1. WHEN a transcript segment temporally overlaps Speaker_Turns of more than one speaker, THE Attribution_Stage SHALL assign the speaker label with the greatest temporal overlap.
2. IF a transcript segment overlaps no Speaker_Turn, THEN THE Attribution_Stage SHALL leave the segment's speaker label unassigned.
3. WHEN diarization completes on a separate-track recording, THE Attribution_Stage SHALL label microphone-track turns with the Self_Profile.

### Requirement 6: Identity matching preserved

**User Story:** As a user, I want people I have already named to keep being recognized across meetings, so that accuracy tuning does not break the voice-profile registry.

#### Acceptance Criteria

1. WHEN a cluster embedding matches a stored identity with cosine similarity greater than or equal to the identification threshold (0.65), THE Identity_Matcher SHALL label the cluster with the stored identity's name.
2. WHEN no stored identity reaches the identification threshold, THE Identity_Matcher SHALL label the cluster with a positional "Speaker N" name.
3. WHEN engine changes alter the embedding computation, THE Evaluation_Harness SHALL verify that two fixtures of the same synthetic voice recorded in separate runs match with cosine similarity greater than or equal to the identification threshold.

### Requirement 7: Performance bound

**User Story:** As a user, I want diarization to stay fast, so that accuracy improvements do not make post-meeting processing noticeably slower.

#### Acceptance Criteria

1. WHEN diarizing any Ground_Truth_Fixture, THE Diarization_Engine SHALL keep RTF less than or equal to 0.15 as measured by the Evaluation_Harness.
2. WHEN the Evaluation_Harness reports RTF, THE Evaluation_Harness SHALL measure only the diarization computation (decode and database work excluded).
