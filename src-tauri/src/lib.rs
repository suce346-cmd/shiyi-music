mod budget;
mod commands;
mod energy;
mod errors;
mod knowledge;
mod models;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            commands::orchestrator::pipeline_generate,
            commands::orchestrator::pipeline_refine,
            commands::llm::test_api,
            commands::orchestrator::cancel_pipeline
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
