use std::{collections::BTreeSet, sync::Arc};

use glam::DVec3;
use steel_registry::{
    game_rules::GameRuleValue,
    vanilla_game_rules::{MAX_COMMAND_FORKS, MAX_COMMAND_SEQUENCE_LENGTH},
    world_clock::WorldClockRef,
};
use text_components::{Modifier, TextComponent, format::Color};

use crate::{
    command::{
        brigadier::CommandSyntaxError,
        registration::{entity_selector_advanced_permission_expr, entity_selector_permission_expr},
        sender::CommandSender,
    },
    entity::{Entity as _, EntityAnchor, SharedEntity},
    permission::{
        OP_GROUP, PermissionContext, PermissionExpr, PermissionKey, PermissionMetadataExpression,
        PermissionRuleExpression, PermissionState,
    },
    player::{KnownPlayer, Player},
    scoreboard::Scoreboard,
    server::Server,
    world::World,
};

use super::{CommandExecutionContext, GameProfileArgument};

type CommandResultCallbackFn = dyn Fn(bool, i32) + Send + Sync;

/// A callback invoked after a terminal command returns or fails.
#[derive(Clone, Default)]
pub(crate) struct CommandResultCallback {
    callback: Option<Arc<CommandResultCallbackFn>>,
}

impl CommandResultCallback {
    pub(crate) fn new(callback: impl Fn(bool, i32) + Send + Sync + 'static) -> Self {
        Self {
            callback: Some(Arc::new(callback)),
        }
    }

    pub(crate) const fn empty() -> Self {
        Self { callback: None }
    }

    pub(crate) fn chain(first: Self, second: Self) -> Self {
        match (first.callback, second.callback) {
            (None, None) => Self::empty(),
            (Some(callback), None) | (None, Some(callback)) => Self {
                callback: Some(callback),
            },
            (Some(first), Some(second)) => Self::new(move |success, result| {
                first(success, result);
                second(success, result);
            }),
        }
    }

    pub(crate) fn on_result(&self, success: bool, result: i32) {
        if let Some(callback) = &self.callback {
            callback(success, result);
        }
    }
}

/// Read-only source data exposed to command argument parsers and suggestion providers.
pub(crate) trait CommandArgumentSource: Send + Sync {
    fn default_world_clock(&self) -> Option<WorldClockRef> {
        None
    }

    fn domain_exists(&self, _domain: &str) -> bool {
        false
    }

    fn domain_names(&self) -> Vec<&str> {
        Vec::new()
    }

    fn command_world_names(&self) -> Vec<String> {
        Vec::new()
    }

    fn permission_context_world_names(&self) -> Vec<String> {
        Vec::new()
    }

    fn command_storage_keys(&self) -> Vec<String> {
        Vec::new()
    }

    fn permission_rule_suggestions(&self) -> Vec<String> {
        Vec::new()
    }

    fn permission_metadata_suggestions(&self) -> Vec<String> {
        Vec::new()
    }

    fn user_permission_rule_suggestions(&self, _targets: &GameProfileArgument) -> Vec<String> {
        Vec::new()
    }

    fn user_permission_metadata_suggestions(&self, _targets: &GameProfileArgument) -> Vec<String> {
        Vec::new()
    }

    fn group_permission_rule_suggestions(&self, _group: &str) -> Vec<String> {
        Vec::new()
    }

    fn group_permission_metadata_suggestions(&self, _group: &str) -> Vec<String> {
        Vec::new()
    }

    fn permission_group_names(&self) -> Vec<String> {
        Vec::new()
    }

    fn non_operator_profile_names(&self) -> Vec<String> {
        Vec::new()
    }

    fn all_profile_names(&self) -> Vec<String> {
        Vec::new()
    }

    fn operator_profile_names(&self) -> Vec<String> {
        Vec::new()
    }

    fn selector_player_names(&self) -> Vec<String> {
        Vec::new()
    }

    fn selector_team_names(&self) -> Vec<String> {
        Vec::new()
    }

    fn scoreboard_objective_names(&self) -> Vec<String> {
        Vec::new()
    }

    fn allows_entity_selectors(&self) -> bool {
        false
    }

    fn allows_advanced_entity_selectors(&self) -> bool {
        false
    }
}

/// Source behavior required by the Steel command scheduler.
pub(crate) trait ExecutionCommandSource:
    CommandArgumentSource + Sized + Send + Sync + 'static
{
    fn with_callback(&self, callback: CommandResultCallback) -> Self;

    fn callback(&self) -> CommandResultCallback;

    fn handle_error(&self, error: &CommandSyntaxError, forked: bool);
}

