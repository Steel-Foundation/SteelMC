//! Player game-mode command and permission projection.

use std::{slice, sync::Arc};

use steel_registry::game_rules::GameRuleValue;
use steel_registry::vanilla_game_rules::SEND_COMMAND_FEEDBACK;
use steel_utils::{Identifier, translations, types::GameType};
use text_components::{TextComponent, translation::Translation};

use super::super::{
    brigadier::{CommandNodeBuilder, CommandSyntaxError},
    execution::{
        CommandPermissionSource, CommandSource, SteelArgumentType, SteelCommandContext,
        SteelCommandRuntime, argument, literal,
    },
    registration::{CommandRegistration, CommandRegistrationError},
};
use crate::command::sender::CommandSender;
use crate::entity::Entity;
use crate::permission::{
    PermissionContext, PermissionExpr, PermissionKey, PermissionKeyError, PermissionSegment,
};
use crate::player::Player;
use crate::server::Server;
use crate::world::World;

const GAME_MODES: [GameType; 4] = [
    GameType::Survival,
    GameType::Creative,
    GameType::Adventure,
    GameType::Spectator,
];

pub(super) fn registration() -> Result<CommandRegistration<CommandSource>, CommandRegistrationError>
{
    let permission = visible_permission().map_err(|source| {
        CommandRegistrationError::InvalidExplicitPermission {
            id: Identifier::vanilla_static("gamemode"),
            source,
        }
    })?;
    Ok(
        CommandRegistration::new(Identifier::vanilla_static("gamemode"), |_| command())
            .permission(permission),
    )
}

fn command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("gamemode").then(
        argument("gamemode", SteelArgumentType::game_mode())
            .executes(set_own_game_mode)
            .then(argument("target", SteelArgumentType::players()).executes(set_target_game_mode)),
    )
}

fn set_own_game_mode(
    context: &SteelCommandContext<CommandSource>,
) -> Result<i32, CommandSyntaxError> {
    let game_mode = required_game_mode(context)?;
    require_game_mode_permission(context.source(), game_mode)?;
    let Some(player) = context.source().player() else {
        return Err(CommandSyntaxError::dynamic(TextComponent::from(
            &translations::PERMISSIONS_REQUIRES_PLAYER,
        )));
    };
    set_game_mode(context.source(), slice::from_ref(player), game_mode)
}

fn set_target_game_mode(
    context: &SteelCommandContext<CommandSource>,
) -> Result<i32, CommandSyntaxError> {
    let game_mode = required_game_mode(context)?;
    require_game_mode_permission(context.source(), game_mode)?;
    let targets = context.players("target")?;
    set_game_mode(context.source(), &targets, game_mode)
}

fn required_game_mode(
    context: &SteelCommandContext<CommandSource>,
) -> Result<GameType, CommandSyntaxError> {
    context.game_mode("gamemode").ok_or_else(|| {
        CommandSyntaxError::dynamic("Parsed gamemode is missing from the command context")
    })
}

fn require_game_mode_permission(
    source: &CommandSource,
    game_mode: GameType,
) -> Result<(), CommandSyntaxError> {
    if source_can_change_game_mode(source, game_mode) {
        return Ok(());
    }
    Err(CommandSyntaxError::dynamic(format!(
        "You do not have permission to use game mode {}",
        game_mode.name()
    )))
}

