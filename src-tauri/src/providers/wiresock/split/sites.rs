use futures_util::{stream, StreamExt, TryStreamExt};
use ipnet::IpNet;
use std::{collections::BTreeSet, net::IpAddr, time::Duration};
use zeroize::Zeroizing;

use super::section;
use crate::providers::warp::sites::normalize_domains;

const SITES: &str = "# ZAPRET UI sites = ";
const ORIGINAL: &str = "# ZAPRET UI original AllowedIPs = ";
const ROUTES_ERROR: &str = "wiresock_split_sites_routes";
const DNS_ERROR: &str = "wiresock_split_sites_dns";

#[derive(Default)]
pub(in crate::providers) struct SiteRules(Vec<(String, Vec<IpAddr>)>);

impl SiteRules {
    pub(in crate::providers) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub(in crate::providers) async fn resolve(sites: &[String]) -> Result<Self, &'static str> {
        let sites = normalize_domains(sites.to_vec()).map_err(|_| "warp_sites_invalid")?;
        if sites.is_empty() {
            return Ok(Self::default());
        }
        let mut resolved: Vec<_> = tokio::time::timeout(
            Duration::from_secs(15),
            stream::iter(sites.into_iter().map(|site| async move {
                let addresses = tokio::net::lookup_host((format!("{site}."), 0))
                    .await
                    .map_err(|_| DNS_ERROR)?
                    .map(|address| address.ip().to_canonical())
                    .filter(|ip| !(ip.is_unspecified() || ip.is_loopback() || ip.is_multicast()))
                    .collect();
                Ok::<_, &'static str>((site, addresses))
            }))
            .buffer_unordered(12)
            .try_collect(),
        )
        .await
        .map_err(|_| DNS_ERROR)??;
        resolved.sort_unstable_by(|(a, _), (b, _)| a.cmp(b));
        Ok(Self(resolved))
    }

    pub(in crate::providers) fn apply(
        &self,
        profile: &mut Zeroizing<String>,
    ) -> Result<(), &'static str> {
        let parsed = site_section(profile);
        if self.is_empty() && parsed.as_ref().is_none_or(|peer| peer.sites.is_empty()) {
            return Ok(());
        }
        let peer = parsed.ok_or(ROUTES_ERROR)?;
        let newline = if profile.contains("\r\n") {
            "\r\n"
        } else {
            "\n"
        };
        let original = peer.original.unwrap_or(peer.allowed);
        let replacement = if self.is_empty() {
            format!("AllowedIPs = {original}{newline}")
        } else {
            let networks = parse_networks(original).ok_or(ROUTES_ERROR)?;
            let mut routes = BTreeSet::new();
            for (_, addresses) in &self.0 {
                let mut matching = addresses
                    .iter()
                    .filter(|&ip| networks.iter().any(|net| net.contains(ip)))
                    .peekable();
                if matching.peek().is_none() {
                    return Err(DNS_ERROR);
                }
                routes.extend(matching.copied().map(IpNet::from));
            }
            let sites =
                serde_json::to_string(&self.0.iter().map(|(site, _)| site).collect::<Vec<_>>())
                    .map_err(|_| ROUTES_ERROR)?;
            let allowed = routes
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            format!("{SITES}{sites}{newline}{ORIGINAL}{original}{newline}AllowedIPs = {allowed}{newline}")
        };
        let mut output = Zeroizing::new(String::with_capacity(profile.len() + replacement.len()));
        for line in profile.split_inclusive('\n') {
            let trimmed = line.trim();
            if allowed_ips(trimmed).is_some() {
                output.push_str(&replacement);
            } else if !trimmed.starts_with(SITES) && !trimmed.starts_with(ORIGINAL) {
                output.push_str(line);
            }
        }
        *profile = output;
        Ok(())
    }
}

struct SiteSection<'a> {
    allowed: &'a str,
    original: Option<&'a str>,
    sites: Vec<String>,
}

fn allowed_ips(line: &str) -> Option<&str> {
    let (key, value) = line.split_once('=')?;
    key.trim()
        .eq_ignore_ascii_case("AllowedIPs")
        .then(|| value.trim())
}

fn parse_networks(value: &str) -> Option<Vec<IpNet>> {
    value
        .split(['#', ';'])
        .next()?
        .split(',')
        .map(|entry| {
            let entry = entry.trim();
            entry
                .parse::<IpNet>()
                .ok()
                .or_else(|| entry.parse::<IpAddr>().ok().map(IpNet::from))
        })
        .collect()
}

fn site_section(profile: &str) -> Option<SiteSection<'_>> {
    let mut in_peer = false;
    let mut peers = 0;
    let mut allowed = None;
    let mut original = None;
    let mut sites = None;
    for line in profile
        .trim_start_matches('\u{feff}')
        .lines()
        .map(str::trim)
    {
        if let Some(section) = section(line) {
            in_peer = section.eq_ignore_ascii_case("Peer");
            peers += usize::from(in_peer);
        } else if let Some(value) = allowed_ips(line) {
            if !in_peer || allowed.replace(value).is_some() {
                return None;
            }
        } else if let Some(value) = line.strip_prefix(ORIGINAL) {
            if !in_peer || original.replace(value).is_some() {
                return None;
            }
        } else if let Some(value) = line.strip_prefix(SITES) {
            if !in_peer
                || sites
                    .replace(serde_json::from_str::<Vec<String>>(value).ok()?)
                    .is_some()
            {
                return None;
            }
        }
    }
    let allowed = allowed?;
    if peers != 1 || original.is_some() != sites.is_some() {
        return None;
    }
    parse_networks(original.unwrap_or(allowed))?;
    Some(SiteSection {
        allowed,
        original,
        sites: sites.unwrap_or_default(),
    })
}

