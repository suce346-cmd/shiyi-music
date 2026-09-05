mod budget;
mod commands;
mod energy;
mod errors;
mod knowledge;
mod logging;
mod models;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // A6：app_data 目录就绪后初始化文件日志
            use tauri::Manager;
            if let Ok(dir) = app.path().app_data_dir() {
                let _ = logging::init(&dir);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::orchestrator::pipeline_generate,
            commands::orchestrator::pipeline_refine,
            commands::llm::test_api,
            commands::orchestrator::cancel_pipeline,
            commands::orchestrator::interject_feedback,
            commands::keychain::keychain_set,
            commands::keychain::keychain_get,
            commands::keychain::keychain_delete,
            commands::history::history_load,
            commands::history::history_save,
            commands::history::history_export_text,
            commands::logdir::open_log_dir
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
