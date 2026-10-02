//! Mixing, streaming and playback of RSDK audio formats.
//!
//! This crate ports the RSDKv4 software mixer (`RSDKv4/Audio.cpp`):
//!
//! - [`Mixer::load_sfx`] mirrors `LoadSfx`: WAV (via a hand-written RIFF parser) and Ogg
//!   Vorbis (via `lewton`) sound effects are decoded and converted to interleaved stereo
//!   16-bit samples at [`SAMPLE_RATE`] when loaded.
//! - [`Mixer::play_sfx`] mirrors `PlaySfx`/`SetSfxAttributes`: sixteen voice channels are
//!   allocated by scanning for a free or same-sound channel, and volume/pan follow
//!   `ProcessAudioMixing`'s integer arithmetic and pan attenuation.
//! - [`Mixer::load_stream`]/[`Mixer::play_stream`]/[`Mixer::pause_stream`] mirror
//!   `LoadMusic`/`PlayMusic`/`PauseSound`/`ResumeSound`, including `ov_pcm_seek`-style loop
//!   points in source PCM frames.
//! - [`Mixer::mix_frame`] mirrors `ProcessAudioPlayback`: one pass over the music stream then
//!   the SFX channels, accumulated in `i32`, clamped to `i16` and emitted as `f32`, with a
//!   BLAKE3 hash for deterministic frame comparison.
//!
//! Upstream lets SDL resample assets to the device rate; this port instead resamples with a
//! deterministic linear interpolation (sources already at 44.1 kHz are copied exactly).
//! [`AudioEngine`] binds a [`Mixer`] to a [`retro_platform::AudioDevice`] and submits one
//! engine tick (735 stereo frames, exactly 60 Hz at 44.1 kHz) at a time.

#![forbid(unsafe_code)]

mod decode;
mod error;
mod mixer;
mod vorbis;
mod wav;

pub use error::AudioError;
pub use mixer::{Mixer, SfxChannel, SfxId, StreamId};

use retro_platform::{AudioDevice, PlatformError};

/// Output sample rate, upstream's `AUDIO_FREQUENCY`.
pub const SAMPLE_RATE: u32 = 44_100;

/// Interleaved output channel count, upstream's `AUDIO_CHANNELS`.
pub const CHANNELS: usize = 2;

/// Maximum volume for the upstream `0..=100` volume scale (`MAX_VOLUME`).
pub const MAX_VOLUME: u8 = 100;

/// Number of SFX voice channels, upstream's non-original `CHANNEL_COUNT`.
pub const SFX_CHANNEL_COUNT: usize = 16;

/// Maximum number of loaded sound effects, upstream's `SFX_COUNT`.
pub const SFX_COUNT: usize = 256;

/// Stereo frames mixed per 60 Hz engine tick at [`SAMPLE_RATE`].
pub const FRAMES_PER_TICK: usize = SAMPLE_RATE as usize / 60;

/// Binds a [`Mixer`] to a [`retro_platform::AudioDevice`].
pub struct AudioEngine {
    mixer: Mixer,
    device: Box<dyn AudioDevice>,
    scratch: Vec<f32>,
}

impl AudioEngine {
    /// Creates an engine over `device`.
    ///
    /// The device must accept [`SAMPLE_RATE`] stereo `f32` frames; upstream allows SDL to
    /// change the device frequency, but this port keeps a single fixed output format.
    pub fn new(device: Box<dyn AudioDevice>) -> Result<Self, AudioError> {
        if device.sample_rate() != SAMPLE_RATE {
            return Err(AudioError::Unsupported(format!(
                "device rate {} Hz does not match mixer rate {SAMPLE_RATE} Hz",
                device.sample_rate()
            )));
        }
        if usize::from(device.channels()) != CHANNELS {
            return Err(AudioError::Unsupported(format!(
                "device has {} channels, mixer produces {CHANNELS}",
                device.channels()
            )));
        }
        Ok(Self {
            mixer: Mixer::new(),
            device,
            scratch: Vec::new(),
        })
    }

