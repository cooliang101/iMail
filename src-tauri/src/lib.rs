use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, State,
};

mod app_logging;
mod desktop_notifications;
mod desktop_platform;
mod embedded_service;
mod http_bridge;
mod local_service;
mod request_cancellation;
mod tray_menu;

#[derive(Default)]
struct DesktopWindowState {
    frontend_ready: AtomicBool,
}

pub fn run_local_service_daemon_from_args() -> bool {
    local_service::run_daemon_from_args()
}

pub fn run_local_service_uninstall_cleanup_from_args() -> Option<Result<(), String>> {
    local_service::run_uninstall_cleanup_from_args()
}

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn show_ready_main_window(app: &AppHandle) {
    if app
        .state::<DesktopWindowState>()
        .frontend_ready
        .load(Ordering::Acquire)
    {
        show_main_window(app);
    }
}

#[tauri::command]
fn desktop_frontend_ready(app: AppHandle, state: State<'_, DesktopWindowState>) {
    let first_ready = !state.frontend_ready.swap(true, Ordering::AcqRel);
    if first_ready {
        log::info!(target: "desktop", "[frontend.ready] webview reported ready");
    }
    show_main_window(&app);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    app_logging::prepare_desktop_process();
    if !desktop_platform::ensure_webview2_runtime() {
        std::process::exit(1);
    }
    let app = tauri::Builder::default()
        .manage(DesktopWindowState::default())
        .manage(tray_menu::TrayMenuState::default())
        .manage(embedded_service::EmbeddedMailServiceState::default())
        .plugin(app_logging::plugin())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            log::info!(target: "desktop", "[process.single_instance] activating existing window");
            show_ready_main_window(app);
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .on_window_event(|window, event| {
            if window.label() == "tray-menu" {
                match event {
                    tauri::WindowEvent::CloseRequested { api, .. } => {
                        api.prevent_close();
                        let _ = window.hide();
                    }
                    tauri::WindowEvent::Focused(false) => {
                        let _ = window.hide();
                    }
                    _ => {}
                }
                return;
            }
            if window.label() == "main" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    log::info!(target: "desktop", "[window.hide] close requested; keeping tray process active");
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .on_tray_icon_event(|app, event| {
            if let TrayIconEvent::Click {
                button,
                button_state: MouseButtonState::Up,
                position,
                ..
            } = event
            {
                match button {
                    MouseButton::Left => {
                        log::info!(target: "desktop", "[tray.click] opening main window");
                        show_ready_main_window(app);
                    }
                    MouseButton::Right => {
                        log::info!(target: "desktop", "[tray.menu] opening custom tray menu");
                        if let Err(error) = tray_menu::show(app, position) {
                            log::error!(target: "desktop", "[tray.menu.failed] {error}");
                        }
                    }
                    _ => {}
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            desktop_frontend_ready,
            desktop_notifications::desktop_notify_message,
            tray_menu::desktop_update_tray_menu,
            tray_menu::desktop_get_tray_menu,
            tray_menu::desktop_resize_tray_menu,
            tray_menu::desktop_tray_action,
            embedded_service::desktop_mail_service_call,
            embedded_service::desktop_cancel_mail_service_call,
            embedded_service::desktop_start_embedded_events,
            embedded_service::desktop_stop_embedded_events,
            embedded_service::desktop_read_embedded_binary,
            embedded_service::desktop_download_embedded,
            embedded_service::desktop_start_external_http,
            app_logging::desktop_log,
            app_logging::desktop_open_app_logs,
            http_bridge::desktop_http_request,
            http_bridge::desktop_cancel_http_request,
            http_bridge::desktop_download,
            http_bridge::desktop_download_external_image,
            http_bridge::desktop_save_binary,
            http_bridge::desktop_save_text,
            http_bridge::desktop_read_binary,
            http_bridge::desktop_start_events,
            http_bridge::desktop_stop_events,
        ])
        .setup(|app| {
            log::info!(target: "desktop", "[tauri.setup] version={} os={} arch={}", env!("CARGO_PKG_VERSION"), std::env::consts::OS, std::env::consts::ARCH);
            let cookie_root = app.path().app_local_data_dir()?.join("http-sessions");
            app.manage(http_bridge::HttpBridgeState::new(cookie_root));
            if std::env::var("IMAIL_DESKTOP_SMOKE_TEST").as_deref() == Ok("true") {
                log::info!(target: "desktop", "[smoke.exit] desktop smoke test completed");
                app.handle().exit(0);
                return Ok(());
            }
            let mut tray = TrayIconBuilder::with_id("main")
                .tooltip("iMail")
                .show_menu_on_left_click(false);
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;
            tray_menu::create_window(app.handle())?;
            log::info!(target: "desktop", "[tauri.ready] tray and desktop bridge initialized");
            Ok(())
        })
        .build(tauri::generate_context!());

    let app = match app {
        Ok(app) => app,
        Err(error) => {
            app_logging::record_build_failure(&error);
            eprintln!("error while building iMail: {error}");
            std::process::exit(1);
        }
    };

    app.run(|app, event| match event {
        tauri::RunEvent::Ready => log::info!(target: "desktop", "[process.ready] event loop ready"),
        tauri::RunEvent::ExitRequested { code, .. } => {
            log::info!(target: "desktop", "[process.exit_requested] code={code:?}");
            if let Err(error) = app
                .state::<embedded_service::EmbeddedMailServiceState>()
                .shutdown()
            {
                log::error!(target: "desktop", "[embedded.shutdown.failed] {error}");
            }
        }
        tauri::RunEvent::Exit => log::info!(target: "desktop", "[process.exit] event loop stopped"),
        _ => {}
    });
}
