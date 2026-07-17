//! Separate per-device track saver (D2).
//!
//! In addition to the existing mixed mono `audio.mp4` (produced by
//! [`IncrementalAudioSaver`](super::incremental_saver::IncrementalAudioSaver),
//! which is left completely untouched), this module persists two extra
//! tracks per meeting:
//!
//! * `mic.mp4`    — microphone only (deterministic single speaker: "me")
//! * `system.mp4` — remote/system audio (the only track that needs diarization)
//!
//! The windows are captured **before** the professional mix in
//! [`AudioPipeline`](super::pipeline::AudioPipeline): each 600 ms window is
//! extracted per device by `AudioMixerRingBuffer::extract_window()` and routed
//! here, so the origin (mic vs system) is preserved. Both windows share the
//! exact same fixed length (zero-padded by `extract_window`), so `mic.mp4` and
//! `system.mp4` stay sample-aligned with each other and with `audio.mp4`.
//!
//! Each track mirrors the incremental-checkpoint strategy of
//! `IncrementalAudioSaver` (30 s checkpoints + FFmpeg concat on finalize) using
//! its own checkpoint subdirectory, so an abrupt stop is as recoverable as the
//! mixed track. This is a parametrized copy rather than a refactor of
//! `IncrementalAudioSaver` on purpose: the hot mixed `audio.mp4` path must stay
//! byte-for-byte identical (zero regression).

use std::path::PathBuf;

use anyhow::{anyhow, Result};
use log::{error, info, warn};

use super::encode::encode_single_audio;
use super::ffmpeg::find_ffmpeg_path;
use super::recording_state::DeviceType;

/// Default state of the "save separate tracks" backend toggle.
///
/// Default is ON in this fork. The frontend toggle UI arrives in task D5; until
/// then the backend honors this constant, overridable at runtime via the
/// `MEETILY_SAVE_SEPARATE_TRACKS` environment variable (see
/// [`should_save_separate_tracks`]). D5 will replace the env override with a
/// read from the `settings` table.
pub const SAVE_SEPARATE_TRACKS_DEFAULT: bool = true;

/// Environment variable that overrides [`SAVE_SEPARATE_TRACKS_DEFAULT`].
///
/// Accepted truthy values: `1`, `true`, `yes`, `on`. Falsy: `0`, `false`,
/// `no`, `off`. Any other value falls back to the default.
pub const SAVE_SEPARATE_TRACKS_ENV: &str = "MEETILY_SAVE_SEPARATE_TRACKS";

/// Resolve whether separate mic/system tracks should be saved.
///
/// Order of precedence:
/// 1. `MEETILY_SAVE_SEPARATE_TRACKS` environment variable (if parseable).
/// 2. [`SAVE_SEPARATE_TRACKS_DEFAULT`].
///
/// D5: the runtime UI toggle (persisted by the frontend and pushed via
/// `set_diarization_settings`) takes precedence; when the UI has not set a value
/// this defers to [`env_save_separate_tracks`] so the env override / compiled
/// default still applies (backward compatible with D2).
pub fn should_save_separate_tracks() -> bool {
    crate::audio::diarization_settings::save_separate_tracks()
}

/// Environment-variable / compiled-default fallback for the "save separate
/// tracks" toggle. Used by the runtime settings resolver when the UI has not
/// pushed an explicit value.
///
/// Accepted truthy values: `1`, `true`, `yes`, `on`. Falsy: `0`, `false`,
/// `no`, `off`. Any other value falls back to [`SAVE_SEPARATE_TRACKS_DEFAULT`].
pub(crate) fn env_save_separate_tracks() -> bool {
    match std::env::var(SAVE_SEPARATE_TRACKS_ENV) {
        Ok(raw) => match raw.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => true,
            "0" | "false" | "no" | "off" => false,
            _ => SAVE_SEPARATE_TRACKS_DEFAULT,
        },
        Err(_) => SAVE_SEPARATE_TRACKS_DEFAULT,
    }
}

/// A single incremental track writer (mic or system).
///
/// Buffers f32 samples, flushes a checkpoint every 30 s into its own
/// `.checkpoints_<name>/` subdirectory, and on finalize concatenates the
/// checkpoints into `<name>.mp4` using the FFmpeg concat demuxer (copy codec,
/// no re-encode).
struct TrackWriter {
    /// Short track name, e.g. `"mic"` or `"system"`. Drives file names.
    name: String,
    checkpoint_buffer: Vec<f32>,
    checkpoint_interval_samples: usize,
    checkpoint_count: u32,
    checkpoints_dir: PathBuf,
    meeting_folder: PathBuf,
    sample_rate: u32,
}

impl TrackWriter {
    fn new(name: &str, meeting_folder: PathBuf, sample_rate: u32) -> Result<Self> {
        let checkpoints_dir = meeting_folder.join(format!(".checkpoints_{}", name));

        // Create the per-track checkpoint directory (independent of the mixed
        // track's `.checkpoints/` so the two never interfere).
        std::fs::create_dir_all(&checkpoints_dir).map_err(|e| {
            anyhow!(
                "Failed to create checkpoints directory {}: {}",
                checkpoints_dir.display(),
                e
            )
        })?;

        Ok(Self {
            name: name.to_string(),
            checkpoint_buffer: Vec::new(),
            checkpoint_interval_samples: sample_rate as usize * 30, // 30 seconds
            checkpoint_count: 0,
            checkpoints_dir,
            meeting_folder,
            sample_rate,
        })
    }