/// Permission lookup required while constructing and traversing Steel command trees.
pub(crate) trait CommandPermissionSource: ExecutionCommandSource {
    fn permission_state(&self, permission: &PermissionExpr) -> Option<PermissionState>;

    fn has_permission(&self, permission: &PermissionExpr) -> bool {
        self.permission_state(permission) == Some(PermissionState::Allow)
    }
}

/// Authorization state captured when command execution starts.
///
/// Vanilla keeps the initiating permission state when `/execute` changes the
/// execution entity, position, or world. Keeping this separate from the
/// mutable execution fields prevents source transforms from changing which
/// contextual Steel permissions apply.
#[derive(Clone, Debug, PartialEq, Eq)]
struct CommandAuthorizationContext {
    permission_context: PermissionContext,
}

impl CommandAuthorizationContext {
    fn for_world_key(world: steel_utils::Identifier) -> Self {
        Self {
            permission_context: PermissionContext::for_world(world),
        }
    }

    const fn permission_context(&self) -> &PermissionContext {
        &self.permission_context
    }
}

/// Immutable Minecraft command execution source.
#[derive(Clone)]
pub(crate) struct CommandSource {
    sender: CommandSender,
    player: Option<Arc<Player>>,
    entity: Option<SharedEntity>,
    world: Arc<World>,
    server: Arc<Server>,
    position: DVec3,
    rotation: (f32, f32),
    anchor: EntityAnchor,
    authorization: CommandAuthorizationContext,
    callback: CommandResultCallback,
    silent: bool,
}

impl CommandSource {
    pub(crate) fn new(sender: CommandSender, server: Arc<Server>) -> Self {
        let player = sender.get_player().map(Arc::clone);
        let world = player.as_ref().map_or_else(
            || Arc::clone(server.overworld()),
            |player| player.get_world(),
        );
        let entity = player
            .as_ref()
            .map(|player| Arc::clone(player) as SharedEntity);
        let position = entity.as_ref().map_or_else(
            || {
                let level_data = world.level_data.read();
                let spawn = &level_data.data().spawn;
                DVec3::new(f64::from(spawn.x), f64::from(spawn.y), f64::from(spawn.z))
            },
            |entity| entity.position(),
        );
        let rotation = entity
            .as_ref()
            .map_or((0.0, 0.0), |entity| entity.rotation());
        let authorization = CommandAuthorizationContext::for_world_key(world.key.clone());

        Self {
            sender,
            player,
            entity,
            world,
            server,
            position,
            rotation,
            anchor: EntityAnchor::default(),
            authorization,
            callback: CommandResultCallback::empty(),
            silent: false,
        }
    }

