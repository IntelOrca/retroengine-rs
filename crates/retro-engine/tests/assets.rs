//! Asset-gated 600-frame headless runs.
//!
//! Run with `cargo test -p retro-engine --test assets -- --ignored --nocapture`.
//! The asset root defaults to `/home/ted/projects/assets` and can be overridden with
//! `RETRO_ASSETS`.

use std::path::PathBuf;
use std::sync::Arc;

use retro_engine::Engine;
use retro_engine::rng::DEFAULT_SEED;
use retro_io::DirSource;

fn asset_root() -> PathBuf {
    std::env::var("RETRO_ASSETS")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/home/ted/projects/assets"))
}

fn source(game: &str) -> Arc<dyn retro_io::DataSource> {
    Arc::new(DirSource::new(asset_root().join(game)).expect("asset folder"))
}

fn run(
    game: &str,
    scene: &str,
    seed: u32,
    hash_every_frame: bool,
) -> (Engine, retro_engine::RunOutcome) {
    let mut engine = Engine::load(source(game), Some(scene), 1, seed).expect("engine load");
    let outcome = engine
        .run_frames(600, hash_every_frame)
        .expect("600-frame run must not fail");
    (engine, outcome)
}

fn histogram(engine: &Engine) -> (String, String) {
    let ported: Vec<String> = engine
        .op_histogram()
        .iter()
        .filter(|(name, _)| !engine.stub_histogram().contains_key(*name))
        .map(|(name, count)| format!("{name}={count}"))
        .collect();
    let stubs: Vec<String> = engine
        .stub_histogram()
        .iter()
        .map(|(name, count)| format!("{name}={count}"))
        .collect();
    (ported.join(" "), stubs.join(" "))
}

fn assert_600_frames(game: &str, scene: &str) {
    let (first, outcome) = run(game, scene, DEFAULT_SEED, true);
    assert_eq!(outcome.frames, 600);
    assert_eq!(outcome.frame_hashes.len(), 601, "frame 0 plus 600 frames");
    assert_eq!(outcome.frame_hashes[600].1, outcome.final_hash);

    // Determinism: a second run of the same data must produce the same final hash, and the
    // per-frame hashes must replay byte for byte (checked by a fresh per-frame run).
    let (second, replay) = run(game, scene, DEFAULT_SEED, true);
    assert_eq!(
        outcome.frame_hashes, replay.frame_hashes,
        "{game}/{scene} must replay identically"
    );
    assert_eq!(first.state_hash(), second.state_hash());

    let (third, _) = run(game, scene, DEFAULT_SEED ^ 0xA5A5_5A5A, false);
    assert_ne!(
        first.state_hash(),
        third.state_hash(),
        "a different seed must change the hash"
    );

    let (folder, act) = first.stage_info();
    let (ported, stubs) = histogram(&first);
    println!("=== {game} {folder} act {act} (600 frames) ===");
    println!("final hash: {}", outcome.final_hash);
    println!("frame hashes: {}", outcome.frame_hashes.len());
    println!("ported-ops: {ported}");
    println!("stubbed-ops: {stubs}");
}

#[test]
#[ignore = "requires assets"]
fn s1_title_600_frames() {
    assert_600_frames("S1", "Title");
}

#[test]
#[ignore = "requires assets"]
fn s1_zone01_600_frames() {
    assert_600_frames("S1", "Zone01");
}

#[test]
#[ignore = "requires assets"]
fn s2_title_600_frames() {
    assert_600_frames("S2", "Title");
}

#[test]
#[ignore = "requires assets"]
fn s2_zone01_600_frames() {
    assert_600_frames("S2", "Zone01");
}

/// Prints a combined summary of the four required scenes.
#[test]
#[ignore = "requires assets"]
fn all_required_scenes_summary() {
    let mut totals = 0;
    for (game, scene) in [
        ("S1", "Title"),
        ("S1", "Zone01"),
        ("S2", "Title"),
        ("S2", "Zone01"),
    ] {
        let (engine, outcome) = run(game, scene, DEFAULT_SEED, false);
        let (ported, stubs) = histogram(&engine);
        let player = engine.state.entities.get(0).copied().unwrap_or_default();
        println!(
            "{game}/{scene}: hash={} ported=[{}] stubs=[{}]",
            outcome.final_hash, ported, stubs
        );
        println!(
            "  player slot0: type={} pos=({},{}) state={} yvel={} sensors={:?}",
            player.type_id,
            player.xpos >> 16,
            player.ypos >> 16,
            player.state,
            player.yvel,
            player.floor_sensors
        );
        totals += 1;
    }
    println!("totals: {totals} scenes x 600 frames");
}
