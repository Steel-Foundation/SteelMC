use simdnbt::{
    Mutf8String,
    owned::{NbtCompound, NbtList, NbtTag},
};
use uuid::Uuid;

use crate::{UuidExt, java};

use super::{
    error::{SnbtError, SnbtErrorKind},
    number::{
        DefaultIntegerKind, IntegerKind, bool_tag_value, can_start_number, integer_tag_value,
        is_allowed_in_unicode_name, is_allowed_in_unquoted_string,
        is_unsuffixed_decimal_integer_token, parse_number_token, scan_number_token,
    },
};

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
        return Err(parser.error(SnbtErrorKind::TrailingData));
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
    match parser.parse_tag() {
        Ok(tag) => Ok((tag, parser.cursor)),
        Err(error) => Err(parser.resolve_error(error)),
    }
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
        return Err(parser.error(SnbtErrorKind::TrailingData));
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
    match parser.parse_compound() {
        Ok(compound) => Ok((compound, parser.cursor)),
        Err(error) => Err(parser.resolve_error(error)),
    }
}
struct Parser<'a> {
    input: &'a str,
    cursor: usize,
    recorded_error: Option<SnbtError>,
}

impl<'a> Parser<'a> {
    const fn new(input: &'a str) -> Self {
        Self {
            input,
            cursor: 0,
            recorded_error: None,
        }
    }

    const fn can_read(&self) -> bool {
        self.cursor < self.input.len()
    }

    const fn error(&self, kind: SnbtErrorKind) -> SnbtError {
        SnbtError::new(self.cursor, kind)
    }

    const fn error_at(cursor: usize, kind: SnbtErrorKind) -> SnbtError {
        SnbtError::new(cursor, kind)
    }

    fn record_error(&mut self, cursor: usize, kind: SnbtErrorKind) {
        if self
            .recorded_error
            .as_ref()
            .is_none_or(|error| cursor > error.cursor())
        {
            self.recorded_error = Some(Self::error_at(cursor, kind));
        }
    }

