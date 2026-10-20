use super::{FlowKey, Protocol};
use serde::Serialize;
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    time::Instant,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Route {
    Direct,
    Warp,
    TgProxy,
    TgProxyWarp,
    Local,
    Unknown,
}

#[derive(Clone, Debug)]
pub(crate) struct ProxyRoute {
    pub local: SocketAddr,
    pub remote: SocketAddr,
    pub closed: Option<Instant>,
}
impl ProxyRoute {
    pub fn new(local: SocketAddr, remote: SocketAddr) -> Self {
        Self {
            local,
            remote,
            closed: None,
        }
    }
}
#[derive(Clone, Debug, Default)]
pub(super) struct RouteContext {
    pub warp: HashMap<(SocketAddr, SocketAddr), Option<Instant>>,
    pub telegram_pid: Option<u32>,
    pub telegram_listener: Option<SocketAddr>,
}
impl RouteContext {
    pub fn new(
        warp: Vec<ProxyRoute>,
        telegram_pid: Option<u32>,
        telegram_listener: Option<SocketAddr>,
    ) -> Self {
        let mut result = Self {
            telegram_pid,
            telegram_listener,
            ..Default::default()
        };
        for route in warp {
            for pair in [(route.local, route.remote), (route.remote, route.local)] {
                result
                    .warp
                    .entry(pair)
                    .and_modify(|old| {
                        *old = match (*old, route.closed) {
                            (Some(a), Some(b)) => Some(a.max(b)),
                            _ => None,
                        };
                    })
                    .or_insert(route.closed);
            }
        }
        result
    }
    pub fn classify(&self, key: &FlowKey, pid: u32, interface: Route, started: Instant) -> Route {
        let telegram = self.telegram_pid == Some(pid)
            || (key.protocol == Protocol::Tcp
                && self.telegram_listener.is_some_and(|listener| {
                    SocketAddr::new(key.remote_address, key.remote_port) == listener
                        || SocketAddr::new(key.local_address, key.local_port) == listener
                        || (listener.ip().is_unspecified()
                            && key.remote_port == listener.port()
                            && (key.remote_address.is_loopback()
                                || key.remote_address == key.local_address))
                }));
        let warp = (key.protocol == Protocol::Tcp
            && self
                .warp
                .get(&(
                    SocketAddr::new(key.local_address, key.local_port),
                    SocketAddr::new(key.remote_address, key.remote_port),
                ))
                .is_some_and(|closed| closed.is_none_or(|end| started <= end)))
            || interface == Route::Warp;
        if telegram {
            if warp {
                Route::TgProxyWarp
            } else {
                Route::TgProxy
            }
        } else if warp {
            Route::Warp
        } else if local(key.remote_address) {
            Route::Local
        } else {
            interface
        }
    }
}
fn local(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            ip.is_loopback()
                || ip.is_private()
                || ip.is_link_local()
                || ip.is_unspecified()
                || ip.is_multicast()
        }
        IpAddr::V6(ip) => {
            ip.is_loopback()
                || ip.is_unique_local()
                || ip.is_unicast_link_local()
                || ip.is_unspecified()
                || ip.is_multicast()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn key(remote: &str) -> FlowKey {
        FlowKey {
            protocol: Protocol::Tcp,
            local_address: "192.168.1.2".parse().unwrap(),
            local_port: 50000,
            remote_address: remote.parse().unwrap(),
            remote_port: 443,
        }
    }
    #[test]
    fn warp_requires_an_exact_flow_or_observed_interface() {
        let now = Instant::now();
        let flow = key("1.1.1.1");
        let context = RouteContext::new(
            vec![ProxyRoute::new(
                "192.168.1.2:50000".parse().unwrap(),
                "1.1.1.1:443".parse().unwrap(),
            )],
            None,
            None,
        );
        assert_eq!(context.classify(&flow, 10, Route::Direct, now), Route::Warp);
        let mut other = flow.clone();
        other.local_port += 1;
        assert_eq!(
            context.classify(&other, 10, Route::Direct, now),
            Route::Direct
        );
        assert_eq!(
            RouteContext::default().classify(&flow, 10, Route::Unknown, now),
            Route::Unknown
        );
        assert_eq!(
            RouteContext::default().classify(&flow, 10, Route::Warp, now),
            Route::Warp
        );
        other.protocol = Protocol::Udp;
        assert_eq!(
            context.classify(&other, 10, Route::Direct, now),
            Route::Direct
        );
    }
    #[test]
    fn proxy_precedes_local_and_can_chain_through_warp() {
        let now = Instant::now();
        let mut flow = key("127.0.0.1");
        flow.remote_port = 1443;
        let context = RouteContext::new(vec![], Some(123), Some("127.0.0.1:1443".parse().unwrap()));
        assert_eq!(
            context.classify(&flow, 10, Route::Direct, now),
            Route::TgProxy
        );
        let external = key("203.0.113.7");
        assert_eq!(
            context.classify(&external, 123, Route::Warp, now),
            Route::TgProxyWarp
        );
        assert_eq!(
            context.classify(&external, 124, Route::Direct, now),
            Route::Direct
        );
        for remote in ["172.20.0.1", "169.254.1.1", "::1", "fc00::1", "fe80::1"] {
            assert_eq!(
                RouteContext::default().classify(&key(remote), 10, Route::Unknown, now),
                Route::Local
            );
        }
    }
    #[test]
    fn old_proxy_mapping_does_not_label_a_reused_tuple() {
        let now = Instant::now();
        let end = now - std::time::Duration::from_secs(1);
        let mut route = ProxyRoute::new(
            "192.168.1.2:50000".parse().unwrap(),
            "1.1.1.1:443".parse().unwrap(),
        );
        route.closed = Some(end);
        let context = RouteContext::new(vec![route], None, None);
        assert_eq!(
            context.classify(&key("1.1.1.1"), 10, Route::Direct, now),
            Route::Direct
        );
        assert_eq!(
            context.classify(&key("1.1.1.1"), 10, Route::Direct, end),
            Route::Warp
        );
    }
}
