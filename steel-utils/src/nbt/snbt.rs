use std::{
    error::Error,
    fmt,
    num::{IntErrorKind, ParseIntError},
};

use simdnbt::{
    Mutf8String,
    owned::{NbtCompound, NbtList, NbtTag},
};
use uuid::Uuid;

use crate::{UuidExt, java};

/// Error returned when parsing SNBT text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnbtError {
    cursor: usize,
    message: String,
}

impl SnbtError {
    fn new(cursor: usize, message: impl Into<String>) -> Self {
        Self {
            cursor,
            message: message.into(),
        }
    }

    /// Returns the byte cursor where parsing failed.
    #[must_use]
    pub const fn cursor(&self) -> usize {
        self.cursor
    }

    /// Returns the parse failure message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for SnbtError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "SNBT parse error at byte {}: {}",
            self.cursor, self.message
        )
    }
}

impl Error for SnbtError {}

/// Parses one complete SNBT tag.
///
/// # Errors
///
/// Returns an error when the input is not valid SNBT or has trailing data.
pub fn parse_snbt(input: &str) -> Result<NbtTag, SnbtError> {
    let (tag, cursor) = parse_snbt_argument(input)?;
    let mut parser = Parser::new(input);
    parser.cursor = cursor;
    parser.skip_whitespace();
    if parser.can_read() {
        return Err(parser.error("trailing data"));
    }

    Ok(tag)
}

/// Parses one SNBT tag and returns the byte cursor consumed by that tag.
///
/// Unlike [`parse_snbt`], this does not consume trailing whitespace after the
/// tag. Command parsers use the returned cursor so the command graph can own
/// node-separating whitespace.
///
/// # Errors
///
/// Returns an error when the input does not start with a valid SNBT tag.
pub fn parse_snbt_argument(input: &str) -> Result<(NbtTag, usize), SnbtError> {
    let mut parser = Parser::new(input);
    let tag = parser.parse_tag()?;
    Ok((tag, parser.cursor))
}

/// Parses one complete SNBT compound.
///
/// # Errors
///
/// Returns an error when the input is not a valid SNBT compound or has trailing
/// data.
pub fn parse_snbt_compound(input: &str) -> Result<NbtCompound, SnbtError> {
    let (compound, cursor) = parse_snbt_compound_argument(input)?;
    let mut parser = Parser::new(input);
    parser.cursor = cursor;
    parser.skip_whitespace();
    if parser.can_read() {
        return Err(parser.error("trailing data"));
    }

    Ok(compound)
}

/// Parses one SNBT compound and returns the byte cursor consumed by it.
///
/// # Errors
///
/// Returns an error when the input does not start with a valid SNBT compound.
pub fn parse_snbt_compound_argument(input: &str) -> Result<(NbtCompound, usize), SnbtError> {
    let mut parser = Parser::new(input);
    let compound = parser.parse_compound()?;
    Ok((compound, parser.cursor))
}

struct Parser<'a> {
    input: &'a str,
    cursor: usize,
}

impl<'a> Parser<'a> {
    const fn new(input: &'a str) -> Self {
        Self { input, cursor: 0 }
    }

    const fn can_read(&self) -> bool {
        self.cursor < self.input.len()
    }

    fn error(&self, message: impl Into<String>) -> SnbtError {
        SnbtError::new(self.cursor, message)
    }

    fn error_at(cursor: usize, message: impl Into<String>) -> SnbtError {
        SnbtError::new(cursor, message)
    }

    fn peek(&self) -> Option<char> {
        self.input[self.cursor..].chars().next()
    }

    fn read(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.cursor += ch.len_utf8();
        Some(ch)
    }

    fn skip_whitespace(&mut self) {
        while self.peek().is_some_and(java::is_whitespace) {
            self.read();
        }
    }

    fn consume_char(&mut self, expected: char) -> bool {
        self.skip_whitespace();
        if self.peek() == Some(expected) {
            self.read();
            return true;
        }

        false
    }

    fn expect_char(&mut self, expected: char) -> Result<(), SnbtError> {
        if self.consume_char(expected) {
            return Ok(());
        }

        Err(self.error(format!("expected '{expected}'")))
    }

