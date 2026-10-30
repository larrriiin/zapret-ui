//! Application rules run outside the webview, including while minimized.
use super::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    #[default]
    Keep,
    On,
    Off,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Rule {
    id: String,
    name: String,
    #[serde(default)]
    icon: Option<String>,
    path: String,
    enabled: bool,
    zapret: Action,
    strategy: Option<String>,
    warp: Action,
    warp_mode: Option<String>,
    #[serde(default)]
    warp_routes: Option<providers::warp::ProxySelection>,
    restore: bool,
    #[serde(default)]
    close_off: bool,
}
#[derive(Clone, Default, Deserialize, Serialize)]
pub struct Config {
    enabled: bool,
    rules: Vec<Rule>,
}
#[derive(Clone, Default, Serialize)]
pub struct Status {
    config: Config,
    active: Vec<String>,
    error: Option<String>,
    paused: bool,
}
#[derive(Default)]
pub struct Store(Mutex<Status>);

fn config_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("application-rules.json"))
}
fn validate(config: &Config) -> Result<(), String> {
    if config.rules.len() > 100 {
        return Err("Too many rules (maximum 100)".into());
    }
    let mut ids = HashSet::new();
    for r in &config.rules {
        if r.id.is_empty() || !ids.insert(&r.id) || r.name.trim().is_empty() || r.name.len() > 160 {
            return Err("Invalid rule name or ID".into());
        }
        if r.icon.as_ref().is_some_and(|icon| {
            ![
                "apps",
                "sports_esports",
                "forum",
                "public",
                "music_note",
                "movie",
                "work",
                "code",
                "school",
                "cloud",
                "shield",
                "bolt",
            ]
            .contains(&icon.as_str())
        }) {
            return Err("Invalid rule icon".into());
        }
        let p = std::path::Path::new(&r.path);
        if !p.is_absolute() || !r.path.to_ascii_lowercase().ends_with(".exe") {
            return Err("Choose an absolute .exe path".into());
        }
        if let Some(s) = &r.strategy {
            if !is_safe_strategy_name(s) {
                return Err("Invalid strategy".into());
            }
        }
        if let Some(m) = &r.warp_mode {
            if ![
                "doh",
                "dot",
                "warp",
                "warp+dot",
                "warp+doh",
                "proxy",
                "tunnel_only",
            ]
            .contains(&m.as_str())
            {
                return Err("Invalid WARP mode".into());
            }
        }
        if let Some(routes) = &r.warp_routes {
            routes.validate().map_err(warp_error)?;
            if r.warp_mode.as_deref() != Some("proxy") {
                return Err("Proxy selection requires Local proxy mode".into());
            }
        }
        if r.zapret == Action::Keep
            && r.strategy.is_none()
            && r.warp == Action::Keep
            && r.warp_mode.is_none()
        {
            return Err("Choose at least one action".into());
        }
    }
    Ok(())
}
#[tauri::command]
pub fn get_application_rules(store: State<'_, Store>) -> Status {
    store.0.lock_unpoisoned().clone()
}
#[tauri::command]
pub fn save_application_rules(
    app: tauri::AppHandle,
    config: Config,
    store: State<'_, Store>,
) -> Result<(), String> {
    validate(&config)?;
    let mut status = store.0.lock_unpoisoned();
    let path = config_path(&app)?;
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("tmp");
    std::fs::write(
        &tmp,
        serde_json::to_vec_pretty(&config).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
    status.config = config;
    status.error = None;
    Ok(())
}
#[tauri::command]
pub async fn choose_rule_application() -> Option<String> {
    rfd::AsyncFileDialog::new()
        .add_filter("Windows application", &["exe"])
        .pick_file()
        .await
        .map(|f| f.path().to_string_lossy().into_owned())
}

fn path_key(path: &str) -> String {
    path.replace('/', "\\").to_lowercase()
}
#[cfg(windows)]
fn running_paths() -> Result<HashSet<String>, String> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
        System::{
            Diagnostics::ToolHelp::*,
            Threading::{
                OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
            },
        },
    };
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut paths = HashSet::new();
        let mut found = Process32FirstW(snapshot, &mut entry);
        while found != 0 {
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, entry.th32ProcessID);
            if !process.is_null() {
                let mut buf = vec![0u16; 32768];
                let mut len = buf.len() as u32;
                if QueryFullProcessImageNameW(process, 0, buf.as_mut_ptr(), &mut len) != 0 {
                    paths.insert(path_key(&String::from_utf16_lossy(&buf[..len as usize])));
                }
                CloseHandle(process);
            }
            found = Process32NextW(snapshot, &mut entry);
        }
        CloseHandle(snapshot);
        Ok(paths)
    }
}
#[cfg(not(windows))]
fn running_paths() -> Result<HashSet<String>, String> {
    Ok(HashSet::new())
}

