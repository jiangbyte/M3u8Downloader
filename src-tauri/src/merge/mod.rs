use crate::error::{AppError, AppResult};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command;

/// Resolve bundled ffmpeg first (installer sidecar), then PATH fallback for dev.
pub fn resolve_ffmpeg() -> AppResult<PathBuf> {
    if let Some(p) = find_bundled_ffmpeg() {
        return Ok(p);
    }
    which_ffmpeg().ok_or_else(|| {
        AppError::msg(
            "未找到 ffmpeg。安装版应已内置；开发环境请安装系统 ffmpeg，或运行 node scripts/prepare-ffmpeg.mjs 后重新打包",
        )
    })
}

fn find_bundled_ffmpeg() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let mut dirs = Vec::new();
    if let Some(dir) = exe.parent() {
        dirs.push(dir.to_path_buf());
        // macOS .app: Contents/MacOS → Contents/Resources / Contents
        if let Some(contents) = dir.parent() {
            dirs.push(contents.join("Resources"));
            dirs.push(contents.to_path_buf());
            if let Some(app_root) = contents.parent() {
                dirs.push(app_root.to_path_buf());
            }
        }
        dirs.push(dir.join("binaries"));
    }

    let names = [
        "ffmpeg",
        "ffmpeg.exe",
        // Keep triple-suffixed names for unpackaged / side-by-side layouts
        "ffmpeg-x86_64-unknown-linux-gnu",
        "ffmpeg-aarch64-unknown-linux-gnu",
        "ffmpeg-x86_64-apple-darwin",
        "ffmpeg-aarch64-apple-darwin",
        "ffmpeg-x86_64-pc-windows-msvc.exe",
    ];

    for dir in &dirs {
        for name in names {
            let candidate = dir.join(name);
            if is_executable_file(&candidate) {
                return Some(candidate);
            }
        }
        if let Ok(rd) = std::fs::read_dir(dir) {
            for entry in rd.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name == "ffmpeg"
                    || name == "ffmpeg.exe"
                    || (name.starts_with("ffmpeg-") && !name.contains("ffprobe"))
                {
                    let p = entry.path();
                    if is_executable_file(&p) {
                        return Some(p);
                    }
                }
            }
        }
    }
    None
}

fn is_executable_file(path: &Path) -> bool {
    match std::fs::metadata(path) {
        Ok(m) if m.is_file() && m.len() > 0 => true,
        _ => false,
    }
}

fn which_ffmpeg() -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        for name in ["ffmpeg", "ffmpeg.exe"] {
            let p = dir.join(name);
            if is_executable_file(&p) {
                return Some(p);
            }
        }
    }
    None
}

pub async fn merge_segments_to_mp4(
    segment_paths: &[PathBuf],
    output: &Path,
    work_dir: &Path,
) -> AppResult<()> {
    if segment_paths.is_empty() {
        return Err(AppError::msg("No segments to merge"));
    }
    let ffmpeg = resolve_ffmpeg()?;
    let list_path = work_dir.join("concat.txt");
    let mut list_body = String::new();
    for p in segment_paths {
        // ffmpeg concat demuxer needs escaped single quotes
        let path_str = p.to_string_lossy().replace('\'', "'\\''");
        list_body.push_str(&format!("file '{path_str}'\n"));
    }
    tokio::fs::write(&list_path, list_body).await?;

    if let Some(parent) = output.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    if output.exists() {
        tokio::fs::remove_file(output).await.ok();
    }

    let status = Command::new(&ffmpeg)
        .args([
            "-y",
            "-f",
            "concat",
            "-safe",
            "0",
            "-i",
            list_path.to_str().unwrap_or("concat.txt"),
            "-c",
            "copy",
            "-bsf:a",
            "aac_adtstoasc",
            output
                .to_str()
                .ok_or_else(|| AppError::msg("Invalid output path"))?,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .status()
        .await
        .map_err(|e| AppError::msg(format!("Failed to spawn ffmpeg: {e}")))?;

    if !status.success() {
        // Retry without aac bitstream filter (some streams don't need it / fail with it)
        let status2 = Command::new(&ffmpeg)
            .args([
                "-y",
                "-f",
                "concat",
                "-safe",
                "0",
                "-i",
                list_path.to_str().unwrap_or("concat.txt"),
                "-c",
                "copy",
                output
                    .to_str()
                    .ok_or_else(|| AppError::msg("Invalid output path"))?,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .status()
            .await
            .map_err(|e| AppError::msg(format!("Failed to spawn ffmpeg: {e}")))?;
        if !status2.success() {
            return Err(AppError::msg(format!(
                "ffmpeg merge failed with status {status2}"
            )));
        }
    }
    Ok(())
}
