//! Runtime-selectable platform backends (headless and SDL3) for video, audio, input, storage and timing.
//!
//! This is the only crate permitted to link SDL3 or touch OS services directly.

pub mod error;
pub mod headless;
#[cfg(feature = "sdl3")]
pub mod sdl3;
pub mod storage;

pub use error::PlatformError;
pub use storage::FsStorage;

/// The engine steps at a fixed 60 Hz.
pub const TARGET_FPS: u64 = 60;

/// Duration of one engine frame in milliseconds (truncated).
pub const FRAME_MS: u64 = 1000 / TARGET_FPS;

/// Description of a window to create.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowDesc {
    /// Window title.
    pub title: String,
    /// Logical framebuffer width in pixels.
    pub width: u32,
    /// Logical framebuffer height in pixels.
    pub height: u32,
    /// Prefer integer scaling when presenting.
    pub integer_scale: bool,
    /// Enable vsync when presenting.
    pub vsync: bool,
    /// Start windowed rather than fullscreen (`Settings.ini` `windowed`).
    pub windowed: bool,
    /// Draw window decorations (`Settings.ini` `border`).
    pub border: bool,
    /// Request exclusive fullscreen (`Settings.ini` `exclusiveFS`).
    ///
    /// SDL3's fullscreen flag already presents the desktop fullscreen; this flag records the
    /// request and is applied as a borderless fullscreen window rather than a mode switch.
    pub exclusive_fullscreen: bool,
}

impl WindowDesc {
    /// Creates a window description with nearest-neighbour integer scaling, vsync enabled,
    /// windowed and bordered.
    #[must_use]
    pub fn new(title: impl Into<String>, width: u32, height: u32) -> Self {
        Self {
            title: title.into(),
            width,
            height,
            integer_scale: true,
            vsync: true,
            windowed: true,
            border: true,
            exclusive_fullscreen: false,
        }
    }
}

/// Description of an audio device to open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioDesc {
    /// Output sample rate in Hz.
    pub sample_rate: u32,
    /// Number of interleaved channels; the engine uses stereo.
    pub channels: u8,
}

impl AudioDesc {
    /// Creates a stereo audio description.
    #[must_use]
    pub const fn stereo(sample_rate: u32) -> Self {
        Self {
            sample_rate,
            channels: 2,
        }
    }
}

impl Default for AudioDesc {
    fn default() -> Self {
        Self::stereo(44_100)
    }
}

/// The available platform backends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendKind {
    /// In-memory deterministic backend used for tests, CI and replay capture.
    Headless,
    /// SDL3 backend with a real window, audio stream and input devices.
    Sdl3,
}

impl BackendKind {
    /// Returns the backend's stable name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Headless => "headless",
            Self::Sdl3 => "sdl3",
        }
    }
}

/// Creates the requested backend.
pub fn create(kind: BackendKind) -> Result<Box<dyn Platform>, PlatformError> {
    match kind {
        BackendKind::Headless => Ok(Box::new(headless::HeadlessPlatform::new())),
        #[cfg(feature = "sdl3")]
        BackendKind::Sdl3 => Ok(Box::new(sdl3::Sdl3Platform::new())),
        #[cfg(not(feature = "sdl3"))]
        BackendKind::Sdl3 => Err(PlatformError::Unsupported(
            "built without the sdl3 feature".to_owned(),
        )),
    }
}

/// Versioned input state polled once per frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputState {
    /// Version of this struct; incremented when fields change meaning.
    pub version: u32,
    /// Whether a keyboard or gamepad is available.
    pub connected: bool,
    /// D-pad up.
    pub up: bool,
    /// D-pad down.
    pub down: bool,
    /// D-pad left.
    pub left: bool,
    /// D-pad right.
    pub right: bool,
    /// Primary action button.
    pub a: bool,
    /// Secondary action button.
    pub b: bool,
    /// Tertiary action button.
    pub c: bool,
    /// Start button.
    pub start: bool,
}

impl InputState {
    /// Current version of the input state layout.
    pub const VERSION: u32 = 1;

    /// Creates a neutral state with no buttons pressed.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            version: Self::VERSION,
            connected: false,
            up: false,
            down: false,
            left: false,
            right: false,
            a: false,
            b: false,
            c: false,
            start: false,
        }
    }
}

impl Default for InputState {
    fn default() -> Self {
        Self::new()
    }
}

