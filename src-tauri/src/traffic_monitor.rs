use serde::Serialize;
use std::collections::{HashMap, HashSet, VecDeque};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

trait MutexExt<T> {
    fn lock_unpoisoned(&self) -> MutexGuard<'_, T>;
}

impl<T> MutexExt<T> for Mutex<T> {
    fn lock_unpoisoned(&self) -> MutexGuard<'_, T> {
        self.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

mod evidence;
mod routes;
mod rules;
use evidence::FlowEvidence;
pub(crate) use routes::ProxyRoute;
use routes::{Route, RouteContext};
use rules::{ActiveZapretRules as CapturePortFilter, Decision, Evidence, ZapretState};

#[derive(Clone, Debug, Default)]
struct IpSetMatcher {
    ipv4: HashMap<u8, HashSet<u32>>,
    ipv6: HashMap<u8, HashSet<u128>>,
    invalid: bool,
}

impl IpSetMatcher {
    fn from_file(value: &str) -> Self {
        let path = Path::new(value.trim_matches('"'));
        let Ok(content) = std::fs::read_to_string(path) else {
            return Self {
                invalid: true,
                ..Self::default()
            };
        };
        let mut matcher = Self::default();
        for raw in content.lines() {
            let line = raw
                .split_once('#')
                .map(|(value, _)| value)
                .unwrap_or(raw)
                .trim()
                .trim_start_matches('\u{feff}');
            if line.is_empty() {
                continue;
            }
            let (address, prefix) = line.split_once('/').unwrap_or((line, ""));
            let Ok(address) = address.parse::<IpAddr>() else {
                matcher.invalid = true;
                continue;
            };
            match address {
                IpAddr::V4(address) => {
                    let prefix = if prefix.is_empty() {
                        32
                    } else if let Ok(prefix @ 0..=32) = prefix.parse::<u8>() {
                        prefix
                    } else {
                        matcher.invalid = true;
                        continue;
                    };
                    let value = u32::from(address) & ipv4_mask(prefix);
                    matcher.ipv4.entry(prefix).or_default().insert(value);
                }
                IpAddr::V6(address) => {
                    let prefix = if prefix.is_empty() {
                        128
                    } else if let Ok(prefix @ 0..=128) = prefix.parse::<u8>() {
                        prefix
                    } else {
                        matcher.invalid = true;
                        continue;
                    };
                    let value = u128::from(address) & ipv6_mask(prefix);
                    matcher.ipv6.entry(prefix).or_default().insert(value);
                }
            }
        }
        matcher
    }

    fn merge(&mut self, other: Self) {
        self.invalid |= other.invalid;
        for (prefix, networks) in other.ipv4 {
            self.ipv4.entry(prefix).or_default().extend(networks);
        }
        for (prefix, networks) in other.ipv6 {
            self.ipv6.entry(prefix).or_default().extend(networks);
        }
    }

    fn is_empty(&self) -> bool {
        self.ipv4.is_empty() && self.ipv6.is_empty()
    }

    fn contains(&self, address: IpAddr) -> bool {
        match address {
            IpAddr::V4(address) => {
                let value = u32::from(address);
                self.ipv4
                    .iter()
                    .any(|(prefix, networks)| networks.contains(&(value & ipv4_mask(*prefix))))
            }
            IpAddr::V6(address) => {
                let value = u128::from(address);
                self.ipv6
                    .iter()
                    .any(|(prefix, networks)| networks.contains(&(value & ipv6_mask(*prefix))))
            }
        }
    }
}

fn ipv4_mask(prefix: u8) -> u32 {
    if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - prefix)
    }
}

fn ipv6_mask(prefix: u8) -> u128 {
    if prefix == 0 {
        0
    } else {
        u128::MAX << (128 - prefix)
    }
}

