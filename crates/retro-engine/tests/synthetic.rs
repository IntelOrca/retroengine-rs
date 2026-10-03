//! Synthetic end-to-end test: a hand-built asset set in memory runs through the real engine.
//!
//! This exercises loading, script compilation/linking, scene instantiation, the startup pass and
//! 60 update frames without any files on disk, and pins determinism of the state hash.

use std::sync::Arc;

use retro_engine::Engine;
use retro_engine::rng::DEFAULT_SEED;
use retro_format_v4::gameconfig::PALETTE_COUNT;
use retro_format_v4::scene::{
    ACTIVE_LAYER_COUNT, ENTITY_ATTRIB_DIRECTION, ENTITY_ATTRIB_DRAW_ORDER, ENTITY_ATTRIB_PRIORITY,
    ENTITY_ATTRIB_STATE, ENTITY_ATTRIB_VALUES,
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
    bytes.push(1); // global variables
    push_string(&mut bytes, "drawOrder");
    bytes.extend_from_slice(&0i32.to_le_bytes());
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
    let mut first = Engine::load(source(), None, None, DEFAULT_SEED).unwrap();
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

    // PlaySfx is a real ported op now; the synthetic config has no SFX slot, so it is a no-op
    // but still counted.
    assert_eq!(first.op_histogram().get("PlaySfx"), Some(&60));
    assert_eq!(first.stub_histogram().get("PlaySfx"), None);

    // A second run of the same data must produce the exact same per-frame hashes.
    let mut second = Engine::load(source(), None, None, DEFAULT_SEED).unwrap();
    let replay = second.run_frames(60, true).unwrap();
    assert_eq!(first.state_hash(), second.state_hash());
    assert_eq!(outcome.frame_hashes, replay.frame_hashes);
}

#[test]
fn different_seed_changes_the_state_hash() {
    let mut first = Engine::load(source(), None, None, 1).unwrap();
    let mut second = Engine::load(source(), None, None, 2).unwrap();
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
    let engine = Engine::load(source(), Some("TEST ZONE"), None, DEFAULT_SEED).unwrap();
    assert_eq!(engine.stage_info(), ("Zone01", "1"));
    let engine = Engine::load(source(), Some("zone01"), None, DEFAULT_SEED).unwrap();
    assert_eq!(engine.stage_info(), ("Zone01", "1"));
    assert!(Engine::load(source(), Some("missing"), None, DEFAULT_SEED).is_err());
}

/// Builds a synthetic asset set whose single object type records draw-event execution order in a
/// global variable and optionally paints a 1x1 rectangle.
fn draw_source(draw_body: &str) -> Arc<dyn retro_io::DataSource> {
    let script = format!("event ObjectDraw\n{draw_body}end event\n");
    let mut bytes = Vec::new();
    push_string(&mut bytes, "Synthetic");
    push_string(&mut bytes, "draw ordering");
    for _ in 0..PALETTE_COUNT {
        bytes.extend_from_slice(&[0, 0, 0]);
    }
    bytes.push(1); // object count
    push_string(&mut bytes, "Draw Object");
    push_string(&mut bytes, "Test/DrawObject.txt");
    bytes.push(1); // global variables
    push_string(&mut bytes, "drawOrder");
    bytes.extend_from_slice(&0i32.to_le_bytes());
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

    let mut scene = Vec::new();
    push_string(&mut scene, "TEST");
    scene.extend_from_slice(&[9; ACTIVE_LAYER_COUNT]);
    scene.push(3); // midpoint
    scene.push(1);
    scene.push(0);
    scene.push(1);
    scene.push(0);
    scene.extend_from_slice(&0u16.to_le_bytes()); // one chunk
    scene.extend_from_slice(&2u16.to_le_bytes()); // two entities
    for slot in 0..2u8 {
        let attributes = ENTITY_ATTRIB_DRAW_ORDER | ENTITY_ATTRIB_VALUES[1];
        scene.extend_from_slice(&attributes.to_le_bytes());
        scene.push(1); // type: the only object
        scene.push(0); // property value
        scene.extend_from_slice(&(48i32 << 16).to_le_bytes());
        scene.extend_from_slice(&((48 + i32::from(slot) * 16) << 16).to_le_bytes());
        scene.push(3); // draw order
        scene.extend_from_slice(&(32i32 + i32::from(slot)).to_le_bytes()); // value1 = slot id
    }

    let mut source = MemorySource::new();
    source.insert("Settings.ini", "[Game]\ngameType=1\ntxtScripts=n\n");
    source.insert("Data/Game/GameConfig.bin", bytes);
    source.insert("Data/Stages/Zone01/StageConfig.bin", stage_config_bytes());
    source.insert("Data/Stages/Zone01/Act1.bin", scene);
    source.insert("Data/Scripts/Test/DrawObject.txt", script);
    Arc::new(source)
}

