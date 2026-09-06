//! Official Windows WARP client. No Zapret state or arbitrary command/path IPC.
use serde::Serialize;
use serde_json::Value;
use std::{
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Mutex,
    time::{Duration, Instant, SystemTime},
};

pub mod installer;

const MODES: &[&str] = &[
    "doh",
    "dot",
    "warp",
    "warp+dot",
    "warp+doh",
    "proxy",
    "tunnel_only",
];
static CLIENT: Mutex<Option<Client>> = Mutex::new(None);

#[derive(Clone, Debug, Serialize)]
pub struct WarpError {
    pub code: String,
    pub detail: String,
}
impl WarpError {
    pub(super) fn new(code: &str, detail: impl ToString) -> Self {
        Self {
            code: code.into(),
            detail: detail.to_string(),
        }
    }
}
type Result<T> = std::result::Result<T, WarpError>;

struct Client {
    path: PathBuf,
    modified: Option<SystemTime>,
    version: String,
    modes: Vec<String>,
    proxy_type: Option<String>,
    port_editable: bool,
}

#[derive(Default, Debug, Serialize)]
pub struct WarpStatus {
    installed: bool,
    connected: bool,
    state: String,
    mode: Option<String>,
    version: Option<String>,
    modes: Vec<String>,
    proxy: Option<ProxyStatus>,
    error: Option<WarpError>,
}
#[derive(Debug, Serialize)]
struct ProxyStatus {
    address: String,
    port: Option<u16>,
    kind: Option<String>,
    active: bool,
    port_editable: bool,
}

pub(super) fn system_exe(relative: &str) -> PathBuf {
    PathBuf::from(std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into()))
        .join("System32")
        .join(relative)
}

// Drain both pipes concurrently; never block a Tauri async worker or leave a timed-out CLI alive.
pub(super) fn run(command: Command, timeout: Duration) -> Result<String> {
    run_with_success_codes(command, timeout, &[0])
}

pub(super) fn run_with_success_codes(
    mut command: Command,
    timeout: Duration,
    success_codes: &[i32],
) -> Result<String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| WarpError::new("warp_process", e))?;
    fn drain(pipe: impl Read + Send + 'static) -> std::thread::JoinHandle<Vec<u8>> {
        std::thread::spawn(move || {
            let mut data = Vec::new();
            let _ = pipe.take(2 * 1024 * 1024).read_to_end(&mut data);
            data
        })
    }
    let stdout = drain(child.stdout.take().unwrap());
    let stderr = drain(child.stderr.take().unwrap());
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < timeout => {
                std::thread::sleep(Duration::from_millis(30))
            }
            other => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout.join();
                let _ = stderr.join();
                return Err(WarpError::new("warp_timeout", format!("{other:?}")));
            }
        }
    };
    let output = String::from_utf8_lossy(&stdout.join().unwrap_or_default())
        .trim()
        .to_owned();
    let error = String::from_utf8_lossy(&stderr.join().unwrap_or_default())
        .trim()
        .to_owned();
    if !status
        .code()
        .is_some_and(|code| success_codes.contains(&code))
    {
        return Err(WarpError::new(
            "warp_cli_error",
            format!(
                "{status}: {}",
                if error.is_empty() { output } else { error }
            ),
        ));
    }
    Ok(output)
}

pub(super) fn verify_signature(path: &Path) -> Result<()> {
    let mut command = Command::new(system_exe("WindowsPowerShell/v1.0/powershell.exe"));
    // Path travels through an environment variable, never interpolated into shell source.
    command.env("ZAPRET_WARP_VERIFY", path).args(["-NoProfile", "-NonInteractive", "-Command",
        "$ErrorActionPreference='Stop'; $OutputEncoding=[Console]::OutputEncoding=[System.Text.UTF8Encoding]::new(); Import-Module \"$PSHOME\\Modules\\Microsoft.PowerShell.Security\\Microsoft.PowerShell.Security.psd1\" -ErrorAction Stop; $s=Get-AuthenticodeSignature -LiteralPath $env:ZAPRET_WARP_VERIFY; if ($s.Status -ne 'Valid' -or $s.SignerCertificate.GetNameInfo([System.Security.Cryptography.X509Certificates.X509NameType]::SimpleName,$false) -ne 'Cloudflare, Inc.') { throw 'Invalid Cloudflare Authenticode signature' }"]);
    run(command, Duration::from_secs(30))
        .map(|_| ())
        .map_err(|e| WarpError::new("warp_signature", e.detail))
}

