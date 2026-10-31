//! Entity-type spawn placement dispatch.

use steel_registry::entity_type::EntityTypeRef;
use steel_utils::{BlockPos, types::Difficulty};

use crate::entity::{ENTITIES, EntitySpawnReason};
use crate::world::World;

/// Vanilla `SpawnPlacements`.
///
/// Predicates are each entity's `SPAWN_RULE`, registered by `#[entity_behavior(spawn_rule)]`.
/// Placement type and heightmap are not modeled yet because only the spawner path consumes this.
pub(crate) struct SpawnPlacements;

impl SpawnPlacements {
    /// Checks the vanilla placement predicate used by normal mob spawners.
    pub(crate) fn check_spawner_spawn_rules(
        entity_type: EntityTypeRef,
        world: &World,
        pos: BlockPos,
    ) -> bool {
        if !entity_type.allowed_in_peaceful && world.difficulty() == Difficulty::Peaceful {
            return false;
        }

        ENTITIES
            .spawn_rule(entity_type)
            .is_none_or(|rule| rule(world, EntitySpawnReason::Spawner, pos))
    }
}
