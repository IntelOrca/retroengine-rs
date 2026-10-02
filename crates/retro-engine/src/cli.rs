//! Command line interface for the headless engine.
//!
//! The runtime loads settings/configs/scripts/scenes, runs startup and 60 Hz update/draw events
//! for `--frames` frames and prints a BLAKE3 hash of the canonical engine state (which includes
//! the software framebuffer). `--dump-frames DIR` writes the presented RGB565 framebuffer to
//! `frame_%04d.png` headlessly; without `--headless` the same frames are presented through the
//! SDL3 backend at 60 Hz.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use clap::Parser;
use retro_io::DirSource;
use retro_platform::{BackendKind, WindowDesc};

use crate::EngineError;
use crate::loader;
use crate::runtime::Engine;

/// Number of frames run when `--frames` is omitted or `0`.
pub const DEFAULT_FRAMES: u64 = 600;

/// Command line arguments for the engine binary.
#[derive(Debug, Parser)]
#[command(
    name = "retro-engine",
    version,
    about = "Retro Engine (RSDK v4 legacy) reimplementation"
)]
pub struct Args {
    /// Path to an unpacked RSDK asset folder (e.g. assets/S1)
    pub assets_dir: PathBuf,
    /// Scene to start: a stage folder (`Zone01`) or a GameConfig scene name
    #[arg(long)]
    pub scene: Option<String>,
    /// Act id to start (numeric, or a stage id such as `B`); defaults to the
    /// GameConfig entry's id
    #[arg(long)]
    pub act: Option<String>,
    /// Run without a window using the deterministic headless backend
    #[arg(long)]
    pub headless: bool,
    /// Number of frames to run (0 means the 600-frame default)
    #[arg(long, default_value_t = 0)]
    pub frames: u64,
    /// Scripted input file to replay (input replay lands in M5; accepted but unused)
    #[arg(long)]
    pub input: Option<PathBuf>,
    /// Directory to dump presented frames into as `frame_%04d.png`
    #[arg(long)]
    pub dump_frames: Option<PathBuf>,
    /// Dump every Nth frame (default 1; frame 0 is dumped before the loop)
    #[arg(long, default_value_t = 1)]
    pub dump_frame_every: u64,
    /// RNG seed
    #[arg(long)]
    pub seed: Option<u32>,
    /// Print one `frame,hash` line per frame instead of only the final hash
    #[arg(long)]
    pub hash_every_frame: bool,
}

/// A validated unpacked asset folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedAssets {
    /// The asset folder root.
    pub root: PathBuf,
    /// The game configuration file inside the asset folder.
    pub game_config: PathBuf,
}

/// Resolves the requested backend.
#[must_use]
pub fn backend_for(args: &Args) -> BackendKind {
    if args.headless {
        BackendKind::Headless
    } else {
        BackendKind::Sdl3
    }
}

/// Validates that `root` is an unpacked RSDK asset folder.
pub fn resolve_assets(root: &Path) -> Result<ResolvedAssets, EngineError> {
    if !root.is_dir() {
        return Err(EngineError::MissingAssets(root.to_path_buf()));
    }
    let game_config = root.join("Data").join("Game").join("GameConfig.bin");
    if !game_config.is_file() {
        return Err(EngineError::MissingGameConfig(game_config));
    }
    Ok(ResolvedAssets {
        root: root.to_path_buf(),
        game_config,
    })
}

