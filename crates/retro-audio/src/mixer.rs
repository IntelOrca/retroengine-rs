//! Deterministic port of the RSDKv4 audio mixer (`RSDKv4/Audio.cpp`).
//!
//! The mixer keeps upstream's structure: SFX are converted to interleaved stereo 16-bit
//! samples at 44.1 kHz when loaded, sixteen voice channels are allocated by scanning for a
//! free or same-sound channel, and every mixed frame is accumulated in `i32`, clamped to
//! `i16` and emitted as `f32`. Music streams are decoded on demand by [`crate::stream`] with a
//! bounded lookahead ring and stepped through with a fixed-point source cursor, so looping
//! matches `ov_pcm_seek(loopPoint)` without holding the whole track.
//!
//! # Deliberate deviations from upstream
//!
//! * When all sixteen voices are busy upstream reads `sfxChannels[-1]` (undefined behaviour);
//!   this port instead steals channels round-robin starting at slot 0
//!   ([`Mixer::play_sfx`]).
//! * Upstream's `ProcessAudioPlayback` clamps `sizeof(mix_buffer)` entries per iteration even
//!   when only `samples_to_do` were mixed, re-clamping stale accumulator data; this port clamps
//!   exactly the frames produced by [`Mixer::mix_frame`].
//! * Upstream has no per-voice volume (`PlaySfx` only sets the loop flag and pan; `sfxVolume`
//!   scales every voice at mix time). [`Mixer::play_sfx`]'s `volume` argument is a non-upstream
//!   extension used by the engine; it is applied before the global `sfxVolume` and each step
//!   truncates toward zero, so a per-voice volume can differ from upstream by a sample unit.

use blake3::Hasher;

use crate::decode;
use crate::stream::VorbisStream;
use crate::{AudioError, CHANNELS, MAX_VOLUME, SAMPLE_RATE, SFX_CHANNEL_COUNT, SFX_COUNT};

/// Identifies a loaded sound effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SfxId(pub usize);

/// Identifies a loaded music stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StreamId(pub usize);

/// Identifies an SFX voice slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SfxChannel(pub usize);

/// A decoded sound effect, already converted to the device format.
struct SfxEntry {
    name: String,
    samples: Vec<i16>,
}

/// One of the sixteen upstream `ChannelInfo` slots.
#[derive(Clone, Copy, Default)]
struct Voice {
    sfx: Option<usize>,
    position: usize,
    volume: u8,
    pan: i8,
    looping: bool,
}

/// A music stream decoded on demand with a bounded lookahead ring.
struct StreamEntry {
    decoder: VorbisStream,
    channels: usize,
    sample_rate: u32,
    step: u64,
}

impl StreamEntry {
    fn new(decoder: VorbisStream) -> Self {
        let channels = decoder.channels();
        let sample_rate = decoder.sample_rate();
        let step = (u64::from(sample_rate) << 32) / u64::from(SAMPLE_RATE);
        Self {
            decoder,
            channels,
            sample_rate,
            step,
        }
    }

    /// Number of source PCM frames (granule-accurate).
    fn frames(&self) -> usize {
        self.decoder.frames()
    }

    /// Linearly interpolates the stereo frame at a fixed-point source position.
    fn frame_at(&mut self, position: u64) -> (i32, i32) {
        self.decoder.frame_at(position)
    }
}

/// The RSDKv4 software mixer.
///
/// Loading decodes assets immediately; [`Mixer::mix_frame`] then advances voices and the
/// stream by exactly the requested number of frames. The mixer is deterministic: identical
/// inputs and call sequences produce identical output and hashes.
pub struct Mixer {
    sfx_bank: Vec<SfxEntry>,
    voices: [Voice; SFX_CHANNEL_COUNT],
    steal: usize,
    streams: Vec<StreamEntry>,
    current_stream: Option<usize>,
    stream_playing: bool,
    stream_paused: bool,
    stream_position: u64,
    stream_loop: Option<usize>,
    sfx_volume: u8,
    stream_volume: u8,
}

impl Mixer {
    /// Creates an empty mixer with both volume groups at full volume.
    #[must_use]
    pub fn new() -> Self {
        Self {
            sfx_bank: Vec::new(),
            voices: [Voice::default(); SFX_CHANNEL_COUNT],
            steal: 0,
            streams: Vec::new(),
            current_stream: None,
            stream_playing: false,
            stream_paused: false,
            stream_position: 0,
            stream_loop: None,
            sfx_volume: MAX_VOLUME,
            stream_volume: MAX_VOLUME,
        }
    }

    /// Sets the music volume as a normalized `0.0..=1.0` value (`bgmVolume` from
    /// `Settings.ini`, where 1.0 maps to `MAX_VOLUME`).
    pub fn set_stream_volume(&mut self, volume: f32) {
        self.stream_volume = normalized_volume(volume);
    }

    /// Sets the SFX volume as a normalized `0.0..=1.0` value (`sfxVolume` from `Settings.ini`,
    /// where 1.0 maps to `MAX_VOLUME`).
    pub fn set_sfx_volume(&mut self, volume: f32) {
        self.sfx_volume = normalized_volume(volume);
    }

    /// Sets the music volume directly in upstream `0..=MAX_VOLUME` units (`SetMusicVolume`,
    /// the script-visible `music.volume`).
    pub fn set_stream_volume_level(&mut self, volume: u8) {
        self.stream_volume = volume.min(MAX_VOLUME);
    }

    /// Sets the SFX volume directly in upstream `0..=MAX_VOLUME` units (`SetGameVolumes`,
    /// the script-visible `engine.sfxVolume`).
    pub fn set_sfx_volume_level(&mut self, volume: u8) {
        self.sfx_volume = volume.min(MAX_VOLUME);
    }

    /// Current music stream position in source PCM frames (`musicPosition` / `ov_pcm_tell`).
    ///
    /// Returns `0` while no stream is playing; a paused stream keeps its position.
    #[must_use]
    pub fn stream_position(&self) -> usize {
        (self.stream_position >> 32) as usize
    }

    /// Returns the music volume in upstream `0..=MAX_VOLUME` units.
    #[must_use]
    pub fn stream_volume(&self) -> u8 {
        self.stream_volume
    }

    /// Returns the SFX volume in upstream `0..=MAX_VOLUME` units.
    #[must_use]
    pub fn sfx_volume(&self) -> u8 {
        self.sfx_volume
    }

