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

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("tick"), |_| command())
        .subcommand_permission(["rate"])
        .subcommand_permission(["step"])
        .subcommand_permission(["sprint"])
        .subcommand_permission(["unfreeze"])
        .subcommand_permission(["freeze"])
}

fn command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("tick")
        .then(literal("query").executes(query_tick))
        .then(
            literal("rate")
                .then(argument("rate", ArgumentType::float(1.0, 10_000.0)).executes(set_tick_rate)),
        )
        .then(
            literal("step")
                .executes(|context| step(context, 1))
                .then(literal("stop").executes(stop_step))
                .then(
                    argument("time", SteelArgumentType::time(1)).executes(|context| {
                        let Some(ticks) = context.time("time") else {
                            return Err(missing_argument("time"));
                        };
                        step(context, ticks)
                    }),
                ),
        )
        .then(
            literal("sprint")
                .then(literal("stop").executes(stop_sprint))
                .then(
                    argument("time", SteelArgumentType::time(1)).executes(|context| {
                        let Some(ticks) = context.time("time") else {
                            return Err(missing_argument("time"));
                        };
                        sprint(context, ticks)
                    }),
                ),
        )
        .then(literal("unfreeze").executes(|context| set_frozen(context, false)))
        .then(literal("freeze").executes(|context| set_frozen(context, true)))
}

fn nanos_to_millis_string(nanos: u64) -> String {
    format!("{:.1}", nanos as f64 / 1_000_000.0)
}

enum TickStatus {
    Sprinting,
    Frozen,
    Lagging,
    Running,
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::unnecessary_wraps,
    reason = "the bounded rate is intentionally truncated and executors share a fallible signature"
)]
fn query_tick(context: &SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError> {
    let (tick_rate, busy_time_nanos, target_time_nanos, status, mut samples, sample_count) = {
        let manager = context.source().server().tick_rate_manager.read();
        let busy_time_nanos = manager.get_average_tick_time_nanos();
        let status = if manager.is_sprinting() {
            TickStatus::Sprinting
        } else if manager.is_frozen() {
            TickStatus::Frozen
        } else if manager.nanoseconds_per_tick < busy_time_nanos {
            TickStatus::Lagging
        } else {
            TickStatus::Running
        };
        (
            manager.tick_rate(),
            busy_time_nanos,
            manager.nanoseconds_per_tick,
            status,
            manager.get_tick_times_nanos(),
            manager.get_sample_count(),
        )
    };

    let tick_rate_string = format!("{tick_rate:.1}");
    let busy_time = nanos_to_millis_string(busy_time_nanos);
    let status_message = match status {
        TickStatus::Sprinting => None,
        TickStatus::Frozen => Some(&translations::COMMANDS_TICK_STATUS_FROZEN),
        TickStatus::Lagging => Some(&translations::COMMANDS_TICK_STATUS_LAGGING),
        TickStatus::Running => Some(&translations::COMMANDS_TICK_STATUS_RUNNING),
    };
    if let Some(status_message) = status_message {
        context
            .source()
            .send_success(&TextComponent::from(status_message), false);
        let message = translations::COMMANDS_TICK_QUERY_RATE_RUNNING
            .message([
                tick_rate_string,
                busy_time,
                nanos_to_millis_string(target_time_nanos),
            ])
            .component();
        context.source().send_success(&message, false);
    } else {
        context.source().send_success(
            &TextComponent::from(&translations::COMMANDS_TICK_STATUS_SPRINTING),
            false,
        );
        let message = translations::COMMANDS_TICK_QUERY_RATE_SPRINTING
            .message([tick_rate_string, busy_time])
            .component();
        context.source().send_success(&message, false);
    }

    samples[..sample_count].sort_unstable();
    let percentile = |numerator: usize| {
        if sample_count == 0 {
            return "0.0".to_owned();
        }
        let index = (sample_count * numerator / 100).min(sample_count - 1);
        nanos_to_millis_string(samples[index])
    };
    let message = translations::COMMANDS_TICK_QUERY_PERCENTILES
        .message([
            percentile(50),
            percentile(95),
            percentile(99),
            sample_count.to_string(),
        ])
        .component();
    context.source().send_success(&message, false);
    Ok(tick_rate as i32)
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "the bounded tick rate intentionally returns its truncated command result"
)]
fn set_tick_rate(context: &SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError> {
    let Some(rate) = context.float("rate") else {
        return Err(missing_argument("rate"));
    };
    context
        .source()
        .server()
        .tick_rate_manager
        .write()
        .set_tick_rate(rate);
    context.source().server().broadcast_ticking_state();

    let message = translations::COMMANDS_TICK_RATE_SUCCESS
        .message([format!("{rate:.1}")])
        .component();
    context.source().send_success(&message, true);
    Ok(rate as i32)
}

#[expect(
    clippy::unnecessary_wraps,
    reason = "Command executors use a shared fallible callback signature."
)]
fn set_frozen(
    context: &SteelCommandContext<CommandSource>,
    frozen: bool,
) -> Result<i32, CommandSyntaxError> {
    let (sprint_report, stopped_step) = {
        let mut manager = context.source().server().tick_rate_manager.write();
        let sprint_report = if frozen {
            manager.stop_sprinting()
        } else {
            None
        };
        let stopped_step = frozen && manager.stop_stepping();
        manager.set_frozen(frozen);
        (sprint_report, stopped_step)
    };

    if let Some(report) = sprint_report {
        context.source().server().broadcast_sprint_report(&report);
    }
    if stopped_step {
        context.source().server().broadcast_ticking_step();
    }
    context.source().server().broadcast_ticking_state();

    let status = if frozen {
        &translations::COMMANDS_TICK_STATUS_FROZEN
    } else {
        &translations::COMMANDS_TICK_STATUS_RUNNING
    };
    context
        .source()
        .send_success(&TextComponent::from(status), true);
    Ok(i32::from(frozen))
}

