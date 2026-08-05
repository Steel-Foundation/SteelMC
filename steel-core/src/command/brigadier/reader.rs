//! Cursor-based command input reader.

use std::str::FromStr;

use steel_utils::java;

use super::{CommandSyntaxError, CommandSyntaxErrorKind};

const SYNTAX_ESCAPE: char = '\\';
const SYNTAX_DOUBLE_QUOTE: char = '"';
const SYNTAX_SINGLE_QUOTE: char = '\'';

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ReaderCursor {
    byte: usize,
    utf16: usize,
}

/// Reads command input while exposing Brigadier-compatible UTF-16 positions.
#[derive(Clone, Debug)]
pub(crate) struct StringReader<'input> {
    input: &'input str,
    total_length: usize,
    cursor: ReaderCursor,
}

impl<'input> StringReader<'input> {
    /// Creates a reader at the beginning of `input`.
    pub(crate) fn new(input: &'input str) -> Self {
        Self {
            input,
            total_length: input.encode_utf16().count(),
            cursor: ReaderCursor::default(),
        }
    }

    /// Returns the complete command input.
    pub(crate) const fn input(&self) -> &'input str {
        self.input
    }

    /// Returns the input length in UTF-16 code units.
    pub(crate) const fn total_length(&self) -> usize {
        self.total_length
    }

    /// Returns the current position in UTF-16 code units.
    pub(crate) const fn cursor(&self) -> usize {
        self.cursor.utf16
    }

    /// Returns the current position in UTF-8 bytes.
    pub(crate) const fn byte_cursor(&self) -> usize {
        self.cursor.byte
    }

    /// Returns the remaining length in UTF-16 code units.
    pub(crate) const fn remaining_length(&self) -> usize {
        self.total_length - self.cursor.utf16
    }

    /// Returns whether at least one character remains.
    pub(crate) const fn can_read(&self) -> bool {
        self.cursor.byte < self.input.len()
    }

    /// Returns whether `length` UTF-16 code units remain.
    pub(crate) fn can_read_length(&self, length: usize) -> bool {
        self.cursor
            .utf16
            .checked_add(length)
            .is_some_and(|end| end <= self.total_length)
    }

    /// Returns the next Unicode scalar without advancing the reader.
    pub(crate) fn peek(&self) -> Option<char> {
        self.remaining().chars().next()
    }

    /// Reads the next Unicode scalar.
    pub(crate) fn read(&mut self) -> Option<char> {
        let character = self.peek()?;
        self.cursor.byte += character.len_utf8();
        self.cursor.utf16 += character.len_utf16();
        Some(character)
    }

    /// Advances past the next Unicode scalar if one remains.
    pub(crate) fn skip(&mut self) -> bool {
        self.read().is_some()
    }

    /// Returns the input before the cursor.
    pub(crate) fn read_so_far(&self) -> &'input str {
        &self.input[..self.cursor.byte]
    }

    /// Returns the input at and after the cursor.
    pub(crate) fn remaining(&self) -> &'input str {
        &self.input[self.cursor.byte..]
    }

    /// Reads all remaining input.
    pub(crate) fn read_remaining(&mut self) -> &'input str {
        let remaining = &self.input[self.cursor.byte..];
        self.cursor.byte = self.input.len();
        self.cursor.utf16 = self.total_length;
        remaining
    }

    /// Advances by an exact UTF-8 byte count while retaining Brigadier's UTF-16 cursor.
    pub(crate) fn advance_bytes(&mut self, bytes: usize) -> bool {
        let Some(consumed) = self.remaining().get(..bytes) else {
            return false;
        };
        self.cursor.byte += bytes;
        self.cursor.utf16 += consumed.encode_utf16().count();
        true
    }

    /// Advances past whitespace recognized by Java's `Character.isWhitespace`.
    pub(crate) fn skip_whitespace(&mut self) {
        while self.peek().is_some_and(java::is_whitespace) {
            self.skip();
        }
    }

    /// Reads an unquoted Brigadier string.
    pub(crate) fn read_unquoted_string(&mut self) -> &'input str {
        let start = self.checkpoint();
        while self.peek().is_some_and(Self::is_allowed_in_unquoted_string) {
            self.skip();
        }
        &self.input[start.byte..self.cursor.byte]
    }

    /// Reads one custom argument token up to Java-compatible whitespace.
    pub(crate) fn read_unquoted_token(&mut self) -> &'input str {
        let start = self.checkpoint();
        while self
            .peek()
            .is_some_and(|character| !java::is_whitespace(character))
        {
            self.skip();
        }
        &self.input[start.byte..self.cursor.byte]
    }

    /// Reads a single- or double-quoted Brigadier string.
    pub(crate) fn read_quoted_string(&mut self) -> Result<String, CommandSyntaxError> {
        let Some(terminator) = self.peek() else {
            return Ok(String::new());
        };
        if !Self::is_quoted_string_start(terminator) {
            return Err(self.error(CommandSyntaxErrorKind::ExpectedStartOfQuote));
        }

        self.skip();
        self.read_string_until(terminator)
    }

    /// Reads a quoted or unquoted Brigadier string.
    pub(crate) fn read_string(&mut self) -> Result<String, CommandSyntaxError> {
        let Some(next) = self.peek() else {
            return Ok(String::new());
        };
        if Self::is_quoted_string_start(next) {
            self.skip();
            self.read_string_until(next)
        } else {
            Ok(self.read_unquoted_string().to_owned())
        }
    }

    /// Reads a signed 32-bit integer.
    pub(crate) fn read_int(&mut self) -> Result<i32, CommandSyntaxError> {
        self.read_number(
            CommandSyntaxErrorKind::ExpectedInt,
            CommandSyntaxErrorKind::InvalidInt,
        )
    }

    /// Reads a signed 64-bit integer.
    pub(crate) fn read_long(&mut self) -> Result<i64, CommandSyntaxError> {
        self.read_number(
            CommandSyntaxErrorKind::ExpectedLong,
            CommandSyntaxErrorKind::InvalidLong,
        )
    }

    /// Reads a 64-bit floating-point number.
    pub(crate) fn read_double(&mut self) -> Result<f64, CommandSyntaxError> {
        self.read_number(
            CommandSyntaxErrorKind::ExpectedDouble,
            CommandSyntaxErrorKind::InvalidDouble,
        )
    }

    /// Reads a 32-bit floating-point number.
    pub(crate) fn read_float(&mut self) -> Result<f32, CommandSyntaxError> {
        self.read_number(
            CommandSyntaxErrorKind::ExpectedFloat,
            CommandSyntaxErrorKind::InvalidFloat,
        )
    }

    /// Reads a lowercase Brigadier boolean.
    pub(crate) fn read_boolean(&mut self) -> Result<bool, CommandSyntaxError> {
        let start = self.checkpoint();
        let value = self.read_string()?;
        match value.as_str() {
            "true" => Ok(true),
            "false" => Ok(false),
            "" => Err(self.error(CommandSyntaxErrorKind::ExpectedBool)),
            _ => {
                self.restore(start);
                Err(self.error(CommandSyntaxErrorKind::InvalidBool(value.into())))
            }
        }
    }

    /// Consumes `expected` or returns a contextual syntax error.
    pub(crate) fn expect(&mut self, expected: char) -> Result<(), CommandSyntaxError> {
        if self.peek() != Some(expected) {
            return Err(self.error(CommandSyntaxErrorKind::ExpectedSymbol(expected)));
        }
        self.skip();
        Ok(())
    }

    pub(super) fn try_read_literal(&mut self, literal: &str) -> bool {
        let Some(remaining) = self.remaining().strip_prefix(literal) else {
            return false;
        };
        if remaining
            .chars()
            .next()
            .is_some_and(|character| character != ' ')
        {
            return false;
        }

        self.cursor.byte += literal.len();
        self.cursor.utf16 += literal.encode_utf16().count();
        true
    }

    fn read_string_until(&mut self, terminator: char) -> Result<String, CommandSyntaxError> {
        let mut result = String::new();
        let mut escaped = false;

        while self.can_read() {
            let character_start = self.checkpoint();
            let Some(character) = self.read() else {
                break;
            };
            if escaped {
                if character == terminator || character == SYNTAX_ESCAPE {
                    result.push(character);
                    escaped = false;
                } else {
                    self.restore(character_start);
                    return Err(self.error(CommandSyntaxErrorKind::InvalidEscape(character)));
                }
            } else if character == SYNTAX_ESCAPE {
                escaped = true;
            } else if character == terminator {
                return Ok(result);
            } else {
                result.push(character);
            }
        }

        Err(self.error(CommandSyntaxErrorKind::ExpectedEndOfQuote))
    }

    fn read_number<T>(
        &mut self,
        expected: CommandSyntaxErrorKind,
        invalid: fn(Box<str>) -> CommandSyntaxErrorKind,
    ) -> Result<T, CommandSyntaxError>
    where
        T: FromStr,
    {
        let start = self.checkpoint();
        while self.peek().is_some_and(Self::is_allowed_number) {
            self.skip();
        }

        let number = &self.input[start.byte..self.cursor.byte];
        if number.is_empty() {
            return Err(self.error(expected));
        }
        if let Ok(value) = number.parse() {
            Ok(value)
        } else {
            let invalid_number = Box::<str>::from(number);
            self.restore(start);
            Err(self.error(invalid(invalid_number)))
        }
    }

    /// Captures the current reader position for later restoration.
    pub(crate) const fn checkpoint(&self) -> ReaderCursor {
        self.cursor
    }

    /// Restores a position previously returned by [`Self::checkpoint`].
    pub(crate) const fn restore(&mut self, checkpoint: ReaderCursor) {
        self.cursor = checkpoint;
    }

    /// Creates a syntax error at the current reader position.
    pub(crate) fn error(&self, kind: CommandSyntaxErrorKind) -> CommandSyntaxError {
        CommandSyntaxError::new(kind, self.input, self.cursor.utf16, self.cursor.byte)
    }

    const fn is_allowed_number(character: char) -> bool {
        character.is_ascii_digit() || matches!(character, '.' | '-')
    }

    const fn is_quoted_string_start(character: char) -> bool {
        matches!(character, SYNTAX_DOUBLE_QUOTE | SYNTAX_SINGLE_QUOTE)
    }

    const fn is_allowed_in_unquoted_string(character: char) -> bool {
        character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.' | '+')
    }
}
