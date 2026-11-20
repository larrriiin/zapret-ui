use aws_lc_rs::{
    agreement::{PrivateKey, X25519},
    encoding::{AsBigEndian, Curve25519SeedBin},
};
use base64::{engine::general_purpose::STANDARD, Engine};
use reqwest::{RequestBuilder, Response, Url};
use serde::Deserialize;
use serde_json::json;
use std::{
    io::Write,
    net::{Ipv4Addr, Ipv6Addr, SocketAddr},
    path::PathBuf,
    time::Duration,
};
use tauri::Manager;
use tauri_plugin_opener::OpenerExt;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use tokio::sync::Mutex;
use zeroize::Zeroizing;

mod routes;
use routes::Service;

// Consumer WARP device registration is separate from the official desktop client.
// This API is undocumented. Reference: https://github.com/vernette/warpscout/blob/master/register.go
const REGISTRATION_URL: &str = "https://api.devices.cloudflare.com/v0a4005/reg";
const DEFAULT_ENDPOINT: &str = "engage.cloudflareclient.com:2408";
const WARP_PORTS: [u16; 4] = [2408, 500, 1701, 4500];
const MASKING_DOMAINS: [&str; 4] = [
    "www.avito.ru",
    "www.ozon.ru",
    "www.wildberries.ru",
    "www.yandex.ru",
];
const RESPONSE_LIMIT: usize = 64 * 1024;
static PROFILE_STATE: Mutex<ProfileState> = Mutex::const_new(ProfileState {
    tunnel: None,
    last_endpoint: None,
    last_file: None,
});

struct ProfileState {
    tunnel: Option<(PrivateKey, TunnelConfig)>,
    last_endpoint: Option<SocketAddr>,
    last_file: Option<PathBuf>,
}

#[derive(Deserialize)]
struct Registration {
    id: String,
    token: Zeroizing<String>,
    config: TunnelConfig,
}

#[derive(Deserialize)]
struct TunnelConfig {
    interface: TunnelInterface,
    peers: Vec<Peer>,
}

#[derive(Deserialize)]
struct TunnelInterface {
    addresses: Addresses,
}

#[derive(Deserialize)]
struct Addresses {
    v4: String,
    #[serde(default)]
    v6: String,
}

#[derive(Deserialize)]
struct Peer {
    public_key: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileOptions {
    endpoint: String,
    random_endpoint: bool,
    masking_domain: Option<String>,
    #[serde(default)]
    services: Option<Vec<Service>>,
    dns: DnsPreset,
    exclude_lan: bool,
    ipv6: bool,
    mtu: u16,
    keepalive: u16,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum DnsPreset {
    Cloudflare,
    Google,
    System,
}

impl DnsPreset {
    fn servers(&self, ipv6: bool) -> Option<&'static str> {
        match (self, ipv6) {
            (Self::Cloudflare, false) => Some("1.1.1.1, 1.0.0.1"),
            (Self::Cloudflare, true) => {
                Some("1.1.1.1, 1.0.0.1, 2606:4700:4700::1111, 2606:4700:4700::1001")
            }
            (Self::Google, false) => Some("8.8.8.8, 8.8.4.4"),
            (Self::Google, true) => {
                Some("8.8.8.8, 8.8.4.4, 2001:4860:4860::8888, 2001:4860:4860::8844")
            }
            (Self::System, _) => None,
        }
    }
}

#[tauri::command]
pub async fn save_warp_profile(
    app: tauri::AppHandle,
    options: ProfileOptions,
    accepted_terms: bool,
) -> Result<String, &'static str> {
    if !accepted_terms {
        return Err("warp_profile_terms_required");
    }
    let mut state = PROFILE_STATE.try_lock().map_err(|_| "warp_profile_busy")?;
    let random = options
        .random_endpoint
        .then(|| random_endpoint(state.last_endpoint));
    let endpoint = if let Some(address) = random {
        address.to_string()
    } else {
        validate_endpoint(&options.endpoint).ok_or("warp_profile_invalid_endpoint")?
    };
    let masking_domain = options
        .masking_domain
        .as_deref()
        .map(select_masking_domain)
        .transpose()?;
    if options.services.as_ref().is_some_and(Vec::is_empty) {
        return Err("warp_profile_services_required");
    }
    if options.mtu < if options.ipv6 { 1280 } else { 576 } {
        return Err("warp_profile_invalid_mtu");
    }
    let downloads = app
        .path()
        .download_dir()
        .map_err(|_| "warp_profile_write_error")?;
    std::fs::create_dir_all(&downloads).map_err(|_| "warp_profile_write_error")?;
    let file_name = format!("WARP-WireSock-{:016X}.conf", rand::random::<u64>());
    let path = downloads.join(&file_name);
    let mut output = tempfile::Builder::new()
        .prefix(".warp-")
        .tempfile_in(&downloads)
        .map_err(|_| "warp_profile_write_error")?;

