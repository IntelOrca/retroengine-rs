//! SDL3 backend providing a real window, streaming RGB565 texture, audio stream and device input.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use ::sdl3::audio::{AudioFormat, AudioSpec, AudioStreamOwner};
use ::sdl3::event::Event;
use ::sdl3::gamepad::{Button, Gamepad};
use ::sdl3::keyboard::Scancode;
use ::sdl3::render::WindowCanvas;
use sdl3_sys::render::SDL_Texture;

use crate::{
    AudioDesc, AudioDevice, Clock, InputSource, InputState, Platform, PlatformError, Storage,
    TARGET_FPS, Window, WindowDesc,
};

static SDL3_INIT_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Number of times the SDL3 backend has initialized SDL in this process.
#[must_use]
pub fn init_count() -> usize {
    SDL3_INIT_COUNT.load(Ordering::Relaxed)
}

/// SDL3 platform backend.
///
/// `init` initializes SDL's core and the event pump only; the video and audio subsystems are
/// initialized on demand by `create_window` and `open_audio`, so `init` succeeds on machines
/// without a display or sound device. All SDL handles are owned and released by `shutdown`,
/// which lets the last `Sdl` handle drop and therefore run `SDL_Quit`, making
/// `init -> shutdown -> init` safe. Windows and audio devices returned by this backend do not
/// borrow from the platform, but callers must drop them before calling `shutdown`.
pub struct Sdl3Platform {
    initialized: bool,
    input: Sdl3Input,
    storage: FsStorage,
    clock: SystemClock,
    sdl: Option<::sdl3::Sdl>,
}

impl Sdl3Platform {
    /// Creates an uninitialized SDL3 platform.
    #[must_use]
    pub fn new() -> Self {
        let quit = Arc::new(AtomicBool::new(false));
        Self {
            initialized: false,
            input: Sdl3Input {
                events: None,
                gamepads: Vec::new(),
                quit,
                state: InputState::new(),
            },
            storage: FsStorage::new("retroengine-user"),
            clock: SystemClock::new(),
            sdl: None,
        }
    }

    /// Overrides the root directory used by [`Storage`].
    pub fn set_storage_root(&mut self, root: impl Into<PathBuf>) {
        self.storage = FsStorage::new(root);
    }
}

impl Default for Sdl3Platform {
    fn default() -> Self {
        Self::new()
    }
}

