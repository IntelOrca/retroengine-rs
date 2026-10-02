//! GIF decoder matching RSDKv4's `LoadGIFFile` / `ReadGifPictureData` state machine.
//!
//! Behavioural notes on the upstream decoder that this port preserves:
//!
//! * Only the first image of the file is decoded; animation frames are ignored because RSDK
//!   spritesheets and tile sheets are single-frame GIFs.
//! * The global colour table is read according to the low three bits of the logical screen
//!   descriptor's packed field (`1 << ((packed & 7) + 1)` entries) even when the table-present
//!   flag is clear; the table-present flag itself is ignored. Palettes are returned to the
//!   caller even though upstream discards them.
//! * The image separator is located by scanning forward for a literal `,` byte, which skips any
//!   extension blocks such as graphic control extensions. Transparent-colour indices are
//!   therefore not tracked: upstream leaves the raw palette index in place.
//! * Image descriptor origin and size fields are ignored in favour of the logical screen size.
//! * If a local colour table is present the decoder skips a fixed 128 entries (384 bytes),
//!   regardless of the size field, exactly as upstream does.
//! * Interlaced images are de-interlaced into logical row order using upstream's four passes.
//! * The LZW table is seeded with `(u8)NO_SUCH_CODE` (= 2) on init but with the full sentinel on
//!   clear codes, and the code width grows from a per-code-read counter rather than the emitted
//!   code value.
//!
//! Deviations from upstream are limited to input validation: a missing `GIF` signature is
//! rejected instead of being ignored, images larger than the engine's 4 MiB `GFXDATA_SIZE` pixel
//! buffer are rejected instead of being skipped, and a truncated LZW sub-block chain reports
//! `Truncated`. A cleanly terminated stream that ends before all pixels are produced leaves the
//! remaining pixels at index 0, matching upstream.

use crate::ImageError;

/// Maximum LZW code value.
const LZ_MAX_CODE: i32 = 4095;
/// Maximum LZW code width in bits.
const LZ_BITS: u32 = 12;
/// First code that cannot be written to the table.
const FIRST_CODE: i32 = 4097;
/// Sentinel for an undefined prefix entry.
const NO_SUCH_CODE: i32 = 4098;
/// `(u8)NO_SUCH_CODE` as written by the upstream initialiser.
const NO_SUCH_CODE_BYTE: i32 = NO_SUCH_CODE & 0xFF;
/// Size of the engine's software-renderer pixel buffer (`Drawing.hpp`).
const GFXDATA_SIZE: usize = 0x800 * 0x800;

const CODE_MASKS: [u32; 13] = [0, 1, 3, 7, 15, 31, 63, 127, 255, 511, 1023, 2047, 4095];

/// A decoded single-frame GIF in indexed colour.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GifImage {
    /// Image width in pixels.
    pub width: u16,
    /// Image height in pixels.
    pub height: u16,
    /// `width * height` palette indices in row-major order.
    pub pixels: Vec<u8>,
    /// Global colour table entries in RGB888 order.
    pub palette: Vec<[u8; 3]>,
}

struct ByteStream<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl ByteStream<'_> {
    fn read_u8(&mut self) -> Result<u8, ImageError> {
        let byte = *self.bytes.get(self.pos).ok_or(ImageError::Truncated)?;
        self.pos += 1;
        Ok(byte)
    }

    fn read_u16(&mut self) -> Result<u16, ImageError> {
        let low = self.read_u8()? as u16;
        let high = self.read_u8()? as u16;
        Ok(low | (high << 8))
    }
}

