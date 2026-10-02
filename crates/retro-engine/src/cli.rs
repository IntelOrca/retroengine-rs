//! Command line interface and asset folder validation.

use std::fmt;
use std::path::{Path, PathBuf};

use clap::Parser;
use retro_platform::{BackendKind, PlatformError};

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
    /// Scene to start, e.g. GHZ
    #[arg(long)]
    pub scene: Option<String>,
    /// Act number to start
    #[arg(long, default_value_t = 1)]
    pub act: u32,
    /// Run without a window using the deterministic headless backend
    #[arg(long)]
    pub headless: bool,
    /// Number of frames to run (0 means unlimited)
    #[arg(long, default_value_t = 0)]
    pub frames: u64,
    /// Scripted input file to replay
    #[arg(long)]
    pub input: Option<PathBuf>,
    /// Directory to dump presented frames into
    #[arg(long)]
    pub dump_frames: Option<PathBuf>,
    /// RNG seed
    #[arg(long)]
    pub seed: Option<u32>,
}

/// A validated unpacked asset folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedAssets {
    /// The asset folder root.
    pub root: PathBuf,
    /// The game configuration file inside the asset folder.
    pub game_config: PathBuf,
}

/// Errors raised while resolving or running the engine.
#[derive(Debug)]
pub enum EngineError {
    /// The assets directory does not exist.
    MissingAssets(PathBuf),
    /// The assets directory has no `Data/Game/GameConfig.bin`.
    MissingGameConfig(PathBuf),
    /// A platform backend failed.
    Platform(PlatformError),
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingAssets(path) => {
                write!(f, "assets directory does not exist: {}", path.display())
            }
            Self::MissingGameConfig(path) => write!(
                f,
                "not an unpacked RSDK asset folder, missing: {}",
                path.display()
            ),
            Self::Platform(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for EngineError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Platform(error) => Some(error),
            _ => None,
        }
    }
}

impl From<PlatformError> for EngineError {
    fn from(error: PlatformError) -> Self {
        Self::Platform(error)
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

/// Resolves the requested backend.
#[must_use]
pub fn backend_for(args: &Args) -> BackendKind {
    if args.headless {
        BackendKind::Headless
    } else {
        BackendKind::Sdl3
    }
}

/// Message emitted when frame-loop flags are passed before the loop is wired.
pub const FRAME_LOOP_PENDING_NOTICE: &str = "note: the frame loop is not wired until M3; M0 only validates arguments and prints the resolved configuration";

/// Resolved engine startup configuration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunReport {
    /// The validated asset folder.
    pub assets: ResolvedAssets,
    /// The backend selected for this run.
    pub backend: BackendKind,
    /// User-facing notices about features that are not wired up yet.
    pub notices: Vec<String>,
}

/// Validates arguments and builds the resolved configuration without running the engine.
pub fn resolve_run(args: &Args) -> Result<RunReport, EngineError> {
    let assets = resolve_assets(&args.assets_dir)?;
    let backend = backend_for(args);
    let mut notices = Vec::new();
    if args.headless || args.frames > 0 || args.input.is_some() || args.dump_frames.is_some() {
        notices.push(FRAME_LOOP_PENDING_NOTICE.to_owned());
    }
    Ok(RunReport {
        assets,
        backend,
        notices,
    })
}

/// Parses arguments, validates the asset folder and prints the resolved configuration.
pub fn run(args: &Args) -> Result<(), EngineError> {
    let report = resolve_run(args)?;
    print_report(args, &report);
    Ok(())
}

fn print_report(args: &Args, report: &RunReport) {
    println!("retro-engine {}", env!("CARGO_PKG_VERSION"));
    println!("assets dir: {}", report.assets.root.display());
    println!("game config: {}", report.assets.game_config.display());
    println!("scene: {}", args.scene.as_deref().unwrap_or("<default>"));
    println!("act: {}", args.act);
    println!("backend: {}", report.backend.name());
    if args.frames == 0 {
        println!("frames: unlimited");
    } else {
        println!("frames: {}", args.frames);
    }
    match args.seed {
        Some(seed) => println!("seed: {seed}"),
        None => println!("seed: <random>"),
    }
    if let Some(input) = &args.input {
        println!("input: {}", input.display());
    }
    if let Some(dir) = &args.dump_frames {
        println!("dump frames: {}", dir.display());
    }
    for notice in &report.notices {
        println!("{notice}");
    }
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
        ])
        .unwrap();
        assert_eq!(args.scene.as_deref(), Some("GHZ"));
        assert_eq!(args.act, 2);
        assert!(args.headless);
        assert_eq!(args.frames, 3);
        assert_eq!(args.input, Some(PathBuf::from("replay.bin")));
        assert_eq!(args.dump_frames, Some(PathBuf::from("out")));
        assert_eq!(args.seed, Some(7));
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

    #[test]
    fn run_prints_and_succeeds() {
        let root = temp_assets("run", true);
        let args = Args::try_parse_from([
            "retro-engine",
            root.to_str().unwrap(),
            "--headless",
            "--frames",
            "3",
        ])
        .unwrap();
        run(&args).unwrap();
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn frame_loop_flags_emit_m3_notice() {
        let root = temp_assets("notice", true);
        let base = root.to_str().unwrap();
        for extra in [
            vec!["--headless"],
            vec!["--frames", "3"],
            vec!["--input", "replay.bin"],
            vec!["--dump-frames", "out"],
        ] {
            let mut argv = vec!["retro-engine", base];
            argv.extend(extra);
            let args = Args::try_parse_from(argv).unwrap();
            let report = resolve_run(&args).unwrap();
            assert!(
                report.notices.iter().any(|n| n.contains("M3")),
                "expected an M3 notice for {args:?}"
            );
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn plain_validation_run_has_no_notices() {
        let root = temp_assets("no-notice", true);
        let args = Args::try_parse_from(["retro-engine", root.to_str().unwrap()]).unwrap();
        let report = resolve_run(&args).unwrap();
        assert!(report.notices.is_empty());
        assert_eq!(report.backend, BackendKind::Sdl3);
        assert_eq!(report.assets.root, root);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn resolve_run_rejects_invalid_assets() {
        let root = std::env::temp_dir().join("retro-engine-cli-resolve-missing");
        let args = Args::try_parse_from(["retro-engine", root.to_str().unwrap()]).unwrap();
        assert!(matches!(
            resolve_run(&args),
            Err(EngineError::MissingAssets(_))
        ));
    }
}
