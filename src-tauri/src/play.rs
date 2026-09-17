use crate::download::{load_playlist_meta, segment_path};
use crate::error::{AppError, AppResult};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::oneshot;

/// Serves per-task local HLS so players can watch while segments keep arriving.
pub struct PlayService {
    state: Mutex<Option<Arc<PlayState>>>,
}

struct PlayState {
    port: u16,
    mounts: Mutex<HashMap<String, PathBuf>>,
}

impl PlayService {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(None),
        }
    }

    pub async fn play_url(&self, task_id: &str, work_dir: PathBuf) -> AppResult<String> {
        let state = self.ensure_server().await?;
        state
            .mounts
            .lock()
            .insert(task_id.to_string(), work_dir);
        Ok(format!(
            "http://127.0.0.1:{}/tasks/{}/index.m3u8",
            state.port, task_id
        ))
    }

    pub fn unmount(&self, task_id: &str) {
        if let Some(state) = self.state.lock().as_ref() {
            state.mounts.lock().remove(task_id);
        }
    }

    async fn ensure_server(&self) -> AppResult<Arc<PlayState>> {
        if let Some(s) = self.state.lock().as_ref() {
            return Ok(s.clone());
        }

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|e| AppError::msg(format!("无法启动边下边播服务: {e}")))?;
        let port = listener
            .local_addr()
            .map_err(|e| AppError::msg(e.to_string()))?
            .port();

        let state = Arc::new(PlayState {
            port,
            mounts: Mutex::new(HashMap::new()),
        });

        let serve_state = state.clone();
        let (ready_tx, ready_rx) = oneshot::channel::<()>();
        tauri::async_runtime::spawn(async move {
            let _ = ready_tx.send(());
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                let state = serve_state.clone();
                tauri::async_runtime::spawn(async move {
                    let mut buf = vec![0u8; 8192];
                    let n = match socket.read(&mut buf).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => n,
                    };
                    let req = String::from_utf8_lossy(&buf[..n]);
                    let path = parse_request_path(&req);
                    let response = handle_request(&state, &path);
                    let _ = socket.write_all(&response).await;
                    let _ = socket.shutdown().await;
                });
            }
        });
        let _ = ready_rx.await;

        *self.state.lock() = Some(state.clone());
        Ok(state)
    }
}

fn parse_request_path(req: &str) -> String {
    let line = req.lines().next().unwrap_or("");
    let mut parts = line.split_whitespace();
    let _method = parts.next();
    let path = parts.next().unwrap_or("/");
    path.split('?').next().unwrap_or("/").to_string()
}

fn handle_request(state: &PlayState, path: &str) -> Vec<u8> {
    // /tasks/{id}/index.m3u8
    // /tasks/{id}/segments/000000.ts
    let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    if parts.len() < 3 || parts[0] != "tasks" {
        return http_response(404, "text/plain; charset=utf-8", b"Not Found");
    }
    let task_id = parts[1];
    let work_dir = {
        let mounts = state.mounts.lock();
        match mounts.get(task_id) {
            Some(p) => p.clone(),
            None => {
                return http_response(404, "text/plain; charset=utf-8", b"Task not mounted");
            }
        }
    };

    if parts.len() == 3 && parts[2] == "index.m3u8" {
        match build_playlist_body(&work_dir) {
            Ok(body) => {
                return http_response(200, "application/vnd.apple.mpegurl", body.as_bytes());
            }
            Err(e) => {
                return http_response(
                    404,
                    "text/plain; charset=utf-8",
                    e.to_string().as_bytes(),
                );
            }
        }
    }

    if parts.len() == 4 && parts[2] == "segments" {
        let name = parts[3];
        if !is_safe_segment_name(name) {
            return http_response(400, "text/plain; charset=utf-8", b"Bad segment name");
        }
        let file = work_dir.join("segments").join(name);
        match std::fs::read(&file) {
            Ok(data) if !data.is_empty() => {
                return http_response(200, "video/mp2t", &data);
            }
            _ => return http_response(404, "text/plain; charset=utf-8", b"Segment missing"),
        }
    }

    http_response(404, "text/plain; charset=utf-8", b"Not Found")
}

fn is_safe_segment_name(name: &str) -> bool {
    name.len() <= 32
        && name.ends_with(".ts")
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
}

/// Contiguous finished segments from index 0 for progressive playback.
pub fn build_playlist_body(work_dir: &Path) -> AppResult<String> {
    let meta = load_playlist_meta(work_dir)?;
    let segments_dir = work_dir.join("segments");
    let mut ready: Vec<&crate::m3u8::SegmentInfo> = Vec::new();
    for seg in &meta.segments {
        let path = segment_path(&segments_dir, seg.index);
        let ok = std::fs::metadata(&path)
            .map(|m| m.len() > 0)
            .unwrap_or(false);
        if !ok {
            break;
        }
        ready.push(seg);
    }
    if ready.is_empty() {
        return Err(AppError::msg("暂无连续可播放分片，请先下载开头几个分片"));
    }

    let target = ready
        .iter()
        .map(|s| s.duration.ceil() as u32)
        .max()
        .unwrap_or(10)
        .max(1);
    let complete = ready.len() == meta.segments.len();

    let mut body = String::new();
    body.push_str("#EXTM3U\n");
    body.push_str("#EXT-X-VERSION:3\n");
    body.push_str(&format!("#EXT-X-TARGETDURATION:{target}\n"));
    body.push_str("#EXT-X-MEDIA-SEQUENCE:0\n");
    if !complete {
        body.push_str("#EXT-X-PLAYLIST-TYPE:EVENT\n");
    }
    for seg in &ready {
        body.push_str(&format!("#EXTINF:{:.3},\n", seg.duration));
        body.push_str(&format!("segments/{:06}.ts\n", seg.index));
    }
    if complete {
        body.push_str("#EXT-X-ENDLIST\n");
    }
    Ok(body)
}

fn http_response(status: u16, content_type: &str, body: &[u8]) -> Vec<u8> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        _ => "Error",
    };
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\nCache-Control: no-cache\r\n\r\n",
        body.len()
    );
    let mut out = header.into_bytes();
    out.extend_from_slice(body);
    out
}

pub fn open_media(target: &str) -> AppResult<()> {
    if let Some(player) = find_player() {
        std::process::Command::new(&player)
            .arg(target)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| AppError::msg(format!("无法启动播放器 {}: {e}", player.display())))?;
        return Ok(());
    }
    open_with_system(target)
}

fn find_player() -> Option<PathBuf> {
    let names = ["mpv", "ffplay", "vlc", "mpv.exe", "ffplay.exe", "vlc.exe"];
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        for name in names {
            let p = dir.join(name);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    // ffplay next to ffmpeg sidecar
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for name in ["ffplay", "ffplay.exe"] {
                let p = dir.join(name);
                if p.is_file() {
                    return Some(p);
                }
            }
        }
    }
    None
}

fn open_with_system(target: &str) -> AppResult<()> {
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(target)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| AppError::msg(format!("xdg-open 失败: {e}")))?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(target)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| AppError::msg(format!("open 失败: {e}")))?;
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", target])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| AppError::msg(format!("start 失败: {e}")))?;
    }
    Ok(())
}