fn split_command_line(args: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for character in args.chars() {
        match character {
            '"' => quoted = !quoted,
            value if value.is_whitespace() && !quoted => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            value => current.push(value),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
enum Protocol {
    Tcp,
    Udp,
}

impl Protocol {
    fn label(self) -> &'static str {
        match self {
            Self::Tcp => "TCP",
            Self::Udp => "UDP",
        }
    }
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct FlowKey {
    protocol: Protocol,
    local_address: IpAddr,
    local_port: u16,
    remote_address: IpAddr,
    remote_port: u16,
}

#[derive(Clone, Debug)]
struct FlowStats {
    pid: u32,
    process_name: String,
    state: String,
    upload_bytes: u64,
    download_bytes: u64,
    zapret_upload_bytes: u64,
    zapret_download_bytes: u64,
    sampled_upload_bytes: u64,
    sampled_download_bytes: u64,
    sampled_zapret_upload_bytes: u64,
    sampled_zapret_download_bytes: u64,
    last_seen: Instant,
    last_event: Option<&'static str>,
    evidence: FlowEvidence,
    interface_route: Route,
    base_upload: u64,
    base_download: u64,
    started: Instant,
    syn_sequence: Option<u32>,
    classification: Option<(Route, Decision)>,
    proxy_route: Option<Route>,
}

impl FlowStats {
    fn route(&mut self, routes: &RouteContext, key: &FlowKey) -> Route {
        let observed = routes.classify(key, self.pid, self.interface_route, self.started);
        if matches!(observed, Route::TgProxy | Route::TgProxyWarp)
            || (observed == Route::Warp && self.interface_route != Route::Warp)
        {
            self.proxy_route = Some(observed);
        }
        self.proxy_route.unwrap_or(observed)
    }
    fn new(pid: u32, process_name: String, state: String, now: Instant) -> Self {
        Self {
            pid,
            process_name,
            state,
            upload_bytes: 0,
            download_bytes: 0,
            zapret_upload_bytes: 0,
            zapret_download_bytes: 0,
            sampled_upload_bytes: 0,
            sampled_download_bytes: 0,
            sampled_zapret_upload_bytes: 0,
            sampled_zapret_download_bytes: 0,
            last_seen: now,
            last_event: None,
            evidence: FlowEvidence::default(),
            interface_route: Route::Unknown,
            base_upload: 0,
            base_download: 0,
            started: now,
            syn_sequence: None,
            classification: None,
            proxy_route: None,
        }
    }
}

struct TrafficData {
    details_limited: bool,
    flows: HashMap<FlowKey, FlowStats>,
    events: VecDeque<TrafficEvent>,
    next_event_id: u64,
    upload_bytes: u64,
    download_bytes: u64,
    zapret_upload_bytes: u64,
    zapret_download_bytes: u64,
    sampled_upload_bytes: u64,
    sampled_download_bytes: u64,
    sampled_zapret_upload_bytes: u64,
    sampled_zapret_download_bytes: u64,
    last_sample: Instant,
}

impl Default for TrafficData {
    fn default() -> Self {
        Self {
            details_limited: false,
            flows: HashMap::new(),
            events: VecDeque::new(),
            next_event_id: 1,
            upload_bytes: 0,
            download_bytes: 0,
            zapret_upload_bytes: 0,
            zapret_download_bytes: 0,
            sampled_upload_bytes: 0,
            sampled_download_bytes: 0,
            sampled_zapret_upload_bytes: 0,
            sampled_zapret_download_bytes: 0,
            last_sample: Instant::now(),
        }
    }
}

impl TrafficData {
    fn reset_filter_counters(&mut self) {
        self.zapret_upload_bytes = 0;
        self.zapret_download_bytes = 0;
        self.sampled_zapret_upload_bytes = 0;
        self.sampled_zapret_download_bytes = 0;
        for flow in self.flows.values_mut() {
            flow.zapret_upload_bytes = 0;
            flow.zapret_download_bytes = 0;
            flow.sampled_zapret_upload_bytes = 0;
            flow.sampled_zapret_download_bytes = 0;
            flow.base_upload = flow.upload_bytes;
            flow.base_download = flow.download_bytes;
        }
    }
}

#[derive(Serialize)]
pub struct TrafficConnection {
    id: String,
    protocol: &'static str,
    local_address: String,
    local_port: u16,
    remote_address: String,
    remote_port: u16,
    pid: u32,
    process_name: String,
    system_process: bool,
    state: String,
    upload_bytes: u64,
    download_bytes: u64,
    upload_bps: u64,
    download_bps: u64,
    zapret_upload_bytes: u64,
    zapret_download_bytes: u64,
    zapret_upload_bps: u64,
    zapret_download_bps: u64,
    zapret_candidate: bool,
    route: Route,
    zapret: Decision,
    hostname: Option<String>,
    last_seen_ms_ago: u64,
}

#[derive(Clone, Serialize)]
pub struct TrafficEvent {
    id: u64,
    timestamp_ms: u64,
    event_type: &'static str,
    protocol: &'static str,
    remote_address: String,
    remote_port: u16,
    pid: u32,
    process_name: String,
    system_process: bool,
    zapret_candidate: bool,
    route: Route,
    zapret: Decision,
}

#[derive(Serialize)]
pub struct TrafficSnapshot {
    details_limited: bool,
    running: bool,
    error: Option<String>,
    filter_active: bool,
    strategy: Option<String>,
    upload_bytes: u64,
    download_bytes: u64,
    upload_bps: u64,
    download_bps: u64,
    zapret_upload_bytes: u64,
    zapret_download_bytes: u64,
    zapret_upload_bps: u64,
    zapret_download_bps: u64,
    connections: Vec<TrafficConnection>,
    events: Vec<TrafficEvent>,
}

pub struct TrafficMonitor {
    running: AtomicBool,
    handle: AtomicIsize,
    error: Mutex<Option<String>>,
    filter: Mutex<CapturePortFilter>,
    filter_key: Mutex<String>,
    data: Mutex<TrafficData>,
    routes: Mutex<RouteContext>,
}

impl Default for TrafficMonitor {
    fn default() -> Self {
        Self {
            running: AtomicBool::new(false),
            handle: AtomicIsize::new(0),
            error: Mutex::new(None),
            filter: Mutex::new(CapturePortFilter::default()),
            filter_key: Mutex::new(String::new()),
            data: Mutex::new(TrafficData::default()),
            routes: Mutex::new(RouteContext::default()),
        }
    }
}

impl TrafficMonitor {
    pub fn refresh_routes(
        &self,
        warp: Vec<ProxyRoute>,
        telegram_pid: Option<u32>,
        telegram_listener: Option<std::net::SocketAddr>,
    ) {
        *self.routes.lock_unpoisoned() = RouteContext::new(warp, telegram_pid, telegram_listener);
    }

    pub fn unknown_filter(&self, strategy: Option<String>) {
        let mut filter = self.filter.lock_unpoisoned();
        let mut key = self.filter_key.lock_unpoisoned();
        let unknown_key = format!("unknown:{strategy:?}");
        if *key == unknown_key {
            return;
        }
        *filter = CapturePortFilter::unknown(strategy);
        *key = unknown_key;
        self.data.lock_unpoisoned().reset_filter_counters();
    }
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Acquire)
    }

    pub fn refresh_filter(&self, args: &str, strategy: Option<String>) {
        let key = CapturePortFilter::source_key(args, strategy.as_deref());
        let mut filter = self.filter.lock_unpoisoned();
        let mut filter_key = self.filter_key.lock_unpoisoned();
        if *filter_key == key {
            return;
        }
        *filter = CapturePortFilter::from_winws_args(args, strategy);
        *filter_key = key;
        self.data.lock_unpoisoned().reset_filter_counters();
    }

    pub fn clear_filter(&self) {
        let mut filter = self.filter.lock_unpoisoned();
        let mut filter_key = self.filter_key.lock_unpoisoned();
        if filter_key.is_empty() && !filter.active() {
            return;
        }
        *filter = CapturePortFilter::default();
        filter_key.clear();
        self.data.lock_unpoisoned().reset_filter_counters();
    }

    pub fn start(self: &Arc<Self>, dll_path: &Path) -> Result<(), String> {
        if self
            .running
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Ok(());
        }

        if !dll_path.is_file() {
            self.running.store(false, Ordering::Release);
            return Err(format!("WinDivert.dll not found: {}", dll_path.display()));
        }

        *self.error.lock_unpoisoned() = None;
        *self.data.lock_unpoisoned() = TrafficData::default();

        let monitor = Arc::clone(self);
        let dll_path = dll_path.to_path_buf();
        std::thread::Builder::new()
            .name("traffic-monitor".to_string())
            .spawn(move || {
                if let Err(error) = platform::capture_loop(&monitor, &dll_path) {
                    *monitor.error.lock_unpoisoned() = Some(error);
                }
                monitor.handle.store(0, Ordering::Release);
                monitor.running.store(false, Ordering::Release);
            })
            .map_err(|error| {
                self.running.store(false, Ordering::Release);
                format!("Failed to start traffic monitor thread: {error}")
            })?;

        Ok(())
    }

    pub fn stop(&self, dll_path: &Path) {
        if !self.running.swap(false, Ordering::AcqRel) {
            return;
        }
        let handle = self.handle.load(Ordering::Acquire);
        if handle != 0 {
            platform::shutdown_capture(handle, dll_path);
        }
        for _ in 0..40 {
            if self.handle.load(Ordering::Acquire) == 0 {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    pub fn snapshot(&self) -> TrafficSnapshot {
        let now = Instant::now();
        let filter = self.filter.lock_unpoisoned();
        let routes = self.routes.lock_unpoisoned();
        let mut data = self.data.lock_unpoisoned();
        let elapsed = now.duration_since(data.last_sample).as_secs_f64().max(0.1);
        data.flows
            .retain(|_, flow| now.duration_since(flow.last_seen) < Duration::from_secs(90));

        // Routing evidence can arrive after the first packet (e.g. SOCKS CONNECT).
        // Reconcile estimates before calculating rates, including demotions.
        let (mut old_up, mut old_down, mut new_up, mut new_down) = (0u64, 0u64, 0u64, 0u64);
        let mut reclassified = Vec::new();
        for (key, flow) in &mut data.flows {
            old_up = old_up.saturating_add(flow.zapret_upload_bytes);
            old_down = old_down.saturating_add(flow.zapret_download_bytes);
            let route = flow.route(&routes, key);
            let decision = classify_connection(&filter, key, &flow.evidence.summary, route);
            if flow
                .classification
                .as_ref()
                .is_some_and(|old| old.0 != route || old.1 != decision)
            {
                reclassified.push((
                    key.clone(),
                    flow.pid,
                    flow.process_name.clone(),
                    route,
                    decision.clone(),
                ));
            }
            flow.classification = Some((route, decision.clone()));
            flow.zapret_upload_bytes = if decision.candidate() {
                flow.upload_bytes.saturating_sub(flow.base_upload)
            } else {
                0
            };
            flow.zapret_download_bytes = if decision.candidate() {
                flow.download_bytes.saturating_sub(flow.base_download)
            } else {
                0
            };
            flow.sampled_zapret_upload_bytes = flow
                .sampled_zapret_upload_bytes
                .min(flow.zapret_upload_bytes);
            flow.sampled_zapret_download_bytes = flow
                .sampled_zapret_download_bytes
                .min(flow.zapret_download_bytes);
            new_up = new_up.saturating_add(flow.zapret_upload_bytes);
            new_down = new_down.saturating_add(flow.zapret_download_bytes);
        }
        data.zapret_upload_bytes = data
            .zapret_upload_bytes
            .saturating_sub(old_up)
            .saturating_add(new_up);
        data.zapret_download_bytes = data
            .zapret_download_bytes
            .saturating_sub(old_down)
            .saturating_add(new_down);
        data.sampled_zapret_upload_bytes = data
            .sampled_zapret_upload_bytes
            .saturating_sub(old_up.saturating_sub(new_up));
        data.sampled_zapret_download_bytes = data
            .sampled_zapret_download_bytes
            .saturating_sub(old_down.saturating_sub(new_down));
        for (key, pid, name, route, decision) in reclassified {
            push_event(&mut data, "classified", &key, pid, name, route, decision);
        }

        let mut connections = Vec::with_capacity(data.flows.len());
        for (key, flow) in &mut data.flows {
            let upload_bps = rate(flow.upload_bytes, flow.sampled_upload_bytes, elapsed);
            let download_bps = rate(flow.download_bytes, flow.sampled_download_bytes, elapsed);
            let zapret_upload_bps = rate(
                flow.zapret_upload_bytes,
                flow.sampled_zapret_upload_bytes,
                elapsed,
            );
            let zapret_download_bps = rate(
                flow.zapret_download_bytes,
                flow.sampled_zapret_download_bytes,
                elapsed,
            );
            flow.sampled_upload_bytes = flow.upload_bytes;
            flow.sampled_download_bytes = flow.download_bytes;
            flow.sampled_zapret_upload_bytes = flow.zapret_upload_bytes;
            flow.sampled_zapret_download_bytes = flow.zapret_download_bytes;

            let age = now.duration_since(flow.last_seen);
            let visible = match key.protocol {
                Protocol::Udp => age < Duration::from_secs(20),
                Protocol::Tcp => {
                    if matches!(
                        flow.state.as_str(),
                        "closed" | "error" | "time-wait" | "delete-tcb"
                    ) {
                        age < Duration::from_secs(5)
                    } else {
                        age < Duration::from_secs(20)
                            || matches!(
                                flow.state.as_str(),
                                "active"
                                    | "connecting"
                                    | "syn-sent"
                                    | "syn-received"
                                    | "established"
                                    | "close-wait"
                            )
                    }
                }
            };
            if !visible {
                continue;
            }

            let route = flow.route(&routes, key);
            let zapret = classify_connection(&filter, key, &flow.evidence.summary, route);
            let zapret_candidate = zapret.candidate();

            connections.push(TrafficConnection {
                id: format!(
                    "{}:{}:{}:{}:{}",
                    key.protocol.label(),
                    key.local_address,
                    key.local_port,
                    key.remote_address,
                    key.remote_port
                ),
                protocol: key.protocol.label(),
                local_address: key.local_address.to_string(),
                local_port: key.local_port,
                remote_address: key.remote_address.to_string(),
                remote_port: key.remote_port,
                pid: flow.pid,
                process_name: flow.process_name.clone(),
                system_process: is_system_process(flow.pid, &flow.process_name),
                state: flow.state.clone(),
                upload_bytes: flow.upload_bytes,
                download_bytes: flow.download_bytes,
                upload_bps,
                download_bps,
                zapret_upload_bytes: flow.zapret_upload_bytes,
                zapret_download_bytes: flow.zapret_download_bytes,
                zapret_upload_bps,
                zapret_download_bps,
                zapret_candidate,
                route,
                zapret,
                hostname: flow.evidence.summary.hostname.clone(),
                last_seen_ms_ago: age.as_millis() as u64,
            });
        }
        connections.sort_by(|left, right| {
            left.process_name
                .to_ascii_lowercase()
                .cmp(&right.process_name.to_ascii_lowercase())
                .then_with(|| left.pid.cmp(&right.pid))
                .then_with(|| left.remote_address.cmp(&right.remote_address))
                .then_with(|| left.remote_port.cmp(&right.remote_port))
        });
        let details_limited = data.details_limited || connections.len() > 1_000;
        connections.truncate(1_000);

        let events = data.events.iter().rev().take(500).cloned().collect();

        let snapshot = TrafficSnapshot {
            details_limited,
            running: self.running.load(Ordering::Acquire),
            error: self.error.lock_unpoisoned().clone(),
            filter_active: filter.active(),
            strategy: filter.strategy.clone(),
            upload_bytes: data.upload_bytes,
            download_bytes: data.download_bytes,
            upload_bps: rate(data.upload_bytes, data.sampled_upload_bytes, elapsed),
            download_bps: rate(data.download_bytes, data.sampled_download_bytes, elapsed),
            zapret_upload_bytes: data.zapret_upload_bytes,
            zapret_download_bytes: data.zapret_download_bytes,
            zapret_upload_bps: rate(
                data.zapret_upload_bytes,
                data.sampled_zapret_upload_bytes,
                elapsed,
            ),
            zapret_download_bps: rate(
                data.zapret_download_bytes,
                data.sampled_zapret_download_bytes,
                elapsed,
            ),
            connections,
            events,
        };

        data.sampled_upload_bytes = data.upload_bytes;
        data.sampled_download_bytes = data.download_bytes;
        data.sampled_zapret_upload_bytes = data.zapret_upload_bytes;
        data.sampled_zapret_download_bytes = data.zapret_download_bytes;
        data.last_sample = now;
        snapshot
    }

    fn record_packet(&self, packet: ParsedPacket<'_>, pid: u32, process_name: String) {
        let now = Instant::now();
        let filter = self.filter.lock_unpoisoned();
        let routes = self.routes.lock_unpoisoned();
        let mut data = self.data.lock_unpoisoned();
        // A reused 5-tuple is a new session. Archive prior counters rather than
        // leaking the old hostname or routing evidence into a new connection.
        if (packet.outbound
            && packet.tcp_flags & 0x12 == 0x02
            && data
                .flows
                .get(&packet.key)
                .is_some_and(|flow| flow.syn_sequence != Some(packet.sequence)))
            || data
                .flows
                .get(&packet.key)
                .is_some_and(|flow| flow.pid != 0 && pid != 0 && flow.pid != pid)
        {
            data.flows.remove(&packet.key);
        }
        let is_new = !data.flows.contains_key(&packet.key);
        if is_new && data.flows.len() >= 4096 {
            data.details_limited = true;
            if packet.outbound {
                data.upload_bytes = data.upload_bytes.saturating_add(packet.bytes);
            } else {
                data.download_bytes = data.download_bytes.saturating_add(packet.bytes);
            }
            return;
        }
        let allow_buffer = data.flows.len() < 512;
        let event_type = packet.event_type(is_new);
        let key = packet.key.clone();
        let (log, event_pid, event_process, route, decision, old_up, old_down, new_up, new_down) = {
            let flow = data
                .flows
                .entry(key.clone())
                .or_insert_with(|| FlowStats::new(pid, process_name.clone(), "active".into(), now));
            if packet.outbound && packet.tcp_flags & 0x12 == 0x02 {
                flow.syn_sequence = Some(packet.sequence);
            }
            if flow.pid == 0 && pid != 0 {
                flow.pid = pid;
                flow.process_name = process_name;
            }
            flow.last_seen = now;
            if packet.interface_route != Route::Unknown {
                flow.interface_route = packet.interface_route;
            }
            if packet.outbound {
                if key.protocol == Protocol::Tcp {
                    flow.evidence
                        .observe_tcp(packet.sequence, packet.payload, allow_buffer);
                } else {
                    flow.evidence.observe_udp(packet.payload);
                }
                flow.upload_bytes = flow.upload_bytes.saturating_add(packet.bytes);
            } else {
                flow.download_bytes = flow.download_bytes.saturating_add(packet.bytes);
            }
            if let Some(event) = event_type {
                flow.state = match event {
                    "connecting" => "connecting",
                    "connected" => "established",
                    "closed" => "closed",
                    "reset" => "error",
                    _ => "active",
                }
                .into();
            }
            let route = flow.route(&routes, &key);
            let decision = classify_connection(&filter, &key, &flow.evidence.summary, route);
            let changed = flow
                .classification
                .as_ref()
                .is_some_and(|old| old.0 != route || old.1 != decision);
            flow.classification = Some((route, decision.clone()));
            let old_up = flow.zapret_upload_bytes;
            let old_down = flow.zapret_download_bytes;
            flow.zapret_upload_bytes = if decision.candidate() {
                flow.upload_bytes.saturating_sub(flow.base_upload)
            } else {
                0
            };
            flow.zapret_download_bytes = if decision.candidate() {
                flow.download_bytes.saturating_sub(flow.base_download)
            } else {
                0
            };
            flow.sampled_zapret_upload_bytes = flow
                .sampled_zapret_upload_bytes
                .min(flow.zapret_upload_bytes);
            flow.sampled_zapret_download_bytes = flow
                .sampled_zapret_download_bytes
                .min(flow.zapret_download_bytes);
            let log = if event_type.is_some() && flow.last_event != event_type {
                flow.last_event = event_type;
                event_type
            } else if changed {
                Some("classified")
            } else {
                None
            };
            (
                log,
                flow.pid,
                flow.process_name.clone(),
                route,
                decision,
                old_up,
                old_down,
                flow.zapret_upload_bytes,
                flow.zapret_download_bytes,
            )
        };
        if packet.outbound {
            data.upload_bytes = data.upload_bytes.saturating_add(packet.bytes);
        } else {
            data.download_bytes = data.download_bytes.saturating_add(packet.bytes);
        }
        data.zapret_upload_bytes = data
            .zapret_upload_bytes
            .saturating_sub(old_up)
            .saturating_add(new_up);
        data.zapret_download_bytes = data
            .zapret_download_bytes
            .saturating_sub(old_down)
            .saturating_add(new_down);
        data.sampled_zapret_upload_bytes = data
            .sampled_zapret_upload_bytes
            .saturating_sub(old_up.saturating_sub(new_up));
        data.sampled_zapret_download_bytes = data
            .sampled_zapret_download_bytes
            .saturating_sub(old_down.saturating_sub(new_down));
        if let Some(log) = log {
            push_event(
                &mut data,
                log,
                &key,
                event_pid,
                event_process,
                route,
                decision,
            );
        }
    }

    fn seed_flow(&self, endpoint: &EndpointOwner, process_name: String) {
        let now = Instant::now();
        let mut data = self.data.lock_unpoisoned();
        if data.flows.len() >= 4096 && !data.flows.contains_key(&endpoint.key) {
            return;
        }
        let flow = data.flows.entry(endpoint.key.clone()).or_insert_with(|| {
            FlowStats::new(
                endpoint.pid,
                process_name.clone(),
                endpoint.state.clone(),
                now,
            )
        });
        flow.pid = endpoint.pid;
        flow.process_name = process_name;
        flow.state = endpoint.state.clone();
    }

    fn mark_missing_tcp_flows(&self, active: &HashSet<FlowKey>) {
        let mut data = self.data.lock_unpoisoned();
        for (key, flow) in &mut data.flows {
            if key.protocol == Protocol::Tcp
                && !active.contains(key)
                && !matches!(flow.state.as_str(), "closed" | "error")
            {
                flow.state = "closed".to_string();
            }
        }
    }
}

fn classify_connection(
    filter: &CapturePortFilter,
    key: &FlowKey,
    evidence: &Evidence,
    route: Route,
) -> Decision {
    let decision = filter.classify(key.protocol, key.remote_port, key.remote_address, evidence);
    if decision.state == ZapretState::Off {
        return decision;
    }
    if matches!(route, Route::Warp | Route::TgProxy | Route::TgProxyWarp) {
        return Decision::new(ZapretState::Unknown, "proxy");
    }
    decision
}

fn push_event(
    data: &mut TrafficData,
    event_type: &'static str,
    key: &FlowKey,
    pid: u32,
    process_name: String,
    route: Route,
    zapret: Decision,
) {
    let event = TrafficEvent {
        id: data.next_event_id,
        timestamp_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis() as u64)
            .unwrap_or_default(),
        event_type,
        protocol: key.protocol.label(),
        remote_address: key.remote_address.to_string(),
        remote_port: key.remote_port,
        pid,
        system_process: is_system_process(pid, &process_name),
        process_name,
        zapret_candidate: zapret.candidate(),
        route,
        zapret,
    };
    data.next_event_id = data.next_event_id.saturating_add(1);
    data.events.push_back(event);
    while data.events.len() > 2_000 {
        data.events.pop_front();
    }
}

fn is_system_process(pid: u32, process_name: &str) -> bool {
    if pid <= 4 {
        return true;
    }
    matches!(
        process_name.to_ascii_lowercase().as_str(),
        "system"
            | "idle"
            | "registry"
            | "svchost.exe"
            | "services.exe"
            | "lsass.exe"
            | "csrss.exe"
            | "wininit.exe"
            | "winlogon.exe"
            | "spoolsv.exe"
            | "audiodg.exe"
            | "dashost.exe"
            | "fontdrvhost.exe"
            | "sihost.exe"
    )
}

fn rate(current: u64, previous: u64, elapsed: f64) -> u64 {
    (current.saturating_sub(previous) as f64 / elapsed).round() as u64
}

#[derive(Debug)]
struct ParsedPacket<'a> {
    key: FlowKey,
    outbound: bool,
    bytes: u64,
    tcp_flags: u8,
    sequence: u32,
    payload: &'a [u8],
    interface_route: Route,
}

impl ParsedPacket<'_> {
    fn event_type(&self, is_new: bool) -> Option<&'static str> {
        if self.key.protocol == Protocol::Udp {
            return is_new.then_some("udp");
        }
        let syn = self.tcp_flags & 0x02 != 0;
        let ack = self.tcp_flags & 0x10 != 0;
        if self.tcp_flags & 0x04 != 0 {
            Some("reset")
        } else if self.tcp_flags & 0x01 != 0 {
            Some("closed")
        } else if syn && ack && !self.outbound {
            Some("connected")
        } else if syn && !ack && self.outbound {
            Some("connecting")
        } else if is_new {
            Some("observed")
        } else {
            None
        }
    }
}

