//! Dimension switching commands: /overworld, /nether, /end

use crate::command::commands::{CommandExecutor, CommandHandlerBuilder, CommandHandlerDyn};
use crate::command::context::CommandContext;
use crate::command::error::CommandError;
use crate::portal::TeleportTransition;

// ---------- /overworld ----------

/// Handler for the "overworld" command.
#[must_use]
pub fn overworld_command_handler() -> impl CommandHandlerDyn {
    CommandHandlerBuilder::new(
        &["overworld"],
        "Teleports the player to the overworld.",
        "minecraft:command.overworld",
    )
    .executes(OverworldExecutor)
}

struct OverworldExecutor;

impl CommandExecutor<()> for OverworldExecutor {
    fn execute(&self, _args: (), context: &mut CommandContext) -> Result<(), CommandError> {
        let player = context.player.as_ref().unwrap().clone();
        let pos = *player.position.lock();
        let rot = player.rotation.load();
        let target = context.server.overworld().clone();
        context.server.queue_dimension_change(
            player,
            TeleportTransition {
                target_world: target,
                position: pos,
                rotation: rot,
                portal_cooldown: 0,
            },
        );
        Ok(())
    }
}

// ---------- /nether ----------

/// Handler for the "nether" command.
#[must_use]
pub fn nether_command_handler() -> impl CommandHandlerDyn {
    CommandHandlerBuilder::new(
        &["nether"],
        "Teleports the player to the nether.",
        "minecraft:command.nether",
    )
    .executes(NetherExecutor)
}

struct NetherExecutor;

impl CommandExecutor<()> for NetherExecutor {
    fn execute(&self, _args: (), context: &mut CommandContext) -> Result<(), CommandError> {
        let Some(target) = context.server.nether().cloned() else {
            return Err(CommandError::CommandFailed(Box::new(
                "Nether is not available.".into(),
            )));
        };
        let player = context.player.as_ref().unwrap().clone();
        let mut pos = *player.position.lock();
        pos.y = 10.0;
        let rot = player.rotation.load();
        context.server.queue_dimension_change(
            player,
            TeleportTransition {
                target_world: target,
                position: pos,
                rotation: rot,
                portal_cooldown: 0,
            },
        );
        Ok(())
    }
}

// ---------- /end ----------

/// Handler for the "end" command.
#[must_use]
pub fn end_command_handler() -> impl CommandHandlerDyn {
    CommandHandlerBuilder::new(
        &["end"],
        "Teleports the player to the end.",
        "minecraft:command.end",
    )
    .executes(EndExecutor)
}

struct EndExecutor;

impl CommandExecutor<()> for EndExecutor {
    fn execute(&self, _args: (), context: &mut CommandContext) -> Result<(), CommandError> {
        let Some(target) = context.server.the_end().cloned() else {
            return Err(CommandError::CommandFailed(Box::new(
                "The End is not available.".into(),
            )));
        };
        let player = context.player.as_ref().unwrap().clone();
        let mut pos = *player.position.lock();
        pos.y = 10.0;
        let rot = player.rotation.load();
        context.server.queue_dimension_change(
            player,
            TeleportTransition {
                target_world: target,
                position: pos,
                rotation: rot,
                portal_cooldown: 0,
            },
        );
        Ok(())
    }
}
