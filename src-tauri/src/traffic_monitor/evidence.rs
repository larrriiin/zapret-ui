//! Bounded, transient parsing of cleartext handshake metadata; no TLS decryption.
use super::rules::Evidence;
use std::time::{Duration, Instant};
const MAX_HELLO: usize = 16 * 1024;

#[derive(Clone, Debug, Default)]
pub(super) struct FlowEvidence {
    pub summary: Evidence,
    buffer: Vec<u8>,
    next_sequence: Option<u32>,
    started: Option<Instant>,
    finished: bool,
}
impl FlowEvidence {
    pub fn observe_tcp(&mut self, sequence: u32, payload: &[u8], allow_buffer: bool) {
        if self.finished || payload.is_empty() {
            return;
        }
        if !allow_buffer
            || self
                .started
                .is_some_and(|t| t.elapsed() > Duration::from_secs(3))
        {
            self.finish();
            return;
        }
        let payload = if let Some(next) = self.next_sequence {
            if sequence == next {
                payload
            } else {
                let overlap = next.wrapping_sub(sequence) as usize;
                if overlap > payload.len() {
                    self.finish();
                    return;
                }
                &payload[overlap..]
            }
        } else {
            if payload[0] != 0x16 && !b"GPHDOCT".contains(&payload[0]) {
                self.finish();
                return;
            }
            self.started = Some(Instant::now());
            payload
        };
        if self.buffer.len() + payload.len() > MAX_HELLO {
            self.finish();
            return;
        }
        self.next_sequence = Some(
            self.next_sequence
                .unwrap_or(sequence)
                .wrapping_add(payload.len() as u32),
        );
        self.buffer.extend_from_slice(payload);
        let result = if self.buffer.first() == Some(&0x16) {
            tls_hello(&self.buffer)
        } else {
            http_host(&self.buffer)
        };
        match result {
            Ok(Some(evidence)) => {
                self.summary = evidence;
                self.finish();
            }
            Err(()) => self.finish(),
            Ok(None) => (),
        }
    }
    pub fn observe_udp(&mut self, payload: &[u8]) {
        if payload.len() >= 20
            && payload[0] & 0xc0 == 0
            && payload[4..8] == [0x21, 0x12, 0xa4, 0x42]
            && u16::from_be_bytes([payload[2], payload[3]]) as usize + 20 == payload.len()
        {
            self.summary.protocol = Some("stun");
        } else if payload.len() >= 7
            && payload[0] & 0xc0 == 0xc0
            && payload[1..5] != [0, 0, 0, 0]
            && payload[5] <= 20
            && payload.len() > 6 + payload[5] as usize
        {
            self.summary.protocol = Some("quic");
        }
    }
    fn finish(&mut self) {
        self.buffer = Vec::new();
        self.finished = true;
    }
}