fn parse_packet(packet: &[u8], outbound: bool) -> Option<ParsedPacket<'_>> {
    let version = packet.first()? >> 4;
    let (source, destination, protocol, transport_offset, bytes) = match version {
        4 if packet.len() >= 20 => {
            let header_len = ((packet[0] & 0x0f) as usize) * 4;
            if header_len < 20 || packet.len() < header_len + 4 {
                return None;
            }
            let protocol = packet[9];
            // Do not interpret a fragment payload as a TCP/UDP header.
            if u16::from_be_bytes([packet[6], packet[7]]) & 0x3fff != 0 {
                return None;
            }
            let source = IpAddr::V4(Ipv4Addr::new(
                packet[12], packet[13], packet[14], packet[15],
            ));
            let destination = IpAddr::V4(Ipv4Addr::new(
                packet[16], packet[17], packet[18], packet[19],
            ));
            let total_len = u16::from_be_bytes([packet[2], packet[3]]) as u64;
            (source, destination, protocol, header_len, total_len)
        }
        6 if packet.len() >= 44 => {
            let source = IpAddr::V6(Ipv6Addr::from(<[u8; 16]>::try_from(&packet[8..24]).ok()?));
            let destination =
                IpAddr::V6(Ipv6Addr::from(<[u8; 16]>::try_from(&packet[24..40]).ok()?));
            let mut next_header = packet[6];
            let mut offset = 40usize;
            while matches!(next_header, 0 | 43 | 44 | 60) {
                if packet.len() < offset + 8 {
                    return None;
                }
                let current = next_header;
                if current == 44 {
                    return None;
                }
                next_header = packet[offset];
                offset += if current == 44 {
                    8
                } else {
                    ((packet[offset + 1] as usize) + 1) * 8
                };
            }
            if packet.len() < offset + 4 {
                return None;
            }
            let total_len = u16::from_be_bytes([packet[4], packet[5]]) as u64 + 40;
            (source, destination, next_header, offset, total_len)
        }
        _ => return None,
    };

    let protocol = match protocol {
        6 => Protocol::Tcp,
        17 => Protocol::Udp,
        _ => return None,
    };
    let source_port = u16::from_be_bytes([packet[transport_offset], packet[transport_offset + 1]]);
    let destination_port =
        u16::from_be_bytes([packet[transport_offset + 2], packet[transport_offset + 3]]);
    let tcp_flags = if protocol == Protocol::Tcp && packet.len() > transport_offset + 13 {
        packet[transport_offset + 13]
    } else {
        0
    };
    let (local_address, local_port, remote_address, remote_port) = if outbound {
        (source, source_port, destination, destination_port)
    } else {
        (destination, destination_port, source, source_port)
    };

    let header_len = if protocol == Protocol::Tcp {
        let length = ((*packet.get(transport_offset + 12)? >> 4) as usize) * 4;
        if length < 20 {
            return None;
        }
        length
    } else {
        8
    };
    let payload_end = (bytes as usize).min(packet.len());
    let payload = packet.get(transport_offset + header_len..payload_end)?;
    let sequence = if protocol == Protocol::Tcp {
        u32::from_be_bytes(
            packet
                .get(transport_offset + 4..transport_offset + 8)?
                .try_into()
                .ok()?,
        )
        .wrapping_add(u32::from(tcp_flags & 0x02 != 0))
    } else {
        0
    };
    Some(ParsedPacket {
        key: FlowKey {
            protocol,
            local_address,
            local_port,
            remote_address,
            remote_port,
        },
        outbound,
        bytes: bytes.min(packet.len() as u64),
        tcp_flags,
        sequence,
        payload,
        interface_route: Route::Unknown,
    })
}

