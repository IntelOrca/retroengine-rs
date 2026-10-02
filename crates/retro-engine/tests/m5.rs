//! M5 wiring tests: scripted input drives scripts, audio ops mix deterministically and save RAM
//! round-trips through in-memory storage.
//!
//! These are fully synthetic (no asset files needed) so they run in CI.

use std::sync::Arc;

use retro_engine::Engine;
use retro_engine::rng::DEFAULT_SEED;
use retro_format_v4::gameconfig::PALETTE_COUNT;
use retro_format_v4::scene::{ACTIVE_LAYER_COUNT, ENTITY_ATTRIB_STATE, ENTITY_ATTRIB_VALUES};
use retro_format_v4::stageconfig::STAGE_PALETTE_COUNT;
use retro_format_v4::userdata::SaveRam;
use retro_input::ScriptedInput;
use retro_io::{DirSource, MemorySource};
use retro_platform::headless::MemoryStorage;

const OBJECT_SOURCE: &str = "\
event ObjectStartup\n\
    object.value0 = 0\n\
end event\n\
event ObjectUpdate\n\
    if keyDown[0].right == true\n\
        object.xpos += 65536\n\
    end if\n\
    if input.pressButton == true\n\
        object.value1 = 1\n\
    end if\n\
    PlaySfx(0, false)\n\
    ReadSaveRAM()\n\
    saveRAM[7] = 1234\n\
    WriteSaveRAM()\n\
end event\n\
";

fn push_string(bytes: &mut Vec<u8>, value: &str) {
    bytes.push(value.len() as u8);
    bytes.extend_from_slice(value.as_bytes());
}

fn game_config_bytes() -> Vec<u8> {
    let mut bytes = Vec::new();
    push_string(&mut bytes, "M5 Synthetic");
    push_string(&mut bytes, "m5 wiring");
    for _ in 0..PALETTE_COUNT {
        bytes.extend_from_slice(&[0, 0, 0]);
    }
    bytes.push(1); // objects
    push_string(&mut bytes, "Mover");
    push_string(&mut bytes, "M5/Mover.txt");
    bytes.push(1); // global variables
    push_string(&mut bytes, "input.pressButton");
    bytes.extend_from_slice(&0i32.to_le_bytes());
    bytes.push(1); // sound effects
    push_string(&mut bytes, "Beep");
    push_string(&mut bytes, "Test/Beep.wav");
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
    bytes.push(1); // load global objects
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
    bytes.extend_from_slice(&(ENTITY_ATTRIB_STATE | ENTITY_ATTRIB_VALUES[0]).to_le_bytes());
    bytes.push(1); // object type
    bytes.push(0); // property value
    bytes.extend_from_slice(&(64i32 << 16).to_le_bytes());
    bytes.extend_from_slice(&(64i32 << 16).to_le_bytes());
    bytes.extend_from_slice(&0i32.to_le_bytes()); // state
    bytes.extend_from_slice(&7i32.to_le_bytes()); // value0
    bytes
}

/// Builds a minimal 16-bit stereo WAV with a short non-silent tone.
fn wav_bytes() -> Vec<u8> {
    let samples: Vec<i16> = (0..64).map(|i| ((i * 500) % 8000) as i16).collect();
    let mut data = Vec::new();
    for sample in &samples {
        data.extend_from_slice(&sample.to_le_bytes());
        data.extend_from_slice(&sample.to_le_bytes());
    }
    let mut out = Vec::new();
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&44_100u32.to_le_bytes());
    out.extend_from_slice(&(44_100 * 4u32).to_le_bytes());
    out.extend_from_slice(&4u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&data);
    out
}

