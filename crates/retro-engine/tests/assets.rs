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
    let mut engine = Engine::load(source(game), Some(scene), None, seed).expect("engine load");
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

/// Builds a scripted replay with player 0 holding RIGHT for `frames` frames.
fn hold_right(frames: usize) -> retro_input::ScriptedInput {
    let mut text = String::from("retro-input 1\n");
    for frame in 0..frames {
        text.push_str(&format!("{frame} RIGHT 0 0 -  - 0 0 -  - 0 0 -  - 0 0 -\n"));
    }
    retro_input::ScriptedInput::from_str(&text).expect("scripted input")
}

fn pcm_hash(samples: &[f32]) -> String {
    let mut hasher = blake3::Hasher::new();
    for sample in samples {
        hasher.update(&sample.to_le_bytes());
    }
    hasher.finalize().to_hex().to_string()
}

fn audio_chain(hashes: &[(u64, String)]) -> String {
    let mut hasher = blake3::Hasher::new();
    for (frame, hash) in hashes {
        hasher.update(&frame.to_le_bytes());
        hasher.update(hash.as_bytes());
    }
    hasher.finalize().to_hex().to_string()
}

fn player_state(engine: &Engine) -> (u8, i32, i32, i32) {
    let player = engine.state.entities.get(0).copied().unwrap_or_default();
    (
        player.type_id,
        player.xpos >> 16,
        player.ypos >> 16,
        player.state,
    )
}

