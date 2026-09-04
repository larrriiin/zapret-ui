use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use super::{Checksum, CoreProvider};

pub const MANIFEST_NAME: &str = ".zapret-ui-core.json";
const MANIFEST_BACKUP_NAME: &str = ".zapret-ui-core.json.backup";
const SCHEMA_VERSION: u32 = 1;
const STAGING_PREFIX: &str = ".binaries-staging-";
const SWAP_NAME: &str = ".binaries-rollback-swap";
const RESTORE_SWAP_NAME: &str = ".binaries-rollback-restore-swap";
const PREVIOUS_BACKUP_NAME: &str = ".binaries-previous-backup";
const USER_FILES: [&str; 3] = [
    "list-general-user.txt",
    "list-exclude-user.txt",
    "ipset-exclude-user.txt",
];
const ACTIVE_FAKE_FILES: [&str; 2] = ["ACTIVE_DISCORD_UDP.bin", "ACTIVE_GAME_UDP.bin"];
const CUSTOM_STRATEGIES_DIR: &str = "custom-strategies";
static MANIFEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CoreManifest {
    pub schema_version: u32,
    pub provider: String,
    #[serde(default)]
    pub channel: Option<String>,
    pub version: String,
    pub source_url: Option<String>,
    pub checksum: Option<Checksum>,
    pub strategy_count: usize,
    #[serde(default)]
    pub imported_legacy: bool,
}

