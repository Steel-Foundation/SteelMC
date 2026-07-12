//! Per-world weather command.

use steel_utils::{Identifier, translations};
use text_components::TextComponent;

use super::super::{
    brigadier::{CommandNodeBuilder, CommandSyntaxError},
    execution::{
        CommandSource, SteelArgumentType, SteelCommandContext, SteelCommandRuntime, argument,
        literal,
    },
    registration::CommandRegistration,
};

const DEFAULT_DURATION: i32 = -1;

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("weather"), |_| command())
}

fn command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("weather")
        .then(weather_literal("clear", WeatherKind::Clear))
        .then(weather_literal("rain", WeatherKind::Rain))
        .then(weather_literal("thunder", WeatherKind::Thunder))
}

fn weather_literal(
    name: &'static str,
    weather: WeatherKind,
) -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal(name)
        .executes(move |context| set_weather(context, weather, DEFAULT_DURATION))
        .then(
            argument("duration", SteelArgumentType::time(1)).executes(move |context| {
                let Some(duration) = context.time("duration") else {
                    return Err(CommandSyntaxError::dynamic(
                        "Parsed weather duration is missing from the command context",
                    ));
                };
                set_weather(context, weather, duration)
            }),
        )
}

#[derive(Clone, Copy)]
enum WeatherKind {
    Clear,
    Rain,
    Thunder,
}

impl WeatherKind {
    fn random_duration(self) -> i32 {
        match self {
            Self::Clear => rand::random_range(12_000..=180_000),
            Self::Rain => rand::random_range(12_000..=24_000),
            Self::Thunder => rand::random_range(3_600..=15_600),
        }
    }

    const fn parameters(self, duration: i32) -> (i32, i32, bool, bool) {
        match self {
            Self::Clear => (duration, 0, false, false),
            Self::Rain => (0, duration, true, false),
            Self::Thunder => (0, duration, true, true),
        }
    }

    fn success_message(self) -> TextComponent {
        match self {
            Self::Clear => TextComponent::from(&translations::COMMANDS_WEATHER_SET_CLEAR),
            Self::Rain => TextComponent::from(&translations::COMMANDS_WEATHER_SET_RAIN),
            Self::Thunder => TextComponent::from(&translations::COMMANDS_WEATHER_SET_THUNDER),
        }
    }
}

#[expect(
    clippy::unnecessary_wraps,
    reason = "Command executors use a shared fallible callback signature."
)]
fn set_weather(
    context: &SteelCommandContext<CommandSource>,
    weather: WeatherKind,
    requested_duration: i32,
) -> Result<i32, CommandSyntaxError> {
    let duration = if requested_duration == DEFAULT_DURATION {
        weather.random_duration()
    } else {
        requested_duration
    };
    let (clear_time, rain_time, raining, thundering) = weather.parameters(duration);
    context
        .source()
        .world()
        .set_weather_parameters(clear_time, rain_time, raining, thundering);
    context
        .source()
        .send_success(&weather.success_message(), true);
    Ok(requested_duration)
}

#[cfg(test)]
mod tests {
    use super::WeatherKind;

    #[test]
    fn weather_kinds_map_to_vanilla_parameter_sets() {
        assert_eq!(WeatherKind::Clear.parameters(40), (40, 0, false, false));
        assert_eq!(WeatherKind::Rain.parameters(40), (0, 40, true, false));
        assert_eq!(WeatherKind::Thunder.parameters(40), (0, 40, true, true));
    }
}
