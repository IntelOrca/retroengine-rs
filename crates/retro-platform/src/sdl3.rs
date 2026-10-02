//! SDL3 backend providing a real window, streaming RGB565 texture, audio stream and device input.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use ::sdl3::GamepadSubsystem;
use ::sdl3::audio::{AudioFormat, AudioSpec, AudioStreamOwner};
use ::sdl3::event::{Event, WindowEvent};
use ::sdl3::gamepad::{Axis, Button, Gamepad};
use ::sdl3::joystick::JoystickId;
use ::sdl3::keyboard::{KeyboardState, Scancode};
use ::sdl3::mouse::MouseButton;
use ::sdl3::render::WindowCanvas;
use sdl3_sys::render::SDL_Texture;

use crate::{
    AudioDesc, AudioDevice, Clock, FsStorage, InputSource, InputState, Platform, PlatformError,
    RawInput, Storage, TARGET_FPS, Window, WindowDesc,
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
    storage_root_override: bool,
    clock: SystemClock,
    sdl: Option<::sdl3::Sdl>,
}

impl Sdl3Platform {
    /// Creates an uninitialized SDL3 platform.
    #[must_use]
    pub fn new() -> Self {
        Self {
            initialized: false,
            input: Sdl3Input::new(Arc::new(AtomicBool::new(false))),
            storage: FsStorage::new("retroengine-user"),
            storage_root_override: false,
            clock: SystemClock::new(),
            sdl: None,
        }
    }

    /// Overrides the root directory used by [`Storage`].
    ///
    /// An explicit override also survives [`Platform::init`]; otherwise `init` roots the storage
    /// at [`crate::user_data_dir`] (the SDL preferred path) when one is available.
    pub fn set_storage_root(&mut self, root: impl Into<PathBuf>) {
        self.storage = FsStorage::new(root);
        self.storage_root_override = true;
    }

