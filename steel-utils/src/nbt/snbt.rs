use std::{
    error::Error,
    fmt,
    num::{IntErrorKind, ParseIntError},
};

use simdnbt::{
    Mutf8String,
    owned::{NbtCompound, NbtList, NbtTag},
};
use text_components::TextComponent;
use uuid::Uuid;

use crate::{UuidExt, java, translations};

/// Error returned when parsing SNBT text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnbtError {
    cursor: usize,
    kind: SnbtErrorKind,
}

impl SnbtError {
    const fn new(cursor: usize, kind: SnbtErrorKind) -> Self {
        Self { cursor, kind }
    }

    /// Returns the byte cursor where parsing failed.
    #[must_use]
    pub const fn cursor(&self) -> usize {
        self.cursor
    }

    /// Returns the specific parse failure.
    #[must_use]
    pub const fn kind(&self) -> &SnbtErrorKind {
        &self.kind
    }

    /// Returns the specific parse failure, consuming this error.
    #[must_use]
    pub fn into_kind(self) -> SnbtErrorKind {
        self.kind
    }

    /// Returns the parse failure as a translatable text component.
    #[must_use]
    pub fn component(&self) -> TextComponent {
        self.kind.component()
    }
}

impl fmt::Display for SnbtError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SNBT parse error at byte {}: {}", self.cursor, self.kind)
    }
}

impl Error for SnbtError {}

/// Specific reason why SNBT parsing failed.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SnbtErrorKind {
    /// Non-whitespace input remained after a complete tag.
    TrailingData,
    /// A grammar symbol was required at the cursor.
    ExpectedSymbol(char),
    /// An SNBT value was required at the cursor.
    ExpectedValue,
    /// A compound key was required at the cursor.
    ExpectedKey,
    /// A compound key was present but empty.
    EmptyKey,
    /// A typed-array element was not an integer.
    ExpectedArrayElement,
    /// A typed-array element used an unsupported integer width.
    InvalidArrayElementType,
    /// The `bool` operation received neither a number nor a boolean.
    ExpectedNumberOrBoolean,
    /// The `uuid` operation did not receive a valid UUID string.
    ExpectedStringUuid,
    /// No built-in operation matched the name and argument count.
    UnknownOperation {
        /// Operation name supplied by the input.
        name: String,
        /// Number of supplied arguments.
        argument_count: usize,
    },
    /// A number was required at the cursor.
    ExpectedNumber,
    /// A binary numeral was required at the cursor.
    ExpectedBinaryNumeral,
    /// A decimal numeral was required at the cursor.
    ExpectedDecimalNumeral,
    /// A hexadecimal numeral was required at the cursor.
    ExpectedHexNumeral,
    /// A quoted string was required at the cursor.
    ExpectedQuotedString,
    /// A quoted string was not terminated.
    UnclosedQuotedString,
    /// An escape introducer was not followed by an escape value.
    UnclosedEscapeSequence,
    /// A quoted string contained an unsupported escape.
    InvalidEscape(char),
    /// A Unicode escape did not contain the required hexadecimal digits.
    ExpectedHexEscape {
        /// Required number of hexadecimal digits.
        digits: usize,
    },
    /// A Unicode escape resolved to an invalid code point.
    InvalidCodepoint(u32),
    /// A named Unicode escape did not begin with a character name.
    ExpectedCharacterName,
    /// A named Unicode escape was not terminated.
    UnclosedCharacterName,
    /// A named Unicode escape did not identify a character.
    InvalidCharacterName(String),
    /// An unquoted string was required at the cursor.
    ExpectedUnquotedString,
    /// A floating-point token could not be parsed.
    InvalidFloatingPoint,
    /// A non-finite floating-point value was supplied.
    NonFiniteNumber,
    /// A number placed underscores outside its digits.
    InvalidUnderscore,
    /// An integer token could not be parsed.
    InvalidInteger,
    /// A decimal integer contained a leading zero.
    LeadingZero,
    /// An unsigned integer was negative.
    ExpectedNonNegativeNumber,
    /// An integer exceeded the parser's intermediate representation.
    IntegerTooLarge,
    /// A number did not fit its requested NBT integer type.
    NumberOutOfRange {
        /// Requested NBT integer type.
        number_type: SnbtNumberType,
        /// Whether the literal requested the unsigned range.
        unsigned: bool,
    },
    /// A number token did not contain any digits.
    InvalidNumber,
}

