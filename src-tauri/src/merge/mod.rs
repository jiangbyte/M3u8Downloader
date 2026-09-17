use crate::error::{AppError, AppResult};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command;

/// Resolve ffmpeg binary: bundled sidecar near the executable, then PATH.
pub fn resolve_ffmpeg() -> AppResult<PathBuf> {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for name in ["ffmpeg", "ffmpeg.exe"] {
                let candidate = dir.join(name);
                if candidate.is_file() {
                    return Ok(candidate);
                }
            }
            // Tauri sidecar naming: binary-name-target-triple
            if let Ok(rd) = std::fs::read_dir(dir) {
                for entry in rd.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if name.starts_with("ffmpeg") {
                        return Ok(entry.path());
                    }
                }
            }
            let binaries = dir.join("binaries");
            if binaries.is_dir() {
                for name in ["ffmpeg", "ffmpeg.exe"] {
                    let candidate = binaries.join(name);
                    if candidate.is_file() {
                        return Ok(candidate);
                    }
                }
            }
        }
    }

    which_ffmpeg().ok_or_else(|| {
        AppError::msg(
            "ffmpeg not found. Install ffmpeg or place a binary next to the app / in src-tauri/binaries/",
        )
    })
}

fn which_ffmpeg() -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        for name in ["ffmpeg", "ffmpeg.exe"] {
            let p = dir.join(name);
            if p.is_file() {
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
            output.to_str().ok_or_else(|| AppError::msg("Invalid output path"))?,
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