#[derive(Clone)]
struct EndpointOwner {
    key: FlowKey,
    pid: u32,
    state: String,
}

#[cfg(windows)]
mod platform {
    use super::*;
    use std::ffi::{c_char, c_void};
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{
        CloseHandle, FreeLibrary, GetLastError, HANDLE, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        GetExtendedTcpTable, GetExtendedUdpTable, GetIfEntry2, MIB_IF_ROW2, MIB_TCP6ROW_OWNER_PID,
        MIB_TCPROW_OWNER_PID, MIB_UDP6ROW_OWNER_PID, MIB_UDPROW_OWNER_PID, TCP_TABLE_OWNER_PID_ALL,
        UDP_TABLE_OWNER_PID,
    };
    use windows_sys::Win32::Networking::WinSock::{AF_INET, AF_INET6};
    use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    const WINDIVERT_LAYER_NETWORK: i32 = 0;
    const WINDIVERT_FLAG_SNIFF: u64 = 1;
    const WINDIVERT_FLAG_RECV_ONLY: u64 = 4;
    const WINDIVERT_SHUTDOWN_RECV: i32 = 1;
    const ERROR_INSUFFICIENT_BUFFER: u32 = 122;
    const ERROR_NO_DATA: u32 = 232;

    type WinDivertOpen = unsafe extern "system" fn(*const c_char, i32, i16, u64) -> HANDLE;
    type WinDivertRecv =
        unsafe extern "system" fn(HANDLE, *mut c_void, u32, *mut u32, *mut c_void) -> i32;
    type WinDivertShutdown = unsafe extern "system" fn(HANDLE, i32) -> i32;
    type WinDivertClose = unsafe extern "system" fn(HANDLE) -> i32;

