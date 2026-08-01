use std::num::{IntErrorKind, ParseIntError};

use simdnbt::owned::NbtTag;

use super::error::{SnbtErrorKind, SnbtNumberType};

pub(super) fn parse_number_token(
    token: &str,
    default_kind: DefaultIntegerKind,
) -> Result<NbtTag, SnbtErrorKind> {
    if should_parse_as_float(token) {
        return parse_float_token(token);
    }

    parse_integer_token(token, default_kind)
}

fn should_parse_as_float(token: &str) -> bool {
    if has_radix_prefix(token) {
        return false;
    }

    token.contains('.')
        || token.contains('e')
        || token.contains('E')
        || token.ends_with(['f', 'F', 'd', 'D'])
}

fn has_radix_prefix(token: &str) -> bool {
    let stripped = token
        .strip_prefix(['+', '-'])
        .map_or(token, |stripped| stripped);
    stripped.starts_with("0x")
        || stripped.starts_with("0X")
        || stripped.starts_with("0b")
        || stripped.starts_with("0B")
}

fn parse_float_token(token: &str) -> Result<NbtTag, SnbtErrorKind> {
    let (body, kind) = if token.ends_with(['f', 'F']) {
        (&token[..token.len() - 1], FloatKind::Float)
    } else if token.ends_with(['d', 'D']) {
        (&token[..token.len() - 1], FloatKind::Double)
    } else {
        (token, FloatKind::Double)
    };
    validate_float_underscore_placement(body)?;
    let body = normalize_number_digits(body)?;
    let value = body
        .parse::<f64>()
        .map_err(|_| SnbtErrorKind::InvalidFloatingPoint)?;
    if !value.is_finite() {
        return Err(SnbtErrorKind::NonFiniteNumber);
    }

    match kind {
        FloatKind::Float => {
            let value = value as f32;
            if !value.is_finite() {
                return Err(SnbtErrorKind::NonFiniteNumber);
            }
            Ok(NbtTag::Float(value))
        }
        FloatKind::Double => Ok(NbtTag::Double(value)),
    }
}

const fn validate_float_underscore_placement(input: &str) -> Result<(), SnbtErrorKind> {
    let bytes = input.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'_' {
            index += 1;
            continue;
        }

        let run_start = index;
        while index < bytes.len() && bytes[index] == b'_' {
            index += 1;
        }
        let surrounded_by_digits = run_start > 0
            && index < bytes.len()
            && bytes[run_start - 1].is_ascii_digit()
            && bytes[index].is_ascii_digit();
        if !surrounded_by_digits {
            return Err(SnbtErrorKind::InvalidUnderscore);
        }
    }
    Ok(())
}

fn parse_integer_token(
    token: &str,
    default_kind: DefaultIntegerKind,
) -> Result<NbtTag, SnbtErrorKind> {
    const SUFFIXES: &[(&str, IntegerKind, IntegerSignedness)] = &[
        ("ub", IntegerKind::Byte, IntegerSignedness::Unsigned),
        ("us", IntegerKind::Short, IntegerSignedness::Unsigned),
        ("ui", IntegerKind::Int, IntegerSignedness::Unsigned),
        ("ul", IntegerKind::Long, IntegerSignedness::Unsigned),
        ("sb", IntegerKind::Byte, IntegerSignedness::Signed),
        ("ss", IntegerKind::Short, IntegerSignedness::Signed),
        ("si", IntegerKind::Int, IntegerSignedness::Signed),
        ("sl", IntegerKind::Long, IntegerSignedness::Signed),
        ("b", IntegerKind::Byte, IntegerSignedness::Default),
        ("s", IntegerKind::Short, IntegerSignedness::Default),
        ("i", IntegerKind::Int, IntegerSignedness::Default),
        ("l", IntegerKind::Long, IntegerSignedness::Default),
    ];

    let lower = token.to_ascii_lowercase();
    for &(suffix, kind, signedness) in SUFFIXES {
        // Vanilla's hex numeral rule consumes `b` as a digit before suffix parsing.
        if suffix == "b" && has_hex_radix_prefix(token) {
            continue;
        }
        let Some(body) = lower.strip_suffix(suffix) else {
            continue;
        };
        let original_body = &token[..body.len()];
        if original_body.is_empty() {
            continue;
        }
        return parse_integer_body(original_body, kind, signedness);
    }

    parse_integer_body(token, default_kind.into(), IntegerSignedness::Default)
}

