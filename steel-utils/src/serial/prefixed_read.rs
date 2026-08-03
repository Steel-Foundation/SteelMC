use std::io::{Cursor, Error, ErrorKind, Read, Result};

use crate::{
    codec::VarInt,
    serial::{PrefixedRead, ReadFrom},
};

/// Reads a Minecraft UTF-8 string whose decoded length is bounded in Java
/// UTF-16 code units.
///
/// Minecraft permits up to three encoded bytes per allowed UTF-16 code unit
/// and replaces malformed UTF-8 with U+FFFD while decoding.
///
/// # Errors
///
/// Returns an error when the encoded byte length is negative, exceeds the
/// maximum encoded length, is not fully available, or decodes to more than
/// `max_utf16_units` UTF-16 code units.
pub fn read_utf(data: &mut Cursor<&[u8]>, max_utf16_units: usize) -> Result<String> {
    let encoded_len = VarInt::read(data)?.0;
    let Ok(encoded_len) = usize::try_from(encoded_len) else {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "encoded string length is negative",
        ));
    };
    let Some(max_encoded_len) = max_utf16_units.checked_mul(3) else {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "maximum UTF-16 length is too large",
        ));
    };
    if encoded_len > max_encoded_len {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "encoded string length exceeds maximum",
        ));
    }

    let start = usize::try_from(data.position()).map_err(|_| {
        Error::new(
            ErrorKind::UnexpectedEof,
            "encoded string starts beyond the addressable input",
        )
    })?;
    let end = start.checked_add(encoded_len).ok_or_else(|| {
        Error::new(
            ErrorKind::UnexpectedEof,
            "encoded string length exceeds the addressable input",
        )
    })?;
    let Some(bytes) = data.get_ref().get(start..end) else {
        return Err(Error::new(
            ErrorKind::UnexpectedEof,
            "encoded string is truncated",
        ));
    };

    let decoded = decode_java_utf8_lossy(bytes);
    let end = u64::try_from(end).map_err(|_| {
        Error::new(
            ErrorKind::InvalidData,
            "encoded string end exceeds the cursor position range",
        )
    })?;
    data.set_position(end);
    if decoded.encode_utf16().count() > max_utf16_units {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "decoded string length exceeds maximum",
        ));
    }
    Ok(decoded)
}

fn decode_java_utf8_lossy(bytes: &[u8]) -> String {
    let mut decoded = String::with_capacity(bytes.len());
    let mut offset = 0;

    while offset < bytes.len() {
        let first = bytes[offset];
        if first.is_ascii() {
            decoded.push(char::from(first));
            offset += 1;
            continue;
        }

        let remaining = bytes.len() - offset;
        match first {
            0xC2..=0xDF => {
                if remaining >= 2 && is_utf8_continuation(bytes[offset + 1]) {
                    let code_point =
                        (u32::from(first & 0x1F) << 6) | u32::from(bytes[offset + 1] & 0x3F);
                    decoded.push(char::from_u32(code_point).unwrap_or(char::REPLACEMENT_CHARACTER));
                    offset += 2;
                } else {
                    decoded.push(char::REPLACEMENT_CHARACTER);
                    offset += 1;
                }
            }
            0xE0..=0xEF => {
                if remaining < 3 {
                    let malformed_len = if remaining == 1
                        || malformed_three_byte_prefix(first, bytes[offset + 1])
                    {
                        1
                    } else {
                        remaining
                    };
                    decoded.push(char::REPLACEMENT_CHARACTER);
                    offset += malformed_len;
                    continue;
                }

                let second = bytes[offset + 1];
                let third = bytes[offset + 2];
                if malformed_three_byte_sequence(first, second, third) {
                    decoded.push(char::REPLACEMENT_CHARACTER);
                    offset += malformed_three_byte_length(first, second);
                    continue;
                }

                let code_point = (u32::from(first & 0x0F) << 12)
                    | (u32::from(second & 0x3F) << 6)
                    | u32::from(third & 0x3F);
                if let Some(character) = char::from_u32(code_point) {
                    decoded.push(character);
                } else {
                    decoded.push(char::REPLACEMENT_CHARACTER);
                }
                offset += 3;
            }
            0xF0..=0xF7 => {
                if remaining < 4 {
                    let malformed_len = malformed_four_byte_prefix_length(
                        first,
                        bytes.get(offset + 1).copied(),
                        bytes.get(offset + 2).copied(),
                        remaining,
                    );
                    decoded.push(char::REPLACEMENT_CHARACTER);
                    offset += malformed_len;
                    continue;
                }

                let second = bytes[offset + 1];
                let third = bytes[offset + 2];
                let fourth = bytes[offset + 3];
                if malformed_four_byte_sequence(first, second, third, fourth) {
                    decoded.push(char::REPLACEMENT_CHARACTER);
                    offset += malformed_four_byte_length(first, second, third);
                    continue;
                }

                let code_point = (u32::from(first & 0x07) << 18)
                    | (u32::from(second & 0x3F) << 12)
                    | (u32::from(third & 0x3F) << 6)
                    | u32::from(fourth & 0x3F);
                decoded.push(char::from_u32(code_point).unwrap_or(char::REPLACEMENT_CHARACTER));
                offset += 4;
            }
            _ => {
                decoded.push(char::REPLACEMENT_CHARACTER);
                offset += 1;
            }
        }
    }

    decoded
}

