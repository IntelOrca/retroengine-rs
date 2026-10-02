//! RSDKv4 user data: save RAM, achievements and leaderboards.
//!
//! # Save RAM (`SGame.bin` / `SData.bin`)
//!
//! `RSDKv4/Userdata.cpp` allocates `int saveRAM[SAVEDATA_SIZE]` with `SAVEDATA_SIZE = 0x2000`
//! (`RSDKv4/Userdata.hpp`) and reads or writes the whole array as little-endian `i32`s, so the
//! canonical file is exactly `0x2000 * 4 = 32768` bytes. `ReadSaveRAMData` prefers `SData.bin`
//! and falls back to `SGame.bin`; [`SaveRam::load`] mirrors that and reports which file was
//! used. [`SaveGame`] is a typed view over the documented first 3072 words of the array.
//!
//! # Achievements (`Achievements.bin`)
//!
//! RSDKv4-Decompilation keeps achievement *definitions* (name/description/status) in memory and
//! only persists the statuses in `UData.bin`. The asset trees produced by the Origins-era engine
//! also ship a 1024-byte `Achievements.bin` containing `0x100` little-endian `i32` statuses,
//! written by the RSDKv5 user core that this data was run with (`RSDKv5/RSDK/User/Core/
//! UserCore.cpp`). [`Achievements`] represents each slot as an [`Achievement`] with the slot
//! index as `id`, the persisted `status`, and an optional `name` that is populated by the
//! caller (the engine resolves names from scripts) and never written back to disk.
//!
//! # `UData.bin`
//!
//! `ReadUserdata` reads `ACHIEVEMENT_COUNT = 0x40` achievement statuses followed by
//! `LEADERBOARD_COUNT = 0x80` leaderboard scores, `0x300` bytes in total; zeros in the
//! leaderboard table are displayed as `0x7FFFFFF` by the engine. [`UserData`] preserves the raw
//! values so files round-trip byte-identically and exposes the display default separately.

use serde::Serialize;

use crate::error::FormatError;
use crate::reader::Reader;
use retro_io::DataSource;

/// Number of `i32` words in the engine's save RAM (`SAVEDATA_SIZE`).
pub const SAVE_RAM_WORDS: usize = 0x2000;
/// Canonical size of a save file in bytes (`SAVEDATA_SIZE * 4`).
pub const SAVE_RAM_BYTES: usize = SAVE_RAM_WORDS * 4;
/// Number of save slots stored at the start of the save RAM.
pub const SAVE_FILE_COUNT: usize = 4;
/// Number of `i32` words used by one [`SaveFile`].
pub const SAVE_FILE_WORDS: usize = 8;
/// Number of time-attack records (`records[0x80]`), starting at word 64.
pub const RECORD_COUNT: usize = 0x80;
/// Number of custom special-stage records (`customSS[0x400]`), starting at word 2048.
pub const CUSTOM_SPECIAL_STAGE_COUNT: usize = 0x400;
/// First word of the custom special-stage table.
pub const CUSTOM_SPECIAL_STAGES_OFFSET: usize = 2048;
/// Word offset of `records[0]`.
pub const RECORDS_OFFSET: usize = 64;
/// Number of words covered by [`SaveGame`].
pub const SAVE_GAME_WORDS: usize = CUSTOM_SPECIAL_STAGES_OFFSET + CUSTOM_SPECIAL_STAGE_COUNT;

/// Number of entries read from `UData.bin` achievement table (`ACHIEVEMENT_COUNT`).
pub const ACHIEVEMENT_COUNT: usize = 0x40;
/// Number of entries read from `UData.bin` leaderboard table (`LEADERBOARD_COUNT`).
pub const LEADERBOARD_COUNT: usize = 0x80;
/// Number of status slots in `Achievements.bin` (RSDKv5 user core size).
pub const ACHIEVEMENT_FILE_SLOTS: usize = 0x100;
/// Byte size of `UData.bin` (`(ACHIEVEMENT_COUNT + LEADERBOARD_COUNT) * 4`).
pub const UDATA_BYTES: usize = (ACHIEVEMENT_COUNT + LEADERBOARD_COUNT) * 4;
/// Score shown by the engine for leaderboard entries that have never been set.
pub const DEFAULT_LEADERBOARD_SCORE: i32 = 0x07FF_FFFF;

