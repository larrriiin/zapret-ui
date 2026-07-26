use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use super::{Checksum, CoreProvider};
use crate::providers::FlowsealProvider;

pub const MANIFEST_NAME: &str = ".zapret-ui-core.json";
const SCHEMA_VERSION: u32 = 1;
const STAGING_PREFIX: &str = ".binaries-staging-";
const SWAP_NAME: &str = ".binaries-rollback-swap";
const USER_FILES: [&str; 3] = [
    "list-general-user.txt",
    "list-exclude-user.txt",
    "ipset-exclude-user.txt",
];
const ACTIVE_FAKE_FILES: [&str; 2] = ["ACTIVE_DISCORD_UDP.bin", "ACTIVE_GAME_UDP.bin"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CoreManifest {
    pub schema_version: u32,
    pub provider: String,
    pub version: String,
    pub source_url: Option<String>,
    pub checksum: Option<Checksum>,
    pub strategy_count: usize,
    #[serde(default)]
    pub imported_legacy: bool,
}

impl CoreManifest {
    pub fn new(
        version: String,
        source_url: Option<String>,
        checksum: Option<Checksum>,
        strategy_count: usize,
        imported_legacy: bool,
    ) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            provider: "flowseal".into(),
            version,
            source_url,
            checksum,
            strategy_count,
            imported_legacy,
        }
    }

    pub fn read(root: &Path) -> Result<Self, String> {
        let path = root.join(MANIFEST_NAME);
        let data = fs::read(&path).map_err(|e| {
            format!(
                "Core manifest is missing or unreadable ({}): {e}",
                path.display()
            )
        })?;
        let value: Self = serde_json::from_slice(&data)
            .map_err(|e| format!("Core manifest is corrupted: {e}"))?;
        if value.schema_version != SCHEMA_VERSION {
            return Err(format!(
                "Unsupported core manifest schema version: {}",
                value.schema_version
            ));
        }
        Ok(value)
    }

    pub fn write_atomic(&self, root: &Path) -> Result<(), String> {
        let path = root.join(MANIFEST_NAME);
        let temporary = root.join(format!("{MANIFEST_NAME}.tmp-{}", std::process::id()));
        let data = serde_json::to_vec_pretty(self)
            .map_err(|e| format!("Cannot serialize core manifest: {e}"))?;
        fs::write(&temporary, data)
            .map_err(|e| format!("Cannot write temporary core manifest: {e}"))?;
        fs::rename(&temporary, &path).map_err(|e| format!("Cannot activate core manifest: {e}"))
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CoreInstallationState {
    pub current_version: Option<String>,
    pub previous_version: Option<String>,
    pub rollback_available: bool,
}

pub struct CoreInstallation {
    parent: PathBuf,
    active: PathBuf,
    previous: PathBuf,
}

impl CoreInstallation {
    pub fn new(active: impl Into<PathBuf>) -> Result<Self, String> {
        let requested = active.into();
        let parent = requested
            .parent()
            .ok_or_else(|| "Core directory has no parent".to_string())?
            .canonicalize()
            .map_err(|e| format!("Cannot verify core parent directory: {e}"))?;
        if requested.file_name().and_then(|n| n.to_str()) != Some("binaries") {
            return Err("Active core directory must be named binaries".into());
        }
        let active = parent.join("binaries");
        Ok(Self {
            previous: parent.join("binaries.previous"),
            parent,
            active,
        })
    }

    pub fn recover(&self) -> Result<(), String> {
        fs::create_dir_all(&self.parent).map_err(|e| format!("Cannot prepare core parent: {e}"))?;
        if !self.active.exists() && self.previous.exists() {
            self.assert_owned(&self.previous, "binaries.previous")?;
            fs::rename(&self.previous, &self.active)
                .map_err(|e| format!("Cannot recover previous core: {e}"))?;
            eprintln!("Recovered binaries.previous after an interrupted core update");
        }
        if self.active.exists() {
            for entry in fs::read_dir(&self.parent).map_err(|e| e.to_string())? {
                let path = entry.map_err(|e| e.to_string())?.path();
                if path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with(STAGING_PREFIX))
                {
                    self.remove_owned_staging(&path)?;
                }
            }
        }
        Ok(())
    }

    pub fn prepare(&self) -> Result<(), String> {
        self.recover()?;
        if self.active.is_dir() && !self.active.join(MANIFEST_NAME).exists() {
            let provider = FlowsealProvider::new(&self.active);
            let (version, count) = provider.validate_installation()?;
            CoreManifest::new(version, None, None, count, true).write_atomic(&self.active)?;
        }
        Ok(())
    }

    pub fn state(&self) -> CoreInstallationState {
        let current_version = CoreManifest::read(&self.active).ok().map(|m| m.version);
        let previous_version = CoreManifest::read(&self.previous).ok().map(|m| m.version);
        CoreInstallationState {
            rollback_available: previous_version.is_some(),
            current_version,
            previous_version,
        }
    }

    pub fn create_staging(&self) -> Result<PathBuf, String> {
        fs::create_dir_all(&self.parent).map_err(|e| e.to_string())?;
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos();
        let path = self
            .parent
            .join(format!("{STAGING_PREFIX}{}-{stamp}", std::process::id()));
        fs::create_dir(&path).map_err(|e| format!("Cannot create staging directory: {e}"))?;
        Ok(path)
    }

    pub fn validate_and_manifest(
        &self,
        root: &Path,
        expected_version: &str,
        source_url: Option<String>,
        checksum: Option<Checksum>,
    ) -> Result<CoreManifest, String> {
        self.assert_staging(root)?;
        reject_symlinks(root)?;
        let provider = FlowsealProvider::new(root);
        let (actual, count) = provider.validate_installation()?;
        if actual != expected_version {
            return Err(format!(
                "Core version mismatch: expected {expected_version}, found {actual}"
            ));
        }
        let manifest = CoreManifest::new(actual, source_url, checksum, count, false);
        serde_json::to_vec(&manifest)
            .map_err(|e| format!("Cannot serialize core manifest: {e}"))?;
        manifest.write_atomic(root)?;
        Ok(manifest)
    }

    pub fn preserve_user_files(&self, from: &Path, to: &Path) -> Result<(), String> {
        let destination = to.join("lists");
        fs::create_dir_all(&destination)
            .map_err(|e| format!("Cannot create lists directory: {e}"))?;
        for name in USER_FILES {
            let source = from.join("lists").join(name);
            if source.is_file() {
                fs::copy(&source, destination.join(name))
                    .map_err(|e| format!("Cannot preserve {name}: {e}"))?;
            }
        }
        let destination_bin = to.join("bin");
        for name in ACTIVE_FAKE_FILES {
            let source = from.join("bin").join(name);
            if source.is_file() {
                fs::copy(&source, destination_bin.join(name))
                    .map_err(|e| format!("Cannot preserve selected fake {name}: {e}"))?;
            }
        }
        Ok(())
    }

    pub fn activate(&self, staging: &Path) -> Result<(), String> {
        self.assert_staging(staging)?;
        CoreManifest::read(staging)?;
        if !self.active.is_dir() {
            return Err("Active core is missing".into());
        }
        self.preserve_user_files(&self.active, staging)?;
        if self.previous.exists() {
            self.assert_owned(&self.previous, "binaries.previous")?;
            fs::remove_dir_all(&self.previous)
                .map_err(|e| format!("Cannot remove old previous core: {e}"))?;
        }
        fs::rename(&self.active, &self.previous)
            .map_err(|e| format!("Cannot save active core: {e}"))?;
        if let Err(error) = fs::rename(staging, &self.active) {
            fs::rename(&self.previous, &self.active).map_err(|rollback| {
                format!("Activation failed ({error}); automatic rollback also failed: {rollback}")
            })?;
            return Err(format!(
                "Activation failed ({error}); automatic rollback completed"
            ));
        }
        if let Err(error) = FlowsealProvider::new(&self.active).validate_installation() {
            let failed = self
                .parent
                .join(format!("{STAGING_PREFIX}failed-{}", std::process::id()));
            fs::rename(&self.active, &failed).map_err(|e| {
                format!(
                    "Post-activation validation failed ({error}); cannot quarantine new core: {e}"
                )
            })?;
            fs::rename(&self.previous, &self.active).map_err(|e| {
                format!(
                    "Post-activation validation failed ({error}); automatic rollback failed: {e}"
                )
            })?;
            self.remove_owned_staging(&failed)?;
            return Err(format!(
                "Post-activation validation failed ({error}); automatic rollback completed"
            ));
        }
        Ok(())
    }

    pub fn rollback(&self) -> Result<CoreInstallationState, String> {
        if !self.previous.is_dir() {
            return Err("Rollback is unavailable: previous core is missing".into());
        }
        CoreManifest::read(&self.previous)?;
        FlowsealProvider::new(&self.previous).validate_installation()?;
        self.preserve_user_files(&self.active, &self.previous)?;
        let swap = self.parent.join(SWAP_NAME);
        if swap.exists() {
            return Err("Rollback swap directory already exists; recovery is required".into());
        }
        fs::rename(&self.active, &swap).map_err(|e| format!("Cannot begin rollback: {e}"))?;
        if let Err(error) = fs::rename(&self.previous, &self.active) {
            fs::rename(&swap, &self.active)
                .map_err(|e| format!("Rollback failed ({error}); restoration failed: {e}"))?;
            return Err(format!("Rollback failed ({error}); active core restored"));
        }
        if let Err(error) = fs::rename(&swap, &self.previous) {
            fs::rename(&self.active, &self.previous).map_err(|e| {
                format!("Rollback swap failed ({error}); cannot move selected core back: {e}")
            })?;
            fs::rename(&swap, &self.active).map_err(|e| {
                format!("Rollback swap failed ({error}); cannot restore active core: {e}")
            })?;
            return Err(format!(
                "Rollback swap failed: {error}; active core restored"
            ));
        }
        FlowsealProvider::new(&self.active).validate_installation()?;
        Ok(self.state())
    }

    pub fn remove_owned_staging(&self, path: &Path) -> Result<(), String> {
        self.assert_staging(path)?;
        if path.exists() {
            reject_symlinks(path)?;
            fs::remove_dir_all(path).map_err(|e| format!("Cannot remove staging: {e}"))?;
        }
        Ok(())
    }

    fn assert_staging(&self, path: &Path) -> Result<(), String> {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if path.parent() != Some(self.parent.as_path()) || !name.starts_with(STAGING_PREFIX) {
            return Err(format!("Refusing unsafe staging path: {}", path.display()));
        }
        Ok(())
    }
    fn assert_owned(&self, path: &Path, expected: &str) -> Result<(), String> {
        if path.parent() != Some(self.parent.as_path())
            || path.file_name().and_then(|n| n.to_str()) != Some(expected)
        {
            return Err(format!("Refusing unsafe core path: {}", path.display()));
        }
        reject_symlinks(path)
    }
}

