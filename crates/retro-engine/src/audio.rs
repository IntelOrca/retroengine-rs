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
//! and loop point); `PlayMusic` decodes the Ogg Vorbis stream on first use. Cached streams are
//! keyed by the resolved file path, and changing a track's file drops its cached stream, so
//! `SetMusicTrack` + `PlayMusic` always plays the track's current file (upstream
//! `AudioLegacy.cpp` reloads the current `fileName` on every `PlayMusic`). Upstream's `musicRatio`
//! cross-fade start position is accepted but ignored: the deterministic mixer always starts a
//! track at frame `0` of the loop point it was given.

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

/// A loaded music stream cached for one track slot, remembering the file it came from.
///
/// Keying the cache by path is what stops a stale stream being replayed after the track's file
/// changes: a slot whose current `TrackInfo::file` differs from `path` is a cache miss.
#[derive(Clone)]
struct CachedStream {
    /// The resolved `Data/Music/<file>` path the stream was loaded from.
    path: String,
    /// Mixer stream loaded from `path`.
    id: StreamId,
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
    /// `music.volume` (upstream `musicVolume`): the script-visible master volume.
    music_volume: u8,
    /// `Settings.ini` `streamVolume` (upstream `engine.streamVolume`): scales the master.
    stream_volume: u8,
    /// Per-track loaded stream, tagged with the path it was loaded from.
    stream_cache: Vec<Option<CachedStream>>,
    /// Per-track path whose stream load last failed (`None` when it has not failed).
    stream_failed: Vec<Option<String>>,
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
            music_volume: MAX_VOLUME,
            stream_volume: MAX_VOLUME,
            stream_cache: vec![None; TRACK_COUNT],
            stream_failed: vec![None; TRACK_COUNT],
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
        state.stream_volume = state.mixer.stream_volume();
        state.mixer.set_sfx_volume(settings.sfx_volume);
        state
    }

    /// Rebuilds the SFX/track tables for a new scene while keeping the output device.
    ///
    /// `LoadStageFiles` calls `StopAllSfx`/`ReleaseStageSfx` and clears every music track; the
    /// windowed device, mute flag and volume settings survive the reload.
    pub fn reload_for_scene(
        &mut self,
        source: Arc<dyn DataSource>,
        game_config: &GameConfig,
        stage_config: &StageConfig,
        settings: &AudioSettings,
    ) {
        let device = self.device.take();
        let muted = self.muted;
        let capture = self.capture;
        *self = Self::for_scene(source, game_config, stage_config, settings);
        self.device = device;
        self.muted = muted;
        self.capture = capture;
    }

    /// `StopAllSfx` plus clearing every music track (`SetMusicTrack("", i, false, 0)`).
    ///
    /// `LoadStageFiles` runs both for a same-folder act reload; a full scene load rebuilds the
    /// whole table through [`AudioState::reload_for_scene`] instead.
    pub fn reset_stage_tracks(&mut self) {
        self.mixer.stop_all_sfx();
        self.mixer.stop_stream();
        for track in &mut self.tracks {
            *track = TrackInfo::default();
        }
        self.stream_cache.fill(None);
        self.stream_failed.fill(None);
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
    /// The engine always mixes locally before submitting, so capture works with or without a
    /// device and with or without muting.
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

    /// Flow-control counters of the attached device, or `None` when no device is attached.
    #[must_use]
    pub fn audio_counters(&self) -> Option<retro_audio::AudioCounters> {
        self.device.as_ref().map(AudioEngine::counters)
    }

    /// One-line `audio:` flow-control report for the attached device, or `None` without one.
    ///
    /// Reports the totals that diagnose playback gaps: device-accepted audio, audio dropped by a
    /// backlog resync, resyncs, underruns, and the deepest device queue and backlog seen. This is
    /// how a windowed run shows whether the logic loop outran (drops) or fell behind (underruns)
    /// the device; the numbers never affect mixing or the PCM hash.
    #[must_use]
    pub fn audio_diagnostics(&self) -> Option<String> {
        let counters = self.audio_counters()?;
        let seconds = |frames: u64| frames as f64 / f64::from(SAMPLE_RATE);
        let millis = |frames: usize| frames as f64 * 1000.0 / f64::from(SAMPLE_RATE);
        Some(format!(
            "submitted {:.2} s, dropped {:.2} s in {} resync(s), {} underrun(s), \
             peak queue {:.0} ms, peak backlog {:.0} ms",
            seconds(counters.submitted_frames),
            seconds(counters.dropped_frames),
            counters.resyncs,
            counters.underruns,
            millis(counters.max_queued_frames),
            millis(counters.max_backlog_frames),
        ))
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

    /// Music volume in upstream `0..=100` units (`music.volume`).
    ///
    /// This is the script-visible master (`musicVolume` upstream), independent of `Settings.ini`:
    /// the mixer receives `music.volume * streamVolume / 100`.
    #[must_use]
    pub fn music_volume(&self) -> u8 {
        self.music_volume
    }

    /// Sets the music volume in upstream `0..=100` units (`music.volume` / `SetMusicVolume`).
    ///
    /// Upstream keeps `masterVolume` (`music.volume`) and `bgmVolume` (from `Settings.ini`)
    /// separately and mixes with their product; this port applies the product to the mixer's
    /// single stream-volume field.
    pub fn set_music_volume_level(&mut self, volume: i32) {
        self.music_volume = volume.clamp(0, i32::from(MAX_VOLUME)) as u8;
        self.apply_stream_volume();
    }

    /// Pushes `music.volume * Settings.ini streamVolume` onto the mixer.
    fn apply_stream_volume(&mut self) {
        let effective =
            u16::from(self.music_volume) * u16::from(self.stream_volume) / u16::from(MAX_VOLUME);
        self.mixer.set_stream_volume_level(effective as u8);
    }

    /// Music stream position in source PCM frames (`music.position`).
    #[must_use]
    pub fn music_position(&self) -> usize {
        self.mixer.stream_position()
    }

    /// SFX volume in upstream `0..=100` units (`engine.sfxVolume`).
    #[must_use]
    pub fn sfx_volume(&self) -> u8 {
        self.mixer.sfx_volume()
    }

    /// Sets the SFX volume in upstream `0..=100` units (`engine.sfxVolume` / `SetGameVolumes`).
    pub fn set_sfx_volume_level(&mut self, volume: i32) {
        self.mixer
            .set_sfx_volume_level(volume.clamp(0, i32::from(MAX_VOLUME)) as u8);
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
    ///
    /// Changing the file drops the track's cached stream (and any recorded load failure), so the
    /// next [`AudioState::play_music`] loads the new file. Like upstream `SetMusicTrack`, this
    /// only writes the track table: music already playing keeps playing until the next
    /// `PlayMusic`. Setting the same file again keeps the cached stream; only the loop flag and
    /// loop point are updated.
    pub fn set_track(&mut self, track: i32, file: &str, looping: bool, loop_point: i32) {
        let Ok(track) = usize::try_from(track) else {
            return;
        };
        let new_file = if file.is_empty() {
            None
        } else {
            Some(format!("Data/Music/{file}"))
        };
        let Some(info) = self.tracks.get_mut(track) else {
            return;
        };
        let file_changed = info.file != new_file;
        if new_file.is_none() {
            *info = TrackInfo::default();
        } else {
            info.file = new_file;
            info.looping = looping;
            info.loop_point = loop_point.max(0);
        }
        if file_changed {
            // The cached stream (or failed path) belongs to the previous file.
            self.stream_cache[track] = None;
            self.stream_failed[track] = None;
        }
    }

    /// `PlayMusic`: loads (once) and starts the stream stored on `track`.
    ///
    /// A track without a file or whose stream fails to load stops the current music, like
    /// upstream, and returns `false`. Returns `true` only when a stream actually started, which
    /// is when upstream sets `trackID`.
    ///
    /// A file-backed play resets `music.volume` to full, exactly like upstream
    /// `AudioLegacy.cpp:49` (`musicVolume = 100` on every `PlayMusic` branch that has a track
    /// file). Scripts fade `music.volume` to `0` before switching tracks (`MusicEvent`), so
    /// without this reset the next `PlayMusic` would start silent. An out-of-range track or one
    /// without a file only stops the music (`StopChannel`), which upstream leaves the volume for.
    pub fn play_music(&mut self, track: i32) -> bool {
        let Some(index) = usize::try_from(track)
            .ok()
            .filter(|index| *index < TRACK_COUNT)
        else {
            self.stop_music();
            return false;
        };
        if self
            .tracks
            .get(index)
            .and_then(|info| info.file.as_ref())
            .is_none()
        {
            self.stop_music();
            return false;
        }
        // Upstream unconditionally sets `musicVolume = 100` after `PlayStream` for a file-backed
        // track, whether or not the stream eventually opens, so reset before loading it.
        self.music_volume = MAX_VOLUME;
        self.apply_stream_volume();
        let Some(id) = self.ensure_stream(index) else {
            self.stop_music();
            return false;
        };
        let info = self.tracks.get(index).cloned().unwrap_or_default();
        if info.looping {
            self.mixer.play_stream(id, info.loop_point);
        } else {
            self.mixer.play_stream(id, -1);
        }
        true
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
    pub fn swap_music_track(&mut self, track: i32, file: &str, loop_point: i32) -> bool {
        if file.is_empty() {
            self.stop_music();
            return false;
        }
        self.set_track(track, file, true, loop_point);
        self.play_music(track)
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

    /// Mixes one engine tick on the single mixer and returns the sample hash.
    ///
    /// With a device attached the mixed samples are submitted through [`AudioEngine`] unless
    /// muted; the submitted buffer is exactly the hashed buffer, so device playback, `--mute`
    /// and headless runs all agree. A prebuffered engine retains ticks the device cannot accept
    /// yet, so a full queue never drops a mixed tick; only a sustained overrun resyncs (counted
    /// in [`AudioState::audio_counters`]). The device is detached only when submission returns a
    /// real error.
    pub fn tick(&mut self) -> [u8; 32] {
        self.scratch.resize(FRAMES_PER_TICK * CHANNELS, 0.0);
        // Always mix on the single engine mixer; the device (when attached and unmuted) receives
        // exactly the buffer that was hashed, so windowed and headless audio are identical.
        let hash = self.mixer.mix_frame(&mut self.scratch, FRAMES_PER_TICK);
        if self.capture {
            self.captured.extend_from_slice(&self.scratch);
        }
        if !self.muted
            && let Some(device) = self.device.as_mut()
            && device.submit(&self.scratch).is_err()
        {
            // A real device error means it is gone: keep mixing locally for the rest of the run.
            // `Ok(0)` (a full queue) is not an error and leaves the device attached.
            self.device = None;
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

    /// Loads the stream for `track`'s current file, reusing the cached stream only when it was
    /// loaded from that same path.
    ///
    /// A changed path always misses the cache (even if [`AudioState::set_track`] did not clear
    /// it), so `PlayMusic` can never replay a stream belonging to a previous file. Load failures
    /// are remembered per path: a path that already failed is not read again until the track's
    /// file changes.
    fn ensure_stream(&mut self, track: usize) -> Option<StreamId> {
        let path = self.tracks.get(track)?.file.clone()?;
        if let Some(Some(cached)) = self.stream_cache.get(track)
            && cached.path == path
        {
            return Some(cached.id);
        }
        if self
            .stream_failed
            .get(track)
            .is_some_and(|failed| failed.as_deref() == Some(path.as_str()))
        {
            return None;
        }
        let source = self.source.clone()?;
        let bytes = match source.read(&path) {
            Ok(bytes) => bytes,
            Err(_) => {
                self.stream_failed[track] = Some(path);
                return None;
            }
        };
        match self.mixer.load_stream(bytes) {
            Ok(id) => {
                self.stream_cache[track] = Some(CachedStream { path, id });
                Some(id)
            }
            Err(_) => {
                self.stream_failed[track] = Some(path);
                None
            }
        }
    }

    /// Takes the samples captured since the last call, leaving the capture buffer empty.
    ///
    /// `--dump-audio` streams the mixed PCM to disk per frame through this: capture stays
    /// enabled but the buffer never grows with the run length. Returns an empty vector while
    /// capture is off, so a caller can drain unconditionally.
    pub fn take_captured_pcm(&mut self) -> Vec<f32> {
        std::mem::take(&mut self.captured)
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
    use retro_audio::MAX_QUEUED_TICKS;
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

    /// Two distinct valid Ogg Vorbis tracks, shared with the `retro-audio` fixtures.
    const TONE: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../retro-audio/tests/fixtures/tone.ogg"
    ));
    const TONE_MONO: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../retro-audio/tests/fixtures/tone_mono.ogg"
    ));

    /// An audio state with two music files at `Data/Music/{a,b}.ogg` for swap tests.
    fn state_with_music() -> (AudioState, Arc<MemorySource>) {
        let mut source = MemorySource::new();
        source.insert("Data/Music/a.ogg", TONE.to_vec());
        source.insert("Data/Music/b.ogg", TONE_MONO.to_vec());
        let source = Arc::new(source);
        let state = AudioState::for_scene(
            Arc::clone(&source) as Arc<dyn DataSource>,
            &game_config(Vec::new()),
            &stage_config(Vec::new()),
            &AudioSettings::default(),
        );
        (state, source)
    }

    #[test]
    fn reset_stage_tracks_clears_music_and_sfx() {
        let (mut state, _) = state_with_sfx();
        state.set_track(3, "Song.ogg", true, 0);
        state.play_sfx(0, false);
        assert!(state.track_file(3).is_some());
        assert!(
            (0..retro_audio::SFX_CHANNEL_COUNT).any(|channel| state
                .mixer
                .channel_sfx(retro_audio::SfxChannel(channel))
                .is_some()),
            "the sfx must be playing before the reset"
        );
        state.reset_stage_tracks();
        assert!(state.track_file(3).is_none(), "tracks are cleared");
        assert!(
            (0..retro_audio::SFX_CHANNEL_COUNT).all(|channel| state
                .mixer
                .channel_sfx(retro_audio::SfxChannel(channel))
                .is_none()),
            "all sfx are stopped"
        );
        assert_eq!(state.music_position(), 0, "the stream is stopped");
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
        // The asset is missing, so the slot's current path is marked failed and nothing plays.
        assert_eq!(
            state.stream_failed[0].as_deref(),
            Some("Data/Music/Theme.ogg")
        );
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
    fn changing_a_tracks_file_loads_and_mixes_the_new_stream() {
        // Repro of the review finding: playing a.ogg, then setting b.ogg and playing again must
        // mix b.ogg, not the cached a.ogg stream.
        let (mut swapped, _) = state_with_music();
        swapped.set_track(0, "a.ogg", false, 0);
        assert!(swapped.play_music(0), "a.ogg loads");
        let a_first = swapped.tick();

        swapped.set_track(0, "b.ogg", false, 0);
        assert!(swapped.play_music(0), "b.ogg loads after the swap");
        let swapped_first = swapped.tick();

        let (mut fresh_a, _) = state_with_music();
        fresh_a.set_track(0, "a.ogg", false, 0);
        assert!(fresh_a.play_music(0));
        let (mut fresh_b, _) = state_with_music();
        fresh_b.set_track(0, "b.ogg", false, 0);
        assert!(fresh_b.play_music(0));

        assert_eq!(
            swapped_first,
            fresh_b.tick(),
            "the swapped state must mix b.ogg exactly"
        );
        assert_ne!(
            swapped_first,
            fresh_a.tick(),
            "the old a.ogg stream must not be replayed"
        );
        assert_ne!(swapped_first, a_first, "b.ogg differs from a.ogg");
        for frame in 1..4 {
            assert_eq!(swapped.tick(), fresh_b.tick(), "frame {frame}");
        }
    }

    #[test]
    fn replaying_the_same_track_reuses_the_cached_stream_and_restarts_it() {
        let (mut state, _) = state_with_music();
        state.set_track(0, "a.ogg", true, 0);
        assert!(state.play_music(0));
        let first_tick = state.tick();
        for _ in 0..3 {
            state.tick();
        }
        assert_ne!(state.music_position(), 0, "playback advanced");

        assert!(state.play_music(0), "the same file plays again");
        assert_eq!(
            state.mixer.stream_count(),
            1,
            "the cached stream is reused, not loaded twice"
        );
        assert_eq!(state.music_position(), 0, "PlayMusic restarts at frame 0");
        assert_eq!(
            state.tick(),
            first_tick,
            "the restart is byte-identical to the first play"
        );
    }

    #[test]
    fn changing_the_file_drops_the_stale_cache_and_failure_state() {
        let (mut state, _) = state_with_music();
        state.set_track(0, "a.ogg", true, 0);
        assert!(state.play_music(0));
        let first_id = state.stream_cache[0].as_ref().map(|cached| cached.id);
        assert_eq!(
            state.stream_cache[0]
                .as_ref()
                .map(|cached| cached.path.as_str()),
            Some("Data/Music/a.ogg")
        );

        state.set_track(0, "b.ogg", true, 0);
        assert!(
            state.stream_cache[0].is_none(),
            "the cached stream belongs to the old file"
        );
        assert!(state.stream_failed[0].is_none());
        assert!(state.play_music(0));
        let second_id = state.stream_cache[0].as_ref().map(|cached| cached.id);
        assert_ne!(first_id, second_id, "the new file loaded a new stream");

        // Setting the same path again keeps the cache; only the loop metadata changes.
        state.set_track(0, "b.ogg", false, 42);
        assert_eq!(
            state.stream_cache[0].as_ref().map(|cached| cached.id),
            second_id,
            "an unchanged path keeps its cached stream"
        );

        // A failed path is remembered, and switching to a new path retries.
        state.set_track(1, "missing.ogg", true, 0);
        assert!(!state.play_music(1));
        assert_eq!(
            state.stream_failed[1].as_deref(),
            Some("Data/Music/missing.ogg")
        );
        state.set_track(1, "a.ogg", true, 0);
        assert!(
            state.stream_failed[1].is_none(),
            "the failure belonged to the old path"
        );
        assert!(state.play_music(1), "a new path must be attempted");
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
        assert_eq!(
            state.music_volume(),
            100,
            "music.volume is the master volume, not the settings volume"
        );
        assert_eq!(
            state.mixer().stream_volume(),
            50,
            "the mixer receives music.volume * streamVolume"
        );
        assert_eq!(state.sfx_volume(), 25);
    }

    #[test]
    fn music_volume_and_position_are_script_writable() {
        let (mut state, _) = state_with_sfx();
        assert_eq!(state.music_volume(), 100, "music.volume defaults to full");
        assert_eq!(
            state.mixer().stream_volume(),
            80,
            "the mixer still applies the Settings.ini streamVolume"
        );
        state.set_music_volume_level(40);
        assert_eq!(state.music_volume(), 40);
        assert_eq!(
            state.mixer().stream_volume(),
            32,
            "a script write scales with the settings volume"
        );
        state.set_music_volume_level(-5);
        assert_eq!(state.music_volume(), 0);
        assert_eq!(state.mixer().stream_volume(), 0);
        state.set_music_volume_level(1000);
        assert_eq!(state.music_volume(), 100);
        assert_eq!(state.mixer().stream_volume(), 80);
        state.set_sfx_volume_level(25);
        assert_eq!(state.sfx_volume(), 25);
        assert_eq!(state.music_position(), 0);
    }

    /// Ticks once with a fresh capture window and returns the mix hash, asserting the tick is
    /// audible (`captured_pcm` holds exactly this tick's non-silent samples).
    fn capture_audible_tick(state: &mut AudioState) -> [u8; 32] {
        state.set_capture(false);
        state.set_capture(true);
        let hash = state.tick();
        assert!(
            state.captured_pcm().iter().any(|sample| *sample != 0.0),
            "the tick must be audible"
        );
        hash
    }

    /// The 50-frame `music.volume -= 2` fade shared by all `MusicEvent` states
    /// (`Data/Scripts/Global/MusicEvent.txt`), from a full 100 down to 0.
    fn fade_music_to_zero(state: &mut AudioState) {
        for _ in 0..50 {
            state.set_music_volume_level(i32::from(state.music_volume()) - 2);
        }
        assert_eq!(state.music_volume(), 0, "the fade must reach silence");
        assert_eq!(state.mixer().stream_volume(), 0);
    }

    /// Sonic 2's EHZ boss sequence: the level track fades to 0, `PlayMusic` switches to the boss
    /// track, the defeat fade reaches 0 again, and `PlayMusic` restores the level track. Upstream
    /// `PlayMusic` resets `musicVolume = 100` (`AudioLegacy.cpp:49`), so both switches must be
    /// audible and byte-identical to a fresh play of the same track.
    #[test]
    fn boss_music_event_fade_then_play_is_audible_again() {
        let (mut state, _) = state_with_music();
        state.set_track(0, "a.ogg", true, 0);
        state.set_track(1, "b.ogg", true, 0);
        assert!(state.play_music(0), "the level track starts");
        capture_audible_tick(&mut state);

        // MUSICEVENT_FADETOBOSS_ACTION: fade the stage music, then PlayMusic(TRACK_BOSS).
        fade_music_to_zero(&mut state);
        state.set_capture(false);
        state.set_capture(true);
        state.tick();
        assert!(
            state.captured_pcm().iter().all(|sample| *sample == 0.0),
            "at music.volume 0 the faded track must be silent"
        );
        assert!(state.play_music(1), "the boss track starts");
        assert_eq!(state.music_volume(), 100, "PlayMusic resets music.volume");
        assert_eq!(
            state.mixer().stream_volume(),
            80,
            "the settings streamVolume still scales the reset master volume"
        );
        let boss_tick = capture_audible_tick(&mut state);
        let (mut fresh, _) = state_with_music();
        fresh.set_track(1, "b.ogg", true, 0);
        assert!(fresh.play_music(1));
        assert_eq!(
            boss_tick,
            fresh.tick(),
            "the boss track must restart byte-identically to a fresh play"
        );

        // MUSICEVENT_FADETOSTAGE_ACTION: fade the boss music, then PlayMusic(TRACK_STAGE).
        fade_music_to_zero(&mut state);
        assert!(state.play_music(0), "the level track starts again");
        assert_eq!(state.music_volume(), 100);
        let stage_tick = capture_audible_tick(&mut state);
        let (mut fresh_stage, _) = state_with_music();
        fresh_stage.set_track(0, "a.ogg", true, 0);
        assert!(fresh_stage.play_music(0));
        assert_eq!(
            stage_tick,
            fresh_stage.tick(),
            "the level track must be audible again after the boss"
        );
    }

    /// Upstream sets `musicVolume = 100` for every file-backed `PlayMusic`, even when the stream
    /// later fails to open; only an empty track (`StopChannel`) keeps the faded volume.
    #[test]
    fn play_music_reset_applies_to_failed_and_empty_tracks_like_upstream() {
        let (mut state, _) = state_with_sfx();
        state.set_track(0, "Missing.ogg", true, 0);
        state.set_music_volume_level(0);
        assert!(!state.play_music(0), "the missing stream reports failure");
        assert_eq!(
            state.music_volume(),
            100,
            "a file-backed play resets music.volume even when the load fails"
        );

        state.set_music_volume_level(0);
        state.set_track(1, "", false, 0);
        assert!(!state.play_music(1), "an empty track stops the music");
        assert_eq!(
            state.music_volume(),
            0,
            "upstream's empty-track branch only calls StopChannel"
        );
    }

    /// `SwapMusicTrack` calls `PlayMusic` upstream (`AudioLegacy.cpp:76`), so it resets as well.
    #[test]
    fn swap_music_track_resets_a_faded_volume() {
        let (mut state, _) = state_with_music();
        state.set_track(0, "a.ogg", true, 0);
        assert!(state.play_music(0));
        fade_music_to_zero(&mut state);
        assert!(state.swap_music_track(1, "b.ogg", 0));
        assert_eq!(state.music_volume(), 100);
        capture_audible_tick(&mut state);

        // StopMusic/PauseMusic/ResumeMusic must not reset (`AudioLegacy.hpp:25`).
        state.set_music_volume_level(0);
        state.stop_music();
        state.pause_music();
        state.resume_music();
        assert_eq!(state.music_volume(), 0);
        assert_eq!(state.mixer().stream_volume(), 0);
    }

    #[test]
    fn play_music_reports_whether_the_stream_loaded() {
        let (mut state, _) = state_with_sfx();
        state.set_track(0, "Missing.ogg", true, 0);
        assert!(!state.play_music(0), "a missing stream must report failure");
        assert!(!state.swap_music_track(1, "Missing.ogg", 0));
        assert!(state.track_file(0).is_some(), "track metadata is kept");
    }

    #[test]
    fn device_attached_output_is_non_silent_and_matches_the_hash() {
        use retro_platform::headless::HeadlessPlatform;
        use retro_platform::{AudioDesc, Platform};

        let mut platform = HeadlessPlatform::new();
        platform.init().unwrap();
        let device = platform
            .open_audio(AudioDesc::stereo(SAMPLE_RATE))
            .expect("headless audio device");

        let (mut local, _) = state_with_sfx();
        let (mut attached, _) = state_with_sfx();
        attached.set_device(AudioEngine::new(device).unwrap());
        for state in [&mut local, &mut attached] {
            state.play_sfx(0, false);
            state.set_capture(true);
        }

        let mut hashes = Vec::new();
        for _ in 0..4 {
            let local_hash = local.tick();
            let attached_hash = attached.tick();
            assert_eq!(
                local_hash, attached_hash,
                "device path must not change the mix"
            );
            hashes.push(local_hash);
        }
        assert_ne!(hashes[0], [0u8; 32]);
        assert_eq!(
            platform.captured_pcm(),
            attached.captured_pcm(),
            "the device received exactly the hashed buffer"
        );
        assert!(
            platform.captured_pcm().iter().any(|sample| *sample != 0.0),
            "device output must be non-silent"
        );
        assert_eq!(local.captured_pcm(), attached.captured_pcm());
    }

    #[test]
    fn muted_device_receives_nothing_but_hashes_identically() {
        use retro_platform::headless::HeadlessPlatform;
        use retro_platform::{AudioDesc, Platform};

        let mut platform = HeadlessPlatform::new();
        platform.init().unwrap();
        let device = platform
            .open_audio(AudioDesc::stereo(SAMPLE_RATE))
            .expect("headless audio device");

        let (mut local, _) = state_with_sfx();
        let (mut muted, _) = state_with_sfx();
        muted.set_device(AudioEngine::new(device).unwrap());
        muted.set_muted(true);
        for state in [&mut local, &mut muted] {
            state.play_sfx(0, false);
        }
        for _ in 0..4 {
            assert_eq!(local.tick(), muted.tick());
        }
        assert!(
            platform.captured_pcm().is_empty(),
            "muted output must not be submitted"
        );
    }

    /// A device whose queue never drains: it reports a huge backlog and accepts nothing.
    struct StalledDevice {
        calls: std::rc::Rc<std::cell::Cell<usize>>,
    }

    impl retro_platform::AudioDevice for StalledDevice {
        fn sample_rate(&self) -> u32 {
            SAMPLE_RATE
        }

        fn channels(&self) -> u8 {
            CHANNELS as u8
        }

        fn submit(&mut self, _frames: &[f32]) -> Result<usize, retro_platform::PlatformError> {
            self.calls.set(self.calls.get() + 1);
            Ok(0)
        }

        fn queued_frames(&self) -> usize {
            usize::MAX / 2
        }

        fn close(&mut self) -> Result<(), retro_platform::PlatformError> {
            Ok(())
        }
    }

    /// A device that reports a real error on every submission.
    struct FailingDevice;

    impl retro_platform::AudioDevice for FailingDevice {
        fn sample_rate(&self) -> u32 {
            SAMPLE_RATE
        }

        fn channels(&self) -> u8 {
            CHANNELS as u8
        }

        fn submit(&mut self, _frames: &[f32]) -> Result<usize, retro_platform::PlatformError> {
            Err(retro_platform::PlatformError::Other(
                "device lost".to_owned(),
            ))
        }

        fn queued_frames(&self) -> usize {
            0
        }

        fn close(&mut self) -> Result<(), retro_platform::PlatformError> {
            Ok(())
        }
    }

    #[test]
    fn tick_with_a_stalled_device_returns_promptly_and_keeps_the_device() {
        let calls = std::rc::Rc::new(std::cell::Cell::new(0));
        let stalled = StalledDevice {
            calls: std::rc::Rc::clone(&calls),
        };
        let (mut local, _) = state_with_sfx();
        let (mut attached, _) = state_with_sfx();
        attached.set_device(AudioEngine::new(Box::new(stalled)).unwrap());
        for state in [&mut local, &mut attached] {
            state.play_sfx(0, false);
        }

        let started = std::time::Instant::now();
        for _ in 0..8 {
            assert_eq!(
                local.tick(),
                attached.tick(),
                "a stalled device must not change the mix hash"
            );
        }
        assert!(
            started.elapsed() < std::time::Duration::from_secs(1),
            "tick must not block against a stalled device"
        );
        assert_eq!(calls.get(), 8, "each tick submits exactly once");
        assert!(
            attached.device.is_some(),
            "a zero acceptance is not an error; the device stays attached"
        );
    }

    /// A prebuffered device whose queue drains slowly, for the hash-stability integration test.
    struct DrainingProbeDevice {
        queued: std::rc::Rc<std::cell::Cell<usize>>,
        started: std::rc::Rc<std::cell::Cell<bool>>,
        submits: std::rc::Rc<std::cell::Cell<usize>>,
        underruns: std::rc::Rc<std::cell::Cell<usize>>,
    }

    impl DrainingProbeDevice {
        fn new() -> (
            Self,
            std::rc::Rc<std::cell::Cell<bool>>,
            std::rc::Rc<std::cell::Cell<usize>>,
        ) {
            let queued = std::rc::Rc::new(std::cell::Cell::new(0));
            let started = std::rc::Rc::new(std::cell::Cell::new(false));
            let submits = std::rc::Rc::new(std::cell::Cell::new(0));
            let device = Self {
                queued: std::rc::Rc::clone(&queued),
                started: std::rc::Rc::clone(&started),
                submits,
                underruns: std::rc::Rc::new(std::cell::Cell::new(0)),
            };
            (device, started, queued)
        }
    }

    impl retro_platform::AudioDevice for DrainingProbeDevice {
        fn sample_rate(&self) -> u32 {
            SAMPLE_RATE
        }

        fn channels(&self) -> u8 {
            CHANNELS as u8
        }

        fn submit(&mut self, frames: &[f32]) -> Result<usize, retro_platform::PlatformError> {
            if self.started.get() {
                let drain = if self.submits.get().is_multiple_of(3) {
                    FRAMES_PER_TICK
                } else {
                    0
                };
                if drain > self.queued.get() {
                    self.underruns.set(self.underruns.get() + 1);
                }
                self.queued.set(self.queued.get().saturating_sub(drain));
            }
            let total = frames.len() / CHANNELS;
            self.queued.set(self.queued.get() + total);
            self.submits.set(self.submits.get() + 1);
            Ok(total)
        }

        fn queued_frames(&self) -> usize {
            self.queued.get()
        }

        fn start(&mut self) -> Result<(), retro_platform::PlatformError> {
            self.started.set(true);
            Ok(())
        }

        fn close(&mut self) -> Result<(), retro_platform::PlatformError> {
            Ok(())
        }
    }

    #[test]
    fn prebuffered_draining_device_keeps_the_pcm_hash_and_queue_bounded() {
        let (device, started, queued) = DrainingProbeDevice::new();
        let (mut local, _) = state_with_sfx();
        let (mut attached, _) = state_with_sfx();
        attached.set_device(
            AudioEngine::with_prebuffer(Box::new(device)).expect("prebuffered audio engine"),
        );
        for state in [&mut local, &mut attached] {
            state.play_sfx(0, false);
            state.set_capture(true);
        }

        let mut hashes = Vec::new();
        for frame in 0..30 {
            assert_eq!(
                local.tick(),
                attached.tick(),
                "frame {frame}: device queueing must not change the mix hash"
            );
            hashes.push(local.last_hash_hex());
            assert!(
                queued.get() <= MAX_QUEUED_TICKS * FRAMES_PER_TICK,
                "frame {frame}: queue above the cap"
            );
        }
        assert_ne!(
            hashes[0],
            "0".repeat(64),
            "the scripted run must be audible"
        );
        assert!(
            started.get(),
            "the prebuffered engine must start the device"
        );
        assert_eq!(
            local.captured_pcm(),
            attached.captured_pcm(),
            "the device receives the hashed samples unchanged"
        );
        // This probe drains at one third of real time, so the engine must have resynced rather
        // than silently skipping: the gap is counted and reported.
        let counters = attached.audio_counters().expect("device attached");
        assert!(counters.resyncs > 0, "a slow device must resync");
        assert!(
            counters.dropped_frames > 0,
            "resyncs must count the frames they trimmed"
        );
        let report = attached.audio_diagnostics().expect("device attached");
        assert!(report.contains("dropped "), "{report}");
    }

    #[test]
    fn tick_detaches_the_device_only_on_a_real_error() {
        let (mut state, _) = state_with_sfx();
        state.set_device(AudioEngine::new(Box::new(FailingDevice)).unwrap());
        state.play_sfx(0, false);
        let hashes: Vec<[u8; 32]> = (0..4).map(|_| state.tick()).collect();
        assert!(
            state.device.is_none(),
            "a device error must detach the device"
        );

        // Mixing continues and stays deterministic after the device is gone.
        let (mut reference, _) = state_with_sfx();
        reference.play_sfx(0, false);
        let expected: Vec<[u8; 32]> = (0..4).map(|_| reference.tick()).collect();
        assert_eq!(hashes, expected);
    }
}
