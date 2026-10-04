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

/// Pinned `fb.blake3` values for `zone01_right400.input` (RIGHT held from line 400). Record 400
/// is the first frame the input applies; before the tick-alignment fix Rust consumed line 399
/// and diverged here. The reference records are indexed by absolute tick, so line `N` belongs
/// to record `N`.
const S1_ZONE01_RIGHT400: &[(u64, &str)] = &[
    (
        399,
        "54cea0d080abb5365fe92eb6762997dc5e77bff5173547a3167e129e20dcfda8",
    ),
    (
        400,
        "74b1da08461bf04d13ddcca239c9f2b398fa1d146e7e390868a3506512718839",
    ),
    (
        401,
        "4946532824b4a462fb18cd8eead520d2e7861ead44844abeba9d4929dabe1fce",
    ),
    (
        450,
        "6902212df1cc68574b5b1a0e50b164d9691a86dc9cc1e256522b4c9c32a5b97f",
    ),
    (
        599,
        "6c868397f495761b00f4bf9b62060031d56848e2b5092f18953c4df69a62bdca",
    ),
];
const S2_ZONE01_RIGHT400: &[(u64, &str)] = &[
    (
        399,
        "70aeaa0e71249ff6d10f462472a87ecd337c775f5fc6d1d22e02bf6a7dc01778",
    ),
    (
        400,
        "cf850e33011f1ba1de0ba0eead838c349c5082e119efd79480abc4b001c029e3",
    ),
    (
        401,
        "cf850e33011f1ba1de0ba0eead838c349c5082e119efd79480abc4b001c029e3",
    ),
    (
        450,
        "c4f7c627245f8d8f96bd728f3dd39c1826258450f5c432ceb77bebb3ec50eca4",
    ),
    (
        599,
        "de712689faecf9856e083a5dfb28fac6abb832d11b0d6635f8a246dd1b4c39eb",
    ),
];

/// Pinned `fb.blake3` values for `zone01_pause.input` (START held from line 200). The Player
/// object's pause path writes `engine.state = 5` (`ENGINE_INITPAUSE`); upstream's
/// `Legacy::v4::ProcessEngine` resets the mode and skips that frame entirely, so the ring
/// animation advances one frame later than a naive normal-stage pass would. The pins bracket the
/// skipped frame (201) and the first frame after it.
const S1_ZONE01_PAUSE: &[(u64, &str)] = &[
    (
        199,
        "ef6c6a3fc1e658c5b39b613a96e2cf3f76eb26ad01e7e14808b45b0f5d37b4a8",
    ),
    (
        200,
        "75ca993239d1846c99ab64575ced2f2660d08844de611ef7603223a5ea4e64a2",
    ),
    (
        201,
        "75ca993239d1846c99ab64575ced2f2660d08844de611ef7603223a5ea4e64a2",
    ),
    (
        202,
        "17589b2e8ea1dd431a14fd96d3d507a7e230ea19ac5c6d973a33a0c30e9cb922",
    ),
    (
        203,
        "1e88e6f8c55cd0e15ebf70ec7e0deba3d5ba25017d35c536283fa3a842e59a10",
    ),
    (
        204,
        "348a3fdf922a0aebd5ff1d87b62d2860ccdb0cf5b9f3ae63762808594c39eab4",
    ),
    (
        205,
        "ddb215c828cf362a6f432fc556da664e6bf5b3b17e9d1f382eb28d80ba14652f",
    ),
    (
        250,
        "28762717777d553b6485d86152523650a54dc632f68154e41b99daeb898aadac",
    ),
];
const S2_ZONE01_PAUSE: &[(u64, &str)] = &[
    (
        199,
        "bd448c9e3a9d4aebe9438461ef1ceb3824dc7cc1fddc43d6088d3a27c3c94403",
    ),
    (
        200,
        "bd448c9e3a9d4aebe9438461ef1ceb3824dc7cc1fddc43d6088d3a27c3c94403",
    ),
    (
        201,
        "bd448c9e3a9d4aebe9438461ef1ceb3824dc7cc1fddc43d6088d3a27c3c94403",
    ),
    (
        202,
        "bd448c9e3a9d4aebe9438461ef1ceb3824dc7cc1fddc43d6088d3a27c3c94403",
    ),
    (
        203,
        "bd448c9e3a9d4aebe9438461ef1ceb3824dc7cc1fddc43d6088d3a27c3c94403",
    ),
    (
        204,
        "45a45e8a77d904addb8547b06d124340cf7228075ed1506e3e4d7af4747f4b05",
    ),
    (
        205,
        "45a45e8a77d904addb8547b06d124340cf7228075ed1506e3e4d7af4747f4b05",
    ),
    (
        250,
        "191f16026cf57e7b50ee995578caa8ecca26fb6c5137ba8108cf4c91cf6d546a",
    ),
];

