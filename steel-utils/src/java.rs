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

#[cfg(test)]
mod tests {
    use super::is_whitespace;

    #[test]
    fn matches_java_whitespace_exclusions() {
        assert!(is_whitespace(' '));
        assert!(is_whitespace('\u{1680}'));
        for non_breaking_space in ['\u{0085}', '\u{00a0}', '\u{2007}', '\u{202f}'] {
            assert!(!is_whitespace(non_breaking_space));
        }
    }
}
