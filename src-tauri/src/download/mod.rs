use crate::crypto::{decrypt_aes128, parse_iv};
use crate::error::{AppError, AppResult};
use crate::http::{RequestOptions, build_client, build_headers, fetch_bytes, fetch_text};
use crate::m3u8::parser::parse_playlist;
use crate::m3u8::{KeyInfo, MediaPlaylist, PlaylistKind, SegmentInfo};
use crate::merge::merge_segments_to_mp4;
use crate::task::{TaskControl, TaskSnapshot, TaskStatus};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use tauri::{AppHandle, Emitter};
use tokio::sync::{Semaphore, mpsc};

#[derive(Clone)]
pub struct DownloadContext {
    pub app: AppHandle,
    #[allow(dead_code)]
    pub task_id: String,
    pub media_url: String,
    pub opts: RequestOptions,
    pub concurrency: usize,
    pub work_dir: PathBuf,
    pub output_path: PathBuf,
    pub cleanup_segments: bool,
    pub control: Arc<TaskControl>,
    pub snapshot: Arc<Mutex<TaskSnapshot>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlaylistMeta {
    pub media_sequence: u64,
    pub segments: Vec<SegmentInfo>,
}

pub async fn wait_if_paused(control: &TaskControl) -> bool {
    loop {
        if control.cancelled.load(Ordering::SeqCst) {
            return false;
        }
        if !control.paused.load(Ordering::SeqCst) {
            return true;
        }
        // Wake promptly on resume/cancel/interrupt instead of only polling.
        tokio::select! {
            _ = control.wait_signal() => {}
            _ = tokio::time::sleep(tokio::time::Duration::from_millis(150)) => {}
        }
    }
}

fn is_cancel_err(e: &AppError) -> bool {
    let s = e.to_string();
    s == "Cancelled" || s == "SegmentCancelled"
}

fn is_pause_err(e: &AppError) -> bool {
    e.to_string() == "Paused"
}

fn is_stop_err(e: &AppError) -> bool {
    is_cancel_err(e) || is_pause_err(e)
}

fn emit_progress(app: &AppHandle, snap: &TaskSnapshot) {
    let _ = app.emit("task-progress", snap);
}

fn emit_segment_update(app: &AppHandle, task_id: &str) {
    let _ = app.emit("segment-update", task_id);
}

async fn load_key_cache(
    client: &reqwest::Client,
    headers: &reqwest::header::HeaderMap,
    key: &KeyInfo,
    cache: &Mutex<HashMap<String, Vec<u8>>>,
) -> AppResult<Vec<u8>> {
    let uri = key
        .uri
        .as_ref()
        .ok_or_else(|| AppError::msg("AES key URI missing"))?;
    {
        let guard = cache.lock();
        if let Some(v) = guard.get(uri) {
            return Ok(v.clone());
        }
    }
    let bytes = fetch_bytes(client, uri, headers, None).await?;
    cache.lock().insert(uri.clone(), bytes.clone());
    Ok(bytes)
}

pub fn segment_path(dir: &Path, index: usize) -> PathBuf {
    dir.join(format!("{:06}.ts", index))
}

fn save_playlist_meta(work_dir: &Path, media: &MediaPlaylist) -> AppResult<()> {
    let meta = PlaylistMeta {
        media_sequence: media.media_sequence,
        segments: media.segments.clone(),
    };
    let path = work_dir.join("segments_meta.json");
    let data = serde_json::to_string_pretty(&meta)?;
    std::fs::write(path, data)?;
    Ok(())
}

pub fn load_playlist_meta(work_dir: &Path) -> AppResult<PlaylistMeta> {
    let path = work_dir.join("segments_meta.json");
    let data = std::fs::read_to_string(path)
        .map_err(|_| AppError::msg("分片元数据不存在，请先开始或继续任务"))?;
    Ok(serde_json::from_str(&data)?)
}

pub async fn run_download(ctx: DownloadContext) -> AppResult<()> {
    ctx.control.running.store(true, Ordering::SeqCst);
    {
        let mut s = ctx.snapshot.lock();
        if !ctx.control.cancelled.load(Ordering::SeqCst) {
            s.status = TaskStatus::Downloading;
            s.error = None;
            emit_progress(&ctx.app, &s);
        }
    }

    let result = run_download_inner(ctx.clone()).await;
    ctx.control.running.store(false, Ordering::SeqCst);

    if ctx.control.cancelled.load(Ordering::SeqCst) {
        let mut s = ctx.snapshot.lock();
        if !matches!(s.status, TaskStatus::Completed) {
            s.status = TaskStatus::Cancelled;
            s.speed_bps = 0.0;
            s.eta_secs = None;
            emit_progress(&ctx.app, &s);
        }
        return Ok(());
    }

    result
}

async fn run_download_inner(ctx: DownloadContext) -> AppResult<()> {
    if ctx.control.cancelled.load(Ordering::SeqCst) {
        return Ok(());
    }
    if !wait_if_paused(&ctx.control).await {
        return Ok(());
    }

    let client = build_client(&ctx.opts)?;
    let headers = build_headers(&ctx.opts)?;
    let text = fetch_text(&client, &ctx.media_url, &headers).await?;
    if ctx.control.cancelled.load(Ordering::SeqCst) {
        return Ok(());
    }

    let analyzed = parse_playlist(&text, &ctx.media_url)?;
    let media: MediaPlaylist = match analyzed.kind {
        PlaylistKind::Media => analyzed
            .media
            .ok_or_else(|| AppError::msg("Empty media playlist"))?,
        PlaylistKind::Master => {
            return Err(AppError::msg(
                "Expected media playlist; select a variant first",
            ));
        }
    };

    if !media.end_list {
        log::warn!("Playlist has no EXT-X-ENDLIST; treating as VOD snapshot");
    }

    let segments_dir = ctx.work_dir.join("segments");
    tokio::fs::create_dir_all(&segments_dir).await?;
    let _ = save_playlist_meta(&ctx.work_dir, &media);

    let total = media.segments.len();
    let mut pending: Vec<SegmentInfo> = Vec::new();
    let mut initial_done = 0u64;
    for seg in &media.segments {
        let path = segment_path(&segments_dir, seg.index);
        if let Ok(meta) = tokio::fs::metadata(&path).await {
            if meta.len() > 0 {
                initial_done += 1;
                continue;
            }
        }
        // Skip user-cancelled segments in this run; they can be started manually
        if ctx.control.is_segment_cancelled(seg.index) {
            continue;
        }
        pending.push(seg.clone());
    }

    {
        let mut s = ctx.snapshot.lock();
        s.total_segments = total as u64;
        s.downloaded_segments = initial_done;
        s.progress = if total == 0 {
            0.0
        } else {
            initial_done as f64 / total as f64 * 100.0
        };
        emit_progress(&ctx.app, &s);
    }

    if pending.is_empty() {
        // All done or only cancelled left
        let missing = (0..total).any(|i| {
            let p = segment_path(&segments_dir, i);
            !p.exists() || std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0) == 0
        });
        if missing {
            let mut s = ctx.snapshot.lock();
            s.status = TaskStatus::Paused;
            s.error = Some("部分分片未完成，可在详情中单独开始/重试".into());
            emit_progress(&ctx.app, &s);
            return Ok(());
        }
        return finish_merge(&ctx, total, &segments_dir).await;
    }

