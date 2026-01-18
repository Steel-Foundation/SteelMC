//! This module contains the command context.
use std::sync::Arc;

use steel_utils::math::Vector3;

use crate::command::error::CommandError;
use crate::command::sender::CommandSender;
use crate::player::Player;
use crate::world::World;

/// The context of a command.
#[derive(Clone)]
pub struct CommandContext {
    /// The sender of the command.
    pub sender: CommandSender,
    /// The player targeted by the command.
    pub player: Option<Arc<Player>>,
    /// The world/dimension of the command.
    pub world: Option<Arc<World>>,
    /// The position of the command.
    pub position: Option<Vector3<f64>>,
    /// The rotation of the command.
    pub rotation: Option<(f32, f32)>,
    /// The anchor of the command.
    pub anchor: EntityAnchor,
}

/// The position anchor to use for an entity.
#[derive(Clone, Default)]
pub enum EntityAnchor {
    /// The feet of the entity.
    #[default]
    Feet,
    /// The eyes of the entity.
    Eyes,
}

impl CommandContext {
    /// Creates a new command context.
    #[must_use]
    pub fn new(sender: CommandSender) -> Self {
        let player = sender.get_player().cloned();
        let position = player.as_ref().map(|p| *p.position.lock());
        let world = player.as_ref().map(|p| Arc::clone(&p.world));

        Self {
            sender,
            player,
            world,
            position,
            rotation: None,
            anchor: EntityAnchor::default(),
        }
    }

    /// Gets the world for this command context.
    ///
    /// Returns an error if no world is associated with this context.
    /// This mirrors vanilla's `CommandSourceStack.getLevel()`.
    pub fn get_world(&self) -> Result<&Arc<World>, CommandError> {
        self.world.as_ref().ok_or(CommandError::InvalidRequirement)
    }
}