impl SnbtErrorKind {
    /// Returns this failure as a translatable text component.
    #[must_use]
    pub fn component(&self) -> TextComponent {
        match self {
            Self::TrailingData => TextComponent::from(&translations::ARGUMENT_NBT_TRAILING),
            Self::ExpectedSymbol(symbol) => translations::ARGUMENT_LITERAL_INCORRECT
                .message([symbol.to_string()])
                .component(),
            Self::ExpectedValue | Self::ExpectedUnquotedString => {
                TextComponent::from(&translations::SNBT_PARSER_EXPECTED_UNQUOTED_STRING)
            }
            Self::ExpectedKey | Self::ExpectedQuotedString => {
                translations::ARGUMENT_LITERAL_INCORRECT
                    .message(["\""])
                    .component()
            }
            Self::EmptyKey => TextComponent::from(&translations::SNBT_PARSER_EMPTY_KEY),
            Self::ExpectedNumber => translations::ARGUMENT_LITERAL_INCORRECT
                .message(["+"])
                .component(),
            Self::ExpectedBinaryNumeral => {
                TextComponent::from(&translations::SNBT_PARSER_EXPECTED_BINARY_NUMERAL)
            }
            Self::ExpectedDecimalNumeral => {
                TextComponent::from(&translations::SNBT_PARSER_EXPECTED_DECIMAL_NUMERAL)
            }
            Self::ExpectedHexNumeral => {
                TextComponent::from(&translations::SNBT_PARSER_EXPECTED_HEX_NUMERAL)
            }
            Self::ExpectedArrayElement => {
                TextComponent::from(&translations::SNBT_PARSER_EXPECTED_INTEGER_TYPE)
            }
            Self::InvalidArrayElementType => {
                TextComponent::from(&translations::SNBT_PARSER_INVALID_ARRAY_ELEMENT_TYPE)
            }
            Self::ExpectedNumberOrBoolean => {
                TextComponent::from(&translations::SNBT_PARSER_EXPECTED_NUMBER_OR_BOOLEAN)
            }
            Self::ExpectedStringUuid => {
                TextComponent::from(&translations::SNBT_PARSER_EXPECTED_STRING_UUID)
            }
            Self::UnknownOperation {
                name,
                argument_count,
            } => translations::SNBT_PARSER_NO_SUCH_OPERATION
                .message([format!("{name}/{argument_count}")])
                .component(),
            Self::UnclosedQuotedString => {
                TextComponent::from(&translations::SNBT_PARSER_INVALID_STRING_CONTENTS)
            }
            Self::UnclosedEscapeSequence | Self::InvalidEscape(_) => {
                translations::ARGUMENT_LITERAL_INCORRECT
                    .message(["b"])
                    .component()
            }
            Self::ExpectedCharacterName => translations::ARGUMENT_LITERAL_INCORRECT
                .message(["{"])
                .component(),
            Self::UnclosedCharacterName => translations::ARGUMENT_LITERAL_INCORRECT
                .message(["}"])
                .component(),
            Self::ExpectedHexEscape { digits } => translations::SNBT_PARSER_EXPECTED_HEX_ESCAPE
                .message([digits.to_string()])
                .component(),
            Self::InvalidCodepoint(codepoint) => translations::SNBT_PARSER_INVALID_CODEPOINT
                .message([format!("U+{codepoint:08X}")])
                .component(),
            Self::InvalidCharacterName(_) => {
                TextComponent::from(&translations::SNBT_PARSER_INVALID_CHARACTER_NAME)
            }
            Self::NonFiniteNumber => {
                TextComponent::from(&translations::SNBT_PARSER_INFINITY_NOT_ALLOWED)
            }
            // The shipped assets consistently use Mojang's misspelled `undescore` key.
            Self::InvalidUnderscore => {
                TextComponent::from(&translations::SNBT_PARSER_UNDESCORE_NOT_ALLOWED)
            }
            Self::LeadingZero => {
                TextComponent::from(&translations::SNBT_PARSER_LEADING_ZERO_NOT_ALLOWED)
            }
            Self::ExpectedNonNegativeNumber => {
                TextComponent::from(&translations::SNBT_PARSER_EXPECTED_NON_NEGATIVE_NUMBER)
            }
            Self::InvalidFloatingPoint
            | Self::InvalidInteger
            | Self::IntegerTooLarge
            | Self::NumberOutOfRange { .. }
            | Self::InvalidNumber => translations::SNBT_PARSER_NUMBER_PARSE_FAILURE
                .message([self.to_string()])
                .component(),
        }
    }
}

