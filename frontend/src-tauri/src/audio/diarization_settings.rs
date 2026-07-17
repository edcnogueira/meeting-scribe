//! Runtime toggles for diarization features (task D5).
//!
//! Backs three user-facing settings that the frontend persists (tauri-plugin
//! store / localStorage) and pushes into the backend via
//! [`set_diarization_settings`]:
//!   - `enabled`              — master switch for diarization.
//!   - `auto_diarize`         — run diarization automatically when a meeting is
//!     saved.
//!   - `save_separate_tracks` — persist `mic.mp4` + `system.mp4` (D2) for higher
//!     diarization quality.
//!
//! Each value is a tri-state atomic: `UNSET` means "fall back to the environment
//! variable / compiled default" (so behavior is unchanged for D2/D3 users who
//! never touch the UI), otherwise the explicit boolean the UI last pushed. This
//! keeps the change additive: nothing in the recording / diarization hot paths
//! needs a DB pool or an `AppHandle`, they just read a process-global atomic.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicI8, Ordering};

/// Tri-state sentinel: the UI has not set an explicit value yet.
const UNSET: i8 = -1;

static ENABLED: AtomicI8 = AtomicI8::new(UNSET);
static AUTO_DIARIZE: AtomicI8 = AtomicI8::new(UNSET);
static SAVE_SEPARATE_TRACKS: AtomicI8 = AtomicI8::new(UNSET);

/// Read a tri-state cell, deferring to `fallback` while it is `UNSET`.
fn resolve(cell: &AtomicI8, fallback: impl FnOnce() -> bool) -> bool {
    match cell.load(Ordering::Relaxed) {
        0 => false,
        1 => true,
        _ => fallback(),
    }
}

/// Write an explicit boolean into a tri-state cell.
fn store(cell: &AtomicI8, value: bool) {
    cell.store(i8::from(value), Ordering::Relaxed);
}

/// Master diarization switch. Defaults to enabled until the UI says otherwise.
pub fn enabled() -> bool {
    resolve(&ENABLED, || true)
}

/// Whether to auto-diarize on meeting save. Falls back to the
/// `MEETILY_AUTO_DIARIZE` environment default (task D3).
pub fn auto_diarize() -> bool {
    resolve(&AUTO_DIARIZE, crate::audio::diarization::auto_diarize_enabled)
}

/// Whether to persist separate mic/system tracks. Falls back to the
/// `MEETILY_SAVE_SEPARATE_TRACKS` environment default (task D2).
pub fn save_separate_tracks() -> bool {
    resolve(
        &SAVE_SEPARATE_TRACKS,
        crate::audio::track_saver::env_save_separate_tracks,
    )
}

/// Serializable view of the three toggles (as currently resolved).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiarizationSettings {
    pub enabled: bool,
    pub auto_diarize: bool,
    pub save_separate_tracks: bool,
}

fn snapshot() -> DiarizationSettings {
    DiarizationSettings {
        enabled: enabled(),
        auto_diarize: auto_diarize(),
        save_separate_tracks: save_separate_tracks(),
    }
}

/// Return the effective diarization toggles (UI override or env/default).
#[tauri::command]
pub async fn get_diarization_settings() -> Result<DiarizationSettings, String> {
    Ok(snapshot())
}

/// Push the UI's toggle values into the backend runtime. Additive: recording
/// (`save_separate_tracks`) and auto-diarize honor these immediately without a
/// restart. Persistence across restarts lives in the frontend store; the
/// frontend re-pushes on startup.
#[tauri::command]
pub async fn set_diarization_settings(
    enabled: bool,
    auto_diarize: bool,
    save_separate_tracks: bool,
) -> Result<DiarizationSettings, String> {
    store(&ENABLED, enabled);
    store(&AUTO_DIARIZE, auto_diarize);
    store(&SAVE_SEPARATE_TRACKS, save_separate_tracks);
    Ok(snapshot())
}

#[cfg(test)]
mod tests {
    // Exercises the tri-state logic on a local cell to avoid touching the
    // process-global statics (which other modules' tests also read).
    use super::{resolve, store, UNSET};
    use std::sync::atomic::AtomicI8;

    #[test]
    fn unset_defers_to_fallback() {
        let cell = AtomicI8::new(UNSET);
        assert!(resolve(&cell, || true));
        assert!(!resolve(&cell, || false));
    }

    #[test]
    fn explicit_value_wins_over_fallback() {
        let cell = AtomicI8::new(UNSET);
        store(&cell, false);
        assert!(!resolve(&cell, || true));
        store(&cell, true);
        assert!(resolve(&cell, || false));
    }
}
