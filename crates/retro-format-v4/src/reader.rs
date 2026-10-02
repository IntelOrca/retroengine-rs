//! A small bounds-checked byte cursor with explicit endianness and RSDK string reads.

use crate::FormatError;

/// Bounds-checked reader over an in-memory byte slice.
///
/// RSDKv4 files are little-endian, so [`Reader::read_u16_le`], [`Reader::read_i32_le`] and
/// friends are used by the format modules; the big-endian variants exist for completeness and
/// for formats that mix byte orders (e.g. the GIF decoder used by a later work package).
///
/// Every read validates the requested range before advancing. No method panics, even for
/// pathological lengths such as `usize::MAX`.
#[derive(Debug, Clone)]
pub struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    /// Creates a reader positioned at the start of `bytes`.
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    /// The current offset in bytes from the start of the input.
    pub fn position(&self) -> usize {
        self.position
    }

    /// The number of bytes not yet consumed.
    pub fn remaining(&self) -> usize {
        self.bytes.len() - self.position
    }

    /// Whether all input has been consumed.
    pub fn is_empty(&self) -> bool {
        self.remaining() == 0
    }

    /// The complete input slice, independent of the cursor position.
    pub fn data(&self) -> &'a [u8] {
        self.bytes
    }

    /// The not-yet-consumed input slice.
    pub fn remaining_bytes(&self) -> &'a [u8] {
        &self.bytes[self.position..]
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], FormatError> {
        let end = self
            .position
            .checked_add(length)
            .ok_or(FormatError::Truncated)?;
        let slice = self
            .bytes
            .get(self.position..end)
            .ok_or(FormatError::Truncated)?;
        self.position = end;
        Ok(slice)
    }

    /// Reads `length` raw bytes.
    pub fn read_bytes(&mut self, length: usize) -> Result<&'a [u8], FormatError> {
        self.take(length)
    }

    /// Reads a fixed-size byte array.
    pub fn read_array<const N: usize>(&mut self) -> Result<[u8; N], FormatError> {
        let slice = self.take(N)?;
        let mut array = [0u8; N];
        array.copy_from_slice(slice);
        Ok(array)
    }

    /// Advances the cursor by `length` bytes without returning them.
    pub fn skip(&mut self, length: usize) -> Result<(), FormatError> {
        self.take(length).map(|_| ())
    }

    /// Reads one unsigned byte.
    pub fn read_u8(&mut self) -> Result<u8, FormatError> {
        let byte = *self
            .bytes
            .get(self.position)
            .ok_or(FormatError::Truncated)?;
        self.position += 1;
        Ok(byte)
    }

    /// Reads one signed byte.
    pub fn read_i8(&mut self) -> Result<i8, FormatError> {
        Ok(self.read_u8()? as i8)
    }

    /// Reads a little-endian `u16`.
    pub fn read_u16_le(&mut self) -> Result<u16, FormatError> {
        Ok(u16::from_le_bytes(self.read_array::<2>()?))
    }

    /// Reads a little-endian `i16`.
    pub fn read_i16_le(&mut self) -> Result<i16, FormatError> {
        Ok(i16::from_le_bytes(self.read_array::<2>()?))
    }

    /// Reads a little-endian `u32`.
    pub fn read_u32_le(&mut self) -> Result<u32, FormatError> {
        Ok(u32::from_le_bytes(self.read_array::<4>()?))
    }

    /// Reads a little-endian `i32`.
    pub fn read_i32_le(&mut self) -> Result<i32, FormatError> {
        Ok(i32::from_le_bytes(self.read_array::<4>()?))
    }

    /// Reads a little-endian `u64`.
    pub fn read_u64_le(&mut self) -> Result<u64, FormatError> {
        Ok(u64::from_le_bytes(self.read_array::<8>()?))
    }

    /// Reads a big-endian `u16`.
    pub fn read_u16_be(&mut self) -> Result<u16, FormatError> {
        Ok(u16::from_be_bytes(self.read_array::<2>()?))
    }

    /// Reads a big-endian `i16`.
    pub fn read_i16_be(&mut self) -> Result<i16, FormatError> {
        Ok(i16::from_be_bytes(self.read_array::<2>()?))
    }

    /// Reads a big-endian `u32`.
    pub fn read_u32_be(&mut self) -> Result<u32, FormatError> {
        Ok(u32::from_be_bytes(self.read_array::<4>()?))
    }

    /// Reads a big-endian `i32`.
    pub fn read_i32_be(&mut self) -> Result<i32, FormatError> {
        Ok(i32::from_be_bytes(self.read_array::<4>()?))
    }

    /// Reads an RSDK string: a `u8` length followed by that many UTF-8 bytes.
    ///
    /// The engine treats these as plain C strings; RSDKv4 assets are pure ASCII in every known
    /// file. Non-UTF-8 payloads are rejected with [`FormatError::Invalid`] instead of being
    /// silently replaced so that corrupt configuration files surface as errors.
    pub fn read_string(&mut self) -> Result<String, FormatError> {
        let length = self.read_u8()? as usize;
        let bytes = self.take(length)?;
        let text = std::str::from_utf8(bytes).map_err(|error| {
            FormatError::invalid(format!(
                "string at offset {} is not valid UTF-8: {error}",
                self.position - length
            ))
        })?;
        Ok(text.to_owned())
    }

    /// Reads a `u8`-length-prefixed byte string without UTF-8 validation.
    pub fn read_prefixed_bytes(&mut self) -> Result<&'a [u8], FormatError> {
        let length = self.read_u8()? as usize;
        self.take(length)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_unsigned_and_signed_bytes() {
        let mut reader = Reader::new(&[0x00, 0x7F, 0x80, 0xFF]);
        assert_eq!(reader.read_u8().unwrap(), 0x00);
        assert_eq!(reader.read_u8().unwrap(), 0x7F);
        assert_eq!(reader.read_u8().unwrap(), 0x80);
        assert_eq!(reader.read_u8().unwrap(), 0xFF);
        assert!(reader.is_empty());

        let mut reader = Reader::new(&[0xFF, 0x80]);
        assert_eq!(reader.read_i8().unwrap(), -1);
        assert_eq!(reader.read_i8().unwrap(), -128);
    }

    #[test]
    fn reads_explicit_endianness() {
        let mut reader = Reader::new(&[0x34, 0x12, 0xFF, 0xFE]);
        assert_eq!(reader.read_u16_le().unwrap(), 0x1234);
        assert_eq!(reader.read_i16_le().unwrap(), -257);

        let mut reader = Reader::new(&[0x12, 0x34, 0xFF, 0xFF, 0xFF, 0xFE]);
        assert_eq!(reader.read_u16_be().unwrap(), 0x1234);
        assert_eq!(reader.read_i32_be().unwrap(), -2);
    }

    #[test]
    fn reads_i32_and_u64_little_endian() {
        let mut reader = Reader::new(&[0x78, 0x56, 0x34, 0x12]);
        assert_eq!(reader.read_u32_le().unwrap(), 0x1234_5678);
        let mut reader = Reader::new(&[0xFF, 0xFF, 0xFF, 0xFF]);
        assert_eq!(reader.read_i32_le().unwrap(), -1);
        let mut reader = Reader::new(&[1, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(reader.read_u64_le().unwrap(), 1);
    }

    #[test]
    fn reads_arrays_and_skips() {
        let mut reader = Reader::new(&[1, 2, 3, 4, 5]);
        assert_eq!(reader.read_array::<2>().unwrap(), [1, 2]);
        reader.skip(2).unwrap();
        assert_eq!(reader.read_u8().unwrap(), 5);
        assert!(reader.skip(1).is_err());
    }

    #[test]
    fn truncation_reports_error_without_advancing() {
        let mut reader = Reader::new(&[0x01, 0x02]);
        assert!(matches!(reader.read_u32_le(), Err(FormatError::Truncated)));
        assert_eq!(reader.position(), 0);
        assert_eq!(reader.remaining(), 2);

        assert!(matches!(reader.read_bytes(3), Err(FormatError::Truncated)));
        assert!(matches!(
            reader.read_bytes(usize::MAX),
            Err(FormatError::Truncated)
        ));
        assert_eq!(reader.position(), 0);
    }

    #[test]
    fn reads_length_prefixed_strings() {
        let mut reader = Reader::new(&[5, b'H', b'e', b'l', b'l', b'o', 0]);
        assert_eq!(reader.read_string().unwrap(), "Hello");
        assert_eq!(reader.read_string().unwrap(), "");
        assert!(reader.is_empty());
    }

    #[test]
    fn string_errors() {
        let mut reader = Reader::new(&[5, b'a']);
        assert!(matches!(reader.read_string(), Err(FormatError::Truncated)));

        let mut reader = Reader::new(&[2, 0xC3, 0x28]);
        assert!(matches!(reader.read_string(), Err(FormatError::Invalid(_))));
    }

    #[test]
    fn prefixed_bytes_are_raw() {
        let mut reader = Reader::new(&[3, 0xFF, 0x00, 0x80]);
        assert_eq!(reader.read_prefixed_bytes().unwrap(), &[0xFF, 0x00, 0x80]);
        assert!(reader.is_empty());
    }

    #[test]
    fn never_panics_on_arbitrary_input() {
        let mut state = 0x9E37_79B9u32;
        for length in 0..128usize {
            let bytes: Vec<u8> = (0..length)
                .map(|_| {
                    state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    (state >> 24) as u8
                })
                .collect();
            let mut reader = Reader::new(&bytes);
            while !reader.is_empty() && reader.position() < bytes.len() {
                let position = reader.position();
                let _ = reader.read_u8();
                let _ = reader.read_i32_le();
                let _ = reader.read_string();
                let _ = reader.read_bytes(3);
                if reader.position() == position {
                    break;
                }
            }
        }
    }
}
