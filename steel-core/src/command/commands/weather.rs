//! Handler for the "weather" command.
use crate::command::arguments::time::TimeArgument;
use crate::command::commands::{
    CommandExecutor, CommandHandlerBuilder, CommandHandlerDyn, argument, literal,
};
use crate::command::context::CommandContext;
use crate::command::error::CommandError;
use steel_utils::translations;

/// Handler for the "weather" command.
#[must_use]
pub fn command_handler() -> impl CommandHandlerDyn {
    CommandHandlerBuilder::new(
        &["weather"],
        "Changes the weather in the current dimension.",
        "minecraft:command.weather",
    )
    .then(
        literal("rain")
            .then(argument("duration", TimeArgument).executes(WeatherCommandExecutor::Rain))
            .executes(WeatherCommandExecutor::Rain),
    )
    .then(
        literal("thunder")
            .then(argument("duration", TimeArgument).executes(WeatherCommandExecutor::Thunder))
            .executes(WeatherCommandExecutor::Thunder),
    )
    .then(
        literal("clear")
            .then(argument("duration", TimeArgument).executes(WeatherCommandExecutor::Clear))
            .executes(WeatherCommandExecutor::Clear),
    )
}

enum WeatherCommandExecutor {
    Clear,
    Rain,
    Thunder,
}

impl CommandExecutor<()> for WeatherCommandExecutor {
    fn execute(&self, _args: (), context: &mut CommandContext) -> Result<(), CommandError> {
        let duration = match self {
            WeatherCommandExecutor::Clear => rand::random_range(12_000..=180_000),
            WeatherCommandExecutor::Rain => rand::random_range(12_000..=24_000),
            WeatherCommandExecutor::Thunder => rand::random_range(3_600..=15_600),
        };

        self.execute(((), duration), context)
    }
}

impl CommandExecutor<((), i32)> for WeatherCommandExecutor {
    fn execute(&self, _args: ((), i32), context: &mut CommandContext) -> Result<(), CommandError> {
        // let ((), _duration) = args;
        // let _world = &context.world;

        // TODO: Apply the duration to the world's weather system once weather state is implemented

        match self {
            WeatherCommandExecutor::Clear => {
                context
                    .sender
                    .send_message(&translations::COMMANDS_WEATHER_SET_CLEAR.msg().into());
            }
            WeatherCommandExecutor::Rain => {
                context
                    .sender
                    .send_message(&translations::COMMANDS_WEATHER_SET_RAIN.msg().into());
            }
            WeatherCommandExecutor::Thunder => {
                context
                    .sender
                    .send_message(&translations::COMMANDS_WEATHER_SET_THUNDER.msg().into());
            }
        }

        Ok(())
    }
}
