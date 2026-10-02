//! Asset-gated integration tests against a local unpacked Sonic 1/2 asset tree.
//!
//! Run with `cargo test -p retro-image -- --ignored --nocapture`. The expected pixel and palette
//! hashes were produced independently with Pillow 12 (`Image.getdata()` for the palette indices
//! and the raw global colour table bytes for the palette).

#![forbid(unsafe_code)]

use retro_image::decode_gif;

const ASSET_ROOT: &str = "/home/ted/projects/assets";

fn blake3_hex(data: &[u8]) -> String {
    blake3::hash(data).to_hex().to_string()
}

fn palette_bytes(palette: &[[u8; 3]]) -> Vec<u8> {
    palette.iter().flatten().copied().collect()
}

fn collect_gifs(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let mut entries: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", dir.display()))
        .map(|entry| entry.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            collect_gifs(&path, out);
        } else if path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("gif"))
        {
            out.push(path);
        }
    }
}

fn decode_asset(relative: &str) -> retro_image::GifImage {
    let path = format!("{ASSET_ROOT}/{relative}");
    let bytes = std::fs::read(&path).unwrap_or_else(|error| panic!("cannot read {path}: {error}"));
    decode_gif(&bytes).unwrap_or_else(|error| panic!("cannot decode {path}: {error}"))
}

fn check(
    relative: &str,
    width: u16,
    height: u16,
    palette_len: usize,
    pixels_hash: &str,
    palette_hash: &str,
) {
    let image = decode_asset(relative);
    println!(
        "{relative}: {}x{} palette_entries={} pixels_blake3={} palette_blake3={}",
        image.width,
        image.height,
        image.palette.len(),
        blake3_hex(&image.pixels),
        blake3_hex(&palette_bytes(&image.palette)),
    );
    assert_eq!((image.width, image.height), (width, height));
    assert_eq!(image.pixels.len(), width as usize * height as usize);
    assert_eq!(image.palette.len(), palette_len);
    assert_eq!(blake3_hex(&image.pixels), pixels_hash);
    assert_eq!(blake3_hex(&palette_bytes(&image.palette)), palette_hash);
}

#[test]
#[ignore = "requires local Sonic 1/2 asset trees"]
fn all_corpus_gifs_decode() {
    let mut paths = Vec::new();
    for game in ["S1", "S2"] {
        collect_gifs(
            &std::path::Path::new(ASSET_ROOT).join(game).join("Data"),
            &mut paths,
        );
    }
    paths.sort();
    for path in &paths {
        let bytes = std::fs::read(path)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
        let image = decode_gif(&bytes)
            .unwrap_or_else(|error| panic!("cannot decode {}: {error}", path.display()));
        assert!(
            image.width > 0 && image.height > 0,
            "{} has zero dimensions",
            path.display()
        );
        assert_eq!(
            image.pixels.len(),
            image.width as usize * image.height as usize,
            "{} pixel count mismatch",
            path.display()
        );
    }
    println!(
        "decoded {} GIFs under {ASSET_ROOT}/S1/Data and {ASSET_ROOT}/S2/Data",
        paths.len()
    );
    assert_eq!(paths.len(), 158, "expected the full 158-file S1+S2 corpus");
}

#[test]
#[ignore = "requires a local Sonic 1 asset tree"]
fn s1_zone01_16x16_tiles_matches_pillow() {
    check(
        "S1/Data/Stages/Zone01/16x16Tiles.gif",
        16,
        16384,
        256,
        "8f97d37992562477653c1c5775397977c713088dd7e124eb62c06a9328ecf212",
        "faafb11a9a97b85d373fbd0be89768db9abcb15cbb723dcdbbcd0a5061be88ff",
    );
}

#[test]
#[ignore = "requires a local Sonic 1 asset tree"]
fn s1_title_sheet_matches_pillow() {
    check(
        "S1/Data/Sprites/Title/Title.gif",
        512,
        512,
        256,
        "103db25ed9fa77ccf0b2805ce498a3356519fc52b69c6bd153a50b6a918dee76",
        "55b96c3f4d618b93ce8c10efe7cc6a6b2d6a3ae2aa4982a1544316ee1bac1aa4",
    );
}

#[test]
#[ignore = "requires a local Sonic 2 asset tree"]
fn s2_title_sheet_matches_pillow() {
    check(
        "S2/Data/Sprites/Title/Title.gif",
        512,
        512,
        256,
        "33aabf47d9e9420d0c4d3c453ba77566a73781afe1bc44bbfe3d879e28e0848a",
        "2ca012901e8bf901f78ec1203a95578be82345029b20d389d106e22126f60b49",
    );
}
