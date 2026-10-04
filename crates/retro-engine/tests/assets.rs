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

/// Regression for the S2 `Zone08` (HPZ) Act 1 hang that started at frame ~160: `HPZSetup` is
/// type 40, its `ObjectDraw` appends `object.entityPos` to draw list 2 (`HPZSetup.txt:888`), and
/// the port walked that list by live length, so the draw event re-appended itself forever
/// (unbounded memory, no frame completion). `DrawObjectList` must snapshot the size like upstream
/// (`Drawing.cpp:997-1007`). The wall-clock bound keeps a regression from hanging the suite.
#[test]
#[ignore = "requires assets"]
fn s2_zone08_act1_600_frames_finish_under_wall_clock_bound() {
    use std::time::{Duration, Instant};

    let started = Instant::now();
    let (engine, outcome) = run("S2", "Zone08", DEFAULT_SEED, false);
    let elapsed = started.elapsed();

    assert_eq!(outcome.frames, 600);
    assert_eq!(
        engine.stage_info(),
        ("Zone08", "1"),
        "the run must stay in S2 Zone08 Act 1"
    );
    assert!(
        engine
            .state
            .draw_lists
            .iter()
            .all(|list| list.len() <= retro_scene::ENTITY_COUNT),
        "draw lists must stay bounded by the entity bank"
    );
    assert!(
        elapsed < Duration::from_secs(60),
        "S2/Zone08 Act 1 must finish 600 frames, took {elapsed:?}"
    );
    println!(
        "S2 Zone08 act 1: 600 frames in {elapsed:?}, hash {}",
        outcome.final_hash
    );
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
/// S1's death sets `camera[0].enabled = false` (`PlayerObject.txt:1582`), so frames 376-555 run
/// `SetPlayerLockedScreenPosition` (pins 400/500); 556 resumes the follow after respawn.
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
        400,
        "22d7cb5662beb35c229cee6f1bc6f02a28a2e0e572fe86ce29dd211c67ab957f",
    ),
    (
        450,
        "a8c13aa2185bfd190270f88a817589de00ced1c3270dc71a70af259c56778e24",
    ),
    (
        500,
        "4e3c60bc81c9ea24361c5b1759a9433eb10e9903edebf7438220ac5182cfa5c5",
    ),
    (
        556,
        "df783f7531b9128e26bbff557953acb02e66a23867e9d05e89be35b6d61b445b",
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

/// Pinned `fb.blake3` values for S2 `Zone03` (ARZ) Act 1 idle 600 frames (`zone01_idle.input`).
/// The camera stays `CAMERASTYLE_FOLLOW` for the whole window: ARZ's `Water.txt:464` only
/// writes `CAMERASTYLE_STATIC` under `USE_ORIGINS` + vs mode, and the standalone drowning path
/// (`Water.txt:460`, `camera[0].enabled = false`) needs 1800 frames underwater that idle input
/// never reaches. The window still pins the second camera's out-of-range `target == -1` early
/// return and the ARZ tile/palette state.
const S2_ARZ_IDLE: &[(u64, &str)] = &[
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
        "ce049c7ad23431200a39caaf0d4142c90bd588b67c04475229417dc8223e0805",
    ),
    (
        240,
        "b975203682371f3d6623a6a469b084621a88aa78bb0fe3f30f658c8897688787",
    ),
    (
        300,
        "f66e647a8b1aa324c7b33b2ffa3fc293e3e7e02ad1c3bdbd4de8eaacccc617fb",
    ),
    (
        400,
        "3a72059b08861f5f373bffb954976c989551eca5c4cfc90334f17719be8feed3",
    ),
    (
        500,
        "ed7dc2c15616ccc29cb1d11235e804f5c1ce2113e5e6cb01b0aeb6b9c74d4618",
    ),
    (
        599,
        "fa6bc0fe6891c2bab3eb78f30840de66cc1faaf291ac7cb122a92f989e60d74d",
    ),
];

