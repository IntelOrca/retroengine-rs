//! Streaming Ogg Vorbis decoding for music tracks.
//!
//! SFX are decoded fully at load time, but music can be minutes long: `LoadMusic` upstream hands
//! the track to libvorbisfile and `ProcessAudioPlayback` pulls packets with `ov_read` as the
//! device consumes them. This module ports that shape onto `lewton`: [`VorbisStream`] owns an
//! `OggStreamReader` over the compressed bytes and keeps only a bounded ring of decoded frames
//! ([`STREAM_RING_FRAMES`], 500 ms), pulling one packet at a time as the mixer's source cursor
//! reaches it.
//!
//! Decoding is packet-sequential, so a sample's value never depends on where playback started.
//! Loop points jump backwards, which the ring cannot serve directly; [`VorbisStream::restart_at`]
//! re-enters the stream with a page seek and an exact decoded-position anchor:
//!
//! * While decoding forwards, every page boundary crossed records the exact decoded frame index
//!   reached (`get_last_absgp` changes to the page granule at a boundary, and the running frame
//!   count is exact because decoding started at frame 0).
//! * To restart at frame `target`, the decoder seeks (`seek_absgp_pg`) to a recorded page two
//!   entries before the one that contains `target`, discards samples until a recorded boundary
//!   re-anchors the frame count, and then decodes forward. The first packet after a seek has no
//!   overlap window, so its samples are not exact; the two-page margin guarantees at least one
//!   exact page before `target`, making every packet at or after `target` byte-identical to a
//!   whole-track decode.
//! * If no anchor exists yet (a restart before the first recorded page, a malformed stream, or a
//!   chained/multiplexed Ogg whose page granules are not monotonic) the stream restarts from the
//!   beginning and discards packets up to `target`, which is slower but always exact.
//!
//! Page granules and decoded frame counts can differ by a few hundred samples at a boundary
//! (Vorbis block padding), so anchors record the decoder's own running frame count rather than
//! trusting the granule as a position.

use std::collections::VecDeque;
use std::io::Cursor;
use std::sync::Arc;

use lewton::inside_ogg::OggStreamReader;

use crate::vorbis::map_error;
use crate::{AudioError, SAMPLE_RATE};

/// Decoded lookahead retained per music stream: 500 ms at 44.1 kHz.
///
/// The ring only has to cover the distance between the mixer's source cursor and the packets
/// decoded for it; 500 ms is far more than one packet (at most 8192 frames) plus the one-frame
/// interpolation lookahead.
pub(crate) const STREAM_RING_FRAMES: usize = SAMPLE_RATE as usize / 2;

/// One scanned Ogg page with a real granule position (header pages and -1 pages are skipped).
#[derive(Clone, Copy)]
struct PageInfo {
    granule: u64,
}

/// A page boundary whose decoded frame index was recorded during sequential decoding.
#[derive(Clone, Copy)]
struct PageAnchor {
    granule: u64,
    position: usize,
}

/// A music stream decoded on demand with a bounded lookahead ring.
pub(crate) struct VorbisStream {
    /// Compressed bytes, kept so the decoder can be recreated for a from-zero restart.
    bytes: Arc<[u8]>,
    reader: OggStreamReader<Cursor<Arc<[u8]>>>,
    channels: usize,
    sample_rate: u32,
    /// Source PCM frames: the last page granule, clamped to the decoded length at end of stream.
    frames: usize,
    /// Exact decoded frame index one past the last frame pulled from `reader`.
    output_position: usize,
    /// Interleaved decoded frames `[ring_start, ring_start + buffered_frames())`.
    ring: VecDeque<i16>,
    ring_start: usize,
    /// Data pages in file order, empty when page anchors cannot be trusted.
    pages: Vec<PageInfo>,
    /// Index into `pages` of the next boundary the reader is expected to cross.
    next_page: usize,
    /// Recorded page boundaries in increasing order.
    anchors: Vec<PageAnchor>,
    /// Whether `output_position` is exact (false right after a page seek until re-anchored).
    anchored: bool,
    last_absgp: Option<u64>,
    eof: bool,
}

impl VorbisStream {
    /// Creates a stream over `bytes`, validating its Vorbis headers.
    pub(crate) fn new(bytes: Vec<u8>) -> Result<Self, AudioError> {
        let bytes: Arc<[u8]> = bytes.into();
        let reader = OggStreamReader::new(Cursor::new(Arc::clone(&bytes))).map_err(map_error)?;
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
        let mut pages = scan_pages(&bytes);
        let last_granule = pages.last().map_or(0, |page| page.granule);
        // Chained or multiplexed streams reset their granule counter mid-file; page seeks are not
        // meaningful there, so disable anchors and restart those streams from the beginning.
        if !pages
            .windows(2)
            .all(|pair| pair[0].granule <= pair[1].granule)
        {
            pages.clear();
        }
        Ok(Self {
            bytes,
            reader,
            channels,
            sample_rate,
            frames: last_granule as usize,
            output_position: 0,
            ring: VecDeque::new(),
            ring_start: 0,
            pages,
            next_page: 0,
            anchors: Vec::new(),
            anchored: true,
            last_absgp: None,
            eof: false,
        })
    }

