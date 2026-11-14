//! Optional elevation and an explicit handoff before the single-instance lock.
use std::sync::OnceLock;

pub(crate) const ADMIN_REQUIRED: &str = "admin_required:";

pub(crate) fn require(reason: &str) -> Result<(), String> {
    check(crate::is_admin(), reason)
}

fn check(elevated: bool, reason: &str) -> Result<(), String> {
    if elevated {
        Ok(())
    } else {
        Err(format!("{ADMIN_REQUIRED}{reason}"))
    }
}

#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct RestartContext {
    section: String,
    repeat: bool,
    clear_lists: bool,
    resume_telegram: bool,
    automation: crate::automations::HandoffState,
    routing: crate::providers::warp::RoutingHandoff,
}

impl RestartContext {
    fn validate(&self) -> Result<(), String> {
        if [
            "home",
            "automations",
            "zapret-settings",
            "sites",
            "ips",
            "diagnostics",
            "traffic",
            "settings",
            "telegram",
            "warp-settings",
            "setup",
        ]
        .contains(&self.section.as_str())
        {
            Ok(())
        } else {
            Err("Invalid restart section".into())
        }
    }
}

static CONTEXT: OnceLock<RestartContext> = OnceLock::new();

/// Test write access without truncating existing data. Protected installs can
/// request elevation, while portable/user-owned installs remain usable normally.
pub(crate) fn writable(path: &std::path::Path) -> Result<(), String> {
    writable_as(crate::is_admin(), path)
}

fn writable_as(elevated: bool, path: &std::path::Path) -> Result<(), String> {
    use std::{
        fs::OpenOptions,
        time::{SystemTime, UNIX_EPOCH},
    };
    if elevated {
        return Ok(());
    }
    let result = if path.is_file() {
        OpenOptions::new().write(true).open(path).map(drop)
    } else {
        let mut directory = path;
        while !directory.is_dir() {
            let Some(parent) = directory.parent() else {
                return Ok(());
            };
            directory = parent;
        }
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos();
        let probe = directory.join(format!(
            ".zapret-write-probe-{}-{stamp}",
            std::process::id()
        ));
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&probe)
            .and_then(|file| {
                drop(file);
                std::fs::remove_file(probe)
            })
    };
    match result {
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            Err(format!("{ADMIN_REQUIRED}files"))
        }
        // Let the operation report other I/O failures with its existing context.
        _ => Ok(()),
    }
}

