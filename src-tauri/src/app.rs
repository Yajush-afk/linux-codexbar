use crate::config;
use crate::models::{AppConfig, ProviderSnapshot, ProviderState, UsageWindowSnapshot};
use crate::providers::{claude, codex, opencode};
use chrono::{DateTime, Local};
use std::fs;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::path::PathBuf;
use std::time::Duration;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{Manager, Wry};

#[derive(Clone)]
struct ProviderMenuHandles {
    header: MenuItem<Wry>,
    line_one: MenuItem<Wry>,
    line_two: MenuItem<Wry>,
    line_three: MenuItem<Wry>,
}

#[derive(Clone)]
struct TrayHandles {
    refresh_state: MenuItem<Wry>,
    config_hint: MenuItem<Wry>,
    opencode: ProviderMenuHandles,
    codex: ProviderMenuHandles,
    claude: ProviderMenuHandles,
}

struct AppRuntime {
    tray: TrayHandles,
    config: Mutex<AppConfig>,
    refreshing: AtomicBool,
}

impl AppRuntime {
    fn new(tray: TrayHandles, config: AppConfig) -> Arc<Self> {
        Arc::new(Self {
            tray,
            config: Mutex::new(config),
            refreshing: AtomicBool::new(false),
        })
    }

    fn set_refresh_state(&self, text: &str) {
        let _ = self.tray.refresh_state.set_text(text);
    }

    fn set_config_hint(&self, text: &str) {
        let _ = self.tray.config_hint.set_text(text);
    }

    fn update_provider_section(handles: &ProviderMenuHandles, snapshot: &ProviderSnapshot) {
        let _ = handles.header.set_text(snapshot.name);
        let lines = provider_lines(snapshot);
        let _ = handles.line_one.set_text(&lines[0]);
        let _ = handles.line_two.set_text(&lines[1]);
        let _ = handles.line_three.set_text(&lines[2]);
    }

    fn render_snapshots(&self, snapshots: &[ProviderSnapshot], refreshed_at: DateTime<Local>) {
        self.set_refresh_state(&format!("Last refresh {}", refreshed_at.format("%H:%M:%S")));
        self.set_config_hint("Edit config.json, then use Reload config and Refresh now");

        for snapshot in snapshots {
            match snapshot.name {
                "OpenCode" => Self::update_provider_section(&self.tray.opencode, snapshot),
                "Codex" => Self::update_provider_section(&self.tray.codex, snapshot),
                "Claude" => Self::update_provider_section(&self.tray.claude, snapshot),
                _ => {}
            }
        }
    }

    fn refresh(self: Arc<Self>) {
        if self.refreshing.swap(true, Ordering::SeqCst) {
            self.set_refresh_state("Refresh already running");
            return;
        }

        self.set_refresh_state("Refreshing now...");
        let state = self.clone();
        tauri::async_runtime::spawn(async move {
            let config = {
                let guard = state.config.lock().unwrap();
                guard.clone()
            };

            let (opencode_snapshot, codex_snapshot, claude_snapshot) = tokio::join!(
                opencode::fetch(&config.providers.opencode),
                codex::fetch(&config.providers.codex),
                claude::fetch(&config.providers.claude),
            );

            let refreshed_at = Local::now();
            state.render_snapshots(&[opencode_snapshot, codex_snapshot, claude_snapshot], refreshed_at);
            state.refreshing.store(false, Ordering::SeqCst);
        });
    }

    fn reload_config(self: Arc<Self>) {
        match config::reload() {
            Ok(config) => {
                let interval = config.refresh_interval_seconds;
                *self.config.lock().unwrap() = config;
                self.set_config_hint(&format!("Reloaded config.json | poll every {}s", interval));
                self.refresh();
            }
            Err(error) => {
                self.set_config_hint(&format!("Config reload failed: {error}"));
            }
        }
    }
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let config = match config::load_or_create() {
                Ok(config) => config,
                Err(error) => return Err(Box::new(std::io::Error::other(error))),
            };
            let tray = build_tray(app)?;
            let runtime = AppRuntime::new(tray, config.clone());
            runtime.set_config_hint(&format!(
                "Config: {}",
                config::config_file_path()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|_| "unavailable".to_string())
            ));

            let managed_runtime = runtime.clone();
            app.manage(runtime.clone());
            runtime.clone().refresh();
            start_refresh_loop(managed_runtime);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn start_refresh_loop(runtime: Arc<AppRuntime>) {
    tauri::async_runtime::spawn(async move {
        loop {
            let seconds = {
                let config = runtime.config.lock().unwrap();
                config.refresh_interval_seconds.max(30)
            };
            tokio::time::sleep(Duration::from_secs(seconds)).await;
            runtime.clone().refresh();
        }
    });
}