fn parse_integer_body(
    body: &str,
    kind: IntegerKind,
    signedness: IntegerSignedness,
) -> Result<NbtTag, SnbtErrorKind> {
    let (negative, body) = match body.as_bytes().first().copied() {
        Some(b'-') => (true, &body[1..]),
        Some(b'+') => (false, &body[1..]),
        _ => (false, body),
    };
    if body.is_empty() {
        return Err(SnbtErrorKind::InvalidInteger);
    }

    let (radix, digits) = if body.starts_with("0x") || body.starts_with("0X") {
        (16, &body[2..])
    } else if body.starts_with("0b") || body.starts_with("0B") {
        (2, &body[2..])
    } else {
        (10, body)
    };
    if digits.is_empty() {
        return Err(SnbtErrorKind::InvalidInteger);
    }
    if radix == 10 && digits.len() > 1 && digits.starts_with('0') {
        return Err(SnbtErrorKind::LeadingZero);
    }

    let digits = normalize_number_digits(digits)?;
    let signed = signedness == IntegerSignedness::Signed
        || (radix == 10 && signedness != IntegerSignedness::Unsigned);
    if negative && !signed {
        return Err(SnbtErrorKind::ExpectedNonNegativeNumber);
    }

    if signed {
        let magnitude = i128::from_str_radix(&digits, radix).map_err(integer_parse_error_kind)?;
        let value = if negative { -magnitude } else { magnitude };
        return kind.to_signed_tag(value);
    }

    let value = u128::from_str_radix(&digits, radix).map_err(integer_parse_error_kind)?;
    kind.to_unsigned_tag(value)
}

const fn integer_parse_error_kind(error: ParseIntError) -> SnbtErrorKind {
    match error.kind() {
        IntErrorKind::PosOverflow | IntErrorKind::NegOverflow => SnbtErrorKind::IntegerTooLarge,
        _ => SnbtErrorKind::InvalidInteger,
    }
}

fn normalize_number_digits(input: &str) -> Result<String, SnbtErrorKind> {
    if input.is_empty() {
        return Err(SnbtErrorKind::InvalidNumber);
    }
    if input.starts_with('_') || input.ends_with('_') {
        return Err(SnbtErrorKind::InvalidUnderscore);
    }

    Ok(input.chars().filter(|ch| *ch != '_').collect())
}

fn has_hex_radix_prefix(token: &str) -> bool {
    let stripped = token
        .strip_prefix(['+', '-'])
        .map_or(token, |stripped| stripped);
    stripped.starts_with("0x") || stripped.starts_with("0X")
}

pub(super) fn integer_tag_value(tag: &NbtTag) -> Option<(IntegerKind, i64)> {
    match tag {
        NbtTag::Byte(value) => Some((IntegerKind::Byte, i64::from(*value))),
        NbtTag::Short(value) => Some((IntegerKind::Short, i64::from(*value))),
        NbtTag::Int(value) => Some((IntegerKind::Int, i64::from(*value))),
        NbtTag::Long(value) => Some((IntegerKind::Long, *value)),
        _ => None,
    }
}

pub(super) fn bool_tag_value(tag: &NbtTag) -> Option<bool> {
    match tag {
        NbtTag::Byte(value) => Some(*value != 0),
        NbtTag::Short(value) => Some(*value != 0),
        NbtTag::Int(value) => Some(*value != 0),
        NbtTag::Long(value) => Some(*value != 0),
        NbtTag::Float(value) => Some(*value != 0.0),
        NbtTag::Double(value) => Some(*value != 0.0),
        _ => None,
    }
}

pub(super) const fn can_start_number(ch: char) -> bool {
    matches!(ch, '+' | '-' | '.' | '0'..='9')
}

pub(super) struct NumberScanError {
    pub(super) cursor: usize,
    pub(super) kind: SnbtErrorKind,
}

impl NumberScanError {
    const fn new(cursor: usize, kind: SnbtErrorKind) -> Self {
        Self { cursor, kind }
    }
}