fn reject_symlinks(root: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(root)
        .map_err(|e| format!("Cannot inspect {}: {e}", root.display()))?;
    if metadata.file_type().is_symlink() {
        return Err(format!(
            "Symlink/junction is not allowed: {}",
            root.display()
        ));
    }
    if metadata.is_dir() {
        for entry in fs::read_dir(root).map_err(|e| e.to_string())? {
            reject_symlinks(&entry.map_err(|e| e.to_string())?.path())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "zapret-install-test-{}-{stamp}",
                std::process::id()
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn fixture(root: &Path, version: &str) {
        fs::create_dir_all(root.join("bin")).unwrap();
        fs::create_dir_all(root.join("lists")).unwrap();
        fs::create_dir_all(root.join("utils")).unwrap();
        fs::write(
            root.join("service.bat"),
            format!("set local_version={version}\n"),
        )
        .unwrap();
        fs::write(root.join("general.bat"), "\"%BIN%winws.exe\" --wf-tcp=80\n").unwrap();
        for file in ["winws.exe", "WinDivert.dll", "WinDivert64.sys"] {
            fs::write(root.join("bin").join(file), b"fixture").unwrap();
        }
        fs::write(root.join("utils/test zapret.ps1"), b"fixture").unwrap();
    }

    fn manifest(version: &str) -> CoreManifest {
        CoreManifest::new(version.into(), None, None, 1, false)
    }

    #[test]
    fn manifest_round_trip_and_unknown_schema_rejected() {
        let temp = Temp::new();
        manifest("1.2.3").write_atomic(&temp.0).unwrap();
        assert_eq!(CoreManifest::read(&temp.0).unwrap(), manifest("1.2.3"));
        let mut invalid = manifest("1.2.3");
        invalid.schema_version = 99;
        fs::write(
            temp.0.join(MANIFEST_NAME),
            serde_json::to_vec(&invalid).unwrap(),
        )
        .unwrap();
        assert!(CoreManifest::read(&temp.0)
            .unwrap_err()
            .contains("Unsupported"));
    }

    #[test]
    fn imports_legacy_installation_without_moving_it() {
        let temp = Temp::new();
        let active = temp.0.join("binaries");
        fixture(&active, "1.0.0");
        let installation = CoreInstallation::new(&active).unwrap();
        installation.prepare().unwrap();
        let imported = CoreManifest::read(&active).unwrap();
        assert!(imported.imported_legacy);
        assert_eq!(imported.version, "1.0.0");
        assert!(!installation.state().rollback_available);
    }

    #[test]
    fn validates_fixture_and_reports_missing_required_files() {
        let temp = Temp::new();
        let active = temp.0.join("binaries");
        fs::create_dir(&active).unwrap();
        let installation = CoreInstallation::new(&active).unwrap();
        for missing in ["service.bat", "bin/winws.exe", "utils/test zapret.ps1"] {
            let staging = installation.create_staging().unwrap();
            fixture(&staging, "2.0.0");
            fs::remove_file(staging.join(missing)).unwrap();
            assert!(installation
                .validate_and_manifest(&staging, "2.0.0", None, None)
                .unwrap_err()
                .contains("Missing required"));
            installation.remove_owned_staging(&staging).unwrap();
        }
        let staging = installation.create_staging().unwrap();
        fixture(&staging, "2.0.0");
        installation
            .validate_and_manifest(&staging, "2.0.0", None, None)
            .unwrap();
    }

    #[test]
    fn rejects_missing_bad_strategy_and_version_mismatch() {
        let temp = Temp::new();
        let active = temp.0.join("binaries");
        fs::create_dir(&active).unwrap();
        let installation = CoreInstallation::new(&active).unwrap();
        let staging = installation.create_staging().unwrap();
        fixture(&staging, "2.0.0");
        fs::remove_file(staging.join("general.bat")).unwrap();
        assert!(installation
            .validate_and_manifest(&staging, "2.0.0", None, None)
            .unwrap_err()
            .contains("No Flowseal strategies"));
        installation.remove_owned_staging(&staging).unwrap();
        let staging = installation.create_staging().unwrap();
        fixture(&staging, "2.0.0");
        fs::write(staging.join("general.bat"), "invalid").unwrap();
        assert!(installation
            .validate_and_manifest(&staging, "2.0.0", None, None)
            .unwrap_err()
            .contains("cannot be parsed"));
        installation.remove_owned_staging(&staging).unwrap();
        let staging = installation.create_staging().unwrap();
        fixture(&staging, "2.0.0");
        assert!(installation
            .validate_and_manifest(&staging, "3.0.0", None, None)
            .unwrap_err()
            .contains("version mismatch"));
    }

    #[test]
    fn activation_and_rollback_swap_versions_and_keep_current_lists() {
        let temp = Temp::new();
        let active = temp.0.join("binaries");
        fixture(&active, "1.0.0");
        manifest("1.0.0").write_atomic(&active).unwrap();
        fs::write(active.join("lists/list-general-user.txt"), "current").unwrap();
        let installation = CoreInstallation::new(&active).unwrap();
        let staging = installation.create_staging().unwrap();
        fixture(&staging, "2.0.0");
        installation
            .validate_and_manifest(&staging, "2.0.0", None, None)
            .unwrap();
        installation.activate(&staging).unwrap();
        assert_eq!(
            installation.state().current_version.as_deref(),
            Some("2.0.0")
        );
        fs::write(
            active.join("lists/list-general-user.txt"),
            "changed-after-update",
        )
        .unwrap();
        let state = installation.rollback().unwrap();
        assert_eq!(state.current_version.as_deref(), Some("1.0.0"));
        assert_eq!(state.previous_version.as_deref(), Some("2.0.0"));
        assert_eq!(
            fs::read_to_string(active.join("lists/list-general-user.txt")).unwrap(),
            "changed-after-update"
        );
    }

    #[test]
    fn rollback_unavailable_recovery_idempotent_and_removal_is_scoped() {
        let temp = Temp::new();
        let active = temp.0.join("binaries");
        let previous = temp.0.join("binaries.previous");
        fixture(&previous, "1.0.0");
        manifest("1.0.0").write_atomic(&previous).unwrap();
        let installation = CoreInstallation::new(&active).unwrap();
        installation.recover().unwrap();
        installation.recover().unwrap();
        assert!(active.is_dir());
        assert!(installation.rollback().unwrap_err().contains("unavailable"));
        let outside = temp.0.join("unrelated");
        fs::create_dir(&outside).unwrap();
        assert!(installation.remove_owned_staging(&outside).is_err());
        assert!(outside.exists());
    }
}