const fn is_utf8_continuation(byte: u8) -> bool {
    byte & 0xC0 == 0x80
}

const fn malformed_three_byte_prefix(first: u8, second: u8) -> bool {
    (first == 0xE0 && second & 0xE0 != 0xA0) || !is_utf8_continuation(second)
}

const fn malformed_three_byte_sequence(first: u8, second: u8, third: u8) -> bool {
    malformed_three_byte_prefix(first, second) || !is_utf8_continuation(third)
}

const fn malformed_three_byte_length(first: u8, second: u8) -> usize {
    if malformed_three_byte_prefix(first, second) {
        1
    } else {
        2
    }
}

const fn malformed_four_byte_second(first: u8, second: u8) -> bool {
    (first == 0xF0 && (second < 0x90 || second > 0xBF))
        || (first == 0xF4 && (second < 0x80 || second > 0x8F))
        || first > 0xF4
        || !is_utf8_continuation(second)
}

const fn malformed_four_byte_prefix_length(
    first: u8,
    second: Option<u8>,
    third: Option<u8>,
    remaining: usize,
) -> usize {
    let Some(second) = second else {
        return 1;
    };
    if malformed_four_byte_second(first, second) {
        return 1;
    }
    if let Some(third) = third
        && !is_utf8_continuation(third)
    {
        return 2;
    }
    remaining
}

const fn malformed_four_byte_sequence(first: u8, second: u8, third: u8, fourth: u8) -> bool {
    malformed_four_byte_second(first, second)
        || !is_utf8_continuation(third)
        || !is_utf8_continuation(fourth)
}

const fn malformed_four_byte_length(first: u8, second: u8, third: u8) -> usize {
    if malformed_four_byte_second(first, second) {
        1
    } else if !is_utf8_continuation(third) {
        2
    } else {
        3
    }
}

impl PrefixedRead for String {
    fn read_prefixed_bound<P: TryInto<usize> + ReadFrom>(
        data: &mut Cursor<&[u8]>,
        bound: usize,
    ) -> Result<Self> {
        let len: usize = P::read(data)?
            .try_into()
            .map_err(|_| Error::other("Invalid Prefix"))?;

        if len > bound {
            Err(Error::other("To long"))?;
        }

        let mut buf = vec![0; len];
        data.read_exact(&mut buf)?;
        String::from_utf8(buf).map_err(Error::other)
    }
}

impl<T: ReadFrom> PrefixedRead for Vec<T> {
    fn read_prefixed_bound<P: TryInto<usize> + ReadFrom>(
        data: &mut Cursor<&[u8]>,
        bound: usize,
    ) -> Result<Self> {
        let len: usize = P::read(data)?
            .try_into()
            .map_err(|_| Error::other("Invalid Prefix"))?;

        if len > bound {
            Err(Error::other("To long"))?;
        }
        let mut items = Vec::with_capacity(len);
        for _ in 0..len {
            items.push(T::read(data)?);
        }
        Ok(items)
    }
}