#[expect(
    clippy::unnecessary_wraps,
    reason = "Command executors use a shared fallible callback signature."
)]
fn step(
    context: &SteelCommandContext<CommandSource>,
    ticks: i32,
) -> Result<i32, CommandSyntaxError> {
    let success = context
        .source()
        .server()
        .tick_rate_manager
        .write()
        .step_game_if_paused(ticks);
    if success {
        context.source().server().broadcast_ticking_step();
        let message = translations::COMMANDS_TICK_STEP_SUCCESS
            .message([ticks.to_string()])
            .component();
        context.source().send_success(&message, true);
    } else {
        context
            .source()
            .send_failure(TextComponent::from(&translations::COMMANDS_TICK_STEP_FAIL));
    }
    Ok(1)
}

#[expect(
    clippy::unnecessary_wraps,
    reason = "Command executors use a shared fallible callback signature."
)]
fn stop_step(context: &SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError> {
    let stopped = context
        .source()
        .server()
        .tick_rate_manager
        .write()
        .stop_stepping();
    if stopped {
        context.source().server().broadcast_ticking_step();
        context.source().send_success(
            &TextComponent::from(&translations::COMMANDS_TICK_STEP_STOP_SUCCESS),
            true,
        );
        Ok(1)
    } else {
        context.source().send_failure(TextComponent::from(
            &translations::COMMANDS_TICK_STEP_STOP_FAIL,
        ));
        Ok(0)
    }
}

#[expect(
    clippy::unnecessary_wraps,
    reason = "Command executors use a shared fallible callback signature."
)]
fn sprint(
    context: &SteelCommandContext<CommandSource>,
    ticks: i32,
) -> Result<i32, CommandSyntaxError> {
    let interrupted = context
        .source()
        .server()
        .tick_rate_manager
        .write()
        .request_game_to_sprint(ticks);
    context.source().server().broadcast_ticking_state();
    if interrupted {
        context.source().send_success(
            &TextComponent::from(&translations::COMMANDS_TICK_SPRINT_STOP_SUCCESS),
            true,
        );
    }
    context.source().send_success(
        &TextComponent::from(&translations::COMMANDS_TICK_STATUS_SPRINTING),
        true,
    );
    Ok(1)
}

#[expect(
    clippy::unnecessary_wraps,
    reason = "Command executors use a shared fallible callback signature."
)]
fn stop_sprint(context: &SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError> {
    let report = context
        .source()
        .server()
        .tick_rate_manager
        .write()
        .stop_sprinting();
    if let Some(report) = report {
        context.source().server().broadcast_sprint_report(&report);
        context.source().server().broadcast_ticking_state();
        context.source().send_success(
            &TextComponent::from(&translations::COMMANDS_TICK_SPRINT_STOP_SUCCESS),
            true,
        );
        Ok(1)
    } else {
        context.source().send_failure(TextComponent::from(
            &translations::COMMANDS_TICK_SPRINT_STOP_FAIL,
        ));
        Ok(0)
    }
}

fn missing_argument(name: &str) -> CommandSyntaxError {
    CommandSyntaxError::dynamic(format!(
        "Parsed `{name}` is missing from the tick command context"
    ))
}

#[cfg(test)]
mod tests {
    use super::super::create_dispatcher;
    use crate::command::{
        brigadier::{ArgumentType, CommandDispatcher, NodeId},
        execution::{CommandSource, SteelArgumentType, SteelCommandRuntime},
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
            panic!("child `{name}` should exist");
        };
        child
    }

    #[test]
    fn tick_graph_matches_the_target_shape_and_permissions() {
        init_test_registry();
        let Ok(dispatcher) = create_dispatcher() else {
            panic!("built-in commands should register");
        };
        let tick = child(&dispatcher, dispatcher.root(), "tick");
        let Some(children) = dispatcher.children(tick) else {
            panic!("tick root should exist");
        };
        let names = children
            .iter()
            .map(|child| {
                let Some(node) = dispatcher.node(*child) else {
                    panic!("tick child should exist");
                };
                node.name()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            ["query", "rate", "step", "sprint", "unfreeze", "freeze"]
        );

        let query = child(&dispatcher, tick, "query");
        assert!(
            dispatcher
                .node(query)
                .is_some_and(|node| node.is_executable() && node.is_restricted())
        );
        for name in ["rate", "step", "sprint", "unfreeze", "freeze"] {
            let node = child(&dispatcher, tick, name);
            let Some(node) = dispatcher.node(node) else {
                panic!("tick subcommand should exist");
            };
            assert!(node.is_restricted());
        }

        let rate = child(&dispatcher, tick, "rate");
        let rate = child(&dispatcher, rate, "rate");
        assert_eq!(
            dispatcher.node(rate).and_then(|node| node.argument_type()),
            Some(&SteelArgumentType::from(ArgumentType::float(1.0, 10_000.0)))
        );

        for parent in ["step", "sprint"] {
            let parent = child(&dispatcher, tick, parent);
            let time = child(&dispatcher, parent, "time");
            assert_eq!(
                dispatcher.node(time).and_then(|node| node.argument_type()),
                Some(&SteelArgumentType::time(1))
            );
        }
    }
}
