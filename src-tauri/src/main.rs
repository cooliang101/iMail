// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if let Some(result) = imail_lib::run_local_service_uninstall_cleanup_from_args() {
        if let Err(error) = result {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return;
    }
    if imail_lib::run_local_service_daemon_from_args() {
        return;
    }
    imail_lib::run();
}