    let concurrency = ctx.concurrency.max(1).min(pending.len().max(1));
    let (tx, rx) = mpsc::channel::<SegmentInfo>(pending.len().max(1));
    for seg in pending {
        if ctx.control.cancelled.load(Ordering::SeqCst) {
            break;
        }
        let _ = tx.send(seg).await;
    }
    drop(tx);

    let key_cache: Arc<Mutex<HashMap<String, Vec<u8>>>> = Arc::new(Mutex::new(HashMap::new()));
    let downloaded_bytes = Arc::new(AtomicU64::new(0));
    let done_count = Arc::new(AtomicU64::new(initial_done));
    let speed_window = Arc::new(Mutex::new((Instant::now(), 0u64)));
    let rx = Arc::new(tokio::sync::Mutex::new(rx));
    let sem = Arc::new(Semaphore::new(concurrency));
    let task_id = ctx.task_id.clone();

    let mut handles = Vec::with_capacity(concurrency);
    for _ in 0..concurrency {
        let client = client.clone();
        let headers = headers.clone();
        let segments_dir = segments_dir.clone();
        let key_cache = key_cache.clone();
        let control = ctx.control.clone();
        let snapshot = ctx.snapshot.clone();
        let app = ctx.app.clone();
        let downloaded_bytes = downloaded_bytes.clone();
        let done_count = done_count.clone();
        let speed_window = speed_window.clone();
        let rx = rx.clone();
        let sem = sem.clone();
        let media_sequence = media.media_sequence;
        let total_u64 = total as u64;
        let initial_done = initial_done;
        let task_id = task_id.clone();

        handles.push(tokio::spawn(async move {
            loop {
                if control.cancelled.load(Ordering::SeqCst) {
                    break;
                }
                if !wait_if_paused(&control).await {
                    break;
                }

                let seg = {
                    let mut guard = rx.lock().await;
                    guard.recv().await
                };
                let Some(seg) = seg else { break };

                if control.is_segment_cancelled(seg.index) {
                    continue;
                }

                let Ok(_permit) = sem.clone().acquire_owned().await else {
                    break;
                };

                if control.cancelled.load(Ordering::SeqCst) {
                    break;
                }
                if control.is_segment_cancelled(seg.index) {
                    continue;
                }
                if control.is_segment_active(seg.index) {
                    continue;
                }
                // Skip if already finished (e.g. manual download completed it)
                let existing = segment_path(&segments_dir, seg.index);
                if std::fs::metadata(&existing).map(|m| m.len()).unwrap_or(0) > 0 {
                    let done = done_count.fetch_add(1, Ordering::SeqCst) + 1;
                    {
                        let mut s = snapshot.lock();
                        s.downloaded_segments = done;
                        s.total_segments = total_u64;
                        s.progress = done as f64 / total_u64 as f64 * 100.0;
                        emit_progress(&app, &s);
                    }
                    continue;
                }
                if !wait_if_paused(&control).await {
                    break;
                }

                control.mark_segment_active(seg.index, true);
                emit_segment_update(&app, &task_id);

                let result = download_one_segment(
                    &client,
                    &headers,
                    &seg,
                    media_sequence,
                    &segments_dir,
                    &key_cache,
                    &control,
                )
                .await;

                control.mark_segment_active(seg.index, false);

                match result {
                    Ok(nbytes) => {
                        control.clear_segment_failed(seg.index);
                        if control.cancelled.load(Ordering::SeqCst)
                            || control.is_segment_cancelled(seg.index)
                        {
                            let _ = std::fs::remove_file(segment_path(&segments_dir, seg.index));
                            emit_segment_update(&app, &task_id);
                            continue;
                        }
                        downloaded_bytes.fetch_add(nbytes, Ordering::Relaxed);
                        let finished = done_count.fetch_add(1, Ordering::Relaxed) + 1;
                        let mut speed = 0.0f64;
                        {
                            let mut win = speed_window.lock();
                            win.1 += nbytes;
                            let elapsed = win.0.elapsed().as_secs_f64();
                            if elapsed >= 0.5 {
                                speed = win.1 as f64 / elapsed;
                                *win = (Instant::now(), 0);
                            }
                        }
                        let mut s = snapshot.lock();
                        if control.cancelled.load(Ordering::SeqCst) {
                            break;
                        }
                        if control.paused.load(Ordering::SeqCst) {
                            s.status = TaskStatus::Paused;
                        } else if matches!(
                            s.status,
                            TaskStatus::Queued | TaskStatus::Paused | TaskStatus::Downloading
                        ) {
                            s.status = TaskStatus::Downloading;
                        }
                        s.downloaded_segments = finished;
                        s.total_segments = total_u64;
                        if speed > 0.0 {
                            s.speed_bps = speed;
                            if finished > initial_done {
                                let avg_bytes = downloaded_bytes.load(Ordering::Relaxed) as f64
                                    / (finished - initial_done) as f64;
                                if avg_bytes > 0.0 {
                                    let remain_bytes =
                                        avg_bytes * total_u64.saturating_sub(finished) as f64;
                                    s.eta_secs = Some((remain_bytes / speed).round() as u64);
                                }
                            }
                        }
                        s.progress = if total_u64 == 0 {
                            0.0
                        } else {
                            finished as f64 / total_u64 as f64 * 100.0
                        };
                        emit_progress(&app, &s);
                        emit_segment_update(&app, &task_id);
                    }
                    Err(e) if is_pause_err(&e) => {
                        let _ = std::fs::remove_file(segment_path(&segments_dir, seg.index));
                        let _ = std::fs::remove_file(
                            segment_path(&segments_dir, seg.index).with_extension("ts.part"),
                        );
                        emit_segment_update(&app, &task_id);
                        // Task paused: loop back and wait_if_paused
                        continue;
                    }
                    Err(e) if is_cancel_err(&e) || control.is_segment_cancelled(seg.index) => {
                        let _ = std::fs::remove_file(segment_path(&segments_dir, seg.index));
                        let _ = std::fs::remove_file(
                            segment_path(&segments_dir, seg.index).with_extension("ts.part"),
                        );
                        emit_segment_update(&app, &task_id);
                        if control.cancelled.load(Ordering::SeqCst) {
                            break;
                        }
                        // segment-level cancel: continue other segments
                    }
                    Err(e) => {
                        control.mark_segment_failed(seg.index, e.to_string());
                        emit_segment_update(&app, &task_id);
                        // continue other segments instead of failing whole task
                    }
                }
            }
        }));
    }