/// Which file a [`SaveRam`] was loaded from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SaveFileKind {
    /// `SData.bin` (preferred by the engine).
    SData,
    /// `SGame.bin` (fallback).
    SGame,
}

/// Raw save RAM: the engine's `int saveRAM[0x2000]`.
///
/// Words are stored exactly as they appear on disk (little-endian). A file shorter than
/// [`SAVE_RAM_BYTES`] is accepted like the engine's unchecked `fRead` and keeps only the words
/// present, so parsing and re-serialising any such file is byte-identical.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SaveRam {
    /// Little-endian `i32` words, at most [`SAVE_RAM_WORDS`] long.
    pub words: Vec<i32>,
}

impl SaveRam {
    /// Engine's fallback save file name.
    pub const SAVE_PATH: &str = "SGame.bin";
    /// Engine's preferred save file name.
    pub const MODERN_SAVE_PATH: &str = "SData.bin";

    /// Creates a save RAM from words, validating the maximum length.
    pub fn new(words: Vec<i32>) -> Result<Self, FormatError> {
        if words.len() > SAVE_RAM_WORDS {
            return Err(FormatError::invalid(format!(
                "save RAM has {} words, maximum is {SAVE_RAM_WORDS}",
                words.len()
            )));
        }
        Ok(Self { words })
    }

    /// Creates a full zeroed save RAM of [`SAVE_RAM_WORDS`] words.
    pub fn zeroed() -> Self {
        Self {
            words: vec![0; SAVE_RAM_WORDS],
        }
    }

    /// Parses save RAM bytes. The length must be a multiple of four and at most
    /// [`SAVE_RAM_BYTES`].
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, FormatError> {
        if !bytes.len().is_multiple_of(4) {
            return Err(FormatError::invalid(format!(
                "save data length {} is not a multiple of 4",
                bytes.len()
            )));
        }
        let count = bytes.len() / 4;
        if count > SAVE_RAM_WORDS {
            return Err(FormatError::invalid(format!(
                "save data has {count} words, maximum is {SAVE_RAM_WORDS}"
            )));
        }
        let mut reader = Reader::new(bytes);
        let mut words = Vec::with_capacity(count);
        for _ in 0..count {
            words.push(reader.read_i32_le()?);
        }
        Ok(Self { words })
    }

    /// Serialises the words little-endian. `to_bytes(from_bytes(bytes)) == bytes` for every
    /// accepted input.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.words.len() * 4);
        for word in &self.words {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes
    }

    /// Whether this save RAM contains the full [`SAVE_RAM_WORDS`] words.
    pub fn is_full(&self) -> bool {
        self.words.len() == SAVE_RAM_WORDS
    }

    /// Returns one word, or `None` when `index` is beyond the stored data.
    pub fn word(&self, index: usize) -> Option<i32> {
        self.words.get(index).copied()
    }

    /// Overwrites one word. Returns [`FormatError::Truncated`] when `index` is beyond the data.
    pub fn set_word(&mut self, index: usize, value: i32) -> Result<(), FormatError> {
        let slot = self.words.get_mut(index).ok_or(FormatError::Truncated)?;
        *slot = value;
        Ok(())
    }

    /// Loads `SData.bin` (preferred) or `SGame.bin` through `src`, mirroring
    /// `ReadSaveRAMData`.
    pub fn load(src: &dyn DataSource) -> Result<(Self, SaveFileKind), FormatError> {
        if src.exists(Self::MODERN_SAVE_PATH) {
            return Ok((
                Self::from_bytes(&src.read(Self::MODERN_SAVE_PATH)?)?,
                SaveFileKind::SData,
            ));
        }
        Ok((
            Self::from_bytes(&src.read(Self::SAVE_PATH)?)?,
            SaveFileKind::SGame,
        ))
    }
}

