//! Asset-gated integration tests against the local Sonic 1/2 asset trees.
//!
//! Run with `cargo test -p retro-format-v4 -- --ignored --nocapture`. The tests parse every
//! `GameConfig.bin`, `StageConfig.bin`, `Settings.ini`, `SGame.bin` and `Achievements.bin` in
//! `/home/ted/projects/assets/{S1,S2}` through a `DirSource`, round-trip the save files and
//! dump each stage configuration to `/home/ted/projects/tmp/m1-dumps/<game>/<stage>.json`.

#![forbid(unsafe_code)]

use std::path::Path;

use retro_format_v4::{Achievements, GameConfig, GameType, SaveRam, Settings, StageConfig};
use retro_io::{DataSource, DirSource};

const ASSET_ROOT: &str = "/home/ted/projects/assets";
const DUMP_ROOT: &str = "/home/ted/projects/tmp/m1-dumps";

fn source(game: &str) -> DirSource {
    let root = format!("{ASSET_ROOT}/{game}");
    DirSource::new(&root).unwrap_or_else(|error| panic!("cannot open {root}: {error}"))
}

#[test]
#[ignore = "requires the local Sonic 1/2 asset trees"]
fn game_configs_parse_and_meet_object_floor() {
    let mut total_objects = 0usize;
    for game in ["S1", "S2"] {
        let source = source(game);
        let config = GameConfig::load(&source).unwrap_or_else(|error| panic!("{game}: {error}"));
        println!("{game}: title={:?}", config.title);
        println!(
            "{game}: objects={} variables={} sfx={} players={} scenes={}",
            config.objects.len(),
            config.global_variables.len(),
            config.sound_effects.len(),
            config.players.len(),
            config.scene_count()
        );
        for category in &config.categories {
            println!(
                "{game}: category {:?} scenes={}",
                category.name,
                category.scenes.len()
            );
        }
        assert_eq!(config.palette.len(), 0x60, "{game}: palette size");
        assert_eq!(config.categories.len(), 4, "{game}: category count");
        assert!(config.scene_count() > 0, "{game}: no scenes");
        // Every scene must point at a real stage folder so later loaders cannot fail silently.
        for category in &config.categories {
            for scene in &category.scenes {
                assert!(
                    source
                        .size(&format!("Data/Stages/{}/StageConfig.bin", scene.folder))
                        .is_some(),
                    "{game}: scene {} references missing folder {}",
                    scene.name,
                    scene.folder
                );
            }
        }
        total_objects += config.objects.len();
    }
    println!("total GameConfig objects: {total_objects}");
    assert!(
        total_objects >= 75,
        "expected at least 75 objects across S1+S2, found {total_objects}"
    );
}

#[test]
#[ignore = "requires the local Sonic 1/2 asset trees"]
fn stage_configs_parse_and_dump() {
    let dump_root = Path::new(DUMP_ROOT);
    std::fs::create_dir_all(dump_root).unwrap();
    let mut grand_total = 0usize;
    for game in ["S1", "S2"] {
        let source = source(game);
        let game_dir = dump_root.join(game);
        std::fs::create_dir_all(&game_dir).unwrap();

        let stages_root = format!("{ASSET_ROOT}/{game}/Data/Stages");
        let mut folders: Vec<String> = std::fs::read_dir(&stages_root)
            .unwrap_or_else(|error| panic!("cannot list {stages_root}: {error}"))
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| Path::new(&stages_root).join(name).is_dir())
            .collect();
        folders.sort();

        let mut object_counts = Vec::with_capacity(folders.len());
        let mut global_scenes = 0usize;
        for folder in &folders {
            let config = StageConfig::load(&format!("Data/Stages/{folder}"), &source)
                .unwrap_or_else(|error| panic!("{game}/{folder}: {error}"));
            assert_eq!(
                config.palette.len(),
                0x20,
                "{game}/{folder}: stage palette size"
            );
            assert!(
                !config.objects.is_empty(),
                "{game}/{folder}: empty object list"
            );
            object_counts.push(config.objects.len());
            if config.load_global_objects {
                global_scenes += 1;
            }
            let json = serde_json::to_string_pretty(&config).unwrap();
            std::fs::write(game_dir.join(format!("{folder}.json")), json).unwrap();
        }

        println!(
            "{game}: stage configs={} objects={} object_min={} object_max={} global_flag={}",
            folders.len(),
            object_counts.iter().sum::<usize>(),
            object_counts.iter().copied().min().unwrap_or(0),
            object_counts.iter().copied().max().unwrap_or(0),
            global_scenes
        );
        grand_total += folders.len();
    }
    println!("total stage configs parsed: {grand_total}");
    assert!(grand_total > 0);
}

