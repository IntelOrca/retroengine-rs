//! Asset-gated integration tests for the WP1c scene formats.
//!
//! Run with `cargo test -p retro-format-v4 -- --ignored --nocapture`. The tests parse every
//! `Act*.bin`, `128x128Tiles.bin`, `16x16Tiles.gif`, `CollisionMasks.bin` and `Backgrounds.bin`
//! under `/home/ted/projects/assets/{S1,S2}/Data/Stages`, every `Data/Animations/*.ani` and
//! every `Data/Palettes/*.act`. JSON dumps for the representative `Zone01` stage are written to
//! `/home/ted/projects/tmp/m1-dumps/<game>/<stage>/`.
//!
//! `16x16Tiles.gif` is a 16x16384 sheet (1024 tiles) in both games. `SYZ_PalCycle.act` in S1 is
//! a 384-byte (128 colour) partial palette; every other `.act` is the full 768 bytes. Neither is
//! an error: upstream never validates the palette size.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::path::Path;

use retro_core::color::rgb888_to_rgb565;
use retro_format_v4::{
    AnimationFile, Backgrounds, CollisionMasks, Palette, Scene, TileSheet16, TileSheet128,
    palette::{PALETTE_BYTES, PALETTE_COLORS},
};
use retro_io::{DataSource, DirSource};

const ASSET_ROOT: &str = "/home/ted/projects/assets";
const DUMP_ROOT: &str = "/home/ted/projects/tmp/m1-dumps";

fn source(game: &str) -> DirSource {
    let root = format!("{ASSET_ROOT}/{game}");
    DirSource::new(&root).unwrap_or_else(|error| panic!("cannot open {root}: {error}"))
}

fn stage_folders(game: &str) -> Vec<String> {
    let stages_root = format!("{ASSET_ROOT}/{game}/Data/Stages");
    let mut folders: Vec<String> = std::fs::read_dir(&stages_root)
        .unwrap_or_else(|error| panic!("cannot list {stages_root}: {error}"))
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| Path::new(&stages_root).join(name).is_dir())
        .collect();
    folders.sort();
    folders
}

fn write_dump(game: &str, stage: &str, name: &str, value: &impl serde::Serialize) {
    let directory = Path::new(DUMP_ROOT).join(game).join(stage);
    std::fs::create_dir_all(&directory).unwrap();
    let json = serde_json::to_string_pretty(value).unwrap();
    std::fs::write(directory.join(name), json).unwrap();
}

#[test]
#[ignore = "requires the local Sonic 1/2 asset trees"]
fn scenes_parse_and_meet_layout_invariants() {
    let mut grand_acts = 0usize;
    let mut grand_entities = 0usize;
    let mut grand_tiles = 0u64;
    for game in ["S1", "S2"] {
        let source = source(game);
        let mut acts = 0usize;
        let mut entities = 0usize;
        let mut tiles = 0u64;
        let mut min_entities = usize::MAX;
        let mut max_entities = 0usize;
        let mut folders_with_scenes = 0usize;
        for folder in stage_folders(game) {
            let files = source
                .enumerate(&format!("Data/Stages/{folder}"))
                .unwrap_or_else(|error| panic!("{game}/{folder}: {error}"));
            let mut folder_acts = 0usize;
            for path in files {
                let name = path.rsplit('/').next().unwrap_or(&path);
                let lower = name.to_ascii_lowercase();
                if !lower.starts_with("act") || !lower.ends_with(".bin") {
                    continue;
                }
                let act = &name[3..name.len() - 4];

                // Strict parsing also verifies that entity parsing consumes the whole file.
                let scene = Scene::load(&format!("Data/Stages/{folder}"), act, &source)
                    .unwrap_or_else(|error| panic!("{game}/{folder}/Act{act}: {error}"));
                assert_eq!(
                    scene.layout.len(),
                    usize::from(scene.width) * usize::from(scene.height),
                    "{game}/{folder}/Act{act}: layout size"
                );
                for entity in &scene.entities {
                    assert!(
                        entity.values.len() <= 4,
                        "{game}/{folder}/Act{act}: object values"
                    );
                }
                write_dump(
                    game,
                    &folder,
                    &format!("scene-{act}.json"),
                    &serde_json::json!({
                        "act": act,
                        "scene": scene,
                    }),
                );
                folder_acts += 1;
                acts += 1;
                entities += scene.entities.len();
                tiles += scene.layout.len() as u64;
                min_entities = min_entities.min(scene.entities.len());
                max_entities = max_entities.max(scene.entities.len());
            }
            assert!(folder_acts > 0, "{game}/{folder}: no Act*.bin scenes found");
            folders_with_scenes += 1;
        }
        // Metrics only for Act files with entities; `min_entities` starts at usize::MAX when a
        // folder has scenes but no entities at all (does not happen in these trees).
        println!(
            "{game}: stages={} acts={} entities={} min_entities={} max_entities={} tiles={}",
            folders_with_scenes,
            acts,
            entities,
            min_entities.min(max_entities),
            max_entities,
            tiles
        );
        grand_acts += acts;
        grand_entities += entities;
        grand_tiles += tiles;
    }
    println!("total scenes parsed: {grand_acts} entities: {grand_entities} tiles: {grand_tiles}");
    assert!(grand_acts > 0, "no scenes parsed");
    assert!(grand_entities > 0, "no entities parsed");
}