pub(super) fn saved_sites(profile: &str) -> Option<Vec<String>> {
    site_section(profile).map(|peer| peer.sites)
}

pub(super) fn reset_profile_routes(profile: &str) -> Result<Zeroizing<String>, &'static str> {
    reset_routes(profile, "0.0.0.0/0", "::/0")
}

pub(super) fn full_tunnel_routes(profile: &str) -> Result<Zeroizing<String>, &'static str> {
    // Both halves are equivalent to the default route, while the explicit
    // list prevents WireSock from falling back to its own global rules.
    reset_routes(profile, "0.0.0.0/1, 128.0.0.0/1", "::/1, 8000::/1")
}

fn reset_routes(profile: &str, ipv4: &str, ipv6: &str) -> Result<Zeroizing<String>, &'static str> {
    let peer = site_section(profile).ok_or(ROUTES_ERROR)?;
    let networks = parse_networks(peer.original.unwrap_or(peer.allowed)).ok_or(ROUTES_ERROR)?;
    let routes: Vec<_> = [
        (networks.iter().any(|net| matches!(net, IpNet::V4(_))), ipv4),
        (networks.iter().any(|net| matches!(net, IpNet::V6(_))), ipv6),
    ]
    .into_iter()
    .filter_map(|(has_family, route)| has_family.then_some(route))
    .collect();
    let newline = if profile.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut output = Zeroizing::new(String::with_capacity(profile.len()));
    let mut in_peer = false;
    for line in profile.split_inclusive('\n') {
        let trimmed = line.trim_start_matches('\u{feff}').trim();
        if let Some(name) = section(trimmed) {
            in_peer = name.eq_ignore_ascii_case("Peer");
        }
        if in_peer && allowed_ips(trimmed).is_some() {
            output.push_str(&format!("AllowedIPs = {}{newline}", routes.join(", ")));
            continue;
        }
        let directive = match trimmed.get(..5) {
            Some(prefix) if prefix.eq_ignore_ascii_case("#@ws:") => &trimmed[5..],
            _ => trimmed,
        };
        let key = directive.split_once('=').map(|(key, _)| key.trim());
        if in_peer
            && (trimmed.starts_with(SITES)
                || trimmed.starts_with(ORIGINAL)
                || key.is_some_and(|key| key.eq_ignore_ascii_case("DisallowedIPs")))
        {
            continue;
        }
        output.push_str(line);
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn site_rules_round_trip_and_stay_inside_original_allowed_ips() {
        let original = "[Interface]\r\nPrivateKey = test\r\n[Peer]\r\nAllowedIPs = 192.0.2.0/24, ::/0 # keep\r\nEndpoint = example.com:2408";
        let mut profile = Zeroizing::new(original.into());
        let rules = SiteRules(vec![(
            "example.com".into(),
            ["192.0.2.1", "198.51.100.2", "2001:db8::1"]
                .into_iter()
                .map(|ip| ip.parse().unwrap())
                .collect(),
        )]);
        rules.apply(&mut profile).unwrap();
        assert!(profile.contains("\r\nAllowedIPs = 192.0.2.1/32, 2001:db8::1/128\r\n"));
        assert_eq!(saved_sites(&profile).unwrap(), ["example.com"]);
        rules.apply(&mut profile).unwrap();
        assert_eq!(profile.matches(ORIGINAL).count(), 1);
        SiteRules::default().apply(&mut profile).unwrap();
        assert_eq!(profile.as_str(), original);
        let unreachable = SiteRules(vec![(
            "other.example".into(),
            vec!["198.51.100.2".parse().unwrap()],
        )]);
        assert_eq!(unreachable.apply(&mut profile), Err(DNS_ERROR));
        assert_eq!(profile.as_str(), original);
        assert!(saved_sites("[Peer]\nAllowedIPs = 0.0.0.0/0\n[Peer]\nAllowedIPs = ::/0").is_none());
    }

    #[test]
    fn override_replaces_ip_filter_but_keeps_peer_and_address_family() {
        let config = "[Interface]\nAddress = 192.0.2.10/32\n[Peer]\nAllowedIPs = 192.0.2.0/24\n#@ws:DisallowedIPs = 192.0.2.6/32\nEndpoint = example.com:2408\n";
        let overridden = reset_profile_routes(config).unwrap();
        assert_eq!(overridden.as_str(), "[Interface]\nAddress = 192.0.2.10/32\n[Peer]\nAllowedIPs = 0.0.0.0/0\nEndpoint = example.com:2408\n");
        assert!(reset_profile_routes(&format!("{config}[Peer]\nAllowedIPs = ::/0\n")).is_err());
    }
}
