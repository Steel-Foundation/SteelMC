//! Vanilla `MessageArgument` parsing, length limits, and selector substitution.

use std::collections::BTreeMap;

use simdnbt::owned::NbtTag;
use steel_registry::init_vanilla_registry;
use steel_utils::text::DisplayResolutor;
use text_components::{
    Modifier as _,
    content::{Content, NbtSource},
    format::Color,
};

use super::{
    CommandSyntaxError, CommandSyntaxErrorKind, SteelArgumentType, TestDispatcher, TestSource,
    TextComponent, argument, literal,
};
use crate::command::execution::text::CommandTextResolutionSource;

/// A resolution source with a fixed selector-to-display-name table.
#[derive(Default)]
struct TestResolutionSource {
    display_names: BTreeMap<String, Vec<TextComponent>>,
}

impl CommandTextResolutionSource for TestResolutionSource {
    fn selector_display_names(
        &self,
        selector: &str,
    ) -> Result<Vec<TextComponent>, CommandSyntaxError> {
        Ok(self
            .display_names
            .get(selector)
            .cloned()
            .unwrap_or_default())
    }

    fn score_selector_names(
        &self,
        _selector: &str,
    ) -> Result<Option<Vec<String>>, CommandSyntaxError> {
        Ok(None)
    }

    fn score(&self, _holder: &str, _objective: &str) -> Result<Option<i32>, CommandSyntaxError> {
        Ok(None)
    }

    fn nbt_source(&self, _source: &NbtSource) -> Result<Vec<NbtTag>, CommandSyntaxError> {
        Ok(Vec::new())
    }
}

fn message_dispatcher() -> TestDispatcher {
    let mut dispatcher = TestDispatcher::new();
    let command = literal("say").then(argument("message", SteelArgumentType::message()).executes(
        |context| {
            // Touch the parsed value so a wrongly typed argument fails the test loudly.
            context.message("message").map(|_| 1)
        },
    ));
    assert!(dispatcher.register(command).is_ok());
    dispatcher
}

/// Parses `input` and resolves the message against `source`, or panics.
fn resolved_message(source: &TestResolutionSource, input: &str) -> TextComponent {
    resolved_message_with(source, TestSource::new(), input)
}

fn resolved_message_with(
    resolution: &TestResolutionSource,
    parse_source: TestSource,
    input: &str,
) -> TextComponent {
    let dispatcher = message_dispatcher();
    let parse = dispatcher.parse(input, parse_source);
    let Ok(chain) = dispatcher.context_chain(parse) else {
        panic!("{input} should parse");
    };
    let Ok(message) = chain.top_context().message("message") else {
        panic!("{input} should bind a message argument");
    };
    let Ok(resolved) = message.resolve(resolution) else {
        panic!("{input} should resolve");
    };
    resolved
}

#[test]
fn message_argument_consumes_the_whole_remainder_as_plain_text() {
    let source = TestResolutionSource::default();

    for text in [
        "hello world",
        "this is a longer message",
        "trailing spaces stay   ",
        "{not structured}",
        "true",
    ] {
        let resolved = resolved_message(&source, &format!("say {text}"));
        assert_eq!(resolved.to_plain(&DisplayResolutor), text, "{text}");
    }
}

#[test]
fn message_argument_substitutes_permitted_selectors() {
    let mut source = TestResolutionSource::default();
    source
        .display_names
        .insert("@p".to_owned(), vec![TextComponent::plain("Alex")]);

    let resolved = resolved_message(&source, "say hello @p and welcome");
    assert_eq!(
        resolved.to_plain(&DisplayResolutor),
        "hello Alex and welcome"
    );
}

#[test]
fn message_argument_joins_multiple_entities_with_the_gray_vanilla_separator() {
    let mut source = TestResolutionSource::default();
    source.display_names.insert(
        "@a".to_owned(),
        vec![
            TextComponent::plain("Player1").color(Color::Red),
            TextComponent::plain("Player2"),
            TextComponent::plain("Player3"),
        ],
    );

    let resolved = resolved_message(&source, "say @a");
    assert_eq!(
        resolved.to_plain(&DisplayResolutor),
        "Player1, Player2, Player3"
    );

    // The selector expands into one joined child; its separators use vanilla's gray
    // `ComponentUtils.DEFAULT_SEPARATOR`, and each display name keeps its own styling.
    let joined = &resolved.children[0];
    assert_eq!(joined.children[0].format.color, Some(Color::Red));
    for separator in [&joined.children[1], &joined.children[3]] {
        assert_eq!(separator.format.color, Some(Color::Gray));
        assert!(
            matches!(&separator.content, Content::Text { text } if text.as_ref() == ", "),
            "separator should be a plain \", \": {separator:?}"
        );
    }
}

