//! Vanilla-shaped text rendering for command-visible NBT values.

use std::fmt::Write as _;

use simdnbt::owned::{NbtCompound, NbtTag};
use text_components::{Modifier as _, TextComponent, format::Color};

const MAX_DEPTH: usize = 64;
const MAX_ARRAY_LENGTH: usize = 128;

/// Renders one NBT tag using vanilla's command-component syntax highlighting.
///
/// Float and double digits intentionally use Rust formatting. All surrounding
/// SNBT syntax and styling follows `TextComponentTagVisitor` from vanilla 26.2.
#[must_use]
pub fn command_nbt_component(tag: &NbtTag, plain: bool) -> TextComponent {
    let mut visitor = NbtComponentVisitor {
        plain,
        sort_keys: tracing::enabled!(tracing::Level::DEBUG),
        result: TextComponent::new(),
    };
    visitor.visit(tag, 0);
    visitor.result
}

struct NbtComponentVisitor {
    plain: bool,
    sort_keys: bool,
    result: TextComponent,
}

impl NbtComponentVisitor {
    fn visit(&mut self, tag: &NbtTag, depth: usize) {
        match tag {
            NbtTag::Byte(value) => {
                self.number((*value).to_string());
                self.number_type("b");
            }
            NbtTag::Short(value) => {
                self.number(value.to_string());
                self.number_type("s");
            }
            NbtTag::Int(value) => self.number(value.to_string()),
            NbtTag::Long(value) => {
                self.number(value.to_string());
                self.number_type("L");
            }
            NbtTag::Float(value) => {
                self.number(format!("{value:?}"));
                self.number_type("f");
            }
            NbtTag::Double(value) => {
                self.number(format!("{value:?}"));
                self.number_type("d");
            }
            NbtTag::ByteArray(values) => self.byte_array(values),
            NbtTag::String(value) => self.string(&value.to_string()),
            NbtTag::List(values) => {
                let values = values.as_nbt_tags();
                self.list(&values, depth);
            }
            NbtTag::Compound(value) => self.compound(value, depth),
            NbtTag::IntArray(values) => self.int_array(values),
            NbtTag::LongArray(values) => self.long_array(values),
        }
    }

    fn byte_array(&mut self, values: &[u8]) {
        self.token("[");
        self.number_type("B");
        self.token(";");
        for (index, value) in values.iter().take(MAX_ARRAY_LENGTH).enumerate() {
            self.token(" ");
            self.number((*value as i8).to_string());
            self.number_type("b");
            if index + 1 != values.len() {
                self.token(",");
            }
        }
        if values.len() > MAX_ARRAY_LENGTH {
            self.folded();
        }
        self.token("]");
    }

    fn int_array(&mut self, values: &[i32]) {
        self.token("[");
        self.number_type("I");
        self.token(";");
        for (index, value) in values.iter().take(MAX_ARRAY_LENGTH).enumerate() {
            self.token(" ");
            self.number(value.to_string());
            if index + 1 != values.len() {
                self.token(",");
            }
        }
        if values.len() > MAX_ARRAY_LENGTH {
            self.folded();
        }
        self.token("]");
    }

    fn long_array(&mut self, values: &[i64]) {
        self.token("[");
        self.number_type("L");
        self.token(";");
        for (index, value) in values.iter().take(MAX_ARRAY_LENGTH).enumerate() {
            self.token(" ");
            self.number(value.to_string());
            self.number_type("L");
            if index + 1 != values.len() {
                self.token(",");
            }
        }
        if values.len() > MAX_ARRAY_LENGTH {
            self.folded();
        }
        self.token("]");
    }

    fn list(&mut self, values: &[NbtTag], depth: usize) {
        self.token("[");
        if values.is_empty() {
            self.token("]");
            return;
        }
        if depth >= MAX_DEPTH {
            self.folded();
            self.token("]");
            return;
        }
        for (index, value) in values.iter().enumerate() {
            if index != 0 {
                self.token(",");
                self.token(" ");
            }
            self.visit(value, depth + 1);
        }
        self.token("]");
    }

    fn compound(&mut self, value: &NbtCompound, depth: usize) {
        self.token("{");
        if value.is_empty() {
            self.token("}");
            return;
        }
        if depth >= MAX_DEPTH {
            self.folded();
            self.token("}");
            return;
        }
        let mut entries = value
            .iter()
            .map(|(key, tag)| (key.to_string(), tag))
            .collect::<Vec<_>>();
        if self.sort_keys {
            entries.sort_by(|(left, _), (right, _)| left.encode_utf16().cmp(right.encode_utf16()));
        }
        for (index, (key, tag)) in entries.into_iter().enumerate() {
            if index != 0 {
                self.token(",");
                self.token(" ");
            }
            self.key(&key);
            self.token(":");
            self.token(" ");
            self.visit(tag, depth + 1);
        }
        self.token("}");
    }

