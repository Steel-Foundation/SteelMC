use std::sync::Arc;

use glam::DVec3;
use steel_registry::{
    game_rules::GameRuleValue,
    vanilla_game_rules::{MAX_COMMAND_FORKS, MAX_COMMAND_SEQUENCE_LENGTH},
};
use text_components::{Modifier, TextComponent, format::Color};

use crate::{
    command::{
        brigadier::{CommandSyntaxError, CommandSyntaxErrorKind},
        context::EntityAnchor,
        sender::CommandSender,
    },
    entity::{Entity as _, SharedEntity},
    permission::{PermissionContext, PermissionExpr, PermissionState},
    player::Player,
    server::Server,
    world::World,
};

use super::CommandExecutionContext;

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

/// Source behavior required by the Steel command scheduler.
pub(crate) trait ExecutionCommandSource: Sized + Send + Sync + 'static {
    fn with_callback(&self, callback: CommandResultCallback) -> Self;

    fn callback(&self) -> CommandResultCallback;

    fn handle_error(&self, error: &CommandSyntaxError, forked: bool);
}

/// Permission lookup required while constructing and traversing Steel command trees.
pub(crate) trait CommandPermissionSource: ExecutionCommandSource {
    fn permission_state(&self, permission: &PermissionExpr) -> Option<PermissionState>;
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

        Self {
            sender,
            player,
            entity,
            world,
            server,
            position,
            rotation,
            anchor: EntityAnchor::default(),
            callback: CommandResultCallback::empty(),
            silent: false,
        }
    }

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

    pub(crate) fn with_suppressed_output(&self) -> Self {
        let mut source = self.clone();
        source.silent = true;
        source
    }

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
        let message = match error.kind() {
            CommandSyntaxErrorKind::Dynamic(message) => message.as_ref().clone(),
            _ => TextComponent::from(error.raw_message()),
        };
        self.send_failure(message);
    }
}

impl CommandPermissionSource for CommandSource {
    fn permission_state(&self, permission: &PermissionExpr) -> Option<PermissionState> {
        let CommandSender::Player(player) = &self.sender else {
            return Some(PermissionState::Allow);
        };
        let context = PermissionContext::for_world(self.world.domain(), self.world.key.clone());
        player.permission_state_in(permission, &context)
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

    use super::{game_rule_integer, normalize_rotation};

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
}
