use reqwest::header::{CACHE_CONTROL, USER_AGENT};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

const PRODUCTION_URL: &str =
    "https://raw.githubusercontent.com/larrriiin/zapret-ui/main/core-channel/stable.json";
const MAX_MANIFEST_BYTES: usize = 256 * 1024;
const EMBEDDED: &str = include_str!("../../../core-channel/stable.json");

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StableManifest {
    pub schema_version: u32,
    pub channel: String,
    pub provider: String,
    pub version: String,
    pub artifacts: Vec<Artifact>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub url: String,
    pub checksum: ArtifactChecksum,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ArtifactChecksum {
    pub algorithm: String,
    pub value: String,
}

impl StableManifest {
    pub fn parse_and_validate(bytes: &[u8], provider: &str) -> Result<Self, String> {
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
        for artifact in &value.artifacts {
            let url = reqwest::Url::parse(&artifact.url)
                .map_err(|_| "artifact URL is invalid".to_string())?;
            if url.scheme() != "https" {
                return Err("stable artifacts must use HTTPS".into());
            }
            if artifact.checksum.algorithm != "sha256" {
                return Err("stable artifacts require SHA-256".into());
            }
            let digest = &artifact.checksum.value;
            if digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err("SHA-256 must contain exactly 64 hexadecimal characters".into());
            }
            if digest.bytes().any(|b| b.is_ascii_uppercase()) {
                return Err("SHA-256 must be lowercase".into());
            }
        }
        Ok(value)
    }
}

fn channel_url() -> String {
    #[cfg(debug_assertions)]
    if let Ok(value) = std::env::var("ZAPRET_CORE_CHANNEL_URL") {
        return value;
    }
    PRODUCTION_URL.to_string()
}

pub async fn resolve_stable(
    client: &reqwest::Client,
    provider: &str,
    user_agent: &str,
) -> Result<StableManifest, String> {
    let remote = async {
        let response = client
            .get(channel_url())
            .header(CACHE_CONTROL, "no-cache")
            .header(USER_AGENT, user_agent)
            .send()
            .await
            .map_err(|_| "channel request failed")?;
        if !response.status().is_success() {
            return Err("channel returned an HTTP error");
        }
        if response
            .content_length()
            .is_some_and(|n| n > MAX_MANIFEST_BYTES as u64)
        {
            return Err("channel response is too large");
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|_| "channel response could not be read")?;
        StableManifest::parse_and_validate(&bytes, provider)
            .map_err(|_| "remote channel validation failed")
    }
    .await;
    match remote {
        Ok(value) => Ok(value),
        Err(reason) => {
            eprintln!("Stable core channel fallback: {reason}");
            StableManifest::parse_and_validate(EMBEDDED.as_bytes(), provider)
                .map_err(|_| "Stable core channel is temporarily unavailable".to_string())
        }
    }
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
        let parts: Option<Vec<_>> = s.split('.').map(|p| p.parse().ok()).collect();
        parts.filter(|p| p.len() >= 2 && p.len() <= 4)
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
    fn json(overrides: &str) -> Vec<u8> {
        format!(r#"{{"schemaVersion":1,"channel":"stable","provider":"flowseal","version":"1.2.3","artifacts":[{{"url":"https://example.invalid/a.zip","checksum":{{"algorithm":"sha256","value":"{}"}}}}]{} }}"#, "a".repeat(64), overrides).into_bytes()
    }
    #[test]
    fn valid_v1() {
        StableManifest::parse_and_validate(&json(""), "flowseal").unwrap();
    }
    #[test]
    fn comparisons() {
        assert_eq!(
            compare_versions("1.2.2", "1.2.3"),
            CoreUpdateStatus::UpdateAvailable
        );
        assert_eq!(
            compare_versions("1.2.3", "1.2.3"),
            CoreUpdateStatus::UpToDate
        );
        assert_eq!(compare_versions("1.3.0", "1.2.3"), CoreUpdateStatus::Ahead);
        assert_eq!(compare_versions("Err", "1.2.3"), CoreUpdateStatus::Unknown);
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
            let s = String::from_utf8(json("")).unwrap().replace(from, to);
            assert!(StableManifest::parse_and_validate(s.as_bytes(), "flowseal").is_err());
        }
        let empty=String::from_utf8(json("")).unwrap().replace(r#"[{"url"# , r#"_EMPTY_[{"url"#).replace(r#""artifacts":_EMPTY_[{"url":"https://example.invalid/a.zip","checksum":{"algorithm":"sha256","value":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}}]"#, r#""artifacts":[]"#);
        assert!(StableManifest::parse_and_validate(empty.as_bytes(), "flowseal").is_err());
        for digest in [
            "abc",
            "gggggggggggggggggggggggggggggggggggggggggggggggggggggggggggggggg",
        ] {
            let s = String::from_utf8(json(""))
                .unwrap()
                .replace(&"a".repeat(64), digest);
            assert!(StableManifest::parse_and_validate(s.as_bytes(), "flowseal").is_err());
        }
    }
}