#[tauri::command]
pub(crate) fn request_admin_from_tray(app: tauri::AppHandle, reason: String) -> Result<(), String> {
    use tauri::{Emitter, Manager};
    if ![
        "service",
        "traffic",
        "tests",
        "hosts",
        "core",
        "files",
        "warp_apps",
    ]
    .contains(&reason.as_str())
    {
        return Err("Invalid privilege reason".into());
    }
    if let Some(tray) = app.get_webview_window("tray-menu") {
        let _ = tray.hide();
    }
    let main = app
        .get_webview_window("main")
        .ok_or("Main window missing")?;
    main.show().map_err(|e| e.to_string())?;
    main.set_focus().map_err(|e| e.to_string())?;
    app.emit_to("main", "admin-required-action", reason)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn elevation_context() -> Option<RestartContext> {
    CONTEXT.get().cloned()
}

pub(crate) fn resume_telegram() -> bool {
    CONTEXT.get().is_some_and(|context| context.resume_telegram)
}

pub(crate) fn automation_handoff() -> crate::automations::HandoffState {
    CONTEXT
        .get()
        .map(|context| context.automation.clone())
        .unwrap_or_default()
}

pub(crate) fn resume_routing() -> Result<(), String> {
    let Some(context) = CONTEXT.get() else {
        return Ok(());
    };
    tauri::async_runtime::block_on(crate::providers::warp::resume_handoff(
        context.routing.clone(),
    ))
    .map_err(|error| format!("{}: {}", error.code, error.detail))
}

#[cfg(windows)]
mod handoff {
    use super::*;
    use std::{
        ffi::OsString,
        io,
        os::windows::ffi::OsStrExt,
        time::{SystemTime, UNIX_EPOCH},
    };
    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0},
        System::Threading::{
            CreateEventW, OpenEventW, OpenProcess, QueryFullProcessImageNameW, SetEvent,
            WaitForSingleObject, EVENT_MODIFY_STATE, PROCESS_QUERY_LIMITED_INFORMATION,
            PROCESS_SYNCHRONIZE,
        },
    };

    struct Handle(HANDLE);
    impl Drop for Handle {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }
    fn event_prefix(pid: u32) -> String {
        format!("Local\\ZAPRET-UI-elevation-{pid}-")
    }

    pub(super) fn launch(context: &RestartContext) -> io::Result<()> {
        launch_with(context, crate::elevation::request_restart, 30_000)
    }

    fn launch_with(
        context: &RestartContext,
        launch_child: impl FnOnce(&[OsString]) -> io::Result<bool>,
        timeout: u32,
    ) -> io::Result<()> {
        let pid = std::process::id();
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        let name = format!("{}{stamp}", event_prefix(pid));
        let event = unsafe { CreateEventW(std::ptr::null(), 1, 0, wide(&name).as_ptr()) };
        if event.is_null() {
            return Err(io::Error::last_os_error());
        }
        let event = Handle(event);
        let args = [
            OsString::from(format!("--elevation-parent={pid}")),
            OsString::from(format!("--elevation-ready={name}")),
            OsString::from(format!(
                "--elevation-context={}",
                serde_json::to_string(context)?
            )),
        ];
        if !launch_child(&args)? {
            return Err(io::Error::other("Already elevated"));
        }
        // Keep the old application alive if the child cannot even start its handoff.
        if unsafe { WaitForSingleObject(event.0, timeout) } != WAIT_OBJECT_0 {
            return Err(io::Error::other(
                "Elevated process did not acknowledge startup",
            ));
        }
        Ok(())
    }

    pub(super) fn accept(args: &[String]) -> Result<(), String> {
        let Some(parent) = args
            .iter()
            .find_map(|a| a.strip_prefix("--elevation-parent="))
        else {
            return Ok(());
        };
        if !crate::is_admin() {
            return Err("elevation_not_granted".into());
        }
        let pid: u32 = parent.parse().map_err(|_| "Invalid elevation parent")?;
        if pid == 0 || pid == std::process::id() {
            return Err("Invalid elevation parent".into());
        }
        let name = args
            .iter()
            .find_map(|a| a.strip_prefix("--elevation-ready="))
            .ok_or("Missing handoff event")?;
        if !name.starts_with(&event_prefix(pid)) || name.len() > 150 {
            return Err("Invalid handoff event".into());
        }
        let context: RestartContext = serde_json::from_str(
            args.iter()
                .find_map(|a| a.strip_prefix("--elevation-context="))
                .ok_or("Missing restart context")?,
        )
        .map_err(|e| e.to_string())?;
        context.validate()?;
        unsafe {
            let parent = OpenProcess(
                PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
                0,
                pid,
            );
            if parent.is_null() {
                return Err(io::Error::last_os_error().to_string());
            }
            let parent = Handle(parent);
            let mut image = vec![0u16; 32768];
            let mut length = image.len() as u32;
            if QueryFullProcessImageNameW(parent.0, 0, image.as_mut_ptr(), &mut length) == 0 {
                return Err(io::Error::last_os_error().to_string());
            }
            let own: Vec<u16> = std::env::current_exe()
                .map_err(|e| e.to_string())?
                .as_os_str()
                .encode_wide()
                .collect();
            if image[..length as usize] != own {
                return Err("Unexpected elevation parent executable".into());
            }
            let event = OpenEventW(EVENT_MODIFY_STATE, 0, wide(name).as_ptr());
            if event.is_null() {
                return Err(io::Error::last_os_error().to_string());
            }
            let event = Handle(event);
            if SetEvent(event.0) == 0 {
                return Err(io::Error::last_os_error().to_string());
            }
            // The parent releases proxies, children and the single-instance lock first.
            if WaitForSingleObject(parent.0, 60_000) != WAIT_OBJECT_0 {
                return Err("Elevation handoff timed out".into());
            }
        }
        CONTEXT
            .set(context)
            .map_err(|_| "Duplicate elevation handoff".to_string())
    }
    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn startup_without_handoff_arguments_is_accepted() {
            assert!(accept(&["--autostart".into()]).is_ok());
        }

        #[test]
        fn handoff_requires_child_ack_and_preserves_cancellation_errors() {
            let context = RestartContext {
                section: "traffic".into(),
                ..Default::default()
            };
            assert!(launch_with(&context, |_| Ok(true), 1).is_err());
            let error =
                launch_with(&context, |_| Err(io::Error::from_raw_os_error(1223)), 1).unwrap_err();
            assert_eq!(error.raw_os_error(), Some(1223));
            assert!(launch_with(
                &context,
                |args| {
                    assert!(args.iter().all(|a| a != "--autostart"));
                    let name = args
                        .iter()
                        .find_map(|a| a.to_str()?.strip_prefix("--elevation-ready="))
                        .unwrap()
                        .to_owned();
                    std::thread::spawn(move || unsafe {
                        let event = OpenEventW(EVENT_MODIFY_STATE, 0, wide(&name).as_ptr());
                        assert!(!event.is_null());
                        let event = Handle(event);
                        assert_ne!(SetEvent(event.0), 0);
                    });
                    Ok(true)
                },
                5_000
            )
            .is_ok());
        }
    }
}