/// Decodes the first frame of `bytes` as an indexed GIF.
pub fn decode_gif(bytes: &[u8]) -> Result<GifImage, ImageError> {
    let mut stream = ByteStream { bytes, pos: 0 };

    let mut signature = [0u8; 6];
    for byte in &mut signature {
        *byte = stream.read_u8()?;
    }
    if &signature[..3] != b"GIF" {
        return Err(ImageError::Invalid("missing GIF signature".to_string()));
    }

    let width = stream.read_u16()?;
    let height = stream.read_u16()?;
    let packed = stream.read_u8()?;
    let palette_entries = 1usize << ((packed & 0x07) as u32 + 1);
    let _background_index = stream.read_u8()?;
    let _pixel_aspect = stream.read_u8()?;

    let mut palette = Vec::with_capacity(palette_entries);
    for _ in 0..palette_entries {
        let red = stream.read_u8()?;
        let green = stream.read_u8()?;
        let blue = stream.read_u8()?;
        palette.push([red, green, blue]);
    }

    loop {
        if stream.read_u8()? == 0x2C {
            break;
        }
    }

    let _left = stream.read_u16()?;
    let _top = stream.read_u16()?;
    let _image_width = stream.read_u16()?;
    let _image_height = stream.read_u16()?;
    let image_packed = stream.read_u8()?;
    let interlaced = image_packed & 0x40 != 0;

    if image_packed & 0x80 != 0 {
        stream.pos = stream
            .pos
            .checked_add(128 * 3)
            .ok_or(ImageError::Truncated)?;
        if stream.pos > bytes.len() {
            return Err(ImageError::Truncated);
        }
    }

    let mut decoder = LzwDecoder::new(bytes, stream.pos)?;
    let pixel_count = (width as usize)
        .checked_mul(height as usize)
        .ok_or_else(|| ImageError::Invalid("image dimensions overflow".to_string()))?;
    if pixel_count > GFXDATA_SIZE {
        return Err(ImageError::Unsupported(format!(
            "image of {width}x{height} exceeds the engine's {GFXDATA_SIZE}-byte graphics buffer"
        )));
    }
    let mut pixels = vec![0u8; pixel_count];
    let row_width = width as usize;

    if interlaced {
        const ROW_START: [u16; 4] = [0, 4, 2, 1];
        const ROW_STEP: [u16; 4] = [8, 8, 4, 2];
        for pass in 0..4 {
            let mut y = ROW_START[pass];
            while y < height {
                decoder.read_line(&mut pixels, y as usize * row_width, row_width);
                y += ROW_STEP[pass];
            }
        }
    } else {
        for y in 0..height as usize {
            decoder.read_line(&mut pixels, y * row_width, row_width);
        }
    }

    if decoder.truncated {
        return Err(ImageError::Truncated);
    }
    if decoder.overflow {
        return Err(ImageError::Invalid("LZW stack overflow".to_string()));
    }

    Ok(GifImage {
        width,
        height,
        pixels,
        palette,
    })
}

struct LzwDecoder<'a> {
    bytes: &'a [u8],
    pos: usize,
    block: [u8; 256],
    block_len: usize,
    block_pos: usize,
    completed: bool,
    truncated: bool,
    overflow: bool,
    depth: u8,
    clear_code: i32,
    eof_code: i32,
    running_code: i32,
    running_bits: u32,
    max_code_plus_one: i32,
    prev_code: i32,
    shift_state: u32,
    shift_data: u32,
    stack: [u8; 4096],
    stack_ptr: usize,
    suffix: [u8; 4096],
    prefix: [i32; 4096],
}

impl<'a> LzwDecoder<'a> {
    fn new(bytes: &'a [u8], pos: usize) -> Result<Self, ImageError> {
        let depth = *bytes.get(pos).ok_or(ImageError::Truncated)?;
        if depth > 11 {
            return Err(ImageError::Unsupported(format!(
                "LZW minimum code size {depth} exceeds 11"
            )));
        }
        let clear_code = 1i32 << depth;
        let eof_code = clear_code + 1;
        let running_bits = depth as u32 + 1;
        let mut decoder = Self {
            bytes,
            pos: pos + 1,
            block: [0; 256],
            block_len: 0,
            block_pos: 0,
            completed: false,
            truncated: false,
            overflow: false,
            depth,
            clear_code,
            eof_code,
            running_code: eof_code + 1,
            running_bits,
            max_code_plus_one: 1 << running_bits,
            prev_code: NO_SUCH_CODE,
            shift_state: 0,
            shift_data: 0,
            stack: [0; 4096],
            stack_ptr: 0,
            suffix: [0; 4096],
            prefix: [0; 4096],
        };
        decoder.prefix.fill(NO_SUCH_CODE_BYTE);
        Ok(decoder)
    }

