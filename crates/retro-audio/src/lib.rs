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
//!   points in source PCM frames. Like upstream's libvorbisfile path, music packets are decoded
//!   on demand, not at load: only a bounded lookahead ring is resident.
//! - [`Mixer::mix_frame`] mirrors `ProcessAudioPlayback`: one pass over the music stream then
//!   the SFX channels, accumulated in `i32`, clamped to `i16` and emitted as `f32`, with a
//!   BLAKE3 hash for deterministic frame comparison.
//!
//! Upstream lets SDL resample assets to the device rate; this port instead resamples with a
//! deterministic linear interpolation (sources already at 44.1 kHz are copied exactly).
//! [`AudioEngine`] owns a [`retro_platform::AudioDevice`] and submits already-mixed interleaved
//! stereo `f32` buffers; the engine always mixes on its single [`Mixer`] and forwards one engine
//! tick (735 stereo frames, exactly 60 Hz at 44.1 kHz) per logic frame, so device playback and
//! headless hashing see the same samples.
//!
//! # Device flow control
//!
//! A logic frame can run late (windowed vsync, a load hitch, OS scheduling) and a device can run
//! late too (a Windows audio glitch), so production and playback are decoupled by a small
//! in-process backlog:
//!
//! * [`AudioEngine::with_prebuffer`] queues [`PREBUFFER_TICKS`] ticks before starting the device,
//!   so playback begins with real headroom.
//! * Each submission is appended to a small in-process backlog and then offered to the device up
//!   to a high-water mark of [`MAX_QUEUED_TICKS`] on the device queue (about 100 ms). A device
//!   that is momentarily full (or has not drained since the last logic frame) no longer rejects
//!   the tick; the frames wait in the backlog and are pushed as room appears. The offer happens
//!   before any trim, so a catch-up burst flows into device queue room instead of being dropped.
//! * If the backlog still exceeds [`MAX_BACKLOG_TICKS`] after that offer, the engine is more than
//!   `~200 ms` ahead of real time and cannot stay there; it drops the *oldest* backlog frames
//!   (resynchronizing to live audio instead of adding permanent latency) and counts them.
//! * Every drop, every device underrun and the deepest queue/backlog seen are recorded in
//!   [`AudioCounters`] ([`AudioEngine::counters`]), and the engine prints a one-time warning on
//!   the first gap of each kind, so no skip is ever silent.
//!
//! The mixer itself always advances exactly one tick per logic frame, so device flow control can
//! never change the mixed PCM hash. While the backlog is within [`MAX_BACKLOG_TICKS`] no produced
//! frame is dropped.

#![forbid(unsafe_code)]

mod decode;
mod error;
mod mixer;
mod stream;
mod vorbis;
mod wav;

pub use error::AudioError;
pub use mixer::{Mixer, SfxChannel, SfxId, StreamId};

use std::collections::VecDeque;

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

/// High-water mark on queued device audio in engine ticks (about 100 ms at 60 Hz).
///
/// A prebuffered engine never offers a device more than this at once, so device latency stays
/// bounded. Frames that do not fit are retained in the in-process backlog rather than dropped
/// (see [`MAX_BACKLOG_TICKS`]), and the mixer still advances exactly one tick per logic frame, so
/// flow control never changes the mixed PCM hash.
pub const MAX_QUEUED_TICKS: usize = 6;

/// High-water mark on frames produced but not yet accepted by the device, in engine ticks.
///
/// A running system drains the device queue in real time, so the backlog only grows when the
/// logic loop produces audio faster than the device consumes it. Up to this much (about 100 ms)
/// is retained; beyond it the engine is more than [`MAX_QUEUED_TICKS`] + [`MAX_BACKLOG_TICKS`]
/// ticks (~200 ms) ahead of playback and drops the *oldest* backlog frames so audio stays close
/// to live. Dropped frames are counted, never silent.
pub const MAX_BACKLOG_TICKS: usize = 6;

/// Snapshot of an [`AudioEngine`]'s flow-control counters.
///
/// All counters are monotonic over the engine's lifetime and are safe to poll every frame; they
/// never influence mixing, so the deterministic PCM hash is unaffected by device behaviour.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AudioCounters {
    /// Frames passed to [`AudioEngine::submit`].
    pub produced_frames: u64,
    /// Frames accepted by the device.
    pub submitted_frames: u64,
    /// Frames dropped to resynchronize after the backlog exceeded [`MAX_BACKLOG_TICKS`].
    pub dropped_frames: u64,
    /// Number of resynchronizations that dropped at least one frame.
    pub resyncs: u64,
    /// Submissions seen while the device was started but its queue had run empty.
    pub underruns: u64,
    /// Deepest device queue observed, in frames.
    pub max_queued_frames: usize,
    /// Deepest backlog observed, in frames.
    pub max_backlog_frames: usize,
}

