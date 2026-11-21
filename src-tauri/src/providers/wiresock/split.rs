use super::{cli, generated_directories, generated_path, imported, locate, valid_profile_name};
use crate::providers::warp::profile::{prepare_profile_file, profile_directory, write_profile};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, io::Read, path::Path};
use zeroize::Zeroizing;

pub(crate) mod global;
pub(in crate::providers) mod sites;

const READ_ERROR: &str = "wiresock_split_read_error";
const WRITE_ERROR: &str = "wiresock_split_write_error";
const INVALID_APPS: &str = "warp_profile_invalid_applications";
const MAX_PROFILE_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct SplitApplications {
    allowed: Vec<String>,
    disallowed: Vec<String>,
}

impl SplitApplications {
    pub(in crate::providers) fn normalize(&mut self) -> Result<(), &'static str> {
        for list in [&mut self.allowed, &mut self.disallowed] {
            let mut seen = HashSet::new();
            let mut entries = Vec::new();
            for entry in list
                .iter()
                .map(|entry| entry.trim())
                .filter(|entry| !entry.is_empty())
            {
                if entry.contains(['#', ';', '*', '?', ',']) || entry.chars().any(char::is_control)
                {
                    return Err(INVALID_APPS);
                }
                if seen.insert(entry.to_lowercase()) {
                    entries.push(entry.to_owned());
                }
            }
            *list = entries;
        }
        Ok(())
    }

    pub(in crate::providers) fn directives(&self, newline: &str) -> String {
        let mut output = String::new();
        for (key, value) in [
            ("AllowedApps", &self.allowed),
            ("DisallowedApps", &self.disallowed),
        ] {
            if !value.is_empty() {
                output.push_str(&format!("#@ws:{key} = {}{newline}", value.join(", ")));
            }
        }
        output
    }
}

fn section(line: &str) -> Option<&str> {
    let end = line.strip_prefix('[')?.split_once(']')?.0;
    Some(end.trim())
}

fn application_directive(line: &str) -> Option<(bool, &str)> {
    let line = match line.get(..5) {
        Some(prefix) if prefix.eq_ignore_ascii_case("#@ws:") => &line[5..],
        _ => line,
    };
    let (key, value) = line.split_once('=')?;
    let key = key.trim();
    let value = value.split(['#', ';']).next()?.trim();
    if key.eq_ignore_ascii_case("AllowedApps") {
        Some((true, value))
    } else if key.eq_ignore_ascii_case("DisallowedApps") {
        Some((false, value))
    } else {
        None
    }
}

fn peer_applications(profile: &str) -> Result<SplitApplications, &'static str> {
    let mut common = None;
    let mut peer = None;
    for line in profile
        .trim_start_matches('\u{feff}')
        .lines()
        .map(str::trim)
    {
        if let Some(section) = section(line) {
            merge_peer_applications(&mut common, peer.take())?;
            peer = section
                .eq_ignore_ascii_case("Peer")
                .then(SplitApplications::default);
        } else if let (Some(peer), Some((allowed, value))) =
            (&mut peer, application_directive(line))
        {
            let list = if allowed {
                &mut peer.allowed
            } else {
                &mut peer.disallowed
            };
            *list = value.split(',').map(str::to_owned).collect();
        }
    }
    merge_peer_applications(&mut common, peer)?;
    common.ok_or(READ_ERROR)
}

fn merge_peer_applications(
    common: &mut Option<SplitApplications>,
    peer: Option<SplitApplications>,
) -> Result<(), &'static str> {
    if let Some(mut peer) = peer {
        peer.normalize()?;
        match common {
            Some(previous) if *previous != peer => return Err("wiresock_split_different_peers"),
            None => *common = Some(peer),
            _ => {}
        }
    }
    Ok(())
}

fn with_applications(profile: &str, applications: &SplitApplications) -> Zeroizing<String> {
    let newline = if profile.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let directives = applications.directives(newline);
    let mut result = Zeroizing::new(String::with_capacity(profile.len() + directives.len()));
    let mut peer = false;
    for line in profile.split_inclusive('\n') {
        let directive = line.trim_start_matches('\u{feff}').trim();
        if let Some(name) = section(directive) {
            peer = name.eq_ignore_ascii_case("Peer");
            result.push_str(line);
            if peer && !directives.is_empty() {
                if !line.ends_with('\n') {
                    result.push_str(newline);
                }
                result.push_str(&directives);
            }
        } else if !peer || application_directive(directive).is_none() {
            result.push_str(line);
        }
    }
    result
}