    fn parse_tag(&mut self) -> Result<NbtTag, SnbtError> {
        self.skip_whitespace();
        let Some(ch) = self.peek() else {
            return Err(self.error("expected tag"));
        };

        match ch {
            '{' => Ok(NbtTag::Compound(self.parse_compound()?)),
            '[' => self.parse_list_or_array(),
            '"' | '\'' => Ok(NbtTag::String(self.parse_quoted_string()?.into())),
            ch if can_start_number(ch) => self.parse_number(DefaultIntegerKind::Int),
            ch if is_allowed_in_unquoted_string(ch) => self.parse_unquoted_value(),
            _ => Err(self.error("expected tag")),
        }
    }

    fn parse_compound(&mut self) -> Result<NbtCompound, SnbtError> {
        self.expect_char('{')?;
        let mut compound = NbtCompound::new();
        if self.consume_char('}') {
            return Ok(compound);
        }

        loop {
            let key = self.parse_map_key()?;
            self.expect_char(':')?;
            let tag = self.parse_tag()?;
            compound.remove(&key);
            compound.insert(key, tag);

            if self.consume_char(',') {
                if self.consume_char('}') {
                    return Ok(compound);
                }
                continue;
            }

            self.expect_char('}')?;
            return Ok(compound);
        }
    }

    fn parse_map_key(&mut self) -> Result<String, SnbtError> {
        self.skip_whitespace();
        let key = match self.peek() {
            Some('"' | '\'') => self.parse_quoted_string()?,
            Some(ch) if is_allowed_in_unquoted_string(ch) => self.parse_unquoted_string()?,
            _ => return Err(self.error("expected compound key")),
        };

        if key.is_empty() {
            return Err(self.error("expected compound key"));
        }

        Ok(key)
    }

    fn parse_list_or_array(&mut self) -> Result<NbtTag, SnbtError> {
        self.expect_char('[')?;
        if self.consume_char(']') {
            return Ok(NbtTag::List(NbtList::Empty));
        }

        let prefix_cursor = self.cursor;
        self.skip_whitespace();
        if let Some(prefix) = self.peek().filter(|ch| matches!(ch, 'B' | 'I' | 'L')) {
            self.read();
            if self.consume_char(';') {
                return self.parse_typed_array(prefix);
            }
        }
        self.cursor = prefix_cursor;

        let mut tags = Vec::new();
        loop {
            tags.push(self.parse_tag()?);
            if self.consume_char(',') {
                if self.consume_char(']') {
                    break;
                }
                continue;
            }

            self.expect_char(']')?;
            break;
        }

        Ok(NbtTag::List(NbtList::from(tags)))
    }

    fn parse_typed_array(&mut self, prefix: char) -> Result<NbtTag, SnbtError> {
        match prefix {
            'B' => {
                let values =
                    self.parse_integer_array(DefaultIntegerKind::Byte, &[IntegerKind::Byte])?;
                Ok(NbtTag::ByteArray(
                    values.into_iter().map(|value| value as u8).collect(),
                ))
            }
            'I' => {
                let values = self.parse_integer_array(
                    DefaultIntegerKind::Int,
                    &[IntegerKind::Byte, IntegerKind::Short, IntegerKind::Int],
                )?;
                Ok(NbtTag::IntArray(
                    values.into_iter().map(|value| value as i32).collect(),
                ))
            }
            'L' => Ok(NbtTag::LongArray(self.parse_integer_array(
                DefaultIntegerKind::Long,
                &[
                    IntegerKind::Byte,
                    IntegerKind::Short,
                    IntegerKind::Int,
                    IntegerKind::Long,
                ],
            )?)),
            _ => Err(self.error("expected typed array")),
        }
    }

    fn parse_integer_array(
        &mut self,
        default_kind: DefaultIntegerKind,
        allowed_kinds: &[IntegerKind],
    ) -> Result<Vec<i64>, SnbtError> {
        let mut values = Vec::new();
        if self.consume_char(']') {
            return Ok(values);
        }

        loop {
            let cursor = self.cursor;
            let tag = self.parse_number(default_kind)?;
            let Some((kind, value)) = integer_tag_value(&tag) else {
                return Err(Self::error_at(cursor, "expected integer array element"));
            };
            if !allowed_kinds.contains(&kind) {
                return Err(Self::error_at(cursor, "invalid typed array element width"));
            }
            values.push(value);

            if self.consume_char(',') {
                if self.consume_char(']') {
                    return Ok(values);
                }
                continue;
            }

            self.expect_char(']')?;
            return Ok(values);
        }
    }

