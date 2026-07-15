//! Entity killing command.

use std::slice;

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
use crate::entity::SharedEntity;

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("kill"), |_| command())
}

fn command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("kill")
        .executes(kill_self)
        .then(argument("targets", SteelArgumentType::entities()).executes(kill_targets))
}

fn kill_self(context: &SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError> {
    let Some(entity) = context.source().entity() else {
        return Err(CommandSyntaxError::dynamic(TextComponent::from(
            &translations::PERMISSIONS_REQUIRES_ENTITY,
        )));
    };
    kill_entities(context, slice::from_ref(entity))
}

fn kill_targets(context: &SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError> {
    let targets = context.entities("targets")?;
    kill_entities(context, &targets)
}

fn kill_entities(
    context: &SteelCommandContext<CommandSource>,
    targets: &[SharedEntity],
) -> Result<i32, CommandSyntaxError> {
    let Ok(result) = i32::try_from(targets.len()) else {
        return Err(CommandSyntaxError::dynamic(
            "Target count exceeds the command result range",
        ));
    };
    for target in targets {
        target.kill(context.source().world());
    }

    let message = if let [target] = targets {
        translations::COMMANDS_KILL_SUCCESS_SINGLE
            .message([TextComponent::plain(target.plain_text_name())])
            .component()
    } else {
        translations::COMMANDS_KILL_SUCCESS_MULTIPLE
            .message([TextComponent::plain(targets.len().to_string())])
            .component()
    };
    context.source().send_success(&message, true);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use steel_registry::test_support::init_test_registry;

    use super::super::create_dispatcher;
    use crate::command::execution::SteelArgumentType;

    #[test]
    fn kill_graph_supports_self_and_multiple_entity_targets() {
        init_test_registry();
        let Ok(dispatcher) = create_dispatcher() else {
            panic!("built-in commands should register");
        };
        let Some(kill) = dispatcher.children(dispatcher.root()).and_then(|children| {
            children.iter().copied().find(|child| {
                dispatcher
                    .node(*child)
                    .is_some_and(|node| node.name() == "kill")
            })
        }) else {
            panic!("kill root should exist");
        };
        let Some(kill_node) = dispatcher.node(kill) else {
            panic!("kill root node should exist");
        };
        assert!(kill_node.is_executable());

        let Some(targets) = dispatcher
            .children(kill)
            .and_then(|children| children.first())
        else {
            panic!("kill targets should exist");
        };
        assert!(matches!(
            dispatcher.node(*targets),
            Some(node)
                if node.is_executable()
                    && node.argument_type() == Some(&SteelArgumentType::entities())
        ));
    }
}
