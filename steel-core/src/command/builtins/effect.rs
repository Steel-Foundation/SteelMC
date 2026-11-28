//! Vanilla command to give an effect to an entity.

use super::super::{
    brigadier::{ArgumentType, CommandNodeBuilder},
    execution::{CommandSource, SteelArgumentType, SteelCommandRuntime, literal},
    registration::CommandRegistration,
};
use crate::command::brigadier::CommandSyntaxError;
use crate::command::execution::{SteelCommandContext, argument};
use crate::entity::{MobEffectInstance, SharedEntity};
use std::slice;
use steel_registry::mob_effect::MobEffectRef;
use steel_registry::mob_effect_instance::INFINITE_EFFECT_DURATION;
use steel_utils::{Identifier, translations};
use text_components::TextComponent;

const BASE_DURATION: i32 = 600;
const BASE_AMPLIFIER: i32 = 0;
const BASE_HIDE_PARTICLES: bool = true;

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
                        .executes(|ctx| {
                            give(ctx, BASE_DURATION, BASE_AMPLIFIER, BASE_HIDE_PARTICLES)
                        })
                        .then(
                            argument("seconds", ArgumentType::integer(1, 1_000_000))
                                .executes(|ctx| {
                                    give(
                                        ctx,
                                        ctx.integer("seconds")?,
                                        BASE_AMPLIFIER,
                                        BASE_HIDE_PARTICLES,
                                    )
                                })
                                .then(
                                    argument("amplifier", ArgumentType::integer(0, 255))
                                        .executes(|ctx| {
                                            give(
                                                ctx,
                                                ctx.integer("seconds")?,
                                                ctx.integer("amplifier")?,
                                                BASE_HIDE_PARTICLES,
                                            )
                                        })
                                        .then(
                                            argument("hideParticles", ArgumentType::bool())
                                                .executes(|ctx| {
                                                    give(
                                                        ctx,
                                                        ctx.integer("seconds")?,
                                                        ctx.integer("amplifier")?,
                                                        !ctx.boolean("hideParticles")?,
                                                    )
                                                }),
                                        ),
                                ),
                        )
                        .then(
                            literal("infinite")
                                .executes(|ctx| {
                                    give(
                                        ctx,
                                        INFINITE_EFFECT_DURATION,
                                        BASE_AMPLIFIER,
                                        BASE_HIDE_PARTICLES,
                                    )
                                })
                                .then(
                                    argument("amplifier", ArgumentType::integer(0, 255))
                                        .executes(|ctx| {
                                            give(
                                                ctx,
                                                INFINITE_EFFECT_DURATION,
                                                ctx.integer("amplifier")?,
                                                BASE_HIDE_PARTICLES,
                                            )
                                        })
                                        .then(
                                            argument("hideParticles", ArgumentType::bool())
                                                .executes(|ctx| {
                                                    give(
                                                        ctx,
                                                        INFINITE_EFFECT_DURATION,
                                                        ctx.integer("amplifier")?,
                                                        !ctx.boolean("hideParticles")?,
                                                    )
                                                }),
                                        ),
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

fn give(
    ctx: &SteelCommandContext<CommandSource>,
    duration_in_tick: i32,
    amplifier: i32,
    hide_particles: bool,
) -> Result<i32, CommandSyntaxError> {
    let entities = ctx.entities("targets")?;
    let effect = ctx.mob_effect("effect")?;
    let seconds = if duration_in_tick == -1 {
        -1
    } else {
        duration_in_tick * 20
    };

    let count = entities
        .iter()
        .filter_map(|entity| entity.as_living_entity())
        .filter(|living| {
            living.add_mob_effect(
                MobEffectInstance::with_duration(effect, seconds, amplifier)
                    .with_visible(hide_particles),
            )
        })
        .count() as i32;

    if count == 0 {
        return Err(CommandSyntaxError::dynamic(
            translations::COMMANDS_EFFECT_GIVE_FAILED.msg().component(),
        ));
    }

    let message = if count == 1 {
        translations::COMMANDS_EFFECT_GIVE_SUCCESS_SINGLE
            .message([
                TextComponent::plain(effect.key.to_string()),
                entities
                    .first()
                    .expect("entities should contain exactly one element")
                    .display_name(),
            ])
            .component()
    } else {
        translations::COMMANDS_EFFECT_GIVE_SUCCESS_MULTIPLE
            .message([
                TextComponent::plain(effect.key.to_string()),
                TextComponent::plain(entities.len().to_string()),
            ])
            .component()
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
        effect_give_node(&dispatcher, give);
    }

    fn effect_clear_node(
        dispatcher: &CommandDispatcher<CommandSource, SteelCommandRuntime>,
        give_node: NodeId,
    ) {
        let clear = child(dispatcher, give_node, "clear");

        let targets = child(dispatcher, clear, "targets");
        assert_eq!(
            dispatcher
                .node(targets)
                .and_then(|node| node.argument_type()),
            Some(&SteelArgumentType::entities())
        );
        assert!(matches!(
            dispatcher.node(targets),
            Some(node) if node.is_executable()
        ));

        let effect = child(dispatcher, targets, "effect");
        assert_eq!(
            dispatcher
                .node(effect)
                .and_then(|node| node.argument_type()),
            Some(&SteelArgumentType::mob_effect())
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
        let give = child(dispatcher, give_node, "give");
        let targets = child(dispatcher, give, "targets");

        let effect = child(dispatcher, targets, "effect");
        assert_eq!(
            dispatcher
                .node(effect)
                .and_then(|node| node.argument_type()),
            Some(&SteelArgumentType::mob_effect())
        );
        assert!(matches!(
            dispatcher.node(effect),
            Some(node) if node.is_executable()
        ));

        let infinite = child(dispatcher, effect, "infinite");
        assert!(matches!(
            dispatcher.node(infinite),
            Some(node) if node.is_executable()
        ));
        effect_give_infinite_or_second_node(dispatcher, infinite);

        let seconds = child(dispatcher, effect, "seconds");
        assert_eq!(
            dispatcher
                .node(seconds)
                .and_then(|node| node.argument_type()),
            Some(&SteelArgumentType::from(ArgumentType::integer(
                1, 1_000_000
            )))
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
        let amplifier = child(dispatcher, parent_node, "amplifier");
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

        let hide_particles = child(dispatcher, amplifier, "hideParticles");
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