    for h in handles {
        let _ = h.await;
    }

    if ctx.control.cancelled.load(Ordering::SeqCst) {
        return Ok(());
    }

    if ctx.control.paused.load(Ordering::SeqCst) {
        let mut s = ctx.snapshot.lock();
        s.status = TaskStatus::Paused;
        s.speed_bps = 0.0;
        s.eta_secs = None;
        emit_progress(&ctx.app, &s);
        return Ok(());
    }

    let mut missing = 0usize;
    for i in 0..total {
        let p = segment_path(&segments_dir, i);
        if !p.exists() || std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0) == 0 {
            missing += 1;
        }
    }

    if missing > 0 {
        let mut s = ctx.snapshot.lock();
        s.status = TaskStatus::Paused;
        s.speed_bps = 0.0;
        s.error = Some(format!(
            "还有 {missing} 个分片未完成，可在详情中单独开始/重试"
        ));
        emit_progress(&ctx.app, &s);
        return Ok(());
    }

    finish_merge(&ctx, total, &segments_dir).await
}

async fn finish_merge(ctx: &DownloadContext, total: usize, segments_dir: &Path) -> AppResult<()> {
    if ctx.control.cancelled.load(Ordering::SeqCst) {
        return Ok(());
    }

    let mut paths = Vec::with_capacity(total);
    for i in 0..total {
        paths.push(segment_path(segments_dir, i));
    }

    {
        let mut s = ctx.snapshot.lock();
        s.status = TaskStatus::Merging;
        s.progress = 100.0;
        s.speed_bps = 0.0;
        s.error = None;
        emit_progress(&ctx.app, &s);
    }

    merge_segments_to_mp4(&paths, &ctx.output_path, &ctx.work_dir).await?;

    if ctx.control.cancelled.load(Ordering::SeqCst) {
        return Ok(());
    }

    if ctx.cleanup_segments {
        let _ = tokio::fs::remove_dir_all(segments_dir).await;
        let _ = tokio::fs::remove_file(ctx.work_dir.join("concat.txt")).await;
    }

    {
        let mut s = ctx.snapshot.lock();
        s.status = TaskStatus::Completed;
        s.progress = 100.0;
        s.output_path = Some(ctx.output_path.to_string_lossy().to_string());
        s.eta_secs = Some(0);
        s.speed_bps = 0.0;
        emit_progress(&ctx.app, &s);
    }
    Ok(())
}