#[test]
#[ignore = "requires the local Sonic 1/2 asset trees"]
fn settings_parse_as_origins_configs() {
    for game in ["S1", "S2"] {
        let source = source(game);
        let settings = Settings::load(&source).unwrap_or_else(|error| panic!("{game}: {error}"));
        println!(
            "{game}: data_file={:?} game_type={:?} txt_scripts={} pix_width={} win={}x{} volumes={}/{}",
            settings.game.data_file,
            settings.game.game_type,
            settings.game.txt_scripts,
            settings.video.pix_width,
            settings.video.win_width,
            settings.video.win_height,
            settings.audio.stream_volume,
            settings.audio.sfx_volume
        );
        assert_eq!(
            settings.game.game_type,
            GameType::Origins,
            "{game}: gameType != Origins (1)"
        );
        assert!(!settings.game.txt_scripts, "{game}: txtScripts");
        assert_eq!(
            settings.game.data_file.as_deref(),
            Some("Data.rsdk"),
            "{game}: dataFile"
        );
        assert_eq!(settings.video.pix_width, 424, "{game}: pixWidth");
        assert_eq!(settings.video.win_width, 848, "{game}: winWidth");
        assert_eq!(settings.video.win_height, 480, "{game}: winHeight");
        assert_eq!(settings.audio.stream_volume, 1.0, "{game}: streamVolume");
        assert_eq!(settings.audio.sfx_volume, 1.0, "{game}: sfxVolume");
        assert_eq!(settings.keyboard_maps.len(), 4, "{game}: keyboard maps");
        assert_eq!(
            settings.keyboard_maps[0].up,
            Some(0x26),
            "{game}: keyboard up"
        );
        assert_eq!(
            settings.keyboard_maps[2].up,
            Some(0),
            "{game}: keyboard map 3"
        );
    }
}

#[test]
#[ignore = "requires the local Sonic 1/2 asset trees"]
fn save_and_achievement_files_round_trip() {
    for game in ["S1", "S2"] {
        let source = source(game);

        let achievements =
            Achievements::load(&source).unwrap_or_else(|error| panic!("{game}: {error}"));
        let bytes = source.read(Achievements::PATH).unwrap();
        println!(
            "{game}: Achievements.bin size={} slots={}",
            bytes.len(),
            achievements.entries.len()
        );
        assert_eq!(
            achievements.entries.len(),
            0x100,
            "{game}: achievement slots"
        );
        assert_eq!(
            achievements.to_bytes(),
            bytes,
            "{game}: achievements round-trip"
        );
    }

    let source = source("S1");
    let size = source
        .size(SaveRam::SAVE_PATH)
        .expect("S1/SGame.bin missing");
    assert_eq!(size, 0x2000 * 4, "S1/SGame.bin size");
    let (save, kind) = SaveRam::load(&source).unwrap();
    let bytes = source.read(SaveRam::SAVE_PATH).unwrap();
    println!(
        "S1: SGame.bin size={} kind={:?} words={} nonzero={}",
        size,
        kind,
        save.words.len(),
        save.words.iter().filter(|word| **word != 0).count()
    );
    assert_eq!(save.words.len(), 0x2000, "S1: save word count");
    assert_eq!(save.to_bytes(), bytes, "S1: save round-trip");

    if source.exists(SaveRam::MODERN_SAVE_PATH) {
        let (modern, kind) = SaveRam::load(&source).unwrap();
        assert_eq!(kind, retro_format_v4::SaveFileKind::SData);
        assert_eq!(
            modern.to_bytes(),
            source.read(SaveRam::MODERN_SAVE_PATH).unwrap()
        );
    }
}
