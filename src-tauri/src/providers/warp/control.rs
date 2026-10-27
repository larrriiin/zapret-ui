//! All callers enter through the parent CLIENT gate, including startup and polling.
use super::*;
use serde::Deserialize;
use std::io::Write;
use std::net::{Ipv4Addr, SocketAddr, TcpStream};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Automation {
    pub restore_rules: bool,
}
#[derive(Default)]
pub(super) struct Control {
    pub directory: PathBuf,
    pub automation: Automation,
    pub errors: Vec<WarpError>,
    pub sites_error: Option<WarpError>,
    pub applications_error: Option<WarpError>,
    pub automation_error: Option<WarpError>,
    pub archive: Option<PathBuf>,
    pub sites_restore_attempted: bool,
    pub applications_restore_attempted: bool,
}
pub(super) static CONTROL: Mutex<Option<Control>> = Mutex::new(None);

pub fn initialize(directory: PathBuf) -> Result<()> {
    let path = directory.join("warp-automation.json");
    let read = || -> Result<Automation> {
        if !path.exists() {
            return Ok(Automation::default());
        }
        serde_json::from_slice(
            &std::fs::read(path).map_err(|e| WarpError::new("warp_preferences_error", e))?,
        )
        .map_err(|e| WarpError::new("warp_preferences_error", e))
    };
    let (automation, errors) = match read() {
        Ok(automation) => (automation, Vec::new()),
        Err(error) => (Automation::default(), vec![error]),
    };
    *CONTROL
        .lock()
        .map_err(|e| WarpError::new("warp_process", e))? = Some(Control {
        directory,
        automation,
        errors,
        ..Default::default()
    });
    Ok(())
}
pub(super) fn remember_error(control: &mut Control, error: WarpError) {
    if control
        .errors
        .last()
        .is_none_or(|last| last.code != error.code || last.detail != error.detail)
    {
        control.errors.push(error);
        if control.errors.len() > 8 {
            control.errors.remove(0);
        }
    }
}

pub(super) fn protocol(value: &str) -> Option<String> {
    ["MASQUE", "WireGuard"]
        .into_iter()
        .find(|p| p.eq_ignore_ascii_case(value.trim()))
        .map(str::to_owned)
}
pub(super) fn parse_protocol(settings: &Value) -> Option<String> {
    [
        "tunnel_protocol",
        "warp_tunnel_protocol",
        "preferred_tunnel_protocol",
    ]
    .iter()
    .find_map(|key| {
        settings
            .pointer(&format!("/settings/{key}"))
            .and_then(Value::as_str)
            .and_then(protocol)
    })
}
pub(super) fn parse_text_protocol(settings: &str) -> Option<String> {
    settings.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.trim()
            .to_ascii_lowercase()
            .ends_with("tunnel protocol")
            .then(|| protocol(value))
            .flatten()
    })
}
pub(super) fn protocol_supported(help: &str) -> bool {
    help.contains("<PROTOCOL>")
        && help.lines().any(|l| l.trim().starts_with("- MASQUE:"))
        && help.lines().any(|l| l.trim().starts_with("- WireGuard:"))
}
pub(super) fn full_tunnel(mode: Option<&str>) -> bool {
    matches!(mode, Some("warp" | "warp+dot" | "warp+doh" | "tunnel_only"))
}

pub(super) fn socks_ready(port: u16) -> bool {
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let Ok(mut socket) = TcpStream::connect_timeout(&address, Duration::from_millis(350)) else {
        return false;
    };
    let _ = socket.set_read_timeout(Some(Duration::from_millis(350)));
    let _ = socket.set_write_timeout(Some(Duration::from_millis(350)));
    let mut response = [0; 2];
    socket.write_all(&[5, 1, 0]).is_ok()
        && socket.read_exact(&mut response).is_ok()
        && response == [5, 0]
}

// The small boundary permits orchestration tests without running a CLI or changing Windows.
pub(super) trait RoutingClient {
    fn read(&self) -> WarpStatus;
    fn command(&self, args: &[&str]) -> Result<()>;
    fn ready(&self, port: u16) -> bool;
    fn can_set_protocol(&self) -> bool;
    fn readiness_timeout(&self) -> Duration {
        Duration::from_secs(25)
    }
}
impl RoutingClient for Client {
    fn read(&self) -> WarpStatus {
        self.status()
    }
    fn command(&self, args: &[&str]) -> Result<()> {
        cli(&self.path, args).map(|_| ())
    }
    fn ready(&self, port: u16) -> bool {
        socks_ready(port)
    }
    fn can_set_protocol(&self) -> bool {
        self.protocol_editable
    }
}
// Configuration commands can make the official service reconnect on their own.
// Cancel that side effect even when a command fails after changing the service.
pub(super) fn preserve_connection(
    client: &impl RoutingClient,
    before: &WarpStatus,
    change: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let result = change();
    if !before.connected {
        client.command(&["disconnect"])?;
    }
    result
}