pub(super) fn scan_number_token(input: &str, allow_float: bool) -> Result<usize, NumberScanError> {
    let bytes = input.as_bytes();
    let has_sign = matches!(bytes.first(), Some(b'+' | b'-'));
    let mut cursor = usize::from(has_sign);
    let Some(&first) = bytes.get(cursor) else {
        return Err(NumberScanError::new(
            cursor,
            SnbtErrorKind::ExpectedDecimalNumeral,
        ));
    };

    if first == b'.' {
        if !allow_float {
            return Err(NumberScanError::new(0, SnbtErrorKind::ExpectedNumber));
        }

        cursor += 1;
        cursor = scan_required_numeral(
            bytes,
            cursor,
            |byte| byte.is_ascii_digit(),
            SnbtErrorKind::ExpectedDecimalNumeral,
        )?;
        cursor = scan_optional_exponent(bytes, cursor);
        return Ok(cursor + float_suffix_len(&bytes[cursor..]));
    }

    if !first.is_ascii_digit() {
        return Err(NumberScanError::new(
            cursor,
            if has_sign {
                SnbtErrorKind::ExpectedDecimalNumeral
            } else {
                SnbtErrorKind::ExpectedNumber
            },
        ));
    }

    if first == b'0' {
        if matches!(bytes.get(cursor + 1), Some(b'x' | b'X')) {
            cursor += 2;
            cursor = scan_required_numeral(
                bytes,
                cursor,
                |byte| byte.is_ascii_hexdigit(),
                SnbtErrorKind::ExpectedHexNumeral,
            )?;
            return Ok(cursor + integer_suffix_len(&bytes[cursor..]));
        }
        if matches!(bytes.get(cursor + 1), Some(b'b' | b'B'))
            && matches!(bytes.get(cursor + 2), Some(b'0' | b'1' | b'_'))
        {
            cursor += 2;
            cursor = scan_required_numeral(
                bytes,
                cursor,
                |byte| matches!(byte, b'0' | b'1'),
                SnbtErrorKind::ExpectedBinaryNumeral,
            )?;
            return Ok(cursor + integer_suffix_len(&bytes[cursor..]));
        }
    }

    let numeral_start = cursor;
    cursor = scan_required_numeral(
        bytes,
        cursor,
        |byte| byte.is_ascii_digit(),
        SnbtErrorKind::ExpectedDecimalNumeral,
    )?;

    if allow_float {
        match bytes.get(cursor) {
            Some(b'.') => {
                cursor += 1;
                cursor =
                    try_scan_numeral(bytes, cursor, |byte| byte.is_ascii_digit()).unwrap_or(cursor);
                cursor = scan_optional_exponent(bytes, cursor);
                return Ok(cursor + float_suffix_len(&bytes[cursor..]));
            }
            Some(b'e' | b'E') => {
                if let Some(exponent_end) = try_scan_exponent(bytes, cursor) {
                    cursor = exponent_end;
                    return Ok(cursor + float_suffix_len(&bytes[cursor..]));
                }
            }
            Some(b'f' | b'F' | b'd' | b'D') => return Ok(cursor + 1),
            _ => {}
        }
    }

    let digit_count = bytes[numeral_start..cursor]
        .iter()
        .filter(|byte| **byte != b'_')
        .count();
    if first == b'0' && digit_count > 1 {
        return Err(NumberScanError::new(cursor, SnbtErrorKind::LeadingZero));
    }

    Ok(cursor + integer_suffix_len(&bytes[cursor..]))
}

fn scan_required_numeral(
    bytes: &[u8],
    start: usize,
    accepts_digit: impl Fn(u8) -> bool,
    expected: SnbtErrorKind,
) -> Result<usize, NumberScanError> {
    let mut cursor = start;
    while bytes
        .get(cursor)
        .is_some_and(|byte| accepts_digit(*byte) || *byte == b'_')
    {
        cursor += 1;
    }

    if cursor == start {
        return Err(NumberScanError::new(start, expected));
    }
    if bytes[start] == b'_' || bytes[cursor - 1] == b'_' {
        return Err(NumberScanError::new(
            start,
            SnbtErrorKind::InvalidUnderscore,
        ));
    }

    Ok(cursor)
}

fn try_scan_numeral(
    bytes: &[u8],
    start: usize,
    accepts_digit: impl Fn(u8) -> bool,
) -> Option<usize> {
    scan_required_numeral(
        bytes,
        start,
        accepts_digit,
        SnbtErrorKind::ExpectedDecimalNumeral,
    )
    .ok()
}

fn scan_optional_exponent(bytes: &[u8], cursor: usize) -> usize {
    try_scan_exponent(bytes, cursor).unwrap_or(cursor)
}

fn try_scan_exponent(bytes: &[u8], cursor: usize) -> Option<usize> {
    if !matches!(bytes.get(cursor), Some(b'e' | b'E')) {
        return None;
    }

    let numeral_start =
        cursor + 1 + usize::from(matches!(bytes.get(cursor + 1), Some(b'+' | b'-')));
    try_scan_numeral(bytes, numeral_start, |byte| byte.is_ascii_digit())
}

