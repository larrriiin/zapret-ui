use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use super::CorePaths;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "algorithm", content = "value", rename_all = "lowercase")]
pub enum Checksum {
    Sha256(String),
    Md5(String),
}

/// Download information without assumptions about a provider's hosting service.
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub struct CoreArtifact {
    pub url: String,
    pub checksum: Checksum,
}

#[allow(dead_code)]
pub struct CoreRelease {
    pub provider: String,
    pub channel: String,
    pub version: String,
    pub artifacts: Vec<CoreArtifact>,
}

/// Boundary between Tauri/core orchestration and an upstream core distribution.
pub trait CoreProvider: Send + Sync {
    fn provider_name(&self) -> &'static str;
    fn paths(&self) -> &CorePaths;
    fn ipset_url(&self) -> &'static str;
    fn service_script(&self) -> PathBuf;
    fn test_script(&self) -> PathBuf;
    fn winws_executable(&self) -> PathBuf;
    fn local_version(&self) -> String;
    fn is_installed(&self) -> bool;
    fn strategies(&self) -> Result<Vec<String>, String>;
    fn parse_strategy(&self, strategy: &str, game_filter: &str) -> Result<String, String>;
    /// Validates provider-specific on-disk structure and returns its version and strategy count.
    fn validate_installation(&self) -> Result<(String, usize), String>;
}
