use super::warp::{self, WarpError};
use serde::{Deserialize, Serialize};
use std::{
    ffi::{OsStr, OsString},
    net::IpAddr,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{Duration, UNIX_EPOCH},
};
use tauri::Manager;

pub mod installer;
pub mod setup;
pub(crate) mod split;

pub(super) static SWITCH: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
const CLI: &str = "wiresock-connect-cli.exe";
const PROGRAMS: [&str; 2] = ["WireSock Secure Connect", "WireSock Secure Connect Pro"];

fn installation_directories() -> impl Iterator<Item = PathBuf> {
    ["ProgramW6432", "ProgramFiles", "ProgramFiles(x86)"]
        .into_iter()
        .filter(|_| cfg!(windows))
        .filter_map(std::env::var_os)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .flat_map(|root| PROGRAMS.map(|name| root.join(name)))
}

fn locate() -> Option<PathBuf> {
    installation_directories()
        .flat_map(|root| [root.join("command-line").join(CLI), root.join(CLI)])
        .find(|exe| exe.is_file())
}

fn application() -> Option<PathBuf> {
    installation_directories()
        .map(|dir| dir.join("bin").join("WireSockConnect.exe"))
        .find(|exe| exe.is_file())
}

pub(crate) fn is_installed() -> bool {
    locate().is_some() || application().is_some()
}

#[derive(Serialize)]
pub struct Installation {
    installed: bool,
    version: Option<String>,
}

#[tauri::command]
pub fn get_wiresock_installation() -> Installation {
    let app = application();
    let cli = locate();
    Installation {
        installed: app.is_some() || cli.is_some(),
        version: app
            .as_deref()
            .and_then(file_version)
            .or_else(|| cli.as_deref().and_then(file_version)),
    }
}

#[cfg(windows)]
fn file_version(path: &Path) -> Option<String> {
    use std::{mem::size_of, os::windows::ffi::OsStrExt, ptr};
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW, VS_FIXEDFILEINFO,
    };

    let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut handle = 0;
    // The UTF-16 filename is terminated. Version queries use the buffers sized by Windows.
    unsafe {
        let size = GetFileVersionInfoSizeW(path.as_ptr(), &mut handle);
        if size == 0 {
            return None;
        }
        let mut version = vec![0u8; size as usize];
        if GetFileVersionInfoW(path.as_ptr(), 0, size, version.as_mut_ptr().cast()) == 0 {
            return None;
        }
        let root = [b'\\' as u16, 0];
        let mut value = ptr::null_mut();
        let mut length = 0;
        if VerQueryValueW(
            version.as_ptr().cast(),
            root.as_ptr(),
            &mut value,
            &mut length,
        ) == 0
            || value.is_null()
            || (length as usize) < size_of::<VS_FIXEDFILEINFO>()
        {
            return None;
        }
        let info = value.cast::<VS_FIXEDFILEINFO>().read_unaligned();
        (info.dwSignature == 0xfeef04bd).then(|| {
            format!(
                "{}.{}.{}.{}",
                info.dwFileVersionMS >> 16,
                info.dwFileVersionMS & 0xffff,
                info.dwFileVersionLS >> 16,
                info.dwFileVersionLS & 0xffff
            )
        })
    }
}

#[cfg(not(windows))]
fn file_version(_path: &Path) -> Option<String> {
    None
}

async fn cli_output(
    path: &Path,
    args: Vec<OsString>,
    seconds: u64,
) -> Result<Output, &'static str> {
    let mut command = Command::new(path);
    // The CLI normally localizes its headings to the Windows UI language.
    command
        .args(args)
        .env("DOTNET_SYSTEM_GLOBALIZATION_INVARIANT", "1")
        .env(
            "DOTNET_SYSTEM_GLOBALIZATION_PREDEFINED_CULTURES_ONLY",
            "false",
        );
    tauri::async_runtime::spawn_blocking(move || {
        warp::run_output(command, Duration::from_secs(seconds))
    })
    .await
    .map_err(|_| "wiresock_cli_failed")?
    .map_err(|error| {
        if error.code == "warp_timeout" {
            "wiresock_timeout"
        } else {
            "wiresock_cli_failed"
        }
    })
}

async fn cli(path: &Path, args: Vec<OsString>, seconds: u64) -> Result<String, &'static str> {
    let output = cli_output(path, args, seconds).await?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err("wiresock_cli_failed")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