/// M5 asset run: 600 frames with NullInput, a scripted replay and audio hashing.
///
/// Reports the state hash, player position and audio/PCM hashes, verifies two NullInput runs are
/// byte-identical, and checks the shipped S1 `SGame.bin` loads into the save store.
#[test]
#[ignore = "requires assets"]
fn m5_input_audio_save_600_frames() {
    for (game, scene) in [
        ("S1", "Title"),
        ("S1", "Zone01"),
        ("S2", "Title"),
        ("S2", "Zone01"),
    ] {
        let mut first = Engine::load(source(game), Some(scene), None, DEFAULT_SEED).unwrap();
        first.state.audio.set_capture(true);
        let outcome = first.run_frames(600, true).unwrap();
        let first_pcm = pcm_hash(first.state.audio.captured_pcm());
        let first_audio = audio_chain(&outcome.audio_hashes);
        let first_player = player_state(&first);

        let mut second = Engine::load(source(game), Some(scene), None, DEFAULT_SEED).unwrap();
        second.state.audio.set_capture(true);
        let replay = second.run_frames(600, true).unwrap();
        assert_eq!(
            outcome.frame_hashes, replay.frame_hashes,
            "{game}/{scene}: two NullInput runs must replay identically"
        );
        assert_eq!(
            outcome.audio_hashes, replay.audio_hashes,
            "{game}/{scene}: audio hashes must replay identically"
        );
        assert_eq!(
            first_pcm,
            pcm_hash(second.state.audio.captured_pcm()),
            "{game}/{scene}: captured PCM must replay identically"
        );

        let mut scripted = Engine::load(source(game), Some(scene), None, DEFAULT_SEED).unwrap();
        scripted.set_scripted_input(hold_right(600));
        let scripted_outcome = scripted.run_frames(600, false).unwrap();
        let scripted_player = player_state(&scripted);
        if scene == "Zone01" {
            assert!(
                scripted_player.1 > first_player.1,
                "{game}/{scene}: holding RIGHT must move the player right \
                 (idle x={}, scripted x={})",
                first_player.1,
                scripted_player.1
            );
            assert_ne!(
                first.state_hash(),
                scripted.state_hash(),
                "{game}/{scene}: scripted input must change the canonical state"
            );
        }

        println!(
            "{game}/{scene}: hash={} player(type,x,y,state)={first_player:?} \
             audio={first_audio} pcm={first_pcm}",
            outcome.final_hash
        );
        println!(
            "  scripted: hash={} player={scripted_player:?}",
            scripted_outcome.final_hash
        );

        if game == "S1" && scene == "Zone01" {
            let expected = std::fs::read(asset_root().join("S1").join("SGame.bin"))
                .expect("S1 ships SGame.bin");
            let loaded = first.state.save.save_ram().to_bytes();
            assert_eq!(
                loaded, expected,
                "S1 SGame.bin must load byte-exactly through ReadSaveRAM"
            );
            assert_eq!(
                first.state.save.save_file_kind(),
                retro_format_v4::userdata::SaveFileKind::SGame,
                "the only shipped save file is SGame.bin"
            );
            let nonzero = first
                .state
                .save
                .save_ram()
                .words
                .iter()
                .filter(|word| **word != 0)
                .count();
            println!(
                "  save: SGame.bin words={} nonzero={nonzero}",
                first.state.save.save_ram().words.len()
            );
        }
    }
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

/// Regression for the `BoxCollision` subtraction overflow: this stage has only `Act3.bin` and
/// was the first real-data crash the reviewer found.
#[test]
#[ignore = "requires assets"]
fn s2_br8zone09_act3_600_frames_regression() {
    let mut engine = Engine::load(source("S2"), Some("BR8Zone09"), Some("3"), DEFAULT_SEED)
        .expect("BR8Zone09 Act3 must load");
    let outcome = engine
        .run_frames(600, false)
        .expect("BR8Zone09 Act3 must run 600 frames without HostError");
    println!("S2/BR8Zone09 Act3 hash: {}", outcome.final_hash);
    assert_eq!(outcome.frames, 600);
}

/// Boots every `Data/Stages/*/Act*.bin` in S1 and S2 for 60 frames and asserts that none of
/// them panics or returns a `HostError`. This is the sweep that found the `BoxCollision`
/// overflow; it is kept as an asset-gated regression guard.
#[test]
#[ignore = "requires assets"]
fn sweep_every_act_boots_60_frames() {
    use std::time::Instant;

    let root = asset_root();
    let started = Instant::now();
    let mut failures = Vec::new();
    let mut total = 0usize;
    let mut games = vec!["S1".to_owned(), "S2".to_owned()];
    games.sort();
    for game in &games {
        let stages_dir = root.join(game).join("Data/Stages");
        let mut folders: Vec<std::path::PathBuf> = std::fs::read_dir(&stages_dir)
            .unwrap_or_else(|error| panic!("{}: {error}", stages_dir.display()))
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.path().is_dir())
            .map(|entry| entry.path())
            .collect();
        folders.sort();
        for folder in folders {
            let folder_name = folder
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            let mut acts: Vec<(String, std::path::PathBuf)> = std::fs::read_dir(&folder)
                .unwrap_or_else(|error| panic!("{}: {error}", folder.display()))
                .filter_map(|entry| entry.ok())
                .filter_map(|entry| {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    name.strip_prefix("Act")
                        .and_then(|rest| rest.strip_suffix(".bin"))
                        .map(|act| (act.to_owned(), entry.path()))
                })
                .collect();
            acts.sort();
            for (act, path) in acts {
                total += 1;
                let source: Arc<dyn retro_io::DataSource> =
                    Arc::new(DirSource::new(root.join(game)).expect("asset folder"));
                let result = Engine::load(source, Some(&folder_name), Some(&act), DEFAULT_SEED)
                    .and_then(|mut engine| engine.run_frames(60, false).map(|_| ()));
                if let Err(error) = result {
                    failures.push(format!(
                        "{game}/{folder_name}/{}: {error}",
                        path.file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                            .unwrap_or_default()
                    ));
                }
            }
        }
    }
    println!(
        "sweep: {total} acts booted for 60 frames in {:?}",
        started.elapsed()
    );
    assert!(
        failures.is_empty(),
        "{} act(s) failed to boot:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