impl fmt::Display for SnbtErrorKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TrailingData => formatter.write_str("trailing data"),
            Self::ExpectedSymbol(symbol) => write!(formatter, "expected '{symbol}'"),
            Self::ExpectedValue => formatter.write_str("expected tag"),
            Self::ExpectedKey => formatter.write_str("expected compound key"),
            Self::EmptyKey => formatter.write_str("compound key cannot be empty"),
            Self::ExpectedArrayElement => formatter.write_str("expected integer array element"),
            Self::InvalidArrayElementType => {
                formatter.write_str("invalid typed array element width")
            }
            Self::ExpectedNumberOrBoolean => formatter.write_str("bool expects a numeric tag"),
            Self::ExpectedStringUuid => formatter.write_str("uuid expects a valid string tag"),
            Self::UnknownOperation {
                name,
                argument_count,
            } => write!(
                formatter,
                "unknown SNBT operation '{name}/{argument_count}'"
            ),
            Self::ExpectedNumber => formatter.write_str("expected number"),
            Self::ExpectedBinaryNumeral => formatter.write_str("expected binary numeral"),
            Self::ExpectedDecimalNumeral => formatter.write_str("expected decimal numeral"),
            Self::ExpectedHexNumeral => formatter.write_str("expected hexadecimal numeral"),
            Self::ExpectedQuotedString => formatter.write_str("expected quoted string"),
            Self::UnclosedQuotedString => formatter.write_str("unclosed quoted string"),
            Self::UnclosedEscapeSequence => formatter.write_str("unclosed escape sequence"),
            Self::InvalidEscape(character) => {
                write!(formatter, "invalid escape '\\{character}'")
            }
            Self::ExpectedHexEscape { digits } => {
                write!(formatter, "expected {digits} hexadecimal escape digits")
            }
            Self::InvalidCodepoint(codepoint) => {
                write!(formatter, "invalid Unicode code point U+{codepoint:08X}")
            }
            Self::ExpectedCharacterName => formatter.write_str("expected Unicode character name"),
            Self::UnclosedCharacterName => formatter.write_str("unclosed Unicode character name"),
            Self::InvalidCharacterName(name) => {
                write!(formatter, "unknown Unicode name '{name}'")
            }
            Self::ExpectedUnquotedString => formatter.write_str("expected unquoted string"),
            Self::InvalidFloatingPoint => formatter.write_str("invalid floating-point literal"),
            Self::NonFiniteNumber => formatter.write_str("floating-point literal must be finite"),
            Self::InvalidUnderscore => {
                formatter.write_str("invalid underscore placement in number literal")
            }
            Self::InvalidInteger => formatter.write_str("invalid integer literal"),
            Self::LeadingZero => formatter.write_str("integer literal cannot have leading zeroes"),
            Self::ExpectedNonNegativeNumber => {
                formatter.write_str("unsigned integer literal cannot be negative")
            }
            Self::IntegerTooLarge => formatter.write_str("integer literal is too large"),
            Self::NumberOutOfRange {
                number_type,
                unsigned,
            } => {
                if *unsigned {
                    write!(formatter, "unsigned {number_type} literal is out of range")
                } else {
                    write!(formatter, "{number_type} literal is out of range")
                }
            }
            Self::InvalidNumber => formatter.write_str("invalid number literal"),
        }
    }
}

