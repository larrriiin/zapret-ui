//! Transparent, per-process TCP relay using the existing WinDivert driver.
//! Packet reflection uses the documented WinDivert NETWORK reinjection API;
//! no application injection, TLS interception, DNS rewriting or UDP capture.
use std::{
    collections::HashMap,
    ffi::{c_void, CString},
    io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, FreeLibrary, HANDLE, HMODULE, INVALID_HANDLE_VALUE},
    NetworkManagement::IpHelper::{
        GetExtendedTcpTable, MIB_TCP6ROW_OWNER_PID, MIB_TCPROW_OWNER_PID, TCP_TABLE_OWNER_PID_ALL,
    },
    Networking::WinSock::{AF_INET, AF_INET6},
    System::{
        LibraryLoader::{
            GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR,
            LOAD_LIBRARY_SEARCH_SYSTEM32,
        },
        Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION},
    },
};

const MAX_FLOWS: usize = 4096;
const MAX_RELAYS: usize = 256;
type Open = unsafe extern "system" fn(*const i8, i32, i16, u64) -> HANDLE;
type Recv = unsafe extern "system" fn(HANDLE, *mut c_void, u32, *mut u32, *mut c_void) -> i32;
type Send = unsafe extern "system" fn(HANDLE, *const c_void, u32, *mut u32, *const c_void) -> i32;
type Shutdown = unsafe extern "system" fn(HANDLE, i32) -> i32;
type Close = unsafe extern "system" fn(HANDLE) -> i32;
type Checksums = unsafe extern "system" fn(*mut c_void, u32, *mut c_void, u64) -> i32;

