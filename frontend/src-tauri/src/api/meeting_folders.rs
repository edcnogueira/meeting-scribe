// Meeting folder organization (O1).
//
// The filesystem is the source of truth for the organization tree: every folder
// shown in the app is a real directory under the recordings base dir, and every
// directory under that base is either an *organization* folder or a *meeting*
// folder. The link between a meeting row and its location on disk is the
// existing `meetings.folder_path` column — there is no separate table.
//
// A directory is treated as a meeting when it is referenced by some
// `folder_path` in the database, or when it contains recording artifacts
// (audio.*/transcript*.json). Everything else is an organization folder.
// Dotfiles (e.g. `.checkpoints`) are ignored while scanning.
//
// Every path coming from the frontend is canonicalized and validated to live
// inside the recordings base dir before any filesystem operation runs — the
// backend never trusts an arbitrary path from JS.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Runtime};

use crate::audio::audio_processing::sanitize_filename;
use crate::audio::recording_preferences::{ensure_recordings_directory, load_recording_preferences};
use crate::database::repositories::meeting::MeetingsRepository;
use crate::state::AppState;

/// A meeting leaf in the folder tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeetingNode {
    /// Database meeting id, when the directory maps to a known meeting.
    pub id: Option<String>,
    pub title: String,
    /// Absolute (canonical) path of the meeting directory, when it exists.
    pub path: Option<String>,
    /// True when the database references a directory that is no longer on disk.
    pub missing: bool,
}

/// An organization folder in the tree, plus its nested content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FolderNode {
    pub name: String,
    pub path: String,
    pub folders: Vec<FolderNode>,
    pub meetings: Vec<MeetingNode>,
}

/// The full tree returned to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeetingFolderTree {
    pub base_path: String,
    /// Top-level organization folders.
    pub folders: Vec<FolderNode>,
    /// Legacy/root/missing meetings that don't live under an organization folder.
    pub unfiled: Vec<MeetingNode>,
}

/// Minimal meeting row used by the pure tree/prefix logic (kept independent of
/// the database model so it can be unit-tested without a pool).
#[derive(Debug, Clone)]
pub struct MeetingRow {
    pub id: String,
    pub title: String,
    pub folder_path: Option<String>,
}

// ----------------------------------------------------------------------------
// Pure helpers (unit-tested with temp dirs — no AppHandle / pool required).
// ----------------------------------------------------------------------------

/// Canonicalize `path` and ensure it resolves to a location inside `base`
/// (which must already be canonical). The path must exist.
pub fn validate_within_base(base: &Path, path: &Path) -> Result<PathBuf, String> {
    let canonical = path.canonicalize().map_err(|e| {
        format!(
            "Path does not exist or is inaccessible: {} ({e})",
            path.display()
        )
    })?;
    if !canonical.starts_with(base) {
        return Err(format!(
            "Path is outside the recordings directory: {}",
            path.display()
        ));
    }
    Ok(canonical)
}

/// Create an organization folder under `parent` (or the base root when `None`).
/// Nesting is allowed; the name is sanitized; a collision is an error.
pub fn create_folder(base: &Path, parent: Option<&Path>, name: &str) -> Result<PathBuf, String> {
    let sanitized = sanitize_filename(name);
    if sanitized.is_empty() {
        return Err("Folder name cannot be empty".to_string());
    }

    let parent_dir = match parent {
        Some(p) => validate_within_base(base, p)?,
        None => base.to_path_buf(),
    };

    let target = parent_dir.join(&sanitized);
    if target.exists() {
        return Err(format!(
            "A folder or meeting named '{sanitized}' already exists here"
            ));
    }

    std::fs::create_dir_all(&target).map_err(|e| format!("Failed to create folder: {e}"))?;
    Ok(target)
}