fn set_game_mode(
    source: &CommandSource,
    targets: &[Arc<Player>],
    game_mode: GameType,
) -> Result<i32, CommandSyntaxError> {
    let mut changed = 0usize;
    let send_target_feedback =
        source.world().get_game_rule(&SEND_COMMAND_FEEDBACK) == GameRuleValue::Bool(true);

    for target in targets {
        if !target.set_game_mode(game_mode) {
            continue;
        }
        changed += 1;

        if source
            .entity()
            .is_some_and(|entity| entity.uuid() == target.uuid())
        {
            let message = translations::COMMANDS_GAMEMODE_SUCCESS_SELF
                .message([TextComponent::from(game_mode_translation(game_mode))])
                .component();
            source.send_success(&message, true);
            continue;
        }

        if send_target_feedback {
            let message = translations::GAME_MODE_CHANGED
                .message([TextComponent::from(game_mode_translation(game_mode))])
                .component();
            target.send_message(&message);
        }
        let message = translations::COMMANDS_GAMEMODE_SUCCESS_OTHER
            .message([
                TextComponent::plain(target.plain_text_name()),
                TextComponent::from(game_mode_translation(game_mode)),
            ])
            .component();
        source.send_success(&message, true);
    }

    i32::try_from(changed).map_err(|_| {
        CommandSyntaxError::dynamic("Changed player count exceeds the command result range")
    })
}

pub(crate) fn handle_client_request(
    player: &Arc<Player>,
    server: &Arc<Server>,
    game_mode: GameType,
) {
    let world = player.get_world();
    if !player_can_change_game_mode(player, &world, game_mode) {
        log::warn!(
            "Player {} tried to change game mode to {} without permission",
            player.gameprofile.name,
            game_mode.name()
        );
        return;
    }

    let source = CommandSource::new(
        CommandSender::Player(Arc::clone(player)),
        Arc::clone(server),
    );
    if let Err(error) = set_game_mode(&source, slice::from_ref(player), game_mode) {
        log::error!(
            "Failed to apply client game-mode change for {}: {error}",
            player.gameprofile.name
        );
    }
}

pub(crate) fn player_can_use_client_switcher(player: &Player, world: &World) -> bool {
    any_game_mode_allowed(|game_mode| player_can_change_game_mode(player, world, game_mode))
}

fn any_game_mode_allowed(mut allows_game_mode: impl FnMut(GameType) -> bool) -> bool {
    GAME_MODES.into_iter().any(&mut allows_game_mode)
}

fn source_can_change_game_mode(source: &CommandSource, game_mode: GameType) -> bool {
    let permission = match game_mode_permission(game_mode) {
        Ok(permission) => permission,
        Err(error) => {
            log::error!(
                "invalid built-in gamemode permission key for {}: {error}",
                game_mode.name()
            );
            return false;
        }
    };
    source.has_permission(&permission)
}

fn player_can_change_game_mode(player: &Player, world: &World, game_mode: GameType) -> bool {
    let permission = match game_mode_permission(game_mode) {
        Ok(permission) => permission,
        Err(error) => {
            log::error!(
                "invalid built-in gamemode permission key for {}: {error}",
                game_mode.name()
            );
            return false;
        }
    };
    let context = PermissionContext::for_world(world.key.clone());
    player.has_permission_in(&permission, &context)
}

fn visible_permission() -> Result<PermissionExpr, PermissionKeyError> {
    GAME_MODES
        .into_iter()
        .map(game_mode_permission)
        .collect::<Result<Vec<_>, _>>()
        .map(PermissionExpr::Any)
}

fn game_mode_permission(game_mode: GameType) -> Result<PermissionExpr, PermissionKeyError> {
    let root = PermissionKey::parse("minecraft.command.gamemode")?;
    let segment = PermissionSegment::parse(game_mode.name())?;
    let mode = root.child(&segment)?;
    Ok(PermissionExpr::scoped_key(root, mode))
}

