//! Vanilla kick command.

use super::super::{
    brigadier::{CommandNodeBuilder, CommandSyntaxError},
    execution::{
        CommandSource, SteelArgumentType, SteelCommandContext, SteelCommandRuntime, argument,
        literal,
    },
    registration::CommandRegistration,
};
use crate::entity::EntityMethods;
use steel_utils::{Identifier, translations};
use text_components::TextComponent;
use text_components::content::Content;

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("kick"), |_| command())
}

fn command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("kick").then(
        argument("targets", SteelArgumentType::players())
            .executes(kick_players)
            .then(argument("reason", SteelArgumentType::message()).executes(kick_players)),
    )
}

fn kick_players(ctx: &SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError> {
    let mut count: i32 = 0;

    let targets = ctx.players("targets")?;
    let message = match ctx.message("reason") {
        Ok(message) => {
            let parsed = message.resolve_component(ctx.source())?;

            if let Content::Text { text } = &parsed.content {
                TextComponent::from_snbt(text).unwrap_or(parsed)
            } else {
                parsed
            }
        }
        Err(_) => translations::MULTIPLAYER_DISCONNECT_KICKED
            .msg()
            .component(),
    };

    for players in targets {
        players.disconnect(message.clone());
        ctx.source().send_success(
            &translations::COMMANDS_KICK_SUCCESS
                .message([players.display_name(), message.clone()])
                .component(),
            true,
        );
        count += 1;
    }

    Ok(count)
}

#[cfg(test)]
mod tests {
    use steel_registry::init_vanilla_registry;

    use super::super::create_dispatcher;
    use super::*;
    use crate::command::brigadier::{CommandDispatcher, NodeId};

    type Dispatcher = CommandDispatcher<CommandSource, SteelCommandRuntime>;

    fn child(dispatcher: &Dispatcher, parent: NodeId, name: &str) -> NodeId {
        let Some(children) = dispatcher.children(parent) else {
            panic!("parent node should exist");
        };
        let Some(child) = children.iter().copied().find(|child| {
            dispatcher
                .node(*child)
                .is_some_and(|node| node.name() == name)
        }) else {
            panic!("child {name} should exist");
        };
        child
    }

    #[test]
    fn clear_graph_matches_vanilla_argument_shape() {
        init_vanilla_registry();
        let Ok(dispatcher) = create_dispatcher() else {
            panic!("built-in commands should register");
        };
        let kick = child(&dispatcher, dispatcher.root(), "kick");
        let Some(clear_node) = dispatcher.node(kick) else {
            panic!("kick node should exist");
        };
        assert!(clear_node.is_restricted());

        let targets = child(&dispatcher, kick, "targets");
        assert_eq!(
            dispatcher
                .node(targets)
                .and_then(|node| node.argument_type()),
            Some(&SteelArgumentType::players())
        );
        assert!(matches!(
            dispatcher.node(targets),
            Some(node) if node.is_executable()
        ));

        let reason = child(&dispatcher, targets, "reason");
        assert_eq!(
            dispatcher
                .node(reason)
                .and_then(|node| node.argument_type()),
            Some(&SteelArgumentType::message())
        );
        assert!(matches!(
            dispatcher.node(reason),
            Some(node) if node.is_executable()
        ));
    }
}
