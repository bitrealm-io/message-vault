//! Commands for the Message Crate this app starts for itself
//! (`crate::local_server`).

use crate::local_server::{self, Launch, LocalServer, Status};
use std::path::PathBuf;
use tauri::{AppHandle, Manager, State};

/// The folder inside the installed app that holds the built website. It
/// matches the `bundle.resources` target in `tauri.conf.json`.
const WEBSITE_RESOURCE: &str = "website";

/// The server's data folder for this app.
fn data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Could not find the app's data folder: {e}"))?;
    Ok(local_server::data_dir_in(&app_data))
}

/// The origin `cargo tauri dev` loads the screens from, which the server has
/// to be told to allow. An installed app loads them from its own origin,
/// which every server allows, and names nothing here.
fn dev_origins(app: &AppHandle) -> Vec<String> {
    if !tauri::is_dev() {
        return Vec::new();
    }
    app.config()
        .build
        .dev_url
        .iter()
        .map(|url| url.origin().ascii_serialization())
        .collect()
}

/// Everything the server is started with.
fn launch(app: &AppHandle) -> Result<Launch, String> {
    let static_dir = app
        .path()
        .resource_dir()
        .map_err(|e| format!("Could not find the app's resources: {e}"))?
        .join(WEBSITE_RESOURCE);
    Ok(Launch {
        program: local_server::locate_server()?,
        data_dir: data_dir(app)?,
        static_dir,
        address: local_server::OWN_ADDRESS
            .parse()
            .map_err(|e| format!("{}: {e}", local_server::OWN_ADDRESS))?,
        cors_origins: dev_origins(app),
    })
}

/// Make sure the app's own Message Crate is running, and report its state.
/// The screens call this when the server address is the app's own; a start
/// already under way, or a Message Crate already answering, is left alone.
///
/// # Errors
///
/// Returns an error when the app cannot work out what to start: its data
/// folder, its resources, or the server program is missing.
#[tauri::command]
pub fn start_local_server(
    app: AppHandle,
    server: State<'_, LocalServer>,
) -> Result<Status, String> {
    server.ensure_started(launch(&app)?);
    Ok(server.status())
}

/// Report the state of the app's own Message Crate without starting it.
#[tauri::command]
pub fn local_server_status(server: State<'_, LocalServer>) -> Status {
    server.status()
}

/// Open the server's data folder in the file manager, creating it first so
/// the button works before the first start has finished.
///
/// # Errors
///
/// Returns an error when the folder cannot be found, created, or opened.
#[tauri::command]
pub fn open_data_folder(app: AppHandle) -> Result<(), String> {
    let dir = data_dir(&app)?;
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
    open::that_detached(&dir).map_err(|e| format!("Could not open {}: {e}", dir.display()))
}