impl Platform for Sdl3Platform {
    fn name(&self) -> &'static str {
        "sdl3"
    }

    fn init(&mut self) -> Result<(), PlatformError> {
        if self.initialized {
            return Ok(());
        }
        let sdl = ::sdl3::init().map_err(PlatformError::sdl)?;
        self.input.events = Some(sdl.event_pump().map_err(PlatformError::sdl)?);
        if let Ok(gamepad) = sdl.gamepad() {
            for id in gamepad.gamepads().unwrap_or_default() {
                if let Ok(opened) = gamepad.open(id) {
                    self.input.gamepads.push(opened);
                }
            }
        }
        self.sdl = Some(sdl);
        SDL3_INIT_COUNT.fetch_add(1, Ordering::Relaxed);
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> Result<(), PlatformError> {
        self.input.events = None;
        self.input.gamepads.clear();
        self.sdl = None;
        self.initialized = false;
        Ok(())
    }

    fn is_initialized(&self) -> bool {
        self.initialized
    }

    fn create_window(&mut self, desc: WindowDesc) -> Result<Box<dyn Window>, PlatformError> {
        let sdl = self.sdl.as_ref().ok_or(PlatformError::NotInitialized)?;
        if desc.width == 0 || desc.height == 0 {
            return Err(PlatformError::InvalidArgument(
                "window dimensions must be non-zero".to_owned(),
            ));
        }
        let video = sdl.video().map_err(PlatformError::sdl)?;
        let window = video
            .window(&desc.title, desc.width, desc.height)
            .position_centered()
            .resizable()
            .build()
            .map_err(PlatformError::other)?;
        let canvas = window.into_canvas();
        let renderer = canvas.raw();
        let texture = unsafe {
            sdl3_sys::render::SDL_CreateTexture(
                renderer,
                sdl3_sys::pixels::SDL_PIXELFORMAT_RGB565,
                sdl3_sys::render::SDL_TEXTUREACCESS_STREAMING,
                desc.width as i32,
                desc.height as i32,
            )
        };
        if texture.is_null() {
            return Err(PlatformError::sdl(::sdl3::get_error()));
        }
        unsafe {
            sdl3_sys::render::SDL_SetTextureScaleMode(
                texture,
                sdl3_sys::everything::SDL_SCALEMODE_NEAREST,
            );
            let presentation = if desc.integer_scale {
                sdl3_sys::render::SDL_LOGICAL_PRESENTATION_INTEGER_SCALE
            } else {
                sdl3_sys::render::SDL_LOGICAL_PRESENTATION_LETTERBOX
            };
            sdl3_sys::render::SDL_SetRenderLogicalPresentation(
                renderer,
                desc.width as i32,
                desc.height as i32,
                presentation,
            );
            sdl3_sys::render::SDL_SetRenderVSync(renderer, i32::from(desc.vsync));
        }
        Ok(Box::new(Sdl3Window {
            canvas,
            texture,
            width: desc.width,
            height: desc.height,
            quit: self.input.quit.clone(),
        }))
    }

    fn open_audio(&mut self, desc: AudioDesc) -> Result<Box<dyn AudioDevice>, PlatformError> {
        let sdl = self.sdl.as_ref().ok_or(PlatformError::NotInitialized)?;
        if desc.channels != 2 {
            return Err(PlatformError::InvalidArgument(
                "SDL3 audio output is stereo only".to_owned(),
            ));
        }
        let audio = sdl.audio().map_err(PlatformError::sdl)?;
        let spec = AudioSpec {
            freq: Some(desc.sample_rate as i32),
            channels: Some(desc.channels as i32),
            format: Some(AudioFormat::f32_sys()),
        };
        let device = audio
            .open_playback_device(&spec)
            .map_err(PlatformError::sdl)?;
        let stream = device
            .open_device_stream(Some(&spec))
            .map_err(PlatformError::sdl)?;
        stream.resume().map_err(PlatformError::sdl)?;
        Ok(Box::new(Sdl3Audio {
            stream,
            sample_rate: desc.sample_rate,
            channels: desc.channels,
        }))
    }

    fn input(&mut self) -> &mut dyn InputSource {
        &mut self.input
    }

    fn storage(&mut self) -> &mut dyn Storage {
        &mut self.storage
    }

    fn clock(&mut self) -> &mut dyn Clock {
        &mut self.clock
    }
}

/// SDL3 window presenting a streaming RGB565 texture with nearest scaling.
pub struct Sdl3Window {
    canvas: WindowCanvas,
    texture: *mut SDL_Texture,
    width: u32,
    height: u32,
    quit: Arc<AtomicBool>,
}

impl Drop for Sdl3Window {
    fn drop(&mut self) {
        unsafe {
            sdl3_sys::render::SDL_DestroyTexture(self.texture);
        }
    }
}

