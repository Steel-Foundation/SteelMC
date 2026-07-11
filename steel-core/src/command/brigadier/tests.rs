use super::{CommandSyntaxError, CommandSyntaxErrorKind, StringRange, StringReader};
use steel_utils::translations;
use text_components::{content::Content, format::Color, interactivity::ClickEvent};

fn assert_error(
    result: Result<impl Sized, CommandSyntaxError>,
    expected_kind: CommandSyntaxErrorKind,
    expected_cursor: usize,
) -> CommandSyntaxError {
    let Err(error) = result else {
        panic!("expected command syntax error");
    };

    assert_eq!(error.kind(), &expected_kind);
    assert_eq!(error.cursor(), Some(expected_cursor));
    error
}

#[test]
fn reader_tracks_input_and_utf16_positions() {
    let mut reader = StringReader::new("a\u{1f600}z");

    assert_eq!(reader.input(), "a\u{1f600}z");
    assert_eq!(reader.total_length(), 4);
    assert_eq!(reader.remaining_length(), 4);
    assert!(reader.can_read());
    assert!(reader.can_read_length(4));
    assert!(!reader.can_read_length(5));
    assert_eq!(reader.peek(), Some('a'));

    assert_eq!(reader.read(), Some('a'));
    assert_eq!(reader.cursor(), 1);
    assert_eq!(reader.read_so_far(), "a");
    assert_eq!(reader.remaining(), "\u{1f600}z");

    assert_eq!(reader.read(), Some('\u{1f600}'));
    assert_eq!(reader.cursor(), 3);
    assert_eq!(reader.remaining_length(), 1);
    assert_eq!(reader.peek(), Some('z'));

    assert!(reader.skip());
    assert_eq!(reader.cursor(), 4);
    assert!(!reader.can_read());
    assert!(!reader.skip());
    assert_eq!(reader.read(), None);
}

#[test]
fn reader_advances_utf8_bytes_and_tracks_utf16_positions() {
    let mut reader = StringReader::new("a\u{1f600}z");

    assert!(!reader.advance_bytes(2));
    assert_eq!(reader.cursor(), 0);
    assert!(reader.advance_bytes(5));
    assert_eq!(reader.cursor(), 3);
    assert_eq!(reader.remaining(), "z");
    assert!(!reader.advance_bytes(2));
    assert_eq!(reader.cursor(), 3);
}

#[test]
fn reader_skips_java_whitespace_only() {
    let mut reader = StringReader::new(" \t\n\u{001c}\u{1680}\u{2000}\u{2028}\u{3000}text");
    reader.skip_whitespace();
    assert_eq!(reader.remaining(), "text");

    for non_breaking_space in ['\u{0085}', '\u{00a0}', '\u{2007}', '\u{202f}'] {
        let input = format!("{non_breaking_space}text");
        let mut reader = StringReader::new(&input);
        reader.skip_whitespace();
        assert_eq!(reader.cursor(), 0);
    }
}

#[test]
fn reader_reads_unquoted_strings_with_brigadier_character_set() {
    let mut reader = StringReader::new("abc_123-.+ remaining");
    assert_eq!(reader.read_unquoted_string(), "abc_123-.+");
    assert_eq!(reader.remaining(), " remaining");

    let mut empty = StringReader::new(" remaining");
    assert_eq!(empty.read_unquoted_string(), "");
    assert_eq!(empty.cursor(), 0);
}

#[test]
fn reader_reads_single_and_double_quoted_strings() {
    let mut double = StringReader::new("\"hello 'world'\" tail");
    assert_eq!(double.read_quoted_string(), Ok("hello 'world'".to_owned()));
    assert_eq!(double.read_so_far(), "\"hello 'world'\"");
    assert_eq!(double.remaining(), " tail");

    let mut single = StringReader::new("'hello \"world\"'");
    assert_eq!(
        single.read_quoted_string(),
        Ok("hello \"world\"".to_owned())
    );

    let mut empty_input = StringReader::new("");
    assert_eq!(empty_input.read_quoted_string(), Ok(String::new()));

    let mut empty_quoted = StringReader::new("''tail");
    assert_eq!(empty_quoted.read_quoted_string(), Ok(String::new()));
    assert_eq!(empty_quoted.remaining(), "tail");
}

#[test]
fn reader_unescapes_only_the_active_quote_and_backslash() {
    let mut reader = StringReader::new("\"hello \\\"world\\\" \\\\ done\"");
    assert_eq!(
        reader.read_quoted_string(),
        Ok("hello \"world\" \\ done".to_owned())
    );

    let mut wrong_quote = StringReader::new("'hello\\\"world'");
    let error = assert_error(
        wrong_quote.read_quoted_string(),
        CommandSyntaxErrorKind::InvalidEscape('"'),
        7,
    );
    assert_eq!(error.input(), Some("'hello\\\"world'"));
}