    fn resolve_error(&mut self, error: SnbtError) -> SnbtError {
        match self.recorded_error.take() {
            Some(recorded) if recorded.cursor() >= error.cursor() => recorded,
            _ => error,
        }
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

    fn consume_repeated_separator(&mut self, separator: char) -> bool {
        self.skip_whitespace();
        if self.peek() == Some(separator) {
            self.read();
            return true;
        }

        self.record_error(self.cursor, SnbtErrorKind::ExpectedSymbol(separator));
        false
    }

    fn expect_char(&mut self, expected: char) -> Result<(), SnbtError> {
        if self.consume_char(expected) {
            return Ok(());
        }

        Err(self.error(SnbtErrorKind::ExpectedSymbol(expected)))
    }

    fn parse_tag(&mut self) -> Result<NbtTag, SnbtError> {
        self.skip_whitespace();
        let Some(ch) = self.peek() else {
            return Err(self.error(SnbtErrorKind::ExpectedValue));
        };

        match ch {
            '{' => Ok(NbtTag::Compound(self.parse_compound()?)),
            '[' => self.parse_list_or_array(),
            '"' | '\'' => Ok(NbtTag::String(self.parse_quoted_string()?.into())),
            ch if can_start_number(ch) => self.parse_number(DefaultIntegerKind::Int, true),
            ch if is_allowed_in_unquoted_string(ch) => self.parse_unquoted_value(),
            _ => Err(self.error(SnbtErrorKind::ExpectedValue)),
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

            if self.consume_repeated_separator(',') {
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
            _ => return Err(self.error(SnbtErrorKind::ExpectedKey)),
        };

        if key.is_empty() {
            return Err(self.error(SnbtErrorKind::EmptyKey));
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
        let array_type = match self.peek() {
            Some('B') => Some(TypedArrayKind::Byte),
            Some('I') => Some(TypedArrayKind::Int),
            Some('L') => Some(TypedArrayKind::Long),
            _ => None,
        };
        if let Some(array_type) = array_type {
            self.read();
            if self.consume_char(';') {
                return self.parse_typed_array(array_type);
            }
        }
        self.cursor = prefix_cursor;

        let mut tags = Vec::new();
        loop {
            tags.push(self.parse_tag()?);
            if self.consume_repeated_separator(',') {
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

    fn parse_typed_array(&mut self, array_type: TypedArrayKind) -> Result<NbtTag, SnbtError> {
        match array_type {
            TypedArrayKind::Byte => {
                let values =
                    self.parse_integer_array(DefaultIntegerKind::Byte, &[IntegerKind::Byte])?;
                Ok(NbtTag::ByteArray(
                    values.into_iter().map(|value| value as u8).collect(),
                ))
            }
            TypedArrayKind::Int => {
                let values = self.parse_integer_array(
                    DefaultIntegerKind::Int,
                    &[IntegerKind::Byte, IntegerKind::Short, IntegerKind::Int],
                )?;
                Ok(NbtTag::IntArray(
                    values.into_iter().map(|value| value as i32).collect(),
                ))
            }
            TypedArrayKind::Long => Ok(NbtTag::LongArray(self.parse_integer_array(
                DefaultIntegerKind::Long,
                &[
                    IntegerKind::Byte,
                    IntegerKind::Short,
                    IntegerKind::Int,
                    IntegerKind::Long,
                ],
            )?)),
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
            let tag = self.parse_number(default_kind, false)?;
            let Some((kind, value)) = integer_tag_value(&tag) else {
                return Err(Self::error_at(cursor, SnbtErrorKind::ExpectedArrayElement));
            };
            if !allowed_kinds.contains(&kind) {
                return Err(Self::error_at(
                    cursor,
                    SnbtErrorKind::InvalidArrayElementType,
                ));
            }
            values.push(value);

            if self.consume_repeated_separator(',') {
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
        let value = self.parse_unquoted_string()?;
        let after_value = self.cursor;

        self.skip_whitespace();
        if self.peek() == Some('(') {
            self.read();
            return self.parse_builtin(&value);
        }
        self.record_error(self.cursor, SnbtErrorKind::ExpectedSymbol('('));
        self.cursor = after_value;

        if value.eq_ignore_ascii_case("true") {
            Ok(NbtTag::Byte(1))
        } else if value.eq_ignore_ascii_case("false") {
            Ok(NbtTag::Byte(0))
        } else {
            Ok(NbtTag::String(Mutf8String::from(value)))
        }
    }

    fn parse_builtin(&mut self, name: &str) -> Result<NbtTag, SnbtError> {
        let arguments = self.parse_builtin_arguments()?;
        let error_cursor = self.cursor;

        if name == "bool" && arguments.len() == 1 {
            let Some(value) = arguments.first() else {
                return Err(Self::error_at(
                    error_cursor,
                    SnbtErrorKind::ExpectedNumberOrBoolean,
                ));
            };
            return bool_tag_value(value)
                .map(|value| NbtTag::Byte(i8::from(value)))
                .ok_or_else(|| {
                    Self::error_at(error_cursor, SnbtErrorKind::ExpectedNumberOrBoolean)
                });
        }

        if name == "uuid" && arguments.len() == 1 {
            let Some(NbtTag::String(uuid)) = arguments.first() else {
                return Err(Self::error_at(
                    error_cursor,
                    SnbtErrorKind::ExpectedStringUuid,
                ));
            };
            // Steel intentionally accepts the `uuid` crate's formats instead of Java's
            // legacy `UUID.fromString` edge cases. Canonical dashed UUIDs are compatible.
            let uuid = Uuid::parse_str(uuid.as_str().to_str().as_ref())
                .map_err(|_| Self::error_at(error_cursor, SnbtErrorKind::ExpectedStringUuid))?;
            return Ok(NbtTag::IntArray(uuid.to_int_array().to_vec()));
        }

        Err(Self::error_at(
            error_cursor,
            SnbtErrorKind::UnknownOperation {
                name: name.to_owned(),
                argument_count: arguments.len(),
            },
        ))
    }

    fn parse_builtin_arguments(&mut self) -> Result<Vec<NbtTag>, SnbtError> {
        let mut arguments = Vec::new();
        if self.consume_char(')') {
            return Ok(arguments);
        }

        loop {
            arguments.push(self.parse_tag()?);
            if self.consume_repeated_separator(',') {
                if self.consume_char(')') {
                    return Ok(arguments);
                }
                continue;
            }

            self.expect_char(')')?;
            return Ok(arguments);
        }
    }

    fn parse_number(
        &mut self,
        default_kind: DefaultIntegerKind,
        allow_float: bool,
    ) -> Result<NbtTag, SnbtError> {
        let start = self.cursor;
        let token_len = scan_number_token(&self.input[start..], allow_float)
            .map_err(|error| Self::error_at(start + error.cursor, error.kind))?;
        self.cursor += token_len;

        let token = &self.input[start..self.cursor];
        let result = parse_number_token(token, default_kind);
        let records_float_candidate =
            allow_float && result.is_ok() && is_unsuffixed_decimal_integer_token(token);
        if records_float_candidate {
            self.record_error(self.cursor, SnbtErrorKind::ExpectedSymbol('.'));
        }

        result.map_err(|kind| self.error(kind))
    }

    fn parse_quoted_string(&mut self) -> Result<String, SnbtError> {
        let Some(terminator @ ('"' | '\'')) = self.read() else {
            return Err(self.error(SnbtErrorKind::ExpectedQuotedString));
        };

        let mut value = String::new();
        while let Some(ch) = self.read() {
            match ch {
                ch if ch == terminator => return Ok(value),
                '\\' => value.push(self.parse_escape()?),
                _ => value.push(ch),
            }
        }

        Err(self.error(SnbtErrorKind::UnclosedQuotedString))
    }

    fn parse_escape(&mut self) -> Result<char, SnbtError> {
        let escape_cursor = self.cursor;
        let Some(ch) = self.read() else {
            return Err(Self::error_at(
                escape_cursor,
                SnbtErrorKind::UnclosedEscapeSequence,
            ));
        };

        match ch {
            'b' => Ok('\u{0008}'),
            's' => Ok(' '),
            't' => Ok('\t'),
            'n' => Ok('\n'),
            'f' => Ok('\u{000C}'),
            'r' => Ok('\r'),
            '\\' | '\'' | '"' => Ok(ch),
            'x' => self.parse_code_point_escape(2, self.cursor),
            'u' => self.parse_code_point_escape(4, self.cursor),
            'U' => self.parse_code_point_escape(8, self.cursor),
            'N' => self.parse_named_escape(),
            _ => Err(Self::error_at(
                escape_cursor,
                SnbtErrorKind::InvalidEscape(ch),
            )),
        }
    }

    fn parse_code_point_escape(
        &mut self,
        digits: usize,
        digit_cursor: usize,
    ) -> Result<char, SnbtError> {
        let mut value = 0_u32;
        for _ in 0..digits {
            let Some(ch) = self.read() else {
                return Err(Self::error_at(
                    digit_cursor,
                    SnbtErrorKind::ExpectedHexEscape { digits },
                ));
            };
            let Some(digit) = ch.to_digit(16) else {
                return Err(Self::error_at(
                    digit_cursor,
                    SnbtErrorKind::ExpectedHexEscape { digits },
                ));
            };
            value = value * 16 + digit;
        }

        char::from_u32(value).ok_or_else(|| self.error(SnbtErrorKind::InvalidCodepoint(value)))
    }

    fn parse_named_escape(&mut self) -> Result<char, SnbtError> {
        let brace_cursor = self.cursor;
        if self.read() != Some('{') {
            return Err(Self::error_at(
                brace_cursor,
                SnbtErrorKind::ExpectedCharacterName,
            ));
        }

        let name_start = self.cursor;
        while self.peek().is_some_and(is_allowed_in_unicode_name) {
            self.read();
        }
        if self.cursor == name_start {
            return Err(Self::error_at(
                name_start,
                SnbtErrorKind::InvalidCharacterName(String::new()),
            ));
        }
        if self.peek() != Some('}') {
            return Err(self.error(SnbtErrorKind::UnclosedCharacterName));
        }

        let name = self.input[name_start..self.cursor].to_owned();
        self.read();
        unicode_names2::character(&name)
            .ok_or_else(|| Self::error_at(self.cursor, SnbtErrorKind::InvalidCharacterName(name)))
    }

    fn parse_unquoted_string(&mut self) -> Result<String, SnbtError> {
        let start = self.cursor;
        while self.peek().is_some_and(is_allowed_in_unquoted_string) {
            self.read();
        }

        if self.cursor == start {
            return Err(Self::error_at(start, SnbtErrorKind::ExpectedUnquotedString));
        }

        Ok(self.input[start..self.cursor].to_owned())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TypedArrayKind {
    Byte,
    Int,
    Long,
}