    #[expect(
        dead_code,
        reason = "source-aware runtime extensions need access to the original sender"
    )]
    pub(crate) const fn sender(&self) -> &CommandSender {
        &self.sender
    }

    pub(crate) const fn player(&self) -> Option<&Arc<Player>> {
        self.player.as_ref()
    }

    pub(crate) const fn entity(&self) -> Option<&SharedEntity> {
        self.entity.as_ref()
    }

    pub(crate) const fn world(&self) -> &Arc<World> {
        &self.world
    }

    pub(crate) const fn server(&self) -> &Arc<Server> {
        &self.server
    }

    pub(crate) const fn position(&self) -> DVec3 {
        self.position
    }

    pub(crate) const fn rotation(&self) -> (f32, f32) {
        self.rotation
    }

    pub(crate) const fn anchor(&self) -> EntityAnchor {
        self.anchor
    }

    pub(crate) fn with_entity(&self, entity: SharedEntity) -> Self {
        let mut source = self.clone();
        source.player = self
            .server
            .get_players()
            .into_iter()
            .find(|player| player.uuid() == entity.uuid());
        source.entity = Some(entity);
        source
    }

    pub(crate) fn with_world(&self, world: Arc<World>) -> Self {
        let mut source = self.clone();
        if self.world.key != world.key {
            let scale =
                self.world.dimension_type.coordinate_scale / world.dimension_type.coordinate_scale;
            source.position.x *= scale;
            source.position.z *= scale;
        }
        source.world = world;
        source
    }

    pub(crate) fn with_position(&self, position: DVec3) -> Self {
        let mut source = self.clone();
        source.position = position;
        source
    }

    pub(crate) fn with_rotation(&self, rotation: (f32, f32)) -> Self {
        let mut source = self.clone();
        source.rotation = normalize_rotation(rotation);
        source
    }

    pub(crate) fn with_anchor(&self, anchor: EntityAnchor) -> Self {
        let mut source = self.clone();
        source.anchor = anchor;
        source
    }

    pub(crate) fn facing_position(&self, target: DVec3) -> Self {
        let delta = target - self.anchor_position();
        let horizontal = delta.x.hypot(delta.z);
        let pitch = -delta.y.atan2(horizontal).to_degrees() as f32;
        let yaw = delta.z.atan2(delta.x).to_degrees() as f32 - 90.0;
        self.with_rotation((yaw, pitch))
    }

    pub(crate) fn anchor_position(&self) -> DVec3 {
        if self.anchor == EntityAnchor::Eyes
            && let Some(entity) = &self.entity
        {
            return DVec3::new(
                self.position.x,
                self.position.y + entity.get_eye_height(),
                self.position.z,
            );
        }
        self.position
    }

    #[expect(
        dead_code,
        reason = "silent command-source derivation is retained for future vanilla command paths"
    )]
    pub(crate) fn with_suppressed_output(&self) -> Self {
        let mut source = self.clone();
        source.silent = true;
        source
    }

    #[expect(
        dead_code,
        reason = "custom executors need to inspect silent command-source state"
    )]
    pub(crate) const fn is_silent(&self) -> bool {
        self.silent
    }

    pub(crate) fn send_success(&self, message: &TextComponent) {
        if !self.silent {
            self.sender.send_message(message);
        }
    }

    pub(crate) fn send_failure(&self, message: TextComponent) {
        if !self.silent {
            self.sender.send_message(&message.color(Color::Red));
        }
    }

    fn sequence_limit(&self) -> usize {
        let value = game_rule_integer(
            self.world.get_game_rule(&MAX_COMMAND_SEQUENCE_LENGTH),
            MAX_COMMAND_SEQUENCE_LENGTH.default_value,
            1,
        );
        value.max(1) as usize
    }

    fn fork_limit(&self) -> usize {
        let value = game_rule_integer(
            self.world.get_game_rule(&MAX_COMMAND_FORKS),
            MAX_COMMAND_FORKS.default_value,
            0,
        );
        value.max(0) as usize
    }
}

impl ExecutionCommandSource for CommandSource {
    fn with_callback(&self, callback: CommandResultCallback) -> Self {
        let mut source = self.clone();
        source.callback = callback;
        source
    }

    fn callback(&self) -> CommandResultCallback {
        self.callback.clone()
    }

    fn handle_error(&self, error: &CommandSyntaxError, forked: bool) {
        if forked || self.silent {
            return;
        }
        self.send_failure(error.message_component());
        if let Some(context) = error.context_component() {
            self.sender.send_message(&context);
        }
    }
}

impl CommandArgumentSource for CommandSource {
    fn default_world_clock(&self) -> Option<WorldClockRef> {
        self.world.dimension_type.default_clock
    }

    fn domain_exists(&self, domain: &str) -> bool {
        self.server.worlds.has_domain(domain)
    }

    fn domain_names(&self) -> Vec<&str> {
        self.server.worlds.domain_names().collect()
    }

    fn command_world_names(&self) -> Vec<String> {
        let domain = self.world.domain();
        let mut names = Vec::new();
        for key in self.server.worlds.keys() {
            names.push(key.to_string());
            if key.namespace.as_ref() == domain {
                names.push(key.path.to_string());
            }
        }
        names
    }

    fn permission_context_world_names(&self) -> Vec<String> {
        self.server.worlds.keys().map(ToString::to_string).collect()
    }

    fn command_storage_keys(&self) -> Vec<String> {
        self.server
            .command_storage
            .get(self.world.domain())
            .map_or_else(Vec::new, |storage| {
                storage
                    .keys()
                    .into_iter()
                    .map(|key| key.to_string())
                    .collect()
            })
    }

    fn permission_rule_suggestions(&self) -> Vec<String> {
        self.server.permission_rule_suggestions()
    }

    fn permission_metadata_suggestions(&self) -> Vec<String> {
        self.server.permission_metadata_suggestions()
    }

