//! M7 collision fidelity: replay `zone01_right.input` and compare the player trajectory against
//! golden values pinned from the C++ reference harness (`tools/ref-harness`).
//!
//! The reference's record `f` is the scene-load tick, so the Rust run's first executed frame
//! (`state.frame == 1`) lines up with reference record 1: `samples[i]` is compared against
//! reference record `i + 1`. With that offset both runs match all 599 comparable records
//! (title card, slopes, the loop, the spring launch and the death window).
//!
//! Two M7 fidelity fixes made the death window match: `STAGEMODE_FROZEN` now runs
//! `ProcessFrozenObjects`/`HandleCameras`/`DrawStageGFX` (only `PRIORITY_ALWAYS` entities
//! update) instead of freezing the stage completely, and the VM's `ForEachAll` now stores the
//! matched entity slot on the foreach stack instead of the scan start.
//!
//! Run with `cargo test -p retro-engine --test collision_parity -- --ignored --nocapture`.
//! `RETRO_ASSETS` overrides the asset root; `COLLISION_PARITY_PRINT=1` prints a per-frame CSV
//! plus the computed hashes instead of asserting them (for diffing against the reference
//! `records.jsonl`).

#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::sync::Arc;

use retro_engine::Engine;
use retro_input::ScriptedInput;
use retro_io::DirSource;

/// Number of frames the reference `zone01_right.input` run covers.
const FRAMES: usize = 600;

/// Reference records that the S1 run matches exactly (records 1..=599).
const S1_REFERENCE_WINDOW: usize = 599;
/// Reference records that the S2 run matches exactly (records 1..=599).
const S2_REFERENCE_WINDOW: usize = 599;

/// BLAKE3 of the reference player state over records 1..=599 (S1).
const S1_WINDOW_HASH: &str = "cc33f18af82672bca5776ed977a7b6aef6045596d0d1311e086afff7163b837b";
/// BLAKE3 of the reference player state over records 1..=599 (S2).
const S2_WINDOW_HASH: &str = "0931a79860865b3c6b8b577f0a2f2f2596f44150abf43db69033defbfe112fec";
/// BLAKE3 of the full 600-frame S1 Rust trajectory (pinned from the verified run).
const S1_FULL_HASH: &str = "9deb4f26db7a9edceeb32d67c94ac11fde129164da09a1583ca171cb8ddee45c";
/// BLAKE3 of the full 600-frame S2 Rust trajectory (pinned from the verified run).
const S2_FULL_HASH: &str = "e791f166f8a022d41ac6a611c3859b530803616697a301de2e3722cb53816567";

/// One frame of player state, covering the fields the reference `records.jsonl` exposes for
/// collision debugging.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PlayerSample {
    xpos: i32,
    ypos: i32,
    xvel: i32,
    yvel: i32,
    speed: i32,
    state: i32,
    angle: i32,
    gravity: u8,
    collision_mode: u8,
    collision_plane: u8,
    control_mode: i8,
    control_lock: u8,
    floor_sensors: [u8; 5],
    values: [i32; 48],
}

impl PlayerSample {
    fn from_engine(engine: &Engine) -> Self {
        let slot = usize::try_from(engine.state.stage.player_list_pos).unwrap_or(0);
        let entity = engine.state.entities.get(slot).copied().unwrap_or_default();
        Self {
            xpos: entity.xpos,
            ypos: entity.ypos,
            xvel: entity.xvel,
            yvel: entity.yvel,
            speed: entity.speed,
            state: entity.state,
            angle: entity.angle,
            gravity: entity.gravity,
            collision_mode: entity.collision_mode,
            collision_plane: entity.collision_plane,
            control_mode: entity.control_mode,
            control_lock: entity.control_lock,
            floor_sensors: entity.floor_sensors,
            values: entity.values,
        }
    }

    fn write_csv(self, frame: usize, out: &mut String) {
        let fields: Vec<String> = [
            self.xpos,
            self.ypos,
            self.xvel,
            self.yvel,
            self.speed,
            self.state,
            self.angle,
            i32::from(self.gravity),
            i32::from(self.collision_mode),
            i32::from(self.collision_plane),
            i32::from(self.control_mode),
            i32::from(self.control_lock),
        ]
        .iter()
        .map(i32::to_string)
        .collect();
        let sensors: Vec<String> = self.floor_sensors.iter().map(u8::to_string).collect();
        let values: Vec<String> = self.values.iter().map(i32::to_string).collect();
        out.push_str(&format!(
            "{frame},{},{},{}\n",
            fields.join(","),
            sensors.join(" "),
            values.join(" ")
        ));
    }
}

fn asset_root() -> PathBuf {
    std::env::var("RETRO_ASSETS")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/home/ted/projects/assets"))
}

