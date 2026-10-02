//! Asset-gated M4 render runs: real S1/S2 scenes rendered for 600 frames with framebuffer
//! hashing, frame dumps and stub-histogram checks.
//!
//! Run with `cargo test --release -p retro-engine --test render_assets -- --ignored --nocapture`.
//! The asset root defaults to `/home/ted/projects/assets` and can be overridden with
//! `RETRO_ASSETS`. Frames are dumped to `/home/ted/projects/tmp/m4-frames/<game>/` (overridable
//! with `RETRO_FRAMES`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use retro_engine::Engine;
use retro_engine::rng::DEFAULT_SEED;
use retro_io::DirSource;

/// Frame numbers dumped for `Zone01` runs (frame 0 is the pre-loop framebuffer).
const DUMP_FRAMES: [u64; 5] = [0, 60, 120, 300, 599];

fn asset_root() -> PathBuf {
    std::env::var("RETRO_ASSETS")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/home/ted/projects/assets"))
}

fn dump_root() -> PathBuf {
    std::env::var("RETRO_FRAMES")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/home/ted/projects/tmp/m4-frames"))
}

fn source(game: &str) -> Arc<dyn retro_io::DataSource> {
    Arc::new(DirSource::new(asset_root().join(game)).expect("asset folder"))
}

/// One completed render run.
struct RenderRun {
    final_hash: String,
    /// `(frame, non-blank ratio)` for every dumped frame.
    dumped: Vec<(u64, f64)>,
    /// `name -> count` of ops that are still deterministic stubs.
    stubs: BTreeMap<String, u64>,
    /// `name -> count` of every executed op.
    ops: BTreeMap<String, u64>,
    /// `(frame, framebuffer hash)` of every frame when per-frame hashing is requested.
    frame_hashes: Vec<(u64, String)>,
}

