//! M7 fidelity tests: deferred `LoadStage` scene switching and title background scrolling.
//!
//! The state-machine tests use a synthetic two-act/two-zone data set and run everywhere; the
//! title/act regressions are asset-gated (`-- --ignored --nocapture`, `RETRO_ASSETS` override).

#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::sync::Arc;

use retro_engine::Engine;
use retro_engine::rng::DEFAULT_SEED;
use retro_format_v4::gameconfig::PALETTE_COUNT;
use retro_format_v4::scene::{
    ACTIVE_LAYER_COUNT, ENTITY_ATTRIB_PRIORITY, ENTITY_ATTRIB_STATE, ENTITY_ATTRIB_VALUES,
};
use retro_format_v4::stageconfig::STAGE_PALETTE_COUNT;
use retro_input::ScriptedInput;
use retro_io::{DataSource, DirSource, MemorySource};

// ---------------------------------------------------------------------------------------------
// Synthetic data: `Zone01` has Act1/Act2 (same folder), `Zone02` has Act1 (different folder).
// ---------------------------------------------------------------------------------------------

/// Global `Loader` object: after five updates sets `stage.listPos` and requests a load.
///
/// `loadCount` gates the request so a reload cannot loop, and lets a test observe how many times
/// startup ran.
const LOADER_SOURCE: &str = "\
event ObjectStartup\n\
    loadCount += 1\n\
end event\n\
event ObjectUpdate\n\
    object.value0 += 1\n\
    if loadCount == 1\n\
        if object.value0 == 5\n\
            stage.activeList = 1\n\
            stage.listPos = 1\n\
            LoadStage()\n\
        end if\n\
    end if\n\
end event\n\
";

/// `Zone02`'s own object (its StageConfig does not load the global objects).
const STAGE_ONLY_SOURCE: &str = "\
event ObjectStartup\n\
    object.value2 = loadCount\n\
end event\n\
event ObjectUpdate\n\
    object.value1 += 1\n\
    object.value2 = loadCount\n\
end event\n\
";

/// `Frozen` object: every entity counts updates, and reaching two freezes the stage.
const FROZEN_SOURCE: &str = "\
event ObjectUpdate\n\
    object.value0 += 1\n\
    if object.value0 == 2\n\
        stage.state = 3\n\
    end if\n\
end event\n\
";

fn push_string(bytes: &mut Vec<u8>, value: &str) {
    bytes.push(value.len() as u8);
    bytes.extend_from_slice(value.as_bytes());
}

fn scene_entry(bytes: &mut Vec<u8>, folder: &str, id: &str, name: &str) {
    push_string(bytes, folder);
    push_string(bytes, id);
    push_string(bytes, name);
    bytes.push(1);
}

/// GameConfig with a global `Loader` object and a three-entry Regular stage list.
fn game_config_bytes() -> Vec<u8> {
    let mut bytes = Vec::new();
    push_string(&mut bytes, "M7 Synthetic");
    push_string(&mut bytes, "m7 deferred load");
    for _ in 0..PALETTE_COUNT {
        bytes.extend_from_slice(&[0, 0, 0]);
    }
    bytes.push(1); // objects
    push_string(&mut bytes, "Loader");
    push_string(&mut bytes, "M7/Loader.txt");
    bytes.push(1); // global variables
    push_string(&mut bytes, "loadCount");
    bytes.extend_from_slice(&0i32.to_le_bytes());
    bytes.push(0); // sound effects
    bytes.push(0); // players
    // Presentation.
    bytes.push(0);
    // Regular: Zone01/1, Zone01/2, Zone02/1.
    bytes.push(3);
    scene_entry(&mut bytes, "Zone01", "1", "TEST ZONE 1");
    scene_entry(&mut bytes, "Zone01", "2", "TEST ZONE 2");
    scene_entry(&mut bytes, "Zone02", "1", "TEST ZONE 3");
    // Special, Bonus.
    bytes.push(0);
    bytes.push(0);
    bytes
}

fn stage_config_bytes(load_global_objects: u8, stage_objects: &[(&str, &str)]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.push(load_global_objects);
    for _ in 0..STAGE_PALETTE_COUNT {
        bytes.extend_from_slice(&[0, 0, 0]);
    }
    bytes.push(0); // sfx
    bytes.push(stage_objects.len() as u8);
    for (name, path) in stage_objects {
        push_string(&mut bytes, name);
        push_string(&mut bytes, path);
    }
    bytes
}

