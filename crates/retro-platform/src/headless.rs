//! Deterministic in-memory backend used for tests, CI and replay capture.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, HashMap};
use std::rc::Rc;

use crate::{
    AudioDesc, AudioDevice, Clock, InputSource, InputState, Platform, PlatformError, Storage,
    TARGET_FPS, Window, WindowDesc,
};

#[derive(Clone, Debug)]
struct SharedFrame(Rc<Cell<u64>>);

/// Headless platform with an in-memory framebuffer, scripted input, captured PCM and a deterministic clock.
pub struct HeadlessPlatform {
    initialized: bool,
    frame: SharedFrame,
    script: Rc<RefCell<Vec<InputState>>>,
    captured_pcm: Rc<RefCell<Vec<f32>>>,
    input: HeadlessInput,
    clock: HeadlessClock,
    storage: MemoryStorage,
}

impl HeadlessPlatform {
    /// Creates an uninitialized headless platform.
    #[must_use]
    pub fn new() -> Self {
        let frame = SharedFrame(Rc::new(Cell::new(0)));
        let script = Rc::new(RefCell::new(Vec::new()));
        let captured_pcm = Rc::new(RefCell::new(Vec::new()));
        Self {
            initialized: false,
            input: HeadlessInput {
                frame: frame.clone(),
                script: script.clone(),
                state: InputState::new(),
            },
            clock: HeadlessClock {
                frame: frame.clone(),
            },
            frame,
            script,
            captured_pcm,
            storage: MemoryStorage::new(),
        }
    }

    /// Replaces the scripted per-frame input states.
    pub fn set_input_script(&mut self, script: Vec<InputState>) {
        *self.script.borrow_mut() = script;
    }

    /// Replaces the scripted input states and returns the platform.
    #[must_use]
    pub fn with_input_script(mut self, script: Vec<InputState>) -> Self {
        self.set_input_script(script);
        self
    }

    /// Returns a copy of all PCM samples submitted to headless audio devices.
    #[must_use]
    pub fn captured_pcm(&self) -> Vec<f32> {
        self.captured_pcm.borrow().clone()
    }

    /// Number of frames advanced on the deterministic clock.
    #[must_use]
    pub fn frames_advanced(&self) -> u64 {
        self.frame.0.get()
    }
}

impl Default for HeadlessPlatform {
    fn default() -> Self {
        Self::new()
    }
}