/// Rename an organization folder in place. Returns `(old_canonical, new_path)`.
/// The filesystem rename only — callers update the database prefix afterwards.
pub fn rename_folder(base: &Path, path: &Path, new_name: &str) -> Result<(PathBuf, PathBuf), String> {
    let sanitized = sanitize_filename(new_name);
    if sanitized.is_empty() {
        return Err("Folder name cannot be empty".to_string());
    }

    let src = validate_within_base(base, path)?;
    if src == base {
        return Err("Cannot rename the recordings root".to_string());
    }

    let parent = src
        .parent()
        .ok_or_else(|| "Folder has no parent".to_string())?;
    let dest = parent.join(&sanitized);

    if dest == src {
        // Renaming to the same name is a no-op.
        return Ok((src.clone(), dest));
    }
    if dest.exists() {
        return Err(format!(
            "A folder or meeting named '{sanitized}' already exists here"
            ));
    }

    std::fs::rename(&src, &dest).map_err(|e| format!("Failed to rename folder: {e}"))?;
    Ok((src, dest))
}

/// Delete an organization folder, refusing anything that is not empty. Dotfiles
/// (e.g. `.DS_Store`) don't count as content.
pub fn delete_empty_folder(base: &Path, path: &Path) -> Result<(), String> {
    let dir = validate_within_base(base, path)?;
    if dir == base {
        return Err("Cannot delete the recordings root".to_string());
    }

    for entry in std::fs::read_dir(&dir).map_err(|e| format!("Failed to read folder: {e}"))? {
        let entry = entry.map_err(|e| format!("Failed to read folder entry: {e}"))?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') {
            continue;
        }
        return Err("Folder is not empty. Only empty folders can be deleted.".to_string());
    }

    std::fs::remove_dir_all(&dir).map_err(|e| format!("Failed to delete folder: {e}"))?;
    Ok(())
}

/// Move a meeting directory into `target_parent` (or the base root when `None`).
/// Returns the new directory path. Filesystem move only.
pub fn move_meeting_dir(
    base: &Path,
    source: &Path,
    target_parent: Option<&Path>,
) -> Result<PathBuf, String> {
    let src = validate_within_base(base, source)?;
    let parent_dir = match target_parent {
        Some(p) => validate_within_base(base, p)?,
        None => base.to_path_buf(),
    };

    let name = src
        .file_name()
        .ok_or_else(|| "Source has no name".to_string())?;
    let dest = parent_dir.join(name);

    if dest == src {
        return Ok(dest);
    }
    if dest.exists() {
        return Err("A folder or meeting with the same name already exists in the target".to_string());
    }

    std::fs::rename(&src, &dest).map_err(|e| format!("Failed to move meeting: {e}"))?;
    Ok(dest)
}

/// Rewrite a stored `folder_path` when its `old_prefix` directory was renamed
/// or moved to `new_prefix`. Returns `None` when the path is unaffected.
pub fn rewrite_folder_path_prefix(
    old_prefix: &str,
    new_prefix: &str,
    folder_path: &str,
) -> Option<String> {
    if folder_path == old_prefix {
        return Some(new_prefix.to_string());
    }
    let sep = std::path::MAIN_SEPARATOR;
    let with_sep = format!("{old_prefix}{sep}");
    folder_path
        .strip_prefix(&with_sep)
        .map(|rest| format!("{new_prefix}{sep}{rest}"))
}

/// Decide whether a (canonical) directory is a meeting directory.
fn is_meeting_dir(canon: &Path, known: &HashMap<PathBuf, &MeetingRow>) -> bool {
    if known.contains_key(canon) {
        return true;
    }

    // Fall back to artifact detection so directories created/moved outside the
    // app (Finder) are still recognized as meetings.
    if let Ok(entries) = std::fs::read_dir(canon) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with("transcript") && name.ends_with(".json") {
                return true;
            }
            if (name.starts_with("audio") || name.starts_with("recording"))
                && (name.ends_with(".mp4")
                    || name.ends_with(".m4a")
                    || name.ends_with(".wav")
                    || name.ends_with(".mp3"))
            {
                return true;
            }
        }
    }
    false
}

