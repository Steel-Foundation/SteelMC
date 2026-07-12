//! Per-world clock command.
//!
//! Vanilla applies clock mutations server-wide. Steel intentionally applies
//! them only to the command source's world so multiple worlds in one domain can
//! keep independent timelines. Use `execute in <world> run time ...` to target
//! a different world.

use steel_registry::{timeline::TimelineRef, world_clock::WorldClockRef};
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

const CLOCK_ARGUMENT: &str = "clock";

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("time"), |_| command())
}

fn command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    let command = add_clock_nodes(literal("time"), ClockSelection::Default, true);
    command.then(literal("of").then(add_clock_nodes(
        argument(CLOCK_ARGUMENT, SteelArgumentType::world_clock()),
        ClockSelection::Argument(CLOCK_ARGUMENT),
        false,
    )))
}

#[derive(Clone, Copy)]
enum ClockSelection {
    Default,
    Argument(&'static str),
}

impl ClockSelection {
    const fn argument_name(self) -> Option<&'static str> {
        match self {
            Self::Default => None,
            Self::Argument(name) => Some(name),
        }
    }
}

fn add_clock_nodes(
    node: CommandNodeBuilder<CommandSource, SteelCommandRuntime>,
    selection: ClockSelection,
    include_game_time: bool,
) -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    let mut query = literal("query")
        .then(literal("time").executes(move |context| query_time(context, selection)))
        .then(
            argument(
                "timeline",
                SteelArgumentType::timeline(selection.argument_name()),
            )
            .executes(move |context| query_timeline(context, selection))
            .then(
                literal("repetition")
                    .executes(move |context| query_timeline_repetitions(context, selection)),
            ),
        );
    if include_game_time {
        query = query.then(literal("gametime").executes(query_game_time));
    }

    node.then(
        literal("set")
            .then(
                argument("time", SteelArgumentType::time(0))
                    .executes(move |context| set_total_ticks(context, selection)),
            )
            .then(
                argument(
                    "timemarker",
                    SteelArgumentType::time_marker(selection.argument_name()),
                )
                .executes(move |context| set_time_marker(context, selection)),
            ),
    )
    .then(
        literal("add").then(
            argument("time", SteelArgumentType::time(i32::MIN))
                .executes(move |context| add_time(context, selection)),
        ),
    )
    .then(literal("pause").executes(move |context| set_paused(context, selection, true)))
    .then(literal("resume").executes(move |context| set_paused(context, selection, false)))
    .then(
        literal("rate").then(
            argument("rate", ArgumentType::float(1.0E-5, 1_000.0))
                .executes(move |context| set_rate(context, selection)),
        ),
    )
    .then(query)
}

fn selected_clock(
    context: &SteelCommandContext<CommandSource>,
    selection: ClockSelection,
) -> Result<WorldClockRef, CommandSyntaxError> {
    match selection {
        ClockSelection::Default => context
            .source()
            .world()
            .dimension_type
            .default_clock
            .ok_or_else(|| {
                let message = translations::COMMANDS_TIME_NO_DEFAULT_CLOCK
                    .message([context.source().world().dimension_type.key.to_string()])
                    .component();
                CommandSyntaxError::dynamic(message)
            }),
        ClockSelection::Argument(name) => context.world_clock(name).ok_or_else(|| {
            CommandSyntaxError::dynamic(format!(
                "Parsed world clock {name} is missing from the command context"
            ))
        }),
    }
}

fn clock_total_ticks(
    context: &SteelCommandContext<CommandSource>,
    clock: WorldClockRef,
) -> Result<i64, CommandSyntaxError> {
    context
        .source()
        .world()
        .clock_total_ticks(clock)
        .ok_or_else(|| missing_clock(clock))
}

fn set_total_ticks(
    context: &SteelCommandContext<CommandSource>,
    selection: ClockSelection,
) -> Result<i32, CommandSyntaxError> {
    let clock = selected_clock(context, selection)?;
    let Some(total_ticks) = context.time("time") else {
        return Err(missing_argument("time"));
    };
    context
        .source()
        .world()
        .set_clock_total_ticks(clock, i64::from(total_ticks))
        .ok_or_else(|| missing_clock(clock))?;
    let message = translations::COMMANDS_TIME_SET_ABSOLUTE
        .message([clock.key.to_string(), total_ticks.to_string()])
        .component();
    context.source().send_success(&message, true);
    Ok(total_ticks)
}

fn add_time(
    context: &SteelCommandContext<CommandSource>,
    selection: ClockSelection,
) -> Result<i32, CommandSyntaxError> {
    let clock = selected_clock(context, selection)?;
    let Some(ticks) = context.time("time") else {
        return Err(missing_argument("time"));
    };
    let total_ticks = context
        .source()
        .world()
        .add_clock_ticks(clock, ticks)
        .ok_or_else(|| missing_clock(clock))?;
    let message = translations::COMMANDS_TIME_SET_ABSOLUTE
        .message([clock.key.to_string(), total_ticks.to_string()])
        .component();
    context.source().send_success(&message, true);
    Ok(wrap_time(total_ticks))
}