#[test]
#[ignore = "requires the local Sonic 1/2 asset trees"]
fn stage_assets_match_upstream_layouts() {
    for game in ["S1", "S2"] {
        let source = source(game);
        let mut tiles128_total = 0usize;
        let mut collision_total = 0usize;
        let mut backgrounds_total = 0usize;
        let mut gif_tiles = 0usize;
        for folder in stage_folders(game) {
            let directory = format!("Data/Stages/{folder}");

            let tiles_path = format!("{directory}/128x128Tiles.bin");
            assert_eq!(
                source.size(&tiles_path),
                Some(retro_format_v4::tiles::TILE_SHEET_128_BYTES as u64),
                "{game}/{folder}: 128x128Tiles.bin size"
            );
            let sheet = TileSheet128::load(&directory, &source)
                .unwrap_or_else(|error| panic!("{game}/{folder}: {error}"));
            assert_eq!(
                sheet.entries.len(),
                retro_format_v4::tiles::TILE_SHEET_128_ENTRY_COUNT
            );
            assert!(
                sheet.entries.iter().all(|tile| tile.tile_index < 0x400),
                "{game}/{folder}: tile index out of range"
            );
            tiles128_total += sheet.entries.len();

            let gif = TileSheet16::load(&directory, &source)
                .unwrap_or_else(|error| panic!("{game}/{folder}: {error}"));
            assert_eq!(
                (gif.width, gif.height),
                (16, 16384),
                "{game}/{folder}: GIF size"
            );
            assert_eq!(gif.pixels.len(), 16 * 16384, "{game}/{folder}: GIF pixels");
            assert_eq!(gif.tile_count(), 1024, "{game}/{folder}: GIF tile count");
            assert_eq!(
                gif.tile(0).map(<[u8]>::len),
                Some(256),
                "{game}/{folder}: first tile"
            );
            assert!(gif.tile(1024).is_none(), "{game}/{folder}: tile overflow");
            gif_tiles += gif.tile_count();

            let collision_path = format!("{directory}/CollisionMasks.bin");
            assert_eq!(
                source.size(&collision_path),
                Some(retro_format_v4::collision::COLLISION_FILE_BYTES as u64),
                "{game}/{folder}: CollisionMasks.bin size"
            );
            let masks = CollisionMasks::load(&directory, &source)
                .unwrap_or_else(|error| panic!("{game}/{folder}: {error}"));
            assert_eq!(masks.planes[0].tiles.len(), 0x400);
            assert_eq!(masks.planes[1].tiles.len(), 0x400);
            collision_total += masks.planes[0].tiles.len() + masks.planes[1].tiles.len();

            let backgrounds_path = format!("{directory}/Backgrounds.bin");
            assert!(
                source.size(&backgrounds_path).unwrap_or(0) > 0,
                "{game}/{folder}: Backgrounds.bin missing"
            );
            let backgrounds = Backgrounds::load(&directory, &source)
                .unwrap_or_else(|error| panic!("{game}/{folder}: {error}"));
            assert_eq!(
                backgrounds.layers.len(),
                usize::from(backgrounds.layer_count),
                "{game}/{folder}: background layer count"
            );
            backgrounds_total += backgrounds.layers.len();
        }
        println!(
            "{game}: stages={} tile128_entries={} gif_tiles={} collision_tiles={} background_layers={}",
            stage_folders(game).len(),
            tiles128_total,
            gif_tiles,
            collision_total,
            backgrounds_total
        );
    }
}