enum State {
    Connected,
    Connecting,
    Disconnecting,
    Disconnected,
}

#[derive(Default, Serialize)]
pub struct WireSockStatus {
    installed: bool,
    state: Option<State>,
    address: Option<IpAddr>,
    profile: Option<String>,
    version: Option<String>,
    error: Option<&'static str>,
    split_tunneling: Option<bool>,
}

fn parse_status(output: &str) -> Option<WireSockStatus> {
    let mut state = None;
    let mut address = None;
    let mut profile = None;
    for line in output.trim_start_matches('\u{feff}').lines().map(str::trim) {
        if let Some(name) = line
            .strip_prefix("Connected to server using profile ")
            .or_else(|| line.strip_prefix("Connecting to "))
            .or_else(|| line.strip_prefix("Disconnecting from profile "))
            .or_else(|| line.strip_prefix("Disconnecting from server using profile "))
        {
            profile = Some(name.trim().to_owned());
        }
        let (label, value) = line.split_once(':').unwrap_or(("", line));
        let label = label.trim().to_ascii_lowercase();
        let value = value.trim();
        if label.contains("address") || label.contains(" ip") {
            address = value.parse::<IpAddr>().ok().or(address);
        } else if label.is_empty() || label.contains("status") || label.contains("state") {
            let normalized = value.to_ascii_lowercase();
            if normalized.trim_end_matches('.') == "not connected" {
                state = Some(State::Disconnected);
                continue;
            }
            let word = value
                .split(|c: char| c.is_whitespace() || c == '(' || c == '.')
                .next()
                .unwrap_or("");
            state = match word.to_ascii_lowercase().as_str() {
                "connected" => Some(State::Connected),
                "connecting" => Some(State::Connecting),
                "disconnecting" => Some(State::Disconnecting),
                "notconnected" | "disconnected" => Some(State::Disconnected),
                _ => state,
            };
        }
    }
    state.map(|state| WireSockStatus {
        installed: true,
        state: Some(state),
        address: address.filter(|_| state == State::Connected),
        profile: profile.filter(|_| state != State::Disconnected),
        ..Default::default()
    })
}

async fn status(path: &Path) -> Result<WireSockStatus, &'static str> {
    let result = cli_output(path, vec!["status".into()], 4)
        .await
        .and_then(|output| {
            let report = parse_status(&String::from_utf8_lossy(&output.stdout))
                .or_else(|| parse_status(&String::from_utf8_lossy(&output.stderr)));
            status_result(output.status.success(), report)
        });
    #[cfg(windows)]
    if result.is_err()
        && matches!(
            crate::windows_services::query_state("WireSockConnectService"),
            Ok(Some(crate::windows_services::ServiceState::Stopped))
        )
    {
        return Ok(disconnected_status());
    }
    result
}

fn status_result(
    success: bool,
    report: Option<WireSockStatus>,
) -> Result<WireSockStatus, &'static str> {
    if success {
        report.ok_or("wiresock_status_unknown")
    } else {
        report
            .filter(|report| report.state == Some(State::Disconnected))
            .ok_or("wiresock_cli_failed")
    }
}

async fn wait_for_state(path: &Path, target: State) -> Option<WireSockStatus> {
    for attempt in 0..5 {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_millis(300)).await;
        }
        if let Ok(report) = status(path).await {
            if report.state == Some(target) {
                return Some(report);
            }
        }
    }
    None
}

#[tauri::command]
pub async fn get_wiresock_status(app: tauri::AppHandle) -> WireSockStatus {
    let installation = get_wiresock_installation();
    let result = match locate() {
        Some(path) => status(&path).await,
        None if installation.installed => Err("wiresock_cli_missing"),
        None => return WireSockStatus::default(),
    };
    let mut report = result.unwrap_or_else(|error| WireSockStatus {
        installed: true,
        error: Some(error),
        ..Default::default()
    });
    report.version = installation.version;
    report.split_tunneling = split::global::split_tunneling_enabled(&app).ok();
    split::global::describe(&app, &mut report);
    report
}