/// Pinned `fb.blake3` values for `title_start.input` (Title screen, A held from line 800). The
/// pins bracket the START press (800/801), the Zone01 load, and the scripted-press frame the
/// reference synthesizes from the held A once control unlocks (1010/1028) — the last one only
/// matches after scripts are allowed to write `keyDown`/`keyPress`.
const S1_TITLE_START: &[(u64, &str)] = &[
    (
        799,
        "121a1ddd7805bd80b7d2d15c3823bda3c4dccbf84e7ca59891844e2e13de7c73",
    ),
    (
        800,
        "b376db1445d3d79e1a3302f03815148e8bc4840ddf3faef4b1ecdc0a04c022f1",
    ),
    (
        801,
        "55c43613158bf94016dd4735377fde6d90c1df04c1c9b4a9aa8cbc96896fda67",
    ),
    (
        810,
        "470deca280529f3a3c818d7cf9a569eeab4b2ec1e91230dd203b4f59b7d7a052",
    ),
    (
        900,
        "3408e5d452de309ff60e8139aa8d202ed4e3d2750ca5934da7c2da367902d648",
    ),
    (
        1010,
        "43b17467378d67a65515f366a904b6b8fdbeba9eac5e260788541b3891eb66b8",
    ),
    (
        1028,
        "8d08448bc063d3d27db260b805f662d7b2778f697e381c8df215dd3b36b6952d",
    ),
    (
        1199,
        "ce03a1fd2f2a0602b2a7103bc411338148a0b2fe7a802093b2b07494e7f433c6",
    ),
];
const S2_TITLE_START: &[(u64, &str)] = &[
    (
        799,
        "019a5d0ba09dd477845b4229c77a0e3bc2e42b3c936bd6544df769d872400287",
    ),
    (
        800,
        "e4a9da8eb828d77bb0ea567f29c99d5dd2e40ebf012cbda8905b09fd773cf03f",
    ),
    (
        801,
        "f19ce473806d423c06b839c6dfc1caf17e768e7f2d8f42d224823f5d03574721",
    ),
    (
        810,
        "e1dc29df7dd03c08489c19dc9f77df73e800b3f5010d50a59731bc0bc9a6be6b",
    ),
    (
        900,
        "31b18548dfcb5e288c39f29540910aeb7ccf1f193acbabf773a4afc505681717",
    ),
    (
        1010,
        "5af4f38e6b675b25743acf7d705f31e19f871729e25bf15d0a6b6087eaabc0d0",
    ),
    (
        1028,
        "6ce00da631d143a7a9e62db9e940291349ab6d5fdcb04c7b27620fe1f1f1586e",
    ),
    (
        1199,
        "59eb4f2fc624acf47b2e582df4dc895840914eaf12fe57287fecd4b976de353c",
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

fn scripted_input(file: &str) -> retro_input::ScriptedInput {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools/ref-harness/testdata")
        .join(file);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
    retro_input::ScriptedInput::from_str(&text).expect("scripted input")
}

/// Runs `input_file` for `pins` and asserts every pinned framebuffer hash. `scene` selects the
/// starting scene (the Title START replay leaves it mid-run).
fn check_scripted_pins(game: &str, scene: &str, input_file: &str, pins: &[(u64, &str)]) {
    let scripted = scripted_input(input_file);
    let seed = scripted.seed().unwrap_or(DEFAULT_SEED);
    let mut engine = Engine::load(source(game), Some(scene), Some("1"), seed).expect("engine load");
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
            "{game}/{scene} {input_file} frame {target} must match the reference harness"
        );
    }
}

#[test]
#[ignore = "requires assets; pins reference-harness framebuffer hashes"]
fn zone01_right_framebuffer_matches_reference() {
    for (game, pins) in [("S1", S1_ZONE01_RIGHT), ("S2", S2_ZONE01_RIGHT)] {
        check_scripted_pins(game, "Zone01", "zone01_right.input", pins);
    }
}

#[test]
#[ignore = "requires assets; pins reference-harness framebuffer hashes"]
fn zone01_right400_framebuffer_matches_reference() {
    for (game, pins) in [("S1", S1_ZONE01_RIGHT400), ("S2", S2_ZONE01_RIGHT400)] {
        check_scripted_pins(game, "Zone01", "zone01_right400.input", pins);
    }
}

/// The scripted pause (`engine.state = 5`, `ENGINE_INITPAUSE`) makes upstream skip exactly one
/// stage frame before resetting the mode; the port used to ignore the write and process the frame
/// normally, shifting the shared ring animation by one frame.
#[test]
#[ignore = "requires assets; pins reference-harness framebuffer hashes"]
fn zone01_pause_framebuffer_matches_reference() {
    for (game, pins) in [("S1", S1_ZONE01_PAUSE), ("S2", S2_ZONE01_PAUSE)] {
        check_scripted_pins(game, "Zone01", "zone01_pause.input", pins);
    }
}

#[test]
#[ignore = "requires assets; pins reference-harness framebuffer hashes"]
fn title_start_framebuffer_matches_reference() {
    for (game, pins) in [("S1", S1_TITLE_START), ("S2", S2_TITLE_START)] {
        check_scripted_pins(game, "Title", "title_start.input", pins);
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

/// S1 `Continue` idle: the scripted countdown expires without input, `ContinueSetup` writes
/// `engine.state = 8` (`ENGINE_RESETGAME`) on standalone and the engine must reset to the first
/// GameConfig entry. No reference-harness window exists for this flow yet, so this is a
/// completion guard rather than a framebuffer pin.
#[test]
#[ignore = "requires assets; flow guard without a reference pin yet"]
fn s1_continue_idle_resets_to_the_first_game_config_entry() {
    let mut engine = Engine::load(source("S1"), Some("Continue"), Some("1"), DEFAULT_SEED)
        .expect("S1/Continue must load");
    assert_eq!(engine.stage_info(), ("Continue", "1"));

    let mut reset_frame = None;
    for frame in 0..2400u64 {
        engine.run_frame().expect("frame");
        if engine.stage_info().0 != "Continue" {
            reset_frame = Some(frame);
            break;
        }
    }
    let reset_frame = reset_frame.expect("the idle countdown must expire into the reset flow");
    assert_eq!(engine.state.stage.active_list, 0);
    assert_eq!(engine.state.stage.list_pos, 0);
    println!(
        "S1/Continue idle reset to {} act {} at frame {reset_frame}",
        engine.stage_info().0,
        engine.stage_info().1
    );
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
