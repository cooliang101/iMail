use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager,
};

mod http_bridge;

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .manage(http_bridge::HttpBridgeState::new())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main_window(app);
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => show_main_window(app),
            "compose" => {
                show_main_window(app);
                let _ = app.emit("desktop-compose", ());
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|app, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(app);
            }
        })
        .invoke_handler(tauri::generate_handler![
            http_bridge::desktop_http_request,
            http_bridge::desktop_download,
            http_bridge::desktop_start_events,
            http_bridge::desktop_stop_events,
        ])
        .setup(|app| {
            if std::env::var("IMAIL_DESKTOP_SMOKE_TEST").as_deref() == Ok("true") {
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
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building iMail");

    app.run(|_app, _event| {});
}
