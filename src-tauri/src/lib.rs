// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
use tauri_plugin_log::{log};

#[tauri::command]
fn start_screen_share() -> String {
    log::info!("Starting screen sharing session...");
    "ABC123".to_string()
}

#[tauri::command]
fn join_screen_share(sessionId: String) -> String {
    log::info!("Joining session...");
    format!("Joining session {}", sessionId)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(
            tauri_plugin_log::Builder::new()
                .target(tauri_plugin_log::Target::new(
                    tauri_plugin_log::TargetKind::Stdout,
                ))
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            join_screen_share,
            start_screen_share
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
