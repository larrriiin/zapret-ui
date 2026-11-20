use crate::providers::warp::{
    installer::{download_and_install, InstallerFormat},
    verify_publisher, WarpError,
};
use sha2::{Digest, Sha256};
#[cfg(windows)]
use tauri::Manager;

// Stable packages and hashes published together on the official download page.
const VERSION: &str = "3.8.2.1";
const PUBLISHER: &str = "The Anti-Cloud Corporation";

fn package(arch: &str) -> Option<(&'static str, &'static str)> {
    match arch {
        "x86_64" => Some((
            "windows_x64",
            "bb46770c2be2af619aa08bc0f53f430088317af0e966192b653f91e1e8483504",
        )),
        "x86" => Some((
            "windows_x32",
            "24a985c865f763c330022afcdec0d1ec545909e33934f5ff332e9a15770ae17a",
        )),
        "aarch64" => Some((
            "windows_arm64",
            "0e6e2d4c2342d36f1102650720a223f80c46c154f5fd1b93814f05a18d9bc08f",
        )),
        _ => None,
    }
}

#[cfg(windows)]
fn windows_arch() -> &'static str {
    use windows_sys::Win32::System::{
        SystemInformation::{
            IMAGE_FILE_MACHINE_AMD64, IMAGE_FILE_MACHINE_ARM64, IMAGE_FILE_MACHINE_I386,
        },
        Threading::{GetCurrentProcess, IsWow64Process2},
    };
    let mut process = 0;
    let mut native = 0;
    // The process pseudo-handle remains valid and the output pointers refer to live u16s.
    if unsafe { IsWow64Process2(GetCurrentProcess(), &mut process, &mut native) } != 0 {
        match native {
            IMAGE_FILE_MACHINE_AMD64 => return "x86_64",
            IMAGE_FILE_MACHINE_ARM64 => return "aarch64",
            IMAGE_FILE_MACHINE_I386 => return "x86",
            _ => {}
        }
    }
    std::env::consts::ARCH
}

fn official_url(url: &reqwest::Url) -> bool {
    url.scheme() == "https"
        && url.port_or_known_default() == Some(443)
        && url.username().is_empty()
        && url.password().is_none()
        && matches!(url.host_str(), Some("wiresock.net" | "www.wiresock.net"))
}

#[tauri::command]
pub async fn install_wiresock(app: tauri::AppHandle) -> Result<(), WarpError> {
    #[cfg(not(windows))]
    {
        let _ = app;
        Err(WarpError::new("wiresock_install_platform", ""))
    }
    #[cfg(windows)]
    {
        if super::is_installed() {
            return Ok(());
        }
        let (platform, hash) = package(windows_arch())
            .ok_or_else(|| WarpError::new("wiresock_install_platform", ""))?;
        let url = format!("https://wiresock.net/_api/download-release.php?platform={platform}&product=wiresock-secure-connect&version={VERSION}");
        let log_directory = app.path().app_log_dir().ok();
        download_and_install(
            &url,
            official_url,
            InstallerFormat::Exe,
            64 * 1024 * 1024,
            move |path| {
                let data =
                    std::fs::read(path).map_err(|error| WarpError::new("warp_signature", error))?;
                if !data.starts_with(b"MZ") || crate::hex_encode(&Sha256::digest(data)) != hash {
                    return Err(WarpError::new(
                        "warp_signature",
                        "Unexpected WireSock installer format or SHA-256",
                    ));
                }
                verify_publisher(path, PUBLISHER)?;
                install_verified(path, log_directory.as_deref())
            },
        )
        .await
        .map_err(|error| {
            let code = match error.code.as_str() {
                "warp_busy" => "wiresock_install_busy",
                "warp_download" => "wiresock_install_download",
                "warp_signature" => "wiresock_install_signature",
                "warp_install_error" => "wiresock_install_error",
                code => code,
            };
            WarpError::new(code, error.detail)
        })
    }
}