/// One of the four save slots (the engine's `SaveFile`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct SaveFile {
    /// `characterID`: 0/8/16/24 for Sonic/Tails/Knuckles/Sonic+Tails.
    pub character_id: i32,
    /// `lives`.
    pub lives: i32,
    /// `score`.
    pub score: i32,
    /// `scoreBonus`: extra 100k-point bonuses earned.
    pub score_bonus: i32,
    /// `stageID`: index of the last played scene.
    pub stage_id: i32,
    /// `emeralds`.
    pub emeralds: i32,
    /// `specialStageID`.
    pub special_stage_id: i32,
    /// `unused`.
    pub unused: i32,
}

impl SaveFile {
    fn from_words(words: &[i32]) -> Self {
        Self {
            character_id: words[0],
            lives: words[1],
            score: words[2],
            score_bonus: words[3],
            stage_id: words[4],
            emeralds: words[5],
            special_stage_id: words[6],
            unused: words[7],
        }
    }

    fn write_to(&self, words: &mut [i32]) {
        words[0] = self.character_id;
        words[1] = self.lives;
        words[2] = self.score;
        words[3] = self.score_bonus;
        words[4] = self.stage_id;
        words[5] = self.emeralds;
        words[6] = self.special_stage_id;
        words[7] = self.unused;
    }
}

/// Typed view over the documented first [`SAVE_GAME_WORDS`] words of the save RAM
/// (`SaveGame` in `RSDKv4/Userdata.hpp`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SaveGame {
    /// The four save slots (words 0..32).
    pub files: [SaveFile; SAVE_FILE_COUNT],
    /// `saveInitialized` (word 32).
    pub save_initialized: i32,
    /// `musVolume` (word 33).
    pub music_volume: i32,
    /// `sfxVolume` (word 34).
    pub sfx_volume: i32,
    /// `spindashEnabled` (word 35).
    pub spindash_enabled: i32,
    /// `boxRegion` (word 36).
    pub box_region: i32,
    /// `vDPadSize` (word 37).
    pub vd_pad_size: i32,
    /// `vDPadOpacity` (word 38).
    pub vd_pad_opacity: i32,
    /// `vDPadX_Move` (word 39).
    pub vd_pad_x_move: i32,
    /// `vDPadY_Move` (word 40).
    pub vd_pad_y_move: i32,
    /// `vDPadX_Jump` (word 41).
    pub vd_pad_x_jump: i32,
    /// `vDPadY_Jump` (word 42).
    pub vd_pad_y_jump: i32,
    /// `tailsUnlocked` (word 43).
    pub tails_unlocked: i32,
    /// `knuxUnlocked` (word 44).
    pub knuckles_unlocked: i32,
    /// `unlockedActs` (word 45).
    pub unlocked_acts: i32,
    /// `unlockedHPZ` (word 46).
    pub unlocked_hpz: i32,
    /// `records[0x80]` (words 64..192).
    pub records: Vec<i32>,
    /// `customSS[0x400]` (words 2048..3072).
    pub custom_special_stages: Vec<i32>,
}

