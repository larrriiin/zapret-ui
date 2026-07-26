use std::{cmp::Ordering, path::PathBuf};

use crate::core::{Checksum, CorePaths, CoreProvider, CoreRelease};

const VERSION_URL: &str =
    "https://raw.githubusercontent.com/Flowseal/zapret-discord-youtube/main/.service/version.txt";
const RELEASE_API: &str =
    "https://api.github.com/repos/Flowseal/zapret-discord-youtube/releases/tags";
const FALLBACK_METADATA_URL: &str =
    "https://sourceforge.net/projects/zapret-discord-youtube.mirror/best_release.json";
const FALLBACK_LATEST_URL: &str =
    "https://sourceforge.net/projects/zapret-discord-youtube.mirror/files/latest/download";
const IPSET_URL: &str = "https://raw.githubusercontent.com/Flowseal/zapret-discord-youtube/refs/heads/main/.service/ipset-service.txt";

pub struct FlowsealProvider {
    paths: CorePaths,
}

impl FlowsealProvider {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            paths: CorePaths::new(root),
        }
    }

    pub fn parse_local_version(content: &str) -> Result<String, String> {
        content
            .lines()
            .find_map(|line| {
                if line.to_ascii_lowercase().contains("local_version=") {
                    line.split_once('=')
                        .map(|(_, value)| value.trim().trim_matches('"').to_owned())
                } else {
                    None
                }
            })
            .filter(|version| !version.is_empty())
            .ok_or_else(|| "Err: No Version String Found".to_string())
    }

    pub async fn fetch_fallback_release(
        &self,
        client: &reqwest::Client,
    ) -> Result<CoreRelease, String> {
        let response = client
            .get(FALLBACK_METADATA_URL)
            .header("Cache-Control", "no-cache")
            .send()
            .await
            .map_err(|e| format!("Failed to send SourceForge request: {e}"))?;
        if !response.status().is_success() {
            return Err(format!(
                "SourceForge returned status: {}",
                response.status()
            ));
        }
        let body = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse SourceForge JSON: {e}"))?;
        parse_fallback_release(&body)
    }

    pub fn fallback_latest_url(&self) -> &'static str {
        FALLBACK_LATEST_URL
    }

    fn parse_strategy_content(
        &self,
        content: &str,
        strategy: &str,
        game_filter: &str,
    ) -> Result<String, String> {
        let (gf, gftcp, gfudp) = match game_filter {
            "all" => ("1024-65535", "1024-65535", "1024-65535"),
            "tcp" => ("1024-65535", "1024-65535", "12"),
            "udp" => ("1024-65535", "12", "1024-65535"),
            _ => ("12", "12", "12"),
        };
        let lines: Vec<_> = content.lines().collect();
        let start = lines
            .iter()
            .position(|line| line.to_ascii_lowercase().contains("winws.exe"))
            .ok_or_else(|| format!("Не найдена строка с winws.exe в {}.bat", strategy))?;
        let mut command = String::new();
        for raw in &lines[start..] {
            let line = raw.trim();
            if let Some(rest) = line.strip_suffix('^') {
                command.push_str(rest);
                command.push(' ');
            } else {
                command.push_str(line);
                break;
            }
        }
        let lower = command.to_ascii_lowercase();
        let offset = lower
            .find("winws.exe\"")
            .map(|p| p + 10)
            .or_else(|| lower.find("winws.exe ").map(|p| p + 10))
            .ok_or_else(|| format!("Не найдена строка с winws.exe в {}.bat", strategy))?;
        let suffix = std::path::MAIN_SEPARATOR.to_string();
        let bin = self.paths.bin_dir().to_string_lossy().to_string() + &suffix;
        let lists = self.paths.lists_dir().to_string_lossy().to_string() + &suffix;
        let root = self.paths.root().to_string_lossy().to_string() + &suffix;
        let args = command[offset..]
            .replace("%GameFilter%", gf)
            .replace("%GameFilterTCP%", gftcp)
            .replace("%GameFilterUDP%", gfudp)
            .replace("%BIN%", &bin)
            .replace("%LISTS%", &lists);
        Ok(args
            .split_whitespace()
            .map(|word| {
                if let Some(rest) = word.strip_prefix("\"@") {
                    format!("\"{}{}", root, rest)
                } else {
                    word.to_owned()
                }
            })
            .collect::<Vec<_>>()
            .join(" "))
    }
}

