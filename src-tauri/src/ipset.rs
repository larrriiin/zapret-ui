use std::collections::BTreeSet;
use std::path::Path;

#[derive(serde::Serialize)]
pub struct UpdateResult {
    pub changed: bool,
    pub count: usize,
}

fn entries(content: &str) -> BTreeSet<&str> {
    content
        .lines()
        .map(|line| line.trim().trim_start_matches('\u{feff}'))
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect()
}

/// Called only after validation of the entire downloaded list.
pub fn install_if_changed(path: &Path, content: &str) -> Result<UpdateResult, String> {
    let incoming = entries(content);
    let changed = match std::fs::read_to_string(path) {
        Ok(current) => entries(&current) != incoming,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
        Err(error) => return Err(format!("Failed to read current IPSet list: {error}")),
    };
    if changed {
        std::fs::write(path, content)
            .map_err(|error| format!("Failed to install downloaded IPSet list: {error}"))?;
    }
    Ok(UpdateResult {
        changed,
        count: incoming.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_entries_ignoring_order_comments_and_formatting() {
        assert_eq!(
            entries("\u{feff}# comment\r\n1.2.3.4\r\n 10.0.0.0/8 \r\n"),
            entries("10.0.0.0/8\n1.2.3.4\n1.2.3.4")
        );
        assert_ne!(entries("1.2.3.4"), entries("1.2.3.4\n::1"));
        assert_ne!(entries("1.2.3.4\n::1"), entries("1.2.3.4"));
        assert_ne!(entries("1.2.3.4"), entries("1.2.3.5"));
    }

    #[test]
    fn unchanged_preserves_file_and_changes_install() {
        let path = std::env::temp_dir().join(format!(
            "zapret-ipset-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let original = "# preserved\r\n1.2.3.4\r\n::1\r\n";
        std::fs::write(&path, original).unwrap();
        let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
        assert!(!install_if_changed(&path, "::1\n1.2.3.4").unwrap().changed);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        assert_eq!(
            std::fs::metadata(&path).unwrap().modified().unwrap(),
            modified
        );
        for content in ["1.2.3.4\n::1\n10.0.0.0/8", "1.2.3.4\n10.0.0.0/8"] {
            assert!(install_if_changed(&path, content).unwrap().changed);
            assert_eq!(std::fs::read_to_string(&path).unwrap(), content);
        }
        std::fs::remove_file(&path).unwrap();
        assert!(install_if_changed(&path, "::1").unwrap().changed);
        std::fs::remove_file(path).unwrap();
    }
}
