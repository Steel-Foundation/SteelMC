//! Handler for the "flyspeed" command.
use crate::command::arguments::float::FloatArgument;
use crate::command::commands::{
    CommandExecutor, CommandHandlerBuilder, CommandHandlerDyn, argument,
};
use crate::command::context::CommandContext;
use crate::command::error::CommandError;
use text_components::TextComponent;

/// Handler for the "flyspeed" command.
#[must_use]
pub fn command_handler() -> impl CommandHandlerDyn {
    CommandHandlerBuilder::new(
        &["flyspeed"],
        "Sets the player's flying speed.",
        "minecraft:command.flyspeed",
    )
    .executes(FlySpeedQueryExecutor)
    .then(
        argument("speed", FloatArgument::bounded(Some(0.0), Some(10.0))).executes(FlySpeedExecutor),
    )
}

struct FlySpeedQueryExecutor;

impl CommandExecutor<()> for FlySpeedQueryExecutor {
    fn execute(&self, _args: (), context: &mut CommandContext) -> Result<(), CommandError> {
        let player = context
            .sender
            .get_player()
            .ok_or(CommandError::InvalidRequirement)?;

        let speed = player.get_flying_speed();
        let multiplier = speed / 0.05; // Show as multiplier of default speed

        context.sender.send_message(&TextComponent::from(format!(
            "Current flying speed: {speed:.3} ({multiplier:.1}x)"
        )));

        Ok(())
    }
}

struct FlySpeedExecutor;

impl CommandExecutor<((), f32)> for FlySpeedExecutor {
    fn execute(&self, args: ((), f32), context: &mut CommandContext) -> Result<(), CommandError> {
        let ((), speed) = args;

        let player = context
            .sender
            .get_player()
            .ok_or(CommandError::InvalidRequirement)?;

        player.set_flying_speed(speed);
        player.send_abilities();

        let multiplier = speed / 0.05;
        context.sender.send_message(&TextComponent::from(format!(
            "Set flying speed to {speed:.3} ({multiplier:.1}x)"
        )));

        Ok(())
    }
}
