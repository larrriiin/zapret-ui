use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;
use std::sync::{Mutex, MutexGuard};

/// Acquire `Mutex` access without panicking when the mutex is poisoned. If a
/// previous holder panicked the data is still well-formed for our use-cases
/// (mostly `Option<...>` state in `AppState`), so recovering is strictly
/// better than bringing the whole tray/UI thread down with an unwrap.
trait MutexExt<T> {
    fn lock_unpoisoned(&self) -> MutexGuard<'_, T>;
}
impl<T> MutexExt<T> for Mutex<T> {
    fn lock_unpoisoned(&self) -> MutexGuard<'_, T> {
        match self.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}
use tauri::menu::{MenuBuilder, MenuItemBuilder, SubmenuBuilder};
use tauri::tray::{TrayIconBuilder, TrayIconEvent};
use tauri::Emitter;
use tauri::Manager;
use tauri::State;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use std::sync::atomic::{AtomicBool, Ordering};
use tauri_plugin_notification::NotificationExt;

mod core;
mod providers;
use core::{Checksum, CoreInstallation, CoreInstallationState, CoreManager, CoreRelease};

const CREATE_NO_WINDOW: u32 = 0x08000000;
const GITHUB_USER_AGENT: &str = "zapret-ui-updater";
static CORE_OPERATION_ACTIVE: AtomicBool = AtomicBool::new(false);
static EXIT_IN_PROGRESS: AtomicBool = AtomicBool::new(false);

struct CoreOperationGuard;

impl CoreOperationGuard {
    fn acquire() -> Result<Self, String> {
        CORE_OPERATION_ACTIVE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| {
                "Another core update or rollback operation is already in progress".to_string()
            })?;
        Ok(Self)
    }
}

impl Drop for CoreOperationGuard {
    fn drop(&mut self) {
        CORE_OPERATION_ACTIVE.store(false, Ordering::Release);
    }
}

struct AppState {
    active_strategy: Mutex<Option<String>>,
    test_process_pid: Mutex<Option<u32>>,
    status_item: Mutex<Option<tauri::menu::MenuItem<tauri::Wry>>>,
    strategy_item: Mutex<Option<tauri::menu::MenuItem<tauri::Wry>>>,
    toggle_item: Mutex<Option<tauri::menu::MenuItem<tauri::Wry>>>,
    quit_item: Mutex<Option<tauri::menu::MenuItem<tauri::Wry>>>,
    show_item: Mutex<Option<tauri::menu::MenuItem<tauri::Wry>>>,
    strategies_submenu: Mutex<Option<tauri::menu::Submenu<tauri::Wry>>>,
    tray_handle: Mutex<Option<tauri::tray::TrayIcon<tauri::Wry>>>,
    notification_shown: AtomicBool,
    last_strategy: Mutex<Option<String>>,
    translations: Mutex<Option<TrayTranslations>>,
    temp_process_child: Mutex<Option<std::process::Child>>,
}

#[derive(serde::Deserialize, serde::Serialize, Clone)]
struct TrayTranslations {
    exit: String,
    show: String,
    status_prefix: String,
    strategy_prefix: String,
    toggle_on: String,
    toggle_off: String,
    change_strategy: String,
    minimized_title: String,
    minimized_body: String,
    status_on: String,
    status_off: String,
}

#[derive(serde::Serialize)]
struct ZapretStatus {
    running: bool,
    strategy: Option<String>,
    mode: Option<String>,
}

fn restart_context(
    status: ZapretStatus,
    operation: &str,
) -> Result<Option<(String, String)>, String> {
    if !status.running {
        return Ok(None);
    }
    Ok(Some((
        status.strategy.ok_or_else(|| {
            format!("Cannot {operation} while zapret is running: active strategy is unknown")
        })?,
        status.mode.ok_or_else(|| {
            format!("Cannot {operation} while zapret is running: launch mode is unknown")
        })?,
    )))
}

#[derive(serde::Serialize)]
struct FiltersStatus {
    /// "disabled" | "all" | "tcp" | "udp"
    game_filter: String,
    /// "none" | "any" | "loaded"
    ipset: String,
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Allowed filenames that commands may read/write in the `lists/` directory.
/// Kept in sync with the three files the frontend actually uses.
const ALLOWED_LIST_FILENAMES: &[&str] = &[
    "list-general-user.txt",
    "list-exclude-user.txt",
    "ipset-exclude-user.txt",
];

/// Strategy names come from the frontend and are concatenated into shell
/// commands and filesystem paths. Upstream presets use names like
/// `general (FAKE TLS AUTO ALT2)`, so the allowed charset has to include
/// spaces and parentheses. We still reject path separators, traversal
/// sequences, and shell metacharacters that would be unsafe when the name is
/// substituted into the registry-write / service-creation bat template.
fn is_safe_strategy_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 128 {
        return false;
    }
    if name.contains("..") || name.contains('/') || name.contains('\\') || name.contains(':') {
        return false;
    }
    if name.starts_with('.')
        || name.starts_with('-')
        || name.starts_with(' ')
        || name.ends_with(' ')
    {
        return false;
    }
    name.chars().all(|c| {
        c.is_ascii_alphanumeric()
            || matches!(c, ' ' | '(' | ')' | '[' | ']' | '.' | '_' | '-' | '+' | ',')
    })
}

fn ensure_safe_list_filename(filename: &str) -> Result<(), String> {
    if ALLOWED_LIST_FILENAMES.contains(&filename) {
        Ok(())
    } else {
        Err(format!("Invalid list filename: {}", filename))
    }
}

/// Returns the absolute path to a tool shipped directly in the Windows
/// `System32` directory, falling back to the bare name outside Windows (so
/// unit tests and non-Windows targets still compile / run meaningfully).
///
/// Using absolute paths here avoids `PATH`-based hijacking: a malicious
/// executable placed earlier in `PATH` than System32 could otherwise be picked
/// up when we invoke `sc`, `net`, `taskkill`, `reg`, or `curl`.
///
/// NOTE: this helper is only correct for tools that live directly inside
/// `System32`. `powershell.exe`, for example, is shipped under
/// `System32\WindowsPowerShell\v1.0\powershell.exe`; use `powershell_path()`
/// instead.
fn system32_tool(name: &str) -> PathBuf {
    #[cfg(windows)]
    {
        let system_root =
            std::env::var("SystemRoot").unwrap_or_else(|_| String::from(r"C:\Windows"));
        PathBuf::from(system_root).join("System32").join(name)
    }
    #[cfg(not(windows))]
    {
        PathBuf::from(name)
    }
}

/// Returns the absolute path to the built-in Windows PowerShell 5.x host.
/// On Windows this is always
/// `%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe`.
fn powershell_path() -> PathBuf {
    #[cfg(windows)]
    {
        let system_root =
            std::env::var("SystemRoot").unwrap_or_else(|_| String::from(r"C:\Windows"));
        PathBuf::from(system_root).join(r"System32\WindowsPowerShell\v1.0\powershell.exe")
    }
    #[cfg(not(windows))]
    {
        PathBuf::from("powershell")
    }
}

/// Lightweight shape check for a single entry in a user list. The frontend
/// already validates the same invariants, but the backend must not trust that
/// — we're about to write this string into a file that is read back by the
/// native `winws.exe` driver. Reject anything that would smuggle newlines,
/// comment markers, or shell metacharacters.
fn is_safe_list_entry(entry: &str) -> bool {
    if entry.is_empty() || entry.len() > 253 {
        return false;
    }
    // No control characters, CR/LF, tabs, or NUL.
    if entry.chars().any(|c| c.is_control()) {
        return false;
    }
    // No leading '#': the file format uses '#' to denote comments and zapret
    // must not interpret user entries as comments or be tricked into skipping
    // adjacent lines.
    if entry.starts_with('#') {
        return false;
    }
    // Conservative charset: hostnames, IPv4, IPv4/CIDR, IPv6 literal/CIDR.
    entry
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ':' | '/'))
}

/// Ensures that the requested list file resolves to a path strictly below
/// `<binaries>/lists/`. Protects against symlink-based traversal that would
/// otherwise allow the filename allowlist to be sidestepped at runtime.
fn resolve_list_path(filename: &str) -> Result<PathBuf, String> {
    ensure_safe_list_filename(filename)?;
    let lists_dir = find_binaries_dir().join("lists");
    let file_path = lists_dir.join(filename);

    // If the parent resolves, require the file to resolve inside it. When the
    // file does not yet exist on disk (first write), canonicalize the parent
    // and re-join the bare filename — this still blocks `..` from being smuggled
    // via a symlink at `lists/`.
    let canonical_parent = std::fs::canonicalize(&lists_dir).map_err(|e| {
        format!(
            "Failed to resolve lists directory {}: {}",
            lists_dir.display(),
            e
        )
    })?;
    let canonical_parent = strip_verbatim_prefix(canonical_parent);

    if file_path.exists() {
        let canonical_file = std::fs::canonicalize(&file_path)
            .map_err(|e| format!("Failed to resolve {}: {}", file_path.display(), e))?;
        let canonical_file = strip_verbatim_prefix(canonical_file);
        if !canonical_file.starts_with(&canonical_parent) {
            return Err(format!("List file {} escapes its directory", filename));
        }
        Ok(canonical_file)
    } else {
        Ok(canonical_parent.join(filename))
    }
}

/// Escapes a value for embedding inside a PowerShell single-quoted string.
fn ps_single_quote_escape(s: &str) -> String {
    s.replace('\'', "''")
}

/// Encodes a PowerShell script for `powershell.exe -EncodedCommand`. The
/// expected format is base64 over the UTF-16LE bytes of the script.
///
/// Passing the script via the command-line this way (as opposed to writing a
/// `.bat` into `%TEMP%` and then executing it with `-Verb RunAs`) eliminates a
/// TOCTOU window: other processes under the same user cannot swap in a
/// malicious payload between our write and the elevated execute.
fn encode_powershell_command(script: &str) -> String {
    use base64::Engine;
    let utf16_le: Vec<u8> = script
        .encode_utf16()
        .flat_map(|u| u.to_le_bytes())
        .collect();
    base64::engine::general_purpose::STANDARD.encode(utf16_le)
}

/// Extracts `zip_path` into `dest`, rejecting any entry whose resolved path
/// would escape `dest` (zip-slip). `dest` must already exist.
fn extract_zip_safely(zip_path: &std::path::Path, dest: &std::path::Path) -> Result<(), String> {
    use std::io::Read;

    let file = std::fs::File::open(zip_path)
        .map_err(|e| format!("Failed to open {}: {}", zip_path.display(), e))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| format!("Failed to read zip {}: {}", zip_path.display(), e))?;

    let canonical_dest = std::fs::canonicalize(dest)
        .map(strip_verbatim_prefix)
        .map_err(|e| format!("Failed to canonicalize {}: {}", dest.display(), e))?;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("Failed to read zip entry {}: {}", i, e))?;

        // `enclosed_name` strips `..` and absolute-path components; if the
        // archive still contains something unsafe we bail out entirely.
        let rel = entry
            .enclosed_name()
            .ok_or_else(|| format!("Unsafe path in archive: {:?}", entry.name()))?;
        let out_path = canonical_dest.join(&rel);

        if entry.is_dir() {
            std::fs::create_dir_all(&out_path)
                .map_err(|e| format!("Failed to create dir {}: {}", out_path.display(), e))?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create dir {}: {}", parent.display(), e))?;
        }

        // Defense in depth: even after `enclosed_name` validation, verify the
        // final write target is strictly below `dest`.
        if !out_path.starts_with(&canonical_dest) {
            return Err(format!(
                "Zip entry {} resolves outside destination",
                rel.display()
            ));
        }

        let mut out = std::fs::File::create(&out_path)
            .map_err(|e| format!("Failed to create {}: {}", out_path.display(), e))?;
        let mut buf = [0u8; 16 * 1024];
        loop {
            let n = entry
                .read(&mut buf)
                .map_err(|e| format!("Failed to read {}: {}", rel.display(), e))?;
            if n == 0 {
                break;
            }
            std::io::Write::write_all(&mut out, &buf[..n])
                .map_err(|e| format!("Failed to write {}: {}", out_path.display(), e))?;
        }
    }
    Ok(())
}

/// Computes the SHA-256 digest of a file as a lowercase hex string.
fn sha256_file(path: &std::path::Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;

    let mut file = std::fs::File::open(path)
        .map_err(|e| format!("Failed to open {}: {}", path.display(), e))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex_encode(&hasher.finalize()))
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

/// Fetches the expected SHA-256 digest of the given release asset from the
/// GitHub Releases API. Returns a lowercase hex string on success.
async fn fetch_expected_sha256(version: &str, asset_name: &str) -> Result<String, String> {
    if version.is_empty()
        || !version
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+'))
    {
        return Err(format!("Invalid upstream version tag: {}", version));
    }

    let url = core_manager().provider().release_api_url(version);
    let client = reqwest::Client::builder()
        .user_agent(GITHUB_USER_AGENT)
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {}", e))?;

    let resp = client
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("Failed to fetch release metadata: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!(
            "GitHub API returned status {} for {}",
            resp.status(),
            url
        ));
    }

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse release metadata: {}", e))?;

    let assets = body
        .get("assets")
        .and_then(|a| a.as_array())
        .ok_or_else(|| "Release metadata is missing assets".to_string())?;

    let asset = assets
        .iter()
        .find(|a| a.get("name").and_then(|n| n.as_str()) == Some(asset_name))
        .ok_or_else(|| format!("Asset {} not found in release {}", asset_name, version))?;

    let digest = asset
        .get("digest")
        .and_then(|d| d.as_str())
        .ok_or_else(|| format!("Asset {} has no digest field", asset_name))?;

    let hex = digest
        .strip_prefix("sha256:")
        .ok_or_else(|| format!("Unsupported digest format: {}", digest))?;

    if hex.len() != 64 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("Malformed sha256 digest: {}", digest));
    }

    Ok(hex.to_ascii_lowercase())
}

pub async fn fetch_fallback_release_info(client: &reqwest::Client) -> Result<CoreRelease, String> {
    core_manager().fetch_fallback_release(client).await
}

