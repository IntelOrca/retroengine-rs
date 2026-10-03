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
//! headless hashing see the same samples. Submission is best-effort: the buffer is offered to
//! the device once and whatever is not accepted is dropped, so the frame loop never waits on the
//! audio device.
//!
//! [`AudioEngine::with_prebuffer`] adds bounded device flow control: playback only starts after
//! [`PREBUFFER_TICKS`] ticks are queued, and submissions are capped at [`MAX_QUEUED_TICKS`]
//! ticks. The mixer itself always advances exactly one tick per logic frame, so dropping device
//! output can never change the mixed PCM hash.

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

/// Engine ticks of mixed audio queued before a prebuffered device is started.
///
/// A tick is [`FRAMES_PER_TICK`] stereo frames (about 16.7 ms at 44.1 kHz/60 Hz), so playback
/// begins with roughly 50 ms of real audio already queued. That headroom covers startup and
/// timing jitter without the device draining to silence between logic frames.
pub const PREBUFFER_TICKS: usize = 3;

/// Hard cap on queued audio in engine ticks (about 100 ms at 60 Hz).
///
/// A prebuffered engine never offers a device more than this. A device that stops draining parks
/// the queue at the cap and later ticks are dropped until it drains again; the mixer still
/// advances exactly one tick per logic frame, so dropped device output never changes the hash.
pub const MAX_QUEUED_TICKS: usize = 6;

/// Device flow control for an engine created with [`AudioEngine::with_prebuffer`].
struct QueueControl {
    /// Frames that must be queued before the device is started.
    prebuffer_frames: usize,
    /// Hard upper bound on queued frames.
    cap_frames: usize,
    /// Whether the device has been started (or a start was already requested).
    started: bool,
}

/// Owns a [`retro_platform::AudioDevice`] and submits already-mixed sample buffers.
///
/// The engine keeps exactly one [`Mixer`] (in `retro_engine::AudioState`); this type never mixes,
/// it only forwards the caller's interleaved stereo `f32` samples, so device output is
/// byte-identical to the samples that were hashed. [`AudioEngine::with_prebuffer`] additionally
/// delays the device start until the prebuffer is queued and drops output above the queue cap;
/// both only affect device submission, never the mixed samples the caller passes in.
pub struct AudioEngine {
    device: Box<dyn AudioDevice>,
    queue: Option<QueueControl>,
}

impl AudioEngine {
    /// Creates an engine over `device` that starts it immediately.
    ///
    /// The device must accept [`SAMPLE_RATE`] stereo `f32` frames; upstream allows SDL to
    /// change the device frequency, but this port keeps a single fixed output format. For
    /// bounded latency and startup headroom use [`AudioEngine::with_prebuffer`] instead.
    pub fn new(device: Box<dyn AudioDevice>) -> Result<Self, AudioError> {
        Self::validate(device.as_ref())?;
        let mut engine = Self {
            device,
            queue: None,
        };
        // A failed start is not fatal here: the engine keeps mixing and offering frames, and a
        // no-op start is the norm for capture-style devices.
        let _ = engine.device.start();
        Ok(engine)
    }

    /// Creates an engine that prebuffers [`PREBUFFER_TICKS`] ticks before starting `device`.
    ///
    /// The device is left as its backend opened it (SDL's simplified device stream starts
    /// paused); once the queue reaches the prebuffer depth the engine calls
    /// [`AudioDevice::start`]. Subsequent submissions are capped at [`MAX_QUEUED_TICKS`] ticks so
    /// a stalled device parks the queue instead of growing latency without bound. Mixing is
    /// untouched: the caller's buffers are the same whether or not a device is attached.
    pub fn with_prebuffer(device: Box<dyn AudioDevice>) -> Result<Self, AudioError> {
        Self::validate(device.as_ref())?;
        Ok(Self {
            device,
            queue: Some(QueueControl {
                prebuffer_frames: PREBUFFER_TICKS * FRAMES_PER_TICK,
                cap_frames: MAX_QUEUED_TICKS * FRAMES_PER_TICK,
                started: false,
            }),
        })
    }