impl Window for Sdl3Window {
    fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    fn present(
        &mut self,
        framebuffer: &[u16],
        width: u32,
        height: u32,
    ) -> Result<(), PlatformError> {
        if width != self.width || height != self.height {
            return Err(PlatformError::InvalidArgument(format!(
                "frame {width}x{height} does not match window {}x{}",
                self.width, self.height
            )));
        }
        if framebuffer.len() != width as usize * height as usize {
            return Err(PlatformError::InvalidArgument(format!(
                "framebuffer of {} pixels does not match {width}x{height}",
                framebuffer.len()
            )));
        }
        let renderer = self.canvas.raw();
        let pitch = (width * 2) as i32;
        let updated = unsafe {
            sdl3_sys::render::SDL_UpdateTexture(
                self.texture,
                std::ptr::null(),
                framebuffer.as_ptr().cast::<std::ffi::c_void>(),
                pitch,
            )
        };
        if !updated {
            return Err(PlatformError::sdl(::sdl3::get_error()));
        }
        unsafe {
            sdl3_sys::render::SDL_SetRenderDrawColor(renderer, 0, 0, 0, 255);
            sdl3_sys::render::SDL_RenderClear(renderer);
            let copied = sdl3_sys::render::SDL_RenderTexture(
                renderer,
                self.texture,
                std::ptr::null(),
                std::ptr::null(),
            );
            if !copied {
                return Err(PlatformError::sdl(::sdl3::get_error()));
            }
            if !sdl3_sys::render::SDL_RenderPresent(renderer) {
                return Err(PlatformError::sdl(::sdl3::get_error()));
            }
        }
        Ok(())
    }

    fn readback(&self) -> Result<Vec<u16>, PlatformError> {
        Err(PlatformError::Unsupported(
            "SDL3 renderer readback is not implemented yet".to_owned(),
        ))
    }

    fn set_title(&mut self, title: &str) {
        let _ = self.canvas.window_mut().set_title(title);
    }

    fn should_close(&self) -> bool {
        self.quit.load(Ordering::Relaxed)
    }
}

/// SDL3 audio stream accepting interleaved f32 samples.
pub struct Sdl3Audio {
    stream: AudioStreamOwner,
    sample_rate: u32,
    channels: u8,
}

impl AudioDevice for Sdl3Audio {
    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn channels(&self) -> u8 {
        self.channels
    }

    fn submit(&mut self, frames: &[f32]) -> Result<usize, PlatformError> {
        let channels = self.channels as usize;
        if channels == 0 || !frames.len().is_multiple_of(channels) {
            return Err(PlatformError::InvalidArgument(
                "sample count is not a whole number of frames".to_owned(),
            ));
        }
        self.stream
            .put_data_f32(frames)
            .map_err(PlatformError::sdl)?;
        Ok(frames.len() / channels)
    }

    fn queued_frames(&self) -> usize {
        let bytes = self.stream.queued_bytes().unwrap_or(0).max(0) as usize;
        bytes / (std::mem::size_of::<f32>() * self.channels.max(1) as usize)
    }

    fn close(&mut self) -> Result<(), PlatformError> {
        Ok(())
    }
}

/// SDL3 input source polling the keyboard and any opened gamepads.
pub struct Sdl3Input {
    events: Option<::sdl3::EventPump>,
    gamepads: Vec<Gamepad>,
    quit: Arc<AtomicBool>,
    state: InputState,
}

impl InputSource for Sdl3Input {
    fn poll(&mut self) -> InputState {
        let Some(events) = self.events.as_mut() else {
            return InputState::new();
        };
        for event in events.poll_iter() {
            if let Event::Quit { .. } = event {
                self.quit.store(true, Ordering::Relaxed);
            }
        }
        let keyboard = events.keyboard_state();
        let mut state = InputState::new();
        state.connected = true;
        state.up =
            keyboard.is_scancode_pressed(Scancode::Up) || keyboard.is_scancode_pressed(Scancode::W);
        state.down = keyboard.is_scancode_pressed(Scancode::Down)
            || keyboard.is_scancode_pressed(Scancode::S);
        state.left = keyboard.is_scancode_pressed(Scancode::Left)
            || keyboard.is_scancode_pressed(Scancode::A);
        state.right = keyboard.is_scancode_pressed(Scancode::Right)
            || keyboard.is_scancode_pressed(Scancode::D);
        state.a = keyboard.is_scancode_pressed(Scancode::Space)
            || keyboard.is_scancode_pressed(Scancode::Z);
        state.b = keyboard.is_scancode_pressed(Scancode::X);
        state.c = keyboard.is_scancode_pressed(Scancode::C);
        state.start = keyboard.is_scancode_pressed(Scancode::Return)
            || keyboard.is_scancode_pressed(Scancode::Escape);
        for gamepad in &self.gamepads {
            state.connected = true;
            state.up |= gamepad.button(Button::DPadUp);
            state.down |= gamepad.button(Button::DPadDown);
            state.left |= gamepad.button(Button::DPadLeft);
            state.right |= gamepad.button(Button::DPadRight);
            state.a |= gamepad.button(Button::South);
            state.b |= gamepad.button(Button::East);
            state.c |= gamepad.button(Button::North);
            state.start |= gamepad.button(Button::Start);
        }
        self.state = state;
        self.state
    }
}