pub(super) async fn prepare_warp_connection(switch: bool) -> Result<(), WarpError> {
    let Some(path) = locate() else { return Ok(()) };
    let state = match status(&path).await {
        Ok(report) => report.state,
        #[cfg(windows)]
        Err(_)
            if matches!(
                crate::windows_services::query_state("WireSockConnectService"),
                Ok(None | Some(crate::windows_services::ServiceState::Stopped))
            ) =>
        {
            Some(State::Disconnected)
        }
        Err(code) => return Err(WarpError::new(code, "")),
    };
    if state == Some(State::Disconnected) {
        return Ok(());
    }
    if !switch {
        return Err(WarpError::new("wiresock_active", ""));
    }
    disconnect(&path)
        .await
        .map(|_| ())
        .map_err(|code| WarpError::new(code, ""))
}

pub(crate) fn valid_profile_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && !name.starts_with('-')
        && !name
            .chars()
            .any(|c| c.is_control() || c == '/' || c == '\\')
}

fn parse_profiles(output: &str) -> Vec<String> {
    let mut profiles = Vec::new();
    for line in output.trim_start_matches('\u{feff}').lines().map(str::trim) {
        let lower = line.to_ascii_lowercase();
        if lower.ends_with(':')
            || lower.starts_with("no profiles")
            || lower.starts_with("no available profiles")
        {
            continue;
        }
        let line = line
            .strip_prefix("- ")
            .or_else(|| line.strip_prefix("* "))
            .unwrap_or(line);
        let name = line
            .split_once(". ")
            .filter(|(num, _)| num.parse::<usize>().is_ok())
            .map_or(line, |(_, name)| name)
            .trim()
            .trim_matches('"');
        if valid_profile_name(name) && !profiles.iter().any(|value| value == name) {
            profiles.push(name.to_string());
        }
    }
    profiles
}

async fn imported(path: &Path) -> Result<Vec<String>, &'static str> {
    cli(path, vec!["list".into()], 6)
        .await
        .map(|out| parse_profiles(&out))
}

