use std::io;
use std::process::Output;

pub(crate) fn validate_output(output: Output) -> io::Result<Output> {
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "Diagnostic command failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(output)
}

pub(crate) fn parse_proxy(output: &str) -> io::Result<Option<&str>> {
    let output = output.trim();
    if output == "PROXY_DISABLED" {
        return Ok(None);
    }
    if let Some(proxy) = output.strip_prefix("PROXY_ENABLED:").map(str::trim) {
        if !proxy.is_empty() && !proxy.contains(['\r', '\n']) {
            return Ok(Some(proxy));
        }
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "Invalid proxy result",
    ))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TcpTimestamps {
    Enabled,
    Disabled,
    Allowed,
}

/// Read the machine-readable commands from `netsh interface tcp dump` without
/// executing them or relying on localized labels in `show global`.
pub(crate) fn parse_tcp_timestamps(output: &str) -> io::Result<TcpTimestamps> {
    let mut result = None;
    for line in output.lines() {
        let mut tokens = line.split_whitespace();
        if tokens.next() != Some("set") || tokens.next() != Some("global") {
            continue;
        }
        for token in tokens {
            let Some(value) = token.strip_prefix("timestamps=") else {
                continue;
            };
            let state = match value {
                "enabled" => TcpTimestamps::Enabled,
                "disabled" => TcpTimestamps::Disabled,
                "allowed" => TcpTimestamps::Allowed,
                _ => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Unknown timestamps state",
                    ))
                }
            };
            if result.replace(state).is_some() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Duplicate timestamps state",
                ));
            }
        }
    }
    result.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Missing timestamps state"))
}

pub(crate) fn parse_doh_count(output: &str) -> io::Result<u32> {
    output
        .trim()
        .strip_prefix("DOH:")
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Invalid DNS result"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    use std::os::windows::process::ExitStatusExt;

    #[cfg(windows)]
    #[test]
    fn failed_commands_are_rejected_even_with_plausible_stdout() {
        for code in [1, 5, 1060, 234] {
            assert!(validate_output(Output {
                status: std::process::ExitStatus::from_raw(code),
                stdout: b"PROXY_DISABLED\nDOH:1\nRUNNING".to_vec(),
                stderr: b"Access denied".to_vec(),
            })
            .is_err());
        }
    }

    #[test]
    fn proxy_addresses_keep_ports_ipv6_and_protocol_assignments() {
        for proxy in [
            "127.0.0.1:1080",
            "[::1]:8080",
            "http=host:80;https=[2001:db8::1]:443",
        ] {
            assert_eq!(
                parse_proxy(&format!("PROXY_ENABLED:{proxy}\r\n")).unwrap(),
                Some(proxy)
            );
        }
        assert_eq!(parse_proxy("PROXY_DISABLED").unwrap(), None);
        for invalid in ["", "error", "PROXY_ENABLED:", "PROXY_DISABLED\nerror"] {
            assert!(parse_proxy(invalid).is_err());
        }
    }

    #[test]
    fn timestamps_require_an_explicit_known_global_setting() {
        for (setting, expected) in [
            ("enabled", TcpTimestamps::Enabled),
            ("disabled", TcpTimestamps::Disabled),
            ("allowed", TcpTimestamps::Allowed),
        ] {
            let dump = format!("# Локализованный комментарий\npushd interface tcp\nset global rss=enabled timestamps={setting}\npopd\n");
            assert_eq!(parse_tcp_timestamps(&dump).unwrap(), expected);
        }
        for invalid in [
            "",
            "command failed",
            "set global timestamps=default",
            "# timestamps=enabled",
            "set supplemental timestamps=enabled",
            "set global timestamps=enabled timestamps=disabled",
        ] {
            assert!(parse_tcp_timestamps(invalid).is_err());
        }
    }

    #[test]
    fn dns_requires_a_valid_explicit_count() {
        assert_eq!(parse_doh_count("DOH:0\r\n").unwrap(), 0);
        assert_eq!(parse_doh_count("DOH:12").unwrap(), 12);
        for invalid in ["", "error", "DOH:-1", "DOH:0\nDOH:1", "DOH:abc"] {
            assert!(parse_doh_count(invalid).is_err());
        }
    }
}
