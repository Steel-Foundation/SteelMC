//! Steel player-flight command.

use std::{slice, sync::Arc};

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
use crate::player::{Abilities, DEFAULT_FLYING_SPEED, Player};

const MAX_FLY_SPEED_MULTIPLIER: f32 = 30.0;

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::from_steel("fly"), |_| command())
}

fn command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("fly")
        .executes(|context| toggle_flight_command(context, FlyTargets::Source))
        .then_all([
            literal("target").then(
                argument("targets", SteelArgumentType::players())
                    .executes(|context| toggle_flight_command(context, FlyTargets::Argument))
                    .then_all([
                        argument("value", ArgumentType::bool()).executes(set_target_flight),
                        literal("speed")
                            .executes(|context| {
                                query_flying_speed_command(context, FlyTargets::Argument)
                            })
                            .then(speed_argument().executes(|context| {
                                set_flying_speed_command(context, FlyTargets::Argument)
                            })),
                    ]),
            ),
            literal("speed")
                .executes(|context| query_flying_speed_command(context, FlyTargets::Source))
                .then(
                    speed_argument()
                        .executes(|context| set_flying_speed_command(context, FlyTargets::Source)),
                ),
        ])
}

fn speed_argument() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    argument("speed", ArgumentType::float(0.0, MAX_FLY_SPEED_MULTIPLIER))
}

#[derive(Clone, Copy)]
enum FlyTargets {
    Source,
    Argument,
}

enum ResolvedFlyTargets<'context> {
    Source(&'context Arc<Player>),
    Argument(Vec<Arc<Player>>),
}

impl ResolvedFlyTargets<'_> {
    fn as_slice(&self) -> &[Arc<Player>] {
        match self {
            Self::Source(player) => slice::from_ref(player),
            Self::Argument(players) => players,
        }
    }
}

impl FlyTargets {
    fn resolve(
        self,
        context: &SteelCommandContext<CommandSource>,
    ) -> Result<ResolvedFlyTargets<'_>, CommandSyntaxError> {
        match self {
            Self::Source => {
                let Some(player) = context.source().player() else {
                    return Err(CommandSyntaxError::dynamic(TextComponent::from(
                        &translations::PERMISSIONS_REQUIRES_PLAYER,
                    )));
                };
                Ok(ResolvedFlyTargets::Source(player))
            }
            Self::Argument => Ok(ResolvedFlyTargets::Argument(context.players("targets")?)),
        }
    }
}

fn toggle_flight_command(
    context: &SteelCommandContext<CommandSource>,
    target_selection: FlyTargets,
) -> Result<i32, CommandSyntaxError> {
    let targets = target_selection.resolve(context)?;
    toggle_flight(targets.as_slice());
    Ok(1)
}

fn set_target_flight(
    context: &SteelCommandContext<CommandSource>,
) -> Result<i32, CommandSyntaxError> {
    let targets = context.players("targets")?;
    let Some(allowed) = context.boolean("value") else {
        return Err(missing_argument("value"));
    };
    set_flight(&targets, allowed);
    Ok(1)
}

fn query_flying_speed_command(
    context: &SteelCommandContext<CommandSource>,
    target_selection: FlyTargets,
) -> Result<i32, CommandSyntaxError> {
    let targets = target_selection.resolve(context)?;
    query_flying_speed(context.source(), targets.as_slice());
    Ok(1)
}

fn set_flying_speed_command(
    context: &SteelCommandContext<CommandSource>,
    target_selection: FlyTargets,
) -> Result<i32, CommandSyntaxError> {
    let targets = target_selection.resolve(context)?;
    let multiplier = required_speed(context)?;
    set_flying_speed(context.source(), targets.as_slice(), multiplier);
    Ok(1)
}

fn required_speed(context: &SteelCommandContext<CommandSource>) -> Result<f32, CommandSyntaxError> {
    context
        .float("speed")
        .ok_or_else(|| missing_argument("speed"))
}

fn toggle_flight(targets: &[Arc<Player>]) {
    for target in targets {
        {
            let mut abilities = target.abilities.lock();
            let allowed = !abilities.may_fly;
            set_flight_allowed(&mut abilities, allowed);
        }
        target.send_abilities();
    }
}

fn set_flight(targets: &[Arc<Player>], allowed: bool) {
    for target in targets {
        {
            let mut abilities = target.abilities.lock();
            set_flight_allowed(&mut abilities, allowed);
        }
        target.send_abilities();
    }
}

