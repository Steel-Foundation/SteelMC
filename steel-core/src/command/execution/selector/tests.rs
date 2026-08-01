use std::{ptr, sync::Weak};

use glam::DVec3;
use simdnbt::owned::{NbtCompound, NbtTag};
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::{test_support::init_test_registry, vanilla_entities};
use steel_utils::types::GameType;
use text_components::{TextComponent, content::Content};

use crate::{
    command::{
        brigadier::{CommandSyntaxError, StringReader, Suggestion, SuggestionsBuilder},
        execution::{CommandArgumentSource, CommandResultCallback, ExecutionCommandSource},
    },
    entity::{Entity, EntityBase},
    scoreboard::{ScoreHolder, Scoreboard},
};

use super::model::{
    entity_name_filter_matches, entity_nbt_filter_matches, game_mode_filter_matches,
    score_filter_matches, team_filter_matches,
};
use super::parser::{parse_selector_plan, read_selector_argument};
use super::{
    EntitySelector, IntRange, SelectorFilter, SelectorKind, SelectorParseErrorKind, SelectorType,
    parse_entity_selector, suggest_entity_selector,
};

struct SelectorTestEntity {
    base: EntityBase,
}

impl SelectorTestEntity {
    fn new() -> Self {
        Self {
            base: EntityBase::new(
                1,
                DVec3::ZERO,
                vanilla_entities::ITEM.dimensions,
                Weak::new(),
            ),
        }
    }
}

crate::entity::impl_test_downcast_type!(SelectorTestEntity);

impl Entity for SelectorTestEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        &vanilla_entities::ITEM
    }
}

struct TestSource {
    selectors: bool,
    advanced: bool,
    callback: CommandResultCallback,
}

impl TestSource {
    const fn new(selectors: bool, advanced: bool) -> Self {
        Self {
            selectors,
            advanced,
            callback: CommandResultCallback::empty(),
        }
    }
}

impl ExecutionCommandSource for TestSource {
    fn with_callback(&self, callback: CommandResultCallback) -> Self {
        Self {
            selectors: self.selectors,
            advanced: self.advanced,
            callback,
        }
    }

    fn callback(&self) -> CommandResultCallback {
        self.callback.clone()
    }

    fn handle_error(&self, _error: &CommandSyntaxError, _forked: bool) {}
}

impl CommandArgumentSource for TestSource {
    fn selector_player_names(&self) -> Vec<String> {
        vec!["Alex".to_owned(), "Steve".to_owned()]
    }

    fn selector_team_names(&self) -> Vec<String> {
        vec!["red".to_owned()]
    }

    fn allows_entity_selectors(&self) -> bool {
        self.selectors
    }

    fn allows_advanced_entity_selectors(&self) -> bool {
        self.advanced
    }
}

fn parse(
    input: &str,
    source: &TestSource,
    single: bool,
    players_only: bool,
) -> Result<EntitySelector, CommandSyntaxError> {
    parse_entity_selector(&mut StringReader::new(input), source, single, players_only)
}

#[test]
fn selector_permissions_distinguish_basic_and_advanced_syntax() {
    let denied = TestSource::new(false, false);
    assert!(parse("Steve", &denied, true, true).is_ok());
    assert!(parse("@s", &denied, true, false).is_err());

    let basic = TestSource::new(true, false);
    assert!(parse("@e", &basic, false, false).is_ok());
    assert!(parse("@e[]", &basic, false, false).is_ok());
    assert!(parse("@e[distance=..5]", &basic, false, false).is_err());
}

#[test]
fn selector_argument_shapes_enforce_cardinality_and_player_only_rules() {
    let source = TestSource::new(true, true);

    assert!(parse("@e", &source, true, false).is_err());
    assert!(parse("@e[limit=1]", &source, true, false).is_ok());
    assert!(parse("@e", &source, false, true).is_err());
    assert!(parse("@a", &source, false, true).is_ok());
    assert!(parse("@s", &source, true, true).is_ok());
}

#[test]
fn selector_parses_core_filters_and_nested_snbt() {
    init_test_registry();
    let Ok(selector) = parse_selector_plan(
        "@e[type=pig,nbt={Tags:[\"foo\"]},scores={kills=1..},team=!red]",
        true,
    ) else {
        panic!("selector should parse");
    };

    assert!(matches!(
        selector.kind,
        SelectorKind::Selector(SelectorType::AllEntities)
    ));
    assert!(selector.filters.iter().any(|filter| matches!(
        filter,
        SelectorFilter::EntityType { value, inverted: false }
            if ptr::eq(*value, &raw const vanilla_entities::PIG)
    )));
    assert!(selector.filters.iter().any(|filter| matches!(
        filter,
        SelectorFilter::Nbt {
            inverted: false,
            ..
        }
    )));
    assert!(
        selector
            .filters
            .iter()
            .any(|filter| matches!(filter, SelectorFilter::Scores(scores) if scores.len() == 1))
    );
    assert!(selector.filters.iter().any(|filter| matches!(
        filter,
        SelectorFilter::Team { value, inverted: true } if value == "red"
    )));
}