/// Download a single segment (manual start / retry).
/// Honors task cancel; ignores task pause only while this segment is marked manual_allow.
pub async fn download_segment_manual(
    app: AppHandle,
    task_id: String,
    work_dir: PathBuf,
    opts: RequestOptions,
    control: Arc<TaskControl>,
    snapshot: Arc<Mutex<TaskSnapshot>>,
    index: usize,
) -> AppResult<()> {
    let meta = load_playlist_meta(&work_dir)?;
    let seg = meta
        .segments
        .iter()
        .find(|s| s.index == index)
        .cloned()
        .ok_or_else(|| AppError::msg(format!("分片 {index} 不存在")))?;

    if control.cancelled.load(Ordering::SeqCst) {
        return Err(AppError::msg("Cancelled"));
    }

    control.clear_segment_cancelled(index);
    control.clear_segment_failed(index);
    control.allow_manual(index);
    control.mark_segment_active(index, true);
    emit_segment_update(&app, &task_id);

    let segments_dir = work_dir.join("segments");
    tokio::fs::create_dir_all(&segments_dir).await?;
    // Remove old file for retry
    let _ = tokio::fs::remove_file(segment_path(&segments_dir, index)).await;
    let _ = tokio::fs::remove_file(segment_path(&segments_dir, index).with_extension("ts.part")).await;

    let client = build_client(&opts)?;
    let headers = build_headers(&opts)?;
    let key_cache: Mutex<HashMap<String, Vec<u8>>> = Mutex::new(HashMap::new());

    let result = download_one_segment_inner(
        &client,
        &headers,
        &seg,
        meta.media_sequence,
        &segments_dir,
        &key_cache,
        &control,
        false, // honor_pause=false while manual_allow is set
    )
    .await;

    control.mark_segment_active(index, false);
    control.clear_manual(index);

    match result {
        Ok(_) => {
            if control.cancelled.load(Ordering::SeqCst) || control.is_segment_cancelled(index) {
                let _ = tokio::fs::remove_file(segment_path(&segments_dir, index)).await;
                emit_segment_update(&app, &task_id);
                return Ok(());
            }
            control.clear_segment_failed(index);
            // Recount done
            let total = {
                let s = snapshot.lock();
                s.total_segments.max(meta.segments.len() as u64)
            };
            let mut done = 0u64;
            for i in 0..total as usize {
                let p = segment_path(&segments_dir, i);
                if std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0) > 0 {
                    done += 1;
                }
            }
            {
                let mut s = snapshot.lock();
                s.downloaded_segments = done;
                s.total_segments = total;
                s.progress = if total == 0 {
                    0.0
                } else {
                    done as f64 / total as f64 * 100.0
                };
                if done >= total && total > 0 {
                    s.error = Some("全部分片已齐，点击继续以合并 MP4".into());
                }
                emit_progress(&app, &s);
            }
            emit_segment_update(&app, &task_id);
            Ok(())
        }
        Err(e) if is_stop_err(&e) => {
            let _ = tokio::fs::remove_file(segment_path(&segments_dir, index)).await;
            emit_segment_update(&app, &task_id);
            Ok(())
        }
        Err(e) => {
            control.mark_segment_failed(index, e.to_string());
            emit_segment_update(&app, &task_id);
            Err(e)
        }
    }
}

