//! Verifies the command extension surface from outside the `steel-core` crate.

use steel_core::command::{
    CommandArgument, CommandArgumentParser, CommandError, CommandParserSource, CommandReader,
    CommandRegistration, CommandRegistry, CommandSuggestionContext, CommandSuggestions,
    SuspendedCommand, SuspendedCommandPoll, argument, literal,
};
use steel_protocol::packets::game::{
    ArgumentType as ProtocolArgumentType, SuggestionType as ProtocolSuggestionType,
};
use steel_utils::{DowncastType, DowncastTypeKey, Identifier};
use text_components::TextComponent;

#[derive(Debug, PartialEq, Eq)]
struct NegatedBooleanParser;

// SAFETY: This test-owned key uniquely identifies the concrete parser implementation.
unsafe impl DowncastType for NegatedBooleanParser {
    const TYPE_KEY: DowncastTypeKey =
        DowncastTypeKey::new("steel_test:command/parser/negated_bool");
}

#[derive(Debug)]
struct NegatedBoolean(bool);

// SAFETY: This test-owned key uniquely identifies the concrete parsed value.
unsafe impl DowncastType for NegatedBoolean {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel_test:command/value/negated_bool");
}

impl CommandArgumentParser for NegatedBooleanParser {
    type Value = NegatedBoolean;

    fn parse(
        &self,
        reader: &mut CommandReader<'_, '_>,
        _source: CommandParserSource<'_>,
    ) -> Result<Self::Value, CommandError> {
        reader.read_boolean().map(|value| NegatedBoolean(!value))
    }

    fn list_suggestions(
        &self,
        _context: CommandSuggestionContext<'_>,
        suggestions: &mut CommandSuggestions<'_, '_>,
    ) {
        for value in ["true", "false"] {
            if value.starts_with(suggestions.remaining()) {
                suggestions.suggest(value);
            }
        }
    }

    fn protocol_argument(&self) -> (ProtocolArgumentType, Option<ProtocolSuggestionType>) {
        (ProtocolArgumentType::Bool, None)
    }
}

struct ReadySuspension;

impl SuspendedCommand for ReadySuspension {
    fn poll(&mut self) -> SuspendedCommandPoll {
        SuspendedCommandPoll::Ready(Ok(1))
    }
}

#[test]
fn downstream_commands_can_register_primitive_keyed_and_suspended_nodes() {
    fn assert_send<T: Send>() {}
    assert_send::<CommandRegistry>();

    let command = CommandRegistration::new(Identifier::new("steel_test", "extension"), || {
        literal("extension")
            .then(
                argument("value", CommandArgument::custom(NegatedBooleanParser)).executes(
                    |context| {
                        let Some(value) = context.value::<NegatedBoolean>("value") else {
                            return Err(CommandError::from("missing parsed negated boolean"));
                        };
                        context.source().send_success(
                            &TextComponent::plain(format!("negated value: {}", value.0)),
                            false,
                        );
                        Ok(i32::from(value.0))
                    },
                ),
            )
            .then(literal("wait").executes_suspended(|_| Ok(ReadySuspension)))
    })
    .alias("extension_alias")
    .subcommand_permission(["wait"]);

    let mut registry = CommandRegistry::new();
    assert!(registry.declare_permission("steel_test.extra").is_ok());
    assert!(registry.register(command).is_ok());
}
