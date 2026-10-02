//! Asset folder discovery, path resolution and byte-level access for unpacked RSDK asset trees.
//!
//! RSDK paths are written with forward slashes and mixed casing (`Data/Game/GameConfig.bin`),
//! while the same tree may be stored on a case-sensitive filesystem with different casing (and,
//! on Windows, with backslashes). `DirSource` resolves each path segment exactly first and then
//! case-insensitively, mirroring the `fcaseopen` behaviour of the C++ engine. The optional
//! `MemorySource` backend performs no I/O and is intended for tests and tooling; OS access is
//! restricted to `DirSource` and to tests.

#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Errors returned by [`DataSource`] implementations.
#[derive(Debug, thiserror::Error)]
pub enum IoError {
    /// The requested path does not exist in the source.
    #[error("file not found: {0}")]
    NotFound(String),
    /// An underlying filesystem operation failed.
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    /// The path is absolute, escapes the source root or is otherwise unusable.
    #[error("invalid path: {0}")]
    InvalidPath(String),
    /// The source cannot provide the requested path.
    #[error("unsupported: {0}")]
    Unsupported(String),
}

/// Byte-level access to an unpacked RSDK asset tree.
pub trait DataSource {
    /// Reads the whole file at `path`.
    fn read(&self, path: &str) -> Result<Vec<u8>, IoError>;
    /// Returns whether a file exists at `path`.
    fn exists(&self, path: &str) -> bool;
    /// Returns the size in bytes of the file at `path`, if it exists.
    fn size(&self, path: &str) -> Option<u64>;
    /// Full paths of files directly under `dir` (relative to the source root), sorted, with the
    /// on-disk casing.
    fn enumerate(&self, dir: &str) -> Result<Vec<String>, IoError>;
}

/// Normalises an asset path to a `/`-separated, root-relative form.
///
/// Backslashes become forward slashes, leading slashes and empty or `.` components are dropped,
/// and `..` components or absolute paths (drive letters, UNC shares) are rejected.
fn normalize(path: &str) -> Result<String, IoError> {
    let path = path.replace('\\', "/");
    if path.starts_with("//") {
        return Err(IoError::InvalidPath(path));
    }
    let bytes = path.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        return Err(IoError::InvalidPath(path));
    }
    let mut normalized = String::with_capacity(path.len());
    for component in path.trim_start_matches('/').split('/') {
        if component.is_empty() || component == "." {
            continue;
        }
        if component == ".." {
            return Err(IoError::InvalidPath(path));
        }
        if !normalized.is_empty() {
            normalized.push('/');
        }
        normalized.push_str(component);
    }
    Ok(normalized)
}

/// [`DataSource`] backed by a directory on the local filesystem.
#[derive(Debug)]
pub struct DirSource {
    root: PathBuf,
}

impl DirSource {
    /// Opens `root` as an asset source. The directory must exist.
    pub fn new(root: impl AsRef<Path>) -> Result<Self, IoError> {
        let root = root.as_ref();
        let metadata = std::fs::metadata(root)?;
        if !metadata.is_dir() {
            return Err(IoError::InvalidPath(root.display().to_string()));
        }
        Ok(Self {
            root: root.to_path_buf(),
        })
    }

    /// Resolves a normalised path segment by segment, trying the exact spelling first and then an
    /// ASCII case-insensitive match against each directory entry.
    fn resolve(&self, normalized: &str) -> Result<Option<PathBuf>, IoError> {
        let mut current = self.root.clone();
        if normalized.is_empty() {
            return Ok(Some(current));
        }
        for segment in normalized.split('/') {
            let exact = current.join(segment);
            if exact.exists() {
                current = exact;
                continue;
            }
            let entries = match std::fs::read_dir(&current) {
                Ok(entries) => entries,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(error) => return Err(IoError::Io(error)),
            };
            let mut found = false;
            for entry in entries {
                let entry = entry?;
                if entry
                    .file_name()
                    .to_string_lossy()
                    .eq_ignore_ascii_case(segment)
                {
                    current = entry.path();
                    found = true;
                    break;
                }
            }
            if !found {
                return Ok(None);
            }
        }
        Ok(Some(current))
    }

    fn relative_path(&self, path: &Path) -> String {
        path.strip_prefix(&self.root)
            .unwrap_or(path)
            .components()
            .map(|component| component.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/")
    }
}

impl DataSource for DirSource {
    fn read(&self, path: &str) -> Result<Vec<u8>, IoError> {
        let path = normalize(path)?;
        if path.is_empty() {
            return Err(IoError::NotFound(path));
        }
        match self.resolve(&path)? {
            Some(resolved) => std::fs::read(&resolved).map_err(|error| {
                if error.kind() == std::io::ErrorKind::NotFound {
                    IoError::NotFound(path.clone())
                } else {
                    IoError::Io(error)
                }
            }),
            None => Err(IoError::NotFound(path)),
        }
    }

    fn exists(&self, path: &str) -> bool {
        let Ok(path) = normalize(path) else {
            return false;
        };
        if path.is_empty() {
            return false;
        }
        matches!(self.resolve(&path), Ok(Some(resolved)) if resolved.is_file())
    }