async fn download_one_segment(
    client: &reqwest::Client,
    headers: &reqwest::header::HeaderMap,
    seg: &SegmentInfo,
    media_sequence: u64,
    segments_dir: &Path,
    key_cache: &Mutex<HashMap<String, Vec<u8>>>,
    control: &TaskControl,
) -> AppResult<u64> {
    download_one_segment_inner(
        client,
        headers,
        seg,
        media_sequence,
        segments_dir,
        key_cache,
        control,
        true,
    )
    .await
}

async fn download_one_segment_inner(
    client: &reqwest::Client,
    headers: &reqwest::header::HeaderMap,
    seg: &SegmentInfo,
    media_sequence: u64,
    segments_dir: &Path,
    key_cache: &Mutex<HashMap<String, Vec<u8>>>,
    control: &TaskControl,
    honor_pause: bool,
) -> AppResult<u64> {
    let mut last_err = None;
    for attempt in 0..5u32 {
        if let Some(reason) = control.should_stop(seg.index, honor_pause) {
            return Err(AppError::msg(reason));
        }
        if honor_pause && !wait_if_paused(control).await {
            return Err(AppError::msg("Cancelled"));
        }
        if let Some(reason) = control.should_stop(seg.index, honor_pause) {
            return Err(AppError::msg(reason));
        }

        let epoch = control.current_epoch();
        let attempt_fut = download_attempt(
            client,
            headers,
            seg,
            media_sequence,
            segments_dir,
            key_cache,
        );

        let result = tokio::select! {
            r = attempt_fut => r,
            _ = control.wait_epoch_change(epoch) => {
                Err(AppError::msg(
                    control
                        .should_stop(seg.index, honor_pause)
                        .unwrap_or("SegmentCancelled"),
                ))
            }
        };

        match result {
            Ok(n) => {
                if let Some(reason) = control.should_stop(seg.index, honor_pause) {
                    let _ = std::fs::remove_file(segment_path(segments_dir, seg.index));
                    return Err(AppError::msg(reason));
                }
                return Ok(n);
            }
            Err(e) if is_stop_err(&e) => {
                return Err(e);
            }
            Err(e) => {
                if let Some(reason) = control.should_stop(seg.index, honor_pause) {
                    return Err(AppError::msg(reason));
                }
                last_err = Some(e);
                let backoff = 200u64 * 2u64.pow(attempt);
                let epoch = control.current_epoch();
                tokio::select! {
                    _ = tokio::time::sleep(tokio::time::Duration::from_millis(backoff)) => {}
                    _ = control.wait_epoch_change(epoch) => {
                        return Err(AppError::msg(
                            control
                                .should_stop(seg.index, honor_pause)
                                .unwrap_or("SegmentCancelled"),
                        ));
                    }
                }
            }
        }
    }
    Err(last_err.unwrap_or_else(|| AppError::msg("Segment download failed")))
}

