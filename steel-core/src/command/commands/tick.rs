//! Handler for the "tick" command.
use steel_utils::translations;
use text_components::TextComponent;

use crate::command::arguments::float::FloatArgument;
use crate::command::arguments::time::TimeArgument;
use crate::command::commands::{
    CommandExecutor, CommandHandlerBuilder, CommandHandlerDyn, argument, literal,
};
use crate::command::context::CommandContext;
use crate::command::error::CommandError;

/// Handler for the "tick" command.
#[must_use]
pub fn command_handler() -> impl CommandHandlerDyn {
    CommandHandlerBuilder::new(
        &["tick"],
        "Controls server tick rate.",
        "minecraft:command.tick",
    )
    // /tick query
    .then(literal("query").executes(TickQueryExecutor))
    // /tick rate <rate>
    .then(
        literal("rate").then(
            argument("rate", FloatArgument::bounded(Some(1.0), Some(10000.0)))
                .executes(TickRateExecutor),
        ),
    )
    // /tick freeze
    .then(literal("freeze").executes(TickFreezeExecutor))
    // /tick unfreeze
    .then(literal("unfreeze").executes(TickUnfreezeExecutor))
    // /tick step [time] | /tick step stop
    .then(
        literal("step")
            .executes(TickStepDefaultExecutor)
            .then(literal("stop").executes(TickStepStopExecutor))
            .then(argument("time", TimeArgument).executes(TickStepExecutor)),
    )
    // /tick sprint <time> | /tick sprint stop
    .then(
        literal("sprint")
            .then(literal("stop").executes(TickSprintStopExecutor))
            .then(argument("time", TimeArgument).executes(TickSprintExecutor)),
    )
}

/// Converts nanoseconds to a formatted millisecond string.
fn nanos_to_ms_string(nanos: u64) -> String {
    format!("{:.1}", nanos as f64 / 1_000_000.0)
}

// /tick query
struct TickQueryExecutor;
impl CommandExecutor<()> for TickQueryExecutor {
    fn execute(&self, _args: (), context: &mut CommandContext) -> Result<(), CommandError> {
        let tick_manager = context.server.tick_rate_manager.read();

        let tick_rate = tick_manager.tick_rate();
        let busy_time_nanos = tick_manager.get_average_tick_time_nanos();
        let busy_time = nanos_to_ms_string(busy_time_nanos);
        let tick_rate_string = format!("{tick_rate:.1}");

        // Send status and rate info based on current state
        if tick_manager.is_sprinting() {
            context
                .sender
                .send_message(&translations::COMMANDS_TICK_STATUS_SPRINTING.msg().into());
            context.sender.send_message(
                &translations::COMMANDS_TICK_QUERY_RATE_SPRINTING
                    .message([
                        TextComponent::from(tick_rate_string),
                        TextComponent::from(busy_time),
                    ])
                    .into(),
            );
        } else {
            // Determine status
            if tick_manager.is_frozen() {
                context
                    .sender
                    .send_message(&translations::COMMANDS_TICK_STATUS_FROZEN.msg().into());
            } else if tick_manager.nanoseconds_per_tick < busy_time_nanos {
                context
                    .sender
                    .send_message(&translations::COMMANDS_TICK_STATUS_LAGGING.msg().into());
            } else {
                context
                    .sender
                    .send_message(&translations::COMMANDS_TICK_STATUS_RUNNING.msg().into());
            }

            let target_mspt = nanos_to_ms_string(tick_manager.nanoseconds_per_tick);
            context.sender.send_message(
                &translations::COMMANDS_TICK_QUERY_RATE_RUNNING
                    .message([
                        TextComponent::from(tick_rate_string),
                        TextComponent::from(busy_time),
                        TextComponent::from(target_mspt),
                    ])
                    .into(),
            );
        }

        // Get percentiles (vanilla sorts and calculates from the raw samples)
        let mut samples = tick_manager.get_tick_times_nanos();
        let sample_count = tick_manager.get_sample_count();
        drop(tick_manager);

        samples[..sample_count].sort_unstable();

        let p50 = if sample_count > 0 {
            nanos_to_ms_string(samples[sample_count / 2])
        } else {
            "0.0".to_string()
        };
        let p95 = if sample_count > 0 {
            nanos_to_ms_string(samples[(sample_count as f64 * 0.95) as usize])
        } else {
            "0.0".to_string()
        };
        let p99 = if sample_count > 0 {
            nanos_to_ms_string(samples[(sample_count as f64 * 0.99) as usize])
        } else {
            "0.0".to_string()
        };

        context.sender.send_message(
            &translations::COMMANDS_TICK_QUERY_PERCENTILES
                .message([
                    TextComponent::from(p50),
                    TextComponent::from(p95),
                    TextComponent::from(p99),
                    TextComponent::from(format!("{sample_count}")),
                ])
                .into(),
        );

        Ok(())
    }
}

// /tick rate <rate>
struct TickRateExecutor;
impl CommandExecutor<((), f32)> for TickRateExecutor {
    fn execute(&self, args: ((), f32), context: &mut CommandContext) -> Result<(), CommandError> {
        let ((), rate) = args;

        context.server.broadcast_ticking_state();
        context.server.tick_rate_manager.write().set_tick_rate(rate);

        let rate_string = format!("{rate:.1}");
        context.sender.send_message(
            &translations::COMMANDS_TICK_RATE_SUCCESS
                .message([TextComponent::from(rate_string)])
                .into(),
        );

        Ok(())
    }
}

