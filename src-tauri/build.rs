use std::fs;
use std::path::Path;

fn main() {
    // 1. Read tauri.conf.json to get the "master" version
    let config_path = Path::new("tauri.conf.json");
    if let Ok(config_str) = fs::read_to_string(config_path) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&config_str) {
            if let Some(version) = v["version"].as_str() {
                // 2. Export version as an environment variable for the Rust code
                println!("cargo:rustc-env=APP_VERSION={}", version);

                // 3. Keep version.txt in sync automatically (in project root)
                // Since build.rs runs in src-tauri, the root is one level up
                let version_txt_path = Path::new("../version.txt");
                let _ = fs::write(version_txt_path, version);
            }
        }
    }

    tauri_build::build();
}