fn input_text() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools/ref-harness/testdata/zone01_right.input");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

/// Loads `Zone01` act 1, replays the scripted input and returns the per-frame player state.
fn replay(game: &str) -> Vec<PlayerSample> {
    replay_with_engine(game).1
}

/// Like [`replay`], but also returns the engine after the final frame.
fn replay_with_engine(game: &str) -> (Engine, Vec<PlayerSample>) {
    let source = Arc::new(DirSource::new(asset_root().join(game)).expect("asset folder"));
    let scripted = ScriptedInput::from_str(&input_text()).expect("scripted input");
    let seed = scripted.seed().unwrap_or(retro_engine::rng::DEFAULT_SEED);
    let mut engine = Engine::load(source, Some("Zone01"), Some("1"), seed).expect("engine load");
    engine.set_scripted_input(scripted);
    let mut samples = Vec::with_capacity(FRAMES);
    for _ in 0..FRAMES {
        engine.run_frame().expect("run must not fail");
        samples.push(PlayerSample::from_engine(&engine));
    }
    (engine, samples)
}

/// Compact hash of the collision-relevant player state, matching the pinned reference values.
fn trajectory_hash(samples: &[PlayerSample]) -> String {
    let mut hasher = blake3::Hasher::new();
    for sample in samples {
        for value in [
            sample.xpos,
            sample.ypos,
            sample.xvel,
            sample.yvel,
            sample.speed,
            sample.state,
            sample.angle,
        ] {
            hasher.update(&value.to_le_bytes());
        }
        for value in [
            sample.gravity,
            sample.collision_mode,
            sample.collision_plane,
            sample.control_mode as u8,
            sample.control_lock,
        ] {
            hasher.update(&[value]);
        }
    }
    hasher.finalize().to_hex().to_string()
}

fn check(game: &str, window: usize, window_golden: &str, full_golden: &str) {
    let samples = replay(game);
    if std::env::var("COLLISION_PARITY_PRINT").is_ok() {
        let mut out = String::from(
            "frame,xpos,ypos,xvel,yvel,speed,state,angle,gravity,mode,plane,controlMode,controlLock,sensors,values\n",
        );
        for (frame, sample) in samples.iter().enumerate() {
            sample.write_csv(frame, &mut out);
        }
        print!("{out}");
        eprintln!(
            "{game} window hash: {}",
            trajectory_hash(&samples[..window])
        );
        eprintln!("{game} full hash: {}", trajectory_hash(&samples));
        return;
    }
    assert_eq!(
        trajectory_hash(&samples[..window]),
        window_golden,
        "{game}: player trajectory must match the pinned reference window"
    );
    assert_eq!(
        trajectory_hash(&samples),
        full_golden,
        "{game}: full player trajectory must match the pinned verified run"
    );
}

#[test]
#[ignore = "requires assets"]
fn s1_zone01_right_player_trajectory_matches_the_reference() {
    check("S1", S1_REFERENCE_WINDOW, S1_WINDOW_HASH, S1_FULL_HASH);
}

#[test]
#[ignore = "requires assets"]
fn s2_zone01_right_player_trajectory_matches_the_reference() {
    check("S2", S2_REFERENCE_WINDOW, S2_WINDOW_HASH, S2_FULL_HASH);
}

/// Focused regression for the death window: once `Player_State_Death` freezes the stage, the
/// frozen update must still run `PRIORITY_ALWAYS` entities so the death script pins the control
/// mode once and then accumulates gravity. Before the fix the player's `yvel` stayed pinned at
/// `-0x70000` forever and the framebuffer diverged at record 377.
#[test]
#[ignore = "requires assets"]
fn s1_zone01_death_window_matches_reference() {
    let (_engine, samples) = replay_with_engine("S1");
    // `samples[i]` is reference record `i + 1`: the death trigger lands on record 376.
    let trigger = samples[375];
    assert_eq!(
        trigger.state, 28,
        "record 376 must enter Player_State_Death"
    );
    assert_eq!(trigger.yvel, -0x70000);
    assert_eq!(trigger.xvel, 0);
    assert_eq!(
        trigger.control_mode, 0,
        "control is still P1 on the trigger frame"
    );

    assert_eq!(
        samples[376].control_mode, -1,
        "Player_State_Death locks control on the first frozen frame"
    );
    for (offset, sample) in samples[376..=390].iter().enumerate() {
        assert_eq!(sample.state, 28, "death state must persist while frozen");
        assert_eq!(sample.xpos, 50_339_557, "death launch keeps xpos fixed");
        assert_eq!(sample.xvel, 0);
        assert_eq!(
            sample.yvel,
            -0x70000 + (offset as i32 + 1) * 0x3800,
            "gravity must accumulate once per frozen frame"
        );
    }
}
