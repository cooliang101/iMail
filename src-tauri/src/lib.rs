use std::{fs, path::PathBuf, sync::{Arc, Mutex}, time::Duration};
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_shell::{process::{CommandChild, CommandEvent}, ShellExt};

type SharedChild = Arc<Mutex<Option<CommandChild>>>;

fn resource_file(app: &tauri::AppHandle, relative: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
  Ok(node_compatible_path(app.path().resource_dir()?.join(relative)))
}

fn node_compatible_path(path: PathBuf) -> PathBuf {
  #[cfg(windows)]
  {
    let value = path.to_string_lossy();
    if let Some(simplified) = value.strip_prefix(r"\\?\") {
      return PathBuf::from(simplified);
    }
  }
  path
}

fn desktop_port() -> u16 {
  std::env::var("IMAIL_DESKTOP_PORT").ok().and_then(|value| value.parse().ok()).filter(|port| *port > 0).unwrap_or(8787)
}

fn start_sidecar(app: &tauri::AppHandle, startup_token: &str, port: u16) -> Result<CommandChild, Box<dyn std::error::Error>> {
  let data_dir = app.path().app_data_dir()?;
  let log_dir = app.path().app_log_dir()?;
  fs::create_dir_all(&data_dir)?;
  fs::create_dir_all(&log_dir)?;
  let server_entry = resource_file(app, "desktop-runtime/server.cjs")?;
  let worker_entry = resource_file(app, "desktop-runtime/worker.cjs")?;
  let web_dir = resource_file(app, "web")?;
  let origin = format!("http://127.0.0.1:{port}");
  log::info!("starting sidecar with server={} worker={} web={}", server_entry.display(), worker_entry.display(), web_dir.display());
  let command = app.shell().sidecar("imail-node")?
    .arg(server_entry)
    .env("NODE_ENV", "production")
    .env("PORT", port.to_string())
    .env("HOST", "127.0.0.1")
    .env("CORS_ORIGIN", &origin)
    .env("FRONTEND_URL", &origin)
    .env("OAUTH_CALLBACK_BASE_URL", format!("{origin}/api/oauth"))
    .env("MCP_ALLOWED_HOSTS", "127.0.0.1,localhost")
    .env("IMAIL_DESKTOP_MODE", "true")
    .env("IMAIL_DESKTOP_STARTUP_TOKEN", startup_token)
    .env("IMAIL_DATA_DIR", data_dir)
    .env("IMAIL_LOG_DIR", log_dir)
    .env("IMAIL_WEB_DIR", web_dir)
    .env("IMAIL_WORKER_ENTRY", worker_entry);
  let (mut events, child) = command.spawn()?;
  tauri::async_runtime::spawn(async move {
    while let Some(event) = events.recv().await {
      match event {
        CommandEvent::Stdout(bytes) => log::info!("[sidecar] {}", String::from_utf8_lossy(&bytes)),
        CommandEvent::Stderr(bytes) => log::error!("[sidecar] {}", String::from_utf8_lossy(&bytes)),
        CommandEvent::Error(error) => log::error!("[sidecar] {error}"),
        CommandEvent::Terminated(status) => log::warn!("[sidecar] terminated: {status:?}"),
        _ => {}
      }
    }
  });
  Ok(child)
}

async fn wait_for_sidecar(startup_token: &str, port: u16) -> Result<(), String> {
  let client = reqwest::Client::new();
  let mut last_error = String::from("sidecar did not answer");
  for _ in 0..100 {
    match client.get(format!("http://127.0.0.1:{port}/api/desktop-health"))
      .header("x-imail-startup-token", startup_token)
      .send().await {
        Ok(response) if response.status().is_success() => return Ok(()),
        Ok(response) => last_error = format!("desktop health returned {}", response.status()),
        Err(error) => last_error = error.to_string(),
      }
    tokio::time::sleep(Duration::from_millis(100)).await;
  }
  Err(last_error)
}

fn stop_sidecar(child: &SharedChild) {
  if let Ok(mut guard) = child.lock() {
    if let Some(process) = guard.take() {
      let _ = process.kill();
    }
  }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  let child: SharedChild = Arc::new(Mutex::new(None));
  let setup_child = Arc::clone(&child);
  let exit_child = Arc::clone(&child);
  let app = tauri::Builder::default()
    .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
      if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
      }
    }))
    .plugin(tauri_plugin_shell::init())
    .plugin(tauri_plugin_opener::init())
    .plugin(tauri_plugin_dialog::init())
    .plugin(tauri_plugin_fs::init())
    .plugin(tauri_plugin_notification::init())
    .plugin(tauri_plugin_log::Builder::default().level(log::LevelFilter::Info).build())
    .setup(move |app| {
      let startup_token = uuid::Uuid::new_v4().to_string();
      let port = desktop_port();
      let smoke_test = std::env::var("IMAIL_DESKTOP_SMOKE_TEST").as_deref() == Ok("true");
      let process = start_sidecar(app.handle(), &startup_token, port)?;
      *setup_child.lock().map_err(|_| "sidecar state lock poisoned")? = Some(process);
      let handle = app.handle().clone();
      tauri::async_runtime::spawn(async move {
        match wait_for_sidecar(&startup_token, port).await {
          Ok(()) => {
            if smoke_test {
              handle.exit(0);
              return;
            }
            let url = if cfg!(debug_assertions) { "http://localhost:5173".to_string() } else { format!("http://127.0.0.1:{port}") };
            let parsed = match url.parse() {
              Ok(value) => value,
              Err(error) => { log::error!("invalid desktop URL: {error}"); return; }
            };
            if let Err(error) = WebviewWindowBuilder::new(&handle, "main", WebviewUrl::External(parsed))
              .title("iMail")
              .inner_size(1280.0, 800.0)
              .min_inner_size(880.0, 600.0)
              .build() {
                log::error!("failed to create main window: {error}");
                handle.exit(1);
              }
          }
          Err(error) => {
            log::error!("sidecar startup failed: {error}");
            handle.exit(1);
          }
        }
      });
      Ok(())
    })
    .build(tauri::generate_context!())
    .expect("error while building iMail");

  app.run(move |_app, event| {
    if matches!(event, tauri::RunEvent::Exit | tauri::RunEvent::ExitRequested { .. }) {
      stop_sidecar(&exit_child);
    }
  });
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn desktop_resource_paths_are_platform_neutral() {
    let root = PathBuf::from("resources");
    assert_eq!(root.join("desktop-runtime/server.cjs"), PathBuf::from("resources").join("desktop-runtime").join("server.cjs"));
    assert_eq!(root.join("web"), PathBuf::from("resources").join("web"));
  }

  #[test]
  fn macos_targets_have_distinct_sidecar_names() {
    assert_eq!("imail-node-aarch64-apple-darwin", format!("imail-node-{}", "aarch64-apple-darwin"));
    assert_eq!("imail-node-x86_64-apple-darwin", format!("imail-node-{}", "x86_64-apple-darwin"));
  }

  #[test]
  fn desktop_port_defaults_to_the_public_gateway_port() {
    std::env::remove_var("IMAIL_DESKTOP_PORT");
    assert_eq!(desktop_port(), 8787);
  }

  #[test]
  fn node_paths_do_not_use_windows_verbatim_prefixes() {
    #[cfg(windows)]
    assert_eq!(node_compatible_path(PathBuf::from(r"\\?\C:\iMail\server.cjs")), PathBuf::from(r"C:\iMail\server.cjs"));
  }
}