    fn read_byte(&mut self) -> u8 {
        if self.completed {
            return 0;
        }
        if self.block_pos == self.block_len {
            let Some(&size) = self.bytes.get(self.pos) else {
                self.completed = true;
                self.truncated = true;
                return 0;
            };
            self.pos += 1;
            let size = size as usize;
            if size == 0 {
                self.completed = true;
                return 0;
            }
            let available = size.min(self.bytes.len().saturating_sub(self.pos));
            self.block[..available].copy_from_slice(&self.bytes[self.pos..self.pos + available]);
            if available < size {
                self.block[available..size].fill(0);
                self.truncated = true;
                self.pos = self.bytes.len();
            } else {
                self.pos += size;
            }
            self.block_len = size;
            self.block_pos = 0;
        }
        let byte = self.block[self.block_pos];
        self.block_pos += 1;
        byte
    }

    fn read_code(&mut self) -> i32 {
        while self.shift_state < self.running_bits {
            let byte = self.read_byte();
            self.shift_data |= (byte as u32) << self.shift_state;
            self.shift_state += 8;
        }
        let code = (self.shift_data & CODE_MASKS[self.running_bits as usize]) as i32;
        self.shift_data >>= self.running_bits;
        self.shift_state -= self.running_bits;
        self.running_code += 1;
        if self.running_code > self.max_code_plus_one && self.running_bits < LZ_BITS {
            self.max_code_plus_one <<= 1;
            self.running_bits += 1;
        }
        code
    }

    fn trace_prefix(&self, code: i32, clear_code: i32) -> u8 {
        let mut code = code;
        let mut i = 0;
        while code > clear_code && i <= LZ_MAX_CODE {
            i += 1;
            if code < 0 || code >= self.prefix.len() as i32 {
                return (code & 0xFF) as u8;
            }
            code = self.prefix[code as usize];
        }
        (code & 0xFF) as u8
    }

    fn read_line(&mut self, out: &mut [u8], mut offset: usize, length: usize) {
        let mut i = 0usize;
        let mut stack_ptr = self.stack_ptr;
        let eof_code = self.eof_code;
        let clear_code = self.clear_code;
        let mut prev_code = self.prev_code;

        while stack_ptr != 0 && i < length {
            stack_ptr -= 1;
            out[offset] = self.stack[stack_ptr];
            offset += 1;
            i += 1;
        }

        while i < length {
            let gif_code = self.read_code();
            if gif_code == eof_code {
                if i != length - 1 {
                    return;
                }
                i += 1;
            } else if gif_code == clear_code {
                self.prefix.fill(NO_SUCH_CODE);
                self.running_code = self.eof_code + 1;
                self.running_bits = self.depth as u32 + 1;
                self.max_code_plus_one = 1 << self.running_bits;
                prev_code = NO_SUCH_CODE;
                self.prev_code = NO_SUCH_CODE;
            } else {
                if gif_code < clear_code {
                    out[offset] = gif_code as u8;
                    offset += 1;
                    i += 1;
                } else {
                    if !(0..=LZ_MAX_CODE).contains(&gif_code) {
                        return;
                    }

                    let mut code;
                    if self.prefix[gif_code as usize] == NO_SUCH_CODE {
                        if gif_code != self.running_code - 2 {
                            return;
                        }
                        code = prev_code;
                        let value = self.trace_prefix(prev_code, clear_code);
                        let index = (self.running_code - 2) as usize;
                        self.suffix[index] = value;
                        if !self.push_stack(&mut stack_ptr, value) {
                            return;
                        }
                    } else {
                        code = gif_code;
                    }

                    let mut c = 0i32;
                    loop {
                        let within = c <= LZ_MAX_CODE;
                        c += 1;
                        if !within || code <= clear_code || code > LZ_MAX_CODE {
                            break;
                        }
                        let value = self.suffix[code as usize];
                        if !self.push_stack(&mut stack_ptr, value) {
                            return;
                        }
                        code = self.prefix[code as usize];
                    }
                    if c >= LZ_MAX_CODE || code > LZ_MAX_CODE {
                        return;
                    }

                    if !self.push_stack(&mut stack_ptr, code as u8) {
                        return;
                    }
                    while stack_ptr != 0 && i < length {
                        stack_ptr -= 1;
                        out[offset] = self.stack[stack_ptr];
                        offset += 1;
                        i += 1;
                    }
                }

                if prev_code != NO_SUCH_CODE {
                    if self.running_code < 2 || self.running_code > FIRST_CODE {
                        return;
                    }
                    let index = (self.running_code - 2) as usize;
                    self.prefix[index] = prev_code;
                    if gif_code == self.running_code - 2 {
                        self.suffix[index] = self.trace_prefix(prev_code, clear_code);
                    } else {
                        self.suffix[index] = self.trace_prefix(gif_code, clear_code);
                    }
                }
                prev_code = gif_code;
            }
        }

        self.prev_code = prev_code;
        self.stack_ptr = stack_ptr;
    }