/// Builds a synthetic asset set with a green and a blue 1x1 `DrawRect` object (types 1 and 2)
/// whose draw lists are set per entity, plus a one-chunk act layout used by layer slot 0.
fn overlap_source(orders: [u8; 2]) -> Arc<dyn retro_io::DataSource> {
    let mut bytes = Vec::new();
    push_string(&mut bytes, "Synthetic");
    push_string(&mut bytes, "draw overlap");
    for _ in 0..PALETTE_COUNT {
        bytes.extend_from_slice(&[0, 0, 0]);
    }
    bytes.push(2); // object count: all names first, then the paired script paths
    push_string(&mut bytes, "Green Object");
    push_string(&mut bytes, "Blue Object");
    push_string(&mut bytes, "Test/GreenObject.txt");
    push_string(&mut bytes, "Test/BlueObject.txt");
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

    let mut scene = Vec::new();
    push_string(&mut scene, "TEST");
    scene.extend_from_slice(&[0, 9, 9, 9]); // layer slot 0 draws act layout 0
    scene.push(3); // midpoint: list 0 is under layer 0, list 3 is over it
    scene.push(1);
    scene.push(0);
    scene.push(1);
    scene.push(0);
    scene.extend_from_slice(&0u16.to_le_bytes()); // one chunk
    scene.extend_from_slice(&2u16.to_le_bytes()); // two entities
    for (slot, order) in orders.into_iter().enumerate() {
        let attributes = ENTITY_ATTRIB_DRAW_ORDER | ENTITY_ATTRIB_PRIORITY;
        scene.extend_from_slice(&attributes.to_le_bytes());
        scene.push(1 + slot as u8); // type: green then blue
        scene.push(0); // property value
        scene.extend_from_slice(&(100i32 << 16).to_le_bytes());
        scene.extend_from_slice(&(100i32 << 16).to_le_bytes());
        scene.push(order);
        scene.push(2); // PRIORITY_ALWAYS keeps the entity active regardless of scroll
    }

    let mut source = MemorySource::new();
    source.insert(
        "Settings.ini",
        "[Game]
gameType=1
txtScripts=n
",
    );
    source.insert("Data/Game/GameConfig.bin", bytes);
    source.insert("Data/Stages/Zone01/StageConfig.bin", stage_config_bytes());
    source.insert("Data/Stages/Zone01/Act1.bin", scene);
    source.insert(
        "Data/Scripts/Test/GreenObject.txt",
        "event ObjectDraw\n    DrawRect(0, 0, 1, 1, 0, 255, 0, 255)\nend event\n",
    );
    source.insert(
        "Data/Scripts/Test/BlueObject.txt",
        "event ObjectDraw\n    DrawRect(0, 0, 1, 1, 0, 0, 255, 255)\nend event\n",
    );
    Arc::new(source)
}

/// Replaces the synthetic (empty) tile assets with a one-chunk, fully opaque red layer so the
/// framebuffer proves which draw pass wrote the overlapping pixel last.
fn install_overlap_layer(engine: &mut Engine) {
    engine.state.stage.active_layers = [0, 9, 9, 9];
    {
        let layer = &mut engine.state.layers[0];
        layer.xsize = 1;
        layer.ysize = 1;
        layer.layer_type = retro_render::LAYER_HSCROLL;
        layer.set_entry(0, 0, 0);
    }
    let mut pixels = vec![0u8; retro_render::TILE_SET_16_SIZE];
    pixels[..256].fill(1);
    engine.state.render.tiles.pixels = pixels;
    engine.state.render.tiles.chunks = vec![
        retro_render::ChunkEntry {
            gfx_data_pos: 0,
            direction: 0,
            visual_plane: 0,
        };
        64
    ];
    engine.state.render.palette.set_entry(0, 1, 255, 0, 0);
}

#[test]
fn draw_order_interleaves_objects_and_tile_layers() {
    // `midpoint == 3`: list 0 runs before tile layer 0, list 3 runs after it. The opaque red
    // layer therefore covers the green object but not the blue one.
    let mut engine = Engine::load(overlap_source([0, 3]), None, None, DEFAULT_SEED).unwrap();
    install_overlap_layer(&mut engine);
    engine.run_frame().unwrap();
    assert_eq!(
        engine.state.draw_lists[3],
        vec![33],
        "only the blue entity is in list 3"
    );
    assert_eq!(
        engine.framebuffer().get(0, 0),
        0x001F,
        "blue object draws after the tile layer"
    );

    // Removing the blue entity leaves only the green list-0 object, which the layer covers.
    let mut engine = Engine::load(overlap_source([0, 3]), None, None, DEFAULT_SEED).unwrap();
    install_overlap_layer(&mut engine);
    engine
        .state
        .entities
        .get_mut(33)
        .expect("blue entity")
        .type_id = 0;
    engine.run_frame().unwrap();
    assert_eq!(
        engine.framebuffer().get(0, 0),
        0xF800,
        "tile layer draws over the list-0 object"
    );

    // Two objects in the same list are processed in ascending slot order, so blue wins.
    let mut engine = Engine::load(overlap_source([3, 3]), None, None, DEFAULT_SEED).unwrap();
    install_overlap_layer(&mut engine);
    engine.run_frame().unwrap();
    assert_eq!(engine.state.draw_lists[3], vec![32, 33]);
    assert_eq!(
        engine.framebuffer().get(0, 0),
        0x001F,
        "later slot in the same draw list wins"
    );
}