/// Recursively scan `dir`, returning its organization sub-folders and the
/// meetings that live directly inside it.
fn scan_dir(
    dir: &Path,
    known: &HashMap<PathBuf, &MeetingRow>,
    seen: &mut HashSet<String>,
) -> (Vec<FolderNode>, Vec<MeetingNode>) {
    let mut folders = Vec::new();
    let mut meetings = Vec::new();

    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return (folders, meetings),
    };

    let mut subdirs: Vec<PathBuf> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') {
            // Ignore dotfolders like `.checkpoints`.
            continue;
        }
        subdirs.push(path);
    }
    subdirs.sort();

    for path in subdirs {
        let canon = path.canonicalize().unwrap_or_else(|_| path.clone());
        if is_meeting_dir(&canon, known) {
            let node = if let Some(m) = known.get(&canon) {
                seen.insert(m.id.clone());
                MeetingNode {
                    id: Some(m.id.clone()),
                    title: m.title.clone(),
                    path: Some(canon.to_string_lossy().to_string()),
                    missing: false,
                }
            } else {
                let title = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                MeetingNode {
                    id: None,
                    title,
                    path: Some(canon.to_string_lossy().to_string()),
                    missing: false,
                }
            };
            meetings.push(node);
        } else {
            let (sub_folders, sub_meetings) = scan_dir(&path, known, seen);
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            folders.push(FolderNode {
                name,
                path: canon.to_string_lossy().to_string(),
                folders: sub_folders,
                meetings: sub_meetings,
            });
        }
    }

    (folders, meetings)
}

/// Build the folder tree from the on-disk layout under `base` plus the known
/// meeting rows. Meetings sitting at the base root, with a NULL `folder_path`,
/// or whose directory has vanished are collected under "Unfiled".
pub fn build_tree(base: &Path, meetings: &[MeetingRow]) -> MeetingFolderTree {
    let mut known: HashMap<PathBuf, &MeetingRow> = HashMap::new();
    for m in meetings {
        if let Some(fp) = &m.folder_path {
            if let Ok(canon) = Path::new(fp).canonicalize() {
                known.insert(canon, m);
            }
        }
    }

    let mut seen: HashSet<String> = HashSet::new();
    let (folders, root_meetings) = scan_dir(base, &known, &mut seen);

    // Meetings living directly at the base root are "unfiled".
    let mut unfiled: Vec<MeetingNode> = root_meetings;

    // Add database meetings we never encountered on disk: NULL folder_path
    // (legacy) or a directory that is gone (missing).
    for m in meetings {
        if seen.contains(&m.id) {
            continue;
        }
        match &m.folder_path {
            None => unfiled.push(MeetingNode {
                id: Some(m.id.clone()),
                title: m.title.clone(),
                path: None,
                missing: false,
            }),
            Some(fp) => unfiled.push(MeetingNode {
                id: Some(m.id.clone()),
                title: m.title.clone(),
                path: Some(fp.clone()),
                missing: true,
            }),
        }
    }

    MeetingFolderTree {
        base_path: base.to_string_lossy().to_string(),
        folders,
        unfiled,
    }
}

// ----------------------------------------------------------------------------
// Tauri commands.
// ----------------------------------------------------------------------------

/// Resolve and canonicalize the recordings base directory, creating it if needed.
async fn recordings_base<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    let prefs = load_recording_preferences(app)
        .await
        .map_err(|e| format!("Failed to load recording preferences: {e}"))?;
    ensure_recordings_directory(&prefs.save_folder)
        .map_err(|e| format!("Failed to ensure recordings directory: {e}"))?;
    prefs
        .save_folder
        .canonicalize()
        .map_err(|e| format!("Failed to resolve recordings directory: {e}"))
}

/// Scan the recordings base and return the organization tree mirrored from disk.
#[tauri::command]
pub async fn api_list_meeting_folder_tree<R: Runtime>(
    app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
) -> Result<MeetingFolderTree, String> {
    let base = recordings_base(&app).await?;
    let pool = state.db_manager.pool();
    let models = MeetingsRepository::get_meetings(pool)
        .await
        .map_err(|e| format!("Failed to load meetings: {e}"))?;

    let rows: Vec<MeetingRow> = models
        .into_iter()
        .map(|m| MeetingRow {
            id: m.id,
            title: m.title,
            folder_path: m.folder_path,
        })
        .collect();

    // Disk scanning is blocking I/O — keep it off the async runtime threads.
    let tree = tokio::task::spawn_blocking(move || build_tree(&base, &rows))
        .await
        .map_err(|e| format!("Folder scan task failed: {e}"))?;
    Ok(tree)
}