impl CoreProvider for FlowsealProvider {
    fn provider_name(&self) -> &'static str {
        "flowseal"
    }
    fn paths(&self) -> &CorePaths {
        &self.paths
    }
    fn version_url(&self) -> &'static str {
        VERSION_URL
    }
    fn release_api_url(&self, version: &str) -> String {
        format!("{RELEASE_API}/{version}")
    }
    fn archive_name(&self, version: &str) -> String {
        format!("zapret-discord-youtube-{version}.zip")
    }
    fn release_download_url(&self, version: &str) -> String {
        format!(
            "https://github.com/Flowseal/zapret-discord-youtube/releases/download/{version}/{}",
            self.archive_name(version)
        )
    }
    fn ipset_url(&self) -> &'static str {
        IPSET_URL
    }
    fn service_script(&self) -> PathBuf {
        self.paths.root().join("service.bat")
    }
    fn test_script(&self) -> PathBuf {
        self.paths.utils_dir().join("test zapret.ps1")
    }
    fn winws_executable(&self) -> PathBuf {
        self.paths.bin_dir().join("winws.exe")
    }
    fn local_version(&self) -> String {
        let path = self.service_script();
        if !path.exists() {
            return format!("Err: Not Found at {:?}", path);
        }
        std::fs::read_to_string(&path)
            .map_err(|e| format!("Err: Read Failed ({e})"))
            .and_then(|content| Self::parse_local_version(&content))
            .unwrap_or_else(|e| e)
    }
    fn is_installed(&self) -> bool {
        self.paths.root().exists() && self.service_script().exists()
    }
    fn strategies(&self) -> Result<Vec<String>, String> {
        let mut result = std::fs::read_dir(self.paths.root())
            .map_err(|e| format!("Failed to read binaries ({:?}): {}", self.paths.root(), e))?
            .filter_map(Result::ok)
            .filter(|e| {
                e.path()
                    .extension()
                    .and_then(|x| x.to_str())
                    .is_some_and(|x| x.eq_ignore_ascii_case("bat"))
            })
            .filter_map(|e| {
                e.path()
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .map(str::to_owned)
            })
            .filter(|name| !name.eq_ignore_ascii_case("service"))
            .collect::<Vec<_>>();
        result.sort_by(|a, b| natural_compare(a, b));
        Ok(result)
    }
    fn parse_strategy(&self, strategy: &str, game_filter: &str) -> Result<String, String> {
        let content = std::fs::read_to_string(self.paths.root().join(format!("{strategy}.bat")))
            .map_err(|e| format!("Не удалось прочитать {strategy}.bat: {e}"))?;
        self.parse_strategy_content(&content, strategy, game_filter)
    }
    fn validate_installation(&self) -> Result<(String, usize), String> {
        let root = self.paths.root();
        if !root.is_dir() {
            return Err(format!(
                "Unexpected archive structure: {} is not a directory",
                root.display()
            ));
        }
        for path in [
            self.service_script(),
            self.winws_executable(),
            self.test_script(),
        ] {
            if !path.is_file() {
                return Err(format!("Missing required file: {}", path.display()));
            }
        }
        for path in [self.paths.lists_dir(), self.paths.utils_dir()] {
            if !path.is_dir() {
                return Err(format!("Missing required directory: {}", path.display()));
            }
        }
        let windivert = self.paths.bin_dir().join("WinDivert.dll");
        let windivert64 = self.paths.bin_dir().join("WinDivert64.sys");
        for path in [windivert, windivert64] {
            if !path.is_file() {
                return Err(format!("Missing required file: {}", path.display()));
            }
        }
        let strategies = self.strategies()?;
        if strategies.is_empty() {
            return Err("No Flowseal strategies found".to_string());
        }
        for strategy in &strategies {
            self.parse_strategy(strategy, "all")
                .map_err(|e| format!("Strategy {strategy} cannot be parsed: {e}"))?;
        }
        let version = self.local_version();
        if version.starts_with("Err:") {
            return Err(format!("Cannot determine Flowseal version: {version}"));
        }
        Ok((version, strategies.len()))
    }
}