async fn read_profile(
    app: &tauri::AppHandle,
    profile: &str,
) -> Result<Zeroizing<String>, &'static str> {
    if !valid_profile_name(profile) {
        return Err("wiresock_profile_missing");
    }
    // WireSock can discard comments on import/export. Keep the managed file for
    // site profiles so their source domains and original routes remain editable.
    let managed = match generated_path(generated_directories(app), profile) {
        Some((_, file)) => {
            let config = read_config(&file)?;
            if sites::saved_sites(&config).is_some_and(|sites| !sites.is_empty()) {
                return Ok(config);
            }
            Some(config)
        }
        None => None,
    };
    let path = locate().ok_or("wiresock_not_installed")?;
    let name = imported(&path)
        .await?
        .into_iter()
        .find(|name| name.eq_ignore_ascii_case(profile));
    let Some(name) = name else {
        return managed.ok_or("wiresock_profile_missing");
    };
    let temporary = tempfile::Builder::new()
        .prefix("wiresock-export-")
        .tempdir()
        .map_err(|_| READ_ERROR)?;
    let file = temporary.path().join("profile.conf");
    cli(
        &path,
        vec!["export".into(), name.into(), file.clone().into_os_string()],
        12,
    )
    .await
    .map_err(|_| READ_ERROR)?;
    read_config(&file)
}

fn read_config(file: &Path) -> Result<Zeroizing<String>, &'static str> {
    let mut content = Zeroizing::new(String::new());
    let length = std::fs::File::open(file)
        .and_then(|file| {
            file.take(MAX_PROFILE_BYTES + 1)
                .read_to_string(&mut content)
        })
        .map_err(|_| READ_ERROR)?;
    if length as u64 > MAX_PROFILE_BYTES {
        return Err(READ_ERROR);
    }
    Ok(content)
}

#[derive(Serialize)]
pub struct SplitRules {
    applications: SplitApplications,
    sites: Option<Vec<String>>,
}

#[tauri::command]
pub async fn get_wiresock_split_rules(
    app: tauri::AppHandle,
    profile: String,
) -> Result<SplitRules, &'static str> {
    let _switch = super::SWITCH.lock().await;
    let content = read_profile(&app, &profile).await?;
    Ok(SplitRules {
        applications: peer_applications(&content)?,
        sites: sites::saved_sites(&content),
    })
}

#[tauri::command]
pub async fn save_wiresock_split_profile(
    app: tauri::AppHandle,
    profile: String,
    mut applications: SplitApplications,
    sites: Vec<String>,
) -> Result<String, &'static str> {
    applications.normalize()?;
    let sites = sites::SiteRules::resolve(&sites).await?;
    let _switch = super::SWITCH.lock().await;
    let current = read_profile(&app, &profile).await?;
    peer_applications(&current)?;
    let mut updated = with_applications(&current, &applications);
    sites.apply(&mut updated)?;
    let directory = profile_directory(&app).map_err(|_| WRITE_ERROR)?;
    let name = format!("WireSock-Split-{:016X}", rand::random::<u64>());
    let output = prepare_profile_file(&directory).map_err(|_| WRITE_ERROR)?;
    write_profile(output, &directory.join(format!("{name}.conf")), &updated)
        .map_err(|_| WRITE_ERROR)?;
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_app_rules_and_preserves_other_profile_lines() {
        let original = concat!(
            "\u{feff}[Interface]\r\nPrivateKey = test\r\n",
            "[Peer]\r\nAllowedApps = chrome, firefox # browser rules\r\n",
            "#@ws:DisallowedApps = steam\r\n#@ws:DisallowedIPs = 192.168.0.0/16\r\n",
            "AllowedIPs = 192.0.2.0/24\r\nEndpoint = example.com:2408",
        );
        let mut applications = peer_applications(original).unwrap();
        assert_eq!(applications.allowed, ["chrome", "firefox"]);
        assert_eq!(applications.disallowed, ["steam"]);
        applications.allowed = vec![
            " discord.exe ".into(),
            "C:\\Program Files\\Game".into(),
            "Discord.exe".into(),
        ];
        applications.disallowed.clear();
        applications.normalize().unwrap();
        let copy = with_applications(original, &applications);
        assert_eq!(
            copy.as_str(),
            concat!(
                "\u{feff}[Interface]\r\nPrivateKey = test\r\n[Peer]\r\n",
                "#@ws:AllowedApps = discord.exe, C:\\Program Files\\Game\r\n",
                "#@ws:DisallowedIPs = 192.168.0.0/16\r\n",
                "AllowedIPs = 192.0.2.0/24\r\nEndpoint = example.com:2408",
            ),
        );
        assert!(peer_applications(&copy).unwrap() == applications);
        assert!(
            peer_applications(&with_applications(&copy, &SplitApplications::default()))
                .is_ok_and(|rules| rules == SplitApplications::default())
        );
    }

    #[test]
    fn protects_different_peer_rules_and_rejects_unsupported_patterns() {
        assert!(matches!(
            peer_applications("[Peer]\nAllowedApps = chrome\n[Peer]\nAllowedApps = firefox"),
            Err("wiresock_split_different_peers")
        ));
        let mut apps = SplitApplications {
            allowed: vec!["C:\\Games\\*.exe".into()],
            ..Default::default()
        };
        assert_eq!(apps.normalize(), Err(INVALID_APPS));
    }
}