/// NBT integer type requested by an SNBT number suffix or array.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnbtNumberType {
    /// Signed or unsigned byte.
    Byte,
    /// Signed or unsigned short.
    Short,
    /// Signed or unsigned integer.
    Int,
    /// Signed or unsigned long.
    Long,
}

impl fmt::Display for SnbtNumberType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Byte => "byte",
            Self::Short => "short",
            Self::Int => "int",
            Self::Long => "long",
        })
    }
}

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

fn parse_number_token(
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

struct NumberScanError {
    cursor: usize,
    kind: SnbtErrorKind,
}

impl NumberScanError {
    const fn new(cursor: usize, kind: SnbtErrorKind) -> Self {
        Self { cursor, kind }
    }
}

fn scan_number_token(input: &str, allow_float: bool) -> Result<usize, NumberScanError> {
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

fn is_unsuffixed_decimal_integer_token(token: &str) -> bool {
    let digits = token
        .strip_prefix(['+', '-'])
        .map_or(token, |stripped| stripped);
    !digits.is_empty()
        && digits
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'_')
}

const fn is_allowed_in_unquoted_string(ch: char) -> bool {
    matches!(ch, '0'..='9' | 'A'..='Z' | 'a'..='z' | '_' | '-' | '.' | '+')
}

const fn is_allowed_in_unicode_name(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '-' | ' ')
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TypedArrayKind {
    Byte,
    Int,
    Long,
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
    fn hexadecimal_number_runs_are_greedy_before_suffixes() {
        let compound = compound_tag("{first:0xAB,second:0x1B}");

        assert_eq!(compound.int("first"), Some(0xAB));
        assert_eq!(compound.int("second"), Some(0x1B));
    }

    #[test]
    fn zero_with_a_byte_suffix_is_not_a_binary_prefix() {
        assert_eq!(parse_snbt("0b").expect("byte zero parses"), NbtTag::Byte(0));
        assert_eq!(parse_snbt("0B").expect("byte zero parses"), NbtTag::Byte(0));
    }

    #[test]
    fn negative_radix_literals_require_explicit_signed_suffixes() {
        assert_eq!(
            parse_snbt("-0x1sI").expect("explicitly signed hex literal parses"),
            NbtTag::Int(-1)
        );
        assert_eq!(
            parse_snbt("-0b1sB").expect("explicitly signed binary literal parses"),
            NbtTag::Byte(-1)
        );

        for literal in ["-0x1", "-0b1", "-0x1i", "-0b1B", "-0x1uI", "-0b1uB"] {
            assert!(parse_snbt(literal).is_err(), "{literal} should not parse");
        }
    }

    #[test]
    fn number_runs_allow_repeated_interior_underscores() {
        let compound =
            compound_tag("{decimal:1__2,binary:0b1__0,hex:0xA__B,float:1__2.3__4,exponent:1e1__2}");

        assert_eq!(compound.int("decimal"), Some(12));
        assert_eq!(compound.int("binary"), Some(2));
        assert_eq!(compound.int("hex"), Some(0xAB));
        assert_eq!(compound.double("float"), Some(12.34));
        assert_eq!(compound.double("exponent"), Some(1e12));
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
    fn rejects_underscores_at_number_run_boundaries() {
        for literal in [
            "+_1", "1_", "0x_1", "0x1_", "0b_1", "0b1_", "1_.0", "1._0", "1_e2", "1e_2", "1e+_2",
            "1.0_", "1e2_",
        ] {
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
        let compound = compound_tag(
            "{enabled:bool(1),id:uuid('123e4567-e89b-12d3-a456-426614174000'),compact:uuid('123e4567e89b12d3a456426614174000')}",
        );

        assert_eq!(compound.byte("enabled"), Some(1));
        assert_eq!(
            compound.int_array("id"),
            Some(uuid.to_int_array().as_slice())
        );
        assert_eq!(
            compound.int_array("compact"),
            Some(uuid.to_int_array().as_slice())
        );
    }

    #[test]
    fn builtin_operation_lookup_uses_the_actual_argument_count() {
        for (input, name, argument_count) in [
            ("unknown()", "unknown", 0),
            ("unknown(1)", "unknown", 1),
            ("bool()", "bool", 0),
            ("bool(1,2)", "bool", 2),
        ] {
            let error = parse_snbt(input).expect_err("operation arity should not match");

            assert_eq!(error.cursor(), input.len(), "{input}");
            assert_eq!(
                error.kind(),
                &SnbtErrorKind::UnknownOperation {
                    name: name.to_owned(),
                    argument_count,
                },
                "{input}"
            );
            assert_eq!(
                error.component(),
                translations::SNBT_PARSER_NO_SUCH_OPERATION
                    .message([format!("{name}/{argument_count}")])
                    .component(),
                "{input}"
            );
        }

        assert_eq!(
            parse_snbt("bool(1,)").expect("a trailing argument separator is valid"),
            NbtTag::Byte(1)
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
        assert_eq!(error.kind(), &SnbtErrorKind::TrailingData);
        assert_eq!(
            error.component(),
            TextComponent::from(&translations::ARGUMENT_NBT_TRAILING)
        );
    }

    #[test]
    fn errors_preserve_semantic_kinds_and_translation_arguments() {
        let expected_value = parse_snbt("{value:}").expect_err("missing value should fail");
        assert_eq!(expected_value.kind(), &SnbtErrorKind::ExpectedValue);
        assert_eq!(
            expected_value.component(),
            TextComponent::from(&translations::SNBT_PARSER_EXPECTED_UNQUOTED_STRING)
        );

        let expected_key = parse_snbt("{:1}").expect_err("missing key should fail");
        assert_eq!(expected_key.kind(), &SnbtErrorKind::ExpectedKey);
        assert_eq!(
            expected_key.component(),
            translations::ARGUMENT_LITERAL_INCORRECT
                .message(["\""])
                .component()
        );

        let expected_number =
            parse_snbt("[B;,]").expect_err("missing typed-array number should fail");
        assert_eq!(expected_number.kind(), &SnbtErrorKind::ExpectedNumber);
        assert_eq!(
            expected_number.component(),
            translations::ARGUMENT_LITERAL_INCORRECT
                .message(["+"])
                .component()
        );

        let invalid_underscore =
            parse_snbt("0b1_").expect_err("trailing binary underscore should fail");
        assert_eq!(invalid_underscore.kind(), &SnbtErrorKind::InvalidUnderscore);
        assert_eq!(
            invalid_underscore.component(),
            TextComponent::from(&translations::SNBT_PARSER_UNDESCORE_NOT_ALLOWED)
        );

        let invalid_uuid = parse_snbt("uuid('invalid')").expect_err("invalid UUID should fail");
        assert_eq!(invalid_uuid.kind(), &SnbtErrorKind::ExpectedStringUuid);
        assert_eq!(
            invalid_uuid.component(),
            TextComponent::from(&translations::SNBT_PARSER_EXPECTED_STRING_UUID)
        );

        let unknown_operation =
            parse_snbt("unknown(1)").expect_err("unknown operation should fail");
        assert_eq!(
            unknown_operation.kind(),
            &SnbtErrorKind::UnknownOperation {
                name: "unknown".to_owned(),
                argument_count: 1,
            }
        );
        assert_eq!(
            unknown_operation.component(),
            translations::SNBT_PARSER_NO_SUCH_OPERATION
                .message(["unknown/1"])
                .component()
        );
    }

    #[test]
    fn numeric_syntax_errors_preserve_specific_kinds_and_cursors() {
        for (input, cursor, kind, component) in [
            (
                "+",
                1,
                SnbtErrorKind::ExpectedDecimalNumeral,
                TextComponent::from(&translations::SNBT_PARSER_EXPECTED_DECIMAL_NUMERAL),
            ),
            (
                ".",
                1,
                SnbtErrorKind::ExpectedDecimalNumeral,
                TextComponent::from(&translations::SNBT_PARSER_EXPECTED_DECIMAL_NUMERAL),
            ),
            (
                "0x",
                2,
                SnbtErrorKind::ExpectedHexNumeral,
                TextComponent::from(&translations::SNBT_PARSER_EXPECTED_HEX_NUMERAL),
            ),
        ] {
            let error = parse_snbt(input).expect_err("incomplete number should fail");

            assert_eq!(error.cursor(), cursor, "{input}");
            assert_eq!(error.kind(), &kind, "{input}");
            assert_eq!(error.component(), component, "{input}");
        }

        let signed_nonnumeric = parse_snbt("-x").expect_err("signed string should fail");
        assert_eq!(signed_nonnumeric.cursor(), 1);
        assert_eq!(
            signed_nonnumeric.kind(),
            &SnbtErrorKind::ExpectedDecimalNumeral
        );

        let out_of_range = parse_snbt("128b").expect_err("out-of-range byte should fail");
        assert_eq!(out_of_range.cursor(), "128b".len());
        assert_eq!(
            out_of_range.kind(),
            &SnbtErrorKind::NumberOutOfRange {
                number_type: SnbtNumberType::Byte,
                unsigned: false,
            }
        );
    }

    #[test]
    fn argument_parser_stops_after_a_complete_number() {
        let (tag, cursor) = parse_snbt_argument("1z").expect("integer prefix parses");

        assert_eq!(tag, NbtTag::Int(1));
        assert_eq!(cursor, 1);
    }

    #[test]
    fn errors_match_vanilla_alternative_selection_and_cursors() {
        for (input, cursor, kind, expected_literal) in [
            ("{a:1", 4, SnbtErrorKind::ExpectedSymbol('.'), "."),
            ("{a:true", 7, SnbtErrorKind::ExpectedSymbol('('), "("),
            (r#"{a:"x""#, 6, SnbtErrorKind::ExpectedSymbol(','), ","),
            (r#""\q""#, 2, SnbtErrorKind::InvalidEscape('q'), "b"),
            (r#""\N""#, 3, SnbtErrorKind::ExpectedCharacterName, "{"),
            (r#""\N{ABC""#, 7, SnbtErrorKind::UnclosedCharacterName, "}"),
        ] {
            let error = parse_snbt(input).expect_err("input should not parse");

            assert_eq!(error.cursor(), cursor, "{input}");
            assert_eq!(error.kind(), &kind, "{input}");
            assert_eq!(
                error.component(),
                translations::ARGUMENT_LITERAL_INCORRECT
                    .message([expected_literal])
                    .component(),
                "{input}"
            );
        }

        let unclosed_string = parse_snbt(r#""abc"#).expect_err("unclosed string should fail");
        assert_eq!(unclosed_string.cursor(), 4);
        assert_eq!(unclosed_string.kind(), &SnbtErrorKind::UnclosedQuotedString);
        assert_eq!(
            unclosed_string.component(),
            TextComponent::from(&translations::SNBT_PARSER_INVALID_STRING_CONTENTS)
        );
    }
}
