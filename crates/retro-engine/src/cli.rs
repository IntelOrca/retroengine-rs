//! Command line interface for the headless engine.
//!
//! M3 runs the real scene loop without rendering: settings/configs/scripts/scenes are loaded,
//! startup and update events execute at 60 Hz for `--frames` frames (600 by default) and a
//! BLAKE3 state hash is printed, either once at the end or per frame with
//! `--hash-every-frame`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use clap::Parser;
use retro_io::DirSource;
use retro_platform::BackendKind;

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
    /// Act number to start
    #[arg(long, default_value_t = 1)]
    pub act: u32,
    /// Run without a window using the deterministic headless backend
    #[arg(long)]
    pub headless: bool,
    /// Number of frames to run (0 means the 600-frame default)
    #[arg(long, default_value_t = 0)]
    pub frames: u64,
    /// Scripted input file to replay (input replay lands in M5; accepted but unused)
    #[arg(long)]
    pub input: Option<PathBuf>,
    /// Directory to dump presented frames into (rendering lands in M4; accepted but unused)
    #[arg(long)]
    pub dump_frames: Option<PathBuf>,
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

/// Parses arguments, loads the requested scene and runs the headless frame loop.
pub fn run(args: &Args) -> Result<(), EngineError> {
    let assets = resolve_assets(&args.assets_dir)?;
    let source = DirSource::new(&assets.root)?;
    let seed = args.seed.unwrap_or(crate::rng::DEFAULT_SEED);
    let mut engine = Engine::load(Arc::new(source), args.scene.as_deref(), args.act, seed)?;
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
    if !args.headless {
        println!("note: the SDL3 window/present path lands in M4; running the headless loop");
    }
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
            "note: frame dumping requires the M4 renderer; {} is ignored",
            dir.display()
        );
    }

    let outcome = engine.run_frames(frames, args.hash_every_frame)?;
    if args.hash_every_frame {
        for (frame, hash) in &outcome.frame_hashes {
            println!("{frame},{hash}");
        }
    } else {
        println!("hash: {}", outcome.final_hash);
    }
    report_histograms(&engine);
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
    act: u32,
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
        assert_eq!(args.act, 1);
        assert!(!args.headless);
        assert_eq!(args.frames, 0);
        assert!(args.scene.is_none());
        assert!(!args.hash_every_frame);
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
            "--seed",
            "7",
            "--hash-every-frame",
        ])
        .unwrap();
        assert_eq!(args.scene.as_deref(), Some("GHZ"));
        assert_eq!(args.act, 2);
        assert!(args.headless);
        assert_eq!(args.frames, 3);
        assert_eq!(args.input, Some(PathBuf::from("replay.bin")));
        assert_eq!(args.dump_frames, Some(PathBuf::from("out")));
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
