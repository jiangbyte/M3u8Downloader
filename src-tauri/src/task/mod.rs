use crate::download::{DownloadContext, download_segment_manual, run_download};
use crate::error::{AppError, AppResult};
use crate::http::{RequestOptions, build_client, build_headers, fetch_text};
use crate::m3u8::parser::parse_playlist;
use crate::m3u8::AnalyzeResult;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TaskStatus {
    Queued,
    Downloading,
    Paused,
    Merging,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskSnapshot {
    pub id: String,
    pub title: String,
    pub url: String,
    pub status: TaskStatus,
    pub progress: f64,
    pub downloaded_segments: u64,
    pub total_segments: u64,
    pub speed_bps: f64,
    pub eta_secs: Option<u64>,
    pub output_path: Option<String>,
    pub work_dir: String,
    pub error: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SegmentState {
    Pending,
    Downloading,
    Done,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SegmentStatus {
    pub index: usize,
    pub name: String,
    pub state: SegmentState,
    pub size: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskDetail {
    pub task: TaskSnapshot,
    pub segments: Vec<SegmentStatus>,
    pub done_count: u64,
    pub pending_count: u64,
}

pub struct TaskControl {
    pub paused: AtomicBool,
    pub cancelled: AtomicBool,
    /// True while a download worker for this task is running.
    pub running: AtomicBool,
    /// Bumped on pause/cancel/remove to abort in-flight HTTP downloads.
    epoch: AtomicU64,
    interrupt: tokio::sync::Notify,
    segment_cancel: Mutex<std::collections::HashSet<usize>>,
    segment_active: Mutex<std::collections::HashSet<usize>>,
    segment_failed: Mutex<HashMap<usize, String>>,
    /// Segments allowed to run even while the task is paused (manual start/retry).
    manual_allow: Mutex<std::collections::HashSet<usize>>,
}

impl TaskControl {
    pub fn new(paused: bool, cancelled: bool, running: bool) -> Self {
        Self {
            paused: AtomicBool::new(paused),
            cancelled: AtomicBool::new(cancelled),
            running: AtomicBool::new(running),
            epoch: AtomicU64::new(0),
            interrupt: tokio::sync::Notify::new(),
            segment_cancel: Mutex::new(std::collections::HashSet::new()),
            segment_active: Mutex::new(std::collections::HashSet::new()),
            segment_failed: Mutex::new(HashMap::new()),
            manual_allow: Mutex::new(std::collections::HashSet::new()),
        }
    }

    pub fn interrupt_downloads(&self) {
        self.epoch.fetch_add(1, Ordering::SeqCst);
        self.interrupt.notify_waiters();
    }

    pub fn current_epoch(&self) -> u64 {
        self.epoch.load(Ordering::SeqCst)
    }

    pub async fn wait_epoch_change(&self, epoch: u64) {
        loop {
            if self.epoch.load(Ordering::SeqCst) != epoch {
                return;
            }
            self.interrupt.notified().await;
        }
    }

    /// Wake pause-waiters (cancel/resume) without aborting via epoch if not needed.
    pub fn notify_waiters(&self) {
        self.interrupt.notify_waiters();
    }

    pub async fn wait_signal(&self) {
        self.interrupt.notified().await;
    }

    pub fn is_segment_cancelled(&self, index: usize) -> bool {
        self.segment_cancel.lock().contains(&index)
    }

    pub fn mark_segment_cancelled(&self, index: usize) {
        self.segment_cancel.lock().insert(index);
        self.segment_active.lock().remove(&index);
        self.manual_allow.lock().remove(&index);
    }

    pub fn clear_segment_cancelled(&self, index: usize) {
        self.segment_cancel.lock().remove(&index);
    }

    pub fn cancel_all_active_segments(&self) {
        let active: Vec<usize> = self.segment_active.lock().iter().copied().collect();
        for index in active {
            self.mark_segment_cancelled(index);
        }
        self.manual_allow.lock().clear();
    }

    pub fn mark_segment_active(&self, index: usize, active: bool) {
        let mut set = self.segment_active.lock();
        if active {
            set.insert(index);
        } else {
            set.remove(&index);
        }
    }

    pub fn is_segment_active(&self, index: usize) -> bool {
        self.segment_active.lock().contains(&index)
    }

    pub fn mark_segment_failed(&self, index: usize, err: String) {
        self.segment_failed.lock().insert(index, err);
        self.segment_active.lock().remove(&index);
    }

    pub fn clear_segment_failed(&self, index: usize) {
        self.segment_failed.lock().remove(&index);
    }

    pub fn segment_error(&self, index: usize) -> Option<String> {
        self.segment_failed.lock().get(&index).cloned()
    }

    pub fn allow_manual(&self, index: usize) {
        self.manual_allow.lock().insert(index);
    }

    pub fn clear_manual(&self, index: usize) {
        self.manual_allow.lock().remove(&index);
    }

    pub fn clear_all_manual(&self) {
        self.manual_allow.lock().clear();
    }

    pub fn is_manual_allowed(&self, index: usize) -> bool {
        self.manual_allow.lock().contains(&index)
    }

    /// Whether this segment download should stop now.
    /// `honor_pause`: pool workers honor pause; manual can ignore if allow_manual.
    pub fn should_stop(&self, index: usize, honor_pause: bool) -> Option<&'static str> {
        if self.cancelled.load(Ordering::SeqCst) {
            return Some("Cancelled");
        }
        if self.is_segment_cancelled(index) {
            return Some("SegmentCancelled");
        }
        if honor_pause
            && self.paused.load(Ordering::SeqCst)
            && !self.is_manual_allowed(index)
        {
            return Some("Paused");
        }
        None
    }
}

struct ManagedTask {
    snapshot: Arc<Mutex<TaskSnapshot>>,
    control: Arc<TaskControl>,
}

pub struct TaskManager {
    inner: Mutex<HashMap<String, ManagedTask>>,
    persist_path: PathBuf,
}

impl TaskManager {
    pub fn new(app_data: PathBuf) -> Self {
        let persist_path = app_data.join("tasks.json");
        let mgr = Self {
            inner: Mutex::new(HashMap::new()),
            persist_path,
        };
        let _ = mgr.load();
        mgr
    }

    fn load(&self) -> AppResult<()> {
        if !self.persist_path.exists() {
            return Ok(());
        }
        let data = std::fs::read_to_string(&self.persist_path)?;
        let list: Vec<TaskSnapshot> = serde_json::from_str(&data)?;
        let mut guard = self.inner.lock();
        for mut snap in list {
            if matches!(
                snap.status,
                TaskStatus::Downloading
                    | TaskStatus::Merging
                    | TaskStatus::Queued
                    | TaskStatus::Paused
            ) {
                snap.status = TaskStatus::Paused;
            }
            let id = snap.id.clone();
            guard.insert(
                id,
                ManagedTask {
                    snapshot: Arc::new(Mutex::new(snap)),
                    control: Arc::new(TaskControl::new(true, false, false)),
                },
            );
        }
        Ok(())
    }

    pub fn persist(&self) {
        let guard = self.inner.lock();
        let list: Vec<TaskSnapshot> = guard
            .values()
            .map(|t| t.snapshot.lock().clone())
            .collect();
        drop(guard);
        if let Some(parent) = self.persist_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(data) = serde_json::to_string_pretty(&list) {
            let _ = std::fs::write(&self.persist_path, data);
        }
    }

    pub fn get(&self, id: &str) -> Option<TaskSnapshot> {
        self.inner
            .lock()
            .get(id)
            .map(|t| t.snapshot.lock().clone())
    }

    pub fn task_detail(&self, id: &str) -> AppResult<TaskDetail> {
        let guard = self.inner.lock();
        let managed = guard
            .get(id)
            .ok_or_else(|| AppError::msg("Task not found"))?;
        let task = managed.snapshot.lock().clone();
        let control = managed.control.clone();
        drop(guard);

        let segments_dir = PathBuf::from(&task.work_dir).join("segments");
        let total = task.total_segments as usize;
        let mut segments = Vec::with_capacity(total);
        let mut done_count = 0u64;

        let resolve_state = |index: usize, size: u64| -> (SegmentState, Option<String>) {
            if size > 0 {
                return (SegmentState::Done, None);
            }
            if control.is_segment_active(index) {
                return (SegmentState::Downloading, None);
            }
            if let Some(err) = control.segment_error(index) {
                return (SegmentState::Failed, Some(err));
            }
            if control.is_segment_cancelled(index) {
                return (SegmentState::Cancelled, None);
            }
            (SegmentState::Pending, None)
        };

        if total == 0 {
            if let Ok(rd) = std::fs::read_dir(&segments_dir) {
                let mut files: Vec<_> = rd
                    .flatten()
                    .filter_map(|e| {
                        let name = e.file_name().to_string_lossy().to_string();
                        if name.ends_with(".ts") && !name.ends_with(".ts.part") {
                            let size = e.metadata().map(|m| m.len()).unwrap_or(0);
                            Some((name, size))
                        } else {
                            None
                        }
                    })
                    .collect();
                files.sort_by(|a, b| a.0.cmp(&b.0));
                for (i, (name, size)) in files.into_iter().enumerate() {
                    let (state, error) = resolve_state(i, size);
                    if state == SegmentState::Done {
                        done_count += 1;
                    }
                    segments.push(SegmentStatus {
                        index: i,
                        name,
                        state,
                        size,
                        error,
                    });
                }
            }
        } else {
            for i in 0..total {
                let name = format!("{:06}.ts", i);
                let path = segments_dir.join(&name);
                let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                let (state, error) = resolve_state(i, size);
                if state == SegmentState::Done {
                    done_count += 1;
                }
                segments.push(SegmentStatus {
                    index: i,
                    name,
                    state,
                    size,
                    error,
                });
            }
        }

        let pending_count = segments
            .iter()
            .filter(|s| {
                matches!(
                    s.state,
                    SegmentState::Pending | SegmentState::Failed | SegmentState::Cancelled
                )
            })
            .count() as u64;
        Ok(TaskDetail {
            task,
            segments,
            done_count,
            pending_count,
        })
    }

    pub fn list(&self) -> Vec<TaskSnapshot> {
        let mut list: Vec<_> = self
            .inner
            .lock()
            .values()
            .map(|t| t.snapshot.lock().clone())
            .collect();
        list.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        list
    }

    pub async fn analyze(url: String, opts: RequestOptions) -> AppResult<AnalyzeResult> {
        let client = build_client(&opts)?;
        let headers = build_headers(&opts)?;
        let text = fetch_text(&client, &url, &headers).await?;
        parse_playlist(&text, &url)
    }

    pub fn start_task(&self, app: AppHandle, input: StartTaskInput) -> AppResult<TaskSnapshot> {
        let id = Uuid::new_v4().to_string();
        let title = input
            .filename
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| {
                url::Url::parse(&input.url)
                    .ok()
                    .and_then(|u| {
                        u.path_segments()
                            .and_then(|s| s.last().map(|x| x.to_string()))
                    })
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| "video".into())
            });
        let title = title.trim_end_matches(".mp4").trim().to_string();
        let title = if title.is_empty() {
            "video".into()
        } else {
            title
        };

        let base_dir = input
            .output_dir
            .as_ref()
            .map(PathBuf::from)
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| {
                dirs::download_dir()
                    .or_else(dirs::home_dir)
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join("M3U8Downloads")
            });

        let work_dir = base_dir.join(&id);
        let output_path = base_dir.join(format!("{title}.mp4"));
        std::fs::create_dir_all(&work_dir)?;

        let media_url = input
            .selected_variant_url
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| input.url.clone());

        let concurrency = input.concurrency.unwrap_or(16).clamp(1, 64);
        let cleanup = input.cleanup_segments.unwrap_or(true);
        write_task_meta(&work_dir, &media_url, &input.options, concurrency, cleanup);

        let created_at = chrono::Local::now().to_rfc3339();
        let snap = TaskSnapshot {
            id: id.clone(),
            title: title.clone(),
            url: input.url.clone(),
            status: TaskStatus::Queued,
            progress: 0.0,
            downloaded_segments: 0,
            total_segments: 0,
            speed_bps: 0.0,
            eta_secs: None,
            output_path: Some(output_path.to_string_lossy().to_string()),
            work_dir: work_dir.to_string_lossy().to_string(),
            error: None,
            created_at,
        };

        let control = Arc::new(TaskControl::new(false, false, true));
        let snapshot = Arc::new(Mutex::new(snap.clone()));

        self.inner.lock().insert(
            id.clone(),
            ManagedTask {
                snapshot: snapshot.clone(),
                control: control.clone(),
            },
        );
        self.persist();
        let _ = app.emit("task-progress", &snap);

        let ctx = DownloadContext {
            app: app.clone(),
            task_id: id,
            media_url,
            opts: input.options,
            concurrency,
            work_dir,
            output_path,
            cleanup_segments: input.cleanup_segments.unwrap_or(true),
            control: control.clone(),
            snapshot,
        };

        tauri::async_runtime::spawn(async move {
            let _ = run_download(ctx).await;
            control.running.store(false, Ordering::SeqCst);
        });

        Ok(snap)
    }

    pub fn pause(&self, id: &str) -> AppResult<TaskSnapshot> {
        let guard = self.inner.lock();
        let task = guard
            .get(id)
            .ok_or_else(|| AppError::msg("Task not found"))?;
        task.control.paused.store(true, Ordering::SeqCst);
        // Drop manual overrides so pause stops every in-flight segment.
        task.control.clear_all_manual();
        task.control.interrupt_downloads();
        let mut s = task.snapshot.lock();
        if matches!(
            s.status,
            TaskStatus::Downloading | TaskStatus::Queued | TaskStatus::Merging
        ) {
            s.status = TaskStatus::Paused;
            s.speed_bps = 0.0;
            s.eta_secs = None;
        }
        let snap = s.clone();
        drop(s);
        drop(guard);
        self.persist();
        Ok(snap)
    }

    pub fn resume(&self, app: AppHandle, id: &str) -> AppResult<TaskSnapshot> {
        let (restart, snap_clone, control, snapshot) = {
            let guard = self.inner.lock();
            let task = guard
                .get(id)
                .ok_or_else(|| AppError::msg("Task not found"))?;
            let worker_alive = task.control.running.load(Ordering::SeqCst);
            // Clear cancel so a previously cancelled task can restart.
            task.control.cancelled.store(false, Ordering::SeqCst);
            task.control.paused.store(false, Ordering::SeqCst);
            task.control.notify_waiters();
            let mut s = task.snapshot.lock();
            let restart = !worker_alive
                && matches!(
                    s.status,
                    TaskStatus::Paused | TaskStatus::Failed | TaskStatus::Cancelled
                );
            if worker_alive {
                s.status = TaskStatus::Downloading;
                s.error = None;
            } else {
                s.status = TaskStatus::Queued;
                s.error = None;
            }
            let snap = s.clone();
            (restart, snap, task.control.clone(), task.snapshot.clone())
        };
        self.persist();
        let _ = app.emit("task-progress", &snap_clone);

        if restart {
            control.running.store(true, Ordering::SeqCst);
            control.cancelled.store(false, Ordering::SeqCst);
            control.paused.store(false, Ordering::SeqCst);
            let work_dir = PathBuf::from(&snap_clone.work_dir);
            let output_path = PathBuf::from(
                snap_clone
                    .output_path
                    .clone()
                    .unwrap_or_else(|| work_dir.join("output.mp4").to_string_lossy().to_string()),
            );
            let media_url = std::fs::read_to_string(work_dir.join("meta.url"))
                .unwrap_or_else(|_| snap_clone.url.clone());
            let opts: RequestOptions = std::fs::read_to_string(work_dir.join("meta.opts.json"))
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default();
            let concurrency: usize = std::fs::read_to_string(work_dir.join("meta.concurrency"))
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(16);
            let cleanup = std::fs::read_to_string(work_dir.join("meta.cleanup"))
                .ok()
                .map(|s| s.trim() == "1")
                .unwrap_or(true);

            let running_flag = control.clone();
            let ctx = DownloadContext {
                app: app.clone(),
                task_id: id.to_string(),
                media_url,
                opts,
                concurrency,
                work_dir,
                output_path,
                cleanup_segments: cleanup,
                control,
                snapshot,
            };
            tauri::async_runtime::spawn(async move {
                let _ = run_download(ctx).await;
                running_flag.running.store(false, Ordering::SeqCst);
            });
        }

        Ok(snap_clone)
    }

    pub fn cancel(&self, id: &str) -> AppResult<TaskSnapshot> {
        let guard = self.inner.lock();
        let task = guard
            .get(id)
            .ok_or_else(|| AppError::msg("Task not found"))?;
        // Wake pause waiters, abort in-flight HTTP, cancel active segments.
        task.control.paused.store(false, Ordering::SeqCst);
        task.control.cancelled.store(true, Ordering::SeqCst);
        task.control.cancel_all_active_segments();
        task.control.interrupt_downloads();
        let mut s = task.snapshot.lock();
        s.status = TaskStatus::Cancelled;
        s.speed_bps = 0.0;
        s.eta_secs = None;
        let snap = s.clone();
        drop(s);
        drop(guard);
        self.persist();
        Ok(snap)
    }

    pub fn remove(&self, id: &str, delete_files: bool) -> AppResult<()> {
        let task = self.inner.lock().remove(id);
        if let Some(task) = task {
            // Must clear pause so workers blocked in wait_if_paused can observe cancel.
            task.control.paused.store(false, Ordering::SeqCst);
            task.control.cancelled.store(true, Ordering::SeqCst);
            task.control.cancel_all_active_segments();
            task.control.interrupt_downloads();
            let work_dir = task.snapshot.lock().work_dir.clone();
            if delete_files {
                // Best-effort; download worker may still hold files briefly.
                let dir = work_dir.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    let _ = std::fs::remove_dir_all(dir);
                });
            }
        }
        self.persist();
        Ok(())
    }

    /// Start or retry a single segment (can run while task is paused).
    pub fn start_segment(&self, app: AppHandle, id: &str, index: usize) -> AppResult<()> {
        let (work_dir, opts, control, snapshot) = {
            let guard = self.inner.lock();
            let task = guard
                .get(id)
                .ok_or_else(|| AppError::msg("Task not found"))?;
            if task.control.cancelled.load(Ordering::SeqCst) {
                return Err(AppError::msg("任务已取消，请先点击继续"));
            }
            if task.control.is_segment_active(index) {
                return Err(AppError::msg("该分片已在下载中"));
            }
            let work_dir = PathBuf::from(task.snapshot.lock().work_dir.clone());
            let opts: RequestOptions = std::fs::read_to_string(work_dir.join("meta.opts.json"))
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default();
            (
                work_dir,
                opts,
                task.control.clone(),
                task.snapshot.clone(),
            )
        };

        control.clear_segment_cancelled(index);
        control.clear_segment_failed(index);
        let _ = app.emit("segment-update", id);

        let task_id = id.to_string();
        tauri::async_runtime::spawn(async move {
            let _ = download_segment_manual(
                app, task_id, work_dir, opts, control, snapshot, index,
            )
            .await;
        });
        Ok(())
    }

    pub fn cancel_segment(&self, app: AppHandle, id: &str, index: usize) -> AppResult<()> {
        let guard = self.inner.lock();
        let task = guard
            .get(id)
            .ok_or_else(|| AppError::msg("Task not found"))?;
        task.control.mark_segment_cancelled(index);
        drop(guard);
        let _ = app.emit("segment-update", id);
        Ok(())
    }

    pub fn retry_segment(&self, app: AppHandle, id: &str, index: usize) -> AppResult<()> {
        self.start_segment(app, id, index)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartTaskInput {
    pub url: String,
    pub selected_variant_url: Option<String>,
    pub output_dir: Option<String>,
    pub filename: Option<String>,
    pub concurrency: Option<usize>,
    pub cleanup_segments: Option<bool>,
    #[serde(default)]
    pub options: RequestOptions,
}

pub fn write_task_meta(
    work_dir: &std::path::Path,
    media_url: &str,
    opts: &RequestOptions,
    concurrency: usize,
    cleanup: bool,
) {
    let _ = std::fs::create_dir_all(work_dir);
    let _ = std::fs::write(work_dir.join("meta.url"), media_url);
    if let Ok(s) = serde_json::to_string_pretty(opts) {
        let _ = std::fs::write(work_dir.join("meta.opts.json"), s);
    }
    let _ = std::fs::write(work_dir.join("meta.concurrency"), concurrency.to_string());
    let _ = std::fs::write(
        work_dir.join("meta.cleanup"),
        if cleanup { "1" } else { "0" },
    );
}
