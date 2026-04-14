mod app;
mod config;
mod models;
mod providers;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    app::run();
}
