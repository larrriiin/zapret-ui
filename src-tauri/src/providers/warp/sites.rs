//! Session-scoped Windows PAC routing. The recovery journal is durable before
//! changing WinINet; only settings still owned by this session are restored.
use super::{Result, WarpError};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs::{self, OpenOptions},
    io::{Read, Write},
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

static SITES: Mutex<Option<Runtime>> = Mutex::new(None);
static STOPPING: AtomicBool = AtomicBool::new(false);

fn error(detail: impl ToString) -> WarpError {
    WarpError::new("warp_sites_error", detail)
}

pub(super) fn persist_json(path: &std::path::Path, value: &impl Serialize) -> Result<()> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(error)?
        .as_nanos();
    let temporary = path.with_extension(format!("{}.{}.tmp", std::process::id(), stamp));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(error)?;
    let result = (|| {
        file.write_all(&serde_json::to_vec(value).map_err(error)?)
            .map_err(error)?;
        file.sync_all().map_err(error)?;
        drop(file);
        fs::rename(&temporary, path).map_err(error)
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SiteStatus {
    pub domains: Vec<String>,
    pub enabled: bool,
    #[serde(skip)]
    pub(crate) runtime_active: bool,
    #[serde(skip)]
    pub(crate) connections: Vec<super::WarpConnection>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct ProxySettings {
    flags: u32,
    url: String,
}

#[derive(Serialize, Deserialize)]
struct Journal {
    previous: ProxySettings,
    owned: ProxySettings,
}

struct Runtime {
    directory: PathBuf,
    domains: Vec<String>,
    desired_enabled: bool,
    server: Option<PacServer>,
}

struct PacServer {
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    url: String,
    port: u16,
    #[cfg(windows)]
    relay: Option<SiteRelay>,
}

#[cfg(windows)]
#[derive(Clone)]
struct SiteConnection {
    id: u64,
    rule: String,
    target: String,
    closed: Option<Instant>,
    failure: Option<String>,
}

#[cfg(windows)]
struct SiteRelayShared {
    stop: AtomicBool,
    next_id: AtomicU64,
    connections: Mutex<HashMap<u64, SiteConnection>>,
}

#[cfg(windows)]
struct SiteRelay {
    shared: Arc<SiteRelayShared>,
    worker: Option<JoinHandle<()>>,
    port: u16,
}
impl Drop for PacServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(windows)]
impl Drop for SiteRelay {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(windows)]
impl SiteRelay {
    fn start(domains: Vec<String>, warp_port: u16) -> Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).map_err(error)?;
        listener.set_nonblocking(true).map_err(error)?;
        let port = listener.local_addr().map_err(error)?.port();
        let shared = Arc::new(SiteRelayShared {
            stop: AtomicBool::new(false),
            next_id: AtomicU64::new(1),
            connections: Mutex::new(HashMap::new()),
        });
        let worker_shared = shared.clone();
        let worker = thread::Builder::new()
            .name("warp-sites-relay".into())
            .spawn(move || {
                let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                else {
                    return;
                };
                runtime.block_on(async move {
                    let Ok(listener) = tokio::net::TcpListener::from_std(listener) else {
                        return;
                    };
                    let mut tasks = tokio::task::JoinSet::new();
                    while !worker_shared.stop.load(Ordering::Acquire) {
                        match tokio::time::timeout(Duration::from_millis(100), listener.accept())
                            .await
                        {
                            Ok(Ok((client, _))) => {
                                tasks.spawn(handle_site_connection(
                                    client,
                                    worker_shared.clone(),
                                    domains.clone(),
                                    warp_port,
                                ));
                            }
                            Ok(Err(_)) => break,
                            Err(_) => {}
                        }
                        while tasks.try_join_next().is_some() {}
                    }
                    tasks.abort_all();
                });
            })
            .map_err(error)?;
        Ok(Self {
            shared,
            worker: Some(worker),
            port,
        })
    }

    fn connections(&self) -> Vec<super::WarpConnection> {
        let mut connections = self
            .shared
            .connections
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        connections.retain(|_, connection| {
            connection
                .closed
                .is_none_or(|closed| closed.elapsed() < Duration::from_secs(120))
        });
        connections
            .values()
            .map(|connection| super::WarpConnection {
                id: format!("site:{}", connection.id),
                source: "site".into(),
                rule: connection.rule.clone(),
                target: connection.target.clone(),
                state: if connection.failure.is_some() {
                    "failed"
                } else {
                    "active"
                }
                .into(),
                error: connection.failure.clone(),
            })
            .collect()
    }
}

