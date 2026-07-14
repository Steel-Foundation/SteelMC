use text_components::TextComponent;

use super::{
    ArgumentType, CommandDispatcher, CommandNodeBuilder, CommandRequirement, NodeId, StringRange,
    Suggestion, SuggestionError, Suggestions, SuggestionsBuilder, argument, literal,
};

#[derive(Debug)]
struct TestSource {
    allowed: bool,
}

fn register(
    dispatcher: &mut CommandDispatcher<TestSource>,
    builder: CommandNodeBuilder<TestSource>,
) -> NodeId {
    let Ok(node) = dispatcher.register(builder) else {
        panic!("command registration should succeed");
    };
    node
}

fn texts(suggestions: &Suggestions) -> Vec<&str> {
    suggestions.list().iter().map(Suggestion::text).collect()
}

fn suggestions(
    dispatcher: &CommandDispatcher<TestSource>,
    input: &str,
    allowed: bool,
) -> Suggestions {
    let parse = dispatcher.parse(input, TestSource { allowed });
    let Ok(suggestions) = dispatcher.completion_suggestions(&parse) else {
        panic!("parser-generated suggestion ranges should be valid");
    };
    suggestions
}

#[test]
fn suggestion_builder_replaces_remaining_text_and_omits_noops() {
    let Ok(mut builder) = SuggestionsBuilder::new("Hello w", 6) else {
        panic!("suggestion start should be valid");
    };
    builder.suggest("world!");
    builder.suggest("everybody");
    builder.suggest("w");
    let Ok(suggestions) = builder.build() else {
        panic!("builder ranges should remain valid");
    };

    assert_eq!(suggestions.range(), StringRange::between(6, 7));
    assert_eq!(texts(&suggestions), ["everybody", "world!"]);
}

#[test]
fn integer_suggestions_sort_numerically() {
    let Ok(mut builder) = SuggestionsBuilder::new("value ", 6) else {
        panic!("suggestion start should be valid");
    };
    for value in [2, 4, 6, 8, 30, 32] {
        builder.suggest_integer(value);
    }
    let Ok(suggestions) = builder.build() else {
        panic!("builder ranges should remain valid");
    };

    assert_eq!(texts(&suggestions), ["2", "4", "6", "8", "30", "32"]);
}

#[test]
fn suggestions_merge_different_ranges() {
    let first = Suggestions::new(
        StringRange::at(5),
        vec![
            Suggestion::new(StringRange::at(5), "ar"),
            Suggestion::new(StringRange::at(5), "az"),
        ],
    );
    let second = Suggestions::new(
        StringRange::between(4, 5),
        vec![Suggestion::new(StringRange::between(4, 5), "apple")],
    );

    let Ok(merged) = Suggestions::merge("foo b", vec![first, second]) else {
        panic!("suggestion ranges should be valid");
    };

    assert_eq!(merged.range(), StringRange::between(4, 5));
    assert_eq!(texts(&merged), ["apple", "bar", "baz"]);
}

#[test]
fn suggestion_application_uses_utf16_ranges() {
    let suggestion = Suggestion::new(StringRange::between(1, 3), "face");
    assert_eq!(suggestion.apply("a\u{1f600}b"), Ok("afaceb".to_owned()));

    let invalid = Suggestion::new(StringRange::between(2, 3), "face");
    assert!(matches!(
        invalid.apply("a\u{1f600}b"),
        Err(SuggestionError::InvalidRange { .. })
    ));
}

#[test]
fn suggestion_tooltips_are_preserved() {
    let tooltip = TextComponent::const_plain("details");
    let suggestion = Suggestion::with_tooltip(StringRange::at(0), "value", tooltip.clone());

    assert_eq!(suggestion.range(), StringRange::at(0));
    assert_eq!(suggestion.tooltip(), Some(&tooltip));
}

