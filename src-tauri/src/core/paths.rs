use std::path::{Path, PathBuf};

/// Paths shared by the application and the installed core provider.
#[derive(Clone, Debug)]
pub struct CorePaths {
    root: PathBuf,
}

impl CorePaths {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn bin_dir(&self) -> PathBuf {
        self.root.join("bin")
    }
    pub fn lists_dir(&self) -> PathBuf {
        self.root.join("lists")
    }
    pub fn utils_dir(&self) -> PathBuf {
        self.root.join("utils")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_standard_core_directories() {
        let paths = CorePaths::new("core");
        assert_eq!(paths.bin_dir(), PathBuf::from("core/bin"));
        assert_eq!(paths.lists_dir(), PathBuf::from("core/lists"));
        assert_eq!(paths.utils_dir(), PathBuf::from("core/utils"));
    }
}