#[cfg(windows)]
async fn handle_site_connection(
    mut client: tokio::net::TcpStream,
    shared: Arc<SiteRelayShared>,
    domains: Vec<String>,
    warp_port: u16,
) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let request = async {
        let version = client.read_u8().await?;
        let count = client.read_u8().await? as usize;
        if version != 5 || count == 0 || count > 32 {
            return Err(std::io::Error::other("Invalid SOCKS5 greeting"));
        }
        let mut methods = vec![0; count];
        client.read_exact(&mut methods).await?;
        if !methods.contains(&0) {
            client.write_all(&[5, 0xff]).await?;
            return Err(std::io::Error::other(
                "SOCKS5 authentication is unsupported",
            ));
        }
        client.write_all(&[5, 0]).await?;
        let mut header = [0; 4];
        client.read_exact(&mut header).await?;
        if header[..3] != [5, 1, 0] {
            return Err(std::io::Error::other("Invalid SOCKS5 CONNECT request"));
        }
        let host = match header[3] {
            1 => {
                let mut bytes = [0; 4];
                client.read_exact(&mut bytes).await?;
                IpAddr::V4(bytes.into()).to_string()
            }
            3 => {
                let length = client.read_u8().await? as usize;
                if length == 0 {
                    return Err(std::io::Error::other("Empty SOCKS5 hostname"));
                }
                let mut bytes = vec![0; length];
                client.read_exact(&mut bytes).await?;
                String::from_utf8(bytes)
                    .map_err(|_| std::io::Error::other("Invalid SOCKS5 hostname"))?
                    .to_ascii_lowercase()
            }
            4 => {
                let mut bytes = [0; 16];
                client.read_exact(&mut bytes).await?;
                IpAddr::V6(bytes.into()).to_string()
            }
            _ => return Err(std::io::Error::other("Unsupported SOCKS5 address type")),
        };
        let port = client.read_u16().await?;
        let address = tokio::net::lookup_host((host.as_str(), port))
            .await?
            .next()
            .ok_or_else(|| std::io::Error::other("Site address was not resolved"))?;
        let rule = domains
            .iter()
            .filter(|domain| {
                host.as_str() == domain.as_str() || host.ends_with(&format!(".{domain}"))
            })
            .max_by_key(|domain| domain.len())
            .cloned()
            .unwrap_or_else(|| host.clone());
        Ok::<_, std::io::Error>((rule, format!("{host}:{port}"), address))
    };

    let Ok(Ok((rule, target, address))) =
        tokio::time::timeout(Duration::from_secs(10), request).await
    else {
        return;
    };
    let id = shared.next_id.fetch_add(1, Ordering::Relaxed);
    shared
        .connections
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(
            id,
            SiteConnection {
                id,
                rule,
                target,
                closed: None,
                failure: None,
            },
        );
    let result = async {
        let mut upstream = warp_socks_connect(warp_port, address).await?;
        client.write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0]).await?;
        if let Err(error) = tokio::io::copy_bidirectional(&mut client, &mut upstream).await {
            if !super::connection_closed_normally(&error) {
                return Err(error);
            }
        }
        Ok::<_, std::io::Error>(())
    }
    .await;
    let mut connections = shared.connections.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(connection) = connections.get_mut(&id) {
        if let Err(failure) = result {
            connection.failure = Some(failure.to_string());
            connection.closed = Some(Instant::now());
        } else {
            connections.remove(&id);
        }
    }
}