fn scene_bytes(title: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    push_string(&mut bytes, title);
    bytes.extend_from_slice(&[9; ACTIVE_LAYER_COUNT]);
    bytes.push(3); // midpoint
    bytes.push(1); // width
    bytes.push(0);
    bytes.push(1); // height
    bytes.push(0);
    bytes.extend_from_slice(&0u16.to_le_bytes()); // one chunk
    bytes.extend_from_slice(&1u16.to_le_bytes()); // one entity
    bytes.extend_from_slice(&(ENTITY_ATTRIB_STATE | ENTITY_ATTRIB_VALUES[0]).to_le_bytes());
    bytes.push(1); // object type 1
    bytes.push(0); // property value
    bytes.extend_from_slice(&(64i32 << 16).to_le_bytes());
    bytes.extend_from_slice(&(64i32 << 16).to_le_bytes());
    bytes.extend_from_slice(&0i32.to_le_bytes()); // state
    bytes.extend_from_slice(&0i32.to_le_bytes()); // value0
    bytes
}

/// GameConfig with a single global `Frozen` object and a one-entry Regular list.
fn frozen_game_config_bytes() -> Vec<u8> {
    let mut bytes = Vec::new();
    push_string(&mut bytes, "M7 Frozen");
    push_string(&mut bytes, "m7 frozen stage");
    for _ in 0..PALETTE_COUNT {
        bytes.extend_from_slice(&[0, 0, 0]);
    }
    bytes.push(1); // objects
    push_string(&mut bytes, "Frozen");
    push_string(&mut bytes, "M7/Frozen.txt");
    bytes.push(0); // global variables
    bytes.push(0); // sound effects
    bytes.push(0); // players
    bytes.push(0); // Presentation
    bytes.push(1); // Regular
    scene_entry(&mut bytes, "Zone01", "1", "FROZEN ZONE");
    bytes.push(0); // Special
    bytes.push(0); // Bonus
    bytes
}

/// Act with two entities of the `Frozen` object: slot 32 is `PRIORITY_ALWAYS`, slot 33 is the
/// default `PRIORITY_BOUNDS`.
fn frozen_scene_bytes() -> Vec<u8> {
    let mut bytes = Vec::new();
    push_string(&mut bytes, "FROZEN");
    bytes.extend_from_slice(&[9; ACTIVE_LAYER_COUNT]);
    bytes.push(3); // midpoint
    bytes.push(1); // width
    bytes.push(0);
    bytes.push(1); // height
    bytes.push(0);
    bytes.extend_from_slice(&0u16.to_le_bytes()); // one chunk
    bytes.extend_from_slice(&2u16.to_le_bytes()); // two entities
    bytes.extend_from_slice(&ENTITY_ATTRIB_PRIORITY.to_le_bytes());
    bytes.push(1); // object type 1
    bytes.push(0); // property value
    bytes.extend_from_slice(&(64i32 << 16).to_le_bytes());
    bytes.extend_from_slice(&(64i32 << 16).to_le_bytes());
    bytes.push(2); // PRIORITY_ALWAYS
    bytes.extend_from_slice(&0u16.to_le_bytes()); // no attributes: PRIORITY_BOUNDS
    bytes.push(1); // object type 1
    bytes.push(0); // property value
    bytes.extend_from_slice(&(96i32 << 16).to_le_bytes());
    bytes.extend_from_slice(&(64i32 << 16).to_le_bytes());
    bytes
}

fn frozen_source() -> Arc<dyn DataSource> {
    let mut source = MemorySource::new();
    source.insert("Settings.ini", "[Game]\ngameType=1\ntxtScripts=n\n");
    source.insert("Data/Game/GameConfig.bin", frozen_game_config_bytes());
    source.insert(
        "Data/Stages/Zone01/StageConfig.bin",
        stage_config_bytes(1, &[]),
    );
    source.insert("Data/Stages/Zone01/Act1.bin", frozen_scene_bytes());
    source.insert("Data/Scripts/M7/Frozen.txt", FROZEN_SOURCE);
    Arc::new(source)
}