fn discover() -> Option<PathBuf> {
    // Official Windows installation directories; never search PATH/current directory.
    ["ProgramW6432", "ProgramFiles", "ProgramFiles(x86)"]
        .iter()
        .filter_map(std::env::var_os)
        .map(|root| PathBuf::from(root).join("Cloudflare/Cloudflare WARP/warp-cli.exe"))
        .find(|path| path.is_absolute() && path.is_file())
}

fn cli(path: &Path, args: &[&str]) -> Result<String> {
    let mut command = Command::new(path);
    command.args(args);
    run(command, Duration::from_secs(10))
}

fn supported_modes(help: &str) -> Vec<String> {
    MODES
        .iter()
        .filter(|mode| {
            help.lines().any(|line| {
                line.trim()
                    .strip_prefix("- ")
                    .and_then(|s| s.split_once(':'))
                    .is_some_and(|(name, _)| name.trim() == **mode)
            })
        })
        .map(|m| (*m).into())
        .collect()
}

impl Client {
    fn load(path: PathBuf) -> Result<Self> {
        verify_signature(&path)?;
        let version = cli(&path, &["--version"])?;
        let help = cli(&path, &["--help"])?;
        if !help.contains("--json") {
            return Err(WarpError::new(
                "warp_unsupported",
                "JSON output unavailable",
            ));
        }
        let mode_help = cli(&path, &["mode", "--help"])?;
        let modes = supported_modes(&mode_help);
        if modes.is_empty() {
            return Err(WarpError::new(
                "warp_unsupported",
                "No recognized modes in CLI help",
            ));
        }
        let proxy_type = mode_help
            .lines()
            .find(|l| l.trim().starts_with("- proxy:"))
            .filter(|l| l.contains("SOCKS5"))
            .map(|_| "SOCKS5".into());
        let port_help = cli(&path, &["proxy", "port", "--help"]).unwrap_or_default();
        let port_editable = port_help.contains("<PORT>") && port_help.contains("127.0.0.1");
        Ok(Self {
            modified: path.metadata().ok().and_then(|m| m.modified().ok()),
            path,
            version,
            modes,
            proxy_type,
            port_editable,
        })
    }

    fn status(&self) -> WarpStatus {
        let mut result = WarpStatus {
            installed: true,
            state: "error".into(),
            version: Some(self.version.clone()),
            modes: self.modes.clone(),
            ..Default::default()
        };
        let read = || -> Result<(Value, Value)> {
            let status = serde_json::from_str(&cli(&self.path, &["--json", "status"])?)
                .map_err(|e| WarpError::new("warp_unsupported", e))?;
            let settings = serde_json::from_str(&cli(&self.path, &["--json", "settings"])?)
                .map_err(|e| WarpError::new("warp_unsupported", e))?;
            Ok((status, settings))
        };
        match read() {
            Ok((status, settings)) => {
                result.state = parse_state(&status).into();
                result.connected = result.state == "connected";
                result.mode = settings
                    .pointer("/settings/operation_mode")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                if result.state == "error" || result.mode.is_none() {
                    result.error = Some(WarpError::new(
                        "warp_status_error",
                        status.get("status").unwrap_or(&Value::Null),
                    ));
                }
                if result.mode.as_deref() == Some("proxy") {
                    result.proxy = Some(ProxyStatus {
                        address: "127.0.0.1".into(),
                        port: settings
                            .pointer("/settings/proxy_port")
                            .and_then(Value::as_u64)
                            .and_then(|p| u16::try_from(p).ok())
                            .filter(|p| *p > 0),
                        kind: self.proxy_type.clone(),
                        active: result.connected,
                        port_editable: self.port_editable,
                    });
                }
            }
            Err(error) => result.error = Some(error),
        }
        result
    }
}

fn parse_state(status: &Value) -> &'static str {
    match status
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "connected" => "connected",
        "disconnected" => "disconnected",
        "connecting" => "connecting",
        "disconnecting" => "disconnecting",
        _ => "error",
    }
}