/// Device flow control for an engine created with [`AudioEngine::with_prebuffer`].
struct QueueControl {
    /// Frames that must be queued before the device is started.
    prebuffer_frames: usize,
    /// High-water mark on queued device frames.
    cap_frames: usize,
    /// High-water mark on backlog frames.
    backlog_cap_frames: usize,
    /// Produced frames not yet accepted by the device, oldest first.
    backlog: VecDeque<f32>,
    /// Whether the device has been started (or a start was already requested).
    started: bool,
    /// Whether the first backlog resync has been reported on stderr.
    warned_resync: bool,
    /// Whether the first device underrun has been reported on stderr.
    warned_underrun: bool,
}

/// Owns a [`retro_platform::AudioDevice`] and submits already-mixed sample buffers.
///
/// The engine keeps exactly one [`Mixer`] (in `retro_engine::AudioState`); this type never mixes,
/// it only forwards the caller's interleaved stereo `f32` samples, so device output is
/// byte-identical to the samples that were hashed. [`AudioEngine::with_prebuffer`] additionally
/// delays the device start until the prebuffer is queued and retains frames above the device
/// queue high-water in an in-process backlog; both only affect device submission, never the mixed
/// samples the caller passes in.
pub struct AudioEngine {
    device: Box<dyn AudioDevice>,
    queue: Option<QueueControl>,
    counters: AudioCounters,
}