#[test]
fn selector_preserves_translatable_snbt_errors() {
    let Err(error) = parse_selector_plan("@e[nbt={id:}]", true) else {
        panic!("missing selector NBT value should fail");
    };

    let SelectorParseErrorKind::Invalid(component) = error.kind else {
        panic!("selector error should preserve its text component");
    };
    assert!(matches!(
        component.content,
        Content::Translate(ref message)
            if message.key == "snbt.parser.expected_unquoted_string"
    ));
}

#[test]
fn selector_reader_leaves_following_command_input_unconsumed() {
    let mut reader = StringReader::new("@e[nbt={Tags:[\"foo]bar\"],data:{x:1b}}] next");
    let Ok(raw) = read_selector_argument(&mut reader) else {
        panic!("selector should be read");
    };

    assert_eq!(raw, "@e[nbt={Tags:[\"foo]bar\"],data:{x:1b}}]");
    assert_eq!(reader.remaining(), " next");
}

#[test]
fn selector_reports_missing_predicate_and_advancement_foundations() {
    let source = TestSource::new(true, true);
    let predicate = parse("@e[predicate=test]", &source, false, false);
    assert!(matches!(
        predicate,
        Err(error) if error.raw_message().contains("predicate needs")
    ));
    let advancements = parse("@e[advancements={}]", &source, false, false);
    assert!(matches!(
        advancements,
        Err(error) if error.raw_message().contains("advancements needs")
    ));
}

#[test]
fn selector_entity_filters_use_command_identity_and_nbt_snapshot() {
    init_test_registry();
    let entity = SelectorTestEntity::new();
    entity.set_custom_name(Some(TextComponent::plain("Named item")));
    let mut custom_data = NbtCompound::new();
    custom_data.insert("flag", NbtTag::Byte(1));
    entity.set_custom_data(custom_data);

    let mut expected_data = NbtCompound::new();
    expected_data.insert("flag", NbtTag::Byte(1));
    let mut expected = NbtCompound::new();
    expected.insert("data", NbtTag::Compound(expected_data));

    assert!(entity_name_filter_matches("Named item", false, &entity));
    assert!(entity_nbt_filter_matches(&expected, false, &entity));
    assert!(!entity_nbt_filter_matches(&expected, true, &entity));
    assert!(!game_mode_filter_matches(GameType::Creative, true, &entity));
}

#[test]
fn selector_score_and_team_filters_use_one_domain_scoreboard() {
    let scoreboard = Scoreboard::new();
    let Ok(kills) = scoreboard.add_objective("kills") else {
        panic!("objective should be added");
    };
    let Ok(red) = scoreboard.add_team("red") else {
        panic!("team should be added");
    };
    let holder = ScoreHolder::new("Steve");
    assert!(scoreboard.set_score(&holder, &kills, 5).is_ok());
    assert!(scoreboard.add_holder_to_team(&holder, &red).is_ok());

    assert!(score_filter_matches(
        &[("kills".to_owned(), IntRange::exactly(5))],
        holder.name(),
        &scoreboard
    ));
    assert!(team_filter_matches(
        "red",
        false,
        holder.name(),
        &scoreboard
    ));
    assert!(!team_filter_matches("", false, holder.name(), &scoreboard));
}

#[test]
fn selector_suggestions_include_supported_options_and_source_values() {
    init_test_registry();
    let source = TestSource::new(true, true);

    let Ok(mut root) = SuggestionsBuilder::new("s", 0) else {
        panic!("suggestion builder should be valid");
    };
    suggest_entity_selector(&mut root, &source, false, false);
    let Ok(root) = root.build() else {
        panic!("root suggestions should build");
    };
    assert!(
        root.list()
            .iter()
            .any(|suggestion| suggestion.text() == "Steve")
    );

    let Ok(mut single_player) = SuggestionsBuilder::new("@", 0) else {
        panic!("suggestion builder should be valid");
    };
    suggest_entity_selector(&mut single_player, &source, true, true);
    let Ok(single_player) = single_player.build() else {
        panic!("single-player selector suggestions should build");
    };
    let roots = single_player
        .list()
        .iter()
        .map(Suggestion::text)
        .collect::<Vec<_>>();
    assert_eq!(roots.len(), 6);
    for selector in ["@a", "@e", "@n", "@p", "@r", "@s"] {
        assert!(roots.contains(&selector));
    }

    let Ok(mut options) = SuggestionsBuilder::new("@e[", 0) else {
        panic!("suggestion builder should be valid");
    };
    suggest_entity_selector(&mut options, &source, false, false);
    let Ok(options) = options.build() else {
        panic!("option suggestions should build");
    };
    assert!(
        options
            .list()
            .iter()
            .any(|suggestion| suggestion.text() == "@e[nbt=")
    );
    assert!(
        !options
            .list()
            .iter()
            .any(|suggestion| suggestion.text() == "@e[predicate=")
    );

    let Ok(mut team) = SuggestionsBuilder::new("@e[team=R", 0) else {
        panic!("suggestion builder should be valid");
    };
    suggest_entity_selector(&mut team, &source, false, false);
    let Ok(team) = team.build() else {
        panic!("team suggestions should build");
    };
    assert!(
        team.list()
            .iter()
            .any(|suggestion| suggestion.text() == "@e[team=red")
    );
}