#[cfg(windows)]
async fn warp_socks_connect(
    port: u16,
    target: SocketAddr,
) -> std::io::Result<tokio::net::TcpStream> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream = tokio::net::TcpStream::connect((Ipv4Addr::LOCALHOST, port)).await?;
    stream.write_all(&[5, 1, 0]).await?;
    let mut greeting = [0; 2];
    stream.read_exact(&mut greeting).await?;
    if greeting != [5, 0] {
        return Err(std::io::Error::other("WARP rejected SOCKS5 authentication"));
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
        return Err(std::io::Error::other(format!(
            "WARP SOCKS5 CONNECT failed ({})",
            header[1]
        )));
    }
    let length = match header[3] {
        1 => 4,
        4 => 16,
        3 => stream.read_u8().await? as usize,
        _ => return Err(std::io::Error::other("Invalid WARP SOCKS5 response")),
    };
    let mut address = vec![0; length + 2];
    stream.read_exact(&mut address).await?;
    Ok(stream)
}

pub fn normalize_domains(input: Vec<String>) -> Result<Vec<String>> {
    if input.len() > 500 {
        return Err(WarpError::new("warp_sites_invalid", "Maximum: 500 domains"));
    }
    let mut domains = Vec::new();
    for entry in input {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        let host = entry
            .strip_prefix("*.")
            .unwrap_or(entry)
            .trim_end_matches('.');
        // Accept domains only, never URLs, ports, credentials or script fragments.
        if host.len() > 253
            || host.contains(['/', '\\', ':', '@', '?', '#', '%'])
            || host.chars().any(char::is_whitespace)
        {
            return Err(WarpError::new("warp_sites_invalid", entry));
        }
        let url = reqwest::Url::parse(&format!("https://{host}/"))
            .map_err(|_| WarpError::new("warp_sites_invalid", entry))?;
        let domain = url
            .host_str()
            .ok_or_else(|| WarpError::new("warp_sites_invalid", entry))?
            .to_ascii_lowercase();
        if domain.len() > 253
            || !domain.contains('.')
            || domain.parse::<std::net::IpAddr>().is_ok()
            || domain.split('.').any(|label| {
                label.is_empty()
                    || label.len() > 63
                    || label.starts_with('-')
                    || label.ends_with('-')
                    || !label
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            })
        {
            return Err(WarpError::new("warp_sites_invalid", entry));
        }
        if !domains.contains(&domain) {
            domains.push(domain);
        }
    }
    Ok(domains)
}

pub(super) fn pac_script(domains: &[String], port: u16) -> String {
    let domains = serde_json::to_string(domains).expect("string list is serializable");
    format!(
        r#"function FindProxyForURL(url, host) {{
  host = host.toLowerCase().replace(/\.$/, "");
  var domains = {domains};
  for (var i = 0; i < domains.length; i++) {{
    var d = domains[i];
    if (host === d || host.slice(-(d.length + 1)) === "." + d)
      // The local relay records the selected hostname and forwards the
      // resolved numeric destination to WARP's SOCKS5 endpoint.
      return "SOCKS5 127.0.0.1:{port}";
  }}
  return "DIRECT";
}}
"#
    )
}

impl PacServer {
    fn running(&self) -> bool {
        let pac_running = self
            .worker
            .as_ref()
            .is_some_and(|worker| !worker.is_finished());
        #[cfg(windows)]
        let relay_running = self.relay.as_ref().is_some_and(|relay| {
            relay
                .worker
                .as_ref()
                .is_some_and(|worker| !worker.is_finished())
        });
        #[cfg(not(windows))]
        let relay_running = true;
        pac_running && relay_running
    }