    fn user_permission_rule_suggestions(&self, targets: &GameProfileArgument) -> Vec<String> {
        let mut suggestions = BTreeSet::new();
        for uuid in profile_argument_uuids(self, targets) {
            let Some(state) = self.server.player_permission_state(uuid) else {
                continue;
            };
            suggestions.extend(
                state
                    .overrides()
                    .entries()
                    .iter()
                    .filter(|entry| can_manage_permission(self, entry.key()))
                    .map(|entry| {
                        PermissionRuleExpression::new(entry.key().clone(), entry.context().clone())
                            .to_string()
                    }),
            );
        }
        suggestions.into_iter().collect()
    }

    fn user_permission_metadata_suggestions(&self, targets: &GameProfileArgument) -> Vec<String> {
        if !has_permission_key(self, "steel.permission.metadata") {
            return Vec::new();
        }
        let mut suggestions = BTreeSet::new();
        for uuid in profile_argument_uuids(self, targets) {
            let Some(state) = self.server.player_permission_state(uuid) else {
                continue;
            };
            suggestions.extend(state.metadata_overrides().entries().iter().map(|entry| {
                PermissionMetadataExpression::new(entry.key().clone(), entry.context().clone())
                    .to_string()
            }));
        }
        suggestions.into_iter().collect()
    }

