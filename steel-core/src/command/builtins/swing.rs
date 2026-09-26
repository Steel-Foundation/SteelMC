//! Vanilla arm-swing animation command.

use std::slice;

use steel_utils::{Identifier, translations, types::InteractionHand};
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
    CommandRegistration::new(Identifier::vanilla_static("swing"), |_| command())
}

fn command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("swing").executes(swing_self).then(
        argument("targets", SteelArgumentType::entities())
            .executes(swing_targets_main_hand)
            .then(literal("mainhand").executes(swing_targets_main_hand))
            .then(literal("offhand").executes(swing_targets_off_hand)),
    )
}

fn swing_self(context: &SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError> {
    let Some(entity) = context.source().entity() else {
        return Err(CommandSyntaxError::dynamic(TextComponent::from(
            &translations::PERMISSIONS_REQUIRES_ENTITY,
        )));
    };
    swing_entities(context, slice::from_ref(entity), InteractionHand::MainHand)
}

fn swing_targets_main_hand(
    context: &SteelCommandContext<CommandSource>,
) -> Result<i32, CommandSyntaxError> {
    let targets = context.entities("targets")?;
    swing_entities(context, &targets, InteractionHand::MainHand)
}

fn swing_targets_off_hand(
    context: &SteelCommandContext<CommandSource>,
) -> Result<i32, CommandSyntaxError> {
    let targets = context.entities("targets")?;
    swing_entities(context, &targets, InteractionHand::OffHand)
}

fn swing_entities(
    context: &SteelCommandContext<CommandSource>,
    targets: &[SharedEntity],
    hand: InteractionHand,
) -> Result<i32, CommandSyntaxError> {
    let mut living_targets = Vec::with_capacity(targets.len());
    for target in targets {
        if let Some(living) = target.as_living_entity() {
            living.swing(hand, true);
            living_targets.push(target);
        }
    }

    if living_targets.is_empty() {
        return Err(CommandSyntaxError::dynamic(TextComponent::from(
            &translations::COMMANDS_SWING_FAILED_NOTLIVING,
        )));
    }

    let message = if let [target] = living_targets.as_slice() {
        translations::COMMANDS_SWING_SUCCESS_SINGLE
            .message([target.display_name()])
            .component()
    } else {
        translations::COMMANDS_SWING_SUCCESS_MULTIPLE
            .message([living_targets.len().to_string()])
            .component()
    };
    context.source().send_success(&message, true);

    i32::try_from(living_targets.len())
        .map_err(|_| CommandSyntaxError::dynamic("Target count exceeds the command result range"))
}

#[cfg(test)]
mod tests {
    use steel_registry::init_vanilla_registry;

    use super::super::create_dispatcher;
    use crate::command::execution::SteelArgumentType;

    #[test]
    fn swing_graph_supports_self_targets_and_explicit_hands() {
        init_vanilla_registry();
        let Ok(dispatcher) = create_dispatcher() else {
            panic!("built-in commands should register");
        };
        let Some(swing) = dispatcher.children(dispatcher.root()).and_then(|children| {
            children.iter().copied().find(|child| {
                dispatcher
                    .node(*child)
                    .is_some_and(|node| node.name() == "swing")
            })
        }) else {
            panic!("swing root should exist");
        };
        let Some(swing_node) = dispatcher.node(swing) else {
            panic!("swing root node should exist");
        };
        assert!(swing_node.is_executable());

        let Some(targets) = dispatcher
            .children(swing)
            .and_then(|children| children.first())
            .copied()
        else {
            panic!("swing targets should exist");
        };
        assert!(matches!(
            dispatcher.node(targets),
            Some(node)
                if node.is_executable()
                    && node.argument_type() == Some(&SteelArgumentType::entities())
        ));

        let Some(hand_children) = dispatcher.children(targets) else {
            panic!("swing hand literals should exist");
        };
        let hand_names = hand_children
            .iter()
            .map(|child| {
                let Some(node) = dispatcher.node(*child) else {
                    panic!("swing hand literal should exist");
                };
                assert!(node.is_executable());
                node.name()
            })
            .collect::<Vec<_>>();
        assert_eq!(hand_names, ["mainhand", "offhand"]);
    }
}
