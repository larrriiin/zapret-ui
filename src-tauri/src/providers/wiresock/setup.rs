use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    io::Write,
    path::{Path, PathBuf},
    sync::Mutex,
    time::Duration,
};
use tauri::Manager;

// https://github.com/Flowseal/zapret-discord-youtube/discussions/16450
const DOMAINS: &str = "wiresock.net\ncdn.paddle.com\ncdn.jsdelivr.net\ncdn.skypack.dev\ngenerator-config-warp.vercel.app\nwarp5.llimonix.pw\nmtrcs.llimonix.dev\nupload.wikimedia.org\nllimonix.pw\nworkers.dev\nvercel.app";
const ADDRESSES: &str = "95.179.146.125/32\n15.197.167.90/32\n3.33.186.135/32\n104.16.179.112/32\n104.16.180.112/32\n8.6.112.5/32\n8.47.69.5/32\n216.198.79.131/32\n64.29.17.131/32\n187.52.45.33/32\n188.114.96.1/32\n188.114.97.1/32\n185.15.59.240/32\n185.199.111.153/32\n185.199.108.153/32\n185.199.109.153/32\n185.199.110.153/32";
const HOSTS: &str = "95.179.146.125 wiresock.net\n187.52.45.33 warp6.llimonix.pw\n64.29.17.3 generator-config-warp.vercel.app\n216.198.79.131 generator-config-warp.vercel.app";
const READ_ERROR: &str = "wiresock_setup_read";
const WRITE_ERROR: &str = "wiresock_setup_write";
const RESTART_ERROR: &str = "wiresock_zapret_restart_failed";
const STRATEGY_ERROR: &str = "wiresock_zapret_strategy_required";
static PENDING_RESTART: Mutex<Option<(String, String)>> = Mutex::new(None);

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SetupRequest {
    profile: String,
    generated: bool,
    strategy: String,
}

impl SetupRequest {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if !super::valid_profile_name(&self.profile)
            || (self.generated && !super::is_generated(&self.profile))
        {
            return Err("wiresock_profile_missing".into());
        }
        if !self.strategy.is_empty() && !crate::is_safe_strategy_name(&self.strategy) {
            return Err(STRATEGY_ERROR.into());
        }
        Ok(())
    }
}

struct Files {
    domains: PathBuf,
    ipset: PathBuf,
    upstream_ipset: PathBuf,
    hosts: PathBuf,
}

impl Files {
    fn locate() -> Result<Self, String> {
        let system = std::env::var_os("SystemRoot").ok_or(READ_ERROR)?;
        Ok(Self {
            domains: crate::resolve_list_path("list-general-user.txt").map_err(|_| READ_ERROR)?,
            ipset: crate::resolve_core_list_path(crate::ipset::USER_INCLUDE_FILE)
                .map_err(|_| READ_ERROR)?,
            upstream_ipset: crate::resolve_core_list_path("ipset-all.txt")
                .map_err(|_| READ_ERROR)?,
            hosts: PathBuf::from(system).join("System32/drivers/etc/hosts"),
        })
    }
}

#[derive(Serialize)]
pub struct Setup {
    domains: usize,
    ipset: usize,
    hosts: usize,
    ipset_mode: &'static str,
    restart_pending: bool,
}

#[derive(Serialize)]
pub(crate) struct AppliedSetup {
    status: Option<super::WireSockStatus>,
    connection_error: Option<&'static str>,
}

fn read_list(path: &Path) -> Result<String, String> {
    match std::fs::read_to_string(path) {
        Ok(content) => Ok(content),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(_) => Err(READ_ERROR.into()),
    }
}

fn missing(text: &str, expected: &str, addresses: bool) -> Vec<String> {
    let current: HashSet<_> = text
        .trim_start_matches('\u{feff}')
        .lines()
        .map(|line| line.split('#').next().unwrap_or("").trim())
        .map(|line| {
            if addresses {
                line.strip_suffix("/32").unwrap_or(line).to_owned()
            } else {
                line.trim_end_matches('.').to_ascii_lowercase()
            }
        })
        .collect();
    expected
        .lines()
        .filter(|entry| {
            !current.contains(if addresses {
                entry.strip_suffix("/32").unwrap_or(entry)
            } else {
                entry
            })
        })
        .map(str::to_owned)
        .collect()
}

