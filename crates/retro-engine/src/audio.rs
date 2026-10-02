//! Engine-side audio: SFX/music tables, lazy asset loading and per-frame mixing.
//!
//! This module wires [`retro_audio::Mixer`] into the script host. SFX entries are indexed exactly
//! like upstream `LoadGlobalSfx`/`LoadStageFiles` build them: the `GameConfig.bin` sound effects
//! occupy slots `0..global_count`, the `StageConfig.bin` sound effects follow. Decoding is lazy:
//! the first `PlaySfx`/`PlayMusic` for an entry reads it through the [`DataSource`] and caches the
//! mixer id, so a run that never touches audio never pays for it.
//!
//! Every logic frame mixes exactly [`retro_audio::FRAMES_PER_TICK`] stereo frames (735 at
//! 44.1 kHz, i.e. 60 Hz). [`AudioState::tick`] hashes those samples regardless of whether a
//! device is attached, so headless `--audio-hash` output is identical to windowed playback and
//! `--mute` only skips the device submit.
//!
//! # Music streams
//!
//! `SetMusicTrack`/`SwapMusicTrack` only store the track metadata (`Data/Music/<file>`, loop flag
//! and loop point); `PlayMusic` decodes the Ogg Vorbis stream on first use. Upstream's
//! `musicRatio` cross-fade start position is accepted but ignored: the deterministic mixer always
//! starts a swapped track at frame `0` of the loop point it was given.

use std::sync::Arc;

use retro_audio::{
    AudioEngine, CHANNELS, FRAMES_PER_TICK, MAX_VOLUME, Mixer, SAMPLE_RATE, SfxId, StreamId,
};
use retro_format_v4::{AudioSettings, GameConfig, StageConfig};
use retro_io::DataSource;

/// Upstream `TRACK_COUNT`.
pub const TRACK_COUNT: usize = 0x10;

/// One `musicTracks` entry (`TrackInfo`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct TrackInfo {
    /// `Data/Music/<file>`, or `None` when no track was set.
    file: Option<String>,
    /// `trackLoop`.
    looping: bool,
    /// `loopPoint`, a source PCM frame index.
    loop_point: i32,
}

/// Mixer, SFX/music tables and the optional output device.
///
/// The state is deliberately deterministic: identical op sequences over identical assets produce
/// identical per-frame hashes with or without a device and with or without muting.
pub struct AudioState {
    mixer: Mixer,
    device: Option<AudioEngine>,
    source: Option<Arc<dyn DataSource>>,
    sfx_paths: Vec<Option<String>>,
    sfx_cache: Vec<Option<SfxId>>,
    sfx_failed: Vec<bool>,
    tracks: Vec<TrackInfo>,
    stream_cache: Vec<Option<StreamId>>,
    stream_failed: Vec<bool>,
    scratch: Vec<f32>,
    capture: bool,
    captured: Vec<f32>,
    muted: bool,
    last_hash: [u8; 32],
}

impl AudioState {
    /// An audio state with no asset source and no sound effects (used by host unit tests).
    #[must_use]
    pub fn empty() -> Self {
        Self {
            mixer: Mixer::new(),
            device: None,
            source: None,
            sfx_paths: Vec::new(),
            sfx_cache: Vec::new(),
            sfx_failed: Vec::new(),
            tracks: vec![TrackInfo::default(); TRACK_COUNT],
            stream_cache: vec![None; TRACK_COUNT],
            stream_failed: vec![false; TRACK_COUNT],
            scratch: Vec::new(),
            capture: false,
            captured: Vec::new(),
            muted: false,
            last_hash: [0; 32],
        }
    }

    /// Builds the SFX table from the loaded configs and applies `Settings.ini` volumes.
    ///
    /// Global sound effects come first, then stage sound effects, matching
    /// `LoadGlobalSfx` + `LoadStageFiles`.
    #[must_use]
    pub fn for_scene(
        source: Arc<dyn DataSource>,
        game_config: &GameConfig,
        stage_config: &StageConfig,
        settings: &AudioSettings,
    ) -> Self {
        let mut state = Self::empty();
        state.source = Some(source);
        state.sfx_paths = game_config
            .sound_effects
            .iter()
            .chain(&stage_config.sound_effects)
            .map(|sfx| Some(format!("Data/SoundFX/{}", sfx.path)))
            .collect();
        state.sfx_cache = vec![None; state.sfx_paths.len()];
        state.sfx_failed = vec![false; state.sfx_paths.len()];
        state.mixer.set_stream_volume(settings.stream_volume);
        state.mixer.set_sfx_volume(settings.sfx_volume);
        state
    }

