//! Update the headless source from an exact official GitHub commit, retaining
//! the pinned interpreter/dependencies. Incompatible releases never activate.
use super::*;

const API: &str = "https://api.github.com/repos/Flowseal/tg-ws-proxy";
#[derive(Clone, Deserialize, Serialize)]
pub struct Release {
    pub version: String,
    pub commit: String,
}
#[derive(Serialize)]
pub struct UpdateInfo {
    installed: bool,
    current: Option<String>,
    latest: Option<String>,
    available: bool,
}
fn newer(current: &str, latest: &str) -> bool {
    matches!(
        crate::core::compare_versions(current, latest),
        crate::core::CoreUpdateStatus::UpdateAvailable
    )
}
fn valid_version(value: &str) -> bool {
    value.len() < 40
        && value.split('.').count() == 3
        && value
            .split('.')
            .all(|part| !part.is_empty() && part.parse::<u32>().is_ok())
}
fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .https_only(true)
        .user_agent("zapret-ui-telegram-updater")
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| e.to_string())
}
async fn json(client: &reqwest::Client, url: &str) -> Result<serde_json::Value, String> {
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| e.to_string())?;
        if bytes.len() + chunk.len() > 1024 * 1024 {
            return Err("tg_update_metadata_invalid".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}
async fn latest() -> Result<Release, String> {
    let client = client()?;
    let release = json(&client, &format!("{API}/releases/latest")).await?;
    if release["draft"] != false || release["prerelease"] != false {
        return Err("tg_update_metadata_invalid".into());
    }
    let tag = release["tag_name"]
        .as_str()
        .ok_or("tg_update_metadata_invalid")?;
    let version = tag.trim_start_matches('v');
    if !valid_version(version) {
        return Err("tg_update_metadata_invalid".into());
    }
    let commit = json(&client, &format!("{API}/commits/{tag}")).await?;
    let sha = commit["sha"].as_str().ok_or("tg_update_metadata_invalid")?;
    if sha.len() != 40 || !sha.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err("tg_update_metadata_invalid".into());
    }
    Ok(Release {
        version: version.into(),
        commit: sha.into(),
    })
}
#[tauri::command]
pub async fn check_telegram_update(app: tauri::AppHandle) -> Result<UpdateInfo, String> {
    let path = root(&app)?;
    if !installed(&path) {
        return Ok(UpdateInfo {
            installed: false,
            current: None,
            latest: None,
            available: false,
        });
    }
    let current = installed_version(&path).ok_or("tg_not_installed")?;
    let release = latest().await?;
    let available = newer(&current, &release.version);
    let result = UpdateInfo {
        installed: true,
        current: Some(current),
        latest: Some(release.version.clone()),
        available,
    };
    *app.state::<TelegramState>().update.lock_unpoisoned() = Some(release);
    Ok(result)
}
fn copy_tree(source: &Path, target: &Path) -> Result<(), String> {
    fs::create_dir_all(target).map_err(|e| e.to_string())?;
    for item in fs::read_dir(source).map_err(|e| e.to_string())? {
        let item = item.map_err(|e| e.to_string())?;
        let kind = item.file_type().map_err(|e| e.to_string())?;
        if kind.is_symlink() {
            return Err("Module symlink rejected".into());
        }
        let dest = target.join(item.file_name());
        if kind.is_dir() {
            copy_tree(&item.path(), &dest)?;
        } else {
            fs::copy(item.path(), dest).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
fn activate(root: &Path, health: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
    let module = root.join("module");
    let previous = root.join("previous");
    let staging = root.join("staging");
    if previous.exists() {
        fs::remove_dir_all(&previous).map_err(|e| e.to_string())?;
    }
    fs::rename(&module, &previous).map_err(|e| e.to_string())?;
    if let Err(error) = fs::rename(&staging, &module) {
        fs::rename(&previous, &module).map_err(|e| format!("tg_rollback_failed: {e}"))?;
        return Err(error.to_string());
    }
    if let Err(error) = health() {
        fs::remove_dir_all(&module).map_err(|e| format!("tg_rollback_failed: {e}"))?;
        fs::rename(&previous, &module).map_err(|e| format!("tg_rollback_failed: {e}"))?;
        return Err(format!("tg_update_rolled_back: {error}"));
    }
    Ok(())
}
#[tauri::command]
pub async fn update_telegram(app: tauri::AppHandle) -> Result<String, String> {
    let _operation = Operation::acquire()?;
    let path = root(&app)?;
    if !installed(&path) {
        return Err("tg_not_installed".into());
    }
    let release = app
        .state::<TelegramState>()
        .update
        .lock_unpoisoned()
        .clone()
        .ok_or("tg_check_update_first")?;
    let current = installed_version(&path).ok_or("tg_not_installed")?;
    if !newer(&current, &release.version) {
        return Ok(current);
    }
    // No user paths/URLs are accepted. The commit came from the official API.
    let response = client()?
        .get(format!(
            "https://codeload.github.com/Flowseal/tg-ws-proxy/zip/{}",
            release.commit
        ))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?;
    let total = response.content_length();
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| e.to_string())?;
        if bytes.len() + chunk.len() > 32 * 1024 * 1024 {
            return Err("Module too large".into());
        }
        bytes.extend_from_slice(&chunk);
        let percent = total
            .filter(|n| *n > 0)
            .map(|n| (bytes.len() as u64 * 80 / n).min(80))
            .unwrap_or(0);
        let _ = app.emit("telegram-download-progress", percent);
    }
    tauri::async_runtime::spawn_blocking(move || {
        let staging = path.join("staging");
        let result = (|| {
            if staging.exists() {
                fs::remove_dir_all(&staging).map_err(|e| e.to_string())?;
            }
            copy_tree(&path.join("module"), &staging)?;
            fs::remove_dir_all(staging.join("proxy")).map_err(|e| e.to_string())?;
            extract(&bytes, &staging, "source")?;
            fs::write(staging.join("runner.py"), RUNNER).map_err(|e| e.to_string())?;
            check_module(&staging).map_err(|_| "tg_update_incompatible".to_string())?;
            let mut metadata: serde_json::Value = serde_json::from_slice(
                &fs::read(path.join("module/installed.json")).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            metadata["version"] = release.version.clone().into();
            metadata["source_commit"] = release.commit.into();
            metadata["source_sha256"] = digest(&bytes).into();
            fs::write(
                staging.join("installed.json"),
                serde_json::to_vec_pretty(&metadata).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            let was_running = get_telegram_status(app.clone(), app.state())?.running;
            shutdown(&app);
            let activation = activate(&path, || {
                if was_running {
                    start_blocking(&app)
                } else {
                    Ok(())
                }
            });
            if let Err(error) = activation {
                if was_running {
                    start_blocking(&app).map_err(|e| format!("{error}; tg_restart_failed: {e}"))?;
                }
                return Err(error);
            }
            let _ = app.emit("telegram-download-progress", 100u32);
            let _ = app.emit("telegram-module-changed", ());
            Ok(release.version)
        })();
        if staging.exists() {
            let _ = fs::remove_dir_all(staging);
        }
        result
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    #[ignore = "Read-only live GitHub release/commit check; requires network"]
    async fn official_release_metadata() {
        let release = latest().await.unwrap();
        assert!(valid_version(&release.version));
        assert_eq!(release.commit.len(), 40);
        println!(
            "Official release: {} at {}",
            release.version, release.commit
        );
    }
    #[test]
    fn versions_are_strict_and_never_downgrade() {
        assert!(valid_version("1.10.2"));
        assert!(newer("1.9.9", "1.10.2"));
        assert!(!newer("1.10.2", "1.10.2"));
        assert!(!newer("2.0.0", "1.10.2"));
        for value in ["../main", "1.2.3-beta", "1.2", "1.2.3/evil"] {
            assert!(!valid_version(value));
        }
    }
    #[test]
    fn failed_activation_restores_previous_files() {
        let root = std::env::temp_dir().join(format!("zapret-tg-rollback-{}", std::process::id()));
        fs::create_dir_all(root.join("module")).unwrap();
        fs::create_dir_all(root.join("staging")).unwrap();
        fs::write(root.join("module/version"), "old").unwrap();
        fs::write(root.join("staging/version"), "new").unwrap();
        assert!(activate(&root, || Err("bad startup".into())).is_err());
        assert_eq!(
            fs::read_to_string(root.join("module/version")).unwrap(),
            "old"
        );
        fs::remove_dir_all(&root).unwrap();
    }
}
