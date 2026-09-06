use std::fmt::Write as _;

use simdnbt::owned::NbtTag;

use crate::{
    java::{double_to_string, float_to_string},
    nbt::nbt_list_values,
};

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
        NbtTag::Float(value) => write!(output, "{}f", float_to_string(*value)).ok()?,
        NbtTag::Double(value) => write!(output, "{}d", double_to_string(*value)).ok()?,
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