fn synthetic_source() -> Arc<dyn DataSource> {
    let mut source = MemorySource::new();
    source.insert("Settings.ini", "[Game]\ngameType=1\ntxtScripts=n\n");
    source.insert("Data/Game/GameConfig.bin", game_config_bytes());
    source.insert(
        "Data/Stages/Zone01/StageConfig.bin",
        stage_config_bytes(1, &[]),
    );
    source.insert("Data/Stages/Zone01/Act1.bin", scene_bytes("ZONE01 ACT1"));
    source.insert("Data/Stages/Zone01/Act2.bin", scene_bytes("ZONE01 ACT2"));
    source.insert(
        "Data/Stages/Zone02/StageConfig.bin",
        stage_config_bytes(0, &[("StageOnly", "M7/StageOnly.txt")]),
    );
    source.insert("Data/Stages/Zone02/Act1.bin", scene_bytes("ZONE02 ACT1"));
    source.insert("Data/Scripts/M7/Loader.txt", LOADER_SOURCE);
    source.insert("Data/Scripts/M7/StageOnly.txt", STAGE_ONLY_SOURCE);
    Arc::new(source)
}

fn load_count(engine: &Engine) -> i32 {
    let index = engine
        .state
        .game_config
        .global_variables
        .iter()
        .position(|variable| variable.name == "loadCount")
        .expect("loadCount global");
    engine.scripts.vm_state.global_variables[index]
}

#[test]
fn load_stage_request_is_applied_once_after_the_frame() {
    let mut engine = Engine::load(synthetic_source(), None, None, DEFAULT_SEED).unwrap();
    assert_eq!(engine.stage_info(), ("Zone01", "1"));
    assert_eq!(
        engine.state.stage.active_list, 1,
        "default resolves the Regular list"
    );
    assert_eq!(engine.state.stage.list_pos, 0);
    assert_eq!(engine.state.stage.list_size, 3);
    assert_eq!(
        engine.state.stage.state, 1,
        "`stage.state` starts in STAGEMODE_NORMAL"
    );
    assert_eq!(load_count(&engine), 1, "startup ran on the first act");

    // Four update frames: the request has not been made yet.
    engine.run_frames(4, false).unwrap();
    assert!(!engine.state.load_stage_requested);
    assert_eq!(engine.stage_info(), ("Zone01", "1"));

    // Frame 5: the script op queues the request, but the frame still completes without a load.
    engine.run_frame().unwrap();
    assert!(engine.state.load_stage_requested, "request is deferred");
    assert_eq!(
        engine.stage_info(),
        ("Zone01", "1"),
        "the current frame must not switch scenes"
    );
    assert_eq!(engine.op_histogram().get("LoadStage"), Some(&1));

    // Frame 6: the request is consumed and the same-folder Act2 loads, with a fresh startup.
    engine.run_frame().unwrap();
    assert!(!engine.state.load_stage_requested);
    assert_eq!(engine.stage_info(), ("Zone01", "2"));
    assert_eq!(engine.state.stage.list_pos, 1);
    assert_eq!(
        load_count(&engine),
        2,
        "startup ran again on the reloaded act"
    );
    let entity = engine.state.entities.get(32).copied().unwrap_or_default();
    assert_eq!(entity.type_id, 1);
    assert_eq!(
        entity.values[0], 0,
        "entities are re-seeded from the act file"
    );

    // The reload cannot loop: `loadCount == 2` disables the trigger.
    engine.run_frames(120, false).unwrap();
    assert_eq!(engine.stage_info(), ("Zone01", "2"));
    assert_eq!(load_count(&engine), 2);
    assert_eq!(engine.op_histogram().get("LoadStage"), Some(&1));
}

