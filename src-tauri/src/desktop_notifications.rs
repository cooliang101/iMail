use serde::Deserialize;
use tauri::{AppHandle, Emitter, Manager};

#[derive(Clone, Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationTarget {
    message_id: String,
    account_email: Option<String>,
}

#[derive(Deserialize)]
pub struct MessageNotification {
    title: String,
    body: Option<String>,
    target: NotificationTarget,
}

#[tauri::command]
pub fn desktop_notify_message(app: AppHandle, input: MessageNotification) -> Result<(), String> {
    let mut notification = notify_rust::Notification::new();
    notification
        .summary(&input.title)
        .app_id(&app.config().identifier);
    if let Some(body) = &input.body {
        notification.body(body);
    }
    let handle = notification.show().map_err(|error| error.to_string())?;
    std::thread::spawn(move || {
        let _ = handle.wait_for_response(|response: &notify_rust::NotificationResponse| {
            if !matches!(
                response,
                notify_rust::NotificationResponse::Default
                    | notify_rust::NotificationResponse::Action(_)
            ) {
                return;
            }
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
            let _ = app.emit("imail-notification-click", input.target);
        });
    });
    Ok(())
}