const fn float_suffix_len(bytes: &[u8]) -> usize {
    matches!(bytes.first(), Some(b'f' | b'F' | b'd' | b'D')) as usize
}

fn integer_suffix_len(bytes: &[u8]) -> usize {
    if matches!(bytes.first(), Some(b'u' | b'U' | b's' | b'S'))
        && matches!(
            bytes.get(1),
            Some(b'b' | b'B' | b's' | b'S' | b'i' | b'I' | b'l' | b'L')
        )
    {
        return 2;
    }
    usize::from(matches!(
        bytes.first(),
        Some(b'b' | b'B' | b's' | b'S' | b'i' | b'I' | b'l' | b'L')
    ))
}

pub(super) fn is_unsuffixed_decimal_integer_token(token: &str) -> bool {
    let digits = token
        .strip_prefix(['+', '-'])
        .map_or(token, |stripped| stripped);
    !digits.is_empty()
        && digits
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'_')
}

pub(super) const fn is_allowed_in_unquoted_string(ch: char) -> bool {
    matches!(ch, '0'..='9' | 'A'..='Z' | 'a'..='z' | '_' | '-' | '.' | '+')
}

pub(super) const fn is_allowed_in_unicode_name(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '-' | ' ')
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DefaultIntegerKind {
    Byte,
    Int,
    Long,
}

impl From<DefaultIntegerKind> for IntegerKind {
    fn from(value: DefaultIntegerKind) -> Self {
        match value {
            DefaultIntegerKind::Byte => Self::Byte,
            DefaultIntegerKind::Int => Self::Int,
            DefaultIntegerKind::Long => Self::Long,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum IntegerKind {
    Byte,
    Short,
    Int,
    Long,
}

impl IntegerKind {
    fn to_signed_tag(self, value: i128) -> Result<NbtTag, SnbtErrorKind> {
        match self {
            Self::Byte => {
                let value = i8::try_from(value).map_err(|_| SnbtErrorKind::NumberOutOfRange {
                    number_type: self.into(),
                    unsigned: false,
                })?;
                Ok(NbtTag::Byte(value))
            }
            Self::Short => {
                let value = i16::try_from(value).map_err(|_| SnbtErrorKind::NumberOutOfRange {
                    number_type: self.into(),
                    unsigned: false,
                })?;
                Ok(NbtTag::Short(value))
            }
            Self::Int => {
                let value = i32::try_from(value).map_err(|_| SnbtErrorKind::NumberOutOfRange {
                    number_type: self.into(),
                    unsigned: false,
                })?;
                Ok(NbtTag::Int(value))
            }
            Self::Long => {
                let value = i64::try_from(value).map_err(|_| SnbtErrorKind::NumberOutOfRange {
                    number_type: self.into(),
                    unsigned: false,
                })?;
                Ok(NbtTag::Long(value))
            }
        }
    }

    fn to_unsigned_tag(self, value: u128) -> Result<NbtTag, SnbtErrorKind> {
        match self {
            Self::Byte => {
                if value > u128::from(u8::MAX) {
                    return Err(SnbtErrorKind::NumberOutOfRange {
                        number_type: self.into(),
                        unsigned: true,
                    });
                }
                Ok(NbtTag::Byte(value as u8 as i8))
            }
            Self::Short => {
                if value > u128::from(u16::MAX) {
                    return Err(SnbtErrorKind::NumberOutOfRange {
                        number_type: self.into(),
                        unsigned: true,
                    });
                }
                Ok(NbtTag::Short(value as u16 as i16))
            }
            Self::Int => {
                if value > u128::from(u32::MAX) {
                    return Err(SnbtErrorKind::NumberOutOfRange {
                        number_type: self.into(),
                        unsigned: true,
                    });
                }
                Ok(NbtTag::Int(value as u32 as i32))
            }
            Self::Long => {
                if value > u128::from(u64::MAX) {
                    return Err(SnbtErrorKind::NumberOutOfRange {
                        number_type: self.into(),
                        unsigned: true,
                    });
                }
                Ok(NbtTag::Long(value as u64 as i64))
            }
        }
    }
}

impl From<IntegerKind> for SnbtNumberType {
    fn from(value: IntegerKind) -> Self {
        match value {
            IntegerKind::Byte => Self::Byte,
            IntegerKind::Short => Self::Short,
            IntegerKind::Int => Self::Int,
            IntegerKind::Long => Self::Long,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IntegerSignedness {
    Default,
    Signed,
    Unsigned,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FloatKind {
    Float,
    Double,
}