    fn size(&self, path: &str) -> Option<u64> {
        let path = normalize(path).ok()?;
        if path.is_empty() {
            return None;
        }
        let resolved = self.resolve(&path).ok()??;
        std::fs::metadata(resolved)
            .ok()
            .filter(|metadata| metadata.is_file())
            .map(|metadata| metadata.len())
    }

    fn enumerate(&self, dir: &str) -> Result<Vec<String>, IoError> {
        let dir = normalize(dir)?;
        let Some(resolved) = self.resolve(&dir)? else {
            return Err(IoError::NotFound(dir));
        };
        if !resolved.is_dir() {
            return Err(IoError::NotFound(dir));
        }
        let mut files = Vec::new();
        for entry in std::fs::read_dir(&resolved)? {
            let entry = entry?;
            if entry.file_type()?.is_file() {
                files.push(self.relative_path(&resolved.join(entry.file_name())));
            }
        }
        files.sort();
        Ok(files)
    }
}

/// In-memory [`DataSource`], useful for tests and tooling. Keys are normalised on insert and on
/// lookup and matched ASCII case-insensitively, while `enumerate` reports the casing used at
/// insert time. Entries inserted under an invalid path are unreachable through the trait methods.
#[derive(Debug, Default)]
pub struct MemorySource {
    files: HashMap<String, Vec<u8>>,
    lookup: HashMap<String, String>,
}

impl MemorySource {
    /// Creates an empty source.
    pub fn new() -> Self {
        Self::default()
    }

    /// Stores `data` under `path`.
    pub fn insert(&mut self, path: impl Into<String>, data: impl Into<Vec<u8>>) {
        let path = path.into();
        let key = normalize(&path).unwrap_or(path);
        let folded = key.to_ascii_lowercase();
        if let Some(previous) = self.lookup.insert(folded, key.clone())
            && previous != key
        {
            self.files.remove(&previous);
        }
        self.files.insert(key, data.into());
    }

    fn stored_key(&self, path: &str) -> Result<Option<&String>, IoError> {
        let key = normalize(path)?;
        Ok(self.lookup.get(&key.to_ascii_lowercase()))
    }
}

impl DataSource for MemorySource {
    fn read(&self, path: &str) -> Result<Vec<u8>, IoError> {
        match self.stored_key(path)? {
            Some(key) => Ok(self.files[key].clone()),
            None => Err(IoError::NotFound(normalize(path)?)),
        }
    }

    fn exists(&self, path: &str) -> bool {
        self.stored_key(path).is_ok_and(|key| key.is_some())
    }

    fn size(&self, path: &str) -> Option<u64> {
        let key = self.stored_key(path).ok()??;
        self.files.get(key).map(|data| data.len() as u64)
    }