    fn start(script: String, port: u16) -> Result<Self> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).map_err(error)?;
        listener.set_nonblocking(true).map_err(error)?;
        let address = listener.local_addr().map_err(error)?;
        let revision = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(error)?
            .as_nanos();
        let path = format!("/warp-sites-{revision}.pac");
        let url = format!("http://{address}{path}");
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let worker = thread::Builder::new().name("warp-sites-pac".into()).spawn(move || {
            while !stopping.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let _ = stream.set_read_timeout(Some(Duration::from_millis(300)));
                        let _ = stream.set_write_timeout(Some(Duration::from_millis(300)));
                        let mut request = Vec::new();
                        let mut buffer = [0; 1024];
                        let deadline = Instant::now() + Duration::from_millis(500);
                        while request.len() < 4096 && Instant::now() < deadline && !stopping.load(Ordering::Acquire) && !request.windows(4).any(|w| w == b"\r\n\r\n") {
                            match stream.read(&mut buffer) { Ok(0) | Err(_) => break, Ok(n) => request.extend_from_slice(&buffer[..n]) }
                        }
                        let request = String::from_utf8_lossy(&request);
                        let valid = request.lines().next() == Some(&format!("GET {path} HTTP/1.1"))
                            && request.lines().any(|line| line.split_once(':').is_some_and(|(key, value)| key.eq_ignore_ascii_case("host") && value.trim() == address.to_string()));
                        let (status, body) = if valid { ("200 OK", script.as_str()) } else { ("404 Not Found", "") };
                        let _ = write!(stream, "HTTP/1.1 {status}\r\nContent-Type: application/x-ns-proxy-autoconfig\r\nCache-Control: no-store\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}", body.len());
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => thread::sleep(Duration::from_millis(30)),
                    Err(_) => break,
                }
            }
        }).map_err(error)?;
        Ok(Self {
            stop,
            worker: Some(worker),
            url,
            port,
            #[cfg(windows)]
            relay: None,
        })
    }

    #[cfg(windows)]
    fn start_with_relay(domains: &[String], warp_port: u16) -> Result<Self> {
        let relay = SiteRelay::start(domains.to_vec(), warp_port)?;
        let mut server = Self::start(pac_script(domains, relay.port), warp_port)?;
        server.relay = Some(relay);
        Ok(server)
    }

    fn connections(&self) -> Vec<super::WarpConnection> {
        #[cfg(windows)]
        if let Some(relay) = &self.relay {
            return relay.connections();
        }
        Vec::new()
    }
}

impl Runtime {
    fn recover(&mut self) -> Result<()> {
        self.recover_with(system::read, system::write)
    }

    fn recover_with(
        &mut self,
        read: impl FnOnce() -> Result<ProxySettings>,
        write: impl FnOnce(&ProxySettings) -> Result<()>,
    ) -> Result<()> {
        let path = self.directory.join("warp-sites-recovery.json");
        if path.exists() {
            let journal: Journal =
                serde_json::from_slice(&fs::read(&path).map_err(error)?).map_err(error)?;
            // Another app/user may have changed the proxy since activation.
            if read()? == journal.owned {
                write(&journal.previous)?;
            }
            fs::remove_file(path).map_err(error)?;
        }
        self.server = None;
        Ok(())
    }

    fn status(&mut self) -> Result<SiteStatus> {
        if let Some(server) = &self.server {
            if !server.running()
                || system::read()?
                    != (ProxySettings {
                        flags: 5,
                        url: server.url.clone(),
                    })
            {
                self.recover()?;
            }
        } else {
            self.recover()?;
        }
        Ok(SiteStatus {
            domains: self.domains.clone(),
            enabled: self.desired_enabled,
            runtime_active: self.server.is_some(),
            connections: self
                .server
                .as_ref()
                .map(PacServer::connections)
                .unwrap_or_default(),
        })
    }
}

pub fn initialize(directory: PathBuf) -> Result<()> {
    let mut guard = SITES.lock().map_err(error)?;
    STOPPING.store(false, Ordering::Release);
    fs::create_dir_all(&directory).map_err(error)?;
    let path = directory.join("warp-sites.json");
    let state_path = directory.join("warp-sites-state.json");
    let desired_enabled = if state_path.exists() {
        serde_json::from_slice(&fs::read(&state_path).map_err(error)?).map_err(error)?
    } else {
        false
    };
    *guard = Some(Runtime {
        directory,
        domains: Vec::new(),
        desired_enabled,
        server: None,
    });
    let runtime = guard.as_mut().unwrap();
    // Recovery must run even if the unrelated domain preferences are damaged.
    runtime.recover()?;
    let domains = if path.exists() {
        normalize_domains(serde_json::from_slice(&fs::read(path).map_err(error)?).map_err(error)?)?
    } else {
        Vec::new()
    };
    runtime.domains = domains;
    Ok(())
}

pub fn status() -> Result<SiteStatus> {
    let mut guard = SITES.lock().map_err(error)?;
    match guard.as_mut() {
        Some(runtime) => runtime.status(),
        None => Ok(SiteStatus::default()),
    }
}