    /// Append a window of samples, flushing a checkpoint at the 30 s threshold.
    fn add_samples(&mut self, samples: &[f32]) -> Result<()> {
        self.checkpoint_buffer.extend_from_slice(samples);

        if self.checkpoint_buffer.len() >= self.checkpoint_interval_samples {
            self.save_checkpoint()?;
            self.checkpoint_buffer.clear();
        }

        Ok(())
    }

    /// Encode the current buffer as `<name>_chunk_NNN.mp4`.
    fn save_checkpoint(&mut self) -> Result<()> {
        if self.checkpoint_buffer.is_empty() {
            warn!("[{}] Attempted to save empty checkpoint, skipping", self.name);
            return Ok(());
        }

        let checkpoint_path = self
            .checkpoints_dir
            .join(format!("{}_chunk_{:03}.mp4", self.name, self.checkpoint_count));

        encode_single_audio(
            bytemuck::cast_slice(&self.checkpoint_buffer),
            self.sample_rate,
            1, // mono
            &checkpoint_path,
        )?;

        let duration_seconds = self.checkpoint_buffer.len() as f32 / self.sample_rate as f32;
        self.checkpoint_count += 1;

        info!(
            "[{}] Saved checkpoint {}: {:.2}s ({} samples)",
            self.name,
            self.checkpoint_count,
            duration_seconds,
            self.checkpoint_buffer.len()
        );

        Ok(())
    }

    /// Finalize this track: flush the tail, concat checkpoints into
    /// `<meeting>/<name>.mp4`, then remove the checkpoint directory.
    async fn finalize(&mut self) -> Result<PathBuf> {
        if !self.checkpoint_buffer.is_empty() {
            self.save_checkpoint()?;
            self.checkpoint_buffer.clear();
        }

        if self.checkpoint_count == 0 {
            return Err(anyhow!(
                "[{}] No audio checkpoints to merge - track may have been silent",
                self.name
            ));
        }

        let final_path = self.meeting_folder.join(format!("{}.mp4", self.name));
        self.merge_checkpoints(&final_path).await?;

        // Best-effort cleanup of the per-track checkpoint directory.
        if let Err(e) = std::fs::remove_dir_all(&self.checkpoints_dir) {
            warn!(
                "[{}] Failed to clean up checkpoints directory {}: {}",
                self.name,
                self.checkpoints_dir.display(),
                e
            );
        }

        info!("[{}] Finalized track: {}", self.name, final_path.display());
        Ok(final_path)
    }

    /// Merge checkpoint files into `output` via FFmpeg concat (copy codec).
    async fn merge_checkpoints(&self, output: &PathBuf) -> Result<()> {
        let list_file = self.checkpoints_dir.join("concat_list.txt");
        let mut list_content = String::new();

        for i in 0..self.checkpoint_count {
            let checkpoint_path = self
                .checkpoints_dir
                .join(format!("{}_chunk_{:03}.mp4", self.name, i));

            if !checkpoint_path.exists() {
                return Err(anyhow!(
                    "[{}] Checkpoint file missing: {}",
                    self.name,
                    checkpoint_path.display()
                ));
            }

            let abs_path = checkpoint_path.canonicalize()?;
            list_content.push_str(&format!("file '{}'\n", abs_path.display()));
        }

        std::fs::write(&list_file, list_content)?;

        let ffmpeg_path = find_ffmpeg_path()
            .ok_or_else(|| anyhow!("FFmpeg not found. Please install FFmpeg to finalize tracks."))?;

        let mut command = std::process::Command::new(ffmpeg_path);
        command.args([
            "-f",
            "concat",
            "-safe",
            "0",
            "-i",
            list_file.to_str().unwrap(),
            "-c",
            "copy",
            "-y",
            output.to_str().unwrap(),
        ]);

        // Hide console window on Windows to prevent CMD popup during finalization.
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            command.creation_flags(CREATE_NO_WINDOW);
        }

        let ffmpeg_output = command.output()?;

        if !ffmpeg_output.status.success() {
            let stderr = String::from_utf8_lossy(&ffmpeg_output.stderr);
            error!("[{}] FFmpeg merge failed: {}", self.name, stderr);
            return Err(anyhow!("FFmpeg concat failed for {}: {}", self.name, stderr));
        }

        if !output.exists() {
            return Err(anyhow!(
                "[{}] Merged track file was not created: {}",
                self.name,
                output.display()
            ));
        }

        info!(
            "[{}] Merged {} checkpoints → {}",
            self.name,
            self.checkpoint_count,
            output.display()
        );

        Ok(())
    }
}

/// Result of finalizing the separate tracks. Both are `Option` because a track
/// that never received audio (e.g. no system device) simply has no file.
#[derive(Debug, Clone, Default)]
pub struct SeparateTrackPaths {
    pub mic: Option<PathBuf>,
    pub system: Option<PathBuf>,
}