    fn enumerate(&self, dir: &str) -> Result<Vec<String>, IoError> {
        let dir = normalize(dir)?;
        let prefix = if dir.is_empty() {
            String::new()
        } else {
            format!("{}/", dir.to_ascii_lowercase())
        };
        let mut files = Vec::new();
        for key in self.files.keys() {
            let folded = key.to_ascii_lowercase();
            let Some(rest) = folded.strip_prefix(&prefix) else {
                continue;
            };
            if rest.is_empty() || rest.contains('/') {
                continue;
            }
            files.push(key.clone());
        }
        files.sort();
        Ok(files)
    }
}

/// Unit tests. All filesystem access here is test-only and uses `tempfile` directories.
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    fn write_file(root: &Path, relative: &str, data: &[u8]) {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, data).unwrap();
    }

    fn temp_source() -> (tempfile::TempDir, DirSource) {
        let dir = tempfile::tempdir().unwrap();
        write_file(dir.path(), "Data/Game/GameConfig.bin", b"config-data");
        write_file(dir.path(), "Data/Sprites/Title/Title.gif", b"title");
        write_file(dir.path(), "Data/Sprites/Title/SonicTeam.gif", b"team");
        write_file(dir.path(), "Data/Sprites/Title/Sub/Nested.gif", b"nested");
        write_file(dir.path(), "Data/Stages/Zone01/16x16Tiles.gif", b"tiles");
        let source = DirSource::new(dir.path()).unwrap();
        (dir, source)
    }

    #[test]
    fn reads_exact_case() {
        let (_dir, source) = temp_source();
        assert_eq!(
            source.read("Data/Game/GameConfig.bin").unwrap(),
            b"config-data"
        );
    }

    #[test]
    fn reads_case_insensitively_with_mixed_case_on_disk() {
        let (_dir, source) = temp_source();
        assert_eq!(
            source.read("data/game/gameconfig.bin").unwrap(),
            b"config-data"
        );
        assert_eq!(
            source.read("DATA/GAME/GAMECONFIG.BIN").unwrap(),
            b"config-data"
        );
    }

    #[test]
    fn normalises_backslashes_and_leading_slash() {
        let (_dir, source) = temp_source();
        assert_eq!(
            source.read("data\\game\\gameconfig.bin").unwrap(),
            b"config-data"
        );
        assert_eq!(
            source.read("/Data/Game/GameConfig.bin").unwrap(),
            b"config-data"
        );
        assert_eq!(
            source.read("./Data//Game/./GameConfig.bin").unwrap(),
            b"config-data"
        );
    }

    #[test]
    fn rejects_parent_traversal() {
        let (_dir, source) = temp_source();
        assert!(matches!(
            source.read("../secret"),
            Err(IoError::InvalidPath(_))
        ));
        assert!(matches!(
            source.read("Data/../secret"),
            Err(IoError::InvalidPath(_))
        ));
        assert!(matches!(
            source.read("..\\secret"),
            Err(IoError::InvalidPath(_))
        ));
        assert!(!source.exists("../secret"));
        assert_eq!(source.size("../secret"), None);
        assert!(matches!(
            source.enumerate(".."),
            Err(IoError::InvalidPath(_))
        ));
    }

    #[test]
    fn rejects_absolute_paths() {
        let (_dir, source) = temp_source();
        assert!(matches!(
            source.read("C:\\Windows\\System32\\config"),
            Err(IoError::InvalidPath(_))
        ));
        assert!(matches!(
            source.read("C:/Windows/System32/config"),
            Err(IoError::InvalidPath(_))
        ));
        assert!(matches!(
            source.read("//server/share/file"),
            Err(IoError::InvalidPath(_))
        ));
    }

    #[test]
    fn missing_file_reports_not_found() {
        let (_dir, source) = temp_source();
        assert!(
            matches!(source.read("Data/Game/Missing.bin"), Err(IoError::NotFound(path)) if path == "Data/Game/Missing.bin")
        );
        assert!(!source.exists("Data/Game/Missing.bin"));
        assert_eq!(source.size("Data/Game/Missing.bin"), None);
    }

    #[test]
    fn reports_size() {
        let (_dir, source) = temp_source();
        assert_eq!(source.size("Data/Game/GameConfig.bin"), Some(11));
        assert_eq!(source.size("data/game/gameconfig.bin"), Some(11));
        assert!(source.exists("data/game/gameconfig.bin"));
    }

    #[test]
    fn enumerates_direct_children_sorted_with_disk_casing() {
        let (_dir, source) = temp_source();
        let files = source.enumerate("data/sprites/title").unwrap();
        assert_eq!(
            files,
            [
                "Data/Sprites/Title/SonicTeam.gif",
                "Data/Sprites/Title/Title.gif"
            ]
        );
    }

    #[test]
    fn enumerate_ignores_subdirectories() {
        let (_dir, source) = temp_source();
        let files = source.enumerate("Data/Sprites/Title").unwrap();
        assert!(!files.iter().any(|file| file.ends_with("Nested.gif")));
    }

    #[test]
    fn enumerate_missing_directory_reports_not_found() {
        let (_dir, source) = temp_source();
        assert!(matches!(
            source.enumerate("Data/Nope"),
            Err(IoError::NotFound(_))
        ));
    }

    #[test]
    fn dir_source_requires_existing_directory() {
        let dir = tempfile::tempdir().unwrap();
        assert!(DirSource::new(dir.path().join("does-not-exist")).is_err());
        let file = dir.path().join("file.bin");
        fs::write(&file, b"x").unwrap();
        assert!(matches!(
            DirSource::new(&file),
            Err(IoError::InvalidPath(_))
        ));
    }

    #[test]
    fn memory_source_round_trips() {
        let mut source = MemorySource::new();
        source.insert("Data/Game/GameConfig.bin", vec![1, 2, 3, 4]);
        source.insert("Data/Sprites/Title/Title.gif", vec![9]);
        source.insert("readme.txt", vec![0]);
        assert_eq!(
            source.read("data\\game\\gameconfig.bin").unwrap(),
            vec![1, 2, 3, 4]
        );
        assert!(source.exists("DATA/game/GAMECONFIG.BIN"));
        assert_eq!(source.size("Data/Game/GameConfig.bin"), Some(4));
        assert_eq!(
            source.enumerate("data/game").unwrap(),
            ["Data/Game/GameConfig.bin"]
        );
        assert_eq!(
            source.enumerate("Data/Sprites/Title").unwrap(),
            ["Data/Sprites/Title/Title.gif"]
        );
        assert!(
            matches!(source.read("Data/Missing.bin"), Err(IoError::NotFound(path)) if path == "Data/Missing.bin")
        );
        assert!(!source.exists("Data/Missing.bin"));
        assert_eq!(source.size("Data/Missing.bin"), None);
        assert_eq!(source.enumerate("").unwrap(), ["readme.txt"]);
    }

    #[test]
    fn memory_source_rejects_invalid_lookups() {
        let mut source = MemorySource::new();
        source.insert("../escape", vec![1]);
        assert!(matches!(
            source.read("../escape"),
            Err(IoError::InvalidPath(_))
        ));
        assert!(!source.exists("../escape"));
        assert_eq!(source.size("../escape"), None);
    }
}
