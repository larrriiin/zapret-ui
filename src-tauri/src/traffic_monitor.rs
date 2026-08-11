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

#[derive(Clone, Debug, Default)]
pub struct CapturePortFilter {
    tcp: Vec<(u16, u16)>,
    udp: Vec<(u16, u16)>,
    profiles: Vec<ProfileFilter>,
    pub strategy: Option<String>,
}

impl CapturePortFilter {
    pub fn from_winws_args(args: &str, strategy: Option<String>) -> Self {
        let mut filter = Self {
            strategy,
            ..Self::default()
        };

        let mut profile = ProfileFilter::default();
        for token in split_command_line(args) {
            let token = token.trim_matches('"');
            if let Some(value) = token.strip_prefix("--wf-tcp=") {
                filter.tcp.extend(parse_port_ranges(value));
            } else if let Some(value) = token.strip_prefix("--wf-udp=") {
                filter.udp.extend(parse_port_ranges(value));
            } else if token == "--new" {
                if profile.has_selector() {
                    filter.profiles.push(profile);
                }
                profile = ProfileFilter::default();
            } else if let Some(value) = token.strip_prefix("--filter-tcp=") {
                profile.tcp.extend(parse_port_ranges(value));
            } else if let Some(value) = token.strip_prefix("--filter-udp=") {
                profile.udp.extend(parse_port_ranges(value));
            } else if let Some(value) = token.strip_prefix("--ipset=") {
                profile.includes.merge(IpSetMatcher::from_file(value));
            } else if let Some(value) = token.strip_prefix("--ipset-exclude=") {
                profile.excludes.merge(IpSetMatcher::from_file(value));
            } else if token.starts_with("--hostlist=")
                || token.starts_with("--hostlist-domains=")
                || token.starts_with("--hostlist-auto=")
            {
                profile.requires_hostname = true;
            } else if token.starts_with("--filter-l7=") {
                profile.has_l7_filter = true;
            }
        }
        if profile.has_selector() {
            filter.profiles.push(profile);
        }
        filter
    }

    pub fn source_key(args: &str, strategy: Option<&str>) -> String {
        let mut key = format!("{}\n{args}", strategy.unwrap_or_default());
        for token in split_command_line(args) {
            let path = token
                .strip_prefix("--ipset=")
                .or_else(|| token.strip_prefix("--ipset-exclude="));
            let Some(path) = path else { continue };
            let path = Path::new(path.trim_matches('"'));
            if let Ok(metadata) = std::fs::metadata(path) {
                let modified = metadata
                    .modified()
                    .ok()
                    .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|duration| duration.as_nanos())
                    .unwrap_or_default();
                key.push_str(&format!(
                    "\n{}:{}:{modified}",
                    path.display(),
                    metadata.len()
                ));
            }
        }
        key
    }

    fn matches(
        &self,
        protocol: Protocol,
        source_port: u16,
        destination_port: u16,
        remote_address: IpAddr,
    ) -> bool {
        let ranges = match protocol {
            Protocol::Tcp => &self.tcp,
            Protocol::Udp => &self.udp,
        };
        let captured = ranges.iter().any(|(start, end)| {
            (*start..=*end).contains(&source_port) || (*start..=*end).contains(&destination_port)
        });
        captured
            && self.profiles.iter().any(|profile| {
                profile.matches(protocol, source_port, destination_port, remote_address)
            })
    }

    fn active(&self) -> bool {
        self.strategy.is_some() && (!self.tcp.is_empty() || !self.udp.is_empty())
    }
}

#[derive(Clone, Debug, Default)]
struct ProfileFilter {
    tcp: Vec<(u16, u16)>,
    udp: Vec<(u16, u16)>,
    includes: IpSetMatcher,
    excludes: IpSetMatcher,
    requires_hostname: bool,
    has_l7_filter: bool,
}

impl ProfileFilter {
    fn has_selector(&self) -> bool {
        !self.tcp.is_empty()
            || !self.udp.is_empty()
            || !self.includes.is_empty()
            || !self.excludes.is_empty()
            || self.requires_hostname
            || self.has_l7_filter
    }

    fn matches(
        &self,
        protocol: Protocol,
        source_port: u16,
        destination_port: u16,
        remote_address: IpAddr,
    ) -> bool {
        let ports = match protocol {
            Protocol::Tcp => &self.tcp,
            Protocol::Udp => &self.udp,
        };
        if ports.is_empty()
            || !ports.iter().any(|(start, end)| {
                (*start..=*end).contains(&source_port)
                    || (*start..=*end).contains(&destination_port)
            })
        {
            return false;
        }
        if self.excludes.contains(remote_address) {
            return false;
        }
        if !self.includes.is_empty() {
            return self.includes.contains(remote_address);
        }

        // A passive packet monitor cannot reliably recover SNI/HTTP Host for
        // every connection (ECH and QUIC are notable examples). Do not mark a
        // hostlist-only profile as Zapret traffic just because it uses port 443.
        if self.requires_hostname {
            return false;
        }

        // Narrow L7-only profiles (Discord/STUN) and unrestricted profiles can
        // still be classified by their protocol and port selection.
        self.has_l7_filter || (!self.tcp.is_empty() || !self.udp.is_empty())
    }
}