#[test]
fn quoted_string_errors_match_brigadier_cursors() {
    let mut missing_start = StringReader::new("hello world\"");
    assert_error(
        missing_start.read_quoted_string(),
        CommandSyntaxErrorKind::ExpectedStartOfQuote,
        0,
    );

    let mut missing_end = StringReader::new("\"hello world");
    assert_error(
        missing_end.read_quoted_string(),
        CommandSyntaxErrorKind::ExpectedEndOfQuote,
        12,
    );

    let mut invalid_escape = StringReader::new("\"hello\\nworld\"");
    assert_error(
        invalid_escape.read_quoted_string(),
        CommandSyntaxErrorKind::InvalidEscape('n'),
        7,
    );
}

#[test]
fn quoted_string_error_cursor_uses_utf16_units() {
    let mut missing_end = StringReader::new("\"\u{1f600}");
    assert_error(
        missing_end.read_quoted_string(),
        CommandSyntaxErrorKind::ExpectedEndOfQuote,
        3,
    );

    let mut invalid_escape = StringReader::new("\"\u{1f600}\\x\"");
    assert_error(
        invalid_escape.read_quoted_string(),
        CommandSyntaxErrorKind::InvalidEscape('x'),
        4,
    );
}

#[test]
fn reader_selects_quoted_or_unquoted_string() {
    let mut unquoted = StringReader::new("hello world");
    assert_eq!(unquoted.read_string(), Ok("hello".to_owned()));

    let mut quoted = StringReader::new("\"hello world\"");
    assert_eq!(quoted.read_string(), Ok("hello world".to_owned()));

    let mut empty = StringReader::new("");
    assert_eq!(empty.read_string(), Ok(String::new()));
}

#[test]
fn reader_reads_integers_and_rolls_back_invalid_values() {
    let mut reader = StringReader::new("-12345 tail");
    assert_eq!(reader.read_int(), Ok(-12_345));
    assert_eq!(reader.remaining(), " tail");

    let mut invalid = StringReader::new("12.34 tail");
    assert_error(
        invalid.read_int(),
        CommandSyntaxErrorKind::InvalidInt("12.34".into()),
        0,
    );
    assert_eq!(invalid.cursor(), 0);
    assert_eq!(invalid.remaining(), "12.34 tail");

    let mut missing = StringReader::new("tail");
    assert_error(missing.read_int(), CommandSyntaxErrorKind::ExpectedInt, 0);
}

#[test]
fn reader_reads_longs_and_rolls_back_invalid_values() {
    let mut reader = StringReader::new("-9223372036854775808 tail");
    assert_eq!(reader.read_long(), Ok(i64::MIN));

    let mut invalid = StringReader::new("9223372036854775808");
    assert_error(
        invalid.read_long(),
        CommandSyntaxErrorKind::InvalidLong("9223372036854775808".into()),
        0,
    );

    let mut missing = StringReader::new("tail");
    assert_error(missing.read_long(), CommandSyntaxErrorKind::ExpectedLong, 0);
}

#[test]
fn reader_reads_floating_point_numbers_and_rolls_back_invalid_values() {
    let mut double = StringReader::new("-12.5 tail");
    assert_eq!(double.read_double(), Ok(-12.5));

    let mut float = StringReader::new(".5 tail");
    assert_eq!(float.read_float(), Ok(0.5));

    let mut invalid_double = StringReader::new("12.34.56");
    assert_error(
        invalid_double.read_double(),
        CommandSyntaxErrorKind::InvalidDouble("12.34.56".into()),
        0,
    );

    let mut invalid_float = StringReader::new("-");
    assert_error(
        invalid_float.read_float(),
        CommandSyntaxErrorKind::InvalidFloat("-".into()),
        0,
    );

    let mut missing_double = StringReader::new("tail");
    assert_error(
        missing_double.read_double(),
        CommandSyntaxErrorKind::ExpectedDouble,
        0,
    );

    let mut missing_float = StringReader::new("tail");
    assert_error(
        missing_float.read_float(),
        CommandSyntaxErrorKind::ExpectedFloat,
        0,
    );
}

#[test]
fn reader_reads_booleans_and_rolls_back_invalid_values() {
    let mut truth = StringReader::new("true tail");
    assert_eq!(truth.read_boolean(), Ok(true));

    let mut falsehood = StringReader::new("false tail");
    assert_eq!(falsehood.read_boolean(), Ok(false));

    let mut invalid = StringReader::new("tuesday tail");
    assert_error(
        invalid.read_boolean(),
        CommandSyntaxErrorKind::InvalidBool("tuesday".into()),
        0,
    );
    assert_eq!(invalid.cursor(), 0);

    let mut missing = StringReader::new(" tail");
    assert_error(
        missing.read_boolean(),
        CommandSyntaxErrorKind::ExpectedBool,
        0,
    );
}