impl RenderRun {
    fn stub_list(&self) -> String {
        self.stubs
            .iter()
            .map(|(name, count)| format!("{name}={count}"))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Loads a scene and runs it for `frames`, optionally dumping the `Zone01` frame set.
fn render(game: &str, scene: &str, frames: u64, dump: bool, per_frame: bool) -> RenderRun {
    let mut engine =
        Engine::load(source(game), Some(scene), None, DEFAULT_SEED).expect("engine load");
    let mut dumped = Vec::new();
    let mut frame_hashes = Vec::new();
    if per_frame {
        frame_hashes.push((0, engine.state_hash()));
    }
    if dump {
        let dir = dump_root().join(game);
        std::fs::create_dir_all(&dir).expect("create dump dir");
        dumped.push((0, dump_frame(&dir, 0, &engine)));
    }
    for target in 1..=frames {
        engine
            .run_frame()
            .unwrap_or_else(|error| panic!("{game}/{scene} frame {target}: {error}"));
        if per_frame {
            frame_hashes.push((target, engine.state_hash()));
        }
        if dump && DUMP_FRAMES.contains(&target) {
            let dir = dump_root().join(game);
            dumped.push((target, dump_frame(&dir, target, &engine)));
        }
    }
    RenderRun {
        final_hash: engine.state_hash(),
        dumped,
        stubs: engine.stub_histogram().clone(),
        ops: engine.op_histogram().clone(),
        frame_hashes,
    }
}

/// Writes one framebuffer to `DIR/frame_%04d.png` and returns its non-blank pixel ratio.
fn dump_frame(dir: &Path, frame: u64, engine: &Engine) -> f64 {
    let framebuffer = engine.framebuffer();
    let path = dir.join(format!("frame_{frame:04}.png"));
    let png = framebuffer.to_png_bytes().expect("encode PNG");
    std::fs::write(&path, png).expect("write PNG");
    framebuffer.non_black_ratio()
}

fn assert_no_unknown_ops(run: &RenderRun, game: &str, scene: &str) {
    assert!(
        !run.ops.contains_key("Unimplemented"),
        "{game}/{scene} executed an op with no ported implementation"
    );
    assert!(
        !run.stubs.contains_key("Unimplemented"),
        "{game}/{scene} executed an unknown op"
    );
}

fn report(game: &str, scene: &str, run: &RenderRun) {
    println!("=== {game}/{scene}: 600 frames ===");
    println!("final hash: {}", run.final_hash);
    for (frame, ratio) in &run.dumped {
        println!("  frame {frame:03}: non-blank ratio {ratio:.4}");
    }
    println!("stubbed-ops: {}", run.stub_list());
}

/// Runs one scene twice and asserts byte-identical per-frame hashes.
fn assert_deterministic(game: &str, scene: &str, dump: bool) -> RenderRun {
    let first = render(game, scene, 600, dump, true);
    assert_no_unknown_ops(&first, game, scene);
    let second = render(game, scene, 600, false, true);
    assert_eq!(
        first.frame_hashes, second.frame_hashes,
        "{game}/{scene} must replay identically"
    );
    assert_eq!(first.final_hash, second.final_hash);
    report(game, scene, &first);
    first
}

#[test]
#[ignore = "requires assets"]
fn zone01_act_num_matches_the_resolved_act() {
    for (act, expected) in [("1", 1), ("2", 2), ("3", 3)] {
        let engine =
            Engine::load(source("S1"), Some("Zone01"), Some(act), DEFAULT_SEED).expect("load");
        assert_eq!(engine.state.stage.act_num, expected, "act {act}");
    }
    // Bonus stages use non-numeric act ids; upstream boot leaves the fresh actID at 0.
    let engine = Engine::load(source("S2"), Some("Zone01"), Some("B"), DEFAULT_SEED).expect("load");
    assert_eq!(engine.state.stage.act_num, 0, "act B");
}

#[test]
#[ignore = "requires assets"]
fn s1_title_render_600_frames() {
    let run = assert_deterministic("S1", "Title", false);
    assert_eq!(run.frame_hashes.len(), 601);
}

#[test]
#[ignore = "requires assets"]
fn s1_zone01_render_600_frames() {
    let run = assert_deterministic("S1", "Zone01", true);
    // The title card covers the screen for the first ~200 frames; the stage must be visible in
    // the later dumps.
    for frame in [300u64, 599] {
        let ratio = run
            .dumped
            .iter()
            .find(|(dumped, _)| *dumped == frame)
            .map(|(_, ratio)| *ratio)
            .unwrap_or(0.0);
        assert!(ratio > 0.25, "S1/Zone01 frame {frame} is blank ({ratio})");
    }
}

#[test]
#[ignore = "requires assets"]
fn s2_title_render_600_frames() {
    let run = assert_deterministic("S2", "Title", false);
    assert_eq!(run.frame_hashes.len(), 601);
}

#[test]
#[ignore = "requires assets"]
fn s2_zone01_render_600_frames() {
    let run = assert_deterministic("S2", "Zone01", true);
    for frame in [300u64, 599] {
        let ratio = run
            .dumped
            .iter()
            .find(|(dumped, _)| *dumped == frame)
            .map(|(_, ratio)| *ratio)
            .unwrap_or(0.0);
        assert!(ratio > 0.25, "S2/Zone01 frame {frame} is blank ({ratio})");
    }
}

/// Prints a combined summary of all four required scenes and their dumped frames.
#[test]
#[ignore = "requires assets"]
fn all_required_scenes_render_summary() {
    for (game, scene) in [
        ("S1", "Title"),
        ("S1", "Zone01"),
        ("S2", "Title"),
        ("S2", "Zone01"),
    ] {
        let first = render(game, scene, 600, scene == "Zone01", true);
        assert_no_unknown_ops(&first, game, scene);
        let second = render(game, scene, 600, false, false);
        assert_eq!(first.final_hash, second.final_hash);
        report(game, scene, &first);
        println!(
            "  ported op count: {}",
            first.ops.len().saturating_sub(first.stubs.len())
        );
    }
}
