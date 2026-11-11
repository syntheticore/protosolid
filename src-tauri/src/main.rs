// Hides the console window of release builds on Windows
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod files;
mod mcp;
mod menu;

use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use serde_json::Value;
use tauri::webview::PageLoadEvent;
use tauri::window::Color;
#[cfg(target_os = "macos")]
use tauri::RunEvent;
use tauri::{
    AppHandle, Emitter, EventTarget, Manager, Theme, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, Window, WindowEvent,
};

use mcp::Mcp;

pub const MAIN: &str = "main";
const PREFERENCES: &str = "preferences";
const SPLASH: &str = "splash";

const BACKGROUND: Color = Color(0x1c, 0x21, 0x27, 0xff);

// The theme chosen in the preferences, None follows the system
#[derive(Default)]
struct ThemeSource(Mutex<Option<Theme>>);

// Fullscreen and maximized state of the document window, to report changes
#[derive(Default)]
struct WindowState(Mutex<(bool, bool)>);

// Gives the renderer its window.desktop interface and platform name, as Tauri owns window.ipc
fn bridge_script() -> String {
    let platform = match std::env::consts::OS {
        "windows" => "win32",
        "macos" => "darwin",
        os => os,
    };
    format!(
        "window.platform = '{platform}';\n{}",
        include_str!("bridge.js")
    )
}

fn theme_source(app: &AppHandle) -> Option<Theme> {
    *app.state::<ThemeSource>().0.lock().unwrap()
}

fn emit_dark_mode(app: &AppHandle) {
    let theme = theme_source(app).or_else(|| {
        app.webview_windows()
            .values()
            .next()
            .and_then(|window| window.theme().ok())
    });
    app.emit("dark-mode", [theme == Some(Theme::Dark)]).unwrap();
}

fn create_main_window(app: &AppHandle) -> tauri::Result<()> {
    let builder = WebviewWindowBuilder::new(app, MAIN, WebviewUrl::default())
        .title("ProtoSolid")
        .inner_size(1250.0, 850.0)
        .min_inner_size(1000.0, 450.0)
        .background_color(BACKGROUND)
        .theme(theme_source(app))
        .visible(false)
        // Tauri's file drop handling would swallow the page's own drag and drop on Windows
        .disable_drag_drop_handler()
        .initialization_script(&bridge_script())
        // Requests in flight are never answered by a reloaded page
        .on_page_load(|window, payload| {
            if matches!(payload.event(), PageLoadEvent::Started) {
                window
                    .state::<Mcp>()
                    .reject_pending("The document window was reloaded");
            }
        });

    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true);
    #[cfg(not(target_os = "macos"))]
    let builder = builder.decorations(false);

    let window = builder.build()?;

    // The frameless window draws its own menu, the native one only provides shortcuts
    #[cfg(not(target_os = "macos"))]
    {
        window.set_menu(app.state::<menu::AppMenu>().menu.clone())?;
        window.hide_menu()?;
    }
    #[cfg(target_os = "macos")]
    let _ = window;

    Ok(())
}

fn show_preferences(app: &AppHandle, section: Option<String>) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window(PREFERENCES) {
        if let Some(section) = section {
            app.emit_to(PREFERENCES, "show-preferences-section", [section])?;
        }
        window.unminimize()?;
        window.show()?;
        return window.set_focus();
    }

    let mut url = "?window=preferences".to_string();
    if let Some(section) = section {
        url += &format!("&section={section}");
    }

    let mut builder = WebviewWindowBuilder::new(app, PREFERENCES, WebviewUrl::App(url.into()))
        .title("Settings")
        .inner_size(760.0, 500.0)
        .min_inner_size(680.0, 480.0)
        .center()
        .resizable(false)
        .decorations(false)
        .background_color(BACKGROUND)
        .theme(theme_source(app))
        .visible(false)
        // Tauri's file drop handling would swallow the page's own drag and drop on Windows
        .disable_drag_drop_handler()
        .initialization_script(&bridge_script())
        .on_page_load(|window, payload| {
            if matches!(payload.event(), PageLoadEvent::Finished) {
                emit_dark_mode(window.app_handle());
                window.show().unwrap();
            }
        });
    if let Some(main) = app.get_webview_window(MAIN) {
        builder = builder.parent(&main)?;
    }
    builder.build()?;
    Ok(())
}

fn show_splash(app: &AppHandle) -> tauri::Result<()> {
    let splash = WebviewWindowBuilder::new(app, SPLASH, WebviewUrl::App("splash.html".into()))
        .title("ProtoSolid")
        .inner_size(300.0, 200.0)
        .center()
        .resizable(false)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .visible(false)
        .initialization_script(format!("window.appVersion = '{}'", app.package_info().version))
        .build()?;

    let app = app.clone();
    // New windows get time to settle before they animate in, which avoids stutter on Windows.
    // The main window loads hidden as soon as the splash screen has transitioned in.
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(500));
        splash.show().unwrap();
        thread::sleep(Duration::from_millis(500));
        create_main_window(&app).unwrap();
    });
    Ok(())
}

fn sync_window_state(window: &Window) {
    let (Ok(fullscreen), Ok(maximized)) = (window.is_fullscreen(), window.is_maximized()) else {
        return;
    };
    let state = window.state::<WindowState>();
    let mut previous = state.0.lock().unwrap();
    if previous.0 != fullscreen {
        window
            .emit_to(MAIN, "fullscreen-changed", [fullscreen])
            .unwrap();
    }
    if previous.1 != maximized {
        window
            .emit_to(MAIN, "maximize-changed", [maximized])
            .unwrap();
    }
    *previous = (fullscreen, maximized);
}