const fn game_mode_translation(game_mode: GameType) -> &'static Translation<0> {
    match game_mode {
        GameType::Survival => &translations::GAME_MODE_SURVIVAL,
        GameType::Creative => &translations::GAME_MODE_CREATIVE,
        GameType::Adventure => &translations::GAME_MODE_ADVENTURE,
        GameType::Spectator => &translations::GAME_MODE_SPECTATOR,
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::test_support::init_test_registry;

    use super::super::create_dispatcher;
    use super::*;
    use crate::permission::{PermissionEntry, PermissionSet};

    fn permission_key(value: &str) -> PermissionKey {
        PermissionKey::parse(value).expect("test permission key should parse")
    }

    fn permissions(entries: impl IntoIterator<Item = PermissionEntry>) -> PermissionSet {
        PermissionSet::from_entries(entries)
    }

    fn allow(value: &str) -> PermissionEntry {
        PermissionEntry::allow(permission_key(value))
    }

    fn deny(value: &str) -> PermissionEntry {
        PermissionEntry::deny(permission_key(value))
    }

    fn allows_mode(permissions: &PermissionSet, game_mode: GameType) -> bool {
        permissions.allows(&game_mode_permission(game_mode).expect("permission should build"))
    }

    #[test]
    fn scoped_game_mode_permissions_preserve_root_and_child_rules() {
        let root = permissions([allow("minecraft.command.gamemode")]);
        assert!(GAME_MODES.into_iter().all(|mode| allows_mode(&root, mode)));

        let creative_only = permissions([allow("minecraft.command.gamemode.creative")]);
        assert!(allows_mode(&creative_only, GameType::Creative));
        assert!(!allows_mode(&creative_only, GameType::Survival));

        let creative_denied = permissions([
            allow("minecraft.command.gamemode"),
            deny("minecraft.command.gamemode.creative"),
        ]);
        assert!(!allows_mode(&creative_denied, GameType::Creative));
        assert!(allows_mode(&creative_denied, GameType::Survival));

        let visible = visible_permission().expect("visibility permission should build");
        assert!(creative_only.allows(&visible));

        let all_denied = permissions([
            allow("minecraft.command.gamemode"),
            deny("minecraft.command.gamemode.survival"),
            deny("minecraft.command.gamemode.creative"),
            deny("minecraft.command.gamemode.adventure"),
            deny("minecraft.command.gamemode.spectator"),
        ]);
        assert!(!all_denied.allows(&visible));
    }

    #[test]
    fn client_switcher_requires_at_least_one_allowed_mode() {
        let creative_only = permissions([allow("minecraft.command.gamemode.creative")]);
        assert!(any_game_mode_allowed(|mode| allows_mode(
            &creative_only,
            mode
        )));

        let none = PermissionSet::default();
        assert!(!any_game_mode_allowed(|mode| allows_mode(&none, mode)));

        let all_denied = permissions([
            allow("minecraft.command.gamemode"),
            deny("minecraft.command.gamemode.survival"),
            deny("minecraft.command.gamemode.creative"),
            deny("minecraft.command.gamemode.adventure"),
            deny("minecraft.command.gamemode.spectator"),
        ]);
        assert!(!any_game_mode_allowed(|mode| allows_mode(
            &all_denied,
            mode
        )));
    }

    #[test]
    fn gamemode_graph_matches_vanilla_shape() {
        init_test_registry();
        let Ok(dispatcher) = create_dispatcher() else {
            panic!("built-in commands should register");
        };
        let Some(root) = dispatcher.children(dispatcher.root()).and_then(|children| {
            children.iter().copied().find(|child| {
                dispatcher
                    .node(*child)
                    .is_some_and(|node| node.name() == "gamemode")
            })
        }) else {
            panic!("gamemode root should exist");
        };
        let Some(game_mode) = dispatcher
            .children(root)
            .and_then(|children| children.first())
            .copied()
        else {
            panic!("gamemode argument should exist");
        };
        assert!(matches!(
            dispatcher.node(game_mode),
            Some(node)
                if node.is_executable()
                    && node.argument_type() == Some(&SteelArgumentType::game_mode())
        ));
        let Some(target) = dispatcher
            .children(game_mode)
            .and_then(|children| children.first())
        else {
            panic!("target argument should exist");
        };
        assert!(matches!(
            dispatcher.node(*target),
            Some(node)
                if node.name() == "target"
                    && node.is_executable()
                    && node.argument_type() == Some(&SteelArgumentType::players())
        ));
    }
}
