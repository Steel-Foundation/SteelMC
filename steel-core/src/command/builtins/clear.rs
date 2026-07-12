//! Vanilla player inventory clearing command.

use std::{slice, sync::Arc};

use steel_registry::item_stack::ItemStack;
use steel_utils::{Identifier, translations};
use text_components::TextComponent;

use super::super::{
    brigadier::{ArgumentType, CommandNodeBuilder, CommandSyntaxError},
    execution::{
        CommandSource, SteelArgumentType, SteelCommandContext, SteelCommandRuntime, argument,
        literal,
    },
    registration::CommandRegistration,
};
use crate::{entity::Entity as _, player::Player};

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("clear"), |_| command())
}

fn command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("clear").executes(clear_self).then(
        argument("targets", SteelArgumentType::players())
            .executes(clear_targets)
            .then(
                argument("item", SteelArgumentType::item_predicate())
                    .executes(clear_matching)
                    .then(
                        argument("maxCount", ArgumentType::integer(0, i32::MAX))
                            .executes(clear_matching_with_limit),
                    ),
            ),
    )
}

fn clear_self(context: &SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError> {
    let Some(player) = context.source().player() else {
        return Err(CommandSyntaxError::dynamic(TextComponent::from(
            &translations::PERMISSIONS_REQUIRES_PLAYER,
        )));
    };
    clear_players(context, slice::from_ref(player), &matches_any_item, -1)
}

fn clear_targets(context: &SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError> {
    let targets = context.players("targets")?;
    clear_players(context, &targets, &matches_any_item, -1)
}

fn clear_matching(context: &SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError> {
    clear_matching_with_count(context, -1)
}

fn clear_matching_with_limit(
    context: &SteelCommandContext<CommandSource>,
) -> Result<i32, CommandSyntaxError> {
    let Some(max_count) = context.integer("maxCount") else {
        return Err(missing_argument("maxCount"));
    };
    clear_matching_with_count(context, max_count)
}

fn clear_matching_with_count(
    context: &SteelCommandContext<CommandSource>,
    max_count: i32,
) -> Result<i32, CommandSyntaxError> {
    let targets = context.players("targets")?;
    let Some(predicate) = context.item_predicate("item") else {
        return Err(missing_argument("item"));
    };
    clear_players(
        context,
        &targets,
        &|stack| predicate.matches(stack),
        max_count,
    )
}

fn clear_players(
    context: &SteelCommandContext<CommandSource>,
    targets: &[Arc<Player>],
    predicate: &dyn Fn(&ItemStack) -> bool,
    max_count: i32,
) -> Result<i32, CommandSyntaxError> {
    let mut count = 0;
    for target in targets {
        count += target.clear_or_count_matching_items(predicate, max_count);
    }

    if count == 0 {
        let message = if let [target] = targets {
            translations::CLEAR_FAILED_SINGLE
                .message([TextComponent::plain(target.plain_text_name())])
                .component()
        } else {
            translations::CLEAR_FAILED_MULTIPLE
                .message([TextComponent::plain(targets.len().to_string())])
                .component()
        };
        return Err(CommandSyntaxError::dynamic(message));
    }

    let count_component = TextComponent::plain(count.to_string());
    let message = if max_count == 0 {
        if let [target] = targets {
            translations::COMMANDS_CLEAR_TEST_SINGLE
                .message([
                    count_component,
                    TextComponent::plain(target.plain_text_name()),
                ])
                .component()
        } else {
            translations::COMMANDS_CLEAR_TEST_MULTIPLE
                .message([
                    count_component,
                    TextComponent::plain(targets.len().to_string()),
                ])
                .component()
        }
    } else if let [target] = targets {
        translations::COMMANDS_CLEAR_SUCCESS_SINGLE
            .message([
                count_component,
                TextComponent::plain(target.plain_text_name()),
            ])
            .component()
    } else {
        translations::COMMANDS_CLEAR_SUCCESS_MULTIPLE
            .message([
                count_component,
                TextComponent::plain(targets.len().to_string()),
            ])
            .component()
    };
    context.source().send_success(&message, true);
    Ok(count)
}

const fn matches_any_item(_stack: &ItemStack) -> bool {
    true
}

fn missing_argument(name: &str) -> CommandSyntaxError {
    CommandSyntaxError::dynamic(format!(
        "Parsed value for {name} is missing from the command context"
    ))
}

#[cfg(test)]
mod tests {
    use steel_registry::test_support::init_test_registry;

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
        init_test_registry();
        let Ok(dispatcher) = create_dispatcher() else {
            panic!("built-in commands should register");
        };
        let clear = child(&dispatcher, dispatcher.root(), "clear");
        let Some(clear_node) = dispatcher.node(clear) else {
            panic!("clear node should exist");
        };
        assert!(clear_node.is_restricted());
        assert!(clear_node.is_executable());

        let targets = child(&dispatcher, clear, "targets");
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

        let item = child(&dispatcher, targets, "item");
        assert_eq!(
            dispatcher.node(item).and_then(|node| node.argument_type()),
            Some(&SteelArgumentType::item_predicate())
        );
        assert!(matches!(
            dispatcher.node(item),
            Some(node) if node.is_executable()
        ));

        let max_count = child(&dispatcher, item, "maxCount");
        assert_eq!(
            dispatcher
                .node(max_count)
                .and_then(|node| node.argument_type()),
            Some(&SteelArgumentType::from(ArgumentType::integer(0, i32::MAX)))
        );
        assert!(matches!(
            dispatcher.node(max_count),
            Some(node) if node.is_executable()
        ));
    }
}
