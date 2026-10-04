//! Ogg Vorbis decoding via `lewton`.
//!
//! Upstream uses libvorbisfile (`LoadSfx`/`LoadMusic` in `RSDKv4/Audio.cpp`); `lewton`
//! decodes the same bitstreams in pure Rust. SFX are decoded fully up front, matching the
//! upstream OGG-SFX path.

use std::io::Cursor;

use lewton::inside_ogg::OggStreamReader;

use crate::AudioError;
use crate::decode::DecodedAudio;

/// Decodes an Ogg Vorbis byte stream into interleaved i16 samples.
pub(crate) fn decode_vorbis(bytes: &[u8]) -> Result<DecodedAudio, AudioError> {
    let mut reader = OggStreamReader::new(Cursor::new(bytes)).map_err(map_error)?;
    let channels = usize::from(reader.ident_hdr.audio_channels);
    let sample_rate = reader.ident_hdr.audio_sample_rate;
    if channels == 0 || channels > 2 {
        return Err(AudioError::Unsupported(format!(
            "{channels}-channel vorbis stream"
        )));
    }
    if sample_rate == 0 {
        return Err(AudioError::Invalid("vorbis sample rate is zero".to_owned()));
    }

    let mut samples = Vec::new();
    while let Some(packet) = reader.read_dec_packet_itl().map_err(map_error)? {
        samples.extend_from_slice(&packet);
    }
    // The final Vorbis packet is block-padded; the page granule position is the true length.
    let frames = samples.len() / channels;
    let total = reader
        .get_last_absgp()
        .and_then(|granule| usize::try_from(granule).ok())
        .unwrap_or(frames)
        .min(frames);
    samples.truncate(total * channels);
    Ok(DecodedAudio {
        samples,
        channels,
        sample_rate,
    })
}

pub(crate) fn map_error(error: lewton::VorbisError) -> AudioError {
    AudioError::Decode(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TONE: &[u8] = include_bytes!("../tests/fixtures/tone.ogg");
    const TONE_MONO: &[u8] = include_bytes!("../tests/fixtures/tone_mono.ogg");

    fn energy(decoded: &DecodedAudio) -> f64 {
        let sum: f64 = decoded
            .samples
            .iter()
            .map(|s| f64::from(*s) * f64::from(*s))
            .sum();
        sum / decoded.samples.len().max(1) as f64
    }

    #[test]
    fn decodes_stereo_fixture() {
        let decoded = decode_vorbis(TONE).unwrap();
        assert_eq!(decoded.channels, 2);
        assert_eq!(decoded.sample_rate, 44_100);
        assert_eq!(decoded.frames(), 2205);
        assert!(energy(&decoded) > 1000.0);
    }

    #[test]
    fn decodes_mono_fixture() {
        let decoded = decode_vorbis(TONE_MONO).unwrap();
        assert_eq!(decoded.channels, 1);
        assert_eq!(decoded.sample_rate, 44_100);
        assert_eq!(decoded.frames(), 882);
        assert!(energy(&decoded) > 1000.0);
    }

    #[test]
    fn rejects_garbage() {
        assert!(matches!(
            decode_vorbis(b"not an ogg stream"),
            Err(AudioError::Decode(_))
        ));
    }

    #[test]
    fn rejects_truncated_stream() {
        let truncated = &TONE[..TONE.len() / 3];
        assert!(decode_vorbis(truncated).is_err());
    }
}