    /// The directory [`Storage`] is rooted at.
    #[must_use]
    pub fn storage_root(&self) -> &Path {
        self.storage.root()
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
        self.input.attach(&sdl)?;
        self.sdl = Some(sdl);
        if !self.storage_root_override
            && let Some(root) = crate::user_data_dir()
        {
            self.storage = FsStorage::new(root);
        }
        SDL3_INIT_COUNT.fetch_add(1, Ordering::Relaxed);
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> Result<(), PlatformError> {
        self.input.detach();
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
        self.input
            .set_window((desc.width, desc.height), desc.integer_scale);
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

/// Digital trigger threshold in raw SDL axis units, equivalent to the upstream 0.3 deadzone.
pub const TRIGGER_DEADZONE: i16 = 9830;

/// Plain gamepad button snapshot for [`gamepad_state`], decoupled from SDL types so the mapping
/// can be unit-tested without initializing SDL.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GamepadButtons {
    /// North face button.
    pub north: bool,
    /// East face button.
    pub east: bool,
    /// South face button.
    pub south: bool,
    /// West face button.
    pub west: bool,
    /// Back/select button.
    pub back: bool,
    /// Guide button.
    pub guide: bool,
    /// Start button.
    pub start: bool,
    /// Left shoulder button.
    pub left_shoulder: bool,
    /// Right shoulder button.
    pub right_shoulder: bool,
    /// D-pad up.
    pub dpad_up: bool,
    /// D-pad down.
    pub dpad_down: bool,
    /// D-pad left.
    pub dpad_left: bool,
    /// D-pad right.
    pub dpad_right: bool,
    /// Left trigger held past [`TRIGGER_DEADZONE`].
    pub trigger_left: bool,
    /// Right trigger held past [`TRIGGER_DEADZONE`].
    pub trigger_right: bool,
}

/// Converts plain button/axis data into the shared [`retro_input::GamepadState`].
///
/// The mapping mirrors the decompilation's `[Controller 1]` defaults: A/B/C/X are the
/// south/east/north/west faces, Y/Z are the left/right triggers and L/R the shoulders. Upstream
/// binds Select to the guide button; this port also accepts Back so pads without an exposed guide
/// button can select. D-pad bits map straight to directions and the raw left stick is passed
/// through, letting [`retro_input::InputMappings::apply`] derive stick directions with the shared
/// deadzone.
#[must_use]
pub fn gamepad_state(
    connected: bool,
    buttons: GamepadButtons,
    axis_x: i16,
    axis_y: i16,
) -> retro_input::GamepadState {
    use retro_input::ButtonState;

    let mut held = ButtonState::NONE;
    let mut set = |pressed: bool, flag: ButtonState| {
        if pressed {
            held.insert(flag);
        }
    };
    set(buttons.south, ButtonState::A);
    set(buttons.east, ButtonState::B);
    set(buttons.north, ButtonState::C);
    set(buttons.west, ButtonState::X);
    set(buttons.trigger_left, ButtonState::Y);
    set(buttons.trigger_right, ButtonState::Z);
    set(buttons.left_shoulder, ButtonState::L);
    set(buttons.right_shoulder, ButtonState::R);
    set(buttons.start, ButtonState::START);
    set(buttons.guide || buttons.back, ButtonState::SELECT);
    set(buttons.dpad_up, ButtonState::UP);
    set(buttons.dpad_down, ButtonState::DOWN);
    set(buttons.dpad_left, ButtonState::LEFT);
    set(buttons.dpad_right, ButtonState::RIGHT);

    retro_input::GamepadState {
        connected,
        held,
        axis_x,
        axis_y,
    }
}

/// Reads one open SDL gamepad into the shared plain-data model.
fn read_gamepad(gamepad: &Gamepad) -> retro_input::GamepadState {
    let buttons = GamepadButtons {
        north: gamepad.button(Button::North),
        east: gamepad.button(Button::East),
        south: gamepad.button(Button::South),
        west: gamepad.button(Button::West),
        back: gamepad.button(Button::Back),
        guide: gamepad.button(Button::Guide),
        start: gamepad.button(Button::Start),
        left_shoulder: gamepad.button(Button::LeftShoulder),
        right_shoulder: gamepad.button(Button::RightShoulder),
        dpad_up: gamepad.button(Button::DPadUp),
        dpad_down: gamepad.button(Button::DPadDown),
        dpad_left: gamepad.button(Button::DPadLeft),
        dpad_right: gamepad.button(Button::DPadRight),
        trigger_left: gamepad.axis(Axis::TriggerLeft) > TRIGGER_DEADZONE,
        trigger_right: gamepad.axis(Axis::TriggerRight) > TRIGGER_DEADZONE,
    };
    gamepad_state(
        true,
        buttons,
        gamepad.axis(Axis::LeftX),
        gamepad.axis(Axis::LeftY),
    )
}

/// Copies the SDL keyboard state into a scancode-indexed boolean array.
fn keyboard_array(keyboard: &KeyboardState<'_>) -> Vec<bool> {
    let mut keys = vec![false; retro_input::KEY_COUNT];
    for (scancode, pressed) in keyboard.scancodes() {
        if let Some(slot) = keys.get_mut(scancode.to_i32() as usize) {
            *slot = pressed;
        }
    }
    keys
}

/// One finger currently touching the window.
struct ActiveTouch {
    finger_id: u64,
    x: f32,
    y: f32,
}

/// Logical screen width touches are scaled to (`SCREEN_XSIZE`).
pub const TOUCH_SCREEN_XSIZE: i16 = 424;
/// Logical screen height touches are scaled to (`SCREEN_YSIZE`).
pub const TOUCH_SCREEN_YSIZE: i16 = 240;

/// Scales a normalized `0..=1` SDL finger coordinate into logical screen pixels.
///
/// Upstream assigns `touchX = finger->x * SCREEN_XSIZE` and `touchY = finger->y * SCREEN_YSIZE`
/// (`RetroEngine.cpp:119-122`), so scripts see `CheckTouchRect`-style logical coordinates rather
/// than a full `i16` range.
fn normalize_touch(value: f32, size: i16) -> i16 {
    if !value.is_finite() {
        return 0;
    }
    (value.clamp(0.0, 1.0) * f32::from(size)).round() as i16
}

/// Mapping from window-space points to the logical 424x240 touchscreen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct WindowMapping {
    /// Current window size in window points; updated on `WindowResized`.
    size: (u32, u32),
    /// Logical presentation size (the framebuffer size given to `create_window`).
    logical: (u32, u32),
    /// Whether the renderer presents with integer scaling (letterboxing bars when aspect
    /// differs).
    integer_scale: bool,
}

impl Default for WindowMapping {
    fn default() -> Self {
        let logical = (TOUCH_SCREEN_XSIZE as u32, TOUCH_SCREEN_YSIZE as u32);
        Self {
            size: logical,
            logical,
            integer_scale: true,
        }
    }
}

/// Maps a window-space mouse point onto the engine's logical screen coordinates.
///
/// Mirrors upstream's `displaySettings.width`/`offsetX` mapping (`RetroEngine.cpp:80-92`): the
/// logical image is scaled to fit the window (floored to an integer when integer scaling is on)
/// and centred, then the point is normalized and scaled to `SCREEN_XSIZE`/`SCREEN_YSIZE`. Points
/// in the letterbox bars clamp to the nearest edge and non-finite input maps to the origin, so
/// `CheckTouchRect` never sees out-of-range values.
#[must_use]
pub fn window_to_logical(
    x: f32,
    y: f32,
    window: (u32, u32),
    logical: (u32, u32),
    integer_scale: bool,
) -> (i16, i16) {
    let (window_w, window_h) = (window.0 as f32, window.1 as f32);
    let (logical_w, logical_h) = (logical.0 as f32, logical.1 as f32);
    if !x.is_finite()
        || !y.is_finite()
        || window_w <= 0.0
        || window_h <= 0.0
        || logical_w <= 0.0
        || logical_h <= 0.0
    {
        return (0, 0);
    }
    let mut scale = f32::min(window_w / logical_w, window_h / logical_h);
    if integer_scale {
        scale = scale.floor();
    }
    if scale <= 0.0 || !scale.is_finite() {
        scale = 1.0;
    }
    let display_w = logical_w * scale;
    let display_h = logical_h * scale;
    let offset_x = (window_w - display_w) * 0.5;
    let offset_y = (window_h - display_h) * 0.5;
    let normalized_x = ((x - offset_x) / display_w).clamp(0.0, 1.0);
    let normalized_y = ((y - offset_y) / display_h).clamp(0.0, 1.0);
    (
        normalize_touch(normalized_x, TOUCH_SCREEN_XSIZE),
        normalize_touch(normalized_y, TOUCH_SCREEN_YSIZE),
    )
}

/// Last mouse position in window points and left-button state.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct MouseData {
    x: f32,
    y: f32,
    down: bool,
}