fn set_time_marker(
    context: &SteelCommandContext<CommandSource>,
    selection: ClockSelection,
) -> Result<i32, CommandSyntaxError> {
    let clock = selected_clock(context, selection)?;
    let Some(marker) = context.identifier("timemarker") else {
        return Err(missing_argument("timemarker"));
    };
    match context
        .source()
        .world()
        .move_clock_to_time_marker(clock, marker)
    {
        Some(true) => {}
        Some(false) => {
            let message = missing_time_marker_message(clock, marker);
            return Err(CommandSyntaxError::dynamic(message));
        }
        None => return Err(missing_clock(clock)),
    }
    let total_ticks = clock_total_ticks(context, clock)?;
    let message = translations::COMMANDS_TIME_SET_TIME_MARKER
        .message([clock.key.to_string(), marker.to_string()])
        .component();
    context.source().send_success(&message, true);
    Ok(wrap_time(total_ticks))
}

fn set_paused(
    context: &SteelCommandContext<CommandSource>,
    selection: ClockSelection,
    paused: bool,
) -> Result<i32, CommandSyntaxError> {
    let clock = selected_clock(context, selection)?;
    context
        .source()
        .world()
        .set_clock_paused(clock, paused)
        .ok_or_else(|| missing_clock(clock))?;
    let translation = if paused {
        &translations::COMMANDS_TIME_PAUSE
    } else {
        &translations::COMMANDS_TIME_RESUME
    };
    let message = translation.message([clock.key.to_string()]).component();
    context.source().send_success(&message, true);
    Ok(1)
}

fn set_rate(
    context: &SteelCommandContext<CommandSource>,
    selection: ClockSelection,
) -> Result<i32, CommandSyntaxError> {
    let clock = selected_clock(context, selection)?;
    let Some(rate) = context.float("rate") else {
        return Err(missing_argument("rate"));
    };
    context
        .source()
        .world()
        .set_clock_rate(clock, rate)
        .ok_or_else(|| missing_clock(clock))?;
    let message = translations::COMMANDS_TIME_RATE
        .message([clock.key.to_string(), rate.to_string()])
        .component();
    context.source().send_success(&message, true);
    Ok(1)
}

#[expect(
    clippy::unnecessary_wraps,
    reason = "Command executors use a shared fallible callback signature."
)]
fn query_game_time(
    context: &SteelCommandContext<CommandSource>,
) -> Result<i32, CommandSyntaxError> {
    let game_time = context.source().world().game_time();
    let message = translations::COMMANDS_TIME_QUERY_GAMETIME
        .message([game_time.to_string()])
        .component();
    context.source().send_success(&message, false);
    Ok(wrap_time(game_time))
}

fn query_time(
    context: &SteelCommandContext<CommandSource>,
    selection: ClockSelection,
) -> Result<i32, CommandSyntaxError> {
    let clock = selected_clock(context, selection)?;
    let total_ticks = clock_total_ticks(context, clock)?;
    let message = translations::COMMANDS_TIME_QUERY_ABSOLUTE
        .message([clock.key.to_string(), total_ticks.to_string()])
        .component();
    context.source().send_success(&message, false);
    Ok(wrap_time(total_ticks))
}

fn selected_timeline(
    context: &SteelCommandContext<CommandSource>,
    clock: WorldClockRef,
) -> Result<TimelineRef, CommandSyntaxError> {
    let Some(timeline) = context.timeline("timeline") else {
        return Err(missing_argument("timeline"));
    };
    if timeline.clock != clock {
        let message = wrong_timeline_for_clock_message(clock, timeline);
        return Err(CommandSyntaxError::dynamic(message));
    }
    Ok(timeline)
}

fn missing_time_marker_message(clock: WorldClockRef, marker: &Identifier) -> TextComponent {
    translations::COMMANDS_TIME_NO_TIME_MARKER_FOUND
        .message([marker.to_string(), clock.key.to_string()])
        .component()
}

fn wrong_timeline_for_clock_message(clock: WorldClockRef, timeline: TimelineRef) -> TextComponent {
    translations::COMMANDS_TIME_WRONG_TIMELINE_FOR_CLOCK
        .message([timeline.key.to_string(), clock.key.to_string()])
        .component()
}

fn query_timeline(
    context: &SteelCommandContext<CommandSource>,
    selection: ClockSelection,
) -> Result<i32, CommandSyntaxError> {
    let clock = selected_clock(context, selection)?;
    let timeline = selected_timeline(context, clock)?;
    let current_ticks = timeline.current_ticks(clock_total_ticks(context, clock)?);
    let message = translations::COMMANDS_TIME_QUERY_TIMELINE
        .message([timeline.key.to_string(), current_ticks.to_string()])
        .component();
    context.source().send_success(&message, false);
    Ok(wrap_time(current_ticks))
}

