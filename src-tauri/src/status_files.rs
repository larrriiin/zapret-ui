use crate::MutexExt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

#[derive(PartialEq, Eq)]
pub(crate) struct FileStamp {
    pub(crate) path: PathBuf,
    len: u64,
    modified: SystemTime,
    created: Option<SystemTime>,
}

impl FileStamp {
    pub(crate) fn read(path: &Path) -> io::Result<Self> {
        let metadata = fs::metadata(path)?;
        if !metadata.is_file() {
            return Err(io::Error::from(io::ErrorKind::NotFound));
        }
        Ok(Self {
            path: path.to_path_buf(),
            len: metadata.len(),
            modified: metadata.modified()?,
            created: metadata.created().ok(),
        })
    }

    pub(crate) fn optional(path: &Path) -> io::Result<Option<Self>> {
        match Self::read(path) {
            Ok(stamp) => Ok(Some(stamp)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }
}

pub(crate) type Cache<T> = Mutex<Option<(Vec<FileStamp>, T)>>;

pub(crate) fn read<T: Clone>(
    cache: &Cache<T>,
    stamps: impl Fn() -> io::Result<Vec<FileStamp>>,
    load: impl FnOnce(&[FileStamp]) -> io::Result<T>,
) -> io::Result<T> {
    let mut cache = cache.lock_unpoisoned();
    let before = stamps()?;
    if let Some((saved, value)) = &*cache {
        if *saved == before {
            return Ok(value.clone());
        }
    }
    *cache = None;
    let value = load(&before)?;
    // A write during the read must not cache a result under stale metadata.
    if stamps().is_ok_and(|after| after == before) {
        *cache = Some((before, value.clone()));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn unchanged_files_reuse_results_and_external_changes_invalidate_them() -> io::Result<()> {
        let directory =
            std::env::temp_dir().join(format!("zapret-status-files-{}", std::process::id()));
        fs::create_dir(&directory)?;
        let path = directory.join("list.txt");
        let cache = Mutex::new(None);
        let loads = Cell::new(0);
        let read = || {
            super::read(
                &cache,
                || Ok(FileStamp::optional(&path)?.into_iter().collect()),
                |files| {
                    loads.set(loads.get() + 1);
                    files
                        .first()
                        .map_or_else(|| Ok(String::new()), |file| fs::read_to_string(&file.path))
                },
            )
        };
        fs::write(&path, "first")?;
        assert_eq!(read()?, "first");
        assert_eq!(read()?, "first");
        assert_eq!(loads.get(), 1);
        fs::write(&path, "updated content")?;
        assert_eq!(read()?, "updated content");
        fs::remove_file(&path)?;
        assert_eq!(read()?, "");
        assert_eq!(loads.get(), 3);
        fs::remove_dir(directory)
    }
}