fn memory_source() -> MemorySource {
    let mut source = MemorySource::new();
    source.insert(
        "Settings.ini",
        "[Game]\ngameType=1\ntxtScripts=n\n[Audio]\nstreamVolume=1.0\nsfxVolume=1.0\n",
    );
    source.insert("Data/Game/GameConfig.bin", game_config_bytes());
    source.insert("Data/Stages/Zone01/StageConfig.bin", stage_config_bytes());
    source.insert("Data/Stages/Zone01/Act1.bin", scene_bytes());
    source.insert("Data/Scripts/M5/Mover.txt", OBJECT_SOURCE);
    source.insert("Data/SoundFX/Test/Beep.wav", wav_bytes());
    source
}

fn source() -> Arc<dyn retro_io::DataSource> {
    Arc::new(memory_source())
}

/// A scripted replay with player 0 holding RIGHT for `frames` frames.
fn hold_right(frames: usize) -> ScriptedInput {
    let mut text = String::from("retro-input 1\n");
    for frame in 0..frames {
        text.push_str(&format!("{frame} RIGHT 0 0 -  - 0 0 -  - 0 0 -  - 0 0 -\n"));
    }
    ScriptedInput::from_str(&text).expect("scripted input")
}

fn player_x(engine: &Engine) -> i32 {
    engine
        .state
        .entities
        .get(retro_scene::SCENE_ENTITY_START)
        .expect("scene entity")
        .xpos
}

#[test]
fn scripted_right_moves_the_player_and_differs_from_null_input() {
    let mut idle = Engine::load(source(), None, None, DEFAULT_SEED).unwrap();
    idle.run_frames(60, false).unwrap();
    let idle_x = player_x(&idle);

    let mut scripted = Engine::load(source(), None, None, DEFAULT_SEED).unwrap();
    scripted.set_scripted_input(hold_right(60));
    scripted.run_frames(60, false).unwrap();
    let scripted_x = player_x(&scripted);

    assert_eq!(idle_x, 64 << 16, "NullInput keeps the spawn position");
    assert_eq!(
        scripted_x,
        idle_x + 60 * 65536,
        "one pixel of movement per held frame"
    );
    assert_ne!(idle.state_hash(), scripted.state_hash());
}

#[test]
fn input_press_button_global_tracks_pressed_edges() {
    let mut idle = Engine::load(source(), None, None, DEFAULT_SEED).unwrap();
    idle.run_frames(3, false).unwrap();
    let idle_value = idle
        .state
        .entities
        .get(retro_scene::SCENE_ENTITY_START)
        .unwrap()
        .values[1];
    assert_eq!(idle_value, 0, "no input means pressButton stays false");

    let mut pressed = Engine::load(source(), None, None, DEFAULT_SEED).unwrap();
    let mut text = String::from("retro-input 1\n");
    for frame in 0..3 {
        text.push_str(&format!("{frame} A 0 0 -  - 0 0 -  - 0 0 -  - 0 0 -\n"));
    }
    pressed.set_scripted_input(ScriptedInput::from_str(&text).unwrap());
    pressed.run_frames(3, false).unwrap();
    let pressed_value = pressed
        .state
        .entities
        .get(retro_scene::SCENE_ENTITY_START)
        .unwrap()
        .values[1];
    assert_eq!(
        pressed_value, 1,
        "A sets the rev03 input.pressButton global"
    );
}