    fn parse_unquoted_value(&mut self) -> Result<NbtTag, SnbtError> {
        let start = self.cursor;
        let value = self.parse_unquoted_string()?;
        let after_value = self.cursor;

        self.skip_whitespace();
        if self.consume_char('(') {
            return self.parse_builtin(&value, start);
        }
        self.cursor = after_value;

        if value.eq_ignore_ascii_case("true") {
            Ok(NbtTag::Byte(1))
        } else if value.eq_ignore_ascii_case("false") {
            Ok(NbtTag::Byte(0))
        } else {
            Ok(NbtTag::String(Mutf8String::from(value)))
        }
    }

    fn parse_builtin(&mut self, name: &str, name_cursor: usize) -> Result<NbtTag, SnbtError> {
        match name {
            "bool" => {
                let value = self.parse_tag()?;
                self.expect_char(')')?;
                bool_tag_value(&value)
                    .map(|value| NbtTag::Byte(i8::from(value)))
                    .ok_or_else(|| Self::error_at(name_cursor, "bool expects a numeric tag"))
            }
            "uuid" => {
                let value = self.parse_tag()?;
                self.expect_char(')')?;
                let NbtTag::String(uuid) = value else {
                    return Err(Self::error_at(name_cursor, "uuid expects a string tag"));
                };
                let uuid = Uuid::parse_str(uuid.as_str().to_str().as_ref())
                    .map_err(|_| Self::error_at(name_cursor, "invalid UUID"))?;
                Ok(NbtTag::IntArray(uuid.to_int_array().to_vec()))
            }
            _ => Err(Self::error_at(
                name_cursor,
                format!("unknown SNBT function '{name}'"),
            )),
        }
    }

    fn parse_number(&mut self, default_kind: DefaultIntegerKind) -> Result<NbtTag, SnbtError> {
        let start = self.cursor;
        while self.peek().is_some_and(is_number_token_char) {
            self.read();
        }

        let token = &self.input[start..self.cursor];
        if token.is_empty() {
            return Err(Self::error_at(start, "expected number"));
        }

        parse_number_token(token, default_kind).map_err(|message| Self::error_at(start, message))
    }

    fn parse_quoted_string(&mut self) -> Result<String, SnbtError> {
        let quote_cursor = self.cursor;
        let Some(terminator @ ('"' | '\'')) = self.read() else {
            return Err(self.error("expected quoted string"));
        };

        let mut value = String::new();
        while let Some(ch) = self.read() {
            match ch {
                ch if ch == terminator => return Ok(value),
                '\\' => value.push(self.parse_escape()?),
                _ => value.push(ch),
            }
        }

        Err(Self::error_at(quote_cursor, "unclosed quoted string"))
    }

    fn parse_escape(&mut self) -> Result<char, SnbtError> {
        let escape_cursor = self.cursor;
        let Some(ch) = self.read() else {
            return Err(Self::error_at(escape_cursor, "unclosed escape sequence"));
        };

        match ch {
            'b' => Ok('\u{0008}'),
            's' => Ok(' '),
            't' => Ok('\t'),
            'n' => Ok('\n'),
            'f' => Ok('\u{000C}'),
            'r' => Ok('\r'),
            '\\' | '\'' | '"' => Ok(ch),
            'x' => self.parse_code_point_escape(2, escape_cursor),
            'u' => self.parse_code_point_escape(4, escape_cursor),
            'U' => self.parse_code_point_escape(8, escape_cursor),
            'N' => self.parse_named_escape(escape_cursor),
            _ => Err(Self::error_at(
                escape_cursor,
                format!("invalid escape '\\{ch}'"),
            )),
        }
    }