#[derive(Clone, Debug, Default)]
struct IpSetMatcher {
    ipv4: HashMap<u8, HashSet<u32>>,
    ipv6: HashMap<u8, HashSet<u128>>,
}

impl IpSetMatcher {
    fn from_file(value: &str) -> Self {
        let path = Path::new(value.trim_matches('"'));
        let Ok(content) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        let mut matcher = Self::default();
        for raw in content.lines() {
            let line = raw
                .split_once('#')
                .map(|(value, _)| value)
                .unwrap_or(raw)
                .trim();
            if line.is_empty() {
                continue;
            }
            let (address, prefix) = line.split_once('/').unwrap_or((line, ""));
            let Ok(address) = address.parse::<IpAddr>() else {
                continue;
            };
            match address {
                IpAddr::V4(address) => {
                    let prefix = prefix.parse::<u8>().unwrap_or(32).min(32);
                    let value = u32::from(address) & ipv4_mask(prefix);
                    matcher.ipv4.entry(prefix).or_default().insert(value);
                }
                IpAddr::V6(address) => {
                    let prefix = prefix.parse::<u8>().unwrap_or(128).min(128);
                    let value = u128::from(address) & ipv6_mask(prefix);
                    matcher.ipv6.entry(prefix).or_default().insert(value);
                }
            }
        }
        matcher
    }

    fn merge(&mut self, other: Self) {
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

fn parse_port_ranges(value: &str) -> Vec<(u16, u16)> {
    value
        .trim_matches('"')
        .split(',')
        .filter_map(|part| {
            let part = part.trim();
            if part.is_empty() {
                return None;
            }
            if let Some((start, end)) = part.split_once('-') {
                let start = start.parse::<u16>().ok()?;
                let end = end.parse::<u16>().ok()?;
                (start <= end).then_some((start, end))
            } else {
                part.parse::<u16>().ok().map(|port| (port, port))
            }
        })
        .collect()
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
}

impl FlowStats {
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
        }
    }
}

struct TrafficData {
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
}

#[derive(Serialize)]
pub struct TrafficSnapshot {
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
        }
    }
}

impl TrafficMonitor {
    pub fn refresh_filter(&self, args: &str, strategy: Option<String>) {
        let key = CapturePortFilter::source_key(args, strategy.as_deref());
        if *self.filter_key.lock_unpoisoned() == key {
            return;
        }
        let filter = CapturePortFilter::from_winws_args(args, strategy);
        *self.filter.lock_unpoisoned() = filter;
        *self.filter_key.lock_unpoisoned() = key;
    }

