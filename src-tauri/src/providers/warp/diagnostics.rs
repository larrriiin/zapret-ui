use super::*;
use tauri_plugin_opener::OpenerExt;

fn executable(cli: &Path) -> Result<PathBuf> {
    let path = cli
        .parent()
        .ok_or_else(|| WarpError::new("warp_diag_unavailable", ""))?
        .join("warp-diag.exe");
    if !path.is_file() {
        return Err(WarpError::new("warp_diag_unavailable", ""));
    }
    verify_signature(&path)?;
    Ok(path)
}
pub(super) fn supported(cli_path: &Path) -> Result<bool> {
    let path = executable(cli_path)?;
    let help = cli(&path, &["--help"])?;
    Ok(help.contains("--output <OUTPUT>") && help.contains("WARP Diagnostics"))
}

#[derive(Serialize)]
pub struct ConnectionCheck {
    installed: bool,
    connected: bool,
    mode: Option<String>,
    protocol: Option<String>,
    proxy_available: Option<bool>,
    route: String,
    error: Option<WarpError>,
}
fn trace_uses_warp(trace: &str) -> bool {
    trace
        .lines()
        .any(|line| matches!(line.trim(), "warp=on" | "warp=plus"))
}
fn finish_check(check: ConnectionCheck) -> Result<ConnectionCheck> {
    if let Some(error) = &check.error {
        if let Some(control) = control::CONTROL
            .lock()
            .map_err(|e| WarpError::new("warp_process", e))?
            .as_mut()
        {
            control::remember_error(control, error.clone());
        }
    }
    Ok(check)
}
#[tauri::command]
pub async fn check_warp_connection() -> Result<ConnectionCheck> {
    tauri::async_runtime::spawn_blocking(|| {
        let mut guard = CLIENT
            .try_lock()
            .map_err(|_| WarpError::new("warp_busy", ""))?;
        let mut check = ConnectionCheck {
            installed: false,
            connected: false,
            mode: None,
            protocol: None,
            proxy_available: None,
            route: "unverified".into(),
            error: None,
        };
        let Some(path) = discover() else {
            check.error = Some(WarpError::new("warp_not_installed", ""));
            return finish_check(check);
        };
        check.installed = true;
        let modified = path.metadata().ok().and_then(|m| m.modified().ok());
        if guard
            .as_ref()
            .is_none_or(|c| c.path != path || c.modified != modified)
        {
            *guard = Some(Client::load(path)?);
        }
        let status = guard.as_ref().unwrap().status();
        check.connected = status.connected;
        check.mode = status.mode;
        check.protocol = status.protocol;
        check.error = status.error;
        if !check.connected || check.error.is_some() {
            return finish_check(check);
        }
        let mut builder = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(8));
        if check.mode.as_deref() == Some("proxy") {
            let Some(port) = status
                .proxy
                .filter(|p| p.kind.as_deref() == Some("SOCKS5"))
                .and_then(|p| p.port)
            else {
                check.proxy_available = Some(false);
                check.error = Some(WarpError::new("warp_proxy_unavailable", ""));
                return finish_check(check);
            };
            let ready = control::socks_ready(port);
            check.proxy_available = Some(ready);
            if !ready {
                check.error = Some(WarpError::new("warp_proxy_unavailable", ""));
                return finish_check(check);
            }
            builder = builder.proxy(
                reqwest::Proxy::all(format!("socks5h://127.0.0.1:{port}"))
                    .map_err(|e| WarpError::new("warp_check_failed", e))?,
            );
        } else if !control::full_tunnel(check.mode.as_deref()) {
            return finish_check(check); // DNS-only mode cannot confirm a WARP traffic route.
        }
        let tested = tauri::async_runtime::block_on(async {
            let mut response = builder
                .build()?
                .get("https://www.cloudflare.com/cdn-cgi/trace")
                .send()
                .await?
                .error_for_status()?;
            let mut body = Vec::new();
            while let Some(chunk) = response.chunk().await? {
                if body.len() + chunk.len() > 16 * 1024 {
                    return Ok(false);
                }
                body.extend_from_slice(&chunk);
            }
            Ok::<bool, reqwest::Error>(trace_uses_warp(&String::from_utf8_lossy(&body)))
        });
        match tested {
            Ok(true) => check.route = "working".into(),
            Ok(false) => {
                check.route = "failed".into();
                check.error = Some(WarpError::new("warp_check_not_warp", ""));
            }
            Err(error) => {
                check.route = "failed".into();
                check.error = Some(WarpError::new("warp_check_failed", error));
            }
        }
        finish_check(check)
    })
    .await
    .map_err(|e| WarpError::new("warp_process", e))?
}

#[tauri::command]
pub async fn create_warp_diagnostics() -> Result<String> {
    tauri::async_runtime::spawn_blocking(|| {
        let _gate = CLIENT
            .try_lock()
            .map_err(|_| WarpError::new("warp_busy", ""))?;
        let path = discover().ok_or_else(|| WarpError::new("warp_not_installed", ""))?;
        if !supported(&path)? {
            return Err(WarpError::new("warp_diag_unavailable", ""));
        }
        let executable = executable(&path)?;
        let mut guard = control::CONTROL
            .lock()
            .map_err(|e| WarpError::new("warp_process", e))?;
        let control = guard
            .as_mut()
            .ok_or_else(|| WarpError::new("warp_preferences_error", ""))?;
        let stamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_err(|e| WarpError::new("warp_diag_failed", e))?
            .as_nanos();
        let directory = control
            .directory
            .join("warp-diagnostics")
            .join(stamp.to_string());
        std::fs::create_dir_all(&directory).map_err(|e| WarpError::new("warp_diag_failed", e))?;
        let mut command = Command::new(executable);
        // Only the supported output option: no packet capture, verbose logging or feedback upload.
        command
            .arg("--output")
            .arg(&directory)
            .current_dir(&directory);
        run(command, Duration::from_secs(90))?;
        let archive = std::fs::read_dir(&directory)
            .map_err(|e| WarpError::new("warp_diag_failed", e))?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .find(|path| {
                path.is_file()
                    && path
                        .extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("zip"))
            })
            .ok_or_else(|| WarpError::new("warp_diag_failed", "No diagnostic ZIP produced"))?;
        control.archive = Some(archive.clone());
        Ok(archive.to_string_lossy().into_owned())
    })
    .await
    .map_err(|e| WarpError::new("warp_process", e))?
}
#[tauri::command]
pub fn reveal_warp_diagnostics(app: tauri::AppHandle) -> Result<()> {
    let guard = control::CONTROL
        .lock()
        .map_err(|e| WarpError::new("warp_process", e))?;
    let archive = guard
        .as_ref()
        .and_then(|c| c.archive.as_ref())
        .filter(|p| p.is_file())
        .ok_or_else(|| WarpError::new("warp_diag_failed", "No archive created this session"))?;
    app.opener()
        .reveal_item_in_dir(archive)
        .map_err(|e| WarpError::new("warp_diag_failed", e))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn successful_http_response_is_not_enough() {
        assert!(!trace_uses_warp("ip=1.1.1.1\nwarp=off\n"));
        assert!(!trace_uses_warp("<html>warp=on</html>"));
        assert!(trace_uses_warp("ip=1.1.1.1\nwarp=on\n"));
        assert!(trace_uses_warp("warp=plus\n"));
    }
}
