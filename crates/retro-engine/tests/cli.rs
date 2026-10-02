//! CLI integration tests: `--list`, help/version text, actionable errors and (asset-gated)
//! `--scene GHZ --act 1` runs.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_retroengine")
}

fn run(args: &[&str]) -> Output {
    Command::new(bin())
        .args(args)
        .output()
        .expect("retroengine must be runnable")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn temp_assets(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("retro-engine-it-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
}

fn push_string(bytes: &mut Vec<u8>, value: &str) {
    bytes.push(value.len() as u8);
    bytes.extend_from_slice(value.as_bytes());
}

/// A minimal but valid `GameConfig.bin` with one Regular scene (`Zone01` / `GREEN HILL ZONE 1`).
fn game_config_bytes() -> Vec<u8> {
    let mut bytes = Vec::new();
    push_string(&mut bytes, "Sonic Test");
    push_string(&mut bytes, "cli integration");
    for _ in 0..retro_format_v4::gameconfig::PALETTE_COUNT {
        bytes.extend_from_slice(&[0, 0, 0]);
    }
    bytes.push(0); // objects
    bytes.push(0); // global variables
    bytes.push(0); // sound effects
    bytes.push(0); // players
    bytes.push(0); // Presentation
    bytes.push(1); // Regular
    push_string(&mut bytes, "Zone01");
    push_string(&mut bytes, "1");
    push_string(&mut bytes, "GREEN HILL ZONE 1");
    bytes.push(1);
    bytes.push(0); // Special
    bytes.push(0); // Bonus
    bytes
}

fn write_assets(root: &Path) {
    let game = root.join("Data").join("Game");
    std::fs::create_dir_all(&game).unwrap();
    std::fs::write(game.join("GameConfig.bin"), game_config_bytes()).unwrap();
    std::fs::write(root.join("Settings.ini"), "[Game]\ngameType=1\n").unwrap();
    let stage = root.join("Data").join("Stages").join("Zone01");
    std::fs::create_dir_all(&stage).unwrap();
    std::fs::write(stage.join("Act1.bin"), b"not parsed by --list").unwrap();
    std::fs::write(stage.join("ActB.bin"), b"not parsed by --list").unwrap();
}

#[test]
fn list_prints_categories_scenes_and_acts() {
    let root = temp_assets("list");
    write_assets(&root);
    let output = run(&[root.to_str().unwrap(), "--list"]);
    let text = stdout(&output);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(text.contains("game: Sonic Test"), "{text}");
    assert!(text.contains("[1] Regular"), "{text}");
    assert!(text.contains("GREEN HILL ZONE 1"), "{text}");
    assert!(text.contains("acts: 1, B"), "{text}");
    assert!(text.contains("--scene"), "{text}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn list_json_is_machine_readable() {
    let root = temp_assets("list-json");
    write_assets(&root);
    let output = run(&[root.to_str().unwrap(), "--list-json"]);
    let text = stdout(&output);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(text.contains("\"game\": \"Sonic Test\""), "{text}");
    assert!(text.contains("\"scene_count\": 1"), "{text}");
    assert!(text.contains("\"folder\": \"Zone01\""), "{text}");
    assert!(text.contains("\"acts\": ["), "{text}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn help_and_version_are_clear() {
    let help = run(&["--help"]);
    let text = stdout(&help);
    assert!(help.status.success(), "{}", stderr(&help));
    assert!(text.contains("EXAMPLES:"), "{text}");
    assert!(text.contains("--scene"), "{text}");
    assert!(text.contains("GHZ"), "{text}");
    assert!(text.contains("--list"), "{text}");

    let version = run(&["--version"]);
    let text = stdout(&version);
    assert!(version.status.success(), "{}", stderr(&version));
    assert!(text.contains("retroengine 0.1.0"), "{text}");
}

#[test]
fn invalid_scene_reports_candidates() {
    let root = temp_assets("bad-scene");
    write_assets(&root);
    let output = run(&[root.to_str().unwrap(), "--scene", "NOPE", "--headless"]);
    assert!(!output.status.success());
    let text = stderr(&output);
    assert!(text.contains("unknown scene 'NOPE'"), "{text}");
    assert!(text.contains("--list"), "{text}");
    let _ = std::fs::remove_dir_all(&root);
}

/// Runs the M6 acceptance command on the real asset trees.
#[test]
#[ignore = "requires assets"]
fn scene_ghz_act1_headless_runs_for_both_games() {
    let asset_root = std::env::var("RETRO_ASSETS")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/home/ted/projects/assets"));
    for game in ["S1", "S2"] {
        let root = asset_root.join(game);
        let output = run(&[
            root.to_str().unwrap(),
            "--scene",
            "GHZ",
            "--act",
            "1",
            "--headless",
            "--frames",
            "60",
            "--mute",
        ]);
        let text = stdout(&output);
        assert!(output.status.success(), "{game}: {}", stderr(&output));
        assert!(text.contains("scene: Zone01 act 1"), "{game}: {text}");
        assert!(text.contains("hash: "), "{game}: {text}");
    }
}