#[test]
fn load_stage_switches_folders_and_relinks_globals() {
    let mut engine = Engine::load(synthetic_source(), None, None, DEFAULT_SEED).unwrap();
    engine.run_frames(5, false).unwrap();
    engine.run_frame().unwrap();
    engine.run_frame().unwrap();
    assert_eq!(engine.stage_info(), ("Zone01", "2"));

    // Full-folder load: `Zone02` does not load the global objects, so the object list is the
    // stage's own.
    engine.state.stage.list_pos = 2;
    engine.state.load_stage_requested = true;
    engine.run_frame().unwrap();
    assert_eq!(engine.stage_info(), ("Zone02", "1"));
    assert!(!engine.state.load_stage_requested);
    assert_eq!(engine.state.stage.active_list, 1);
    assert_eq!(engine.state.stage.list_size, 3);
    assert!(
        engine.state.objects.type_id("Loader").is_none(),
        "globals must be dropped when the new StageConfig does not load them"
    );
    assert_eq!(engine.state.objects.type_id("StageOnly"), Some(1));
    assert_eq!(engine.state.objects.len(), 2, "BlankObject + StageOnly");
    assert_eq!(
        load_count(&engine),
        2,
        "global variables persist across a full scene load"
    );
    let entity = engine.state.entities.get(32).copied().unwrap_or_default();
    assert_eq!(entity.type_id, 1);
    engine.run_frames(1, false).unwrap();
    let entity = engine.state.entities.get(32).copied().unwrap_or_default();
    assert_eq!(
        entity.values[2], 2,
        "the new stage scripts read the preserved global variables"
    );
    engine.run_frames(60, false).unwrap();
    assert_eq!(engine.stage_info(), ("Zone02", "1"));
}

#[test]
fn invalid_stage_list_position_errors() {
    let mut engine = Engine::load(synthetic_source(), None, None, DEFAULT_SEED).unwrap();
    engine.state.stage.list_pos = 99;
    engine.state.load_stage_requested = true;
    let error = engine.run_frame().unwrap_err();
    assert!(
        matches!(
            error,
            retro_engine::EngineError::InvalidStageList { list: 1, pos: 99 }
        ),
        "unexpected error: {error:?}"
    );
}

#[test]
fn frozen_stage_updates_only_priority_always_entities() {
    use retro_engine::state::{STAGEMODE_FROZEN, STAGEMODE_NORMAL};

    let mut engine = Engine::load(frozen_source(), None, None, DEFAULT_SEED).unwrap();
    let value = |engine: &Engine, slot: usize| engine.state.entities.get(slot).unwrap().values[0];

    // Frames 1 and 2: both entities update normally and freeze the stage on frame 2.
    engine.run_frame().unwrap();
    assert_eq!(value(&engine, 32), 1);
    assert_eq!(value(&engine, 33), 1);
    assert_eq!(engine.state.stage.state, STAGEMODE_NORMAL);
    engine.run_frame().unwrap();
    assert_eq!(value(&engine, 32), 2);
    assert_eq!(value(&engine, 33), 2);
    assert_eq!(engine.state.stage.state, STAGEMODE_FROZEN);

    // Frame 3 (`ProcessFrozenObjects`): only `PRIORITY_ALWAYS` entities update, but the type
    // groups are rebuilt and the stage still draws, so the ALWAYS entity enters the draw list.
    engine.run_frame().unwrap();
    assert_eq!(value(&engine, 32), 3, "PRIORITY_ALWAYS keeps updating");
    assert_eq!(value(&engine, 33), 2, "PRIORITY_BOUNDS is frozen");
    let group = engine
        .state
        .type_groups
        .get(1)
        .expect("type group for the Frozen object");
    assert!(
        group.entity_refs.contains(&32) && group.entity_refs.contains(&33),
        "frozen type groups still list every active entity: {:?}",
        group.entity_refs
    );
    assert!(
        engine.state.draw_lists[3].contains(&32) && engine.state.draw_lists[3].contains(&33),
        "the frozen draw pass still draws every active entity: {:?}",
        engine.state.draw_lists[3]
    );
}

// ---------------------------------------------------------------------------------------------
// Asset-gated tests.
// ---------------------------------------------------------------------------------------------

fn asset_root() -> PathBuf {
    std::env::var("RETRO_ASSETS")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/home/ted/projects/assets"))
}

fn source(game: &str) -> Arc<dyn DataSource> {
    Arc::new(DirSource::new(asset_root().join(game)).expect("asset folder"))
}

fn load(game: &str) -> Engine {
    Engine::load(source(game), Some("Title"), None, DEFAULT_SEED).expect("engine load")
}

/// Replay that presses A from `press_at` until `frames`.
fn start_replay(frames: usize, press_at: usize) -> ScriptedInput {
    replay(frames, press_at, None)
}