    fn group_permission_rule_suggestions(&self, group: &str) -> Vec<String> {
        if !can_manage_group(self, group) {
            return Vec::new();
        }
        self.server
            .permission_groups
            .config_snapshot()
            .groups
            .get(group)
            .into_iter()
            .flat_map(|group| group.allow.iter().chain(&group.deny))
            .filter(|expression| {
                PermissionRuleExpression::parse(expression.as_str())
                    .is_ok_and(|expression| can_manage_permission(self, expression.key()))
            })
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    fn group_permission_metadata_suggestions(&self, group: &str) -> Vec<String> {
        if !has_permission_key(self, "steel.permission.metadata") || !can_manage_group(self, group)
        {
            return Vec::new();
        }
        self.server
            .permission_groups
            .config_snapshot()
            .groups
            .get(group)
            .into_iter()
            .flat_map(|group| group.metadata.iter().map(|rule| rule.key.clone()))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    fn permission_group_names(&self) -> Vec<String> {
        self.server.permission_groups.group_names()
    }

    fn non_operator_profile_names(&self) -> Vec<String> {
        profile_names(self.server(), false, false)
    }

    fn all_profile_names(&self) -> Vec<String> {
        all_profile_names(self.server())
    }

    fn operator_profile_names(&self) -> Vec<String> {
        profile_names(self.server(), true, true)
    }

    fn selector_player_names(&self) -> Vec<String> {
        let domain = self.world.domain();
        self.server
            .get_players()
            .into_iter()
            .filter(|player| player.get_world().domain() == domain)
            .map(|player| player.gameprofile.name.clone())
            .collect()
    }

    fn selector_team_names(&self) -> Vec<String> {
        self.server
            .scoreboards
            .get(self.world.domain())
            .map_or_else(Vec::new, Scoreboard::team_names)
    }

    fn scoreboard_objective_names(&self) -> Vec<String> {
        self.server
            .scoreboards
            .get(self.world.domain())
            .map_or_else(Vec::new, Scoreboard::objective_names)
    }

    fn allows_entity_selectors(&self) -> bool {
        let Ok(permission) = entity_selector_permission_expr() else {
            tracing::error!("built-in entity selector permission key is invalid");
            return false;
        };
        CommandPermissionSource::has_permission(self, &permission)
    }

    fn allows_advanced_entity_selectors(&self) -> bool {
        let Ok(permission) = entity_selector_advanced_permission_expr() else {
            tracing::error!("built-in advanced entity selector permission key is invalid");
            return false;
        };
        CommandPermissionSource::has_permission(self, &permission)
    }
}

fn profile_argument_uuids(
    source: &CommandSource,
    argument: &GameProfileArgument,
) -> BTreeSet<uuid::Uuid> {
    match argument {
        GameProfileArgument::Selector(selector) => selector.find_players(source).map_or_else(
            |_| BTreeSet::new(),
            |players| {
                players
                    .into_iter()
                    .map(|player| player.gameprofile.id)
                    .collect()
            },
        ),
        GameProfileArgument::Direct(value) => {
            let known = source.server.known_players();
            let uuid = uuid::Uuid::parse_str(value)
                .ok()
                .or_else(|| known.by_name(value).map(KnownPlayer::uuid))
                .or_else(|| {
                    source
                        .server
                        .get_players()
                        .into_iter()
                        .find(|player| player.gameprofile.name.eq_ignore_ascii_case(value))
                        .map(|player| player.gameprofile.id)
                });
            uuid.into_iter().collect()
        }
    }
}

fn can_manage_permission(source: &CommandSource, permission: &PermissionKey) -> bool {
    has_permission_key(
        source,
        &format!("steel.permission.manage.{}", permission.as_str()),
    )
}

fn can_manage_group(source: &CommandSource, group: &str) -> bool {
    has_permission_key(source, &format!("steel.permission.group.{group}"))
}

fn has_permission_key(source: &CommandSource, value: &str) -> bool {
    PermissionKey::parse(value)
        .is_ok_and(|key| CommandPermissionSource::has_permission(source, &PermissionExpr::key(key)))
}

fn profile_names(server: &Server, operator: bool, include_known: bool) -> Vec<String> {
    let players = server.get_players();
    let mut names = Vec::new();
    for player in &players {
        let is_operator = player
            .permission_groups()
            .iter()
            .any(|group| group == OP_GROUP);
        if operator == is_operator {
            names.push(player.gameprofile.name.clone());
        }
    }

    if !include_known {
        return names;
    }

    for known in server.known_players().entries() {
        if names
            .iter()
            .any(|name| name.eq_ignore_ascii_case(known.last_known_name()))
            || players.iter().any(|player| {
                player
                    .gameprofile
                    .name
                    .eq_ignore_ascii_case(known.last_known_name())
            })
        {
            continue;
        }
        let is_operator = server
            .player_permission_state(known.uuid())
            .is_some_and(|state| state.groups().iter().any(|group| group == OP_GROUP));
        if operator == is_operator {
            names.push(known.last_known_name().to_owned());
        }
    }
    names
}

fn all_profile_names(server: &Server) -> Vec<String> {
    let players = server.get_players();
    let mut names = players
        .iter()
        .map(|player| player.gameprofile.name.clone())
        .collect::<Vec<_>>();
    for known in server.known_players().entries() {
        if names
            .iter()
            .any(|name| name.eq_ignore_ascii_case(known.last_known_name()))
        {
            continue;
        }
        names.push(known.last_known_name().to_owned());
    }
    names
}

impl CommandPermissionSource for CommandSource {
    fn permission_state(&self, permission: &PermissionExpr) -> Option<PermissionState> {
        let CommandSender::Player(player) = &self.sender else {
            return Some(PermissionState::Allow);
        };
        player.permission_state_in(permission, self.authorization.permission_context())
    }
}

impl CommandExecutionContext<CommandSource> {
    pub(crate) fn for_source(source: &CommandSource) -> Self {
        Self::new(source.sequence_limit(), source.fork_limit())
    }
}

const fn game_rule_integer(value: GameRuleValue, default: GameRuleValue, fallback: i32) -> i32 {
    match value {
        GameRuleValue::Int(value) => value,
        GameRuleValue::Bool(_) => match default {
            GameRuleValue::Int(value) => value,
            GameRuleValue::Bool(_) => fallback,
        },
    }
}

fn normalize_rotation((mut yaw, mut pitch): (f32, f32)) -> (f32, f32) {
    yaw = yaw.rem_euclid(360.0);
    if yaw >= 180.0 {
        yaw -= 360.0;
    }
    pitch = pitch.rem_euclid(360.0);
    if pitch >= 180.0 {
        pitch -= 360.0;
    }
    (yaw, pitch)
}

#[cfg(test)]
mod tests {
    use steel_registry::game_rules::GameRuleValue;
    use steel_utils::Identifier;

    use crate::permission::PermissionContext;

    use super::{CommandAuthorizationContext, game_rule_integer, normalize_rotation};

    #[test]
    fn rotation_normalization_matches_command_source_stack() {
        assert_eq!(normalize_rotation((540.0, -540.0)), (-180.0, -180.0));
        assert_eq!(normalize_rotation((-181.0, 181.0)), (179.0, -179.0));
    }

    #[test]
    fn integer_game_rule_falls_back_to_its_extracted_default() {
        assert_eq!(
            game_rule_integer(GameRuleValue::Int(12), GameRuleValue::Int(7), 1),
            12
        );
        assert_eq!(
            game_rule_integer(GameRuleValue::Bool(false), GameRuleValue::Int(7), 1),
            7
        );
    }

    #[test]
    fn authorization_context_captures_initial_world_scope() {
        let world = Identifier::new("lobby", "spawn");
        let authorization = CommandAuthorizationContext::for_world_key(world.clone());

        assert_eq!(
            authorization.permission_context(),
            &PermissionContext::for_world(world)
        );
    }
}