pub(super) fn set_mode(client: &impl RoutingClient, before: &WarpStatus, mode: &str) -> Result<()> {
    let set_protocol = mode == "proxy" && before.protocol.as_deref() != Some("MASQUE");
    if set_protocol && !client.can_set_protocol() {
        return Err(WarpError::new("warp_protocol_unavailable", ""));
    }
    if before.mode.as_deref() == Some(mode) && !set_protocol {
        return Ok(());
    }
    preserve_connection(client, before, || {
        if set_protocol {
            client.command(&["tunnel", "protocol", "set", "MASQUE"])?;
        }
        client.command(&["mode", mode])
    })
}

pub(super) fn ensure_proxy(client: &impl RoutingClient, confirmed: bool) -> Result<u16> {
    let status = client.read();
    if let Some(error) = status.error {
        return Err(error);
    }
    if status.mode.is_none() {
        return Err(WarpError::new("warp_status_error", ""));
    }
    if !status
        .mode
        .as_deref()
        .is_some_and(|mode| MODES.contains(&mode))
    {
        return Err(WarpError::new(
            "warp_unsupported",
            "Unrecognized client mode",
        ));
    }
    if full_tunnel(status.mode.as_deref()) && !confirmed {
        return Err(WarpError::new("warp_confirm_proxy", ""));
    }
    if !status.modes.iter().any(|mode| mode == "proxy") {
        return Err(WarpError::new(
            "warp_unsupported",
            "Local proxy mode unavailable",
        ));
    }
    if status.protocol.as_deref() != Some("MASQUE") {
        if !client.can_set_protocol() {
            return Err(WarpError::new("warp_protocol_unavailable", ""));
        }
        client.command(&["tunnel", "protocol", "set", "MASQUE"])?;
    }
    if status.mode.as_deref() != Some("proxy") {
        client.command(&["mode", "proxy"])?;
    }
    if !status.connected
        || status.mode.as_deref() != Some("proxy")
        || status.protocol.as_deref() != Some("MASQUE")
    {
        client.command(&["connect"])?;
    }
    let deadline = Instant::now() + client.readiness_timeout();
    loop {
        let status = client.read();
        if let Some(error) = status.error {
            return Err(error);
        }
        if status.mode.as_deref() != Some("proxy") || status.protocol.as_deref() != Some("MASQUE") {
            return Err(WarpError::new("warp_proxy_incompatible", ""));
        }
        if let Some(port) = status
            .proxy
            .filter(|p| p.active && p.kind.as_deref() == Some("SOCKS5"))
            .and_then(|p| p.port)
        {
            if client.ready(port) {
                return Ok(port);
            }
        }
        if Instant::now() >= deadline {
            return Err(WarpError::new("warp_proxy_unavailable", ""));
        }
        std::thread::sleep(Duration::from_millis(300));
    }
}