/// A platform backend.
pub trait Platform {
    /// Stable backend name.
    fn name(&self) -> &'static str;
    /// Initializes backend resources.
    fn init(&mut self) -> Result<(), PlatformError>;
    /// Releases backend resources.
    fn shutdown(&mut self) -> Result<(), PlatformError>;
    /// Whether `init` has completed successfully.
    fn is_initialized(&self) -> bool;
    /// Creates a window that presents u16 RGB565 frames.
    fn create_window(&mut self, desc: WindowDesc) -> Result<Box<dyn Window>, PlatformError>;
    /// Opens a stereo f32 audio device.
    fn open_audio(&mut self, desc: AudioDesc) -> Result<Box<dyn AudioDevice>, PlatformError>;
    /// Returns the input source for this backend.
    fn input(&mut self) -> &mut dyn InputSource;
    /// Returns user-file storage for this backend.
    fn storage(&mut self) -> &mut dyn Storage;
    /// Returns the frame clock for this backend.
    fn clock(&mut self) -> &mut dyn Clock;
    /// Native video driver name for startup diagnostics, when the backend has one.
    ///
    /// Backends without a display (or before one is initialized) return `None`.
    fn video_driver(&self) -> Option<&'static str> {
        None
    }
}

/// A presentable RGB565 framebuffer target.
pub trait Window {
    /// Logical size of the window in pixels.
    fn size(&self) -> (u32, u32);
    /// Presents a framebuffer of `width * height` u16 RGB565 pixels.
    fn present(
        &mut self,
        framebuffer: &[u16],
        width: u32,
        height: u32,
    ) -> Result<(), PlatformError>;
    /// Reads back the most recently presented framebuffer.
    fn readback(&self) -> Result<Vec<u16>, PlatformError>;
    /// Updates the window title.
    fn set_title(&mut self, title: &str);
    /// Whether the user asked to close the window.
    fn should_close(&self) -> bool;
}

/// A stereo f32 sample sink.
pub trait AudioDevice {
    /// Device sample rate in Hz.
    fn sample_rate(&self) -> u32;
    /// Number of interleaved channels.
    fn channels(&self) -> u8;
    /// Queues interleaved stereo samples; returns the number of frames accepted.
    ///
    /// Implementations must return promptly instead of spinning or waiting for queue space:
    /// when the queue is full they should accept what fits (possibly nothing) and let the
    /// caller decide what to do with the rest (`retro_audio::AudioEngine` retains unaccepted
    /// frames in a bounded backlog in prebuffered mode, and drops them in immediate mode).
    fn submit(&mut self, frames: &[f32]) -> Result<usize, PlatformError>;
    /// Frames currently queued for playback.
    fn queued_frames(&self) -> usize;
    /// Starts playback once the caller has queued its prebuffer.
    ///
    /// Some backends (SDL's simplified device stream among them) open the device paused so the
    /// caller can fill the queue before the hardware starts draining it. Backends that start
    /// immediately, and capture-only backends, keep the no-op default. Callers may start more
    /// than once; implementations should make repeated starts harmless.
    fn start(&mut self) -> Result<(), PlatformError> {
        Ok(())
    }
    /// Closes the device.
    fn close(&mut self) -> Result<(), PlatformError>;
    /// Human-readable device/driver description for startup diagnostics.
    fn description(&self) -> String {
        "unknown".to_owned()
    }
}

/// Raw device state captured by one input poll, free of backend-specific types.
///
/// Backends that support it return this from [`InputSource::poll_raw`]. The [`RawInput::keys`]
/// slice is indexed by SDL scancode number (`0..retro_input::KEY_COUNT`); use
/// [`RawInput::key_down`] for a bounds-checked lookup. Gamepads are already normalized to
/// [`retro_input::GamepadState`] and can be fed to [`retro_input::InputMappings::apply`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RawInput {
    /// Keyboard state indexed by SDL scancode number.
    pub keys: Vec<bool>,
    /// Gamepad state per player slot, in slot order.
    pub gamepads: [retro_input::GamepadState; retro_input::PLAYER_COUNT],
    /// Active touch points; only the first [`RawInput::touch_count`] entries are valid.
    pub touches: [retro_input::TouchPoint; retro_input::MAX_TOUCHES],
    /// Number of active touch points.
    pub touch_count: u8,
}