    struct WinDivertApi {
        module: windows_sys::Win32::Foundation::HMODULE,
        open: WinDivertOpen,
        recv: WinDivertRecv,
        shutdown: WinDivertShutdown,
        close: WinDivertClose,
    }

    impl WinDivertApi {
        unsafe fn load(path: &Path) -> Result<Self, String> {
            let path_wide: Vec<u16> = path
                .as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            let module = unsafe { LoadLibraryW(path_wide.as_ptr()) };
            if module.is_null() {
                return Err(format!(
                    "Failed to load WinDivert.dll (Windows error {})",
                    unsafe { GetLastError() }
                ));
            }

            macro_rules! proc_address {
                ($name:literal, $ty:ty) => {{
                    let proc = unsafe { GetProcAddress(module, concat!($name, "\0").as_ptr()) };
                    match proc {
                        Some(proc) => unsafe {
                            std::mem::transmute::<unsafe extern "system" fn() -> isize, $ty>(proc)
                        },
                        None => {
                            unsafe { FreeLibrary(module) };
                            return Err(format!("WinDivert.dll does not export {}", $name));
                        }
                    }
                }};
            }

            Ok(Self {
                module,
                open: proc_address!("WinDivertOpen", WinDivertOpen),
                recv: proc_address!("WinDivertRecv", WinDivertRecv),
                shutdown: proc_address!("WinDivertShutdown", WinDivertShutdown),
                close: proc_address!("WinDivertClose", WinDivertClose),
            })
        }
    }

    impl Drop for WinDivertApi {
        fn drop(&mut self) {
            unsafe { FreeLibrary(self.module) };
        }
    }

