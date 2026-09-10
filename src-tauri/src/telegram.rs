//! Optional, pinned headless Flowseal module. No system Python or pip is used.
use crate::MutexExt;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Seek, SeekFrom},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    time::{Duration, Instant},
};
use tauri::{Emitter, Manager};
pub mod update;

const LOCK: &str = include_str!("../telegram-module.json");
const RUNNER: &str = include_str!("../telegram-runner.py");
static BUSY: AtomicBool = AtomicBool::new(false);
#[derive(Default)]
pub struct TelegramState {
    child: Mutex<Option<Child>>,
    update: Mutex<Option<update::Release>>,
}
struct Operation;
impl Operation {
    fn acquire() -> Result<Self, String> {
        BUSY.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| "tg_busy")?;
        Ok(Self)
    }
}
impl Drop for Operation {
    fn drop(&mut self) {
        BUSY.store(false, Ordering::Release);
    }
}
#[derive(Deserialize)]
struct Lock {
    version: String,
    artifacts: Vec<Artifact>,
}
#[derive(Deserialize)]
struct Artifact {
    url: String,
    sha256: String,
    size: u64,
    kind: String,
}
#[derive(Clone, Deserialize, Serialize)]
pub struct Config {
    port: u16,
    secret: String,
    #[serde(default = "default_host")]
    host: String,
    #[serde(default = "default_dc_ips")]
    dc_ips: std::collections::BTreeMap<u16, String>,
    #[serde(default)]
    cfproxy: bool,
    #[serde(default)]
    cfproxy_domains: Vec<String>,
    #[serde(default)]
    worker: bool,
    #[serde(default)]
    worker_domains: Vec<String>,
    #[serde(default)]
    start_with_zapret: bool,
}
fn default_host() -> String {
    "127.0.0.1".into()
}
fn default_dc_ips() -> std::collections::BTreeMap<u16, String> {
    [(2, "149.154.167.220".into()), (4, "149.154.167.220".into())].into()
}
fn valid_domain(value: &str) -> bool {
    value.len() <= 253
        && value.contains('.')
        && value.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
}
fn validate_config(cfg: &Config) -> Result<(), String> {
    if cfg.port == 0 || cfg.secret.len() != 32 || !cfg.secret.bytes().all(|c| c.is_ascii_hexdigit())
    {
        return Err("tg_invalid_config".into());
    }
    if cfg.host.parse::<std::net::Ipv4Addr>().is_err() {
        return Err("tg_invalid_host".into());
    }
    if cfg.dc_ips.len() > 32
        || cfg
            .dc_ips
            .iter()
            .any(|(dc, ip)| *dc == 0 || *dc > 32767 || ip.parse::<std::net::Ipv4Addr>().is_err())
    {
        return Err("tg_invalid_dc".into());
    }
    if cfg.cfproxy_domains.len() > 32
        || cfg.worker_domains.len() > 32
        || cfg
            .cfproxy_domains
            .iter()
            .chain(&cfg.worker_domains)
            .any(|d| !valid_domain(d))
    {
        return Err("tg_invalid_domain".into());
    }
    if cfg.worker && cfg.worker_domains.is_empty() {
        return Err("tg_worker_domain_required".into());
    }
    Ok(())
}
#[derive(Serialize)]
pub struct Status {
    installed: bool,
    running: bool,
    busy: bool,
    version: String,
    download_bytes: u64,
    port: u16,
    host: String,
}
fn lock() -> Lock {
    serde_json::from_str(LOCK).expect("embedded Telegram lock")
}
fn root(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_local_data_dir()
        .map(|p| p.join("telegram"))
        .map_err(|e| e.to_string())
}
fn installed(path: &Path) -> bool {
    installed_version(path).is_some()
        && path.join("module/python.exe").is_file()
        && path.join("module/runner.py").is_file()
        && config(path).is_ok()
}
fn installed_version(path: &Path) -> Option<String> {
    let data = fs::read(path.join("module/installed.json")).ok()?;
    serde_json::from_slice::<Lock>(&data)
        .ok()
        .map(|m| m.version)
}
fn config(path: &Path) -> Result<Config, String> {
    let cfg: Config =
        serde_json::from_slice(&fs::read(path.join("config.json")).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    validate_config(&cfg)?;
    Ok(cfg)
}
fn process(path: &Path) -> Command {
    let mut cmd = Command::new(path.join("python.exe"));
    cmd.current_dir(path).arg("-I").arg(path.join("runner.py"));
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    cmd
}
#[tauri::command]
pub fn get_telegram_status(
    app: tauri::AppHandle,
    state: tauri::State<'_, TelegramState>,
) -> Result<Status, String> {
    let path = root(&app)?;
    // Recover an interrupted directory swap before reporting installation state.
    if !path.join("module").exists() && path.join("previous/installed.json").is_file() {
        if let Ok(_operation) = Operation::acquire() {
            fs::rename(path.join("previous"), path.join("module")).map_err(|e| e.to_string())?;
        }
    }
    let mut child = state.child.lock_unpoisoned();
    let running = match child.as_mut() {
        Some(c) => c.try_wait().map_err(|e| e.to_string())?.is_none(),
        None => false,
    };
    if !running {
        *child = None;
    }
    let metadata = lock();
    Ok(Status {
        installed: installed(&path),
        running,
        busy: BUSY.load(Ordering::Acquire),
        version: installed_version(&path).unwrap_or(metadata.version),
        download_bytes: metadata.artifacts.iter().map(|a| a.size).sum(),
        port: config(&path).map(|c| c.port).unwrap_or(1443),
        host: config(&path)
            .map(|c| c.host)
            .unwrap_or_else(|_| default_host()),
    })
}
fn allowed(url: &reqwest::Url) -> bool {
    url.scheme() == "https"
        && url.port_or_known_default() == Some(443)
        && url.username().is_empty()
        && url.password().is_none()
        && matches!(
            url.host_str(),
            Some(
                "www.python.org" | "github.com" | "codeload.github.com" | "files.pythonhosted.org"
            )
        )
}
fn verified(bytes: &[u8], artifact: &Artifact) -> bool {
    bytes.len() as u64 == artifact.size && digest(bytes) == artifact.sha256
}
fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn extract(bytes: &[u8], target: &Path, kind: &str) -> Result<(), String> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let mut expanded = 0u64;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
        // Check the original ZIP name too: platform normalization can strip a
        // drive prefix before enclosed_name() returns its relative path.
        let name = entry.name();
        if name.contains([':', '\0'])
            || name.starts_with(['/', '\\'])
            || name.split(['/', '\\']).any(|part| part == "..")
        {
            return Err("Unsafe module archive path".into());
        }
        let safe = entry.enclosed_name().ok_or("Unsafe module archive path")?;
        if entry.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000) {
            return Err("Module symlink rejected".into());
        }
        let relative = if kind == "source" {
            let stripped: PathBuf = safe.components().skip(1).collect();
            if !stripped.starts_with("proxy") && stripped != Path::new("LICENSE") {
                continue;
            }
            stripped
        } else {
            safe.to_path_buf()
        };
        if relative.as_os_str().is_empty() {
            continue;
        }
        // Windows drive/device/alternate stream names must never reach extraction.
        if relative
            .components()
            .any(|c| c.as_os_str().to_string_lossy().contains(':'))
        {
            return Err("Unsafe module archive path".into());
        }
        expanded = expanded
            .checked_add(entry.size())
            .ok_or("Module too large")?;
        if expanded > 256 * 1024 * 1024 {
            return Err("Module too large".into());
        }
        let out = target.join(relative);
        if entry.is_dir() {
            fs::create_dir_all(out).map_err(|e| e.to_string())?;
        } else {
            fs::create_dir_all(out.parent().ok_or("Invalid module path")?)
                .map_err(|e| e.to_string())?;
            let mut file = fs::File::create(out).map_err(|e| e.to_string())?;
            std::io::copy(&mut entry, &mut file).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
fn check_module(staging: &Path) -> Result<(), String> {
    let mut child = process(staging)
        .arg("--check")
        .spawn()
        .map_err(|e| e.to_string())?;
    let until = Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            return if status.success() {
                Ok(())
            } else {
                Err("tg_check_failed".into())
            };
        }
        if Instant::now() >= until {
            let _ = child.kill();
            let _ = child.wait();
            return Err("tg_check_failed".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}
#[tauri::command]
pub async fn install_telegram(app: tauri::AppHandle) -> Result<(), String> {
    let _operation = Operation::acquire()?;
    if !cfg!(all(windows, target_arch = "x86_64")) {
        return Err("tg_arch_unsupported".into());
    }
    let path = root(&app)?;
    if installed(&path) {
        return Ok(());
    }
    let staging = path.join("staging");
    if staging.exists() {
        fs::remove_dir_all(&staging).map_err(|e| e.to_string())?;
    }
    fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
    let result = async {
        let client = reqwest::Client::builder()
            .https_only(true)
            .timeout(Duration::from_secs(600))
            .redirect(reqwest::redirect::Policy::custom(|a| {
                if a.previous().len() >= 5 || !allowed(a.url()) {
                    a.error("Untrusted module redirect")
                } else {
                    a.follow()
                }
            }))
            .build()
            .map_err(|e| e.to_string())?;
        let metadata = lock();
        let total: u64 = metadata.artifacts.iter().map(|a| a.size).sum();
        let mut done = 0u64;
        for artifact in metadata.artifacts {
            let url = reqwest::Url::parse(&artifact.url).map_err(|e| e.to_string())?;
            if !allowed(&url) {
                return Err("Untrusted module URL".into());
            }
            let mut stream = client
                .get(url)
                .send()
                .await
                .map_err(|e| e.to_string())?
                .error_for_status()
                .map_err(|e| e.to_string())?
                .bytes_stream();
            let mut bytes = Vec::new();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(|e| e.to_string())?;
                if (bytes.len() + chunk.len()) as u64 > artifact.size {
                    return Err("tg_checksum_failed".into());
                }
                bytes.extend_from_slice(&chunk);
                let _ = app.emit(
                    "telegram-download-progress",
                    ((done + bytes.len() as u64) * 95 / total) as u32,
                );
            }
            if !verified(&bytes, &artifact) {
                return Err("tg_checksum_failed".into());
            }
            let destination = staging.clone();
            tauri::async_runtime::spawn_blocking(move || {
                extract(&bytes, &destination, &artifact.kind)
            })
            .await
            .map_err(|e| e.to_string())??;
            done += artifact.size;
        }
        fs::write(staging.join("python313._pth"), "python313.zip\n.\n")
            .map_err(|e| e.to_string())?;
        fs::write(staging.join("runner.py"), RUNNER).map_err(|e| e.to_string())?;
        let check_path = staging.clone();
        tauri::async_runtime::spawn_blocking(move || check_module(&check_path))
            .await
            .map_err(|e| e.to_string())??;
        fs::write(staging.join("installed.json"), LOCK).map_err(|e| e.to_string())?;
        let destination = path.join("module");
        // Only a previously incomplete installation can reach here.
        if destination.exists() {
            fs::remove_dir_all(&destination).map_err(|e| e.to_string())?;
        }
        fs::rename(&staging, &destination).map_err(|e| e.to_string())?;
        let _ = app.emit("telegram-download-progress", 100u32);
        let _ = app.emit("telegram-module-changed", ());
        Ok(())
    }
    .await;
    if staging.exists() {
        let _ = fs::remove_dir_all(staging);
    }
    result
}
#[tauri::command]
pub async fn start_telegram(app: tauri::AppHandle) -> Result<(), String> {
    let _operation = Operation::acquire()?;
    tauri::async_runtime::spawn_blocking(move || start_blocking(&app))
        .await
        .map_err(|e| e.to_string())?
}
pub fn start_on_app_launch_if_configured(app: &tauri::AppHandle) -> Result<(), String> {
    let path = root(app)?;
    if !installed(&path) || !config(&path)?.start_with_zapret {
        return Ok(());
    }
    let _operation = Operation::acquire()?;
    start_blocking(app)
}
fn start_blocking(app: &tauri::AppHandle) -> Result<(), String> {
    let path = root(app)?;
    if !installed(&path) {
        return Err("tg_not_installed".into());
    }
    let state = app.state::<TelegramState>();
    let mut slot = state.child.lock_unpoisoned();
    if let Some(child) = slot.as_mut() {
        if child.try_wait().map_err(|e| e.to_string())?.is_none() {
            return Ok(());
        }
    }
    let cfg = config(&path)?;
    fs::write(path.join("module/runner.py"), RUNNER).map_err(|e| e.to_string())?;
    let host: std::net::Ipv4Addr = cfg.host.parse().map_err(|_| "tg_invalid_host")?;
    let address = (host, cfg.port);
    let probe = TcpListener::bind(address).map_err(|_| "tg_port_busy")?;
    drop(probe);
    let mut child = process(&path.join("module"))
        .arg(std::process::id().to_string())
        .spawn()
        .map_err(|e| e.to_string())?;
    let until = Instant::now() + Duration::from_secs(15);
    loop {
        if child.try_wait().map_err(|e| e.to_string())?.is_some() {
            return Err("tg_start_failed".into());
        }
        let connect_address = (
            if host.is_unspecified() {
                std::net::Ipv4Addr::LOCALHOST
            } else {
                host
            },
            cfg.port,
        );
        if TcpStream::connect_timeout(&connect_address.into(), Duration::from_millis(150)).is_ok() {
            *slot = Some(child);
            return Ok(());
        }
        if Instant::now() >= until {
            let _ = child.kill();
            let _ = child.wait();
            return Err("tg_start_failed".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}
pub fn shutdown(app: &tauri::AppHandle) {
    if let Some(mut child) = app.state::<TelegramState>().child.lock_unpoisoned().take() {
        let _ = child.kill();
        let _ = child.wait();
    }
}
#[tauri::command]
pub fn stop_telegram(app: tauri::AppHandle) -> Result<(), String> {
    let _operation = Operation::acquire()?;
    shutdown(&app);
    Ok(())
}
#[tauri::command]
pub fn remove_telegram(app: tauri::AppHandle) -> Result<(), String> {
    let _operation = Operation::acquire()?;
    shutdown(&app);
    let root = root(&app)?;
    for name in ["module", "previous"] {
        let module = root.join(name);
        if module.exists() {
            fs::remove_dir_all(module).map_err(|e| e.to_string())?;
        }
    }
    let _ = app.emit("telegram-module-changed", ());
    Ok(())
}
#[tauri::command]
pub fn set_telegram_port(app: tauri::AppHandle, port: u16) -> Result<(), String> {
    let _operation = Operation::acquire()?;
    if port == 0 {
        return Err("tg_invalid_config".into());
    }
    if get_telegram_status(app.clone(), app.state())?.running {
        return Err("tg_stop_to_configure".into());
    }
    let path = root(&app)?;
    let mut cfg = config(&path)?;
    cfg.port = port;
    write_config(&path, &cfg)
}
fn write_config(path: &Path, cfg: &Config) -> Result<(), String> {
    validate_config(cfg)?;
    let temp = path.join("config.tmp");
    fs::write(&temp, serde_json::to_vec(&cfg).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    fs::rename(temp, path.join("config.json")).map_err(|e| e.to_string())
}
#[tauri::command]
pub fn get_telegram_config(app: tauri::AppHandle) -> Result<Config, String> {
    config(&root(&app)?)
}
#[tauri::command]
pub fn save_telegram_config(app: tauri::AppHandle, settings: Config) -> Result<Config, String> {
    let _operation = Operation::acquire()?;
    if get_telegram_status(app.clone(), app.state())?.running {
        return Err("tg_stop_to_configure".into());
    }
    let path = root(&app)?;
    let mut settings = settings;
    // Secret changes have a dedicated command and never come from an editable form.
    settings.secret = config(&path)?.secret;
    write_config(&path, &settings)?;
    Ok(settings)
}
#[tauri::command]
pub async fn regenerate_telegram_secret(app: tauri::AppHandle) -> Result<Config, String> {
    let _operation = Operation::acquire()?;
    if get_telegram_status(app.clone(), app.state())?.running {
        return Err("tg_stop_to_configure".into());
    }
    let path = root(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        fs::write(path.join("module/runner.py"), RUNNER).map_err(|e| e.to_string())?;
        let mut child = process(&path.join("module"))
            .arg("--rotate-secret")
            .spawn()
            .map_err(|e| e.to_string())?;
        let until = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(exit) = child.try_wait().map_err(|e| e.to_string())? {
                if exit.success() {
                    return config(&path);
                }
                return Err("tg_check_failed".into());
            }
            if Instant::now() >= until {
                let _ = child.kill();
                let _ = child.wait();
                return Err("tg_check_failed".into());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub fn telegram_link(app: tauri::AppHandle) -> Result<String, String> {
    let path = root(&app)?;
    if !installed(&path) {
        return Err("tg_not_installed".into());
    }
    let cfg = config(&path)?;
    Ok(format!(
        "tg://proxy?server={}&port={}&secret=dd{}",
        if cfg.host == "0.0.0.0" {
            "127.0.0.1"
        } else {
            &cfg.host
        },
        cfg.port,
        cfg.secret
    ))
}
#[tauri::command]
pub fn open_telegram(app: tauri::AppHandle) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_url(telegram_link(app.clone())?, None::<&str>)
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub fn telegram_logs(app: tauri::AppHandle) -> Result<String, String> {
    let path = root(&app)?;
    let log = path.join("proxy.log");
    if !log.exists() {
        return Ok(String::new());
    }
    let mut file = fs::File::open(log).map_err(|e| e.to_string())?;
    let size = file.metadata().map_err(|e| e.to_string())?.len();
    file.seek(SeekFrom::Start(size.saturating_sub(32768)))
        .map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    file.take(32768)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    Ok(if let Ok(cfg) = config(&path) {
        text.replace(&cfg.secret, "[secret]")
    } else {
        text
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};
    #[test]
    fn migrates_original_config_and_validates_extended_settings() {
        let mut cfg: Config =
            serde_json::from_str(r#"{"port":1443,"secret":"00000000000000000000000000000000"}"#)
                .unwrap();
        assert!(validate_config(&cfg).is_ok());
        assert_eq!(cfg.host, "127.0.0.1");
        assert_eq!(cfg.dc_ips.len(), 2);
        cfg.worker = true;
        assert!(validate_config(&cfg).is_err());
        cfg.worker_domains.push("example.workers.dev".into());
        assert!(validate_config(&cfg).is_ok());
        cfg.cfproxy_domains.push("https://evil.test/path".into());
        assert!(validate_config(&cfg).is_err());
        cfg.cfproxy_domains.clear();
        cfg.host = "0.0.0.0".into();
        assert!(validate_config(&cfg).is_ok());
        cfg.dc_ips.insert(2, "999.0.0.1".into());
        assert!(validate_config(&cfg).is_err());
    }
    fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes) in entries {
            zip.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }
    #[test]
    fn extraction_rejects_traversal_before_writing() {
        let target = Path::new("unused-telegram-test-directory");
        for name in ["../outside.py", "C:/outside.py", "library.py:stream"] {
            assert!(
                extract(&archive(&[(name, b"bad")]), target, "wheel").is_err(),
                "{name}"
            );
        }
        assert!(!target.exists());
    }
    #[test]
    fn source_archive_excludes_gui_and_scripts() {
        let target = Path::new("unused-telegram-source-test-directory");
        let bytes = archive(&[
            ("upstream/windows.py", b"gui"),
            ("upstream/ui/tray.py", b"gui"),
            ("upstream/setup.py", b"script"),
        ]);
        extract(&bytes, target, "source").unwrap();
        assert!(!target.exists());
    }
    #[test]
    fn lock_and_download_trust() {
        let metadata = lock();
        assert_eq!(metadata.artifacts.len(), 7);
        for item in metadata.artifacts {
            assert!(allowed(&item.url.parse().unwrap()));
            assert_eq!(item.sha256.len(), 64);
            assert!(item.size < 32 * 1024 * 1024);
        }
        for url in [
            "http://www.python.org/a",
            "https://github.com.evil.test/a",
            "https://user@github.com/a",
            "https://github.com:444/a",
        ] {
            assert!(!allowed(&url.parse().unwrap()));
        }
    }
    #[test]
    fn rejects_corrupt_download() {
        let a = Artifact {
            url: String::new(),
            kind: "wheel".into(),
            size: 3,
            sha256: digest(b"abc"),
        };
        assert!(verified(b"abc", &a));
        assert!(!verified(b"abd", &a));
        assert!(!verified(b"ab", &a));
    }
}
