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
    fn execute(&self, args: ((), i32), context: &mut CommandContext) -> Result<(), CommandError> {
        let ((), duration) = args;
        let world = &context.world;
        let mut lock = world.level_data.write();
        let (clear_weather_time, weather_time, raining, thundering) = match self {
            WeatherCommandExecutor::Clear => (duration, 0, false, false),
            WeatherCommandExecutor::Rain => (0, duration, true, false),
            WeatherCommandExecutor::Thunder => (0, duration, true, true),
        };

        lock.set_clear_weather_time(clear_weather_time);
        lock.set_rain_time(weather_time);
        lock.set_thunder_time(weather_time);
        lock.set_raining(raining);
        lock.set_thundering(thundering);

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
