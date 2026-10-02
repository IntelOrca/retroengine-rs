//! Minimal RIFF/WAVE reader for the PCM variants found in the S1/S2 assets.
//!
//! The parser accepts 8/16/24/32-bit PCM and 32-bit IEEE float, mono or stereo, including
//! `WAVE_FORMAT_EXTENSIBLE` headers. It mirrors the formats `SDL_LoadWAV` accepts upstream
//! (`LoadSfx` in `RSDKv4/Audio.cpp`) without depending on SDL.

use crate::AudioError;
use crate::decode::DecodedAudio;

const FORMAT_PCM: u16 = 1;
const FORMAT_FLOAT: u16 = 3;
const FORMAT_EXTENSIBLE: u16 = 0xFFFE;

struct WavFormat {
    format: u16,
    channels: u16,
    sample_rate: u32,
    bits: u16,
}

fn u16_at(bytes: &[u8], offset: usize) -> Result<u16, AudioError> {
    let raw = bytes.get(offset..offset + 2).ok_or(AudioError::Truncated)?;
    Ok(u16::from_le_bytes([raw[0], raw[1]]))
}

fn u32_at(bytes: &[u8], offset: usize) -> Result<u32, AudioError> {
    let raw = bytes.get(offset..offset + 4).ok_or(AudioError::Truncated)?;
    Ok(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
}

fn parse_format(chunk: &[u8]) -> Result<WavFormat, AudioError> {
    if chunk.len() < 16 {
        return Err(AudioError::Truncated);
    }
    let mut format = u16_at(chunk, 0)?;
    let channels = u16_at(chunk, 2)?;
    let sample_rate = u32_at(chunk, 4)?;
    let bits = u16_at(chunk, 14)?;
    if format == FORMAT_EXTENSIBLE {
        // The first two bytes of the sub-format GUID hold the real format tag.
        format = u16_at(chunk, 24)?;
    }
    if channels == 0 || channels > 2 {
        return Err(AudioError::Unsupported(format!(
            "{channels}-channel wav data"
        )));
    }
    if sample_rate == 0 {
        return Err(AudioError::Invalid("wav sample rate is zero".to_owned()));
    }
    match (format, bits) {
        (FORMAT_PCM, 8 | 16 | 24 | 32) | (FORMAT_FLOAT, 32) => {}
        _ => {
            return Err(AudioError::Unsupported(format!(
                "wav format {format} with {bits}-bit samples"
            )));
        }
    }
    Ok(WavFormat {
        format,
        channels,
        sample_rate,
        bits,
    })
}

fn convert_sample(raw: &[u8], format: u16, bits: u16) -> i16 {
    match (format, bits) {
        (FORMAT_PCM, 8) => (i16::from(raw[0]) - 128) << 8,
        (FORMAT_PCM, 16) => i16::from_le_bytes([raw[0], raw[1]]),
        (FORMAT_PCM, 24) => {
            let value = i32::from(raw[0]) | (i32::from(raw[1]) << 8) | (i32::from(raw[2]) << 16);
            ((value << 8) >> 8 >> 8) as i16
        }
        (FORMAT_PCM, 32) => {
            let value = i32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]);
            (value >> 16) as i16
        }
        (FORMAT_FLOAT, 32) => {
            let value = f32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]);
            (value.clamp(-1.0, 1.0) * 32767.0) as i16
        }
        _ => 0,
    }
}

