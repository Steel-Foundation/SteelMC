//! Styled text for messages, titles and chat.
//!
//! Every API taking text accepts either a vanilla JSON text component (any string starting
//! with `{`) or plain text. [`Text`] builds components; [`legacy`] converts Bukkit-style
//! `&` color codes (including `&#rrggbb`) as used by `EssentialsX` configuration files.

use core::fmt::Write as _;

/// A text component builder serialized to vanilla JSON.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Text {
    text: String,
    color: Option<String>,
    flags: [Option<bool>; 5],
    click: Option<(&'static str, String)>,
    hover: Option<Box<Text>>,
    insertion: Option<String>,
    extra: Vec<Text>,
}

const BOLD: usize = 0;
const ITALIC: usize = 1;
const UNDERLINED: usize = 2;
const STRIKETHROUGH: usize = 3;
const OBFUSCATED: usize = 4;
const FLAG_NAMES: [&str; 5] = [
    "bold",
    "italic",
    "underlined",
    "strikethrough",
    "obfuscated",
];

impl Text {
    /// Creates a literal text component.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ..Self::default()
        }
    }

    /// Sets a named color (`gold`) or hex color (`#ffaa00`).
    #[must_use]
    pub fn color(mut self, color: impl Into<String>) -> Self {
        self.color = Some(color.into());
        self
    }

    /// Sets bold.
    #[must_use]
    pub const fn bold(mut self, value: bool) -> Self {
        self.flags[BOLD] = Some(value);
        self
    }

    /// Sets italic.
    #[must_use]
    pub const fn italic(mut self, value: bool) -> Self {
        self.flags[ITALIC] = Some(value);
        self
    }

    /// Sets underlined.
    #[must_use]
    pub const fn underlined(mut self, value: bool) -> Self {
        self.flags[UNDERLINED] = Some(value);
        self
    }

    /// Sets strikethrough.
    #[must_use]
    pub const fn strikethrough(mut self, value: bool) -> Self {
        self.flags[STRIKETHROUGH] = Some(value);
        self
    }

    /// Runs `command` (with its leading `/`) when clicked.
    #[must_use]
    pub fn click_run_command(mut self, command: impl Into<String>) -> Self {
        self.click = Some(("run_command", command.into()));
        self
    }

    /// Puts `command` in the chat box when clicked.
    #[must_use]
    pub fn click_suggest_command(mut self, command: impl Into<String>) -> Self {
        self.click = Some(("suggest_command", command.into()));
        self
    }

    /// Opens `url` when clicked.
    #[must_use]
    pub fn click_open_url(mut self, url: impl Into<String>) -> Self {
        self.click = Some(("open_url", url.into()));
        self
    }

    /// Shows `text` when hovered.
    #[must_use]
    pub fn hover(mut self, text: Self) -> Self {
        self.hover = Some(Box::new(text));
        self
    }

    /// Inserts `text` into the chat box on shift-click.
    #[must_use]
    pub fn insertion(mut self, text: impl Into<String>) -> Self {
        self.insertion = Some(text.into());
        self
    }

    /// Appends a child component inheriting this component's style.
    #[must_use]
    pub fn append(mut self, child: Self) -> Self {
        self.extra.push(child);
        self
    }

    /// Serializes to vanilla JSON.
    #[must_use]
    pub fn to_json(&self) -> String {
        let mut out = String::new();
        self.write_json(&mut out);
        out
    }

    fn write_json(&self, out: &mut String) {
        out.push_str("{\"text\":");
        write_string(out, &self.text);
        if let Some(color) = &self.color {
            out.push_str(",\"color\":");
            write_string(out, color);
        }
        for (flag, name) in self.flags.iter().zip(FLAG_NAMES) {
            if let Some(value) = flag {
                let _ = write!(out, ",\"{name}\":{value}");
            }
        }
        if let Some((action, value)) = &self.click {
            let field = match *action {
                "open_url" => "url",
                _ => "command",
            };
            let _ = write!(
                out,
                ",\"click_event\":{{\"action\":\"{action}\",\"{field}\":"
            );
            write_string(out, value);
            out.push('}');
        }
        if let Some(hover) = &self.hover {
            out.push_str(",\"hover_event\":{\"action\":\"show_text\",\"value\":");
            hover.write_json(out);
            out.push('}');
        }
        if let Some(insertion) = &self.insertion {
            out.push_str(",\"insertion\":");
            write_string(out, insertion);
        }
        if !self.extra.is_empty() {
            out.push_str(",\"extra\":[");
            for (index, child) in self.extra.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                child.write_json(out);
            }
            out.push(']');
        }
        out.push('}');
    }

    /// Plain text of this component and its children, without styling.
    #[must_use]
    pub fn plain(&self) -> String {
        let mut out = self.text.clone();
        for child in &self.extra {
            out.push_str(&child.plain());
        }
        out
    }
}

