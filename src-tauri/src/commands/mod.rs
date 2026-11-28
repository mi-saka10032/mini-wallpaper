pub mod action;
pub mod app_setting;
pub mod backup;
pub mod collection;
pub mod error;
pub mod fullscreen;
pub mod monitor_config;
pub mod scene;
pub mod tag;
pub mod toast;
pub mod wallpaper;
pub mod watched_folder;

// 重导出所有 command 函数，便于 rust-analyzer 提供跳转支持（IDE 导航用途）
#[allow(unused_imports)]
pub use action::trigger_action;
#[allow(unused_imports)]
pub use app_setting::{get_setting, get_settings, set_setting};
#[allow(unused_imports)]
pub use backup::{export_backup, get_data_size, import_backup};
#[allow(unused_imports)]
pub use collection::{
    add_wallpapers_to_collection, create_collection, create_smart_collection, delete_collection,
    get_collection_wallpapers, get_collections, preview_smart_count,
    remove_wallpapers_from_collection, rename_collection, reorder_collection_wallpapers,
    reorder_collections, toggle_favorite, update_smart_collection,
};
#[allow(unused_imports)]
pub use fullscreen::init_fullscreen_detection;
#[allow(unused_imports)]
pub use monitor_config::{
    delete_monitor_config, get_monitor_config, get_monitor_configs, start_timers,
    upsert_monitor_config,
};
#[allow(unused_imports)]
pub use scene::{
    apply_scene, delete_scene, get_current_scene_state, list_scenes, rename_scene, save_scene,
};
#[allow(unused_imports)]
pub use tag::{
    delete_tag, get_tags, get_wallpaper_tags, get_wallpapers_by_tags, rename_tag,
    set_wallpaper_tags, tag_wallpapers, untag_wallpapers,
};
#[allow(unused_imports)]
pub use toast::close_toast_window;
#[allow(unused_imports)]
pub use wallpaper::{
    delete_wallpapers, empty_trash, get_supported_extensions, get_trashed_wallpapers,
    get_wallpaper, get_wallpapers, import_wallpaper_bytes, import_wallpapers, purge_wallpapers,
    restore_wallpapers, save_video_thumbnail,
};
#[allow(unused_imports)]
pub use watched_folder::{
    add_watched_folder, delete_watched_wallpapers, list_watched_folders, remove_watched_folder,
    set_active_view, sync_watched_folder,
};
/// 聚合所有 command，供 lib.rs 一行调用
macro_rules! all_handlers {
    () => {
        tauri::generate_handler![
            // wallpaper
            $crate::commands::wallpaper::get_supported_extensions,
            $crate::commands::wallpaper::get_wallpapers,
            $crate::commands::wallpaper::get_wallpaper,
            $crate::commands::wallpaper::import_wallpapers,
            $crate::commands::wallpaper::import_wallpaper_bytes,
            $crate::commands::wallpaper::save_video_thumbnail,
            $crate::commands::wallpaper::delete_wallpapers,
            $crate::commands::wallpaper::get_trashed_wallpapers,
            $crate::commands::wallpaper::restore_wallpapers,
            $crate::commands::wallpaper::purge_wallpapers,
            $crate::commands::wallpaper::empty_trash,
            // collection
            $crate::commands::collection::get_collections,
            $crate::commands::collection::create_collection,
            $crate::commands::collection::rename_collection,
            $crate::commands::collection::delete_collection,
            $crate::commands::collection::get_collection_wallpapers,
            $crate::commands::collection::add_wallpapers_to_collection,
            $crate::commands::collection::remove_wallpapers_from_collection,
            $crate::commands::collection::reorder_collection_wallpapers,
            $crate::commands::collection::reorder_collections,
            $crate::commands::collection::toggle_favorite,
            $crate::commands::collection::create_smart_collection,
            $crate::commands::collection::update_smart_collection,
            $crate::commands::collection::preview_smart_count,
            // monitor_config
            $crate::commands::monitor_config::get_monitor_configs,
            $crate::commands::monitor_config::get_monitor_config,
            $crate::commands::monitor_config::upsert_monitor_config,
            $crate::commands::monitor_config::delete_monitor_config,
            $crate::commands::monitor_config::start_timers,
            $crate::commands::monitor_config::get_extend_viewport,
            // app_setting
            $crate::commands::app_setting::get_settings,
            $crate::commands::app_setting::get_setting,
            $crate::commands::app_setting::set_setting,
            // action
            $crate::commands::action::trigger_action,
            // backup
            $crate::commands::backup::export_backup,
            $crate::commands::backup::import_backup,
            $crate::commands::backup::get_data_size,
            // fullscreen
            $crate::commands::fullscreen::init_fullscreen_detection,
            // toast
            $crate::commands::toast::close_toast_window,
            // tag
            $crate::commands::tag::get_tags,
            $crate::commands::tag::get_wallpaper_tags,
            $crate::commands::tag::get_wallpapers_by_tags,
            $crate::commands::tag::tag_wallpapers,
            $crate::commands::tag::untag_wallpapers,
            $crate::commands::tag::set_wallpaper_tags,
            $crate::commands::tag::rename_tag,
            $crate::commands::tag::delete_tag,
            // scene
            $crate::commands::scene::get_current_scene_state,
            $crate::commands::scene::list_scenes,
            $crate::commands::scene::save_scene,
            $crate::commands::scene::rename_scene,
            $crate::commands::scene::delete_scene,
            $crate::commands::scene::apply_scene,
            // watched folder
            $crate::commands::watched_folder::list_watched_folders,
            $crate::commands::watched_folder::add_watched_folder,
            $crate::commands::watched_folder::remove_watched_folder,
            $crate::commands::watched_folder::sync_watched_folder,
            $crate::commands::watched_folder::set_active_view,
            $crate::commands::watched_folder::delete_watched_wallpapers,
        ]
    };
}
pub(crate) use all_handlers;
