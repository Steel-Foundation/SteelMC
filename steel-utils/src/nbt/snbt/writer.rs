use std::fmt::Write as _;

use simdnbt::owned::NbtTag;

use crate::nbt::nbt_list_values;

/// Renders an NBT value with Vanilla `Tag::toString` semantics.
///
/// Compound keys use Java string ordering, and floating-point values use the
/// decimal selected by `Float.toString` / `Double.toString`.
#[must_use]
pub fn to_canonical_snbt(tag: &NbtTag) -> Option<String> {
    let mut output = String::new();
    write_canonical_snbt(tag, &mut output)?;
    Some(output)
}

fn write_canonical_snbt(tag: &NbtTag, output: &mut String) -> Option<()> {
    match tag {
        NbtTag::Byte(value) => write!(output, "{value}b").ok()?,
        NbtTag::Short(value) => write!(output, "{value}s").ok()?,
        NbtTag::Int(value) => write!(output, "{value}").ok()?,
        NbtTag::Long(value) => write!(output, "{value}L").ok()?,
        NbtTag::Float(value) => write!(output, "{}f", java_float_string(*value)).ok()?,
        NbtTag::Double(value) => write!(output, "{}d", java_double_string(*value)).ok()?,
        NbtTag::ByteArray(values) => {
            output.push_str("[B;");
            for (index, value) in values.iter().enumerate() {
                if index != 0 {
                    output.push(',');
                }
                write!(output, "{}B", *value as i8).ok()?;
            }
            output.push(']');
        }
        NbtTag::String(value) => {
            quote_and_escape(&value.to_owned().try_into_string().ok()?, output);
        }
        NbtTag::List(values) => {
            output.push('[');
            for (index, value) in nbt_list_values(values).iter().enumerate() {
                if index != 0 {
                    output.push(',');
                }
                write_canonical_snbt(value, output)?;
            }
            output.push(']');
        }
        NbtTag::Compound(compound) => {
            let mut entries = compound
                .iter()
                .map(|(key, value)| Some((key.to_owned().try_into_string().ok()?, value)))
                .collect::<Option<Vec<_>>>()?;
            entries.sort_by(|(left, _), (right, _)| left.encode_utf16().cmp(right.encode_utf16()));
            output.push('{');
            for (index, (key, value)) in entries.into_iter().enumerate() {
                if index != 0 {
                    output.push(',');
                }
                write_key(&key, output);
                output.push(':');
                write_canonical_snbt(value, output)?;
            }
            output.push('}');
        }
        NbtTag::IntArray(values) => {
            output.push_str("[I;");
            for (index, value) in values.iter().enumerate() {
                if index != 0 {
                    output.push(',');
                }
                write!(output, "{value}").ok()?;
            }
            output.push(']');
        }
        NbtTag::LongArray(values) => {
            output.push_str("[L;");
            for (index, value) in values.iter().enumerate() {
                if index != 0 {
                    output.push(',');
                }
                write!(output, "{value}L").ok()?;
            }
            output.push(']');
        }
    }
    Some(())
}

fn write_key(value: &str, output: &mut String) {
    let mut chars = value.chars();
    let simple = !value.eq_ignore_ascii_case("true")
        && !value.eq_ignore_ascii_case("false")
        && chars.next().is_some_and(|character| {
            character.is_ascii_alphabetic() || matches!(character, '.' | '_')
        })
        && chars.all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '+' | '-')
        });
    if simple {
        output.push_str(value);
    } else {
        quote_and_escape(value, output);
    }
}

fn quote_and_escape(value: &str, output: &mut String) {
    let quote = value
        .chars()
        .find_map(|character| match character {
            '"' => Some('\''),
            '\'' => Some('"'),
            _ => None,
        })
        .unwrap_or('"');
    output.push(quote);
    for character in value.chars() {
        match character {
            '\\' => output.push_str("\\\\"),
            character if character == quote => {
                output.push('\\');
                output.push(character);
            }
            '\u{0008}' => output.push_str("\\b"),
            '\t' => output.push_str("\\t"),
            '\n' => output.push_str("\\n"),
            '\u{000c}' => output.push_str("\\f"),
            '\r' => output.push_str("\\r"),
            character if character < ' ' => {
                let _ = write!(output, "\\x{:02x}", u32::from(character));
            }
            character => output.push(character),
        }
    }
    output.push(quote);
}

fn java_float_string(value: f32) -> String {
    java_floating_string(
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

fn java_double_string(value: f64) -> String {
    java_floating_string(
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
fn java_floating_string(
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
