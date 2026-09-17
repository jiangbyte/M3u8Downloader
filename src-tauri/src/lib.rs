mod commands;
mod crypto;
mod download;
mod error;
mod http;
mod m3u8;
mod merge;
mod play;
mod task;

use std::sync::Arc;
use tauri::Manager;
use play::PlayService;
use task::TaskManager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            let data_dir = app
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from(".").join("data"));
            std::fs::create_dir_all(&data_dir).ok();
            let manager = Arc::new(TaskManager::new(data_dir));
            app.manage(manager);
            app.manage(Arc::new(PlayService::new()));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::analyze_url,
            commands::start_task,
            commands::list_tasks,
            commands::get_task_detail,
            commands::pause_task,
            commands::resume_task,
            commands::cancel_task,
            commands::remove_task,
            commands::start_segment,
            commands::cancel_segment,
            commands::retry_segment,
            commands::play_task,
            commands::open_path,
            commands::default_output_dir,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