pub fn disable() -> Result<()> {
    let mut guard = SITES.lock().map_err(error)?;
    if let Some(runtime) = guard.as_mut() {
        runtime.recover()?;
    }
    Ok(())
}

pub fn shutdown() -> Result<()> {
    STOPPING.store(true, Ordering::Release);
    disable()
}

pub fn reconcile_port(port: Option<u16>) -> Result<()> {
    let mut guard = SITES.lock().map_err(error)?;
    if let Some(runtime) = guard.as_mut() {
        if runtime
            .server
            .as_ref()
            .is_some_and(|server| Some(server.port) != port)
        {
            runtime.recover()?;
        }
    }
    Ok(())
}

pub fn configure(domains: Vec<String>, enabled: bool, port: Option<u16>) -> Result<()> {
    let domains = normalize_domains(domains)?;
    if enabled && domains.is_empty() {
        return Err(WarpError::new("warp_sites_invalid", ""));
    }
    let mut guard = SITES.lock().map_err(error)?;
    if STOPPING.load(Ordering::Acquire) {
        return Err(error("Application is exiting"));
    }
    let runtime = guard
        .as_mut()
        .ok_or_else(|| error("Site settings are unavailable"))?;
    // Validate and bind the replacement before touching any active configuration.
    let replacement = if enabled {
        let port = port
            .filter(|p| *p > 0)
            .ok_or_else(|| WarpError::new("warp_sites_requires_proxy", ""))?;
        #[cfg(windows)]
        let server = PacServer::start_with_relay(&domains, port)?;
        #[cfg(not(windows))]
        let server = PacServer::start(pac_script(&domains, port), port)?;
        Some(server)
    } else {
        None
    };
    persist_json(&runtime.directory.join("warp-sites.json"), &domains)?;
    persist_json(&runtime.directory.join("warp-sites-state.json"), &enabled)?;
    runtime.domains = domains;
    runtime.desired_enabled = enabled;
    runtime.recover()?;
    if let Some(server) = replacement {
        let journal = Journal {
            previous: system::read()?,
            owned: ProxySettings {
                flags: 5,
                url: server.url.clone(),
            },
        };
        persist_json(
            &runtime.directory.join("warp-sites-recovery.json"),
            &journal,
        )?;
        // Keep the server alive even if notifying Windows fails; recovery retries
        // on the next command/startup rather than losing the original settings.
        runtime.server = Some(server);
        if let Err(e) = system::write(&journal.owned) {
            let _ = runtime.recover();
            return Err(e);
        }
        if system::read()? != journal.owned {
            runtime.recover()?;
            return Err(error(
                "Windows did not apply the proxy settings (check browser or organization policies)",
            ));
        }
    }
    Ok(())
}

#[cfg(windows)]
mod system {
    use super::*;
    use std::{mem::size_of, ptr::null_mut};
    use windows_sys::Win32::{Foundation::GlobalFree, Networking::WinInet::*};