    fn push_stack(&mut self, stack_ptr: &mut usize, value: u8) -> bool {
        if *stack_ptr >= self.stack.len() {
            self.overflow = true;
            return false;
        }
        self.stack[*stack_ptr] = value;
        *stack_ptr += 1;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn lzw_encode(indices: &[u8], min_code_size: u8) -> Vec<(u16, u32)> {
        let clear = 1u16 << min_code_size;
        let eoi = clear + 1;
        let mut code_size = min_code_size as u32 + 1;
        let mut codes = vec![(clear, code_size)];
        if !indices.is_empty() {
            let mut dict: HashMap<(u16, u8), u16> = HashMap::new();
            let mut next = eoi + 1;
            let mut w = indices[0] as u16;
            for &byte in &indices[1..] {
                if let Some(&code) = dict.get(&(w, byte)) {
                    w = code;
                } else {
                    codes.push((w, code_size));
                    dict.insert((w, byte), next);
                    next += 1;
                    if next > (1 << code_size) && code_size < 12 {
                        code_size += 1;
                    }
                    w = byte as u16;
                }
            }
            codes.push((w, code_size));
        }
        codes.push((eoi, code_size));
        codes
    }

    fn interlace_order(indices: &[u8], width: usize, height: usize) -> Vec<u8> {
        let mut ordered = Vec::with_capacity(indices.len());
        for (start, step) in [(0usize, 8usize), (4, 8), (2, 4), (1, 2)] {
            let mut y = start;
            while y < height {
                ordered.extend_from_slice(&indices[y * width..(y + 1) * width]);
                y += step;
            }
        }
        ordered
    }

    fn build_gif_from_codes(
        width: u16,
        height: u16,
        palette: &[[u8; 3]],
        min_code_size: u8,
        codes: &[(u16, u32)],
        interlaced: bool,
    ) -> Vec<u8> {
        assert!(palette.len().is_power_of_two() && (2..=256).contains(&palette.len()));
        let bits = palette.len().trailing_zeros() as u8 - 1;
        let mut gif = Vec::new();
        gif.extend_from_slice(b"GIF89a");
        gif.extend_from_slice(&width.to_le_bytes());
        gif.extend_from_slice(&height.to_le_bytes());
        gif.push(0x80 | bits);
        gif.push(0);
        gif.push(0);
        for colour in palette {
            gif.extend_from_slice(colour);
        }
        gif.push(0x2C);
        gif.extend_from_slice(&0u16.to_le_bytes());
        gif.extend_from_slice(&0u16.to_le_bytes());
        gif.extend_from_slice(&width.to_le_bytes());
        gif.extend_from_slice(&height.to_le_bytes());
        gif.push(if interlaced { 0x40 } else { 0 });
        gif.push(min_code_size);

        let mut packed = Vec::new();
        let mut accumulator = 0u32;
        let mut bits_used = 0u32;
        for &(code, size) in codes {
            accumulator |= (code as u32) << bits_used;
            bits_used += size;
            while bits_used >= 8 {
                packed.push((accumulator & 0xFF) as u8);
                accumulator >>= 8;
                bits_used -= 8;
            }
        }
        if bits_used > 0 {
            packed.push((accumulator & 0xFF) as u8);
        }
        for chunk in packed.chunks(255) {
            gif.push(chunk.len() as u8);
            gif.extend_from_slice(chunk);
        }
        gif.push(0);
        gif.push(0x3B);
        gif
    }

    fn build_gif(
        width: u16,
        height: u16,
        palette: &[[u8; 3]],
        min_code_size: u8,
        indices: &[u8],
        interlaced: bool,
    ) -> Vec<u8> {
        let ordered = if interlaced {
            interlace_order(indices, width as usize, height as usize)
        } else {
            indices.to_vec()
        };
        let codes = lzw_encode(&ordered, min_code_size);
        build_gif_from_codes(width, height, palette, min_code_size, &codes, interlaced)
    }

    fn checkerboard(width: usize, height: usize) -> Vec<u8> {
        (0..width * height)
            .map(|i| ((i % width + i / width) % 2) as u8)
            .collect()
    }

    const PALETTE: [[u8; 3]; 2] = [[10, 20, 30], [200, 150, 100]];

    #[test]
    fn decodes_single_colour_gif() {
        let gif = build_gif(3, 2, &PALETTE, 2, &[0; 6], false);
        let image = decode_gif(&gif).unwrap();
        assert_eq!(image.width, 3);
        assert_eq!(image.height, 2);
        assert_eq!(image.pixels, vec![0; 6]);
        assert_eq!(image.palette, PALETTE);
    }

    #[test]
    fn decodes_two_colour_checkerboard() {
        let indices = checkerboard(4, 4);
        let gif = build_gif(4, 4, &PALETTE, 2, &indices, false);
        let image = decode_gif(&gif).unwrap();
        assert_eq!((image.width, image.height), (4, 4));
        assert_eq!(image.pixels, indices);
        assert_eq!(image.palette, PALETTE);
    }

    #[test]
    fn decodes_larger_palette_ordered_dither() {
        let palette: Vec<[u8; 3]> = (0..8).map(|i| [i * 30, 255 - i * 30, i * 10]).collect();
        let indices: Vec<u8> = (0..8 * 8).map(|i| ((i % 4) + (i / 8) % 4) as u8).collect();
        let gif = build_gif(8, 8, &palette, 3, &indices, false);
        let image = decode_gif(&gif).unwrap();
        assert_eq!(image.pixels, indices);
        assert_eq!(image.palette, palette);
    }

    #[test]
    fn decodes_interlaced_gif_into_logical_order() {
        let indices = checkerboard(8, 8);
        let gif = build_gif(8, 8, &PALETTE, 2, &indices, true);
        let image = decode_gif(&gif).unwrap();
        assert_eq!(image.pixels, indices);
    }

    #[test]
    fn handles_mid_stream_clear_and_eoi() {
        let codes = [(4u16, 3u32), (0, 3), (1, 3), (4, 3), (1, 3), (0, 3), (5, 3)];
        let gif = build_gif_from_codes(2, 2, &PALETTE, 2, &codes, false);
        let image = decode_gif(&gif).unwrap();
        assert_eq!(image.pixels, [0, 1, 1, 0]);
    }

    #[test]
    fn early_eoi_aborts_row_and_leaves_zeroes() {
        let codes = [(4u16, 3u32), (1, 3), (5, 3), (0, 3), (1, 3)];
        let gif = build_gif_from_codes(4, 1, &PALETTE, 2, &codes, false);
        let image = decode_gif(&gif).unwrap();
        assert_eq!(image.pixels, [1, 0, 0, 0]);
    }

    #[test]
    fn skips_graphic_control_extension() {
        let indices = checkerboard(4, 4);
        let gif = build_gif(4, 4, &PALETTE, 2, &indices, false);
        let mut with_gce = gif.clone();
        let insert_at = 13 + PALETTE.len() * 3;
        let gce = [0x21, 0xF9, 0x04, 0x01, 0x00, 0x00, 0x01, 0x00];
        with_gce.splice(insert_at..insert_at, gce);
        let image = decode_gif(&with_gce).unwrap();
        assert_eq!(image.pixels, indices);
        assert_eq!(image.palette, PALETTE);
    }

    #[test]
    fn skips_fixed_128_entry_local_colour_table() {
        let indices = checkerboard(4, 4);
        let mut gif = build_gif(4, 4, &PALETTE, 2, &indices, false);
        let descriptor = 13 + PALETTE.len() * 3;
        assert_eq!(gif[descriptor], 0x2C);
        gif[descriptor + 9] |= 0x80;
        let local_table: Vec<u8> = (0..128 * 3).map(|i| (i % 251) as u8).collect();
        gif.splice(descriptor + 10..descriptor + 10, local_table);
        let image = decode_gif(&gif).unwrap();
        assert_eq!(image.pixels, indices);
        assert_eq!(image.palette, PALETTE);
    }

    #[test]
    fn accepts_gif87a_version() {
        let mut gif = build_gif(4, 4, &PALETTE, 2, &checkerboard(4, 4), false);
        gif[3..6].copy_from_slice(b"87a");
        assert!(decode_gif(&gif).is_ok());
    }

    #[test]
    fn rejects_non_gif_signature() {
        let mut gif = build_gif(1, 1, &PALETTE, 2, &[0], false);
        gif[..3].copy_from_slice(b"NOT");
        assert!(matches!(decode_gif(&gif), Err(ImageError::Invalid(_))));
    }

    #[test]
    fn rejects_oversized_lzw_code_size() {
        let codes = [(4096u16, 13u32), (4097, 13)];
        let gif = build_gif_from_codes(1, 1, &PALETTE, 12, &codes, false);
        assert!(matches!(decode_gif(&gif), Err(ImageError::Unsupported(_))));
    }

    #[test]
    fn truncated_input_errors() {
        assert!(matches!(decode_gif(&[]), Err(ImageError::Truncated)));
        assert!(matches!(decode_gif(b"GIF89a"), Err(ImageError::Truncated)));

        let gif = build_gif(4, 4, &PALETTE, 2, &checkerboard(4, 4), false);
        for cut in 6..13 + 2 * 3 {
            assert!(
                matches!(decode_gif(&gif[..cut]), Err(ImageError::Truncated)),
                "cut at {cut}"
            );
        }
        assert!(matches!(
            decode_gif(&gif[..gif.len() - 3]),
            Err(ImageError::Truncated)
        ));
    }

    #[test]
    fn rejects_images_larger_than_the_engine_buffer() {
        let gif = build_gif_from_codes(4096, 4096, &PALETTE, 2, &[(4, 3), (5, 3)], false);
        assert!(matches!(decode_gif(&gif), Err(ImageError::Unsupported(_))));
    }

    #[test]
    fn every_prefix_decodes_without_panicking() {
        let gif = build_gif(4, 4, &PALETTE, 2, &checkerboard(4, 4), false);
        for cut in 0..gif.len() {
            let _ = decode_gif(&gif[..cut]);
        }
    }

    #[test]
    #[ignore = "writes generated GIFs to the temp directory for external inspection"]
    fn dump_generated_gifs_for_inspection() {
        let dir = std::env::temp_dir().join("retro-image-generated");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("single.gif"),
            build_gif(3, 2, &PALETTE, 2, &[0; 6], false),
        )
        .unwrap();
        std::fs::write(
            dir.join("checker.gif"),
            build_gif(4, 4, &PALETTE, 2, &checkerboard(4, 4), false),
        )
        .unwrap();
        std::fs::write(
            dir.join("interlaced.gif"),
            build_gif(8, 8, &PALETTE, 2, &checkerboard(8, 8), true),
        )
        .unwrap();
        let palette: Vec<[u8; 3]> = (0..8).map(|i| [i * 30, 255 - i * 30, i * 10]).collect();
        let indices: Vec<u8> = (0..8 * 8).map(|i| ((i % 4) + (i / 8) % 4) as u8).collect();
        std::fs::write(
            dir.join("palette8.gif"),
            build_gif(8, 8, &palette, 3, &indices, false),
        )
        .unwrap();
        println!("wrote generated GIFs to {}", dir.display());
    }

    #[test]
    fn arbitrary_bytes_do_not_panic() {
        let mut state = 0x1234_5678u32;
        for length in 0..256usize {
            let mut bytes: Vec<u8> = (0..length)
                .map(|_| {
                    state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    (state >> 24) as u8
                })
                .collect();
            if bytes.len() >= 6 {
                bytes[..6].copy_from_slice(b"GIF89a");
            }
            let _ = decode_gif(&bytes);
        }
    }
}
