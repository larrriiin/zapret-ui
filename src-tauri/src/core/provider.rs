use std::path::PathBuf;

use super::CorePaths;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Checksum {
    Sha256(String),
    Md5(String),
}

/// Download information without assumptions about a provider's hosting service.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoreRelease {
    pub version: String,
    pub download_url: String,
    pub checksum: Option<Checksum>,
}

/// Boundary between Tauri/core orchestration and an upstream core distribution.
pub trait CoreProvider: Send + Sync {
    fn paths(&self) -> &CorePaths;
    fn version_url(&self) -> &'static str;
    fn release_api_url(&self, version: &str) -> String;
    fn archive_name(&self, version: &str) -> String;
    fn release_download_url(&self, version: &str) -> String;
    fn ipset_url(&self) -> &'static str;
    fn service_script(&self) -> PathBuf;
    fn test_script(&self) -> PathBuf;
    fn winws_executable(&self) -> PathBuf;
    fn local_version(&self) -> String;
    fn is_installed(&self) -> bool;
    fn strategies(&self) -> Result<Vec<String>, String>;
    fn parse_strategy(&self, strategy: &str, game_filter: &str) -> Result<String, String>;
}