/// Decodes a RIFF/WAVE byte stream into interleaved i16 samples.
pub(crate) fn decode_wav(bytes: &[u8]) -> Result<DecodedAudio, AudioError> {
    if bytes.len() < 12 {
        return Err(AudioError::Truncated);
    }
    if bytes.get(0..4) != Some(b"RIFF") || bytes.get(8..12) != Some(b"WAVE") {
        return Err(AudioError::Invalid("not a RIFF/WAVE container".to_owned()));
    }

    let mut format: Option<WavFormat> = None;
    let mut data: Option<&[u8]> = None;
    let mut offset = 12usize;
    while let Some(header) = bytes.get(offset..offset + 8) {
        let id = &header[0..4];
        let size = u32::from_le_bytes([header[4], header[5], header[6], header[7]]) as usize;
        let body_start = offset + 8;
        let body_end = body_start
            .checked_add(size)
            .ok_or_else(|| AudioError::Invalid("wav chunk size overflows".to_owned()))?;
        let body = bytes
            .get(body_start..body_end)
            .ok_or(AudioError::Truncated)?;
        match id {
            b"fmt " => format = Some(parse_format(body)?),
            b"data" => data = Some(body),
            _ => {}
        }
        offset = body_end + (size & 1);
    }

    let format = format.ok_or_else(|| AudioError::Invalid("wav has no fmt chunk".to_owned()))?;
    let data = data.ok_or_else(|| AudioError::Invalid("wav has no data chunk".to_owned()))?;
    let sample_bytes = usize::from(format.bits / 8);
    let block = sample_bytes * usize::from(format.channels);
    let frames = data.len() / block;
    let mut samples = Vec::with_capacity(frames * usize::from(format.channels));
    for frame in 0..frames {
        let base = frame * block;
        for channel in 0..usize::from(format.channels) {
            let start = base + channel * sample_bytes;
            samples.push(convert_sample(
                &data[start..start + sample_bytes],
                format.format,
                format.bits,
            ));
        }
    }
    Ok(DecodedAudio {
        samples,
        channels: usize::from(format.channels),
        sample_rate: format.sample_rate,
    })
}

