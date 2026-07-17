//! Tauri command surface for the diarization engine (init/status/download).
//! Mirrors `parakeet_engine::commands`.

use crate::diarization_engine::engine::{
    DiarizationDownloadProgress, DiarizationEngine, DiarizationModelInfo,
};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use tauri::{command, AppHandle, Emitter, Manager, Runtime};

/// Global diarization engine instance.
pub static DIARIZATION_ENGINE: Mutex<Option<Arc<DiarizationEngine>>> = Mutex::new(None);

/// Models directory (shared root with the other engines; the engine appends
/// its own `diarization` subdirectory).
static MODELS_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Configure the models directory from `app_data_dir`. Call during app setup
/// before `diarization_init`.
pub fn set_models_directory<R: Runtime>(app: &AppHandle<R>) {
    let app_data_dir = match app.path().app_data_dir() {
        Ok(dir) => dir,
        Err(e) => {
            log::error!("Failed to get app data dir for diarization: {}", e);
            return;
        }
    };

    let models_dir = app_data_dir.join("models");
    if !models_dir.exists() {
        if let Err(e) = std::fs::create_dir_all(&models_dir) {
            log::error!("Failed to create models directory: {}", e);
            return;
        }
    }

    log::info!("Diarization models directory set to: {}", models_dir.display());
    *MODELS_DIR.lock().unwrap() = Some(models_dir);
}

fn get_models_directory() -> Option<PathBuf> {
    MODELS_DIR.lock().unwrap().clone()
}

/// Fetch the shared engine handle if initialized.
pub fn get_engine() -> Option<Arc<DiarizationEngine>> {
    DIARIZATION_ENGINE.lock().unwrap().as_ref().cloned()
}

#[command]
pub async fn diarization_init() -> Result<(), String> {
    let mut guard = DIARIZATION_ENGINE.lock().unwrap();
    if guard.is_some() {
        return Ok(());
    }
    let models_dir = get_models_directory();
    let engine = DiarizationEngine::new_with_models_dir(models_dir)
        .map_err(|e| format!("Failed to initialize diarization engine: {}", e))?;
    *guard = Some(Arc::new(engine));
    Ok(())
}

#[command]
pub async fn diarization_get_model_status() -> Result<DiarizationModelInfo, String> {
    let engine = get_engine().ok_or_else(|| "Diarization engine not initialized".to_string())?;
    Ok(engine.discover_model().await)
}

#[command]
pub async fn diarization_is_model_available() -> Result<bool, String> {
    match get_engine() {
        Some(engine) => Ok(engine.is_available().await),
        None => Ok(false),
    }
}

#[command]
pub async fn diarization_is_model_loaded() -> Result<bool, String> {
    let engine = get_engine().ok_or_else(|| "Diarization engine not initialized".to_string())?;
    Ok(engine.is_model_loaded().await)
}

#[command]
pub async fn diarization_get_models_directory() -> Result<String, String> {
    let engine = get_engine().ok_or_else(|| "Diarization engine not initialized".to_string())?;
    Ok(engine.get_models_directory().await.to_string_lossy().to_string())
}

#[command]
pub async fn diarization_download_model<R: Runtime>(app_handle: AppHandle<R>) -> Result<(), String> {
    let engine = get_engine().ok_or_else(|| "Diarization engine not initialized".to_string())?;

    let app_for_progress = app_handle.clone();
    let progress_callback = Box::new(move |progress: DiarizationDownloadProgress| {
        if let Err(e) = app_for_progress.emit(
            "diarization-model-download-progress",
            serde_json::json!({
                "modelName": crate::diarization_engine::engine::DIARIZATION_MODEL_NAME,
                "progress": progress.percent,
                "downloaded_bytes": progress.downloaded_bytes,
                "total_bytes": progress.total_bytes,
                "downloaded_mb": progress.downloaded_mb,
                "total_mb": progress.total_mb,
                "speed_mbps": progress.speed_mbps,
                "status": if progress.percent == 100 { "completed" } else { "downloading" }
            }),
        ) {
            log::error!("Failed to emit diarization download progress: {}", e);
        }
    });

    let result = engine.download_model(Some(progress_callback)).await;

    match result {
        Ok(()) => {
            let _ = app_handle.emit(
                "diarization-model-download-complete",
                serde_json::json!({ "modelName": crate::diarization_engine::engine::DIARIZATION_MODEL_NAME }),
            );
            Ok(())
        }
        Err(e) => {
            let _ = app_handle.emit(
                "diarization-model-download-error",
                serde_json::json!({ "error": e.to_string() }),
            );
            Err(format!("Failed to download diarization model: {}", e))
        }
    }
}

#[command]
pub async fn diarization_cancel_download() -> Result<(), String> {
    let engine = get_engine().ok_or_else(|| "Diarization engine not initialized".to_string())?;
    engine.cancel_download().await;
    Ok(())
}