    /// Decodes and stores a WAV or Ogg Vorbis sound effect (`LoadSfx` upstream).
    ///
    /// Returns an error if the container is malformed, unsupported, or all 256 slots are in use.
    pub fn load_sfx(&mut self, name: &str, bytes: &[u8]) -> Result<SfxId, AudioError> {
        if self.sfx_bank.len() >= SFX_COUNT {
            return Err(AudioError::Unsupported(format!(
                "sfx slot limit of {SFX_COUNT} reached"
            )));
        }
        let decoded = decode::decode(bytes)?;
        let samples = decode::to_stereo_44100(&decoded);
        let id = SfxId(self.sfx_bank.len());
        self.sfx_bank.push(SfxEntry {
            name: name.to_owned(),
            samples,
        });
        Ok(id)
    }

    /// Starts a sound effect on a voice channel (`PlaySfx`/`SetSfxAttributes` upstream).
    ///
    /// Allocation scans channels in order and picks the first channel that is free or already
    /// playing `id`, matching upstream. When all sixteen channels are busy the longest-lived
    /// slot is reused round-robin; upstream's `sfxChannels[-1]` out-of-bounds access on this
    /// path is not reproduced.
    pub fn play_sfx(&mut self, id: SfxId, volume: u8, pan: i8) -> SfxChannel {
        let channel = self.allocate_channel(id.0);
        let loaded = self.sfx_bank.get(id.0).is_some();
        let voice = &mut self.voices[channel];
        voice.sfx = if loaded { Some(id.0) } else { None };
        voice.position = 0;
        voice.volume = volume.min(MAX_VOLUME);
        voice.pan = pan.clamp(-100, 100);
        voice.looping = false;
        SfxChannel(channel)
    }

    /// Sets whether a voice restarts when it reaches the end of its sample (`loopSFX`).
    pub fn set_sfx_loop(&mut self, channel: SfxChannel, looping: bool) {
        if let Some(voice) = self.voices.get_mut(channel.0) {
            voice.looping = looping;
        }
    }

    /// Stops the voice on `channel` (`StopSfx` upstream).
    pub fn stop_sfx(&mut self, channel: SfxChannel) {
        if let Some(voice) = self.voices.get_mut(channel.0) {
            *voice = Voice::default();
        }
    }

    /// Stops every SFX voice (`StopAllSfx` upstream).
    pub fn stop_all_sfx(&mut self) {
        self.voices = [Voice::default(); SFX_CHANNEL_COUNT];
    }

    /// Stops every voice currently playing `id` (`StopSfx` upstream).
    pub fn stop_sfx_id(&mut self, id: SfxId) {
        for voice in &mut self.voices {
            if voice.sfx == Some(id.0) {
                *voice = Voice::default();
            }
        }
    }

    /// Updates the loop flag and pan of every voice playing `id` (`SetSfxAttributes` upstream).
    ///
    /// `loop_count` of `-1` keeps the current loop flag, matching the upstream
    /// `loopCount == -1 ? loopSFX : loopCount` assignment. Pan is clamped to `-100..=100` like
    /// [`Mixer::play_sfx`].
    pub fn set_sfx_attributes(&mut self, id: SfxId, loop_count: i32, pan: i8) {
        for voice in &mut self.voices {
            if voice.sfx == Some(id.0) {
                if loop_count != -1 {
                    voice.looping = loop_count != 0;
                }
                voice.pan = pan.clamp(-100, 100);
            }
        }
    }

    /// Loads an Ogg Vorbis music stream (`LoadMusic` upstream).
    ///
    /// Only the headers and Ogg page table are read here; audio packets are decoded on demand
    /// with a bounded lookahead ring, so load time and memory do not scale with track length.
    pub fn load_stream(&mut self, bytes: Vec<u8>) -> Result<StreamId, AudioError> {
        if !bytes.starts_with(b"OggS") {
            return Err(AudioError::Unsupported(
                "music streams must be Ogg Vorbis".to_owned(),
            ));
        }
        let decoder = VorbisStream::new(bytes)?;
        let id = StreamId(self.streams.len());
        self.streams.push(StreamEntry::new(decoder));
        Ok(id)
    }

    /// Starts a stream from its beginning (`PlayMusic` upstream).
    ///
    /// `loop_point` is a source PCM frame index. A value of `0` or greater loops back to that
    /// frame forever; a negative value plays the stream once and stops at the end. Upstream
    /// keeps the loop flag and loop point separately (`TrackInfo::trackLoop`); the negative
    /// value encodes "no loop" here.
    pub fn play_stream(&mut self, id: StreamId, loop_point: i32) {
        let Some(stream) = self.streams.get(id.0) else {
            return;
        };
        let frames = stream.frames();
        let loop_point = if loop_point >= 0 && frames > 0 {
            Some((loop_point as usize).min(frames - 1))
        } else {
            None
        };
        self.current_stream = Some(id.0);
        self.stream_playing = frames > 0;
        self.stream_paused = false;
        self.stream_position = 0;
        self.stream_loop = loop_point;
    }

    /// Stops and rewinds the current stream (`StopMusic` upstream).
    pub fn stop_stream(&mut self) {
        self.current_stream = None;
        self.stream_playing = false;
        self.stream_paused = false;
        self.stream_position = 0;
        self.stream_loop = None;
    }

    /// Pauses the current stream (`PauseSound` upstream); mixing emits silence but keeps position.
    pub fn pause_stream(&mut self) {
        if self.stream_playing {
            self.stream_paused = true;
        }
    }

    /// Resumes a paused stream (`ResumeSound` upstream).
    pub fn resume_stream(&mut self) {
        if self.stream_playing && self.stream_paused {
            self.stream_paused = false;
        }
    }

    /// Whether a stream is currently loaded for playback.
    #[must_use]
    pub fn stream_playing(&self) -> bool {
        self.stream_playing
    }

    /// Whether stream playback is paused.
    #[must_use]
    pub fn stream_paused(&self) -> bool {
        self.stream_paused
    }

    /// Number of loaded sound effects.
    #[must_use]
    pub fn sfx_count(&self) -> usize {
        self.sfx_bank.len()
    }

    /// Number of loaded streams.
    #[must_use]
    pub fn stream_count(&self) -> usize {
        self.streams.len()
    }

    /// Name recorded for a loaded sound effect.
    #[must_use]
    pub fn sfx_name(&self, id: SfxId) -> Option<&str> {
        self.sfx_bank.get(id.0).map(|entry| entry.name.as_str())
    }

