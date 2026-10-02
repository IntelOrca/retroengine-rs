//! Engine entry point: loads an unpacked RSDK asset folder and runs headless or windowed.
#![forbid(unsafe_code)]

pub mod cli;

pub use cli::{Args, EngineError, ResolvedAssets, resolve_assets, run};

/// Builds a deterministic RGB565 test pattern used as the M0 parity artifact.
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
