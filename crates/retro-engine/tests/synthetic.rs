//! Synthetic end-to-end test: a hand-built asset set in memory runs through the real engine.
//!
//! This exercises loading, script compilation/linking, scene instantiation, the startup pass and
//! 60 update frames without any files on disk, and pins determinism of the state hash.

use std::sync::Arc;

use retro_engine::Engine;
use retro_engine::rng::DEFAULT_SEED;
use retro_format_v4::gameconfig::PALETTE_COUNT;
use retro_format_v4::scene::{
    ACTIVE_LAYER_COUNT, ENTITY_ATTRIB_DIRECTION, ENTITY_ATTRIB_STATE, ENTITY_ATTRIB_VALUES,
};
use retro_format_v4::stageconfig::STAGE_PALETTE_COUNT;
use retro_io::MemorySource;

const OBJECT_SOURCE: &str = "\
event ObjectStartup\n\
    object.value0 = 0\n\
end event\n\
event ObjectUpdate\n\
    object.value0 += 1\n\
    object.value1 = object.value0\n\
    Rand(object.value2, 100)\n\
    PlaySfx(0, 0)\n\
end event\n\
";

fn push_string(bytes: &mut Vec<u8>, value: &str) {
    bytes.push(value.len() as u8);
    bytes.extend_from_slice(value.as_bytes());
}

fn game_config_bytes() -> Vec<u8> {
    let mut bytes = Vec::new();
    push_string(&mut bytes, "Synthetic");
    push_string(&mut bytes, "synthetic test data");
    for _ in 0..PALETTE_COUNT {
        bytes.extend_from_slice(&[0, 0, 0]);
    }
    bytes.push(1); // object count
    push_string(&mut bytes, "Test Object");
    push_string(&mut bytes, "Test/TestObject.txt");
    bytes.push(0); // global variables
    bytes.push(0); // sound effects
    bytes.push(0); // players
    for category in 0..4 {
        let scenes = u8::from(category == 0);
        bytes.push(scenes);
        if category == 0 {
            push_string(&mut bytes, "Zone01");
            push_string(&mut bytes, "1");
            push_string(&mut bytes, "TEST ZONE");
            bytes.push(1);
        }
    }
    bytes
}

fn stage_config_bytes() -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.push(1); // load_global_objects
    for _ in 0..STAGE_PALETTE_COUNT {
        bytes.extend_from_slice(&[0, 0, 0]);
    }
    bytes.push(0); // sfx
    bytes.push(0); // objects
    bytes
}

fn scene_bytes() -> Vec<u8> {
    let mut bytes = Vec::new();
    push_string(&mut bytes, "TEST");
    bytes.extend_from_slice(&[9; ACTIVE_LAYER_COUNT]);
    bytes.push(3); // midpoint
    bytes.push(1); // width
    bytes.push(0);
    bytes.push(1); // height
    bytes.push(0);
    bytes.extend_from_slice(&0u16.to_le_bytes()); // one chunk
    bytes.extend_from_slice(&1u16.to_le_bytes()); // one entity
    let attributes = ENTITY_ATTRIB_STATE | ENTITY_ATTRIB_DIRECTION | ENTITY_ATTRIB_VALUES[0];
    bytes.extend_from_slice(&attributes.to_le_bytes());
    bytes.push(1); // type: the global object
    bytes.push(0); // property value
    bytes.extend_from_slice(&(64i32 << 16).to_le_bytes());
    bytes.extend_from_slice(&(64i32 << 16).to_le_bytes());
    bytes.extend_from_slice(&0i32.to_le_bytes()); // state
    bytes.push(0); // direction
    bytes.extend_from_slice(&7i32.to_le_bytes()); // value0
    bytes
}

fn source() -> Arc<dyn retro_io::DataSource> {
    let mut source = MemorySource::new();
    source.insert("Settings.ini", "[Game]\ngameType=1\ntxtScripts=n\n");
    source.insert("Data/Game/GameConfig.bin", game_config_bytes());
    source.insert("Data/Stages/Zone01/StageConfig.bin", stage_config_bytes());
    source.insert("Data/Stages/Zone01/Act1.bin", scene_bytes());
    source.insert("Data/Scripts/Test/TestObject.txt", OBJECT_SOURCE);
    Arc::new(source)
}

#[test]
fn synthetic_scene_runs_sixty_frames_deterministically() {
    let mut first = Engine::load(source(), None, 1, DEFAULT_SEED).unwrap();
    let outcome = first.run_frames(60, true).unwrap();
    assert_eq!(outcome.frames, 60);
    assert_eq!(outcome.frame_hashes.len(), 61, "frame 0 plus 60 frames");
    assert!(
        outcome
            .frame_hashes
            .iter()
            .all(|(_, hash)| hash.len() == 64)
    );

    // Startup events run against the temp slot, so the scene entity keeps its file value of 7
    // and each of the 60 updates increments it once.
    let entity = first
        .state
        .entities
        .get(retro_scene::SCENE_ENTITY_START)
        .expect("scene entity placed at slot 32");
    assert_eq!(entity.type_id, 1);
    assert_eq!(entity.values[0], 7 + 60, "one update per frame");
    assert_eq!(entity.values[1], 7 + 60, "value1 mirrors value0");
    assert_eq!(entity.state, 0);
    assert_eq!(entity.direction, 0);
    assert!((0..100).contains(&entity.values[2]), "Rand is bounded");

    // The stub histogram sees the PlaySfx call in every update.
    assert_eq!(first.stub_histogram().get("PlaySfx"), Some(&60));

    // A second run of the same data must produce the exact same per-frame hashes.
    let mut second = Engine::load(source(), None, 1, DEFAULT_SEED).unwrap();
    let replay = second.run_frames(60, true).unwrap();
    assert_eq!(first.state_hash(), second.state_hash());
    assert_eq!(outcome.frame_hashes, replay.frame_hashes);
}

#[test]
fn different_seed_changes_the_state_hash() {
    let mut first = Engine::load(source(), None, 1, 1).unwrap();
    let mut second = Engine::load(source(), None, 1, 2).unwrap();
    first.run_frames(10, false).unwrap();
    second.run_frames(10, false).unwrap();
    assert_ne!(first.state_hash(), second.state_hash());
    let rng_first = first
        .state
        .entities
        .get(retro_scene::SCENE_ENTITY_START)
        .unwrap()
        .values[2];
    let rng_second = second
        .state
        .entities
        .get(retro_scene::SCENE_ENTITY_START)
        .unwrap()
        .values[2];
    assert_ne!(rng_first, rng_second, "the script's Rand calls differ");
}

#[test]
fn scene_selection_by_name_and_folder() {
    let engine = Engine::load(source(), Some("TEST ZONE"), 1, DEFAULT_SEED).unwrap();
    assert_eq!(engine.stage_info(), ("Zone01", "1"));
    let engine = Engine::load(source(), Some("zone01"), 1, DEFAULT_SEED).unwrap();
    assert_eq!(engine.stage_info(), ("Zone01", "1"));
    assert!(Engine::load(source(), Some("missing"), 1, DEFAULT_SEED).is_err());
}