impl AudioEngine {
    /// Creates an engine over `device` that starts it immediately.
    ///
    /// The device must accept [`SAMPLE_RATE`] stereo `f32` frames; upstream allows SDL to
    /// change the device frequency, but this port keeps a single fixed output format. For
    /// bounded latency and startup headroom use [`AudioEngine::with_prebuffer`] instead.
    ///
    /// This is the immediate mode: [`AudioEngine::submit`] offers the whole buffer to the device
    /// once and reports how much it accepted; the caller drops the rest. There is no backlog.
    pub fn new(device: Box<dyn AudioDevice>) -> Result<Self, AudioError> {
        Self::validate(device.as_ref())?;
        let mut engine = Self {
            device,
            queue: None,
            counters: AudioCounters::default(),
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
    /// [`AudioDevice::start`]. Subsequent submissions first fill the device up to
    /// [`MAX_QUEUED_TICKS`] ticks and keep the remainder in a backlog of at most
    /// [`MAX_BACKLOG_TICKS`] ticks, so a device that is momentarily full (or a logic burst that
    /// runs ahead of real time) never loses output. Only a backlog above its cap is trimmed, and
    /// then the oldest frames are dropped and counted in [`AudioEngine::counters`]; the engine
    /// prints a one-time warning for the first drop and the first underrun. Mixing is untouched:
    /// the caller's buffers are the same whether or not a device is attached.
    pub fn with_prebuffer(device: Box<dyn AudioDevice>) -> Result<Self, AudioError> {
        Self::validate(device.as_ref())?;
        Ok(Self {
            device,
            queue: Some(QueueControl {
                prebuffer_frames: PREBUFFER_TICKS * FRAMES_PER_TICK,
                cap_frames: MAX_QUEUED_TICKS * FRAMES_PER_TICK,
                backlog_cap_frames: MAX_BACKLOG_TICKS * FRAMES_PER_TICK,
                backlog: VecDeque::new(),
                started: false,
                warned_resync: false,
                warned_underrun: false,
            }),
            counters: AudioCounters::default(),
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

    /// Submits interleaved stereo `f32` samples and returns the number of frames the engine
    /// retained.
    ///
    /// In immediate mode ([`AudioEngine::new`]) the buffer is offered to the device exactly once
    /// and the returned count is what the device accepted; the caller drops the rest. This call
    /// never retries, so a device that accepts nothing can not stall the engine's frame loop.
    ///
    /// In prebuffered mode ([`AudioEngine::with_prebuffer`]) all frames are appended to the
    /// backlog, offered to the device while its queue is below [`MAX_QUEUED_TICKS`] (at most one
    /// device call per flush chunk, and never a retry against a device that accepts nothing), and
    /// the returned count is the number of frames retained. Frames are only dropped when the
    /// backlog exceeds [`MAX_BACKLOG_TICKS`], in which case the oldest are trimmed and counted in
    /// [`AudioEngine::counters`]. The device is started once its queue reaches
    /// [`PREBUFFER_TICKS`]. Every queue decision reads [`AudioDevice::queued_frames`], which is
    /// advisory: a queue that drains between the read and the submission only makes the offered
    /// slice smaller than it could be, never larger than the caps.
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
        self.counters.produced_frames += total as u64;

        if self.queue.is_none() {
            let accepted = self.device.submit(frames).map_err(device_error)?.min(total);
            self.counters.submitted_frames += accepted as u64;
            self.counters.max_queued_frames = self
                .counters
                .max_queued_frames
                .max(self.device.queued_frames());
            return Ok(accepted);
        }

        let queue = self
            .queue
            .as_mut()
            .expect("prebuffered engine has a queue control");
        let backlogged_before = queue.backlog.len() / CHANNELS;
        queue.backlog.extend(frames.iter().copied());

        // A started device with an empty queue ran dry between logic frames. Keep playing into
        // the gap (there is no silence to splice), but count and report it: it means production
        // is behind the device clock, not that frames were dropped.
        if queue.started && self.device.queued_frames() == 0 {
            self.counters.underruns += 1;
            if !queue.warned_underrun {
                queue.warned_underrun = true;
                eprintln!(
                    "audio: device underrun: queue empty while playing (production is behind \
                     real time); further underruns only counted"
                );
            }
        }

        // Offer the backlog to the device while there is room below the high-water mark. This
        // runs before any trim: a burst (a catch-up tick sequence) flows into the device queue
        // whenever it has room, and only frames the device cannot take contribute to the
        // backlog. The loop stops on a partial acceptance so a stalled device is never retried
        // within a tick.
        loop {
            let queued = self.device.queued_frames();
            self.counters.max_queued_frames = self.counters.max_queued_frames.max(queued);
            let room = queue.cap_frames.saturating_sub(queued);
            let available = queue.backlog.len() / CHANNELS;
            if room == 0 || available == 0 {
                break;
            }
            let take = room.min(available);
            let accepted = self
                .device
                .submit(&queue.backlog.make_contiguous()[..take * CHANNELS])
                .map_err(device_error)?
                .min(take);
            self.counters.submitted_frames += accepted as u64;
            queue.backlog.drain(..accepted * CHANNELS);
            if accepted < take {
                break;
            }
        }

        // Resynchronization: even after offering everything the device had room for, the backlog
        // still exceeds its cap, so the engine is further ahead of real time than the policy
        // allows. Trim the oldest frames and report the gap.
        self.counters.max_backlog_frames = self
            .counters
            .max_backlog_frames
            .max(queue.backlog.len() / CHANNELS);
        let mut retained = total;
        if queue.backlog.len() > queue.backlog_cap_frames * CHANNELS {
            let excess = queue.backlog.len().div_ceil(CHANNELS) - queue.backlog_cap_frames;
            queue.backlog.drain(..excess * CHANNELS);
            // This submission sits at the back of the backlog, so it only loses frames once the
            // trim has consumed everything queued before it.
            retained = (backlogged_before + total)
                .min(queue.backlog_cap_frames)
                .saturating_sub(backlogged_before);
            self.counters.dropped_frames += excess as u64;
            self.counters.resyncs += 1;
            if !queue.warned_resync {
                queue.warned_resync = true;
                eprintln!(
                    "audio: resync: backlog above {MAX_BACKLOG_TICKS} ticks, dropped {} ms of \
                     oldest queued audio (further resyncs only counted)",
                    excess as f64 * 1000.0 / f64::from(SAMPLE_RATE)
                );
            }
        }

        if !queue.started && self.device.queued_frames() >= queue.prebuffer_frames {
            self.device.start().map_err(device_error)?;
            queue.started = true;
        }
        Ok(retained)
    }

    /// Frames currently queued on the device.
    #[must_use]
    pub fn queued_frames(&self) -> usize {
        self.device.queued_frames()
    }

    /// Frames produced but not yet accepted by the device (always zero in immediate mode).
    #[must_use]
    pub fn backlog_frames(&self) -> usize {
        self.queue
            .as_ref()
            .map_or(0, |queue| queue.backlog.len() / CHANNELS)
    }

    /// Snapshot of the flow-control counters.
    #[must_use]
    pub const fn counters(&self) -> AudioCounters {
        self.counters
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

    /// Asserts the accounting invariant: every produced frame was accepted by the device,
    /// dropped by a resync, or is still waiting in the backlog. (Accepted frames are either in
    /// the device queue or already consumed by it, so the device's own drain does not appear.)
    fn assert_frame_conservation(engine: &AudioEngine, produced: u64) {
        let counters = engine.counters();
        assert_eq!(counters.produced_frames, produced);
        assert_eq!(
            counters.produced_frames,
            counters.submitted_frames + counters.dropped_frames + engine.backlog_frames() as u64,
            "frames must be accounted for exactly once"
        );
    }

    #[test]
    fn a_full_device_buffers_a_burst_in_the_backlog_without_dropping() {
        // The device accepts everything but never drains: the device queue parks at its cap and
        // the rest of the burst waits in the backlog, so no produced tick is dropped until the
        // backlog itself is full.
        let (mut engine, state) = prebuffered_draining(0, 0, 0, 0);
        let tick = tick_buffer();
        let total_ticks = MAX_QUEUED_TICKS + MAX_BACKLOG_TICKS;
        for submit in 0..total_ticks {
            assert_eq!(engine.submit(&tick).unwrap(), FRAMES_PER_TICK);
            assert_eq!(
                engine.counters().dropped_frames,
                0,
                "submit {submit} dropped while within the backlog cap"
            );
        }
        assert!(state.started.get());
        assert_eq!(
            engine.queued_frames(),
            MAX_QUEUED_TICKS * FRAMES_PER_TICK,
            "the device queue parks at its high-water mark"
        );
        assert_eq!(
            engine.backlog_frames(),
            MAX_BACKLOG_TICKS * FRAMES_PER_TICK,
            "the backlog holds the overflow"
        );
        assert_frame_conservation(&engine, (total_ticks * FRAMES_PER_TICK) as u64);

        // One more tick exceeds the backlog cap: the oldest frame is trimmed, counted, and the
        // submission reports that none of its frames were retained.
        assert_eq!(engine.submit(&tick).unwrap(), 0);
        let counters = engine.counters();
        assert_eq!(counters.dropped_frames, FRAMES_PER_TICK as u64);
        assert_eq!(counters.resyncs, 1);
        assert_eq!(
            engine.backlog_frames(),
            MAX_BACKLOG_TICKS * FRAMES_PER_TICK,
            "a resync trims back to the backlog cap, not below it"
        );
        assert_frame_conservation(&engine, ((total_ticks + 1) * FRAMES_PER_TICK) as u64);
    }

    #[test]
    fn partial_device_acceptance_retains_the_remainder_in_the_backlog() {
        // A device that accepts one frame per call but never queues: each tick moves one frame
        // and the rest stays in the bounded backlog (production far outruns it, so resyncs are
        // expected and counted, never silent).
        let (device, calls) = MockDevice::new(1, 0);
        let mut engine = AudioEngine::with_prebuffer(Box::new(device)).unwrap();
        let tick = tick_buffer();
        let mut produced = 0u64;
        for _ in 0..16 {
            engine.submit(&tick).unwrap();
            produced += FRAMES_PER_TICK as u64;
            assert!(engine.backlog_frames() <= MAX_BACKLOG_TICKS * FRAMES_PER_TICK);
            assert!(engine.queued_frames() <= MAX_QUEUED_TICKS * FRAMES_PER_TICK);
            assert_frame_conservation(&engine, produced);
        }
        assert_eq!(
            calls.get(),
            16,
            "one flush attempt per tick; a partial acceptance is never retried within the tick"
        );
        assert_eq!(engine.counters().submitted_frames, 16);
        assert!(
            engine.counters().dropped_frames > 0,
            "a one-frame-per-tick device cannot keep up, so resyncs are reported"
        );
    }

    #[test]
    fn slow_draining_device_resyncs_with_counted_drops_instead_of_silent_skips() {
        // The device consumes one tick every third submit (1/3 real time), so production wins and
        // the queue climbs to the cap. The backlog then holds the excess; once it overflows the
        // engine trims oldest frames, and every trimmed frame is counted.
        let (mut engine, state) = prebuffered_draining(1, 3, 0, 0);
        let tick = tick_buffer();
        let device_cap = MAX_QUEUED_TICKS * FRAMES_PER_TICK;
        let backlog_cap = MAX_BACKLOG_TICKS * FRAMES_PER_TICK;
        let mut produced = 0u64;
        for submit in 0..300 {
            engine.submit(&tick).unwrap();
            produced += FRAMES_PER_TICK as u64;
            assert!(
                engine.queued_frames() <= device_cap,
                "submit {submit}: device queue past the cap"
            );
            assert!(
                engine.backlog_frames() <= backlog_cap,
                "submit {submit}: backlog past the cap"
            );
            assert_frame_conservation(&engine, produced);
        }
        let counters = engine.counters();
        assert!(state.started.get());
        assert_eq!(
            state.underruns.get(),
            0,
            "the device never ran dry while production outran it"
        );
        assert!(
            counters.resyncs > 0,
            "a permanently slow device must resync and count it"
        );
        assert!(
            counters.dropped_frames > 0,
            "a resync must report the frames it trimmed"
        );
        assert!(engine.queued_frames() >= FRAMES_PER_TICK);
    }

    #[test]
    fn started_device_that_ran_dry_is_counted_as_an_underrun() {
        let (mut engine, state) = prebuffered_draining(0, 0, 0, 0);
        let tick = tick_buffer();
        for _ in 0..PREBUFFER_TICKS {
            engine.submit(&tick).unwrap();
        }
        assert!(state.started.get());
        assert_eq!(engine.counters().underruns, 0);

        // The device is ahead of production: it consumes everything while the logic loop stalls,
        // three times in a row. Each empty queue is counted, no tick is dropped, and the new tick
        // is offered immediately.
        let mut produced = (PREBUFFER_TICKS * FRAMES_PER_TICK) as u64;
        for expected_underruns in 1..=3 {
            state.queued.set(0);
            engine.submit(&tick).unwrap();
            produced += FRAMES_PER_TICK as u64;
            assert_eq!(engine.counters().underruns, expected_underruns);
            assert_eq!(engine.counters().dropped_frames, 0, "no frames were lost");
            assert_eq!(engine.queued_frames(), FRAMES_PER_TICK);
            assert_frame_conservation(&engine, produced);
        }
    }

    #[test]
    fn jittered_drain_within_the_prebuffer_never_underruns_or_resyncs() {
        // Normal cadence drains one tick per submit; every fifth submit drains two (a full tick
        // of scheduling jitter) and the following submit drains none (the engine catches up).
        // The three-tick prebuffer keeps at least one tick queued throughout, and the backlog
        // absorbs the jitter without a resync.
        let (mut engine, state) = prebuffered_draining(1, 1, 1, 5);
        let tick = tick_buffer();
        let device_cap = MAX_QUEUED_TICKS * FRAMES_PER_TICK;
        let mut min_queued = usize::MAX;
        for submit in 0..240 {
            engine.submit(&tick).unwrap();
            let queued = engine.queued_frames();
            assert!(
                queued <= device_cap,
                "queue grew past the cap at submit {submit}"
            );
            if state.started.get() {
                min_queued = min_queued.min(queued);
            }
        }
        assert!(state.started.get());
        assert_eq!(state.underruns.get(), 0);
        assert_eq!(engine.counters().dropped_frames, 0);
        assert_eq!(engine.counters().resyncs, 0);
        assert!(
            min_queued >= FRAMES_PER_TICK,
            "a one-tick stall must leave at least one tick queued, saw {min_queued}"
        );
        assert_frame_conservation(&engine, (240 * FRAMES_PER_TICK) as u64);
    }

    #[test]
    fn stalled_device_parks_with_a_bounded_backlog_and_stays_prompt() {
        // The device reports a huge queue and accepts nothing. Submissions must not spin, and
        // the backlog must stay bounded (drops counted) instead of growing without limit.
        let (device, calls) = MockDevice::new(0, usize::MAX / 2);
        let mut engine = AudioEngine::with_prebuffer(Box::new(device)).unwrap();
        let tick = tick_buffer();
        let started = Instant::now();
        for _ in 0..64 {
            engine.submit(&tick).unwrap();
        }
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "submissions against a stalled device must be prompt"
        );
        assert_eq!(
            calls.get(),
            0,
            "a device reporting a full queue is never called; nothing can be accepted"
        );
        assert!(
            engine.backlog_frames() <= MAX_BACKLOG_TICKS * FRAMES_PER_TICK,
            "the backlog must stay bounded"
        );
        let counters = engine.counters();
        assert_eq!(counters.submitted_frames, 0);
        assert_eq!(
            counters.dropped_frames,
            (64 * FRAMES_PER_TICK - engine.backlog_frames()) as u64
        );
        assert!(counters.resyncs > 0);
    }

    #[test]
    fn immediate_mode_has_no_backlog_and_drops_unaccepted_frames() {
        // `AudioEngine::new` keeps the old immediate contract: one offer, caller drops the rest.
        let (device, calls) = MockDevice::new(0, 0);
        let mut engine = AudioEngine::new(Box::new(device)).unwrap();
        let tick = tick_buffer();
        for _ in 0..8 {
            assert_eq!(engine.submit(&tick).unwrap(), 0);
        }
        assert_eq!(calls.get(), 8);
        assert_eq!(engine.backlog_frames(), 0);
        assert_eq!(engine.counters().dropped_frames, 0);
        assert_eq!(engine.counters().submitted_frames, 0);
    }
}
