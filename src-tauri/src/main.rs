//! Desktop process that hosts the Message Vault window.
//!
//! The Vite UI in `web/` runs inside a WebView (a browser-like window). A web
//! page cannot read local phone backups, run the Rust exporters, open native
//! file dialogs, or start ffmpeg. This process is the native host: it owns
//! the window, talks to the operating system, and exposes those jobs as
//! commands the UI can call.

// On Windows release builds, hide the extra console window so only the app
// window appears.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod state;

use state::AppState;
use std::sync::{Arc, Mutex};

/// Start the desktop window and wait until the user quits.
fn main() {
    // Import and export name this Build to the vault on every request; the SPA
    // does the same for its own requests (`web/src/lib/api.ts`).
    vault_http::identify_desktop_app(env!("MESSAGE_VAULT_BUILD"));

    let app_state = Arc::new(Mutex::new(AppState::new()));

    // Native open/save dialogs. A WebView page cannot show the OS file picker.
    let dialog_plugin = tauri_plugin_dialog::init();
    // Open files and links with the OS default handler.
    let shell_plugin = tauri_plugin_shell::init();

    let builder = tauri::Builder::default()
        .plugin(dialog_plugin)
        .plugin(shell_plugin)
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            commands::extract::extract,
            commands::extract::cancel,
            commands::ffmpeg::probe_ffmpeg_tools,
            commands::ffmpeg::set_ffmpeg_tools_dir,
            commands::format::format,
            commands::paths::home_dir,
            commands::paths::path_stat,
            commands::paths::ios_backup_encrypted,
            commands::paths::imessage_backup_identities,
            commands::paths::open_path,
            commands::push::push,
            commands::pull::pull,
            commands::staging::summarize_staging,
            commands::staging::transcode_staging,
            commands::staging::delete_staging,
        ]);

    builder
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