fn query_timeline_repetitions(
    context: &SteelCommandContext<CommandSource>,
    selection: ClockSelection,
) -> Result<i32, CommandSyntaxError> {
    let clock = selected_clock(context, selection)?;
    let timeline = selected_timeline(context, clock)?;
    let repetitions = timeline.period_count(clock_total_ticks(context, clock)?);
    let message = translations::COMMANDS_TIME_QUERY_TIMELINE_REPETITIONS
        .message([timeline.key.to_string(), repetitions.to_string()])
        .component();
    context.source().send_success(&message, false);
    Ok(wrap_time(i64::from(repetitions)))
}

fn missing_argument(name: &str) -> CommandSyntaxError {
    CommandSyntaxError::dynamic(format!(
        "Parsed value for {name} is missing from the command context"
    ))
}

fn missing_clock(clock: WorldClockRef) -> CommandSyntaxError {
    CommandSyntaxError::dynamic(format!("World clock {} is not initialized", clock.key))
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "the remainder is always within the signed 32-bit result range"
)]
fn wrap_time(ticks: i64) -> i32 {
    (ticks % i64::from(i32::MAX)) as i32
}

#[cfg(test)]
mod tests {
    use super::super::create_dispatcher;
    use super::{
        CLOCK_ARGUMENT, missing_time_marker_message, wrap_time, wrong_timeline_for_clock_message,
    };
    use crate::command::{
        brigadier::{ArgumentType, CommandDispatcher, NodeId},
        execution::{CommandSource, SteelArgumentType, SteelCommandRuntime},
    };
    use steel_registry::{
        test_support::init_test_registry, vanilla_timelines, vanilla_world_clocks,
    };
    use steel_utils::{Identifier, translations};

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
    fn time_graph_matches_the_26_2_clock_shape() {
        init_test_registry();
        let Ok(dispatcher) = create_dispatcher() else {
            panic!("built-in commands should register");
        };
        let root = child(&dispatcher, dispatcher.root(), "time");
        let children = dispatcher.children(root).unwrap_or_default();
        let names = children
            .iter()
            .filter_map(|child| {
                let node = dispatcher.node(*child)?;
                Some(node.name())
            })
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            ["set", "add", "pause", "resume", "rate", "query", "of"]
        );

        let set = child(&dispatcher, root, "set");
        let time = child(&dispatcher, set, "time");
        assert_eq!(
            dispatcher.node(time).and_then(|node| node.argument_type()),
            Some(&SteelArgumentType::time(0))
        );
        let marker = child(&dispatcher, set, "timemarker");
        assert_eq!(
            dispatcher
                .node(marker)
                .and_then(|node| node.argument_type()),
            Some(&SteelArgumentType::time_marker(None))
        );

        let rate = child(&dispatcher, root, "rate");
        let rate_value = child(&dispatcher, rate, "rate");
        assert_eq!(
            dispatcher
                .node(rate_value)
                .and_then(|node| node.argument_type()),
            Some(&SteelArgumentType::from(ArgumentType::float(
                1.0E-5, 1_000.0
            )))
        );

        let of = child(&dispatcher, root, "of");
        let clock = child(&dispatcher, of, CLOCK_ARGUMENT);
        assert_eq!(
            dispatcher.node(clock).and_then(|node| node.argument_type()),
            Some(&SteelArgumentType::world_clock())
        );
        let of_set = child(&dispatcher, clock, "set");
        let of_marker = child(&dispatcher, of_set, "timemarker");
        assert_eq!(
            dispatcher
                .node(of_marker)
                .and_then(|node| node.argument_type()),
            Some(&SteelArgumentType::time_marker(Some("clock")))
        );
    }

    #[test]
    fn wrap_time_matches_vanilla_modulus() {
        assert_eq!(wrap_time(0), 0);
        assert_eq!(wrap_time(i64::from(i32::MAX)), 0);
        assert_eq!(wrap_time(i64::from(i32::MAX) + 4), 4);
        assert_eq!(wrap_time(-4), -4);
    }

    #[test]
    fn time_marker_and_timeline_errors_use_vanillas_argument_order() {
        let marker = Identifier::vanilla_static("missing");
        assert_eq!(
            missing_time_marker_message(&vanilla_world_clocks::OVERWORLD, &marker),
            translations::COMMANDS_TIME_NO_TIME_MARKER_FOUND
                .message(["minecraft:missing", "minecraft:overworld"])
                .component()
        );
        assert_eq!(
            wrong_timeline_for_clock_message(
                &vanilla_world_clocks::THE_END,
                &vanilla_timelines::DAY,
            ),
            translations::COMMANDS_TIME_WRONG_TIMELINE_FOR_CLOCK
                .message(["minecraft:day", "minecraft:the_end"])
                .component()
        );
    }
}