    let client = reqwest::Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(12))
        .build()
        .map_err(|_| "warp_profile_network_error")?;
    let service_routes = match &options.services {
        Some(services) => Some(routes::fetch(&client, services.clone()).await?),
        None => None,
    };

    // Rotating the connection address does not require a new device registration.
    if state.tunnel.is_none() {
        let key = PrivateKey::generate(&X25519).map_err(|_| "warp_profile_key_error")?;
        let public_key = key
            .compute_public_key()
            .map_err(|_| "warp_profile_key_error")?;
        let config = register(&client, STANDARD.encode(public_key.as_ref())).await?;
        state.tunnel = Some((key, config));
    }
    let (key, config) = state
        .tunnel
        .as_ref()
        .ok_or("warp_profile_registration_error")?;
    let profile = build_profile(
        key,
        config,
        &options,
        &endpoint,
        masking_domain,
        service_routes.as_deref(),
    )?;
    output
        .write_all(profile.as_bytes())
        .and_then(|()| output.as_file().sync_all())
        .map_err(|_| "warp_profile_write_error")?;
    output
        .persist_noclobber(&path)
        .map_err(|_| "warp_profile_write_error")?;
    if random.is_some() {
        state.last_endpoint = random;
    }
    state.last_file = Some(path);
    Ok(file_name)
}

#[tauri::command]
pub fn reveal_warp_profile(app: tauri::AppHandle) -> Result<(), &'static str> {
    let file = PROFILE_STATE
        .try_lock()
        .ok()
        .and_then(|state| state.last_file.clone())
        .filter(|path| path.is_file())
        .ok_or("warp_profile_reveal_error")?;
    app.opener()
        .reveal_item_in_dir(file)
        .map_err(|_| "warp_profile_reveal_error")
}

async fn register(
    client: &reqwest::Client,
    public_key: String,
) -> Result<TunnelConfig, &'static str> {
    let accepted_at = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|_| "warp_profile_registration_error")?;
    let mut device_url =
        Url::parse(REGISTRATION_URL).map_err(|_| "warp_profile_registration_error")?;
    let response = send(client.post(device_url.clone()).json(&json!({
        "key": public_key,
        "tos": accepted_at,
    })))
    .await?;
    let registration = read_registration(response).await?;
    if registration.id.is_empty()
        || !registration
            .id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        || registration.token.is_empty()
    {
        return Err("warp_profile_response_error");
    }
    device_url
        .path_segments_mut()
        .map_err(|_| "warp_profile_response_error")?
        .push(&registration.id);
    send(
        client
            .patch(device_url)
            .bearer_auth(registration.token.as_str())
            .json(&json!({ "warp_enabled": true })),
    )
    .await?;
    Ok(registration.config)
}

async fn send(request: RequestBuilder) -> Result<Response, &'static str> {
    let response = request
        .header("User-Agent", "okhttp/3.12.1")
        .header("CF-Client-Version", "a-6.11-2223")
        .send()
        .await
        .map_err(|_| "warp_profile_network_error")?;
    match response.status().as_u16() {
        200..=299 => Ok(response),
        429 => Err("warp_profile_rate_limited"),
        _ => Err("warp_profile_registration_error"),
    }
}

async fn read_registration(mut response: Response) -> Result<Registration, &'static str> {
    let mut body = Zeroizing::new(Vec::new());
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "warp_profile_network_error")?
    {
        if chunk.len() > RESPONSE_LIMIT - body.len() {
            return Err("warp_profile_response_error");
        }
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&body).map_err(|_| "warp_profile_response_error")
}

fn validate_endpoint(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return Some(DEFAULT_ENDPOINT.into());
    }
    if let Ok(address) = value.parse::<SocketAddr>() {
        return (address.port() != 0).then(|| address.to_string());
    }
    let (host, port) = value.rsplit_once(':')?;
    let port = port.parse::<u16>().ok().filter(|port| *port != 0)?;
    valid_hostname(host.trim_end_matches('.')).then(|| format!("{host}:{port}"))
}

fn valid_hostname(host: &str) -> bool {
    host.len() <= 253
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
}

fn select_masking_domain(domain: &str) -> Result<&str, &'static str> {
    let domain = domain.trim();
    if domain.is_empty() {
        return Ok(MASKING_DOMAINS[rand::random_range(0..MASKING_DOMAINS.len())]);
    }
    if domain.contains('.') && valid_hostname(domain) && domain.parse::<Ipv4Addr>().is_err() {
        return Ok(domain);
    }
    Err("warp_profile_invalid_masking_domain")
}