    /// The bound mixer.
    #[must_use]
    pub fn mixer(&self) -> &Mixer {
        &self.mixer
    }

    /// Mutable access to the bound mixer.
    pub fn mixer_mut(&mut self) -> &mut Mixer {
        &mut self.mixer
    }

    /// Mixes one engine tick ([`FRAMES_PER_TICK`] frames) and submits it to the device.
    pub fn tick(&mut self) -> Result<[u8; 32], AudioError> {
        self.pump(FRAMES_PER_TICK)
    }

    /// Mixes `frames` stereo frames, submits them all and returns the output hash.
    pub fn pump(&mut self, frames: usize) -> Result<[u8; 32], AudioError> {
        self.scratch.resize(frames * CHANNELS, 0.0);
        let hash = self.mixer.mix_frame(&mut self.scratch, frames);
        let mut submitted = 0;
        while submitted < frames {
            let accepted = self
                .device
                .submit(&self.scratch[submitted * CHANNELS..])
                .map_err(device_error)?;
            if accepted == 0 {
                return Err(AudioError::Invalid(
                    "audio device accepted no frames".to_owned(),
                ));
            }
            submitted += accepted;
        }
        Ok(hash)
    }

    /// Frames currently queued on the device.
    #[must_use]
    pub fn queued_frames(&self) -> usize {
        self.device.queued_frames()
    }

    /// Closes the device.
    pub fn close(mut self) -> Result<(), AudioError> {
        self.device.close().map_err(device_error)
    }
}

fn device_error(error: PlatformError) -> AudioError {
    AudioError::Invalid(format!("audio device: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use retro_platform::headless::HeadlessPlatform;
    use retro_platform::{AudioDesc, Platform};

    fn headless_device(rate: u32) -> (HeadlessPlatform, Box<dyn AudioDevice>) {
        let mut platform = HeadlessPlatform::new();
        platform.init().unwrap();
        let device = platform.open_audio(AudioDesc::stereo(rate)).unwrap();
        (platform, device)
    }

    #[test]
    fn engine_ticks_exactly_one_second_of_frames() {
        let (platform, device) = headless_device(SAMPLE_RATE);
        let mut engine = AudioEngine::new(device).unwrap();
        let mut hashes = Vec::new();
        for _ in 0..60 {
            hashes.push(engine.tick().unwrap());
        }
        assert_eq!(engine.queued_frames(), FRAMES_PER_TICK * 60);
        assert_eq!(
            platform.captured_pcm().len(),
            FRAMES_PER_TICK * 60 * CHANNELS
        );
        assert!(platform.captured_pcm().iter().all(|s| *s == 0.0));
        assert!(hashes.windows(2).all(|pair| pair[0] == pair[1]));
        engine.close().unwrap();
    }

    #[test]
    fn engine_pump_matches_mixer_hash() {
        let (platform, device) = headless_device(SAMPLE_RATE);
        let mut engine = AudioEngine::new(device).unwrap();
        let data = [0xE8u8, 0x03].repeat(0x100);
        let wav = crate::wav::build_wav(1, 2, SAMPLE_RATE, 16, &data);
        let id = engine.mixer_mut().load_sfx("tick", &wav).unwrap();
        engine.mixer_mut().play_sfx(id, 100, 0);
        let hash = engine.pump(8).unwrap();

        let mut reference = Mixer::new();
        let id = reference.load_sfx("tick", &wav).unwrap();
        reference.play_sfx(id, 100, 0);
        let mut out = vec![0.0f32; 16];
        assert_eq!(hash, reference.mix_frame(&mut out, 8));
        assert_eq!(platform.captured_pcm().len(), 16);
    }

    #[test]
    fn engine_rejects_mismatched_devices() {
        let (_platform, device) = headless_device(48_000);
        assert!(matches!(
            AudioEngine::new(device),
            Err(AudioError::Unsupported(_))
        ));
    }
}