/// Replay that presses A from `press_at` and additionally holds Right from `right_at`.
fn replay(frames: usize, press_at: usize, right_at: Option<usize>) -> ScriptedInput {
    let mut text = String::from("retro-input 1\n");
    for frame in 0..frames {
        let mut buttons = Vec::new();
        if frame >= press_at {
            buttons.push("A");
        }
        if right_at.is_some_and(|start| frame >= start) {
            buttons.push("RIGHT");
        }
        let buttons = if buttons.is_empty() {
            "-".to_owned()
        } else {
            buttons.join("|")
        };
        text.push_str(&format!(
            "{frame} {buttons} 0 0 -  - 0 0 -  - 0 0 -  - 0 0 -\n"
        ));
    }
    ScriptedInput::from_str(&text).expect("scripted input")
}

/// Counts pixels that differ by more than 30 (sum of RGB deltas) between two framebuffers in
/// `x_range`/`y_range`.
fn pixel_diff(
    engine: &Engine,
    other: &Engine,
    x_range: std::ops::Range<usize>,
    y_range: std::ops::Range<usize>,
) -> usize {
    let a = engine.framebuffer();
    let b = other.framebuffer();
    let mut changed = 0;
    for y in y_range {
        for x in x_range.clone() {
            let (Ok(x), Ok(y)) = (i32::try_from(x), i32::try_from(y)) else {
                continue;
            };
            let pa = a.get(x, y);
            let pb = b.get(x, y);
            let delta = (i32::from(pa >> 11 & 0x1F) - i32::from(pb >> 11 & 0x1F)).abs()
                + (i32::from(pa >> 5 & 0x3F) - i32::from(pb >> 5 & 0x3F)).abs()
                + (i32::from(pa & 0x1F) - i32::from(pb & 0x1F)).abs();
            if delta > 30 {
                changed += 1;
            }
        }
    }
    changed
}

#[test]
#[ignore = "requires assets"]
fn title_start_loads_zone01() {
    for game in ["S1", "S2"] {
        let mut engine = load(game);
        engine.set_scripted_input(start_replay(1100, 800));
        engine.run_frames(1100, false).expect("run must not fail");
        let (folder, act) = engine.stage_info();
        assert_eq!(
            (folder, act),
            ("Zone01", "1"),
            "{game}: Start on the title must load Zone01 act 1"
        );
        assert!(!engine.state.load_stage_requested);
        assert_eq!(engine.state.stage.active_list, 1, "{game}: Regular list");
        assert!(engine.state.stage.list_size > 0, "{game}: list size filled");
        let player = engine.state.entities.get(0).copied().unwrap_or_default();
        assert_eq!(player.type_id, 1, "{game}: player object must spawn");
        assert!(
            engine.state.camera.target == 0,
            "{game}: level scripts must hand the camera to the player (got {})",
            engine.state.camera.target
        );
        let loaded_hash = engine.state_hash();

        // No repeated load/fade loop: the level stays put and no further `LoadStage` runs.
        let loads = engine.op_histogram().get("LoadStage").copied().unwrap_or(0);
        engine.run_frames(120, false).expect("run must not fail");
        assert_eq!(engine.stage_info(), ("Zone01", "1"));
        assert_eq!(
            engine.op_histogram().get("LoadStage").copied().unwrap_or(0),
            loads,
            "{game}: no repeated LoadStage after the transition"
        );

        // Determinism: the whole title -> zone flow replays byte for byte.
        let mut replay = load(game);
        replay.set_scripted_input(start_replay(1100, 800));
        replay.run_frames(1100, false).expect("run must not fail");
        assert_eq!(
            loaded_hash,
            replay.state_hash(),
            "{game}: title -> Zone01 must replay identically"
        );
    }
}

