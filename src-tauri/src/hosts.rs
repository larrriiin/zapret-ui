use std::collections::HashSet;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::net::IpAddr;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const URL: &str = "https://raw.githubusercontent.com/Flowseal/zapret-discord-youtube/refs/heads/main/.service/hosts";
const MAX_BYTES: usize = 2 * 1024 * 1024;

#[derive(serde::Serialize)]
pub struct UpdateResult {
    added: usize,
    backup: Option<String>,
}

fn hostname(value: &str) -> Option<String> {
    let value = value.trim_end_matches('.');
    (!value.is_empty()
        && value.len() <= 253
        && value.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        }))
    .then(|| value.to_ascii_lowercase())
}

fn records(text: &str) -> Result<Vec<(IpAddr, Vec<String>)>, String> {
    let mut result = Vec::new();
    for line in text.trim_start_matches('\u{feff}').lines() {
        let mut fields = line.split('#').next().unwrap_or("").split_whitespace();
        let Some(ip) = fields.next() else { continue };
        let ip = ip.parse().map_err(|_| "invalid_source".to_string())?;
        let names: Vec<String> = fields
            .map(|s| hostname(s).ok_or("invalid_source".to_string()))
            .collect::<Result<_, _>>()?;
        if names.is_empty() {
            return Err("invalid_source".into());
        }
        result.push((ip, names));
    }
    if result.is_empty() {
        return Err("invalid_source".into());
    }
    Ok(result)
}

fn additions(existing: &[u8], source: &str) -> Result<(String, usize), String> {
    // Preserve original bytes, including legacy-encoded comments. UTF-16 is
    // rejected rather than appending incompatible ASCII bytes to it.
    if existing.contains(&0)
        || existing.starts_with(&[0xff, 0xfe])
        || existing.starts_with(&[0xfe, 0xff])
    {
        return Err("unsupported_encoding".into());
    }
    let mut seen = HashSet::new();
    for line in String::from_utf8_lossy(existing)
        .trim_start_matches('\u{feff}')
        .lines()
    {
        let mut fields = line.split('#').next().unwrap_or("").split_whitespace();
        if let Some(ip) = fields.next().and_then(|s| s.parse::<IpAddr>().ok()) {
            seen.extend(fields.filter_map(hostname).map(|name| (ip, name)));
        }
    }
    let mut output = String::new();
    let mut count = 0;
    for (ip, names) in records(source)? {
        for name in names {
            if seen.insert((ip, name.clone())) {
                output.push_str(&format!("{ip}\t{name}\r\n"));
                count += 1;
            }
        }
    }
    if count > 0 && !existing.is_empty() && !existing.ends_with(b"\n") {
        output.insert_str(0, "\r\n");
    }
    Ok((output, count))
}

fn update_file(path: &Path, source: &str) -> Result<UpdateResult, String> {
    let mut options = OpenOptions::new();
    options.read(true).write(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0); // Exclude concurrent writers throughout read/backup/append.
    }
    let mut file = options.open(path).map_err(|_| "open_failed".to_string())?;
    let mut original = Vec::new();
    (&mut file)
        .take((MAX_BYTES + 1) as u64)
        .read_to_end(&mut original)
        .map_err(|_| "read_failed")?;
    if original.len() > MAX_BYTES {
        return Err("file_too_large".into());
    }
    let (append, added) = additions(&original, source)?;
    if added == 0 {
        return Ok(UpdateResult {
            added,
            backup: None,
        });
    }
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "backup_failed")?
        .as_nanos();
    let backup = path.with_file_name(format!("hosts.zapret-{stamp}.bak"));
    let mut copy = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&backup)
        .map_err(|_| "backup_failed")?;
    copy.write_all(&original)
        .and_then(|_| copy.sync_all())
        .map_err(|_| "backup_failed")?;
    if file
        .write_all(append.as_bytes())
        .and_then(|_| file.sync_all())
        .is_err()
    {
        file.set_len(original.len() as u64)
            .and_then(|_| file.sync_all())
            .map_err(|_| "restore_failed")?;
        return Err("write_failed".into());
    }
    Ok(UpdateResult {
        added,
        backup: Some(backup.to_string_lossy().into_owned()),
    })
}

#[tauri::command]
pub async fn update_hosts() -> Result<UpdateResult, String> {
    if !crate::is_admin() {
        return Err("admin_required".into());
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|_| "download_failed")?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "download_failed")?
        .as_nanos();
    let mut response = client
        .get(format!("{URL}?t={stamp}"))
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|_| "download_failed")?;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| "download_failed")? {
        if bytes.len() + chunk.len() > MAX_BYTES {
            return Err("invalid_source".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    let source = String::from_utf8(bytes).map_err(|_| "invalid_source")?;
    records(&source)?;
    let root = std::env::var_os("SystemRoot").ok_or("open_failed")?;
    let path = std::path::PathBuf::from(root).join("System32/drivers/etc/hosts");
    tauri::async_runtime::spawn_blocking(move || update_file(&path, &source))
        .await
        .map_err(|_| "write_failed".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deduplicates_pairs_preserves_multiple_ips_and_ignores_comments() {
        let old = b"# 1.1.1.1 new.test\r\n0.0.0.0 EXAMPLE.test alias.test. # custom";
        let (extra, count) = additions(
            old,
            "1.2.3.4 example.test alias.test new.test\n1.2.3.4 NEW.TEST",
        )
        .unwrap();
        assert_eq!(count, 3);
        assert_eq!(
            extra,
            "\r\n1.2.3.4\texample.test\r\n1.2.3.4\talias.test\r\n1.2.3.4\tnew.test\r\n"
        );
        let merged = [old.as_slice(), extra.as_bytes()].concat();
        assert_eq!(
            additions(&merged, "1.2.3.4 example.test alias.test new.test")
                .unwrap()
                .1,
            0
        );
    }

    #[test]
    fn validates_whole_download_before_modifying() {
        for source in [
            "",
            "# empty",
            "<html>error</html>",
            "1.2.3.4 good.test\ninvalid",
            "1.2.3.4",
        ] {
            assert!(additions(b"", source).is_err());
        }
        assert!(additions(&[0xff, 0xfe, 0, 1], "::1 test.local").is_err());
        assert_eq!(
            additions(b"# legacy \xff\n", "\u{feff}::1 test.local")
                .unwrap()
                .1,
            1
        );
    }

    #[test]
    fn backs_up_exact_bytes_and_repeated_update_is_noop() {
        let dir = std::env::temp_dir().join(format!(
            "zapret-hosts-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("hosts");
        let original = b"127.0.0.1 localhost\r\n# custom \xff";
        std::fs::write(&path, original).unwrap();
        let result = update_file(&path, "1.2.3.4 new.test").unwrap();
        assert_eq!(std::fs::read(result.backup.unwrap()).unwrap(), original);
        let first = std::fs::read(&path).unwrap();
        assert!(first.starts_with(original));
        let second = update_file(&path, "1.2.3.4 new.test").unwrap();
        assert_eq!(second.added, 0);
        assert!(second.backup.is_none());
        assert_eq!(std::fs::read(&path).unwrap(), first);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