#[test]
fn draw_events_run_after_updates_in_slot_order() {
    let mut engine = Engine::load(
        draw_source("    drawOrder *= 10\n    drawOrder += object.value1\n"),
        None,
        None,
        DEFAULT_SEED,
    )
    .unwrap();
    engine.run_frame().unwrap();
    // Entities live at slots 32 and 33; the draw list is built during the update pass in
    // ascending slot order and processed in exactly that order. Each draw event folds its
    // identity (value1 = slot id) into value0, so the encoded order proves it.
    assert_eq!(engine.state.draw_lists[3], vec![32, 33]);
    assert_eq!(engine.scripts.vm_state.global_variables[0], 32 * 10 + 33);
    assert_eq!(engine.op_histogram().get("DrawRect"), None);
}

#[test]
fn dimming_is_presentation_only_and_never_enters_the_hash() {
    // Draw something so the framebuffer comparison is meaningful.
    let body = "    DrawRect(0, 0, 2, 2, 255, 0, 0, 255)\n";
    // The default `[Window] DimLimit` (300 s at 60 fps) is wired into the render state.
    let mut control = Engine::load(draw_source(body), None, None, DEFAULT_SEED).unwrap();
    assert_eq!(control.state.render.dim_limit, 18_000);
    assert_eq!(control.state.render.dim_timer, 0);

    // An immediate dim limit decays `dimPercent` (the SDL present buffer darkens) without
    // touching the framebuffer or the canonical hash.
    let mut dimmed = Engine::load(draw_source(body), None, None, DEFAULT_SEED).unwrap();
    dimmed.state.render.dim_limit = 0;
    for _ in 0..5 {
        control.run_frame().unwrap();
        dimmed.run_frame().unwrap();
    }
    assert_eq!(
        dimmed.state.render.dim_timer, 0,
        "limit 0 engages immediately"
    );
    assert!(
        dimmed.state.render.dim_amount() < 1.0,
        "dim amount decays after the idle limit"
    );
    assert_eq!(control.state.render.dim_amount(), 1.0);
    assert_eq!(control.framebuffer().get(0, 0), 0xF800);
    assert_eq!(
        control.state.render.framebuffer.hash(),
        dimmed.state.render.framebuffer.hash(),
        "dimming must not modify the framebuffer"
    );
    assert_eq!(control.state_hash(), dimmed.state_hash());

    // Idle frames advance the timer towards the limit.
    let mut idle = Engine::load(draw_source(body), None, None, DEFAULT_SEED).unwrap();
    idle.state.render.dim_limit = 10;
    let mut script = String::from("retro-input 1\n");
    for frame in 0..4 {
        script.push_str(&format!("{frame} - 0 0 -  - 0 0 -  - 0 0 -  - 0 0 -\n"));
    }
    script.push_str("4 RIGHT 0 0 -  - 0 0 -  - 0 0 -  - 0 0 -\n");
    idle.set_scripted_input(retro_input::ScriptedInput::from_str(&script).unwrap());
    for _ in 0..4 {
        idle.run_frame().unwrap();
    }
    assert_eq!(idle.state.render.dim_timer, 4);
    // A held button resets the timer.
    idle.run_frame().unwrap();
    assert!(idle.state.input.right);
    assert_eq!(idle.state.render.dim_timer, 0);
}

#[test]
fn framebuffer_pixels_are_part_of_the_state_hash() {
    let mut red = Engine::load(
        draw_source("    DrawRect(0, 0, 2, 2, 255, 0, 0, 255)\n"),
        None,
        None,
        DEFAULT_SEED,
    )
    .unwrap();
    red.run_frame().unwrap();
    assert_eq!(red.framebuffer().get(0, 0), 0xF800);

    let mut blue = Engine::load(
        draw_source("    DrawRect(0, 0, 2, 2, 0, 0, 255, 255)\n"),
        None,
        None,
        DEFAULT_SEED,
    )
    .unwrap();
    blue.run_frame().unwrap();
    assert_eq!(blue.framebuffer().get(0, 0), 0x001F);
    assert_ne!(
        red.state_hash(),
        blue.state_hash(),
        "identical entity state but different pixels must hash differently"
    );
}