    /// Attaches a windowed output device; mixing continues through it from now on.
    pub fn set_device(&mut self, device: AudioEngine) {
        self.device = Some(device);
    }

    /// Enables or disables output. Mixing and hashing are unaffected.
    pub fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
    }

    /// Whether output is muted.
    #[must_use]
    pub fn is_muted(&self) -> bool {
        self.muted
    }

    /// Enables capture of every mixed sample in [`AudioState::captured_pcm`].
    ///
    /// Capture is only filled on the local-mixer path (no device, or muted with a device): the
    /// windowed submit path hashes the samples but does not retain them.
    pub fn set_capture(&mut self, capture: bool) {
        self.capture = capture;
        if !capture {
            self.captured.clear();
        }
    }

    /// The captured interleaved stereo samples (empty unless capture was enabled).
    #[must_use]
    pub fn captured_pcm(&self) -> &[f32] {
        &self.captured
    }

    /// The hash of the most recent [`AudioState::tick`].
    #[must_use]
    pub const fn last_hash(&self) -> [u8; 32] {
        self.last_hash
    }

    /// The most recent mix hash as lower-case hex.
    #[must_use]
    pub fn last_hash_hex(&self) -> String {
        hash_hex(&self.last_hash)
    }

    /// The bound mixer.
    #[must_use]
    pub fn mixer(&self) -> &Mixer {
        &self.mixer
    }

    /// Mutable access to the mixer (tests and tooling).
    pub fn mixer_mut(&mut self) -> &mut Mixer {
        &mut self.mixer
    }

    /// Music volume in upstream `0..=100` units.
    #[must_use]
    pub fn stream_volume(&self) -> u8 {
        self.mixer.stream_volume()
    }

    /// SFX volume in upstream `0..=100` units.
    #[must_use]
    pub fn sfx_volume(&self) -> u8 {
        self.mixer.sfx_volume()
    }

    /// Number of SFX slots built from the configs.
    #[must_use]
    pub fn sfx_slots(&self) -> usize {
        self.sfx_paths.len()
    }

    /// The SFX asset path for `index`, if the slot exists.
    #[must_use]
    pub fn sfx_path(&self, index: usize) -> Option<&str> {
        self.sfx_paths.get(index).and_then(Option::as_deref)
    }

    /// The `Data/Music/<file>` path stored on `track`, if any.
    #[must_use]
    pub fn track_file(&self, track: i32) -> Option<&str> {
        let track = usize::try_from(track).ok()?;
        self.tracks.get(track)?.file.as_deref()
    }

    /// `SetMusicTrack`: stores a track's file, loop flag and loop point.
    pub fn set_track(&mut self, track: i32, file: &str, looping: bool, loop_point: i32) {
        let Ok(track) = usize::try_from(track) else {
            return;
        };
        let Some(info) = self.tracks.get_mut(track) else {
            return;
        };
        if file.is_empty() {
            *info = TrackInfo::default();
        } else {
            info.file = Some(format!("Data/Music/{file}"));
            info.looping = looping;
            info.loop_point = loop_point.max(0);
        }
    }

    /// `PlayMusic`: loads (once) and starts the stream stored on `track`.
    ///
    /// A track without a file stops the current music, like upstream.
    pub fn play_music(&mut self, track: i32) {
        let Some(index) = usize::try_from(track)
            .ok()
            .filter(|index| *index < TRACK_COUNT)
        else {
            self.stop_music();
            return;
        };
        if self
            .tracks
            .get(index)
            .and_then(|info| info.file.as_ref())
            .is_none()
        {
            self.stop_music();
            return;
        }
        let Some(id) = self.ensure_stream(index) else {
            self.stop_music();
            return;
        };
        let info = self.tracks.get(index).cloned().unwrap_or_default();
        if info.looping {
            self.mixer.play_stream(id, info.loop_point);
        } else {
            self.mixer.play_stream(id, -1);
        }
    }

    /// `StopMusic`.
    pub fn stop_music(&mut self) {
        self.mixer.stop_stream();
    }

    /// `PauseMusic`.
    pub fn pause_music(&mut self) {
        self.mixer.pause_stream();
    }

    /// `ResumeMusic`.
    pub fn resume_music(&mut self) {
        self.mixer.resume_stream();
    }

    /// `SwapMusicTrack`: replaces a track and immediately plays it.
    ///
    /// An empty file stops the music; the `ratio` operand is accepted by the host but does not
    /// affect the deterministic mixer.
    pub fn swap_music_track(&mut self, track: i32, file: &str, loop_point: i32) {
        if file.is_empty() {
            self.stop_music();
            return;
        }
        self.set_track(track, file, true, loop_point);
        self.play_music(track);
    }

    /// `PlaySfx`: loads (once) and starts the sound effect on a voice channel.
    pub fn play_sfx(&mut self, sfx: i32, looping: bool) {
        let Some(index) = usize::try_from(sfx).ok() else {
            return;
        };
        let Some(id) = self.ensure_sfx(index) else {
            return;
        };
        let channel = self.mixer.play_sfx(id, MAX_VOLUME, 0);
        self.mixer.set_sfx_loop(channel, looping);
    }

    /// `StopSfx`: stops every voice playing `sfx`.
    pub fn stop_sfx(&mut self, sfx: i32) {
        if let Ok(index) = usize::try_from(sfx)
            && let Some(Some(id)) = self.sfx_cache.get(index).copied()
        {
            self.mixer.stop_sfx_id(id);
        }
    }

    /// `SetSfxAttributes`: updates the loop flag (`-1` keeps it) and pan of every matching voice.
    pub fn set_sfx_attributes(&mut self, sfx: i32, loop_count: i32, pan: i32) {
        if let Ok(index) = usize::try_from(sfx)
            && let Some(Some(id)) = self.sfx_cache.get(index).copied()
        {
            self.mixer
                .set_sfx_attributes(id, loop_count, pan.clamp(-100, 100) as i8);
        }
    }

    /// Mixes one engine tick and returns the sample hash.
    ///
    /// With a device attached the samples are submitted through [`AudioEngine`] unless muted;
    /// without one they are mixed locally (and captured when enabled). All paths advance the same
    /// mixer by [`FRAMES_PER_TICK`] frames and hash the same sample stream.
    pub fn tick(&mut self) -> [u8; 32] {
        self.scratch.resize(FRAMES_PER_TICK * CHANNELS, 0.0);
        let hash = if let Some(device) = self.device.as_mut() {
            if self.muted {
                device
                    .mixer_mut()
                    .mix_frame(&mut self.scratch, FRAMES_PER_TICK)
            } else {
                match device.tick() {
                    Ok(hash) => hash,
                    Err(_) => {
                        // The device was lost mid-run: fall back to the local mixer so the
                        // frame loop keeps running; the mixer state is already advanced.
                        self.device = None;
                        self.mixer.mix_frame(&mut self.scratch, FRAMES_PER_TICK)
                    }
                }
            }
        } else {
            self.mixer.mix_frame(&mut self.scratch, FRAMES_PER_TICK)
        };
        if self.capture {
            self.captured.extend_from_slice(&self.scratch);
        }
        self.last_hash = hash;
        hash
    }

    /// Output sample rate the mixer runs at.
    #[must_use]
    pub const fn sample_rate(&self) -> u32 {
        SAMPLE_RATE
    }

    fn ensure_sfx(&mut self, index: usize) -> Option<SfxId> {
        if let Some(Some(id)) = self.sfx_cache.get(index).copied() {
            return Some(id);
        }
        if self.sfx_failed.get(index).copied().unwrap_or(true) {
            return None;
        }
        let path = self.sfx_paths.get(index).and_then(Clone::clone)?;
        let source = self.source.clone()?;
        let bytes = match source.read(&path) {
            Ok(bytes) => bytes,
            Err(_) => {
                self.sfx_failed[index] = true;
                return None;
            }
        };
        let name = path.rsplit('/').next().unwrap_or(&path);
        match self.mixer.load_sfx(name, &bytes) {
            Ok(id) => {
                self.sfx_cache[index] = Some(id);
                Some(id)
            }
            Err(_) => {
                self.sfx_failed[index] = true;
                None
            }
        }
    }

    fn ensure_stream(&mut self, track: usize) -> Option<StreamId> {
        if let Some(Some(id)) = self.stream_cache.get(track).copied() {
            return Some(id);
        }
        if self.stream_failed.get(track).copied().unwrap_or(true) {
            return None;
        }
        let path = self.tracks.get(track)?.file.clone()?;
        let source = self.source.clone()?;
        let bytes = match source.read(&path) {
            Ok(bytes) => bytes,
            Err(_) => {
                self.stream_failed[track] = true;
                return None;
            }
        };
        match self.mixer.load_stream(bytes) {
            Ok(id) => {
                self.stream_cache[track] = Some(id);
                Some(id)
            }
            Err(_) => {
                self.stream_failed[track] = true;
                None
            }
        }
    }
}