    /// Number of interleaved source channels (1 or 2).
    pub(crate) const fn channels(&self) -> usize {
        self.channels
    }

    /// Source sample rate in Hz.
    pub(crate) const fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Source PCM frames, matching `decode_vorbis`'s granule truncation.
    pub(crate) fn frames(&self) -> usize {
        self.frames
    }

    /// Decoded frames currently buffered in the ring.
    pub(crate) fn buffered_frames(&self) -> usize {
        self.ring.len() / self.channels
    }

    /// Number of page anchors recorded so far (test/diagnostic introspection).
    ///
    /// Anchors are keyed by page granule, so looping back over already-decoded pages never grows
    /// this table: it is bounded by the number of pages in the track.
    #[cfg(test)]
    pub(crate) fn anchor_count(&self) -> usize {
        self.anchors.len()
    }

    /// Bytes owned by the decoded lookahead state: the ring plus the anchor/page tables (the
    /// compressed input is counted separately by the caller). Bounded by the ring size, unlike
    /// the whole-track decode this replaced.
    #[cfg(test)]
    pub(crate) fn decoded_state_bytes(&self) -> usize {
        self.ring.capacity() * std::mem::size_of::<i16>()
            + self.anchors.capacity() * std::mem::size_of::<PageAnchor>()
            + self.pages.capacity() * std::mem::size_of::<PageInfo>()
    }

    /// Linearly interpolates the stereo frame at a fixed-point source position.
    ///
    /// Matches `StreamEntry::frame_at` from the whole-buffer implementation exactly: frame
    /// `position >> 32` and its successor are interpolated with a 32-bit fraction.
    pub(crate) fn frame_at(&mut self, position: u64) -> (i32, i32) {
        if self.frames == 0 {
            return (0, 0);
        }
        let frame = (position >> 32) as usize;
        if frame >= self.frames {
            return (0, 0);
        }
        self.ensure_available(frame);
        if frame < self.ring_start || frame >= self.ring_start + self.buffered_frames() {
            // The stream ended early or a decode error stopped it; mix silence.
            return (0, 0);
        }
        let fraction = position & 0xFFFF_FFFF;
        let next = (frame + 1).min(self.frames - 1);
        let lerp = |a: i16, b: i16| -> i32 {
            let weight = ((1u64 << 32) - fraction) as i64;
            let value = i64::from(a) * weight + i64::from(b) * fraction as i64;
            (value >> 32) as i32
        };
        let index = (frame - self.ring_start) * self.channels;
        let next_index = (next - self.ring_start) * self.channels;
        let left = lerp(self.ring[index], self.ring[next_index]);
        let right = if self.channels == 1 {
            left
        } else {
            lerp(self.ring[index + 1], self.ring[next_index + 1])
        };
        (left, right)
    }

    /// Makes sure the ring contains `frame` and its successor, decoding packets as needed.
    fn ensure_available(&mut self, frame: usize) {
        if frame < self.ring_start {
            self.restart_at(frame);
        }
        let end_frame = frame + 1;
        while self.ring_start + self.buffered_frames() <= end_frame && !self.eof {
            let Some(packet) = self.pull_packet() else {
                break;
            };
            self.append(&packet);
        }
        if self.eof && self.anchored {
            self.frames = self.frames.min(self.output_position);
        }
    }

    /// Pulls and decodes the next audio packet, updating the decoded position and page anchors.
    ///
    /// Returns `None` at end of stream or on a decode error (which stops the stream).
    fn pull_packet(&mut self) -> Option<Vec<i16>> {
        match self.reader.read_dec_packet_itl() {
            Ok(Some(packet)) => {
                self.output_position += packet.len() / self.channels;
                self.update_page_anchor();
                Some(packet)
            }
            Ok(None) | Err(_) => {
                self.eof = true;
                None
            }
        }
    }

    /// Records the decoded position of a page boundary the reader has just reached.
    ///
    /// `get_last_absgp` is the page granule once a page's last packet is read. The page list is
    /// advanced in step; the first boundary after a seek re-anchors `output_position` from a
    /// recorded anchor, and each new boundary is recorded once with the exact running position.
    fn update_page_anchor(&mut self) {
        let Some(granule) = self.reader.get_last_absgp() else {
            return;
        };
        if self.last_absgp == Some(granule) {
            return;
        }
        self.last_absgp = Some(granule);
        while self.next_page < self.pages.len() && self.pages[self.next_page].granule < granule {
            self.next_page += 1;
        }
        if self.next_page >= self.pages.len() || self.pages[self.next_page].granule != granule {
            return;
        }
        self.next_page += 1;
        if !self.anchored {
            if let Some(anchor) = self.anchors.iter().find(|anchor| anchor.granule == granule) {
                self.output_position = anchor.position;
                self.anchored = true;
            }
        } else if self
            .anchors
            .last()
            .is_none_or(|anchor| anchor.granule < granule)
        {
            // Only new (higher) granules are appended. Re-decoding pages after a loop or a
            // from-zero restart skips already-recorded granules instead of duplicating them,
            // so the anchor table stays sorted and bounded by the page count.
            self.anchors.push(PageAnchor {
                granule,
                position: self.output_position,
            });
        }
    }