fn inspect(files: &Files) -> Result<(String, String, Setup), String> {
    let domains = read_list(&files.domains)?;
    let ipset = read_list(&files.ipset)?;
    let upstream_ipset = read_list(&files.upstream_ipset)?;
    let hosts = std::fs::read(&files.hosts).map_err(|_| READ_ERROR)?;
    let (_, hosts) = crate::hosts::additions(&hosts, HOSTS).map_err(|_| READ_ERROR)?;
    let setup = Setup {
        domains: missing(&domains, DOMAINS, false).len(),
        ipset: missing(&ipset, ADDRESSES, true).len(),
        hosts,
        ipset_mode: crate::ipset_mode(upstream_ipset.trim_start_matches('\u{feff}')),
        restart_pending: PENDING_RESTART
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some(),
    };
    Ok((domains, ipset, setup))
}

#[tauri::command]
pub async fn get_wiresock_setup() -> Result<Setup, String> {
    tauri::async_runtime::spawn_blocking(|| inspect(&Files::locate()?).map(|(_, _, setup)| setup))
        .await
        .map_err(|_| READ_ERROR.to_string())?
}

fn with_additions(mut current: String, entries: Vec<String>) -> String {
    if !entries.is_empty() {
        if !current.is_empty() && !current.ends_with('\n') {
            current.push_str("\r\n");
        }
        for entry in entries {
            current.push_str(&entry);
            current.push_str("\r\n");
        }
    }
    current
}

fn stage(path: &Path, text: &str) -> Result<tempfile::NamedTempFile, String> {
    let mut temp = tempfile::NamedTempFile::new_in(path.parent().ok_or(WRITE_ERROR)?)
        .map_err(|_| WRITE_ERROR)?;
    temp.write_all(text.as_bytes())
        .and_then(|_| temp.as_file().sync_all())
        .map_err(|_| WRITE_ERROR)?;
    Ok(temp)
}