impl SaveGame {
    /// Extracts the typed view from a save RAM. The RAM must contain at least
    /// [`SAVE_GAME_WORDS`] words.
    pub fn from_ram(ram: &SaveRam) -> Result<Self, FormatError> {
        if ram.words.len() < SAVE_GAME_WORDS {
            return Err(FormatError::Truncated);
        }
        let words = &ram.words;
        let mut files = [SaveFile::default(); SAVE_FILE_COUNT];
        for (index, file) in files.iter_mut().enumerate() {
            *file = SaveFile::from_words(
                &words[index * SAVE_FILE_WORDS..(index + 1) * SAVE_FILE_WORDS],
            );
        }
        Ok(Self {
            files,
            save_initialized: words[32],
            music_volume: words[33],
            sfx_volume: words[34],
            spindash_enabled: words[35],
            box_region: words[36],
            vd_pad_size: words[37],
            vd_pad_opacity: words[38],
            vd_pad_x_move: words[39],
            vd_pad_y_move: words[40],
            vd_pad_x_jump: words[41],
            vd_pad_y_jump: words[42],
            tails_unlocked: words[43],
            knuckles_unlocked: words[44],
            unlocked_acts: words[45],
            unlocked_hpz: words[46],
            records: words[RECORDS_OFFSET..RECORDS_OFFSET + RECORD_COUNT].to_vec(),
            custom_special_stages: words[CUSTOM_SPECIAL_STAGES_OFFSET..SAVE_GAME_WORDS].to_vec(),
        })
    }

    /// Writes the typed view back into a save RAM. The RAM must contain at least
    /// [`SAVE_GAME_WORDS`] words and the variable-length tables must have their canonical
    /// lengths.
    pub fn write_to_ram(&self, ram: &mut SaveRam) -> Result<(), FormatError> {
        if ram.words.len() < SAVE_GAME_WORDS {
            return Err(FormatError::Truncated);
        }
        if self.records.len() != RECORD_COUNT {
            return Err(FormatError::invalid(format!(
                "SaveGame records has {} entries, expected {RECORD_COUNT}",
                self.records.len()
            )));
        }
        if self.custom_special_stages.len() != CUSTOM_SPECIAL_STAGE_COUNT {
            return Err(FormatError::invalid(format!(
                "SaveGame custom_special_stages has {} entries, expected {CUSTOM_SPECIAL_STAGE_COUNT}",
                self.custom_special_stages.len()
            )));
        }
        let words = &mut ram.words;
        for (index, file) in self.files.iter().enumerate() {
            file.write_to(&mut words[index * SAVE_FILE_WORDS..(index + 1) * SAVE_FILE_WORDS]);
        }
        words[32] = self.save_initialized;
        words[33] = self.music_volume;
        words[34] = self.sfx_volume;
        words[35] = self.spindash_enabled;
        words[36] = self.box_region;
        words[37] = self.vd_pad_size;
        words[38] = self.vd_pad_opacity;
        words[39] = self.vd_pad_x_move;
        words[40] = self.vd_pad_y_move;
        words[41] = self.vd_pad_x_jump;
        words[42] = self.vd_pad_y_jump;
        words[43] = self.tails_unlocked;
        words[44] = self.knuckles_unlocked;
        words[45] = self.unlocked_acts;
        words[46] = self.unlocked_hpz;
        words[RECORDS_OFFSET..RECORDS_OFFSET + RECORD_COUNT].copy_from_slice(&self.records);
        words[CUSTOM_SPECIAL_STAGES_OFFSET..SAVE_GAME_WORDS]
            .copy_from_slice(&self.custom_special_stages);
        Ok(())
    }
}

/// One achievement slot from `Achievements.bin`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Achievement {
    /// Slot index, which the engine uses as the achievement id.
    pub id: u32,
    /// Persisted status value (the engine stores `0`/`1`).
    pub status: i32,
    /// Achievement id string (`ACH_GOLD_MEDAL`), resolved by the engine from its own list. Not
    /// stored in `Achievements.bin`, so parsing leaves it `None`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Parsed `Achievements.bin`: `0x100` little-endian `i32` status slots.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Achievements {
    /// Slots in file order; `entries[i].id == i`.
    pub entries: Vec<Achievement>,
}

impl Achievements {
    /// Canonical asset path passed to [`Achievements::load`].
    pub const PATH: &str = "Achievements.bin";

