//! Handler for the "gamerule" command.
use std::borrow::Cow;
use std::sync::Arc;

use steel_registry::REGISTRY;
use steel_registry::game_rules::{GameRuleRef, GameRuleType, GameRuleValue};
use steel_utils::text::TextComponent;
use steel_utils::translations;

use crate::command::arguments::bool::BoolArgument;
use crate::command::arguments::integer::IntegerArgument;
use crate::command::commands::{
    CommandExecutor, CommandHandlerDyn, DynCommandHandler, argument, literal,
};
use crate::command::context::CommandContext;
use crate::command::error::CommandError;
use crate::server::Server;

/// Returns the handler for the "gamerule" command.
#[must_use]
pub fn command_handler() -> impl CommandHandlerDyn {
    let mut handler = DynCommandHandler::new(
        &["gamerule"],
        "Gets or sets a game rule value.",
        "minecraft:command.gamerule",
    );

    for (_, rule) in REGISTRY.game_rules.iter() {
        let Cow::Borrowed(rule_name) = &rule.key.path else {
            unreachable!("registry identifiers are always static")
        };

        match rule.value_type {
            GameRuleType::Bool => {
                handler = handler.then(
                    literal(rule_name)
                        .executes(QueryExecutor(rule))
                        .then(argument("value", BoolArgument).executes(SetBoolExecutor(rule))),
                );
            }
            GameRuleType::Int => {
                handler = handler.then(
                    literal(rule_name).executes(QueryExecutor(rule)).then(
                        argument(
                            "value",
                            IntegerArgument::bounded(rule.min_value, rule.max_value),
                        )
                        .executes(SetIntExecutor(rule)),
                    ),
                );
            }
        }
    }

    handler
}

struct QueryExecutor(GameRuleRef);

impl CommandExecutor<()> for QueryExecutor {
    fn execute(
        &self,
        _args: (),
        context: &mut CommandContext,
        _server: &Arc<Server>,
    ) -> Result<(), CommandError> {
        let world = context.get_world()?;
        let rule_name = self.0.key.path.to_string();
        let value = world.get_game_rule(self.0);

        context.sender.send_message(
            translations::COMMANDS_GAMERULE_QUERY
                .message([
                    TextComponent::from(rule_name),
                    TextComponent::from(value.to_string()),
                ])
                .into(),
        );

        Ok(())
    }
}

struct SetBoolExecutor(GameRuleRef);

impl CommandExecutor<((), bool)> for SetBoolExecutor {
    fn execute(
        &self,
        args: ((), bool),
        context: &mut CommandContext,
        _server: &Arc<Server>,
    ) -> Result<(), CommandError> {
        let ((), value) = args;
        let world = context.get_world()?;
        let rule_name = self.0.key.path.to_string();

        world.set_game_rule(self.0, GameRuleValue::Bool(value));

        context.sender.send_message(
            translations::COMMANDS_GAMERULE_SET
                .message([
                    TextComponent::from(rule_name),
                    TextComponent::from(value.to_string()),
                ])
                .into(),
        );

        Ok(())
    }
}

struct SetIntExecutor(GameRuleRef);

impl CommandExecutor<((), i32)> for SetIntExecutor {
    fn execute(
        &self,
        args: ((), i32),
        context: &mut CommandContext,
        _server: &Arc<Server>,
    ) -> Result<(), CommandError> {
        let ((), value) = args;
        let world = context.get_world()?;
        let rule_name = self.0.key.path.to_string();

        world.set_game_rule(self.0, GameRuleValue::Int(value));

        context.sender.send_message(
            translations::COMMANDS_GAMERULE_SET
                .message([
                    TextComponent::from(rule_name),
                    TextComponent::from(value.to_string()),
                ])
                .into(),
        );

        Ok(())
    }
}