    fn options() -> [INTERNET_PER_CONN_OPTIONW; 2] {
        [
            INTERNET_PER_CONN_OPTIONW {
                dwOption: INTERNET_PER_CONN_FLAGS,
                ..Default::default()
            },
            INTERNET_PER_CONN_OPTIONW {
                dwOption: INTERNET_PER_CONN_AUTOCONFIG_URL,
                ..Default::default()
            },
        ]
    }
    fn list(options: &mut [INTERNET_PER_CONN_OPTIONW; 2]) -> INTERNET_PER_CONN_OPTION_LISTW {
        INTERNET_PER_CONN_OPTION_LISTW {
            dwSize: size_of::<INTERNET_PER_CONN_OPTION_LISTW>() as u32,
            dwOptionCount: 2,
            pOptions: options.as_mut_ptr(),
            ..Default::default()
        }
    }
    pub fn read() -> Result<ProxySettings> {
        let mut options = options();
        // Preserve the user's configured auto-detect checkbox, not just the
        // flags WinINet happened to select during automatic proxy discovery.
        options[0].dwOption = INTERNET_PER_CONN_FLAGS_UI;
        let mut list = list(&mut options);
        let mut size = size_of::<INTERNET_PER_CONN_OPTION_LISTW>() as u32;
        unsafe {
            if InternetQueryOptionW(
                null_mut(),
                INTERNET_OPTION_PER_CONNECTION_OPTION,
                (&mut list as *mut INTERNET_PER_CONN_OPTION_LISTW).cast(),
                &mut size,
            ) == 0
            {
                return Err(error(std::io::Error::last_os_error()));
            }
            let pointer = options[1].Value.pszValue;
            let url = if pointer.is_null() {
                String::new()
            } else {
                let mut len = 0;
                while *pointer.add(len) != 0 {
                    len += 1;
                }
                let text = String::from_utf16_lossy(std::slice::from_raw_parts(pointer, len));
                GlobalFree(pointer.cast());
                text
            };
            Ok(ProxySettings {
                flags: options[0].Value.dwValue,
                url,
            })
        }
    }
    pub fn write(settings: &ProxySettings) -> Result<()> {
        let mut url: Vec<u16> = settings.url.encode_utf16().chain(Some(0)).collect();
        let mut options = options();
        options[0].Value.dwValue = settings.flags;
        options[1].Value.pszValue = url.as_mut_ptr();
        let mut list = list(&mut options);
        unsafe {
            if InternetSetOptionW(
                null_mut(),
                INTERNET_OPTION_PER_CONNECTION_OPTION,
                (&mut list as *mut INTERNET_PER_CONN_OPTION_LISTW).cast(),
                size_of::<INTERNET_PER_CONN_OPTION_LISTW>() as u32,
            ) == 0
            {
                return Err(error(std::io::Error::last_os_error()));
            }
            for option in [INTERNET_OPTION_SETTINGS_CHANGED, INTERNET_OPTION_REFRESH] {
                if InternetSetOptionW(null_mut(), option, null_mut(), 0) == 0 {
                    return Err(error(std::io::Error::last_os_error()));
                }
            }
        }
        Ok(())
    }
}

