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

/// A minimal but valid `StageConfig.bin` with no global objects.
fn stage_config_bytes() -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.push(0); // load_global_objects
    for _ in 0..retro_format_v4::stageconfig::STAGE_PALETTE_COUNT {
        bytes.extend_from_slice(&[0, 0, 0]);
    }
    bytes.push(0); // sound effects
    bytes.push(0); // objects
    bytes
}

/// A minimal valid act file: one tile, no entities.
fn scene_bytes() -> Vec<u8> {
    let mut bytes = Vec::new();
    push_string(&mut bytes, "TEST");
    bytes.extend_from_slice(&[9; retro_format_v4::scene::ACTIVE_LAYER_COUNT]);
    bytes.push(3); // midpoint
    bytes.push(1); // width
    bytes.push(0);
    bytes.push(1); // height
    bytes.push(0);
    bytes.extend_from_slice(&0u16.to_le_bytes()); // chunks
    bytes.extend_from_slice(&0u16.to_le_bytes()); // entities
    bytes
}

fn write_assets(root: &Path) {
    let game = root.join("Data").join("Game");
    std::fs::create_dir_all(&game).unwrap();
    std::fs::write(game.join("GameConfig.bin"), game_config_bytes()).unwrap();
    std::fs::write(
        root.join("Settings.ini"),
        "[Game]\ngameType=1\n[Video]\nwindowed=y\nborder=y\nvsync=y\n",
    )
    .unwrap();
    let stage = root.join("Data").join("Stages").join("Zone01");
    std::fs::create_dir_all(&stage).unwrap();
    std::fs::write(stage.join("StageConfig.bin"), stage_config_bytes()).unwrap();
    std::fs::write(stage.join("Act1.bin"), scene_bytes()).unwrap();
    std::fs::write(stage.join("ActB.bin"), scene_bytes()).unwrap();
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

/// Runs the engine with the SDL dummy drivers and fails the test if it does not exit in time.
#[cfg(unix)]
fn run_bounded(args: &[&str], timeout: std::time::Duration) -> Output {
    run_bounded_with_env(args, timeout, &[])
}

/// Like [`run_bounded`] but lets the caller override individual SDL environment variables.
#[cfg(unix)]
fn run_bounded_with_env(
    args: &[&str],
    timeout: std::time::Duration,
    env: &[(&str, &str)],
) -> Output {
    use std::process::Stdio;
    use std::time::Instant;

    let mut child = Command::new(bin())
        .args(args)
        .env("SDL_VIDEO_DRIVER", "dummy")
        .env("SDL_VIDEODRIVER", "dummy")
        .env("SDL_AUDIODRIVER", "dummy")
        .envs(env.iter().copied())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn retroengine");
    let deadline = Instant::now() + timeout;
    loop {
        if child.try_wait().expect("try_wait").is_some() {
            return child.wait_with_output().expect("wait_with_output");
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("windowed run did not exit within {timeout:?}");
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

/// A windowed run must create its window, present exactly `--frames` frames and exit 0,
/// reporting the selected platform, video driver and audio device. The dummy window is never
/// focused and never receives a close event, so the `--frames` cap alone must terminate it.
#[cfg(unix)]
#[test]
fn windowed_frames_run_exits_promptly() {
    use std::time::Duration;

    let root = temp_assets("windowed-frames");
    write_assets(&root);
    let user_dir = root.join("user");
    let output = run_bounded(
        &[
            root.to_str().unwrap(),
            "--frames",
            "5",
            "--user-dir",
            user_dir.to_str().unwrap(),
        ],
        Duration::from_secs(30),
    );
    let text = stdout(&output);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(text.contains("platform: standalone"), "{text}");
    assert!(text.contains("presented-frames: 5"), "{text}");
    assert!(text.contains("hash: "), "{text}");
    assert!(text.contains("audio: 44100 Hz stereo f32"), "{text}");
    assert!(
        text.contains("audio: submitted ") && text.contains("underrun(s)"),
        "windowed runs must report audio flow-control diagnostics: {text}"
    );
    assert!(text.contains("video: dummy"), "{text}");
    let _ = std::fs::remove_dir_all(&root);
}

/// A headless run with no explicit frame cap must not stop on a hidden 600-frame default: it
/// keeps running past 600 frames and exits cleanly on SIGTERM, printing the normal summary.
#[cfg(unix)]
#[test]
fn headless_unbounded_run_exits_cleanly_on_signal() {
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    let root = temp_assets("headless-unbounded");
    write_assets(&root);

    for frames_args in [Vec::<&str>::new(), vec!["--frames", "0"]] {
        let mut child = Command::new(bin())
            .arg(&root)
            .arg("--headless")
            .arg("--mute")
            .args(&frames_args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn retroengine");

        // The synthetic asset load is near-instant, so the run is still going after this sleep.
        std::thread::sleep(Duration::from_secs(2));
        let killed = Command::new("kill")
            .arg("-TERM")
            .arg(child.id().to_string())
            .status()
            .expect("send SIGTERM");
        assert!(killed.success(), "kill -TERM failed");

        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if child.try_wait().expect("try_wait").is_some() {
                break;
            }
            if Instant::now() > deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("headless run did not exit after SIGTERM (args: {frames_args:?})");
            }
            std::thread::sleep(Duration::from_millis(50));
        }

        let output = child.wait_with_output().expect("wait_with_output");
        let text = stdout(&output);
        assert!(
            output.status.success(),
            "{frames_args:?}: {}",
            stderr(&output)
        );
        assert!(
            text.contains("frames: until quit (Ctrl-C/SIGTERM)"),
            "{text}"
        );
        assert!(text.contains("quit: signal"), "{text}");
        assert!(text.contains("hash: "), "{text}");
        let executed: u64 = text
            .lines()
            .find_map(|line| line.strip_prefix("executed-frames: "))
            .and_then(|value| value.trim().parse().ok())
            .expect("summary must report executed-frames");
        assert!(
            executed > 600,
            "headless default must not stop at 600 frames (ran {executed})"
        );
    }
    let _ = std::fs::remove_dir_all(&root);
}

/// A windowed scripted replay with a frame cap must exit on its own (no quit signal needed),
/// proving the SDL event pump and the audio path do not block the frame loop.
#[cfg(unix)]
#[test]
fn windowed_scripted_input_with_frames_exits_promptly() {
    use std::time::Duration;

    let root = temp_assets("windowed-input-frames");
    write_assets(&root);
    let script = root.join("input.txt");
    std::fs::write(
        &script,
        "retro-input 1\n\
         0 - 0 0 -  - 0 0 -  - 0 0 -  - 0 0 -\n\
         1 A 0 0 -  - 0 0 -  - 0 0 -  - 0 0 -\n",
    )
    .unwrap();
    let output = run_bounded(
        &[
            root.to_str().unwrap(),
            "--input",
            script.to_str().unwrap(),
            "--frames",
            "5",
            "--user-dir",
            root.join("user").to_str().unwrap(),
        ],
        Duration::from_secs(30),
    );
    let text = stdout(&output);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(text.contains("presented-frames: 5"), "{text}");
    let _ = std::fs::remove_dir_all(&root);
}

/// A failed load must exit with an error before any window is created, so a bad asset tree can
/// never leave a frozen window behind.
#[cfg(unix)]
#[test]
fn windowed_load_failure_exits_without_a_frozen_window() {
    use std::time::Duration;

    let root = temp_assets("windowed-bad-scene");
    write_assets(&root);
    let output = run_bounded(
        &[
            root.to_str().unwrap(),
            "--scene",
            "NOPE",
            "--user-dir",
            root.join("user").to_str().unwrap(),
        ],
        Duration::from_secs(30),
    );
    assert!(!output.status.success());
    let text = stderr(&output);
    assert!(text.contains("unknown scene 'NOPE'"), "{text}");
    let _ = std::fs::remove_dir_all(&root);
}

/// A missing audio device is reported and the windowed game keeps running (silent run).
#[cfg(unix)]
#[test]
fn windowed_audio_failure_is_reported_but_does_not_abort() {
    use std::time::Duration;

    let root = temp_assets("windowed-no-audio");
    write_assets(&root);
    let output = run_bounded_with_env(
        &[
            root.to_str().unwrap(),
            "--frames",
            "2",
            "--user-dir",
            root.join("user").to_str().unwrap(),
        ],
        Duration::from_secs(30),
        &[("SDL_AUDIODRIVER", "no-such-audio-driver")],
    );
    let text = stdout(&output);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(text.contains("audio: unavailable:"), "{text}");
    assert!(text.contains("presented-frames: 2"), "{text}");
    let _ = std::fs::remove_dir_all(&root);
}

/// Origins data (`gameType=1` in `Settings.ini`) compiles as standalone by default; `--origins`
/// opts back into the Origins platform blocks.
#[test]
fn origins_flag_selects_the_script_platform() {
    let root = temp_assets("origins-flag");
    write_assets(&root);
    let default = run(&[
        root.to_str().unwrap(),
        "--headless",
        "--frames",
        "2",
        "--mute",
    ]);
    assert!(default.status.success(), "{}", stderr(&default));
    assert!(
        stdout(&default).contains("platform: standalone"),
        "{}",
        stdout(&default)
    );

    let origins = run(&[
        root.to_str().unwrap(),
        "--headless",
        "--frames",
        "2",
        "--mute",
        "--origins",
    ]);
    assert!(origins.status.success(), "{}", stderr(&origins));
    assert!(
        stdout(&origins).contains("platform: origins"),
        "{}",
        stdout(&origins)
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// The `--input` replay's `seed` header seeds the run when `--seed` is omitted; an explicit
/// `--seed` overrides it and a missing header (or no input) keeps the built-in default.
#[test]
fn replay_seed_header_is_used_unless_seed_flag_overrides_it() {
    let root = temp_assets("replay-seed");
    write_assets(&root);
    let replay = root.join("input.txt");
    std::fs::write(
        &replay,
        "retro-input 1\n\
         seed 12345\n\
         0 - 0 0 -  - 0 0 -  - 0 0 -  - 0 0 -\n",
    )
    .unwrap();

    let input_args = [
        root.to_str().unwrap(),
        "--headless",
        "--frames",
        "1",
        "--mute",
        "--input",
        replay.to_str().unwrap(),
    ];
    let header = run(&input_args);
    assert!(header.status.success(), "{}", stderr(&header));
    assert!(
        stdout(&header).contains("seed: 12345"),
        "{}",
        stdout(&header)
    );

    let with_override = run(&[
        root.to_str().unwrap(),
        "--headless",
        "--frames",
        "1",
        "--mute",
        "--input",
        replay.to_str().unwrap(),
        "--seed",
        "7",
    ]);
    assert!(with_override.status.success(), "{}", stderr(&with_override));
    assert!(
        stdout(&with_override).contains("seed: 7"),
        "{}",
        stdout(&with_override)
    );

    // A replay without a seed header leaves the built-in default in place.
    std::fs::write(
        &replay,
        "retro-input 1\n\
         0 - 0 0 -  - 0 0 -  - 0 0 -  - 0 0 -\n",
    )
    .unwrap();
    let headerless = run(&input_args);
    assert!(headerless.status.success(), "{}", stderr(&headerless));
    assert!(
        stdout(&headerless).contains(&format!("seed: {}", retro_engine::rng::DEFAULT_SEED)),
        "{}",
        stdout(&headerless)
    );

    // No `--input` at all keeps the same default.
    let no_input = run(&[
        root.to_str().unwrap(),
        "--headless",
        "--frames",
        "1",
        "--mute",
    ]);
    assert!(no_input.status.success(), "{}", stderr(&no_input));
    assert!(
        stdout(&no_input).contains(&format!("seed: {}", retro_engine::rng::DEFAULT_SEED)),
        "{}",
        stdout(&no_input)
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// A windowed run with scripted input must still pump SDL events so a quit signal can close
/// it. SDL turns SIGTERM into a quit event, exactly like the window close button.
#[cfg(unix)]
#[test]
fn windowed_scripted_input_exits_on_quit_event() {
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    let root = temp_assets("windowed-input");
    write_assets(&root);
    let script = root.join("input.txt");
    std::fs::write(
        &script,
        "retro-input 1\n\
         0 - 0 0 -  - 0 0 -  - 0 0 -  - 0 0 -\n\
         1 A 0 0 -  - 0 0 -  - 0 0 -  - 0 0 -\n",
    )
    .unwrap();

    let mut child = Command::new(bin())
        .arg(&root)
        .arg("--input")
        .arg(&script)
        .arg("--mute")
        .arg("--user-dir")
        .arg(root.join("user"))
        .env("SDL_VIDEO_DRIVER", "dummy")
        .env("SDL_AUDIODRIVER", "dummy")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn retroengine");

    std::thread::sleep(Duration::from_secs(2));
    let killed = Command::new("kill")
        .arg("-TERM")
        .arg(child.id().to_string())
        .status()
        .expect("send SIGTERM");
    assert!(killed.success(), "kill -TERM failed");

    let deadline = Instant::now() + Duration::from_secs(15);
    let status = loop {
        if let Some(status) = child.try_wait().expect("try_wait") {
            break status;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = std::fs::remove_dir_all(&root);
            panic!("windowed --input run did not exit after SIGTERM (events not pumped?)");
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(
        status.success(),
        "windowed --input run must exit cleanly, got {status}"
    );
    let _ = std::fs::remove_dir_all(&root);
}