    fn string(&mut self, value: &str) {
        let (quote, escaped) = quote_and_escape(value);
        self.token(&quote.to_string());
        self.string_value(escaped);
        self.token(&quote.to_string());
    }

    fn key(&mut self, value: &str) {
        if is_simple_value(value) {
            self.key_value(value.to_owned());
            return;
        }
        let (quote, escaped) = quote_and_escape(value);
        self.token(&quote.to_string());
        self.key_value(escaped);
        self.token(&quote.to_string());
    }

    fn token(&mut self, value: &str) {
        self.result
            .children
            .push(TextComponent::plain(value.to_owned()));
    }

    fn folded(&mut self) {
        self.styled("<...>".to_owned(), Color::Gray);
    }

    fn key_value(&mut self, value: String) {
        self.styled(value, Color::Aqua);
    }

    fn string_value(&mut self, value: String) {
        self.styled(value, Color::Green);
    }

    fn number(&mut self, value: String) {
        self.styled(value, Color::Gold);
    }

    fn number_type(&mut self, value: &str) {
        self.styled(value.to_owned(), Color::Red);
    }

    fn styled(&mut self, value: String, color: Color) {
        let component = if self.plain {
            TextComponent::plain(value)
        } else {
            TextComponent::plain(value).color(color)
        };
        self.result.children.push(component);
    }
}

fn is_simple_value(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'+' | b'-'))
}

fn quote_and_escape(value: &str) -> (char, String) {
    let quote = value
        .chars()
        .find_map(|character| match character {
            '"' => Some('\''),
            '\'' => Some('"'),
            _ => None,
        })
        .unwrap_or('"');
    let mut escaped = String::new();
    for character in value.chars() {
        match character {
            '\\' => escaped.push_str("\\\\"),
            character if character == quote => {
                escaped.push('\\');
                escaped.push(character);
            }
            '\u{0008}' => escaped.push_str("\\b"),
            '\t' => escaped.push_str("\\t"),
            '\n' => escaped.push_str("\\n"),
            '\u{000c}' => escaped.push_str("\\f"),
            '\r' => escaped.push_str("\\r"),
            character if character < ' ' => {
                let _ = write!(escaped, "\\x{:02x}", u32::from(character));
            }
            character => escaped.push(character),
        }
    }
    (quote, escaped)
}

#[cfg(test)]
mod tests {
    use simdnbt::owned::{NbtCompound, NbtList, NbtTag};

    use crate::text::DisplayResolutor;

    use super::command_nbt_component;

    fn plain(tag: &NbtTag) -> String {
        command_nbt_component(tag, true).to_plain(&DisplayResolutor)
    }

    #[test]
    fn renders_vanilla_suffixes_arrays_and_signed_bytes() {
        assert_eq!(plain(&NbtTag::Long(7)), "7L");
        assert_eq!(plain(&NbtTag::ByteArray(vec![255, 1])), "[B; -1b, 1b]");
        assert_eq!(plain(&NbtTag::LongArray(vec![2, 3])), "[L; 2L, 3L]");
    }

    #[test]
    fn quotes_and_escapes_strings_and_compound_keys() {
        let mut compound = NbtCompound::new();
        compound.insert("simple", NbtTag::String("can't \"stop\"\n".into()));
        compound.insert("needs space", NbtTag::Int(1));

        assert_eq!(
            plain(&NbtTag::Compound(compound)),
            "{simple: \"can't \\\"stop\\\"\\n\", \"needs space\": 1}"
        );
    }

    #[test]
    fn folds_nested_collections_at_vanillas_depth_limit() {
        let mut tag = NbtTag::Compound(NbtCompound::new());
        for _ in 0..=64 {
            let mut parent = NbtCompound::new();
            parent.insert("value", tag);
            tag = NbtTag::Compound(parent);
        }

        assert!(plain(&tag).contains("{<...>}"));
    }

    #[test]
    fn keeps_empty_collections_visible_at_vanillas_depth_limit() {
        let mut tag = NbtTag::Compound(NbtCompound::new());
        for _ in 0..64 {
            let mut parent = NbtCompound::new();
            parent.insert("value", tag);
            tag = NbtTag::Compound(parent);
        }

        assert!(!plain(&tag).contains("<...>"));
    }

    #[test]
    fn folds_arrays_after_vanillas_element_limit() {
        let rendered = plain(&NbtTag::IntArray((0..129).collect()));

        assert!(rendered.ends_with("127,<...>]"));
        assert!(!rendered.contains(" 128"));
    }

    #[test]
    fn renders_lists_with_vanillas_spacing() {
        let tag = NbtTag::List(NbtList::String(vec!["one".into(), "two".into()]));

        assert_eq!(plain(&tag), "[\"one\", \"two\"]");
    }
}