    fn parse_code_point_escape(
        &mut self,
        digits: usize,
        escape_cursor: usize,
    ) -> Result<char, SnbtError> {
        let mut value = 0_u32;
        for _ in 0..digits {
            let Some(ch) = self.read() else {
                return Err(Self::error_at(escape_cursor, "incomplete unicode escape"));
            };
            let Some(digit) = ch.to_digit(16) else {
                return Err(Self::error_at(escape_cursor, "invalid unicode escape"));
            };
            value = value * 16 + digit;
        }

        char::from_u32(value)
            .ok_or_else(|| Self::error_at(escape_cursor, "invalid unicode code point"))
    }

    fn parse_named_escape(&mut self, escape_cursor: usize) -> Result<char, SnbtError> {
        if self.read() != Some('{') {
            return Err(Self::error_at(
                escape_cursor,
                "expected unicode character name",
            ));
        }

        let name_start = self.cursor;
        while self.peek().is_some_and(|ch| ch != '}') {
            self.read();
        }
        if self.read() != Some('}') {
            return Err(Self::error_at(
                escape_cursor,
                "unclosed unicode character name",
            ));
        }

        let name = &self.input[name_start..self.cursor - 1];
        unicode_names2::character(name)
            .ok_or_else(|| Self::error_at(escape_cursor, format!("unknown unicode name '{name}'")))
    }

    fn parse_unquoted_string(&mut self) -> Result<String, SnbtError> {
        let start = self.cursor;
        while self.peek().is_some_and(is_allowed_in_unquoted_string) {
            self.read();
        }

        if self.cursor == start {
            return Err(Self::error_at(start, "expected unquoted string"));
        }

        Ok(self.input[start..self.cursor].to_owned())
    }
}

fn parse_number_token(token: &str, default_kind: DefaultIntegerKind) -> Result<NbtTag, String> {
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

fn parse_float_token(token: &str) -> Result<NbtTag, String> {
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
        .map_err(|_| "invalid floating-point literal".to_owned())?;
    if !value.is_finite() {
        return Err("floating-point literal must be finite".to_owned());
    }

    match kind {
        FloatKind::Float => {
            let value = value as f32;
            if !value.is_finite() {
                return Err("floating-point literal must be finite".to_owned());
            }
            Ok(NbtTag::Float(value))
        }
        FloatKind::Double => Ok(NbtTag::Double(value)),
    }
}

fn validate_float_underscore_placement(input: &str) -> Result<(), String> {
    let bytes = input.as_bytes();
    for (index, byte) in bytes.iter().copied().enumerate() {
        if byte != b'_' {
            continue;
        }
        let surrounded_by_digits = index > 0
            && index + 1 < bytes.len()
            && bytes[index - 1].is_ascii_digit()
            && bytes[index + 1].is_ascii_digit();
        if !surrounded_by_digits {
            return Err("invalid underscore placement in number literal".to_owned());
        }
    }
    Ok(())
}

fn parse_integer_token(token: &str, default_kind: DefaultIntegerKind) -> Result<NbtTag, String> {
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
        let Some(body) = lower.strip_suffix(suffix) else {
            continue;
        };
        let original_body = &token[..body.len()];
        if original_body.is_empty() {
            continue;
        }
        if let Ok(tag) = parse_integer_body(original_body, kind, signedness) {
            return Ok(tag);
        }
    }

    parse_integer_body(token, default_kind.into(), IntegerSignedness::Default)
}

fn parse_integer_body(
    body: &str,
    kind: IntegerKind,
    signedness: IntegerSignedness,
) -> Result<NbtTag, String> {
    let (negative, body) = match body.as_bytes().first().copied() {
        Some(b'-') => (true, &body[1..]),
        Some(b'+') => (false, &body[1..]),
        _ => (false, body),
    };
    if body.is_empty() {
        return Err("invalid integer literal".to_owned());
    }

    let (radix, digits) = if body.starts_with("0x") || body.starts_with("0X") {
        (16, &body[2..])
    } else if body.starts_with("0b") || body.starts_with("0B") {
        (2, &body[2..])
    } else {
        (10, body)
    };
    if digits.is_empty() {
        return Err("invalid integer literal".to_owned());
    }
    if radix == 10 && digits.len() > 1 && digits.starts_with('0') {
        return Err("integer literal cannot have leading zeroes".to_owned());
    }

    let digits = normalize_number_digits(digits)?;
    let signed = signedness == IntegerSignedness::Signed
        || negative
        || (radix == 10 && signedness != IntegerSignedness::Unsigned);
    if negative && signedness == IntegerSignedness::Unsigned {
        return Err("unsigned integer literal cannot be negative".to_owned());
    }

    if signed {
        let magnitude =
            i128::from_str_radix(&digits, radix).map_err(integer_parse_error_message)?;
        let value = if negative { -magnitude } else { magnitude };
        return kind.to_signed_tag(value);
    }

    let value = u128::from_str_radix(&digits, radix).map_err(integer_parse_error_message)?;
    kind.to_unsigned_tag(value)
}