#[test]
fn message_argument_keeps_unknown_selector_types_literal() {
    let source = TestResolutionSource::default();

    // Vanilla only treats `@` as a selector when a known selector type follows it; anything
    // else stays literal text rather than failing the command.
    for text in ["email me @ home", "@", "trailing @", "@x is not a selector"] {
        let resolved = resolved_message(&source, &format!("say {text}"));
        assert_eq!(resolved.to_plain(&DisplayResolutor), text, "{text}");
    }

    // A known selector type is consumed even when more word characters follow it, matching
    // vanilla: `@everyone` is the selector `@e` followed by the literal text `veryone`.
    let resolved = resolved_message(&source, "say ping @everyone");
    assert_eq!(resolved.to_plain(&DisplayResolutor), "ping veryone");
}

#[test]
fn message_argument_scans_adjacent_selectors() {
    let mut source = TestResolutionSource::default();
    source
        .display_names
        .insert("@s".to_owned(), vec![TextComponent::plain("Alex")]);

    // Scanning resumes immediately after a parsed selector, so a second selector that starts
    // on the very next character is still found.
    let resolved = resolved_message(&source, "say @s@s");
    assert_eq!(resolved.to_plain(&DisplayResolutor), "AlexAlex");
}

#[test]
fn message_argument_substitutes_selectors_in_non_ascii_text() {
    let mut source = TestResolutionSource::default();
    source
        .display_names
        .insert("@p".to_owned(), vec![TextComponent::plain("Alex")]);

    // Selector spans index the raw text by byte, so multi-byte characters on either side of a
    // selector must not shift or split the substitution.
    for (input, expected) in [
        ("say 中文 @p 中文", "中文 Alex 中文"),
        ("say \u{1f600}@p\u{1f600}", "\u{1f600}Alex\u{1f600}"),
        ("say héllo @p", "héllo Alex"),
    ] {
        let resolved = resolved_message(&source, input);
        assert_eq!(resolved.to_plain(&DisplayResolutor), expected, "{input}");
    }
}

#[test]
fn message_argument_propagates_real_selector_syntax_errors() {
    init_vanilla_registry();
    let dispatcher = message_dispatcher();

    for input in [
        "say hello @a[",
        "say @e[type=]",
        "say @a[gamemode=nonsense]",
    ] {
        let parse = dispatcher.parse(input, TestSource::new());
        assert!(
            dispatcher.context_chain(parse).is_err(),
            "{input} should report a selector syntax error"
        );
    }
}

#[test]
fn message_argument_leaves_selectors_literal_without_selector_permission() {
    let mut source = TestResolutionSource::default();
    source
        .display_names
        .insert("@a".to_owned(), vec![TextComponent::plain("Alex")]);

    // A source that may not use selectors sees `@a` as ordinary text, and a selector that
    // would otherwise be a syntax error is likewise inert.
    for text in ["@a", "hello @a there", "@a["] {
        let resolved = resolved_message_with(
            &source,
            TestSource::without_selectors(),
            &format!("say {text}"),
        );
        assert_eq!(resolved.to_plain(&DisplayResolutor), text, "{text}");
    }
}

#[test]
fn message_argument_length_limit_counts_java_string_characters() {
    let dispatcher = message_dispatcher();

    for (label, text, accepted) in [
        ("256 ASCII", "a".repeat(256), true),
        ("257 ASCII", "a".repeat(257), false),
        // 200 three-byte characters: 600 UTF-8 bytes but only 200 Java characters, so
        // vanilla accepts it. A byte-based limit would reject this.
        ("200 BMP", "中".repeat(200), true),
        ("257 BMP", "中".repeat(257), false),
        // Outside the BMP each character is a surrogate pair: 128 of them is exactly 256
        // Java characters, 129 is 258.
        ("128 astral", "\u{1f600}".repeat(128), true),
        ("129 astral", "\u{1f600}".repeat(129), false),
    ] {
        let input = format!("say {text}");
        let parse = dispatcher.parse(&input, TestSource::new());
        let result = dispatcher.context_chain(parse);
        assert_eq!(result.is_ok(), accepted, "{label}");

        if let Err(error) = result {
            let CommandSyntaxErrorKind::Dynamic(component) = error.kind() else {
                panic!("{label} should use a translated dynamic error");
            };
            assert!(
                matches!(
                    &component.content,
                    Content::Translate(message) if message.key == "argument.message.too_long"
                ),
                "{label} used {component:?}"
            );
        }
    }
}
