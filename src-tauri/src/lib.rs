pub mod budget;
pub mod commands;
pub mod energy;
pub mod errors;
pub mod knowledge;
pub mod logging;
pub mod models;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // app_data 目录就绪后初始化文件日志
            use tauri::Manager;
            if let Ok(dir) = app.path().app_data_dir() {
                let _ = logging::init(&dir);
                // 知识库覆盖预热 + prompt 覆盖目录记录（失败回退嵌入版，不阻断启动）
                crate::knowledge::warm_knowledge(&dir);
                crate::commands::prompts::set_prompt_override_dir(dir);
                tracing::info!(
                    kb_source = crate::knowledge::knowledge_source(),
                    "知识库来源就绪"
                );
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::orchestrator::pipeline_generate,
            commands::orchestrator::pipeline_refine,
            commands::llm::test_api,
            commands::orchestrator::cancel_pipeline,
            commands::orchestrator::interject_feedback,
            commands::orchestrator::get_pipeline_meta,
            commands::keychain::keychain_set,
            commands::keychain::keychain_get,
            commands::keychain::keychain_delete,
            commands::history::history_load,
            commands::history::history_save,
            commands::history::history_export_text,
            commands::logdir::open_log_dir,
            commands::logdir::open_config_dir,
            commands::orchestrator::pipeline_resume
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
