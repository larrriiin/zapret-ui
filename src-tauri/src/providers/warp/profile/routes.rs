use futures_util::future::try_join_all;
use ipnet::IpNet;
use serde::Deserialize;
use std::net::IpAddr;

const MAX_LIST_BYTES: usize = 128 * 1024;
const LOAD_ERROR: &str = "warp_profile_routes_error";

#[derive(Clone, Copy, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "lowercase")]
pub(super) enum Service {
    Youtube,
    Discord,
    Telegram,
}

impl Service {
    fn sources(self) -> &'static [&'static str] {
        match self {
            Self::Youtube => &[
                "https://raw.githubusercontent.com/touhidurrr/iplist-youtube/main/lists/cidr4.txt",
                "https://raw.githubusercontent.com/touhidurrr/iplist-youtube/main/lists/cidr6.txt",
            ],
            Self::Discord => &[
                "https://raw.githubusercontent.com/itdoginfo/allow-domains/main/Subnets/IPv4/discord.lst",
                "https://raw.githubusercontent.com/itdoginfo/allow-domains/main/Subnets/IPv6/discord.lst",
            ],
            Self::Telegram => &["https://core.telegram.org/resources/cidr.txt"],
        }
    }
}

pub(super) async fn fetch(
    client: &reqwest::Client,
    mut services: Vec<Service>,
) -> Result<Vec<IpNet>, &'static str> {
    services.sort_unstable();
    services.dedup();
    let urls = services.into_iter().flat_map(Service::sources).copied();
    let lists = try_join_all(urls.map(|url| fetch_list(client, url))).await?;
    Ok(IpNet::aggregate(
        &lists.into_iter().flatten().collect::<Vec<_>>(),
    ))
}

async fn fetch_list(client: &reqwest::Client, url: &str) -> Result<Vec<IpNet>, &'static str> {
    let mut response = client.get(url).send().await.map_err(|_| LOAD_ERROR)?;
    if !response.status().is_success() {
        return Err(LOAD_ERROR);
    }
    let mut data = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| LOAD_ERROR)? {
        if chunk.len() > MAX_LIST_BYTES - data.len() {
            return Err(LOAD_ERROR);
        }
        data.extend_from_slice(&chunk);
    }
    parse_list(std::str::from_utf8(&data).map_err(|_| LOAD_ERROR)?)
}

fn parse_list(text: &str) -> Result<Vec<IpNet>, &'static str> {
    let mut routes = Vec::new();
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let net: IpNet = line.parse().map_err(|_| LOAD_ERROR)?;
        let public = match net.network() {
            IpAddr::V4(ip) => {
                net.prefix_len() >= 8
                    && !ip.is_private()
                    && !ip.is_loopback()
                    && !ip.is_link_local()
                    && !ip.is_multicast()
                    && !ip.is_broadcast()
                    && ip.octets()[0] != 0
                    && ip.octets()[0] < 240
            }
            IpAddr::V6(ip) => net.prefix_len() >= 16 && ip.segments()[0] & 0xe000 == 0x2000,
        };
        if public {
            routes.push(net.trunc());
        }
    }
    if routes.is_empty() {
        return Err(LOAD_ERROR);
    }
    Ok(routes)
}

pub(super) fn format_allowed_ips(
    routes: Option<&[IpNet]>,
    has_ipv6: bool,
) -> Result<String, &'static str> {
    let Some(routes) = routes else {
        return Ok(if has_ipv6 {
            "0.0.0.0/0, ::/0"
        } else {
            "0.0.0.0/0"
        }
        .into());
    };
    let routes = routes
        .iter()
        .filter(|net| has_ipv6 || matches!(net, IpNet::V4(_)))
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    if routes.is_empty() {
        return Err(LOAD_ERROR);
    }
    Ok(routes.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_routes_exclude_defaults_local_networks_and_unsupported_address_families() {
        let parsed = parse_list(
            "# routes\n0.0.0.0/0\n::/0\n10.0.0.0/8\nfc00::/7\n91.108.56.12/22\n2a0a:f280::/32",
        )
        .unwrap();
        assert_eq!(
            format_allowed_ips(Some(&parsed), false),
            Ok("91.108.56.0/22".into())
        );
        assert_eq!(
            format_allowed_ips(Some(&parsed), true),
            Ok("91.108.56.0/22, 2a0a:f280::/32".into())
        );
        assert_eq!(parse_list("0.0.0.0/0\n::/0"), Err(LOAD_ERROR));
        assert_eq!(
            parse_list("91.108.56.0/22\nEndpoint = other"),
            Err(LOAD_ERROR)
        );
        assert_eq!(format_allowed_ips(Some(&[]), true), Err(LOAD_ERROR));
    }
}