    pub fn capture_loop(monitor: &Arc<TrafficMonitor>, dll_path: &Path) -> Result<(), String> {
        let api = unsafe { WinDivertApi::load(dll_path)? };
        let filter = b"ip or ipv6\0";
        let handle = unsafe {
            (api.open)(
                filter.as_ptr().cast(),
                WINDIVERT_LAYER_NETWORK,
                30_000,
                WINDIVERT_FLAG_SNIFF | WINDIVERT_FLAG_RECV_ONLY,
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(format!("WinDivertOpen failed (Windows error {})", unsafe {
                GetLastError()
            }));
        }
        monitor.handle.store(handle as isize, Ordering::Release);

        let mut owners = EndpointIndex::refresh();
        seed_tcp_connections(monitor, &owners);
        let mut last_owner_refresh = Instant::now();
        let mut packet = vec![0u8; 65_535];
        let mut address = [0u8; 80];
        let mut interfaces: HashMap<u32, (Instant, Route)> = HashMap::new();

        while monitor.running.load(Ordering::Acquire) {
            let mut received = 0u32;
            let ok = unsafe {
                (api.recv)(
                    handle,
                    packet.as_mut_ptr().cast(),
                    packet.len() as u32,
                    &mut received,
                    address.as_mut_ptr().cast(),
                )
            };
            if ok == 0 {
                if monitor.running.load(Ordering::Acquire) {
                    let error = unsafe { GetLastError() };
                    unsafe { (api.close)(handle) };
                    return Err(format!("WinDivertRecv failed (Windows error {error})"));
                }
                break;
            }

            if last_owner_refresh.elapsed() >= Duration::from_secs(1) {
                owners = EndpointIndex::refresh();
                seed_tcp_connections(monitor, &owners);
                last_owner_refresh = Instant::now();
            }

            // WINDIVERT_ADDRESS packs Layer/Event into bits 0..15, then Sniffed
            // and Outbound. We only need the Outbound bit and deliberately keep
            // the rest opaque so this code remains independent of C bitfields.
            let flags = u32::from_le_bytes(address[8..12].try_into().unwrap_or_default());
            let outbound = (flags & (1 << 17)) != 0;
            if let Some(mut parsed) = parse_packet(&packet[..received as usize], outbound) {
                let index = u32::from_le_bytes(address[16..20].try_into().unwrap_or_default());
                let entry = interfaces
                    .entry(index)
                    .or_insert((Instant::now() - Duration::from_secs(6), Route::Unknown));
                if entry.0.elapsed() > Duration::from_secs(5) {
                    let mut row = MIB_IF_ROW2 {
                        InterfaceIndex: index,
                        ..Default::default()
                    };
                    entry.1 = if unsafe { GetIfEntry2(&mut row) } == 0 {
                        let name = format!(
                            "{} {}",
                            String::from_utf16_lossy(&row.Alias),
                            String::from_utf16_lossy(&row.Description)
                        )
                        .to_ascii_lowercase();
                        if name.contains("cloudflare") && name.contains("warp") {
                            Route::Warp
                        } else if matches!(row.Type, 6 | 71) {
                            Route::Direct
                        } else if row.Type == 24 {
                            Route::Local
                        } else {
                            Route::Unknown
                        }
                    } else {
                        Route::Unknown
                    };
                    entry.0 = Instant::now();
                }
                parsed.interface_route = entry.1;
                let pid = owners.pid_for(&parsed.key);
                let process_name = owners.process_name(pid);
                monitor.record_packet(parsed, pid, process_name);
            }
        }

        unsafe { (api.close)(handle) };
        Ok(())
    }

    pub fn shutdown_capture(handle: isize, dll_path: &Path) {
        if let Ok(api) = unsafe { WinDivertApi::load(dll_path) } {
            unsafe {
                (api.shutdown)(handle as HANDLE, WINDIVERT_SHUTDOWN_RECV);
            }
        }
    }

    struct EndpointIndex {
        tcp: HashMap<FlowKey, EndpointOwner>,
        udp: HashMap<(IpAddr, u16), u32>,
        process_names: Mutex<HashMap<u32, String>>,
    }

    impl EndpointIndex {
        fn refresh() -> Self {
            let mut index = Self {
                tcp: HashMap::new(),
                udp: HashMap::new(),
                process_names: Mutex::new(HashMap::new()),
            };
            index.read_tcp4();
            index.read_tcp6();
            index.read_udp4();
            index.read_udp6();
            index
        }

        fn pid_for(&self, key: &FlowKey) -> u32 {
            if key.protocol == Protocol::Tcp {
                return self.tcp.get(key).map(|entry| entry.pid).unwrap_or(0);
            }
            self.udp
                .get(&(key.local_address, key.local_port))
                .or_else(|| {
                    let wildcard = match key.local_address {
                        IpAddr::V4(_) => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                        IpAddr::V6(_) => IpAddr::V6(Ipv6Addr::UNSPECIFIED),
                    };
                    self.udp.get(&(wildcard, key.local_port))
                })
                .copied()
                .unwrap_or(0)
        }

        fn process_name(&self, pid: u32) -> String {
            if pid == 0 {
                return "System".to_string();
            }
            if let Some(name) = self.process_names.lock_unpoisoned().get(&pid).cloned() {
                return name;
            }
            let name = query_process_name(pid).unwrap_or_else(|| format!("PID {pid}"));
            self.process_names
                .lock_unpoisoned()
                .insert(pid, name.clone());
            name
        }

        fn read_tcp4(&mut self) {
            for row in read_table::<MIB_TCPROW_OWNER_PID>(|buffer, size| unsafe {
                GetExtendedTcpTable(buffer, size, 0, AF_INET as u32, TCP_TABLE_OWNER_PID_ALL, 0)
            }) {
                if row.dwRemotePort == 0 {
                    continue;
                }
                let key = FlowKey {
                    protocol: Protocol::Tcp,
                    local_address: IpAddr::V4(Ipv4Addr::from(row.dwLocalAddr.to_ne_bytes())),
                    local_port: port_from_dword(row.dwLocalPort),
                    remote_address: IpAddr::V4(Ipv4Addr::from(row.dwRemoteAddr.to_ne_bytes())),
                    remote_port: port_from_dword(row.dwRemotePort),
                };
                self.tcp.insert(
                    key.clone(),
                    EndpointOwner {
                        key,
                        pid: row.dwOwningPid,
                        state: tcp_state(row.dwState).to_string(),
                    },
                );
            }
        }

        fn read_tcp6(&mut self) {
            for row in read_table::<MIB_TCP6ROW_OWNER_PID>(|buffer, size| unsafe {
                GetExtendedTcpTable(buffer, size, 0, AF_INET6 as u32, TCP_TABLE_OWNER_PID_ALL, 0)
            }) {
                if row.dwRemotePort == 0 {
                    continue;
                }
                let key = FlowKey {
                    protocol: Protocol::Tcp,
                    local_address: IpAddr::V6(Ipv6Addr::from(row.ucLocalAddr)),
                    local_port: port_from_dword(row.dwLocalPort),
                    remote_address: IpAddr::V6(Ipv6Addr::from(row.ucRemoteAddr)),
                    remote_port: port_from_dword(row.dwRemotePort),
                };
                self.tcp.insert(
                    key.clone(),
                    EndpointOwner {
                        key,
                        pid: row.dwOwningPid,
                        state: tcp_state(row.dwState).to_string(),
                    },
                );
            }
        }

        fn read_udp4(&mut self) {
            for row in read_table::<MIB_UDPROW_OWNER_PID>(|buffer, size| unsafe {
                GetExtendedUdpTable(buffer, size, 0, AF_INET as u32, UDP_TABLE_OWNER_PID, 0)
            }) {
                self.udp.insert(
                    (
                        IpAddr::V4(Ipv4Addr::from(row.dwLocalAddr.to_ne_bytes())),
                        port_from_dword(row.dwLocalPort),
                    ),
                    row.dwOwningPid,
                );
            }
        }

        fn read_udp6(&mut self) {
            for row in read_table::<MIB_UDP6ROW_OWNER_PID>(|buffer, size| unsafe {
                GetExtendedUdpTable(buffer, size, 0, AF_INET6 as u32, UDP_TABLE_OWNER_PID, 0)
            }) {
                self.udp.insert(
                    (
                        IpAddr::V6(Ipv6Addr::from(row.ucLocalAddr)),
                        port_from_dword(row.dwLocalPort),
                    ),
                    row.dwOwningPid,
                );
            }
        }
    }

    fn seed_tcp_connections(monitor: &TrafficMonitor, owners: &EndpointIndex) {
        let active = owners.tcp.keys().cloned().collect::<HashSet<_>>();
        monitor.mark_missing_tcp_flows(&active);
        for endpoint in owners.tcp.values() {
            monitor.seed_flow(endpoint, owners.process_name(endpoint.pid));
        }
    }

    fn read_table<T: Copy>(call: impl Fn(*mut c_void, &mut u32) -> u32) -> Vec<T> {
        let mut size = 0u32;
        let first = call(std::ptr::null_mut(), &mut size);
        if first != ERROR_INSUFFICIENT_BUFFER || size < 4 {
            return Vec::new();
        }
        let mut buffer = vec![0u8; size as usize];
        let result = call(buffer.as_mut_ptr().cast(), &mut size);
        if result != 0 && result != ERROR_NO_DATA {
            return Vec::new();
        }
        let count = u32::from_ne_bytes(buffer[0..4].try_into().unwrap_or_default()) as usize;
        let row_size = std::mem::size_of::<T>();
        if row_size == 0 || 4 + count.saturating_mul(row_size) > buffer.len() {
            return Vec::new();
        }
        (0..count)
            .map(|index| unsafe {
                std::ptr::read_unaligned(buffer.as_ptr().add(4 + index * row_size).cast::<T>())
            })
            .collect()
    }

    fn port_from_dword(value: u32) -> u16 {
        u16::from_be(value as u16)
    }

    fn tcp_state(state: u32) -> &'static str {
        match state {
            2 => "listen",
            3 => "syn-sent",
            4 => "syn-received",
            5 => "established",
            6 => "fin-wait-1",
            7 => "fin-wait-2",
            8 => "close-wait",
            9 => "closing",
            10 => "last-ack",
            11 => "time-wait",
            12 => "delete-tcb",
            _ => "closed",
        }
    }