pub(super) fn reconcile(control: &mut Control, status: &WarpStatus) -> Result<()> {
    let port = status
        .proxy
        .as_ref()
        .filter(|p| p.active && p.kind.as_deref() == Some("SOCKS5"))
        .and_then(|p| p.port)
        .filter(|_| status.protocol.as_deref() == Some("MASQUE") && status.error.is_none())
        .filter(|port| socks_ready(*port));
    // On unverified state release only our routing resources, preserving user preferences.
    if port.is_none() {
        sites::disable()?;
        applications::disable()?;
        control.sites_restore_attempted = false;
        control.applications_restore_attempted = false;
    }
    sites::reconcile_port(port)?;
    applications::reconcile_port(port)?;
    let selected_sites = sites::status()?;
    let selected_apps = applications::status()?;
    if selected_sites.runtime_active {
        control.sites_restore_attempted = true;
    }
    if selected_apps.runtime_active {
        control.applications_restore_attempted = true;
    }
    if !control.automation.restore_rules {
        return Ok(());
    }
    let Some(port) = port else {
        if selected_sites.enabled || selected_apps.enabled {
            let error = WarpError::new("warp_restore_waiting", "");
            remember_error(control, error.clone());
            if selected_sites.enabled {
                control.sites_error = Some(error.clone());
            }
            if selected_apps.enabled {
                control.applications_error = Some(error);
            }
        }
        return Ok(());
    };
    if selected_sites.enabled && !selected_sites.runtime_active && !control.sites_restore_attempted
    {
        control.sites_restore_attempted = true;
        match sites::configure(selected_sites.domains, true, Some(port)) {
            Ok(()) => control.sites_error = None,
            Err(error) => {
                control.sites_error = Some(error.clone());
                remember_error(control, error);
            }
        }
    }
    if selected_apps.enabled
        && !selected_apps.runtime_active
        && !control.applications_restore_attempted
    {
        control.applications_restore_attempted = true;
        match applications::configure(selected_apps.paths, true, Some(port)) {
            Ok(()) => control.applications_error = None,
            Err(error) => {
                control.applications_error = Some(error.clone());
                remember_error(control, error);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    struct Fake {
        mode: RefCell<String>,
        protocol: RefCell<String>,
        commands: RefCell<Vec<Vec<String>>>,
        connected: RefCell<bool>,
        fail_connect: bool,
        reconnect_on_configuration: bool,
        fail_mode: bool,
        fail_disconnect: bool,
        ready: bool,
        supported: bool,
    }
    impl Fake {
        fn new(mode: &str, protocol: &str) -> Self {
            Self {
                mode: RefCell::new(mode.into()),
                protocol: RefCell::new(protocol.into()),
                commands: RefCell::default(),
                connected: RefCell::new(false),
                fail_connect: false,
                reconnect_on_configuration: false,
                fail_mode: false,
                fail_disconnect: false,
                ready: true,
                supported: true,
            }
        }
    }
    impl RoutingClient for Fake {
        fn read(&self) -> WarpStatus {
            WarpStatus {
                installed: true,
                connected: *self.connected.borrow(),
                mode: Some(self.mode.borrow().clone()),
                protocol: Some(self.protocol.borrow().clone()),
                modes: vec!["proxy".into(), "warp".into()],
                proxy: (self.mode.borrow().as_str() == "proxy").then(|| ProxyStatus {
                    address: "127.0.0.1".into(),
                    port: Some(40000),
                    kind: Some("SOCKS5".into()),
                    active: *self.connected.borrow(),
                    port_editable: true,
                }),
                ..Default::default()
            }
        }
        fn command(&self, args: &[&str]) -> Result<()> {
            self.commands
                .borrow_mut()
                .push(args.iter().map(|s| s.to_string()).collect());
            match args {
                ["tunnel", "protocol", "set", protocol] => {
                    *self.protocol.borrow_mut() = protocol.to_string();
                    if self.reconnect_on_configuration {
                        *self.connected.borrow_mut() = true;
                    }
                }
                ["mode", mode] => {
                    *self.mode.borrow_mut() = mode.to_string();
                    if self.reconnect_on_configuration {
                        *self.connected.borrow_mut() = true;
                    }
                    if self.fail_mode {
                        return Err(WarpError::new("warp_cli_error", "mode failed"));
                    }
                }
                ["connect"] if self.fail_connect => {
                    return Err(WarpError::new("warp_cli_error", "connection refused"))
                }
                ["connect"] => *self.connected.borrow_mut() = true,
                ["disconnect"] if self.fail_disconnect => {
                    return Err(WarpError::new("warp_cli_error", "disconnect failed"))
                }
                ["disconnect"] => *self.connected.borrow_mut() = false,
                _ => panic!("Unexpected command"),
            }
            Ok(())
        }
        fn ready(&self, _: u16) -> bool {
            self.ready
        }
        fn can_set_protocol(&self) -> bool {
            self.supported
        }
        fn readiness_timeout(&self) -> Duration {
            Duration::ZERO
        }
    }
    #[test]
    fn manual_mode_changes_preserve_disconnection_despite_cli_autoconnect() {
        for (from, to, protocol) in [
            ("proxy", "warp", "MASQUE"),
            ("warp", "proxy", "WireGuard"),
            ("warp", "doh", "MASQUE"),
        ] {
            let mut fake = Fake::new(from, protocol);
            fake.reconnect_on_configuration = true;
            set_mode(&fake, &fake.read(), to).unwrap();
            assert!(!fake.read().connected);
            assert_eq!(fake.read().mode.as_deref(), Some(to));
            assert_eq!(fake.commands.borrow().last().unwrap(), &["disconnect"]);
            assert!(!fake
                .commands
                .borrow()
                .iter()
                .any(|args| args == &["connect"]));
        }
    }
    #[test]
    fn manual_mode_change_keeps_connected_clients_and_same_mode_is_a_noop() {
        let fake = Fake::new("warp", "MASQUE");
        *fake.connected.borrow_mut() = true;
        set_mode(&fake, &fake.read(), "proxy").unwrap();
        assert!(fake.read().connected);
        assert_eq!(*fake.commands.borrow(), vec![vec!["mode", "proxy"]]);
        fake.commands.borrow_mut().clear();
        set_mode(&fake, &fake.read(), "proxy").unwrap();
        assert!(fake.commands.borrow().is_empty());
    }
    #[test]
    fn failed_configuration_still_disconnects_and_disconnect_failure_is_reported() {
        let mut fake = Fake::new("warp", "WireGuard");
        fake.reconnect_on_configuration = true;
        fake.fail_mode = true;
        assert_eq!(
            set_mode(&fake, &fake.read(), "proxy").unwrap_err().detail,
            "mode failed"
        );
        assert!(!fake.read().connected);
        fake.fail_mode = false;
        fake.fail_disconnect = true;
        assert_eq!(
            set_mode(&fake, &fake.read(), "warp").unwrap_err().detail,
            "disconnect failed"
        );
    }
    #[test]
    fn manual_protocol_changes_also_preserve_disconnection() {
        let mut fake = Fake::new("warp", "MASQUE");
        fake.reconnect_on_configuration = true;
        preserve_connection(&fake, &fake.read(), || {
            fake.command(&["tunnel", "protocol", "set", "WireGuard"])
        })
        .unwrap();
        assert!(!fake.read().connected);
        assert_eq!(fake.read().protocol.as_deref(), Some("WireGuard"));
    }
    #[test]
    fn full_tunnel_requires_confirmation_before_any_mutation() {
        let fake = Fake::new("warp", "WireGuard");
        assert_eq!(
            ensure_proxy(&fake, false).unwrap_err().code,
            "warp_confirm_proxy"
        );
        assert!(fake.commands.borrow().is_empty());
        assert_eq!(ensure_proxy(&fake, true).unwrap(), 40000);
        assert_eq!(
            *fake.commands.borrow(),
            vec![
                vec!["tunnel", "protocol", "set", "MASQUE"],
                vec!["mode", "proxy"],
                vec!["connect"]
            ]
        );
    }
    #[test]
    fn dns_mode_needs_no_confirmation_and_proxy_always_requires_masque() {
        let fake = Fake::new("doh", "WireGuard");
        assert_eq!(ensure_proxy(&fake, false).unwrap(), 40000);
        assert_eq!(fake.protocol.borrow().as_str(), "MASQUE");
        let mut unsupported = Fake::new("proxy", "WireGuard");
        unsupported.supported = false;
        assert_eq!(
            ensure_proxy(&unsupported, false).unwrap_err().code,
            "warp_protocol_unavailable"
        );
        assert!(unsupported.commands.borrow().is_empty());
    }
    #[test]
    fn refusal_and_unavailable_endpoint_never_report_active() {
        let mut refusal = Fake::new("proxy", "MASQUE");
        refusal.fail_connect = true;
        assert!(ensure_proxy(&refusal, false)
            .unwrap_err()
            .detail
            .contains("refused"));
        let mut unavailable = Fake::new("proxy", "MASQUE");
        unavailable.ready = false;
        assert_eq!(
            ensure_proxy(&unavailable, false).unwrap_err().code,
            "warp_proxy_unavailable"
        );
    }
    #[test]
    fn an_unknown_client_mode_never_gets_silently_replaced() {
        let fake = Fake::new("future-mode", "MASQUE");
        assert_eq!(
            ensure_proxy(&fake, false).unwrap_err().code,
            "warp_unsupported"
        );
        assert!(fake.commands.borrow().is_empty());
    }
    #[test]
    fn already_connected_proxy_is_not_reconnected() {
        let fake = Fake::new("proxy", "MASQUE");
        *fake.connected.borrow_mut() = true;
        assert_eq!(ensure_proxy(&fake, false).unwrap(), 40000);
        assert!(fake.commands.borrow().is_empty());
    }
    #[test]
    fn removed_autoconnect_is_ignored_in_legacy_preferences() {
        let preferences: Automation =
            serde_json::from_str(r#"{"connect_on_start":true,"restore_rules":true}"#).unwrap();
        assert!(preferences.restore_rules);
        assert_eq!(
            serde_json::to_value(preferences).unwrap(),
            serde_json::json!({"restore_rules":true})
        );
    }
    #[test]
    fn protocol_help_and_settings_are_strict() {
        assert!(protocol_supported(
            "<PROTOCOL>\n - MASQUE: current\n - WireGuard: legacy"
        ));
        assert!(!protocol_supported("MASQUE WireGuard"));
        assert_eq!(
            parse_protocol(&serde_json::json!({"settings":{"tunnel_protocol":"Masque"}}))
                .as_deref(),
            Some("MASQUE")
        );
        assert_eq!(
            parse_text_protocol("(User set) WARP tunnel protocol: WireGuard").as_deref(),
            Some("WireGuard")
        );
        assert_eq!(parse_text_protocol("DNS protocol: WireGuard"), None);
        assert_eq!(parse_text_protocol("WARP tunnel protocol: Invented"), None);
        let defaults: Automation = serde_json::from_str("{}").unwrap();
        assert!(!defaults.restore_rules);
    }
}