    /// Checks that `device` matches the mixer output format.
    fn validate(device: &dyn AudioDevice) -> Result<(), AudioError> {
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
        Ok(())
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
    /// its queue is full or it is unresponsive, and frames it did not accept are dropped by the
    /// caller. This call never retries — the old implementation looped until every frame was
    /// accepted and detached the device on a partial result — so a device that accepts nothing
    /// can not stall the engine's frame loop. The local mix (and therefore the deterministic
    /// hash) is unaffected by dropped output.
    ///
    /// A prebuffered engine ([`AudioEngine::with_prebuffer`]) offers at most the room left below
    /// [`MAX_QUEUED_TICKS`] and starts the device once the queue reaches [`PREBUFFER_TICKS`].
    /// Both decisions read [`AudioDevice::queued_frames`], which is advisory: a queue that drains
    /// between the read and the submission only makes the offered slice smaller than it could be,
    /// never larger than the cap.
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
        let offered = match &self.queue {
            Some(queue) => {
                frames_within_capacity(self.device.queued_frames(), total, queue.cap_frames)
            }
            None => total,
        };
        let accepted = if offered == 0 {
            0
        } else {
            self.device
                .submit(&frames[..offered * CHANNELS])
                .map_err(device_error)?
                .min(offered)
        };

        if let Some(queue) = &mut self.queue
            && !queue.started
            && self.device.queued_frames() >= queue.prebuffer_frames
        {
            self.device.start().map_err(device_error)?;
            queue.started = true;
        }
        Ok(accepted)
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

/// Number of frames a single submit may enqueue so the queue never passes `cap`.
///
/// A pure function so the cap logic is unit-testable without a device. `queued` may overstate
/// the real queue (a device can drain between the read and the submission), which only makes the
/// result conservative.
fn frames_within_capacity(queued: usize, total: usize, cap: usize) -> usize {
    cap.saturating_sub(queued).min(total)
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

    /// Shared counters for [`DrainingDevice`], observable from the test.
    #[derive(Default)]
    struct DrainingState {
        /// Frames currently queued on the simulated device.
        queued: Cell<usize>,
        /// Whether the engine called [`AudioDevice::start`].
        started: Cell<bool>,
        /// Number of `submit` calls seen so far.
        submits: Cell<u64>,
        /// Times the device wanted more frames than were queued.
        underruns: Cell<usize>,
    }

    /// Audio device stub that drains its queue like hardware does.
    ///
    /// On every `submit` the device first consumes `drain_ticks` ticks when `submits` is a
    /// multiple of `drain_every` (0 disables). With `jitter_every` set, every submit that is a
    /// multiple of it consumes `jitter_ticks` extra ticks (a late engine frame) and the following
    /// submit consumes that many fewer (the engine catches up by waking early). A drain that
    /// would take more frames than are queued is recorded as an underrun and saturates at
    /// silence, matching a device that ran dry. All offered frames are appended, so the queue
    /// equals submissions minus drains.
    struct DrainingDevice {
        state: Rc<DrainingState>,
        /// Ticks consumed by the periodic drain.
        drain_ticks: usize,
        /// A periodic drain happens every this many `submit` calls (0 disables).
        drain_every: u64,
        /// Extra ticks consumed by the jitter drain.
        jitter_ticks: usize,
        /// A jitter drain happens every this many `submit` calls (0 disables).
        jitter_every: u64,
    }

    impl DrainingDevice {
        fn drain_frames(&self) -> usize {
            if !self.state.started.get() {
                return 0;
            }
            let submits = self.state.submits.get();
            let mut ticks = 0;
            if self.drain_every != 0 && submits.is_multiple_of(self.drain_every) {
                ticks += self.drain_ticks;
            }
            if self.jitter_every != 0 {
                let phase = submits % self.jitter_every;
                if phase == 0 {
                    ticks += self.jitter_ticks;
                } else if phase == 1 {
                    ticks = ticks.saturating_sub(self.jitter_ticks);
                }
            }
            ticks * FRAMES_PER_TICK
        }
    }

    impl AudioDevice for DrainingDevice {
        fn sample_rate(&self) -> u32 {
            SAMPLE_RATE
        }

        fn channels(&self) -> u8 {
            CHANNELS as u8
        }

        fn submit(&mut self, frames: &[f32]) -> Result<usize, PlatformError> {
            let queued = self.state.queued.get();
            let drain = self.drain_frames();
            if drain > queued {
                self.state.underruns.set(self.state.underruns.get() + 1);
            }
            self.state.queued.set(queued.saturating_sub(drain));
            let total = frames.len() / CHANNELS;
            self.state.queued.set(self.state.queued.get() + total);
            self.state.submits.set(self.state.submits.get() + 1);
            Ok(total)
        }

        fn queued_frames(&self) -> usize {
            self.state.queued.get()
        }

        fn start(&mut self) -> Result<(), PlatformError> {
            self.state.started.set(true);
            Ok(())
        }

        fn close(&mut self) -> Result<(), PlatformError> {
            Ok(())
        }
    }

    fn prebuffered_draining(
        drain_ticks: usize,
        drain_every: u64,
        jitter_ticks: usize,
        jitter_every: u64,
    ) -> (AudioEngine, Rc<DrainingState>) {
        let state = Rc::new(DrainingState::default());
        let device = DrainingDevice {
            state: Rc::clone(&state),
            drain_ticks,
            drain_every,
            jitter_ticks,
            jitter_every,
        };
        let engine = AudioEngine::with_prebuffer(Box::new(device)).unwrap();
        (engine, state)
    }

    fn tick_buffer() -> Vec<f32> {
        vec![0.0f32; FRAMES_PER_TICK * CHANNELS]
    }

    #[test]
    fn capacity_bounds_a_single_submission_to_the_remaining_room() {
        let tick = FRAMES_PER_TICK;
        let cap = tick * MAX_QUEUED_TICKS;
        assert_eq!(frames_within_capacity(0, tick, cap), tick);
        assert_eq!(frames_within_capacity(cap - 1, tick, cap), 1);
        assert_eq!(frames_within_capacity(cap, tick, cap), 0);
        assert_eq!(frames_within_capacity(cap * 100, tick, cap), 0);
        assert_eq!(frames_within_capacity(0, cap + 500, cap), cap);
        assert_eq!(frames_within_capacity(0, 0, cap), 0);
        assert_eq!(frames_within_capacity(0, tick, 0), 0);
    }

    #[test]
    fn prebuffer_defers_start_until_the_target_is_queued() {
        let (mut engine, state) = prebuffered_draining(0, 0, 0, 0);
        let tick = tick_buffer();
        for _ in 0..PREBUFFER_TICKS - 1 {
            engine.submit(&tick).unwrap();
            assert!(
                !state.started.get(),
                "playback must not start before the prebuffer is queued"
            );
        }
        assert_eq!(
            engine.queued_frames(),
            (PREBUFFER_TICKS - 1) * FRAMES_PER_TICK
        );
        engine.submit(&tick).unwrap();
        assert!(
            state.started.get(),
            "playback starts once the prebuffer is in"
        );
        assert_eq!(engine.queued_frames(), PREBUFFER_TICKS * FRAMES_PER_TICK);
    }

    #[test]
    fn slow_draining_device_never_underruns_and_parks_at_the_cap() {
        // The device consumes one tick every third submit (1/3 real time), so production wins
        // and the queue climbs to the cap; the engine must never push past it.
        let (mut engine, state) = prebuffered_draining(1, 3, 0, 0);
        let tick = tick_buffer();
        let cap = MAX_QUEUED_TICKS * FRAMES_PER_TICK;
        let mut max_queued = 0usize;
        for _ in 0..300 {
            engine.submit(&tick).unwrap();
            let queued = engine.queued_frames();
            max_queued = max_queued.max(queued);
            assert!(queued <= cap, "queue grew past the cap: {queued} > {cap}");
        }
        assert!(state.started.get());
        assert_eq!(
            state.underruns.get(),
            0,
            "the prebuffer must cover slow drains"
        );
        assert_eq!(max_queued, cap, "a slow device parks the queue at the cap");
        assert!(engine.queued_frames() >= PREBUFFER_TICKS * FRAMES_PER_TICK);
    }

    #[test]
    fn jittered_drain_within_the_prebuffer_never_underruns() {
        // Normal cadence drains one tick per submit; every fifth submit drains two (a full tick
        // of scheduling jitter) and the following submit drains none (the engine catches up).
        // The three-tick prebuffer keeps at least one tick queued throughout.
        let (mut engine, state) = prebuffered_draining(1, 1, 1, 5);
        let tick = tick_buffer();
        let cap = MAX_QUEUED_TICKS * FRAMES_PER_TICK;
        let mut min_queued = usize::MAX;
        for submit in 0..240 {
            engine.submit(&tick).unwrap();
            let queued = engine.queued_frames();
            assert!(queued <= cap, "queue grew past the cap at submit {submit}");
            if state.started.get() {
                min_queued = min_queued.min(queued);
            }
        }
        assert!(state.started.get());
        assert_eq!(state.underruns.get(), 0);
        assert!(
            min_queued >= FRAMES_PER_TICK,
            "a one-tick stall must leave at least one tick queued, saw {min_queued}"
        );
    }
}