async fn download_attempt(
    client: &reqwest::Client,
    headers: &reqwest::header::HeaderMap,
    seg: &SegmentInfo,
    media_sequence: u64,
    segments_dir: &Path,
    key_cache: &Mutex<HashMap<String, Vec<u8>>>,
) -> AppResult<u64> {
    let mut data = fetch_bytes(client, &seg.url, headers, seg.byte_range).await?;
    if let Some(key_info) = &seg.key {
        if key_info.method.eq_ignore_ascii_case("AES-128") {
            let key = load_key_cache(client, headers, key_info, key_cache).await?;
            let iv = parse_iv(
                key_info.iv.as_deref(),
                media_sequence,
                seg.index as u64,
            )?;
            data = decrypt_aes128(&data, &key, &iv)?;
        } else if !key_info.method.eq_ignore_ascii_case("NONE") {
            return Err(AppError::msg(format!(
                "Unsupported encryption method: {}",
                key_info.method
            )));
        }
    }
    let path = segment_path(segments_dir, seg.index);
    let tmp = path.with_extension("ts.part");
    tokio::fs::write(&tmp, &data).await?;
    tokio::fs::rename(&tmp, &path).await?;
    Ok(data.len() as u64)
}

// silence unused import warning if HashSet only used via TaskControl
#[allow(dead_code)]
fn _hashset_marker() -> HashSet<usize> {
    HashSet::new()
}
