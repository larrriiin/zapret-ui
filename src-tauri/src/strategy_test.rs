use futures_util::{stream, StreamExt};
use reqwest::{header::RANGE, tls::Version, Client};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::{Emitter, Manager};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use crate::{
    core::CoreProvider, split_arguments, AppState, MutexExt, TestResult, CREATE_NO_WINDOW,
};

static TEST_RUNNING: AtomicBool = AtomicBool::new(false);
static TEST_CANCELLED: AtomicBool = AtomicBool::new(false);

const DEFAULT_TARGETS: [(&str, &str); 17] = [
    ("Discord Main", "https://discord.com"),
    ("Discord Gateway", "https://gateway.discord.gg"),
    ("Discord CDN", "https://cdn.discordapp.com"),
    ("Discord Updates", "https://updates.discord.com"),
    ("YouTube Web", "https://www.youtube.com"),
    ("YouTube Short", "https://youtu.be"),
    ("YouTube Image", "https://i.ytimg.com"),
    ("YouTube Video", "https://redirector.googlevideo.com"),
    ("Google Main", "https://www.google.com"),
    ("Google Gstatic", "https://www.gstatic.com"),
    ("Cloudflare Web", "https://www.cloudflare.com"),
    ("Cloudflare CDN", "https://cdnjs.cloudflare.com"),
    ("Cloudflare DNS 1.1.1.1", "PING:1.1.1.1"),
    ("Cloudflare DNS 1.0.0.1", "PING:1.0.0.1"),
    ("Google DNS 8.8.8.8", "PING:8.8.8.8"),
    ("Google DNS 8.8.4.4", "PING:8.8.4.4"),
    ("Quad9 DNS 9.9.9.9", "PING:9.9.9.9"),
];

#[derive(Clone)]
struct Target {
    name: String,
    url: Option<String>,
    host: String,
}

#[derive(Clone, Copy)]
enum ProbeKind {
    Http1,
    Tls12,
    Tls13,
}

struct TestGuard;

impl TestGuard {
    fn acquire() -> Result<Self, String> {
        TEST_RUNNING
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| "wizard_error_already_running".to_string())?;
        TEST_CANCELLED.store(false, Ordering::Release);
        Ok(Self)
    }
}

impl Drop for TestGuard {
    fn drop(&mut self) {
        TEST_CANCELLED.store(false, Ordering::Release);
        TEST_RUNNING.store(false, Ordering::Release);
    }
}

struct RunningStrategy<'a> {
    child: Child,
    app: &'a tauri::AppHandle,
}

struct TestIpsetOverride {
    path: PathBuf,
    original: String,
    temporary: String,
}

impl Drop for TestIpsetOverride {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

impl Drop for RunningStrategy<'_> {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        *self
            .app
            .state::<AppState>()
            .test_process_pid
            .lock_unpoisoned() = None;
    }
}

fn emit_progress(app: &tauri::AppHandle, message_key: &str, params: Value, kind: &str) {
    let _ = app.emit(
        "test-progress",
        json!({ "messageKey": message_key, "params": params, "kind": kind }),
    );
}

fn target_from_value(name: impl Into<String>, value: &str) -> Option<Target> {
    let name = name.into();
    if let Some(host) = value.strip_prefix("PING:") {
        let host = host.trim();
        if host.is_empty() {
            return None;
        }
        return Some(Target {
            name,
            url: None,
            host: host.to_string(),
        });
    }
    let url = reqwest::Url::parse(value).ok()?;
    if url.scheme() != "https" {
        return None;
    }
    let host = url.host_str()?.to_string();
    Some(Target {
        name,
        url: Some(url.to_string()),
        host,
    })
}

fn parse_target_line(line: &str) -> Option<Target> {
    let line = line.trim().trim_start_matches('\u{feff}');
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let (name, value) = line.split_once('=')?;
    let name = name.trim();
    let value = value.trim().trim_matches('"');
    if name.is_empty() || value.is_empty() {
        return None;
    }
    target_from_value(name, value)
}