#[cfg(test)]
pub(crate) fn build_wav(
    format: u16,
    channels: u16,
    sample_rate: u32,
    bits: u16,
    data: &[u8],
) -> Vec<u8> {
    let block = u32::from(bits / 8) * u32::from(channels);
    let mut out = Vec::new();
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&format.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&(sample_rate * block).to_le_bytes());
    out.extend_from_slice(&(block as u16).to_le_bytes());
    out.extend_from_slice(&bits.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(data);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn samples_16(samples: &[i16]) -> Vec<u8> {
        samples.iter().flat_map(|s| s.to_le_bytes()).collect()
    }

    #[test]
    fn decodes_16_bit_stereo() {
        let data = samples_16(&[1000, -1000, 2000, -2000]);
        let wav = build_wav(FORMAT_PCM, 2, 44_100, 16, &data);
        let decoded = decode_wav(&wav).unwrap();
        assert_eq!(decoded.samples, vec![1000, -1000, 2000, -2000]);
        assert_eq!(decoded.channels, 2);
        assert_eq!(decoded.sample_rate, 44_100);
    }

    #[test]
    fn decodes_16_bit_mono() {
        let data = samples_16(&[i16::MIN, -1, 0, 1, i16::MAX]);
        let wav = build_wav(FORMAT_PCM, 1, 48_000, 16, &data);
        let decoded = decode_wav(&wav).unwrap();
        assert_eq!(decoded.samples, vec![i16::MIN, -1, 0, 1, i16::MAX]);
        assert_eq!(decoded.channels, 1);
        assert_eq!(decoded.sample_rate, 48_000);
    }

    #[test]
    fn decodes_8_bit_mono_as_unsigned() {
        let wav = build_wav(FORMAT_PCM, 1, 44_100, 8, &[0, 128, 255]);
        let decoded = decode_wav(&wav).unwrap();
        assert_eq!(decoded.samples, vec![-32768, 0, 32512]);
    }

    #[test]
    fn decodes_24_bit_mono_with_sign_extension() {
        let data = [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x7F, 0x00, 0x00, 0x80];
        let wav = build_wav(FORMAT_PCM, 1, 44_100, 24, &data);
        let decoded = decode_wav(&wav).unwrap();
        assert_eq!(decoded.samples, vec![-1, 32767, -32768]);
    }

    #[test]
    fn decodes_float_mono() {
        let data: Vec<u8> = [0.5f32, -0.5, 2.0, -2.0]
            .iter()
            .flat_map(|f| f.to_le_bytes())
            .collect();
        let wav = build_wav(FORMAT_FLOAT, 1, 44_100, 32, &data);
        let decoded = decode_wav(&wav).unwrap();
        assert_eq!(decoded.samples, vec![16383, -16383, 32767, -32767]);
    }

    #[test]
    fn decodes_extensible_pcm() {
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&52u32.to_le_bytes());
        wav.extend_from_slice(b"WAVE");
        wav.extend_from_slice(b"fmt ");
        wav.extend_from_slice(&40u32.to_le_bytes());
        wav.extend_from_slice(&FORMAT_EXTENSIBLE.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&44_100u32.to_le_bytes());
        wav.extend_from_slice(&88_200u32.to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(&22u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(&0u32.to_le_bytes());
        wav.extend_from_slice(&[
            1, 0, 0, 0, 0, 0, 0x10, 0, 0x80, 0, 0, 0xAA, 0, 0x38, 0x9B, 0x71,
        ]);
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&2u32.to_le_bytes());
        wav.extend_from_slice(&samples_16(&[1234]));
        let decoded = decode_wav(&wav).unwrap();
        assert_eq!(decoded.samples, vec![1234]);
    }

    #[test]
    fn rejects_short_header() {
        assert!(matches!(decode_wav(b"RIFF"), Err(AudioError::Truncated)));
    }

    #[test]
    fn rejects_non_riff_data() {
        let mut bytes = vec![0u8; 16];
        bytes[0..4].copy_from_slice(b"NOPE");
        assert!(matches!(decode_wav(&bytes), Err(AudioError::Invalid(_))));
    }

    #[test]
    fn rejects_truncated_chunk_body() {
        let data = samples_16(&[1, 2, 3, 4]);
        let mut wav = build_wav(FORMAT_PCM, 2, 44_100, 16, &data);
        wav.truncate(wav.len() - 2);
        assert!(matches!(decode_wav(&wav), Err(AudioError::Truncated)));
    }

    #[test]
    fn rejects_missing_fmt_chunk() {
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&12u32.to_le_bytes());
        wav.extend_from_slice(b"WAVE");
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&4u32.to_le_bytes());
        wav.extend_from_slice(&[0, 0, 0, 0]);
        assert!(matches!(decode_wav(&wav), Err(AudioError::Invalid(_))));
    }

    #[test]
    fn rejects_missing_data_chunk() {
        let wav = build_wav(FORMAT_PCM, 1, 44_100, 16, &[]);
        let mut header_only = wav[..36].to_vec();
        header_only[4..8].copy_from_slice(&36u32.to_le_bytes());
        assert!(matches!(
            decode_wav(&header_only),
            Err(AudioError::Invalid(_))
        ));
    }

    #[test]
    fn rejects_unsupported_formats() {
        let data = vec![0u8; 4];
        let adpcm = build_wav(2, 1, 44_100, 4, &data);
        assert!(matches!(
            decode_wav(&adpcm),
            Err(AudioError::Unsupported(_))
        ));
        let surround = build_wav(FORMAT_PCM, 3, 44_100, 16, &data);
        assert!(matches!(
            decode_wav(&surround),
            Err(AudioError::Unsupported(_))
        ));
        let float16 = build_wav(FORMAT_FLOAT, 1, 44_100, 16, &data);
        assert!(matches!(
            decode_wav(&float16),
            Err(AudioError::Unsupported(_))
        ));
    }

    #[test]
    fn ignores_trailing_partial_frame() {
        let data = samples_16(&[10, 20, 30, 40, 50]);
        let wav = build_wav(FORMAT_PCM, 2, 44_100, 16, &data);
        let decoded = decode_wav(&wav).unwrap();
        assert_eq!(decoded.samples, vec![10, 20, 30, 40]);
    }
}
