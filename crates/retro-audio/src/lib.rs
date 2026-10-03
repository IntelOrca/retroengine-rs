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
//! [`AudioEngine`] owns a [`retro_platform::AudioDevice`] and submits already-mixed interleaved
//! stereo `f32` buffers; the engine always mixes on its single [`Mixer`] and forwards one engine
//! tick (735 stereo frames, exactly 60 Hz at 44.1 kHz) per logic frame, so device playback and
//! headless hashing see the same samples. Submission is best-effort: when the device queue is
//! full the frame is dropped instead of retried, so a stalled audio device can never block the
//! main thread.

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

/// Owns a [`retro_platform::AudioDevice`] and submits already-mixed sample buffers.
///
/// The engine keeps exactly one [`Mixer`] (in `retro_engine::AudioState`); this type never mixes,
/// it only forwards the caller's interleaved stereo `f32` samples, so device output is
/// byte-identical to the samples that were hashed.
pub struct AudioEngine {
    device: Box<dyn AudioDevice>,
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
        Ok(Self { device })
    }

    /// Device sample rate in Hz.
    #[must_use]
    pub fn sample_rate(&self) -> u32 {
        self.device.sample_rate()
    }

    /// Number of interleaved channels.
    #[must_use]
    pub fn channels(&self) -> u8 {
        self.device.channels()
    }

    /// Submits interleaved stereo `f32` samples once and returns the number of stereo frames the
    /// device accepted.
    ///
    /// The device is allowed to accept fewer frames than `frames` contains (or none at all) when
    /// its queue is full, and frames it did not accept are dropped by the caller. This call
    /// never retries, so a stalled device can not block the engine's frame loop; the local mix
    /// (and therefore the deterministic hash) is unaffected by dropped output.
    pub fn submit(&mut self, frames: &[f32]) -> Result<usize, AudioError> {
        if !frames.len().is_multiple_of(CHANNELS) {
            return Err(AudioError::Invalid(
                "sample count is not a whole number of stereo frames".to_owned(),
            ));
        }
        let total = frames.len() / CHANNELS;
        if total == 0 {
            return Ok(0);
        }
        let accepted = self.device.submit(frames).map_err(device_error)?;
        Ok(accepted.min(total))
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
    use std::cell::Cell;
    use std::rc::Rc;
    use std::time::{Duration, Instant};

    fn headless_device(rate: u32) -> (HeadlessPlatform, Box<dyn AudioDevice>) {
        let mut platform = HeadlessPlatform::new();
        platform.init().unwrap();
        let device = platform.open_audio(AudioDesc::stereo(rate)).unwrap();
        (platform, device)
    }

    /// Audio device stub whose submissions always accept `accept` frames and count calls.
    struct MockDevice {
        sample_rate: u32,
        channels: u8,
        accept: usize,
        queued: usize,
        calls: Rc<Cell<usize>>,
    }

    impl MockDevice {
        fn new(accept: usize, queued: usize) -> (Self, Rc<Cell<usize>>) {
            let calls = Rc::new(Cell::new(0));
            (
                Self {
                    sample_rate: SAMPLE_RATE,
                    channels: CHANNELS as u8,
                    accept,
                    queued,
                    calls: Rc::clone(&calls),
                },
                calls,
            )
        }
    }

    impl AudioDevice for MockDevice {
        fn sample_rate(&self) -> u32 {
            self.sample_rate
        }

        fn channels(&self) -> u8 {
            self.channels
        }

        fn submit(&mut self, frames: &[f32]) -> Result<usize, PlatformError> {
            self.calls.set(self.calls.get() + 1);
            Ok(self.accept.min(frames.len() / self.channels as usize))
        }

        fn queued_frames(&self) -> usize {
            self.queued
        }

        fn close(&mut self) -> Result<(), PlatformError> {
            Ok(())
        }
    }

    #[test]
    fn submit_never_retries_a_stalled_device() {
        let (device, calls) = MockDevice::new(0, 10_000_000);
        let mut engine = AudioEngine::new(Box::new(device)).unwrap();
        let frames = vec![0.0f32; FRAMES_PER_TICK * CHANNELS];
        let started = Instant::now();
        assert_eq!(engine.submit(&frames).unwrap(), 0);
        assert_eq!(
            calls.get(),
            1,
            "a stalled device must be asked exactly once"
        );
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "submit must not spin against a stalled device"
        );
    }

    #[test]
    fn submit_reports_a_partial_acceptance_without_looping() {
        let (device, calls) = MockDevice::new(1, 0);
        let mut engine = AudioEngine::new(Box::new(device)).unwrap();
        let frames = vec![0.0f32; FRAMES_PER_TICK * CHANNELS];
        assert_eq!(engine.submit(&frames).unwrap(), 1);
        assert_eq!(calls.get(), 1, "one submit call, no retry loop");
    }

    #[test]
    fn engine_submits_exactly_one_second_of_frames() {
        let (platform, device) = headless_device(SAMPLE_RATE);
        let mut engine = AudioEngine::new(device).unwrap();
        for _ in 0..60 {
            engine
                .submit(&vec![0.0; FRAMES_PER_TICK * CHANNELS])
                .unwrap();
        }
        assert_eq!(engine.queued_frames(), FRAMES_PER_TICK * 60);
        assert_eq!(
            platform.captured_pcm().len(),
            FRAMES_PER_TICK * 60 * CHANNELS
        );
        assert!(platform.captured_pcm().iter().all(|s| *s == 0.0));
        engine.close().unwrap();
    }

    #[test]
    fn engine_submits_mixer_output_unchanged() {
        let (platform, device) = headless_device(SAMPLE_RATE);
        let mut engine = AudioEngine::new(device).unwrap();
        let data = [0xE8u8, 0x03].repeat(0x100);
        let wav = crate::wav::build_wav(1, 2, SAMPLE_RATE, 16, &data);
        let mut mixer = Mixer::new();
        let id = mixer.load_sfx("tick", &wav).unwrap();
        mixer.play_sfx(id, 100, 0);
        let mut out = vec![0.0f32; 16];
        let hash = mixer.mix_frame(&mut out, 8);
        assert!(out.iter().any(|sample| *sample != 0.0), "non-silent mix");

        assert_eq!(engine.submit(&out).unwrap(), 8);
        assert_eq!(platform.captured_pcm(), out);
        assert_ne!(hash, [0u8; 32]);
    }

    #[test]
    fn engine_rejects_partial_frames() {
        let (_platform, device) = headless_device(SAMPLE_RATE);
        let mut engine = AudioEngine::new(device).unwrap();
        assert!(matches!(engine.submit(&[0.0]), Err(AudioError::Invalid(_))));
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