#[test]
fn suggestion_builder_can_restart_for_a_provider() {
    let tooltip = TextComponent::const_plain("details");
    let Ok(mut builder) = SuggestionsBuilder::new("prefix v", 7) else {
        panic!("suggestion start should be valid");
    };
    assert_eq!(builder.input(), "prefix v");
    assert_eq!(builder.start(), 7);
    builder.suggest("discarded");

    let Ok(mut restarted) = builder.restart() else {
        panic!("restarting should preserve a valid range");
    };
    restarted.suggest_with_tooltip("value", tooltip.clone());
    let Ok(suggestions) = restarted.build() else {
        panic!("builder ranges should remain valid");
    };

    assert_eq!(texts(&suggestions), ["value"]);
    assert_eq!(suggestions.list()[0].tooltip(), Some(&tooltip));
}

#[test]
fn completes_root_literals_case_insensitively() {
    let mut dispatcher = CommandDispatcher::new();
    register(&mut dispatcher, literal("foo"));
    register(&mut dispatcher, literal("bar"));
    register(&mut dispatcher, literal("baz"));

    let all = suggestions(&dispatcher, "", true);
    assert_eq!(all.range(), StringRange::at(0));
    assert_eq!(texts(&all), ["bar", "baz", "foo"]);

    let partial = suggestions(&dispatcher, "B", true);
    assert_eq!(partial.range(), StringRange::between(0, 1));
    assert_eq!(texts(&partial), ["bar", "baz"]);
}

#[test]
fn completes_subcommands_and_boolean_arguments() {
    let mut dispatcher = CommandDispatcher::new();
    register(
        &mut dispatcher,
        literal("parent")
            .then(literal("foo"))
            .then(literal("bar"))
            .then(argument("enabled", ArgumentType::bool())),
    );

    let subcommands = suggestions(&dispatcher, "parent ", true);
    assert_eq!(subcommands.range(), StringRange::at(7));
    assert_eq!(texts(&subcommands), ["bar", "false", "foo", "true"]);

    let boolean = suggestions(&dispatcher, "parent t", true);
    assert_eq!(boolean.range(), StringRange::between(7, 8));
    assert_eq!(texts(&boolean), ["true"]);
}

#[test]
fn exact_suggestions_are_omitted() {
    let mut dispatcher = CommandDispatcher::new();
    register(&mut dispatcher, literal("parent").then(literal("child")));

    let suggestions = suggestions(&dispatcher, "parent child", true);

    assert!(suggestions.is_empty());
    assert_eq!(suggestions.range(), StringRange::at(0));
}

#[test]
fn completion_follows_redirect_contexts() {
    let mut dispatcher = CommandDispatcher::new();
    let actual = register(&mut dispatcher, literal("actual").then(literal("sub")));
    register(&mut dispatcher, literal("alias").redirects(actual));

    let suggestions = suggestions(&dispatcher, "alias s", true);

    assert_eq!(suggestions.range(), StringRange::between(6, 7));
    assert_eq!(texts(&suggestions), ["sub"]);
}

#[test]
fn completion_filters_nodes_that_fail_requirements() {
    let mut dispatcher = CommandDispatcher::new();
    register(&mut dispatcher, literal("public"));
    register(
        &mut dispatcher,
        literal("secret").requires(CommandRequirement::contextual(|source: &TestSource| {
            source.allowed
        })),
    );

    assert_eq!(texts(&suggestions(&dispatcher, "", false)), ["public"]);
    assert_eq!(
        texts(&suggestions(&dispatcher, "", true)),
        ["public", "secret"]
    );
}

#[test]
fn completion_ranges_count_supplementary_characters_as_two_units() {
    let mut dispatcher = CommandDispatcher::new();
    register(&mut dispatcher, literal("\u{1f600}").then(literal("child")));

    let child = suggestions(&dispatcher, "\u{1f600} ", true);
    assert_eq!(child.range(), StringRange::at(3));
    assert_eq!(texts(&child), ["child"]);

    let mut root = CommandDispatcher::new();
    register(&mut root, literal("\u{1f600}face"));
    let partial = suggestions(&root, "\u{1f600}", true);
    assert_eq!(partial.range(), StringRange::between(0, 2));
    assert_eq!(texts(&partial), ["\u{1f600}face"]);
}
