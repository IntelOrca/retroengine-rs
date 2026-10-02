//! Persistent v4 user data: save RAM (`SGame.bin`/`SData.bin`) and `Achievements.bin`.
//!
//! [`SaveStore`] owns a [`retro_platform::Storage`] backend, so desktop builds persist through
//! the platform's SDL3 filesystem storage while tests use the deterministic in-memory backend.
//! No library code touches `std::fs` directly.
//!
//! # Save RAM
//!
//! The canonical file is `SAVE_RAM_WORDS * 4 = 32768` bytes. [`SaveStore::load_save_ram`] reads
//! [`SaveRam::SAVE_PATH`] (`SGame.bin`) and falls back to [`SaveRam::MODERN_SAVE_PATH`]
//! (`SData.bin`), remembering which file was used so [`SaveStore::write_save_ram`] updates that
//! same file (defaulting to `SGame.bin` when nothing was loaded). When neither file exists it
//! leaves a full zeroed RAM and reports `Ok(false)`, mirroring upstream `ReadSaveRAMData`.
//!
//! Upstream RSDKv4-Decompilation reads `SData.bin` first; this port is scoped to the v4 asset
//! trees, which ship `SGame.bin`, and the M5 contract defines `SGame.bin` as the primary file.
//! Both layouts round-trip byte-exactly because the loaded [`SaveFileKind`] is preserved.
//!
//! # Achievements
//!
//! `Achievements.bin` is a table of little-endian `i32` statuses ([`ACHIEVEMENT_FILE_SLOTS`]
//! slots in the shipped files). A missing file yields a zeroed table and `Ok(false)`. Names are
//! engine-side metadata and never affect the bytes written.
//!
//! # Atomic writes
//!
//! Every write goes to `<path>.tmp` and is then [`Storage::rename`]d over the target, so a
//! failed write cannot truncate or corrupt the previous user data.

use retro_format_v4::FormatError;
use retro_format_v4::userdata::{
    ACHIEVEMENT_FILE_SLOTS, Achievement, Achievements, SaveFileKind, SaveRam,
};
use retro_platform::{PlatformError, Storage};

/// Suffix of the temporary file used to make writes atomic.
pub const TEMP_SUFFIX: &str = ".tmp";