const fn set_flight_allowed(abilities: &mut Abilities, allowed: bool) {
    abilities.may_fly = allowed;
    if !allowed {
        abilities.flying = false;
    }
}

fn set_flying_speed(source: &CommandSource, targets: &[Arc<Player>], multiplier: f32) {
    let speed = speed_from_multiplier(multiplier);
    for target in targets {
        target.set_flying_speed(speed);
        target.send_abilities();
        source.send_success(
            &TextComponent::plain(format!(
                "Set flying speed for player '{}' to {multiplier:.1}x ({speed:.3})",
                target.gameprofile.name
            )),
            true,
        );
    }
}

fn query_flying_speed(source: &CommandSource, targets: &[Arc<Player>]) {
    for target in targets {
        let speed = target.get_flying_speed();
        let multiplier = speed / DEFAULT_FLYING_SPEED;
        source.send_success(
            &TextComponent::plain(format!(
                "Current flying speed for player '{}': {multiplier:.1}x ({speed:.3})",
                target.gameprofile.name
            )),
            false,
        );
    }
}

fn speed_from_multiplier(multiplier: f32) -> f32 {
    multiplier * DEFAULT_FLYING_SPEED
}

fn missing_argument(name: &str) -> CommandSyntaxError {
    CommandSyntaxError::dynamic(format!(
        "Parsed value for {name} is missing from the command context"
    ))
}

#[cfg(test)]
mod tests {
    use super::super::create_dispatcher;
    use super::{MAX_FLY_SPEED_MULTIPLIER, set_flight_allowed, speed_from_multiplier};
    use crate::{
        command::{
            brigadier::{ArgumentType, CommandDispatcher, NodeId},
            execution::{CommandSource, SteelArgumentType, SteelCommandRuntime},
        },
        player::{Abilities, DEFAULT_FLYING_SPEED},
    };
    use steel_registry::test_support::init_test_registry;

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
    fn fly_graph_uses_explicit_target_and_bounded_speed_branches() {
        init_test_registry();
        let Ok(dispatcher) = create_dispatcher() else {
            panic!("built-in commands should register");
        };
        let fly = child(&dispatcher, dispatcher.root(), "fly");
        assert!(matches!(
            dispatcher.node(fly),
            Some(node) if node.is_executable()
        ));

        let target = child(&dispatcher, fly, "target");
        let targets = child(&dispatcher, target, "targets");
        assert!(matches!(
            dispatcher.node(targets),
            Some(node)
                if node.is_executable()
                    && node.argument_type() == Some(&SteelArgumentType::players())
        ));
        let value = child(&dispatcher, targets, "value");
        assert_eq!(
            dispatcher.node(value).and_then(|node| node.argument_type()),
            Some(&SteelArgumentType::from(ArgumentType::bool()))
        );

        let own_speed = child(&dispatcher, fly, "speed");
        let own_speed_value = child(&dispatcher, own_speed, "speed");
        let target_speed = child(&dispatcher, targets, "speed");
        let target_speed_value = child(&dispatcher, target_speed, "speed");
        let expected = SteelArgumentType::from(ArgumentType::float(0.0, MAX_FLY_SPEED_MULTIPLIER));
        for node in [own_speed_value, target_speed_value] {
            assert_eq!(
                dispatcher.node(node).and_then(|node| node.argument_type()),
                Some(&expected)
            );
            assert!(matches!(
                dispatcher.node(node),
                Some(node) if node.is_executable()
            ));
        }
    }

    #[test]
    fn disabling_flight_clears_active_flying_state() {
        let mut abilities = Abilities {
            may_fly: true,
            flying: true,
            ..Abilities::default()
        };

        set_flight_allowed(&mut abilities, false);
        assert!(!abilities.may_fly);
        assert!(!abilities.flying);

        set_flight_allowed(&mut abilities, true);
        assert!(abilities.may_fly);
        assert!(!abilities.flying);
    }

    #[test]
    fn fly_speed_uses_vanilla_default_as_the_multiplier_base() {
        let speed = speed_from_multiplier(MAX_FLY_SPEED_MULTIPLIER);
        let expected = 30.0 * DEFAULT_FLYING_SPEED;
        assert!((speed - expected).abs() <= f32::EPSILON);
    }
}