    /// Appends decoded frames to the ring, dropping the oldest frames above the ring size.
    fn append(&mut self, samples: &[i16]) {
        let frames = samples.len() / self.channels;
        if frames == 0 {
            return;
        }
        let overflow = (self.buffered_frames() + frames).saturating_sub(STREAM_RING_FRAMES);
        if overflow > 0 {
            let drop = overflow.min(self.buffered_frames());
            self.ring.drain(..drop * self.channels);
            self.ring_start += drop;
        }
        self.ring.extend(samples.iter().copied());
    }

    /// Restarts decoding so the ring can serve `target` again.
    fn restart_at(&mut self, target: usize) {
        self.ring.clear();
        self.ring_start = target;
        self.eof = false;
        self.last_absgp = None;
        if target == 0 || !self.restart_from_anchor(target) {
            self.restart_from_zero(target);
        }
    }

    /// Restarts from a recorded page anchor at or before `target`; false when impossible.
    ///
    /// The seek targets an anchor two page records before the target: the first packet decoded
    /// after a seek has no overlap window, so its samples are not exact, and the packet after it
    /// can still belong to the same page. Seeking two pages back therefore guarantees at least
    /// one full page of exact lead-in before the target, leaving every packet at or after the
    /// target byte-identical to a whole-track decode.
    fn restart_from_anchor(&mut self, target: usize) -> bool {
        let Some(index) = self
            .anchors
            .iter()
            .rposition(|anchor| anchor.position <= target)
        else {
            return false;
        };
        // Decoding a couple of pages from zero is cheaper and always exact; the seek path needs
        // at least two pages of margin (see below).
        if index < 2 {
            return false;
        }
        let anchor = self.anchors[index - 2];
        if anchor.granule == 0 || self.reader.seek_absgp_pg(anchor.granule).is_err() {
            return false;
        }
        self.next_page = self
            .pages
            .partition_point(|page| page.granule < anchor.granule);
        self.anchored = false;
        self.output_position = anchor.position;
        loop {
            let Some(packet) = self.pull_packet() else {
                return false;
            };
            // `output_position` is meaningless until a recorded page boundary re-anchors it.
            if !self.anchored {
                continue;
            }
            let frames = packet.len() / self.channels;
            let end = self.output_position;
            let start = end.saturating_sub(frames);
            if end > target && start <= target {
                self.append(&packet[(target - start) * self.channels..]);
                return true;
            }
            if start > target {
                // The first anchored page ends after the target, so the target was decoded
                // before the decoder was re-anchored; a from-zero restart is guaranteed correct.
                return false;
            }
        }
    }

    /// Recreates the decoder and discards packets from frame 0 up to `target`.
    fn restart_from_zero(&mut self, target: usize) {
        let Ok(reader) =
            OggStreamReader::new(Cursor::new(Arc::clone(&self.bytes))).map_err(map_error)
        else {
            self.eof = true;
            return;
        };
        self.reader = reader;
        self.next_page = 0;
        self.anchored = true;
        self.output_position = 0;
        self.last_absgp = None;
        loop {
            let Some(packet) = self.pull_packet() else {
                return;
            };
            let frames = packet.len() / self.channels;
            let end = self.output_position;
            let start = end.saturating_sub(frames);
            if end > target && start <= target {
                self.append(&packet[(target - start) * self.channels..]);
                return;
            }
            if start > target {
                return;
            }
        }
    }
}

/// Scans Ogg page headers and returns pages with a positive granule position, in file order.
fn scan_pages(bytes: &[u8]) -> Vec<PageInfo> {
    let mut pages = Vec::new();
    let mut offset = 0usize;
    while offset + 27 <= bytes.len() {
        if &bytes[offset..offset + 4] != b"OggS" {
            break;
        }
        let granule = i64::from_le_bytes(
            bytes[offset + 6..offset + 14]
                .try_into()
                .expect("slice length checked"),
        );
        let segments = usize::from(bytes[offset + 26]);
        if offset + 27 + segments > bytes.len() {
            break;
        }
        let body: usize = bytes[offset + 27..offset + 27 + segments]
            .iter()
            .map(|size| usize::from(*size))
            .sum();
        if granule > 0 {
            pages.push(PageInfo {
                granule: granule as u64,
            });
        }
        offset += 27 + segments + body;
    }
    pages
}