/// Parses arguments, loads the requested scene and runs the frame loop.
pub fn run(args: &Args) -> Result<(), EngineError> {
    let assets = resolve_assets(&args.assets_dir)?;
    let source = DirSource::new(&assets.root)?;
    let seed = args.seed.unwrap_or(crate::rng::DEFAULT_SEED);
    let mut engine = Engine::load(
        Arc::new(source),
        args.scene.as_deref(),
        args.act.as_deref(),
        seed,
    )?;
    let (folder, act) = engine.stage_info();
    let folder = folder.to_owned();
    let act = act.to_owned();

    println!("retro-engine {}", env!("CARGO_PKG_VERSION"));
    println!("assets dir: {}", assets.root.display());
    println!("game config: {}", assets.game_config.display());
    println!("game: {}", engine.game_title());
    println!("scene: {folder} act {act}");
    println!("profile: {}", engine.settings().profile.name());
    println!("backend: {}", backend_for(args).name());
    let frames = if args.frames == 0 {
        DEFAULT_FRAMES
    } else {
        args.frames
    };
    println!("frames: {frames}");
    println!("seed: {seed}");
    if let Some(input) = &args.input {
        println!(
            "note: input replay is not wired until M5; {} is ignored (no keys pressed)",
            input.display()
        );
    }
    if let Some(dir) = &args.dump_frames {
        println!(
            "dump frames: {} (every {})",
            dir.display(),
            args.dump_frame_every
        );
    }

    let mut platform = retro_platform::create(backend_for(args))?;
    platform.init()?;
    let (width, height) = (
        engine.framebuffer().width() as u32,
        engine.framebuffer().height() as u32,
    );
    let mut window = if args.headless {
        None
    } else {
        Some(platform.create_window(WindowDesc::new(
            format!("{} - {folder} {act}", engine.game_title()),
            width,
            height,
        ))?)
    };

    if let Some(dir) = &args.dump_frames {
        std::fs::create_dir_all(dir)?;
        dump_frame(dir, 0, engine.framebuffer())?;
    }
    if args.hash_every_frame {
        println!("0,{}", engine.state_hash());
    }

    let dump_every = args.dump_frame_every.max(1);
    let mut present_buffer = Vec::new();
    let mut presented = 0u64;
    for _ in 0..frames {
        engine.run_frame()?;
        let frame = engine.state.frame;
        if let Some(window) = &mut window {
            engine.framebuffer().copy_visible_into(&mut present_buffer);
            apply_dim(&mut present_buffer, engine.state.render.dim_amount());
            window.present(&present_buffer, width, height)?;
            platform.clock().advance_frame();
            platform.clock().sleep_until_next_frame()?;
            presented += 1;
            if window.should_close() {
                break;
            }
        }
        if let Some(dir) = &args.dump_frames
            && frame % dump_every == 0
        {
            dump_frame(dir, frame, engine.framebuffer())?;
        }
        if args.hash_every_frame {
            println!("{frame},{}", engine.state_hash());
        }
    }

    if !args.hash_every_frame {
        let hash = engine.state_hash();
        println!("hash: {hash}");
    }
    if let Some(window) = window.as_mut() {
        println!("presented-frames: {presented}");
        println!("window-title: {}", engine.game_title());
        window.set_title(engine.game_title());
    }
    report_histograms(&engine);
    platform.shutdown()?;
    Ok(())
}

/// Darkens a present buffer by the `FlipScreen` dim amount (a black overlay of alpha
/// `1 - amount`), mirroring SDL's alpha blend on the 16-bit RGB565 channels.
fn apply_dim(pixels: &mut [u16], amount: f32) {
    if amount >= 1.0 {
        return;
    }
    let alpha = ((1.0 - amount) * 255.0).clamp(0.0, 255.0) as u32;
    let scale = 255 - alpha;
    for pixel in pixels {
        let r = (u32::from(*pixel >> 11) & 0x1F) * scale / 255;
        let g = (u32::from(*pixel >> 5) & 0x3F) * scale / 255;
        let b = (u32::from(*pixel) & 0x1F) * scale / 255;
        *pixel = ((r << 11) | (g << 5) | b) as u16;
    }
}

/// Writes one framebuffer to `DIR/frame_%04d.png`.
fn dump_frame(
    dir: &Path,
    frame: u64,
    framebuffer: &retro_render::Framebuffer,
) -> Result<(), EngineError> {
    let path = dir.join(format!("frame_{frame:04}.png"));
    let bytes = framebuffer
        .to_png_bytes()
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    std::fs::write(path, bytes)?;
    Ok(())
}