fn random_endpoint(previous: Option<SocketAddr>) -> SocketAddr {
    // Consumer WARP's ingress range and WireGuard UDP ports.
    // https://developers.cloudflare.com/cloudflare-one/team-and-resources/devices/cloudflare-one-client/deployment/firewall/
    let candidates = (1..=10)
        .flat_map(|host| {
            WARP_PORTS.map(|port| SocketAddr::from((Ipv4Addr::new(162, 159, 192, host), port)))
        })
        .filter(|candidate| Some(*candidate) != previous)
        .collect::<Vec<_>>();
    candidates[rand::random_range(0..candidates.len())]
}

fn interface_address<'a>(value: &'a str, prefix: &str) -> &'a str {
    value.strip_suffix(prefix).unwrap_or(value)
}

fn build_profile(
    key: &PrivateKey,
    config: &TunnelConfig,
    options: &ProfileOptions,
    endpoint: &str,
    masking_domain: Option<&str>,
    service_routes: Option<&[ipnet::IpNet]>,
) -> Result<Zeroizing<String>, &'static str> {
    let invalid = "warp_profile_response_error";
    let public_key = &config.peers.first().ok_or(invalid)?.public_key;
    if !STANDARD
        .decode(public_key)
        .is_ok_and(|bytes| bytes.len() == 32)
    {
        return Err(invalid);
    }
    let addresses = &config.interface.addresses;
    let ipv4 = interface_address(&addresses.v4, "/32")
        .parse::<Ipv4Addr>()
        .map_err(|_| invalid)?;
    let mut address = format!("{ipv4}/32");
    let has_ipv6 = options.ipv6 && !addresses.v6.is_empty();
    if has_ipv6 {
        let ipv6 = interface_address(&addresses.v6, "/128")
            .parse::<Ipv6Addr>()
            .map_err(|_| invalid)?;
        address.push_str(&format!(", {ipv6}/128"));
    }
    let allowed_ips = routes::format_allowed_ips(service_routes, has_ipv6)?;
    let dns = if service_routes.is_none() {
        options
            .dns
            .servers(has_ipv6)
            .map_or_else(String::new, |servers| format!("DNS = {servers}\n"))
    } else {
        String::new()
    };
    let lan_exclusions = if !options.exclude_lan {
        ""
    } else if has_ipv6 {
        "DisallowedIPs = 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16, 169.254.0.0/16, fc00::/7, fe80::/10\n"
    } else {
        "DisallowedIPs = 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16, 169.254.0.0/16\n"
    };
    let keepalive = if options.keepalive == 0 {
        String::new()
    } else {
        format!("PersistentKeepalive = {}\n", options.keepalive)
    };
    // WireSock recommends disabling junk packets while emulating QUIC.
    let masking = masking_domain.map_or_else(String::new, |domain| {
        format!("Jc = 0\nId = {domain}\nIp = QUIC\nIb = cURL\n")
    });
    let private_key: Curve25519SeedBin<'static> =
        key.as_be_bytes().map_err(|_| "warp_profile_key_error")?;
    let private_key = Zeroizing::new(STANDARD.encode(private_key.as_ref()));
    Ok(Zeroizing::new(format!(
        "[Interface]\nPrivateKey = {}\nAddress = {address}\n{dns}MTU = {}\n{masking}\n\
         [Peer]\nPublicKey = {public_key}\nAllowedIPs = {allowed_ips}\nEndpoint = {endpoint}\n{lan_exclusions}{keepalive}",
        private_key.as_str(), options.mtu,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_server_addresses_without_allowing_config_injection() {
        assert_eq!(validate_endpoint("  ").as_deref(), Some(DEFAULT_ENDPOINT));
        assert_eq!(
            validate_endpoint("vpn.example:2408").as_deref(),
            Some("vpn.example:2408")
        );
        assert_eq!(
            validate_endpoint("192.0.2.1:2408").as_deref(),
            Some("192.0.2.1:2408")
        );
        assert_eq!(
            validate_endpoint("[2001:db8::1]:2408").as_deref(),
            Some("[2001:db8::1]:2408")
        );
        for invalid in [
            "https://vpn.example:443",
            "example.com:0",
            "example.com:65536",
            "2001:db8::1:443",
            "vpn.example:443\nDNS = 1.2.3.4",
        ] {
            assert_eq!(validate_endpoint(invalid), None, "{invalid}");
        }
    }

    #[test]
    fn chooses_a_masking_domain_and_cloudflare_port_without_using_arbitrary_destinations() {
        let value = select_masking_domain("").unwrap();
        assert!(MASKING_DOMAINS.contains(&value));
        assert_eq!(
            select_masking_domain(" www.example.com "),
            Ok("www.example.com")
        );
        for invalid in [
            "https://example.com",
            "example.com:443",
            "192.0.2.1",
            "example.com\nIp = DNS",
        ] {
            assert_eq!(
                select_masking_domain(invalid),
                Err("warp_profile_invalid_masking_domain")
            );
        }
        let socket = random_endpoint(None);
        assert!(WARP_PORTS.contains(&socket.port()));
        assert!((1..=10).any(|host| socket.ip() == Ipv4Addr::new(162, 159, 192, host)));
        for _ in 0..40 {
            assert_ne!(random_endpoint(Some(socket)), socket);
        }
    }

    #[test]
    fn serializes_the_registered_wireguard_key_and_validates_peer_data() {
        let key = PrivateKey::generate(&X25519).unwrap();
        let registration: Registration = serde_json::from_value(json!({
            "id": "test-device",
            "token": "test-token",
            "config": {
                "interface": { "addresses": { "v4": "172.16.0.2", "v6": "2001:db8::2/128" } },
                "peers": [{ "public_key": STANDARD.encode([1; 32]) }],
            },
        }))
        .unwrap();
        let mut config = registration.config;
        let mut options: ProfileOptions = serde_json::from_value(json!({
            "endpoint": "", "randomEndpoint": true, "maskingDomain": "",
            "services": null, "dns": "cloudflare", "excludeLan": false,
            "ipv6": true, "mtu": 1280, "keepalive": 0
        }))
        .unwrap();
        let profile = build_profile(
            &key,
            &config,
            &options,
            DEFAULT_ENDPOINT,
            Some("www.example.com"),
            None,
        )
        .unwrap();
        let encoded = profile
            .lines()
            .find_map(|line| line.strip_prefix("PrivateKey = "))
            .unwrap();
        let decoded = STANDARD.decode(encoded).unwrap();
        let private_key = PrivateKey::from_private_key(&X25519, &decoded).unwrap();
        assert_eq!(
            private_key.compute_public_key().unwrap().as_ref(),
            key.compute_public_key().unwrap().as_ref()
        );
        assert!(profile.contains("Address = 172.16.0.2/32, 2001:db8::2/128\n"));
        assert!(profile.contains("AllowedIPs = 0.0.0.0/0, ::/0\n"));
        assert!(profile.contains(&format!("Endpoint = {DEFAULT_ENDPOINT}\n")));
        assert!(profile
            .contains("DNS = 1.1.1.1, 1.0.0.1, 2606:4700:4700::1111, 2606:4700:4700::1001\n"));
        assert!(profile.contains("Ip = QUIC\n"));
        assert!(!profile.contains("PersistentKeepalive = "));
        assert!(!profile.contains("test-token"));

        let routes = [
            "91.108.56.0/22".parse().unwrap(),
            "2a0a:f280::/32".parse().unwrap(),
        ];
        let profile = build_profile(
            &key,
            &config,
            &options,
            DEFAULT_ENDPOINT,
            Some("www.example.com"),
            Some(&routes),
        )
        .unwrap();
        assert!(profile.contains("Jc = 0\nId = www.example.com\nIp = QUIC\nIb = cURL\n"));
        assert!(profile.contains("AllowedIPs = 91.108.56.0/22, 2a0a:f280::/32\n"));
        assert!(!profile.contains("DNS = "));

        options.ipv6 = false;
        options.dns = DnsPreset::Google;
        options.exclude_lan = true;
        options.keepalive = 25;
        options.mtu = 1200;
        let profile = build_profile(&key, &config, &options, DEFAULT_ENDPOINT, None, None).unwrap();
        assert!(profile.contains("Address = 172.16.0.2/32\n"));
        assert!(profile.contains("AllowedIPs = 0.0.0.0/0\n"));
        assert!(profile.contains("DNS = 8.8.8.8, 8.8.4.4\nMTU = 1200\n"));
        assert!(profile.contains("DisallowedIPs = 10.0.0.0/8"));
        assert!(profile.contains("PersistentKeepalive = 25\n"));
        assert!(!profile.contains("Ip = QUIC"));
        assert!(!profile.contains("fc00::/7"));

        config.peers[0].public_key.push_str("\nEndpoint = unwanted");
        assert!(matches!(
            build_profile(&key, &config, &options, DEFAULT_ENDPOINT, None, None),
            Err("warp_profile_response_error")
        ));
    }
}
