//! Asset-gated tests for the real Sonic 1/2 `Settings.ini` keyboard maps.
//!
//! Run with `cargo test -p retro-input -- --ignored --nocapture`.

#![forbid(unsafe_code)]

use retro_format_v4::Settings;
use retro_input::{Button, InputMappings, PLAYER_COUNT};

const ASSET_ROOT: &str = "/home/ted/projects/assets";

fn load(game: &str) -> Settings {
    let path = format!("{ASSET_ROOT}/{game}/Settings.ini");
    let bytes = std::fs::read(&path).unwrap_or_else(|error| panic!("cannot read {path}: {error}"));
    Settings::from_bytes(&bytes).unwrap_or_else(|error| panic!("cannot parse {path}: {error}"))
}

#[test]
#[ignore = "requires the local Sonic 1/2 asset trees"]
fn slot_one_matchers_origins_defaults() {
    for game in ["S1", "S2"] {
        let settings = load(game);
        let map = &settings.keyboard_maps[0];
        assert_eq!(map.up, Some(0x26), "{game}: up is VK_UP");
        assert_eq!(map.down, Some(0x28), "{game}: down is VK_DOWN");
        assert_eq!(map.left, Some(0x25), "{game}: left is VK_LEFT");
        assert_eq!(map.right, Some(0x27), "{game}: right is VK_RIGHT");
        assert_eq!(map.button_a, Some(0x41), "{game}: buttonA is 'A'");
        assert_eq!(map.button_b, Some(0x53), "{game}: buttonB is 'S'");
        assert_eq!(map.button_c, Some(0x44), "{game}: buttonC is 'D'");
        assert_eq!(map.button_x, Some(0x51), "{game}: buttonX is 'Q'");
        assert_eq!(map.button_y, Some(0x57), "{game}: buttonY is 'W'");
        assert_eq!(map.button_z, Some(0x45), "{game}: buttonZ is 'E'");
        assert_eq!(map.start, Some(0x0D), "{game}: start is Return");
        assert_eq!(map.select, Some(0x09), "{game}: select is Tab");

        let mappings = InputMappings::from_settings(&settings);
        assert_eq!(mappings.scancode_for(0, Button::Up), Some(82), "{game}: up");
        assert_eq!(mappings.scancode_for(0, Button::A), Some(4), "{game}: A");
        assert_eq!(mappings.scancode_for(0, Button::B), Some(22), "{game}: B");
        assert_eq!(mappings.scancode_for(0, Button::C), Some(7), "{game}: C");
        assert_eq!(mappings.scancode_for(0, Button::X), Some(20), "{game}: X");
        assert_eq!(mappings.scancode_for(0, Button::Y), Some(26), "{game}: Y");
        assert_eq!(mappings.scancode_for(0, Button::Z), Some(8), "{game}: Z");
        assert_eq!(
            mappings.scancode_for(0, Button::Start),
            Some(40),
            "{game}: start is SDL_SCANCODE_RETURN"
        );
        assert_eq!(
            mappings.scancode_for(0, Button::Select),
            Some(43),
            "{game}: select is SDL_SCANCODE_TAB"
        );
    }
}

#[test]
#[ignore = "requires the local Sonic 1/2 asset trees"]
fn reports_all_keyboard_maps() {
    for game in ["S1", "S2"] {
        let settings = load(game);
        let mappings = InputMappings::from_settings(&settings);
        println!("{game}: {} keyboard maps", settings.keyboard_maps.len());
        for slot in 0..PLAYER_COUNT {
            let bindings: Vec<String> = Button::ALL
                .into_iter()
                .filter_map(|button| {
                    mappings
                        .scancode_for(slot as u8, button)
                        .map(|scancode| format!("{}={scancode}", button.name()))
                })
                .collect();
            println!("  slot {}: {}", slot + 1, bindings.join(" "));
        }
        assert_eq!(settings.keyboard_maps.len(), 4, "{game}: four maps");
    }
}