    fn query_process_name(pid: u32) -> Option<String> {
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if handle.is_null() {
            return None;
        }
        let mut buffer = vec![0u16; 32_768];
        let mut length = buffer.len() as u32;
        let ok = unsafe { QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut length) };
        unsafe { CloseHandle(handle) };
        if ok == 0 || length == 0 {
            return None;
        }
        let full = String::from_utf16_lossy(&buffer[..length as usize]);
        Some(
            Path::new(&full)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(&full)
                .to_string(),
        )
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn bundled_windivert_exports_monitoring_api() {
            let dll = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("binaries")
                .join("bin")
                .join("WinDivert.dll");
            if !dll.is_file() {
                eprintln!(
                    "skipping bundled WinDivert export check: {} is not available",
                    dll.display()
                );
                return;
            }
            let api = unsafe { WinDivertApi::load(&dll) };
            assert!(api.is_ok(), "{}", api.err().unwrap_or_default());
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use super::*;

    pub fn capture_loop(_monitor: &Arc<TrafficMonitor>, _dll_path: &Path) -> Result<(), String> {
        Err("Traffic monitoring is only available on Windows".to_string())
    }

    pub fn shutdown_capture(_handle: isize, _dll_path: &Path) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_packet(remote_port: u16, outbound: bool) -> ParsedPacket<'static> {
        ParsedPacket {
            key: FlowKey {
                protocol: Protocol::Tcp,
                local_address: "192.168.1.10".parse().unwrap(),
                local_port: 50_000,
                remote_address: "1.1.1.1".parse().unwrap(),
                remote_port,
            },
            outbound,
            bytes: 100,
            tcp_flags: 0,
            sequence: 0,
            payload: &[],
            interface_route: Route::Direct,
        }
    }

    #[test]
    fn game_filter_does_not_match_browser_ephemeral_ports() {
        let monitor = TrafficMonitor::default();
        monitor.refresh_filter(
            "--wf-tcp=80,443,1024-65535 --filter-tcp=1024-65535 --dpi-desync=fake",
            Some("GameFilter".into()),
        );
        // Same local port, different remote ports. Both packet directions must
        // agree with the connection badge and classified byte counters.
        for outbound in [true, false] {
            monitor.record_packet(test_packet(443, outbound), 10, "browser.exe".into());
            monitor.record_packet(test_packet(22, outbound), 10, "ssh.exe".into());
            monitor.record_packet(test_packet(30_000, outbound), 11, "game.exe".into());
        }
        let snapshot = monitor.snapshot();
        assert_eq!(snapshot.upload_bytes, 300);
        assert_eq!(snapshot.download_bytes, 300);
        assert_eq!(snapshot.zapret_upload_bytes, 100);
        assert_eq!(snapshot.zapret_download_bytes, 100);
        for connection in snapshot.connections {
            let expected = connection.remote_port == 30_000;
            assert_eq!(connection.zapret_candidate, expected);
            assert_eq!(
                connection.zapret_upload_bytes,
                if expected { 100 } else { 0 }
            );
            assert_eq!(
                connection.zapret_download_bytes,
                if expected { 100 } else { 0 }
            );
        }
        assert!(snapshot.events.iter().any(|event| event.zapret_candidate));
        assert!(snapshot
            .events
            .iter()
            .all(|event| { event.zapret_candidate == (event.remote_port == 30_000) }));
    }

    #[test]
    fn handshake_resolution_backfills_estimates_and_reused_tuples_forget_names() {
        let monitor = TrafficMonitor::default();
        monitor.refresh_filter(
            "--wf-tcp=443 --filter-tcp=443 --hostlist-domains=example.com --dpi-desync=fake",
            Some("test".into()),
        );
        let mut first = test_packet(443, true);
        first.sequence = 100;
        first.payload = b"GET / HTTP/1.1\r\nHo";
        let next = first.sequence + first.payload.len() as u32;
        monitor.record_packet(first, 10, "browser.exe".into());
        let snapshot = monitor.snapshot();
        assert_eq!(snapshot.connections[0].zapret.state, ZapretState::Unknown);
        assert_eq!(snapshot.zapret_upload_bytes, 0);
        let mut rest = test_packet(443, true);
        rest.sequence = next;
        rest.payload = b"st: example.com\r\n\r\n";
        monitor.record_packet(rest, 10, "browser.exe".into());
        let snapshot = monitor.snapshot();
        assert_eq!(
            snapshot.connections[0].hostname.as_deref(),
            Some("example.com")
        );
        assert_eq!(snapshot.connections[0].zapret.state, ZapretState::Expected);
        assert_eq!(snapshot.zapret_upload_bytes, 200);
        let mut syn = test_packet(443, true);
        syn.sequence = 10000;
        syn.tcp_flags = 2;
        monitor.record_packet(syn, 10, "browser.exe".into());
        let snapshot = monitor.snapshot();
        assert!(snapshot.connections[0].hostname.is_none());
        assert_eq!(snapshot.connections[0].zapret.state, ZapretState::Unknown);
        assert_eq!(snapshot.connections[0].upload_bytes, 100);
        assert_eq!(snapshot.zapret_upload_bytes, 200); // prior session remains in totals
    }

    #[test]
    fn late_proxy_evidence_removes_zapret_estimates_but_keeps_raw_bytes() {
        let monitor = TrafficMonitor::default();
        monitor.refresh_filter(
            "--wf-tcp=443 --filter-tcp=443 --dpi-desync=fake",
            Some("test".into()),
        );
        monitor.record_packet(test_packet(443, true), 10, "browser.exe".into());
        assert_eq!(monitor.snapshot().zapret_upload_bytes, 100);
        monitor.refresh_routes(
            vec![ProxyRoute::new(
                "192.168.1.10:50000".parse().unwrap(),
                "1.1.1.1:443".parse().unwrap(),
            )],
            None,
            None,
        );
        let snapshot = monitor.snapshot();
        assert_eq!(snapshot.zapret_upload_bytes, 0);
        assert_eq!(snapshot.upload_bytes, 100);
        assert_eq!(snapshot.connections[0].route, Route::Warp);
        assert_eq!(snapshot.connections[0].zapret.state, ZapretState::Unknown);
        assert_eq!(snapshot.connections[0].zapret.reason, "proxy");
        monitor.refresh_routes(Vec::new(), None, None);
        assert_eq!(monitor.snapshot().connections[0].route, Route::Warp);
        let mut syn = test_packet(443, true);
        syn.sequence = 20000;
        syn.tcp_flags = 2;
        monitor.record_packet(syn, 10, "browser.exe".into());
        assert_eq!(monitor.snapshot().connections[0].route, Route::Direct);
    }

    #[test]
    fn classification_requires_an_active_strategy() {
        let monitor = TrafficMonitor::default();
        monitor.refresh_filter("--wf-tcp=443 --filter-tcp=443 --dpi-desync=fake", None);
        monitor.record_packet(test_packet(443, true), 10, "browser.exe".into());
        let snapshot = monitor.snapshot();
        assert!(!snapshot.filter_active);
        assert_eq!(snapshot.upload_bytes, 100);
        assert_eq!(snapshot.zapret_upload_bytes, 0);
        assert!(!snapshot.connections[0].zapret_candidate);
    }

    #[test]
    fn filter_changes_reset_estimates_without_discarding_all_traffic() {
        let monitor = TrafficMonitor::default();
        let args = "--wf-tcp=443 --filter-tcp=443 --dpi-desync=fake";
        monitor.refresh_filter(args, Some("first".into()));
        monitor.record_packet(test_packet(443, true), 10, "browser.exe".into());
        assert_eq!(monitor.snapshot().zapret_upload_bytes, 100);
        monitor.refresh_filter(args, Some("first".into()));
        assert_eq!(monitor.snapshot().zapret_upload_bytes, 100);
        monitor.refresh_filter(
            "--wf-tcp=80 --filter-tcp=80 --dpi-desync=fake",
            Some("second".into()),
        );
        let snapshot = monitor.snapshot();
        assert_eq!(snapshot.upload_bytes, 100);
        assert_eq!(snapshot.zapret_upload_bytes, 0);
        assert_eq!(snapshot.zapret_upload_bps, 0);
        assert_eq!(snapshot.connections[0].zapret_upload_bytes, 0);
        assert!(!snapshot.connections[0].zapret_candidate);
        monitor.refresh_filter(args, Some("first".into()));
        monitor.record_packet(test_packet(443, false), 10, "browser.exe".into());
        monitor.clear_filter();
        let snapshot = monitor.snapshot();
        assert_eq!(snapshot.download_bytes, 100);
        assert_eq!(snapshot.zapret_download_bytes, 0);
        assert_eq!(snapshot.zapret_download_bps, 0);
        assert!(!snapshot.connections[0].zapret_candidate);
    }

