use futures_util::{Stream, StreamExt};
use minisign_verify::{PublicKey, Signature};
use reqwest::header::{CACHE_CONTROL, USER_AGENT};
use serde::{Deserialize, Serialize};
use std::{cmp::Ordering, pin::Pin};

use super::{Checksum, CoreArtifact, CoreRelease};

const PRODUCTION_URL: &str =
    "https://raw.githubusercontent.com/larrriiin/zapret-ui/main/core-channel/stable.json";
const PRODUCTION_SIGNATURE_URL: &str =
    "https://raw.githubusercontent.com/larrriiin/zapret-ui/main/core-channel/stable.json.sig";
const CORE_CHANNEL_PUBLIC_KEY: &str = "RWSF180m7EnKBXIHk0nI+xssrK0ft4gpVAuq3H1BtC3ICzDhZV7pHFji";
const MAX_MANIFEST_BYTES: usize = 256 * 1024;
const MAX_SIGNATURE_BYTES: usize = 16 * 1024;
const EMBEDDED: &str = include_str!("../../../core-channel/stable.json");

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StableManifest {
    schema_version: u32,
    channel: String,
    provider: String,
    version: String,
    artifacts: Vec<ManifestArtifact>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ManifestArtifact {
    url: String,
    checksum: ManifestChecksum,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ManifestChecksum {
    algorithm: String,
    value: String,
}

impl StableManifest {
    fn parse_and_validate(bytes: &[u8], provider: &str) -> Result<CoreRelease, String> {
        if bytes.len() > MAX_MANIFEST_BYTES {
            return Err("channel manifest exceeds 256 KiB".into());
        }
        let value: Self = serde_json::from_slice(bytes)
            .map_err(|_| "channel manifest is not valid schema v1 JSON".to_string())?;
        if value.schema_version != 1 {
            return Err(format!(
                "unsupported channel schema {}",
                value.schema_version
            ));
        }
        if value.channel != "stable" {
            return Err("channel must be stable".into());
        }
        if value.provider != provider {
            return Err("channel provider does not match the installed provider".into());
        }
        if value.version.trim().is_empty() {
            return Err("channel version is empty".into());
        }
        if value.artifacts.is_empty() {
            return Err("stable channel contains no artifacts".into());
        }
        let artifacts = value
            .artifacts
            .into_iter()
            .map(|artifact| {
                let url = reqwest::Url::parse(&artifact.url)
                    .map_err(|_| "artifact URL is invalid".to_string())?;
                if url.scheme() != "https" {
                    return Err("stable artifacts must use HTTPS".into());
                }
                if artifact.checksum.algorithm != "sha256" {
                    return Err("stable artifacts require SHA-256".into());
                }
                let digest = artifact.checksum.value;
                if digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_hexdigit()) {
                    return Err("SHA-256 must contain exactly 64 hexadecimal characters".into());
                }
                if digest.bytes().any(|b| b.is_ascii_uppercase()) {
                    return Err("SHA-256 must be lowercase".into());
                }
                Ok(CoreArtifact {
                    url: artifact.url,
                    checksum: Checksum::Sha256(digest),
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(CoreRelease {
            provider: value.provider,
            channel: value.channel,
            version: value.version,
            artifacts,
        })
    }
}

fn channel_url() -> String {
    #[cfg(debug_assertions)]
    if let Ok(value) = std::env::var("ZAPRET_CORE_CHANNEL_URL") {
        return value;
    }
    PRODUCTION_URL.to_string()
}

async fn collect_limited<S, B, E>(mut stream: Pin<Box<S>>, limit: usize) -> Result<Vec<u8>, String>
where
    S: Stream<Item = Result<B, E>> + ?Sized,
    B: AsRef<[u8]>,
    E: std::fmt::Display,
{
    let mut body = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("channel response could not be read: {e}"))?;
        let chunk = chunk.as_ref();
        if body.len().saturating_add(chunk.len()) > limit {
            return Err("channel response is too large".into());
        }
        body.extend_from_slice(chunk);
    }
    Ok(body)
}

fn resolve_bytes(
    remote: Result<(Vec<u8>, Vec<u8>), String>,
    embedded: &[u8],
    provider: &str,
) -> Result<CoreRelease, String> {
    match remote.and_then(|(bytes, signature)| {
        verify_signature(&bytes, &signature)?;
        StableManifest::parse_and_validate(&bytes, provider)
    }) {
        Ok(release) => Ok(release),
        Err(reason) => {
            eprintln!("Stable core channel fallback: {reason}");
            StableManifest::parse_and_validate(embedded, provider)
                .map_err(|_| "Stable core channel is temporarily unavailable".to_string())
        }
    }
}

fn verify_with_key(bytes: &[u8], signature: &[u8], public_key: &str) -> Result<(), String> {
    let signature =
        std::str::from_utf8(signature).map_err(|_| "channel signature is not UTF-8".to_string())?;
    let public_key = PublicKey::from_base64(public_key)
        .map_err(|e| format!("embedded channel public key is invalid: {e}"))?;
    let signature =
        Signature::decode(signature).map_err(|e| format!("channel signature is invalid: {e}"))?;
    public_key
        .verify(bytes, &signature, false)
        .map_err(|e| format!("channel signature verification failed: {e}"))
}

fn verify_signature(bytes: &[u8], signature: &[u8]) -> Result<(), String> {
    verify_with_key(bytes, signature, CORE_CHANNEL_PUBLIC_KEY)
}

async fn download_limited(
    client: &reqwest::Client,
    url: &str,
    user_agent: &str,
    limit: usize,
) -> Result<Vec<u8>, String> {
    let response = client
        .get(url)
        .header(CACHE_CONTROL, "no-cache")
        .header(USER_AGENT, user_agent)
        .send()
        .await
        .map_err(|_| "channel request failed".to_string())?;
    if !response.status().is_success() {
        return Err("channel returned an HTTP error".into());
    }
    if response.content_length().is_some_and(|n| n > limit as u64) {
        return Err("channel response is too large".into());
    }
    collect_limited(Box::pin(response.bytes_stream()), limit).await
}

pub async fn resolve_stable(
    client: &reqwest::Client,
    provider: &str,
    user_agent: &str,
) -> Result<CoreRelease, String> {
    let manifest_url = channel_url();
    let signature_url = if manifest_url == PRODUCTION_URL {
        PRODUCTION_SIGNATURE_URL.to_string()
    } else {
        format!("{manifest_url}.sig")
    };
    let (manifest, signature) = futures_util::future::join(
        download_limited(client, &manifest_url, user_agent, MAX_MANIFEST_BYTES),
        download_limited(client, &signature_url, user_agent, MAX_SIGNATURE_BYTES),
    )
    .await;
    let remote = manifest.and_then(|manifest| signature.map(|signature| (manifest, signature)));
    resolve_bytes(remote, EMBEDDED.as_bytes(), provider)
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CoreUpdateStatus {
    NotInstalled,
    UpdateAvailable,
    UpToDate,
    Ahead,
    Unknown,
}

pub fn compare_versions(local: &str, stable: &str) -> CoreUpdateStatus {
    fn parse(s: &str) -> Option<Vec<u64>> {
        let s = s.trim().strip_prefix('v').unwrap_or(s.trim());
        let mut parts: Vec<u64> = s
            .split('.')
            .map(str::parse)
            .collect::<Result<_, _>>()
            .ok()?;
        if !(2..=4).contains(&parts.len()) {
            return None;
        }
        while parts.last() == Some(&0) {
            parts.pop();
        }
        Some(parts)
    }
    match (parse(local), parse(stable)) {
        (Some(a), Some(b)) => match a.cmp(&b) {
            Ordering::Less => CoreUpdateStatus::UpdateAvailable,
            Ordering::Equal => CoreUpdateStatus::UpToDate,
            Ordering::Greater => CoreUpdateStatus::Ahead,
        },
        _ => CoreUpdateStatus::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn valid() -> Vec<u8> {
        format!(r#"{{"schemaVersion":1,"channel":"stable","provider":"flowseal","version":"1.2.3","artifacts":[{{"url":"https://example.invalid/a.zip","checksum":{{"algorithm":"sha256","value":"{}"}}}}]}}"#, "a".repeat(64)).into_bytes()
    }
    #[test]
    fn valid_v1() {
        StableManifest::parse_and_validate(&valid(), "flowseal").unwrap();
    }
    #[test]
    fn comparisons_normalize_zero_components() {
        assert_eq!(
            compare_versions("1.2.2", "1.2.3"),
            CoreUpdateStatus::UpdateAvailable
        );
        assert_eq!(compare_versions("1.2", "1.2.0"), CoreUpdateStatus::UpToDate);
        assert_eq!(compare_versions("1.3.0", "1.2.3"), CoreUpdateStatus::Ahead);
        assert_eq!(compare_versions("Err", "1.2.3"), CoreUpdateStatus::Unknown);
    }
    #[test]
    fn remote_error_and_invalid_remote_use_embedded() {
        assert_eq!(
            resolve_bytes(Err("offline".into()), &valid(), "flowseal")
                .unwrap()
                .version,
            "1.2.3"
        );
        assert_eq!(
            resolve_bytes(
                Ok((b"not json".to_vec(), b"not a signature".to_vec())),
                &valid(),
                "flowseal"
            )
            .unwrap()
            .version,
            "1.2.3"
        );
    }
    #[tokio::test(flavor = "current_thread")]
    async fn streaming_limit_stops_oversized_body() {
        let chunks = futures_util::stream::iter([
            Ok::<_, String>(vec![0; MAX_MANIFEST_BYTES]),
            Ok(vec![b'x']),
        ]);
        assert!(collect_limited(Box::pin(chunks), MAX_MANIFEST_BYTES)
            .await
            .unwrap_err()
            .contains("too large"));
    }
    #[test]
    fn verifies_minisign_signature_and_rejects_modified_content() {
        const KEY: &str = "RWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3";
        const SIGNATURE: &str = "untrusted comment: signature from minisign secret key\nRUQf6LRCGA9i559r3g7V1qNyJDApGip8MfqcadIgT9CuhV3EMhHoN1mGTkUidF/z7SrlQgXdy8ofjb7bNJJylDOocrCo8KLzZwo=\ntrusted comment: timestamp:1633700835\tfile:test\tprehashed\nwLMDjy9FLAuxZ3q4NlEvkgtyhrr0gtTu6KC4KBJdITbbOeAi1zBIYo0v4iTgt8jJpIidRJnp94ABQkJAgAooBQ==";
        assert!(verify_with_key(b"test", SIGNATURE.as_bytes(), KEY).is_ok());
        assert!(verify_with_key(b"modified", SIGNATURE.as_bytes(), KEY).is_err());
    }
    #[test]
    fn core_channel_key_matches_the_tauri_updater_identity() {
        use base64::Engine;

        let config: serde_json::Value =
            serde_json::from_str(include_str!("../../tauri.conf.json")).unwrap();
        let encoded = config["plugins"]["updater"]["pubkey"].as_str().unwrap();
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .unwrap();
        let decoded = std::str::from_utf8(&decoded).unwrap();
        assert!(decoded.lines().any(|line| line == CORE_CHANNEL_PUBLIC_KEY));
    }
    #[test]
    fn rejects_invalid_fields() {
        for (from, to) in [
            ("\"schemaVersion\":1", "\"schemaVersion\":2"),
            ("\"channel\":\"stable\"", "\"channel\":\"beta\""),
            ("\"provider\":\"flowseal\"", "\"provider\":\"other\""),
            ("\"version\":\"1.2.3\"", "\"version\":\"\""),
            ("https://example.invalid", "http://example.invalid"),
            ("\"algorithm\":\"sha256\"", "\"algorithm\":\"md5\""),
        ] {
            let body = String::from_utf8(valid()).unwrap().replace(from, to);
            assert!(StableManifest::parse_and_validate(body.as_bytes(), "flowseal").is_err());
        }
        let empty = serde_json::json!({"schemaVersion":1,"channel":"stable","provider":"flowseal","version":"1.2.3","artifacts":[]});
        assert!(
            StableManifest::parse_and_validate(empty.to_string().as_bytes(), "flowseal").is_err()
        );
        for digest in [
            "abc",
            "gggggggggggggggggggggggggggggggggggggggggggggggggggggggggggggggg",
        ] {
            let body = String::from_utf8(valid())
                .unwrap()
                .replace(&"a".repeat(64), digest);
            assert!(StableManifest::parse_and_validate(body.as_bytes(), "flowseal").is_err());
        }
    }
}