/// Computes the MD5 digest of a file as a lowercase hex string.
pub fn md5_file(path: &std::path::Path) -> Result<String, String> {
    use std::io::Read;

    let mut file = std::fs::File::open(path)
        .map_err(|e| format!("Failed to open {}: {}", path.display(), e))?;
    let mut context = md5::Context::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
        if n == 0 {
            break;
        }
        context.consume(&buf[..n]);
    }
    let digest = context.compute();
    Ok(format!("{:x}", digest))
}

/// On Windows, `std::fs::canonicalize` returns the verbatim/extended-length
/// form (e.g. `\\?\C:\foo\bar`). That form is fine for Rust's file APIs but
/// breaks `cmd.exe` and downstream `.bat` scripts, which refuse to use it as
/// current directory. Strip the `\\?\` prefix for normal drive paths and
/// leave UNC/network paths alone.
fn strip_verbatim_prefix(path: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        let s = match path.to_str() {
            Some(s) => s,
            None => return path,
        };
        if let Some(rest) = s.strip_prefix(r"\\?\") {
            // Drive-letter form: "C:\..." — safe to strip.
            let bytes = rest.as_bytes();
            if bytes.len() >= 3
                && bytes[0].is_ascii_alphabetic()
                && bytes[1] == b':'
                && bytes[2] == b'\\'
            {
                return PathBuf::from(rest);
            }
            // Verbatim UNC "\\?\UNC\server\share" — rewrite to "\\server\share".
            if let Some(unc_rest) = rest.strip_prefix("UNC\\") {
                let mut out = String::from(r"\\");
                out.push_str(unc_rest);
                return PathBuf::from(out);
            }
        }
    }
    path
}

/// Returns `path` with symlinks resolved if it exists; otherwise returns
/// `path` unchanged. We canonicalize every resolved `binaries/` root so that
/// callers composing paths via `dir.join(...)` can't be fooled by a symlink
/// swap after the initial existence check.
fn canonicalize_or_passthrough(path: PathBuf) -> PathBuf {
    match std::fs::canonicalize(&path) {
        Ok(p) => strip_verbatim_prefix(p),
        Err(_) => path,
    }
}

fn find_binaries_dir() -> PathBuf {
    // 1. Direct sibling of the exe (production after first download)
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let candidate = parent.join("binaries");
            if candidate.exists() {
                return canonicalize_or_passthrough(candidate);
            }
        }
    }

    // 2. Climb up from exe (dev mode: exe is deep inside src-tauri/target/debug)
    if let Ok(exe) = std::env::current_exe() {
        let mut dir = exe.parent().map(|p| p.to_path_buf());
        for _ in 0..5 {
            if let Some(d) = &dir {
                let candidate = d.join("binaries");
                if candidate.exists() {
                    return canonicalize_or_passthrough(candidate);
                }
                dir = d.parent().map(|p| p.to_path_buf());
            } else {
                break;
            }
        }
    }

    // 3. CWD fallback (tauri dev)
    if let Ok(cwd) = std::env::current_dir() {
        let candidate = cwd.join("binaries");
        if candidate.exists() {
            return canonicalize_or_passthrough(candidate);
        }
    }

    // 4. Default: next to exe (will be created on first download). Don't
    // canonicalize — the directory doesn't exist yet and canonicalize() would
    // fail on Windows in that case.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            return parent.join("binaries");
        }
    }

    PathBuf::from("binaries")
}

fn core_manager_at(root: impl Into<PathBuf>) -> CoreManager {
    CoreManager::new(root)
}

fn core_manager() -> CoreManager {
    core_manager_at(find_binaries_dir())
}