#[test]
#[ignore = "requires the local Sonic 1/2 asset trees"]
fn zone01_assets_match_documented_sizes_and_dump() {
    for game in ["S1", "S2"] {
        let source = source(game);
        let directory = "Data/Stages/Zone01";
        // S1's Zone01 is Green Hill, S2's is Emerald Hill.
        let (title, width, height) = if game == "S1" {
            ("GREEN HILL", 80, 10)
        } else {
            ("EMERALD HILL", 88, 8)
        };

        let scene = Scene::load(directory, "1", &source).unwrap();
        assert_eq!(scene.title, title, "{game}: Zone01 title");
        assert_eq!(scene.width, width, "{game}: Zone01 width");
        assert_eq!(scene.height, height, "{game}: Zone01 height");
        assert_eq!(scene.active_layers[0], 1, "{game}: active layers");
        assert_eq!(scene.mid_point, 3, "{game}: mid point");
        assert_eq!(
            scene.layout.len(),
            usize::from(width) * usize::from(height),
            "{game}: Act1 layout"
        );
        write_dump(game, "Zone01", "scene-Act1.json", &scene);

        let tiles128 = TileSheet128::load(directory, &source).unwrap();
        assert_eq!(
            tiles128.entries.len(),
            0x200 * 64,
            "{game}: tile128 entries"
        );
        write_dump(game, "Zone01", "tiles128.json", &tiles128);

        let tiles16 = TileSheet16::load(directory, &source).unwrap();
        assert_eq!((tiles16.width, tiles16.height), (16, 16384));
        write_dump(game, "Zone01", "tiles16.json", &tiles16);

        let collision = CollisionMasks::load(directory, &source).unwrap();
        assert_eq!(collision.planes[0].tiles.len(), 0x400);
        write_dump(game, "Zone01", "collision.json", &collision);

        let backgrounds = Backgrounds::load(directory, &source).unwrap();
        assert_eq!(
            backgrounds.layer_count, 8,
            "{game}: Zone01 background layers"
        );
        write_dump(game, "Zone01", "backgrounds.json", &backgrounds);

        println!(
            "{game}/Zone01: scene={}x{} entities={} tiles128={} tiles16={} collision_planes={} backgrounds={}",
            scene.width,
            scene.height,
            scene.entities.len(),
            tiles128.entries.len(),
            tiles16.tile_count(),
            collision.planes.len(),
            backgrounds.layers.len()
        );
    }
}