    /// Number of stereo frames in a loaded sound effect at [`SAMPLE_RATE`].
    #[must_use]
    pub fn sfx_frames(&self, id: SfxId) -> Option<usize> {
        self.sfx_bank
            .get(id.0)
            .map(|entry| entry.samples.len() / CHANNELS)
    }

    /// Number of source frames in a loaded stream.
    #[must_use]
    pub fn stream_frames(&self, id: StreamId) -> Option<usize> {
        self.streams.get(id.0).map(StreamEntry::frames)
    }

    /// Source sample rate of a loaded stream.
    #[must_use]
    pub fn stream_sample_rate(&self, id: StreamId) -> Option<u32> {
        self.streams.get(id.0).map(|entry| entry.sample_rate)
    }

    /// Source channel count of a loaded stream.
    #[must_use]
    pub fn stream_channels(&self, id: StreamId) -> Option<usize> {
        self.streams.get(id.0).map(|entry| entry.channels)
    }

    /// Sound effect currently assigned to a voice channel.
    #[must_use]
    pub fn channel_sfx(&self, channel: SfxChannel) -> Option<SfxId> {
        self.voices
            .get(channel.0)
            .and_then(|voice| voice.sfx)
            .map(SfxId)
    }

    /// Current playback position of a voice channel in stereo frames.
    #[must_use]
    pub fn channel_position(&self, channel: SfxChannel) -> Option<usize> {
        self.voices
            .get(channel.0)
            .filter(|voice| voice.sfx.is_some())
            .map(|voice| voice.position)
    }

    /// Mixes exactly `frames` stereo sample-frames into `out` (`ProcessAudioPlayback` upstream).
    ///
    /// `out` is interpreted as interleaved stereo; if it is shorter than `frames * 2` samples
    /// only the frames that fit are mixed. Returns a BLAKE3 hash of the mixed `f32` samples.
    pub fn mix_frame(&mut self, out: &mut [f32], frames: usize) -> [u8; 32] {
        let frames = frames.min(out.len() / CHANNELS);
        let mixed = &mut out[..frames * CHANNELS];
        for frame in mixed.as_chunks_mut::<CHANNELS>().0 {
            let mut left = 0i32;
            let mut right = 0i32;
            if self.stream_playing && !self.stream_paused {
                self.mix_stream(&mut left, &mut right);
            }
            for index in 0..SFX_CHANNEL_COUNT {
                self.mix_voice(index, &mut left, &mut right);
            }
            frame[0] = to_f32(left);
            frame[1] = to_f32(right);
        }
        hash_samples(mixed)
    }

    /// Finds the upstream channel: first free or already playing `sfx`, else round-robin steal.
    fn allocate_channel(&mut self, sfx: usize) -> usize {
        for (index, voice) in self.voices.iter().enumerate() {
            if voice.sfx.is_none() || voice.sfx == Some(sfx) {
                return index;
            }
        }
        let channel = self.steal;
        self.steal = (self.steal + 1) % SFX_CHANNEL_COUNT;
        channel
    }

    /// Mixes one stream frame into the accumulator, applying the stream volume.
    fn mix_stream(&mut self, left: &mut i32, right: &mut i32) {
        let Some(index) = self.current_stream else {
            return;
        };
        let Some(stream) = self.streams.get_mut(index) else {
            self.stream_playing = false;
            return;
        };
        let frames = stream.frames();
        if frames == 0 {
            self.stream_playing = false;
            return;
        }
        if (self.stream_position >> 32) as usize >= frames {
            match self.stream_loop {
                Some(loop_point) => self.stream_position = (loop_point as u64) << 32,
                None => {
                    self.stream_playing = false;
                    return;
                }
            }
        }
        let (sample_left, sample_right) = stream.frame_at(self.stream_position);
        let volume = i32::from(self.stream_volume);
        *left += sample_left * volume / i32::from(MAX_VOLUME);
        *right += sample_right * volume / i32::from(MAX_VOLUME);
        self.stream_position += stream.step;
    }

    /// Mixes one voice frame into the accumulator (`ProcessAudioMixing` upstream).
    fn mix_voice(&mut self, index: usize, left: &mut i32, right: &mut i32) {
        let Some(sfx_index) = self.voices[index].sfx else {
            return;
        };
        let Some(entry) = self.sfx_bank.get(sfx_index) else {
            self.voices[index] = Voice::default();
            return;
        };
        let frames = entry.samples.len() / CHANNELS;
        if self.voices[index].position >= frames {
            if self.voices[index].looping && frames > 0 {
                self.voices[index].position = 0;
            } else {
                self.voices[index] = Voice::default();
                return;
            }
        }

        let position = self.voices[index].position;
        let mut sample_left = i32::from(entry.samples[position * CHANNELS]);
        let mut sample_right = i32::from(entry.samples[position * CHANNELS + 1]);

        let voice_volume = i32::from(self.voices[index].volume) * i32::from(self.sfx_volume)
            / i32::from(MAX_VOLUME);
        sample_left = sample_left * voice_volume / i32::from(MAX_VOLUME);
        sample_right = sample_right * voice_volume / i32::from(MAX_VOLUME);

        let pan = i32::from(self.voices[index].pan);
        if pan < 0 {
            let attenuation = 1.0 - (pan as f32 / 100.0).abs();
            sample_right = (sample_right as f32 * attenuation) as i32;
        } else if pan > 0 {
            let attenuation = 1.0 - (pan as f32 / 100.0).abs();
            sample_left = (sample_left as f32 * attenuation) as i32;
        }

        *left += sample_left;
        *right += sample_right;
        self.voices[index].position += 1;
    }
}

impl Default for Mixer {
    fn default() -> Self {
        Self::new()
    }
}

fn normalized_volume(volume: f32) -> u8 {
    if volume.is_nan() {
        return 0;
    }
    (volume.clamp(0.0, 1.0) * f32::from(MAX_VOLUME)).round() as u8
}

/// Clamps an accumulator sample to the i16 range and normalizes it to `f32`.
fn to_f32(sample: i32) -> f32 {
    sample.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as f32 / 32768.0
}

