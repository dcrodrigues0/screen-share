// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
use tauri::{
    utils::config::WebviewUrl,
    webview::{NewWindowResponse, WebviewWindowBuilder},
};
use tauri_plugin_log::log;

//#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(
            tauri_plugin_log::Builder::new()
                .target(tauri_plugin_log::Target::new(
                    tauri_plugin_log::TargetKind::Stdout,
                ))
                .build(),
        )
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