impl Platform for HeadlessPlatform {
    fn name(&self) -> &'static str {
        "headless"
    }

    fn init(&mut self) -> Result<(), PlatformError> {
        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> Result<(), PlatformError> {
        self.initialized = false;
        Ok(())
    }

    fn is_initialized(&self) -> bool {
        self.initialized
    }

    fn create_window(&mut self, desc: WindowDesc) -> Result<Box<dyn Window>, PlatformError> {
        if !self.initialized {
            return Err(PlatformError::NotInitialized);
        }
        if desc.width == 0 || desc.height == 0 {
            return Err(PlatformError::InvalidArgument(
                "window dimensions must be non-zero".to_owned(),
            ));
        }
        Ok(Box::new(HeadlessWindow::new(desc)))
    }

    fn open_audio(&mut self, desc: AudioDesc) -> Result<Box<dyn AudioDevice>, PlatformError> {
        if !self.initialized {
            return Err(PlatformError::NotInitialized);
        }
        if desc.channels != 2 {
            return Err(PlatformError::InvalidArgument(
                "headless audio supports stereo only".to_owned(),
            ));
        }
        Ok(Box::new(HeadlessAudio {
            sample_rate: desc.sample_rate,
            channels: desc.channels,
            captured: self.captured_pcm.clone(),
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

/// Headless window storing the most recently presented RGB565 frame.
pub struct HeadlessWindow {
    width: u32,
    height: u32,
    title: String,
    last_frame: Vec<u16>,
    presents: u64,
}

impl HeadlessWindow {
    fn new(desc: WindowDesc) -> Self {
        Self {
            width: desc.width,
            height: desc.height,
            title: desc.title,
            last_frame: vec![0; desc.width as usize * desc.height as usize],
            presents: 0,
        }
    }

    /// The most recently presented frame.
    #[must_use]
    pub fn last_frame(&self) -> &[u16] {
        &self.last_frame
    }

    /// Number of times a frame has been presented.
    #[must_use]
    pub fn present_count(&self) -> u64 {
        self.presents
    }

    /// Current window title.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }
}

impl Window for HeadlessWindow {
    fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    fn present(
        &mut self,
        framebuffer: &[u16],
        width: u32,
        height: u32,
    ) -> Result<(), PlatformError> {
        let expected = width as usize * height as usize;
        if width == 0 || height == 0 || framebuffer.len() != expected {
            return Err(PlatformError::InvalidArgument(format!(
                "framebuffer of {} pixels does not match {width}x{height}",
                framebuffer.len()
            )));
        }
        self.width = width;
        self.height = height;
        self.last_frame.clear();
        self.last_frame.extend_from_slice(framebuffer);
        self.presents += 1;
        Ok(())
    }

    fn readback(&self) -> Result<Vec<u16>, PlatformError> {
        Ok(self.last_frame.clone())
    }

    fn set_title(&mut self, title: &str) {
        self.title = title.to_owned();
    }

    fn should_close(&self) -> bool {
        false
    }
}

/// Headless audio device that appends all submitted samples to a capture buffer.
pub struct HeadlessAudio {
    sample_rate: u32,
    channels: u8,
    captured: Rc<RefCell<Vec<f32>>>,
}

impl AudioDevice for HeadlessAudio {
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
        self.captured.borrow_mut().extend_from_slice(frames);
        Ok(frames.len() / channels)
    }

    fn queued_frames(&self) -> usize {
        self.captured.borrow().len() / self.channels.max(1) as usize
    }

    fn close(&mut self) -> Result<(), PlatformError> {
        Ok(())
    }
}

struct HeadlessInput {
    frame: SharedFrame,
    script: Rc<RefCell<Vec<InputState>>>,
    state: InputState,
}

impl InputSource for HeadlessInput {
    fn poll(&mut self) -> InputState {
        let script = self.script.borrow();
        self.state = if script.is_empty() {
            InputState::new()
        } else {
            let index = (self.frame.0.get() as usize).min(script.len() - 1);
            script[index]
        };
        self.state
    }
}

struct HeadlessClock {
    frame: SharedFrame,
}

impl Clock for HeadlessClock {
    fn frame(&self) -> u64 {
        self.frame.0.get()
    }

    fn now_ms(&self) -> u64 {
        self.frame.0.get() * 1000 / TARGET_FPS
    }

    fn advance_frame(&mut self) {
        self.frame.0.set(self.frame.0.get() + 1);
    }

    fn sleep_until_next_frame(&self) -> Result<(), PlatformError> {
        Ok(())
    }
}

/// In-memory user-file storage.
#[derive(Debug, Default)]
pub struct MemoryStorage {
    files: HashMap<String, Vec<u8>>,
}

impl MemoryStorage {
    /// Creates empty storage.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of stored files.
    #[must_use]
    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    /// Whether no files are stored.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}

impl Storage for MemoryStorage {
    fn read(&self, path: &str) -> Result<Vec<u8>, PlatformError> {
        self.files
            .get(path)
            .cloned()
            .ok_or_else(|| PlatformError::Other(format!("no such file: {path}")))
    }

    fn write(&mut self, path: &str, data: &[u8]) -> Result<(), PlatformError> {
        self.files.insert(path.to_owned(), data.to_vec());
        Ok(())
    }

    fn exists(&self, path: &str) -> bool {
        self.files.contains_key(path)
    }

    fn list(&self, dir: &str) -> Result<Vec<String>, PlatformError> {
        let prefix = if dir.is_empty() || dir == "/" {
            String::new()
        } else {
            format!("{}/", dir.trim_end_matches('/'))
        };
        let mut entries = BTreeSet::new();
        for key in self.files.keys() {
            if let Some(rest) = key.strip_prefix(&prefix) {
                let name = rest.split('/').next().unwrap_or(rest);
                if !name.is_empty() {
                    entries.insert(name.to_owned());
                }
            }
        }
        Ok(entries.into_iter().collect())
    }

    fn remove(&mut self, path: &str) -> Result<(), PlatformError> {
        self.files.remove(path);
        Ok(())
    }

    fn rename(&mut self, from: &str, to: &str) -> Result<(), PlatformError> {
        let data = self
            .files
            .remove(from)
            .ok_or_else(|| PlatformError::Other(format!("no such file: {from}")))?;
        self.files.insert(to.to_owned(), data);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_platform() -> HeadlessPlatform {
        let mut platform = HeadlessPlatform::new();
        platform.init().unwrap();
        platform
    }

    #[test]
    fn headless_framebuffer_round_trips() {
        let mut platform = make_platform();
        let mut window = platform
            .create_window(WindowDesc::new("test", 4, 2))
            .unwrap();
        let frame: Vec<u16> = (0..8).collect();
        window.present(&frame, 4, 2).unwrap();
        assert_eq!(window.size(), (4, 2));
        assert_eq!(window.readback().unwrap(), frame);
    }

    #[test]
    fn headless_present_rejects_wrong_length() {
        let mut platform = make_platform();
        let mut window = platform
            .create_window(WindowDesc::new("test", 4, 2))
            .unwrap();
        let error = window.present(&[0; 7], 4, 2).unwrap_err();
        assert!(matches!(error, PlatformError::InvalidArgument(_)));
    }

    #[test]
    fn headless_requires_init_before_window() {
        let mut platform = HeadlessPlatform::new();
        let result = platform.create_window(WindowDesc::new("test", 4, 2));
        assert!(matches!(result, Err(PlatformError::NotInitialized)));
    }

    #[test]
    fn headless_scripted_input_follows_frames() {
        let mut platform = make_platform();
        let mut a = InputState::new();
        a.right = true;
        let mut b = InputState::new();
        b.a = true;
        platform.set_input_script(vec![a, b]);

        assert_eq!(platform.input().poll(), a);
        platform.clock().advance_frame();
        assert_eq!(platform.input().poll(), b);
        platform.clock().advance_frame();
        assert_eq!(platform.input().poll(), b);
        assert_eq!(platform.frames_advanced(), 2);
    }

    #[test]
    fn headless_clock_is_deterministic() {
        let mut platform = make_platform();
        assert_eq!(platform.clock().now_ms(), 0);
        platform.clock().advance_frame();
        assert_eq!(platform.clock().now_ms(), 16);
        platform.clock().advance_frame();
        assert_eq!(platform.clock().now_ms(), 33);
        assert_eq!(platform.clock().frame(), 2);
        platform.clock().sleep_until_next_frame().unwrap();
        assert_eq!(platform.clock().now_ms(), 33);
    }

    #[test]
    fn headless_audio_captures_pcm() {
        let mut platform = make_platform();
        let mut device = platform.open_audio(AudioDesc::stereo(48_000)).unwrap();
        assert_eq!(device.sample_rate(), 48_000);
        assert_eq!(device.channels(), 2);
        let accepted = device.submit(&[0.0, 0.5, 1.0, -1.0]).unwrap();
        assert_eq!(accepted, 2);
        assert_eq!(platform.captured_pcm(), vec![0.0, 0.5, 1.0, -1.0]);
        assert_eq!(device.queued_frames(), 2);
        assert!(device.submit(&[0.0]).is_err());
        device.close().unwrap();
    }

    #[test]
    fn headless_storage_round_trips() {
        let mut platform = make_platform();
        let storage = platform.storage();
        assert!(storage.read("save.dat").is_err());
        storage.write("saves/slot1.dat", b"one").unwrap();
        storage.write("saves/slot2.dat", b"two").unwrap();
        storage.write("config.ini", b"cfg").unwrap();
        assert!(storage.exists("saves/slot1.dat"));
        assert_eq!(storage.read("saves/slot1.dat").unwrap(), b"one");
        assert_eq!(
            storage.list("saves").unwrap(),
            vec!["slot1.dat", "slot2.dat"]
        );
        assert_eq!(storage.list("").unwrap(), vec!["config.ini", "saves"]);
        storage.remove("config.ini").unwrap();
        assert!(!storage.exists("config.ini"));
    }

    #[test]
    fn memory_storage_renames() {
        let mut storage = MemoryStorage::new();
        storage.write("save.tmp", b"new").unwrap();
        storage.write("save.bin", b"old").unwrap();
        storage.rename("save.tmp", "save.bin").unwrap();
        assert!(!storage.exists("save.tmp"));
        assert_eq!(storage.read("save.bin").unwrap(), b"new");
        assert!(storage.rename("missing.tmp", "save.bin").is_err());
        assert_eq!(storage.file_count(), 1);
    }

    #[test]
    fn storage_default_rename_copies_then_removes() {
        #[derive(Default)]
        struct DefaultRenameStorage(MemoryStorage);

        impl Storage for DefaultRenameStorage {
            fn read(&self, path: &str) -> Result<Vec<u8>, PlatformError> {
                self.0.read(path)
            }

            fn write(&mut self, path: &str, data: &[u8]) -> Result<(), PlatformError> {
                self.0.write(path, data)
            }

            fn exists(&self, path: &str) -> bool {
                self.0.exists(path)
            }

            fn list(&self, dir: &str) -> Result<Vec<String>, PlatformError> {
                self.0.list(dir)
            }

            fn remove(&mut self, path: &str) -> Result<(), PlatformError> {
                self.0.remove(path)
            }
        }

        let mut storage = DefaultRenameStorage::default();
        storage.write("from.dat", b"data").unwrap();
        storage.rename("from.dat", "to.dat").unwrap();
        assert!(!storage.exists("from.dat"));
        assert_eq!(storage.read("to.dat").unwrap(), b"data");
    }

    #[test]
    fn headless_does_not_initialize_sdl() {
        let _guard = crate::platform_test_lock();
        let before = crate::sdl3_init_count();
        let mut platform = make_platform();
        let mut window = platform
            .create_window(WindowDesc::new("test", 2, 2))
            .unwrap();
        window.present(&[1, 2, 3, 4], 2, 2).unwrap();
        platform.clock().advance_frame();
        let _ = platform.input().poll();
        assert_eq!(crate::sdl3_init_count(), before);
    }
}
