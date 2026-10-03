//! Asset-gated regressions for the M6 feedback fixes (title positions and player idle spawn).
//!
//! Run with `cargo test -p retro-engine --test feedback -- --ignored --nocapture`.
//! The asset root defaults to `/home/ted/projects/assets` and can be overridden with
//! `RETRO_ASSETS`.

#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::sync::Arc;

use retro_engine::Engine;
use retro_engine::rng::DEFAULT_SEED;
use retro_io::{DataSource, DirSource};

fn asset_root() -> PathBuf {
    std::env::var("RETRO_ASSETS")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/home/ted/projects/assets"))
}

fn source(game: &str) -> Arc<dyn DataSource> {
    Arc::new(DirSource::new(asset_root().join(game)).expect("asset folder"))
}

fn load(game: &str, scene: &str) -> Engine {
    Engine::load(source(game), Some(scene), None, DEFAULT_SEED).expect("engine load")
}

/// Bounding box `(min_x, max_x, count)` of near-white pixels in `y_min..y_max`.
fn white_bbox(engine: &Engine, y_min: usize, y_max: usize) -> Option<(usize, usize, usize)> {
    let framebuffer = engine.framebuffer();
    let width = framebuffer.width();
    let height = framebuffer.height();
    let mut min_x = width;
    let mut max_x = 0usize;
    let mut count = 0usize;
    for y in y_min..y_max.min(height) {
        for x in 0..width {
            let pixel = framebuffer.get(x as i32, y as i32);
            let r = (pixel >> 11) & 0x1F;
            let g = (pixel >> 5) & 0x3F;
            let b = pixel & 0x1F;
            if r > 26 && g > 54 && b > 26 {
                count += 1;
                min_x = min_x.min(x);
                max_x = max_x.max(x);
            }
        }
    }
    (count > 0).then_some((min_x, max_x, count))
}

/// Bounding box `(min_x, max_x, count)` of any non-black pixel in the whole frame.
fn lit_bbox(engine: &Engine) -> Option<(usize, usize, usize)> {
    let framebuffer = engine.framebuffer();
    let width = framebuffer.width();
    let height = framebuffer.height();
    let mut min_x = width;
    let mut max_x = 0usize;
    let mut count = 0usize;
    for y in 0..height {
        for x in 0..width {
            let pixel = framebuffer.get(x as i32, y as i32);
            if pixel != 0 {
                count += 1;
                min_x = min_x.min(x);
                max_x = max_x.max(x);
            }
        }
    }
    (count > 0).then_some((min_x, max_x, count))
}

fn assert_centred(bbox: Option<(usize, usize, usize)>, label: &str) {
    let (min_x, max_x, count) = bbox.unwrap_or_else(|| panic!("{label}: frame is blank"));
    let centre = (min_x + max_x) as f64 / 2.0;
    assert!(
        count > 500,
        "{label}: only {count} lit pixels (expected the drawn content)"
    );
    assert!(
        (centre - 212.0).abs() <= 4.0,
        "{label}: bbox {min_x}..={max_x} centres at {centre}, expected 212 +/- 4"
    );
}

#[test]
#[ignore = "requires assets"]
fn s2_title_pre_title_text_and_banner_are_centred() {
    let mut engine = load("S2", "Title");
    engine.run_frames(360, false).expect("title must run");
    // The ST Screen layer shows "SONIC AND MILES \"TAILS\" PROWER IN" centred.
    assert_centred(lit_bbox(&engine), "S2 pre-title text");

    let mut engine = load("S2", "Title");
    engine.run_frames(650, false).expect("title must run");
    // The Sonic/Tails logo banner (near-white sprites) must be centred.
    assert_centred(white_bbox(&engine, 40, 170), "S2 title banner");
    let hash = engine.state_hash();

    let mut replay = load("S2", "Title");
    let outcome = replay.run_frames(650, false).expect("title must replay");
    assert_eq!(
        outcome.final_hash, hash,
        "S2 title must render deterministically"
    );
}

#[test]
#[ignore = "requires assets"]
fn s1_title_banner_is_centred_and_stable() {
    let mut engine = load("S1", "Title");
    engine.run_frames(650, false).expect("title must run");
    let logo_type = engine
        .state
        .objects
        .type_id("Logo")
        .expect("S1 title registers the Logo object");
    let logo = (0..retro_scene::ENTITY_COUNT)
        .filter_map(|slot| engine.state.entities.get(slot).copied())
        .find(|entity| entity.type_id == logo_type)
        .expect("the Logo object spawns on the title");
    let frames = &engine.state.object_frames[usize::from(logo_type)];
    let top = &frames[9];
    let bottom = &frames[10];
    let y = logo.ypos >> 16;
    let band = (
        usize::try_from(y + top.pivot_y).unwrap(),
        usize::try_from(y + bottom.pivot_y + bottom.height).unwrap(),
    );
    assert_centred(white_bbox(&engine, band.0, band.1), "S1 title banner");
    let hash = engine.state_hash();

    let mut replay = load("S1", "Title");
    let outcome = replay.run_frames(650, false).expect("title must replay");
    assert_eq!(
        outcome.final_hash, hash,
        "S1 title must render deterministically"
    );
}

/// Reads the script-defined animation index of a GameConfig global (e.g. `ANI_STOPPED`).
fn animation_global(engine: &Engine, name: &str) -> i32 {
    let index = engine
        .state
        .game_config
        .global_variables
        .iter()
        .position(|variable| variable.name == name)
        .unwrap_or_else(|| panic!("{name} global must exist"));
    engine.scripts.vm_state.global_variables[index]
}

#[test]
#[ignore = "requires assets"]
fn player_spawns_in_the_idle_animation() {
    for (game, scene) in [("S1", "Zone01"), ("S2", "Zone01")] {
        let mut engine = load(game, scene);
        engine.run_frames(60, false).expect("level must run");
        let stopped = animation_global(&engine, "ANI_STOPPED");
        let flailing = animation_global(&engine, "ANI_FLAILING1");
        let player = engine.state.entities.get(0).copied().unwrap_or_default();
        assert_eq!(
            player.type_id, 1,
            "{game}/{scene}: slot 0 must be the player object"
        );
        assert_eq!(
            i32::from(player.animation),
            stopped,
            "{game}/{scene}: player must spawn in the idle animation"
        );
        assert_ne!(
            i32::from(player.animation),
            flailing,
            "{game}/{scene}: player must not be balancing/flailing"
        );
        assert_eq!(
            player.floor_sensors[..3],
            [1, 1, 1],
            "{game}/{scene}: L/C/R floor sensors must touch on spawn"
        );
        assert_eq!(player.gravity, 0, "{game}/{scene}: spawn must be grounded");
    }
}