impl<T: PrefixedRead> PrefixedRead for Option<T> {
    fn read_prefixed_bound<P: TryInto<usize> + ReadFrom>(
        data: &mut Cursor<&[u8]>,
        bound: usize,
    ) -> Result<Self> {
        if bool::read(data)? {
            Ok(Some(T::read_prefixed_bound::<P>(data, bound)?))
        } else {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, ErrorKind};

    use crate::{
        codec::VarInt,
        serial::{WriteTo as _, prefixed_read::read_utf},
    };

    fn encoded_string(bytes: &[u8]) -> Vec<u8> {
        let mut encoded = Vec::with_capacity(VarInt::MAX_SIZE + bytes.len());
        VarInt(i32::try_from(bytes.len()).expect("test string length should fit in a VarInt"))
            .write(&mut encoded)
            .expect("test string length should encode");
        encoded.extend_from_slice(bytes);
        encoded
    }

    #[test]
    fn truncated_string_leaves_cursor_after_prefix() {
        let bytes = [3, b'a'];
        let mut cursor = Cursor::new(bytes.as_slice());

        let error = read_utf(&mut cursor, 3).expect_err("the declared body is incomplete");

        assert_eq!(error.kind(), ErrorKind::UnexpectedEof);
        assert_eq!(cursor.position(), 1);
    }

    #[test]
    fn length_failures_match_vanilla_cursor_consumption() {
        let encoded_too_long = encoded_string(b"abcd");
        let mut cursor = Cursor::new(encoded_too_long.as_slice());
        let error = read_utf(&mut cursor, 1).expect_err("four encoded bytes exceed the bound");
        assert_eq!(error.kind(), ErrorKind::InvalidData);
        assert_eq!(cursor.position(), 1);

        let decoded_too_long = encoded_string(b"ab");
        let mut cursor = Cursor::new(decoded_too_long.as_slice());
        let error = read_utf(&mut cursor, 1).expect_err("two UTF-16 units exceed the bound");
        assert_eq!(error.kind(), ErrorKind::InvalidData);
        assert_eq!(cursor.position(), decoded_too_long.len() as u64);
    }

    #[test]
    fn accepts_maximum_encoded_length() {
        const MAX_UTF16_UNITS: usize = 32_767;
        let bytes = [0xE0, 0xA0, 0x80].repeat(MAX_UTF16_UNITS);
        let encoded = encoded_string(&bytes);
        let mut cursor = Cursor::new(encoded.as_slice());

        let decoded =
            read_utf(&mut cursor, MAX_UTF16_UNITS).expect("the exact encoded bound is valid");

        assert_eq!(decoded.encode_utf16().count(), MAX_UTF16_UNITS);
        assert_eq!(cursor.position(), encoded.len() as u64);
    }

    #[test]
    fn malformed_sequences_match_java_replacement_grouping() {
        let cases: &[(&[u8], &str)] = &[
            (&[0xC0, 0x80], "\u{FFFD}\u{FFFD}"),
            (&[0xE0, 0x80, 0x80], "\u{FFFD}\u{FFFD}\u{FFFD}"),
            (&[0xED, 0xA0, 0x80], "\u{FFFD}"),
            (&[0xE1, 0x80], "\u{FFFD}"),
            (&[0xE1, 0x80, b'A'], "\u{FFFD}A"),
            (&[0xF0, 0x90, 0x80], "\u{FFFD}"),
            (
                &[0xF4, 0x90, 0x80, 0x80],
                "\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}",
            ),
        ];

        for &(bytes, expected) in cases {
            let encoded = encoded_string(bytes);
            let decoded = read_utf(&mut Cursor::new(encoded.as_slice()), 16)
                .expect("malformed UTF-8 is decoded lossily");
            assert_eq!(decoded, expected, "unexpected decoding for {bytes:X?}");
        }
    }

    #[test]
    fn rejects_impossible_maximum_without_reading_the_body() {
        let encoded = encoded_string(b"");
        let mut cursor = Cursor::new(encoded.as_slice());

        let error =
            read_utf(&mut cursor, usize::MAX).expect_err("the encoded bound should overflow");

        assert_eq!(error.kind(), ErrorKind::InvalidInput);
        assert_eq!(cursor.position(), 1);
    }
}