#[tauri::command]
fn minimize(window: WebviewWindow) -> tauri::Result<()> {
    window.minimize()
}

#[tauri::command]
fn maximize(window: WebviewWindow) -> tauri::Result<()> {
    window.maximize()
}

#[tauri::command]
fn unmaximize(window: WebviewWindow) -> tauri::Result<()> {
    window.unmaximize()
}

#[tauri::command]
fn close(window: WebviewWindow) -> tauri::Result<()> {
    window.close()
}

#[tauri::command]
fn restart(app: AppHandle) {
    app.restart()
}

#[tauri::command]
fn set_save_enabled(app: AppHandle, enabled: bool) -> tauri::Result<()> {
    app.state::<menu::AppMenu>().save.set_enabled(enabled)
}

#[tauri::command]
fn get_recent_files(app: AppHandle) -> Vec<String> {
    menu::recent_files(&app)
}

#[tauri::command]
fn add_recent_file(app: AppHandle, path: String) -> tauri::Result<()> {
    let mut files = menu::recent_files(&app);
    files.retain(|file| *file != path);
    files.insert(0, path);
    menu::set_recent_files(&app, files)
}

#[tauri::command]
fn remove_recent_file(app: AppHandle, path: String) -> tauri::Result<()> {
    let mut files = menu::recent_files(&app);
    files.retain(|file| *file != path);
    menu::set_recent_files(&app, files)
}

// Runs on the main thread, so quick repeated requests can't create the window twice.
// The command is asynchronous, as creating windows from synchronous commands deadlocks on Windows.
#[tauri::command]
async fn open_preferences(app: AppHandle, section: Option<String>) -> tauri::Result<()> {
    let handle = app.clone();
    app.run_on_main_thread(move || show_preferences(&handle, section).unwrap())
}

#[tauri::command]
fn close_preferences(window: WebviewWindow) -> tauri::Result<()> {
    if window.label() != PREFERENCES {
        return Ok(());
    }
    window.close()
}

#[tauri::command]
fn preferences_updated(
    app: AppHandle,
    window: WebviewWindow,
    preferences: Value,
) -> tauri::Result<()> {
    app.emit_filter(
        "preferences-updated",
        [preferences],
        |target| !matches!(target, EventTarget::WebviewWindow { label } if label == window.label()),
    )
}

#[tauri::command]
fn mcp_configure(app: AppHandle, config: mcp::Config) {
    app.state::<Mcp>().configure(config)
}

#[tauri::command]
fn mcp_response(app: AppHandle, id: u64, response: Value) {
    app.state::<Mcp>().receive(id, response)
}

#[tauri::command]
fn mcp_status(app: AppHandle) -> mcp::Status {
    app.state::<Mcp>().status()
}

#[tauri::command]
fn set_theme(app: AppHandle, theme: String) -> tauri::Result<()> {
    let theme = match theme.as_str() {
        "light" => Some(Theme::Light),
        "dark" => Some(Theme::Dark),
        "system" => None,
        _ => return Ok(()),
    };
    *app.state::<ThemeSource>().0.lock().unwrap() = theme;
    for window in app.webview_windows().values() {
        window.set_theme(theme)?;
    }
    emit_dark_mode(&app);
    Ok(())
}

#[tauri::command]
fn vue_ready(app: AppHandle) {
    emit_dark_mode(&app);
    // Lets the loaded window settle before it animates in
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(500));
        if let Some(main) = app.get_webview_window(MAIN) {
            main.show().unwrap();
        }
        if let Some(splash) = app.get_webview_window(SPLASH) {
            splash.destroy().unwrap();
        }
    });
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(ThemeSource::default())
        .manage(WindowState::default())
        .manage(menu::RecentFiles::default())
        .invoke_handler(tauri::generate_handler![
            minimize,
            maximize,
            unmaximize,
            close,
            restart,
            set_save_enabled,
            get_recent_files,
            add_recent_file,
            remove_recent_file,
            open_preferences,
            close_preferences,
            preferences_updated,
            mcp_configure,
            mcp_response,
            mcp_status,
            set_theme,
            vue_ready,
            files::get_save_path,
            files::get_load_path,
            files::load_file,
            files::save_file,
        ])
        .on_menu_event(|app, event| menu::handle_event(app, event.id().as_ref()))
        .on_window_event(|window, event| match event {
            WindowEvent::ThemeChanged(_) => emit_dark_mode(window.app_handle()),
            WindowEvent::Resized(_) if window.label() == MAIN => sync_window_state(window),
            WindowEvent::Destroyed if window.label() == MAIN => window
                .state::<Mcp>()
                .reject_pending("The document window was closed"),
            _ => {}
        })
        .setup(|app| {
            let handle = app.handle();
            app.manage(Mcp::new(handle.clone()));
            app.manage(menu::build(handle)?);
            #[cfg(target_os = "macos")]
            app.set_menu(app.state::<menu::AppMenu>().menu.clone())?;
            menu::load_recent_files(handle)?;
            show_splash(handle)?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building ProtoSolid")
        .run(|_app, _event| {
            // Like other macOS apps, ProtoSolid keeps running without windows
            #[cfg(target_os = "macos")]
            match _event {
                RunEvent::Reopen { .. } if _app.get_webview_window(MAIN).is_none() => {
                    create_main_window(_app).unwrap()
                }
                RunEvent::ExitRequested {
                    api, code: None, ..
                } => api.prevent_exit(),
                _ => {}
            }
        });
}
