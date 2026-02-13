//! Handler for the "flyspeed" command.
use std::slice;
use std::sync::Arc;

use crate::command::arguments::bool::BoolArgument;
use crate::command::arguments::float::FloatArgument;
use crate::command::arguments::player::PlayerArgument;
use crate::command::commands::{CommandHandlerBuilder, CommandHandlerDyn, argument, literal};
use crate::command::context::CommandContext;
use crate::command::error::CommandError;
use crate::command::sender::CommandSender;
use crate::player::Player;
use text_components::TextComponent;

/// Handler for the "flyspeed" command.
#[must_use]
pub fn command_handler() -> impl CommandHandlerDyn {
    CommandHandlerBuilder::new(
        &["fly"],
        "Sets the target's flying parameters (may_fly, speed).",
        "minecraft:command.fly",
    )
    .executes(|(), ctx: &mut CommandContext| {
        let player = ctx
            .sender
            .get_player()
            .ok_or(CommandError::InvalidRequirement)?;

        toggle_fly(slice::from_ref(player));

        Ok(())
    })
    .then(
        argument("target", PlayerArgument::multiple())
            .executes(
                |((), targets): ((), Vec<Arc<Player>>), _ctx: &mut CommandContext| {
                    toggle_fly(&targets);
                    Ok(())
                },
            )
            .then(argument("value", BoolArgument).executes(
                |(((), targets), value): (((), Vec<Arc<Player>>), bool),
                 _ctx: &mut CommandContext| {
                    set_fly(&targets, value);
                    Ok(())
                },
            ))
            .then(
                literal("speed")
                    .executes(
                        |((), targets): ((), Vec<Arc<Player>>), ctx: &mut CommandContext| {
                            query_flying_speed(&targets, &ctx.sender);
                            Ok(())
                        },
                    )
                    .then(
                        argument("speed", FloatArgument::bounded(Some(0.0), Some(10.0))).executes(
                            |(((), targets), speed): (((), Vec<Arc<Player>>), f32),
                             ctx: &mut CommandContext| {
                                set_flying_speed(&targets, speed, &ctx.sender);

                                Ok(())
                            },
                        ),
                    ),
            ),
    )
    .then(
        literal("speed")
            .executes(|(), ctx: &mut CommandContext| {
                let player = ctx
                    .sender
                    .get_player()
                    .ok_or(CommandError::InvalidRequirement)?;

                query_flying_speed(slice::from_ref(player), &ctx.sender);

                Ok(())
            })
            .then(
                argument("speed", FloatArgument::bounded(Some(0.0), Some(10.0))).executes(
                    |((), speed): ((), f32), ctx: &mut CommandContext| {
                        let player = ctx
                            .sender
                            .get_player()
                            .ok_or(CommandError::InvalidRequirement)?;
                        set_flying_speed(slice::from_ref(player), speed, &ctx.sender);
                        Ok(())
                    },
                ),
            ),
    )
}

fn toggle_fly(targets: &[Arc<Player>]) {
    for target in targets {
        {
            let mut lock = target.abilities.lock();
            lock.may_fly = !lock.may_fly;
            if !lock.may_fly {
                lock.flying = false;
            }
        }
        target.send_abilities();
    }
}

fn set_fly(targets: &[Arc<Player>], value: bool) {
    for target in targets {
        {
            let mut lock = target.abilities.lock();
            lock.may_fly = value;
            if !value {
                lock.flying = false;
            }
        }
        target.send_abilities();
    }
}

fn set_flying_speed(targets: &[Arc<Player>], multiplier: f32, sender: &CommandSender) {
    let speed = multiplier * 0.05;
    for target in targets {
        target.set_flying_speed(speed);
        target.send_abilities();
        sender.send_message(&TextComponent::from(format!(
            "Set flying speed for player '{}' to {multiplier:.1}x ({speed:.3})",
            target.gameprofile.name.clone()
        )));
    }
}

fn query_flying_speed(targets: &[Arc<Player>], sender: &CommandSender) {
    for target in targets {
        let speed = target.get_flying_speed();
        let multiplier = speed / 0.05; // Show as multiplier of default speed

        sender.send_message(&TextComponent::from(format!(
            "Current flying speed for player '{}': {multiplier:.1}x ({speed:.3})",
            target.gameprofile.name.clone()
        )));
    }
}
