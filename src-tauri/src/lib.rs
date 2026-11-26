mod application;
mod commands;
mod context;
mod dto;
mod entities;
pub mod events;
mod migration;
mod platform;
mod services;
mod setup;
mod tasks;
mod utils;

use setup::{focus_existing_window, AUTOSTART_ARG};

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![AUTOSTART_ARG]),
        ))
        .plugin(tauri_plugin_single_instance::init(focus_existing_window))
        .plugin(crate::platform::global_shortcut::build_plugin())
        .setup(setup::init)
        .invoke_handler(commands::all_handlers!())
        .run(tauri::generate_context!())
        .expect("error while running Mini Wallpaper");
}