    /// Parses `Achievements.bin` bytes. The length must be a multiple of four and at most
    /// [`ACHIEVEMENT_FILE_SLOTS`] entries; shorter files keep only the slots present.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, FormatError> {
        if !bytes.len().is_multiple_of(4) {
            return Err(FormatError::invalid(format!(
                "achievements length {} is not a multiple of 4",
                bytes.len()
            )));
        }
        let count = bytes.len() / 4;
        if count > ACHIEVEMENT_FILE_SLOTS {
            return Err(FormatError::invalid(format!(
                "achievements file has {count} slots, maximum is {ACHIEVEMENT_FILE_SLOTS}"
            )));
        }
        let mut reader = Reader::new(bytes);
        let mut entries = Vec::with_capacity(count);
        for id in 0..count {
            entries.push(Achievement {
                id: id as u32,
                status: reader.read_i32_le()?,
                name: None,
            });
        }
        Ok(Self { entries })
    }

    /// Serialises the status slots little-endian. Names are engine-side metadata and are not
    /// written.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.entries.len() * 4);
        for entry in &self.entries {
            bytes.extend_from_slice(&entry.status.to_le_bytes());
        }
        bytes
    }

    /// Reads [`Achievements::PATH`] through `src` and parses it.
    pub fn load(src: &dyn DataSource) -> Result<Self, FormatError> {
        Self::from_bytes(&src.read(Self::PATH)?)
    }

    /// The persisted status of one slot.
    pub fn status(&self, id: u32) -> Option<i32> {
        self.entries
            .get(id as usize)
            .filter(|entry| entry.id == id)
            .map(|entry| entry.status)
    }

    /// Attaches an engine-side name to a slot; has no effect on serialisation.
    pub fn set_name(&mut self, id: u32, name: impl Into<String>) -> bool {
        match self
            .entries
            .get_mut(id as usize)
            .filter(|entry| entry.id == id)
        {
            Some(entry) => {
                entry.name = Some(name.into());
                true
            }
            None => false,
        }
    }
}

/// One leaderboard entry from `UData.bin`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct LeaderboardEntry {
    /// Raw `score` word.
    pub score: i32,
}

/// Parsed `UData.bin`: achievement statuses followed by leaderboard scores.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UserData {
    /// `ACHIEVEMENT_COUNT` raw status words.
    pub achievements: Vec<i32>,
    /// `LEADERBOARD_COUNT` leaderboard entries.
    pub leaderboards: Vec<LeaderboardEntry>,
}

impl UserData {
    /// Canonical asset path passed to [`UserData::load`].
    pub const PATH: &str = "UData.bin";

    /// Parses `UData.bin`. Trailing bytes beyond [`UDATA_BYTES`] are ignored, matching the
    /// engine's fixed-size reads; anything shorter is [`FormatError::Truncated`].
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, FormatError> {
        if bytes.len() < UDATA_BYTES {
            return Err(FormatError::Truncated);
        }
        let mut reader = Reader::new(bytes);
        let mut achievements = Vec::with_capacity(ACHIEVEMENT_COUNT);
        for _ in 0..ACHIEVEMENT_COUNT {
            achievements.push(reader.read_i32_le()?);
        }
        let mut leaderboards = Vec::with_capacity(LEADERBOARD_COUNT);
        for _ in 0..LEADERBOARD_COUNT {
            leaderboards.push(LeaderboardEntry {
                score: reader.read_i32_le()?,
            });
        }
        Ok(Self {
            achievements,
            leaderboards,
        })
    }