impl From<&str> for Text {
    fn from(text: &str) -> Self {
        Self::new(text)
    }
}

impl From<String> for Text {
    fn from(text: String) -> Self {
        Self::new(text)
    }
}

fn write_string(out: &mut String, value: &str) {
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            control if u32::from(control) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", u32::from(control));
            }
            other => out.push(other),
        }
    }
    out.push('"');
}

const LEGACY_COLORS: [&str; 16] = [
    "black",
    "dark_blue",
    "dark_green",
    "dark_aqua",
    "dark_red",
    "dark_purple",
    "gold",
    "gray",
    "dark_gray",
    "blue",
    "green",
    "aqua",
    "red",
    "light_purple",
    "yellow",
    "white",
];

/// Parses Bukkit legacy formatting (`&a`, `&l`, `&r`, `&#rrggbb`, also `§`) into a component.
///
/// Like the legacy format, a color code resets all decorations that precede it.
#[must_use]
pub fn legacy(input: &str) -> Text {
    let mut root = Text::new("");
    let mut current = Text::new("");
    let mut characters = input.chars().peekable();
    while let Some(character) = characters.next() {
        if character != '&' && character != '§' {
            current.text.push(character);
            continue;
        }
        let Some(&code) = characters.peek() else {
            current.text.push(character);
            continue;
        };
        let style = if code == '#' {
            let hex: String = characters.clone().skip(1).take(6).collect();
            if hex.len() == 6 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
                for _ in 0..7 {
                    characters.next();
                }
                Some(Style::Color(format!("#{}", hex.to_ascii_lowercase())))
            } else {
                None
            }
        } else {
            let style = Style::from_code(code);
            if style.is_some() {
                characters.next();
            }
            style
        };
        let Some(style) = style else {
            current.text.push(character);
            continue;
        };
        let mut next = match &style {
            Style::Color(_) | Style::Reset => Text::new(""),
            Style::Flag(_) => Text {
                text: String::new(),
                ..current.clone()
            },
        };
        match style {
            Style::Color(color) => next.color = Some(color),
            Style::Flag(flag) => next.flags[flag] = Some(true),
            Style::Reset => {}
        }
        if !current.text.is_empty() {
            root.extra.push(current);
        }
        current = next;
    }
    if !current.text.is_empty() {
        root.extra.push(current);
    }
    root
}

/// Removes legacy formatting codes, returning plain text.
#[must_use]
pub fn strip_legacy(input: &str) -> String {
    legacy(input).plain()
}

enum Style {
    Color(String),
    Flag(usize),
    Reset,
}

impl Style {
    fn from_code(code: char) -> Option<Self> {
        let code = code.to_ascii_lowercase();
        if let Some(index) = code.to_digit(16) {
            return Some(Self::Color(LEGACY_COLORS[index as usize].to_owned()));
        }
        Some(match code {
            'l' => Self::Flag(BOLD),
            'o' => Self::Flag(ITALIC),
            'n' => Self::Flag(UNDERLINED),
            'm' => Self::Flag(STRIKETHROUGH),
            'k' => Self::Flag(OBFUSCATED),
            'r' => Self::Reset,
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_escapes_and_serializes_styles() {
        let json = Text::new("say \"hi\"\n")
            .color("gold")
            .bold(true)
            .click_run_command("/spawn")
            .hover(Text::new("go"))
            .to_json();
        assert_eq!(
            json,
            r#"{"text":"say \"hi\"\n","color":"gold","bold":true,"click_event":{"action":"run_command","command":"/spawn"},"hover_event":{"action":"show_text","value":{"text":"go"}}}"#
        );
    }

    #[test]
    fn legacy_colors_reset_decorations_but_flags_accumulate() {
        let text = legacy("&a&lHi &7there&r!");
        let parts: Vec<_> = text
            .extra
            .iter()
            .map(|part| (part.text.as_str(), part.color.as_deref(), part.flags[BOLD]))
            .collect();
        assert_eq!(
            parts,
            [
                ("Hi ", Some("green"), Some(true)),
                ("there", Some("gray"), None),
                ("!", None, None)
            ]
        );
    }

    #[test]
    fn legacy_hex_colors_and_unknown_codes() {
        let text = legacy("&#FFaa00Gold &zstays & end");
        assert_eq!(text.extra[0].color.as_deref(), Some("#ffaa00"));
        assert_eq!(text.plain(), "Gold &zstays & end");
        assert_eq!(strip_legacy("§cRed"), "Red");
    }
}
