//! Entity events.

use glam::DVec3;
use steel_utils::Identifier;
use uuid::Uuid;

use super::Event;
use crate::entity::Entity;

/// Copied identity of an entity involved in an event.
#[derive(Debug, Clone)]
pub struct EntityInfo {
    /// Network entity ID.
    pub id: i32,
    /// Persistent UUID.
    pub uuid: Uuid,
    /// Entity type key.
    pub kind: Identifier,
    /// Whether the entity is a player.
    pub is_player: bool,
    /// Position when the event fired.
    pub position: DVec3,
}

impl EntityInfo {
    /// Captures an entity's identity.
    #[must_use]
    pub fn of(entity: &(impl Entity + ?Sized)) -> Self {
        Self {
            id: entity.id(),
            uuid: entity.uuid(),
            kind: entity.entity_type().key.clone(),
            is_player: entity.as_player().is_some(),
            position: entity.position(),
        }
    }
}

/// Damage to a living entity before invulnerability cooldowns and armor apply.
pub struct EntityDamageEvent {
    /// Key of the world the damage happens in.
    pub world: Identifier,
    /// Damaged entity.
    pub victim: EntityInfo,
    /// Entity responsible, such as a projectile's shooter.
    pub attacker: Option<EntityInfo>,
    /// Damage type key.
    pub cause: Identifier,
    /// Damage amount.
    pub amount: f32,
    /// Whether no damage is dealt.
    pub cancelled: bool,
}

impl Event for EntityDamageEvent {
    fn is_cancelled(&self) -> bool {
        self.cancelled
    }
}

/// A non-player entity is about to be added to a world.
pub struct EntitySpawnEvent {
    /// World key.
    pub world: Identifier,
    /// Spawning entity.
    pub entity: EntityInfo,
    /// Whether the entity is not added.
    pub cancelled: bool,
}

impl Event for EntitySpawnEvent {
    fn is_cancelled(&self) -> bool {
        self.cancelled
    }
}

/// A non-player living entity died.
pub struct EntityDeathEvent {
    /// World key.
    pub world: Identifier,
    /// Dead entity.
    pub entity: EntityInfo,
    /// Whether loot and experience drop.
    pub drop_loot: bool,
}

impl Event for EntityDeathEvent {}