// /tick freeze
struct TickFreezeExecutor;
impl CommandExecutor<()> for TickFreezeExecutor {
    fn execute(&self, _args: (), context: &mut CommandContext) -> Result<(), CommandError> {
        let mut tick_manager = context.server.tick_rate_manager.write();

        // Stop sprinting if active (vanilla behavior)
        if tick_manager.is_sprinting() {
            tick_manager.stop_sprinting();
        }

        // Stop stepping if active (vanilla behavior)
        if tick_manager.is_stepping_forward() {
            tick_manager.stop_stepping();
        }

        tick_manager.set_frozen(true);
        drop(tick_manager);

        context.server.broadcast_ticking_state();

        context
            .sender
            .send_message(&translations::COMMANDS_TICK_STATUS_FROZEN.msg().into());

        Ok(())
    }
}

// /tick unfreeze
struct TickUnfreezeExecutor;
impl CommandExecutor<()> for TickUnfreezeExecutor {
    fn execute(&self, _args: (), context: &mut CommandContext) -> Result<(), CommandError> {
        context.server.tick_rate_manager.write().set_frozen(false);
        context.server.broadcast_ticking_state();

        context
            .sender
            .send_message(&translations::COMMANDS_TICK_STATUS_RUNNING.msg().into());

        Ok(())
    }
}

// /tick step (default 1 tick)
struct TickStepDefaultExecutor;
impl CommandExecutor<()> for TickStepDefaultExecutor {
    fn execute(&self, _args: (), context: &mut CommandContext) -> Result<(), CommandError> {
        step_impl(1, context)
    }
}

// /tick step <time>
struct TickStepExecutor;
impl CommandExecutor<((), i32)> for TickStepExecutor {
    fn execute(&self, args: ((), i32), context: &mut CommandContext) -> Result<(), CommandError> {
        let ((), ticks) = args;
        step_impl(ticks, context)
    }
}

fn step_impl(ticks: i32, context: &mut CommandContext) -> Result<(), CommandError> {
    let success = context
        .server
        .tick_rate_manager
        .write()
        .step_game_if_paused(ticks);

    if success {
        context.server.broadcast_ticking_step();
        context.sender.send_message(
            &translations::COMMANDS_TICK_STEP_SUCCESS
                .message([TextComponent::from(format!("{ticks}"))])
                .into(),
        );
        Ok(())
    } else {
        Err(CommandError::CommandFailed(Box::new(
            translations::COMMANDS_TICK_STEP_FAIL.msg().into(),
        )))
    }
}

// /tick step stop
struct TickStepStopExecutor;
impl CommandExecutor<()> for TickStepStopExecutor {
    fn execute(&self, _args: (), context: &mut CommandContext) -> Result<(), CommandError> {
        let stopped = context.server.tick_rate_manager.write().stop_stepping();

        if stopped {
            context.server.broadcast_ticking_step();
            context
                .sender
                .send_message(&translations::COMMANDS_TICK_STEP_STOP_SUCCESS.msg().into());
            Ok(())
        } else {
            Err(CommandError::CommandFailed(Box::new(
                translations::COMMANDS_TICK_STEP_STOP_FAIL.msg().into(),
            )))
        }
    }
}

// /tick sprint <time>
struct TickSprintExecutor;
impl CommandExecutor<((), i32)> for TickSprintExecutor {
    fn execute(&self, args: ((), i32), context: &mut CommandContext) -> Result<(), CommandError> {
        let ((), ticks) = args;

        let interrupted = context
            .server
            .tick_rate_manager
            .write()
            .request_game_to_sprint(ticks);

        // Broadcast state change (unfrozen during sprint)
        context.server.broadcast_ticking_state();

        if interrupted {
            context
                .sender
                .send_message(&translations::COMMANDS_TICK_SPRINT_STOP_SUCCESS.msg().into());
        }

        context
            .sender
            .send_message(&translations::COMMANDS_TICK_STATUS_SPRINTING.msg().into());

        Ok(())
    }
}

// /tick sprint stop
struct TickSprintStopExecutor;
impl CommandExecutor<()> for TickSprintStopExecutor {
    fn execute(&self, _args: (), context: &mut CommandContext) -> Result<(), CommandError> {
        let report = context.server.tick_rate_manager.write().stop_sprinting();

        if let Some(report) = report {
            // Broadcast state change (restored previous frozen state)
            context.server.broadcast_ticking_state();

            // Send sprint report
            context.sender.send_message(
                &translations::COMMANDS_TICK_SPRINT_REPORT
                    .message([
                        TextComponent::from(format!("{}", report.ticks_per_second)),
                        TextComponent::from(format!("{:.2}", report.ms_per_tick)),
                    ])
                    .into(),
            );
            Ok(())
        } else {
            Err(CommandError::CommandFailed(Box::new(
                translations::COMMANDS_TICK_SPRINT_STOP_FAIL.msg().into(),
            )))
        }
    }
}