#[test]
#[ignore = "requires assets"]
fn act_reload_advances_without_reload_loop() {
    for game in ["S1", "S2"] {
        let mut engine = load(game);
        engine.set_scripted_input(start_replay(1100, 800));
        engine.run_frames(1100, false).expect("run must not fail");
        assert_eq!(engine.stage_info(), ("Zone01", "1"));

        // Act 1 -> Act 2: same stage folder, so upstream takes the "reloading" path.
        engine.state.stage.list_pos += 1;
        engine.state.load_stage_requested = true;
        engine.run_frame().unwrap();
        assert_eq!(engine.stage_info(), ("Zone01", "2"), "{game}: act 1 -> 2");
        assert_eq!(
            engine.state.stage.act_num, 2,
            "{game}: `stage.actNum` follows"
        );
        assert_eq!(
            engine.state.render.fade_mode, 0,
            "{game}: fade reset on load"
        );
        let player = engine.state.entities.get(0).copied().unwrap_or_default();
        assert_eq!(player.type_id, 1, "{game}: player spawns in act 2");

        // Settle, then jump to the next zone in the Regular list (a different folder).
        engine.run_frames(120, false).expect("run must not fail");
        let list = engine
            .state
            .game_config
            .category_for_engine_index(1)
            .expect("Regular list");
        let next_zone = list
            .scenes
            .iter()
            .position(|entry| entry.folder != "Zone01")
            .expect("a second zone exists");
        let loads_before = engine.op_histogram().get("LoadStage").copied().unwrap_or(0);
        engine.state.stage.list_pos = next_zone as i32;
        engine.state.load_stage_requested = true;
        engine.run_frame().unwrap();
        let (folder, act) = engine.stage_info();
        let (folder, act) = (folder.to_owned(), act.to_owned());
        assert_eq!(folder, "Zone02", "{game}: next zone loads (act {act})");
        assert_eq!(engine.state.stage.list_pos, next_zone as i32);
        assert_eq!(engine.state.render.fade_mode, 0, "{game}: fade reset");

        // The new zone runs on and does not re-trigger the transition.
        engine.run_frames(120, false).expect("run must not fail");
        assert_eq!(engine.stage_info(), (folder.as_str(), act.as_str()));
        assert_eq!(
            engine.op_histogram().get("LoadStage").copied().unwrap_or(0),
            loads_before,
            "{game}: no load loop in the new zone"
        );
    }
}

#[test]
#[ignore = "requires assets"]
fn s1_title_background_scrolls() {
    // The Logo object takes over around frame 500 and drives `screen.xoffset += 2` every frame.
    let mut engine = load("S1");
    engine.run_frames(520, false).expect("title must run");
    let before = engine.state.screen.x_scroll;
    assert_eq!(
        engine.state.camera.target, -1,
        "the title camera stays targetless; script scroll is authoritative"
    );
    let mut later = load("S1");
    later.run_frames(640, false).expect("title must run");
    assert_eq!(
        later.state.screen.x_scroll - before,
        240,
        "S1 title scroll advances 2 px/frame over 120 frames"
    );
    assert!(
        later.state.screen.x_scroll > before,
        "S1 title background must scroll"
    );
    // The water band is background-only: it must move while the centred logo animates.
    assert!(
        pixel_diff(&engine, &later, 0..140, 150..240) > 2000,
        "S1 title background pixels must change across 120 frames"
    );
}

#[test]
#[ignore = "requires assets"]
fn s2_title_background_scrolls_via_deformation() {
    // `ST Logo` fills `stage.deformationData2` from its startup and advances
    // `tileLayer[2].deformationOffset` every 8 frames, which drives the bottom background.
    let mut engine = load("S2");
    engine.run_frames(700, false).expect("title must run");
    let written = engine.state.render.deform_data[2]
        .iter()
        .filter(|value| **value != 0)
        .count();
    assert!(
        written > 0,
        "S2 title must populate `stage.deformationData2` from its startup script"
    );
    let before_offset = engine.state.layers[2].deformation_offset;
    let mut later = load("S2");
    later.run_frames(820, false).expect("title must run");
    assert!(
        later.state.layers[2].deformation_offset > before_offset,
        "S2 deformation offset must advance ({} -> {})",
        before_offset,
        later.state.layers[2].deformation_offset
    );
    assert!(
        pixel_diff(&engine, &later, 0..140, 180..240) > 2000,
        "S2 title background pixels must change across 120 frames"
    );
}

#[test]
#[ignore = "requires assets"]
fn player_camera_follows_after_the_title_transition() {
    let mut engine = load("S1");
    engine.set_scripted_input(start_replay(1100, 800));
    engine.run_frames(1100, false).expect("run must not fail");
    let half = engine.state.screen.center_x();
    assert!(
        engine.state.screen.x_scroll == engine.state.camera.xpos - half,
        "camera follow must remain authoritative once a zone hands it the player"
    );
    // Holding Right moves the player; the camera and scroll must follow.
    let before = engine.state.screen.x_scroll;
    let mut moving = load("S1");
    moving.set_scripted_input(replay(1300, 800, Some(1000)));
    moving.run_frames(1300, false).expect("run must not fail");
    assert!(
        moving.state.screen.x_scroll > before,
        "camera scroll must track the moving player ({} -> {})",
        before,
        moving.state.screen.x_scroll
    );
}