/// Hashes interleaved `f32` samples as little-endian bytes.
fn hash_samples(samples: &[f32]) -> [u8; 32] {
    let mut hasher = Hasher::new();
    let mut bytes = [0u8; 4];
    for sample in samples {
        bytes.copy_from_slice(&sample.to_le_bytes());
        hasher.update(&bytes);
    }
    *hasher.finalize().as_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FRAMES_PER_TICK;
    use crate::wav::build_wav;

    const TONE: &[u8] = include_bytes!("../tests/fixtures/tone.ogg");

    fn wav_stereo(samples: &[i16]) -> Vec<u8> {
        let data: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        build_wav(1, 2, SAMPLE_RATE, 16, &data)
    }

    fn wav_mono(samples: &[i16]) -> Vec<u8> {
        let data: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        build_wav(1, 1, SAMPLE_RATE, 16, &data)
    }

    fn load_stereo(mixer: &mut Mixer, samples: &[i16]) -> SfxId {
        mixer.load_sfx("test", &wav_stereo(samples)).unwrap()
    }

    fn hash_of(samples: &[f32]) -> [u8; 32] {
        hash_samples(samples)
    }

    #[test]
    fn silence_in_silence_out() {
        let mut mixer = Mixer::new();
        let mut out = vec![0.0f32; 16];
        let hash = mixer.mix_frame(&mut out, 8);
        assert!(out.iter().all(|s| *s == 0.0));
        assert_eq!(hash, hash_of(&[0.0; 16]));
    }

    #[test]
    fn single_voice_full_volume_kat() {
        let mut mixer = Mixer::new();
        let id = load_stereo(
            &mut mixer,
            &[1000, -1000, 2000, -2000, 3000, -3000, 4000, -4000],
        );
        mixer.play_sfx(id, 100, 0);
        let mut out = vec![0.0f32; 8];
        mixer.mix_frame(&mut out, 4);
        let expected = [
            1000.0 / 32768.0,
            -1000.0 / 32768.0,
            2000.0 / 32768.0,
            -2000.0 / 32768.0,
            3000.0 / 32768.0,
            -3000.0 / 32768.0,
            4000.0 / 32768.0,
            -4000.0 / 32768.0,
        ];
        assert_eq!(out, expected);
    }

    #[test]
    fn volume_truncates_toward_zero_like_upstream() {
        let mut mixer = Mixer::new();
        let id = load_stereo(&mut mixer, &[1001, -1001, 100, -100]);
        mixer.play_sfx(id, 50, 0);
        let mut out = vec![0.0f32; 4];
        mixer.mix_frame(&mut out, 2);
        assert_eq!(out[0], 500.0 / 32768.0);
        assert_eq!(out[1], -500.0 / 32768.0);
        assert_eq!(out[2], 50.0 / 32768.0);
        assert_eq!(out[3], -50.0 / 32768.0);
    }

    #[test]
    fn pan_attenuates_the_opposite_channel() {
        let mut mixer = Mixer::new();
        let id = load_stereo(&mut mixer, &[1000, 1000, 1000, 1000]);
        mixer.play_sfx(id, 100, -100);
        let mut out = vec![0.0f32; 2];
        mixer.mix_frame(&mut out, 1);
        assert_eq!(out[0], 1000.0 / 32768.0);
        assert_eq!(out[1], 0.0);

        let mut mixer = Mixer::new();
        let id = load_stereo(&mut mixer, &[1000, 1000, 1000, 1000]);
        mixer.play_sfx(id, 100, 50);
        let mut out = vec![0.0f32; 2];
        mixer.mix_frame(&mut out, 1);
        assert_eq!(out[0], 500.0 / 32768.0);
        assert_eq!(out[1], 1000.0 / 32768.0);
    }

    #[test]
    fn mono_sfx_is_duplicated_to_stereo() {
        let mut mixer = Mixer::new();
        let id = mixer.load_sfx("mono", &wav_mono(&[1000, -2000])).unwrap();
        mixer.play_sfx(id, 100, 0);
        let mut out = vec![0.0f32; 4];
        mixer.mix_frame(&mut out, 2);
        assert_eq!(
            out,
            [
                1000.0 / 32768.0,
                1000.0 / 32768.0,
                -2000.0 / 32768.0,
                -2000.0 / 32768.0
            ]
        );
    }

    #[test]
    fn voice_ends_and_frees_its_channel() {
        let mut mixer = Mixer::new();
        let id = load_stereo(&mut mixer, &[1000, 1000]);
        let channel = mixer.play_sfx(id, 100, 0);
        let mut out = vec![0.0f32; 8];
        mixer.mix_frame(&mut out, 4);
        assert_eq!(mixer.channel_sfx(channel), None);
        assert_eq!(out[0], 1000.0 / 32768.0);
        assert_eq!(out[1], 1000.0 / 32768.0);
        assert!(out[2..].iter().all(|s| *s == 0.0));
    }

    #[test]
    fn looping_sfx_wraps_to_start() {
        let mut mixer = Mixer::new();
        let id = load_stereo(&mut mixer, &[1000, 1000, 2000, 2000]);
        let channel = mixer.play_sfx(id, 100, 0);
        mixer.set_sfx_loop(channel, true);
        let mut out = vec![0.0f32; 8];
        mixer.mix_frame(&mut out, 4);
        assert_eq!(out[0], 1000.0 / 32768.0);
        assert_eq!(out[2], 2000.0 / 32768.0);
        assert_eq!(out[4], 1000.0 / 32768.0);
        assert_eq!(out[6], 2000.0 / 32768.0);
        assert_eq!(mixer.channel_sfx(channel), Some(id));
    }

    #[test]
    fn same_sfx_reuses_its_channel_and_free_channels_come_first() {
        let mut mixer = Mixer::new();
        let first = load_stereo(&mut mixer, &[1000, 1000, 1000, 1000]);
        let second = load_stereo(&mut mixer, &[2000, 2000, 2000, 2000]);
        let third = load_stereo(&mut mixer, &[3000, 3000, 3000, 3000]);

        let a = mixer.play_sfx(first, 100, 0);
        let b = mixer.play_sfx(second, 100, 0);
        assert_eq!(a, SfxChannel(0));
        assert_eq!(b, SfxChannel(1));

        let again = mixer.play_sfx(first, 100, 0);
        assert_eq!(again, SfxChannel(0));

        mixer.stop_sfx(a);
        let c = mixer.play_sfx(third, 100, 0);
        assert_eq!(c, SfxChannel(0));
    }

    #[test]
    fn exhausted_channels_steal_round_robin() {
        let mut mixer = Mixer::new();
        let mut ids = Vec::new();
        for index in 0..18 {
            let sample = (index as i16 + 1) * 100;
            ids.push(load_stereo(&mut mixer, &[sample, sample]));
        }
        for id in &ids[..16] {
            mixer.play_sfx(*id, 100, 0);
        }
        let stolen_first = mixer.play_sfx(ids[16], 100, 0);
        assert_eq!(stolen_first, SfxChannel(0));
        assert_eq!(mixer.channel_sfx(SfxChannel(0)), Some(ids[16]));
        let stolen_second = mixer.play_sfx(ids[17], 100, 0);
        assert_eq!(stolen_second, SfxChannel(1));
        assert_eq!(mixer.channel_sfx(SfxChannel(1)), Some(ids[17]));
    }

    #[test]
    fn stop_and_stop_all_silence_voices() {
        let mut mixer = Mixer::new();
        let first = load_stereo(&mut mixer, &[1000, 1000, 1000, 1000]);
        let second = load_stereo(&mut mixer, &[2000, 2000, 2000, 2000]);
        let a = mixer.play_sfx(first, 100, 0);
        mixer.play_sfx(second, 100, 0);
        mixer.stop_sfx(a);
        let mut out = vec![0.0f32; 4];
        mixer.mix_frame(&mut out, 2);
        assert_eq!(out, [2000.0 / 32768.0; 4]);

        mixer.stop_all_sfx();
        mixer.mix_frame(&mut out, 2);
        assert_eq!(out, [0.0; 4]);
    }

    #[test]
    fn stop_sfx_id_stops_every_matching_voice() {
        let mut mixer = Mixer::new();
        let first = load_stereo(&mut mixer, &[1000, 1000, 1000, 1000]);
        let second = load_stereo(&mut mixer, &[2000, 2000, 2000, 2000]);
        mixer.play_sfx(first, 100, 0);
        mixer.play_sfx(second, 100, 0);
        // Re-playing an id reuses its channel, so there is one voice per id here.
        mixer.play_sfx(first, 100, 0);
        mixer.stop_sfx_id(second);
        let mut out = vec![0.0f32; 4];
        mixer.mix_frame(&mut out, 2);
        assert_eq!(out, [1000.0 / 32768.0; 4]);
        mixer.stop_sfx_id(first);
        mixer.mix_frame(&mut out, 2);
        assert_eq!(out, [0.0; 4]);
    }

    #[test]
    fn set_sfx_attributes_updates_loop_and_pan() {
        let mut mixer = Mixer::new();
        let id = load_stereo(&mut mixer, &[1000, 1000, 1000, 1000]);
        let channel = mixer.play_sfx(id, 100, 0);
        assert_eq!(mixer.channel_position(channel), Some(0));

        // `-1` keeps the loop flag, any other value sets it.
        mixer.set_sfx_attributes(id, -1, -100);
        let mut out = vec![0.0f32; 2];
        mixer.mix_frame(&mut out, 1);
        assert_eq!(out, [1000.0 / 32768.0, 0.0]);

        mixer.set_sfx_attributes(id, 0, 100);
        mixer.mix_frame(&mut out, 1);
        assert_eq!(out, [0.0, 1000.0 / 32768.0]);

        // Loop flag set: the voice restarts instead of ending.
        mixer.set_sfx_attributes(id, 1, 0);
        let mut long = vec![0.0f32; 16];
        mixer.mix_frame(&mut long, 8);
        assert_eq!(mixer.channel_sfx(channel), Some(id));
        assert_eq!(long[0], 1000.0 / 32768.0);
        assert_eq!(long[8], 1000.0 / 32768.0);
    }

    #[test]
    fn mix_frame_clamps_to_the_output_buffer() {
        let mut mixer = Mixer::new();
        let id = load_stereo(&mut mixer, &[1000, 1000, 2000, 2000]);
        mixer.play_sfx(id, 100, 0);
        let mut out = vec![9.0f32; 3];
        mixer.mix_frame(&mut out, 10);
        assert_eq!(out[0], 1000.0 / 32768.0);
        assert_eq!(out[1], 1000.0 / 32768.0);
        assert_eq!(out[2], 9.0);
    }

    #[test]
    fn stream_pause_holds_position_and_resume_continues() {
        let mut paused = Mixer::new();
        let id = paused.load_stream(TONE.to_vec()).unwrap();
        paused.play_stream(id, 0);
        let mut first = vec![0.0f32; 200];
        paused.mix_frame(&mut first, 100);
        paused.pause_stream();
        assert!(paused.stream_paused());
        let mut silent = vec![0.0f32; 100];
        paused.mix_frame(&mut silent, 50);
        assert!(silent.iter().all(|s| *s == 0.0));
        paused.resume_stream();
        assert!(!paused.stream_paused());
        let mut resumed = vec![0.0f32; 200];
        paused.mix_frame(&mut resumed, 100);

        let mut reference = Mixer::new();
        let id = reference.load_stream(TONE.to_vec()).unwrap();
        reference.play_stream(id, 0);
        let mut straight = vec![0.0f32; 400];
        reference.mix_frame(&mut straight, 200);
        assert_eq!(resumed, straight[200..]);
    }

    #[test]
    fn stream_stops_at_end_without_loop() {
        let mut mixer = Mixer::new();
        let id = mixer.load_stream(TONE.to_vec()).unwrap();
        let frames = mixer.stream_frames(id).unwrap();
        mixer.play_stream(id, -1);
        let mut out = vec![0.0f32; (frames + 64) * 2];
        mixer.mix_frame(&mut out, frames + 64);
        assert!(!mixer.stream_playing());
        assert!(out[frames * 2..].iter().all(|s| *s == 0.0));
    }

    #[test]
    fn stream_loop_wraps_exactly() {
        let mut mixer = Mixer::new();
        let id = mixer.load_stream(TONE.to_vec()).unwrap();
        let frames = mixer.stream_frames(id).unwrap();
        mixer.play_stream(id, 0);
        let mut first = vec![0.0f32; frames * 2];
        let first_hash = mixer.mix_frame(&mut first, frames);
        let mut second = vec![0.0f32; frames * 2];
        let second_hash = mixer.mix_frame(&mut second, frames);
        assert_eq!(first_hash, second_hash);
        assert_eq!(first, second);
        assert!(mixer.stream_playing());
    }

    #[test]
    fn stream_loop_point_seeks_to_the_requested_frame() {
        let mut mixer = Mixer::new();
        let id = mixer.load_stream(TONE.to_vec()).unwrap();
        let frames = mixer.stream_frames(id).unwrap();
        let loop_point = frames / 2;
        mixer.play_stream(id, loop_point as i32);
        let mut full = vec![0.0f32; frames * 2];
        mixer.mix_frame(&mut full, frames);
        let count = frames - loop_point;
        let mut wrapped = vec![0.0f32; count * 2];
        mixer.mix_frame(&mut wrapped, count);
        assert_eq!(wrapped, full[loop_point * 2..frames * 2]);
    }

    #[test]
    fn determinism_same_inputs_same_hashes() {
        fn run(volume: f32) -> ([u8; 32], [u8; 32]) {
            let mut mixer = Mixer::new();
            mixer.set_stream_volume(volume);
            let id = mixer.load_stream(TONE.to_vec()).unwrap();
            mixer.play_stream(id, 0);
            let mut out = vec![0.0f32; 128];
            let first = mixer.mix_frame(&mut out, 64);
            let second = mixer.mix_frame(&mut out, 64);
            (first, second)
        }
        let (a1, a2) = run(1.0);
        let (b1, b2) = run(1.0);
        let (c1, _) = run(0.5);
        assert_eq!(a1, b1);
        assert_eq!(a2, b2);
        assert_ne!(a1, c1);
    }

    #[test]
    fn determinism_different_inputs_different_hashes() {
        let mut quiet = Mixer::new();
        let id = load_stereo(&mut quiet, &[1000, 1000, 2000, 2000]);
        quiet.play_sfx(id, 100, 0);
        let mut out = vec![0.0f32; 4];
        let quiet_hash = quiet.mix_frame(&mut out, 2);

        let mut loud = Mixer::new();
        let id = load_stereo(&mut loud, &[1000, 1000, 2000, 2000]);
        loud.play_sfx(id, 50, 0);
        let mut out = vec![0.0f32; 4];
        let loud_hash = loud.mix_frame(&mut out, 2);
        assert_ne!(quiet_hash, loud_hash);
    }

    #[test]
    fn mixing_clamps_to_i16_range() {
        let mut mixer = Mixer::new();
        let loud = load_stereo(&mut mixer, &[30000, -30000, 30000, -30000]);
        let also_loud = load_stereo(&mut mixer, &[30000, -30000, 30000, -30000]);
        mixer.play_sfx(loud, 100, 0);
        mixer.play_sfx(also_loud, 100, 0);
        let mut out = vec![0.0f32; 4];
        mixer.mix_frame(&mut out, 2);
        assert_eq!(out[0], 32767.0 / 32768.0);
        assert_eq!(out[1], -32768.0 / 32768.0);
        assert_eq!(out[2], 32767.0 / 32768.0);
        assert_eq!(out[3], -32768.0 / 32768.0);
    }

    #[test]
    fn stream_volume_scales_samples_like_upstream() {
        let mut full = Mixer::new();
        let id = full.load_stream(TONE.to_vec()).unwrap();
        full.play_stream(id, 0);
        let mut full_out = vec![0.0f32; 64];
        full.mix_frame(&mut full_out, 32);

        let mut half = Mixer::new();
        half.set_stream_volume(0.5);
        let id = half.load_stream(TONE.to_vec()).unwrap();
        half.play_stream(id, 0);
        let mut half_out = vec![0.0f32; 64];
        half.mix_frame(&mut half_out, 32);

        for (full_sample, half_sample) in full_out.iter().zip(&half_out) {
            let full_i16 = (full_sample * 32768.0) as i32;
            let expected = (full_i16 * 50 / 100) as f32 / 32768.0;
            assert_eq!(*half_sample, expected);
        }
        assert!(half_out.iter().any(|sample| *sample != 0.0));
    }

    #[test]
    fn rejects_unknown_containers() {
        let mut mixer = Mixer::new();
        assert!(matches!(
            mixer.load_sfx("bad", b"garbage"),
            Err(AudioError::Unsupported(_))
        ));
        assert!(matches!(
            mixer.load_stream(b"RIFF....".to_vec()),
            Err(AudioError::Unsupported(_))
        ));
    }

    #[test]
    fn volume_levels_and_stream_position_are_script_visible() {
        let mut mixer = Mixer::new();
        mixer.set_stream_volume_level(40);
        assert_eq!(mixer.stream_volume(), 40);
        mixer.set_stream_volume_level(200);
        assert_eq!(
            mixer.stream_volume(),
            MAX_VOLUME,
            "clamped like SetMusicVolume"
        );
        mixer.set_sfx_volume_level(0);
        assert_eq!(mixer.sfx_volume(), 0);
        mixer.set_sfx_volume_level(MAX_VOLUME + 1);
        assert_eq!(mixer.sfx_volume(), MAX_VOLUME);

        assert_eq!(mixer.stream_position(), 0, "no stream playing");
        let id = mixer.load_stream(TONE.to_vec()).unwrap();
        mixer.play_stream(id, 0);
        let mut out = vec![0.0f32; 128];
        mixer.mix_frame(&mut out, 64);
        let position = mixer.stream_position();
        assert!(position > 0, "position {position}");
        mixer.pause_stream();
        let paused = mixer.stream_position();
        mixer.mix_frame(&mut out, 32);
        assert_eq!(mixer.stream_position(), paused, "pause holds position");
    }

    #[test]
    fn volumes_are_normalized_and_clamped() {
        let mut mixer = Mixer::new();
        mixer.set_sfx_volume(0.5);
        assert_eq!(mixer.sfx_volume(), 50);
        mixer.set_sfx_volume(-1.0);
        assert_eq!(mixer.sfx_volume(), 0);
        mixer.set_sfx_volume(4.0);
        assert_eq!(mixer.sfx_volume(), 100);
        mixer.set_stream_volume(f32::NAN);
        assert_eq!(mixer.stream_volume(), 0);
    }

    // -----------------------------------------------------------------------
    // Streaming decode equivalence (whole-track reference vs on-demand ring)
    // -----------------------------------------------------------------------

    const TONE_MONO: &[u8] = include_bytes!("../tests/fixtures/tone_mono.ogg");

    /// The whole-buffer interpolation the pre-streaming `StreamEntry` implemented.
    fn reference_frame(
        samples: &[i16],
        channels: usize,
        position: u64,
        frames: usize,
    ) -> (i32, i32) {
        if frames == 0 {
            return (0, 0);
        }
        let frame = (position >> 32) as usize;
        if frame >= frames {
            return (0, 0);
        }
        let fraction = position & 0xFFFF_FFFF;
        let next = (frame + 1).min(frames - 1);
        let lerp = |a: i16, b: i16| -> i32 {
            let weight = ((1u64 << 32) - fraction) as i64;
            let value = i64::from(a) * weight + i64::from(b) * fraction as i64;
            (value >> 32) as i32
        };
        let left = lerp(samples[frame * channels], samples[next * channels]);
        let right = if channels == 1 {
            left
        } else {
            lerp(samples[frame * channels + 1], samples[next * channels + 1])
        };
        (left, right)
    }

    /// Mixes `frames` frames of the whole-buffer reference stream into `out`.
    fn reference_mix(
        decoded: &crate::decode::DecodedAudio,
        position: &mut u64,
        loop_point: Option<usize>,
        out: &mut [f32],
        frames: usize,
    ) {
        let total = decoded.frames();
        let step = (u64::from(decoded.sample_rate) << 32) / u64::from(SAMPLE_RATE);
        let mut playing = true;
        for frame in out[..frames * CHANNELS].as_chunks_mut::<CHANNELS>().0 {
            if !playing {
                frame[0] = 0.0;
                frame[1] = 0.0;
                continue;
            }
            if (*position >> 32) as usize >= total {
                match loop_point {
                    Some(loop_point) => *position = (loop_point as u64) << 32,
                    None => {
                        playing = false;
                        frame[0] = 0.0;
                        frame[1] = 0.0;
                        continue;
                    }
                }
            }
            let (left, right) =
                reference_frame(&decoded.samples, decoded.channels, *position, total);
            frame[0] = left as f32 / 32768.0;
            frame[1] = right as f32 / 32768.0;
            *position += step;
        }
    }

    /// Runs the streaming mixer and the whole-buffer reference over the same playback and compares
    /// every mixed frame.
    fn assert_stream_matches_reference(
        bytes: &[u8],
        loop_point: Option<i32>,
        output_frames: usize,
        label: &str,
    ) {
        let decoded = crate::vorbis::decode_vorbis(bytes).unwrap();
        let mut mixer = Mixer::new();
        let id = mixer.load_stream(bytes.to_vec()).unwrap();
        assert_eq!(
            mixer.stream_frames(id),
            Some(decoded.frames()),
            "{label}: stream length"
        );
        mixer.play_stream(id, loop_point.unwrap_or(-1));

        let mut position = 0u64;
        let mut out = vec![0.0f32; FRAMES_PER_TICK * CHANNELS];
        let mut expected = vec![0.0f32; FRAMES_PER_TICK * CHANNELS];
        let mut remaining = output_frames;
        while remaining > 0 {
            let frames = remaining.min(FRAMES_PER_TICK);
            expected.fill(0.0);
            reference_mix(
                &decoded,
                &mut position,
                loop_point.map(|point| point as usize),
                &mut expected,
                frames,
            );
            mixer.mix_frame(&mut out, frames);
            let count = frames * CHANNELS;
            if out[..count] != expected[..count] {
                let index = out[..count]
                    .iter()
                    .zip(&expected[..count])
                    .position(|(got, want)| got != want)
                    .expect("slices differ");
                panic!(
                    "{label}: mixed frame {} sample {} differs: got {} want {}",
                    output_frames - remaining + index / CHANNELS,
                    index,
                    out[index],
                    expected[index]
                );
            }
            remaining -= frames;
        }
    }

    #[test]
    fn streaming_decode_matches_whole_decode_at_every_position() {
        for (bytes, channels) in [(TONE, 2usize), (TONE_MONO, 1usize)] {
            let decoded = crate::vorbis::decode_vorbis(bytes).unwrap();
            assert_eq!(decoded.channels, channels);
            let frames = decoded.frames();
            let mut mixer = Mixer::new();
            let id = mixer.load_stream(bytes.to_vec()).unwrap();
            assert_eq!(mixer.stream_frames(id), Some(frames));
            for frame in 0..frames {
                let position = (frame as u64) << 32;
                let got = mixer.streams[id.0].frame_at(position);
                let want = reference_frame(&decoded.samples, channels, position, frames);
                assert_eq!(got, want, "frame {frame}");
            }
        }
    }

    #[test]
    fn streaming_decode_matches_whole_decode_when_looping() {
        let frames = crate::vorbis::decode_vorbis(TONE).unwrap().frames();
        for loop_point in [0i32, 1, 100, 700, frames as i32 - 1] {
            assert_stream_matches_reference(
                TONE,
                Some(loop_point),
                frames * 3 + 17,
                &format!("loop {loop_point}"),
            );
        }
    }

    #[test]
    fn streaming_decode_matches_whole_decode_without_looping() {
        let frames = crate::vorbis::decode_vorbis(TONE).unwrap().frames();
        assert_stream_matches_reference(TONE, None, frames + 200, "once");
    }

    #[test]
    fn streaming_decode_buffers_are_bounded() {
        let mut mixer = Mixer::new();
        let id = mixer.load_stream(TONE.to_vec()).unwrap();
        mixer.play_stream(id, 0);
        let mut out = vec![0.0f32; FRAMES_PER_TICK * CHANNELS];
        for _ in 0..64 {
            mixer.mix_frame(&mut out, FRAMES_PER_TICK);
            assert!(
                mixer.streams[id.0].decoder.buffered_frames() <= crate::stream::STREAM_RING_FRAMES,
                "decoded buffer above the ring size"
            );
        }
    }

    /// Frames a raw packet decode produces, before granule truncation.
    fn raw_decoded_frames(bytes: &[u8]) -> usize {
        let mut reader =
            lewton::inside_ogg::OggStreamReader::new(std::io::Cursor::new(bytes)).unwrap();
        let channels = usize::from(reader.ident_hdr.audio_channels);
        let mut frames = 0;
        while let Some(packet) = reader.read_dec_packet_itl().unwrap() {
            frames += packet.len() / channels;
        }
        frames
    }

    #[test]
    fn streaming_decode_applies_the_final_granule_truncation() {
        // Vorbis pads the final packet, so the raw decode produces more frames than the last page
        // granule allows. The streaming length must use the same clamp as the whole decode, and
        // the padded tail must never be reachable by interpolation.
        for bytes in [TONE, TONE_MONO] {
            let decoded = crate::vorbis::decode_vorbis(bytes).unwrap();
            let frames = decoded.frames();
            let raw_frames = raw_decoded_frames(bytes);
            assert!(
                frames < raw_frames,
                "fixture must exercise final-block padding ({frames} of {raw_frames})"
            );

            let mut mixer = Mixer::new();
            let id = mixer.load_stream(bytes.to_vec()).unwrap();
            assert_eq!(
                mixer.stream_frames(id),
                Some(frames),
                "streaming length must match the granule clamp"
            );
            let last = ((frames - 1) as u64) << 32;
            assert_eq!(
                mixer.streams[id.0].frame_at(last),
                reference_frame(&decoded.samples, decoded.channels, last, frames),
                "last valid frame must match the whole decode"
            );
            assert_eq!(
                mixer.streams[id.0].frame_at((frames as u64) << 32),
                (0, 0),
                "the padded tail is past the granule"
            );
        }
    }

    #[test]
    fn looping_does_not_grow_the_anchor_table() {
        let mut mixer = Mixer::new();
        let id = mixer.load_stream(TONE.to_vec()).unwrap();
        let frames = mixer.stream_frames(id).unwrap();
        mixer.play_stream(id, 0);
        let mut out = vec![0.0f32; FRAMES_PER_TICK * CHANNELS];

        // One full pass records an anchor at every page boundary.
        for _ in 0..frames.div_ceil(FRAMES_PER_TICK) {
            mixer.mix_frame(&mut out, FRAMES_PER_TICK);
        }
        let anchors = mixer.streams[id.0].decoder.anchor_count();
        assert!(anchors > 0, "a page-anchored stream must record anchors");

        // Three more loops re-decode the same pages; already-recorded granules must be reused.
        for _ in 0..(frames * 3).div_ceil(FRAMES_PER_TICK) {
            mixer.mix_frame(&mut out, FRAMES_PER_TICK);
        }
        assert_eq!(
            mixer.streams[id.0].decoder.anchor_count(),
            anchors,
            "anchors are keyed by page granule and must not grow per loop"
        );
    }

    #[test]
    fn streaming_decode_matches_whole_decode_for_dense_loop_points() {
        // Sweep loop points across both restart paths (from-zero discard and page-anchor seek)
        // and compare every mixed frame with the whole-buffer reference.
        let frames = crate::vorbis::decode_vorbis(TONE).unwrap().frames();
        let mut loop_point = 1i32;
        while loop_point < frames as i32 {
            assert_stream_matches_reference(
                TONE,
                Some(loop_point),
                frames + 64,
                &format!("loop {loop_point}"),
            );
            loop_point += 137;
        }
    }

    // -----------------------------------------------------------------------
    // Asset-gated streaming equivalence (ignored by default; run with --ignored)
    // -----------------------------------------------------------------------

    fn assets_root() -> Option<std::path::PathBuf> {
        let root = std::env::var_os("RETRO_ASSETS")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from("/home/ted/projects/assets"));
        root.is_dir().then_some(root)
    }

    fn collect_ogg(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_ogg(&path, out);
            } else if path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("ogg"))
            {
                out.push(path);
            }
        }
    }

    #[test]
    #[ignore = "requires the S1/S2 asset folders (RETRO_ASSETS or /home/ted/projects/assets)"]
    fn streaming_matches_whole_decode_for_every_music_stream() {
        let Some(root) = assets_root() else {
            eprintln!("assets not found; skipping");
            return;
        };
        let mut files = Vec::new();
        for game in ["S1", "S2"] {
            collect_ogg(&root.join(game).join("Data").join("Music"), &mut files);
        }
        files.sort();
        assert!(!files.is_empty());
        let mut checked = 0usize;
        for path in &files {
            let bytes = std::fs::read(path).unwrap();
            let decoded = crate::vorbis::decode_vorbis(&bytes).unwrap();
            let frames = decoded.frames();
            assert_stream_matches_reference(&bytes, None, frames + 32, &path.display().to_string());

            // Restarting at synthetic loop points (all after at least one page boundary) exercises
            // the page-anchor seek path against the whole-buffer reference.
            for loop_point in [1, frames / 4, frames / 2, frames - 2] {
                let loop_point = loop_point.max(1).min(frames - 1);
                assert_stream_matches_reference(
                    &bytes,
                    Some(loop_point as i32),
                    frames + frames / 2,
                    &format!("{} loop {loop_point}", path.display()),
                );
            }
            checked += 1;
        }
        println!("checked {checked} streams");
    }

    #[test]
    #[ignore = "requires the S1/S2 asset folders (RETRO_ASSETS or /home/ted/projects/assets)"]
    fn streaming_decode_memory_does_not_scale_with_track_length() {
        let Some(root) = assets_root() else {
            eprintln!("assets not found; skipping");
            return;
        };
        let mut files = Vec::new();
        collect_ogg(&root.join("S2").join("Data").join("Music"), &mut files);
        let path = files
            .into_iter()
            .max_by_key(|path| std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0))
            .expect("S2 music");
        let bytes = std::fs::read(&path).unwrap();
        let compressed_bytes = bytes.len();
        let mut mixer = Mixer::new();
        let id = mixer.load_stream(bytes).unwrap();
        let frames = mixer.stream_frames(id).unwrap();
        assert!(
            frames > crate::stream::STREAM_RING_FRAMES * 100,
            "{} is too short to prove bounded memory ({frames} frames)",
            path.display()
        );
        mixer.play_stream(id, 0);

        // Mix the whole track without ever holding more than the fixed ring: if the decoder
        // buffered the track (as the old whole-track load did), resident state would be
        // `frames * channels * 2` bytes rather than a constant.
        let mut out = vec![0.0f32; FRAMES_PER_TICK * CHANNELS];
        let mut max_ring = 0usize;
        let mut max_state = 0usize;
        let mut mixed = 0usize;
        while mixed < frames {
            let count = (frames - mixed).min(FRAMES_PER_TICK);
            mixer.mix_frame(&mut out, count);
            mixed += count;
            let decoder = &mixer.streams[id.0].decoder;
            max_ring = max_ring.max(decoder.buffered_frames());
            max_state = max_state.max(decoder.decoded_state_bytes());
        }
        assert!(
            max_ring <= crate::stream::STREAM_RING_FRAMES,
            "decoded buffer scales with track length (peak {max_ring} frames)"
        );
        let ring_bytes = crate::stream::STREAM_RING_FRAMES * CHANNELS * std::mem::size_of::<i16>();
        assert!(
            max_state < ring_bytes * 4,
            "decoded state {max_state} B must stay near the {ring_bytes} B ring"
        );
        println!(
            "{}: {} B compressed, mixed {mixed} frames ({:.1} s) with peak ring {max_ring} frames \
             and {max_state} B decoded state (whole-track PCM would be {} B)",
            path.display(),
            compressed_bytes,
            frames as f64 / f64::from(SAMPLE_RATE),
            frames * CHANNELS * std::mem::size_of::<i16>(),
        );
    }
}
