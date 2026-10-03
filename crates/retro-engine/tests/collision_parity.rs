//! M7 collision fidelity: replay `zone01_right.input` and compare the player trajectory against
//! golden values pinned from the C++ reference harness (`tools/ref-harness`).
//!
//! The reference's record `f` is the scene-load tick, so the Rust run's first executed frame
//! (`state.frame == 1`) lines up with reference record 1: `samples[i]` is compared against
//! reference record `i + 1`. With that offset the S1 run matches the reference through record
//! 376 (title card, slopes, the loop and the first death) and the S2 run matches all 599
//! comparable records. The first S1 divergence is the `Player_State_Death` control-mode reset
//! (documented in the M7 report); it is not a `ProcessObjectMovement` difference.
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

/// Reference records that the S1 run matches exactly (records 1..=376).
const S1_REFERENCE_WINDOW: usize = 376;
/// Reference records that the S2 run matches exactly (records 1..=599).
const S2_REFERENCE_WINDOW: usize = 599;

/// BLAKE3 of the reference player state over records 1..=376 (S1).
const S1_WINDOW_HASH: &str = "448d5df4c5bc49dea3f47a41441b92467b17a6254bab997da0fb287315b824bd";
/// BLAKE3 of the reference player state over records 1..=599 (S2).
const S2_WINDOW_HASH: &str = "551451a91caf19a5576086ead8efdbbc4fde97d03e3700ba25987b49587150b5";
/// BLAKE3 of the full 600-frame S1 Rust trajectory (pinned from the verified run).
const S1_FULL_HASH: &str = "9af170b8d51a587e5d469a981b6897b2d176c0e5f49926a148edd7f4cd34436d";
/// BLAKE3 of the full 600-frame S2 Rust trajectory (pinned from the verified run).
const S2_FULL_HASH: &str = "1a1505bc188e3f01f5353f7e657a2fb075638031f6fc80a0af6bc82eee3d26d2";

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
    samples
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
            "frame,xpos,ypos,xvel,yvel,speed,state,angle,gravity,mode,plane,sensors,values\n",
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
