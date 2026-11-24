//! Vanilla command to give an effect to an entity.

use super::super::{
    brigadier::{ArgumentType, CommandNodeBuilder},
    execution::{CommandSource, SteelArgumentType, SteelCommandRuntime, literal},
    registration::CommandRegistration,
};
use crate::command::brigadier::CommandSyntaxError;
use crate::command::execution::{SteelCommandContext, argument};
use crate::entity::SharedEntity;
use std::slice;
use steel_registry::mob_effect::MobEffectRef;
use steel_utils::{Identifier, translations};
use text_components::TextComponent;

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("effect"), |_| command())
}

fn command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("effect")
        .then(
            literal("clear").executes(clear_self).then(
                argument("targets", SteelArgumentType::entities())
                    .executes(clear_multiple)
                    .then(
                        argument("effect", SteelArgumentType::mob_effect())
                            .executes(clear_multiple_with_effect),
                    ),
            ),
        )
        .then(
            literal("give").then(
                argument("targets", SteelArgumentType::entities()).then(
                    argument("effect", SteelArgumentType::mob_effect())
                        .then(
                            argument("seconds", ArgumentType::integer(1, 1000000)).then(
                                argument("amplifier", ArgumentType::integer(0, 255))
                                    .then(argument("hideParticles", ArgumentType::bool())),
                            ),
                        )
                        .then(
                            literal("infinite").then(
                                argument("amplifier", ArgumentType::integer(0, 255))
                                    .then(argument("hideParticles", ArgumentType::bool())),
                            ),
                        ),
                ),
            ),
        )
}

