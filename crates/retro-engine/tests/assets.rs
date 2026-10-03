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

/// BLAKE3 of the visible RGB565 framebuffer, rows contiguous and little-endian, exactly the
/// `fb.blake3` value the C reference harness writes for each frame.
fn framebuffer_hash(engine: &Engine) -> String {
    let framebuffer = engine.framebuffer();
    let mut bytes = Vec::with_capacity(framebuffer.width() * framebuffer.height() * 2);
    for y in 0..framebuffer.height() {
        for x in 0..framebuffer.width() {
            bytes.extend_from_slice(&framebuffer.get(x as i32, y as i32).to_le_bytes());
        }
    }
    blake3::hash(&bytes).to_hex().to_string()
}

/// Pinned `fb.blake3` values from `tools/ref-harness/run.sh` with the header-only idle input
/// (`testdata/zone01_idle.input`; the reference seeds `1592594996`, the Rust run uses
/// `DEFAULT_SEED` and the frames below do not depend on `Rand`). Frames 0-600 are
/// pixel-identical, which covers the tile layers, the paused title-card stage mode, the
/// faithful `SetPlayerScreenPosition` camera and the palette rotation phase.
const S1_ZONE01_IDLE: &[(u64, &str)] = &[
    (
        0,
        "df783f7531b9128e26bbff557953acb02e66a23867e9d05e89be35b6d61b445b",
    ),
    (
        60,
        "7c91097af19244c9d9c878134d1ab692633e0ae9a15b0c41f010076b66db7cd3",
    ),
    (
        155,
        "0ad5666c971afd738e1f4a5ef1e763e82229285bf3a32a9b9f87e878dce95d79",
    ),
    (
        240,
        "0134d81718a04d39a02dccc39f4f8a99551531b0254b5cf54642ce713c32ca60",
    ),
    (
        600,
        "36167df6df9716003b03a39ec3693c3ea090eed8ea7f9634f02e4d1672485940",
    ),
];
const S2_ZONE01_IDLE: &[(u64, &str)] = &[
    (
        0,
        "df783f7531b9128e26bbff557953acb02e66a23867e9d05e89be35b6d61b445b",
    ),
    (
        60,
        "bbd21b4d1a50717f804317784d525574e45bf8479714fad95b7b08bf1d4ef819",
    ),
    (
        155,
        "13898f40d171bac3b568dffc8822f814113ad200601a70e3c7fbe2508c301dda",
    ),
    (
        240,
        "33aa287fc9677419baf3f8cb49e4f61978b4c97265b11cdf1aa7de70d509c10c",
    ),
    (
        600,
        "1442e6f5545812096bdd3527a3f8be0c39b03d59a298b31ce5a4f270d852a038",
    ),
];

/// Pinned `fb.blake3` values from `tools/ref-harness/run.sh` with `zone01_right.input` (idle
/// through frame 59, then RIGHT held). All 600 frames are pixel-identical; the pins include the
/// spring launch (350), the S2 Monitor that the `ForEachAll` stack bug used to destroy (360),
/// the S1 death trigger (376) and the first frame the frozen death update used to miss (377).
const S1_ZONE01_RIGHT: &[(u64, &str)] = &[
    (
        0,
        "df783f7531b9128e26bbff557953acb02e66a23867e9d05e89be35b6d61b445b",
    ),
    (
        155,
        "0ad5666c971afd738e1f4a5ef1e763e82229285bf3a32a9b9f87e878dce95d79",
    ),
    (
        350,
        "5e3ef5d2e61da802a62d6c1716c2b3ea3682ff773e44825570cbcc381a9264ed",
    ),
    (
        360,
        "3219999b801abb7411a69d10c80179a639a60dec5031ba4b52f6b83e742960b0",
    ),
    (
        376,
        "6958cb611f9298cc84449455a21ef9a4c89636df481e2619d1e48385dd7a83df",
    ),
    (
        377,
        "f103ba91b014c90d1f9f4fce28db635d61a87f7be9c6244779bf549d799d1dab",
    ),
    (
        390,
        "81cd82c671ceefafdcad0407d6cd75b9ad245223e9233b55d86d1493240e42c5",
    ),
    (
        450,
        "a8c13aa2185bfd190270f88a817589de00ced1c3270dc71a70af259c56778e24",
    ),
    (
        599,
        "303d8314155c837392dd7581f3a1a09e4fb92b95f9c690d14cf912a7e86062a1",
    ),
];
const S2_ZONE01_RIGHT: &[(u64, &str)] = &[
    (
        0,
        "df783f7531b9128e26bbff557953acb02e66a23867e9d05e89be35b6d61b445b",
    ),
    (
        155,
        "13898f40d171bac3b568dffc8822f814113ad200601a70e3c7fbe2508c301dda",
    ),
    (
        350,
        "ac5763aa55bc3f6467094ddba0c0d1ae57f4719d2740c9a66b6cb78f4bbed896",
    ),
    (
        360,
        "20566e18acd09bc5500b5ab3a0e35304ebbe8d29f8b033e41665bd02e6c4a3f6",
    ),
    (
        376,
        "f7ccf5b3f2f196c2895d19be1049ef1866831b26c5658ebf5f74a8b5b8704ec8",
    ),
    (
        390,
        "60b4237653ddcf3359ce910432eecd717fd07bed6c7d941c94a7a78c8740a751",
    ),
    (
        599,
        "0963dfe800f1e877761b686dd3760a5dda0c95f52cc5b5eceebedb6c8481fa68",
    ),
];

#[test]
#[ignore = "requires assets; pins reference-harness framebuffer hashes"]
fn zone01_idle_framebuffer_matches_reference() {
    for (game, pins) in [("S1", S1_ZONE01_IDLE), ("S2", S2_ZONE01_IDLE)] {
        let mut engine = Engine::load(source(game), Some("Zone01"), Some("1"), DEFAULT_SEED)
            .expect("engine load");
        let mut frame = 0u64;
        for &(target, expected) in pins {
            while frame < target {
                engine.run_frame().expect("frame");
                frame += 1;
            }
            assert_eq!(
                framebuffer_hash(&engine),
                expected,
                "{game}/Zone01 idle frame {target} must match the reference harness"
            );
        }
    }
}

fn scripted_right_input() -> retro_input::ScriptedInput {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools/ref-harness/testdata/zone01_right.input");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
    retro_input::ScriptedInput::from_str(&text).expect("scripted input")
}

#[test]
#[ignore = "requires assets; pins reference-harness framebuffer hashes"]
fn zone01_right_framebuffer_matches_reference() {
    for (game, pins) in [("S1", S1_ZONE01_RIGHT), ("S2", S2_ZONE01_RIGHT)] {
        let scripted = scripted_right_input();
        let seed = scripted.seed().unwrap_or(DEFAULT_SEED);
        let mut engine =
            Engine::load(source(game), Some("Zone01"), Some("1"), seed).expect("engine load");
        engine.set_scripted_input(scripted);
        let mut frame = 0u64;
        for &(target, expected) in pins {
            while frame < target {
                engine.run_frame().expect("frame");
                frame += 1;
            }
            assert_eq!(
                framebuffer_hash(&engine),
                expected,
                "{game}/Zone01 hold-RIGHT frame {target} must match the reference harness"
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