fn build_tray(app: &tauri::App<Wry>) -> tauri::Result<TrayHandles> {
    let refresh_state = MenuItem::with_id(app, "refresh-state", "Starting up...", false, None::<&str>)?;
    let config_hint = MenuItem::with_id(app, "config-hint", "Preparing config.json...", false, None::<&str>)?;

    let opencode = provider_menu_handles(app, "opencode")?;
    let codex = provider_menu_handles(app, "codex")?;
    let claude = provider_menu_handles(app, "claude")?;

    let refresh_now = MenuItem::with_id(app, "refresh-now", "Refresh now", true, None::<&str>)?;
    let reload_config = MenuItem::with_id(app, "reload-config", "Reload config", true, None::<&str>)?;
    let open_config = MenuItem::with_id(app, "open-config", "Open config.json", true, None::<&str>)?;
    let open_config_dir = MenuItem::with_id(app, "open-config-dir", "Open config directory", true, None::<&str>)?;
    let install_autostart_item = MenuItem::with_id(app, "install-autostart", "Install autostart", true, None::<&str>)?;
    let remove_autostart_item = MenuItem::with_id(app, "remove-autostart", "Remove autostart", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;

    let separator_a = PredefinedMenuItem::separator(app)?;
    let separator_b = PredefinedMenuItem::separator(app)?;
    let separator_c = PredefinedMenuItem::separator(app)?;
    let separator_d = PredefinedMenuItem::separator(app)?;

    let menu = Menu::with_items(
        app,
        &[
            &refresh_state,
            &config_hint,
            &separator_a,
            &opencode.header,
            &opencode.line_one,
            &opencode.line_two,
            &opencode.line_three,
            &separator_b,
            &codex.header,
            &codex.line_one,
            &codex.line_two,
            &codex.line_three,
            &separator_c,
            &claude.header,
            &claude.line_one,
            &claude.line_two,
            &claude.line_three,
            &separator_d,
            &refresh_now,
            &reload_config,
            &open_config,
            &open_config_dir,
            &install_autostart_item,
            &remove_autostart_item,
            &quit,
        ],
    )?;

    let _tray = TrayIconBuilder::with_id("linux-codexbar-tray")
        .tooltip("Linux CodexBar")
        .icon(app.default_window_icon().unwrap().clone())
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            let Some(runtime) = app.try_state::<Arc<AppRuntime>>().map(|state| state.inner().clone()) else {
                return;
            };
            match event.id.as_ref() {
                "refresh-now" => runtime.refresh(),
                "reload-config" => runtime.reload_config(),
                "open-config" => open_path(config::config_file_path().ok()),
                "open-config-dir" => open_path(config::config_dir_path().ok()),
                "install-autostart" => runtime.set_config_hint(&install_autostart_entry().unwrap_or_else(|e| format!("Autostart install failed: {e}"))),
                "remove-autostart" => runtime.set_config_hint(&remove_autostart_entry().unwrap_or_else(|e| format!("Autostart removal failed: {e}"))),
                "quit" => app.exit(0),
                _ => {}
            }
        })
        .build(app)?;

    Ok(TrayHandles {
        refresh_state,
        config_hint,
        opencode,
        codex,
        claude,
    })
}

fn provider_menu_handles(app: &tauri::App<Wry>, prefix: &str) -> tauri::Result<ProviderMenuHandles> {
    Ok(ProviderMenuHandles {
        header: MenuItem::with_id(app, format!("{prefix}-header"), prefix.to_string(), false, None::<&str>)?,
        line_one: MenuItem::with_id(app, format!("{prefix}-line-1"), "  waiting for first refresh", false, None::<&str>)?,
        line_two: MenuItem::with_id(app, format!("{prefix}-line-2"), "  no data yet", false, None::<&str>)?,
        line_three: MenuItem::with_id(app, format!("{prefix}-line-3"), "  source unavailable", false, None::<&str>)?,
    })
}

