//! Shared decoding helpers.
//!
//! Upstream converts every WAV and Ogg SFX to the device format (`Sint16`, stereo, 44.1 kHz)
//! once at load time (`LoadSfx` in `RSDKv4/Audio.cpp`), so this module exposes the same
//! conversion for the mixer to apply to decoded assets.

use crate::vorbis;
use crate::wav;
use crate::{AudioError, SAMPLE_RATE};

/// Audio decoded in its source layout: interleaved signed 16-bit samples.
pub(crate) struct DecodedAudio {
    /// Interleaved samples in `channels`-channel order.
    pub(crate) samples: Vec<i16>,
    /// Number of interleaved channels (1 or 2).
    pub(crate) channels: usize,
    /// Source sample rate in Hz.
    pub(crate) sample_rate: u32,
}

impl DecodedAudio {
    /// Number of sample frames.
    pub(crate) fn frames(&self) -> usize {
        self.samples.len().checked_div(self.channels).unwrap_or(0)
    }
}

/// Decodes a WAV or Ogg Vorbis container by sniffing its magic bytes.
pub(crate) fn decode(bytes: &[u8]) -> Result<DecodedAudio, AudioError> {
    if bytes.starts_with(b"RIFF") {
        wav::decode_wav(bytes)
    } else if bytes.starts_with(b"OggS") {
        vorbis::decode_vorbis(bytes)
    } else {
        Err(AudioError::Unsupported(
            "expected a RIFF/WAVE or OggS container".to_owned(),
        ))
    }
}

/// Interpolates one stereo frame at a fixed-point source position (32 fractional bits).
fn stereo_at(audio: &DecodedAudio, position: u64) -> (i16, i16) {
    let frames = audio.frames();
    if frames == 0 {
        return (0, 0);
    }
    let frame = (position >> 32) as usize;
    if frame >= frames {
        return (0, 0);
    }
    let fraction = position & 0xFFFF_FFFF;
    let next = (frame + 1).min(frames - 1);
    let channels = audio.channels;
    let lerp = |a: i16, b: i16| -> i16 {
        let weight = ((1u64 << 32) - fraction) as i64;
        let value = i64::from(a) * weight + i64::from(b) * fraction as i64;
        (value >> 32) as i16
    };
    let left = lerp(
        audio.samples[frame * channels],
        audio.samples[next * channels],
    );
    let right = if channels == 1 {
        left
    } else {
        lerp(
            audio.samples[frame * channels + 1],
            audio.samples[next * channels + 1],
        )
    };
    (left, right)
}

/// Converts decoded audio to interleaved stereo at [`SAMPLE_RATE`].
///
/// Sources already in the device format are copied byte-for-byte so WAV playback stays exact.
/// Other rates are resampled with linear interpolation; upstream hands this to SDL's
/// audio-conversion path, which is not reproducible outside SDL, so the port uses a
/// deterministic approximation instead.
pub(crate) fn to_stereo_44100(audio: &DecodedAudio) -> Vec<i16> {
    let frames = audio.frames();
    if frames == 0 {
        return Vec::new();
    }
    if audio.sample_rate == SAMPLE_RATE && audio.channels == 2 {
        return audio.samples.clone();
    }
    if audio.sample_rate == SAMPLE_RATE {
        let mut out = Vec::with_capacity(frames * 2);
        for &sample in &audio.samples {
            out.push(sample);
            out.push(sample);
        }
        return out;
    }
    let out_frames =
        (frames as u64 * u64::from(SAMPLE_RATE) / u64::from(audio.sample_rate)) as usize;
    let step = (u64::from(audio.sample_rate) << 32) / u64::from(SAMPLE_RATE);
    let mut out = Vec::with_capacity(out_frames * 2);
    let mut position = 0u64;
    for _ in 0..out_frames {
        let (left, right) = stereo_at(audio, position);
        out.push(left);
        out.push(right);
        position += step;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn audio(samples: Vec<i16>, channels: usize, sample_rate: u32) -> DecodedAudio {
        DecodedAudio {
            samples,
            channels,
            sample_rate,
        }
    }

    #[test]
    fn stereo_at_device_rate_is_copied_exactly() {
        let source = audio(vec![1000, -1000, 2000, -2000], 2, SAMPLE_RATE);
        assert_eq!(to_stereo_44100(&source), source.samples);
    }

    #[test]
    fn mono_at_device_rate_is_duplicated() {
        let source = audio(vec![1000, -2000], 1, SAMPLE_RATE);
        assert_eq!(to_stereo_44100(&source), vec![1000, 1000, -2000, -2000]);
    }

    #[test]
    fn half_rate_source_doubles_the_frame_count() {
        let source = audio(vec![100, -100, 300, -300], 2, 22_050);
        let converted = to_stereo_44100(&source);
        assert_eq!(converted, vec![100, -100, 200, -200, 300, -300, 300, -300]);
    }

    #[test]
    fn double_rate_source_halves_the_frame_count() {
        let source = audio(vec![100, 100, 200, 200, 300, 300, 400, 400], 2, 88_200);
        let converted = to_stereo_44100(&source);
        assert_eq!(converted, vec![100, 100, 300, 300]);
    }

    #[test]
    fn empty_audio_converts_to_empty() {
        assert!(to_stereo_44100(&audio(Vec::new(), 1, 8000)).is_empty());
    }
}