/// Create a new organization folder (nesting allowed).
#[tauri::command]
pub async fn api_create_meeting_folder<R: Runtime>(
    app: AppHandle<R>,
    _state: tauri::State<'_, AppState>,
    parent_path: Option<String>,
    name: String,
) -> Result<String, String> {
    let base = recordings_base(&app).await?;
    let parent = parent_path.as_ref().map(PathBuf::from);
    let created = create_folder(&base, parent.as_deref(), &name)?;
    Ok(created.to_string_lossy().to_string())
}

/// Rename an organization folder and re-prefix `folder_path` for every meeting
/// under it. Filesystem first, database second; the rename is rolled back
/// (best-effort) if the database update fails.
#[tauri::command]
pub async fn api_rename_meeting_folder<R: Runtime>(
    app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    path: String,
    new_name: String,
) -> Result<String, String> {
    let base = recordings_base(&app).await?;

    let src = validate_within_base(&base, Path::new(&path))?;
    if src == base {
        return Err("Cannot rename the recordings root".to_string());
    }
    let parent = src
        .parent()
        .ok_or_else(|| "Folder has no parent".to_string())?
        .to_path_buf();
    let sanitized = sanitize_filename(&new_name);
    if sanitized.is_empty() {
        return Err("Folder name cannot be empty".to_string());
    }
    let dest = parent.join(&sanitized);
    if dest != src && dest.exists() {
        return Err(format!(
            "A folder or meeting named '{sanitized}' already exists here"
            ));
    }

    let pool = state.db_manager.pool();
    let models = MeetingsRepository::get_meetings(pool)
        .await
        .map_err(|e| format!("Failed to load meetings: {e}"))?;

    // Compute affected meetings BEFORE the rename, while the source dirs still
    // exist (needed to canonicalize each stored path for prefix matching).
    let src_str = src.to_string_lossy().to_string();
    let dest_str = dest.to_string_lossy().to_string();
    let mut updates: Vec<(String, String)> = Vec::new();
    for m in &models {
        if let Some(fp) = &m.folder_path {
            if let Ok(canon) = Path::new(fp).canonicalize() {
                let canon_str = canon.to_string_lossy().to_string();
                if let Some(new_path) = rewrite_folder_path_prefix(&src_str, &dest_str, &canon_str) {
                    updates.push((m.id.clone(), new_path));
                }
            }
        }
    }

    if dest != src {
        std::fs::rename(&src, &dest).map_err(|e| format!("Failed to rename folder: {e}"))?;
    }

    if let Err(e) = MeetingsRepository::update_folder_paths(pool, &updates).await {
        // Best-effort rollback so the database never points at a moved dir.
        let _ = std::fs::rename(&dest, &src);
        return Err(format!("Failed to update meeting locations: {e}"));
    }

    Ok(dest_str)
}

/// Delete an organization folder — only when empty (no meetings, no subfolders).
#[tauri::command]
pub async fn api_delete_meeting_folder<R: Runtime>(
    app: AppHandle<R>,
    _state: tauri::State<'_, AppState>,
    path: String,
) -> Result<(), String> {
    let base = recordings_base(&app).await?;
    delete_empty_folder(&base, Path::new(&path))?;
    Ok(())
}

