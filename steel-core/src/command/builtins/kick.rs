//! Vanilla player-kicking command.

use steel_utils::{Identifier, translations};
use text_components::TextComponent;

use super::super::{
    brigadier::{CommandNodeBuilder, CommandSyntaxError},
    execution::{
        CommandSource, SteelArgumentType, SteelCommandContext, SteelCommandRuntime, argument,
        literal,
    },
    registration::CommandRegistration,
};
use crate::entity::Entity as _;

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("kick"), |_| command())
}

fn command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("kick").then(
        argument("targets", SteelArgumentType::players())
            .executes(kick_default_reason)
            .then(argument("reason", SteelArgumentType::message()).executes(kick_with_reason)),
    )
}

fn kick_default_reason(
    context: &SteelCommandContext<CommandSource>,
) -> Result<i32, CommandSyntaxError> {
    kick_players(
        context,
        TextComponent::from(&translations::MULTIPLAYER_DISCONNECT_KICKED),
    )
}

fn kick_with_reason(
    context: &SteelCommandContext<CommandSource>,
) -> Result<i32, CommandSyntaxError> {
    let reason = context.message("reason")?;
    kick_players(context, TextComponent::plain(reason.to_owned()))
}

fn kick_players(
    context: &SteelCommandContext<CommandSource>,
    reason: TextComponent,
) -> Result<i32, CommandSyntaxError> {
    let targets = context.players("targets")?;
    let Ok(count) = i32::try_from(targets.len()) else {
        return Err(CommandSyntaxError::dynamic(
            "Target player count exceeds the command result range",
        ));
    };

    for target in &targets {
        target.disconnect(reason.clone());
        let message = translations::COMMANDS_KICK_SUCCESS
            .message([
                TextComponent::plain(target.plain_text_name()),
                reason.clone(),
            ])
            .component();
        context.source().send_success(&message, true);
    }

    Ok(count)
}

#[cfg(test)]
mod tests {
    use steel_registry::init_vanilla_registry;

    use super::super::create_dispatcher;
    use crate::command::execution::SteelArgumentType;

    #[test]
    fn kick_graph_requires_targets_then_optionally_accepts_a_reason() {
        init_vanilla_registry();
        let Ok(dispatcher) = create_dispatcher() else {
            panic!("built-in commands should register");
        };
        let Some(kick) = dispatcher.children(dispatcher.root()).and_then(|children| {
            children.iter().copied().find(|child| {
                dispatcher
                    .node(*child)
                    .is_some_and(|node| node.name() == "kick")
            })
        }) else {
            panic!("kick root should exist");
        };
        let Some(kick_node) = dispatcher.node(kick) else {
            panic!("kick root node should exist");
        };
        assert!(!kick_node.is_executable());

        let Some(targets) = dispatcher
            .children(kick)
            .and_then(|children| children.first())
        else {
            panic!("kick targets should exist");
        };
        assert!(matches!(
            dispatcher.node(*targets),
            Some(node)
                if node.is_executable()
                    && node.argument_type() == Some(&SteelArgumentType::players())
        ));

        let Some(reason) = dispatcher
            .children(*targets)
            .and_then(|children| children.first())
        else {
            panic!("kick reason should exist");
        };
        assert!(matches!(
            dispatcher.node(*reason),
            Some(node)
                if node.is_executable()
                    && node.argument_type() == Some(&SteelArgumentType::message())
        ));
    }
}
