//! Jockey mechanics.
//!
//! Mirrors Vanilla's jockey system - when certain mobs spawn, they may have
//! another mob riding on top of them (e.g., zombie jockey on chicken, skeleton
//! jockey on spider, etc.).

use steel_utils::ErasedType;

use crate::entity::Mob;
use crate::entity_living::LivingEntity;
use crate::entity::SharedEntity;
use crate::world::World;

// Jockey spawn reason types
#[derive(Clone, Debug, Copy, PartialEq, Eq)]
pub enum JockeySpawnReason {
    Natural,
    Spawner,
}

// A jockey pair: the rider and the mount
type JockeyPair = (SharedEntity, SharedEntity);

// Check if an entity is a valid jockey rider
fn is_valid_jockey_rider(entity: &dyn LivingEntity) -> bool {
    // Would check entity type for valid jockeys (zombie, skeleton, etc.)
    false
}

// Check if an entity is a valid mount
fn is_valid_mount(entity: &dyn LivingEntity) -> bool {
    // Would check entity type for valid mounts (chicken, spider, etc.)
    false
}

/// Attempts to create a jockey spawn.
///
/// Mirrors Vanilla's jockey spawn logic - when a mob spawns, there's a chance
/// it will have a jockey riding on top.
pub fn attempt_jockey_spawn(
    world: &World,
    parent: &SharedEntity,
    reason: JockeySpawnReason,
) -> Option<JockeyPair> {
    // Natural jockey chance varies by mob type
    // For example: zombie chicken jockey has ~10% chance
    // Skeletonspider jockey has ~5% chance
    
    // Check if the parent entity can have a jockey
    if let Some(parent_living) = parent.as_living_entity() {
        // Determine jockey type based on parent
        // Create the jockey entity
        // Attach the jockey to the parent
        // Return the jockey pair
    }
    
    None
}

/// Ticks jockey state - checks if jockey should stay on, fall off, etc.
pub fn tick_jockey(
    rider: &SharedEntity,
    mount: &SharedEntity,
    world: &World,
) {
    // Check if rider is still alive
    if !rider.is_alive() || !mount.is_alive() {
        // Remove jockey if either died
        // Would detach rider from mount
        return;
    }
    
    // Check if rider should fall off
    // Vanilla checks: player distance, mob AI, etc.
    // If conditions met, detach rider
}

/// Get the rider entity from a mob
pub fn get_jockey(rider: &SharedEntity) -> Option<SharedEntity> {
    // Would check entity NBT or metadata for jockey info
    None
}

/// Set the jockey on a mount
pub fn set_jockey(mount: &SharedEntity, rider: SharedEntity) {
    // Would store jockey info in entity NBT/metadata
}