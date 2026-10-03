//! Developer utility entry point for engine diagnostics and tooling.
#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};

/// Developer utility for engine diagnostics and tooling.
#[derive(Debug, Parser)]
#[command(
    name = "retro-dev",
    version,
    about = "Retro Engine developer utilities"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Writes a tiny synthetic RSDK v4 asset folder (no game assets) used by CI smoke runs.
    ///
    /// The folder contains a `Settings.ini`, one `GameConfig.bin`, one `Zone01` stage with a
    /// single entity and object script, and one generated WAV sound effect, so it boots in
    /// seconds headlessly or in a window.
    GenSynthetic {
        /// Directory to write the asset folder into (parents are created as needed).
        dir: PathBuf,
    },
}

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::GenSynthetic { dir } => match write_synthetic(&dir) {
            Ok(()) => {
                println!("wrote synthetic assets to {}", dir.display());
                std::process::ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("error: {error}");
                std::process::ExitCode::FAILURE
            }
        },
    }
}

/// `Settings.ini` for the synthetic folder: Origins data (`gameType=1`) and the shipped
/// windowed/bordered/vsync defaults.
const SETTINGS_INI: &str = "\
[Game]\n\
gameType=1\n\
txtScripts=n\n\
[Video]\n\
windowed=y\n\
border=y\n\
exclusiveFS=n\n\
vsync=y\n\
";

/// Object script: records update counts, exercises the RNG and plays the generated sound effect.
const OBJECT_SOURCE: &str = "\
event ObjectStartup\n\
    object.value0 = 0\n\
end event\n\
event ObjectUpdate\n\
    object.value0 += 1\n\
    object.value1 = object.value0\n\
    Rand(object.value2, 100)\n\
    PlaySfx(0, false)\n\
end event\n\
";

fn push_string(bytes: &mut Vec<u8>, value: &str) {
    bytes.push(value.len() as u8);
    bytes.extend_from_slice(value.as_bytes());
}

/// One `GameConfig.bin`: one global object, one global variable, one global SFX and one
/// presentation scene (`Zone01` / `TEST ZONE`).
fn game_config_bytes() -> Vec<u8> {
    let mut bytes = Vec::new();
    push_string(&mut bytes, "Synthetic");
    push_string(&mut bytes, "retro-dev smoke data");
    for _ in 0..retro_format_v4::gameconfig::PALETTE_COUNT {
        bytes.extend_from_slice(&[0, 0, 0]);
    }
    bytes.push(1); // objects
    push_string(&mut bytes, "Test Object");
    push_string(&mut bytes, "Test/TestObject.txt");
    bytes.push(1); // global variables
    push_string(&mut bytes, "drawOrder");
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

/// `StageConfig.bin` that loads the global objects and adds none of its own.
fn stage_config_bytes() -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.push(1); // load global objects
    for _ in 0..retro_format_v4::stageconfig::STAGE_PALETTE_COUNT {
        bytes.extend_from_slice(&[0, 0, 0]);
    }
    bytes.push(0); // sound effects
    bytes.push(0); // objects
    bytes
}

/// `Act1.bin`: a 1x1 stage with the single global object at (64, 64).
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
    bytes.extend_from_slice(&1u16.to_le_bytes()); // entities
    let attributes = retro_format_v4::scene::ENTITY_ATTRIB_STATE
        | retro_format_v4::scene::ENTITY_ATTRIB_DIRECTION
        | retro_format_v4::scene::ENTITY_ATTRIB_VALUES[0];
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

/// A short 16-bit stereo 44.1 kHz WAV so the windowed smoke run actually mixes and submits audio.
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

/// Writes the synthetic asset folder rooted at `root`, creating directories as needed.
///
/// The folder is self-contained (no game assets) and is what the CI smoke steps run.
pub fn write_synthetic(root: &Path) -> std::io::Result<()> {
    let write = |path: &str, bytes: &[u8]| -> std::io::Result<()> {
        let full = root.join(path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(full, bytes)
    };
    write("Settings.ini", SETTINGS_INI.as_bytes())?;
    write("Data/Game/GameConfig.bin", &game_config_bytes())?;
    write("Data/Stages/Zone01/StageConfig.bin", &stage_config_bytes())?;
    write("Data/Stages/Zone01/Act1.bin", &scene_bytes())?;
    write("Data/Scripts/Test/TestObject.txt", OBJECT_SOURCE.as_bytes())?;
    write("Data/SoundFX/Test/Beep.wav", &wav_bytes())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn temp_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("retro-dev-{name}-{}", std::process::id()))
    }

    #[test]
    fn gen_synthetic_writes_a_bootable_folder() {
        let root = temp_root("synthetic");
        let _ = std::fs::remove_dir_all(&root);
        write_synthetic(&root).unwrap();
        for path in [
            "Settings.ini",
            "Data/Game/GameConfig.bin",
            "Data/Stages/Zone01/StageConfig.bin",
            "Data/Stages/Zone01/Act1.bin",
            "Data/Scripts/Test/TestObject.txt",
            "Data/SoundFX/Test/Beep.wav",
        ] {
            assert!(root.join(path).is_file(), "{path} must be written");
        }

        let source = Arc::new(retro_io::DirSource::new(&root).unwrap());
        let mut engine =
            retro_engine::Engine::load_default(source, Some("TEST ZONE"), Some("1")).unwrap();
        engine.run_frames(5, false).unwrap();
        assert_eq!(engine.state.frame, 5);
        assert_eq!(
            engine.state.audio.mixer().sfx_count(),
            1,
            "the generated WAV must load through PlaySfx"
        );
        assert!(
            engine
                .op_histogram()
                .get("PlaySfx")
                .is_some_and(|count| *count >= 5)
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
