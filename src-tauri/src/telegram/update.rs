//! Update the headless source from an official GitHub release tag, retaining
//! the pinned interpreter and installing reviewed dependency additions in staging.
//! Incompatible releases never activate.
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
fn missing_dependencies(installed: &Lock) -> Vec<Artifact> {
    lock()
        .artifacts
        .into_iter()
        .filter(|artifact| artifact.kind == "wheel" && !installed.artifacts.contains(artifact))
        .collect()
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
fn prepare_staging(root: &Path, source: &[u8], wheels: &[Vec<u8>]) -> Result<(), String> {
    let staging = root.join("staging");
    if staging.exists() {
        fs::remove_dir_all(&staging).map_err(|e| e.to_string())?;
    }
    copy_tree(&root.join("module"), &staging)?;
    fs::remove_dir_all(staging.join("proxy")).map_err(|e| e.to_string())?;
    extract(source, &staging, "source")?;
    for wheel in wheels {
        extract(wheel, &staging, "wheel")?;
    }
    fs::write(staging.join("runner.py"), RUNNER).map_err(|e| e.to_string())?;
    check_module(&staging).map_err(|_| "tg_update_incompatible".to_string())
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
    let mut metadata: serde_json::Value = serde_json::from_slice(
        &fs::read(path.join("module/installed.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let installed: Lock = serde_json::from_value(metadata.clone()).map_err(|e| e.to_string())?;
    let dependencies = missing_dependencies(&installed);
    let total: u64 = dependencies.iter().map(|artifact| artifact.size).sum();
    let mut done = 0u64;
    let mut wheels = Vec::new();
    let client = client()?;
    for artifact in dependencies {
        let bytes = download_artifact(&client, &artifact, |downloaded| {
            let _ = app.emit(
                "telegram-download-progress",
                80 + (done + downloaded) * 15 / total,
            );
        })
        .await?;
        done += artifact.size;
        wheels.push(bytes);
    }
    // Record the dependency set actually supplied by this application so the next
    // update does not download it again. Runtime metadata remains unchanged.
    let mut artifacts: Vec<_> = installed
        .artifacts
        .into_iter()
        .filter(|artifact| artifact.kind != "wheel")
        .collect();
    artifacts.extend(
        lock()
            .artifacts
            .into_iter()
            .filter(|artifact| artifact.kind == "wheel"),
    );
    metadata["artifacts"] = serde_json::to_value(artifacts).map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        let staging = path.join("staging");
        let result = (|| {
            prepare_staging(&path, &bytes, &wheels)?;
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
    fn legacy_lock() -> Lock {
        let mut installed = lock();
        let original_wheels = [
            "certifi-",
            "cffi-",
            "cryptography-",
            "pycparser-",
            "typing_extensions-",
        ];
        installed.artifacts.retain(|artifact| {
            artifact.kind != "wheel"
                || original_wheels
                    .iter()
                    .any(|name| artifact.url.rsplit('/').next().unwrap().starts_with(name))
        });
        installed.version = "1.10.2".into();
        installed
    }
    #[test]
    fn legacy_installations_receive_reviewed_http2_dependencies() {
        let mut installed = legacy_lock();
        let dependencies = missing_dependencies(&installed);
        assert_eq!(dependencies.len(), 8);
        for name in [
            "httpx-",
            "httpcore-",
            "anyio-",
            "idna-",
            "h2-",
            "h11-",
            "hpack-",
            "hyperframe-",
        ] {
            assert!(
                dependencies.iter().any(|artifact| artifact
                    .url
                    .rsplit('/')
                    .next()
                    .unwrap()
                    .starts_with(name)),
                "{name}"
            );
        }
        installed.artifacts.extend(dependencies);
        assert!(missing_dependencies(&installed).is_empty());
        installed
            .artifacts
            .iter_mut()
            .find(|artifact| artifact.kind == "wheel")
            .unwrap()
            .sha256 = "outdated".into();
        assert_eq!(missing_dependencies(&installed).len(), 1);
    }
    #[cfg(windows)]
    #[test]
    #[ignore = "Requires locked archives and artifacts/tg-source/legacy-source.zip (v1.10.2)"]
    fn upgrades_legacy_module_in_staging() {
        let artifacts = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("artifacts");
        let root = std::env::temp_dir().join(format!(
            "zapret-tg-migration-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let module = root.join("module");
        fs::create_dir_all(&module).unwrap();
        let read_artifact = |artifact: &Artifact| {
            let file = match artifact.kind.as_str() {
                "runtime" => artifacts.join("tg-downloads/python.zip"),
                "source" => artifacts.join("tg-source/source.zip"),
                _ => artifacts
                    .join("tg-downloads")
                    .join(artifact.url.rsplit('/').next().unwrap()),
            };
            let bytes = fs::read(file).unwrap();
            assert!(verified(&bytes, artifact), "{}", artifact.url);
            bytes
        };
        let installed = legacy_lock();
        for artifact in installed
            .artifacts
            .iter()
            .filter(|artifact| artifact.kind != "source")
        {
            extract(&read_artifact(artifact), &module, &artifact.kind).unwrap();
        }
        let legacy_source = fs::read(artifacts.join("tg-source/legacy-source.zip")).unwrap();
        assert_eq!(
            digest(&legacy_source),
            "efe9914f9b9c1e517d8fa46c73f0fdf9e66e90dbcdc8b9a6363de6d41f42a65b"
        );
        extract(&legacy_source, &module, "source").unwrap();
        fs::write(module.join("python313._pth"), "python313.zip\n.\n").unwrap();
        fs::write(module.join("runner.py"), RUNNER).unwrap();
        check_module(&module).unwrap();
        let config = fs::read(root.join("config.json")).unwrap();
        let source = read_artifact(
            &lock()
                .artifacts
                .into_iter()
                .find(|artifact| artifact.kind == "source")
                .unwrap(),
        );
        assert_eq!(
            prepare_staging(&root, &source, &[]).unwrap_err(),
            "tg_update_incompatible"
        );
        check_module(&module).unwrap();
        assert!(!module.join("httpx").exists());
        let wheels: Vec<_> = missing_dependencies(&installed)
            .iter()
            .map(read_artifact)
            .collect();
        prepare_staging(&root, &source, &wheels).unwrap();
        assert!(!module.join("httpx").exists());
        activate(&root, || check_module(&module)).unwrap();
        assert!(module.join("httpx").is_dir());
        assert!(!root.join("previous/httpx").exists());
        assert_eq!(fs::read(root.join("config.json")).unwrap(), config);
        fs::remove_dir_all(root).unwrap();
    }
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