    /// Serialises the stored tables little-endian. Values parsed by [`UserData::from_bytes`] and
    /// built by [`UserData::zeroed`] always have the canonical lengths and reproduce the input
    /// bytes exactly.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(UDATA_BYTES);
        for status in &self.achievements {
            bytes.extend_from_slice(&status.to_le_bytes());
        }
        for entry in &self.leaderboards {
            bytes.extend_from_slice(&entry.score.to_le_bytes());
        }
        bytes
    }

    /// Constructs an all-zero `UData.bin` image.
    pub fn zeroed() -> Self {
        Self {
            achievements: vec![0; ACHIEVEMENT_COUNT],
            leaderboards: vec![LeaderboardEntry::default(); LEADERBOARD_COUNT],
        }
    }

    /// Reads [`UserData::PATH`] through `src` and parses it.
    pub fn load(src: &dyn DataSource) -> Result<Self, FormatError> {
        Self::from_bytes(&src.read(Self::PATH)?)
    }

    /// The score the engine displays for `index`: stored zeros are shown as
    /// [`DEFAULT_LEADERBOARD_SCORE`].
    pub fn leaderboard_score(&self, index: usize) -> Option<i32> {
        self.leaderboards.get(index).map(|entry| {
            if entry.score == 0 {
                DEFAULT_LEADERBOARD_SCORE
            } else {
                entry.score
            }
        })
    }

    /// Applies the engine's zero-to-default normalisation to every leaderboard entry.
    pub fn normalized(&self) -> Self {
        Self {
            achievements: self.achievements.clone(),
            leaderboards: self
                .leaderboards
                .iter()
                .map(|entry| LeaderboardEntry {
                    score: if entry.score == 0 {
                        DEFAULT_LEADERBOARD_SCORE
                    } else {
                        entry.score
                    },
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use retro_io::MemorySource;

    fn pattern_words(count: usize) -> Vec<i32> {
        (0..count)
            .map(|index| (index as i32).wrapping_mul(16_777_619))
            .collect()
    }

    #[test]
    fn save_ram_round_trips() {
        let words = pattern_words(SAVE_RAM_WORDS);
        let ram = SaveRam::new(words.clone()).unwrap();
        let bytes = ram.to_bytes();
        assert_eq!(bytes.len(), SAVE_RAM_BYTES);
        let parsed = SaveRam::from_bytes(&bytes).unwrap();
        assert_eq!(parsed.words, words);
        assert_eq!(parsed.to_bytes(), bytes);
        assert!(parsed.is_full());
        assert_eq!(parsed.word(1), Some(words[1]));
        assert_eq!(parsed.word(SAVE_RAM_WORDS), None);
    }

    #[test]
    fn save_ram_accepts_short_files_like_upstream() {
        let bytes = 7i32.to_le_bytes();
        let ram = SaveRam::from_bytes(&bytes).unwrap();
        assert_eq!(ram.words, [7]);
        assert!(!ram.is_full());
        assert_eq!(ram.to_bytes(), bytes);
        assert!(SaveRam::from_bytes(&[]).unwrap().words.is_empty());
    }

    #[test]
    fn save_ram_rejects_malformed_lengths() {
        assert!(matches!(
            SaveRam::from_bytes(&[0, 0, 0]),
            Err(FormatError::Invalid(_))
        ));
        let oversized = vec![0u8; SAVE_RAM_BYTES + 4];
        assert!(matches!(
            SaveRam::from_bytes(&oversized),
            Err(FormatError::Invalid(_))
        ));
        assert!(matches!(
            SaveRam::new(vec![0; SAVE_RAM_WORDS + 1]),
            Err(FormatError::Invalid(_))
        ));
    }

    #[test]
    fn save_ram_zeroed_and_mutation() {
        let mut ram = SaveRam::zeroed();
        assert!(ram.is_full());
        assert!(ram.words.iter().all(|word| *word == 0));
        ram.set_word(0x1FFF, 42).unwrap();
        assert_eq!(ram.word(0x1FFF), Some(42));
        assert!(matches!(
            ram.set_word(0x2000, 1),
            Err(FormatError::Truncated)
        ));
    }

    #[test]
    fn save_ram_load_prefers_sdata() {
        let mut source = MemorySource::new();
        source.insert("SGame.bin", SaveRam::zeroed().to_bytes());
        let (ram, kind) = SaveRam::load(&source).unwrap();
        assert_eq!(kind, SaveFileKind::SGame);
        assert!(ram.is_full());

        source.insert("SData.bin", vec![1, 2, 3, 4]);
        let (ram, kind) = SaveRam::load(&source).unwrap();
        assert_eq!(kind, SaveFileKind::SData);
        assert_eq!(ram.words, [0x0403_0201]);

        assert!(matches!(
            SaveRam::load(&MemorySource::new()),
            Err(FormatError::Io(_))
        ));
    }

    #[test]
    fn save_game_typed_view_round_trips() {
        let mut ram = SaveRam::zeroed();
        ram.set_word(0, 8).unwrap();
        ram.set_word(1, 3).unwrap();
        ram.set_word(2, 123_456).unwrap();
        ram.set_word(4, 7).unwrap();
        ram.set_word(32, 1).unwrap();
        ram.set_word(33, 100).unwrap();
        ram.set_word(RECORDS_OFFSET + 5, 999).unwrap();
        ram.set_word(CUSTOM_SPECIAL_STAGES_OFFSET + 1, 55).unwrap();

        let game = SaveGame::from_ram(&ram).unwrap();
        assert_eq!(game.files[0].character_id, 8);
        assert_eq!(game.files[0].lives, 3);
        assert_eq!(game.files[0].score, 123_456);
        assert_eq!(game.files[0].stage_id, 7);
        assert_eq!(game.save_initialized, 1);
        assert_eq!(game.music_volume, 100);
        assert_eq!(game.records.len(), RECORD_COUNT);
        assert_eq!(game.records[5], 999);
        assert_eq!(game.custom_special_stages.len(), CUSTOM_SPECIAL_STAGE_COUNT);
        assert_eq!(game.custom_special_stages[1], 55);

        let mut written = SaveRam::zeroed();
        game.write_to_ram(&mut written).unwrap();
        assert_eq!(written, ram);

        assert!(matches!(
            SaveGame::from_ram(&SaveRam::new(vec![0; 32]).unwrap()),
            Err(FormatError::Truncated)
        ));
        let mut truncated = SaveGame::from_ram(&SaveRam::zeroed()).unwrap();
        truncated.records.pop();
        assert!(matches!(
            truncated.write_to_ram(&mut SaveRam::zeroed()),
            Err(FormatError::Invalid(_))
        ));
    }

    #[test]
    fn achievements_round_trip() {
        let statuses: Vec<i32> = (0..ACHIEVEMENT_FILE_SLOTS as i32).map(|i| i % 3).collect();
        let mut bytes = Vec::new();
        for status in &statuses {
            bytes.extend_from_slice(&status.to_le_bytes());
        }
        let achievements = Achievements::from_bytes(&bytes).unwrap();
        assert_eq!(achievements.entries.len(), ACHIEVEMENT_FILE_SLOTS);
        assert_eq!(achievements.entries[5].id, 5);
        assert_eq!(achievements.status(5), Some(2));
        assert_eq!(achievements.status(0x100), None);
        assert_eq!(achievements.to_bytes(), bytes);

        let zeros = Achievements::from_bytes(&vec![0u8; 1024]).unwrap();
        assert_eq!(zeros.entries.len(), 256);
        assert_eq!(zeros.to_bytes(), vec![0u8; 1024]);
        assert!(zeros.entries.iter().all(|entry| entry.name.is_none()));
    }

    #[test]
    fn achievements_names_do_not_affect_bytes() {
        let mut achievements = Achievements::from_bytes(&[0, 0, 0, 0, 1, 0, 0, 0]).unwrap();
        assert!(achievements.set_name(1, "ACH_TEST"));
        assert!(!achievements.set_name(2, "ACH_MISSING"));
        assert_eq!(achievements.entries[1].name.as_deref(), Some("ACH_TEST"));
        assert_eq!(achievements.to_bytes(), [0, 0, 0, 0, 1, 0, 0, 0]);
        let json = serde_json::to_string(&achievements).unwrap();
        assert!(json.contains("ACH_TEST"));
    }

    #[test]
    fn achievements_reject_malformed_lengths() {
        assert!(matches!(
            Achievements::from_bytes(&[0, 0, 0]),
            Err(FormatError::Invalid(_))
        ));
        let oversized = vec![0u8; (ACHIEVEMENT_FILE_SLOTS + 1) * 4];
        assert!(matches!(
            Achievements::from_bytes(&oversized),
            Err(FormatError::Invalid(_))
        ));
    }

    #[test]
    fn achievements_load_through_data_source() {
        let mut source = MemorySource::new();
        source.insert("Achievements.bin", vec![1, 0, 0, 0]);
        let achievements = Achievements::load(&source).unwrap();
        assert_eq!(achievements.status(0), Some(1));
    }

    fn udata_bytes() -> Vec<u8> {
        let mut bytes = Vec::new();
        for index in 0..ACHIEVEMENT_COUNT {
            bytes.extend_from_slice(&(index as i32).to_le_bytes());
        }
        for index in 0..LEADERBOARD_COUNT {
            bytes.extend_from_slice(&(index as i32).to_le_bytes());
        }
        bytes
    }

    #[test]
    fn user_data_round_trips() {
        let bytes = udata_bytes();
        assert_eq!(bytes.len(), UDATA_BYTES);
        let data = UserData::from_bytes(&bytes).unwrap();
        assert_eq!(data.achievements.len(), ACHIEVEMENT_COUNT);
        assert_eq!(data.leaderboards.len(), LEADERBOARD_COUNT);
        assert_eq!(data.achievements[3], 3);
        assert_eq!(data.leaderboards[3].score, 3);
        assert_eq!(data.to_bytes(), bytes);

        let mut with_extra = bytes.clone();
        with_extra.extend_from_slice(&[0xAA; 8]);
        assert_eq!(UserData::from_bytes(&with_extra).unwrap().to_bytes(), bytes);
    }

    #[test]
    fn user_data_rejects_truncated_input() {
        let bytes = udata_bytes();
        assert!(matches!(
            UserData::from_bytes(&bytes[..UDATA_BYTES - 1]),
            Err(FormatError::Truncated)
        ));
        assert!(matches!(
            UserData::from_bytes(&[]),
            Err(FormatError::Truncated)
        ));
    }

    #[test]
    fn user_data_leaderboard_defaults() {
        let data = UserData::zeroed();
        assert_eq!(data.leaderboard_score(0), Some(DEFAULT_LEADERBOARD_SCORE));
        assert_eq!(data.leaderboard_score(LEADERBOARD_COUNT), None);

        let normalized = data.normalized();
        assert_eq!(normalized.leaderboards[0].score, DEFAULT_LEADERBOARD_SCORE);
        assert_eq!(
            normalized.leaderboard_score(0),
            Some(DEFAULT_LEADERBOARD_SCORE)
        );
        // The raw form still round-trips the stored zeros.
        assert_eq!(data.to_bytes(), vec![0u8; UDATA_BYTES]);
    }

    #[test]
    fn never_panics_on_arbitrary_input() {
        let mut state = 0x0BAD_F00Du32;
        for length in 0..1024usize {
            let bytes: Vec<u8> = (0..length)
                .map(|_| {
                    state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    (state >> 24) as u8
                })
                .collect();
            let _ = SaveRam::from_bytes(&bytes);
            let _ = Achievements::from_bytes(&bytes);
            let _ = UserData::from_bytes(&bytes);
            if length % 4 == 0 {
                if let Ok(ram) = SaveRam::from_bytes(&bytes) {
                    let _ = SaveGame::from_ram(&ram);
                }
                if let Ok(achievements) = Achievements::from_bytes(&bytes) {
                    let _ = achievements.to_bytes();
                }
            }
        }
    }
}