pub(crate) fn accept_handoff() -> Result<(), String> {
    #[cfg(windows)]
    handoff::accept(&std::env::args().collect::<Vec<_>>())?;
    Ok(())
}

#[tauri::command]
pub(crate) async fn restart_as_admin(
    app: tauri::AppHandle,
    mut context: RestartContext,
) -> Result<(), String> {
    use tauri::Manager;
    context.validate()?;
    if crate::is_admin() {
        return Ok(());
    }
    if crate::strategy_test::is_running() {
        return Err("elevation_operation_busy".into());
    }
    let _operation =
        crate::CoreOperationGuard::acquire().map_err(|_| "elevation_operation_busy")?;
    context.resume_telegram =
        crate::telegram::get_telegram_status(app.clone(), app.state())?.running;
    context.automation = crate::automations::capture_handoff(app.state());
    context.routing = crate::providers::warp::capture_handoff()
        .map_err(|e| format!("{}: {}", e.code, e.detail))?;
    #[cfg(windows)]
    tauri::async_runtime::spawn_blocking(move || handoff::launch(&context))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| {
            if e.raw_os_error() == Some(1223) {
                "elevation_cancelled".into()
            } else {
                format!("elevation_failed: {e}")
            }
        })?;
    // No shutdown or proxy cleanup happens until Windows and the child confirm success.
    crate::graceful_exit(&app);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ordinary_mode_returns_a_recoverable_permission_error() {
        assert_eq!(
            check(false, "service"),
            Err("admin_required:service".into())
        );
        assert!(check(true, "service").is_ok());
    }

    #[test]
    fn permission_probe_never_truncates_files_or_leaves_probe_files() {
        let directory =
            std::env::temp_dir().join(format!("zapret-permission-probe-{}", std::process::id()));
        std::fs::create_dir(&directory).unwrap();
        let file = directory.join("settings.txt");
        std::fs::write(&file, "saved settings").unwrap();
        writable_as(false, &file).unwrap();
        writable_as(false, &directory.join("nested/new.txt")).unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "saved settings");
        assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 1);
        std::fs::remove_file(file).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }
    #[test]
    fn restart_context_cannot_inject_arbitrary_navigation_or_arguments() {
        for section in ["home", "setup", "diagnostics"] {
            assert!(RestartContext {
                section: section.into(),
                ..Default::default()
            }
            .validate()
            .is_ok());
        }
        assert!(RestartContext {
            section: "--autostart".into(),
            ..Default::default()
        }
        .validate()
        .is_err());
        assert!(serde_json::from_str::<RestartContext>(
            r#"{"section":"home","command":"cmd.exe"}"#
        )
        .is_err());
    }
}
