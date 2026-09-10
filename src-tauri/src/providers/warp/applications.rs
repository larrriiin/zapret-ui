use super::{Result, WarpError};
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::{atomic::Ordering, Mutex},
};

#[cfg(windows)]
#[path = "tcp.rs"]
mod tcp;
static APPLICATIONS: Mutex<Option<Runtime>> = Mutex::new(None);

#[derive(Clone, Debug, Default, Serialize)]
pub struct ApplicationStatus {
    pub paths: Vec<String>,
    pub enabled: bool,
    #[serde(skip)]
    pub(crate) runtime_active: bool,
    pub active_connections: usize,
    pub total_connections: u64,
    pub failed_connections: u64,
    pub error: Option<String>,
    #[serde(skip)]
    pub(crate) connections: Vec<super::WarpConnection>,
}
#[derive(Default, Serialize, Deserialize)]
struct Preferences {
    paths: Vec<String>,
    #[serde(default)]
    enabled: bool,
}
struct Runtime {
    directory: PathBuf,
    preferences: Preferences,
    #[cfg(windows)]
    engine: Option<tcp::Engine>,
    error: Option<String>,
    stopping: bool,
}
fn error(e: impl ToString) -> WarpError {
    WarpError::new("warp_apps_error", e)
}
pub fn initialize(directory: PathBuf) -> Result<()> {
    std::fs::create_dir_all(&directory).map_err(error)?;
    let path = directory.join("warp-applications.json");
    let preferences = if path.exists() {
        serde_json::from_slice(&std::fs::read(path).map_err(error)?).map_err(error)?
    } else {
        Preferences::default()
    };
    *APPLICATIONS.lock().map_err(error)? = Some(Runtime {
        directory,
        preferences,
        #[cfg(windows)]
        engine: None,
        error: None,
        stopping: false,
    });
    Ok(())
}
pub fn status() -> Result<ApplicationStatus> {
    let mut guard = APPLICATIONS.lock().map_err(error)?;
    let Some(runtime) = guard.as_mut() else {
        return Ok(ApplicationStatus::default());
    };
    let mut status = ApplicationStatus {
        paths: runtime.preferences.paths.clone(),
        enabled: runtime.preferences.enabled,
        error: runtime.error.clone(),
        ..Default::default()
    };
    #[cfg(windows)]
    if let Some(engine) = &runtime.engine {
        let running = engine.running();
        status.runtime_active = running;
        status.active_connections = engine.shared.active.load(Ordering::Relaxed);
        status.total_connections = engine.shared.total.load(Ordering::Relaxed);
        status.failed_connections = engine.shared.failures.load(Ordering::Relaxed);
        status.error = engine.shared.error.lock().map_err(error)?.clone();
        status.connections = engine.shared.connections();
        if !running {
            runtime.error = status.error.clone();
            runtime.engine = None;
        }
    }
    Ok(status)
}
pub fn disable() -> Result<()> {
    if let Some(runtime) = APPLICATIONS.lock().map_err(error)?.as_mut() {
        #[cfg(windows)]
        {
            runtime.engine = None;
        }
    }
    Ok(())
}
pub fn shutdown() -> Result<()> {
    if let Some(runtime) = APPLICATIONS.lock().map_err(error)?.as_mut() {
        runtime.stopping = true;
        #[cfg(windows)]
        {
            runtime.engine = None;
        }
    }
    Ok(())
}
pub fn reconcile_port(port: Option<u16>) -> Result<()> {
    #[cfg(windows)]
    if let Some(runtime) = APPLICATIONS.lock().map_err(error)?.as_mut() {
        if runtime
            .engine
            .as_ref()
            .is_some_and(|engine| Some(engine.port) != port)
        {
            runtime.engine = None;
        }
    }
    Ok(())
}
fn validate_paths(paths: Vec<String>) -> Result<Vec<String>> {
    if paths.len() > 100 {
        return Err(WarpError::new(
            "warp_apps_invalid",
            "Maximum: 100 applications",
        ));
    }
    let mut result: Vec<String> = Vec::new();
    for path in paths {
        let path = PathBuf::from(path);
        if !path.is_absolute()
            || path
                .extension()
                .is_none_or(|ext| !ext.eq_ignore_ascii_case("exe"))
            || !path.is_file()
        {
            return Err(WarpError::new("warp_apps_invalid", path.display()));
        }
        let path = path.canonicalize().map_err(error)?;
        let file = path.file_name().unwrap().to_string_lossy().to_lowercase();
        // Never redirect the relay's own process or the WARP tunnel into itself.
        if path
            == std::env::current_exe()
                .map_err(error)?
                .canonicalize()
                .map_err(error)?
            || matches!(
                file.as_str(),
                "warp-svc.exe" | "warp-cli.exe" | "cloudflare warp.exe"
            )
        {
            return Err(WarpError::new(
                "warp_apps_invalid",
                "WARP and ZAPRET UI cannot be selected",
            ));
        }
        let path = path
            .to_string_lossy()
            .trim_start_matches("\\\\?\\")
            .to_string();
        if !result.iter().any(|other| other.eq_ignore_ascii_case(&path)) {
            result.push(path);
        }
    }
    Ok(result)
}
pub fn configure(paths: Vec<String>, enabled: bool, port: Option<u16>) -> Result<()> {
    let mut guard = APPLICATIONS.lock().map_err(error)?;
    let runtime = guard
        .as_mut()
        .ok_or_else(|| error("Application settings are unavailable"))?;
    if runtime.stopping {
        return Err(error("Application is exiting"));
    }
    // Disable must work even when a previously selected executable was removed.
    if !enabled {
        #[cfg(windows)]
        {
            runtime.engine = None;
        }
    }
    let paths = if !enabled && paths == runtime.preferences.paths {
        paths
    } else {
        validate_paths(paths)?
    };
    if enabled && paths.is_empty() {
        return Err(WarpError::new("warp_apps_invalid", ""));
    }
    if enabled && port.is_none() {
        return Err(WarpError::new("warp_sites_requires_proxy", ""));
    }
    #[cfg(windows)]
    if enabled && runtime.engine.is_some() {
        return Err(WarpError::new("warp_apps_stop_first", ""));
    }
    let preferences = Preferences { paths, enabled };
    super::sites::persist_json(
        &runtime.directory.join("warp-applications.json"),
        &preferences,
    )?;
    runtime.preferences = preferences;
    runtime.error = None;
    #[cfg(windows)]
    if enabled {
        let dll = crate::find_binaries_dir().join("bin/WinDivert.dll");
        runtime.engine = Some(
            tcp::Engine::start(runtime.preferences.paths.clone(), port.unwrap(), &dll).map_err(
                |e| {
                    runtime.error = Some(e.to_string());
                    if e.raw_os_error() == Some(5) {
                        WarpError::new("warp_apps_admin", "")
                    } else {
                        error(e)
                    }
                },
            )?,
        );
    }
    #[cfg(not(windows))]
    if enabled {
        return Err(error("Windows only"));
    }
    Ok(())
}

#[tauri::command]
pub async fn choose_warp_applications() -> Result<Vec<String>> {
    let files = rfd::AsyncFileDialog::new()
        .add_filter("Windows application", &["exe"])
        .pick_files()
        .await;
    match files {
        Some(files) => validate_paths(
            files
                .into_iter()
                .map(|file| file.path().to_string_lossy().to_string())
                .collect(),
        ),
        None => Ok(vec![]),
    }
}