pub fn parse_fallback_release(body: &serde_json::Value) -> Result<CoreRelease, String> {
    let release = body
        .pointer("/platform_releases/windows")
        .or_else(|| body.get("release"))
        .ok_or_else(|| "No release information found in SourceForge JSON".to_string())?;
    let filename = release
        .get("filename")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing filename in SourceForge JSON".to_string())?;
    Ok(CoreRelease {
        version: filename
            .split('/')
            .find(|s| !s.is_empty())
            .ok_or_else(|| "Failed to parse version from SourceForge filename".to_string())?
            .to_owned(),
        download_url: release
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "Missing url in SourceForge JSON".to_string())?
            .to_owned(),
        checksum: Some(Checksum::Md5(
            release
                .get("md5sum")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing md5sum in SourceForge JSON".to_string())?
                .to_owned(),
        )),
    })
}

fn natural_compare(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek(), b.peek()) {
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let an: String = std::iter::from_fn(|| a.next_if(|c| c.is_ascii_digit())).collect();
                let bn: String = std::iter::from_fn(|| b.next_if(|c| c.is_ascii_digit())).collect();
                let ord = an
                    .trim_start_matches('0')
                    .len()
                    .cmp(&bn.trim_start_matches('0').len())
                    .then_with(|| an.cmp(&bn));
                if ord != Ordering::Equal {
                    return ord;
                }
            }
            (Some(_), Some(_)) => {
                let ord = a.next().cmp(&b.next());
                if ord != Ordering::Equal {
                    return ord;
                }
            }
            _ => return a.next().cmp(&b.next()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn temp() -> PathBuf {
        let p = std::env::temp_dir().join(format!("zapret-provider-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).expect("temp");
        p
    }
    #[test]
    fn release_urls_and_names() {
        let p = FlowsealProvider::new("x");
        assert_eq!(p.archive_name("1.2"), "zapret-discord-youtube-1.2.zip");
        assert!(p
            .release_download_url("1.2")
            .ends_with("/1.2/zapret-discord-youtube-1.2.zip"));
    }
    #[test]
    fn parses_sourceforge_release_with_md5() {
        let body = serde_json::json!({
            "platform_releases": { "windows": {
                "filename": "/v1/archive.zip",
                "url": "https://example.invalid/archive.zip",
                "md5sum": "abcdef"
            }}
        });
        let release = parse_fallback_release(&body).expect("fallback release");
        assert_eq!(release.version, "v1");
        assert_eq!(release.checksum, Some(Checksum::Md5("abcdef".to_string())));
    }

    #[test]
    fn parses_local_version() {
        assert_eq!(
            FlowsealProvider::parse_local_version("set LOCAL_VERSION=1.2\n").as_deref(),
            Ok("1.2")
        );
        assert!(FlowsealProvider::parse_local_version("echo no").is_err());
        assert!(FlowsealProvider::parse_local_version("local_version=\"\"").is_err());
    }
    #[test]
    fn finds_and_naturally_sorts_strategies() {
        let root = temp();
        for n in ["ALT11.bat", "service.bat", "ALT2.bat"] {
            std::fs::write(root.join(n), "").expect("write");
        }
        let p = FlowsealProvider::new(&root);
        assert_eq!(p.strategies().expect("strategies"), ["ALT2", "ALT11"]);
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn parses_multiline_strategy_and_variables() {
        let p = FlowsealProvider::new("core");
        let args=p.parse_strategy_content("\"%BIN%winws.exe\" --filter-tcp=%GameFilterTCP% ^\n --hostlist=\"%LISTS%list.txt\" --fake=\"@bin\\fake.bin\"", "ALT", "tcp").expect("parse");
        assert!(args.contains("1024-65535"));
        assert!(args.contains("core"));
        assert!(p.parse_strategy_content("echo no", "ALT", "all").is_err());
    }
}
