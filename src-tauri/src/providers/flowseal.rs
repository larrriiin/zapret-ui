use serde::{Deserialize, Serialize};
use std::{
    cmp::Ordering,
    fs,
    path::{Path, PathBuf},
};

use crate::core::{CoreManifest, CorePaths, CoreProvider};

const IPSET_URL: &str = "https://raw.githubusercontent.com/Flowseal/zapret-discord-youtube/refs/heads/main/.service/ipset-service.txt";
const STRATEGY_CATALOG_NAME: &str = "strategies.json";
const STRATEGY_CATALOG_BACKUP_NAME: &str = ".strategies.json.backup";
const CUSTOM_STRATEGIES_DIR: &str = "custom-strategies";
const STRATEGY_CATALOG_SCHEMA_VERSION: u32 = 3;
const CMD_ESCAPED_CATALOG_SCHEMA_VERSION: u32 = 2;
static STRATEGY_CATALOG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct StrategyCatalog {
    schema_version: u32,
    strategies: Vec<StrategyCatalogEntry>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct StrategyCatalogEntry {
    name: String,
    arguments: String,
    source: StrategySource,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum StrategySource {
    Flowseal,
    Custom,
}

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

    /// BAT strategies are parsed without going through cmd.exe, so reproduce
    /// cmd's caret escaping for unquoted arguments before storing or launching
    /// them. For example, Flowseal writes `^!` to pass a literal `!` to winws.
    fn unescape_cmd_arguments(arguments: &str) -> String {
        let mut result = String::with_capacity(arguments.len());
        let mut chars = arguments.chars().peekable();
        let mut in_quotes = false;
        while let Some(character) = chars.next() {
            match character {
                '"' => {
                    in_quotes = !in_quotes;
                    result.push(character);
                }
                '^' if !in_quotes => match chars.next() {
                    Some(escaped) => result.push(escaped),
                    None => result.push(character),
                },
                _ => result.push(character),
            }
        }
        result
    }

    fn migrate_catalog(catalog: &mut StrategyCatalog) -> bool {
        if catalog.schema_version != CMD_ESCAPED_CATALOG_SCHEMA_VERSION {
            return false;
        }
        for strategy in &mut catalog.strategies {
            strategy.arguments = Self::unescape_cmd_arguments(&strategy.arguments);
        }
        catalog.schema_version = STRATEGY_CATALOG_SCHEMA_VERSION;
        true
    }

    fn extract_strategy_arguments(content: &str, strategy: &str) -> Result<String, String> {
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
        let arguments = command[offset..].trim();
        if arguments.is_empty() {
            return Err(format!("Не найдены аргументы winws.exe в {}.bat", strategy));
        }
        Ok(Self::unescape_cmd_arguments(arguments))
    }

    fn resolve_strategy_arguments(&self, arguments: &str, game_filter: &str) -> String {
        let (gf, gftcp, gfudp) = match game_filter {
            "all" => ("1024-65535", "1024-65535", "1024-65535"),
            "tcp" => ("1024-65535", "1024-65535", "12"),
            "udp" => ("1024-65535", "12", "1024-65535"),
            _ => ("12", "12", "12"),
        };
        let suffix = std::path::MAIN_SEPARATOR.to_string();
        let bin = self.paths.bin_dir().to_string_lossy().to_string() + &suffix;
        let lists = self.paths.lists_dir().to_string_lossy().to_string() + &suffix;
        let root = self.paths.root().to_string_lossy().to_string() + &suffix;
        let args = arguments
            .replace("%GameFilter%", gf)
            .replace("%GameFilterTCP%", gftcp)
            .replace("%GameFilterUDP%", gfudp)
            .replace("%BIN%", &bin)
            .replace("%LISTS%", &lists);
        args.split_whitespace()
            .map(|word| {
                if let Some(rest) = word.strip_prefix("\"@") {
                    format!("\"{}{}", root, rest)
                } else {
                    word.to_owned()
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn strategy_files(directory: &Path) -> Result<Vec<(String, PathBuf)>, String> {
        if !directory.is_dir() {
            return Ok(Vec::new());
        }
        let mut result = fs::read_dir(directory)
            .map_err(|e| format!("Failed to read strategies ({}): {e}", directory.display()))?
            .filter_map(Result::ok)
            .filter(|entry| {
                entry.file_type().is_ok_and(|file_type| file_type.is_file())
                    && entry
                        .path()
                        .extension()
                        .and_then(|extension| extension.to_str())
                        .is_some_and(|extension| extension.eq_ignore_ascii_case("bat"))
            })
            .filter_map(|entry| {
                let path = entry.path();
                let name = path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .map(str::to_owned)?;
                Some((name, path))
            })
            .collect::<Vec<_>>();
        result.sort_by(|(a, _), (b, _)| natural_compare(a, b));
        Ok(result)
    }

    fn upstream_strategy_files(&self) -> Result<Vec<(String, PathBuf)>, String> {
        Ok(Self::strategy_files(self.paths.root())?
            .into_iter()
            .filter(|(name, _)| !name.eq_ignore_ascii_case("service"))
            .collect())
    }

    fn custom_strategy_files(&self) -> Result<Vec<(String, PathBuf)>, String> {
        Self::strategy_files(&self.paths.root().join(CUSTOM_STRATEGIES_DIR))
    }

    fn catalog_path(&self) -> PathBuf {
        self.paths.root().join(STRATEGY_CATALOG_NAME)
    }

    fn upstream_service_script(&self) -> PathBuf {
        self.paths.root().join("service.bat")
    }

    fn upstream_test_script(&self) -> PathBuf {
        self.paths.utils_dir().join("test zapret.ps1")
    }

    fn version_from_upstream_service(&self) -> Result<String, String> {
        let path = self.upstream_service_script();
        let content = fs::read_to_string(&path).map_err(|e| {
            format!(
                "Cannot read upstream version source {}: {e}",
                path.display()
            )
        })?;
        Self::parse_local_version(&content)
            .map_err(|e| format!("Cannot determine Flowseal version: {e}"))
    }

    fn version_from_manifest(&self) -> Result<String, String> {
        let manifest = CoreManifest::read(self.paths.root())?;
        if manifest.provider != self.provider_name() {
            return Err(format!(
                "Core manifest provider mismatch: expected {}, found {}",
                self.provider_name(),
                manifest.provider
            ));
        }
        Ok(manifest.version)
    }

    fn build_catalog(
        &self,
        upstream_files: &[(String, PathBuf)],
    ) -> Result<StrategyCatalog, String> {
        let mut strategies: Vec<StrategyCatalogEntry> =
            if upstream_files.is_empty() && self.catalog_path().is_file() {
                let mut existing = self.read_catalog_file()?;
                Self::migrate_catalog(&mut existing);
                existing
                    .strategies
                    .into_iter()
                    .filter(|entry| entry.source == StrategySource::Flowseal)
                    .collect()
            } else {
                Vec::new()
            };
        for (source, files) in [
            (StrategySource::Flowseal, upstream_files.to_vec()),
            (StrategySource::Custom, self.custom_strategy_files()?),
        ] {
            for (name, path) in files {
                let content = fs::read_to_string(&path)
                    .map_err(|e| format!("Не удалось прочитать {}: {e}", path.display()))?;
                let entry = StrategyCatalogEntry {
                    arguments: Self::extract_strategy_arguments(&content, &name)?,
                    name: name.clone(),
                    source,
                };
                if let Some(existing) = strategies
                    .iter_mut()
                    .find(|item| item.name.eq_ignore_ascii_case(&name))
                {
                    // A custom strategy intentionally takes precedence if a future
                    // Flowseal release introduces a preset with the same name.
                    *existing = entry;
                } else {
                    strategies.push(entry);
                }
            }
        }
        strategies.sort_by(|a, b| natural_compare(&a.name, &b.name));
        Ok(StrategyCatalog {
            schema_version: STRATEGY_CATALOG_SCHEMA_VERSION,
            strategies,
        })
    }

    fn write_catalog(&self, catalog: &StrategyCatalog) -> Result<(), String> {
        let path = self.catalog_path();
        let backup = self.paths.root().join(STRATEGY_CATALOG_BACKUP_NAME);
        let temporary = self.paths.root().join(format!(
            "{STRATEGY_CATALOG_NAME}.tmp-{}",
            std::process::id()
        ));
        if temporary.exists() {
            fs::remove_file(&temporary)
                .map_err(|e| format!("Cannot clean temporary strategy catalog: {e}"))?;
        }
        if backup.exists() {
            if path.exists() {
                fs::remove_file(&backup)
                    .map_err(|e| format!("Cannot clean strategy catalog backup: {e}"))?;
            } else {
                fs::rename(&backup, &path)
                    .map_err(|e| format!("Cannot recover strategy catalog backup: {e}"))?;
            }
        }
        let data = serde_json::to_vec_pretty(catalog)
            .map_err(|e| format!("Cannot serialize strategy catalog: {e}"))?;
        fs::write(&temporary, data)
            .map_err(|e| format!("Cannot write temporary strategy catalog: {e}"))?;
        if path.exists() {
            fs::rename(&path, &backup)
                .map_err(|e| format!("Cannot prepare strategy catalog replacement: {e}"))?;
        }
        if let Err(error) = fs::rename(&temporary, &path) {
            let restore_error = if backup.exists() {
                fs::rename(&backup, &path).err()
            } else {
                None
            };
            let _ = fs::remove_file(&temporary);
            return match restore_error {
                Some(restore) => Err(format!(
                    "Cannot activate strategy catalog ({error}); cannot restore previous catalog: {restore}"
                )),
                None => Err(format!("Cannot activate strategy catalog: {error}")),
            };
        }
        if backup.exists() {
            if let Err(error) = fs::remove_file(&backup) {
                eprintln!("Cannot remove stale strategy catalog backup: {error}");
            }
        }
        Ok(())
    }

    fn recover_catalog_backup(&self) -> Result<(), String> {
        let path = self.catalog_path();
        let backup = self.paths.root().join(STRATEGY_CATALOG_BACKUP_NAME);
        if !backup.exists() {
            return Ok(());
        }
        if path.exists() {
            fs::remove_file(&backup)
                .map_err(|e| format!("Cannot clean strategy catalog backup: {e}"))
        } else {
            fs::rename(&backup, &path)
                .map_err(|e| format!("Cannot recover strategy catalog backup: {e}"))
        }
    }

    fn remove_upstream_strategy_files(
        &self,
        strategies: &[(String, PathBuf)],
    ) -> Result<(), String> {
        for (_, path) in strategies {
            fs::remove_file(path).map_err(|e| {
                format!(
                    "Cannot remove imported Flowseal strategy {}: {e}",
                    path.display()
                )
            })?;
        }
        Ok(())
    }

    fn read_catalog_file(&self) -> Result<StrategyCatalog, String> {
        let path = self.catalog_path();
        let metadata =
            fs::metadata(&path).map_err(|e| format!("Cannot inspect strategy catalog: {e}"))?;
        if metadata.len() > 4 * 1024 * 1024 {
            return Err("Strategy catalog exceeds the 4 MB size limit".to_string());
        }
        let data = fs::read(&path).map_err(|e| format!("Cannot read strategy catalog: {e}"))?;
        let catalog: StrategyCatalog = serde_json::from_slice(&data)
            .map_err(|e| format!("Strategy catalog is corrupted: {e}"))?;
        if catalog.schema_version != STRATEGY_CATALOG_SCHEMA_VERSION
            && catalog.schema_version != CMD_ESCAPED_CATALOG_SCHEMA_VERSION
        {
            return Err(format!(
                "Unsupported strategy catalog schema version: {}",
                catalog.schema_version
            ));
        }
        for (index, entry) in catalog.strategies.iter().enumerate() {
            if entry.name.trim().is_empty() || entry.arguments.trim().is_empty() {
                return Err(format!("Strategy catalog entry {index} is incomplete"));
            }
            if catalog.strategies[..index]
                .iter()
                .any(|previous| previous.name.eq_ignore_ascii_case(&entry.name))
            {
                return Err(format!(
                    "Strategy catalog contains duplicate name: {}",
                    entry.name
                ));
            }
        }
        Ok(catalog)
    }

    fn rebuild_catalog_locked(&self) -> Result<StrategyCatalog, String> {
        self.recover_catalog_backup()?;
        let upstream_files = self.upstream_strategy_files()?;
        let catalog = self.build_catalog(&upstream_files)?;
        self.write_catalog(&catalog)?;
        self.remove_upstream_strategy_files(&upstream_files)?;
        Ok(catalog)
    }

    fn rebuild_catalog(&self) -> Result<StrategyCatalog, String> {
        let _guard = STRATEGY_CATALOG_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        self.rebuild_catalog_locked()
    }

    fn read_catalog(&self) -> Result<StrategyCatalog, String> {
        let _guard = STRATEGY_CATALOG_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        self.recover_catalog_backup()?;
        match self.read_catalog_file() {
            Ok(mut catalog) => {
                if self.upstream_strategy_files()?.is_empty() {
                    if Self::migrate_catalog(&mut catalog) {
                        self.write_catalog(&catalog)?;
                    }
                    Ok(catalog)
                } else {
                    self.rebuild_catalog_locked()
                }
            }
            Err(error) => {
                if self.upstream_strategy_files()?.is_empty() {
                    Err(error)
                } else {
                    self.rebuild_catalog_locked()
                }
            }
        }
    }
}

impl CoreProvider for FlowsealProvider {
    fn provider_name(&self) -> &'static str {
        "flowseal"
    }
    fn paths(&self) -> &CorePaths {
        &self.paths
    }
    fn ipset_url(&self) -> &'static str {
        IPSET_URL
    }
    fn winws_executable(&self) -> PathBuf {
        self.paths.bin_dir().join("winws.exe")
    }
    fn local_version(&self) -> String {
        self.version_from_manifest()
            .or_else(|_| self.version_from_upstream_service())
            .unwrap_or_else(|e| format!("Err: {e}"))
    }
    fn is_installed(&self) -> bool {
        let runtime_ready = self.winws_executable().is_file() && self.catalog_path().is_file();
        let managed = self.version_from_manifest().is_ok() && runtime_ready;
        let legacy = self.upstream_service_script().is_file() && self.winws_executable().is_file();
        self.paths.root().is_dir() && (managed || legacy)
    }
    fn strategies(&self) -> Result<Vec<String>, String> {
        Ok(self
            .read_catalog()?
            .strategies
            .into_iter()
            .map(|strategy| strategy.name)
            .collect())
    }
    fn parse_strategy(&self, strategy: &str, game_filter: &str) -> Result<String, String> {
        let catalog = self.read_catalog()?;
        let entry = catalog
            .strategies
            .iter()
            .find(|entry| entry.name.eq_ignore_ascii_case(strategy))
            .ok_or_else(|| format!("Стратегия не найдена в каталоге: {strategy}"))?;
        Ok(self.resolve_strategy_arguments(&entry.arguments, game_filter))
    }
    fn import_custom_strategy(&self, name: &str, content: &str) -> Result<(), String> {
        if content.is_empty() || content.len() > 256 * 1024 || content.contains('\0') {
            return Err("strategy_import_error_invalid_content".to_string());
        }
        if name.eq_ignore_ascii_case("service") {
            return Err("strategy_import_error_reserved_name".to_string());
        }
        Self::extract_strategy_arguments(content, name)
            .map_err(|_| "strategy_import_error_parse".to_string())?;

        let directory = self.paths.root().join(CUSTOM_STRATEGIES_DIR);
        fs::create_dir_all(&directory).map_err(|e| format!("strategy_import_error_write: {e}"))?;
        let path = directory.join(format!("{name}.bat"));
        if path.exists() {
            return Err("strategy_import_error_exists".to_string());
        }
        let temporary = directory.join(format!(".{name}.tmp-{}", std::process::id()));
        fs::write(&temporary, content).map_err(|e| format!("strategy_import_error_write: {e}"))?;
        if let Err(error) = fs::rename(&temporary, &path) {
            let _ = fs::remove_file(&temporary);
            return Err(format!("strategy_import_error_write: {error}"));
        }
        if let Err(error) = self.rebuild_catalog() {
            let _ = fs::remove_file(&path);
            return Err(format!("strategy_import_error_catalog: {error}"));
        }
        Ok(())
    }
    fn validate_installation(&self) -> Result<(String, usize), String> {
        let root = self.paths.root();
        if !root.is_dir() {
            return Err(format!(
                "Unexpected archive structure: {} is not a directory",
                root.display()
            ));
        }
        for path in [self.winws_executable()] {
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
        let upstream_strategies = self.upstream_strategy_files()?;
        for (strategy, path) in &upstream_strategies {
            let content = fs::read_to_string(path)
                .map_err(|e| format!("Cannot read strategy {strategy}: {e}"))?;
            Self::extract_strategy_arguments(&content, strategy)
                .map_err(|e| format!("Strategy {strategy} cannot be parsed: {e}"))?;
        }
        // A downloaded or legacy Flowseal package still carries service.bat.
        // Once it has been imported, the manifest is the authoritative version source.
        let version = if self.upstream_service_script().is_file() {
            self.version_from_upstream_service()?
        } else {
            self.version_from_manifest()?
        };
        let catalog = self.rebuild_catalog()?;
        let flowseal_count = catalog
            .strategies
            .iter()
            .filter(|entry| entry.source == StrategySource::Flowseal)
            .count();
        if flowseal_count == 0 {
            return Err("No Flowseal strategies found in strategy catalog".to_string());
        }
        Ok((version, flowseal_count))
    }

    fn finalize_installation(&self) -> Result<(), String> {
        // Never remove the only upstream version source until a readable manifest
        // for this provider has been committed successfully.
        let manifest_version = self.version_from_manifest()?;
        let path = self.upstream_service_script();
        if path.is_file() {
            let upstream_version = self.version_from_upstream_service()?;
            if upstream_version != manifest_version {
                return Err(format!(
                    "Flowseal version mismatch before cleanup: manifest has {manifest_version}, service.bat has {upstream_version}"
                ));
            }
            fs::remove_file(&path).map_err(|e| {
                format!(
                    "Cannot remove imported upstream service script {}: {e}",
                    path.display()
                )
            })?;
        }
        let test_script = self.upstream_test_script();
        if test_script.is_file() {
            fs::remove_file(&test_script).map_err(|e| {
                format!(
                    "Cannot remove imported upstream test script {}: {e}",
                    test_script.display()
                )
            })?;
        }
        Ok(())
    }
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
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let p =
            std::env::temp_dir().join(format!("zapret-provider-{}-{stamp}", std::process::id()));
        std::fs::create_dir_all(&p).expect("temp");
        p
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
            let content = if n == "service.bat" {
                "set local_version=1\n"
            } else {
                "\"%BIN%winws.exe\" --wf-tcp=80\n"
            };
            std::fs::write(root.join(n), content).expect("write");
        }
        std::fs::write(
            root.join(STRATEGY_CATALOG_NAME),
            serde_json::to_vec(&StrategyCatalog {
                schema_version: 1,
                strategies: Vec::new(),
            })
            .unwrap(),
        )
        .expect("legacy catalog");
        let p = FlowsealProvider::new(&root);
        assert_eq!(p.strategies().expect("strategies"), ["ALT2", "ALT11"]);
        assert!(root.join(STRATEGY_CATALOG_NAME).is_file());
        assert!(!root.join("ALT2.bat").exists());
        assert!(!root.join("ALT11.bat").exists());
        assert!(root.join("service.bat").is_file());
        assert_eq!(
            p.read_catalog_file().unwrap().schema_version,
            STRATEGY_CATALOG_SCHEMA_VERSION
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn recovers_catalog_backup_without_flowseal_bat_files() {
        let root = temp();
        let catalog = StrategyCatalog {
            schema_version: STRATEGY_CATALOG_SCHEMA_VERSION,
            strategies: vec![StrategyCatalogEntry {
                name: "general".to_string(),
                arguments: "--wf-tcp=80".to_string(),
                source: StrategySource::Flowseal,
            }],
        };
        std::fs::write(
            root.join(STRATEGY_CATALOG_BACKUP_NAME),
            serde_json::to_vec(&catalog).unwrap(),
        )
        .expect("backup catalog");
        let provider = FlowsealProvider::new(&root);

        assert_eq!(provider.strategies().unwrap(), ["general"]);
        assert!(root.join(STRATEGY_CATALOG_NAME).is_file());
        assert!(!root.join(STRATEGY_CATALOG_BACKUP_NAME).exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn imports_custom_strategy_and_runs_it_from_catalog() {
        let root = temp();
        std::fs::write(root.join("general.bat"), "\"%BIN%winws.exe\" --wf-tcp=80\n")
            .expect("upstream strategy");
        let provider = FlowsealProvider::new(&root);
        provider
            .import_custom_strategy(
                "My strategy",
                "\"%BIN%winws.exe\" --filter-udp=%GameFilterUDP% --hostlist=\"%LISTS%list.txt\"\n",
            )
            .expect("import custom strategy");

        assert_eq!(
            provider.strategies().expect("catalog"),
            ["My strategy", "general"]
        );
        let custom_path = root.join(CUSTOM_STRATEGIES_DIR).join("My strategy.bat");
        assert!(custom_path.is_file());
        std::fs::remove_file(custom_path).expect("remove source after catalog generation");
        let arguments = provider
            .parse_strategy("My strategy", "udp")
            .expect("parse catalog entry");
        assert!(arguments.contains("--filter-udp=1024-65535"));
        assert!(arguments.contains("lists"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_duplicate_and_unparseable_custom_strategies() {
        let root = temp();
        let provider = FlowsealProvider::new(&root);
        assert_eq!(
            provider.import_custom_strategy("broken", "echo nothing"),
            Err("strategy_import_error_parse".to_string())
        );
        provider
            .import_custom_strategy("custom", "winws.exe --wf-tcp=80")
            .expect("first import");
        assert_eq!(
            provider.import_custom_strategy("custom", "winws.exe --wf-tcp=443"),
            Err("strategy_import_error_exists".to_string())
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn keeps_service_source_when_manifest_version_does_not_match() {
        let root = temp();
        std::fs::write(root.join("service.bat"), "set local_version=1.0.0\n").unwrap();
        CoreManifest::new("flowseal".into(), "2.0.0".into(), None, None, 1, false)
            .write_atomic(&root)
            .unwrap();
        let provider = FlowsealProvider::new(&root);

        assert!(provider.finalize_installation().is_err());
        assert!(root.join("service.bat").is_file());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn parses_multiline_strategy_and_variables() {
        let p = FlowsealProvider::new("core");
        let template = FlowsealProvider::extract_strategy_arguments("\"%BIN%winws.exe\" --filter-tcp=%GameFilterTCP% ^\n --hostlist=\"%LISTS%list.txt\" --fake=^! --literal-caret=^^ --quoted=\"^!\" --fake-file=\"@bin\\fake.bin\"", "ALT").expect("parse");
        let args = p.resolve_strategy_arguments(&template, "tcp");
        assert!(args.contains("1024-65535"));
        assert!(args.contains("core"));
        assert!(args.contains("--fake=!"));
        assert!(args.contains("--literal-caret=^"));
        assert!(args.contains("--quoted=\"^!\""));
        assert!(!args.contains("--fake=^!"));
        assert!(FlowsealProvider::extract_strategy_arguments("echo no", "ALT").is_err());
    }

    #[test]
    fn migrates_cmd_escaped_catalog_arguments_exactly_once() {
        let mut catalog = StrategyCatalog {
            schema_version: CMD_ESCAPED_CATALOG_SCHEMA_VERSION,
            strategies: vec![StrategyCatalogEntry {
                name: "general (FAKE TLS AUTO)".to_string(),
                arguments: "--fake=^! --literal-caret=^^ --quoted=\"^!\"".to_string(),
                source: StrategySource::Flowseal,
            }],
        };

        assert!(FlowsealProvider::migrate_catalog(&mut catalog));
        assert_eq!(catalog.schema_version, STRATEGY_CATALOG_SCHEMA_VERSION);
        assert_eq!(
            catalog.strategies[0].arguments,
            "--fake=! --literal-caret=^ --quoted=\"^!\""
        );
        assert!(!FlowsealProvider::migrate_catalog(&mut catalog));
        assert_eq!(
            catalog.strategies[0].arguments,
            "--fake=! --literal-caret=^ --quoted=\"^!\""
        );
    }
}
