//! Asset-gated runs over the real S1/S2 sound effects and music.
//!
//! These tests are ignored by default because they need the game assets. Run them with:
//! `cargo test -p retro-audio -- --ignored --nocapture` (set `RETRO_ASSETS` to override the
//! default `/home/ted/projects/assets` location).

use std::fs;
use std::path::{Path, PathBuf};

use retro_audio::{Mixer, SAMPLE_RATE};

fn assets_root() -> Option<PathBuf> {
    let root = std::env::var_os("RETRO_ASSETS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/home/ted/projects/assets"));
    root.is_dir().then_some(root)
}

fn collect(dir: &Path, extension: &str, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, extension, out);
        } else if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case(extension))
        {
            out.push(path);
        }
    }
}

#[test]
#[ignore = "requires the S1/S2 asset folders (RETRO_ASSETS or /home/ted/projects/assets)"]
fn loads_and_mixes_every_sound_effect() {
    let Some(root) = assets_root() else {
        eprintln!("assets not found; skipping");
        return;
    };

    for game in ["S1", "S2"] {
        let dir = root.join(game).join("Data").join("SoundFX");
        let mut files = Vec::new();
        collect(&dir, "wav", &mut files);
        collect(&dir, "ogg", &mut files);
        files.sort();

        let mut mixer = Mixer::new();
        let mut wav_count = 0usize;
        let mut ogg_count = 0usize;
        let mut total_frames = 0usize;
        let mut overall = blake3::Hasher::new();

        for path in &files {
            let bytes =
                fs::read(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            let id = mixer
                .load_sfx(&name, &bytes)
                .unwrap_or_else(|error| panic!("load {}: {error}", path.display()));
            let frames = mixer.sfx_frames(id).expect("loaded sfx has frames");
            total_frames += frames;
            match path.extension().and_then(|ext| ext.to_str()) {
                Some(ext) if ext.eq_ignore_ascii_case("wav") => wav_count += 1,
                _ => ogg_count += 1,
            }

            mixer.play_sfx(id, 100, 0);
            let mut out = vec![0.0f32; 32];
            let hash = mixer.mix_frame(&mut out, 16);
            assert!(out.iter().all(|sample| sample.is_finite()));
            mixer.stop_all_sfx();
            overall.update(&hash);
        }

        let seconds = total_frames as f64 / f64::from(SAMPLE_RATE);
        println!(
            "{game}: {wav_count} wav + {ogg_count} ogg sfx, {total_frames} stereo frames \
             ({seconds:.2}s decoded), combined hash {}",
            overall.finalize().to_hex()
        );
        assert_eq!(wav_count + ogg_count, files.len());
        assert!(!files.is_empty());
        assert!(total_frames > 0);
    }
}

#[test]
#[ignore = "requires the S1/S2 asset folders (RETRO_ASSETS or /home/ted/projects/assets)"]
fn plays_a_music_stream() {
    let Some(root) = assets_root() else {
        eprintln!("assets not found; skipping");
        return;
    };
    let path = root
        .join("S1")
        .join("Data")
        .join("Music")
        .join("GreenHill.ogg");
    let bytes = fs::read(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));

    let mut mixer = Mixer::new();
    let id = mixer
        .load_stream(bytes)
        .unwrap_or_else(|error| panic!("load {}: {error}", path.display()));
    let frames = mixer.stream_frames(id).expect("loaded stream has frames");
    let rate = mixer
        .stream_sample_rate(id)
        .expect("loaded stream has a rate");
    let seconds = frames as f64 / f64::from(rate);
    mixer.play_stream(id, 0);
    let mut out = vec![0.0f32; 600 * 2];
    let hash = mixer.mix_frame(&mut out, 600);

    println!(
        "{}: {frames} source frames @ {rate} Hz ({seconds:.2}s decoded), 600-frame hash {}",
        path.display(),
        hex(&hash)
    );
    assert!(frames > 0);
    assert!(out.iter().all(|sample| sample.is_finite()));
    assert!(out.iter().any(|sample| *sample != 0.0));
}

#[test]
#[ignore = "requires the S1/S2 asset folders (RETRO_ASSETS or /home/ted/projects/assets)"]
fn loads_every_music_stream() {
    let Some(root) = assets_root() else {
        eprintln!("assets not found; skipping");
        return;
    };
    for game in ["S1", "S2"] {
        let dir = root.join(game).join("Data").join("Music");
        let mut files = Vec::new();
        collect(&dir, "ogg", &mut files);
        files.sort();
        let mut mixer = Mixer::new();
        let mut total_frames = 0usize;
        for path in &files {
            let bytes =
                fs::read(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
            let id = mixer
                .load_stream(bytes)
                .unwrap_or_else(|error| panic!("load {}: {error}", path.display()));
            total_frames += mixer.stream_frames(id).expect("loaded stream has frames");
        }
        println!(
            "{game}: {} music streams, {total_frames} source frames",
            files.len()
        );
        assert!(!files.is_empty());
    }
}

fn hex(hash: &[u8; 32]) -> String {
    hash.iter().map(|byte| format!("{byte:02x}")).collect()
}