enum Action {
    Status,
    Connect,
    Disconnect,
    Mode(String),
    Port(u16),
}
async fn execute(action: Action) -> Result<WarpStatus> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut guard = CLIENT
            .try_lock()
            .map_err(|_| WarpError::new("warp_busy", ""))?;
        let Some(path) = discover() else {
            *guard = None;
            return match action {
                Action::Status => Ok(WarpStatus {
                    state: "disconnected".into(),
                    ..Default::default()
                }),
                _ => Err(WarpError::new("warp_not_installed", "")),
            };
        };
        let modified = path.metadata().ok().and_then(|m| m.modified().ok());
        if guard
            .as_ref()
            .is_none_or(|c| c.path != path || c.modified != modified)
        {
            match Client::load(path) {
                Ok(client) => *guard = Some(client),
                Err(error) => {
                    *guard = None;
                    return Ok(WarpStatus {
                        installed: true,
                        state: "error".into(),
                        error: Some(error),
                        ..Default::default()
                    });
                }
            }
        }
        let client = guard.as_ref().unwrap();
        match action {
            Action::Status => {}
            Action::Connect => {
                cli(&client.path, &["connect"])?;
            }
            Action::Disconnect => {
                cli(&client.path, &["disconnect"])?;
            }
            Action::Mode(mode) => {
                if !client.modes.contains(&mode) {
                    return Err(WarpError::new("warp_unsupported", ""));
                }
                cli(&client.path, &["mode", &mode])?;
            }
            Action::Port(port) => {
                if port == 0 || !client.port_editable {
                    return Err(WarpError::new("warp_invalid_port", ""));
                }
                cli(&client.path, &["proxy", "port", &port.to_string()])?;
            }
        }
        Ok(client.status())
    })
    .await
    .map_err(|e| WarpError::new("warp_process", e))?
}

#[tauri::command]
pub async fn get_warp_status() -> Result<WarpStatus> {
    execute(Action::Status).await
}
#[tauri::command]
pub async fn connect_warp() -> Result<WarpStatus> {
    execute(Action::Connect).await
}
#[tauri::command]
pub async fn disconnect_warp() -> Result<WarpStatus> {
    execute(Action::Disconnect).await
}
#[tauri::command]
pub async fn get_warp_mode() -> Result<WarpStatus> {
    execute(Action::Status).await
}
#[tauri::command]
pub async fn set_warp_mode(mode: String) -> Result<WarpStatus> {
    execute(Action::Mode(mode)).await
}
#[tauri::command]
pub async fn set_warp_proxy_port(port: u16) -> Result<WarpStatus> {
    execute(Action::Port(port)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn modes_require_exact_help_entries() {
        assert_eq!(
            supported_modes("- warp+doh: tunnel\n- proxy: SOCKS5\n- invented: no\nwarp dot"),
            vec!["warp+doh", "proxy"]
        );
    }
    #[test]
    fn disconnected_is_never_connected() {
        for (source, expected) in [
            ("Connected", "connected"),
            ("Disconnected", "disconnected"),
            ("Connecting", "connecting"),
            ("Disconnecting", "disconnecting"),
            ("Paused", "error"),
        ] {
            assert_eq!(
                parse_state(&serde_json::json!({"status": source})),
                expected
            );
        }
    }
    #[tokio::test]
    #[ignore = "Read-only integration test requires an installed official Windows WARP client"]
    async fn installed_client_status() {
        let status = get_warp_status().await.unwrap();
        assert!(status.installed);
        assert!(status.error.is_none(), "{:?}", status.error);
        assert!(status.mode.is_some());
        assert_eq!(status.modes.len(), 7);
    }

    #[tokio::test]
    #[ignore = "Temporarily changes modes/port on a disconnected official client, then restores them"]
    async fn installed_client_modes_and_proxy() {
        let original = get_warp_status().await.unwrap();
        assert_eq!(
            original.state, "disconnected",
            "Disconnect WARP before this opt-in test"
        );
        let path = discover().unwrap();
        struct Restore {
            path: PathBuf,
            mode: String,
            port: Option<u16>,
        }
        impl Drop for Restore {
            fn drop(&mut self) {
                if let Some(port) = self.port {
                    let _ = cli(&self.path, &["proxy", "port", &port.to_string()]);
                }
                let _ = cli(&self.path, &["mode", &self.mode]);
            }
        }
        let mut restore = Restore {
            path,
            mode: original.mode.unwrap(),
            port: None,
        };
        for mode in original.modes {
            let status = set_warp_mode(mode.clone()).await.unwrap();
            assert!(status.error.is_none(), "{:?}", status.error);
            assert_eq!(status.mode.as_deref(), Some(mode.as_str()));
            assert!(!status.connected);
            if mode == "proxy" {
                let proxy = status.proxy.unwrap();
                assert_eq!(proxy.kind.as_deref(), Some("SOCKS5"));
                assert_eq!(proxy.address, "127.0.0.1");
                restore.port = proxy.port;
                let changed = set_warp_proxy_port(if proxy.port == Some(40001) {
                    40002
                } else {
                    40001
                })
                .await
                .unwrap();
                assert_ne!(changed.proxy.unwrap().port, proxy.port);
                assert!(set_warp_proxy_port(0).await.is_err());
            }
        }
        assert!(set_warp_mode("warp; calc.exe".into()).await.is_err());
        drop(restore);
    }
}
