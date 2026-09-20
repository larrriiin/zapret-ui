//! Update the headless source from an official GitHub release tag, retaining
//! the pinned interpreter/dependencies. Incompatible releases never activate.
use super::*;
use reqwest::header::LOCATION;

const LATEST_RELEASE_URL: &str = "https://github.com/Flowseal/tg-ws-proxy/releases/latest";
const RELEASE_TAG_PATH: &str = "/Flowseal/tg-ws-proxy/releases/tag/";
#[derive(Clone, Deserialize, Serialize)]
pub struct Release {
    pub version: String,
    pub reference: String,
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
fn release_from_location(location: &str) -> Result<Release, String> {
    let url = reqwest::Url::parse(location).map_err(|_| "tg_update_metadata_invalid")?;
    if url.scheme() != "https"
        || url.host_str() != Some("github.com")
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("tg_update_metadata_invalid".into());
    }
    let tag = url
        .path()
        .strip_prefix(RELEASE_TAG_PATH)
        .filter(|tag| !tag.contains('/'))
        .ok_or("tg_update_metadata_invalid")?;
    let version = tag.strip_prefix('v').unwrap_or(tag);
    if !valid_version(version) {
        return Err("tg_update_metadata_invalid".into());
    }
    Ok(Release {
        version: version.into(),
        reference: format!("v{version}"),
    })
}
async fn latest() -> Result<Release, String> {
    let client = client()?;
    let response = client
        .get(LATEST_RELEASE_URL)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !response.status().is_redirection() {
        return Err(format!("tg_update_check_http: {}", response.status()));
    }
    let location = response
        .headers()
        .get(LOCATION)
        .and_then(|value| value.to_str().ok())
        .ok_or("tg_update_metadata_invalid")?;
    release_from_location(location)
}
fn archive_url(release: &Release) -> String {
    format!(
        "https://codeload.github.com/Flowseal/tg-ws-proxy/zip/refs/tags/{}",
        release.reference
    )
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
    // No user paths/URLs are accepted. The tag came from GitHub's official
    // latest-release redirect and passed the strict semantic-version parser.
    let response = client()?
        .get(archive_url(&release))
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
            if let Some(value) = metadata.as_object_mut() {
                value.remove("source_commit");
                value.insert("source_ref".into(), release.reference.into());
            }
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
    #[ignore = "Read-only live GitHub release redirect check; requires network"]
    async fn official_release_metadata() {
        let release = latest().await.unwrap();
        assert!(valid_version(&release.version));
        assert_eq!(release.reference, format!("v{}", release.version));
        println!(
            "Official release: {} at {}",
            release.version, release.reference
        );
    }
    #[test]
    fn parses_only_the_official_latest_release_redirect() {
        let release =
            release_from_location("https://github.com/Flowseal/tg-ws-proxy/releases/tag/v1.10.4")
                .unwrap();
        assert_eq!(release.version, "1.10.4");
        assert_eq!(release.reference, "v1.10.4");
        assert_eq!(
            archive_url(&release),
            "https://codeload.github.com/Flowseal/tg-ws-proxy/zip/refs/tags/v1.10.4"
        );

        for location in [
            "http://github.com/Flowseal/tg-ws-proxy/releases/tag/v1.10.4",
            "https://github.com.evil.test/Flowseal/tg-ws-proxy/releases/tag/v1.10.4",
            "https://user@github.com/Flowseal/tg-ws-proxy/releases/tag/v1.10.4",
            "https://github.com/Flowseal/tg-ws-proxy/releases/tag/v1.10.4/extra",
            "https://github.com/Flowseal/tg-ws-proxy/releases/tag/v1.10.4-beta",
            "https://github.com/Flowseal/tg-ws-proxy/releases/tag/v1.10.4?x=1",
        ] {
            assert!(release_from_location(location).is_err(), "{location}");
        }
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
