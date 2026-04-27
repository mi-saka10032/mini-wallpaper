pub mod app_setting;
pub mod backup;
pub mod collection;
pub mod fullscreen;
pub mod monitor_config;
pub mod shortcut;
pub mod wallpaper;
pub mod wallpaper_window;

/// 聚合所有 command，供 lib.rs 一行调用
macro_rules! all_handlers {
    () => {
        tauri::generate_handler![
            // wallpaper
            $crate::commands::wallpaper::get_wallpapers,
            $crate::commands::wallpaper::import_wallpapers,
            $crate::commands::wallpaper::delete_wallpapers,
            // collection
            $crate::commands::collection::get_collections,
            $crate::commands::collection::create_collection,
            $crate::commands::collection::rename_collection,
            $crate::commands::collection::delete_collection,
            $crate::commands::collection::get_collection_wallpapers,
            $crate::commands::collection::add_wallpapers_to_collection,
            $crate::commands::collection::remove_wallpapers_from_collection,
            $crate::commands::collection::reorder_collection_wallpapers,
            // monitor_config
            $crate::commands::monitor_config::get_monitor_configs,
            $crate::commands::monitor_config::get_monitor_config,
            $crate::commands::monitor_config::upsert_monitor_config,
            $crate::commands::monitor_config::delete_monitor_config,
            $crate::commands::monitor_config::start_timers,
            // app_setting
            $crate::commands::app_setting::get_settings,
            $crate::commands::app_setting::get_setting,
            $crate::commands::app_setting::set_setting,
            // shortcut
            $crate::commands::shortcut::switch_wallpaper,
            // backup
            $crate::commands::backup::export_backup,
            $crate::commands::backup::import_backup,
            $crate::commands::backup::get_data_size,
            // fullscreen
            $crate::commands::fullscreen::init_fullscreen_detection,
            // wallpaper_window
            $crate::commands::wallpaper_window::create_wallpaper_window,
            $crate::commands::wallpaper_window::destroy_wallpaper_window,
            $crate::commands::wallpaper_window::destroy_all_wallpaper_windows,
            $crate::commands::wallpaper_window::hide_wallpaper_windows,
            $crate::commands::wallpaper_window::show_wallpaper_windows,
        ]
    };
}
pub(crate) use all_handlers;