/// Applies one SDL event to the mouse/window tracking, returning whether it was consumed.
///
/// Mouse motion/buttons mirror the upstream mouse-as-touch fallback: the left button is touch 0
/// unless a real finger is down.
fn apply_pointer_event(event: &Event, mouse: &mut MouseData, window: &mut WindowMapping) -> bool {
    match event {
        Event::MouseMotion {
            x, y, mousestate, ..
        } => {
            mouse.x = *x;
            mouse.y = *y;
            mouse.down = mousestate.left();
            true
        }
        Event::MouseButtonDown {
            mouse_btn: MouseButton::Left,
            x,
            y,
            ..
        } => {
            mouse.x = *x;
            mouse.y = *y;
            mouse.down = true;
            true
        }
        Event::MouseButtonUp {
            mouse_btn: MouseButton::Left,
            x,
            y,
            ..
        } => {
            mouse.x = *x;
            mouse.y = *y;
            mouse.down = false;
            true
        }
        Event::Window {
            win_event: WindowEvent::Resized(width, height),
            ..
        } => {
            let (width, height) = ((*width).max(0) as u32, (*height).max(0) as u32);
            if width > 0 && height > 0 {
                window.size = (width, height);
            }
            true
        }
        _ => false,
    }
}

/// Builds the engine touch array: fingers first, then the mouse as slot 0 when no finger is
/// active. Upstream gives touches priority over the mouse (`RetroEngine.cpp:78`).
fn assemble_touches(
    fingers: &[ActiveTouch],
    mouse: MouseData,
    window: WindowMapping,
) -> ([retro_input::TouchPoint; retro_input::MAX_TOUCHES], u8) {
    let mut touches = [retro_input::TouchPoint::default(); retro_input::MAX_TOUCHES];
    let mut count = 0usize;
    for touch in fingers.iter().take(retro_input::MAX_TOUCHES) {
        touches[count] = retro_input::TouchPoint {
            down: true,
            x: normalize_touch(touch.x, TOUCH_SCREEN_XSIZE),
            y: normalize_touch(touch.y, TOUCH_SCREEN_YSIZE),
        };
        count += 1;
    }
    if count == 0 && mouse.down {
        let (x, y) = window_to_logical(
            mouse.x,
            mouse.y,
            window.size,
            window.logical,
            window.integer_scale,
        );
        touches[0] = retro_input::TouchPoint { down: true, x, y };
        count = 1;
    }
    (touches, count as u8)
}

