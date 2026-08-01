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
        .executes(|context| kill(context, KillTargets::Source))
        .then(
            argument("targets", SteelArgumentType::entities())
                .executes(|context| kill(context, KillTargets::Argument)),
        )
}

#[derive(Clone, Copy)]
enum KillTargets {
    Source,
    Argument,
}

enum ResolvedKillTargets<'context> {
    Source(&'context SharedEntity),
    Argument(Vec<SharedEntity>),
}

impl ResolvedKillTargets<'_> {
    fn as_slice(&self) -> &[SharedEntity] {
        match self {
            Self::Source(entity) => slice::from_ref(entity),
            Self::Argument(entities) => entities,
        }
    }
}

impl KillTargets {
    fn resolve(
        self,
        context: &SteelCommandContext<CommandSource>,
    ) -> Result<ResolvedKillTargets<'_>, CommandSyntaxError> {
        match self {
            Self::Source => {
                let Some(entity) = context.source().entity() else {
                    return Err(CommandSyntaxError::dynamic(TextComponent::from(
                        &translations::PERMISSIONS_REQUIRES_ENTITY,
                    )));
                };
                Ok(ResolvedKillTargets::Source(entity))
            }
            Self::Argument => Ok(ResolvedKillTargets::Argument(context.entities("targets")?)),
        }
    }
}

fn kill(
    context: &SteelCommandContext<CommandSource>,
    target_selection: KillTargets,
) -> Result<i32, CommandSyntaxError> {
    let targets = target_selection.resolve(context)?;
    let targets = targets.as_slice();
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
