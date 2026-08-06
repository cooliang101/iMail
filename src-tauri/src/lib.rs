use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, State,
};

mod app_logging;
mod http_bridge;
mod local_service;

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
    let app = tauri::Builder::default()
        .manage(DesktopWindowState::default())
        .plugin(app_logging::plugin())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            log::info!(target: "desktop", "[process.single_instance] activating existing window");
            show_ready_main_window(app);
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    log::info!(target: "desktop", "[window.hide] close requested; keeping tray process active");
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => {
                log::info!(target: "desktop", "[tray.show] opening main window");
                show_ready_main_window(app);
            }
            "compose" => {
                log::info!(target: "desktop", "[tray.compose] compose requested");
                show_ready_main_window(app);
                if app
                    .state::<DesktopWindowState>()
                    .frontend_ready
                    .load(Ordering::Acquire)
                {
                    let _ = app.emit("desktop-compose", ());
                }
            }
            "quit" => {
                log::info!(target: "desktop", "[process.quit] quit requested from tray menu");
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|app, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                log::info!(target: "desktop", "[tray.click] opening main window");
                show_ready_main_window(app);
            }
        })
        .invoke_handler(tauri::generate_handler![
            desktop_frontend_ready,
            app_logging::desktop_log,
            app_logging::desktop_open_app_logs,
            http_bridge::desktop_http_request,
            http_bridge::desktop_download,
            http_bridge::desktop_read_binary,
            http_bridge::desktop_start_events,
            http_bridge::desktop_stop_events,
            local_service::local_service_status,
            local_service::local_service_enable,
            local_service::local_service_pause,
            local_service::local_service_remove,
            local_service::local_service_open_logs,
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
            let show = MenuItem::with_id(app, "show", "打开 iMail", true, None::<&str>)?;
            let compose = MenuItem::with_id(app, "compose", "写邮件", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "退出 iMail", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &compose, &quit])?;
            let mut tray = TrayIconBuilder::with_id("main")
                .menu(&menu)
                .tooltip("iMail")
                .show_menu_on_left_click(false);
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;
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

    app.run(|_app, event| match event {
        tauri::RunEvent::Ready => log::info!(target: "desktop", "[process.ready] event loop ready"),
        tauri::RunEvent::ExitRequested { code, .. } => {
            log::info!(target: "desktop", "[process.exit_requested] code={code:?}")
        }
        tauri::RunEvent::Exit => log::info!(target: "desktop", "[process.exit] event loop stopped"),
        _ => {}
    });
}