fn open_into(
    subsystem: &GamepadSubsystem,
    gamepads: &mut [Option<Gamepad>; retro_input::PLAYER_COUNT],
    id: JoystickId,
) {
    if gamepads
        .iter()
        .flatten()
        .any(|gamepad| gamepad.id().ok() == Some(id))
    {
        return;
    }
    if let Some(slot) = gamepads.iter_mut().find(|slot| slot.is_none())
        && let Ok(opened) = subsystem.open(id)
    {
        *slot = Some(opened);
    }
}

fn close_from(gamepads: &mut [Option<Gamepad>; retro_input::PLAYER_COUNT], id: JoystickId) {
    let slot = gamepads.iter_mut().find(|slot| {
        slot.as_ref()
            .and_then(|gamepad| gamepad.id().ok())
            .is_some_and(|current| current == id)
    });
    if let Some(slot) = slot {
        *slot = None;
    }
}

fn upsert_touch(touches: &mut Vec<ActiveTouch>, finger_id: u64, x: f32, y: f32) {
    if let Some(touch) = touches
        .iter_mut()
        .find(|touch| touch.finger_id == finger_id)
    {
        touch.x = x;
        touch.y = y;
    } else if touches.len() < retro_input::MAX_TOUCHES {
        touches.push(ActiveTouch { finger_id, x, y });
    }
}

/// SDL3 input source polling the keyboard, hot-plugged gamepads, touches and the mouse each
/// frame.
///
/// [`InputSource::poll_raw`] returns the raw device state without any `Settings.ini` mapping;
/// [`InputMappings`](retro_input::InputMappings) turns it into per-player
/// [`InputState`](retro_input::InputState) values. The mouse drives touch slot 0 while no real
/// touch is active, scaled to logical screen pixels so `CheckTouchRect` works windowed. `poll`
/// keeps the legacy single-player [`InputState`] for the existing engine host. With no window
/// focus, no gamepads and no touch devices this simply reports everything released.
pub struct Sdl3Input {
    events: Option<::sdl3::EventPump>,
    subsystem: Option<GamepadSubsystem>,
    gamepads: [Option<Gamepad>; retro_input::PLAYER_COUNT],
    keys: Vec<bool>,
    touches: Vec<ActiveTouch>,
    mouse: MouseData,
    window: WindowMapping,
    quit: Arc<AtomicBool>,
    state: InputState,
}

impl Sdl3Input {
    fn new(quit: Arc<AtomicBool>) -> Self {
        Self {
            events: None,
            subsystem: None,
            gamepads: std::array::from_fn(|_| None),
            keys: Vec::new(),
            touches: Vec::new(),
            mouse: MouseData::default(),
            window: WindowMapping::default(),
            quit,
            state: InputState::new(),
        }
    }

    /// Records the logical presentation size used to map mouse coordinates back to pixels.
    fn set_window(&mut self, logical: (u32, u32), integer_scale: bool) {
        self.window = WindowMapping {
            size: logical,
            logical,
            integer_scale,
        };
    }

    /// Starts the event pump, opens every attached gamepad and prepares the key array.
    fn attach(&mut self, sdl: &::sdl3::Sdl) -> Result<(), PlatformError> {
        self.events = Some(sdl.event_pump().map_err(PlatformError::sdl)?);
        self.keys = vec![false; retro_input::KEY_COUNT];
        if let Ok(subsystem) = sdl.gamepad() {
            if let Ok(ids) = subsystem.gamepads() {
                for id in ids {
                    open_into(&subsystem, &mut self.gamepads, id);
                }
            }
            self.subsystem = Some(subsystem);
        }
        Ok(())
    }

