//! Command line interface for the headless engine.
//!
//! The runtime loads settings/configs/scripts/scenes, runs startup and 60 Hz update/draw events
//! until stopped and prints a BLAKE3 hash of the canonical engine state (which includes the
//! software framebuffer). Runs are unbounded by default: windowed runs stop when the window
//! closes, headless runs when SIGINT/SIGTERM arrives; an explicit `--frames N` caps either mode
//! after `N` frames (`--frames 0` keeps the unbounded default). `--dump-frames DIR` writes the
//! presented RGB565 framebuffer to `frame_%04d.png` headlessly; without `--headless` the same
//! frames are presented through the SDL3 backend at 60 Hz.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clap::Parser;
use retro_audio::{AudioEngine, MAX_BACKLOG_TICKS, MAX_QUEUED_TICKS, PREBUFFER_TICKS, SAMPLE_RATE};
use retro_format_v4::GameConfig;
use retro_input::ScriptedInput;
use retro_io::{DataSource, DirSource};
use retro_platform::{AudioDesc, BackendKind, FsStorage, Storage, WindowDesc};

use crate::EngineError;
use crate::loader;
use crate::runtime::{Engine, FrameLimit};
use crate::save::{seed_memory_storage, seed_storage_from_source};

/// Command line arguments for the engine binary.
#[derive(Debug, Parser)]
#[command(
    name = "retroengine",
    version,
    about = "Retro Engine (RSDK v4 legacy) reimplementation",
    long_about = "Runs an unpacked Sonic 1 or Sonic 2 (RSDK v4 legacy) asset folder.\n\
        Headless mode is deterministic and prints a BLAKE3 state hash; without --headless \
        the game opens in an SDL3 window and runs until the window closes. Headless runs \
        until SIGINT/SIGTERM by default; --frames N caps either mode after N frames.",
    after_help = "EXAMPLES:\n  \
        retroengine C:\\games\\S1 --headless --frames 600\n  \
        retroengine C:\\games\\S1 --scene GHZ --act 1\n  \
        retroengine C:\\games\\S2 --scene \"EMERALD HILL ZONE 1\"\n  \
        retroengine C:\\games\\S1 --list\n\n\
        Scene names are case-insensitive and ignore spaces/punctuation: Zone01, \
        \"GREEN HILL ZONE 1\", GreenHill, GHZ and GHZ2 all work.\n\
        Saves are written to --user-dir, or %APPDATA%\\retroengine-rs\\retroengine on Windows."
)]
pub struct Args {
    /// Path to an unpacked RSDK asset folder (e.g. C:\games\S1)
    pub assets_dir: PathBuf,
    /// Scene to start: a stage folder (`Zone01`), a GameConfig name (`GREEN HILL ZONE 1`),
    /// a short name (`GHZ`, `GreenHill`, `GHZ2`) or a `--list` index (`7`)
    #[arg(long)]
    pub scene: Option<String>,
    /// Act id to start (`1`, `2`, `B`, ...); case-insensitive. Defaults to the GameConfig
    /// entry's id, or a trailing number in `--scene`
    #[arg(long)]
    pub act: Option<String>,
    /// Run without a window using the deterministic headless backend
    #[arg(long)]
    pub headless: bool,
    /// Number of frames to run before exiting; 0 (the default) means run until quit: the
    /// window closes, or headlessly SIGINT/SIGTERM arrives
    #[arg(long, default_value_t = 0)]
    pub frames: u64,
    /// List categories, scenes and available `Act*.bin` files, then exit
    #[arg(long, conflicts_with = "list_json")]
    pub list: bool,
    /// Like `--list` but prints machine-readable JSON
    #[arg(long)]
    pub list_json: bool,
    /// Scripted input file to replay; overrides the windowed SDL input in either mode
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
    /// Print one `frame,blake3` line per frame for the mixed audio
    #[arg(long)]
    pub audio_hash: bool,
    /// Disable audio output; mixing and `--audio-hash` output are unchanged
    #[arg(long)]
    pub mute: bool,
    /// Compile scripts with the Origins (`USE_ORIGINS`) platform tag instead of the default
    /// standalone (`USE_STANDALONE`) platform. Origins data compiles as standalone otherwise,
    /// even when `Settings.ini` has `gameType=1`
    #[arg(long)]
    pub origins: bool,
    /// Directory to persist user data (save RAM) in; seeded from the shipped SData.bin/SGame.bin
    /// on first run. Headless without it uses in-memory storage; windowed runs default to the
    /// SDL preferred path (`%APPDATA%\retroengine-rs\retroengine` on Windows)
    #[arg(long)]
    pub user_dir: Option<PathBuf>,
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

/// Builds the window description from the parsed `Settings.ini` video flags.
///
/// `windowed=y`/`border=y` (the shipped default) yields a bordered windowed window and `vsync`
/// follows the ini; `exclusiveFS` is carried for the backend. The width/height stay the logical
/// framebuffer size, which the renderer scales to the actual window.
#[must_use]
fn window_desc(
    settings: &retro_format_v4::Settings,
    title: String,
    width: u32,
    height: u32,
) -> WindowDesc {
    WindowDesc {
        vsync: settings.video.vsync,
        windowed: settings.video.windowed,
        border: settings.video.border,
        exclusive_fullscreen: settings.video.exclusive_fs,
        ..WindowDesc::new(title, width, height)
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

/// Resolves the user-data storage for this run.
///
/// `--user-dir` always wins. Headless runs without it use in-memory storage seeded with the
/// shipped `SData.bin`/`SGame.bin` so they stay deterministic and never write files; windowed
/// runs use the SDL preferred path. Real directory storages are seeded from the asset folder on
/// first run (when they hold no save yet) so windowed and headless agree.
fn save_storage(
    args: &Args,
    source: &Arc<dyn DataSource>,
) -> Result<Box<dyn Storage>, EngineError> {
    if let Some(dir) = &args.user_dir {
        let mut storage = FsStorage::new(dir);
        seed_storage_from_source(&mut storage, source.as_ref())?;
        return Ok(Box::new(storage));
    }
    if args.headless {
        return Ok(Box::new(seed_memory_storage(source.as_ref())));
    }
    let root = retro_platform::user_data_dir().unwrap_or_else(|| PathBuf::from("retroengine-user"));
    let mut storage = FsStorage::new(root);
    seed_storage_from_source(&mut storage, source.as_ref())?;
    Ok(Box::new(storage))
}

/// One row of the `--list` output.
#[derive(Clone, Debug)]
struct ListedScene {
    /// Global 1-based index accepted by `--scene`.
    index: usize,
    /// File-order category index.
    category: usize,
    /// Stage folder under `Data/Stages`.
    folder: String,
    /// GameConfig act/stage id.
    id: String,
    /// Display name.
    name: String,
    /// Raw "highlighted" byte.
    highlighted: u8,
    /// Available `Act*.bin` ids, discovered case-insensitively.
    acts: Vec<String>,
}

/// Collects `--list` rows, enumerating each stage folder once.
fn collect_listing(game_config: &GameConfig, source: &dyn DataSource) -> Vec<ListedScene> {
    let mut rows = Vec::new();
    let mut acts_cache: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (category, data) in game_config.categories.iter().enumerate() {
        for entry in &data.scenes {
            let acts = acts_cache
                .entry(entry.folder.clone())
                .or_insert_with(|| {
                    loader::available_acts(source, &format!("Data/Stages/{}", entry.folder))
                })
                .clone();
            rows.push(ListedScene {
                index: rows.len() + 1,
                category,
                folder: entry.folder.clone(),
                id: entry.id.clone(),
                name: entry.name.clone(),
                highlighted: entry.highlighted,
                acts,
            });
        }
    }
    rows
}

/// Renders the plain `--list` output.
#[must_use]
pub fn format_listing(game_config: &GameConfig, root: &Path, source: &dyn DataSource) -> String {
    let rows = collect_listing(game_config, source);
    let folder_width = rows.iter().map(|row| row.folder.len()).max().unwrap_or(0);
    let name_width = rows.iter().map(|row| row.name.len()).max().unwrap_or(0);
    let mut out = String::new();
    out.push_str(&format!("game: {}\n", game_config.title));
    out.push_str(&format!("assets dir: {}\n", root.display()));
    out.push_str(&format!("scenes: {}\n", rows.len()));
    out.push_str("categories:\n");
    let mut current = None;
    for row in &rows {
        if current != Some(row.category) {
            current = Some(row.category);
            out.push_str(&format!(
                "[{}] {}\n",
                row.category, game_config.categories[row.category].name
            ));
        }
        let acts = if row.acts.is_empty() {
            "<none>".to_owned()
        } else {
            row.acts.join(", ")
        };
        out.push_str(&format!(
            "  #{:<3} {:<folder_width$} id={:<3} {:<name_width$} acts: {acts}\n",
            row.index, row.folder, row.id, row.name
        ));
    }
    out.push_str("\nuse: retroengine <assets-dir> --scene <folder|name|GHZ|index> [--act <id>]\n");
    out
}

/// Builds the `--list-json` value.
fn listing_json(
    game_config: &GameConfig,
    root: &Path,
    source: &dyn DataSource,
) -> serde_json::Value {
    let rows = collect_listing(game_config, source);
    let categories: Vec<serde_json::Value> = game_config
        .categories
        .iter()
        .enumerate()
        .map(|(index, category)| {
            let scenes: Vec<serde_json::Value> = rows
                .iter()
                .filter(|row| row.category == index)
                .map(|row| {
                    serde_json::json!({
                        "index": row.index,
                        "folder": row.folder,
                        "id": row.id,
                        "name": row.name,
                        "highlighted": row.highlighted,
                        "acts": row.acts,
                    })
                })
                .collect();
            serde_json::json!({
                "index": index,
                "name": category.name,
                "engine_index": GameConfig::engine_category_index(index),
                "scenes": scenes,
            })
        })
        .collect();
    serde_json::json!({
        "game": game_config.title,
        "assets_dir": root.display().to_string(),
        "scene_count": rows.len(),
        "categories": categories,
    })
}

/// Implements `--list`/`--list-json`: prints the scene table and exits successfully.
pub fn list(args: &Args) -> Result<(), EngineError> {
    let assets = resolve_assets(&args.assets_dir)?;
    let source = DirSource::new(&assets.root)?;
    let game_config = GameConfig::load(&source)?;
    if args.list_json {
        let value = listing_json(&game_config, &assets.root, &source);
        let text = serde_json::to_string_pretty(&value)
            .map_err(|error| EngineError::File(std::io::Error::other(error)))?;
        println!("{text}");
    } else {
        print!("{}", format_listing(&game_config, &assets.root, &source));
    }
    Ok(())
}

/// Parses arguments, loads the requested scene and runs the frame loop.
pub fn run(args: &Args) -> Result<(), EngineError> {
    if args.list || args.list_json {
        return list(args);
    }
    // Headless runs have no window or SDL event pump, so Ctrl-C/SIGTERM would otherwise kill the
    // process without flushing saves or printing the run summary. SDL installs equivalent
    // handlers for windowed runs and turns the signals into quit events.
    let quit_signals = args.headless && retro_platform::signals::install();
    let assets = resolve_assets(&args.assets_dir)?;
    let source: Arc<dyn DataSource> = Arc::new(DirSource::new(&assets.root)?);
    let seed = args.seed.unwrap_or(crate::rng::DEFAULT_SEED);

    let mut platform = retro_platform::create(backend_for(args))?;
    platform.init()?;
    let storage = save_storage(args, &source)?;
    let mut engine = Engine::load_with_options(
        Arc::clone(&source),
        args.scene.as_deref(),
        args.act.as_deref(),
        seed,
        storage,
        loader::LoadOptions {
            origins: args.origins,
        },
    )?;

    if let Some(path) = &args.input {
        let bytes = std::fs::read(path)?;
        let scripted = ScriptedInput::load(&bytes)
            .map_err(|error| EngineError::Input(format!("{}: {error}", path.display())))?;
        engine.set_scripted_input(scripted);
    } else if !args.headless {
        engine.set_platform_input();
    }
    engine.set_muted(args.mute);

    let (folder, act) = engine.stage_info();
    let folder = folder.to_owned();
    let act = act.to_owned();

    println!("retro-engine {}", env!("CARGO_PKG_VERSION"));
    println!("assets dir: {}", assets.root.display());
    println!("game config: {}", assets.game_config.display());
    println!("game: {}", engine.game_title());
    println!("scene: {folder} act {act}");
    println!("profile: {}", engine.settings().profile.name());
    println!("platform: {}", engine.settings().platform.name());
    println!("backend: {}", backend_for(args).name());
    // Runs are unbounded unless the user asked for a concrete frame count.
    let frame_limit = FrameLimit::from_frames(args.frames);
    match frame_limit {
        FrameLimit::Bounded(frames) => println!("frames: {frames}"),
        FrameLimit::Unbounded if args.headless => println!("frames: until quit (Ctrl-C/SIGTERM)"),
        FrameLimit::Unbounded => println!("frames: until the window closes"),
    }
    println!("seed: {seed}");
    if !args.headless && args.input.is_none() && !engine.input.has_keyboard_bindings() {
        eprintln!(
            "warning: Settings.ini has no keyboard bindings; gamepads and the mouse still work"
        );
    }
    if let Some(input) = &args.input {
        println!("input: {} (scripted replay)", input.display());
    }
    if let Some(dir) = &args.dump_frames {
        println!(
            "dump frames: {} (every {})",
            dir.display(),
            args.dump_frame_every
        );
    }

    // Windowed runs push mixed audio to the SDL device. A missing device is not fatal: mixing
    // (and therefore `--audio-hash`) continues headlessly. Audio is opened before the window is
    // created so a failing audio backend happens before any window is shown. The engine queues a
    // few ticks before starting the device (SDL opens it paused), so playback starts with
    // headroom instead of underrunning on the first frames.
    if !args.headless {
        if args.mute {
            println!("audio: muted (--mute)");
        } else {
            match platform.open_audio(AudioDesc::stereo(SAMPLE_RATE)) {
                Ok(device) => {
                    let description = device.description();
                    match AudioEngine::with_prebuffer(device) {
                        Ok(audio) => {
                            println!(
                                "audio: {SAMPLE_RATE} Hz stereo f32 ({description}, \
                                 {PREBUFFER_TICKS}-tick prebuffer, {MAX_QUEUED_TICKS}-tick queue, \
                                 {MAX_BACKLOG_TICKS}-tick backlog)"
                            );
                            engine.set_audio_device(audio);
                        }
                        Err(error) => println!("audio: unavailable: {error}"),
                    }
                }
                Err(error) => println!("audio: unavailable: {error}"),
            }
        }
    }

    // The engine is fully loaded (and any audio device opened) before the window exists, so a
    // slow or failed load cannot leave a half-created window on screen.
    let (width, height) = (
        engine.framebuffer().width() as u32,
        engine.framebuffer().height() as u32,
    );
    let mut window = if args.headless {
        None
    } else {
        let title = format!("{} - {folder} {act}", engine.game_title());
        let mut window = platform.create_window(window_desc(
            engine.raw_settings(),
            title.clone(),
            width,
            height,
        ))?;
        // Title again right after creation so the window is never shown untitled on backends
        // that only apply the title after the first present.
        window.set_title(&title);
        if let Some(driver) = platform.video_driver() {
            println!("video: {driver}");
        }
        Some(window)
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
    let mut executed = 0u64;
    let mut stopped_by_signal = false;
    loop {
        if frame_limit.reached(executed) {
            break;
        }
        if quit_signals && retro_platform::signals::requested() {
            stopped_by_signal = true;
            break;
        }
        executed += 1;
        // Pump SDL events every frame a window exists so close/resize/quit keep working with
        // scripted input (`--input`), which bypasses platform polling; raw device state is only
        // forwarded in platform-input mode.
        if window.is_some() {
            let raw = platform.input().poll_raw();
            if engine.input.uses_platform_input() {
                engine.set_raw_input(raw);
            }
        }
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
        if args.audio_hash {
            println!("{frame},{}", engine.audio_hash());
        }
    }

    // Device flow-control totals: dropped ticks (overrun resyncs) and underruns are the audible
    // gaps the frame loop cannot show, so they are always reported for windowed runs.
    if let Some(report) = engine.audio_diagnostics() {
        println!("audio: {report}");
    }

    if !engine.flush_save() {
        eprintln!("warning: could not persist save RAM");
    }
    if stopped_by_signal {
        println!("quit: signal");
    }
    println!("executed-frames: {executed}");
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
        assert!(!args.list);
        assert!(!args.list_json);
        assert!(!args.hash_every_frame);
        assert!(!args.audio_hash);
        assert!(!args.mute);
        assert!(!args.origins);
        assert_eq!(args.user_dir, None);
        assert_eq!(args.dump_frame_every, 1);
    }

    #[test]
    fn window_desc_defaults_to_a_bordered_windowed_window_and_reads_settings() {
        // `WindowDesc::new` is the Windows-safe default: windowed, bordered, vsync on.
        let default = WindowDesc::new("default", 424, 240);
        assert!(default.windowed, "default must be windowed");
        assert!(default.border, "default must be bordered");
        assert!(default.vsync, "default must request vsync");
        assert!(!default.exclusive_fullscreen);
        assert!(default.integer_scale);

        // The shipped `windowed=y border=y` ini yields exactly that bordered windowed desc.
        let settings = retro_format_v4::Settings::parse(
            "[Video]\nwindowed=y\nborder=y\nexclusiveFS=n\nvsync=y\n",
        )
        .unwrap();
        let desc = window_desc(&settings, "Sonic Test".to_owned(), 424, 240);
        assert!(desc.windowed);
        assert!(desc.border);
        assert!(desc.vsync);
        assert!(!desc.exclusive_fullscreen);
        assert_eq!(desc.title, "Sonic Test");
        assert_eq!((desc.width, desc.height), (424, 240));

        // A fullscreen/borderless ini is carried through instead.
        let settings = retro_format_v4::Settings::parse(
            "[Video]\nwindowed=n\nborder=n\nexclusiveFS=y\nvsync=n\n",
        )
        .unwrap();
        let desc = window_desc(&settings, "Full".to_owned(), 424, 240);
        assert!(!desc.windowed);
        assert!(!desc.border);
        assert!(desc.exclusive_fullscreen);
        assert!(!desc.vsync);
    }

    #[test]
    fn frames_zero_and_the_default_are_unbounded() {
        // Omitted `--frames` parses as 0, which must mean "run until quit", not a hidden cap.
        let args = Args::try_parse_from(["retro-engine", "/tmp/assets"]).unwrap();
        assert_eq!(args.frames, 0);
        assert_eq!(FrameLimit::from_frames(args.frames), FrameLimit::Unbounded);
        assert_eq!(FrameLimit::from_frames(0), FrameLimit::Unbounded);
        assert_eq!(FrameLimit::from_frames(1), FrameLimit::Bounded(1));
        assert_eq!(FrameLimit::from_frames(600), FrameLimit::Bounded(600));

        assert!(!FrameLimit::Unbounded.reached(0));
        assert!(!FrameLimit::Unbounded.reached(u64::MAX));
        assert!(!FrameLimit::Bounded(5).reached(4));
        assert!(FrameLimit::Bounded(5).reached(5));
        assert!(FrameLimit::Bounded(5).reached(6));
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
            "--audio-hash",
            "--mute",
            "--origins",
            "--user-dir",
            "user",
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
        assert!(args.audio_hash);
        assert!(args.mute);
        assert!(args.origins);
        assert_eq!(args.user_dir, Some(PathBuf::from("user")));
        assert!(!args.list);
        assert_eq!(backend_for(&args), BackendKind::Headless);
    }

    fn listing_config() -> GameConfig {
        use retro_format_v4::gameconfig::PALETTE_COUNT;
        use retro_format_v4::{SceneCategory, SceneEntry};

        let mut config = GameConfig {
            title: "Sonic Test".to_owned(),
            subtitle: String::new(),
            palette: vec![[0, 0, 0]; PALETTE_COUNT],
            objects: Vec::new(),
            global_variables: Vec::new(),
            sound_effects: Vec::new(),
            players: Vec::new(),
            categories: retro_format_v4::gameconfig::CATEGORY_NAMES
                .iter()
                .map(|name| SceneCategory {
                    name: (*name).to_owned(),
                    scenes: Vec::new(),
                })
                .collect(),
        };
        config.categories[1].scenes = vec![
            SceneEntry {
                folder: "Zone01".to_owned(),
                id: "1".to_owned(),
                name: "GREEN HILL ZONE 1".to_owned(),
                highlighted: 1,
            },
            SceneEntry {
                folder: "Zone01".to_owned(),
                id: "2".to_owned(),
                name: "2".to_owned(),
                highlighted: 0,
            },
        ];
        config
    }

    #[test]
    fn listing_prints_categories_scenes_and_acts() {
        let mut source = retro_io::MemorySource::new();
        source.insert("Data/Stages/Zone01/Act1.bin", vec![0]);
        source.insert("Data/Stages/Zone01/ActB.bin", vec![0]);
        let text = format_listing(&listing_config(), Path::new("/assets/S1"), &source);
        assert!(text.contains("game: Sonic Test"), "{text}");
        assert!(text.contains("[1] Regular"), "{text}");
        assert!(text.contains("GREEN HILL ZONE 1"), "{text}");
        assert!(text.contains("acts: 1, B"), "{text}");
        assert!(text.contains("--scene"), "{text}");
    }

    #[test]
    fn listing_json_exposes_indexes_and_engine_categories() {
        let source = retro_io::MemorySource::new();
        let value = listing_json(&listing_config(), Path::new("/assets/S1"), &source);
        assert_eq!(value["game"], "Sonic Test");
        assert_eq!(value["scene_count"], 2);
        assert_eq!(value["categories"][1]["name"], "Regular");
        assert_eq!(value["categories"][1]["engine_index"], 1);
        assert_eq!(value["categories"][2]["engine_index"], 3);
        assert_eq!(value["categories"][3]["engine_index"], 2);
        assert_eq!(value["categories"][1]["scenes"][0]["index"], 1);
        assert_eq!(value["categories"][1]["scenes"][0]["folder"], "Zone01");
        assert_eq!(value["categories"][1]["scenes"][1]["index"], 2);
    }

    #[test]
    fn list_flags_parse_and_conflict() {
        let args = Args::try_parse_from(["retroengine", "/tmp/assets", "--list"]).unwrap();
        assert!(args.list);
        assert!(!args.list_json);
        let args = Args::try_parse_from(["retroengine", "/tmp/assets", "--list-json"]).unwrap();
        assert!(args.list_json);
        let error = Args::try_parse_from(["retroengine", "/tmp/assets", "--list", "--list-json"])
            .unwrap_err();
        assert_eq!(error.kind(), clap::error::ErrorKind::ArgumentConflict);
    }

    #[test]
    fn help_shows_examples_and_scene_forms() {
        let error = Args::try_parse_from(["retroengine", "--help"]).unwrap_err();
        assert_eq!(error.kind(), clap::error::ErrorKind::DisplayHelp);
        let help = error.to_string();
        assert!(help.contains("EXAMPLES:"), "{help}");
        assert!(help.contains("--scene"), "{help}");
        assert!(help.contains("GHZ"), "{help}");
        assert!(help.contains("--list"), "{help}");
        assert!(help.contains("--origins"), "{help}");
    }

    #[test]
    fn input_flag_rejects_a_missing_value() {
        let error = Args::try_parse_from(["retro-engine", "/tmp/assets", "--input"]).unwrap_err();
        assert!(matches!(
            error.kind(),
            clap::error::ErrorKind::InvalidValue | clap::error::ErrorKind::MissingRequiredArgument
        ));
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