#[cfg(windows)]
fn install_verified(
    path: &std::path::Path,
    log_directory: Option<&std::path::Path>,
) -> Result<(), WarpError> {
    use std::{ffi::OsString, os::windows::io::AsRawHandle};
    use windows_sys::Win32::{
        Foundation::{ERROR_CANCELLED, WAIT_OBJECT_0},
        System::Threading::{GetExitCodeProcess, WaitForSingleObject, INFINITE},
    };

    let log = log_directory.and_then(|directory| {
        std::fs::create_dir_all(directory).ok()?;
        tempfile::Builder::new()
            .prefix("wiresock-install-")
            .tempdir_in(directory)
            .ok()
    });
    let mut args = vec![OsString::from("-norestart")];
    if let Some(directory) = &log {
        args.extend([
            OsString::from("-log"),
            directory.path().join("setup.log").into_os_string(),
        ]);
    }
    // The official package is a signed Burn executable; it manages its own embedded MSIs.
    let system_directory = crate::providers::warp::system_exe("");
    let process =
        crate::elevation::spawn_elevated(path, &system_directory, &args).map_err(|error| {
            if error.raw_os_error() == Some(ERROR_CANCELLED as i32) {
                WarpError::new("wiresock_install_cancelled", "")
            } else {
                WarpError::new("wiresock_install_launch", error)
            }
        })?;
    let mut code = 0;
    // The owned process handle stays alive until the setup UI closes.
    unsafe {
        if WaitForSingleObject(process.as_raw_handle(), INFINITE) != WAIT_OBJECT_0
            || GetExitCodeProcess(process.as_raw_handle(), &mut code) == 0
        {
            return Err(WarpError::new(
                "wiresock_install_launch",
                std::io::Error::last_os_error(),
            ));
        }
    }

    match setup_error_key(code) {
        None => Ok(()),
        Some("wiresock_install_cancelled") => Err(WarpError::new("wiresock_install_cancelled", "")),
        Some(key) => {
            let mut detail = format!("WireSock Setup {code}");
            if let Some(directory) = log.filter(|dir| {
                std::fs::metadata(dir.path().join("setup.log"))
                    .is_ok_and(|metadata| metadata.len() > 0)
            }) {
                let file = directory.keep().join("setup.log");
                detail.push('\n');
                detail.push_str(&file.to_string_lossy());
            }
            Err(WarpError::new(key, detail))
        }
    }
}

fn setup_error_key(code: u32) -> Option<&'static str> {
    let code = if code >> 16 == 0x8007 {
        code & 0xffff
    } else {
        code
    };
    match code {
        0 | 3010 => None,
        1223 | 1602 => Some("wiresock_install_cancelled"),
        1618 => Some("wiresock_install_busy"),
        1639 => Some("wiresock_install_command_line"),
        _ => Some("wiresock_install_error"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinguishes_installer_failure_cancellation_and_reboot_required() {
        assert_eq!(setup_error_key(0), None);
        assert_eq!(setup_error_key(3010), None);
        assert_eq!(
            setup_error_key(0x8007_04c7),
            Some("wiresock_install_cancelled")
        );
        assert_eq!(setup_error_key(1602), Some("wiresock_install_cancelled"));
        assert_eq!(setup_error_key(0x8007_0652), Some("wiresock_install_busy"));
        assert_eq!(setup_error_key(1639), Some("wiresock_install_command_line"));
        assert_eq!(setup_error_key(0x8007_0643), Some("wiresock_install_error"));
    }

    #[test]
    fn requires_vendor_https_and_pinned_packages_for_supported_architectures() {
        assert!(official_url(
            &"https://wiresock.net/_api/download-release.php"
                .parse()
                .unwrap()
        ));
        for source in [
            "http://wiresock.net/a",
            "https://downloads.wiresock.net.example.com/a",
            "https://user@wiresock.net/a",
            "https://wiresock.net:444/a",
        ] {
            assert!(!official_url(&source.parse().unwrap()));
        }
        for arch in ["x86_64", "x86", "aarch64"] {
            let (_, hash) = package(arch).unwrap();
            assert_eq!(hash.len(), 64);
            assert!(hash.bytes().all(|byte| byte.is_ascii_hexdigit()));
        }
        assert!(package("arm").is_none());
    }
}