    /// Releases the event pump, gamepads and the gamepad subsystem handle.
    fn detach(&mut self) {
        self.events = None;
        self.subsystem = None;
        self.gamepads = std::array::from_fn(|_| None);
        self.keys.clear();
        self.touches.clear();
    }

    /// Drains SDL events and refreshes the keyboard array. Safe to call when uninitialized.
    fn refresh(&mut self) {
        let Self {
            events,
            subsystem,
            gamepads,
            keys,
            touches,
            mouse,
            window,
            quit,
            ..
        } = self;
        let Some(events) = events.as_mut() else {
            return;
        };
        for event in events.poll_iter() {
            if apply_pointer_event(&event, mouse, window) {
                continue;
            }
            match event {
                Event::Quit { .. } => quit.store(true, Ordering::Relaxed),
                Event::Window {
                    win_event: WindowEvent::CloseRequested,
                    ..
                } => quit.store(true, Ordering::Relaxed),
                Event::GamepadAdded { which, .. } => {
                    if let Some(subsystem) = subsystem.as_ref() {
                        open_into(subsystem, gamepads, which);
                    }
                }
                Event::GamepadRemoved { which, .. } => close_from(gamepads, which),
                Event::FingerDown {
                    finger_id, x, y, ..
                }
                | Event::FingerMotion {
                    finger_id, x, y, ..
                } => {
                    upsert_touch(touches, finger_id, x, y);
                }
                Event::FingerUp { finger_id, .. } | Event::FingerCanceled { finger_id, .. } => {
                    touches.retain(|touch| touch.finger_id != finger_id);
                }
                _ => {}
            }
        }
        let keyboard = events.keyboard_state();
        *keys = keyboard_array(&keyboard);
    }

    /// Per-slot gamepad state read from every connected gamepad.
    fn gamepad_states(&self) -> [retro_input::GamepadState; retro_input::PLAYER_COUNT] {
        std::array::from_fn(|index| {
            self.gamepads
                .get(index)
                .and_then(Option::as_ref)
                .filter(|gamepad| gamepad.connected())
                .map_or_else(retro_input::GamepadState::default, read_gamepad)
        })
    }

    fn poll_raw_inner(&mut self) -> RawInput {
        self.refresh();
        if self.events.is_none() {
            return RawInput::default();
        }
        let (touches, count) = assemble_touches(&self.touches, self.mouse, self.window);
        RawInput {
            keys: self.keys.clone(),
            gamepads: self.gamepad_states(),
            touches,
            touch_count: count,
        }
    }
}

impl InputSource for Sdl3Input {
    fn poll(&mut self) -> InputState {
        self.refresh();
        if self.events.is_none() {
            return InputState::new();
        }
        let key = |scancode: Scancode| {
            self.keys
                .get(scancode.to_i32() as usize)
                .copied()
                .unwrap_or(false)
        };
        let mut state = InputState::new();
        state.connected = true;
        state.up = key(Scancode::Up) || key(Scancode::W);
        state.down = key(Scancode::Down) || key(Scancode::S);
        state.left = key(Scancode::Left) || key(Scancode::A);
        state.right = key(Scancode::Right) || key(Scancode::D);
        state.a = key(Scancode::Space) || key(Scancode::Z);
        state.b = key(Scancode::X);
        state.c = key(Scancode::C);
        state.start = key(Scancode::Return) || key(Scancode::Escape);
        for gamepad in &self.gamepad_states() {
            if !gamepad.connected {
                continue;
            }
            use retro_input::ButtonState;
            let held = gamepad.held.union(gamepad.directions());
            state.connected = true;
            state.up |= held.contains(ButtonState::UP);
            state.down |= held.contains(ButtonState::DOWN);
            state.left |= held.contains(ButtonState::LEFT);
            state.right |= held.contains(ButtonState::RIGHT);
            state.a |= held.contains(ButtonState::A);
            state.b |= held.contains(ButtonState::B);
            state.c |= held.contains(ButtonState::C);
            state.start |= held.contains(ButtonState::START);
        }
        self.state = state;
        self.state
    }

