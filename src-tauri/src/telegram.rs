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

const LOCK: &str = include_str!("../telegram-module.json");
const RUNNER: &str = include_str!("../telegram-runner.py");
static BUSY: AtomicBool = AtomicBool::new(false);
#[derive(Default)]
pub struct TelegramState {
    child: Mutex<Option<Child>>,
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
#[derive(Deserialize, Serialize)]
struct Config {
    port: u16,
    secret: String,
}
#[derive(Serialize)]
pub struct Status {
    installed: bool,
    running: bool,
    busy: bool,
    version: String,
    download_bytes: u64,
    port: u16,
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
    if cfg.port == 0 || cfg.secret.len() != 32 || !cfg.secret.bytes().all(|c| c.is_ascii_hexdigit())
    {
        return Err("tg_invalid_config".into());
    }
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
    tauri::async_runtime::spawn_blocking(move || {
        let path = root(&app)?;
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
        let address = (std::net::Ipv4Addr::LOCALHOST, cfg.port);
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
            if TcpStream::connect_timeout(&address.into(), Duration::from_millis(150)).is_ok() {
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
    })
    .await
    .map_err(|e| e.to_string())?
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
    let module = root(&app)?.join("module");
    if module.exists() {
        fs::remove_dir_all(module).map_err(|e| e.to_string())?;
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
    let temp = path.join("config.tmp");
    fs::write(&temp, serde_json::to_vec(&cfg).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    fs::rename(temp, path.join("config.json")).map_err(|e| e.to_string())
}
#[tauri::command]
pub fn telegram_link(app: tauri::AppHandle) -> Result<String, String> {
    let path = root(&app)?;
    if !installed(&path) {
        return Err("tg_not_installed".into());
    }
    let cfg = config(&path)?;
    Ok(format!(
        "tg://proxy?server=127.0.0.1&port={}&secret=dd{}",
        cfg.port, cfg.secret
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