fn is_generated(name: &str) -> bool {
    name == "WARP-WireSock"
        || ["WARP-WireSock-", "WireSock-Split-"]
            .into_iter()
            .filter_map(|prefix| name.strip_prefix(prefix))
            .any(|suffix| suffix.len() == 16 && suffix.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

fn profile_in_source(name: &str, source: ProfileFileSource) -> bool {
    is_generated(name)
        && (source == ProfileFileSource::AppData || !name.starts_with("WireSock-Split-"))
}

fn created_at(path: &Path) -> Option<u64> {
    let metadata = std::fs::symlink_metadata(path).ok()?;
    if !metadata.is_file() {
        return None;
    }
    Some(
        metadata
            .created()
            .ok()?
            .duration_since(UNIX_EPOCH)
            .ok()?
            .as_secs(),
    )
}

fn imported_date(name: &str) -> Option<u64> {
    // Never turn a CLI profile label into a path. Match a filename under the vendor directory.
    let root = PathBuf::from(std::env::var_os("ProgramData")?).join("WireSock Foundation");
    PROGRAMS.into_iter().find_map(|edition| {
        std::fs::read_dir(root.join(edition).join("Profiles"))
            .ok()?
            .flatten()
            .find_map(|entry| {
                let path = entry.path();
                (path
                    .extension()
                    .and_then(OsStr::to_str)
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("conf"))
                    && path
                        .file_stem()
                        .and_then(OsStr::to_str)
                        .is_some_and(|stem| stem.eq_ignore_ascii_case(name)))
                .then(|| created_at(&path))
                .flatten()
            })
    })
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProfileFileSource {
    AppData,
    LegacyDownloads,
}

fn generated_directories(
    app: &tauri::AppHandle,
) -> impl Iterator<Item = (ProfileFileSource, PathBuf)> {
    [
        (
            ProfileFileSource::AppData,
            warp::profile::profile_directory(app),
        ),
        (
            ProfileFileSource::LegacyDownloads,
            app.path().download_dir(),
        ),
    ]
    .into_iter()
    .filter_map(|(source, path)| path.ok().map(|path| (source, path)))
}

fn generated_path(
    directories: impl IntoIterator<Item = (ProfileFileSource, PathBuf)>,
    profile: &str,
) -> Option<(ProfileFileSource, PathBuf)> {
    directories
        .into_iter()
        .filter(|(source, _)| profile_in_source(profile, *source))
        .find_map(|(source, dir)| {
            let path = dir.join(format!("{profile}.conf"));
            std::fs::symlink_metadata(&path)
                .ok()?
                .is_file()
                .then_some((source, path))
        })
}

#[derive(Serialize)]
pub struct Profile {
    name: String,
    imported: bool,
    file_source: Option<ProfileFileSource>,
    created_at: Option<u64>,
}

#[tauri::command]
pub async fn list_wiresock_profiles(app: tauri::AppHandle) -> Result<Vec<Profile>, &'static str> {
    let path = locate().ok_or("wiresock_not_installed")?;
    let mut profiles: Vec<_> = imported(&path)
        .await?
        .into_iter()
        .filter(|name| !split::global::is_runtime_profile(name))
        .map(|name| Profile {
            created_at: imported_date(&name),
            name,
            imported: true,
            file_source: None,
        })
        .collect();
    merge_generated_profiles(&mut profiles, generated_directories(&app));
    profiles.sort_by(|a, b| {
        b.created_at
            .cmp(&a.created_at)
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(profiles)
}

fn merge_generated_profiles(
    profiles: &mut Vec<Profile>,
    directories: impl IntoIterator<Item = (ProfileFileSource, PathBuf)>,
) {
    for (source, dir) in directories {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries
            .flatten()
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        {
            let path = entry.path();
            if path.extension() != Some(OsStr::new("conf")) {
                continue;
            }
            let Some(name) = path
                .file_stem()
                .and_then(OsStr::to_str)
                .filter(|name| profile_in_source(name, source))
            else {
                continue;
            };
            if let Some(profile) = profiles
                .iter_mut()
                .find(|profile| profile.name.eq_ignore_ascii_case(name))
            {
                if profile.file_source.is_none() {
                    profile.file_source = Some(source);
                    profile.created_at = created_at(&path).or(profile.created_at);
                }
            } else {
                profiles.push(Profile {
                    name: name.to_owned(),
                    imported: false,
                    file_source: Some(source),
                    created_at: created_at(&path),
                });
            }
        }
    }
}

async fn import_file(cli_path: &Path, profile: &Path) -> Result<String, &'static str> {
    let full_path = profile
        .canonicalize()
        .map(crate::strip_verbatim_prefix)
        .map_err(|_| "wiresock_import_failed")?;
    let name = profile
        .file_stem()
        .and_then(OsStr::to_str)
        .filter(|name| valid_profile_name(name))
        .ok_or("wiresock_import_failed")?;
    if !full_path.is_file()
        || !profile
            .extension()
            .and_then(OsStr::to_str)
            .is_some_and(|ext| ext.eq_ignore_ascii_case("conf"))
    {
        return Err("wiresock_import_failed");
    }
    cli(
        cli_path,
        vec!["import".into(), full_path.into_os_string()],
        12,
    )
    .await
    .map_err(|_| "wiresock_import_failed")?;
    imported(cli_path)
        .await?
        .into_iter()
        .find(|profile| profile.eq_ignore_ascii_case(name))
        .ok_or("wiresock_import_failed")
}

#[tauri::command]
pub async fn import_wiresock_profile(
    window: tauri::WebviewWindow,
) -> Result<Option<String>, &'static str> {
    let path = locate().ok_or("wiresock_not_installed")?;
    let mut dialog = rfd::AsyncFileDialog::new()
        .set_parent(&window)
        .add_filter("WireGuard", &["conf"]);
    if let Ok(downloads) = window.path().download_dir() {
        dialog = dialog.set_directory(downloads);
    }
    let Some(file) = dialog.pick_file().await else {
        return Ok(None);
    };
    if file
        .path()
        .file_stem()
        .and_then(OsStr::to_str)
        .is_some_and(split::global::is_runtime_profile)
    {
        return Err("wiresock_import_failed");
    }
    let _switch = SWITCH.lock().await;
    import_file(&path, file.path()).await.map(Some)
}

#[tauri::command]
pub(crate) async fn delete_wiresock_profile(
    app: tauri::AppHandle,
    profile: String,
    file_source: Option<ProfileFileSource>,
) -> Result<(), &'static str> {
    if !valid_profile_name(&profile) {
        return Err("wiresock_profile_missing");
    }
    let path = locate().ok_or("wiresock_not_installed")?;
    let _switch = SWITCH.lock().await;
    let file = generated_path(generated_directories(&app), &profile);
    if file.as_ref().map(|(source, _)| *source) != file_source {
        return Err("wiresock_profile_missing");
    }
    let imported_name = imported(&path)
        .await?
        .into_iter()
        .find(|name| name.eq_ignore_ascii_case(&profile));
    if imported_name.is_none() && file.is_none() {
        return Err("wiresock_profile_missing");
    }
    if let Some(name) = imported_name {
        if status(&path).await?.state != Some(State::Disconnected) {
            return Err("wiresock_delete_connected");
        }
        let result = cli(&path, vec!["delete".into(), name.clone().into()], 12).await;
        if imported(&path)
            .await?
            .iter()
            .any(|entry| entry.eq_ignore_ascii_case(&name))
        {
            result?;
            return Err("wiresock_delete_failed");
        }
    }
    if let Some((_, file)) = file {
        std::fs::remove_file(file).map_err(|_| "wiresock_delete_failed")?;
    }
    Ok(())
}

