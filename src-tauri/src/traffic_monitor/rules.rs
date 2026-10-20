use super::{split_command_line, IpSetMatcher, Protocol};
use serde::Serialize;
use std::{collections::HashSet, net::IpAddr, path::Path};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ZapretState {
    Expected,
    Excluded,
    NotMatched,
    Unknown,
    Off,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(super) struct Decision {
    pub state: ZapretState,
    pub reason: &'static str,
    pub profile: Option<usize>,
}
impl Decision {
    pub fn new(state: ZapretState, reason: &'static str) -> Self {
        Self {
            state,
            reason,
            profile: None,
        }
    }
    pub fn candidate(&self) -> bool {
        self.state == ZapretState::Expected
    }
}

#[derive(Clone, Debug, Default)]
pub(super) struct Evidence {
    pub hostname: Option<String>,
    pub protocol: Option<&'static str>,
}

#[derive(Clone, Debug, Default)]
struct Domains {
    suffixes: HashSet<String>,
    exact: HashSet<String>,
    invalid: bool,
}
impl Domains {
    fn add(&mut self, raw: &str) {
        let name = raw
            .trim()
            .trim_start_matches('\u{feff}')
            .to_ascii_lowercase();
        let (exact, name) = name
            .strip_prefix('^')
            .map_or((false, name.as_str()), |s| (true, s));
        let name = name.trim_end_matches('.');
        if name.is_empty() {
            return;
        }
        if name.len() > 253
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
        {
            self.invalid = true;
        } else if exact {
            self.exact.insert(name.into());
        } else {
            self.suffixes.insert(name.into());
        }
    }
    fn load(&mut self, path: &str) {
        match std::fs::read_to_string(path) {
            Ok(content) => {
                for line in content.lines() {
                    self.add(line.split('#').next().unwrap_or_default());
                }
            }
            Err(_) => self.invalid = true,
        }
    }
    fn empty(&self) -> bool {
        self.suffixes.is_empty() && self.exact.is_empty()
    }
    fn contains(&self, hostname: &str) -> bool {
        let name = hostname.trim_end_matches('.').to_ascii_lowercase();
        if self.exact.contains(&name) {
            return true;
        }
        let mut suffix = name.as_str();
        loop {
            if self.suffixes.contains(suffix) {
                return true;
            }
            match suffix.split_once('.') {
                Some((_, rest)) => suffix = rest,
                None => return false,
            }
        }
    }
}

#[derive(Clone, Debug, Default)]
struct Ports {
    ranges: Vec<(u16, u16, bool)>,
    specified: bool,
    invalid: bool,
}
impl Ports {
    fn add(&mut self, value: &str) {
        self.specified = true;
        for value in value.split(',') {
            let (negated, value) = value
                .strip_prefix('~')
                .map_or((false, value), |s| (true, s));
            let parsed = if value == "*" {
                Some((0, u16::MAX))
            } else if let Some((start, end)) = value.split_once('-') {
                start
                    .parse::<u16>()
                    .ok()
                    .zip(end.parse::<u16>().ok())
                    .filter(|(a, b)| a <= b)
            } else {
                value.parse::<u16>().ok().map(|p| (p, p))
            };
            if let Some((start, end)) = parsed {
                self.ranges.push((start, end, negated));
            } else {
                self.invalid = true;
            }
        }
        // Mixed positive/negative selectors vary between core versions. Do not guess.
        if self.ranges.iter().any(|r| r.2) && self.ranges.iter().any(|r| !r.2) {
            self.invalid = true;
        }
    }
    fn contains(&self, port: u16) -> Option<bool> {
        if self.invalid {
            return None;
        }
        if self.ranges.iter().any(|r| r.2) {
            Some(
                self.ranges
                    .iter()
                    .all(|(a, b, _)| !(*a..=*b).contains(&port)),
            )
        } else {
            Some(
                self.ranges
                    .iter()
                    .any(|(a, b, _)| (*a..=*b).contains(&port)),
            )
        }
    }
}

#[derive(Clone, Debug, Default)]
struct Profile {
    tcp: Ports,
    udp: Ports,
    includes: IpSetMatcher,
    excludes: IpSetMatcher,
    hosts: Domains,
    excluded_hosts: Domains,
    l7: Vec<String>,
    l3: Option<String>,
    auto: bool,
    action: bool,
    unsupported: bool,
}
enum Match {
    No(bool),
    Unknown(&'static str),
    Yes(&'static str),
}
impl Profile {
    fn matches(
        &self,
        protocol: Protocol,
        port: u16,
        address: IpAddr,
        evidence: &Evidence,
    ) -> Match {
        if let Some(l3) = &self.l3 {
            if !matches!(l3.as_str(), "ipv4" | "ipv6") {
                return Match::Unknown("unsupported");
            }
            if (l3 == "ipv4") != address.is_ipv4() {
                return Match::No(false);
            }
        }
        let ports = match protocol {
            Protocol::Tcp => &self.tcp,
            Protocol::Udp => &self.udp,
        };
        if self.tcp.specified || self.udp.specified {
            match ports.contains(port) {
                Some(true) => (),
                Some(false) => return Match::No(false),
                None => return Match::Unknown("unsupported"),
            }
        }
        // A known protocol mismatch is decisive even before SNI is available.
        if !self.l7.is_empty()
            && evidence
                .protocol
                .is_some_and(|value| !self.l7.iter().any(|p| p == value))
        {
            return Match::No(false);
        }
        if self.excludes.contains(address) {
            return Match::No(true);
        }
        if self.includes.invalid || self.excludes.invalid {
            return Match::Unknown("rules_unknown");
        }
        if !self.includes.is_empty() && !self.includes.contains(address) {
            return Match::No(false);
        }
        if self.unsupported || self.hosts.invalid || self.excluded_hosts.invalid {
            return Match::Unknown("unsupported");
        }
        if let Some(host) = &evidence.hostname {
            if self.excluded_hosts.contains(host) {
                return Match::No(true);
            }
            if !self.hosts.empty() && !self.hosts.contains(host) && !self.auto {
                return Match::No(false);
            }
        } else if !self.hosts.empty() || !self.excluded_hosts.empty() {
            return Match::Unknown("waiting_hostname");
        }
        if !self.l7.is_empty() {
            match evidence.protocol {
                Some(value) if self.l7.iter().any(|p| p == value) => (),
                Some(_) => return Match::No(false),
                None => return Match::Unknown("waiting_protocol"),
            }
        }
        if self.auto {
            return Match::Unknown("autohostlist");
        }
        Match::Yes(if !self.hosts.empty() {
            "hostlist"
        } else if !self.includes.is_empty() {
            "ipset"
        } else if !self.l7.is_empty() {
            "l7"
        } else {
            "global"
        })
    }
}

#[derive(Clone, Debug, Default)]
pub(super) struct ActiveZapretRules {
    tcp: Ports,
    udp: Ports,
    profiles: Vec<Profile>,
    pub strategy: Option<String>,
    running: bool,
    known: bool,
    capture_unknown: bool,
}
impl ActiveZapretRules {
    pub fn unknown(strategy: Option<String>) -> Self {
        Self {
            strategy,
            running: true,
            ..Default::default()
        }
    }
    pub fn from_winws_args(args: &str, strategy: Option<String>) -> Self {
        let mut rules = Self {
            running: strategy.is_some(),
            strategy,
            known: true,
            ..Default::default()
        };
        let mut profile = Profile::default();
        for token in split_command_line(args) {
            if let Some(v) = token.strip_prefix("--wf-tcp=") {
                rules.tcp.add(v);
            } else if let Some(v) = token.strip_prefix("--wf-udp=") {
                rules.udp.add(v);
            } else if token == "--new" {
                rules.profiles.push(profile);
                profile = Profile::default();
            } else if let Some(v) = token.strip_prefix("--filter-tcp=") {
                profile.tcp.add(v);
            } else if let Some(v) = token.strip_prefix("--filter-udp=") {
                profile.udp.add(v);
            } else if let Some(v) = token.strip_prefix("--filter-l3=") {
                profile.l3 = Some(v.into());
            } else if let Some(v) = token.strip_prefix("--filter-l7=") {
                profile.l7.extend(v.split(',').map(str::to_owned));
            } else if let Some(v) = token.strip_prefix("--ipset=") {
                profile.includes.merge(IpSetMatcher::from_file(v));
            } else if let Some(v) = token.strip_prefix("--ipset-exclude=") {
                profile.excludes.merge(IpSetMatcher::from_file(v));
            } else if let Some(v) = token.strip_prefix("--hostlist=") {
                profile.hosts.load(v);
            } else if let Some(v) = token.strip_prefix("--hostlist-exclude=") {
                profile.excluded_hosts.load(v);
            } else if let Some(v) = token.strip_prefix("--hostlist-domains=") {
                for name in v.split(',') {
                    profile.hosts.add(name);
                }
            } else if let Some(v) = token.strip_prefix("--hostlist-exclude-domains=") {
                for name in v.split(',') {
                    profile.excluded_hosts.add(name);
                }
            } else if token.starts_with("--hostlist-auto=") {
                profile.auto = true;
            } else if let Some(v) = token.strip_prefix("--dpi-desync=") {
                profile.action = v.split(',').any(|mode| mode != "none" && !mode.is_empty());
            } else if token.starts_with("--wf-") {
                rules.capture_unknown = true;
            } else if token.starts_with("--filter-")
                || token.starts_with("--ipset")
                || token.starts_with("--hostcase")
                || token.starts_with("--hostspell")
                || token.starts_with("--domcase")
                || token.starts_with("--methodeol")
                || token.starts_with("--dup")
                || token.starts_with("--wsize")
                || token.starts_with("--wssize")
            {
                profile.unsupported = true;
            }
        }
        rules.profiles.push(profile);
        rules
    }
    pub fn active(&self) -> bool {
        self.running && self.known
    }
    pub fn classify(
        &self,
        protocol: Protocol,
        port: u16,
        address: IpAddr,
        evidence: &Evidence,
    ) -> Decision {
        if !self.running {
            return Decision::new(ZapretState::Off, "off");
        }
        if !self.known {
            return Decision::new(ZapretState::Unknown, "rules_unknown");
        }
        if self.capture_unknown || (!self.tcp.specified && !self.udp.specified) {
            return Decision::new(ZapretState::Unknown, "unsupported");
        }
        let ports = match protocol {
            Protocol::Tcp => &self.tcp,
            Protocol::Udp => &self.udp,
        };
        match ports.contains(port) {
            Some(false) => return Decision::new(ZapretState::NotMatched, "ports"),
            None => return Decision::new(ZapretState::Unknown, "unsupported"),
            Some(true) => (),
        }
        let mut excluded = false;
        for (index, profile) in self.profiles.iter().enumerate() {
            match profile.matches(protocol, port, address, evidence) {
                Match::No(value) => excluded |= value,
                Match::Unknown(reason) => {
                    return Decision {
                        state: ZapretState::Unknown,
                        reason,
                        profile: Some(index + 1),
                    }
                }
                Match::Yes(reason) => {
                    return Decision {
                        state: if profile.action {
                            ZapretState::Expected
                        } else {
                            ZapretState::NotMatched
                        },
                        reason: if profile.action { reason } else { "no_action" },
                        profile: Some(index + 1),
                    }
                }
            }
        }
        Decision::new(
            if excluded {
                ZapretState::Excluded
            } else {
                ZapretState::NotMatched
            },
            if excluded { "excluded" } else { "no_profile" },
        )
    }
    #[cfg(test)]
    pub fn matches(&self, protocol: Protocol, port: u16, address: IpAddr) -> bool {
        self.classify(protocol, port, address, &Evidence::default())
            .candidate()
    }
    pub fn source_key(args: &str, strategy: Option<&str>) -> String {
        let mut key = format!(
            "{}\n{}",
            strategy.unwrap_or_default(),
            split_command_line(args).join("\n")
        );
        for token in split_command_line(args) {
            let Some((flag, path)) = token.split_once('=') else {
                continue;
            };
            if !matches!(
                flag,
                "--ipset"
                    | "--ipset-exclude"
                    | "--hostlist"
                    | "--hostlist-exclude"
                    | "--hostlist-auto"
            ) {
                continue;
            }
            key.push_str(&format!("\n{path}:"));
            match std::fs::metadata(Path::new(path)) {
                Ok(metadata) => key.push_str(&format!(
                    "{}:{:?}",
                    metadata.len(),
                    metadata.modified().ok()
                )),
                Err(_) => key.push_str("missing"),
            }
        }
        key
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn classify(args: &str, host: Option<&str>, protocol: Option<&'static str>) -> Decision {
        let rules = ActiveZapretRules::from_winws_args(
            &format!("--wf-tcp=443 {args}"),
            Some("test".into()),
        );
        rules.classify(
            Protocol::Tcp,
            443,
            "203.0.113.7".parse().unwrap(),
            &Evidence {
                hostname: host.map(str::to_owned),
                protocol,
            },
        )
    }
    #[test]
    fn first_matching_profile_and_no_action_are_respected() {
        let decision = classify("--filter-tcp=443 --hostlist-domains=example.com --dpi-desync=none --new --filter-tcp=443 --dpi-desync=fake",Some("example.com"),Some("tls"));
        assert_eq!(decision.state, ZapretState::NotMatched);
        assert_eq!(decision.profile, Some(1));
        assert_eq!(decision.reason, "no_action");
        let decision = classify("--filter-tcp=443 --hostlist-domains=example.com --dpi-desync=fake --new --filter-tcp=443 --dpi-desync=multisplit",Some("elsewhere.net"),Some("tls"));
        assert_eq!(decision.state, ZapretState::Expected);
        assert_eq!(decision.profile, Some(2));
    }
    #[test]
    fn exclusion_is_per_profile_and_later_profiles_can_still_match() {
        let first = "--filter-tcp=443 --hostlist-exclude-domains=example.com --dpi-desync=fake";
        let excluded = classify(first, Some("sub.example.com"), Some("tls"));
        assert_eq!(excluded.state, ZapretState::Excluded);
        let later = classify(
            &format!("{first} --new --filter-tcp=443 --dpi-desync=fake"),
            Some("sub.example.com"),
            Some("tls"),
        );
        assert_eq!(later.state, ZapretState::Expected);
        assert_eq!(later.profile, Some(2));
    }
    #[test]
    fn missing_sni_is_unknown_and_does_not_bypass_an_earlier_profile() {
        let args = "--filter-tcp=443 --hostlist-domains=example.com --dpi-desync=none --new --filter-tcp=443 --dpi-desync=fake";
        let unknown = classify(args, None, Some("tls"));
        assert_eq!(unknown.state, ZapretState::Unknown);
        assert_eq!(unknown.profile, Some(1));
        assert_eq!(unknown.reason, "waiting_hostname");
        assert_eq!(
            classify(args, Some("elsewhere.net"), Some("tls")).state,
            ZapretState::Expected
        );
        let mismatch = classify("--filter-tcp=443 --filter-l7=http --hostlist-domains=example.com --dpi-desync=none --new --filter-tcp=443 --dpi-desync=fake",None,Some("tls"));
        assert_eq!(mismatch.state, ZapretState::Expected);
        assert_eq!(mismatch.profile, Some(2));
    }
    #[test]
    fn exact_domains_and_suffix_boundaries() {
        let mut domains = Domains::default();
        domains.add("example.com");
        domains.add("^exact.net");
        for name in ["example.com", "A.EXAMPLE.COM."] {
            assert!(domains.contains(name));
        }
        for name in ["notexample.com", "example.com.evil.net", "sub.exact.net"] {
            assert!(!domains.contains(name));
        }
        assert!(domains.contains("exact.net"));
    }
    #[test]
    fn port_protocol_and_hostname_filters_are_conjunctive() {
        let args =
            "--filter-tcp=443 --hostlist-domains=example.com --filter-l7=tls --dpi-desync=fake";
        assert_eq!(
            classify(args, Some("example.com"), Some("tls")).state,
            ZapretState::Expected
        );
        assert_eq!(
            classify(args, Some("elsewhere.net"), Some("tls")).state,
            ZapretState::NotMatched
        );
        assert_eq!(
            classify(args, Some("example.com"), Some("http")).state,
            ZapretState::NotMatched
        );
        assert_eq!(
            classify(args, Some("example.com"), None).state,
            ZapretState::Unknown
        );
        let rules = ActiveZapretRules::from_winws_args(
            "--wf-tcp=80,443 --filter-tcp=~80 --dpi-desync=fake",
            Some("test".into()),
        );
        assert!(rules.matches(Protocol::Tcp, 443, "203.0.113.7".parse().unwrap()));
        assert!(!rules.matches(Protocol::Tcp, 80, "203.0.113.7".parse().unwrap()));
    }
    #[test]
    fn capture_and_autohostlist_uncertainty_are_explicit() {
        for args in [
            "--wf-raw=outbound --dpi-desync=fake",
            "--dpi-desync=fake",
            "--wf-tcp=443 --filter-tcp=443 --hostlist-auto=auto.txt --dpi-desync=fake",
        ] {
            let rules = ActiveZapretRules::from_winws_args(args, Some("test".into()));
            assert_eq!(
                rules
                    .classify(
                        Protocol::Tcp,
                        443,
                        "203.0.113.7".parse().unwrap(),
                        &Evidence::default()
                    )
                    .state,
                ZapretState::Unknown
            );
        }
    }

    #[test]
    fn process_quoting_does_not_reset_an_unchanged_rule_generation() {
        let launch = r#"--wf-tcp=443 --hostlist="C:\Zapret UI\lists\sites.txt" --dpi-desync=fake"#;
        let process =
            r#"--wf-tcp=443   "--hostlist=C:\Zapret UI\lists\sites.txt" --dpi-desync=fake"#;
        assert_eq!(
            ActiveZapretRules::source_key(launch, Some("test")),
            ActiveZapretRules::source_key(process, Some("test"))
        );
    }
}