fn integer_parse_error_message(error: ParseIntError) -> String {
    match error.kind() {
        IntErrorKind::PosOverflow | IntErrorKind::NegOverflow => "integer literal is too large",
        _ => "invalid integer literal",
    }
    .to_owned()
}

fn normalize_number_digits(input: &str) -> Result<String, String> {
    if input.is_empty() {
        return Err("invalid number literal".to_owned());
    }
    if input.starts_with('_') || input.ends_with('_') || input.contains("__") {
        return Err("invalid underscore placement in number literal".to_owned());
    }

    Ok(input.chars().filter(|ch| *ch != '_').collect())
}

fn integer_tag_value(tag: &NbtTag) -> Option<(IntegerKind, i64)> {
    match tag {
        NbtTag::Byte(value) => Some((IntegerKind::Byte, i64::from(*value))),
        NbtTag::Short(value) => Some((IntegerKind::Short, i64::from(*value))),
        NbtTag::Int(value) => Some((IntegerKind::Int, i64::from(*value))),
        NbtTag::Long(value) => Some((IntegerKind::Long, *value)),
        _ => None,
    }
}

fn bool_tag_value(tag: &NbtTag) -> Option<bool> {
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

const fn can_start_number(ch: char) -> bool {
    matches!(ch, '+' | '-' | '.' | '0'..='9')
}

const fn is_number_token_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '+' | '-' | '.')
}

