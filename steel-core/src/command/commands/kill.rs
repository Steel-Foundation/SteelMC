//! Handler for the "kill" command.
//! Mirrors `net.minecraft.server.commands.KillCommand`.

use std::sync::Arc;

use text_components::TextComponent;

use crate::command::arguments::entity::EntityArgument;
use crate::command::commands::{
    CommandExecutor, CommandHandlerBuilder, CommandHandlerDyn, argument,
};
use crate::command::context::CommandContext;
use crate::command::error::CommandError;
use crate::entity::LivingEntity;
use crate::entity::damage::DamageSource;
use crate::player::Player;
use steel_registry::vanilla_damage_types;
use steel_utils::translations;

/// Creates the `/kill` command handler.
#[must_use]
pub fn command_handler() -> impl CommandHandlerDyn {
    CommandHandlerBuilder::new(&["kill"], "Kills entities.", "minecraft:command.kill")
        .executes(KillSelfExecutor)
        .then(argument("targets", EntityArgument::multiple()).executes(KillTargetsExecutor))
}

/// `LivingEntity.kill()` — hurt with `genericKill` at `Float.MAX_VALUE`.
fn kill_player(player: &Player) {
    player.hurt(
        &DamageSource::environment(vanilla_damage_types::GENERIC_KILL),
        f32::MAX,
    );
}

struct KillSelfExecutor;

impl CommandExecutor<()> for KillSelfExecutor {
    fn execute(&self, _args: (), context: &mut CommandContext) -> Result<(), CommandError> {
        let player = context
            .sender
            .get_player()
            .ok_or(CommandError::InvalidRequirement)?;

        kill_player(player);

        // TODO: use getDisplayName() (team formatting, hover event, UUID insertion)
        context.sender.send_message(
            &translations::COMMANDS_KILL_SUCCESS_SINGLE
                .message([TextComponent::plain(player.gameprofile.name.clone())])
                .into(),
        );

        Ok(())
    }
}

struct KillTargetsExecutor;

impl CommandExecutor<((), Vec<Arc<dyn LivingEntity + Send + Sync>>)> for KillTargetsExecutor {
    fn execute(
        &self,
        args: ((), Vec<Arc<dyn LivingEntity + Send + Sync>>),
        context: &mut CommandContext,
    ) -> Result<(), CommandError> {
        let ((), targets) = args;

        if targets.is_empty() {
            return Err(CommandError::CommandFailed(Box::new(
                TextComponent::const_plain("No entity was found"),
            )));
        }

        let victim_count = targets.len();
        let players = context.server.get_players();

        let mut last_name = String::new();
        for target in &targets {
            let target_id = target.id();
            if let Some(player) = players.iter().find(|p| p.id == target_id) {
                kill_player(player);
                last_name.clone_from(&player.gameprofile.name);
            }
            // TODO: non-player entities via Entity::kill() (remove with RemovalReason::KILLED)
        }

        // TODO: use getDisplayName() (team formatting, hover event, UUID insertion)
        if victim_count == 1 {
            context.sender.send_message(
                &translations::COMMANDS_KILL_SUCCESS_SINGLE
                    .message([TextComponent::plain(last_name)])
                    .into(),
            );
        } else {
            context.sender.send_message(
                &translations::COMMANDS_KILL_SUCCESS_MULTIPLE
                    .message([TextComponent::plain(victim_count.to_string())])
                    .into(),
            );
        }

        Ok(())
    }
}