fn provider_lines(snapshot: &ProviderSnapshot) -> [String; 3] {
    match &snapshot.state {
        ProviderState::Disabled => [
            "  provider disabled in config.json".to_string(),
            "  no polling".to_string(),
            "  source: disabled".to_string(),
        ],
        ProviderState::NeedsSetup(message) => [
            format!("  setup needed: {}", shorten(message, 56)),
            "  edit config.json and reload".to_string(),
            "  source: not configured".to_string(),
        ],
        ProviderState::Error(message) => [
            format!("  error: {}", shorten(message, 60)),
            snapshot
                .detail
                .as_ref()
                .map(|detail| format!("  note: {}", shorten(detail, 60)))
                .unwrap_or_else(|| "  retry with Refresh now".to_string()),
            format!(
                "  source: {}",
                snapshot.source.as_deref().unwrap_or("unavailable")
            ),
        ],
        ProviderState::Ready => {
            let line_one = snapshot
                .primary
                .as_ref()
                .map(format_window)
                .unwrap_or_else(|| "  no primary window".to_string());
            let line_two = snapshot
                .secondary
                .as_ref()
                .map(format_window)
                .or_else(|| snapshot.tertiary.as_ref().map(format_window))
                .unwrap_or_else(|| "  no secondary window".to_string());

            let mut extras = Vec::new();
            if let Some(source) = &snapshot.source {
                extras.push(format!("source {source}"));
            }
            if let Some(plan) = &snapshot.plan {
                extras.push(format!("plan {plan}"));
            }
            if let Some(credits) = snapshot.credits_remaining {
                extras.push(format!("credits {:.1}", credits));
            }
            if let Some(detail) = &snapshot.detail {
                extras.push(detail.clone());
            }
            if let Some(updated_at) = snapshot.updated_at {
                extras.push(format!("updated {}", format_timestamp(updated_at)));
            }

            [
                line_one,
                line_two,
                format!("  {}", shorten(&extras.join(" | "), 72)),
            ]
        }
    }
}

fn format_window(window: &UsageWindowSnapshot) -> String {
    let mut parts = vec![
        format!("{} {:.0}% left", window.label, window.remaining_percent()),
    ];
    if let Some(resets_at) = window.resets_at {
        parts.push(format!("resets {}", format_timestamp(resets_at)));
    }
    format!("  {}", parts.join(" | "))
}

fn format_timestamp(timestamp: DateTime<Local>) -> String {
    let now = Local::now();
    let diff = timestamp.signed_duration_since(now);
    let seconds = diff.num_seconds();
    if seconds >= 0 && seconds < 60 {
        return format!("in {}s", seconds);
    }
    if seconds >= 60 && seconds < 3600 {
        return format!("in {}m", seconds / 60);
    }
    if seconds >= 3600 && seconds < 172800 {
        return format!("in {}h {}m", seconds / 3600, (seconds % 3600) / 60);
    }
    if seconds < 0 && seconds > -3600 {
        return format!("{}m ago", (-seconds) / 60);
    }
    if seconds < -3600 && seconds > -172800 {
        return format!("{}h ago", (-seconds) / 3600);
    }
    timestamp.format("%a %H:%M").to_string()
}

fn shorten(value: &str, max_len: usize) -> String {
    if value.len() <= max_len {
        value.to_string()
    } else {
        format!("{}...", &value[..max_len.saturating_sub(3)])
    }
}

fn open_path(path: Option<std::path::PathBuf>) {
    let Some(path) = path else { return };
    let _ = Command::new("xdg-open").arg(path).spawn();
}

fn install_autostart_entry() -> Result<String, String> {
    let path = autostart_desktop_entry_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Failed creating autostart directory {}: {error}", parent.display()))?;
    }
    let current_exe = std::env::current_exe()
        .map_err(|error| format!("Failed resolving current executable: {error}"))?;
    let desktop_entry = format!(
        "[Desktop Entry]\nType=Application\nName=Linux CodexBar\nComment=Tray app for OpenCode, Codex, and Claude usage windows\nExec={}\nTerminal=false\nX-GNOME-Autostart-enabled=true\nCategories=Utility;Development;\n",
        current_exe.display()
    );
    fs::write(&path, desktop_entry)
        .map_err(|error| format!("Failed writing autostart entry {}: {error}", path.display()))?;
    Ok(format!("Autostart installed at {}", path.display()))
}

fn remove_autostart_entry() -> Result<String, String> {
    let path = autostart_desktop_entry_path()?;
    if path.exists() {
        fs::remove_file(&path)
            .map_err(|error| format!("Failed removing autostart entry {}: {error}", path.display()))?;
        Ok("Autostart removed".to_string())
    } else {
        Ok("Autostart entry was not installed".to_string())
    }
}

fn autostart_desktop_entry_path() -> Result<PathBuf, String> {
    let base = dirs::config_dir().ok_or_else(|| "Could not resolve ~/.config".to_string())?;
    Ok(base.join("autostart").join("linux-codexbar.desktop"))
}
