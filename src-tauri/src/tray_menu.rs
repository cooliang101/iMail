use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{
    LogicalSize, Manager, PhysicalPosition, Position, Size, WebviewUrl, WebviewWindowBuilder,
};

const TRAY_WINDOW_LABEL: &str = "tray-menu";
const TRAY_WIDTH: f64 = 360.0;
const MIN_TRAY_HEIGHT: f64 = 248.0;
const MAX_TRAY_HEIGHT: f64 = 640.0;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrayAccount {
    pub id: String,
    pub email: String,
    pub display_name: String,
    pub provider: String,
    pub color: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrayMenuUpdate {
    pub accounts: Vec<TrayAccount>,
    pub theme_id: String,
    pub custom_theme: serde_json::Value,
}

#[derive(Default)]
pub struct TrayMenuState {
    update: Mutex<TrayMenuUpdate>,
    anchor: Mutex<Option<PhysicalPosition<f64>>>,
}

pub fn create_window(app: &tauri::AppHandle) -> tauri::Result<()> {
    WebviewWindowBuilder::new(
        app,
        TRAY_WINDOW_LABEL,
        WebviewUrl::App("index.html?tray-menu=1".into()),
    )
    .title("iMail")
    .inner_size(TRAY_WIDTH, 360.0)
    .visible(false)
    .decorations(false)
    .transparent(true)
    // The panel draws its own theme-aware shadow. A native window shadow creates
    // a second rounded translucent rectangle behind transparent WebView content.
    .shadow(false)
    .resizable(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .focused(false)
    .build()?;
    Ok(())
}

fn clamp(value: f64, min: f64, max: f64) -> f64 {
    value.max(min).min(max.max(min))
}

fn popup_position(
    app: &tauri::AppHandle,
    anchor: PhysicalPosition<f64>,
) -> tauri::Result<PhysicalPosition<i32>> {
    let window = app
        .get_webview_window(TRAY_WINDOW_LABEL)
        .ok_or_else(|| tauri::Error::AssetNotFound("tray menu window".into()))?;
    let size = window.outer_size()?;
    let monitor = app
        .monitor_from_point(anchor.x, anchor.y)?
        .or(app.primary_monitor()?)
        .ok_or_else(|| tauri::Error::AssetNotFound("active monitor".into()))?;
    let position = monitor.work_area().position;
    let work_size = monitor.work_area().size;
    let margin = 8.0;
    let left = position.x as f64 + margin;
    let top = position.y as f64 + margin;
    let right = position.x as f64 + work_size.width as f64 - margin;
    let bottom = position.y as f64 + work_size.height as f64 - margin;
    let x = clamp(
        anchor.x - size.width as f64 / 2.0,
        left,
        right - size.width as f64,
    );
    let preferred_y = anchor.y - size.height as f64 - 10.0;
    let y = if preferred_y >= top {
        preferred_y
    } else {
        clamp(anchor.y + 10.0, top, bottom - size.height as f64)
    };
    Ok(PhysicalPosition::new(x.round() as i32, y.round() as i32))
}

pub fn show(app: &tauri::AppHandle, anchor: PhysicalPosition<f64>) -> tauri::Result<()> {
    *app.state::<TrayMenuState>().anchor.lock().unwrap() = Some(anchor);
    let window = app
        .get_webview_window(TRAY_WINDOW_LABEL)
        .ok_or_else(|| tauri::Error::AssetNotFound("tray menu window".into()))?;
    window.set_position(Position::Physical(popup_position(app, anchor)?))?;
    window.show()?;
    window.set_focus()?;
    Ok(())
}

pub fn hide(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window(TRAY_WINDOW_LABEL) {
        let _ = window.hide();
    }
}

#[tauri::command]
pub fn desktop_update_tray_menu(
    app: tauri::AppHandle,
    state: tauri::State<'_, TrayMenuState>,
    update: TrayMenuUpdate,
) -> Result<(), String> {
    *state.update.lock().map_err(|_| "托盘状态锁定失败")? = update.clone();
    if let Some(window) = app.get_webview_window(TRAY_WINDOW_LABEL) {
        use tauri::Emitter;
        window
            .emit("desktop-tray-menu-updated", update)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn desktop_get_tray_menu(
    state: tauri::State<'_, TrayMenuState>,
) -> Result<TrayMenuUpdate, String> {
    state
        .update
        .lock()
        .map(|update| update.clone())
        .map_err(|_| "托盘状态锁定失败".into())
}

#[tauri::command]
pub fn desktop_resize_tray_menu(
    app: tauri::AppHandle,
    state: tauri::State<'_, TrayMenuState>,
    height: f64,
) -> Result<(), String> {
    let window = app
        .get_webview_window(TRAY_WINDOW_LABEL)
        .ok_or_else(|| "托盘菜单窗口不存在".to_string())?;
    let height = clamp(height, MIN_TRAY_HEIGHT, MAX_TRAY_HEIGHT);
    window
        .set_size(Size::Logical(LogicalSize::new(TRAY_WIDTH, height)))
        .map_err(|error| error.to_string())?;
    if let Some(anchor) = *state.anchor.lock().map_err(|_| "托盘位置锁定失败")? {
        window
            .set_position(Position::Physical(
                popup_position(&app, anchor).map_err(|error| error.to_string())?,
            ))
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn desktop_tray_action(
    app: tauri::AppHandle,
    action: String,
    account_id: Option<String>,
) -> Result<(), String> {
    hide(&app);
    match action.as_str() {
        "hide" => Ok(()),
        "show" => {
            super::show_ready_main_window(&app);
            Ok(())
        }
        "compose" => {
            super::show_ready_main_window(&app);
            use tauri::Emitter;
            app.emit("desktop-compose", ())
                .map_err(|error| error.to_string())
        }
        "account" => {
            let account_id = account_id
                .filter(|id| !id.is_empty())
                .ok_or("缺少邮箱 ID")?;
            super::show_ready_main_window(&app);
            use tauri::Emitter;
            app.emit("desktop-select-account", account_id)
                .map_err(|error| error.to_string())
        }
        "quit" => {
            app.exit(0);
            Ok(())
        }
        _ => Err("不支持的托盘操作".into()),
    }
}
