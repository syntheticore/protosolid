use std::fs;
use std::path::PathBuf;

use base64::prelude::{Engine, BASE64_STANDARD};
use percent_encoding::percent_decode_str;
use serde_json::{json, Value};
use tauri::ipc::{InvokeBody, InvokeResponseBody, Request, Response};
use tauri::WebviewWindow;
use tauri_plugin_dialog::{DialogExt, FileDialogBuilder};

const IMAGE_EXTENSIONS: &[&str] = &[
    "avif", "bmp", "gif", "ico", "jpeg", "jpg", "png", "svg", "webp",
];

// Formats are comma-separated extensions, with or without leading dots
fn format_extensions(format: &str) -> Vec<String> {
    format
        .to_lowercase()
        .split(',')
        .map(|extension| extension.trim().trim_start_matches('.').to_string())
        .collect()
}

fn format_name(extension: &str) -> String {
    if extension == "cad" {
        "ProtoSolid Documents".to_string()
    } else {
        format!("{} Files", extension.to_uppercase())
    }
}

fn dialog(window: &WebviewWindow) -> FileDialogBuilder<tauri::Wry> {
    window.dialog().file().set_parent(window)
}

// Each extension is offered as its own file type
#[tauri::command]
pub async fn get_save_path(
    window: WebviewWindow,
    format: String,
    title: Option<String>,
) -> Option<PathBuf> {
    let extensions = format_extensions(&format);
    let mut dialog = dialog(&window);
    if let Some(title) = title {
        dialog = dialog.set_file_name(format!("{title}.{}", extensions[0]));
    }
    for extension in &extensions {
        dialog = dialog.add_filter(format_name(extension), &[extension.as_str()]);
    }
    dialog
        .add_filter("All Files", &["*"])
        .blocking_save_file()
        .map(|path| path.into_path().unwrap())
}

// All extensions open as one file type, named after the first.
// With multiple, all chosen paths are returned as an array.
#[tauri::command]
pub async fn get_load_path(window: WebviewWindow, format: String, multiple: Option<bool>) -> Value {
    let is_image = format.to_lowercase() == "image/*";
    let extensions = if is_image {
        IMAGE_EXTENSIONS
            .iter()
            .map(|extension| extension.to_string())
            .collect()
    } else {
        format_extensions(&format)
    };
    let name = if is_image {
        "Image Files".to_string()
    } else {
        format_name(&extensions[0])
    };
    let extensions: Vec<&str> = extensions.iter().map(String::as_str).collect();
    let dialog = dialog(&window)
        .add_filter(name, &extensions)
        .add_filter("All Files", &["*"]);
    if multiple.unwrap_or(false) {
        let paths: Vec<PathBuf> = dialog
            .blocking_pick_files()
            .unwrap_or_default()
            .into_iter()
            .map(|path| path.into_path().unwrap())
            .collect();
        json!(paths)
    } else {
        json!(dialog
            .blocking_pick_file()
            .map(|path| path.into_path().unwrap()))
    }
}

// Without an encoding, the file is read as raw bytes
#[tauri::command]
pub async fn load_file(path: String, encoding: Option<String>) -> Result<Response, String> {
    let data = fs::read(&path).map_err(|error| error.to_string())?;
    let text = match encoding.as_deref() {
        None => return Ok(Response::new(data)),
        Some("base64") => BASE64_STANDARD.encode(data),
        Some(_) => String::from_utf8_lossy(&data).into_owned(),
    };
    Ok(Response::new(InvokeResponseBody::Json(
        json!(text).to_string(),
    )))
}

// The data arrives as raw bytes, the URI encoded path in a header
#[tauri::command]
pub async fn save_file(request: Request<'_>) -> Result<(), String> {
    let InvokeBody::Raw(data) = request.body() else {
        return Err("Expected the file contents as raw bytes".to_string());
    };
    let path = request.headers()["path"]
        .to_str()
        .map_err(|error| error.to_string())?;
    let path = percent_decode_str(path)
        .decode_utf8()
        .map_err(|error| error.to_string())?;
    fs::write(path.as_ref(), data).map_err(|error| error.to_string())
}