    pub fn clear_filter(&self) {
        *self.filter.lock_unpoisoned() = CapturePortFilter::default();
        self.filter_key.lock_unpoisoned().clear();
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
        let mut data = self.data.lock_unpoisoned();
        let elapsed = now.duration_since(data.last_sample).as_secs_f64().max(0.1);
        data.flows
            .retain(|_, flow| now.duration_since(flow.last_seen) < Duration::from_secs(90));

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

            let zapret_candidate = filter.active()
                && filter.matches(
                    key.protocol,
                    key.local_port,
                    key.remote_port,
                    key.remote_address,
                );

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
        connections.truncate(1_000);

        let events = data.events.iter().rev().take(500).cloned().collect();

        let snapshot = TrafficSnapshot {
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

    fn record_packet(&self, packet: ParsedPacket, pid: u32, process_name: String) {
        let now = Instant::now();
        let candidate = self.filter.lock_unpoisoned().matches(
            packet.key.protocol,
            packet.source_port,
            packet.destination_port,
            packet.key.remote_address,
        );
        let mut data = self.data.lock_unpoisoned();
        if packet.outbound {
            data.upload_bytes = data.upload_bytes.saturating_add(packet.bytes);
            if candidate {
                data.zapret_upload_bytes = data.zapret_upload_bytes.saturating_add(packet.bytes);
            }
        } else {
            data.download_bytes = data.download_bytes.saturating_add(packet.bytes);
            if candidate {
                data.zapret_download_bytes =
                    data.zapret_download_bytes.saturating_add(packet.bytes);
            }
        }

        let is_new = !data.flows.contains_key(&packet.key);
        let event_type = packet.event_type(is_new);
        let event_key = packet.key.clone();
        let (should_log, event_process, event_pid) = {
            let flow = data.flows.entry(packet.key).or_insert_with(|| {
                FlowStats::new(pid, process_name.clone(), "active".to_string(), now)
            });
            if flow.pid == 0 && pid != 0 {
                flow.pid = pid;
                flow.process_name = process_name;
            }
            if let Some(event) = event_type {
                flow.state = match event {
                    "connecting" => "connecting",
                    "connected" => "established",
                    "closed" => "closed",
                    "reset" => "error",
                    _ => "active",
                }
                .to_string();
            }
            flow.last_seen = now;
            if packet.outbound {
                flow.upload_bytes = flow.upload_bytes.saturating_add(packet.bytes);
                if candidate {
                    flow.zapret_upload_bytes =
                        flow.zapret_upload_bytes.saturating_add(packet.bytes);
                }
            } else {
                flow.download_bytes = flow.download_bytes.saturating_add(packet.bytes);
                if candidate {
                    flow.zapret_download_bytes =
                        flow.zapret_download_bytes.saturating_add(packet.bytes);
                }
            }
            let should_log = event_type.is_some() && flow.last_event != event_type;
            if should_log {
                flow.last_event = event_type;
            }
            (should_log, flow.process_name.clone(), flow.pid)
        };

        if should_log {
            push_event(
                &mut data,
                event_type.unwrap_or("observed"),
                &event_key,
                event_pid,
                event_process,
                candidate,
            );
        }
    }

    fn seed_flow(&self, endpoint: &EndpointOwner, process_name: String) {
        let now = Instant::now();
        let mut data = self.data.lock_unpoisoned();
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

fn push_event(
    data: &mut TrafficData,
    event_type: &'static str,
    key: &FlowKey,
    pid: u32,
    process_name: String,
    zapret_candidate: bool,
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
        zapret_candidate,
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
struct ParsedPacket {
    key: FlowKey,
    source_port: u16,
    destination_port: u16,
    outbound: bool,
    bytes: u64,
    tcp_flags: u8,
}

impl ParsedPacket {
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

fn parse_packet(packet: &[u8], outbound: bool) -> Option<ParsedPacket> {
    let version = packet.first()? >> 4;
    let (source, destination, protocol, transport_offset, bytes) = match version {
        4 if packet.len() >= 20 => {
            let header_len = ((packet[0] & 0x0f) as usize) * 4;
            if header_len < 20 || packet.len() < header_len + 4 {
                return None;
            }
            let protocol = packet[9];
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

    Some(ParsedPacket {
        key: FlowKey {
            protocol,
            local_address,
            local_port,
            remote_address,
            remote_port,
        },
        source_port,
        destination_port,
        outbound,
        bytes: bytes.min(packet.len() as u64),
        tcp_flags,
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
        GetExtendedTcpTable, GetExtendedUdpTable, MIB_TCP6ROW_OWNER_PID, MIB_TCPROW_OWNER_PID,
        MIB_UDP6ROW_OWNER_PID, MIB_UDPROW_OWNER_PID, TCP_TABLE_OWNER_PID_ALL, UDP_TABLE_OWNER_PID,
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
            if let Some(parsed) = parse_packet(&packet[..received as usize], outbound) {
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
            assert!(dll.is_file(), "missing {}", dll.display());
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

    #[test]
    fn parses_winws_port_filters() {
        let filter = CapturePortFilter::from_winws_args(
            "--wf-tcp=80,443,2053 --wf-udp=443,50000-50100 --filter-tcp=80,443,2053 --new --filter-udp=443,50000-50100",
            Some("general".to_string()),
        );
        let remote = "1.1.1.1".parse().unwrap();
        assert!(filter.matches(Protocol::Tcp, 51_000, 443, remote));
        assert!(filter.matches(Protocol::Udp, 50_050, 30_000, remote));
        assert!(!filter.matches(Protocol::Tcp, 51_000, 22, remote));
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
            "--wf-tcp=443 --filter-tcp=443 --ipset=\"{}\" --ipset-exclude=\"{}\"",
            include.display(),
            exclude.display()
        );
        let filter = CapturePortFilter::from_winws_args(&args, Some("test".to_string()));

        assert!(filter.matches(Protocol::Tcp, 50_000, 443, "1.1.1.1".parse().unwrap()));
        assert!(!filter.matches(Protocol::Tcp, 50_000, 443, "1.1.1.2".parse().unwrap()));
        assert!(!filter.matches(Protocol::Tcp, 50_000, 443, "8.8.8.8".parse().unwrap()));
        assert!(filter.matches(Protocol::Tcp, 50_000, 443, "2001:db8::1".parse().unwrap()));

        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn loads_bundled_flowseal_ipset() {
        let lists = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("binaries")
            .join("lists");
        let args = format!(
            "--wf-tcp=443 --filter-tcp=443 --ipset=\"{}\" --ipset-exclude=\"{}\"",
            lists.join("ipset-all.txt").display(),
            lists.join("ipset-exclude.txt").display()
        );
        let filter = CapturePortFilter::from_winws_args(&args, Some("test".to_string()));

        assert!(filter.matches(Protocol::Tcp, 50_000, 443, "1.1.1.1".parse().unwrap()));
        assert!(!filter.matches(Protocol::Tcp, 50_000, 443, "10.0.0.1".parse().unwrap()));
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