    #[test]
    fn unobserved_l7_and_unsupported_selectors_are_not_port_matches() {
        let remote = "1.1.1.1".parse().unwrap();
        for selector in [
            "--filter-l7=discord,stun",
            "--filter-l3=ipv6",
            "--filter-tcp=~443,80",
            "--ipset-ip=1.1.1.1",
        ] {
            let args = format!("--wf-tcp=80,443 --filter-tcp=80,443 {selector}");
            let filter = CapturePortFilter::from_winws_args(&args, Some("test".into()));
            assert!(!filter.matches(Protocol::Tcp, 443, remote), "{selector}");
            assert!(!filter.matches(Protocol::Tcp, 80, remote), "{selector}");
        }
        let filter = CapturePortFilter::from_winws_args(
            "--wf-udp=19294-19344 --filter-udp=19294-19344 --filter-l7=discord,stun",
            Some("test".into()),
        );
        assert!(!filter.matches(Protocol::Udp, 19300, remote));
    }

    #[test]
    fn ipset_hits_do_not_override_hostname_conditions_or_failed_list_reads() {
        let directory = std::env::temp_dir().join(format!(
            "zapret-ui-traffic-selectors-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let list = directory.join("list with spaces.txt");
        std::fs::write(&list, "\u{feff}1.1.1.0/24\n").unwrap();
        let remote = "1.1.1.1".parse().unwrap();
        let base = format!(
            "--wf-tcp=443 --filter-tcp=443 --dpi-desync=fake --ipset=\"{}\"",
            list.display()
        );
        let filter = CapturePortFilter::from_winws_args(&base, Some("test".into()));
        assert!(filter.matches(Protocol::Tcp, 443, remote));
        let conjunction = CapturePortFilter::from_winws_args(
            &format!("{base} --hostlist-domains=example.com"),
            Some("test".into()),
        );
        let evidence = Evidence {
            hostname: Some("example.com".into()),
            protocol: Some("tls"),
        };
        assert_eq!(
            conjunction
                .classify(Protocol::Tcp, 443, remote, &evidence)
                .state,
            ZapretState::Expected
        );
        assert_eq!(
            conjunction
                .classify(Protocol::Tcp, 443, "8.8.8.8".parse().unwrap(), &evidence)
                .state,
            ZapretState::NotMatched
        );
        for selector in [
            "--hostlist-domains=example.com",
            "--hostlist-auto=auto.txt",
            "--filter-l7=tls",
        ] {
            let filter = CapturePortFilter::from_winws_args(
                &format!("{base} {selector}"),
                Some("test".into()),
            );
            assert!(!filter.matches(Protocol::Tcp, 443, remote), "{selector}");
        }
        let missing = directory.join("missing.txt");
        for selector in ["--ipset", "--ipset-exclude"] {
            let filter = CapturePortFilter::from_winws_args(
                &format!(
                    "--wf-tcp=443 --filter-tcp=443 --dpi-desync=fake {selector}=\"{}\"",
                    missing.display()
                ),
                Some("test".into()),
            );
            assert!(!filter.matches(Protocol::Tcp, 443, remote), "{selector}");
        }
        // Empty IPSet is the intentional 'any' setting; a broken list is not.
        std::fs::write(&list, "").unwrap();
        let filter = CapturePortFilter::from_winws_args(&base, Some("test".into()));
        assert!(filter.matches(Protocol::Tcp, 443, remote));
        for content in ["not an IP", "1.1.1.0/99", "2001:db8::/129"] {
            std::fs::write(&list, content).unwrap();
            let filter = CapturePortFilter::from_winws_args(&base, Some("test".into()));
            assert!(!filter.matches(Protocol::Tcp, 443, remote), "{content}");
        }
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn parses_winws_port_filters() {
        let filter = CapturePortFilter::from_winws_args(
            "--wf-tcp=80,443,2053 --wf-udp=443,50000-50100 --filter-tcp=80,443,2053 --dpi-desync=fake --new --filter-udp=443,50000-50100 --dpi-desync=fake",
            Some("general".to_string()),
        );
        let remote = "1.1.1.1".parse().unwrap();
        assert!(filter.matches(Protocol::Tcp, 443, remote));
        assert!(filter.matches(Protocol::Udp, 50_050, remote));
        assert!(!filter.matches(Protocol::Udp, 30_000, remote));
        assert!(!filter.matches(Protocol::Tcp, 22, remote));
        assert!(filter.active());
    }

    #[test]
    fn applies_ipset_and_exclusion_to_profile_match() {
        let directory =
            std::env::temp_dir().join(format!("zapret-ui-traffic-filter-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let include = directory.join("include list.txt");
        let exclude = directory.join("exclude list.txt");
        std::fs::write(&include, "1.1.1.0/24\n2001:db8::/32\n").unwrap();
        std::fs::write(&exclude, "1.1.1.2/32\n").unwrap();
        let args = format!(
            "--wf-tcp=443 --filter-tcp=443 --dpi-desync=fake --ipset=\"{}\" --ipset-exclude=\"{}\"",
            include.display(),
            exclude.display()
        );
        let filter = CapturePortFilter::from_winws_args(&args, Some("test".to_string()));

        assert!(filter.matches(Protocol::Tcp, 443, "1.1.1.1".parse().unwrap()));
        assert!(!filter.matches(Protocol::Tcp, 443, "1.1.1.2".parse().unwrap()));
        assert!(!filter.matches(Protocol::Tcp, 443, "8.8.8.8".parse().unwrap()));
        assert!(filter.matches(Protocol::Tcp, 443, "2001:db8::1".parse().unwrap()));

        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn loads_bundled_flowseal_ipset() {
        let lists = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("binaries")
            .join("lists");
        let include = lists.join("ipset-all.txt");
        let exclude = lists.join("ipset-exclude.txt");
        if !include.is_file() || !exclude.is_file() {
            eprintln!(
                "skipping bundled Flowseal IPSet check: runtime lists are not available in {}",
                lists.display()
            );
            return;
        }
        let args = format!(
            "--wf-tcp=443 --filter-tcp=443 --dpi-desync=fake --ipset=\"{}\" --ipset-exclude=\"{}\"",
            include.display(),
            exclude.display()
        );
        let filter = CapturePortFilter::from_winws_args(&args, Some("test".to_string()));

        assert!(filter.matches(Protocol::Tcp, 443, "1.1.1.1".parse().unwrap()));
        assert!(!filter.matches(Protocol::Tcp, 443, "10.0.0.1".parse().unwrap()));
    }

    #[test]
    fn parses_ipv4_tcp_packet_direction() {
        let mut packet = vec![0u8; 40];
        packet[0] = 0x45;
        packet[2..4].copy_from_slice(&(40u16).to_be_bytes());
        packet[9] = 6;
        packet[12..16].copy_from_slice(&[192, 168, 1, 10]);
        packet[16..20].copy_from_slice(&[1, 1, 1, 1]);
        packet[20..22].copy_from_slice(&(50_000u16).to_be_bytes());
        packet[22..24].copy_from_slice(&(443u16).to_be_bytes());
        packet[32] = 0x50;
        packet[33] = 0x02;

        let parsed = parse_packet(&packet, true).expect("packet");
        assert_eq!(parsed.key.local_address.to_string(), "192.168.1.10");
        assert_eq!(parsed.key.remote_address.to_string(), "1.1.1.1");
        assert_eq!(parsed.key.local_port, 50_000);
        assert_eq!(parsed.key.remote_port, 443);
        assert_eq!(parsed.bytes, 40);
        assert_eq!(parsed.event_type(true), Some("connecting"));
    }
}