/// Filesystem-backed user storage rooted at a base directory.
#[derive(Debug)]
pub struct FsStorage {
    root: PathBuf,
}

impl FsStorage {
    /// Creates storage rooted at `root`.
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The storage root.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }
}

impl Storage for FsStorage {
    fn read(&self, path: &str) -> Result<Vec<u8>, PlatformError> {
        Ok(std::fs::read(self.root.join(path))?)
    }

    fn write(&mut self, path: &str, data: &[u8]) -> Result<(), PlatformError> {
        let full = self.root.join(path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(full, data)?;
        Ok(())
    }

    fn exists(&self, path: &str) -> bool {
        self.root.join(path).exists()
    }

    fn list(&self, dir: &str) -> Result<Vec<String>, PlatformError> {
        let full = self.root.join(dir);
        if !full.exists() {
            return Ok(Vec::new());
        }
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(full)? {
            entries.push(entry?.file_name().to_string_lossy().into_owned());
        }
        entries.sort();
        Ok(entries)
    }

    fn remove(&mut self, path: &str) -> Result<(), PlatformError> {
        match std::fs::remove_file(self.root.join(path)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}

/// Wall-clock driven fixed-step clock.
pub struct SystemClock {
    start: Instant,
    frame: u64,
}

impl SystemClock {
    /// Creates a clock starting now.
    #[must_use]
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
            frame: 0,
        }
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for SystemClock {
    fn frame(&self) -> u64 {
        self.frame
    }

    fn now_ms(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }

    fn advance_frame(&mut self) {
        self.frame += 1;
    }

    fn sleep_until_next_frame(&self) -> Result<(), PlatformError> {
        let target = Duration::from_micros(self.frame * 1_000_000 / TARGET_FPS);
        let elapsed = self.start.elapsed();
        if target > elapsed {
            std::thread::sleep(target - elapsed);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fs_storage_round_trips() {
        let root = std::env::temp_dir().join(format!("retro-platform-test-{}", std::process::id()));
        let mut storage = FsStorage::new(&root);
        storage.write("nested/save.dat", b"data").unwrap();
        assert!(storage.exists("nested/save.dat"));
        assert_eq!(storage.read("nested/save.dat").unwrap(), b"data");
        assert_eq!(storage.list("nested").unwrap(), vec!["save.dat"]);
        storage.remove("nested/save.dat").unwrap();
        assert!(!storage.exists("nested/save.dat"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn sdl3_lifecycle_init_shutdown_reinit() {
        let _guard = crate::platform_test_lock();
        let before = init_count();
        let mut platform = Sdl3Platform::new();
        assert!(!platform.is_initialized());

        platform.init().unwrap();
        assert!(platform.is_initialized());
        platform.shutdown().unwrap();
        assert!(!platform.is_initialized());

        platform.init().unwrap();
        assert!(platform.is_initialized());
        platform.shutdown().unwrap();
        assert!(!platform.is_initialized());
        assert_eq!(init_count(), before + 2);
    }

    #[test]
    fn sdl3_shutdown_without_init_is_safe() {
        let _guard = crate::platform_test_lock();
        let mut platform = Sdl3Platform::new();
        platform.shutdown().unwrap();
        assert!(!platform.is_initialized());
    }
}