impl CoreManifest {
    pub fn new(
        provider: String,
        version: String,
        source_url: Option<String>,
        checksum: Option<Checksum>,
        strategy_count: usize,
        imported_legacy: bool,
    ) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            provider,
            channel: None,
            version,
            source_url,
            checksum,
            strategy_count,
            imported_legacy,
        }
    }

    pub fn read(root: &Path) -> Result<Self, String> {
        let _guard = MANIFEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let path = root.join(MANIFEST_NAME);
        let backup = root.join(MANIFEST_BACKUP_NAME);
        if !path.exists() && backup.is_file() {
            fs::rename(&backup, &path).map_err(|e| format!("Cannot recover core manifest: {e}"))?;
        } else if path.is_file() && backup.exists() {
            fs::remove_file(&backup)
                .map_err(|e| format!("Cannot remove stale core manifest backup: {e}"))?;
        }
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
        let _guard = MANIFEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let path = root.join(MANIFEST_NAME);
        let backup = root.join(MANIFEST_BACKUP_NAME);
        let temporary = root.join(format!("{MANIFEST_NAME}.tmp-{}", std::process::id()));
        if temporary.exists() {
            fs::remove_file(&temporary)
                .map_err(|e| format!("Cannot remove stale temporary core manifest: {e}"))?;
        }
        if backup.exists() {
            if path.exists() {
                fs::remove_file(&backup)
                    .map_err(|e| format!("Cannot remove stale core manifest backup: {e}"))?;
            } else {
                fs::rename(&backup, &path)
                    .map_err(|e| format!("Cannot recover core manifest before writing: {e}"))?;
            }
        }
        let data = serde_json::to_vec_pretty(self)
            .map_err(|e| format!("Cannot serialize core manifest: {e}"))?;
        fs::write(&temporary, data)
            .map_err(|e| format!("Cannot write temporary core manifest: {e}"))?;
        if path.exists() {
            fs::rename(&path, &backup)
                .map_err(|e| format!("Cannot back up current core manifest: {e}"))?;
        }
        if let Err(error) = fs::rename(&temporary, &path) {
            if backup.exists() {
                let _ = fs::rename(&backup, &path);
            }
            let _ = fs::remove_file(&temporary);
            return Err(format!("Cannot activate core manifest: {error}"));
        }
        if backup.exists() {
            fs::remove_file(&backup)
                .map_err(|e| format!("Cannot remove committed core manifest backup: {e}"))?;
        }
        Ok(())
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
        self.recover_interrupted_rollback_restore()?;
        self.recover_interrupted_rollback()?;
        self.recover_interrupted_activation()?;
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

    pub fn prepare(&self, provider: &dyn CoreProvider) -> Result<(), String> {
        self.recover()?;
        self.prepare_active(provider)
    }

    /// Imports/migrates only the active installation. Unlike `prepare`, this
    /// does not inspect update staging directories and is safe for UI reads.
    pub fn prepare_active(&self, provider: &dyn CoreProvider) -> Result<(), String> {
        if self.active.is_dir() && !self.active.join(MANIFEST_NAME).exists() {
            let (version, count) = provider.validate_installation()?;
            CoreManifest::new(
                provider.provider_name().into(),
                version,
                None,
                None,
                count,
                true,
            )
            .write_atomic(&self.active)?;
        }
        if self.active.is_dir() && self.active.join(MANIFEST_NAME).is_file() {
            provider.finalize_installation()?;
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
        provider: &dyn CoreProvider,
    ) -> Result<CoreManifest, String> {
        self.assert_staging(root)?;
        reject_symlinks(root)?;
        let (actual, count) = provider.validate_installation()?;
        if actual != expected_version {
            return Err(format!(
                "Core version mismatch: expected {expected_version}, found {actual}"
            ));
        }
        let manifest = CoreManifest::new(
            provider.provider_name().into(),
            actual,
            source_url,
            checksum,
            count,
            false,
        );
        serde_json::to_vec(&manifest)
            .map_err(|e| format!("Cannot serialize core manifest: {e}"))?;
        manifest.write_atomic(root)?;
        provider.finalize_installation()?;
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
        let custom_source = from.join(CUSTOM_STRATEGIES_DIR);
        if custom_source.is_dir() {
            let custom_destination = to.join(CUSTOM_STRATEGIES_DIR);
            fs::create_dir_all(&custom_destination)
                .map_err(|e| format!("Cannot create custom strategies directory: {e}"))?;
            for entry in fs::read_dir(&custom_source)
                .map_err(|e| format!("Cannot read custom strategies directory: {e}"))?
            {
                let entry = entry.map_err(|e| format!("Cannot read custom strategy: {e}"))?;
                let file_type = entry
                    .file_type()
                    .map_err(|e| format!("Cannot inspect custom strategy: {e}"))?;
                if file_type.is_symlink() {
                    return Err(format!(
                        "Custom strategy cannot be a symbolic link: {}",
                        entry.path().display()
                    ));
                }
                let path = entry.path();
                if !file_type.is_file()
                    || !path
                        .extension()
                        .and_then(|extension| extension.to_str())
                        .is_some_and(|extension| extension.eq_ignore_ascii_case("bat"))
                {
                    continue;
                }
                fs::copy(&path, custom_destination.join(entry.file_name())).map_err(|e| {
                    format!("Cannot preserve custom strategy {}: {e}", path.display())
                })?;
            }
        }
        Ok(())
    }

    pub fn activate(
        &self,
        staging: &Path,
        active_provider: &dyn CoreProvider,
    ) -> Result<(), String> {
        self.assert_staging(staging)?;
        CoreManifest::read(staging)?;
        if !self.active.exists() {
            if self.previous.exists() {
                return Err("Cannot perform first activation while a previous core exists".into());
            }
            fs::rename(staging, &self.active)
                .map_err(|e| format!("Cannot activate first core: {e}"))?;
            if let Err(error) = active_provider
                .validate_installation()
                .and_then(|_| active_provider.finalize_installation())
            {
                self.assert_owned(&self.active, "binaries")?;
                fs::remove_dir_all(&self.active).map_err(|cleanup| {
                    format!(
                        "Post-activation validation failed ({error}); cleanup failed: {cleanup}"
                    )
                })?;
                return Err(format!("Post-activation validation failed: {error}"));
            }
            return Ok(());
        }
        self.preserve_user_files(&self.active, staging)?;
        let previous_backup = self.parent.join(PREVIOUS_BACKUP_NAME);
        if previous_backup.exists() {
            return Err("Previous-core backup already exists; recovery is required".into());
        }
        if self.previous.exists() {
            self.assert_owned(&self.previous, "binaries.previous")?;
            fs::rename(&self.previous, &previous_backup)
                .map_err(|e| format!("Cannot preserve old previous core: {e}"))?;
        }
        if let Err(error) = fs::rename(&self.active, &self.previous) {
            self.restore_previous_backup(&previous_backup)?;
            return Err(format!("Cannot save active core: {error}"));
        }
        if let Err(error) = fs::rename(staging, &self.active) {
            self.restore_failed_activation(&previous_backup)
                .map_err(|rollback| {
                    format!(
                        "Activation failed ({error}); automatic rollback also failed: {rollback}"
                    )
                })?;
            return Err(format!(
                "Activation failed ({error}); automatic rollback completed"
            ));
        }
        if let Err(error) = active_provider
            .validate_installation()
            .and_then(|_| active_provider.finalize_installation())
        {
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
            self.restore_previous_backup(&previous_backup)?;
            self.remove_owned_staging(&failed)?;
            return Err(format!(
                "Post-activation validation failed ({error}); automatic rollback completed"
            ));
        }
        if previous_backup.exists() {
            self.assert_owned(&previous_backup, PREVIOUS_BACKUP_NAME)?;
            fs::remove_dir_all(&previous_backup)
                .map_err(|e| format!("Cannot remove committed previous-core backup: {e}"))?;
        }
        Ok(())
    }

    pub fn rollback(
        &self,
        active_provider: &dyn CoreProvider,
        previous_provider: &dyn CoreProvider,
    ) -> Result<CoreInstallationState, String> {
        self.rollback_with_post_validation(previous_provider, || {
            active_provider
                .validate_installation()
                .and_then(|_| active_provider.finalize_installation())
        })
    }

    fn rollback_with_post_validation(
        &self,
        previous_provider: &dyn CoreProvider,
        post_validate: impl FnOnce() -> Result<(), String>,
    ) -> Result<CoreInstallationState, String> {
        if !self.previous.is_dir() {
            return Err("Rollback is unavailable: previous core is missing".into());
        }
        CoreManifest::read(&self.previous)?;
        previous_provider.validate_installation()?;
        previous_provider.finalize_installation()?;
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
        if let Err(validation_error) = post_validate() {
            self.reverse_completed_rollback().map_err(|restore_error| {
                format!(
                    "Rollback post-validation failed ({validation_error}); restoration failed: {restore_error}"
                )
            })?;
            return Err(format!(
                "Rollback post-validation failed ({validation_error}); original installation restored"
            ));
        }
        Ok(self.state())
    }

    fn reverse_completed_rollback(&self) -> Result<(), String> {
        let swap = self.parent.join(RESTORE_SWAP_NAME);
        if swap.exists() {
            return Err("Rollback restore swap unexpectedly exists during restoration".into());
        }
        fs::rename(&self.active, &swap)
            .map_err(|e| format!("Cannot move failed rollback core aside: {e}"))?;
        if let Err(error) = fs::rename(&self.previous, &self.active) {
            return Err(format!(
                "Cannot restore original active core ({error}); recovery marker was retained"
            ));
        }
        if let Err(error) = fs::rename(&swap, &self.previous) {
            return Err(format!(
                "Cannot restore original previous core ({error}); original active core is working and recovery marker was retained"
            ));
        }
        Ok(())
    }

    fn recover_interrupted_rollback_restore(&self) -> Result<(), String> {
        let restore_swap = self.parent.join(RESTORE_SWAP_NAME);
        if !restore_swap.exists() {
            return Ok(());
        }
        self.assert_owned(&restore_swap, RESTORE_SWAP_NAME)?;
        match (self.active.exists(), self.previous.exists()) {
            (false, true) => {
                fs::rename(&self.previous, &self.active).map_err(|e| {
                    format!("Cannot restore original active core during recovery: {e}")
                })?;
                fs::rename(&restore_swap, &self.previous).map_err(|e| {
                    format!(
                        "Original active core was recovered, but previous core restoration must be retried: {e}"
                    )
                })?;
            }
            (true, false) => {
                fs::rename(&restore_swap, &self.previous).map_err(|e| {
                    format!("Cannot finish previous core restoration during recovery: {e}")
                })?;
            }
            (true, true) => {
                return Err(
                    "Ambiguous rollback restoration: active, previous, and restore swap all exist"
                        .to_string(),
                );
            }
            (false, false) => {
                return Err(
                    "Cannot recover rollback restoration: only the invalid restore swap remains"
                        .to_string(),
                );
            }
        }
        eprintln!("Recovered interrupted rollback restoration");
        Ok(())
    }

    fn recover_interrupted_rollback(&self) -> Result<(), String> {
        let swap = self.parent.join(SWAP_NAME);
        if !swap.exists() {
            return Ok(());
        }
        self.assert_owned(&swap, SWAP_NAME)?;
        match (self.active.exists(), self.previous.exists()) {
            (false, true) => {
                fs::rename(&swap, &self.active)
                    .map_err(|e| format!("Cannot restore active core from rollback swap: {e}"))?;
            }
            (true, false) => {
                fs::rename(&swap, &self.previous)
                    .map_err(|e| format!("Cannot complete interrupted rollback from swap: {e}"))?;
            }
            (false, false) => {
                fs::rename(&swap, &self.active)
                    .map_err(|e| format!("Cannot recover sole core from rollback swap: {e}"))?;
            }
            (true, true) => {
                return Err(
                    "Ambiguous interrupted rollback: active, previous, and swap all exist"
                        .to_string(),
                );
            }
        }
        eprintln!("Recovered interrupted core rollback");
        Ok(())
    }

    fn recover_interrupted_activation(&self) -> Result<(), String> {
        let backup = self.parent.join(PREVIOUS_BACKUP_NAME);
        if !backup.exists() {
            return Ok(());
        }
        self.assert_owned(&backup, PREVIOUS_BACKUP_NAME)?;
        match (self.active.exists(), self.previous.exists()) {
            (true, false) => self.restore_previous_backup(&backup)?,
            (false, true) => self.restore_failed_activation(&backup)?,
            (true, true) => {
                fs::remove_dir_all(&backup)
                    .map_err(|e| format!("Cannot clean committed previous-core backup: {e}"))?;
            }
            (false, false) => {
                fs::rename(&backup, &self.active)
                    .map_err(|e| format!("Cannot recover sole previous-core backup: {e}"))?;
            }
        }
        eprintln!("Recovered interrupted core activation");
        Ok(())
    }

    fn restore_failed_activation(&self, previous_backup: &Path) -> Result<(), String> {
        fs::rename(&self.previous, &self.active)
            .map_err(|e| format!("Cannot restore active core: {e}"))?;
        self.restore_previous_backup(previous_backup)
    }

    fn restore_previous_backup(&self, previous_backup: &Path) -> Result<(), String> {
        if previous_backup.exists() {
            fs::rename(previous_backup, &self.previous)
                .map_err(|e| format!("Cannot restore old previous core: {e}"))?;
        }
        Ok(())
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
    use crate::core::CoreManager;

    fn provider(root: &Path) -> &dyn CoreProvider {
        // Managers are intentionally leaked only in tests so returned provider references
        // remain valid for the duration of each isolated filesystem scenario.
        Box::leak(Box::new(CoreManager::new(root))).provider()
    }

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
        CoreManifest::new("flowseal".into(), version.into(), None, None, 1, false)
    }

    #[test]
    fn manifest_round_trip_and_unknown_schema_rejected() {
        let temp = Temp::new();
        manifest("1.2.3").write_atomic(&temp.0).unwrap();
        assert_eq!(CoreManifest::read(&temp.0).unwrap(), manifest("1.2.3"));
        manifest("1.2.4").write_atomic(&temp.0).unwrap();
        assert_eq!(CoreManifest::read(&temp.0).unwrap(), manifest("1.2.4"));
        assert!(!temp.0.join(MANIFEST_BACKUP_NAME).exists());
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
        installation.prepare(provider(&active)).unwrap();
        let imported = CoreManifest::read(&active).unwrap();
        assert!(imported.imported_legacy);
        assert_eq!(imported.version, "1.0.0");
        assert!(!active.join("service.bat").exists());
        assert_eq!(provider(&active).local_version(), "1.0.0");
        assert!(provider(&active).is_installed());
        assert!(!installation.state().rollback_available);
    }

    #[test]
    fn preserves_custom_strategies_during_core_replacement() {
        let temp = Temp::new();
        let active = temp.0.join("binaries");
        let staging = temp.0.join("candidate");
        fs::create_dir_all(active.join(CUSTOM_STRATEGIES_DIR)).unwrap();
        fs::create_dir_all(staging.join("bin")).unwrap();
        fs::create_dir_all(staging.join("lists")).unwrap();
        fs::write(
            active.join(CUSTOM_STRATEGIES_DIR).join("mine.bat"),
            "winws.exe --wf-tcp=80",
        )
        .unwrap();

        let installation = CoreInstallation::new(&active).unwrap();
        installation.preserve_user_files(&active, &staging).unwrap();

        assert_eq!(
            fs::read_to_string(staging.join(CUSTOM_STRATEGIES_DIR).join("mine.bat")).unwrap(),
            "winws.exe --wf-tcp=80"
        );
    }

    #[test]
    fn validates_fixture_and_reports_missing_required_files() {
        let temp = Temp::new();
        let active = temp.0.join("binaries");
        fs::create_dir(&active).unwrap();
        let installation = CoreInstallation::new(&active).unwrap();
        let staging = installation.create_staging().unwrap();
        fixture(&staging, "2.0.0");
        fs::remove_file(staging.join("bin/winws.exe")).unwrap();
        assert!(installation
            .validate_and_manifest(&staging, "2.0.0", None, None, provider(&staging))
            .unwrap_err()
            .contains("Missing required"));
        installation.remove_owned_staging(&staging).unwrap();
        let staging = installation.create_staging().unwrap();
        fixture(&staging, "2.0.0");
        fs::remove_file(staging.join("service.bat")).unwrap();
        assert!(installation
            .validate_and_manifest(&staging, "2.0.0", None, None, provider(&staging))
            .unwrap_err()
            .contains("manifest"));
        installation.remove_owned_staging(&staging).unwrap();
        let staging = installation.create_staging().unwrap();
        fixture(&staging, "2.0.0");
        installation
            .validate_and_manifest(&staging, "2.0.0", None, None, provider(&staging))
            .unwrap();
        assert!(!staging.join("service.bat").exists());
        assert!(!staging.join("utils/test zapret.ps1").exists());
        assert_eq!(provider(&staging).local_version(), "2.0.0");
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
            .validate_and_manifest(&staging, "2.0.0", None, None, provider(&staging))
            .unwrap_err()
            .contains("No Flowseal strategies"));
        installation.remove_owned_staging(&staging).unwrap();
        let staging = installation.create_staging().unwrap();
        fixture(&staging, "2.0.0");
        fs::write(staging.join("general.bat"), "invalid").unwrap();
        assert!(installation
            .validate_and_manifest(&staging, "2.0.0", None, None, provider(&staging))
            .unwrap_err()
            .contains("cannot be parsed"));
        installation.remove_owned_staging(&staging).unwrap();
        let staging = installation.create_staging().unwrap();
        fixture(&staging, "2.0.0");
        assert!(installation
            .validate_and_manifest(&staging, "3.0.0", None, None, provider(&staging))
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
            .validate_and_manifest(&staging, "2.0.0", None, None, provider(&staging))
            .unwrap();
        installation.activate(&staging, provider(&active)).unwrap();
        assert_eq!(
            installation.state().current_version.as_deref(),
            Some("2.0.0")
        );
        fs::write(
            active.join("lists/list-general-user.txt"),
            "changed-after-update",
        )
        .unwrap();
        let state = installation
            .rollback(
                provider(&active),
                provider(&temp.0.join("binaries.previous")),
            )
            .unwrap();
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
        assert!(installation
            .rollback(
                provider(&active),
                provider(&temp.0.join("binaries.previous"))
            )
            .unwrap_err()
            .contains("unavailable"));
        let outside = temp.0.join("unrelated");
        fs::create_dir(&outside).unwrap();
        assert!(installation.remove_owned_staging(&outside).is_err());
        assert!(outside.exists());
    }

    #[test]
    fn recovers_every_interrupted_rollback_state_idempotently() {
        for (active_exists, previous_exists) in [(false, true), (true, false), (false, false)] {
            let temp = Temp::new();
            let active = temp.0.join("binaries");
            let previous = temp.0.join("binaries.previous");
            let swap = temp.0.join(SWAP_NAME);
            if active_exists {
                fixture(&active, "2.0.0");
                manifest("2.0.0").write_atomic(&active).unwrap();
            }
            if previous_exists {
                fixture(&previous, "1.0.0");
                manifest("1.0.0").write_atomic(&previous).unwrap();
            }
            fixture(&swap, "2.0.0");
            manifest("2.0.0").write_atomic(&swap).unwrap();
            let installation = CoreInstallation::new(&active).unwrap();
            installation.recover().unwrap();
            installation.recover().unwrap();
            assert!(active.exists());
            assert!(!swap.exists());
            if active_exists && !previous_exists {
                assert!(previous.exists());
            }
        }
    }

    #[test]
    fn recovers_interrupted_activation_and_restores_old_previous() {
        let temp = Temp::new();
        let active = temp.0.join("binaries");
        let previous = temp.0.join("binaries.previous");
        let backup = temp.0.join(PREVIOUS_BACKUP_NAME);
        fixture(&previous, "2.0.0");
        manifest("2.0.0").write_atomic(&previous).unwrap();
        fixture(&backup, "0.9.0");
        manifest("0.9.0").write_atomic(&backup).unwrap();
        let installation = CoreInstallation::new(&active).unwrap();
        installation.recover().unwrap();
        installation.recover().unwrap();
        assert_eq!(CoreManifest::read(&active).unwrap().version, "2.0.0");
        assert_eq!(CoreManifest::read(&previous).unwrap().version, "0.9.0");
        assert!(!backup.exists());
    }

    #[test]
    fn successful_activation_replaces_previous_only_after_commit() {
        let temp = Temp::new();
        let active = temp.0.join("binaries");
        let previous = temp.0.join("binaries.previous");
        fixture(&active, "1.0.0");
        manifest("1.0.0").write_atomic(&active).unwrap();
        fs::create_dir(active.join(CUSTOM_STRATEGIES_DIR)).unwrap();
        fs::write(
            active.join(CUSTOM_STRATEGIES_DIR).join("mine.bat"),
            "winws.exe --wf-tcp=443",
        )
        .unwrap();
        fixture(&previous, "0.9.0");
        manifest("0.9.0").write_atomic(&previous).unwrap();
        let installation = CoreInstallation::new(&active).unwrap();
        let staging = installation.create_staging().unwrap();
        fixture(&staging, "2.0.0");
        installation
            .validate_and_manifest(&staging, "2.0.0", None, None, provider(&staging))
            .unwrap();
        installation.activate(&staging, provider(&active)).unwrap();
        assert_eq!(CoreManifest::read(&active).unwrap().version, "2.0.0");
        assert_eq!(CoreManifest::read(&previous).unwrap().version, "1.0.0");
        assert!(active
            .join(CUSTOM_STRATEGIES_DIR)
            .join("mine.bat")
            .is_file());
        assert!(provider(&active)
            .strategies()
            .unwrap()
            .contains(&"mine".to_string()));
        assert!(!temp.0.join(PREVIOUS_BACKUP_NAME).exists());
    }

    #[test]
    fn successful_first_install_has_no_rollback() {
        let temp = Temp::new();
        let active = temp.0.join("binaries");
        let installation = CoreInstallation::new(&active).unwrap();
        let staging = installation.create_staging().unwrap();
        fixture(&staging, "2.0.0");
        installation
            .validate_and_manifest(&staging, "2.0.0", None, None, provider(&staging))
            .unwrap();
        installation.activate(&staging, provider(&active)).unwrap();
        assert_eq!(CoreManifest::read(&active).unwrap().version, "2.0.0");
        assert!(!installation.state().rollback_available);
        assert!(installation
            .rollback(provider(&active), provider(&active))
            .is_err());
        installation.recover().unwrap();
        installation.recover().unwrap();
        assert!(active.exists());
    }

    #[test]
    fn failed_first_install_leaves_no_partial_active_core() {
        let temp = Temp::new();
        let active = temp.0.join("binaries");
        let installation = CoreInstallation::new(&active).unwrap();
        let staging = installation.create_staging().unwrap();
        fixture(&staging, "2.0.0");
        installation
            .validate_and_manifest(&staging, "2.0.0", None, None, provider(&staging))
            .unwrap();
        fs::remove_file(staging.join("bin/winws.exe")).unwrap();
        assert!(installation.activate(&staging, provider(&active)).is_err());
        assert!(!active.exists());
        installation.recover().unwrap();
        installation.recover().unwrap();
        assert!(!active.exists());
    }

    #[test]
    fn failed_activation_restores_active_and_old_previous_and_cleans_candidate() {
        let temp = Temp::new();
        let active = temp.0.join("binaries");
        let previous = temp.0.join("binaries.previous");
        fixture(&active, "1.0.0");
        manifest("1.0.0").write_atomic(&active).unwrap();
        fixture(&previous, "0.9.0");
        manifest("0.9.0").write_atomic(&previous).unwrap();
        let installation = CoreInstallation::new(&active).unwrap();
        let staging = installation.create_staging().unwrap();
        fixture(&staging, "2.0.0");
        installation
            .validate_and_manifest(&staging, "2.0.0", None, None, provider(&staging))
            .unwrap();
        fs::remove_file(staging.join("bin/winws.exe")).unwrap();
        assert!(installation
            .activate(&staging, provider(&active))
            .unwrap_err()
            .contains("automatic rollback completed"));
        assert_eq!(CoreManifest::read(&active).unwrap().version, "1.0.0");
        assert_eq!(CoreManifest::read(&previous).unwrap().version, "0.9.0");
        assert!(!staging.exists());
        assert!(!temp.0.join(PREVIOUS_BACKUP_NAME).exists());
    }

    #[test]
    fn rollback_post_validation_failure_restores_original_directories() {
        let temp = Temp::new();
        let active = temp.0.join("binaries");
        let previous = temp.0.join("binaries.previous");
        fixture(&active, "2.0.0");
        manifest("2.0.0").write_atomic(&active).unwrap();
        fixture(&previous, "1.0.0");
        manifest("1.0.0").write_atomic(&previous).unwrap();
        let installation = CoreInstallation::new(&active).unwrap();
        let error = installation
            .rollback_with_post_validation(provider(&previous), || {
                Err("simulated validation failure".to_string())
            })
            .unwrap_err();
        assert!(error.contains("original installation restored"));
        assert_eq!(CoreManifest::read(&active).unwrap().version, "2.0.0");
        assert_eq!(CoreManifest::read(&previous).unwrap().version, "1.0.0");
        assert!(!temp.0.join(SWAP_NAME).exists());
    }

    #[test]
    fn recovers_rollback_restore_before_original_active_was_restored() {
        let temp = Temp::new();
        let active = temp.0.join("binaries");
        let previous = temp.0.join("binaries.previous");
        let restore_swap = temp.0.join(RESTORE_SWAP_NAME);
        fixture(&previous, "2.0.0");
        manifest("2.0.0").write_atomic(&previous).unwrap();
        fixture(&restore_swap, "1.0.0");
        manifest("1.0.0").write_atomic(&restore_swap).unwrap();

        let installation = CoreInstallation::new(&active).unwrap();
        installation.recover().unwrap();
        installation.recover().unwrap();

        assert_eq!(CoreManifest::read(&active).unwrap().version, "2.0.0");
        assert_eq!(CoreManifest::read(&previous).unwrap().version, "1.0.0");
        assert!(!restore_swap.exists());
    }

    #[test]
    fn recovers_rollback_restore_after_original_active_was_restored() {
        let temp = Temp::new();
        let active = temp.0.join("binaries");
        let previous = temp.0.join("binaries.previous");
        let restore_swap = temp.0.join(RESTORE_SWAP_NAME);
        fixture(&active, "2.0.0");
        manifest("2.0.0").write_atomic(&active).unwrap();
        fixture(&restore_swap, "1.0.0");
        manifest("1.0.0").write_atomic(&restore_swap).unwrap();

        let installation = CoreInstallation::new(&active).unwrap();
        installation.recover().unwrap();
        installation.recover().unwrap();

        assert_eq!(CoreManifest::read(&active).unwrap().version, "2.0.0");
        assert_eq!(CoreManifest::read(&previous).unwrap().version, "1.0.0");
        assert!(!restore_swap.exists());
    }
}