struct Divert {
    module: HMODULE,
    handle: HANDLE,
    recv: Recv,
    send: Send,
    shutdown: Shutdown,
    close: Close,
    checksums: Checksums,
    closed: Mutex<bool>,
}
// WinDivert explicitly supports concurrent receive/send/shutdown on one handle.
unsafe impl std::marker::Send for Divert {}
unsafe impl Sync for Divert {}
impl Drop for Divert {
    fn drop(&mut self) {
        self.finish();
        unsafe {
            FreeLibrary(self.module);
        }
    }
}
impl Divert {
    fn finish(&self) {
        let mut closed = self.closed.lock().unwrap_or_else(|e| e.into_inner());
        if !*closed {
            unsafe {
                (self.close)(self.handle);
            }
            *closed = true;
        }
    }
    fn stop_receiving(&self) {
        let closed = self.closed.lock().unwrap_or_else(|e| e.into_inner());
        if !*closed {
            unsafe {
                (self.shutdown)(self.handle, 1);
            }
        }
    }
    fn open(path: &Path) -> io::Result<Self> {
        use std::os::windows::ffi::OsStrExt;
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        unsafe {
            let module = LoadLibraryExW(
                wide.as_ptr(),
                std::ptr::null_mut(),
                LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32,
            );
            if module.is_null() {
                return Err(io::Error::last_os_error());
            }
            macro_rules! symbol {
                ($name:literal, $ty:ty) => {
                    match GetProcAddress(module, concat!($name, "\0").as_ptr()) {
                        Some(proc) => {
                            std::mem::transmute::<unsafe extern "system" fn() -> isize, $ty>(proc)
                        }
                        None => {
                            FreeLibrary(module);
                            return Err(io::Error::other(concat!("Missing ", $name)));
                        }
                    }
                };
            }
            let open = symbol!("WinDivertOpen", Open);
            let recv = symbol!("WinDivertRecv", Recv);
            let send = symbol!("WinDivertSend", Send);
            let shutdown = symbol!("WinDivertShutdown", Shutdown);
            let close = symbol!("WinDivertClose", Close);
            let checksums = symbol!("WinDivertHelperCalcChecksums", Checksums);
            let filter = CString::new("outbound and tcp and !loopback and !impostor").unwrap();
            let handle = open(filter.as_ptr(), 0, 29000, 0);
            if handle == INVALID_HANDLE_VALUE {
                let e = io::Error::last_os_error();
                FreeLibrary(module);
                return Err(e);
            }
            Ok(Self {
                module,
                handle,
                recv,
                send,
                shutdown,
                close,
                checksums,
                closed: Mutex::new(false),
            })
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Flow {
    local: SocketAddr,
    remote: SocketAddr,
}
#[derive(Clone)]
struct Mapping {
    flow: Flow,
    rule: String,
    alias: u16,
    listener: u16,
    created: Instant,
    closed: Option<Instant>,
    accepted: bool,
    failure: Option<String>,
}
#[derive(Default)]
struct Table {
    flows: HashMap<Flow, Mapping>,
    reverse: HashMap<Flow, Flow>,
    next: u16,
}
impl Table {
    fn insert(&mut self, flow: Flow, listener: u16, rule: String) -> Option<Mapping> {
        if self.flows.len() >= MAX_FLOWS {
            return None;
        }
        for _ in 1024..65535 {
            self.next = if self.next < 1024 || self.next == u16::MAX {
                1024
            } else {
                self.next + 1
            };
            let reverse = Flow {
                local: SocketAddr::new(flow.local.ip(), listener),
                remote: SocketAddr::new(flow.remote.ip(), self.next),
            };
            if self.reverse.contains_key(&reverse) {
                continue;
            }
            let mapping = Mapping {
                flow: flow.clone(),
                rule,
                alias: self.next,
                listener,
                created: Instant::now(),
                closed: None,
                accepted: false,
                failure: None,
            };
            self.reverse.insert(reverse, flow.clone());
            self.flows.insert(flow, mapping.clone());
            return Some(mapping);
        }
        None
    }
    fn prune(&mut self) {
        self.flows.retain(|_, m| {
            m.closed.map_or(
                m.accepted || m.created.elapsed() < Duration::from_secs(30),
                |closed| closed.elapsed() < Duration::from_secs(120),
            )
        });
        self.reverse.retain(|_, flow| self.flows.contains_key(flow));
    }
}

pub struct Shared {
    stop: AtomicBool,
    table: Mutex<Table>,
    pub active: AtomicUsize,
    pub total: AtomicU64,
    pub failures: AtomicU64,
    pub error: Mutex<Option<String>>,
}
impl Shared {
    fn new() -> Self {
        Self {
            stop: AtomicBool::new(false),
            table: Mutex::new(Table::default()),
            active: AtomicUsize::new(0),
            total: AtomicU64::new(0),
            failures: AtomicU64::new(0),
            error: Mutex::new(None),
        }
    }
    fn fail(&self, error: impl ToString) {
        *self.error.lock().unwrap_or_else(|e| e.into_inner()) = Some(error.to_string());
    }
    pub fn connections(&self) -> Vec<super::super::WarpConnection> {
        let mut table = self.table.lock().unwrap_or_else(|e| e.into_inner());
        table.prune();
        table
            .flows
            .values()
            .filter(|mapping| {
                (mapping.accepted && mapping.closed.is_none()) || mapping.failure.is_some()
            })
            .map(|mapping| super::super::WarpConnection {
                id: format!("application:{}:{}", mapping.flow.local, mapping.flow.remote),
                source: "application".into(),
                rule: mapping.rule.clone(),
                target: mapping.flow.remote.to_string(),
                state: if mapping.failure.is_some() {
                    "failed"
                } else {
                    "active"
                }
                .into(),
                error: mapping.failure.clone(),
            })
            .collect()
    }
}
pub struct Engine {
    pub shared: Arc<Shared>,
    driver: Arc<Divert>,
    packet_thread: Option<thread::JoinHandle<()>>,
    relay_thread: Option<thread::JoinHandle<()>>,
    pub port: u16,
}
impl Engine {
    pub fn start(paths: Vec<String>, port: u16, dll: &Path) -> io::Result<Self> {
        // Both listeners are bound before interception starts. v6-only avoids
        // ambiguous mapped-v4 peer addresses in the reverse NAT table.
        let bind = |ipv6| -> io::Result<std::net::TcpListener> {
            let socket = socket2::Socket::new(
                if ipv6 {
                    socket2::Domain::IPV6
                } else {
                    socket2::Domain::IPV4
                },
                socket2::Type::STREAM,
                Some(socket2::Protocol::TCP),
            )?;
            if ipv6 {
                socket.set_only_v6(true)?;
            }
            socket.bind(
                &SocketAddr::new(
                    if ipv6 {
                        Ipv6Addr::UNSPECIFIED.into()
                    } else {
                        Ipv4Addr::UNSPECIFIED.into()
                    },
                    0,
                )
                .into(),
            )?;
            socket.listen(128)?;
            socket.set_nonblocking(true)?;
            Ok(socket.into())
        };
        let v4 = bind(false)?;
        let v6 = bind(true)?;
        let ports = (v4.local_addr()?.port(), v6.local_addr()?.port());
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let driver = Arc::new(Divert::open(dll)?);
        let shared = Arc::new(Shared::new());
        let relay_shared = shared.clone();
        let relay_thread =
            thread::Builder::new()
                .name("warp-tcp-relay".into())
                .spawn(move || {
                    runtime.block_on(async move {
                        let v4 = match TcpListener::from_std(v4) {
                            Ok(l) => l,
                            Err(e) => {
                                relay_shared.fail(e);
                                relay_shared.stop.store(true, Ordering::Release);
                                return;
                            }
                        };
                        let v6 = match TcpListener::from_std(v6) {
                            Ok(l) => l,
                            Err(e) => {
                                relay_shared.fail(e);
                                relay_shared.stop.store(true, Ordering::Release);
                                return;
                            }
                        };
                        let a = tokio::spawn(accept(v4, relay_shared.clone(), port));
                        let b = tokio::spawn(accept(v6, relay_shared.clone(), port));
                        while !relay_shared.stop.load(Ordering::Acquire)
                            && !a.is_finished()
                            && !b.is_finished()
                        {
                            tokio::time::sleep(Duration::from_millis(50)).await;
                        }
                        if !relay_shared.stop.load(Ordering::Acquire) {
                            relay_shared.fail("TCP listener stopped");
                            relay_shared.stop.store(true, Ordering::Release);
                        }
                        a.abort();
                        b.abort();
                        // Dropping this private runtime cancels all owned relay tasks and
                        // closes their sockets. No detached workers survive disable/exit.
                    });
                })?;
        let worker_driver = driver.clone();
        let worker_shared = shared.clone();
        let packet_thread = match thread::Builder::new()
            .name("warp-tcp-packets".into())
            .spawn(move || capture(worker_driver, worker_shared, paths, ports))
        {
            Ok(t) => t,
            Err(e) => {
                shared.stop.store(true, Ordering::Release);
                let _ = relay_thread.join();
                return Err(e);
            }
        };
        Ok(Self {
            shared,
            driver,
            packet_thread: Some(packet_thread),
            relay_thread: Some(relay_thread),
            port,
        })
    }
    pub fn running(&self) -> bool {
        !self.shared.stop.load(Ordering::Acquire)
            && self
                .packet_thread
                .as_ref()
                .is_some_and(|t| !t.is_finished())
    }
}
impl Drop for Engine {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        self.driver.stop_receiving();
        if let Some(worker) = self.relay_thread.take() {
            let _ = worker.join();
        }
        if let Some(worker) = self.packet_thread.take() {
            let _ = worker.join();
        }
        // Last Arc closes the WinDivert handle, releasing interception.
    }
}

struct RelayGuard {
    shared: Arc<Shared>,
    flow: Flow,
}
impl Drop for RelayGuard {
    fn drop(&mut self) {
        self.shared.active.fetch_sub(1, Ordering::Relaxed);
        if let Some(m) = self
            .shared
            .table
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .flows
            .get_mut(&self.flow)
        {
            m.closed = Some(Instant::now());
        }
    }
}
async fn accept(listener: TcpListener, shared: Arc<Shared>, port: u16) {
    while let Ok((client, peer)) = listener.accept().await {
        let Ok(local) = client.local_addr() else {
            continue;
        };
        let flow = {
            let mut table = shared.table.lock().unwrap_or_else(|e| e.into_inner());
            let reverse = Flow {
                local,
                remote: peer,
            };
            let Some(flow) = table.reverse.get(&reverse).cloned() else {
                continue;
            };
            let Some(mapping) = table.flows.get_mut(&flow) else {
                continue;
            };
            if mapping.accepted || mapping.closed.is_some() {
                continue;
            }
            mapping.accepted = true;
            flow
        };
        if shared.active.fetch_add(1, Ordering::Relaxed) >= MAX_RELAYS {
            shared.active.fetch_sub(1, Ordering::Relaxed);
            shared.failures.fetch_add(1, Ordering::Relaxed);
            if let Some(m) = shared
                .table
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .flows
                .get_mut(&flow)
            {
                m.closed = Some(Instant::now());
                m.failure = Some("Too many simultaneous WARP connections".into());
            }
            continue;
        }
        let shared = shared.clone();
        tokio::spawn(async move {
            let _guard = RelayGuard {
                shared: shared.clone(),
                flow: flow.clone(),
            };
            let result = async {
                let mut upstream =
                    tokio::time::timeout(Duration::from_secs(10), socks_connect(port, flow.remote))
                        .await
                        .map_err(|_| {
                            io::Error::new(
                                io::ErrorKind::TimedOut,
                                "WARP SOCKS5 handshake timed out",
                            )
                        })??;
                let mut client = client;
                shared.total.fetch_add(1, Ordering::Relaxed);
                if let Err(error) = tokio::io::copy_bidirectional(&mut client, &mut upstream).await
                {
                    if !super::super::connection_closed_normally(&error) {
                        return Err(error);
                    }
                }
                Ok::<(), io::Error>(())
            }
            .await;
            if let Err(e) = result {
                let message = e.to_string();
                shared.failures.fetch_add(1, Ordering::Relaxed);
                shared.fail(&message);
                if let Some(mapping) = shared
                    .table
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .flows
                    .get_mut(&flow)
                {
                    mapping.failure = Some(message);
                }
            }
        });
    }
}

async fn socks_connect(port: u16, target: SocketAddr) -> io::Result<TcpStream> {
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).await?;
    stream.write_all(&[5, 1, 0]).await?;
    let mut greeting = [0; 2];
    stream.read_exact(&mut greeting).await?;
    if greeting != [5, 0] {
        return Err(io::Error::other(
            "WARP SOCKS5 authentication was not accepted",
        ));
    }
    let mut request = vec![5, 1, 0];
    match target.ip() {
        IpAddr::V4(ip) => {
            request.push(1);
            request.extend(ip.octets());
        }
        IpAddr::V6(ip) => {
            request.push(4);
            request.extend(ip.octets());
        }
    }
    request.extend(target.port().to_be_bytes());
    stream.write_all(&request).await?;
    let mut header = [0; 4];
    stream.read_exact(&mut header).await?;
    if header[0] != 5 || header[1] != 0 || header[2] != 0 {
        return Err(io::Error::other(format!(
            "WARP SOCKS5 CONNECT failed ({})",
            header[1]
        )));
    }
    let length = match header[3] {
        1 => 4,
        4 => 16,
        3 => stream.read_u8().await? as usize,
        _ => return Err(io::Error::other("Invalid SOCKS5 address type")),
    };
    let mut address = vec![0; length + 2];
    stream.read_exact(&mut address).await?;
    Ok(stream)
}

#[derive(Debug)]
struct Packet {
    flow: Flow,
    tcp: usize,
    ipv6: bool,
    syn: bool,
}
fn parse(packet: &[u8]) -> Option<Packet> {
    let (local, remote, mut offset, mut protocol, ipv6, end) = match packet.first()? >> 4 {
        4 if packet.len() >= 20 => {
            let offset = (packet[0] as usize & 15) * 4;
            let end = u16::from_be_bytes([packet[2], packet[3]]) as usize;
            if offset < 20
                || end > packet.len()
                || u16::from_be_bytes([packet[6], packet[7]]) & 0x3fff != 0
            {
                return None;
            }
            (
                Ipv4Addr::new(packet[12], packet[13], packet[14], packet[15]).into(),
                Ipv4Addr::new(packet[16], packet[17], packet[18], packet[19]).into(),
                offset,
                packet[9],
                false,
                end,
            )
        }
        6 if packet.len() >= 40 => {
            let end = 40 + u16::from_be_bytes([packet[4], packet[5]]) as usize;
            if end > packet.len() {
                return None;
            }
            (
                Ipv6Addr::from(<[u8; 16]>::try_from(&packet[8..24]).ok()?).into(),
                Ipv6Addr::from(<[u8; 16]>::try_from(&packet[24..40]).ok()?).into(),
                40,
                packet[6],
                true,
                end,
            )
        }
        _ => return None,
    };
    for _ in 0..8 {
        if protocol == 6 {
            break;
        }
        if !ipv6 || offset + 2 > end {
            return None;
        }
        let size = match protocol {
            0 | 43 | 60 => (packet[offset + 1] as usize + 1) * 8,
            _ => return None,
        };
        protocol = packet[offset];
        offset += size;
    }
    if protocol != 6
        || offset + 20 > end
        || (packet[offset + 12] >> 4) < 5
        || offset + ((packet[offset + 12] >> 4) as usize * 4) > end
    {
        return None;
    }
    Some(Packet {
        flow: Flow {
            local: SocketAddr::new(
                local,
                u16::from_be_bytes([packet[offset], packet[offset + 1]]),
            ),
            remote: SocketAddr::new(
                remote,
                u16::from_be_bytes([packet[offset + 2], packet[offset + 3]]),
            ),
        },
        tcp: offset,
        ipv6,
        syn: packet[offset + 13] & 0x12 == 2,
    })
}

fn reflect(packet: &mut [u8], parsed: &Packet, mapping: &Mapping, reply: bool) {
    if parsed.ipv6 {
        for i in 0..16 {
            packet.swap(8 + i, 24 + i);
        }
    } else {
        for i in 0..4 {
            packet.swap(12 + i, 16 + i);
        }
    }
    let (source, dest) = if reply {
        (mapping.flow.remote.port(), mapping.flow.local.port())
    } else {
        (mapping.alias, mapping.listener)
    };
    packet[parsed.tcp..parsed.tcp + 2].copy_from_slice(&source.to_be_bytes());
    packet[parsed.tcp + 2..parsed.tcp + 4].copy_from_slice(&dest.to_be_bytes());
}
fn bypass(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => {
            v.is_loopback()
                || v.is_private()
                || v.is_link_local()
                || v.is_multicast()
                || v.is_unspecified()
                || v.is_broadcast()
        }
        IpAddr::V6(v) => {
            v.is_loopback()
                || v.is_unicast_link_local()
                || v.is_unique_local()
                || v.is_multicast()
                || v.is_unspecified()
        }
    }
}
pub fn path_key(path: &str) -> String {
    path.trim_start_matches("\\\\?\\")
        .replace('/', "\\")
        .to_lowercase()
}

