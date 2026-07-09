//! Handler for the `time` command
use steel_registry::world_clock::WorldClockRef;
use steel_utils::{Identifier, translations};
use text_components::TextComponent;

use crate::command::{
    arguments::time::TimeArgument,
    commands::{CommandExecutor, CommandHandlerBuilder, CommandHandlerDyn, argument, literal},
    context::CommandContext,
    error::CommandError,
};

/// Handler for the `time` command
#[must_use]
pub fn command_handler() -> impl CommandHandlerDyn {
    CommandHandlerBuilder::new(
        &["time"],
        "Allows interacting with the ingame time.",
        "minecraft:command.time",
    )
    .then(
        literal("query")
            .then(literal("day").executes(TimeQueryExecutor::Day))
            .then(literal("daytime").executes(TimeQueryExecutor::Daytime))
            .then(literal("gametime").executes(TimeQueryExecutor::Gametime)),
    )
    .then(
        literal("set")
            .then(literal("day").executes(TimeMarkerSetExecutor("day")))
            .then(literal("midnight").executes(TimeMarkerSetExecutor("midnight")))
            .then(literal("night").executes(TimeMarkerSetExecutor("night")))
            .then(literal("noon").executes(TimeMarkerSetExecutor("noon")))
            .then(argument("time", TimeArgument).executes(TimeExecutor::Set)),
    )
    .then(literal("add").then(argument("time", TimeArgument).executes(TimeExecutor::Add)))
}

enum TimeQueryExecutor {
    Day,
    Daytime,
    Gametime,
}

impl CommandExecutor<()> for TimeQueryExecutor {
    fn execute(&self, _args: (), context: &mut CommandContext) -> Result<(), CommandError> {
        let number = match self {
            TimeQueryExecutor::Gametime => context.world.game_time(),
            TimeQueryExecutor::Day | TimeQueryExecutor::Daytime => {
                let clock = default_clock(context)?;
                let total_ticks = context.world.clock_total_ticks(clock).ok_or_else(|| {
                    CommandError::CommandFailed(Box::new(TextComponent::from(format!(
                        "world clock {} is not initialized",
                        clock.key
                    ))))
                })?;
                if matches!(self, TimeQueryExecutor::Day) {
                    total_ticks / 24_000
                } else {
                    total_ticks % 24_000
                }
            }
        };
        context.sender.send_message(
            &translations::COMMANDS_TIME_QUERY
                .message([TextComponent::from(format!("{number}"))])
                .into(),
        );
        Ok(())
    }
}

enum TimeExecutor {
    Add,
    Set,
}

impl CommandExecutor<((), i32)> for TimeExecutor {
    fn execute(&self, args: ((), i32), context: &mut CommandContext) -> Result<(), CommandError> {
        let clock = default_clock(context)?;
        let total_ticks = match self {
            TimeExecutor::Add => context.world.add_clock_ticks(clock, args.1),
            TimeExecutor::Set => context
                .world
                .set_clock_total_ticks(clock, i64::from(args.1))
                .map(|()| i64::from(args.1)),
        };
        let Some(total_ticks) = total_ticks else {
            return Err(missing_clock(clock));
        };

        context.sender.send_message(
            &translations::COMMANDS_TIME_SET
                .message([TextComponent::from(format!("{total_ticks}"))])
                .into(),
        );

        Ok(())
    }
}

struct TimeMarkerSetExecutor(&'static str);

impl CommandExecutor<()> for TimeMarkerSetExecutor {
    fn execute(&self, _args: (), context: &mut CommandContext) -> Result<(), CommandError> {
        let clock = default_clock(context)?;
        let marker = Identifier::vanilla_static(self.0);
        match context.world.move_clock_to_time_marker(clock, &marker) {
            Some(true) => {}
            Some(false) => {
                let message = translations::COMMANDS_TIME_NO_TIME_MARKER_FOUND
                    .message([clock.key.to_string(), marker.to_string()])
                    .component();
                return Err(CommandError::CommandFailed(Box::new(message)));
            }
            None => return Err(missing_clock(clock)),
        }
        let Some(total_ticks) = context.world.clock_total_ticks(clock) else {
            return Err(missing_clock(clock));
        };

        context.sender.send_message(
            &translations::COMMANDS_TIME_SET
                .message([TextComponent::from(format!("{total_ticks}"))])
                .into(),
        );

        Ok(())
    }
}

fn default_clock(context: &CommandContext) -> Result<WorldClockRef, CommandError> {
    context.world.dimension_type.default_clock.ok_or_else(|| {
        let message = translations::COMMANDS_TIME_NO_DEFAULT_CLOCK
            .message([context.world.dimension_type.key.to_string()])
            .component();
        CommandError::CommandFailed(Box::new(message))
    })
}

fn missing_clock(clock: WorldClockRef) -> CommandError {
    CommandError::CommandFailed(Box::new(TextComponent::from(format!(
        "world clock {} is not initialized",
        clock.key
    ))))
}
