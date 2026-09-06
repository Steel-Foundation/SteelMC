use std::{error::Error, fmt};

use text_components::TextComponent;

use crate::translations;

/// Error returned when parsing SNBT text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnbtError {
    cursor: usize,
    kind: SnbtErrorKind,
}

impl SnbtError {
    pub(super) const fn new(cursor: usize, kind: SnbtErrorKind) -> Self {
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
                TextComponent::from(&translations::SNBT_PARSER_UNDERSCORE_NOT_ALLOWED)
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