#[cfg(not(windows))]
mod system {
    use super::*;
    pub fn read() -> Result<ProxySettings> {
        Err(error("Windows only"))
    }
    pub fn write(_: &ProxySettings) -> Result<()> {
        Err(error("Windows only"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_and_normalizes_domains() {
        assert_eq!(
            normalize_domains(vec![
                " EXAMPLE.com ".into(),
                "*.example.com.".into(),
                "пример.рф".into()
            ])
            .unwrap(),
            vec!["example.com", "xn--e1afmkfd.xn--p1ai"]
        );
        for bad in [
            "https://example.com",
            "a.com/path",
            "a.com:80",
            "127.0.0.1",
            "a..com",
            "-a.com",
            "a.com;alert(1)",
            "*.com",
            "a.com\nother.com",
            "a.com@b.com",
        ] {
            assert!(normalize_domains(vec![bad.into()]).is_err(), "{bad}");
        }
    }
    #[test]
    fn serves_only_the_pac_endpoint() {
        let script = pac_script(&["example.com".into()], 40000);
        let server = PacServer::start(script.clone(), 40000).unwrap();
        let url = reqwest::Url::parse(&server.url).unwrap();
        let address = format!("127.0.0.1:{}", url.port().unwrap());
        for (path, expected) in [(url.path(), "200 OK"), ("/other", "404 Not Found")] {
            let mut stream = std::net::TcpStream::connect(&address).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            write!(stream, "GET {path} HTTP/1.1\r\nHost: {address}\r\n\r\n").unwrap();
            let mut response = String::new();
            stream.read_to_string(&mut response).unwrap();
            assert!(response.contains(expected));
            if expected == "200 OK" {
                assert!(response.ends_with(&script));
            }
        }
    }

    #[test]
    fn pac_routes_exact_domains_and_subdomains_without_suffix_collisions() {
        let script = pac_script(
            &["example.com".into(), "xn--e1afmkfd.xn--p1ai".into()],
            41234,
        );
        // Execute the generated PAC, not a second implementation of its matcher.
        let checks = r#"
const assert = require('node:assert/strict');
for (const host of ['example.com','www.example.com','a.b.example.com','EXAMPLE.COM.','xn--e1afmkfd.xn--p1ai'])
  assert.equal(FindProxyForURL('https://' + host + '/', host), 'SOCKS5 127.0.0.1:41234', host);
for (const host of ['other.com','notexample.com','example.com.attacker.test','localhost','127.0.0.1','com'])
  assert.equal(FindProxyForURL('https://' + host + '/', host), 'DIRECT', host);
"#;
        let output = std::process::Command::new("node")
            .args(["-e", &format!("{script}\n{checks}")])
            .output()
            .expect("Node.js is required for PAC execution tests");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[cfg(windows)]
    #[test]
    fn site_relay_reports_only_live_and_failed_connections() {
        let upstream = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let upstream_port = upstream.local_addr().unwrap().port();
        let (release, released) = std::sync::mpsc::channel();
        let upstream_worker = thread::spawn(move || {
            let (mut stream, _) = upstream.accept().unwrap();
            let mut greeting = [0; 3];
            stream.read_exact(&mut greeting).unwrap();
            assert_eq!(greeting, [5, 1, 0]);
            stream.write_all(&[5, 0]).unwrap();
            let mut request = [0; 10];
            stream.read_exact(&mut request).unwrap();
            assert_eq!(&request[..4], &[5, 1, 0, 1]);
            stream.write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0]).unwrap();
            let mut byte = [0; 1];
            stream.read_exact(&mut byte).unwrap();
            stream.write_all(&byte).unwrap();
            released.recv().unwrap();
        });
        let relay = SiteRelay::start(vec!["127.0.0.1".into()], upstream_port).unwrap();
        let mut client = std::net::TcpStream::connect((Ipv4Addr::LOCALHOST, relay.port)).unwrap();
        client.write_all(&[5, 1, 0]).unwrap();
        let mut greeting = [0; 2];
        client.read_exact(&mut greeting).unwrap();
        assert_eq!(greeting, [5, 0]);
        client
            .write_all(&[5, 1, 0, 1, 127, 0, 0, 1, 0, 80])
            .unwrap();
        let mut response = [0; 10];
        client.read_exact(&mut response).unwrap();
        assert_eq!(response[1], 0);
        client.write_all(&[42]).unwrap();
        let mut echoed = [0; 1];
        client.read_exact(&mut echoed).unwrap();
        assert_eq!(echoed, [42]);
        assert!(relay
            .connections()
            .iter()
            .any(|connection| connection.state == "active" && connection.source == "site"));
        release.send(()).unwrap();
        drop(client);
        upstream_worker.join().unwrap();
        for _ in 0..50 {
            if relay.connections().is_empty() {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("normally closed site connection remained visible");
    }

    #[test]
    fn recovery_restores_owned_settings_preserves_external_changes_and_retries_failures() {
        let directory = std::env::temp_dir().join(format!(
            "zapret-sites-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&directory).unwrap();
        let path = directory.join("warp-sites-recovery.json");
        let previous = ProxySettings {
            flags: 11,
            url: "https://company.example/proxy.pac".into(),
        };
        let owned = ProxySettings {
            flags: 5,
            url: "http://127.0.0.1:41235/warp-sites-test.pac".into(),
        };
        let data = serde_json::to_vec(&Journal {
            previous: previous.clone(),
            owned: owned.clone(),
        })
        .unwrap();
        let mut runtime = Runtime {
            directory: directory.clone(),
            domains: vec![],
            desired_enabled: false,
            server: None,
        };
        fs::write(&path, &data).unwrap();
        assert!(runtime
            .recover_with(|| Ok(owned.clone()), |_| Err(error("test write failure")))
            .is_err());
        assert!(path.exists(), "failed restoration keeps recovery journal");
        let restored = std::cell::RefCell::new(None);
        runtime
            .recover_with(
                || Ok(owned.clone()),
                |settings| {
                    *restored.borrow_mut() = Some(settings.clone());
                    Ok(())
                },
            )
            .unwrap();
        assert_eq!(*restored.borrow(), Some(previous.clone()));
        assert!(!path.exists());
        fs::write(&path, data).unwrap();
        runtime
            .recover_with(
                || Ok(previous),
                |_| panic!("external settings must not be overwritten"),
            )
            .unwrap();
        assert!(!path.exists());
        fs::remove_dir(directory).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn wininet_settings_can_be_read_without_mutating_them() {
        let settings = system::read().unwrap();
        assert!(settings.flags > 0);
    }
}