fn capture(driver: Arc<Divert>, shared: Arc<Shared>, paths: Vec<String>, ports: (u16, u16)) {
    let paths: Vec<(String, String)> = paths
        .into_iter()
        .map(|path| (path_key(&path), path))
        .collect();
    let mut buffer = vec![0; 65575];
    let mut address = [0u64; 10];
    let mut prune = Instant::now();
    loop {
        let mut len = 0;
        if unsafe {
            (driver.recv)(
                driver.handle,
                buffer.as_mut_ptr().cast(),
                buffer.len() as u32,
                &mut len,
                address.as_mut_ptr().cast(),
            )
        } == 0
        {
            if !shared.stop.load(Ordering::Acquire) {
                shared.fail(io::Error::last_os_error());
            }
            break;
        }
        let packet = &mut buffer[..len as usize];
        let mut changed = false;
        let mut drop_packet = false;
        if !shared.stop.load(Ordering::Acquire) {
            if let Some(parsed) = parse(packet) {
                let (existing, reply) = {
                    let mut table = shared.table.lock().unwrap_or_else(|e| e.into_inner());
                    if prune.elapsed() > Duration::from_secs(5) {
                        table.prune();
                        prune = Instant::now();
                    }
                    if let Some(original) = table.reverse.get(&parsed.flow) {
                        (table.flows.get(original).cloned(), true)
                    } else {
                        (table.flows.get(&parsed.flow).cloned(), false)
                    }
                };
                let mapping = if existing.is_some() {
                    existing
                } else if parsed.syn
                    && !bypass(parsed.flow.remote.ip())
                    && parsed.flow.local.ip() != parsed.flow.remote.ip()
                {
                    if let Some(rule) = selected(&parsed.flow, &paths) {
                        let mapping = shared
                            .table
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .insert(
                                parsed.flow.clone(),
                                if parsed.ipv6 { ports.1 } else { ports.0 },
                                rule,
                            );
                        if mapping.is_none() {
                            drop_packet = true;
                            shared.failures.fetch_add(1, Ordering::Relaxed);
                        }
                        mapping
                    } else {
                        None
                    }
                } else {
                    None
                };
                if let Some(mapping) = mapping {
                    reflect(packet, &parsed, &mapping, reply);
                    address[1] &= !(1 << 17);
                    changed = true;
                }
            }
        }
        if drop_packet {
            continue;
        }
        if changed {
            unsafe {
                (driver.checksums)(
                    packet.as_mut_ptr().cast(),
                    len,
                    address.as_mut_ptr().cast(),
                    0,
                );
            }
        }
        if unsafe {
            (driver.send)(
                driver.handle,
                packet.as_ptr().cast(),
                len,
                std::ptr::null_mut(),
                address.as_ptr().cast(),
            )
        } == 0
        {
            shared.fail(io::Error::last_os_error());
            break;
        }
    }
    shared.stop.store(true, Ordering::Release);
    // Release interception immediately on failure, even before the next UI poll.
    driver.finish();
}

