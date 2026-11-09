use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use serde_json::Value;
use tauri::menu::{
    Menu, MenuBuilder, MenuItem, MenuItemBuilder, PredefinedMenuItem, Submenu, SubmenuBuilder,
};
use tauri::{AppHandle, Emitter, Manager, Wry};
use tauri_plugin_opener::OpenerExt;

use crate::MAIN;

const MAX_RECENT_FILES: usize = 10;

// Menu items the document window handles, sent as events of the same name
const FORWARDED: &[&str] = &[
    "createFile",
    "openFile",
    "connectRepo",
    "importFile",
    "exportFile",
    "saveFile",
    "saveFileAs",
    "saveAll",
    "publishRepo",
    "closeFile",
    "closeFiles",
];

pub struct AppMenu {
    pub menu: Menu<Wry>,
    pub save: MenuItem<Wry>,
    recent: Submenu<Wry>,
}

#[derive(Default)]
pub struct RecentFiles(Mutex<Vec<String>>);

fn item(app: &AppHandle, id: &str, label: &str, accelerator: &str) -> tauri::Result<MenuItem<Wry>> {
    MenuItemBuilder::with_id(id, label)
        .accelerator(accelerator)
        .build(app)
}

pub fn build(app: &AppHandle) -> tauri::Result<AppMenu> {
    let mac = cfg!(target_os = "macos");
    let save = item(app, "saveFile", "Save", "CmdOrCtrl+S")?;
    let recent = SubmenuBuilder::new(app, "Open Recent").build()?;

    let mut file = SubmenuBuilder::new(app, "File")
        .item(&item(app, "createFile", "New Document", "CmdOrCtrl+N")?)
        .item(&item(app, "openFile", "Open...", "CmdOrCtrl+O")?)
        .item(&recent)
        .text("connectRepo", "Connect...")
        .item(&item(app, "importFile", "Import...", "CmdOrCtrl+I")?)
        .item(&item(app, "exportFile", "Export...", "CmdOrCtrl+E")?)
        .separator()
        .item(&save)
        .item(&item(app, "saveFileAs", "Save As...", "CmdOrCtrl+Shift+S")?)
        .item(&item(app, "saveAll", "Save All", "CmdOrCtrl+Alt+S")?)
        .text("publishRepo", "Publish...")
        .separator()
        .item(&item(app, "closeFile", "Close File", "CmdOrCtrl+W")?)
        .text("closeFiles", "Close All Files");
    if !mac {
        file = file.quit();
    }

    let view = SubmenuBuilder::new(app, "View")
        .item(&item(
            app,
            "devtools",
            "Toggle Developer Tools",
            if mac { "Alt+Cmd+I" } else { "Ctrl+Shift+I" },
        )?)
        .item(&item(
            app,
            "fullscreen",
            "Toggle Full Screen",
            if mac { "Ctrl+Cmd+F" } else { "F11" },
        )?)
        .build()?;

    let window = SubmenuBuilder::new(app, "Window")
        .minimize()
        .maximize()
        .separator()
        .close_window()
        .build()?;

    let help = SubmenuBuilder::new(app, "Help")
        .text("learnMore", "Learn More")
        .build()?;

    let mut menu = MenuBuilder::new(app);
    if mac {
        menu = menu.item(
            &SubmenuBuilder::new(app, "ProtoSolid")
                .about(None)
                .separator()
                .item(&item(app, "preferences", "Settings...", "CmdOrCtrl+,")?)
                .separator()
                .hide()
                .hide_others()
                .show_all()
                .separator()
                .quit()
                .build()?,
        );
    }
    menu = menu.item(&file.build()?);
    // Windows and Linux leave text editing to the web view, as the menu is hidden there
    if mac {
        menu = menu.item(
            &SubmenuBuilder::new(app, "Edit")
                .undo()
                .redo()
                .separator()
                .cut()
                .copy()
                .paste()
                .build()?,
        );
    }
    let menu = menu.item(&view).item(&window).item(&help).build()?;

    Ok(AppMenu { menu, save, recent })
}

pub fn handle_event(app: &AppHandle, id: &str) {
    let main = app.get_webview_window(MAIN);
    match id {
        "preferences" => crate::show_preferences(app, None).unwrap(),
        "devtools" => {
            if let Some(main) = main {
                if main.is_devtools_open() {
                    main.close_devtools()
                } else {
                    main.open_devtools()
                }
            }
        }
        "fullscreen" => {
            if let Some(main) = main {
                main.set_fullscreen(!main.is_fullscreen().unwrap()).unwrap()
            }
        }
        "learnMore" => app
            .opener()
            .open_url("https://protosolid.org", None::<&str>)
            .unwrap(),
        "clearRecent" => set_recent_files(app, Vec::new()).unwrap(),
        _ => {
            if let Some(index) = id.strip_prefix("recent:") {
                let path = recent_files(app)[index.parse::<usize>().unwrap()].clone();
                app.emit_to(MAIN, "openFile", [path]).unwrap();
            } else if FORWARDED.contains(&id) {
                app.emit_to(MAIN, id, Vec::<Value>::new()).unwrap();
            }
        }
    }
}

fn recent_files_path(app: &AppHandle) -> tauri::Result<PathBuf> {
    let directory = app.path().app_data_dir()?;
    fs::create_dir_all(&directory)?;
    Ok(directory.join("recent-files.json"))
}

pub fn recent_files(app: &AppHandle) -> Vec<String> {
    app.state::<RecentFiles>().0.lock().unwrap().clone()
}

// The list doesn't exist before the first document is opened or saved
pub fn load_recent_files(app: &AppHandle) -> tauri::Result<()> {
    let files = fs::read_to_string(recent_files_path(app)?)
        .ok()
        .and_then(|data| serde_json::from_str(&data).ok())
        .unwrap_or_default();
    *app.state::<RecentFiles>().0.lock().unwrap() = files;
    update_recent_menu(app)
}

pub fn set_recent_files(app: &AppHandle, mut files: Vec<String>) -> tauri::Result<()> {
    files.truncate(MAX_RECENT_FILES);
    fs::write(recent_files_path(app)?, serde_json::to_string(&files)?)?;
    *app.state::<RecentFiles>().0.lock().unwrap() = files;
    update_recent_menu(app)
}

fn update_recent_menu(app: &AppHandle) -> tauri::Result<()> {
    let recent = &app.state::<AppMenu>().recent;
    for item in recent.items()? {
        recent.remove(&item)?;
    }

    let files = recent_files(app);
    if files.is_empty() {
        return recent.append(&MenuItem::new(app, "No Recent Files", false, None::<&str>)?);
    }
    for (index, file) in files.iter().enumerate() {
        recent.append(&MenuItem::with_id(
            app,
            format!("recent:{index}"),
            file,
            true,
            None::<&str>,
        )?)?;
    }
    recent.append(&PredefinedMenuItem::separator(app)?)?;
    recent.append(&MenuItem::with_id(
        app,
        "clearRecent",
        "Clear Recent",
        true,
        None::<&str>,
    )?)
}
