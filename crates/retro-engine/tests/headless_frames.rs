use retro_engine::placeholder_frame;
use retro_platform::headless::HeadlessPlatform;
use retro_platform::{InputState, Platform, WindowDesc};

fn hash_frame(pixels: &[u16]) -> String {
    let bytes: Vec<u8> = pixels
        .iter()
        .flat_map(|pixel| pixel.to_le_bytes())
        .collect();
    blake3::hash(&bytes).to_hex().to_string()
}

#[test]
fn headless_three_frames_have_stable_hash() {
    let mut platform = HeadlessPlatform::new();
    platform.init().unwrap();
    let mut window = platform
        .create_window(WindowDesc::new("parity", 320, 224))
        .unwrap();
    let frame = placeholder_frame(320, 224);

    let mut hashes = Vec::new();
    for _ in 0..3 {
        window.present(&frame, 320, 224).unwrap();
        platform.clock().advance_frame();
        hashes.push(hash_frame(&window.readback().unwrap()));
    }

    assert_eq!(hashes[0], hashes[1]);
    assert_eq!(hashes[1], hashes[2]);
    assert_eq!(
        hashes[2],
        "bcefc0086ac4bec70bbc3af5406f71bbdeb76bd931d843d7299c3129a5982666"
    );
    assert_eq!(platform.frames_advanced(), 3);
    assert_eq!(retro_platform::sdl3_init_count(), 0);
}

#[test]
fn headless_scripted_input_is_deterministic() {
    let mut pressed = InputState::new();
    pressed.right = true;
    pressed.a = true;
    let mut platform = HeadlessPlatform::new().with_input_script(vec![pressed]);
    platform.init().unwrap();

    let first = platform.input().poll();
    platform.clock().advance_frame();
    let second = platform.input().poll();
    assert_eq!(first, pressed);
    assert_eq!(second, pressed);
    assert_eq!(first.version, InputState::VERSION);
}