fn hostname(value: &[u8]) -> Option<String> {
    let name = std::str::from_utf8(value)
        .ok()?
        .trim_end_matches('.')
        .to_ascii_lowercase();
    (!name.is_empty()
        && name.len() <= 253
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.'))
    .then_some(name)
}
fn http_host(bytes: &[u8]) -> Result<Option<Evidence>, ()> {
    let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") else {
        return Ok(None);
    };
    let headers = std::str::from_utf8(&bytes[..end]).map_err(|_| ())?;
    let mut lines = headers.split("\r\n");
    let request = lines.next().ok_or(())?;
    if !request.contains(" HTTP/1.") {
        return Err(());
    }
    let mut hosts = lines
        .filter_map(|line| line.split_once(':'))
        .filter(|(key, _)| key.eq_ignore_ascii_case("host"));
    let name = hosts
        .next()
        .and_then(|(_, value)| hostname(value.trim().split(':').next()?.as_bytes()));
    // Duplicate Host fields are ambiguous; never pick one arbitrarily.
    Ok(Some(Evidence {
        hostname: if hosts.next().is_none() { name } else { None },
        protocol: Some("http"),
    }))
}
fn take<'a>(bytes: &mut &'a [u8], length: usize) -> Result<&'a [u8], ()> {
    let value = bytes.get(..length).ok_or(())?;
    *bytes = &bytes[length..];
    Ok(value)
}
fn u16_value(bytes: &[u8]) -> usize {
    u16::from_be_bytes([bytes[0], bytes[1]]) as usize
}
fn vector16<'a>(bytes: &mut &'a [u8]) -> Result<&'a [u8], ()> {
    let length = u16_value(take(bytes, 2)?);
    take(bytes, length)
}
fn tls_hello(bytes: &[u8]) -> Result<Option<Evidence>, ()> {
    let mut records = bytes;
    let mut handshake = Vec::new();
    loop {
        if records.len() < 5 {
            return Ok(None);
        }
        if records[0] != 22 || records[1] != 3 {
            return Err(());
        }
        let length = u16_value(&records[3..5]);
        let Some(record) = records.get(5..5 + length) else {
            return Ok(None);
        };
        handshake.extend_from_slice(record);
        if handshake.len() > MAX_HELLO {
            return Err(());
        }
        if handshake.len() >= 4 {
            if handshake[0] != 1 {
                return Err(());
            }
            let size = ((handshake[1] as usize) << 16)
                | ((handshake[2] as usize) << 8)
                | handshake[3] as usize;
            if size > MAX_HELLO - 4 {
                return Err(());
            }
            if handshake.len() >= size + 4 {
                return parse_client_hello(&handshake[4..4 + size]).map(Some);
            }
        }
        records = &records[5 + length..];
    }
}
fn parse_client_hello(mut hello: &[u8]) -> Result<Evidence, ()> {
    take(&mut hello, 34)?;
    let session = take(&mut hello, 1)?[0] as usize;
    take(&mut hello, session)?;
    let ciphers = vector16(&mut hello)?;
    if ciphers.is_empty() || ciphers.len() % 2 != 0 {
        return Err(());
    }
    let compression = take(&mut hello, 1)?[0] as usize;
    if compression == 0 {
        return Err(());
    }
    take(&mut hello, compression)?;
    let mut result = Evidence {
        protocol: Some("tls"),
        ..Default::default()
    };
    if hello.is_empty() {
        return Ok(result);
    }
    let mut extensions = vector16(&mut hello)?;
    let mut seen = std::collections::HashSet::new();
    let mut encrypted = false;
    while !extensions.is_empty() {
        let kind = u16_value(take(&mut extensions, 2)?);
        if !seen.insert(kind) {
            return Err(());
        }
        let mut data = vector16(&mut extensions)?;
        if kind == 0xfe0d {
            encrypted = true;
        }
        if kind == 0 {
            let mut names = vector16(&mut data)?;
            while !names.is_empty() {
                let kind = take(&mut names, 1)?[0];
                let value = vector16(&mut names)?;
                if kind == 0 {
                    if result.hostname.is_some() {
                        return Err(());
                    }
                    result.hostname = hostname(value);
                }
            }
        }
    }
    // An ECH outer name is not evidence of the inner destination.
    if encrypted {
        result.hostname = None;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    pub(super) fn hello(name: &str, ech: bool) -> Vec<u8> {
        let mut server_name = vec![0];
        server_name.extend((name.len() as u16).to_be_bytes());
        server_name.extend(name.as_bytes());
        let mut sni = (server_name.len() as u16).to_be_bytes().to_vec();
        sni.extend(server_name);
        let mut extensions = vec![0, 0];
        extensions.extend((sni.len() as u16).to_be_bytes());
        extensions.extend(sni);
        if ech {
            extensions.extend([0xfe, 0x0d, 0, 0]);
        }
        let mut body = vec![3, 3];
        body.extend([0; 32]);
        body.extend([0, 0, 2, 0x13, 1, 1, 0]);
        body.extend((extensions.len() as u16).to_be_bytes());
        body.extend(extensions);
        let mut handshake = vec![
            1,
            0,
            ((body.len() >> 8) & 255) as u8,
            (body.len() & 255) as u8,
        ];
        handshake.extend(body);
        let mut record = vec![22, 3, 1];
        record.extend((handshake.len() as u16).to_be_bytes());
        record.extend(handshake);
        record
    }
    #[test]
    fn fragmented_tls_and_retransmissions_recover_sni() {
        let packet = hello("Api.Example.com", false);
        let mut flow = FlowEvidence::default();
        flow.observe_tcp(100, &packet[..2], true);
        flow.observe_tcp(100, &packet[..2], true);
        assert!(flow.summary.hostname.is_none());
        flow.observe_tcp(102, &packet[2..], true);
        assert_eq!(flow.summary.hostname.as_deref(), Some("api.example.com"));
        assert!(flow.buffer.is_empty());
    }
    #[test]
    fn ech_gaps_and_limits_do_not_invent_a_hostname() {
        let packet = hello("outer.example.com", true);
        let mut flow = FlowEvidence::default();
        flow.observe_tcp(0, &packet, true);
        assert!(flow.summary.hostname.is_none());
        let mut gap = FlowEvidence::default();
        gap.observe_tcp(0, &packet[..2], true);
        gap.observe_tcp(5, &packet[5..], true);
        assert!(gap.summary.hostname.is_none());
        assert!(gap.buffer.is_empty());
        let mut limit = FlowEvidence::default();
        limit.observe_tcp(0, &[b'G'; MAX_HELLO + 1], true);
        assert!(limit.buffer.is_empty());
    }
    #[test]
    fn http_host_is_bounded_and_duplicate_hosts_are_ambiguous() {
        let mut flow = FlowEvidence::default();
        flow.observe_tcp(
            0,
            b"GET / HTTP/1.1\r\nHost: api.example.com:443\r\n\r\n",
            true,
        );
        assert_eq!(flow.summary.hostname.as_deref(), Some("api.example.com"));
        let mut flow = FlowEvidence::default();
        flow.observe_tcp(
            0,
            b"GET / HTTP/1.1\r\nHost: a.com\r\nHost: b.com\r\n\r\n",
            true,
        );
        assert!(flow.summary.hostname.is_none());
    }
}