impl Default for AudioState {
    fn default() -> Self {
        Self::empty()
    }
}

fn hash_hex(hash: &[u8; 32]) -> String {
    let mut hex = String::with_capacity(64);
    for byte in hash {
        hex.push(char::from_digit(u32::from(byte >> 4), 16).unwrap_or('0'));
        hex.push(char::from_digit(u32::from(byte & 0x0F), 16).unwrap_or('0'));
    }
    hex
}

#[cfg(test)]
mod tests {
    use super::*;
    use retro_format_v4::SoundEffect;
    use retro_io::MemorySource;

    /// Builds a minimal 16-bit stereo WAV with a constant sample.
    fn wav(sample: i16, frames: usize) -> Vec<u8> {
        let mut data = Vec::with_capacity(frames * 4);
        for _ in 0..frames {
            data.extend_from_slice(&sample.to_le_bytes());
            data.extend_from_slice(&sample.to_le_bytes());
        }
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        out.extend_from_slice(b"WAVE");
        out.extend_from_slice(b"fmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
        out.extend_from_slice(&(SAMPLE_RATE * 4).to_le_bytes());
        out.extend_from_slice(&4u16.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&data);
        out
    }

    fn game_config(sound_effects: Vec<SoundEffect>) -> GameConfig {
        GameConfig {
            title: "Test".to_owned(),
            subtitle: String::new(),
            palette: vec![[0, 0, 0]; retro_format_v4::gameconfig::PALETTE_COUNT],
            objects: Vec::new(),
            global_variables: Vec::new(),
            sound_effects,
            players: Vec::new(),
            categories: Vec::new(),
        }
    }

    fn stage_config(sound_effects: Vec<SoundEffect>) -> StageConfig {
        StageConfig {
            load_global_objects: false,
            palette: vec![[0, 0, 0]; retro_format_v4::stageconfig::STAGE_PALETTE_COUNT],
            sound_effects,
            objects: Vec::new(),
        }
    }

    fn state_with_sfx() -> (AudioState, Arc<MemorySource>) {
        let mut source = MemorySource::new();
        source.insert("Data/SoundFX/Global/Beep.wav", wav(1000, 8));
        source.insert("Data/SoundFX/Stage/Bass.wav", wav(2000, 8));
        let source = Arc::new(source);
        let game = game_config(vec![SoundEffect {
            name: "Beep".to_owned(),
            path: "Global/Beep.wav".to_owned(),
        }]);
        let stage = stage_config(vec![SoundEffect {
            name: "Bass".to_owned(),
            path: "Stage/Bass.wav".to_owned(),
        }]);
        let state = AudioState::for_scene(
            Arc::clone(&source) as Arc<dyn DataSource>,
            &game,
            &stage,
            &AudioSettings::default(),
        );
        (state, source)
    }

    #[test]
    fn builds_global_then_stage_sfx_table() {
        let (state, _) = state_with_sfx();
        assert_eq!(state.sfx_slots(), 2);
        assert_eq!(state.sfx_path(0), Some("Data/SoundFX/Global/Beep.wav"));
        assert_eq!(state.sfx_path(1), Some("Data/SoundFX/Stage/Bass.wav"));
        assert_eq!(state.sfx_path(2), None);
    }

    #[test]
    fn lazy_loads_sfx_once_and_caches_failures() {
        let (mut state, _) = state_with_sfx();
        assert_eq!(state.mixer.sfx_count(), 0);
        state.play_sfx(0, false);
        assert_eq!(state.mixer.sfx_count(), 1);
        state.play_sfx(0, false);
        assert_eq!(state.mixer.sfx_count(), 1, "second play reuses the cache");
        state.play_sfx(1, false);
        assert_eq!(state.mixer.sfx_count(), 2);

        // An unknown slot and a slot without a path are ignored without retrying.
        state.play_sfx(9, false);
        state.play_sfx(-1, false);
        assert_eq!(state.mixer.sfx_count(), 2);
    }

    #[test]
    fn missing_asset_marks_the_slot_failed() {
        let source: Arc<dyn DataSource> = Arc::new(MemorySource::new());
        let game = game_config(vec![SoundEffect {
            name: "Missing".to_owned(),
            path: "Global/Missing.wav".to_owned(),
        }]);
        let mut state = AudioState::for_scene(
            source,
            &game,
            &stage_config(Vec::new()),
            &AudioSettings::default(),
        );
        state.play_sfx(0, false);
        assert_eq!(state.mixer.sfx_count(), 0);
        assert!(state.sfx_failed[0]);
        // A second attempt must not touch the source again.
        state.play_sfx(0, false);
        assert_eq!(state.mixer.sfx_count(), 0);
    }

    #[test]
    fn tick_mixes_one_tick_and_is_deterministic() {
        let mut first = state_with_sfx().0;
        let mut second = state_with_sfx().0;
        for state in [&mut first, &mut second] {
            state.play_sfx(0, false);
            state.set_capture(true);
        }
        for frame in 0..3 {
            assert_eq!(first.tick(), second.tick(), "frame {frame}");
        }
        assert_eq!(first.captured_pcm(), second.captured_pcm());
        assert_eq!(first.captured_pcm().len(), FRAMES_PER_TICK * CHANNELS * 3);
        assert!(first.captured_pcm().iter().any(|sample| *sample != 0.0));
    }

    #[test]
    fn mute_does_not_change_the_mix_hash() {
        let (mut loud, _) = state_with_sfx();
        let (mut quiet, _) = state_with_sfx();
        loud.play_sfx(0, false);
        quiet.play_sfx(0, false);
        quiet.set_muted(true);
        for _ in 0..4 {
            assert_eq!(loud.tick(), quiet.tick());
        }
    }

    #[test]
    fn tracks_play_pause_resume_and_swap() {
        let (mut state, _) = state_with_sfx();
        state.set_track(0, "Theme.ogg", true, 100);
        assert!(state.stream_cache[0].is_none());
        state.play_music(0);
        // The asset is missing, so the slot is marked failed and nothing plays.
        assert!(state.stream_failed[0]);
        assert!(!state.mixer.stream_playing());

        state.set_track(1, "", false, 0);
        state.play_music(1);
        assert!(!state.mixer.stream_playing());

        // Pause/resume are no-ops on a stopped stream.
        state.pause_music();
        state.resume_music();
        assert!(!state.mixer.stream_paused());

        state.swap_music_track(2, "", 0);
        assert!(!state.mixer.stream_playing());
    }

    #[test]
    fn volumes_come_from_settings() {
        let source: Arc<dyn DataSource> = Arc::new(MemorySource::new());
        let settings = AudioSettings {
            streams_enabled: true,
            stream_volume: 0.5,
            sfx_volume: 0.25,
        };
        let state = AudioState::for_scene(
            source,
            &game_config(Vec::new()),
            &stage_config(Vec::new()),
            &settings,
        );
        assert_eq!(state.stream_volume(), 50);
        assert_eq!(state.sfx_volume(), 25);
    }
}