fn is_admin() -> bool {
    // net session — самый быстрый и надежный способ проверки прав администратора на Windows
    Command::new(system32_tool("net.exe"))
        .arg("session")
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn elevate_if_needed() {
    if !is_admin() {
        if let Ok(exe) = std::env::current_exe() {
            let args: Vec<String> = std::env::args().skip(1).collect();

            let ps_args = if args.is_empty() {
                String::new()
            } else {
                let formatted = args
                    .iter()
                    .map(|s| format!("'{}'", s.replace("'", "''")))
                    .collect::<Vec<String>>()
                    .join(",");
                format!("-ArgumentList @({})", formatted)
            };

            let ps_command = format!(
                "Start-Process -FilePath '{}' {} -Verb RunAs",
                exe.to_string_lossy().replace("'", "''"),
                ps_args
            );

            let _ = Command::new(powershell_path())
                .args([
                    "-NoProfile",
                    "-WindowStyle",
                    "Hidden",
                    "-Command",
                    &ps_command,
                ])
                .creation_flags(CREATE_NO_WINDOW)
                .spawn();

            std::process::exit(0);
        }
    }
}

fn get_local_version() -> String {
    core_manager().provider().local_version()
}

#[tauri::command]
fn get_update_proxy() -> Option<String> {
    option_env!("ZAPRET_UPDATE_PROXY").map(|s| s.to_string())
}

#[tauri::command]
fn get_local_version_cmd() -> String {
    get_local_version()
}

#[tauri::command]
async fn get_remote_core_version(
    use_proxy: Option<bool>,
    custom_proxy: Option<String>,
) -> Result<String, String> {
    let mut client_builder = reqwest::Client::builder();

    let use_proxy = use_proxy.unwrap_or(false);
    if use_proxy {
        let proxy_url =
            custom_proxy.or_else(|| option_env!("ZAPRET_UPDATE_PROXY").map(|s| s.to_string()));
        if let Some(proxy_url) = proxy_url {
            if let Ok(proxy) = reqwest::Proxy::all(&proxy_url) {
                client_builder = client_builder.proxy(proxy);
            }
        }
    }

    let client = client_builder.build().map_err(|e| e.to_string())?;

    let response_res = client
        .get(core_manager().provider().version_url())
        .send()
        .await;

    match response_res {
        Ok(resp) => {
            let status = resp.status();
            if status.is_success() {
                if let Ok(text) = resp.text().await {
                    let trimmed = text.trim();
                    if !trimmed.is_empty() && !trimmed.contains("Not Found") {
                        return Ok(trimmed.to_string());
                    }
                }
            }
            if let Ok(info) = fetch_fallback_release_info(&client).await {
                return Ok(info.version);
            }
            Err(format!(
                "GitHub returned status {} and SourceForge fallback failed",
                status
            ))
        }
        Err(e) => {
            if let Ok(info) = fetch_fallback_release_info(&client).await {
                return Ok(info.version);
            }
            Err(format!(
                "Failed to connect to GitHub ({}) and SourceForge fallback failed",
                e
            ))
        }
    }
}

fn get_ui_version() -> String {
    // APP_VERSION is dynamically set by build.rs from tauri.conf.json
    env!("APP_VERSION").to_string()
}

#[tauri::command]
fn get_ui_version_cmd() -> String {
    get_ui_version()
}

#[tauri::command]
fn ensure_binaries_present() -> bool {
    core_manager().provider().is_installed()
}

fn parse_bat_args(strategy: &str) -> Result<String, String> {
    if !is_safe_strategy_name(strategy) {
        return Err(format!("Invalid strategy name: {}", strategy));
    }
    let filters = get_filters_status();
    core_manager()
        .provider()
        .parse_strategy(strategy, &filters.game_filter)
}

/// Splits a command-line arguments string into separate arguments, respecting double quotes
fn split_arguments(s: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    for c in s.chars() {
        if c == '"' {
            in_quotes = !in_quotes;
        } else if c.is_whitespace() && !in_quotes {
            if !current.is_empty() {
                args.push(current.clone());
                current.clear();
            }
        } else {
            current.push(c);
        }
    }
    if !current.is_empty() {
        args.push(current);
    }
    args
}

/// Проверяет, запущен ли winws.exe через tasklist.
fn is_zapret_service_running() -> bool {
    let output = Command::new(system32_tool("sc.exe"))
        .args(["query", "zapret"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_lowercase();
            stdout.contains("running") || stdout.contains("start_pending")
        }
        Err(_) => false,
    }
}

/// Проверяет, установлена ли служба `zapret` (даже если сейчас остановлена).
/// `sc query` пишет "the specified service does not exist as an installed service"
/// в stderr при отсутствии службы.
fn is_zapret_service_installed() -> bool {
    let output = Command::new(system32_tool("sc.exe"))
        .args(["query", "zapret"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    match output {
        Ok(out) => {
            let combined = format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            )
            .to_lowercase();
            // Наличие строки "service_name" и отсутствие "does not exist"
            out.status.success()
                && combined.contains("service_name")
                && !combined.contains("does not exist")
        }
        Err(_) => false,
    }
}

fn is_winws_running() -> bool {
    let output = Command::new("tasklist")
        .args(["/fi", "IMAGENAME eq winws.exe", "/fo", "csv", "/nh"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    match output {
        Ok(out) => String::from_utf8_lossy(&out.stdout)
            .to_lowercase()
            .contains("winws.exe"),
        Err(_) => false,
    }
}

/// Читает активную стратегию из реестра Windows
/// (записывается при установке zapret как Windows-сервис).
fn get_strategy_from_registry() -> Option<String> {
    let out = Command::new(system32_tool("reg.exe"))
        .args([
            "query",
            "HKLM\\System\\CurrentControlSet\\Services\\zapret",
            "/v",
            "zapret-discord-youtube",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;

    let stdout = String::from_utf8_lossy(&out.stdout);
    // Строка: "    zapret-discord-youtube    REG_SZ    general (ALT)"
    for line in stdout.lines() {
        if line.contains("REG_SZ") {
            if let Some(pos) = line.find("REG_SZ") {
                let value = line[pos + "REG_SZ".len()..].trim();
                if !value.is_empty() {
                    return Some(value.to_string());
                }
            }
        }
    }
    None
}

// ─── Commands ─────────────────────────────────────────────────────────────────

#[tauri::command]
fn check_status_full() -> Result<String, String> {
    let mut output = String::new();

    // 1. Check Strategy
    let reg_out = Command::new(system32_tool("reg.exe"))
        .args([
            "query",
            "HKLM\\System\\CurrentControlSet\\Services\\zapret",
            "/v",
            "zapret-discord-youtube",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    if let Ok(out) = reg_out {
        let stdout = String::from_utf8_lossy(&out.stdout);
        for line in stdout.lines() {
            if let Some(pos) = line.find("REG_SZ") {
                let strategy = line[pos + "REG_SZ".len()..].trim();
                if !strategy.is_empty() {
                    output.push_str(&format!(
                        "Service strategy installed from \"{}\"\n",
                        strategy
                    ));
                }
                break;
            }
        }
    }

    // 2. Check zapret service
    let zapret_svc = Command::new(system32_tool("sc.exe"))
        .args(["query", "zapret"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    if let Ok(out) = zapret_svc {
        let stdout = String::from_utf8_lossy(&out.stdout);
        if stdout.contains("RUNNING") {
            output.push_str("\"zapret\" service is RUNNING.\n");
        } else if stdout.contains("STOPPED") {
            output.push_str("\"zapret\" service is STOPPED.\n");
        } else if stdout.contains("FAILED 1060") || stdout.contains("1060") {
            // 1060 means service does not exist
        } else {
            // Might be start_pending or other
            output.push_str("\"zapret\" service state is UNKNOWN.\n");
        }
    }

    // 3. Check WinDivert service
    let windivert_svc = Command::new(system32_tool("sc.exe"))
        .args(["query", "WinDivert"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    if let Ok(out) = windivert_svc {
        let stdout = String::from_utf8_lossy(&out.stdout);
        if stdout.contains("RUNNING") {
            output.push_str("\"WinDivert\" service is RUNNING.\n");
        } else if stdout.contains("STOPPED") {
            output.push_str("\"WinDivert\" service is STOPPED.\n");
        }
    }

    // 4. Check bypass (winws.exe)
    output.push('\n');
    let task = Command::new(system32_tool("tasklist.exe"))
        .args(["/FI", "IMAGENAME eq winws.exe"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    if let Ok(out) = task {
        let stdout = String::from_utf8_lossy(&out.stdout).to_lowercase();
        if stdout.contains("winws.exe") {
            output.push_str("Bypass (winws.exe) is RUNNING.\n");
        } else {
            output.push_str("Bypass (winws.exe) is NOT running.\n");
        }
    }

    let trimmed = output.trim().to_string();
    if trimmed.is_empty() {
        Ok("Zapret service is not installed.".to_string())
    } else {
        Ok(trimmed)
    }
}

/// Список стратегий — имена .bat файлов из binaries/ (без service.bat).
#[tauri::command]
fn get_strategies() -> Result<Vec<String>, String> {
    core_manager().provider().strategies()
}

/// Compare strings using natural sort (numbers compared numerically)
fn natural_sort_compare(a: &str, b: &str) -> std::cmp::Ordering {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();

    let mut i = 0;
    let mut j = 0;

    while i < a_chars.len() && j < b_chars.len() {
        let ca = a_chars[i];
        let cb = b_chars[j];

        // If both are digits, compare the full numbers
        if ca.is_ascii_digit() && cb.is_ascii_digit() {
            // Extract full number from a
            let mut num_a = 0u32;
            let start_i = i;
            while i < a_chars.len() && a_chars[i].is_ascii_digit() {
                num_a = num_a * 10 + (a_chars[i] as u32 - '0' as u32);
                i += 1;
            }

            // Extract full number from b
            let mut num_b = 0u32;
            let start_j = j;
            while j < b_chars.len() && b_chars[j].is_ascii_digit() {
                num_b = num_b * 10 + (b_chars[j] as u32 - '0' as u32);
                j += 1;
            }

            // Compare numbers
            if num_a != num_b {
                return num_a.cmp(&num_b);
            }

            // Numbers are equal but different lengths (e.g., "01" vs "1")
            let len_a = i - start_i;
            let len_b = j - start_j;
            if len_a != len_b {
                return len_a.cmp(&len_b);
            }
        } else {
            // Compare characters case-insensitively
            let cmp = ca.to_ascii_lowercase().cmp(&cb.to_ascii_lowercase());
            if cmp != std::cmp::Ordering::Equal {
                return cmp;
            }
            i += 1;
            j += 1;
        }
    }

    // If one string is exhausted, the shorter one comes first
    a_chars.len().cmp(&b_chars.len())
}

/// Текущий статус zapret: запущен ли и какая стратегия.
#[tauri::command]
fn get_zapret_status(state: State<'_, AppState>) -> ZapretStatus {
    let mut running = is_winws_running();
    let is_service = is_zapret_service_running();
    if is_service {
        running = true;
    }

    let mut strategy_lock = state.active_strategy.lock_unpoisoned();

    if !running {
        *strategy_lock = None;
        return ZapretStatus {
            running: false,
            strategy: None,
            mode: None,
        };
    }

    let mode = if is_service {
        Some("service".to_string())
    } else {
        Some("temporary".to_string())
    };

    if strategy_lock.is_some() {
        return ZapretStatus {
            running: true,
            strategy: strategy_lock.clone(),
            mode,
        };
    }

    // Пробуем определить из реестра (если запущен как Windows-сервис)
    let from_reg = get_strategy_from_registry();
    if from_reg.is_some() {
        *strategy_lock = from_reg.clone();
    }

    ZapretStatus {
        running: true,
        strategy: from_reg,
        mode,
    }
}

/// Состояние Game Filter и IPSet Filter по файлам конфигурации.
#[tauri::command]
fn get_filters_status() -> FiltersStatus {
    let dir = find_binaries_dir();

    // ── Game Filter: binaries/utils/game_filter.enabled ──
    // Консольная версия: отсутствие файла = disabled
    let game_flag = dir.join("utils").join("game_filter.enabled");
    let game_filter = if !game_flag.exists() {
        "disabled".to_string()
    } else {
        let content = std::fs::read_to_string(&game_flag).unwrap_or_default();
        // Убираем BOM, пробелы, CRLF
        let mode = content.trim_start_matches('\u{FEFF}').trim().to_lowercase();
        match mode.as_str() {
            "tcp" => "tcp".to_string(),
            "udp" => "udp".to_string(),
            _ => "all".to_string(),
        }
    };

    // ── IPSet Filter: binaries/lists/ipset-all.txt ──
    let ipset_file = dir.join("lists").join("ipset-all.txt");
    let ipset = if !ipset_file.exists() {
        "any".to_string()
    } else {
        let content = std::fs::read_to_string(&ipset_file).unwrap_or_default();
        let lines: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();
        if lines.is_empty() {
            "any".to_string()
        } else if lines.iter().any(|l| l.trim() == "203.0.113.113/32") {
            "none".to_string()
        } else {
            "loaded".to_string()
        }
    };

    FiltersStatus { game_filter, ipset }
}

#[tauri::command]
fn set_game_filter(mode: String) -> Result<(), String> {
    let dir = find_binaries_dir();
    let game_flag = dir.join("utils").join("game_filter.enabled");

    if mode == "disabled" {
        // Удаляем файл для совместимости с консольной версией
        // Консольная версия считает отсутствие файла = disabled
        if game_flag.exists() {
            let _ = std::fs::remove_file(&game_flag);
        }
    } else {
        std::fs::write(&game_flag, mode).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn set_ipset_filter(mode: String) -> Result<(), String> {
    let dir = find_binaries_dir();
    let ipset_file = dir.join("lists").join("ipset-all.txt");
    let backup_file = dir.join("lists").join("ipset-all.txt.backup");

    match mode.as_str() {
        "none" => {
            // Записываем dummy IP для состояния none
            std::fs::write(&ipset_file, "203.0.113.113/32\n").map_err(|e| e.to_string())?;
        }
        "any" => {
            // Перед тем как сделать пустой файл, сохраняем бэкап если есть реальные данные
            // (не пустой и не содержащий dummy IP)
            if ipset_file.exists() && !backup_file.exists() {
                let content = std::fs::read_to_string(&ipset_file).unwrap_or_default();
                let lines: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();
                if !lines.is_empty() && !lines.iter().any(|l| l.trim() == "203.0.113.113/32") {
                    std::fs::copy(&ipset_file, &backup_file).map_err(|e| e.to_string())?;
                }
            }
            // Создаем пустой файл
            std::fs::write(&ipset_file, "").map_err(|e| e.to_string())?;
        }
        "loaded" => {
            // Восстанавливаем из бэкапа если он есть и содержит реальные данные
            if backup_file.exists() {
                let backup_content = std::fs::read_to_string(&backup_file).unwrap_or_default();
                let backup_lines: Vec<&str> = backup_content
                    .lines()
                    .filter(|l| !l.trim().is_empty())
                    .collect();
                // Проверяем что бэкап не содержит dummy IP
                if !backup_lines.is_empty()
                    && !backup_lines.iter().any(|l| l.trim() == "203.0.113.113/32")
                {
                    std::fs::copy(&backup_file, &ipset_file).map_err(|e| e.to_string())?;
                } else {
                    // Бэкап поврежден (содержит none), создаем дефолтный
                    let default_ips = "185.65.148.0/22\n192.229.128.0/17\n";
                    std::fs::write(&ipset_file, default_ips).map_err(|e| e.to_string())?;
                }
            } else {
                // Если бэкапа нет, создаем дефолтный список
                let default_ips = "185.65.148.0/22\n192.229.128.0/17\n";
                std::fs::write(&ipset_file, default_ips).map_err(|e| e.to_string())?;
            }
        }
        _ => return Err(format!("Invalid IPSet mode: {}", mode)),
    }

    Ok(())
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct FakeFileItem {
    pub name: String,
    pub filename: String,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct FakesInfo {
    pub current_discord_fake: String,
    pub current_game_fake: String,
    pub available_fakes: Vec<FakeFileItem>,
}

/// Возвращает информацию о доступных фейках и текущих активных фейках
#[tauri::command]
fn get_fakes_info() -> Result<FakesInfo, String> {
    let bin_dir = find_binaries_dir().join("bin");
    if !bin_dir.exists() {
        return Err("bin folder not found".to_string());
    }

    let discord_active_path = bin_dir.join("ACTIVE_DISCORD_UDP.bin");
    let game_active_path = bin_dir.join("ACTIVE_GAME_UDP.bin");

    let discord_hash = if discord_active_path.exists() {
        sha256_file(&discord_active_path).ok()
    } else {
        None
    };

    let game_hash = if game_active_path.exists() {
        sha256_file(&game_active_path).ok()
    } else {
        None
    };

    let mut available_fakes = Vec::new();
    let mut fake_hashes: Vec<(String, String)> = Vec::new();

    if let Ok(entries) = std::fs::read_dir(&bin_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("bin") {
                if let Some(file_name_str) = path.file_name().and_then(|s| s.to_str()) {
                    if file_name_str.starts_with("ACTIVE_") {
                        continue;
                    }
                    let base_name = path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("")
                        .to_string();
                    if base_name.is_empty() {
                        continue;
                    }
                    let hash = sha256_file(&path).unwrap_or_default();
                    fake_hashes.push((base_name.clone(), hash));
                    available_fakes.push(FakeFileItem {
                        name: base_name,
                        filename: file_name_str.to_string(),
                    });
                }
            }
        }
    }

    available_fakes.sort_by(|a, b| natural_sort_compare(&a.name, &b.name));
    fake_hashes.sort_by(|a, b| natural_sort_compare(&a.0, &b.0));

    let mut current_discord_fake = "quic_initial_steamcommunity_com".to_string();
    if let Some(ref d_hash) = discord_hash {
        for (name, hash) in &fake_hashes {
            if hash.eq_ignore_ascii_case(d_hash) {
                current_discord_fake = name.clone();
            }
        }
    }

    // Determine current GameFilter fake
    let mut current_game_fake = "quic_initial_dbankcloud_ru".to_string();
    if let Some(ref g_hash) = game_hash {
        for (name, hash) in &fake_hashes {
            if hash.eq_ignore_ascii_case(g_hash) {
                current_game_fake = name.clone();
            }
        }
    }

    Ok(FakesInfo {
        current_discord_fake,
        current_game_fake,
        available_fakes,
    })
}

/// Заменяет активный фейк (discord или game) на указанный файл-фейк
#[tauri::command]
fn set_active_fake(fake_type: String, fake_name: String) -> Result<(), String> {
    let bin_dir = find_binaries_dir().join("bin");
    if !bin_dir.exists() {
        return Err("bin folder not found".to_string());
    }

    let target_bin = match fake_type.as_str() {
        "discord" => "ACTIVE_DISCORD_UDP.bin",
        "game" => "ACTIVE_GAME_UDP.bin",
        _ => return Err("Invalid fake type".to_string()),
    };

    if fake_name.contains('/') || fake_name.contains('\\') || fake_name.contains("..") {
        return Err("Invalid fake file name".to_string());
    }

    let source_path = bin_dir.join(format!("{}.bin", fake_name));
    if !source_path.exists() {
        return Err(format!("Fake file '{}.bin' not found", fake_name));
    }

    let target_bin_path = bin_dir.join(target_bin);

    if target_bin_path.exists() {
        let _ = std::fs::remove_file(&target_bin_path);
    }

    std::fs::copy(&source_path, &target_bin_path)
        .map_err(|e| format!("Failed to replace active fake: {}", e))?;

    Ok(())
}

/// Запускает стратегию по имени .bat файла.
#[tauri::command]
fn start_zapret(
    _app: tauri::AppHandle,
    strategy: String,
    mode: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    if !is_safe_strategy_name(&strategy) {
        return Err(format!("Invalid strategy name: {}", strategy));
    }
    // Frontend's "one-shot" button sends `temporary`; a couple of code paths
    // still reference `temp` historically. Accept both, and anything else is
    // rejected. Only `service` takes the elevated branch below.
    let mode_is_service = mode == "service";
    let mode_is_temp = mode == "temporary" || mode == "temp";
    if !mode_is_service && !mode_is_temp {
        return Err(format!("Invalid mode: {}", mode));
    }

    // Убиваем текущий процесс
    let _ = Command::new(system32_tool("taskkill.exe"))
        .args(["/f", "/im", "winws.exe"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();

    let dir = find_binaries_dir();
    let bat_path = dir.join(format!("{}.bat", strategy));
    if !bat_path.exists() {
        return Err(format!("Файл стратегии не найден: {}.bat", strategy));
    }

    // Убеждаемся, что пользовательские списки существуют, иначе winws не запустится
    let lists_dir = dir.join("lists");
    if !lists_dir.exists() {
        let _ = std::fs::create_dir_all(&lists_dir);
    }
    let ipset_user = lists_dir.join("ipset-exclude-user.txt");
    if !ipset_user.exists() {
        let _ = std::fs::write(&ipset_user, "203.0.113.113/32\r\n");
    }
    let list_general_user = lists_dir.join("list-general-user.txt");
    if !list_general_user.exists() {
        let _ = std::fs::write(&list_general_user, "domain.example.abc\r\n");
    }
    let list_exclude_user = lists_dir.join("list-exclude-user.txt");
    if !list_exclude_user.exists() {
        let _ = std::fs::write(&list_exclude_user, "domain.example.abc\r\n");
    }

    if mode == "service" {
        let args = parse_bat_args(&strategy)?;

        // Canonicalize winws.exe before writing it into the service binPath in
        // the registry. That way the service points at the *real* executable
        // under `binaries/bin/`, not at a symlink that could later be
        // redirected to an attacker-controlled binary.
        let bin_path_raw = core_manager_at(&dir).provider().winws_executable();
        let bin_path = std::fs::canonicalize(&bin_path_raw)
            .map(strip_verbatim_prefix)
            .map_err(|e| format!("Failed to resolve {}: {}", bin_path_raw.display(), e))?;
        if !bin_path.starts_with(&dir) {
            return Err(format!(
                "winws.exe resolves outside binaries dir: {}",
                bin_path.display()
            ));
        }
        let bin_str = bin_path.to_str().unwrap_or_default();

        // Проверяем что аргументы не пустые
        if args.is_empty() {
            return Err("Не удалось распарсить аргументы из bat файла".to_string());
        }

        // Собираем PowerShell-скрипт, который:
        //   1) останавливает и удаляет старый сервис zapret (если есть),
        //   2) регистрирует новый через New-Service — тот принимает binPath
        //      как обычную .NET-строку и сам корректно передаёт её в SCM,
        //      поэтому нам не нужно вручную экранировать кавычки для
        //      sc.exe / cmd.exe,
        //   3) стартует сервис и сохраняет имя стратегии в реестре.
        //
        // Скрипт запускается через `powershell.exe -EncodedCommand <base64>`
        // под Start-Process -Verb RunAs. Это убирает промежуточный temp .bat,
        // в который раньше можно было подменить содержимое между записью и
        // elevated-исполнением (TOCTOU).
        let ps_script = format!(
            r#"$ErrorActionPreference = 'Continue'
$exe = '{exe}'
$svcArgs = '{args}'
$strategy = '{strategy}'
$binPath = '"' + $exe + '" ' + $svcArgs
try {{ Stop-Service -Name zapret -Force -ErrorAction SilentlyContinue }} catch {{}}
if (Get-Service -Name zapret -ErrorAction SilentlyContinue) {{
    & "$env:SystemRoot\System32\sc.exe" delete zapret | Out-Null
}}
New-Service -Name zapret `
    -BinaryPathName $binPath `
    -StartupType Automatic `
    -DisplayName 'zapret' `
    -Description 'Zapret DPI bypass software' | Out-Null
try {{
    Start-Service -Name zapret -ErrorAction Stop
}} catch {{
    & "$env:SystemRoot\System32\sc.exe" query zapret
    exit 1
}}
& "$env:SystemRoot\System32\reg.exe" add 'HKLM\System\CurrentControlSet\Services\zapret' /v zapret-discord-youtube /t REG_SZ /d $strategy /f | Out-Null
"#,
            exe = ps_single_quote_escape(bin_str),
            args = ps_single_quote_escape(&args),
            strategy = ps_single_quote_escape(&strategy),
        );

        let encoded = encode_powershell_command(&ps_script);
        let mut cmd = Command::new(powershell_path());
        cmd.args([
            "-NoProfile",
            "-WindowStyle",
            "Hidden",
            "-Command",
            "Start-Process -FilePath powershell -Verb RunAs -Wait -WindowStyle Hidden -ArgumentList @('-NoProfile','-WindowStyle','Hidden','-EncodedCommand',$env:ZAPRET_PS_PAYLOAD)",
        ]);
        cmd.env("ZAPRET_PS_PAYLOAD", &encoded);
        #[cfg(windows)]
        cmd.creation_flags(CREATE_NO_WINDOW);

        match cmd.output() {
            Ok(out) => {
                if !out.status.success() {
                    let stderr = String::from_utf8_lossy(&out.stderr);
                    return Err(format!("Ошибка запуска сервиса: {}", stderr));
                }
            }
            Err(e) => {
                return Err(format!("Не удалось запустить PowerShell: {}", e));
            }
        }
    } else {
        let bin_path = core_manager_at(&dir).provider().winws_executable();
        if !bin_path.exists() {
            return Err("winws.exe not found".to_string());
        }

        let args_str = parse_bat_args(&strategy)?;
        let args = split_arguments(&args_str);

        let mut cmd = Command::new(&bin_path);
        cmd.args(&args);
        cmd.stdout(std::process::Stdio::null());
        cmd.stderr(std::process::Stdio::null());
        #[cfg(windows)]
        cmd.creation_flags(CREATE_NO_WINDOW);

        let child = cmd
            .spawn()
            .map_err(|e| format!("Не удалось запустить winws.exe напрямую: {}", e))?;

        *state.temp_process_child.lock_unpoisoned() = Some(child);
    }

    *state.active_strategy.lock_unpoisoned() = Some(strategy.clone());
    *state.last_strategy.lock_unpoisoned() = Some(strategy);
    Ok("Connected".into())
}

fn stop_zapret_internal() -> Result<(), String> {
    let ps_script = r#"$ErrorActionPreference = 'Continue'
$sys = "$env:SystemRoot\System32"
try { Stop-Service -Name zapret -Force -ErrorAction SilentlyContinue } catch {}
if (Get-Service -Name zapret -ErrorAction SilentlyContinue) {
    & "$sys\sc.exe" delete zapret | Out-Null
}
& "$sys\taskkill.exe" /F /IM winws.exe 2>$null | Out-Null
foreach ($svc in @('WinDivert','WinDivert14')) {
    try { Stop-Service -Name $svc -Force -ErrorAction SilentlyContinue } catch {}
    if (Get-Service -Name $svc -ErrorAction SilentlyContinue) {
        & "$sys\sc.exe" delete $svc | Out-Null
    }
}
"#;
    let encoded = encode_powershell_command(ps_script);
    let status = Command::new(powershell_path())
        .args([
            "-NoProfile",
            "-WindowStyle",
            "Hidden",
            "-Command",
            "Start-Process -FilePath powershell -Verb RunAs -Wait -WindowStyle Hidden -ArgumentList @('-NoProfile','-WindowStyle','Hidden','-EncodedCommand',$env:ZAPRET_PS_PAYLOAD)",
        ])
        .env("ZAPRET_PS_PAYLOAD", &encoded)
        .creation_flags(CREATE_NO_WINDOW)
        .status();

    match status {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(format!("PowerShell process exited with status: {}", s)),
        Err(e) => Err(format!("Failed to start PowerShell: {}", e)),
    }
}

/// Полностью останавливает zapret.
/// Требует прав администратора — запрашивает их через PowerShell -Verb RunAs.
#[tauri::command]
fn stop_zapret(state: State<'_, AppState>) {
    {
        let mut child_lock = state.temp_process_child.lock_unpoisoned();
        if let Some(mut child) = child_lock.take() {
            let _ = child.kill();
        }
    }
    let _ = stop_zapret_internal();
    *state.active_strategy.lock_unpoisoned() = None;
}

fn stop_zapret_on_exit(state: State<'_, AppState>) {
    {
        let mut child_lock = state.temp_process_child.lock_unpoisoned();
        if let Some(mut child) = child_lock.take() {
            let _ = child.kill();
            let _ = Command::new(system32_tool("taskkill.exe"))
                .arg("/F")
                .arg("/IM")
                .arg("winws.exe")
                .creation_flags(CREATE_NO_WINDOW)
                .output();
        }
    }
}

// ─── User Lists Management ────────────────────────────────────────────────────

/// Reads lines from a file in the lists directory, filtering out comments and empty lines
#[tauri::command]
fn read_user_list(filename: String) -> Result<Vec<String>, String> {
    let file_path = resolve_list_path(&filename)?;

    if !file_path.exists() {
        return Ok(Vec::new());
    }

    let content = std::fs::read_to_string(&file_path)
        .map_err(|e| format!("Failed to read {}: {}", filename, e))?;

    let lines: Vec<String> = content
        .lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|s| s.to_string())
        .collect();

    Ok(lines)
}

/// Writes lines to a file in the lists directory
#[tauri::command]
fn write_user_list(filename: String, lines: Vec<String>) -> Result<(), String> {
    let file_path = resolve_list_path(&filename)?;

    for line in &lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if !is_safe_list_entry(trimmed) {
            return Err(format!("Invalid list entry: {}", trimmed));
        }
    }

    let content = lines.join("\r\n");
    std::fs::write(&file_path, content)
        .map_err(|e| format!("Failed to write {}: {}", filename, e))?;

    Ok(())
}

/// Adds a line to a user list file
#[tauri::command]
fn add_to_user_list(filename: String, entry: String) -> Result<(), String> {
    let file_path = resolve_list_path(&filename)?;
    let entry_trimmed = entry.trim();
    if !is_safe_list_entry(entry_trimmed) {
        return Err(format!("Invalid list entry: {}", entry_trimmed));
    }

    let mut lines = if file_path.exists() {
        let content = std::fs::read_to_string(&file_path)
            .map_err(|e| format!("Failed to read {}: {}", filename, e))?;
        content
            .lines()
            .map(|line| line.trim())
            .filter(|line| !line.is_empty())
            .map(|s| s.to_string())
            .collect::<Vec<String>>()
    } else {
        Vec::new()
    };

    // Check for duplicates
    if !lines.iter().any(|l| l.trim() == entry_trimmed) {
        lines.push(entry_trimmed.to_string());
        let content = lines.join("\r\n");
        std::fs::write(&file_path, content)
            .map_err(|e| format!("Failed to write {}: {}", filename, e))?;
    }

    Ok(())
}

/// Removes a line from a user list file
#[tauri::command]
fn remove_from_user_list(filename: String, entry: String) -> Result<(), String> {
    let file_path = resolve_list_path(&filename)?;

    if !file_path.exists() {
        return Ok(());
    }

    let content = std::fs::read_to_string(&file_path)
        .map_err(|e| format!("Failed to read {}: {}", filename, e))?;

    let entry_trimmed = entry.trim();
    let lines: Vec<String> = content
        .lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty() && line.trim() != entry_trimmed)
        .map(|s| s.to_string())
        .collect();

    let content = lines.join("\r\n");
    std::fs::write(&file_path, content)
        .map_err(|e| format!("Failed to write {}: {}", filename, e))?;

    Ok(())
}

/// Saves user list to a text file selected by the user via Save File Dialog
#[tauri::command]
fn save_user_list_to_file(filename: String) -> Result<bool, String> {
    let lines = read_user_list(filename.clone())?;
    let content = lines.join("\r\n");

    let default_name = if filename.contains("general") {
        "list-general-user.txt"
    } else if filename.contains("exclude") {
        "list-exclude-user.txt"
    } else {
        "user-list.txt"
    };

    let file_path = rfd::FileDialog::new()
        .set_title("Сохранить список как...")
        .add_filter("Text Files (*.txt)", &["txt"])
        .set_file_name(default_name)
        .save_file();

    if let Some(path) = file_path {
        std::fs::write(&path, content).map_err(|e| format!("Не удалось сохранить файл: {}", e))?;
        Ok(true)
    } else {
        Ok(false)
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct BackupData {
    version: u32,
    include: Vec<String>,
    exclude: Vec<String>,
    ips: Vec<String>,
}

/// Exports all lists (include, exclude, ips) to a JSON backup file
#[tauri::command]
fn export_backup_file() -> Result<bool, String> {
    let include = read_user_list("list-general-user.txt".to_string()).unwrap_or_default();
    let exclude = read_user_list("list-exclude-user.txt".to_string()).unwrap_or_default();
    let ips = read_user_list("ipset-exclude-user.txt".to_string()).unwrap_or_default();

    let backup = BackupData {
        version: 1,
        include,
        exclude,
        ips,
    };

    let content = serde_json::to_string_pretty(&backup)
        .map_err(|e| format!("Не удалось сериализовать резервную копию: {}", e))?;

    let file_path = rfd::FileDialog::new()
        .set_title("Сохранить резервную копию...")
        .add_filter("JSON Files (*.json)", &["json"])
        .set_file_name("zapret-backup.json")
        .save_file();

    if let Some(path) = file_path {
        std::fs::write(&path, content)
            .map_err(|e| format!("Не удалось сохранить файл резервной копии: {}", e))?;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Imports all lists (include, exclude, ips) from a JSON backup file
#[tauri::command]
fn import_backup_file() -> Result<bool, String> {
    let file_path = rfd::FileDialog::new()
        .set_title("Восстановить резервную копию...")
        .add_filter("JSON Files (*.json)", &["json"])
        .pick_file();

    if let Some(path) = file_path {
        let content = std::fs::read_to_string(&path)
            .map_err(|e| format!("Не удалось прочитать файл резервной копии: {}", e))?;

        let backup: BackupData = serde_json::from_str(&content)
            .map_err(|e| format!("Неверный формат файла резервной копии: {}", e))?;

        write_user_list("list-general-user.txt".to_string(), backup.include)?;
        write_user_list("list-exclude-user.txt".to_string(), backup.exclude)?;
        write_user_list("ipset-exclude-user.txt".to_string(), backup.ips)?;

        Ok(true)
    } else {
        Ok(false)
    }
}

/// Updates the IPSet list from remote source (same as service.bat)
#[tauri::command]
async fn update_ipset_list() -> Result<String, String> {
    let dir = find_binaries_dir();
    let manager = core_manager_at(&dir);
    let provider = manager.provider();
    let list_file = provider.paths().lists_dir().join("ipset-all.txt");
    let url = provider.ipset_url();

    // Check if curl exists in System32
    let curl_path = std::path::Path::new(r"C:\Windows\System32\curl.exe");
    let output = if curl_path.exists() {
        Command::new(curl_path)
            .args(["-L", "-o", list_file.to_str().unwrap_or(""), url])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
    } else {
        // Fallback to PowerShell
        let ps_cmd = format!(
            "$url = '{}'; $out = '{}'; try {{ $res = Invoke-WebRequest -Uri $url -TimeoutSec 30 -UseBasicParsing; if ($res.StatusCode -eq 200) {{ $res.Content | Out-File -FilePath $out -Encoding UTF8 }} else {{ exit 1 }} }} catch {{ exit 1 }}",
            url,
            list_file.to_str().unwrap_or("")
        );
        Command::new(powershell_path())
            .args(["-NoProfile", "-Command", &ps_cmd])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
    };

    match output {
        Ok(out) if out.status.success() => {
            // Validate the downloaded content looks like an IP/CIDR list before
            // handing it off to winws.exe. The remote file is plain text and
            // line-based, so we reject anything that isn't a plausible IPv4/
            // IPv6 literal (with optional /prefix). If the remote is
            // compromised and starts serving something else, we delete the
            // file and fail rather than silently loading garbage.
            let content = std::fs::read_to_string(&list_file)
                .map_err(|e| format!("Failed to read downloaded file: {}", e))?;

            let mut count = 0usize;
            for (idx, raw) in content.lines().enumerate() {
                let line = raw.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                if !is_ip_or_cidr(line) {
                    let _ = std::fs::remove_file(&list_file);
                    return Err(format!(
                        "Downloaded ipset list is not valid (line {}): {:?}",
                        idx + 1,
                        line
                    ));
                }
                count += 1;
            }
            if count == 0 {
                let _ = std::fs::remove_file(&list_file);
                return Err("Downloaded ipset list is empty".to_string());
            }
            Ok(format!("Updated successfully. {} IPs loaded.", count))
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            Err(format!("Failed to update IPSet list: {}", stderr))
        }
        Err(e) => Err(format!("Failed to execute update command: {}", e)),
    }
}

/// Validates that `s` is a syntactically plausible IPv4, IPv4/CIDR, IPv6 or
/// IPv6/CIDR literal. Intentionally loose on range checks (we care about
/// shape, not correctness): the existing file format is consumed by zapret,
/// not parsed as a network spec here.
fn is_ip_or_cidr(s: &str) -> bool {
    let (addr, prefix) = match s.split_once('/') {
        Some((a, p)) => (a, Some(p)),
        None => (s, None),
    };
    if addr.is_empty() {
        return false;
    }
    let is_ipv4 = addr.parse::<std::net::Ipv4Addr>().is_ok();
    let is_ipv6 = addr.parse::<std::net::Ipv6Addr>().is_ok();
    if !is_ipv4 && !is_ipv6 {
        return false;
    }
    match prefix {
        None => true,
        Some(p) => match p.parse::<u8>() {
            Ok(n) if is_ipv4 && n <= 32 => true,
            Ok(n) if is_ipv6 && n <= 128 => true,
            _ => false,
        },
    }
}

#[tauri::command]
async fn download_and_install_update(
    app: tauri::AppHandle,
    window: tauri::Window,
    use_proxy: Option<bool>,
    custom_proxy: Option<String>,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let _operation_guard = CoreOperationGuard::acquire()?;
    let dir = find_binaries_dir();
    let installation = CoreInstallation::new(&dir)?;
    let manager = core_manager_at(&dir);
    installation.prepare(manager.provider())?;
    let temp_parent = std::env::temp_dir();
    let temp_dir = temp_parent.join(format!(
        ".zapret-ui-core-download-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| format!("System clock error: {e}"))?
            .as_nanos()
    ));

    // Create temp directory
    std::fs::create_dir_all(&temp_dir).map_err(|e| format!("Failed to create temp dir: {}", e))?;
    let _download_cleanup = OwnedDirectoryCleanup::new(
        temp_dir.clone(),
        temp_parent.canonicalize().map_err(|e| e.to_string())?,
        ".zapret-ui-core-download-",
    );

    window.emit("download-progress", 5).ok();

    // Prepare proxy client
    let mut client_builder = reqwest::Client::builder();
    let use_proxy = use_proxy.unwrap_or(false);
    if use_proxy {
        let proxy_url =
            custom_proxy.or_else(|| option_env!("ZAPRET_UPDATE_PROXY").map(|s| s.to_string()));
        if let Some(proxy_url) = proxy_url {
            if let Ok(proxy) = reqwest::Proxy::all(&proxy_url) {
                client_builder = client_builder.proxy(proxy);
            }
        }
    }
    let client = client_builder
        .build()
        .map_err(|e| format!("Failed to build http client: {}", e))?;

    // Fetch version from GitHub with fallback to SourceForge
    let provider = manager.provider();
    let mut fetched_from_sf = false;
    let mut sf_info: Option<CoreRelease> = None;

    let latest_version = match client
        .get(provider.version_url())
        .header("Cache-Control", "no-cache")
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => match resp.text().await {
            Ok(text) => {
                let trimmed = text.trim().to_string();
                if !trimmed.is_empty() && !trimmed.contains("Not Found") {
                    trimmed
                } else {
                    fetched_from_sf = true;
                    let info = fetch_fallback_release_info(&client).await?;
                    let version = info.version.clone();
                    sf_info = Some(info);
                    version
                }
            }
            Err(_) => {
                fetched_from_sf = true;
                let info = fetch_fallback_release_info(&client).await?;
                let version = info.version.clone();
                sf_info = Some(info);
                version
            }
        },
        _ => {
            fetched_from_sf = true;
            let info = fetch_fallback_release_info(&client).await?;
            let version = info.version.clone();
            sf_info = Some(info);
            version
        }
    };

    window.emit("download-progress", 10).ok();

    let zip_path = temp_dir.join("update.zip");

    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    let done_flag = Arc::new(AtomicBool::new(false));
    let done_flag_thread = done_flag.clone();
    let window_clone = window.clone();
    std::thread::spawn(move || {
        let steps: &[u16] = &[15, 20, 28, 35, 42, 50, 58, 65, 72, 78, 83, 88];
        for pct in steps {
            if done_flag_thread.load(Ordering::Relaxed) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_secs(3));
            if done_flag_thread.load(Ordering::Relaxed) {
                break;
            }
            window_clone.emit("download-progress", *pct).ok();
        }
    });

    // 1. Try downloading from GitHub
    let mut response = None;
    let mut downloaded_from_sf = false;

    if !fetched_from_sf {
        let github_url = provider.release_download_url(&latest_version);
        if let Ok(resp) = client.get(&github_url).send().await {
            if resp.status().is_success() {
                response = Some(resp);
            }
        }
    }

    // 2. If GitHub failed or wasn't tried, try SourceForge
    if response.is_none() {
        // Ensure we have SF info loaded
        let info = match sf_info {
            Some(i) => i,
            None => fetch_fallback_release_info(&client).await?,
        };

        // Try downloading from the direct SourceForge URL
        match client.get(&info.download_url).send().await {
            Ok(resp) if resp.status().is_success() => {
                response = Some(resp);
                downloaded_from_sf = true;
                sf_info = Some(info);
            }
            _ => {
                // Also try downloading from the generic latest URL user requested as fallback
                let generic_url = manager.fallback_latest_url();
                match client.get(generic_url).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        response = Some(resp);
                        downloaded_from_sf = true;
                        sf_info = Some(info);
                    }
                    Ok(resp) => {
                        done_flag.store(true, Ordering::Relaxed);
                        return Err(format!(
                            "SourceForge download failed with status: {}",
                            resp.status()
                        ));
                    }
                    Err(e) => {
                        done_flag.store(true, Ordering::Relaxed);
                        return Err(format!(
                            "Failed to download from both GitHub and SourceForge. SF error: {}",
                            e
                        ));
                    }
                }
            }
        }
    }

    let response = response.unwrap();

    use futures_util::StreamExt;
    let mut file = std::fs::File::create(&zip_path).map_err(|e| {
        done_flag.store(true, Ordering::Relaxed);
        format!("Failed to create update zip file: {}", e)
    })?;

    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        match chunk {
            Ok(bytes) => {
                use std::io::Write;
                if let Err(e) = file.write_all(&bytes) {
                    done_flag.store(true, Ordering::Relaxed);
                    return Err(format!("Failed to write to zip file: {}", e));
                }
            }
            Err(e) => {
                done_flag.store(true, Ordering::Relaxed);
                return Err(format!("Error while downloading: {}", e));
            }
        }
    }

    done_flag.store(true, Ordering::Relaxed);

    if !zip_path.exists() {
        return Err("Download failed: output file not found".to_string());
    }

    // Verify integrity of downloaded archive before extraction.
    let verified_checksum;
    let source_url;
    if downloaded_from_sf {
        if let Some(info) = sf_info {
            let expected_md5 = match info.checksum {
                Some(Checksum::Md5(value)) => value,
                _ => {
                    return Err(
                        "Downloaded from SourceForge but MD5 metadata is missing".to_string()
                    )
                }
            };
            let actual_md5 = md5_file(&zip_path)?;
            if actual_md5 != expected_md5.to_ascii_lowercase() {
                let _ = std::fs::remove_file(&zip_path);
                return Err(format!(
                    "Checksum mismatch (MD5) for SourceForge download: expected {}, got {}",
                    expected_md5, actual_md5
                ));
            }
            verified_checksum = Checksum::Md5(expected_md5);
            source_url = Some(info.download_url);
        } else {
            return Err("Downloaded from SourceForge but release metadata is missing".to_string());
        }
    } else {
        let asset_name = provider.archive_name(&latest_version);
        let expected_checksum =
            Checksum::Sha256(fetch_expected_sha256(&latest_version, &asset_name).await?);
        let expected_sha256 = match expected_checksum {
            Checksum::Sha256(value) => value,
            Checksum::Md5(_) => unreachable!("GitHub releases require SHA-256"),
        };
        let actual_sha256 = sha256_file(&zip_path)?;
        if actual_sha256 != expected_sha256 {
            let _ = std::fs::remove_file(&zip_path);
            return Err(format!(
                "Checksum mismatch (SHA-256) for {}: expected {}, got {}",
                asset_name, expected_sha256, actual_sha256
            ));
        }
        verified_checksum = Checksum::Sha256(expected_sha256);
        source_url = Some(provider.release_download_url(&latest_version));
    }

    // Extraction
    window.emit("download-progress", 92).ok();
    let staging = installation.create_staging()?;
    let _staging_cleanup = OwnedDirectoryCleanup::new(
        staging.clone(),
        staging
            .parent()
            .ok_or_else(|| "Staging has no parent".to_string())?
            .to_path_buf(),
        ".binaries-staging-",
    );
    let extract_dir = staging.join("package");
    std::fs::create_dir(&extract_dir).map_err(|e| format!("Failed to prepare staging: {e}"))?;

    // Extract natively via the `zip` crate instead of calling
    // `Expand-Archive`. Every entry name is validated against `..`, absolute
    // paths, and drive-letter prefixes before it is written, preventing
    // zip-slip from placing files outside `extract_dir`.
    if let Err(error) = extract_zip_safely(&zip_path, &extract_dir) {
        installation.remove_owned_staging(&staging)?;
        return Err(error);
    }

    window.emit("download-progress", 95).ok();

    let mut extracted_folder = extract_dir.clone();
    if let Ok(entries) = std::fs::read_dir(&extract_dir) {
        let items: Vec<_> = entries.flatten().collect();
        if items.len() == 1 && items[0].path().is_dir() {
            extracted_folder = items[0].path();
        }
    }

    if let Err(error) = copy_dir_contents(&extracted_folder, &staging) {
        installation.remove_owned_staging(&staging)?;
        return Err(error);
    }
    std::fs::remove_dir_all(&extract_dir)
        .map_err(|e| format!("Cannot remove staging package directory: {e}"))?;
    if let Err(error) = installation.validate_and_manifest(
        &staging,
        &latest_version,
        source_url,
        Some(verified_checksum),
        core_manager_at(&staging).provider(),
    ) {
        installation.remove_owned_staging(&staging)?;
        return Err(error);
    }
    installation.preserve_user_files(&dir, &staging)?;

    // Capture the exact runtime state only after the candidate is ready, just
    // before stopping anything managed by the application.
    let restart = restart_context(get_zapret_status(state.clone()), "update")?;
    let filters_status = get_filters_status();
    if restart.is_some() {
        if let Err(error) = stop_zapret_internal() {
            installation.remove_owned_staging(&staging)?;
            return Err(format!("Cannot stop zapret before activation: {error}"));
        }
    }

    let operation = (|| {
        installation.activate(&staging, manager.provider())?;
        if let Err(error) = set_game_filter(filters_status.game_filter)
            .and_then(|_| set_ipset_filter(filters_status.ipset))
        {
            installation
                .rollback(
                    manager.provider(),
                    core_manager_at(dir.with_file_name("binaries.previous")).provider(),
                )
                .map_err(|rollback| {
                    format!(
                        "Cannot restore filter settings ({error}); automatic rollback failed: {rollback}"
                    )
                })?;
            return Err(format!(
                "Cannot restore filter settings ({error}); automatic rollback completed"
            ));
        }
        Ok(())
    })();

    let restart_result = restart.map(|(strategy, mode)| {
        start_zapret(app, strategy, mode, state)
            .map(|_| ())
            .map_err(|e| format!("Cannot restart zapret after core update: {e}"))
    });
    match (operation, restart_result) {
        (Err(error), Some(Err(restart_error))) => Err(format!("{error}; {restart_error}")),
        (Err(error), _) => Err(error),
        (Ok(()), Some(Err(restart_error))) => {
            window.emit("download-progress", 100).ok();
            Ok(format!("Update successful. Warning: {restart_error}"))
        }
        (Ok(()), _) => {
            window.emit("download-progress", 100).ok();
            Ok("Update successful".to_string())
        }
    }
}

struct OwnedDirectoryCleanup {
    path: PathBuf,
    expected_parent: PathBuf,
    prefix: &'static str,
}

impl OwnedDirectoryCleanup {
    fn new(path: PathBuf, expected_parent: PathBuf, prefix: &'static str) -> Self {
        Self {
            path,
            expected_parent,
            prefix,
        }
    }
}

impl Drop for OwnedDirectoryCleanup {
    fn drop(&mut self) {
        if !self.path.exists() {
            return;
        }
        let actual_parent = self
            .path
            .parent()
            .and_then(|parent| parent.canonicalize().ok());
        let expected_parent = self.expected_parent.canonicalize().ok();
        let owned = actual_parent == expected_parent
            && self
                .path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(self.prefix));
        let safe_type = std::fs::symlink_metadata(&self.path)
            .map(|metadata| !metadata.file_type().is_symlink())
            .unwrap_or(false);
        if owned && safe_type {
            if let Err(error) = std::fs::remove_dir_all(&self.path) {
                eprintln!(
                    "Cannot clean owned temporary directory {}: {error}",
                    self.path.display()
                );
            }
        } else {
            eprintln!(
                "Refusing to clean unverified temporary directory {}",
                self.path.display()
            );
        }
    }
}

#[cfg(test)]
mod temporary_cleanup_tests {
    use super::{restart_context, CoreOperationGuard, OwnedDirectoryCleanup, ZapretStatus};

    #[test]
    fn restart_context_preserves_service_and_temporary_modes() {
        for mode in ["service", "temporary"] {
            let context = restart_context(
                ZapretStatus {
                    running: true,
                    strategy: Some("ALT12".to_string()),
                    mode: Some(mode.to_string()),
                },
                "update",
            )
            .unwrap();
            assert_eq!(context, Some(("ALT12".to_string(), mode.to_string())));
        }
        assert_eq!(
            restart_context(
                ZapretStatus {
                    running: false,
                    strategy: None,
                    mode: None,
                },
                "update",
            )
            .unwrap(),
            None
        );
        assert!(restart_context(
            ZapretStatus {
                running: true,
                strategy: None,
                mode: Some("service".to_string()),
            },
            "update",
        )
        .unwrap_err()
        .contains("active strategy is unknown"));
    }

    #[test]
    fn core_operation_guard_rejects_overlap_and_resets_on_drop() {
        let guard = CoreOperationGuard::acquire().unwrap();
        assert!(CoreOperationGuard::acquire()
            .err()
            .unwrap()
            .contains("already in progress"));
        drop(guard);
        assert!(CoreOperationGuard::acquire().is_ok());
    }

    #[test]
    fn owned_temporary_directory_is_cleaned_during_error_unwind() {
        let temp_parent = std::env::temp_dir();
        let expected_parent = temp_parent.canonicalize().unwrap();
        for prefix in [".zapret-ui-core-download-", ".binaries-staging-"] {
            let path = temp_parent.join(format!("{prefix}test-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir(&path).unwrap();
            let result: Result<(), &str> = (|| {
                let _cleanup =
                    OwnedDirectoryCleanup::new(path.clone(), expected_parent.clone(), prefix);
                Err("simulated failure")
            })();
            assert!(result.is_err());
            assert!(!path.exists());
        }
    }
}

#[tauri::command]
fn get_core_installation_state() -> Result<CoreInstallationState, String> {
    let dir = find_binaries_dir();
    let installation = CoreInstallation::new(&dir)?;
    installation.prepare(core_manager_at(dir).provider())?;
    Ok(installation.state())
}

#[tauri::command]
fn rollback_core_update(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<CoreInstallationState, String> {
    let _operation_guard = CoreOperationGuard::acquire()?;
    let dir = find_binaries_dir();
    let previous_dir = dir.with_file_name("binaries.previous");
    let installation = CoreInstallation::new(&dir)?;
    let active_manager = core_manager_at(&dir);
    installation.prepare(active_manager.provider())?;
    let filters = get_filters_status();
    let restart = restart_context(get_zapret_status(state.clone()), "rollback")?;
    if restart.is_some() {
        stop_zapret_internal().map_err(|e| format!("Cannot stop zapret before rollback: {e}"))?;
    }

    let operation = (|| {
        let result = installation.rollback(
            active_manager.provider(),
            core_manager_at(&previous_dir).provider(),
        )?;
        if let Err(error) =
            set_game_filter(filters.game_filter).and_then(|_| set_ipset_filter(filters.ipset))
        {
            installation
                .rollback(
                    active_manager.provider(),
                    core_manager_at(&previous_dir).provider(),
                )
                .map_err(|restore| {
                    format!(
                        "Cannot restore settings after rollback ({error}); cannot undo rollback: {restore}"
                    )
                })?;
            return Err(format!(
                "Cannot restore settings after rollback ({error}); original core restored"
            ));
        }
        Ok(result)
    })();

    let restart_result = restart.map(|(strategy, mode)| {
        start_zapret(app, strategy, mode, state)
            .map_err(|e| format!("Cannot restart zapret after rollback attempt: {e}"))
    });
    match (operation, restart_result) {
        (Ok(_), Some(Err(error))) => Err(error),
        (Err(error), Some(Err(restart))) => Err(format!("{error}; {restart}")),
        (result, _) => result,
    }
}

/// Recursively copies directory contents
fn copy_dir_contents(src: &std::path::Path, dst: &std::path::Path) -> Result<(), String> {
    for entry in std::fs::read_dir(src).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        let file_name = entry.file_name();
        let dest_path = dst.join(&file_name);

        if path.is_dir() {
            std::fs::create_dir_all(&dest_path)
                .map_err(|e| format!("Failed to create directory {:?}: {}", dest_path, e))?;
            copy_dir_contents(&path, &dest_path)?;
        } else {
            if let Err(e) = std::fs::copy(&path, &dest_path) {
                // If it fails (likely due to lock), try to rename the locked destination file first
                let mut old_path = dest_path.clone();
                let new_name = format!("{}.old", file_name.to_str().unwrap_or("locked"));
                old_path.set_file_name(new_name);

                if std::fs::rename(&dest_path, &old_path).is_err() {
                    return Err(format!(
                        "Failed to copy file {:?} to {:?}: {}",
                        path, dest_path, e
                    ));
                }

                // Attempt copy again after rename
                std::fs::copy(&path, &dest_path).map_err(|e2| {
                    format!(
                        "Failed to copy file {:?} to {:?} after renaming: {}",
                        path, dest_path, e2
                    )
                })?;
            }
        }
    }
    Ok(())
}

#[derive(serde::Serialize)]
struct DiagnosticCheck {
    name: String,
    status: String, // "passed", "warning", "error"
    message: String,
    link: Option<String>,
}

#[derive(serde::Serialize)]
struct DiagnosticsResult {
    checks: Vec<DiagnosticCheck>,
    vpn_services: Option<String>,
}

/// Runs all diagnostic checks
#[tauri::command]
async fn run_diagnostics() -> Result<DiagnosticsResult, String> {
    let mut checks = Vec::new();
    let mut vpn_services: Option<String> = None;

    // 1. Base Filtering Engine check
    let bfe_check = Command::new(system32_tool("sc.exe"))
        .args(["query", "BFE"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    match bfe_check {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            if stdout.to_lowercase().contains("running") {
                checks.push(DiagnosticCheck {
                    name: "Base Filtering Engine".to_string(),
                    status: "passed".to_string(),
                    message: "Service is running".to_string(),
                    link: None,
                });
            } else {
                checks.push(DiagnosticCheck {
                    name: "Base Filtering Engine".to_string(),
                    status: "error".to_string(),
                    message: "Service is not running. This service is required for zapret to work"
                        .to_string(),
                    link: None,
                });
            }
        }
        Err(_) => {
            checks.push(DiagnosticCheck {
                name: "Base Filtering Engine".to_string(),
                status: "error".to_string(),
                message: "Failed to check service status".to_string(),
                link: None,
            });
        }
    }

    // 2. Proxy check
    let proxy_check = Command::new(powershell_path())
        .args([
            "-NoProfile",
            "-Command",
            "try { $val = Get-ItemProperty -Path 'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings' -Name ProxyEnable -ErrorAction Stop; if ($val.ProxyEnable -eq 1) { $srv = Get-ItemProperty -Path 'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings' -Name ProxyServer -ErrorAction SilentlyContinue; Write-Host \"PROXY_ENABLED:$($srv.ProxyServer)\" } else { Write-Host \"PROXY_DISABLED\" } } catch { Write-Host \"PROXY_DISABLED\" }"
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    match proxy_check {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            if stdout.contains("PROXY_ENABLED:") {
                let proxy = stdout.split(':').nth(1).unwrap_or("unknown").trim();
                checks.push(DiagnosticCheck {
                    name: "System Proxy".to_string(),
                    status: "warning".to_string(),
                    message: format!("System proxy is enabled: {}. Make sure it's valid or disable it if you don't use a proxy", proxy),
                    link: None,
                });
            } else {
                checks.push(DiagnosticCheck {
                    name: "System Proxy".to_string(),
                    status: "passed".to_string(),
                    message: "No system proxy detected".to_string(),
                    link: None,
                });
            }
        }
        Err(_) => {
            checks.push(DiagnosticCheck {
                name: "System Proxy".to_string(),
                status: "passed".to_string(),
                message: "Proxy check passed".to_string(),
                link: None,
            });
        }
    }

    // 3. TCP timestamps check
    let tcp_check = Command::new(powershell_path())
        .args([
            "-NoProfile",
            "-Command",
            "$out = netsh interface tcp show global; if ($out -match 'RFC 1323.*enabled') { Write-Host 'TIMESTAMPS_ENABLED' } else { Write-Host 'TIMESTAMPS_DISABLED' }"
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    match tcp_check {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            if stdout.contains("TIMESTAMPS_ENABLED") {
                checks.push(DiagnosticCheck {
                    name: "TCP Timestamps".to_string(),
                    status: "passed".to_string(),
                    message: "TCP timestamps are enabled".to_string(),
                    link: None,
                });
            } else {
                // Try to enable
                let _ = Command::new("netsh")
                    .args(["interface", "tcp", "set", "global", "timestamps=enabled"])
                    .creation_flags(CREATE_NO_WINDOW)
                    .output();
                checks.push(DiagnosticCheck {
                    name: "TCP Timestamps".to_string(),
                    status: "warning".to_string(),
                    message: "TCP timestamps were disabled. Attempted to enable them.".to_string(),
                    link: None,
                });
            }
        }
        Err(_) => {
            checks.push(DiagnosticCheck {
                name: "TCP Timestamps".to_string(),
                status: "warning".to_string(),
                message: "Failed to check TCP timestamps".to_string(),
                link: None,
            });
        }
    }

    // 4. Adguard check
    let adguard_check = Command::new("tasklist")
        .args(["/FI", "IMAGENAME eq AdguardSvc.exe", "/FO", "CSV", "/NH"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    match adguard_check {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            if stdout.to_lowercase().contains("adguardsvc") {
                checks.push(DiagnosticCheck {
                    name: "Adguard".to_string(),
                    status: "error".to_string(),
                    message: "Adguard process found. Adguard may cause problems with Discord"
                        .to_string(),
                    link: Some(
                        "https://github.com/Flowseal/zapret-discord-youtube/issues/417".to_string(),
                    ),
                });
            } else {
                checks.push(DiagnosticCheck {
                    name: "Adguard".to_string(),
                    status: "passed".to_string(),
                    message: "Adguard not detected".to_string(),
                    link: None,
                });
            }
        }
        Err(_) => {
            checks.push(DiagnosticCheck {
                name: "Adguard".to_string(),
                status: "passed".to_string(),
                message: "Adguard check passed".to_string(),
                link: None,
            });
        }
    }

    // 5. Killer services check
    let killer_check = Command::new(system32_tool("sc.exe"))
        .args(["query"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    match killer_check {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            if stdout.to_lowercase().contains("killer") {
                checks.push(DiagnosticCheck {
                    name: "Killer Network Service".to_string(),
                    status: "error".to_string(),
                    message: "Killer services found. Killer conflicts with zapret".to_string(),
                    link: Some("https://github.com/Flowseal/zapret-discord-youtube/issues/2512#issuecomment-2821119513".to_string()),
                });
            } else {
                checks.push(DiagnosticCheck {
                    name: "Killer Network Service".to_string(),
                    status: "passed".to_string(),
                    message: "Killer services not detected".to_string(),
                    link: None,
                });
            }
        }
        Err(_) => {
            checks.push(DiagnosticCheck {
                name: "Killer Network Service".to_string(),
                status: "passed".to_string(),
                message: "Killer check passed".to_string(),
                link: None,
            });
        }
    }

    // 6. Intel Connectivity check
    let intel_check = Command::new(system32_tool("sc.exe"))
        .args(["query"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    match intel_check {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_lowercase();
            if stdout.contains("intel") && stdout.contains("connectivity") {
                checks.push(DiagnosticCheck {
                    name: "Intel Connectivity Network Service".to_string(),
                    status: "error".to_string(),
                    message: "Intel Connectivity Network Service found. It conflicts with zapret"
                        .to_string(),
                    link: Some(
                        "https://github.com/ValdikSS/GoodbyeDPI/issues/541#issuecomment-2661670982"
                            .to_string(),
                    ),
                });
            } else {
                checks.push(DiagnosticCheck {
                    name: "Intel Connectivity Network Service".to_string(),
                    status: "passed".to_string(),
                    message: "Intel Connectivity service not detected".to_string(),
                    link: None,
                });
            }
        }
        Err(_) => {
            checks.push(DiagnosticCheck {
                name: "Intel Connectivity Network Service".to_string(),
                status: "passed".to_string(),
                message: "Intel Connectivity check passed".to_string(),
                link: None,
            });
        }
    }

    // 7. Check Point check
    let checkpoint_check = Command::new(system32_tool("sc.exe"))
        .args(["query"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    match checkpoint_check {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_lowercase();
            if stdout.contains("tracsrvwrapper") || stdout.contains("epwd") {
                checks.push(DiagnosticCheck {
                    name: "Check Point".to_string(),
                    status: "error".to_string(),
                    message: "Check Point services found. Check Point conflicts with zapret"
                        .to_string(),
                    link: None,
                });
            } else {
                checks.push(DiagnosticCheck {
                    name: "Check Point".to_string(),
                    status: "passed".to_string(),
                    message: "Check Point services not detected".to_string(),
                    link: None,
                });
            }
        }
        Err(_) => {
            checks.push(DiagnosticCheck {
                name: "Check Point".to_string(),
                status: "passed".to_string(),
                message: "Check Point check passed".to_string(),
                link: None,
            });
        }
    }

    // 8. SmartByte check
    let smartbyte_check = Command::new(system32_tool("sc.exe"))
        .args(["query"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    match smartbyte_check {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_lowercase();
            if stdout.contains("smartbyte") {
                checks.push(DiagnosticCheck {
                    name: "SmartByte".to_string(),
                    status: "error".to_string(),
                    message: "SmartByte services found. SmartByte conflicts with zapret"
                        .to_string(),
                    link: None,
                });
            } else {
                checks.push(DiagnosticCheck {
                    name: "SmartByte".to_string(),
                    status: "passed".to_string(),
                    message: "SmartByte services not detected".to_string(),
                    link: None,
                });
            }
        }
        Err(_) => {
            checks.push(DiagnosticCheck {
                name: "SmartByte".to_string(),
                status: "passed".to_string(),
                message: "SmartByte check passed".to_string(),
                link: None,
            });
        }
    }

    // 9. VPN services check
    let vpn_check = Command::new(system32_tool("sc.exe"))
        .args(["query"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    match vpn_check {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let vpn_lines: Vec<&str> = stdout
                .lines()
                .filter(|l| l.to_lowercase().contains("vpn"))
                .collect();
            if !vpn_lines.is_empty() {
                let services: Vec<String> = vpn_lines
                    .iter()
                    .filter_map(|l| l.split(':').nth(1))
                    .map(|s| s.trim().to_string())
                    .collect();
                vpn_services = Some(services.join(", "));
                checks.push(DiagnosticCheck {
                    name: "VPN Services".to_string(),
                    status: "warning".to_string(),
                    message: "VPN services found. Some VPNs can conflict with zapret".to_string(),
                    link: None,
                });
            } else {
                checks.push(DiagnosticCheck {
                    name: "VPN Services".to_string(),
                    status: "passed".to_string(),
                    message: "No VPN services detected".to_string(),
                    link: None,
                });
            }
        }
        Err(_) => {
            checks.push(DiagnosticCheck {
                name: "VPN Services".to_string(),
                status: "passed".to_string(),
                message: "VPN check passed".to_string(),
                link: None,
            });
        }
    }

    // 10. DNS over HTTPS check
    let doh_check = Command::new(powershell_path())
        .args([
            "-NoProfile",
            "-Command",
            "try { $count = Get-ChildItem -Recurse -Path 'HKLM:System\\CurrentControlSet\\Services\\Dnscache\\InterfaceSpecificParameters\\' | Get-ItemProperty | Where-Object { $_.DohFlags -gt 0 } | Measure-Object | Select-Object -ExpandProperty Count; Write-Host \"DOH:$count\" } catch { Write-Host \"DOH:0\" }"
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    match doh_check {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            if stdout.contains("DOH:0") {
                checks.push(DiagnosticCheck {
                    name: "Secure DNS".to_string(),
                    status: "warning".to_string(),
                    message: "Make sure you have configured secure DNS in a browser with some non-default DNS service provider. If you use Windows 11 you can configure encrypted DNS in the Settings to hide this warning".to_string(),
                    link: None,
                });
            } else {
                checks.push(DiagnosticCheck {
                    name: "Secure DNS".to_string(),
                    status: "passed".to_string(),
                    message: "Secure DNS is configured".to_string(),
                    link: None,
                });
            }
        }
        Err(_) => {
            checks.push(DiagnosticCheck {
                name: "Secure DNS".to_string(),
                status: "warning".to_string(),
                message: "Failed to check DNS configuration".to_string(),
                link: None,
            });
        }
    }

    // 11. Hosts file check
    let hosts_path = std::path::Path::new(r"C:\Windows\System32\drivers\etc\hosts");
    if hosts_path.exists() {
        if let Ok(content) = std::fs::read_to_string(hosts_path) {
            let content_lower = content.to_lowercase();
            if content_lower.contains("youtube.com") || content_lower.contains("youtu.be") {
                checks.push(DiagnosticCheck {
                    name: "Hosts File".to_string(),
                    status: "warning".to_string(),
                    message: "Your hosts file contains entries for youtube.com or youtu.be. This may cause problems with YouTube access".to_string(),
                    link: None,
                });
            } else {
                checks.push(DiagnosticCheck {
                    name: "Hosts File".to_string(),
                    status: "passed".to_string(),
                    message: "No YouTube entries in hosts file".to_string(),
                    link: None,
                });
            }
        }
    }

    // 12. WinDivert check
    let windivert_check = Command::new(system32_tool("sc.exe"))
        .args(["query", "WinDivert"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    match windivert_check {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            if stdout.to_lowercase().contains("running") {
                checks.push(DiagnosticCheck {
                    name: "WinDivert".to_string(),
                    status: "passed".to_string(),
                    message: "WinDivert driver is running".to_string(),
                    link: None,
                });
            } else {
                checks.push(DiagnosticCheck {
                    name: "WinDivert".to_string(),
                    status: "passed".to_string(),
                    message: "WinDivert driver not active (will be started when needed)"
                        .to_string(),
                    link: None,
                });
            }
        }
        Err(_) => {
            checks.push(DiagnosticCheck {
                name: "WinDivert".to_string(),
                status: "passed".to_string(),
                message: "WinDivert check passed".to_string(),
                link: None,
            });
        }
    }

    Ok(DiagnosticsResult {
        checks,
        vpn_services,
    })
}

#[derive(serde::Serialize)]
struct SiteCheckResult {
    domain: String,
    dns_resolved_ips: Vec<String>,
    dns_status: String,
    dns_message: String,
    http_status: String,
    http_code: Option<u16>,
    http_message: String,
    ping_ms: Option<u32>,
    is_zapret_running: bool,
}

#[tauri::command]
async fn check_site(domain: String, state: State<'_, AppState>) -> Result<SiteCheckResult, String> {
    let domain_clean = domain
        .trim()
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .split('/')
        .next()
        .unwrap_or("")
        .to_string();

    if domain_clean.is_empty() {
        return Err("Empty domain".to_string());
    }

    let status = get_zapret_status(state);
    let zapret_running = status.running;

    // 1. DNS Resolution
    let dns_result = tokio::net::lookup_host(format!("{}:443", domain_clean)).await;
    let mut ips = Vec::new();
    let (dns_status, dns_message) = match dns_result {
        Ok(addrs) => {
            for addr in addrs {
                let socket_addr: std::net::SocketAddr = addr;
                let ip_str = socket_addr.ip().to_string();
                if !ips.contains(&ip_str) {
                    ips.push(ip_str);
                }
            }
            if ips.is_empty() {
                ("error".to_string(), "No IP addresses resolved".to_string())
            } else {
                ("ok".to_string(), format!("Resolved {} IPs", ips.len()))
            }
        }
        Err(e) => ("error".to_string(), format!("DNS resolution failed: {}", e)),
    };

    // 2. TCP connection test (Ping)
    let mut ping_ms = None;
    if !ips.is_empty() {
        let ip = &ips[0];
        if let Ok(socket_addr) = format!("{}:443", ip).parse::<std::net::SocketAddr>() {
            let start = std::time::Instant::now();
            let connect_timeout = std::time::Duration::from_secs(3);
            if let Ok(Ok(_)) =
                tokio::time::timeout(connect_timeout, tokio::net::TcpStream::connect(socket_addr))
                    .await
            {
                ping_ms = Some(start.elapsed().as_millis() as u32);
            }
        }
    }

    // 3. HTTPS GET test (moved to frontend JS for accurate browser SNI/TLS/proxy compatibility)
    let (http_status, http_code, http_message) = ("".to_string(), None, "".to_string());

    Ok(SiteCheckResult {
        domain: domain_clean,
        dns_resolved_ips: ips,
        dns_status,
        dns_message,
        http_status,
        http_code,
        http_message,
        ping_ms,
        is_zapret_running: zapret_running,
    })
}

/// Clears Discord cache
#[tauri::command]
fn clear_discord_cache() -> Result<String, String> {
    let mut messages = Vec::new();

    // Check if Discord is running and close it
    let discord_processes = ["Discord.exe", "DiscordPTB.exe", "DiscordCanary.exe"];
    let mut discord_was_running = false;

    for process in &discord_processes {
        let check_output = Command::new("tasklist")
            .args([
                "/FI",
                &format!("IMAGENAME eq {}", process),
                "/FO",
                "CSV",
                "/NH",
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output();

        if let Ok(out) = check_output {
            let stdout = String::from_utf8_lossy(&out.stdout);
            if stdout.to_lowercase().contains(&process.to_lowercase()) {
                discord_was_running = true;
                messages.push(format!("Discord is running, closing {}...", process));

                // Kill the process
                let _ = Command::new(system32_tool("taskkill.exe"))
                    .args(["/F", "/IM", process])
                    .creation_flags(CREATE_NO_WINDOW)
                    .output();
            }
        }
    }

    if discord_was_running {
        // Wait a bit for Discord to close
        std::thread::sleep(std::time::Duration::from_millis(1000));
        messages.push("Discord was successfully closed".to_string());
    }

    // Discord cache is in APPDATA (Roaming), not LOCALAPPDATA
    let appdata = std::env::var("APPDATA").map_err(|_| "Could not find APPDATA".to_string())?;

    let discord_paths = [
        format!("{}\\discord\\Cache", appdata),
        format!("{}\\discord\\Code Cache", appdata),
        format!("{}\\discord\\GPUCache", appdata),
        format!("{}\\DiscordPTB\\Cache", appdata),
        format!("{}\\DiscordPTB\\Code Cache", appdata),
        format!("{}\\DiscordPTB\\GPUCache", appdata),
        format!("{}\\DiscordCanary\\Cache", appdata),
        format!("{}\\DiscordCanary\\Code Cache", appdata),
        format!("{}\\DiscordCanary\\GPUCache", appdata),
    ];

    let mut cleared = 0;

    for path_str in &discord_paths {
        let path = std::path::Path::new(path_str);
        if path.exists() {
            // Count items before deletion for the message
            let mut items_deleted = 0;
            if let Ok(entries) = std::fs::read_dir(path) {
                for entry in entries.flatten() {
                    let _ = std::fs::remove_dir_all(entry.path());
                    items_deleted += 1;
                }
            }

            if items_deleted > 0 {
                cleared += 1;
                messages.push(format!("Successfully deleted {}", path_str));
            }
        }
    }

    if cleared > 0 || discord_was_running {
        Ok(messages.join("\n"))
    } else {
        Ok("No Discord cache found to clear".to_string())
    }
}

/// Checks if running with administrator privileges
#[tauri::command]
fn check_admin_privileges() -> Result<bool, String> {
    Ok(is_admin())
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct TestResult {
    config: String,
    status: String, // "success", "partial", "failed"
    http_ok: i32,
    http_error: i32,
    ping_ok: i32,
    ping_fail: i32,
    #[serde(default)]
    avg_ping_ms: i32,
    #[serde(default)]
    score: i32,
}

#[derive(serde::Serialize)]
#[allow(dead_code)]
struct TestProgress {
    current: usize,
    total: usize,
    config_name: String,
}

#[derive(serde::Serialize)]
struct PrecheckTestsResult {
    service_installed: bool,
    service_running: bool,
    winws_running: bool,
    is_admin: bool,
    strategies_count: usize,
}

/// Pre-flight проверка перед запуском тестов: какие есть блокеры.
/// Фронт решает, что предложить пользователю (остановить, удалить службу,
/// first-run скачивание стратегий).
#[tauri::command]
fn precheck_tests() -> PrecheckTestsResult {
    let dir = find_binaries_dir();
    let strategies_count = std::fs::read_dir(&dir)
        .map(|rd| {
            rd.filter_map(Result::ok)
                .filter(|e| {
                    let name = e.file_name().to_string_lossy().to_ascii_lowercase();
                    name.ends_with(".bat") && !name.starts_with("service")
                })
                .count()
        })
        .unwrap_or(0);

    PrecheckTestsResult {
        service_installed: is_zapret_service_installed(),
        service_running: is_zapret_service_running(),
        winws_running: is_winws_running(),
        is_admin: is_admin(),
        strategies_count,
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct SavedTestResults {
    timestamp: String, // ISO 8601
    test_type: String,
    best: Option<String>,
    results: Vec<TestResult>,
}

fn test_results_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to resolve app data dir: {}", e))?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create app data dir: {}", e))?;
    Ok(dir.join("test_results.json"))
}

#[tauri::command]
fn save_test_results(app: tauri::AppHandle, payload: SavedTestResults) -> Result<(), String> {
    let path = test_results_path(&app)?;
    let json = serde_json::to_string_pretty(&payload)
        .map_err(|e| format!("Failed to serialize test results: {}", e))?;
    std::fs::write(&path, json).map_err(|e| format!("Failed to write test results: {}", e))?;
    Ok(())
}

#[tauri::command]
fn load_test_results(app: tauri::AppHandle) -> Option<SavedTestResults> {
    let path = test_results_path(&app).ok()?;
    let raw = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&raw).ok()
}

/// Cancels a running test process
#[tauri::command]
fn cancel_tests(state: State<'_, AppState>) {
    let mut pid_lock = state.test_process_pid.lock_unpoisoned();
    if let Some(pid) = pid_lock.take() {
        // Kill process tree (/T = tree, /F = force)
        let _ = Command::new(system32_tool("taskkill.exe"))
            .arg("/F")
            .arg("/T")
            .arg("/PID")
            .arg(pid.to_string())
            .creation_flags(CREATE_NO_WINDOW)
            .output();
    }
    // Remove temp script if it still exists
    let temp_script = core_manager()
        .provider()
        .paths()
        .utils_dir()
        .join("test_zapret_ui.ps1");
    let _ = std::fs::remove_file(&temp_script);
}

/// Runs configuration tests with real-time streaming output via Tauri events
#[tauri::command]
async fn run_tests(
    app: tauri::AppHandle,
    test_type: String,
    test_mode: String,
) -> Result<Vec<TestResult>, String> {
    let dir = find_binaries_dir();
    let manager = core_manager_at(&dir);
    let ps_script = manager.provider().test_script();

    if !ps_script.exists() {
        return Err(
            "Test script not found. Please ensure zapret is properly installed.".to_string(),
        );
    }

    let original_content = std::fs::read_to_string(&ps_script)
        .map_err(|e| format!("Failed to read test script: {}", e))?;

    // Replace interactive function CALLS only (not definitions)
    let type_val = if test_type == "dpi" {
        "dpi"
    } else {
        "standard"
    };
    let modified_content = original_content
        .replace(
            "[void][System.Console]::ReadKey($true)",
            "# UI Mode - skipping ReadKey",
        )
        .replace(
            "$testType = Read-TestType",
            &format!("$testType = '{}'", type_val),
        )
        .replace("$mode = Read-ModeSelection", "$mode = 'all'")
        .replace(
            "    $selected = Read-ConfigSelection -allFiles $batFiles",
            "    $selected = $batFiles",
        )
        .replace(
            "    $batFiles = @($selected)",
            "    # UI Mode - using all configs",
        );

    let temp_script = manager
        .provider()
        .paths()
        .utils_dir()
        .join("test_zapret_ui.ps1");
    std::fs::write(&temp_script, modified_content)
        .map_err(|e| format!("Failed to write temp script: {}", e))?;

    let _ = app.emit(
        "test-progress",
        serde_json::json!({
            "line": format!("Starting {} tests ({} configs)...", type_val, test_mode),
            "kind": "info"
        }),
    );

    // Spawn the process and stream output line by line
    let mut child = std::process::Command::new(powershell_path())
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            temp_script.to_str().unwrap_or(""),
        ])
        .current_dir(&dir)
        .creation_flags(CREATE_NO_WINDOW)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to spawn test process: {}", e))?;

    // Store PID so cancel_tests / window-close can kill the process
    {
        let state = app.state::<AppState>();
        let mut pid_lock = state.test_process_pid.lock_unpoisoned();
        *pid_lock = Some(child.id());
    }

    let stdout = child.stdout.take().ok_or("Failed to capture stdout")?;
    let reader = BufReader::new(stdout);

    let mut all_lines: Vec<String> = Vec::new();
    let mut current_config: Option<String> = None;
    // config -> (sum_ms, count)
    let mut ping_stats: std::collections::HashMap<String, (i64, i64)> =
        std::collections::HashMap::new();

    // Regex-like match done manually: "  [N/M] name.bat"
    fn parse_config_header(line: &str) -> Option<(usize, usize, String)> {
        let trimmed = line.trim_start();
        let bracket = trimmed.strip_prefix('[')?;
        let close = bracket.find(']')?;
        let (nums, rest) = bracket.split_at(close);
        let rest = rest.strip_prefix("] ")?;
        let (cur_s, tot_s) = nums.split_once('/')?;
        let cur: usize = cur_s.parse().ok()?;
        let tot: usize = tot_s.parse().ok()?;
        Some((cur, tot, rest.trim().to_string()))
    }

    // Extract ping in milliseconds from lines like "... | Ping: 45 ms".
    // Returns None for "Timeout", "n/a", or non-matching lines.
    fn parse_ping_ms(line: &str) -> Option<i64> {
        let idx = line.find("Ping:")?;
        let after = line[idx + "Ping:".len()..].trim_start();
        if after.starts_with("Timeout") || after.starts_with("n/a") {
            return None;
        }
        let mut digits = String::new();
        for ch in after.chars() {
            if ch.is_ascii_digit() {
                digits.push(ch);
            } else if !digits.is_empty() {
                break;
            } else if !ch.is_whitespace() {
                return None;
            }
        }
        digits.parse().ok()
    }

    for raw in reader.lines().map_while(Result::ok) {
        // Strip ANSI color codes and trim
        let clean: String = raw.chars().filter(|c| c.is_ascii() || *c == '\n').collect();
        let line = clean.trim().to_string();
        if line.is_empty() {
            continue;
        }

        all_lines.push(line.clone());

        // Classify the line for coloring in the UI
        let kind = if line.contains("[ERROR]") || line.contains("[X]") {
            "error"
        } else if line.contains("[WARNING]") || line.contains("[WARN]") || line.contains("[?]") {
            "warning"
        } else if line.contains("[OK]")
            || line.contains("Best config:")
            || line.contains("Best strategy:")
        {
            "success"
        } else if line.contains("---") || line.contains("===") {
            "separator"
        } else if line.starts_with("  [") {
            "config"
        } else {
            "info"
        };

        // Structured event: "[N/M] foo.bat" → test-config-start
        if line.contains(".bat") {
            if let Some((cur, tot, name)) = parse_config_header(&line) {
                current_config = Some(name.clone());
                let _ = app.emit(
                    "test-config-start",
                    serde_json::json!({
                        "index": cur,
                        "total": tot,
                        "name": name,
                    }),
                );
            }
        }

        // Accumulate per-target ping for the current config. PS1 prints lines
        // like "  Discord Main  HTTP:OK ... | Ping: 45 ms". Ignore the "=== "
        // separator and analytics lines.
        if !line.starts_with('=') && !line.contains(" : HTTP OK:") {
            if let (Some(cfg), Some(ms)) = (current_config.as_ref(), parse_ping_ms(&line)) {
                let entry = ping_stats.entry(cfg.clone()).or_insert((0, 0));
                entry.0 += ms;
                entry.1 += 1;
            }
        }

        // Structured event: "Best config: X"
        if let Some(rest) = line.strip_prefix("Best config:") {
            let _ = app.emit("test-best", serde_json::json!({ "config": rest.trim() }));
        }

        let _ = app.emit(
            "test-progress",
            serde_json::json!({
                "line": line,
                "kind": kind
            }),
        );
    }

    let _ = child.wait();

    // Clear PID — process finished (or was killed)
    {
        let state = app.state::<AppState>();
        let mut pid_lock = state.test_process_pid.lock_unpoisoned();
        *pid_lock = None;
    }

    // Clean up temp script
    let _ = std::fs::remove_file(&temp_script);

    // Parse analytics from accumulated lines
    let mut results = Vec::new();
    let mut in_analytics = false;

    for line in &all_lines {
        if line.contains("=== ANALYTICS ===") {
            in_analytics = true;
            continue;
        }
        // Only parse actual analytics data lines: "<config>.bat : HTTP OK: ..."
        // or "<config>.bat : OK: ..." (DPI). Skip "Best config: ...", separators, etc.
        if in_analytics
            && line.contains(".bat")
            && (line.contains(" : HTTP OK:") || line.contains(" : OK:"))
        {
            if let Some(config_name) = line.split(" : ").next() {
                let config = config_name.trim().to_string();
                // service.bat is the installer script, not a DPI strategy
                if config.eq_ignore_ascii_case("service.bat") {
                    continue;
                }
                let http_ok = extract_number(line, "HTTP OK:");
                let http_error = extract_number(line, "ERR:");
                let ping_ok = extract_number(line, "Ping OK:");
                let ping_fail = extract_number(line, "Fail:");
                let avg_ping_ms = ping_stats
                    .get(&config)
                    .filter(|(_, count)| *count > 0)
                    .map(|(sum, count)| ((sum + count / 2) / count) as i32)
                    .unwrap_or(0);

                let status = if http_error == 0 && ping_fail == 0 {
                    "success"
                } else if http_ok > http_error {
                    "partial"
                } else {
                    "failed"
                };

                let score = http_ok * 10 + ping_ok - http_error * 20 - ping_fail * 2;

                results.push(TestResult {
                    config,
                    status: status.to_string(),
                    http_ok,
                    http_error,
                    ping_ok,
                    ping_fail,
                    avg_ping_ms,
                    score,
                });
            }
        }
    }

    let _ = app.emit("test-done", serde_json::json!({ "count": results.len() }));

    Ok(results)
}

/// Вытаскивает первое целое число после указанного префикса. Игнорирует
/// единицы измерения (например `AvgPing: 45 ms` → 45) и запятые в конце
/// (`Ping OK: 5,` → 5).
fn extract_number(text: &str, prefix: &str) -> i32 {
    let Some(pos) = text.find(prefix) else {
        return 0;
    };
    let after = &text[pos + prefix.len()..];
    let mut started = false;
    let mut digits = String::new();
    for ch in after.chars() {
        if ch.is_ascii_digit() {
            digits.push(ch);
            started = true;
        } else if started {
            break;
        } else if ch == '-' && !started {
            // not expecting negatives, but tolerate
            digits.push(ch);
            started = true;
        }
    }
    digits.parse().unwrap_or(0)
}

#[tauri::command]
fn update_tray_translations(
    translations: TrayTranslations,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) {
    {
        let mut lock = state.translations.lock_unpoisoned();
        *lock = Some(translations.clone());
    }

    // Update labels that don't depend on status
    if let Some(mi) = state.quit_item.lock_unpoisoned().as_ref() {
        let _ = mi.set_text(&translations.exit);
    }
    if let Some(mi) = state.show_item.lock_unpoisoned().as_ref() {
        let _ = mi.set_text(&translations.show);
    }
    if let Some(mi) = state.strategies_submenu.lock_unpoisoned().as_ref() {
        let _ = mi.set_text(&translations.change_strategy);
    }

    refresh_tray_menu(&app);
}

fn refresh_tray_menu(app: &tauri::AppHandle) {
    let state = app.state::<AppState>();
    let status = get_zapret_status(state.clone());
    let trans_lock = state.translations.lock_unpoisoned();
    let trans = match trans_lock.as_ref() {
        Some(t) => t,
        None => return, // Wait until translations are loaded
    };

    let status_mi = state.status_item.lock_unpoisoned().clone();
    if let Some(mi) = status_mi {
        let status_text = if status.running {
            &trans.status_on
        } else {
            &trans.status_off
        };
        let text = format!("{}{}", trans.status_prefix, status_text);
        let _ = mi.set_text(text);
    }

    let strategy_mi = state.strategy_item.lock_unpoisoned().clone();
    if let Some(mi) = strategy_mi {
        let text = format!(
            "{}{}",
            trans.strategy_prefix,
            status.strategy.as_deref().unwrap_or("---")
        );
        let _ = mi.set_text(text);
    }

    let toggle_mi = state.toggle_item.lock_unpoisoned().clone();
    if let Some(mi) = toggle_mi {
        let text = if status.running {
            &trans.toggle_off
        } else {
            &trans.toggle_on
        };
        let _ = mi.set_text(text);
    }
}

#[tauri::command]
fn exit_app(app: tauri::AppHandle) {
    graceful_exit(&app);
}

fn graceful_exit(app: &tauri::AppHandle) {
    if EXIT_IN_PROGRESS.swap(true, Ordering::AcqRel) {
        return;
    }

    stop_zapret_on_exit(app.state::<AppState>());
    if let Some(window) = app.get_webview_window("main") {
        if let Err(error) = window.destroy() {
            eprintln!("Failed to destroy main WebView window during shutdown: {error}");
        }
    }
    app.exit(0);
}

// ─── Entry point ──────────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(windows)]
    elevate_if_needed();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--autostart"]),
        ))
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            let _ = app.get_webview_window("main").map(|w| {
                let _ = w.show();
                let _ = w.unminimize();
                let _ = w.set_focus();

                let state = app.state::<AppState>();
                let tray_opt = state.tray_handle.lock_unpoisoned().clone();
                if let Some(tray) = tray_opt {
                    let _ = tray.set_visible(false);
                }
            });
        }))
        .manage(AppState {
            active_strategy: Mutex::new(None),
            test_process_pid: Mutex::new(None),
            status_item: Mutex::new(None),
            strategy_item: Mutex::new(None),
            toggle_item: Mutex::new(None),
            quit_item: Mutex::new(None),
            show_item: Mutex::new(None),
            strategies_submenu: Mutex::new(None),
            tray_handle: Mutex::new(None),
            notification_shown: AtomicBool::new(false),
            last_strategy: Mutex::new(None),
            translations: Mutex::new(None),
            temp_process_child: Mutex::new(None),
        })
        .setup(|app| {
            let is_autostart = std::env::args().any(|a| a == "--autostart");
            // Hide main window on --autostart so the app boots straight into tray
            if is_autostart {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.hide();
                }
            }

            let quit_i = MenuItemBuilder::with_id("quit", "Exit").build(app)?;
            let show_i = MenuItemBuilder::with_id("show", "Restore window").build(app)?;

            let status_info = MenuItemBuilder::with_id("status_info", "Status: ---")
                .enabled(false)
                .build(app)?;
            let strategy_info = MenuItemBuilder::with_id("strategy_info", "Strategy: ---")
                .enabled(false)
                .build(app)?;
            let toggle_i = MenuItemBuilder::with_id("toggle", "Turn On Zapret").build(app)?;

            // Сохраняем ссылки для динамического обновления
            {
                let state = app.state::<AppState>();
                *state.status_item.lock_unpoisoned() = Some(status_info.clone());
                *state.strategy_item.lock_unpoisoned() = Some(strategy_info.clone());
                *state.toggle_item.lock_unpoisoned() = Some(toggle_i.clone());
                *state.quit_item.lock_unpoisoned() = Some(quit_i.clone());
                *state.show_item.lock_unpoisoned() = Some(show_i.clone());
            }

            // Загружаем стратегии
            let strategies = get_strategies().unwrap_or_default();
            let mut strategies_menu_builder = SubmenuBuilder::new(app, "Change strategy");
            for s in strategies {
                strategies_menu_builder = strategies_menu_builder
                    .item(&MenuItemBuilder::with_id(format!("strat_{}", s), s).build(app)?);
            }
            let strategies_submenu = strategies_menu_builder.build()?;
            {
                let state = app.state::<AppState>();
                *state.strategies_submenu.lock_unpoisoned() = Some(strategies_submenu.clone());
            }

            let menu = MenuBuilder::new(app)
                .item(&status_info)
                .item(&strategy_info)
                .separator()
                .item(&show_i)
                .item(&toggle_i)
                .item(&strategies_submenu)
                .separator()
                .item(&quit_i)
                .build()?;

            let tray = TrayIconBuilder::with_id("main-tray")
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .on_menu_event(move |app, event| {
                    match event.id.as_ref() {
                        "quit" => {
                            graceful_exit(app);
                        }
                        "show" => {
                            if let Some(window) = app.get_webview_window("main") {
                                let _ = window.show();
                                let _ = window.set_focus();
                                // Скрываем иконку при разворачивании
                                let state = app.state::<AppState>();
                                let tray_opt = state.tray_handle.lock_unpoisoned().clone();
                                if let Some(tray) = tray_opt {
                                    let _ = tray.set_visible(false);
                                }
                            }
                        }
                        "toggle" => {
                            let state = app.state::<AppState>();
                            let status = get_zapret_status(state.clone());
                            if status.running {
                                stop_zapret(state);
                            } else {
                                let last = state.last_strategy.lock_unpoisoned().clone();
                                let available = get_strategies().unwrap_or_default();
                                let strategy = last
                                    .or(status.strategy)
                                    .or_else(|| available.first().cloned());
                                if let Some(s) = strategy {
                                    let _ =
                                        start_zapret(app.clone(), s, "service".to_string(), state);
                                }
                            }
                            refresh_tray_menu(app);
                        }
                        id if id.starts_with("strat_") => {
                            let strategy = &id[6..];
                            let state = app.state::<AppState>();
                            let _ = start_zapret(
                                app.clone(),
                                strategy.to_string(),
                                "service".to_string(),
                                state,
                            );
                            refresh_tray_menu(app);
                        }
                        _ => {}
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    match event {
                        TrayIconEvent::Click {
                            button: tauri::tray::MouseButton::Left,
                            ..
                        } => {
                            let app = tray.app_handle();
                            if let Some(window) = app.get_webview_window("main") {
                                let _ = window.show();
                                let _ = window.set_focus();
                                // Скрываем иконку при разворачивании
                                let _ = tray.set_visible(false);
                            }
                        }
                        TrayIconEvent::Click {
                            button: tauri::tray::MouseButton::Right,
                            ..
                        } => {
                            refresh_tray_menu(tray.app_handle());
                        }
                        _ => {}
                    }
                })
                .build(app)?;

            // Сохраняем обработчик трея и задаем изначальную видимость (показываем в трее при автостарте)
            {
                let state = app.state::<AppState>();
                let _ = tray.set_visible(is_autostart);
                *state.tray_handle.lock_unpoisoned() = Some(tray);
            }

            // Первоначальное обновление меню и детекция запущенной стратегии
            {
                let state = app.state::<AppState>();
                let status = get_zapret_status(state.clone());
                if status.running {
                    if let Some(s) = status.strategy {
                        *state.last_strategy.lock_unpoisoned() = Some(s);
                    }
                }
                refresh_tray_menu(app.handle());
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                window.hide().unwrap();
                api.prevent_close();

                // Показываем иконку при сворачивании в трей
                let state = window.app_handle().state::<AppState>();
                let tray_opt = state.tray_handle.lock_unpoisoned().clone();
                if let Some(tray) = tray_opt {
                    let _ = tray.set_visible(true);
                }

                // Показываем уведомление (один раз за сессию)
                if !state.notification_shown.swap(true, Ordering::SeqCst) {
                    let trans_lock = state.translations.lock_unpoisoned();
                    let (title, body) = match trans_lock.as_ref() {
                        Some(t) => (&t.minimized_title, &t.minimized_body),
                        None => (
                            &"Zapret minimized".to_string(),
                            &"The app is still running in the system tray.".to_string(),
                        ),
                    };

                    let _ = window
                        .app_handle()
                        .notification()
                        .builder()
                        .title(title)
                        .body(body)
                        .show();
                }

                // Kill any running test process when the window is closed
                let state = window.app_handle().state::<AppState>();
                let mut pid_lock = state.test_process_pid.lock_unpoisoned();
                if let Some(pid) = pid_lock.take() {
                    let _ = Command::new(system32_tool("taskkill.exe"))
                        .arg("/F")
                        .arg("/T")
                        .arg("/PID")
                        .arg(pid.to_string())
                        .creation_flags(CREATE_NO_WINDOW)
                        .output();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_strategies,
            get_local_version_cmd,
            get_ui_version_cmd,
            get_update_proxy,
            get_zapret_status,
            get_filters_status,
            set_game_filter,
            set_ipset_filter,
            get_fakes_info,
            set_active_fake,
            start_zapret,
            stop_zapret,
            read_user_list,
            write_user_list,
            add_to_user_list,
            remove_from_user_list,
            save_user_list_to_file,
            export_backup_file,
            import_backup_file,
            update_ipset_list,
            get_remote_core_version,
            download_and_install_update,
            get_core_installation_state,
            rollback_core_update,
            run_diagnostics,
            clear_discord_cache,
            check_admin_privileges,
            run_tests,
            check_status_full,
            ensure_binaries_present,
            cancel_tests,
            precheck_tests,
            save_test_results,
            load_test_results,
            update_tray_translations,
            exit_app,
            check_site,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
