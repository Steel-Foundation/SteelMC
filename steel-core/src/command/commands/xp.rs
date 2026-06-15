//! Experience Command

use std::sync::Arc;

use steel_utils::translations;
use text_components::TextComponent;

use crate::{
    command::{
        arguments::{integer::IntegerArgument, player::PlayerArgument},
        commands::{CommandHandlerBuilder, CommandHandlerDyn, argument, literal},
        context::CommandContext,
        error::CommandError,
    },
    player::Player,
};

/// Handler for the "xp" command.
#[expect(clippy::too_many_lines, reason = "its fine")]
#[must_use]
pub fn command_handler() -> impl CommandHandlerDyn {
    CommandHandlerBuilder::new(
        &["xp", "experience"],
        "Gives, queries and sets a player's experience level and points.",
        "minecraft:command.xp",
    )
    .then(
        literal("query").then(
            argument("target", PlayerArgument::multiple())
                .then(literal("points").executes(
                    |((), players): ((), Vec<Arc<Player>>), ctx: &mut CommandContext| {
                        for player in players {
                            let points = { player.experience.lock().points() };
                            ctx.sender.send_message(
                                &translations::COMMANDS_EXPERIENCE_QUERY_POINTS
                                    .message([
                                        TextComponent::from(player.gameprofile.name.clone()),
                                        TextComponent::from(points.to_string()),
                                    ])
                                    .into(),
                            );
                        }
                        Ok(())
                    },
                ))
                .then(literal("levels").executes(
                    |((), players): ((), Vec<Arc<Player>>), ctx: &mut CommandContext| {
                        for player in players {
                            let level = { player.experience.lock().level() };
                            ctx.sender.send_message(
                                &translations::COMMANDS_EXPERIENCE_QUERY_LEVELS
                                    .message([
                                        TextComponent::from(player.gameprofile.name.clone()),
                                        TextComponent::from(level.to_string()),
                                    ])
                                    .into(),
                            );
                        }
                        Ok(())
                    },
                )),
        ),
    )
    .then(
        literal("set").then(
            argument("target", PlayerArgument::multiple()).then(
                argument("amount", IntegerArgument::bounded(Some(0), None))
                    .executes(
                        |(((), players), amount): (((), Vec<Arc<Player>>), i32),
                         ctx: &mut CommandContext| {
                            set_experience(players, amount, ExperienceType::Points, ctx)
                        },
                    )
                    .then(literal("points").executes(
                        |(((), players), amount): (((), Vec<Arc<Player>>), i32),
                         ctx: &mut CommandContext| {
                            set_experience(players, amount, ExperienceType::Points, ctx)
                        },
                    ))
                    .then(literal("levels").executes(
                        |(((), players), amount): (((), Vec<Arc<Player>>), i32),
                         ctx: &mut CommandContext| {
                            set_experience(players, amount, ExperienceType::Levels, ctx)
                        },
                    )),
            ),
        ),
    )
    .then(
        literal("add").then(
            argument("target", PlayerArgument::multiple()).then(
                argument("amount", IntegerArgument::new())
                    .executes(
                        |(((), players), amount): (((), Vec<Arc<Player>>), i32),
                         ctx: &mut CommandContext| {
                            add_experience(players, amount, ExperienceType::Points, ctx);
                            Ok(())
                        },
                    )
                    .then(literal("points").executes(
                        |(((), players), amount): (((), Vec<Arc<Player>>), i32),
                         ctx: &mut CommandContext| {
                            add_experience(players, amount, ExperienceType::Points, ctx);
                            Ok(())
                        },
                    ))
                    .then(literal("levels").executes(
                        |(((), players), amount): (((), Vec<Arc<Player>>), i32),
                         ctx: &mut CommandContext| {
                            add_experience(players, amount, ExperienceType::Levels, ctx);
                            Ok(())
                        },
                    )),
            ),
        ),
    )
    .then(
        literal("clear")
            .executes(|(): (), ctx: &mut CommandContext| {
                if let Some(player) = ctx.sender.get_player() {
                    player.experience.lock().set_total_points(0);
                }
                Ok(())
            })
            .then(argument("target", PlayerArgument::multiple()).executes(
                |((), players): ((), Vec<Arc<Player>>), _ctx: &mut CommandContext| {
                    for player in players {
                        player.experience.lock().set_total_points(0);
                    }
                    Ok(())
                },
            )),
    )
}

enum ExperienceType {
    Points,
    Levels,
}

fn set_experience(
    players: Vec<Arc<Player>>,
    amount: i32,
    xp_type: ExperienceType,
    ctx: &mut CommandContext,
) -> Result<(), CommandError> {
    for player in &players {
        let mut experience = player.experience.lock();
        match xp_type {
            ExperienceType::Points => experience
                .set_points(amount)
                .map_err(|err| CommandError::CommandFailed(Box::new(TextComponent::from(err))))?,
            ExperienceType::Levels => experience.set_levels(amount),
        }
    }

    if let [player] = players.as_slice() {
        let translation = match xp_type {
            ExperienceType::Points => &translations::COMMANDS_EXPERIENCE_SET_POINTS_SUCCESS_SINGLE,
            ExperienceType::Levels => &translations::COMMANDS_EXPERIENCE_SET_LEVELS_SUCCESS_SINGLE,
        };

        ctx.sender.send_message(
            &translation
                .message([
                    TextComponent::from(amount.to_string()),
                    TextComponent::from(player.gameprofile.name.clone()),
                ])
                .into(),
        );
    } else {
        let translation = match xp_type {
            ExperienceType::Points => {
                &translations::COMMANDS_EXPERIENCE_SET_POINTS_SUCCESS_MULTIPLE
            }
            ExperienceType::Levels => {
                &translations::COMMANDS_EXPERIENCE_SET_LEVELS_SUCCESS_MULTIPLE
            }
        };

        ctx.sender.send_message(
            &translation
                .message([
                    TextComponent::from(amount.to_string()),
                    TextComponent::from(players.len().to_string()),
                ])
                .into(),
        );
    }

    Ok(())
}

fn add_experience(
    players: Vec<Arc<Player>>,
    amount: i32,
    xp_type: ExperienceType,
    ctx: &mut CommandContext,
) {
    for player in &players {
        let mut experience = player.experience.lock();
        match xp_type {
            ExperienceType::Points => experience.add_points(amount),
            ExperienceType::Levels => experience.add_levels(amount),
        }
    }

    if let [player] = players.as_slice() {
        let translation = match xp_type {
            ExperienceType::Points => &translations::COMMANDS_EXPERIENCE_ADD_POINTS_SUCCESS_SINGLE,
            ExperienceType::Levels => &translations::COMMANDS_EXPERIENCE_ADD_LEVELS_SUCCESS_SINGLE,
        };

        ctx.sender.send_message(
            &translation
                .message([
                    TextComponent::from(amount.to_string()),
                    TextComponent::from(player.gameprofile.name.clone()),
                ])
                .into(),
        );
    } else {
        let translation = match xp_type {
            ExperienceType::Points => {
                &translations::COMMANDS_EXPERIENCE_ADD_POINTS_SUCCESS_MULTIPLE
            }
            ExperienceType::Levels => {
                &translations::COMMANDS_EXPERIENCE_ADD_LEVELS_SUCCESS_MULTIPLE
            }
        };

        ctx.sender.send_message(
            &translation
                .message([
                    TextComponent::from(amount.to_string()),
                    TextComponent::from(players.len().to_string()),
                ])
                .into(),
        );
    }
}