#[tauri::command]
pub async fn connect_wiresock(
    app: tauri::AppHandle,
    profile: String,
    generated: bool,
) -> Result<WireSockStatus, &'static str> {
    connect_profile(&app, profile, generated, false).await
}

#[tauri::command]
pub async fn reconnect_wiresock(
    app: tauri::AppHandle,
    profile: String,
    generated: bool,
) -> Result<WireSockStatus, &'static str> {
    connect_profile(&app, profile, generated, true).await
}

async fn connect_profile(
    app: &tauri::AppHandle,
    profile: String,
    generated: bool,
    reconnect: bool,
) -> Result<WireSockStatus, &'static str> {
    if !valid_profile_name(&profile)
        || split::global::is_runtime_profile(&profile)
        || (generated && !is_generated(&profile))
    {
        return Err("wiresock_profile_missing");
    }
    let path = locate().ok_or("wiresock_not_installed")?;
    let _switch = SWITCH.lock().await;
    let before = status(&path).await?.state;
    if reconnect && !matches!(before, Some(State::Connected | State::Connecting)) {
        return Err("wiresock_connect_failed");
    }
    let split_enabled = split::global::split_tunneling_enabled(app)?;
    let global_rules = if split_enabled {
        let rules = split::global::get_wiresock_global_rules(app.clone())?;
        rules.enabled.then_some(rules)
    } else {
        None
    };
    let use_temporary_profile = !split_enabled || global_rules.is_some();
    let mut names = imported(&path).await?;
    let mut selected = names
        .iter()
        .position(|name| name.eq_ignore_ascii_case(&profile))
        .map(|index| names.swap_remove(index));
    if generated && selected.is_none() {
        let (_, file) = generated_path(generated_directories(app), &profile)
            .ok_or("wiresock_profile_missing")?;
        selected = Some(if use_temporary_profile {
            profile.clone()
        } else {
            import_file(&path, &file).await?
        });
    }
    let mut profile = selected.ok_or("wiresock_profile_missing")?;
    if reconnect {
        disconnect(&path).await?;
    }
    let disconnected = reconnect || before == Some(State::Disconnected);
    if disconnected {
        let cleanup = split::global::cleanup(app, &path).await;
        if use_temporary_profile {
            cleanup?;
        }
    }
    if use_temporary_profile {
        if !disconnected {
            return Err("wiresock_global_error");
        }
        profile = split::global::profile_for_connection(app, &path, &profile, global_rules).await?;
    }
    warp::stop_for_wiresock().await?;
    cli(
        &path,
        vec!["connect".into(), profile.into(), "-exit".into()],
        30,
    )
    .await?;
    let mut report = wait_for_state(&path, State::Connected)
        .await
        .ok_or("wiresock_connect_failed")?;
    split::global::describe(app, &mut report);
    Ok(report)
}

async fn disconnect(path: &Path) -> Result<WireSockStatus, &'static str> {
    let output = cli_output(path, vec!["disconnect".into()], 12).await;
    if output.as_ref().is_ok_and(|output| {
        output.status.success()
            || reports_disconnected(&String::from_utf8_lossy(&output.stdout))
            || reports_disconnected(&String::from_utf8_lossy(&output.stderr))
    }) {
        // The documented command waits until the active tunnel is down.
        return Ok(disconnected_status());
    }
    if let Some(report) = wait_for_state(path, State::Disconnected).await {
        return Ok(report);
    }
    Err("wiresock_disconnect_failed")
}