fn clear_self(ctx: &SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError> {
    let Some(entity) = ctx.source().entity() else {
        return Err(CommandSyntaxError::dynamic(TextComponent::from(
            &translations::PERMISSIONS_REQUIRES_ENTITY,
        )));
    };
    clear(ctx, slice::from_ref(entity), None)
}

fn clear_multiple(ctx: &SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError> {
    let entities = ctx.entities("targets")?;
    clear(ctx, entities.as_slice(), None)
}

fn clear_multiple_with_effect(
    ctx: &SteelCommandContext<CommandSource>,
) -> Result<i32, CommandSyntaxError> {
    let entities = ctx.entities("targets")?;
    let effect = ctx.mob_effect("effect")?;
    clear(ctx, entities.as_slice(), Some(effect))
}
fn clear(
    ctx: &SteelCommandContext<CommandSource>,
    entities: &[SharedEntity],
    effect: Option<MobEffectRef>,
) -> Result<i32, CommandSyntaxError> {
    let count = entities
        .iter()
        .filter_map(|entity| entity.as_living_entity())
        .filter(|living| match effect {
            Some(eff) => living.remove_mob_effect(eff),
            None => living.remove_all_mob_effect(),
        })
        .count() as i32;

    if count == 0 {
        let msg = if effect.is_some() {
            &translations::COMMANDS_EFFECT_CLEAR_SPECIFIC_FAILED
        } else {
            &translations::COMMANDS_EFFECT_CLEAR_EVERYTHING_FAILED
        };
        return Err(CommandSyntaxError::dynamic(TextComponent::from(msg)));
    }

    let message = match (entities, effect) {
        ([single], Some(eff)) => translations::COMMANDS_EFFECT_CLEAR_SPECIFIC_SUCCESS_SINGLE
            .message([
                TextComponent::plain(eff.key.to_string()),
                single.display_name(),
            ])
            .component(),
        ([single], None) => translations::COMMANDS_EFFECT_CLEAR_EVERYTHING_SUCCESS_SINGLE
            .message([single.display_name()])
            .component(),
        (_, Some(eff)) => translations::COMMANDS_EFFECT_CLEAR_SPECIFIC_SUCCESS_MULTIPLE
            .message([
                TextComponent::plain(eff.key.to_string()),
                TextComponent::plain(count.to_string()),
            ])
            .component(),
        (_, None) => translations::COMMANDS_EFFECT_CLEAR_EVERYTHING_SUCCESS_MULTIPLE
            .message([TextComponent::plain(count.to_string())])
            .component(),
    };

    ctx.source().send_success(&message, true);

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
    fn give_graph_matches_vanilla_argument_shape() {
        init_vanilla_registry();
        let Ok(dispatcher) = create_dispatcher() else {
            panic!("built-in commands should register");
        };
        let give = child(&dispatcher, dispatcher.root(), "effect");
        let Some(give_node) = dispatcher.node(give) else {
            panic!("give node should exist");
        };
        assert!(give_node.is_restricted());

        effect_clear_node(&dispatcher, give);

        let targets = child(&dispatcher, give, "targets");
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

    fn effect_clear_node(
        dispatcher: &CommandDispatcher<CommandSource, SteelCommandRuntime>,
        give_node: NodeId,
    ) {
        let clear = child(&dispatcher, give_node, "clear");

        let targets = child(&dispatcher, clear, "targets");
        assert_eq!(
            dispatcher
                .node(targets)
                .and_then(|node| node.argument_type()),
            Some(&SteelArgumentType::from(SteelArgumentType::entities()))
        );
        assert!(matches!(
            dispatcher.node(targets),
            Some(node) if node.is_executable()
        ));

        let effect = child(&dispatcher, targets, "effect");
        assert_eq!(
            dispatcher
                .node(effect)
                .and_then(|node| node.argument_type()),
            Some(&SteelArgumentType::from(SteelArgumentType::mob_effect()))
        );
        assert!(matches!(
            dispatcher.node(effect),
            Some(node) if node.is_executable()
        ));
    }

    fn effect_give_node(
        dispatcher: &CommandDispatcher<CommandSource, SteelCommandRuntime>,
        give_node: NodeId,
    ) {
        let give = child(&dispatcher, give_node, "give");

        let targets = child(&dispatcher, give, "targets");
        assert_eq!(
            dispatcher
                .node(targets)
                .and_then(|node| node.argument_type()),
            Some(&SteelArgumentType::from(SteelArgumentType::entities()))
        );
        assert!(matches!(
            dispatcher.node(targets),
            Some(node) if node.is_executable()
        ));

        let effect = child(&dispatcher, targets, "effect");
        assert_eq!(
            dispatcher
                .node(effect)
                .and_then(|node| node.argument_type()),
            Some(&SteelArgumentType::from(SteelArgumentType::mob_effect()))
        );
        assert!(matches!(
            dispatcher.node(effect),
            Some(node) if node.is_executable()
        ));

        let infinite = child(&dispatcher, effect, "infinite");
        assert!(matches!(
            dispatcher.node(infinite),
            Some(node) if node.is_executable()
        ));
        effect_give_infinite_or_second_node(dispatcher, infinite);

        let seconds = child(&dispatcher, effect, "seconds");
        assert_eq!(
            dispatcher
                .node(seconds)
                .and_then(|node| node.argument_type()),
            Some(&SteelArgumentType::from(ArgumentType::integer(1, 1000000)))
        );
        assert!(matches!(
            dispatcher.node(effect),
            Some(node) if node.is_executable()
        ));
        effect_give_infinite_or_second_node(dispatcher, seconds);
    }

    fn effect_give_infinite_or_second_node(
        dispatcher: &CommandDispatcher<CommandSource, SteelCommandRuntime>,
        parent_node: NodeId,
    ) {
        let amplifier = child(&dispatcher, parent_node, "amplifier");
        assert_eq!(
            dispatcher
                .node(amplifier)
                .and_then(|node| node.argument_type()),
            Some(&SteelArgumentType::from(ArgumentType::integer(0, 255)))
        );
        assert!(matches!(
            dispatcher.node(amplifier),
            Some(node) if node.is_executable()
        ));

        let hide_particles = child(&dispatcher, amplifier, "hideParticles");
        assert_eq!(
            dispatcher
                .node(hide_particles)
                .and_then(|node| node.argument_type()),
            Some(&SteelArgumentType::from(ArgumentType::bool()))
        );
        assert!(matches!(
            dispatcher.node(hide_particles),
            Some(node) if node.is_executable()
        ));
    }
}
