//! Download only from Cloudflare, verify publisher, then run the official MSI UI.
use super::{run_with_success_codes, system_exe, verify_signature, WarpError};
use futures_util::StreamExt;
use std::{
    io::Write,
    process::Command,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

static INSTALLING: AtomicBool = AtomicBool::new(false);
struct InstallGuard;
impl Drop for InstallGuard {
    fn drop(&mut self) {
        INSTALLING.store(false, Ordering::Release);
    }
}

fn official_url(url: &reqwest::Url) -> bool {
    url.scheme() == "https"
        && url.port_or_known_default() == Some(443)
        && url.username().is_empty()
        && url.password().is_none()
        && matches!(
            url.host_str(),
            Some("downloads.cloudflareclient.com" | "1111-releases.cloudflareclient.com")
        )
}

#[tauri::command]
pub async fn install_warp() -> Result<(), WarpError> {
    if INSTALLING.swap(true, Ordering::AcqRel) {
        return Err(WarpError::new("warp_busy", ""));
    }
    let _guard = InstallGuard;
    let client = reqwest::Client::builder()
        .https_only(true)
        .timeout(Duration::from_secs(600))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 5 || !official_url(attempt.url()) {
                attempt.error("Non-Cloudflare redirect blocked")
            } else {
                attempt.follow()
            }
        }))
        .build()
        .map_err(|e| WarpError::new("warp_download", e))?;
    let response = client
        .get("https://downloads.cloudflareclient.com/v1/download/windows/ga")
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|e| WarpError::new("warp_download", e))?;
    if !official_url(response.url()) {
        return Err(WarpError::new("warp_download", "Untrusted URL"));
    }
    // Unique exclusive download file. Reopen read-only before verification, then
    // hold against writes/deletion until msiexec finishes (including its UI).
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "zapret-cloudflare-{}-{unique}.msi",
        std::process::id()
    ));
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true).create_new(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(1);
    }
    let mut file = options
        .open(&path)
        .map_err(|e| WarpError::new("warp_download", e))?;
    let download_result = async {
        let mut stream = response.bytes_stream();
        let mut size = 0usize;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| WarpError::new("warp_download", e))?;
            size += chunk.len();
            if size > 512 * 1024 * 1024 {
                return Err(WarpError::new("warp_download", "Installer exceeds 512 MiB"));
            }
            file.write_all(&chunk)
                .map_err(|e| WarpError::new("warp_download", e))?;
        }
        file.sync_all()
            .map_err(|e| WarpError::new("warp_download", e))?;
        Ok(())
    }
    .await;
    drop(file);
    let result = if let Err(error) = download_result {
        Err(error)
    } else {
        let install_path = path.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let mut options = std::fs::OpenOptions::new();
            options.read(true);
            #[cfg(windows)]
            {
                use std::os::windows::fs::OpenOptionsExt;
                options.share_mode(1);
            }
            let _locked_file = options
                .open(&install_path)
                .map_err(|e| WarpError::new("warp_install_error", e))?;
            verify_signature(&install_path)?;
            let mut command = Command::new(system_exe("msiexec.exe"));
            command.arg("/i").arg(install_path).arg("/norestart");
            // 3010 means installation succeeded and a reboot is required.
            run_with_success_codes(command, Duration::from_secs(1800), &[0, 3010])
                .map(|_| ())
                .map_err(|e| WarpError::new("warp_install_error", e.detail))
        })
        .await
        .map_err(|e| WarpError::new("warp_install_error", e))
        .and_then(|result| result)
    };
    let _ = std::fs::remove_file(path);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn download_allowlist_rejects_lookalikes_and_downgrades() {
        for url in [
            "http://downloads.cloudflareclient.com/a",
            "https://downloads.cloudflareclient.com.evil.com/a",
            "https://example.com/a",
            "https://user@downloads.cloudflareclient.com/a",
            "https://downloads.cloudflareclient.com:444/a",
        ] {
            assert!(!official_url(&url.parse().unwrap()));
        }
        assert!(official_url(
            &"https://downloads.cloudflareclient.com/v1/download/windows/ga"
                .parse()
                .unwrap()
        ));
    }
}
