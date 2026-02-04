//! This module contains the command context.
use std::sync::Arc;

use steel_utils::math::Vector3;

use crate::command::sender::CommandSender;
use crate::player::Player;
use crate::server::Server;
use crate::world::World;

/// The context of a command.
#[derive(Clone)]
pub struct CommandContext {
    /// The sender of the command.
    pub sender: CommandSender,
    /// The player targeted by the command.
    pub player: Option<Arc<Player>>,
    /// The world/dimension of the command.
    pub world: Arc<World>,
    /// The server where the command has been run.
    pub server: Arc<Server>,
    /// The position of the command.
    pub position: Vector3<f64>,
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
    pub fn new(sender: CommandSender, server: Arc<Server>) -> Self {
        let player = sender.get_player().cloned();
        let world = player
            .as_ref()
            .map_or(server.worlds[0].clone(), |p| Arc::clone(&p.world));
        let world_spawn = world.level_data.read().data().spawn.clone();
        let position = player
            .as_ref()
            // TODO: Check this. The default position is the surface of the world center
            // (Where the compass should point to)
            .map_or(
                Vector3::new(
                    f64::from(world_spawn.x),
                    f64::from(world_spawn.y),
                    f64::from(world_spawn.z),
                ),
                |p| *p.position.lock(),
            );

        Self {
            sender,
            player,
            world,
            server,
            position,
            rotation: None,
            anchor: EntityAnchor::default(),
        }
    }
}
