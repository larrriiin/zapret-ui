use super::{read_profile, sites, with_applications, SplitApplications};
use crate::providers::wiresock::{
    cli, import_file, imported, valid_profile_name, WireSockStatus, SWITCH,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{
    io::Write,
    path::{Path, PathBuf},
};
use tauri::Manager;

const PREFERENCES: &str = "wiresock-global-rules.json";
const SPLIT_TUNNELING: &str = "wiresock-split-tunneling.json";
const SPLIT_ERROR: &str = "wiresock_split_preference_error";
const ACTIVE: &str = "wiresock-global-profile.json";
const ERROR: &str = "wiresock_global_error";
const PREFIX: &str = "ZAPRET-UI-Global-";

#[derive(Default, Deserialize, Serialize)]
pub struct GlobalRules {
    pub(in crate::providers::wiresock) enabled: bool,
    applications: SplitApplications,
    sites: Vec<String>,
}

#[derive(Deserialize, Serialize)]
struct ActiveProfile {
    name: String,
    source: String,
}

pub(in crate::providers::wiresock) fn is_runtime_profile(name: &str) -> bool {
    name.get(..PREFIX.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(PREFIX))
        && name[PREFIX.len()..].len() == 16
        && name[PREFIX.len()..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
}

fn path(app: &tauri::AppHandle, file: &str) -> Result<PathBuf, &'static str> {
    app.path()
        .app_local_data_dir()
        .map(|dir| dir.join(file))
        .map_err(|_| ERROR)
}

fn read<T: DeserializeOwned>(path: &Path) -> Result<Option<T>, &'static str> {
    match std::fs::read(path) {
        Ok(data) => serde_json::from_slice(&data).map(Some).map_err(|_| ERROR),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(ERROR),
    }
}

fn save(path: &Path, value: &impl Serialize) -> Result<(), &'static str> {
    let dir = path.parent().ok_or(ERROR)?;
    std::fs::create_dir_all(dir).map_err(|_| ERROR)?;
    let mut output = tempfile::NamedTempFile::new_in(dir).map_err(|_| ERROR)?;
    serde_json::to_writer(&mut output, value).map_err(|_| ERROR)?;
    output.as_file().sync_all().map_err(|_| ERROR)?;
    output.persist(path).map_err(|_| ERROR)?;
    Ok(())
}

#[tauri::command]
pub fn get_wiresock_global_rules(app: tauri::AppHandle) -> Result<GlobalRules, &'static str> {
    Ok(read(&path(&app, PREFERENCES)?)?.unwrap_or_default())
}

pub(in crate::providers::wiresock) fn split_tunneling_enabled(
    app: &tauri::AppHandle,
) -> Result<bool, &'static str> {
    path(app, SPLIT_TUNNELING)
        .and_then(|file| read(&file))
        .map(|value| value.unwrap_or(true))
        .map_err(|_| SPLIT_ERROR)
}

#[tauri::command]
pub async fn set_wiresock_split_tunneling(
    app: tauri::AppHandle,
    enabled: bool,
) -> Result<(), &'static str> {
    let _switch = SWITCH.lock().await;
    path(&app, SPLIT_TUNNELING)
        .and_then(|file| save(&file, &enabled))
        .map_err(|_| SPLIT_ERROR)
}

#[tauri::command]
pub async fn save_wiresock_global_rules(
    app: tauri::AppHandle,
    mut rules: GlobalRules,
) -> Result<(), &'static str> {
    rules.applications.normalize()?;
    rules.sites = crate::providers::warp::sites::normalize_domains(rules.sites)
        .map_err(|_| "warp_sites_invalid")?;
    let _switch = SWITCH.lock().await;
    save(&path(&app, PREFERENCES)?, &rules)
}

pub(in crate::providers::wiresock) fn describe(
    app: &tauri::AppHandle,
    status: &mut WireSockStatus,
) {
    let Ok(Some(active)) = path(app, ACTIVE).and_then(|file| read::<ActiveProfile>(&file)) else {
        return;
    };
    if status
        .profile
        .as_ref()
        .is_some_and(|name| name.eq_ignore_ascii_case(&active.name))
    {
        status.profile = Some(active.source);
    }
}

pub(in crate::providers::wiresock) async fn cleanup(
    app: &tauri::AppHandle,
    cli_path: &Path,
) -> Result<(), &'static str> {
    let file = path(app, ACTIVE)?;
    let Some(active) = read::<ActiveProfile>(&file)? else {
        return Ok(());
    };
    if !is_runtime_profile(&active.name) {
        return Err(ERROR);
    }
    let exists = |names: Vec<String>| {
        names
            .iter()
            .any(|name| name.eq_ignore_ascii_case(&active.name))
    };
    if exists(imported(cli_path).await?) {
        let _ = cli(
            cli_path,
            vec!["delete".into(), active.name.as_str().into()],
            12,
        )
        .await;
        if exists(imported(cli_path).await?) {
            return Err(ERROR);
        }
    }
    std::fs::remove_file(file).map_err(|_| ERROR)
}

pub(in crate::providers::wiresock) async fn profile_for_connection(
    app: &tauri::AppHandle,
    cli_path: &Path,
    profile: &str,
    rules: Option<GlobalRules>,
) -> Result<String, &'static str> {
    if !valid_profile_name(profile) || is_runtime_profile(profile) {
        return Err(ERROR);
    }
    let source = read_profile(app, profile).await?;
    let config = if let Some(rules) = rules {
        let source = sites::reset_profile_routes(&source)?;
        let mut config = with_applications(&source, &rules.applications);
        sites::SiteRules::resolve(&rules.sites)
            .await?
            .apply(&mut config)?;
        config
    } else {
        full_tunnel_config(&source)?
    };
    let temporary = tempfile::tempdir().map_err(|_| ERROR)?;
    let name = format!("{PREFIX}{:016X}", rand::random::<u64>());
    let temp_file = temporary.path().join(format!("{name}.conf"));
    tempfile::NamedTempFile::new_in(temporary.path())
        .and_then(|mut output| {
            output.write_all(config.as_bytes())?;
            output.persist_noclobber(&temp_file)?;
            Ok(())
        })
        .map_err(|_| ERROR)?;
    save(
        &path(app, ACTIVE)?,
        &ActiveProfile {
            name,
            source: profile.to_owned(),
        },
    )?;
    import_file(cli_path, &temp_file).await
}

fn full_tunnel_config(source: &str) -> Result<zeroize::Zeroizing<String>, &'static str> {
    let source =
        sites::full_tunnel_routes(source).map_err(|_| "wiresock_split_full_profile_error")?;
    Ok(with_applications(&source, &SplitApplications::default()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_tunnel_ignores_all_split_rules_and_preserves_the_profile() {
        let profile = "[Interface]\nPrivateKey = key\n[Peer]\nPublicKey = peer\nEndpoint = example.com:2408\nAllowedIPs = 198.51.100.0/24, 2001:db8::/64\n#@ws:AllowedApps = Discord.exe\nDisallowedApps = game.exe\n#@WS:DisallowedIPs = 192.168.0.0/16\n";
        let full = full_tunnel_config(profile).unwrap();
        assert_eq!(full.as_str(), "[Interface]\nPrivateKey = key\n[Peer]\nPublicKey = peer\nEndpoint = example.com:2408\nAllowedIPs = 0.0.0.0/1, 128.0.0.0/1, ::/1, 8000::/1\n");
        assert_eq!(
            full_tunnel_config(&format!("{profile}[Peer]\nAllowedIPs = 192.0.2.0/24\n"))
                .unwrap_err(),
            "wiresock_split_full_profile_error"
        );
    }
}
