//! Engine entry point: loads an unpacked RSDK asset folder and runs the software renderer
//! headlessly (with optional PNG frame dumps) or windowed through SDL3.
#![forbid(unsafe_code)]

pub mod audio;
pub mod cli;
pub mod error;
pub mod host;
pub mod input;
pub mod loader;
pub mod profile;
pub mod rng;
pub mod runtime;
pub mod save;
pub mod state;

pub use audio::AudioState;
pub use cli::{
    Args, ResolvedAssets, backend_for, format_listing, list, resolve_assets, resolve_scene_name,
    run,
};
pub use error::EngineError;
pub use input::EngineInput;
pub use profile::{EngineSettings, RuntimeProfile};
pub use runtime::{DEFAULT_RNG_SEED, Engine, RunOutcome, ScriptRuntime};
pub use save::{SaveError, SaveState, SaveStore, seed_memory_storage, seed_storage_from_source};
pub use state::EngineState;

/// Builds a deterministic RGB565 test pattern used by the headless parity tests.
#[must_use]
pub fn placeholder_frame(width: u32, height: u32) -> Vec<u16> {
    let mut framebuffer = Vec::with_capacity(width as usize * height as usize);
    for y in 0..height {
        for x in 0..width {
            let r = ((x * 255) / width.max(1)) as u8;
            let g = ((y * 255) / height.max(1)) as u8;
            let b = ((x ^ y) & 0xFF) as u8;
            framebuffer.push(retro_core::color::rgb888_to_rgb565(r, g, b));
        }
    }
    framebuffer
}
