use tauri::Manager;

mod http_bridge;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .manage(http_bridge::HttpBridgeState::new())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .invoke_handler(tauri::generate_handler![
            http_bridge::desktop_http_request,
            http_bridge::desktop_download,
            http_bridge::desktop_start_events,
            http_bridge::desktop_stop_events,
        ])
        .setup(|app| {
            if std::env::var("IMAIL_DESKTOP_SMOKE_TEST").as_deref() == Ok("true") {
                app.handle().exit(0);
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building iMail");

    app.run(|_app, _event| {});
}
