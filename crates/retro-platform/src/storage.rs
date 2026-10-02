//! Filesystem-backed user storage, available with or without the SDL3 feature.
//!
//! The engine's save data lives under a single root directory (the SDL preferred path on
//! desktop, `--user-dir` for headless runs or an explicit override in tests). This module is the
//! only place besides the SDL backend that touches `std::fs`.

use std::path::{Path, PathBuf};

use crate::{PlatformError, Storage};

/// Filesystem-backed user storage rooted at a base directory.
#[derive(Debug)]
pub struct FsStorage {
    root: PathBuf,
}

impl FsStorage {
    /// Creates storage rooted at `root`.
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The storage root.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }
}

impl Storage for FsStorage {
    fn read(&self, path: &str) -> Result<Vec<u8>, PlatformError> {
        Ok(std::fs::read(self.root.join(path))?)
    }

    fn write(&mut self, path: &str, data: &[u8]) -> Result<(), PlatformError> {
        let full = self.root.join(path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(full, data)?;
        Ok(())
    }

    fn exists(&self, path: &str) -> bool {
        self.root.join(path).exists()
    }

    fn list(&self, dir: &str) -> Result<Vec<String>, PlatformError> {
        let full = self.root.join(dir);
        if !full.exists() {
            return Ok(Vec::new());
        }
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(full)? {
            entries.push(entry?.file_name().to_string_lossy().into_owned());
        }
        entries.sort();
        Ok(entries)
    }

    fn remove(&mut self, path: &str) -> Result<(), PlatformError> {
        match std::fs::remove_file(self.root.join(path)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    fn rename(&mut self, from: &str, to: &str) -> Result<(), PlatformError> {
        let destination = self.root.join(to);
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::rename(self.root.join(from), destination)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fs_storage_round_trips() {
        let root = std::env::temp_dir().join(format!("retro-platform-test-{}", std::process::id()));
        let mut storage = FsStorage::new(&root);
        storage.write("nested/save.dat", b"data").unwrap();
        assert!(storage.exists("nested/save.dat"));
        assert_eq!(storage.read("nested/save.dat").unwrap(), b"data");
        assert_eq!(storage.list("nested").unwrap(), vec!["save.dat"]);
        storage.write("nested/save.tmp", b"pending").unwrap();
        storage
            .rename("nested/save.tmp", "nested/save.dat")
            .unwrap();
        assert!(!storage.exists("nested/save.tmp"));
        assert_eq!(storage.read("nested/save.dat").unwrap(), b"pending");
        assert!(
            storage
                .rename("nested/missing.tmp", "nested/save.dat")
                .is_err()
        );
        storage.remove("nested/save.dat").unwrap();
        assert!(!storage.exists("nested/save.dat"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