    fn poll_raw(&mut self) -> RawInput {
        self.poll_raw_inner()
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

    #[test]
    fn sdl3_storage_root_uses_the_pref_path_unless_overridden() {
        let _guard = crate::platform_test_lock();
        let mut platform = Sdl3Platform::new();
        platform.set_storage_root("/tmp/retro-platform-explicit");
        platform.init().unwrap();
        assert_eq!(
            platform.storage_root(),
            Path::new("/tmp/retro-platform-explicit"),
            "an explicit root survives init"
        );
        platform.shutdown().unwrap();

        let mut platform = Sdl3Platform::new();
        platform.init().unwrap();
        if let Some(root) = crate::user_data_dir() {
            assert_eq!(
                platform.storage_root(),
                root,
                "init roots storage at the SDL preferred path"
            );
        }
        platform.shutdown().unwrap();
    }

    #[test]
    fn touch_normalization_uses_logical_screen_pixels() {
        assert_eq!(normalize_touch(0.0, TOUCH_SCREEN_XSIZE), 0);
        assert_eq!(normalize_touch(0.5, TOUCH_SCREEN_XSIZE), 212);
        assert_eq!(normalize_touch(1.0, TOUCH_SCREEN_XSIZE), 424);
        assert_eq!(normalize_touch(0.5, TOUCH_SCREEN_YSIZE), 120);
        assert_eq!(normalize_touch(1.0, TOUCH_SCREEN_YSIZE), 240);
        assert_eq!(normalize_touch(f32::NAN, TOUCH_SCREEN_XSIZE), 0);
        assert_eq!(normalize_touch(f32::INFINITY, TOUCH_SCREEN_YSIZE), 0);
        assert_eq!(normalize_touch(4.0, TOUCH_SCREEN_XSIZE), 424);
        assert_eq!(normalize_touch(-1.0, TOUCH_SCREEN_YSIZE), 0);
    }

    #[test]
    fn mouse_mapping_uses_logical_pixels() {
        let logical = (424, 240);
        assert_eq!(
            window_to_logical(0.0, 0.0, (424, 240), logical, true),
            (0, 0)
        );
        assert_eq!(
            window_to_logical(212.0, 120.0, (424, 240), logical, true),
            (212, 120)
        );
        assert_eq!(
            window_to_logical(423.0, 239.0, (424, 240), logical, true),
            (423, 239)
        );
        // 2x integer scale.
        assert_eq!(
            window_to_logical(424.0, 240.0, (848, 480), logical, true),
            (212, 120)
        );
        // Integer scaling floors the scale factor: 500x300 presents at 1x with offsets.
        assert_eq!(
            window_to_logical(250.0, 150.0, (500, 300), logical, true),
            (212, 120)
        );
        assert_eq!(
            window_to_logical(250.0, 30.0, (500, 300), logical, true),
            (212, 0)
        );
        // Non-integer presentation scales continuously instead (same normalized point).
        assert_eq!(
            window_to_logical(250.0, 30.0, (500, 300), logical, false),
            (212, 18)
        );
        // 848x600 with integer scaling letterboxes 848x480 between 60px bars.
        assert_eq!(
            window_to_logical(424.0, 300.0, (848, 600), logical, true),
            (212, 120)
        );
        assert_eq!(
            window_to_logical(0.0, 0.0, (848, 600), logical, true),
            (0, 0)
        );
        assert_eq!(
            window_to_logical(846.0, 598.0, (848, 600), logical, true),
            (423, 240)
        );
        // Degenerate and non-finite inputs never produce out-of-range values.
        assert_eq!(
            window_to_logical(f32::NAN, 0.0, (424, 240), logical, true),
            (0, 0)
        );
        assert_eq!(
            window_to_logical(0.0, f32::INFINITY, (424, 240), logical, true),
            (0, 0)
        );
        assert_eq!(window_to_logical(10.0, 10.0, (0, 0), logical, true), (0, 0));
    }

    #[test]
    fn mouse_drives_touch_zero_when_no_finger_is_active() {
        let window = WindowMapping {
            size: (424, 240),
            logical: (424, 240),
            integer_scale: true,
        };
        let mouse = MouseData {
            x: 212.0,
            y: 120.0,
            down: true,
        };
        let (touches, count) = assemble_touches(&[], mouse, window);
        assert_eq!(count, 1);
        assert_eq!(
            touches[0],
            retro_input::TouchPoint {
                down: true,
                x: 212,
                y: 120
            }
        );
        assert!(
            touches[1..]
                .iter()
                .all(|touch| *touch == retro_input::TouchPoint::default())
        );

        let released = MouseData {
            down: false,
            ..mouse
        };
        let (touches, count) = assemble_touches(&[], released, window);
        assert_eq!(count, 0);
        assert_eq!(touches[0], retro_input::TouchPoint::default());
    }

    #[test]
    fn fingers_take_priority_over_the_mouse() {
        let window = WindowMapping {
            size: (848, 480),
            logical: (424, 240),
            integer_scale: true,
        };
        let fingers = [ActiveTouch {
            finger_id: 1,
            x: 0.25,
            y: 0.5,
        }];
        let mouse = MouseData {
            x: 800.0,
            y: 470.0,
            down: true,
        };
        let (touches, count) = assemble_touches(&fingers, mouse, window);
        assert_eq!(count, 1);
        assert_eq!(
            touches[0],
            retro_input::TouchPoint {
                down: true,
                x: 106,
                y: 120
            }
        );
    }

    #[test]
    fn fabricated_sdl_mouse_events_drive_touch_zero() {
        use ::sdl3::mouse::MouseState;

        let mut mouse = MouseData::default();
        let mut window = WindowMapping::default();
        let motion = |state: u32, x: f32, y: f32| Event::MouseMotion {
            timestamp: 0,
            window_id: 0,
            which: 0,
            mousestate: MouseState::from_sdl_state(state),
            x,
            y,
            xrel: 0.0,
            yrel: 0.0,
        };

        assert!(apply_pointer_event(
            &motion(0, 100.0, 50.0),
            &mut mouse,
            &mut window
        ));
        assert_eq!(
            mouse,
            MouseData {
                x: 100.0,
                y: 50.0,
                down: false
            }
        );
        assert_eq!(assemble_touches(&[], mouse, window).1, 0);

        assert!(apply_pointer_event(
            &motion(1, 212.0, 120.0),
            &mut mouse,
            &mut window
        ));
        let (touches, count) = assemble_touches(&[], mouse, window);
        assert_eq!(count, 1);
        assert_eq!(
            touches[0],
            retro_input::TouchPoint {
                down: true,
                x: 212,
                y: 120
            }
        );

        // A press event without a preceding motion still carries the position.
        mouse = MouseData::default();
        assert!(apply_pointer_event(
            &Event::MouseButtonDown {
                timestamp: 0,
                window_id: 0,
                which: 0,
                mouse_btn: MouseButton::Left,
                clicks: 1,
                x: 10.0,
                y: 20.0,
            },
            &mut mouse,
            &mut window,
        ));
        assert_eq!(
            mouse,
            MouseData {
                x: 10.0,
                y: 20.0,
                down: true
            }
        );

        // Right-button presses are not touch input.
        let untouched = mouse;
        assert!(!apply_pointer_event(
            &Event::MouseButtonDown {
                timestamp: 0,
                window_id: 0,
                which: 0,
                mouse_btn: MouseButton::Right,
                clicks: 1,
                x: 400.0,
                y: 200.0,
            },
            &mut mouse,
            &mut window,
        ));
        assert_eq!(mouse, untouched);

        assert!(apply_pointer_event(
            &Event::MouseButtonUp {
                timestamp: 0,
                window_id: 0,
                which: 0,
                mouse_btn: MouseButton::Left,
                clicks: 1,
                x: 10.0,
                y: 20.0,
            },
            &mut mouse,
            &mut window,
        ));
        assert!(!mouse.down);
        assert_eq!(assemble_touches(&[], mouse, window).1, 0);

        assert!(apply_pointer_event(
            &Event::Window {
                timestamp: 0,
                window_id: 0,
                win_event: WindowEvent::Resized(848, 480),
            },
            &mut mouse,
            &mut window,
        ));
        assert_eq!(window.size, (848, 480));

        // Unrelated events are left to the main dispatch.
        assert!(!apply_pointer_event(
            &Event::Quit { timestamp: 0 },
            &mut mouse,
            &mut window
        ));
    }

    #[test]
    fn gamepad_state_maps_upstream_defaults() {
        use retro_input::ButtonState;

        let all_buttons = GamepadButtons {
            north: true,
            east: true,
            south: true,
            west: true,
            back: false,
            guide: true,
            start: true,
            left_shoulder: true,
            right_shoulder: true,
            dpad_up: true,
            dpad_down: true,
            dpad_left: true,
            dpad_right: true,
            trigger_left: true,
            trigger_right: true,
        };
        let state = gamepad_state(true, all_buttons, -20_000, 10_000);
        assert!(state.connected);
        assert_eq!(state.axis_x, -20_000);
        assert_eq!(state.axis_y, 10_000);
        assert_eq!(
            state.held,
            ButtonState::A
                | ButtonState::B
                | ButtonState::C
                | ButtonState::X
                | ButtonState::Y
                | ButtonState::Z
                | ButtonState::L
                | ButtonState::R
                | ButtonState::START
                | ButtonState::SELECT
                | ButtonState::UP
                | ButtonState::DOWN
                | ButtonState::LEFT
                | ButtonState::RIGHT
        );

        let back_only = GamepadButtons {
            back: true,
            ..GamepadButtons::default()
        };
        assert!(
            gamepad_state(true, back_only, 0, 0)
                .held
                .contains(ButtonState::SELECT)
        );

        let idle = gamepad_state(false, GamepadButtons::default(), 0, 0);
        assert!(!idle.connected);
        assert!(idle.held.is_empty());
    }

    #[test]
    fn vk_mapping_agrees_with_sdl_scancodes() {
        for (vk, scancode) in [
            (0x26, Scancode::Up),
            (0x28, Scancode::Down),
            (0x25, Scancode::Left),
            (0x27, Scancode::Right),
            (0x41, Scancode::A),
            (0x53, Scancode::S),
            (0x44, Scancode::D),
            (0x51, Scancode::Q),
            (0x57, Scancode::W),
            (0x45, Scancode::E),
            (0x0D, Scancode::Return),
            (0x09, Scancode::Tab),
            (0x20, Scancode::Space),
        ] {
            assert_eq!(
                retro_input::vk_to_scancode(vk),
                Some(scancode.to_i32() as u32),
                "vk {vk:#x}"
            );
        }
    }

    #[test]
    fn fabricated_key_arrays_map_through_sdl_scancodes() {
        use retro_format_v4::Settings;
        use retro_input::{Button, GamepadState, InputMappings};
        use std::str::FromStr;

        let settings = Settings::from_str("[Keyboard Map 1]\nup=0x26\nbuttonA=0x41\n").unwrap();
        let mappings = InputMappings::from_settings(&settings);
        let mut keys = vec![false; retro_input::KEY_COUNT];
        keys[Scancode::A.to_i32() as usize] = true;
        keys[Scancode::Up.to_i32() as usize] = true;

        let state = mappings.apply(0, &keys, &GamepadState::default());
        assert!(state.is_held(Button::A));
        assert!(state.is_held(Button::Up));
        assert!(state.is_pressed(Button::A));

        keys[Scancode::A.to_i32() as usize] = false;
        let state = mappings.apply(0, &keys, &GamepadState::default());
        assert!(!state.is_held(Button::A));
        assert!(!state.is_pressed(Button::A));
        assert!(state.is_held(Button::Up));
    }

    #[test]
    fn sdl3_input_without_init_is_idle() {
        let mut input = Sdl3Input::new(Arc::new(AtomicBool::new(false)));
        assert_eq!(input.poll(), InputState::new());
        let raw = input.poll_raw();
        assert_eq!(raw, RawInput::default());
        assert!(!raw.has_gamepad());
    }

    #[test]
    #[ignore = "requires an SDL3 environment with event support"]
    fn sdl3_input_live_poll_does_not_panic() {
        let _guard = crate::platform_test_lock();
        let mut platform = Sdl3Platform::new();
        platform.init().unwrap();
        let raw = platform.input().poll_raw();
        assert_eq!(raw.keys.len(), retro_input::KEY_COUNT);
        let _ = platform.input().poll();
        platform.shutdown().unwrap();
        assert_eq!(platform.input().poll_raw(), RawInput::default());
    }
}