fn report_histograms(engine: &Engine) {
    let unknown = engine.stub_histogram();
    let known: Vec<String> = engine
        .op_histogram()
        .iter()
        .filter(|(name, _)| !unknown.contains_key(*name))
        .map(|(name, count)| format!("{name}={count}"))
        .collect();
    println!("ported-ops: {}", known.join(" "));
    if unknown.is_empty() {
        println!("stubbed-ops: <none>");
    } else {
        let stubs: Vec<String> = unknown
            .iter()
            .map(|(name, count)| format!("{name}={count}"))
            .collect();
        println!("stubbed-ops: {}", stubs.join(" "));
    }
}

/// Resolves the scene using the loaded GameConfig (exposed for tests and tooling).
pub fn resolve_scene_name(
    game_config: &retro_format_v4::GameConfig,
    requested: Option<&str>,
    act: Option<&str>,
) -> Result<(String, String), EngineError> {
    loader::resolve_scene(game_config, requested, act)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_assets(name: &str, with_config: bool) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("retro-engine-cli-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let game = root.join("Data").join("Game");
        std::fs::create_dir_all(&game).unwrap();
        if with_config {
            std::fs::write(game.join("GameConfig.bin"), b"config").unwrap();
        }
        root
    }

    #[test]
    fn parses_required_assets_dir() {
        let args = Args::try_parse_from(["retro-engine", "/tmp/assets"]).unwrap();
        assert_eq!(args.assets_dir, PathBuf::from("/tmp/assets"));
        assert_eq!(args.act, None);
        assert!(!args.headless);
        assert_eq!(args.frames, 0);
        assert!(args.scene.is_none());
        assert!(!args.hash_every_frame);
        assert_eq!(args.dump_frame_every, 1);
    }

    #[test]
    fn parses_all_options() {
        let args = Args::try_parse_from([
            "retro-engine",
            "/tmp/assets",
            "--scene",
            "GHZ",
            "--act",
            "2",
            "--headless",
            "--frames",
            "3",
            "--input",
            "replay.bin",
            "--dump-frames",
            "out",
            "--dump-frame-every",
            "60",
            "--seed",
            "7",
            "--hash-every-frame",
        ])
        .unwrap();
        assert_eq!(args.scene.as_deref(), Some("GHZ"));
        assert_eq!(args.act.as_deref(), Some("2"));
        assert!(args.headless);
        assert_eq!(args.frames, 3);
        assert_eq!(args.input, Some(PathBuf::from("replay.bin")));
        assert_eq!(args.dump_frames, Some(PathBuf::from("out")));
        assert_eq!(args.dump_frame_every, 60);
        assert_eq!(args.seed, Some(7));
        assert!(args.hash_every_frame);
        assert_eq!(backend_for(&args), BackendKind::Headless);
    }

    #[test]
    fn rejects_missing_assets_dir() {
        let args = Args::try_parse_from(["retro-engine"]).unwrap_err();
        assert_eq!(args.kind(), clap::error::ErrorKind::MissingRequiredArgument);
    }

    #[test]
    fn version_flag_is_handled_by_clap() {
        let error = Args::try_parse_from(["retro-engine", "--version"]).unwrap_err();
        assert_eq!(error.kind(), clap::error::ErrorKind::DisplayVersion);
    }

    #[test]
    fn resolves_valid_asset_folder() {
        let root = temp_assets("valid", true);
        let resolved = resolve_assets(&root).unwrap();
        assert_eq!(resolved.root, root);
        assert!(resolved.game_config.ends_with("Data/Game/GameConfig.bin"));
        let _ = std::fs::remove_dir_all(&resolved.root);
    }

    #[test]
    fn rejects_folder_without_game_config() {
        let root = temp_assets("no-config", false);
        let error = resolve_assets(&root).unwrap_err();
        assert!(matches!(error, EngineError::MissingGameConfig(_)));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn rejects_missing_folder() {
        let root = std::env::temp_dir().join("retro-engine-cli-definitely-missing");
        let error = resolve_assets(&root).unwrap_err();
        assert!(matches!(error, EngineError::MissingAssets(_)));
    }
}