fn load_standard_targets(root: &Path) -> Vec<Target> {
    let path = root.join("utils").join("targets.txt");
    let configured = fs::read_to_string(path)
        .ok()
        .map(|content| {
            content
                .lines()
                .filter_map(parse_target_line)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if !configured.is_empty() {
        return configured;
    }
    DEFAULT_TARGETS
        .iter()
        .filter_map(|(name, value)| target_from_value(*name, value))
        .collect()
}

fn build_client(kind: ProbeKind, timeout: Duration) -> Result<Client, String> {
    let mut builder = Client::builder()
        .timeout(timeout)
        .connect_timeout(Duration::from_secs(3))
        .redirect(reqwest::redirect::Policy::limited(5))
        .http1_only();
    builder = match kind {
        ProbeKind::Http1 => builder,
        ProbeKind::Tls12 => builder
            .min_tls_version(Version::TLS_1_2)
            .max_tls_version(Version::TLS_1_2),
        ProbeKind::Tls13 => builder
            .min_tls_version(Version::TLS_1_3)
            .max_tls_version(Version::TLS_1_3),
    };
    builder
        .build()
        .map_err(|e| format!("Cannot build HTTP probe: {e}"))
}

async fn tcp_latency(host: &str) -> Option<i32> {
    let started = Instant::now();
    let connected = tokio::time::timeout(
        Duration::from_secs(2),
        tokio::net::TcpStream::connect((host, 443)),
    )
    .await
    .ok()?
    .ok()?;
    drop(connected);
    Some(started.elapsed().as_millis().min(i32::MAX as u128) as i32)
}

async fn standard_target(
    target: Target,
    clients: [(ProbeKind, Option<Client>); 3],
) -> (String, i32, i32, Option<i32>) {
    let latency = tcp_latency(&target.host).await;
    let mut ok = 0;
    let mut error = 0;
    if let Some(url) = target.url.as_deref() {
        for (_, client) in clients {
            let Some(client) = client else { continue };
            match client.get(url).header(RANGE, "bytes=0-0").send().await {
                Ok(_) => ok += 1,
                Err(_) => error += 1,
            }
        }
    }
    (target.name, ok, error, latency)
}

#[derive(Deserialize)]
struct DpiSuiteEntry {
    #[serde(default)]
    id: String,
    host: String,
}

async fn load_dpi_targets() -> Vec<Target> {
    if let Ok(host) = std::env::var("MONITOR_HOST") {
        let host = host
            .trim()
            .trim_start_matches("https://")
            .trim_end_matches('/');
        if let Some(target) = target_from_value("Custom", &format!("https://{host}")) {
            return vec![target];
        }
    }
    let timeout = Duration::from_secs(5);
    if let Ok(client) = Client::builder().timeout(timeout).build() {
        if let Ok(response) = client
            .get("https://hyperion-cs.github.io/dpi-checkers/ru/tcp-16-20/suite.v2.json")
            .send()
            .await
        {
            if let Ok(entries) = response.json::<Vec<DpiSuiteEntry>>().await {
                let targets = entries
                    .into_iter()
                    .filter_map(|entry| {
                        target_from_value(
                            if entry.id.is_empty() {
                                &entry.host
                            } else {
                                &entry.id
                            },
                            &format!("https://{}", entry.host),
                        )
                    })
                    .collect::<Vec<_>>();
                if !targets.is_empty() {
                    return targets;
                }
            }
        }
    }
    [
        ("Discord", "https://discord.com"),
        ("YouTube", "https://www.youtube.com"),
        ("Google Video", "https://redirector.googlevideo.com"),
        ("Cloudflare", "https://www.cloudflare.com"),
    ]
    .into_iter()
    .filter_map(|(name, value)| target_from_value(name, value))
    .collect()
}

fn dpi_payload() -> Vec<u8> {
    let mut value = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or(0x9e3779b97f4a7c15);
    let mut payload = vec![0u8; 64 * 1024];
    for byte in &mut payload {
        value ^= value << 13;
        value ^= value >> 7;
        value ^= value << 17;
        *byte = value as u8;
    }
    payload
}

async fn dpi_target(
    target: Target,
    clients: [(ProbeKind, Option<Client>); 3],
    payload: Vec<u8>,
) -> (String, i32, i32) {
    let mut ok = 0;
    let mut error = 0;
    let Some(url) = target.url.as_deref() else {
        return (target.name, ok, error);
    };
    for (_, client) in clients {
        let Some(client) = client else { continue };
        match client
            .post(url)
            .header(RANGE, "bytes=0-65535")
            .body(payload.clone())
            .send()
            .await
        {
            Ok(_) => ok += 1,
            Err(_) => error += 1,
        }
    }
    (target.name, ok, error)
}

fn start_strategy<'a>(
    app: &'a tauri::AppHandle,
    executable: &Path,
    arguments: &str,
) -> Result<RunningStrategy<'a>, String> {
    let working_directory = executable
        .parent()
        .ok_or_else(|| "winws executable has no parent directory".to_string())?;
    let child = Command::new(executable)
        .args(split_arguments(arguments))
        .current_dir(working_directory)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| format!("Cannot start winws.exe: {e}"))?;
    *app.state::<AppState>().test_process_pid.lock_unpoisoned() = Some(child.id());
    Ok(RunningStrategy { child, app })
}

