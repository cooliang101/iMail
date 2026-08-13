use std::{collections::BTreeMap, env, fs, path::PathBuf};

const DESKTOP_OAUTH_VARIABLES: [&str; 3] = [
    "GOOGLE_OAUTH_DESKTOP_CLIENT_ID",
    "GOOGLE_OAUTH_DESKTOP_CLIENT_SECRET",
    "MICROSOFT_OAUTH_DESKTOP_CLIENT_ID",
];

fn main() {
    inject_desktop_oauth_environment();
    tauri_build::build()
}

fn inject_desktop_oauth_environment() {
    let repository_env = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("../.env");
    println!("cargo:rerun-if-changed={}", repository_env.display());
    for name in DESKTOP_OAUTH_VARIABLES {
        println!("cargo:rerun-if-env-changed={name}");
    }

    let file_values = fs::read_to_string(repository_env)
        .ok()
        .map(|content| parse_environment(&content))
        .unwrap_or_default();
    let mut missing = Vec::new();
    for name in DESKTOP_OAUTH_VARIABLES {
        let value = env::var(name)
            .ok()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| file_values.get(name).cloned());
        if let Some(value) = value {
            println!("cargo:rustc-env={name}={value}");
        } else {
            missing.push(name);
        }
    }
    if env::var("PROFILE").as_deref() == Ok("release") && !missing.is_empty() {
        panic!(
            "Windows release 构建缺少 Desktop OAuth 配置：{}",
            missing.join(", ")
        );
    }
}

fn parse_environment(content: &str) -> BTreeMap<String, String> {
    content
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let (name, raw_value) = line.split_once('=')?;
            let name = name.trim();
            if !DESKTOP_OAUTH_VARIABLES.contains(&name) {
                return None;
            }
            let raw_value = raw_value.trim();
            let value = if raw_value.len() >= 2
                && ((raw_value.starts_with('"') && raw_value.ends_with('"'))
                    || (raw_value.starts_with('\'') && raw_value.ends_with('\'')))
            {
                &raw_value[1..raw_value.len() - 1]
            } else {
                raw_value
            };
            (!value.is_empty()).then(|| (name.to_string(), value.to_string()))
        })
        .collect()
}
