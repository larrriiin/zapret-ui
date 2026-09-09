//! Session-scoped Windows PAC routing. The recovery journal is durable before
//! changing WinINet; only settings still owned by this session are restored.
use super::{Result, WarpError};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    net::TcpListener,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
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
    server: Option<PacServer>,
}

struct PacServer {
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    url: String,
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
      // Chromium sends SOCKS5 target hostnames to the proxy for resolution.
      // The current WARP local proxy closes those requests, but accepts the
      // numeric SOCKS4 form. SOCKS4 also makes Chromium resolve the selected
      // hostname locally before opening the proxied TCP connection.
      return "SOCKS4 127.0.0.1:{port}";
  }}
  return "DIRECT";
}}
"#
    )
}

impl PacServer {
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
        })
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
            if server
                .worker
                .as_ref()
                .is_none_or(|worker| worker.is_finished())
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
            enabled: self.server.is_some(),
        })
    }
}

pub fn initialize(directory: PathBuf) -> Result<()> {
    let mut guard = SITES.lock().map_err(error)?;
    STOPPING.store(false, Ordering::Release);
    fs::create_dir_all(&directory).map_err(error)?;
    let path = directory.join("warp-sites.json");
    *guard = Some(Runtime {
        directory,
        domains: Vec::new(),
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
        Some(PacServer::start(pac_script(&domains, port), port)?)
    } else {
        None
    };
    persist_json(&runtime.directory.join("warp-sites.json"), &domains)?;
    runtime.domains = domains;
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
  assert.equal(FindProxyForURL('https://' + host + '/', host), 'SOCKS4 127.0.0.1:41234', host);
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