#[test]
fn audio_ops_mix_a_deterministic_pcm_hash_and_mute_is_hash_neutral() {
    let mut first = Engine::load(source(), None, None, DEFAULT_SEED).unwrap();
    let mut second = Engine::load(source(), None, None, DEFAULT_SEED).unwrap();
    let mut muted = Engine::load(source(), None, None, DEFAULT_SEED).unwrap();
    for engine in [&mut first, &mut second, &mut muted] {
        engine.state.audio.set_capture(true);
    }
    muted.set_muted(true);

    let first_outcome = first.run_frames(30, false).unwrap();
    let second_outcome = second.run_frames(30, false).unwrap();
    let muted_outcome = muted.run_frames(30, false).unwrap();

    assert_eq!(first.state.audio.mixer().sfx_count(), 1, "lazy load once");
    assert!(
        first
            .state
            .audio
            .captured_pcm()
            .iter()
            .any(|sample| *sample != 0.0),
        "the generated WAV must be audible"
    );
    assert_eq!(
        first_outcome.audio_hashes, second_outcome.audio_hashes,
        "identical runs must produce identical per-frame audio hashes"
    );
    assert_eq!(
        first_outcome.audio_hashes, muted_outcome.audio_hashes,
        "--mute must not change the mix hash"
    );
    assert_eq!(
        first.state.audio.captured_pcm(),
        second.state.audio.captured_pcm()
    );
    assert_eq!(
        first.state.audio.captured_pcm(),
        muted.state.audio.captured_pcm()
    );
    assert_eq!(
        first_outcome.audio_hashes.len(),
        30,
        "one audio hash per logic frame"
    );
    assert_eq!(
        first.op_histogram().get("PlaySfx"),
        Some(&30),
        "PlaySfx is a ported op"
    );
    assert_eq!(first.stub_histogram().get("PlaySfx"), None);
}

#[test]
fn save_ops_round_trip_through_in_memory_storage() {
    let mut engine = Engine::load_with(
        source(),
        None,
        None,
        DEFAULT_SEED,
        Box::new(MemoryStorage::new()),
    )
    .unwrap();
    engine.run_frames(2, false).unwrap();

    assert_eq!(engine.op_histogram().get("ReadSaveRAM"), Some(&2));
    assert_eq!(engine.op_histogram().get("WriteSaveRAM"), Some(&2));
    assert_eq!(engine.state.save.save_ram().word(7), Some(1234));

    let bytes = engine
        .state
        .save
        .storage()
        .read(SaveRam::MODERN_SAVE_PATH)
        .expect("WriteSaveRAM persists SData.bin");
    let ram = SaveRam::from_bytes(&bytes).unwrap();
    assert_eq!(ram.word(7), Some(1234));
    assert_eq!(ram.words.len(), retro_format_v4::userdata::SAVE_RAM_WORDS);
    assert!(!engine.state.save.dirty());
    assert!(engine.flush_save());
}

#[test]
fn save_state_seeded_from_source_loads_a_shipped_sgame() {
    let mut memory = memory_source();
    let mut save = vec![0u8; 16];
    save[4..8].copy_from_slice(&42i32.to_le_bytes());
    memory.insert("SGame.bin", save.clone());

    let mut engine = Engine::load(Arc::new(memory), None, None, DEFAULT_SEED).unwrap();
    engine.run_frames(1, false).unwrap();
    assert_eq!(
        engine.state.save.save_file_kind(),
        retro_format_v4::userdata::SaveFileKind::SGame
    );
    assert_eq!(engine.state.save.save_ram().to_bytes(), save);
}

// ---------------------------------------------------------------------------
// Asset-gated runs (ignored by default; run with `-- --ignored`)
// ---------------------------------------------------------------------------

fn asset_root() -> std::path::PathBuf {
    std::env::var("RETRO_ASSETS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("/home/ted/projects/assets"))
}

fn asset_source(game: &str) -> Arc<dyn retro_io::DataSource> {
    Arc::new(DirSource::new(asset_root().join(game)).expect("asset folder"))
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

/// Asset run: 600 frames with NullInput, a scripted replay and audio hashing.
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
        let mut first = Engine::load(asset_source(game), Some(scene), None, DEFAULT_SEED).unwrap();
        first.state.audio.set_capture(true);
        let outcome = first.run_frames(600, true).unwrap();
        let first_pcm = pcm_hash(first.state.audio.captured_pcm());
        let first_audio = audio_chain(&outcome.audio_hashes);
        let first_player = player_state(&first);

        let mut second = Engine::load(asset_source(game), Some(scene), None, DEFAULT_SEED).unwrap();
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

        let mut scripted =
            Engine::load(asset_source(game), Some(scene), None, DEFAULT_SEED).unwrap();
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