pub(crate) fn preserve_legacy_ipset(source: &Path, destination: &Path) -> Result<(), String> {
    let previous = read_list(&source.join("ipset-all.txt"))?;
    if missing(&previous, ADDRESSES, true).is_empty() {
        let path = destination.join(crate::ipset::USER_INCLUDE_FILE);
        let current = read_list(&path)?;
        let entries = missing(&current, ADDRESSES, true);
        if !entries.is_empty() {
            stage(&path, &with_additions(current, entries))?
                .persist(&path)
                .map_err(|_| WRITE_ERROR)?;
        }
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn apply_wiresock_setup(
    app: tauri::AppHandle,
    confirmed: bool,
    request: SetupRequest,
) -> Result<AppliedSetup, String> {
    if !confirmed {
        return Err("wiresock_setup_confirmation".into());
    }
    apply_restart_connect(app, request, None).await
}

#[tauri::command]
pub(crate) async fn resume_wiresock_setup(
    app: tauri::AppHandle,
) -> Result<Option<AppliedSetup>, String> {
    let Some((request, temporary_strategy)) = crate::privileges::take_wiresock_setup() else {
        return Ok(None);
    };
    apply_restart_connect(app, request, temporary_strategy)
        .await
        .map(Some)
}

async fn apply_restart_connect(
    app: tauri::AppHandle,
    request: SetupRequest,
    temporary_strategy: Option<String>,
) -> Result<AppliedSetup, String> {
    request.validate()?;
    let setup_app = app.clone();
    let selected_strategy = request.strategy;
    tauri::async_runtime::spawn_blocking(move || {
        let _operation = crate::CoreOperationGuard::acquire()?;
        let files = Files::locate()?;
        let (domains, ipset, setup) = inspect(&files)?;
        crate::privileges::require(if setup.hosts != 0 { "hosts" } else { "service" })?;

        let app_state = setup_app.state::<crate::AppState>();
        let before = crate::read_zapret_status(app_state.inner());
        let was_running = before.running;
        let pending = PENDING_RESTART
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let (strategy, mode) = crate::restart_context(before, "apply WireSock setup")
            .map_err(|_| STRATEGY_ERROR)?
            .or(pending)
            .or_else(|| temporary_strategy.map(|name| (name, "temporary".into())))
            .unwrap_or((selected_strategy, "service".into()));
        if !crate::is_safe_strategy_name(&strategy)
            || crate::parse_strategy_args(&strategy).is_err()
        {
            return Err(STRATEGY_ERROR.into());
        }

        let domains = if setup.domains != 0 {
            let added = missing(&domains, DOMAINS, false);
            Some(stage(&files.domains, &with_additions(domains, added))?)
        } else {
            None
        };
        let ipset = if setup.ipset != 0 {
            let added = missing(&ipset, ADDRESSES, true);
            Some(stage(&files.ipset, &with_additions(ipset, added))?)
        } else {
            None
        };
        if setup.hosts != 0 {
            crate::hosts::update_file(&files.hosts, HOSTS).map_err(|error| {
                match error.as_str() {
                    "backup_failed" | "restore_failed" => error,
                    _ => WRITE_ERROR.into(),
                }
            })?;
        }
        if let Some(domains) = domains {
            domains.persist(&files.domains).map_err(|_| WRITE_ERROR)?;
        }
        if let Some(ipset) = ipset {
            ipset.persist(&files.ipset).map_err(|_| WRITE_ERROR)?;
        }
        if setup.ipset_mode != "loaded" {
            crate::set_ipset_filter("loaded".into()).map_err(|_| WRITE_ERROR)?;
        }
        let (_, _, applied) = inspect(&files)?;
        if applied.domains != 0
            || applied.ipset != 0
            || applied.hosts != 0
            || applied.ipset_mode != "loaded"
        {
            return Err(WRITE_ERROR.into());
        }
        *PENDING_RESTART.lock().unwrap_or_else(|e| e.into_inner()) =
            Some((strategy.clone(), mode.clone()));
        restart_zapret(&setup_app, app_state, was_running, strategy, mode)?;
        *PENDING_RESTART.lock().unwrap_or_else(|e| e.into_inner()) = None;
        Ok::<_, String>(())
    })
    .await
    .map_err(|_| WRITE_ERROR.to_string())??;
    Ok(
        match super::connect_wiresock(app, request.profile, request.generated).await {
            Ok(status) => AppliedSetup {
                status: Some(status),
                connection_error: None,
            },
            Err(error) => AppliedSetup {
                status: None,
                connection_error: Some(error),
            },
        },
    )
}

fn restart_zapret(
    app: &tauri::AppHandle,
    state: tauri::State<'_, crate::AppState>,
    was_running: bool,
    strategy: String,
    mode: String,
) -> Result<(), String> {
    let service = mode == "service";
    if was_running {
        crate::stop_zapret(state.clone()).map_err(|error| {
            eprintln!("Could not stop Zapret for WireSock setup: {error}");
            RESTART_ERROR
        })?;
    }
    crate::start_zapret(app.clone(), strategy, mode, state.clone()).map_err(|error| {
        eprintln!("Could not start Zapret for WireSock setup: {error}");
        RESTART_ERROR
    })?;

    for _ in 0..20 {
        std::thread::sleep(Duration::from_millis(250));
        if service {
            #[cfg(windows)]
            if matches!(
                crate::windows_services::query_state("zapret"),
                Ok(Some(crate::windows_services::ServiceState::Running))
            ) {
                return Ok(());
            }
        } else {
            let mut child = state
                .temp_process_child
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            return if child
                .as_mut()
                .is_some_and(|process| matches!(process.try_wait(), Ok(None)))
            {
                Ok(())
            } else {
                Err(RESTART_ERROR.into())
            };
        }
    }
    Err(RESTART_ERROR.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_list_contents_and_does_not_duplicate_equivalent_entries() {
        assert!(missing("\u{feff}WireSock.NET. # keep\n", "wiresock.net", false).is_empty());
        assert!(missing("95.179.146.125 # keep", "95.179.146.125/32", true).is_empty());
        assert_eq!(
            with_additions("# keep".into(), vec!["example.com".into()]),
            "# keep\r\nexample.com\r\n"
        );
    }

    #[test]
    fn copies_the_legacy_wiresock_entries_before_an_upstream_refresh() {
        let lists = tempfile::tempdir().unwrap();
        let upstream = lists.path().join("ipset-all.txt");
        let user = lists.path().join(crate::ipset::USER_INCLUDE_FILE);
        let content = format!("198.51.100.0/24\n{ADDRESSES}\n");
        std::fs::write(&upstream, &content).unwrap();
        std::fs::write(&user, "192.0.2.0/24\n95.179.146.125\n").unwrap();
        preserve_legacy_ipset(lists.path(), lists.path()).unwrap();
        assert_eq!(std::fs::read_to_string(&upstream).unwrap(), content);
        crate::ipset::install_if_changed(&upstream, "198.51.100.0/24\n").unwrap();
        let saved = std::fs::read_to_string(&user).unwrap();
        assert!(missing(&saved, ADDRESSES, true).is_empty());
        assert!(saved.contains("192.0.2.0/24"));
        assert!(!saved.contains("198.51.100.0/24"));
        assert_eq!(saved.matches("95.179.146.125").count(), 1);
    }
}
