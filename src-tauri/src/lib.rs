mod commands;
mod discord_rpc;
mod downloader;
mod game_state;
#[cfg(windows)]
mod process_guard;
mod rpc_data;

use tauri::Manager;
use tauri::window::Color;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_http::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            let _ = commands::get_hwid();
            let _ = commands::get_hidden_hwid();

            if let Some(window) = app.get_webview_window("main") {
                // Force WebView2 background to #09090b before any content loads
                let _ = window.set_background_color(Some(Color(9, 9, 11, 255)));

                let icon_bytes = include_bytes!("../icons/icon.png");
                let icon = tauri::image::Image::from_bytes(icon_bytes)
                    .expect("failed to load icon");
                let _ = window.set_icon(icon);
            }

            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::show_window,
            commands::fetch_url,
            commands::fetch_server_list,
            commands::ping_server,
            commands::launch_game,
            commands::check_file_exists,
            commands::check_process_running,
            commands::remove_server_mods,
            commands::read_settings_file,
            commands::write_settings_file,
            commands::hash_file_md5,
            commands::hash_string_md5,
            commands::list_game_files,
            commands::pick_directory,
            commands::get_system_info,
            commands::grant_folder_permissions,
            commands::check_firewall_api,
            commands::check_firewall_rules,
            commands::add_firewall_rules,
            commands::remove_firewall_rules,
            commands::check_defender_api,
            commands::check_defender_exclusions,
            commands::add_defender_exclusions,
            commands::remove_defender_exclusions,
            commands::check_folder_permissions,
            commands::fix_folder_permissions,
            commands::set_game_language,
            commands::get_game_language,
            commands::get_hwid_info,
            commands::get_game_settings,
            commands::set_game_settings,
            commands::test_xml_access,
            commands::check_for_updates,
            commands::download_update,
            commands::install_update,
            commands::check_dxvk,
            commands::install_dxvk,
            commands::remove_dxvk,
            downloader::download_game,
            downloader::verify_game_files,
            downloader::repair_game_files,
            downloader::download_modnet_modules,
            downloader::fetch_mod_info,
            downloader::download_mods,
            downloader::clean_mods,
            downloader::fetch_cdn_list,
            discord_rpc::discord_rpc_init,
            discord_rpc::discord_rpc_update,
            discord_rpc::discord_rpc_reconnect,
            discord_rpc::discord_rpc_stop,
        ])
        .on_window_event(|_window, event| {
            if let tauri::WindowEvent::Destroyed = event {
                let _ = discord_rpc::discord_rpc_stop();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