/// Pinned `fb.blake3` values for `zone01_spindash.input` (S1 Zone01): crouch from line 220,
/// spindash charge presses on 270/273/276 and release on line 290. `PlayerObject.txt:3874`
/// then sets `camera[0].style = CAMERASTYLE_HLOCKED` with `scrollDelay = 15`, so frames
/// 290-304 run `SetPlayerHLockedScreenPosition` and frame 305 is back on `FOLLOW`.
///
/// The shipped `SGame.bin` has the `spindash` option off (`saveRAM[35] == 0`), so both the
/// reference harness run and this test feed the engine a scratch save with word 35 set; the
/// option also clears `speedCap`/`airSpeedCap`/`spikeBehavior`, which both sides share.
const S1_ZONE01_SPINDASH: &[(u64, &str)] = &[
    (
        0,
        "df783f7531b9128e26bbff557953acb02e66a23867e9d05e89be35b6d61b445b",
    ),
    (
        269,
        "aa1ffcc89adeef97415a7fa2bf785729f3a40d8d2b795c6e19ced905a73fa49e",
    ),
    (
        290,
        "daca1b272457fdd883990b06334245157b6cee82a76c0914992bf9389ae7af81",
    ),
    (
        291,
        "68e85c9228fa80f213d6a8c613f63a4942709c0ed18c5daa637c8e008a744bc0",
    ),
    (
        295,
        "7fff25c1764044c25316e4b48617cbaad84662aab05f23e509ea2526ebef8913",
    ),
    (
        300,
        "52f5c917d3e31e9c1d04fb101b9d5181579d2d64c3e4914e256e60b506cc2dea",
    ),
    (
        304,
        "3fe5e6a765b1366af27ffa5fb7d1e97d6f34a41f83506bb0bdf552ce40398a1c",
    ),
    (
        305,
        "db04b16066a7501c3b7482906bcc42dbbcb8e12ff217ee66ae1c99f7baf29f87",
    ),
    (
        320,
        "cc5d52de308940a2f8937210fce71acd5813156f1fe7038980ffae92b1404b1d",
    ),
    (
        359,
        "fcc7e3ae7687d46cc4c06c57c42e1f63f84cbca883ec0c3868fa2046c626c41c",
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

/// Pinned `fb.blake3` values from `tools/ref-harness/run.sh --game S1 --boot --frames 1600`
/// (idle input). `--boot` starts with no `stage=`/`scene=` argument, so the real `GameConfig`
/// list is used with `Title` at list position 0. The idle attract demo expires and the engine
/// `LoadStage`s `Zone01` (flat list position 6) on the `STAGEMODE_LOAD` tick at frame 1049;
/// frames 1048/1049 are blank load ticks and 1088 is the first drawn title-card frame.
const S1_BOOT_IDLE: &[(u64, &str)] = &[
    (
        0,
        "df783f7531b9128e26bbff557953acb02e66a23867e9d05e89be35b6d61b445b",
    ),
    (
        59,
        "abcfd1e06c5bd7703a8d0d2c5b6fba0c07e3344be1aeaae573fbfa761b6106f3",
    ),
    (
        600,
        "8adbfbf024411853d5f55d951802f99dc08699fda78636c33528f2c14973c2b5",
    ),
    (
        900,
        "156d3aee7f24a6a902436e8356f4a3503999f29dedb25807aeeee9d348e581d8",
    ),
    (
        1048,
        "df783f7531b9128e26bbff557953acb02e66a23867e9d05e89be35b6d61b445b",
    ),
    (
        1049,
        "df783f7531b9128e26bbff557953acb02e66a23867e9d05e89be35b6d61b445b",
    ),
    (
        1088,
        "b1ba7eb993931deeee16a397137fff1e66ae215ad7d73bf8d10df8bf99db7405",
    ),
    (
        1100,
        "d41a0ba386feccb4d5c86bc47cd1e5daedbf2b88ee18b1d235ff809afcfe2923",
    ),
    (
        1500,
        "d8692a82710f10577d3a4bfde55ff79e1605735b172ddc1958ad0b0e058cb52f",
    ),
    (
        1599,
        "f69be45a040048f841e49fb14d64227478cec862bc8ec41ace67873f048b1e5f",
    ),
];

/// Pinned `fb.blake3` values from `tools/ref-harness/run.sh --game S1 --scene Continue --act 1
/// --frames 1200` (idle input). The countdown expires with no input: `ContinueScreen` reaches
/// `CONTINUESETUP_FADETOMENU`, fades to black, and `ContinueSetup` writes `engine.state = 8`
/// (`ENGINE_RESETGAME`) on frame 806. The reset tick keeps the black framebuffer (805-807) and
/// the engine re-enters the first `GameConfig` entry (`Title`, list position 0) on the
/// `STAGEMODE_LOAD` tick at 807; the Title screen starts drawing at 818.
const S1_CONTINUE_IDLE: &[(u64, &str)] = &[
    (
        0,
        "df783f7531b9128e26bbff557953acb02e66a23867e9d05e89be35b6d61b445b",
    ),
    (
        320,
        "12fb9c27bbdf93e762f8ea9aad3573f4772ca414cc1304360c83f39b91fe23d1",
    ),
    (
        599,
        "185bf124b602857645f274754e6f7162886fcf743c101ccec3dda08c1e9833f8",
    ),
    (
        805,
        "df783f7531b9128e26bbff557953acb02e66a23867e9d05e89be35b6d61b445b",
    ),
    (
        806,
        "df783f7531b9128e26bbff557953acb02e66a23867e9d05e89be35b6d61b445b",
    ),
    (
        807,
        "df783f7531b9128e26bbff557953acb02e66a23867e9d05e89be35b6d61b445b",
    ),
    (
        818,
        "6a5cdea50a08d8995db4db69d5246516241c136e6d363c173860a7763173a93c",
    ),
    (
        850,
        "637666f70f29a0de6936769d668cfea383ede7870745cbb82ba845b6ae85d8bf",
    ),
    (
        900,
        "a29d1c259e1606bc5bbd5c5cf51940728ff7079f60d17d65dbfbaca5ce976029",
    ),
    (
        1199,
        "a56d468a842f8cda4fb6c2bcbdff51b10356a65e494127462caa3ddd8676ef0f",
    ),
];

/// Pinned `fb.blake3` values from `tools/ref-harness/run.sh --game S1 --scene Credits --act 1
/// --frames 1200` (idle input). `CreditsControl` walks the demo stages: `Zone01` (flat list
/// position 6) loads at 211, the `Credits` stage returns at 831 (`Presentation` position 2)
/// and `Zone02` Act 2 (flat list position 10) loads at 1043. The blank frames around each
/// load pin the `STAGEMODE_LOAD` ticks; 215/843 are the first drawn frames after a load.
const S1_CREDITS_IDLE: &[(u64, &str)] = &[
    (
        0,
        "df783f7531b9128e26bbff557953acb02e66a23867e9d05e89be35b6d61b445b",
    ),
    (
        210,
        "df783f7531b9128e26bbff557953acb02e66a23867e9d05e89be35b6d61b445b",
    ),
    (
        211,
        "df783f7531b9128e26bbff557953acb02e66a23867e9d05e89be35b6d61b445b",
    ),
    (
        215,
        "c169c5cd18806a2bf7b0d764d0b0c6cc61b4a28702e441d55869ef1a61e52a7c",
    ),
    (
        600,
        "2a1e533c46850df37333852eb4aa001ac18ddb9fc2a77948c77143fb8748f2b1",
    ),
    (
        830,
        "df783f7531b9128e26bbff557953acb02e66a23867e9d05e89be35b6d61b445b",
    ),
    (
        831,
        "df783f7531b9128e26bbff557953acb02e66a23867e9d05e89be35b6d61b445b",
    ),
    (
        843,
        "dbd64f4fc9c0a4f2bd12417aa8f506ee3c9b68da765fa9e8b6140306a5ec4f4e",
    ),
    (
        1043,
        "df783f7531b9128e26bbff557953acb02e66a23867e9d05e89be35b6d61b445b",
    ),
    (
        1199,
        "93df4c0be3a32bbb20821531ba6534b2a818bba73d69b30393ce0c1b40aed45c",
    ),
];

/// Pinned `fb.blake3` values from `tools/ref-harness/run.sh --game S1 --scene LSelect --act 1
/// --frames 600` (idle input). The level-select `MenuControl` object sets up both text menus,
/// right-aligns them (`alignment = 1`), draws 20+19 rows and highlights `selection1` with the
/// `+128` font rows. The pins cover the initial draw (0), the first drawn frame (6, when the
/// back-scan clears `selection2`), the settled menu (21) and the column scroll phase.
const S1_LSELECT_ACT1_IDLE: &[(u64, &str)] = &[
    (
        0,
        "df783f7531b9128e26bbff557953acb02e66a23867e9d05e89be35b6d61b445b",
    ),
    (
        1,
        "df783f7531b9128e26bbff557953acb02e66a23867e9d05e89be35b6d61b445b",
    ),
    (
        6,
        "26cbbb3bcf6d68ffd1809948ff34e295357a467bdfab3264e33a24fbd02fa4eb",
    ),
    (
        21,
        "129bd878c03d9c295f920416949a59d57576da5c4fea5eff5be8067ba6b02d42",
    ),
    (
        251,
        "2c6640017aa12aa0f5092acb6e25c7e9b7f25f1585bdc825b152bb742a5abc97",
    ),
    (
        256,
        "e46cb4d0567548b04c7a11d22322f1c042c1f9d5506e1862921be73037727505",
    ),
    (
        262,
        "816ab15bdc3606a2b3b6bae0038592f7d090598468aad942b93dd09f38d88705",
    ),
    (
        599,
        "129bd878c03d9c295f920416949a59d57576da5c4fea5eff5be8067ba6b02d42",
    ),
];
const S2_LSELECT_ACT1_IDLE: &[(u64, &str)] = &[
    (
        0,
        "df783f7531b9128e26bbff557953acb02e66a23867e9d05e89be35b6d61b445b",
    ),
    (
        1,
        "df783f7531b9128e26bbff557953acb02e66a23867e9d05e89be35b6d61b445b",
    ),
    (
        6,
        "f236e5f364ab5b984e8f561428c0ad621d0ee09249b1bc17b0a86c33e7dae598",
    ),
    (
        21,
        "5fcbd9dbe431d6033343857ba737623357e00cf344108aca3fea5e3f11c00b88",
    ),
    (
        251,
        "175bdab39b08bacae2dbbd695ca96773fc02833ccf3394f981fdd34f75e73e03",
    ),
    (
        256,
        "276bf8beae03c64512e48913e89e63aa8b60d2cbbc3cb5c57ef3f7779a03eb55",
    ),
    (
        262,
        "6261d2dff3d77207a6b16eaf82b1bece7bad83665a1cedc8870163251de1926f",
    ),
    (
        599,
        "5fcbd9dbe431d6033343857ba737623357e00cf344108aca3fea5e3f11c00b88",
    ),
];

/// Pinned `fb.blake3` values for `zone01_idle.input` on S2 `Special` Act 1 (idle, 600 frames).
/// The halfpipe is revealed by the frame-4 fade-in, so these pins cover the first visible
/// frame, the ring field and the frame-321 message boundary.
const S2_SPECIAL_ACT1_IDLE: &[(u64, &str)] = &[
    (
        0,
        "df783f7531b9128e26bbff557953acb02e66a23867e9d05e89be35b6d61b445b",
    ),
    (
        100,
        "7e181c89f9ebb295ac7e6ff5d5e1e4d90c1532630a59cdb5960b351d6ad30862",
    ),
    (
        300,
        "60da4fbf11085f5113985850a43a7ab9bca8dbdef091dd8001770a14cf8e34fb",
    ),
    (
        321,
        "0a4b066fe736da0271e06466c8f7e793be818f6583c3c7a61e0e9f127f9ebaad",
    ),
    (
        599,
        "a7bd926f52064adb5e473f75eaca5bd19e682560b1987e07cb496375f893264a",
    ),
];

/// Pinned `fb.blake3` values for S2 `Special` Act 2 idle, 600 frames.
const S2_SPECIAL_ACT2_IDLE: &[(u64, &str)] = &[
    (
        0,
        "df783f7531b9128e26bbff557953acb02e66a23867e9d05e89be35b6d61b445b",
    ),
    (
        100,
        "79ebda87081474e4ed416e9c92a97c1fb87567fc3d54d936460db7c5444e4874",
    ),
    (
        300,
        "cb5f6c8ff89c93813e04b3f9318df7306a1031ce01084605c4c668fa04101fce",
    ),
    (
        321,
        "9d44d15b8c3eeeb4573c4461dd47cc521e9dc72678e2fef179c7aa205ad9233f",
    ),
    (
        599,
        "5710835ba81d206d1921cb0109fdb37f2a482fea6b05afbe3cbcc4feb6b09516",
    ),
];

/// Pinned `fb.blake3` values for S2 `Special` Act 3 idle, 600 frames.
const S2_SPECIAL_ACT3_IDLE: &[(u64, &str)] = &[
    (
        0,
        "df783f7531b9128e26bbff557953acb02e66a23867e9d05e89be35b6d61b445b",
    ),
    (
        100,
        "bcf58e6be72bb2f1c3756e72327b88b5ce0d9dd7b8338b666caf323f1e0b047b",
    ),
    (
        300,
        "39131bc5c1324e8a87c3fc9c2638dc3db4f6ab9c0cd9d70e571c1905167f4ce7",
    ),
    (
        321,
        "f68cc77baaf901eec55f77c68c43f9a384e43cb1e7b86fec966621f7584beda4",
    ),
    (
        599,
        "0575a459473f295baa789d7b4d71fa626bebe8dddb871329880155f535c18a9f",
    ),
];

/// Pinned `fb.blake3` values for S2 `Special` Act 4 idle, 600 frames.
const S2_SPECIAL_ACT4_IDLE: &[(u64, &str)] = &[
    (
        0,
        "df783f7531b9128e26bbff557953acb02e66a23867e9d05e89be35b6d61b445b",
    ),
    (
        100,
        "efbd65db076a51e4846517966f862f963daa4acf47efe2f5346addb968264241",
    ),
    (
        300,
        "58959e571885d2c524a50c5d21a1bc87c797419be5d1fc1e0e2acfd000b98c52",
    ),
    (
        321,
        "117595959953b7d4808dfed2604e0ecb4103e40c969556f4bf73d42c041adc2f",
    ),
    (
        599,
        "7a2ae927cc97611f0b3537c37664ea112dded11e62a5f651a9a5840a47f68cda",
    ),
];

/// Pinned `fb.blake3` values for S2 `Special` Act 5 idle, 600 frames.
const S2_SPECIAL_ACT5_IDLE: &[(u64, &str)] = &[
    (
        0,
        "df783f7531b9128e26bbff557953acb02e66a23867e9d05e89be35b6d61b445b",
    ),
    (
        100,
        "11882f2674794260c2e3c4f3faa8717527af84c1a1cdf40c1c67b0fd84ad8364",
    ),
    (
        300,
        "c3a023141b8d0ef609c62a0ed0c7300c0718f27131a8a8c94211e2ac9b314a30",
    ),
    (
        321,
        "a464de2eb0794f105b09d0202e856e26a88b140eb9bf1a093b7ff684b8c60925",
    ),
    (
        599,
        "e83c33c55c7f33521ac8edc4597f0e33252d86b0fe80eaf3788bf04899482c0d",
    ),
];

/// Pinned `fb.blake3` values for S2 `Special` Act 6 idle, 600 frames.
const S2_SPECIAL_ACT6_IDLE: &[(u64, &str)] = &[
    (
        0,
        "df783f7531b9128e26bbff557953acb02e66a23867e9d05e89be35b6d61b445b",
    ),
    (
        100,
        "49bd9fcf28e8ab1059aff3d3702c6f7a885409a26437fb50f846dc2919edde1b",
    ),
    (
        300,
        "a56276c49a152e9933e57afe59b5bd5f0667de53861b155402b1d6d1ebb7ec93",
    ),
    (
        321,
        "6ea6f9becbcac21d8c5d40c279ac61dbc8a300fb07c3317d5802093bf506a300",
    ),
    (
        599,
        "1531c0d705a931a20c1977030a697fbb239890420e7bcc1e6915ac1df40cb2eb",
    ),
];

/// Pinned `fb.blake3` values for S2 `Special` Act 7 idle, 600 frames.
const S2_SPECIAL_ACT7_IDLE: &[(u64, &str)] = &[
    (
        0,
        "df783f7531b9128e26bbff557953acb02e66a23867e9d05e89be35b6d61b445b",
    ),
    (
        100,
        "bb21ca69089068a5438a0f89856eb036c224baed99b1e239b5a3c02a88f4a901",
    ),
    (
        300,
        "ab373f4e6327d9c96dfa51954f0d22a840bd69361d8278a35ebbf9b47b647f51",
    ),
    (
        321,
        "257021fb5668a9f2f9c9cf2070e6e26c1a058471f47bdec4882accbf2e7006fd",
    ),
    (
        599,
        "85e9da320e8f06703741ffc5b8521c22f152a41a792deae7eada118ae3d58606",
    ),
];

/// Pinned `fb.blake3` values for S2 `Special` Act 8 idle, 600 frames.
const S2_SPECIAL_ACT8_IDLE: &[(u64, &str)] = &[
    (
        0,
        "df783f7531b9128e26bbff557953acb02e66a23867e9d05e89be35b6d61b445b",
    ),
    (
        100,
        "102dab0392e6ff8cdb18596bf3edcf1ed0c836fa22481f760a20b70dec314f67",
    ),
    (
        300,
        "ef4ea459289d2bae68bb5bcdc107ceb02e9e902bb7dcd45b3ffb53d5fbe899bb",
    ),
    (
        321,
        "a6a9ff45a970d45ed33d0dbf5b894a08bfde31d553d1f834f72c7296ee30d039",
    ),
    (
        599,
        "f37b72477353f1e0c67cdf1d85d142a09af4550004a27cff26d51afba79bb462",
    ),
];

/// Pinned `fb.blake3` values for `zone01_idle.input` on S1 `Special` Act 1 (idle, 600 frames).
/// `SpecialSetup` runs `BoxCollisionTest(C_SOLID2, ...)` against the player every frame, so this
/// window is a direct `BoxCollision2` regression; the final frame is the reference harness's
/// `cfede829...92c976`.
const S1_SPECIAL_ACT1_IDLE: &[(u64, &str)] = &[
    (
        0,
        "df783f7531b9128e26bbff557953acb02e66a23867e9d05e89be35b6d61b445b",
    ),
    (
        100,
        "f535251667df463dd5bceb1ccbf23fd1ba3da217754059ba5480b06d09ed24b4",
    ),
    (
        300,
        "07a7e43097b4076a4b523ba9092c5d2e380e072405afe3d3b1e0c8c0191222c4",
    ),
    (
        450,
        "7d82c6c476718527980a3d04b5bab13c1f1e4a8d6d0717fec96b22f20ddfb822",
    ),
    (
        599,
        "cfede829d9a0e12f6cb2fff5c35962e83949c47ac458ce0865a3743fc892c976",
    ),
];

/// Pinned `fb.blake3` values for `zone01_idle.input` on S1 `Mission_Zone02` Act 3 (idle, 600
/// frames). The act places 13 `GlassPillar` entities; each runs
/// `BoxCollisionTest(C_SOLID2, ...)` every frame, so the window pins `BoxCollision2` against
/// the placed pillars.
const S1_MISSION_ZONE02_ACT3_IDLE: &[(u64, &str)] = &[
    (
        0,
        "df783f7531b9128e26bbff557953acb02e66a23867e9d05e89be35b6d61b445b",
    ),
    (
        100,
        "47786155f40903a34f7199bcabaf3805bc0dfe42b14a8afcfd2b9cceef01be5f",
    ),
    (
        300,
        "24b3cec66796c305d07866c57b6d6ed4e0ffe8a60cf9b57ce440ea9fec38a2f5",
    ),
    (
        450,
        "fe44c1b1c0d3f34b56293470da263b7c71a6ee246dd17d4914c8e7e4dc359e28",
    ),
    (
        599,
        "6287231b291fdb695137942de1ed03a25adce2c2448bf7528cf28913d372a657",
    ),
];

/// Pinned `fb.blake3` values for `zone01_idle.input` on S2 `Zone07` (OOZ) Act 1 (idle, 600
/// frames). The act places 11 `GasPlatform` entities, whose update calls
/// `BoxCollisionTest(C_SOLID2, ...)` while the platform is not launching, covering the
/// `C_SOLID2` path in the shipped S2 scripts.
const S2_OOZ_GASPLATFORM_ACT1_IDLE: &[(u64, &str)] = &[
    (
        0,
        "df783f7531b9128e26bbff557953acb02e66a23867e9d05e89be35b6d61b445b",
    ),
    (
        100,
        "f176ef1308ceafd5b7a7b50f72a3f10031f66b6dde017d54c588ec8a4fb47786",
    ),
    (
        300,
        "fa3f0f31fb33ad2a04ea1235094d0e30cd01b2a5cc7e11b0c67a21287b580617",
    ),
    (
        450,
        "9268950bdb630a37dc5c96285e3cd55da49c9ad8b99f860636a25c56dbb698c8",
    ),
    (
        599,
        "b9ff0f9c7d9e003b5e48294fef31cdcfc769776433b47b95e94ec9bb8fa18427",
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

#[test]
#[ignore = "requires assets; pins reference-harness framebuffer hashes"]
fn s2_arz_idle_framebuffer_matches_reference() {
    let scripted = scripted_input("zone01_idle.input");
    let seed = scripted.seed().unwrap_or(DEFAULT_SEED);
    let mut engine =
        Engine::load(source("S2"), Some("Zone03"), Some("1"), seed).expect("engine load");
    engine.set_scripted_input(scripted);
    let mut frame = 0u64;
    for &(target, expected) in S2_ARZ_IDLE {
        while frame < target {
            engine.run_frame().expect("frame");
            frame += 1;
        }
        assert_eq!(
            framebuffer_hash(&engine),
            expected,
            "S2/Zone03 idle frame {target} must match the reference harness"
        );
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

/// Runs `input_file` for `pins` and asserts every pinned framebuffer hash. `scene`/`act` select
/// the starting scene (the Title START replay leaves it mid-run).
fn check_scripted_pins(game: &str, scene: &str, act: &str, input_file: &str, pins: &[(u64, &str)]) {
    let scripted = scripted_input(input_file);
    let seed = scripted.seed().unwrap_or(DEFAULT_SEED);
    let mut engine = Engine::load(source(game), Some(scene), Some(act), seed).expect("engine load");
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
        check_scripted_pins(game, "Zone01", "1", "zone01_right.input", pins);
    }
}

#[test]
#[ignore = "requires assets; pins reference-harness framebuffer hashes"]
fn zone01_right400_framebuffer_matches_reference() {
    for (game, pins) in [("S1", S1_ZONE01_RIGHT400), ("S2", S2_ZONE01_RIGHT400)] {
        check_scripted_pins(game, "Zone01", "1", "zone01_right400.input", pins);
    }
}

/// Save storage with the shipped save RAM but `saveRAM[35]` (`options.spindash`) forced on.
///
/// `PlayerObject`'s startup copies word 35 into `options.spindash`; the shipped `SGame.bin`
/// leaves it zero, which makes `actionSpindash` jump instead. The reference window was captured
/// with an identical scratch save, so both engines see the option on. Also exercised:
/// `ReadSaveRAM` must load the provided storage before the startup op reads the word.
fn spindash_storage() -> Box<dyn retro_platform::Storage> {
    use retro_platform::Storage;
    let path = asset_root().join("S1").join("SGame.bin");
    let mut bytes =
        std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    assert!(bytes.len() >= 36 * 4, "save RAM must hold word 35");
    bytes[35 * 4..36 * 4].copy_from_slice(&1i32.to_le_bytes());
    let mut storage = retro_platform::headless::MemoryStorage::new();
    storage.write("SGame.bin", &bytes).expect("seed save RAM");
    Box::new(storage)
}

#[test]
#[ignore = "requires assets; pins reference-harness framebuffer hashes"]
fn zone01_spindash_framebuffer_matches_reference() {
    let scripted = scripted_input("zone01_spindash.input");
    let seed = scripted.seed().unwrap_or(DEFAULT_SEED);
    let mut engine = Engine::load_with_options(
        source("S1"),
        Some("Zone01"),
        Some("1"),
        seed,
        spindash_storage(),
        retro_engine::LoadOptions::default(),
    )
    .expect("engine load");
    engine.set_scripted_input(scripted);
    let mut frame = 0u64;
    for &(target, expected) in S1_ZONE01_SPINDASH {
        while frame < target {
            engine.run_frame().expect("frame");
            frame += 1;
        }
        assert_eq!(
            framebuffer_hash(&engine),
            expected,
            "S1/Zone01 spindash frame {target} must match the reference harness"
        );
    }
}

/// The scripted pause (`engine.state = 5`, `ENGINE_INITPAUSE`) makes upstream skip exactly one
/// stage frame before resetting the mode; the port used to ignore the write and process the frame
/// normally, shifting the shared ring animation by one frame.
#[test]
#[ignore = "requires assets; pins reference-harness framebuffer hashes"]
fn zone01_pause_framebuffer_matches_reference() {
    for (game, pins) in [("S1", S1_ZONE01_PAUSE), ("S2", S2_ZONE01_PAUSE)] {
        check_scripted_pins(game, "Zone01", "1", "zone01_pause.input", pins);
    }
}

#[test]
#[ignore = "requires assets; pins reference-harness framebuffer hashes"]
fn title_start_framebuffer_matches_reference() {
    for (game, pins) in [("S1", S1_TITLE_START), ("S2", S2_TITLE_START)] {
        check_scripted_pins(game, "Title", "1", "title_start.input", pins);
    }
}

/// The full boot flow without `--scene`: the real `GameConfig` list with `Title` at position 0.
/// The idle attract demo must `LoadStage` into `Zone01` (flat reference list position 6) at
/// frame 1049 and keep drawing the demo through 1599, matching the `--boot` reference window
/// frame for frame.
#[test]
#[ignore = "requires assets; pins reference-harness framebuffer hashes"]
fn s1_boot_idle_framebuffer_matches_reference() {
    let mut engine = Engine::load(source("S1"), None, None, DEFAULT_SEED).expect("engine load");
    assert_eq!(engine.stage_info(), ("Title", "1"), "boot starts on Title");
    let mut frame = 0u64;
    for &(target, expected) in S1_BOOT_IDLE {
        while frame < target {
            engine.run_frame().expect("frame");
            frame += 1;
        }
        assert_eq!(
            framebuffer_hash(&engine),
            expected,
            "S1 boot idle frame {target} must match the reference harness"
        );
    }
    assert_eq!(
        engine.stage_info(),
        ("Zone01", "1"),
        "the attract demo must end up in Zone01"
    );
    assert_eq!(
        engine.state.stage.active_list, 1,
        "the attract demo selects the regular category"
    );
}

/// The idle `Continue` countdown expiring into `ENGINE_RESETGAME`: the framebuffer pins bracket
/// the reset tick (806) and the Title reload, and the completion assertions prove the reset
/// lands on the first `GameConfig` entry rather than replaying `Continue`.
#[test]
#[ignore = "requires assets; pins reference-harness framebuffer hashes"]
fn s1_continue_idle_framebuffer_matches_reference() {
    let scripted = scripted_input("zone01_idle.input");
    let seed = scripted.seed().unwrap_or(DEFAULT_SEED);
    let mut engine =
        Engine::load(source("S1"), Some("Continue"), Some("1"), seed).expect("engine load");
    engine.set_scripted_input(scripted);
    let mut frame = 0u64;
    for &(target, expected) in S1_CONTINUE_IDLE {
        while frame < target {
            engine.run_frame().expect("frame");
            frame += 1;
        }
        assert_eq!(
            framebuffer_hash(&engine),
            expected,
            "S1/Continue idle frame {target} must match the reference harness"
        );
    }
    assert_eq!(
        engine.stage_info(),
        ("Title", "1"),
        "the idle countdown must reset to the first GameConfig entry"
    );
    assert_eq!(
        (engine.state.stage.active_list, engine.state.stage.list_pos),
        (0, 0),
        "ENGINE_RESETGAME resets the active list and position"
    );
}

/// The credits demo walk: `CreditsControl` loads `Zone01` (211), reloads `Credits` (831) and
/// starts `Zone02` Act 2 (1043), matching the `--scene Credits` reference window. The final
/// assertion proves the credits script's `LoadStage` calls reached the regular category.
#[test]
#[ignore = "requires assets; pins reference-harness framebuffer hashes"]
fn s1_credits_idle_framebuffer_matches_reference() {
    let scripted = scripted_input("zone01_idle.input");
    let seed = scripted.seed().unwrap_or(DEFAULT_SEED);
    let mut engine =
        Engine::load(source("S1"), Some("Credits"), Some("1"), seed).expect("engine load");
    engine.set_scripted_input(scripted);
    let mut frame = 0u64;
    for &(target, expected) in S1_CREDITS_IDLE {
        while frame < target {
            engine.run_frame().expect("frame");
            frame += 1;
        }
        assert_eq!(
            framebuffer_hash(&engine),
            expected,
            "S1/Credits idle frame {target} must match the reference harness"
        );
    }
    assert_eq!(
        engine.stage_info(),
        ("Zone02", "2"),
        "the credits demo must walk on to Zone02 Act 2"
    );
    assert_eq!(
        engine.state.stage.active_list, 1,
        "the credits script selects the regular category"
    );
}

/// Ports `SetupTextMenu`/`AddTextMenuEntry`/`DrawTextMenu`; the level-select cursor, row
/// highlights, right alignment and `DrawTextMenu`'s selection handling all feed the framebuffer.
/// Also proves the op arms report through `record_op` rather than the stub histogram.
#[test]
#[ignore = "requires assets; pins reference-harness framebuffer hashes"]
fn lselect_act1_menus_framebuffer_matches_reference() {
    for (game, pins) in [("S1", S1_LSELECT_ACT1_IDLE), ("S2", S2_LSELECT_ACT1_IDLE)] {
        let mut engine = Engine::load(source(game), Some("LSelect"), Some("1"), DEFAULT_SEED)
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
                "{game}/LSelect Act1 idle frame {target} must match the reference harness"
            );
        }
        for op in ["SetupMenu", "AddMenuEntry", "DrawMenu"] {
            assert!(
                engine.op_histogram().contains_key(op),
                "{game}/LSelect must record {op}"
            );
            assert!(
                !engine.stub_histogram().contains_key(op),
                "{game}/LSelect must not stub {op}"
            );
        }
    }
}

/// `TextMessage`'s startup fills `MENU_1` through `SetupMenu`/`AddMenuEntry`; before M8 WP2 the
/// menu was empty, so `TextMessage_SetupTextChars` divided by a zero
/// `GetTextInfo(MENU_1, TEXTINFO_TEXTSIZE, ...)` at frame 321. 3D rendering is still stubbed
/// (M9 owns the framebuffer pin), so this is a completion regression.
#[test]
#[ignore = "requires assets; flow guard until the M9 3D render pass is ported"]
fn s2_special_act1_runs_past_the_text_message_divide_by_zero() {
    let mut engine = Engine::load(source("S2"), Some("Special"), Some("1"), DEFAULT_SEED)
        .expect("S2/Special must load");
    let outcome = engine
        .run_frames(600, false)
        .expect("S2/Special Act1 must run 600 frames without HostError");
    assert_eq!(outcome.frames, 600);
    assert_eq!(
        engine.op_histogram().get("SetupMenu"),
        Some(&1),
        "TextMessage's startup must set up MENU_1"
    );
    assert!(
        engine
            .op_histogram()
            .get("AddMenuEntry")
            .copied()
            .unwrap_or(0)
            >= 15,
        "all 15 message rows must be added"
    );
    assert_eq!(
        engine.state.text_menus[0].row_count, 15,
        "MENU_1 holds the 15 special-stage messages"
    );
}

/// M9 wiring: `Special`'s `Halfpipe` startup builds the persistent tube mesh
/// (`vertexCount = 1400`, `faceCount = 740`) and sets the projection/fog scalars. After 600
/// idle frames every matrix/transform op and `Draw3DScene` must be ported with no 3D stubs left.
#[test]
#[ignore = "requires assets; M9 WP1 scene3D wiring"]
fn s2_special_act1_scene3d_wiring() {
    let mut engine = Engine::load(source("S2"), Some("Special"), Some("1"), DEFAULT_SEED)
        .expect("S2/Special must load");
    let scene3d = &engine.state.scene3d;
    assert_eq!(scene3d.vertex_count, 1400, "Halfpipe startup vertex cursor");
    assert_eq!(scene3d.face_count, 740, "Halfpipe startup face cursor");
    assert_eq!(scene3d.projection_x, 216);
    assert_eq!(scene3d.projection_y, 216);
    assert_eq!(scene3d.fog_strength, 0x50);

    engine
        .run_frames(600, false)
        .expect("S2/Special must run 600 frames");

    let matrix_ops = [
        "SetIdentityMatrix",
        "MatrixMultiply",
        "MatrixTranslateXYZ",
        "MatrixScaleXYZ",
        "MatrixRotateX",
        "MatrixRotateY",
        "MatrixRotateZ",
        "MatrixRotateXYZ",
        "MatrixInverse",
        "TransformVertices",
    ];
    for op in matrix_ops {
        assert!(
            !engine.stub_histogram().contains_key(op),
            "{op} must be ported by M9a"
        );
    }
    // The shipped Special acts exercise these six.
    for op in [
        "Draw3DScene",
        "MatrixInverse",
        "MatrixMultiply",
        "MatrixRotateXYZ",
        "MatrixTranslateXYZ",
        "TransformVertices",
    ] {
        assert!(
            engine.op_histogram().contains_key(op),
            "{op} must appear in the op histogram"
        );
    }
    // M9b ported `Draw3DScene`: no 3D op may remain in the stub histogram.
    let stubbed_3d: Vec<&str> = engine
        .stub_histogram()
        .keys()
        .filter(|name| matrix_ops.contains(&name.as_str()) || name.as_str() == "Draw3DScene")
        .map(String::as_str)
        .collect();
    assert!(stubbed_3d.is_empty(), "unexpected 3D stubs: {stubbed_3d:?}");
}

/// M9 WP2 smoke: all eight `Special` acts run 600 idle frames with the rasterizer live, no 3D
/// op is stubbed, the halfpipe is visible at frame 599 and Act 1 is bit-identical across runs.
/// Exact reference parity is pinned by [`s2_special_acts_framebuffers_match_reference`].
#[test]
#[ignore = "requires assets; release-only; M9 rasterizer smoke"]
fn s2_special_acts_3d_smoke() {
    let ops_3d = [
        "Draw3DScene",
        "SetIdentityMatrix",
        "MatrixMultiply",
        "MatrixTranslateXYZ",
        "MatrixScaleXYZ",
        "MatrixRotateX",
        "MatrixRotateY",
        "MatrixRotateZ",
        "MatrixRotateXYZ",
        "MatrixInverse",
        "TransformVertices",
    ];
    for act in 1..=8 {
        let act = act.to_string();
        let mut engine = Engine::load(
            source("S2"),
            Some("Special"),
            Some(act.as_str()),
            DEFAULT_SEED,
        )
        .expect("S2/Special must load");
        engine
            .run_frames(600, false)
            .expect("600-frame run must succeed");
        for op in ops_3d {
            assert!(
                !engine.stub_histogram().contains_key(op),
                "Act {act} still stubs {op}"
            );
        }
        assert!(
            engine
                .op_histogram()
                .get("Draw3DScene")
                .copied()
                .unwrap_or(0)
                >= 600,
            "Act {act} must draw 3D every frame"
        );
        if act == "1" {
            let ratio = engine.state.render.framebuffer.non_black_ratio();
            eprintln!("S2 Special Act 1 frame 599 non-black ratio: {ratio}");
            assert!(
                ratio > 0.05,
                "frame 599 should show the halfpipe, non-black ratio {ratio}"
            );
        }
    }

    // Act 1 determinism: `state_hash` covers the framebuffer and the full scene3D buffers.
    let mut first = Engine::load(source("S2"), Some("Special"), Some("1"), DEFAULT_SEED)
        .expect("S2/Special must load");
    first.run_frames(600, false).expect("600-frame run");
    let expected = first.state_hash();
    let mut second = Engine::load(source("S2"), Some("Special"), Some("1"), DEFAULT_SEED)
        .expect("S2/Special must load");
    second.run_frames(600, false).expect("600-frame run");
    assert_eq!(second.state_hash(), expected, "Act 1 must be deterministic");
}

/// S2 `Special` Acts 1-8 idle: the full 3D pipeline (matrices, sort, `TEXTURED_C`,
/// `TEXTURED_C_BLEND`, `FADED` and `3DSPRITE`) must match the reference harness at frame 0
/// (black fade-in), 100, 300, 321 and 599.
#[test]
#[ignore = "requires assets; pins reference-harness framebuffer hashes"]
fn s2_special_acts_framebuffers_match_reference() {
    for (act, pins) in [
        ("1", S2_SPECIAL_ACT1_IDLE),
        ("2", S2_SPECIAL_ACT2_IDLE),
        ("3", S2_SPECIAL_ACT3_IDLE),
        ("4", S2_SPECIAL_ACT4_IDLE),
        ("5", S2_SPECIAL_ACT5_IDLE),
        ("6", S2_SPECIAL_ACT6_IDLE),
        ("7", S2_SPECIAL_ACT7_IDLE),
        ("8", S2_SPECIAL_ACT8_IDLE),
    ] {
        check_scripted_pins("S2", "Special", act, "zone01_idle.input", pins);
    }
}

/// S1 `Special` Act 1 idle: `SpecialSetup`'s `C_SOLID2` call is the scene's main player
/// interaction, so this pins `BoxCollision2`'s floor/ceiling/wall resolution in real data.
#[test]
#[ignore = "requires assets; pins reference-harness framebuffer hashes"]
fn s1_special_act1_idle_framebuffer_matches_reference() {
    check_scripted_pins(
        "S1",
        "Special",
        "1",
        "zone01_idle.input",
        S1_SPECIAL_ACT1_IDLE,
    );
}

/// S1 `Mission_Zone02` Act 3 idle: the 13 `GlassPillar` instances each call `C_SOLID2` per
/// frame, so this covers `BoxCollision2` against a moving solid pillar.
#[test]
#[ignore = "requires assets; pins reference-harness framebuffer hashes"]
fn s1_mission_zone02_act3_idle_framebuffer_matches_reference() {
    check_scripted_pins(
        "S1",
        "Mission_Zone02",
        "3",
        "zone01_idle.input",
        S1_MISSION_ZONE02_ACT3_IDLE,
    );
}

/// S2 `Zone07` (OOZ) Act 1 idle: the 11 `GasPlatform` instances call `C_SOLID2` in their update
/// loop, covering the shipped S2 user of `BoxCollision2`.
#[test]
#[ignore = "requires assets; pins reference-harness framebuffer hashes"]
fn s2_ooz_gasplatform_act1_idle_framebuffer_matches_reference() {
    check_scripted_pins(
        "S2",
        "Zone07",
        "1",
        "zone01_idle.input",
        S2_OOZ_GASPLATFORM_ACT1_IDLE,
    );
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
/// GameConfig entry. `s1_continue_idle_framebuffer_matches_reference` pins the same window
/// against the reference harness; this longer guard keeps the reset independent of the pinned
/// frame set and reports the exact reset frame.
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