fn test_ipset_override(root: &Path) -> Result<TestIpsetOverride, String> {
    let original = root.join("lists").join("ipset-all.txt");
    let temporary = root.join("lists").join("ipset-test-any.txt");
    fs::write(&temporary, b"").map_err(|e| format!("Cannot prepare temporary DPI ipset: {e}"))?;
    Ok(TestIpsetOverride {
        path: temporary.clone(),
        original: original.to_string_lossy().into_owned(),
        temporary: temporary.to_string_lossy().into_owned(),
    })
}

pub fn cancel(app: &tauri::AppHandle) {
    TEST_CANCELLED.store(true, Ordering::Release);
    if let Some(pid) = app
        .state::<AppState>()
        .test_process_pid
        .lock_unpoisoned()
        .take()
    {
        let _ = Command::new(crate::system32_tool("taskkill.exe"))
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
    }
}

pub async fn run(
    app: &tauri::AppHandle,
    root: &Path,
    provider: &dyn CoreProvider,
    test_type: &str,
    game_filter: &str,
) -> Result<Vec<TestResult>, String> {
    let _guard = TestGuard::acquire()?;
    let is_dpi = match test_type {
        "standard" => false,
        "dpi" => true,
        _ => return Err("wizard_error_invalid_test_type".to_string()),
    };
    let strategies = provider.strategies()?;
    if strategies.is_empty() {
        return Err("wizard_no_strategies".to_string());
    }

    let timeout = if is_dpi {
        Duration::from_secs(5)
    } else {
        Duration::from_secs(4)
    };
    let clients = [ProbeKind::Http1, ProbeKind::Tls12, ProbeKind::Tls13]
        .map(|kind| (kind, build_client(kind, timeout).ok()));
    if clients.iter().all(|(_, client)| client.is_none()) {
        return Err("wizard_error_no_http_clients".to_string());
    }

    let targets = if is_dpi {
        load_dpi_targets().await
    } else {
        load_standard_targets(root)
    };
    let ipset_override = is_dpi.then(|| test_ipset_override(root)).transpose()?;
    let mut results = Vec::with_capacity(strategies.len());
    let mut best: Option<(String, i32)> = None;

    emit_progress(
        app,
        "wizard_log_starting",
        json!({ "count": strategies.len() }),
        "info",
    );

    for (index, name) in strategies.iter().enumerate() {
        if TEST_CANCELLED.load(Ordering::Acquire) {
            break;
        }
        let _ = app.emit(
            "test-config-start",
            json!({ "index": index + 1, "total": strategies.len(), "name": name }),
        );
        emit_progress(
            app,
            "wizard_log_strategy_start",
            json!({ "name": name }),
            "config",
        );

        let mut arguments = provider.parse_strategy(name, game_filter)?;
        if let Some(ipset) = ipset_override.as_ref() {
            arguments = arguments.replace(&ipset.original, &ipset.temporary);
        }
        let mut process = match start_strategy(app, &provider.winws_executable(), &arguments) {
            Ok(process) => process,
            Err(_) => {
                emit_progress(
                    app,
                    "wizard_log_strategy_failed",
                    json!({ "name": name }),
                    "error",
                );
                results.push(TestResult::failed(name.clone()));
                continue;
            }
        };
        tokio::time::sleep(Duration::from_millis(700)).await;
        if process.child.try_wait().ok().flatten().is_some() {
            emit_progress(
                app,
                "wizard_log_strategy_failed",
                json!({ "name": name }),
                "error",
            );
            results.push(TestResult::failed(name.clone()));
            continue;
        }

        let result = if is_dpi {
            let payload = dpi_payload();
            let mut probes = stream::iter(targets.clone().into_iter().map(|target| {
                let clients = clients.clone();
                let payload = payload.clone();
                async move { dpi_target(target, clients, payload).await }
            }))
            .buffer_unordered(8);
            let mut http_ok = 0;
            let mut http_error = 0;
            while let Some((target, ok, error)) = probes.next().await {
                http_ok += ok;
                http_error += error;
                emit_progress(
                    app,
                    if error == 0 {
                        "wizard_log_target_ok"
                    } else {
                        "wizard_log_target_failed"
                    },
                    json!({ "target": target, "ok": ok, "total": ok + error }),
                    if error == 0 { "success" } else { "warning" },
                );
                if TEST_CANCELLED.load(Ordering::Acquire) {
                    break;
                }
            }
            TestResult::from_counts(name.clone(), http_ok, http_error, 0, 0, 0)
        } else {
            let mut probes = stream::iter(targets.clone().into_iter().map(|target| {
                let clients = clients.clone();
                async move { standard_target(target, clients).await }
            }))
            .buffer_unordered(8);
            let (mut http_ok, mut http_error, mut ping_ok, mut ping_fail) = (0, 0, 0, 0);
            let (mut ping_sum, mut ping_count) = (0i64, 0i64);
            while let Some((target, ok, error, latency)) = probes.next().await {
                http_ok += ok;
                http_error += error;
                if let Some(ms) = latency {
                    ping_ok += 1;
                    ping_sum += i64::from(ms);
                    ping_count += 1;
                } else {
                    ping_fail += 1;
                }
                let http_total = ok + error;
                let (message_key, params, kind) = if http_total == 0 {
                    match latency {
                        Some(ms) => (
                            "wizard_log_target_latency",
                            json!({ "target": target, "latency": ms }),
                            "success",
                        ),
                        None => (
                            "wizard_log_target_unreachable",
                            json!({ "target": target }),
                            "warning",
                        ),
                    }
                } else if error == 0 && latency.is_some() {
                    (
                        "wizard_log_target_ok",
                        json!({ "target": target, "ok": ok, "total": http_total }),
                        "success",
                    )
                } else {
                    (
                        "wizard_log_target_failed",
                        json!({ "target": target, "ok": ok, "total": http_total }),
                        "warning",
                    )
                };
                emit_progress(app, message_key, params, kind);
                if TEST_CANCELLED.load(Ordering::Acquire) {
                    break;
                }
            }
            let avg = if ping_count > 0 {
                ((ping_sum + ping_count / 2) / ping_count) as i32
            } else {
                0
            };
            TestResult::from_counts(name.clone(), http_ok, http_error, ping_ok, ping_fail, avg)
        };
        drop(process);

        if best.as_ref().is_none_or(|(_, score)| result.score > *score) {
            best = Some((name.clone(), result.score));
            let _ = app.emit("test-best", json!({ "config": name }));
        }
        results.push(result);
    }

    if TEST_CANCELLED.load(Ordering::Acquire) {
        emit_progress(app, "wizard_log_cancelled", json!({}), "warning");
    } else {
        emit_progress(app, "wizard_log_done", json!({}), "success");
    }
    let _ = app.emit("test-done", json!({ "count": results.len() }));
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_configured_targets_and_rejects_unsafe_schemes() {
        let target = parse_target_line("YouTube = \"https://youtube.com/path\"").unwrap();
        assert_eq!(target.name, "YouTube");
        assert_eq!(target.host, "youtube.com");
        assert!(parse_target_line("Bad = \"http://example.com\"").is_none());
        assert!(parse_target_line("# comment").is_none());
    }

    #[test]
    fn creates_incompressible_sized_dpi_payload() {
        let payload = dpi_payload();
        assert_eq!(payload.len(), 65_536);
        assert!(payload.windows(2).any(|pair| pair[0] != pair[1]));
    }
}