#[test]
fn reader_expects_symbols_without_consuming_mismatches() {
    let mut present = StringReader::new("abc");
    assert_eq!(present.expect('a'), Ok(()));
    assert_eq!(present.cursor(), 1);

    let mut mismatch = StringReader::new("abc");
    assert_error(
        mismatch.expect('b'),
        CommandSyntaxErrorKind::ExpectedSymbol('b'),
        0,
    );
    assert_eq!(mismatch.cursor(), 0);

    let mut empty = StringReader::new("");
    assert_error(
        empty.expect('a'),
        CommandSyntaxErrorKind::ExpectedSymbol('a'),
        0,
    );
}

#[test]
fn command_syntax_error_formats_brigadier_context() {
    let mut reader = StringReader::new("0123456789abc");
    while reader.can_read() {
        assert!(reader.skip());
    }

    let error = assert_error(
        reader.expect('!'),
        CommandSyntaxErrorKind::ExpectedSymbol('!'),
        13,
    );
    assert_eq!(error.raw_message(), "Expected '!'");
    assert_eq!(error.context(), Some("...3456789abc<--[HERE]".to_owned()));
    assert_eq!(
        error.to_string(),
        "Expected '!' at position 13: ...3456789abc<--[HERE]"
    );
}

#[test]
fn command_syntax_error_builds_translated_styled_feedback() {
    let mut reader = StringReader::new("0123456789abcdef");
    for _ in 0..13 {
        assert!(reader.skip());
    }
    let error = assert_error(
        reader.expect('!'),
        CommandSyntaxErrorKind::ExpectedSymbol('!'),
        13,
    );

    assert!(matches!(
        error.message_component().content,
        Content::Translate(ref message) if message.key.as_ref() == translations::PARSING_EXPECTED.0
    ));

    let Some(context) = error.context_component() else {
        panic!("parser errors should include a context component");
    };
    assert_eq!(context.format.color, Some(Color::Gray));
    assert!(matches!(
        context.interactions.click,
        Some(ClickEvent::SuggestCommand { ref command })
            if command.as_ref() == "/0123456789abcdef"
    ));
    assert_eq!(context.children.len(), 4);
    assert!(matches!(
        &context.children[0].content,
        Content::Text { text } if text.as_ref() == "..."
    ));
    assert!(matches!(
        &context.children[1].content,
        Content::Text { text } if text.as_ref() == "3456789abc"
    ));
    assert!(matches!(
        &context.children[2].content,
        Content::Text { text } if text.as_ref() == "def"
    ));
    assert_eq!(context.children[2].format.color, Some(Color::Red));
    assert_eq!(context.children[2].format.underlined, Some(true));
    assert!(matches!(
        &context.children[3].content,
        Content::Translate(message)
            if message.key.as_ref() == translations::COMMAND_CONTEXT_HERE.0
    ));
    assert_eq!(context.children[3].format.color, Some(Color::Red));
    assert_eq!(context.children[3].format.italic, Some(true));
}

#[test]
fn command_syntax_error_context_does_not_split_unicode_scalars() {
    let mut reader = StringReader::new("123456789\u{1f600}x");
    while reader.can_read() {
        assert!(reader.skip());
    }

    let error = assert_error(
        reader.expect('!'),
        CommandSyntaxErrorKind::ExpectedSymbol('!'),
        12,
    );
    assert_eq!(
        error.context(),
        Some("...3456789\u{1f600}x<--[HERE]".to_owned())
    );
}

#[test]
fn dynamic_command_errors_have_no_parser_context() {
    let error = CommandSyntaxError::dynamic("runtime failure");

    assert_eq!(error.input(), None);
    assert_eq!(error.cursor(), None);
    assert_eq!(error.context(), None);
    assert_eq!(error.to_string(), "runtime failure");
}

#[test]
fn string_ranges_use_brigadier_utf16_positions() {
    let empty = StringRange::at(3);
    assert_eq!(empty.start(), 3);
    assert_eq!(empty.end(), 3);
    assert_eq!(empty.len(), 0);
    assert!(empty.is_empty());

    let first = StringRange::between(1, 3);
    let second = StringRange::between(3, 6);
    assert_eq!(
        StringRange::encompassing(first, second),
        StringRange::between(1, 6)
    );
    assert_eq!(first.len(), 2);
    assert!(!first.is_empty());
}
