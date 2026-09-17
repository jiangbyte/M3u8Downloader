use crate::http::RequestOptions;
use crate::m3u8::AnalyzeResult;
use crate::play::{PlayService, build_playlist_body, open_media};
use crate::task::{StartTaskInput, TaskDetail, TaskManager, TaskSnapshot, TaskStatus};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};

#[tauri::command]
pub async fn analyze_url(
    url: String,
    options: Option<RequestOptions>,
) -> Result<AnalyzeResult, String> {
    let opts = options.unwrap_or_default();
    TaskManager::analyze(url, opts)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub fn start_task(
    app: AppHandle,
    manager: State<'_, Arc<TaskManager>>,
    input: StartTaskInput,
) -> Result<TaskSnapshot, String> {
    let snap = manager.start_task(app, input).map_err(Into::<String>::into)?;
    manager.persist();
    Ok(snap)
}

#[tauri::command]
pub fn list_tasks(manager: State<'_, Arc<TaskManager>>) -> Result<Vec<TaskSnapshot>, String> {
    Ok(manager.list())
}

#[tauri::command]
pub fn get_task_detail(
    manager: State<'_, Arc<TaskManager>>,
    id: String,
) -> Result<TaskDetail, String> {
    manager.task_detail(&id).map_err(Into::into)
}

#[tauri::command]
pub fn pause_task(
    app: AppHandle,
    manager: State<'_, Arc<TaskManager>>,
    id: String,
) -> Result<TaskSnapshot, String> {
    let snap = manager.pause(&id).map_err(Into::<String>::into)?;
    let _ = app.emit("task-progress", &snap);
    let _ = app.emit("segment-update", &id);
    Ok(snap)
}

#[tauri::command]
pub fn resume_task(
    app: AppHandle,
    manager: State<'_, Arc<TaskManager>>,
    id: String,
) -> Result<TaskSnapshot, String> {
    manager.resume(app, &id).map_err(Into::into)
}

#[tauri::command]
pub fn cancel_task(
    app: AppHandle,
    manager: State<'_, Arc<TaskManager>>,
    id: String,
) -> Result<TaskSnapshot, String> {
    let snap = manager.cancel(&id).map_err(Into::<String>::into)?;
    let _ = app.emit("task-progress", &snap);
    let _ = app.emit("segment-update", &id);
    Ok(snap)
}

#[tauri::command]
pub fn remove_task(
    manager: State<'_, Arc<TaskManager>>,
    play: State<'_, Arc<PlayService>>,
    id: String,
    delete_files: Option<bool>,
) -> Result<(), String> {
    play.unmount(&id);
    manager
        .remove(&id, delete_files.unwrap_or(false))
        .map_err(Into::into)
}

#[tauri::command]
pub fn start_segment(
    app: AppHandle,
    manager: State<'_, Arc<TaskManager>>,
    id: String,
    index: usize,
) -> Result<(), String> {
    manager.start_segment(app, &id, index).map_err(Into::into)
}

#[tauri::command]
pub fn cancel_segment(
    app: AppHandle,
    manager: State<'_, Arc<TaskManager>>,
    id: String,
    index: usize,
) -> Result<(), String> {
    manager.cancel_segment(app, &id, index).map_err(Into::into)
}

#[tauri::command]
pub fn retry_segment(
    app: AppHandle,
    manager: State<'_, Arc<TaskManager>>,
    id: String,
    index: usize,
) -> Result<(), String> {
    manager.retry_segment(app, &id, index).map_err(Into::into)
}

#[tauri::command]
pub async fn play_task(
    manager: State<'_, Arc<TaskManager>>,
    play: State<'_, Arc<PlayService>>,
    id: String,
) -> Result<String, String> {
    let task = manager
        .get(&id)
        .ok_or_else(|| "Task not found".to_string())?;

    if task.status == TaskStatus::Completed {
        if let Some(out) = &task.output_path {
            let p = PathBuf::from(out);
            if p.is_file() {
                open_media(out).map_err(|e| e.to_string())?;
                return Ok(out.clone());
            }
        }
    }

    let work_dir = PathBuf::from(&task.work_dir);
    // Validate contiguous playable segments before launching player
    build_playlist_body(&work_dir).map_err(|e| e.to_string())?;
    let url = play
        .play_url(&id, work_dir)
        .await
        .map_err(|e| e.to_string())?;
    open_media(&url).map_err(|e| e.to_string())?;
    Ok(url)
}

#[tauri::command]
pub fn open_path(path: String) -> Result<(), String> {
    let p = PathBuf::from(&path);
    let target = if p.is_dir() {
        p
    } else {
        p.parent()
            .map(|x| x.to_path_buf())
            .ok_or_else(|| "Invalid path".to_string())?
    };
    open_dir(&target)
}

#[tauri::command]
pub fn default_output_dir() -> Result<String, String> {
    let dir = dirs::download_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("M3U8Downloads");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.to_string_lossy().to_string())
}

fn open_dir(path: &std::path::Path) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