const fn is_allowed_in_unquoted_string(ch: char) -> bool {
    matches!(ch, '0'..='9' | 'A'..='Z' | 'a'..='z' | '_' | '-' | '.' | '+')
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DefaultIntegerKind {
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
enum IntegerKind {
    Byte,
    Short,
    Int,
    Long,
}

impl IntegerKind {
    fn to_signed_tag(self, value: i128) -> Result<NbtTag, String> {
        match self {
            Self::Byte => {
                let value =
                    i8::try_from(value).map_err(|_| "byte literal is out of range".to_owned())?;
                Ok(NbtTag::Byte(value))
            }
            Self::Short => {
                let value =
                    i16::try_from(value).map_err(|_| "short literal is out of range".to_owned())?;
                Ok(NbtTag::Short(value))
            }
            Self::Int => {
                let value =
                    i32::try_from(value).map_err(|_| "int literal is out of range".to_owned())?;
                Ok(NbtTag::Int(value))
            }
            Self::Long => {
                let value =
                    i64::try_from(value).map_err(|_| "long literal is out of range".to_owned())?;
                Ok(NbtTag::Long(value))
            }
        }
    }

    fn to_unsigned_tag(self, value: u128) -> Result<NbtTag, String> {
        match self {
            Self::Byte => {
                if value > u128::from(u8::MAX) {
                    return Err("unsigned byte literal is out of range".to_owned());
                }
                Ok(NbtTag::Byte(value as u8 as i8))
            }
            Self::Short => {
                if value > u128::from(u16::MAX) {
                    return Err("unsigned short literal is out of range".to_owned());
                }
                Ok(NbtTag::Short(value as u16 as i16))
            }
            Self::Int => {
                if value > u128::from(u32::MAX) {
                    return Err("unsigned int literal is out of range".to_owned());
                }
                Ok(NbtTag::Int(value as u32 as i32))
            }
            Self::Long => {
                if value > u128::from(u64::MAX) {
                    return Err("unsigned long literal is out of range".to_owned());
                }
                Ok(NbtTag::Long(value as u64 as i64))
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn compound_tag(input: &str) -> NbtCompound {
        parse_snbt_compound(input).expect("compound parses")
    }

    #[test]
    fn parses_compounds_lists_and_trailing_commas() {
        let compound = compound_tag("{name:'steel', flags:[true,false,], nested:{value:1b,},}");

        assert_eq!(
            compound
                .string("name")
                .map(|value| value.to_str().into_owned()),
            Some("steel".to_owned())
        );
        assert_eq!(
            compound.get("flags"),
            Some(&NbtTag::List(NbtList::Byte(vec![1, 0])))
        );
        assert_eq!(
            compound
                .compound("nested")
                .and_then(|nested| nested.byte("value")),
            Some(1)
        );
    }

    #[test]
    fn parses_boolean_literals_case_insensitively() {
        let compound = compound_tag("{upper:TRUE,mixed:FaLsE}");

        assert_eq!(compound.byte("upper"), Some(1));
        assert_eq!(compound.byte("mixed"), Some(0));
    }

    #[test]
    fn duplicate_compound_keys_keep_last_value() {
        let compound = compound_tag("{value:1,value:2}");

        assert_eq!(compound.int("value"), Some(2));
        assert_eq!(compound.len(), 1);
    }

    #[test]
    fn parses_integer_widths_and_unsigned_literals() {
        let compound = compound_tag("{a:1b,b:2s,c:3,d:4l,e:0xFFuB,f:0b1010,g:1_000}");

        assert_eq!(compound.byte("a"), Some(1));
        assert_eq!(compound.short("b"), Some(2));
        assert_eq!(compound.int("c"), Some(3));
        assert_eq!(compound.long("d"), Some(4));
        assert_eq!(compound.byte("e"), Some(-1));
        assert_eq!(compound.int("f"), Some(10));
        assert_eq!(compound.int("g"), Some(1000));
    }

    #[test]
    fn parses_floating_point_literals() {
        let compound = compound_tag("{float:1.5f,double:2.5d,exponent:1e2,underscored:1_2.5}");

        assert_eq!(compound.float("float"), Some(1.5));
        assert_eq!(compound.double("double"), Some(2.5));
        assert_eq!(compound.double("exponent"), Some(100.0));
        assert_eq!(compound.double("underscored"), Some(12.5));
    }

    #[test]
    fn rejects_underscores_at_float_component_boundaries() {
        for literal in ["1_.0", "1._0", "1_e2", "1e_2", "1e+_2"] {
            let input = format!("{{value:{literal}}}");
            assert!(
                parse_snbt_compound(&input).is_err(),
                "{literal} should not parse"
            );
        }
    }

    #[test]
    fn parses_typed_arrays() {
        let compound = compound_tag("{bytes:[B;1b,255uB],ints:[I;1,2b,3s],longs:[L;1,2i,3l]}");

        assert_eq!(compound.byte_array("bytes"), Some([1, 255].as_slice()));
        assert_eq!(compound.int_array("ints"), Some([1, 2, 3].as_slice()));
        assert_eq!(compound.long_array("longs"), Some([1, 2, 3].as_slice()));
    }

    #[test]
    fn parses_builtins() {
        let uuid =
            Uuid::parse_str("123e4567-e89b-12d3-a456-426614174000").expect("uuid literal parses");
        let compound =
            compound_tag("{enabled:bool(1),id:uuid('123e4567-e89b-12d3-a456-426614174000')}");

        assert_eq!(compound.byte("enabled"), Some(1));
        assert_eq!(
            compound.int_array("id"),
            Some(uuid.to_int_array().as_slice())
        );
    }

    #[test]
    fn parses_string_escapes() {
        let compound = compound_tag(r#"{text:"\x41\u0042\U00000043\N{LATIN CAPITAL LETTER D}"}"#);

        assert_eq!(
            compound
                .string("text")
                .map(|value| value.to_str().into_owned()),
            Some("ABCD".to_owned())
        );
    }

    #[test]
    fn argument_parser_does_not_consume_trailing_whitespace() {
        let (tag, cursor) = parse_snbt_argument("{value:1} run").expect("tag parses");

        assert!(matches!(tag, NbtTag::Compound(_)));
        assert_eq!(cursor, "{value:1}".len());
    }

    #[test]
    fn full_parser_rejects_trailing_data() {
        let error = parse_snbt("{value:1} trailing").expect_err("trailing data should fail");

        assert_eq!(error.cursor(), "{value:1} ".len());
    }
}