/// Move a meeting's directory into a target folder (or the root when `None`).
/// Filesystem first, database second, with best-effort rollback on failure.
#[tauri::command]
pub async fn api_move_meeting_to_folder<R: Runtime>(
    app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    target_folder_path: Option<String>,
) -> Result<String, String> {
    let base = recordings_base(&app).await?;
    let pool = state.db_manager.pool();

    let meeting = MeetingsRepository::get_meeting_metadata(pool, &meeting_id)
        .await
        .map_err(|e| format!("Failed to load meeting: {e}"))?
        .ok_or_else(|| format!("Meeting not found: {meeting_id}"))?;

    let folder_path = meeting
        .folder_path
        .ok_or_else(|| "Meeting has no folder on disk to move".to_string())?;

    let src = validate_within_base(&base, Path::new(&folder_path))?;
    let target_parent = match &target_folder_path {
        Some(p) => validate_within_base(&base, Path::new(p))?,
        None => base.clone(),
    };

    let name = src
        .file_name()
        .ok_or_else(|| "Source has no name".to_string())?;
    let dest = target_parent.join(name);

    if dest == src {
        return Ok(dest.to_string_lossy().to_string());
    }
    if dest.exists() {
        return Err("A meeting with the same name already exists in the target folder".to_string());
    }

    std::fs::rename(&src, &dest).map_err(|e| format!("Failed to move meeting: {e}"))?;
    let dest_str = dest.to_string_lossy().to_string();

    if let Err(e) = MeetingsRepository::update_folder_path(pool, &meeting_id, &dest_str).await {
        let _ = std::fs::rename(&dest, &src);
        return Err(format!("Failed to update meeting location: {e}"));
    }

    Ok(dest_str)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// Canonicalized temp base dir (macOS temp dirs are symlinks, so scanning
    /// canonicalizes — the base must be canonical too for `starts_with` checks).
    fn temp_base() -> (TempDir, PathBuf) {
        let dir = TempDir::new().unwrap();
        let base = dir.path().canonicalize().unwrap();
        (dir, base)
    }

    fn touch(path: &Path) {
        fs::write(path, b"x").unwrap();
    }

    #[test]
    fn create_folder_nested_and_collision() {
        let (_g, base) = temp_base();

        let setare = create_folder(&base, None, "Setare").unwrap();
        assert!(setare.is_dir());
        assert_eq!(setare, base.join("Setare"));

        let project = create_folder(&base, Some(&setare), "ProjetoX").unwrap();
        assert!(project.is_dir());
        assert_eq!(project, base.join("Setare").join("ProjetoX"));

        // Collision at the same level is rejected.
        let err = create_folder(&base, None, "Setare").unwrap_err();
        assert!(err.contains("already exists"), "got: {}", err);
    }

    #[test]
    fn create_folder_sanitizes_separators() {
        let (_g, base) = temp_base();
        // Slashes are sanitized to underscores — no path traversal.
        let created = create_folder(&base, None, "a/b:c").unwrap();
        assert_eq!(created.file_name().unwrap(), "a_b_c");
        assert!(created.starts_with(&base));
    }

    #[test]
    fn rename_folder_moves_dir() {
        let (_g, base) = temp_base();
        let src = create_folder(&base, None, "Old").unwrap();

        let (old_path, new_path) = rename_folder(&base, &src, "New").unwrap();
        assert_eq!(old_path, src);
        assert_eq!(new_path, base.join("New"));
        assert!(new_path.is_dir());
        assert!(!src.exists());
    }

    #[test]
    fn rename_folder_collision_rejected() {
        let (_g, base) = temp_base();
        let src = create_folder(&base, None, "Old").unwrap();
        create_folder(&base, None, "Taken").unwrap();

        let err = rename_folder(&base, &src, "Taken").unwrap_err();
        assert!(err.contains("already exists"), "got: {}", err);
    }

    #[test]
    fn delete_empty_folder_ok_and_nonempty_refused() {
        let (_g, base) = temp_base();

        let empty = create_folder(&base, None, "Empty").unwrap();
        // A stray dotfile does not count as content.
        touch(&empty.join(".DS_Store"));
        delete_empty_folder(&base, &empty).unwrap();
        assert!(!empty.exists());

        let full = create_folder(&base, None, "Full").unwrap();
        create_folder(&base, Some(&full), "Child").unwrap();
        let err = delete_empty_folder(&base, &full).unwrap_err();
        assert!(err.contains("not empty"), "got: {}", err);
        assert!(full.exists());
    }

    #[test]
    fn move_meeting_dir_and_collision() {
        let (_g, base) = temp_base();
        let folder = create_folder(&base, None, "Setare").unwrap();

        // A meeting directory sitting at the root.
        let meeting = base.join("Standup_2026-01-01_10-00");
        fs::create_dir_all(&meeting).unwrap();
        touch(&meeting.join("audio.mp4"));

        let dest = move_meeting_dir(&base, &meeting, Some(&folder)).unwrap();
        assert_eq!(dest, folder.join("Standup_2026-01-01_10-00"));
        assert!(dest.join("audio.mp4").exists());
        assert!(!meeting.exists());

        // Moving another dir with the same name into the folder collides.
        let meeting2 = base.join("Standup_2026-01-01_10-00");
        fs::create_dir_all(&meeting2).unwrap();
        let err = move_meeting_dir(&base, &meeting2, Some(&folder)).unwrap_err();
        assert!(err.contains("already exists"), "got: {}", err);
    }

    #[test]
    fn validate_within_base_rejects_outside_paths() {
        let (_g, base) = temp_base();
        let (_outside_guard, outside) = temp_base();

        assert!(validate_within_base(&base, &outside).is_err());
        // The base itself and children resolve fine.
        assert!(validate_within_base(&base, &base).is_ok());
        let child = create_folder(&base, None, "Child").unwrap();
        assert!(validate_within_base(&base, &child).is_ok());
    }

    #[test]
    fn rewrite_prefix_updates_children_only() {
        let sep = std::path::MAIN_SEPARATOR;
        let old = format!("{sep}base{sep}Old");
        let new = format!("{sep}base{sep}New");

        // Exact match.
        assert_eq!(
            rewrite_folder_path_prefix(&old, &new, &old),
            Some(new.clone())
        );
        // Nested child.
        let child = format!("{old}{sep}Meeting_1");
        assert_eq!(
            rewrite_folder_path_prefix(&old, &new, &child),
            Some(format!("{new}{sep}Meeting_1"))
        );
        // Unrelated path (prefix is not a path component) is untouched.
        let sibling = format!("{sep}base{sep}OldOther");
        assert_eq!(rewrite_folder_path_prefix(&old, &new, &sibling), None);
    }

    #[test]
    fn build_tree_classifies_folders_meetings_and_unfiled() {
        let (_g, base) = temp_base();

        // Organization folder with a meeting inside it.
        let setare = create_folder(&base, None, "Setare").unwrap();
        let filed = setare.join("ProjectMeeting_2026-01-02_09-00");
        fs::create_dir_all(&filed).unwrap();
        touch(&filed.join("transcripts.json"));

        // A meeting directory at the root -> Unfiled.
        let root_meeting = base.join("RootMeeting_2026-01-03_11-00");
        fs::create_dir_all(&root_meeting).unwrap();
        touch(&root_meeting.join("audio.mp4"));

        let filed_canon = filed.canonicalize().unwrap().to_string_lossy().to_string();
        let root_canon = root_meeting
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .to_string();

        let meetings = vec![
            MeetingRow {
                id: "m-filed".into(),
                title: "Project Meeting".into(),
                folder_path: Some(filed_canon),
            },
            MeetingRow {
                id: "m-root".into(),
                title: "Root Meeting".into(),
                folder_path: Some(root_canon),
            },
            MeetingRow {
                id: "m-null".into(),
                title: "Legacy Meeting".into(),
                folder_path: None,
            },
            MeetingRow {
                id: "m-missing".into(),
                title: "Gone Meeting".into(),
                folder_path: Some(base.join("Nope_2026").to_string_lossy().to_string()),
            },
        ];

        let tree = build_tree(&base, &meetings);

        // One top-level organization folder, holding the filed meeting.
        assert_eq!(tree.folders.len(), 1);
        assert_eq!(tree.folders[0].name, "Setare");
        assert_eq!(tree.folders[0].meetings.len(), 1);
        assert_eq!(tree.folders[0].meetings[0].id.as_deref(), Some("m-filed"));

        // Unfiled: the root meeting, the NULL meeting, and the missing meeting.
        let ids: HashSet<Option<String>> =
            tree.unfiled.iter().map(|m| m.id.clone()).collect();
        assert!(ids.contains(&Some("m-root".to_string())));
        assert!(ids.contains(&Some("m-null".to_string())));
        assert!(ids.contains(&Some("m-missing".to_string())));

        let missing = tree
            .unfiled
            .iter()
            .find(|m| m.id.as_deref() == Some("m-missing"))
            .unwrap();
        assert!(missing.missing);

        let legacy = tree
            .unfiled
            .iter()
            .find(|m| m.id.as_deref() == Some("m-null"))
            .unwrap();
        assert!(!legacy.missing);
        assert!(legacy.path.is_none());
    }
}
