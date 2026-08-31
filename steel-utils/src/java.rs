//! Java standard-library behavior used by vanilla parsing.

/// Returns whether Java's `Character.isWhitespace` recognizes `character`.
#[must_use]
pub const fn is_whitespace(character: char) -> bool {
    matches!(
        character,
        '\u{0009}'..='\u{000d}'
            | '\u{001c}'..='\u{0020}'
            | '\u{1680}'
            | '\u{2000}'..='\u{2006}'
            | '\u{2008}'..='\u{200a}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{205f}'
            | '\u{3000}'
    )
}

/// Returns whether Java's `Character.isSpaceChar` recognizes `character`.
#[must_use]
pub const fn is_space_char(character: char) -> bool {
    matches!(
        character,
        '\u{0020}' | '\u{00a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}'
    )
}

/// Returns Java's `String.length()` for `value`: the number of UTF-16 code units.
///
/// Rust's `str::len` counts UTF-8 bytes, so vanilla limits expressed in Java string
/// characters (for example the 256-character message-argument limit) must use this
/// instead to avoid rejecting non-ASCII input too early.
#[must_use]
pub fn string_length(value: &str) -> usize {
    value.encode_utf16().count()
}

/// Mirrors vanilla `StringUtil.isBlank`.
#[must_use]
pub fn is_blank(value: &str) -> bool {
    value
        .chars()
        .all(|character| is_whitespace(character) || is_space_char(character))
}

#[cfg(test)]
mod tests {
    use super::{is_blank, is_space_char, is_whitespace, string_length};

    #[test]
    fn matches_java_whitespace_exclusions() {
        assert!(is_whitespace(' '));
        assert!(is_whitespace('\u{1680}'));
        for non_breaking_space in ['\u{0085}', '\u{00a0}', '\u{2007}', '\u{202f}'] {
            assert!(!is_whitespace(non_breaking_space));
        }
    }

    #[test]
    fn space_char_includes_unicode_space_separators() {
        for space in [' ', '\u{00a0}', '\u{2007}', '\u{202f}'] {
            assert!(is_space_char(space));
        }
        assert!(!is_space_char('\u{0085}'));
    }

    #[test]
    fn blank_combines_java_whitespace_and_space_char() {
        assert!(is_blank(""));
        assert!(is_blank("\u{001c}\u{00a0}\u{202f}"));
        assert!(!is_blank("\u{0085}"));
        assert!(!is_blank(" text "));
    }

    #[test]
    fn string_length_counts_utf16_code_units() {
        assert_eq!(string_length("hello"), 5);
        // Two-byte UTF-8, one UTF-16 code unit.
        assert_eq!("é".len(), 2);
        assert_eq!(string_length("é"), 1);
        // Three-byte UTF-8, one UTF-16 code unit.
        assert_eq!(string_length("中"), 1);
        // Outside the BMP: one `char`, but a surrogate pair in Java.
        assert_eq!("\u{1f600}".chars().count(), 1);
        assert_eq!(string_length("\u{1f600}"), 2);
    }
}