#[test]
#[ignore = "requires the local Sonic 1/2 asset trees"]
fn animations_parse_every_file() {
    for game in ["S1", "S2"] {
        let source = source(game);
        let paths = source
            .enumerate(retro_format_v4::animation::ANIMATION_DIR)
            .unwrap_or_else(|error| panic!("{game}: {error}"));
        assert!(!paths.is_empty(), "{game}: no animations");
        let mut animations = 0usize;
        let mut frames = 0usize;
        let mut hitboxes = 0usize;
        let mut summary = BTreeMap::new();
        for path in &paths {
            assert!(
                path.to_ascii_lowercase().ends_with(".ani"),
                "{game}: unexpected animation file {path}"
            );
            let file = AnimationFile::load(path, &source)
                .unwrap_or_else(|error| panic!("{game}/{path}: {error}"));
            animations += file.animations.len();
            frames += file.total_frames();
            hitboxes += file.hitboxes.len();
            summary.insert(
                path.rsplit('/').next().unwrap_or(path).to_owned(),
                serde_json::json!({
                    "sheets": file.sheets.len(),
                    "animations": file.animations.len(),
                    "frames": file.total_frames(),
                    "hitboxes": file.hitboxes.len(),
                }),
            );
        }
        write_dump(game, "_global", "animations.json", &summary);
        println!(
            "{game}: ani_files={} animations={} frames={} hitboxes={}",
            paths.len(),
            animations,
            frames,
            hitboxes
        );
        assert!(animations > 0, "{game}: no animations parsed");
        assert!(frames > 0, "{game}: no frames parsed");
    }
}

#[test]
#[ignore = "requires the local Sonic 1/2 asset trees"]
fn palettes_parse_every_file() {
    for game in ["S1", "S2"] {
        let source = source(game);
        let paths = source
            .enumerate("Data/Palettes")
            .unwrap_or_else(|error| panic!("{game}: {error}"));
        assert!(!paths.is_empty(), "{game}: no palettes");
        let mut full = 0usize;
        let mut partial = Vec::new();
        let mut summary = BTreeMap::new();
        for path in &paths {
            let bytes = source
                .read(path)
                .unwrap_or_else(|error| panic!("{game}/{path}: {error}"));
            assert!(
                bytes.len().is_multiple_of(3),
                "{game}/{path}: palette size {} is not a multiple of 3",
                bytes.len()
            );
            assert!(
                bytes.len() <= PALETTE_BYTES,
                "{game}/{path}: palette size {} exceeds {PALETTE_BYTES}",
                bytes.len()
            );
            let palette = Palette::from_bytes(&bytes)
                .unwrap_or_else(|error| panic!("{game}/{path}: {error}"));
            assert_eq!(
                palette.len(),
                bytes.len() / 3,
                "{game}/{path}: colour count"
            );

            // Every colour must convert through the same RGB565 routine the renderer uses.
            let converted = palette.to_rgb565();
            assert_eq!(converted.len(), palette.len());
            for (index, color) in palette.colors.iter().enumerate() {
                assert_eq!(
                    Some(converted[index]),
                    palette.rgb565(index),
                    "{game}/{path}: colour {index}"
                );
                assert_eq!(
                    converted[index],
                    rgb888_to_rgb565(color[0], color[1], color[2])
                );
            }

            if path.ends_with("ElectricFlash.act") {
                assert_eq!(palette.len(), PALETTE_COLORS, "{game}/{path}: full size");
                assert_eq!(bytes.len(), PALETTE_BYTES, "{game}/{path}: 768 bytes");
                assert_eq!(palette.entry(0), Some([255, 0, 255]));
                assert_eq!(
                    palette.rgb565(0),
                    Some(0xF81F),
                    "{game}/{path}: RGB565 of entry 0"
                );
                assert_eq!(palette.stage().len(), 0x20);
            }

            if bytes.len() == PALETTE_BYTES {
                full += 1;
            } else {
                partial.push((
                    path.rsplit('/').next().unwrap_or(path).to_owned(),
                    bytes.len(),
                ));
            }
            summary.insert(
                path.rsplit('/').next().unwrap_or(path).to_owned(),
                serde_json::json!({ "bytes": bytes.len(), "colours": palette.len() }),
            );
        }
        assert!(full > 0, "{game}: no full 768-byte palettes");
        for (name, size) in &partial {
            // The only known partial palette in these trees is S1's SYZ cycle (128 colours).
            assert!(
                name.ends_with("SYZ_PalCycle.act") && *size == 384,
                "{game}: unexpected partial palette {name} ({size} bytes)"
            );
        }
        write_dump(game, "_global", "palettes.json", &summary);
        println!(
            "{game}: act_files={} full_768={} partial={:?}",
            paths.len(),
            full,
            partial
        );
    }
}
