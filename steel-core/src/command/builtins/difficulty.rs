use steel_utils::{Identifier, translations, types::Difficulty};
use text_components::{TextComponent, translation::Translation};

use super::super::{
    brigadier::{CommandNodeBuilder, CommandSyntaxError},
    execution::{CommandSource, SteelCommandContext, SteelCommandRuntime, literal},
    registration::CommandRegistration,
};
use crate::permission::{PermissionContext, PermissionExpr, PermissionKey, PermissionKeyError};
use crate::player::Player;
use crate::world::World;

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("difficulty"), |_| command())
}

pub(crate) fn player_can_change_difficulty(player: &Player, world: &World) -> bool {
    let permission = match difficulty_permission() {
        Ok(permission) => permission,
        Err(error) => {
            log::error!("invalid built-in difficulty permission key: {error}");
            return false;
        }
    };
    let context = PermissionContext::for_world(world.key.clone());
    player.has_permission_in(&permission, &context)
}

fn difficulty_permission() -> Result<PermissionExpr, PermissionKeyError> {
    PermissionKey::parse("minecraft.command.difficulty").map(PermissionExpr::key)
}

fn command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("difficulty")
        .executes(query_difficulty)
        .then(difficulty_literal("peaceful", Difficulty::Peaceful))
        .then(difficulty_literal("easy", Difficulty::Easy))
        .then(difficulty_literal("normal", Difficulty::Normal))
        .then(difficulty_literal("hard", Difficulty::Hard))
}

fn difficulty_literal(
    name: &'static str,
    difficulty: Difficulty,
) -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal(name).executes(move |context| set_difficulty(context, difficulty))
}

#[expect(
    clippy::unnecessary_wraps,
    reason = "Command executors use a shared fallible callback signature."
)]
fn query_difficulty(
    context: &SteelCommandContext<CommandSource>,
) -> Result<i32, CommandSyntaxError> {
    let difficulty = context.source().world().difficulty();
    let message = translations::COMMANDS_DIFFICULTY_QUERY
        .message([TextComponent::from(difficulty_display_name(difficulty))])
        .component();
    context.source().send_success(&message, false);
    Ok(i32::from(u8::from(difficulty)))
}

fn set_difficulty(
    context: &SteelCommandContext<CommandSource>,
    difficulty: Difficulty,
) -> Result<i32, CommandSyntaxError> {
    let domain = context.source().world().domain();
    let worlds = context.source().server().worlds.worlds_in_domain(domain);
    if worlds.iter().all(|world| world.difficulty() == difficulty) {
        return Err(CommandSyntaxError::dynamic(
            translations::COMMANDS_DIFFICULTY_FAILURE
                .message([TextComponent::from(difficulty_display_name(difficulty))])
                .component(),
        ));
    }

    for world in worlds {
        world.set_difficulty(difficulty);
    }

    let message = translations::COMMANDS_DIFFICULTY_SUCCESS
        .message([TextComponent::from(difficulty_display_name(difficulty))])
        .component();
    context.source().send_success(&message, true);
    Ok(0)
}

const fn difficulty_display_name(difficulty: Difficulty) -> &'static Translation<0> {
    match difficulty {
        Difficulty::Peaceful => &translations::OPTIONS_DIFFICULTY_PEACEFUL,
        Difficulty::Easy => &translations::OPTIONS_DIFFICULTY_EASY,
        Difficulty::Normal => &translations::OPTIONS_DIFFICULTY_NORMAL,
        Difficulty::Hard => &translations::OPTIONS_DIFFICULTY_HARD,
    }
}

#[cfg(test)]
mod tests {
    use super::difficulty_permission;
    use crate::permission::{PermissionEntry, PermissionKey, PermissionSet};

    #[test]
    fn client_difficulty_permission_uses_the_command_root() {
        let permission = difficulty_permission().expect("permission should build");
        let allowed = PermissionSet::from_entries([PermissionEntry::allow(
            PermissionKey::parse("minecraft.command.difficulty")
                .expect("test permission should parse"),
        )]);
        let gamemode_only = PermissionSet::from_entries([PermissionEntry::allow(
            PermissionKey::parse("minecraft.command.gamemode")
                .expect("test permission should parse"),
        )]);

        assert!(allowed.allows(&permission));
        assert!(!gamemode_only.allows(&permission));
    }
}