/// Saves the microphone and system audio as two independent tracks, mirroring
/// the incremental-checkpoint approach of the mixed saver.
pub struct SeparateTrackSaver {
    mic: TrackWriter,
    system: TrackWriter,
}

impl SeparateTrackSaver {
    /// Create a saver rooted at `meeting_folder` (same folder as `audio.mp4`).
    pub fn new(meeting_folder: PathBuf, sample_rate: u32) -> Result<Self> {
        Ok(Self {
            mic: TrackWriter::new("mic", meeting_folder.clone(), sample_rate)?,
            system: TrackWriter::new("system", meeting_folder, sample_rate)?,
        })
    }

    /// Route a pre-mix window to the correct track.
    pub fn add_window(&mut self, device_type: DeviceType, samples: &[f32]) -> Result<()> {
        match device_type {
            DeviceType::Microphone => self.mic.add_samples(samples),
            DeviceType::System => self.system.add_samples(samples),
        }
    }

    /// Finalize both tracks. A track that never received samples is reported as
    /// `None` (non-fatal) rather than failing the whole save.
    pub async fn finalize(&mut self) -> Result<SeparateTrackPaths> {
        let mic = match self.mic.finalize().await {
            Ok(path) => Some(path),
            Err(e) => {
                warn!("Skipping mic.mp4 (no data or merge failed): {}", e);
                None
            }
        };

        let system = match self.system.finalize().await {
            Ok(path) => Some(path),
            Err(e) => {
                warn!("Skipping system.mp4 (no data or merge failed): {}", e);
                None
            }
        };

        Ok(SeparateTrackPaths { mic, system })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn should_save_defaults_to_on() {
        // No override set for this key in the test environment.
        std::env::remove_var(SAVE_SEPARATE_TRACKS_ENV);
        assert_eq!(should_save_separate_tracks(), SAVE_SEPARATE_TRACKS_DEFAULT);
        assert!(should_save_separate_tracks());
    }

    #[test]
    fn track_writer_uses_track_scoped_paths() {
        let temp_dir = tempdir().unwrap();
        let meeting_folder = temp_dir.path().join("Meeting");
        std::fs::create_dir_all(&meeting_folder).unwrap();

        let writer = TrackWriter::new("system", meeting_folder.clone(), 48000).unwrap();

        // Checkpoint dir is track-scoped and does not collide with the mixed
        // saver's `.checkpoints/`.
        assert_eq!(
            writer.checkpoints_dir,
            meeting_folder.join(".checkpoints_system")
        );
        assert!(writer.checkpoints_dir.exists());
        assert_ne!(writer.checkpoints_dir, meeting_folder.join(".checkpoints"));
        assert_eq!(writer.checkpoint_interval_samples, 48000 * 30);
    }

    #[test]
    fn add_window_routes_by_device_without_flushing_early() {
        let temp_dir = tempdir().unwrap();
        let meeting_folder = temp_dir.path().join("Meeting");
        std::fs::create_dir_all(&meeting_folder).unwrap();

        let mut saver = SeparateTrackSaver::new(meeting_folder, 48000).unwrap();

        // A few sub-threshold windows: routed to the right buffer, no checkpoint
        // flushed yet (well under 30 s), so no FFmpeg/hardware needed.
        let window = vec![0.25f32; 28_800]; // 600 ms at 48kHz
        for _ in 0..5 {
            saver.add_window(DeviceType::Microphone, &window).unwrap();
            saver.add_window(DeviceType::System, &window).unwrap();
        }

        assert_eq!(saver.mic.checkpoint_buffer.len(), 28_800 * 5);
        assert_eq!(saver.system.checkpoint_buffer.len(), 28_800 * 5);
        assert_eq!(saver.mic.checkpoint_count, 0);
        assert_eq!(saver.system.checkpoint_count, 0);
    }

    #[test]
    fn add_window_flushes_checkpoint_names_per_track() {
        // Drive enough samples past the 30 s threshold and assert the produced
        // checkpoint files are track-scoped. Uses a tiny sample_rate so the
        // threshold is reached with little data; encode still runs FFmpeg, so
        // guard on FFmpeg availability to keep the test hardware-free in CI.
        if find_ffmpeg_path().is_none() {
            eprintln!("FFmpeg not available - skipping checkpoint-name assertion");
            return;
        }

        let temp_dir = tempdir().unwrap();
        let meeting_folder = temp_dir.path().join("Meeting");
        std::fs::create_dir_all(&meeting_folder).unwrap();

        // sample_rate = 8000 (standard, always accepted by FFmpeg) →
        // threshold = 240_000 samples. One over-threshold push flushes one checkpoint.
        let mut writer = TrackWriter::new("mic", meeting_folder, 8000).unwrap();
        writer.add_samples(&vec![0.1f32; 8000 * 30]).unwrap();

        assert_eq!(writer.checkpoint_count, 1);
        assert!(writer.checkpoints_dir.join("mic_chunk_000.mp4").exists());
    }
}