#[derive(Clone, Debug, PartialEq)]
struct Snapshot {
    on: bool,
    choice: Option<String>,
    launch: Option<String>,
    routes: Option<providers::warp::ProxySelection>,
}
fn read(app: &tauri::AppHandle, warp: bool) -> Result<Snapshot, String> {
    if warp {
        let s = tauri::async_runtime::block_on(providers::warp::get_warp_status())
            .map_err(warp_error)?;
        if let Some(e) = s.error {
            return Err(warp_error(e));
        }
        if !s.installed {
            return Err("WARP is not installed".into());
        }
        Ok(Snapshot {
            on: s.connected,
            choice: s.mode,
            launch: None,
            routes: s.application_routes,
        })
    } else {
        let s = get_zapret_status(app.state::<AppState>());
        if s.running && (s.strategy.is_none() || s.mode.is_none()) {
            return Err("Cannot automate Zapret: current strategy is unknown".into());
        }
        Ok(Snapshot {
            on: s.running,
            choice: s.strategy,
            launch: s.mode,
            routes: None,
        })
    }
}
fn warp_error(e: providers::warp::WarpError) -> String {
    if e.detail.is_empty() {
        e.code
    } else {
        format!("{}: {}", e.code, e.detail)
    }
}
fn write(app: &tauri::AppHandle, warp: bool, target: &Snapshot) -> Result<(), String> {
    if warp {
        let status = tauri::async_runtime::block_on(providers::warp::apply_application_connection(
            target.choice.clone(),
            target.on,
            target.routes.clone(),
        ))
        .map_err(warp_error)?;
        if let Some(error) = status.error {
            return Err(warp_error(error));
        }
        let until = Instant::now() + Duration::from_secs(20);
        loop {
            let observed = read(app, true)?;
            if observed.on == target.on
                && (target.choice.is_none() || observed.choice == target.choice)
            {
                break;
            }
            if Instant::now() >= until {
                return Err("WARP did not reach the requested state".into());
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    } else if target.on {
        let strategy = target.choice.clone().ok_or("Choose a Zapret strategy")?;
        start_zapret(
            app.clone(),
            strategy,
            target.launch.clone().unwrap_or("service".into()),
            app.state::<AppState>(),
        )?;
    } else {
        stop_zapret(app.state::<AppState>())?;
    }
    refresh_tray_menu(app);
    Ok(())
}

#[derive(Default)]
struct Slot {
    baseline: Option<Snapshot>,
    applied: Option<Snapshot>,
    rule: Option<Rule>,
    paused: bool,
}
impl Slot {
    fn reconcile(
        &mut self,
        app: &tauri::AppHandle,
        warp: bool,
        rule: Option<&Rule>,
    ) -> Result<(), String> {
        self.transition(
            warp,
            rule,
            || read(app, warp),
            |target| write(app, warp, target),
        )
    }
    fn transition(
        &mut self,
        warp: bool,
        rule: Option<&Rule>,
        mut read_state: impl FnMut() -> Result<Snapshot, String>,
        mut write_state: impl FnMut(&Snapshot) -> Result<(), String>,
    ) -> Result<(), String> {
        if self.rule.is_none() && rule.is_none() {
            self.paused = false;
            return Ok(());
        }
        let current = read_state()?;
        if self.applied.as_ref().is_some_and(|s| *s != current) {
            self.paused = true;
        }
        if self.paused {
            if rule.is_none() {
                *self = Self::default();
            }
            return Ok(());
        }
        let unchanged = self.rule.as_ref().zip(rule).is_some_and(|(a, b)| {
            a.id == b.id
                && a.zapret == b.zapret
                && a.strategy == b.strategy
                && a.warp == b.warp
                && a.warp_mode == b.warp_mode
                && a.warp_routes == b.warp_routes
                && a.restore == b.restore
                && a.close_off == b.close_off
        });
        if unchanged {
            return Ok(());
        }
        let baseline = self.baseline.get_or_insert_with(|| current.clone()).clone();
        let mut target = if self.rule.as_ref().is_some_and(|r| !r.restore) {
            current.clone()
        } else {
            baseline
        };
        if self.rule.as_ref().is_some_and(|r| !r.restore) {
            self.baseline = Some(current.clone());
        }
        if rule.is_none() && self.rule.as_ref().is_some_and(|r| r.close_off) {
            target = current.clone();
            target.on = false;
        }
        if let Some(r) = rule {
            let (action, choice) = if warp {
                (&r.warp, &r.warp_mode)
            } else {
                (&r.zapret, &r.strategy)
            };
            match action {
                Action::On => target.on = true,
                Action::Off => target.on = false,
                Action::Keep => {}
            }
            if choice.is_some() {
                target.choice = choice.clone();
            }
            if warp {
                target.routes = if target.choice.as_deref() == Some("proxy") {
                    r.warp_routes.clone()
                } else {
                    None
                };
            }
        }
        if !warp && !current.on && !target.on && rule.is_some_and(|r| r.strategy.is_some()) {
            self.rule = rule.cloned();
            self.paused = true;
            return Err("Zapret is stopped. Choose Enable to apply a strategy".into());
        }
        // Set the guard before mutating: failures never cause an endless reconnect loop.
        self.rule = rule.cloned();
        self.paused = true;
        if target != current || (warp && rule.is_some()) {
            write_state(&target)?;
        }
        self.applied = Some(read_state()?);
        self.paused = false;
        if rule.is_none() {
            *self = Self::default();
        }
        Ok(())
    }
}

pub fn start(app: tauri::AppHandle) {
    let loaded = (|| -> Result<Config, String> {
        let p = config_path(&app)?;
        if !p.exists() {
            return Ok(Config::default());
        }
        let config = serde_json::from_slice(&std::fs::read(p).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        validate(&config)?;
        Ok(config)
    })();
    {
        let store = app.state::<Store>();
        let mut s = store.0.lock_unpoisoned();
        match loaded {
            Ok(c) => s.config = c,
            Err(e) => s.error = Some(e),
        }
    }
    std::thread::spawn(move || {
        let mut seen = HashMap::<String, Instant>::new();
        let (mut zapret, mut warp) = (Slot::default(), Slot::default());
        loop {
            std::thread::sleep(Duration::from_secs(2));
            if EXIT_IN_PROGRESS.load(Ordering::Acquire) {
                break;
            }
            if CORE_OPERATION_ACTIVE.load(Ordering::Acquire) || strategy_test::is_running() {
                continue;
            }
            let config = app.state::<Store>().0.lock_unpoisoned().config.clone();
            if (!config.enabled || !config.rules.iter().any(|r| r.enabled))
                && zapret.rule.is_none()
                && warp.rule.is_none()
            {
                seen.clear();
                app.state::<Store>().0.lock_unpoisoned().active.clear();
                continue;
            }
            let paths = match running_paths() {
                Ok(p) => p,
                Err(e) => {
                    app.state::<Store>().0.lock_unpoisoned().error = Some(e);
                    continue;
                }
            };
            let now = Instant::now();
            for p in paths {
                seen.insert(p, now);
            }
            seen.retain(|_, time| now.duration_since(*time) < Duration::from_secs(4));
            let active: Vec<_> = config
                .rules
                .iter()
                .filter(|r| config.enabled && r.enabled && seen.contains_key(&path_key(&r.path)))
                .collect();
            let z = active
                .iter()
                .find(|r| r.zapret != Action::Keep || r.strategy.is_some())
                .copied();
            let w = active
                .iter()
                .find(|r| r.warp != Action::Keep || r.warp_mode.is_some())
                .copied();
            // Do not let an update/rollback replace the core during a rule transition.
            let _operation = match CoreOperationGuard::acquire() {
                Ok(guard) => guard,
                Err(_) => continue,
            };
            let mut errors = Vec::new();
            if let Err(e) = zapret.reconcile(&app, false, z) {
                errors.push(format!("Zapret: {e}"));
            }
            if let Err(e) = warp.reconcile(&app, true, w) {
                errors.push(format!("WARP: {e}"));
            }
            let store = app.state::<Store>();
            let mut s = store.0.lock_unpoisoned();
            s.active = active.iter().map(|r| r.id.clone()).collect();
            s.paused = zapret.paused || warp.paused;
            if !errors.is_empty() {
                s.error = Some(errors.join("\n"));
            } else if !s.paused {
                s.error = None;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    #[test]
    fn proxy_subsets_switch_between_rules_and_restore_original_routing() {
        let state = RefCell::new(snapshot(true, "proxy"));
        let mut slot = Slot::default();
        let selection = |site: &str| providers::warp::ProxySelection {
            sites: vec![site.into()],
            applications: vec![],
        };
        let a = Rule {
            id: "a".into(),
            warp: Action::On,
            warp_mode: Some("proxy".into()),
            warp_routes: Some(selection("a.example")),
            restore: true,
            ..Default::default()
        };
        let b = Rule {
            id: "b".into(),
            warp_routes: Some(selection("b.example")),
            ..a.clone()
        };
        step(&mut slot, &state, Some(&a));
        assert_eq!(state.borrow().routes, a.warp_routes);
        step(&mut slot, &state, Some(&b));
        assert_eq!(state.borrow().routes, b.warp_routes);
        step(&mut slot, &state, Some(&a));
        assert_eq!(state.borrow().routes, a.warp_routes);
        step(&mut slot, &state, None);
        assert_eq!(state.borrow().routes, None);
        assert!(state.borrow().on);
    }
    #[test]
    fn manual_route_override_changes_pause_the_application_rule() {
        let state = RefCell::new(snapshot(true, "proxy"));
        let mut slot = Slot::default();
        let rule = Rule {
            id: "a".into(),
            warp: Action::On,
            warp_mode: Some("proxy".into()),
            warp_routes: Some(providers::warp::ProxySelection {
                sites: vec!["a.example".into()],
                applications: vec![],
            }),
            restore: true,
            ..Default::default()
        };
        step(&mut slot, &state, Some(&rule));
        state.borrow_mut().routes = None;
        step(&mut slot, &state, Some(&rule));
        assert!(slot.paused);
        step(&mut slot, &state, None);
        assert!(state.borrow().routes.is_none());
    }
    #[test]
    fn already_connected_proxy_rule_still_activates_routes_once() {
        let state = RefCell::new(snapshot(true, "proxy"));
        let writes = RefCell::new(0);
        let mut slot = Slot::default();
        let rule = Rule {
            id: "proxy".into(),
            warp: Action::On,
            warp_mode: Some("proxy".into()),
            restore: true,
            ..Default::default()
        };
        for _ in 0..2 {
            slot.transition(
                true,
                Some(&rule),
                || Ok(state.borrow().clone()),
                |_| {
                    *writes.borrow_mut() += 1;
                    Ok(())
                },
            )
            .unwrap();
        }
        assert_eq!(*writes.borrow(), 1);
    }
    #[test]
    fn legacy_rules_load_without_an_icon_and_invalid_icons_are_rejected() {
        let mut value = serde_json::to_value(Rule::default()).unwrap();
        value.as_object_mut().unwrap().remove("icon");
        assert!(serde_json::from_value::<Rule>(value)
            .unwrap()
            .icon
            .is_none());
        let rule = Rule {
            id: "a".into(),
            name: "Game".into(),
            path: "C:\\Game\\game.exe".into(),
            warp: Action::On,
            icon: Some("not_an_icon".into()),
            ..Default::default()
        };
        assert!(validate(&Config {
            enabled: true,
            rules: vec![rule]
        })
        .is_err());
    }
    #[test]
    fn changing_icon_and_name_does_not_reapply_connection_actions() {
        let state = RefCell::new(snapshot(false, "warp"));
        let mut slot = Slot::default();
        let mut rule = Rule {
            id: "a".into(),
            warp: Action::On,
            restore: true,
            ..Default::default()
        };
        step(&mut slot, &state, Some(&rule));
        rule.icon = Some("forum".into());
        rule.name = "Renamed".into();
        slot.transition(
            true,
            Some(&rule),
            || Ok(state.borrow().clone()),
            |_| panic!("cosmetic edit must not write"),
        )
        .unwrap();
    }
    #[test]
    fn close_can_explicitly_turn_the_service_off() {
        let state = RefCell::new(snapshot(true, "warp"));
        let mut slot = Slot::default();
        let rule = Rule {
            warp: Action::On,
            restore: false,
            close_off: true,
            ..Default::default()
        };
        step(&mut slot, &state, Some(&rule));
        step(&mut slot, &state, None);
        assert!(!state.borrow().on);
    }
    #[cfg(windows)]
    #[test]
    fn native_process_snapshot_includes_this_executable() {
        let executable = std::env::current_exe().unwrap();
        assert!(running_paths()
            .unwrap()
            .contains(&path_key(&executable.to_string_lossy())));
    }
    fn snapshot(on: bool, choice: &str) -> Snapshot {
        Snapshot {
            on,
            choice: Some(choice.into()),
            launch: None,
            routes: None,
        }
    }
    fn step(slot: &mut Slot, state: &RefCell<Snapshot>, rule: Option<&Rule>) {
        slot.transition(
            true,
            rule,
            || Ok(state.borrow().clone()),
            |target| {
                *state.borrow_mut() = target.clone();
                Ok(())
            },
        )
        .unwrap();
    }
    #[test]
    fn overlapping_rules_restore_original_only_after_last_closes() {
        let state = RefCell::new(snapshot(false, "warp"));
        let mut slot = Slot::default();
        let a = Rule {
            id: "a".into(),
            warp: Action::On,
            warp_mode: Some("proxy".into()),
            restore: true,
            ..Default::default()
        };
        let b = Rule {
            id: "b".into(),
            warp: Action::On,
            restore: true,
            ..Default::default()
        };
        step(&mut slot, &state, Some(&a));
        assert_eq!(*state.borrow(), snapshot(true, "proxy"));
        step(&mut slot, &state, Some(&b));
        assert_eq!(*state.borrow(), snapshot(true, "warp"));
        step(&mut slot, &state, None);
        assert_eq!(*state.borrow(), snapshot(false, "warp"));
    }
    #[test]
    fn manual_changes_survive_rule_changes_and_closure() {
        let state = RefCell::new(snapshot(false, "warp"));
        let mut slot = Slot::default();
        let rule = Rule {
            warp: Action::On,
            restore: true,
            ..Default::default()
        };
        step(&mut slot, &state, Some(&rule));
        *state.borrow_mut() = snapshot(false, "doh");
        step(&mut slot, &state, Some(&rule));
        assert!(slot.paused);
        step(&mut slot, &state, None);
        assert_eq!(*state.borrow(), snapshot(false, "doh"));
        assert!(!slot.paused);
    }
    #[test]
    fn keep_changes_become_the_next_baseline() {
        let state = RefCell::new(snapshot(false, "warp"));
        let mut slot = Slot::default();
        let a = Rule {
            warp: Action::On,
            restore: false,
            ..Default::default()
        };
        let b = Rule {
            id: "b".into(),
            warp: Action::Off,
            restore: true,
            ..Default::default()
        };
        step(&mut slot, &state, Some(&a));
        step(&mut slot, &state, Some(&b));
        step(&mut slot, &state, None);
        assert!(state.borrow().on);
    }
    #[test]
    fn failed_action_is_not_retried_or_restored_over_partial_changes() {
        let state = RefCell::new(snapshot(false, "warp"));
        let mut slot = Slot::default();
        let rule = Rule {
            warp: Action::On,
            restore: true,
            ..Default::default()
        };
        assert!(slot
            .transition(
                true,
                Some(&rule),
                || Ok(state.borrow().clone()),
                |target| {
                    *state.borrow_mut() = target.clone();
                    Err("failed".into())
                }
            )
            .is_err());
        assert!(slot.paused);
        slot.transition(
            true,
            Some(&rule),
            || Ok(state.borrow().clone()),
            |_| panic!("must not retry"),
        )
        .unwrap();
        step(&mut slot, &state, None);
        assert!(state.borrow().on);
    }
    #[test]
    fn matches_full_paths_case_insensitively() {
        assert_eq!(
            path_key("C:/Apps/Discord.exe"),
            path_key("c:\\apps\\DISCORD.EXE")
        );
        assert_ne!(path_key("C:/Apps/game.exe"), path_key("C:/Other/game.exe"));
    }
    #[test]
    fn rejects_empty_actions_and_duplicate_ids() {
        let mut rule = Rule {
            id: "a".into(),
            name: "Game".into(),
            path: "C:\\Game\\game.exe".into(),
            ..Default::default()
        };
        assert!(validate(&Config {
            enabled: true,
            rules: vec![rule.clone()]
        })
        .is_err());
        rule.warp = Action::Off;
        assert!(validate(&Config {
            enabled: true,
            rules: vec![rule.clone()]
        })
        .is_ok());
        assert!(validate(&Config {
            enabled: true,
            rules: vec![rule.clone(), rule]
        })
        .is_err());
    }
}
