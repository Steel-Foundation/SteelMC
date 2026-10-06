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

/// Mirrors vanilla `StringUtil.isBlank`.
#[must_use]
pub fn is_blank(value: &str) -> bool {
    value
        .chars()
        .all(|character| is_whitespace(character) || is_space_char(character))
}

/// Formats an `f32` like Java's `Float.toString`.
#[must_use]
pub fn float_to_string(value: f32) -> String {
    floating_to_string(
        value.is_sign_negative(),
        value.is_nan(),
        value.is_infinite(),
        value == 0.0,
        &format!("{:.8e}", value.abs()),
        9,
        |precision| format!("{:.*e}", precision, value.abs()),
        |candidate| candidate.parse::<f32>().ok().map(f32::to_bits) == Some(value.abs().to_bits()),
    )
}

pub(crate) fn double_to_string(value: f64) -> String {
    floating_to_string(
        value.is_sign_negative(),
        value.is_nan(),
        value.is_infinite(),
        value == 0.0,
        &format!("{:.16e}", value.abs()),
        17,
        |precision| format!("{:.*e}", precision, value.abs()),
        |candidate| candidate.parse::<f64>().ok().map(f64::to_bits) == Some(value.abs().to_bits()),
    )
}

#[expect(
    clippy::too_many_arguments,
    clippy::fn_params_excessive_bools,
    reason = "shared Java float formatting parameters"
)]
fn floating_to_string(
    negative: bool,
    nan: bool,
    infinite: bool,
    zero: bool,
    precise: &str,
    max_digits: usize,
    scientific: impl Fn(usize) -> String,
    rounds_to_value: impl Fn(&str) -> bool,
) -> String {
    if nan {
        return "NaN".to_owned();
    }
    if infinite {
        return if negative { "-Infinity" } else { "Infinity" }.to_owned();
    }
    if zero {
        return if negative { "-0.0" } else { "0.0" }.to_owned();
    }

    let Some((precise_mantissa, _)) = precise.split_once('e') else {
        panic!("Rust scientific formatting omitted its exponent");
    };
    let precise_digits = precise_mantissa.replace('.', "");
    let one_digit_is_exact = precise_digits[1..].bytes().all(|digit| digit == b'0');
    let mut selected = None;
    for length in 1..=max_digits {
        let formatted = scientific(length - 1);
        let Some((mantissa, exponent)) = formatted.split_once('e') else {
            panic!("Rust scientific formatting omitted its exponent");
        };
        if !rounds_to_value(&formatted) {
            continue;
        }
        let Ok(exponent) = exponent.parse::<i32>() else {
            panic!("Rust scientific formatting emitted a non-decimal exponent");
        };
        let digits = mantissa.replace('.', "");
        selected = Some((digits, exponent - length as i32 + 1));
        if length >= 2 || one_digit_is_exact {
            break;
        }
    }
    let Some((mut digits, decimal_exponent)) = selected else {
        panic!("full-precision Rust decimal did not round-trip");
    };
    while digits.ends_with('0') {
        digits.pop();
    }
    let scientific_exponent = digits.len() as i32 + decimal_exponent - 1;
    let mut output = if (-3..0).contains(&scientific_exponent) {
        format!(
            "0.{}{}",
            "0".repeat((-scientific_exponent - 1) as usize),
            digits
        )
    } else if (0..7).contains(&scientific_exponent) {
        let decimal_position = (scientific_exponent + 1) as usize;
        if decimal_position >= digits.len() {
            format!(
                "{}{}.0",
                digits,
                "0".repeat(decimal_position - digits.len())
            )
        } else {
            format!(
                "{}.{}",
                &digits[..decimal_position],
                &digits[decimal_position..]
            )
        }
    } else {
        let fraction = if digits.len() == 1 { "0" } else { &digits[1..] };
        format!("{}.{}E{scientific_exponent}", &digits[..1], fraction)
    };
    if negative {
        output.insert(0, '-');
    }
    output
}

#[cfg(test)]
mod tests {
    use super::{float_to_string, is_blank, is_space_char, is_whitespace};

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
    fn float_string_matches_java_integral_values() {
        assert_eq!(float_to_string(0.0), "0.0");
        assert_eq!(float_to_string(-0.0), "-0.0");
        assert_eq!(float_to_string(90.0), "90.0");
        assert_eq!(float_to_string(45.5), "45.5");
    }
}