/// Errors raised while loading or persisting user data.
#[derive(Debug, thiserror::Error)]
pub enum SaveError {
    /// The storage backend failed.
    #[error("user data storage error: {0}")]
    Storage(#[from] PlatformError),
    /// An existing user data file could not be parsed.
    #[error("user data format error: {0}")]
    Format(#[from] FormatError),
}

/// Persistent user data backed by a [`Storage`].
///
/// The in-memory state is always valid: [`SaveStore::open`] starts with a zeroed save RAM and a
/// zeroed achievement table, and the `load_*` methods replace them only after the file has been
/// parsed successfully.
pub struct SaveStore<S: Storage> {
    storage: S,
    save_ram: SaveRam,
    save_kind: SaveFileKind,
    save_persisted: Vec<u8>,
    achievements: Achievements,
    achievements_persisted: Vec<u8>,
}

impl<S: Storage> SaveStore<S> {
    /// Opens user data on `storage`.
    ///
    /// The backend is probed with [`Storage::list`] so unusable storage fails here instead of at
    /// the first read or write. No user data file is read yet; call [`SaveStore::load_save_ram`]
    /// and [`SaveStore::load_achievements`].
    pub fn open(storage: S) -> Result<Self, SaveError> {
        storage.list("")?;
        let save_ram = SaveRam::zeroed();
        let achievements = zeroed_achievements();
        let save_persisted = save_ram.to_bytes();
        let achievements_persisted = achievements.to_bytes();
        Ok(Self {
            storage,
            save_ram,
            save_kind: SaveFileKind::SGame,
            save_persisted,
            achievements,
            achievements_persisted,
        })
    }

    /// Loads [`SaveRam::SAVE_PATH`] (falling back to [`SaveRam::MODERN_SAVE_PATH`]) if present.
    ///
    /// Returns `Ok(false)` and leaves a zeroed RAM when neither file exists. A file that exists
    /// but is malformed (length not a multiple of four, or more than
    /// [`SAVE_RAM_WORDS`](retro_format_v4::userdata::SAVE_RAM_WORDS) words)
    /// returns [`SaveError::Format`] and leaves the previous in-memory state untouched. Short
    /// files are accepted exactly like the engine's unchecked `fRead`.
    pub fn load_save_ram(&mut self) -> Result<bool, SaveError> {
        let (bytes, kind) = if self.storage.exists(SaveRam::SAVE_PATH) {
            (self.storage.read(SaveRam::SAVE_PATH)?, SaveFileKind::SGame)
        } else if self.storage.exists(SaveRam::MODERN_SAVE_PATH) {
            (
                self.storage.read(SaveRam::MODERN_SAVE_PATH)?,
                SaveFileKind::SData,
            )
        } else {
            self.save_ram = SaveRam::zeroed();
            self.save_kind = SaveFileKind::SGame;
            self.save_persisted = self.save_ram.to_bytes();
            return Ok(false);
        };
        let ram = SaveRam::from_bytes(&bytes)?;
        self.save_ram = ram;
        self.save_kind = kind;
        self.save_persisted = bytes;
        Ok(true)
    }

    /// The in-memory save RAM (zeroed until a load succeeds).
    #[must_use]
    pub fn save_ram(&self) -> &SaveRam {
        &self.save_ram
    }

    /// Mutable access to the in-memory save RAM; changes show up in [`SaveStore::dirty`].
    pub fn save_ram_mut(&mut self) -> &mut SaveRam {
        &mut self.save_ram
    }

    /// Which file the current save RAM was loaded from; the target of [`SaveStore::write_save_ram`].
    #[must_use]
    pub fn save_file_kind(&self) -> SaveFileKind {
        self.save_kind
    }

    /// Writes the save RAM back to the file it was loaded from (`SGame.bin` when nothing was
    /// loaded).
    ///
    /// The bytes are first written to `<path>.tmp` and then renamed over the target. On failure
    /// the temporary file is removed best-effort and the previous user data is left untouched.
    pub fn write_save_ram(&mut self) -> Result<(), SaveError> {
        let path = match self.save_kind {
            SaveFileKind::SGame => SaveRam::SAVE_PATH,
            SaveFileKind::SData => SaveRam::MODERN_SAVE_PATH,
        };
        let bytes = self.save_ram.to_bytes();
        self.write_atomic(path, &bytes)?;
        self.save_persisted = bytes;
        Ok(())
    }

    /// Loads [`Achievements::PATH`] if present.
    ///
    /// Returns `Ok(false)` and leaves a zeroed table of [`ACHIEVEMENT_FILE_SLOTS`] slots when the
    /// file is absent. Malformed files return [`SaveError::Format`] and leave the previous state
    /// untouched.
    pub fn load_achievements(&mut self) -> Result<bool, SaveError> {
        if !self.storage.exists(Achievements::PATH) {
            self.achievements = zeroed_achievements();
            self.achievements_persisted = self.achievements.to_bytes();
            return Ok(false);
        }
        let bytes = self.storage.read(Achievements::PATH)?;
        let achievements = Achievements::from_bytes(&bytes)?;
        self.achievements = achievements;
        self.achievements_persisted = bytes;
        Ok(true)
    }

    /// The in-memory achievement table (zeroed until a load succeeds).
    #[must_use]
    pub fn achievements(&self) -> &Achievements {
        &self.achievements
    }

    /// Mutable access to the achievement table; changes show up in [`SaveStore::dirty`].
    pub fn achievements_mut(&mut self) -> &mut Achievements {
        &mut self.achievements
    }

    /// Sets one achievement status. Returns `false` when `id` has no slot.
    pub fn set_achievement_status(&mut self, id: u32, status: i32) -> bool {
        match self
            .achievements
            .entries
            .get_mut(id as usize)
            .filter(|entry| entry.id == id)
        {
            Some(entry) => {
                entry.status = status;
                true
            }
            None => false,
        }
    }

    /// Writes the achievement table to [`Achievements::PATH`] using the same temp-file-plus-rename
    /// scheme as [`SaveStore::write_save_ram`].
    pub fn write_achievements(&mut self) -> Result<(), SaveError> {
        let bytes = self.achievements.to_bytes();
        self.write_atomic(Achievements::PATH, &bytes)?;
        self.achievements_persisted = bytes;
        Ok(())
    }

    /// Whether the in-memory save RAM or achievement table differs from the last loaded or
    /// written bytes.
    #[must_use]
    pub fn dirty(&self) -> bool {
        self.save_ram.to_bytes() != self.save_persisted
            || self.achievements.to_bytes() != self.achievements_persisted
    }

    fn write_atomic(&mut self, path: &str, bytes: &[u8]) -> Result<(), SaveError> {
        let temp = format!("{path}{TEMP_SUFFIX}");
        self.storage.write(&temp, bytes)?;
        if let Err(error) = self.storage.rename(&temp, path) {
            let _ = self.storage.remove(&temp);
            return Err(error.into());
        }
        Ok(())
    }
}

fn zeroed_achievements() -> Achievements {
    Achievements {
        entries: (0..ACHIEVEMENT_FILE_SLOTS as u32)
            .map(|id| Achievement {
                id,
                status: 0,
                name: None,
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::path::PathBuf;
    use std::rc::Rc;

    use super::*;
    use retro_io::{DataSource, DirSource};
    use retro_platform::headless::MemoryStorage;

    /// A clonable handle to one [`MemoryStorage`], so a test can keep inspecting the backend
    /// after handing it to a [`SaveStore`].
    #[derive(Clone, Default)]
    struct SharedStorage(Rc<RefCell<MemoryStorage>>);

    impl SharedStorage {
        fn new() -> Self {
            Self::default()
        }
    }

    impl Storage for SharedStorage {
        fn read(&self, path: &str) -> Result<Vec<u8>, PlatformError> {
            self.0.borrow().read(path)
        }

        fn write(&mut self, path: &str, data: &[u8]) -> Result<(), PlatformError> {
            self.0.borrow_mut().write(path, data)
        }

        fn exists(&self, path: &str) -> bool {
            self.0.borrow().exists(path)
        }

        fn list(&self, dir: &str) -> Result<Vec<String>, PlatformError> {
            self.0.borrow().list(dir)
        }

        fn remove(&mut self, path: &str) -> Result<(), PlatformError> {
            self.0.borrow_mut().remove(path)
        }

        fn rename(&mut self, from: &str, to: &str) -> Result<(), PlatformError> {
            self.0.borrow_mut().rename(from, to)
        }
    }

    /// Storage that delegates to `MemoryStorage` but fails selected operations.
    struct FailingStorage {
        inner: MemoryStorage,
        fail_read: bool,
        fail_write: bool,
        fail_list: bool,
    }

    impl FailingStorage {
        fn new() -> Self {
            Self {
                inner: MemoryStorage::new(),
                fail_read: false,
                fail_write: false,
                fail_list: false,
            }
        }

        fn error() -> PlatformError {
            PlatformError::Other("injected storage failure".to_owned())
        }
    }

    impl Storage for FailingStorage {
        fn read(&self, path: &str) -> Result<Vec<u8>, PlatformError> {
            if self.fail_read {
                return Err(Self::error());
            }
            self.inner.read(path)
        }

        fn write(&mut self, path: &str, data: &[u8]) -> Result<(), PlatformError> {
            if self.fail_write {
                return Err(Self::error());
            }
            self.inner.write(path, data)
        }

        fn exists(&self, path: &str) -> bool {
            self.inner.exists(path)
        }

        fn list(&self, dir: &str) -> Result<Vec<String>, PlatformError> {
            if self.fail_list {
                return Err(Self::error());
            }
            self.inner.list(dir)
        }

        fn remove(&mut self, path: &str) -> Result<(), PlatformError> {
            self.inner.remove(path)
        }
    }

    fn pattern_bytes(words: usize) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(words * 4);
        for index in 0..words {
            let word = (index as i32).wrapping_mul(16_777_619);
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes
    }

    fn seeded_storage(path: &str, bytes: &[u8]) -> SharedStorage {
        let storage = SharedStorage::new();
        storage.0.borrow_mut().write(path, bytes).unwrap();
        storage
    }

    #[test]
    fn save_ram_round_trip_is_byte_exact_apart_from_the_mutation() {
        let bytes = pattern_bytes(0x2000);
        let storage = seeded_storage(SaveRam::SAVE_PATH, &bytes);
        let mut store = SaveStore::open(storage.clone()).unwrap();

        assert!(store.load_save_ram().unwrap());
        assert_eq!(store.save_file_kind(), SaveFileKind::SGame);
        assert_eq!(store.save_ram().words.len(), 0x2000);
        assert_eq!(store.save_ram().to_bytes(), bytes);
        assert!(!store.dirty());

        store.save_ram_mut().set_word(0x1234, 0x0BAD_F00D).unwrap();
        assert!(store.dirty());
        store.write_save_ram().unwrap();
        assert!(!store.dirty());

        let mut expected = bytes.clone();
        expected[0x1234 * 4..0x1234 * 4 + 4].copy_from_slice(&0x0BAD_F00Du32.to_le_bytes());
        assert_eq!(store.storage.read(SaveRam::SAVE_PATH).unwrap(), expected);
        assert!(
            !store
                .storage
                .exists(&format!("{}{TEMP_SUFFIX}", SaveRam::SAVE_PATH))
        );

        let mut reloaded = SaveStore::open(storage).unwrap();
        assert!(reloaded.load_save_ram().unwrap());
        assert_eq!(
            reloaded.save_ram().word(0x1234),
            Some(0x0BAD_F00D_u32 as i32)
        );
        assert_eq!(reloaded.save_ram().to_bytes(), expected);
        assert!(!reloaded.dirty());
    }

    #[test]
    fn missing_save_is_zeroed_and_not_dirty() {
        let storage = SharedStorage::new();
        let mut store = SaveStore::open(storage).unwrap();

        assert!(!store.load_save_ram().unwrap());
        assert_eq!(store.save_file_kind(), SaveFileKind::SGame);
        assert!(store.save_ram().is_full());
        assert!(store.save_ram().words.iter().all(|word| *word == 0));
        assert!(!store.dirty());

        assert!(!store.load_achievements().unwrap());
        assert_eq!(store.achievements().entries.len(), ACHIEVEMENT_FILE_SLOTS);
        assert!(
            store
                .achievements()
                .entries
                .iter()
                .enumerate()
                .all(|(id, entry)| entry.id == id as u32 && entry.status == 0)
        );
        assert!(!store.dirty());
    }

    #[test]
    fn save_ram_prefers_sgame_and_writes_it_back() {
        let game = pattern_bytes(0x2000);
        let mut data = game.clone();
        data[0..4].copy_from_slice(&7i32.to_le_bytes());
        let storage = seeded_storage(SaveRam::SAVE_PATH, &game);
        storage
            .0
            .borrow_mut()
            .write(SaveRam::MODERN_SAVE_PATH, &data)
            .unwrap();

        let mut store = SaveStore::open(storage.clone()).unwrap();
        assert!(store.load_save_ram().unwrap());
        assert_eq!(store.save_file_kind(), SaveFileKind::SGame);
        assert_eq!(store.save_ram().to_bytes(), game);

        store.save_ram_mut().set_word(0, 99).unwrap();
        store.write_save_ram().unwrap();
        let mut expected = game.clone();
        expected[0..4].copy_from_slice(&99i32.to_le_bytes());
        assert_eq!(store.storage.read(SaveRam::SAVE_PATH).unwrap(), expected);
        assert_eq!(store.storage.read(SaveRam::MODERN_SAVE_PATH).unwrap(), data);
    }

    #[test]
    fn save_ram_falls_back_to_sdata_and_writes_it_back() {
        let bytes = pattern_bytes(0x2000);
        let storage = seeded_storage(SaveRam::MODERN_SAVE_PATH, &bytes);
        let mut store = SaveStore::open(storage.clone()).unwrap();

        assert!(store.load_save_ram().unwrap());
        assert_eq!(store.save_file_kind(), SaveFileKind::SData);
        store.save_ram_mut().set_word(1, 5).unwrap();
        store.write_save_ram().unwrap();

        assert!(!store.storage.exists(SaveRam::SAVE_PATH));
        let mut expected = bytes;
        expected[4..8].copy_from_slice(&5i32.to_le_bytes());
        assert_eq!(
            store.storage.read(SaveRam::MODERN_SAVE_PATH).unwrap(),
            expected
        );
    }

    #[test]
    fn short_save_files_round_trip_like_upstream() {
        let short = pattern_bytes(3);
        let storage = seeded_storage(SaveRam::SAVE_PATH, &short);
        let mut store = SaveStore::open(storage.clone()).unwrap();

        assert!(store.load_save_ram().unwrap());
        assert!(!store.save_ram().is_full());
        assert_eq!(store.save_ram().words.len(), 3);
        store.write_save_ram().unwrap();
        assert_eq!(store.storage.read(SaveRam::SAVE_PATH).unwrap(), short);
        assert!(!store.dirty());
    }

    #[test]
    fn malformed_save_files_error_without_panicking_or_mutating() {
        for bytes in [vec![0u8; 3], vec![0u8; 0x2000 * 4 + 4]] {
            let storage = seeded_storage(SaveRam::SAVE_PATH, &bytes);
            let mut store = SaveStore::open(storage).unwrap();
            assert!(matches!(
                store.load_save_ram(),
                Err(SaveError::Format(FormatError::Invalid(_)))
            ));
            assert!(store.save_ram().words.iter().all(|word| *word == 0));
            assert!(!store.dirty());
        }
    }

    #[test]
    fn malformed_achievements_error_without_panicking() {
        for bytes in [vec![0u8; 3], vec![0u8; (ACHIEVEMENT_FILE_SLOTS + 1) * 4]] {
            let storage = seeded_storage(Achievements::PATH, &bytes);
            let mut store = SaveStore::open(storage).unwrap();
            assert!(matches!(
                store.load_achievements(),
                Err(SaveError::Format(FormatError::Invalid(_)))
            ));
            assert!(
                store
                    .achievements()
                    .entries
                    .iter()
                    .all(|entry| entry.status == 0)
            );
        }
    }

    #[test]
    fn read_and_open_failures_are_reported() {
        let mut storage = FailingStorage::new();
        storage.fail_list = true;
        assert!(matches!(
            SaveStore::open(storage),
            Err(SaveError::Storage(_))
        ));

        let mut storage = FailingStorage::new();
        storage
            .inner
            .write(SaveRam::SAVE_PATH, &pattern_bytes(1))
            .unwrap();
        storage.fail_read = true;
        let mut store = SaveStore::open(storage).unwrap();
        assert!(matches!(store.load_save_ram(), Err(SaveError::Storage(_))));
    }

    #[test]
    fn write_failure_preserves_the_previous_save_and_dirty_state() {
        let bytes = pattern_bytes(0x2000);
        let mut storage = FailingStorage::new();
        storage.inner.write(SaveRam::SAVE_PATH, &bytes).unwrap();
        let mut store = SaveStore::open(storage).unwrap();
        assert!(store.load_save_ram().unwrap());

        store.save_ram_mut().set_word(2, 42).unwrap();
        store.storage.fail_write = true;
        assert!(matches!(store.write_save_ram(), Err(SaveError::Storage(_))));
        assert!(store.dirty());
        assert_eq!(store.storage.inner.read(SaveRam::SAVE_PATH).unwrap(), bytes);
        assert!(
            !store
                .storage
                .exists(&format!("{}{TEMP_SUFFIX}", SaveRam::SAVE_PATH))
        );

        store.storage.fail_write = false;
        store.write_save_ram().unwrap();
        assert!(!store.dirty());
    }

    #[test]
    fn achievements_round_trip_and_status_update() {
        let statuses: Vec<i32> = (0..ACHIEVEMENT_FILE_SLOTS as i32)
            .map(|id| id % 3)
            .collect();
        let mut bytes = Vec::new();
        for status in &statuses {
            bytes.extend_from_slice(&status.to_le_bytes());
        }
        let storage = seeded_storage(Achievements::PATH, &bytes);
        let mut store = SaveStore::open(storage.clone()).unwrap();

        assert!(store.load_achievements().unwrap());
        assert_eq!(store.achievements().entries.len(), ACHIEVEMENT_FILE_SLOTS);
        assert_eq!(store.achievements().status(5), Some(2));
        assert!(!store.dirty());

        assert!(store.set_achievement_status(5, 1));
        assert!(!store.set_achievement_status(0x100, 1));
        assert!(store.achievements_mut().set_name(5, "ACH_TEST"));
        assert!(store.dirty());
        store.write_achievements().unwrap();
        assert!(!store.dirty());

        let mut expected = bytes.clone();
        expected[5 * 4..5 * 4 + 4].copy_from_slice(&1i32.to_le_bytes());
        assert_eq!(store.storage.read(Achievements::PATH).unwrap(), expected);
        assert!(
            !store
                .storage
                .exists(&format!("{}{TEMP_SUFFIX}", Achievements::PATH))
        );

        let mut reloaded = SaveStore::open(storage).unwrap();
        assert!(reloaded.load_achievements().unwrap());
        assert_eq!(reloaded.achievements().status(5), Some(1));
        assert_eq!(reloaded.achievements().to_bytes(), expected);
        assert!(!reloaded.dirty());
    }

    #[test]
    fn short_achievements_files_round_trip_like_upstream() {
        let bytes = 1i32.to_le_bytes();
        let storage = seeded_storage(Achievements::PATH, &bytes);
        let mut store = SaveStore::open(storage.clone()).unwrap();

        assert!(store.load_achievements().unwrap());
        assert_eq!(store.achievements().entries.len(), 1);
        store.write_achievements().unwrap();
        assert_eq!(store.storage.read(Achievements::PATH).unwrap(), bytes);
        assert!(!store.dirty());
    }

    fn asset_root() -> PathBuf {
        std::env::var("RETRO_ASSETS")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/home/ted/projects/assets"))
    }

    #[test]
    #[ignore = "requires assets"]
    fn asset_userdata_loads_and_reports() {
        for game in ["S1", "S2"] {
            let source = DirSource::new(asset_root().join(game)).expect("asset folder");
            let mut storage = MemoryStorage::new();
            for path in [
                SaveRam::SAVE_PATH,
                SaveRam::MODERN_SAVE_PATH,
                Achievements::PATH,
            ] {
                if source.exists(path) {
                    storage
                        .write(path, &source.read(path).expect("user data file"))
                        .unwrap();
                }
            }

            let mut store = SaveStore::open(storage).unwrap();
            let has_save = store.load_save_ram().unwrap();
            let has_achievements = store.load_achievements().unwrap();

            if has_save {
                let file = match store.save_file_kind() {
                    SaveFileKind::SGame => SaveRam::SAVE_PATH,
                    SaveFileKind::SData => SaveRam::MODERN_SAVE_PATH,
                };
                let words = store.save_ram().words.len();
                let nonzero = store
                    .save_ram()
                    .words
                    .iter()
                    .filter(|word| **word != 0)
                    .count();
                println!("{game}: save={file} words={words} nonzero={nonzero}");
                if let Ok(game_view) = retro_format_v4::SaveGame::from_ram(store.save_ram()) {
                    println!(
                        "{game}: save slot 0 character={} lives={} score={} emeralds={} initialized={}",
                        game_view.files[0].character_id,
                        game_view.files[0].lives,
                        game_view.files[0].score,
                        game_view.files[0].emeralds,
                        game_view.save_initialized
                    );
                }
                assert_eq!(
                    store.save_ram().to_bytes(),
                    source.read(file).unwrap(),
                    "{game}: save RAM must round-trip byte-exactly"
                );
                assert!(!store.dirty());
            } else {
                println!("{game}: no SGame.bin/SData.bin, zeroed save RAM");
                assert!(store.save_ram().words.iter().all(|word| *word == 0));
            }

            if has_achievements {
                let named = store
                    .achievements()
                    .entries
                    .iter()
                    .filter(|entry| entry.status != 0)
                    .count();
                println!(
                    "{game}: Achievements.bin slots={} nonzero={named}",
                    store.achievements().entries.len()
                );
                assert_eq!(
                    store.achievements().to_bytes(),
                    source.read(Achievements::PATH).unwrap(),
                    "{game}: achievements must round-trip byte-exactly"
                );
            } else {
                println!("{game}: no Achievements.bin, zeroed table");
            }
        }
    }
}
