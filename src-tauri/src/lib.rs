//! Native desktop host for the Message Crate UI.
//!
//! The screens in `web/` are a Vite app. In the desktop build they run inside
//! a WebView, which is a browser-like window. A web page cannot read local
//! phone backups, run the Rust exporters, open native file dialogs, or start
//! ffmpeg. Those jobs need a program that talks to the operating system.
//!
//! This crate is that program. It owns the window, loads the UI, and exposes
//! commands the UI can call (`extract`, `format`, `push`, `pull`, and the
//! ffmpeg helpers). It also starts the Message Crate server it ships with,
//! when nothing answers at the app's own address (`local_server`). Progress and errors go back to the UI as Tauri events.
//!
//! Tauri also requires a library target (`cdylib` / `staticlib`), so this
//! file exists alongside `main.rs`. Both declare the same modules.

pub mod commands;
pub mod local_server;
pub mod staging_folders;
pub mod state;