fn disconnected_status() -> WireSockStatus {
    WireSockStatus {
        installed: true,
        state: Some(State::Disconnected),
        ..Default::default()
    }
}

fn reports_disconnected(output: &str) -> bool {
    output.lines().any(|line| {
        let line = line.trim().trim_end_matches('.');
        line.eq_ignore_ascii_case("Not connected")
            || line
                .strip_prefix("Profile ")
                .is_some_and(|text| text.ends_with(" is disconnected"))
    })
}

#[tauri::command]
pub async fn disconnect_wiresock(app: tauri::AppHandle) -> Result<WireSockStatus, &'static str> {
    let path = locate().ok_or("wiresock_not_installed")?;
    let _switch = SWITCH.lock().await;
    let report = disconnect(&path).await?;
    let _ = split::global::cleanup(&app, &path).await;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_status_without_confusing_not_connected_and_connected() {
        assert_eq!(
            parse_status("Connection status: NotConnected")
                .unwrap()
                .state,
            Some(State::Disconnected)
        );
        assert_eq!(
            parse_status("Not connected").unwrap().state,
            Some(State::Disconnected)
        );
        assert_eq!(
            parse_status(
                "Connected to server using profile Home VPN\nExternal address: 203.0.113.4"
            )
            .unwrap()
            .address
            .unwrap()
            .to_string(),
            "203.0.113.4"
        );
        assert_eq!(
            parse_status("Disconnecting from profile Home VPN")
                .unwrap()
                .state,
            Some(State::Disconnecting)
        );
        assert!(parse_status("Could not connect to service").is_none());
    }
    #[test]
    fn disconnected_output_is_valid_even_on_nonzero_exit() {
        assert_eq!(
            status_result(false, parse_status("Not connected"))
                .unwrap()
                .state,
            Some(State::Disconnected)
        );
        assert!(status_result(
            false,
            parse_status("The connection to the service has been lost")
        )
        .is_err());
        assert!(reports_disconnected("Profile Home VPN is disconnected"));
        assert!(!reports_disconnected("Not connected to the service"));
        assert_eq!(
            parse_status("Connected to server using profile Home VPN")
                .unwrap()
                .profile
                .as_deref(),
            Some("Home VPN")
        );
    }
    #[test]
    fn keeps_profile_names_and_rejects_command_like_entries() {
        assert_eq!(
            parse_profiles("Available profiles:\n1. Home VPN\n- WARP\n--exit"),
            ["Home VPN", "WARP"]
        );
        assert!(parse_profiles("No profiles found.").is_empty());
        assert!(!is_generated("WARP-WireSock-../../x"));
    }

    #[test]
    fn finds_old_downloads_and_prefers_profiles_saved_in_app_data() {
        let managed = tempfile::tempdir().unwrap();
        let downloads = tempfile::tempdir().unwrap();
        let shared = "WARP-WireSock-0123456789ABCDEF";
        let old = "WARP-WireSock";
        let split = "WireSock-Split-0123456789ABCDEF";
        let external_split = "WireSock-Split-ABCDEF0123456789";
        for file in [
            managed.path().join(format!("{shared}.conf")),
            managed.path().join(format!("{split}.conf")),
            downloads.path().join(format!("{shared}.conf")),
            downloads.path().join(format!("{old}.conf")),
            downloads.path().join(format!("{external_split}.conf")),
            downloads.path().join("user.conf"),
        ] {
            std::fs::write(file, []).unwrap();
        }
        let directories = || {
            [
                (ProfileFileSource::AppData, managed.path().to_owned()),
                (
                    ProfileFileSource::LegacyDownloads,
                    downloads.path().to_owned(),
                ),
            ]
        };
        let mut profiles = Vec::new();
        merge_generated_profiles(&mut profiles, directories());
        assert_eq!(profiles.len(), 3);
        for (name, source) in [
            (shared, ProfileFileSource::AppData),
            (split, ProfileFileSource::AppData),
            (old, ProfileFileSource::LegacyDownloads),
        ] {
            let listed = profiles
                .iter()
                .find(|profile| profile.name == name)
                .unwrap();
            assert_eq!(listed.file_source, Some(source));
            assert_eq!(generated_path(directories(), name).unwrap().0, source);
        }
        assert!(generated_path(directories(), external_split).is_none());
        assert!(generated_path(directories(), "user").is_none());
    }
}