fn rows<T: Copy>(family: u32) -> Vec<T> {
    let mut size = 0;
    unsafe {
        GetExtendedTcpTable(
            std::ptr::null_mut(),
            &mut size,
            0,
            family,
            TCP_TABLE_OWNER_PID_ALL,
            0,
        );
    }
    for _ in 0..3 {
        if !(4..=16 * 1024 * 1024).contains(&size) {
            return vec![];
        }
        let mut bytes = vec![0u8; size as usize];
        let result = unsafe {
            GetExtendedTcpTable(
                bytes.as_mut_ptr().cast(),
                &mut size,
                0,
                family,
                TCP_TABLE_OWNER_PID_ALL,
                0,
            )
        };
        if result == 122 {
            continue;
        }
        if result != 0 {
            return vec![];
        }
        let count = u32::from_ne_bytes(bytes[..4].try_into().unwrap()) as usize;
        if count > (bytes.len() - 4) / std::mem::size_of::<T>() {
            return vec![];
        }
        return (0..count)
            .map(|i| unsafe {
                std::ptr::read_unaligned(
                    bytes
                        .as_ptr()
                        .add(4 + i * std::mem::size_of::<T>())
                        .cast::<T>(),
                )
            })
            .collect();
    }
    vec![]
}
fn selected(flow: &Flow, paths: &[(String, String)]) -> Option<String> {
    let pid = if flow.local.is_ipv4() {
        rows::<MIB_TCPROW_OWNER_PID>(AF_INET as u32)
            .into_iter()
            .find(|r| {
                SocketAddr::new(
                    Ipv4Addr::from(r.dwLocalAddr.to_ne_bytes()).into(),
                    u16::from_be(r.dwLocalPort as u16),
                ) == flow.local
                    && SocketAddr::new(
                        Ipv4Addr::from(r.dwRemoteAddr.to_ne_bytes()).into(),
                        u16::from_be(r.dwRemotePort as u16),
                    ) == flow.remote
            })
            .map(|r| r.dwOwningPid)
    } else {
        rows::<MIB_TCP6ROW_OWNER_PID>(AF_INET6 as u32)
            .into_iter()
            .find(|r| {
                SocketAddr::new(
                    Ipv6Addr::from(r.ucLocalAddr).into(),
                    u16::from_be(r.dwLocalPort as u16),
                ) == flow.local
                    && SocketAddr::new(
                        Ipv6Addr::from(r.ucRemoteAddr).into(),
                        u16::from_be(r.dwRemotePort as u16),
                    ) == flow.remote
            })
            .map(|r| r.dwOwningPid)
    };
    let pid = pid.filter(|p| *p != 0 && *p != std::process::id())?;
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return None;
        }
        let mut buffer = vec![0u16; 32768];
        let mut size = buffer.len() as u32;
        let ok = QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut size);
        CloseHandle(handle);
        if ok == 0 {
            return None;
        }
        let key = path_key(&String::from_utf16_lossy(&buffer[..size as usize]));
        paths
            .iter()
            .find(|(candidate, _)| candidate == &key)
            .map(|(_, original)| original.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    fn packet(v6: bool, src: u16, dst: u16) -> Vec<u8> {
        let offset = if v6 { 40 } else { 20 };
        let mut bytes = vec![0; offset + 20];
        if v6 {
            bytes[0] = 0x60;
            bytes[4..6].copy_from_slice(&20u16.to_be_bytes());
            bytes[6] = 6;
            bytes[8..24].copy_from_slice(&"2001:db8::1".parse::<Ipv6Addr>().unwrap().octets());
            bytes[24..40].copy_from_slice(&"2001:db8::2".parse::<Ipv6Addr>().unwrap().octets());
        } else {
            bytes[0] = 0x45;
            bytes[2..4].copy_from_slice(&40u16.to_be_bytes());
            bytes[9] = 6;
            bytes[12..16].copy_from_slice(&[192, 0, 2, 1]);
            bytes[16..20].copy_from_slice(&[198, 51, 100, 1]);
        }
        bytes[offset..offset + 2].copy_from_slice(&src.to_be_bytes());
        bytes[offset + 2..offset + 4].copy_from_slice(&dst.to_be_bytes());
        bytes[offset + 12] = 0x50;
        bytes[offset + 13] = 2;
        bytes
    }
    #[test]
    fn reflection_and_reverse_mapping_preserve_exact_endpoints() {
        for v6 in [false, true] {
            let mut bytes = packet(v6, 50000, 443);
            let original = parse(&bytes).unwrap();
            let mut table = Table::default();
            let mapping = table
                .insert(original.flow.clone(), 40001, "client.exe".into())
                .unwrap();
            let other = table
                .insert(
                    Flow {
                        local: original.flow.local,
                        remote: SocketAddr::new(original.flow.remote.ip(), 8443),
                    },
                    40001,
                    "client.exe".into(),
                )
                .unwrap();
            assert_ne!(
                mapping.alias, other.alias,
                "same source port to different destinations cannot collide"
            );
            reflect(&mut bytes, &original, &mapping, false);
            let redirected = parse(&bytes).unwrap();
            assert_eq!(
                redirected.flow.local,
                SocketAddr::new(original.flow.remote.ip(), mapping.alias)
            );
            assert_eq!(
                redirected.flow.remote,
                SocketAddr::new(original.flow.local.ip(), 40001)
            );
            let mut reply = packet(v6, 40001, mapping.alias);
            let parsed = parse(&reply).unwrap();
            reflect(&mut reply, &parsed, &mapping, true);
            let reply = parse(&reply).unwrap();
            assert_eq!(reply.flow.local, original.flow.remote);
            assert_eq!(reply.flow.remote, original.flow.local);
        }
    }
    #[test]
    fn rejects_fragments_and_preserves_local_connections() {
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "192.168.0.1",
            "169.254.1.2",
            "::1",
            "fe80::1",
            "fc00::1",
        ] {
            assert!(bypass(ip.parse().unwrap()));
        }
        let mut bytes = packet(false, 1234, 443);
        bytes[6] = 0x20;
        assert!(parse(&bytes).is_none());
        assert!(parse(&[0x45]).is_none());
        assert_eq!(
            path_key(r"\\?\C:\Games\Client.EXE"),
            path_key("c:/games/client.exe")
        );
        assert_ne!(
            path_key(r"C:\Games\client.exe"),
            path_key(r"D:\Other\client.exe")
        );
    }

    #[test]
    #[ignore = "Child process used only by transparent_tcp_smoke"]
    fn tcp_probe_child() {
        let address = std::env::var("ZAPRET_TEST_TCP_ADDR")
            .expect("probe address")
            .parse()
            .unwrap();
        let mut stream =
            std::net::TcpStream::connect_timeout(&address, Duration::from_secs(8)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(8)))
            .unwrap();
        stream.write_all(b"ping").unwrap();
        let mut reply = [0; 4];
        stream.read_exact(&mut reply).unwrap();
        assert_eq!(&reply, b"pong");
    }

    #[test]
    #[ignore = "Requires administrator and bundled WinDivert; redirects only a unique temporary probe executable to a local mock SOCKS5 server"]
    fn transparent_tcp_smoke() {
        let directory =
            std::env::temp_dir().join(format!("zapret-tcp-probe-{}", std::process::id()));
        std::fs::create_dir(&directory).unwrap();
        let probe = directory.join("probe.exe");
        std::fs::copy(std::env::current_exe().unwrap(), &probe).unwrap();
        let listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let mock = thread::spawn(move || -> io::Result<()> {
            let deadline = Instant::now() + Duration::from_secs(12);
            let mut socket = loop {
                match listener.accept() {
                    Ok((s, _)) => break s,
                    Err(e)
                        if e.kind() == io::ErrorKind::WouldBlock && Instant::now() < deadline =>
                    {
                        thread::sleep(Duration::from_millis(20))
                    }
                    Err(e) => return Err(e),
                }
            };
            socket.set_nonblocking(false)?;
            socket.set_read_timeout(Some(Duration::from_secs(8)))?;
            let mut greeting = [0; 3];
            socket.read_exact(&mut greeting)?;
            assert_eq!(greeting, [5, 1, 0]);
            socket.write_all(&[5, 0])?;
            let mut request = [0; 10];
            socket.read_exact(&mut request)?;
            assert_eq!(&request[..8], &[5, 1, 0, 1, 198, 51, 100, 1]);
            socket.write_all(&[5, 0, 0, 1, 127, 0, 0, 1, 0, 0])?;
            let mut data = [0; 4];
            socket.read_exact(&mut data)?;
            assert_eq!(&data, b"ping");
            socket.write_all(b"pong")?;
            Ok(())
        });
        let dll = Path::new(env!("CARGO_MANIFEST_DIR")).join("../binaries/bin/WinDivert.dll");
        let engine = Engine::start(vec![probe.to_string_lossy().to_string()], port, &dll);
        let result = match engine {
            Ok(engine) => {
                let output = std::process::Command::new(&probe)
                    .args([
                        "--ignored",
                        "--exact",
                        "providers::warp::applications::tcp::tests::tcp_probe_child",
                        "--nocapture",
                    ])
                    .env("ZAPRET_TEST_TCP_ADDR", "198.51.100.1:32123")
                    .output()
                    .unwrap();
                eprintln!(
                    "relay total={} failures={} error={:?}",
                    engine.shared.total.load(Ordering::Relaxed),
                    engine.shared.failures.load(Ordering::Relaxed),
                    engine.shared.error.lock().unwrap()
                );
                drop(engine);
                output
            }
            Err(e) => {
                let _ = mock.join();
                std::fs::remove_file(&probe).unwrap();
                std::fs::remove_dir(&directory).unwrap();
                panic!("WinDivert: {e}");
            }
        };
        let proxy_result = mock.join().unwrap();
        eprintln!("mock result: {proxy_result:?}");
        std::fs::remove_file(probe).unwrap();
        std::fs::remove_dir(directory).unwrap();
        assert!(
            result.status.success(),
            "{} {}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        proxy_result.unwrap();
    }
}