impl RawInput {
    /// Whether the given SDL scancode is held, treating unknown scancodes as released.
    #[must_use]
    pub fn key_down(&self, scancode: u32) -> bool {
        self.keys.get(scancode as usize).copied().unwrap_or(false)
    }

    /// Whether any gamepad slot is connected.
    #[must_use]
    pub fn has_gamepad(&self) -> bool {
        self.gamepads.iter().any(|gamepad| gamepad.connected)
    }
}

/// A pollable source of versioned input state.
pub trait InputSource {
    /// Version of the input state produced by this source.
    fn version(&self) -> u32 {
        InputState::VERSION
    }
    /// Polls the current input state.
    fn poll(&mut self) -> InputState;
    /// Polls raw per-slot device state through the shared [`retro_input`] model.
    ///
    /// Backends without a native implementation (and uninitialized ones) return an empty
    /// [`RawInput`]; callers must treat that as "no devices".
    fn poll_raw(&mut self) -> RawInput {
        RawInput::default()
    }
}

/// User-file storage.
pub trait Storage {
    /// Reads a file relative to the storage root.
    fn read(&self, path: &str) -> Result<Vec<u8>, PlatformError>;
    /// Writes a file relative to the storage root.
    fn write(&mut self, path: &str, data: &[u8]) -> Result<(), PlatformError>;
    /// Whether a file exists.
    fn exists(&self, path: &str) -> bool;
    /// Lists immediate children of a directory.
    fn list(&self, dir: &str) -> Result<Vec<String>, PlatformError>;
    /// Removes a file.
    fn remove(&mut self, path: &str) -> Result<(), PlatformError>;
    /// Renames `from` to `to`, replacing `to` when it already exists.
    ///
    /// Backends with an atomic native rename override this so callers can write a temporary file
    /// and swap it in without a window where the destination is missing or truncated. The default
    /// implementation reads the source, writes the destination and removes the source, which is
    /// sufficient for in-memory storage but not crash-safe.
    fn rename(&mut self, from: &str, to: &str) -> Result<(), PlatformError> {
        let data = self.read(from)?;
        self.write(to, &data)?;
        self.remove(from)
    }
}

impl Storage for Box<dyn Storage> {
    fn read(&self, path: &str) -> Result<Vec<u8>, PlatformError> {
        (**self).read(path)
    }

    fn write(&mut self, path: &str, data: &[u8]) -> Result<(), PlatformError> {
        (**self).write(path, data)
    }

    fn exists(&self, path: &str) -> bool {
        (**self).exists(path)
    }

    fn list(&self, dir: &str) -> Result<Vec<String>, PlatformError> {
        (**self).list(dir)
    }

    fn remove(&mut self, path: &str) -> Result<(), PlatformError> {
        (**self).remove(path)
    }

    fn rename(&mut self, from: &str, to: &str) -> Result<(), PlatformError> {
        (**self).rename(from, to)
    }
}

/// Fixed-step frame clock.
pub trait Clock {
    /// Number of completed frames.
    fn frame(&self) -> u64;
    /// Milliseconds elapsed since startup.
    fn now_ms(&self) -> u64;
    /// Advances the clock by one engine frame.
    fn advance_frame(&mut self);
    /// Blocks until the next frame is due.
    fn sleep_until_next_frame(&self) -> Result<(), PlatformError>;
}

/// Returns the preferred per-user data directory for the engine.
///
/// With the SDL3 feature this is `SDL_GetPrefPath("retroengine-rs", "retroengine")` (the same
/// location the windowed backend writes user data to); without it there is no OS preference and
/// `None` is returned. Callers should fall back to their own directory or in-memory storage.
#[must_use]
pub fn user_data_dir() -> Option<std::path::PathBuf> {
    #[cfg(feature = "sdl3")]
    {
        ::sdl3::filesystem::get_pref_path("retroengine-rs", "retroengine").ok()
    }
    #[cfg(not(feature = "sdl3"))]
    {
        None
    }
}

/// Number of times the SDL3 backend has initialized SDL in this process.
#[must_use]
pub fn sdl3_init_count() -> usize {
    #[cfg(feature = "sdl3")]
    {
        sdl3::init_count()
    }
    #[cfg(not(feature = "sdl3"))]
    {
        0
    }
}

/// Serializes tests that observe or mutate the process-wide SDL initialization state.
#[cfg(test)]
pub(crate) fn platform_test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
