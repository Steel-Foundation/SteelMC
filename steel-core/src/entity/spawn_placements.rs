//! Entity-type spawn placement predicates and their central dispatch.

use steel_registry::entity_type::EntityTypeRef;
use steel_registry::vanilla_entities;
use steel_utils::{BlockPos, types::Difficulty};

use crate::entity::entities::{ChickenEntity, CowEntity, EndermiteEntity, PigEntity, SheepEntity};
use crate::entity::{Animal, EntitySpawnReason};
use crate::world::{LevelReader, World};

type SpawnRule = fn(&dyn LevelReader, EntitySpawnReason, BlockPos) -> bool;

struct SpawnPlacementRule {
    entity_type: EntityTypeRef,
    rule: SpawnRule,
}

const SPAWN_PLACEMENT_RULES: &[SpawnPlacementRule] = &[
    SpawnPlacementRule {
        entity_type: &vanilla_entities::CHICKEN,
        rule: <ChickenEntity as Animal>::check_animal_spawn_rules,
    },
    SpawnPlacementRule {
        entity_type: &vanilla_entities::COW,
        rule: <CowEntity as Animal>::check_animal_spawn_rules,
    },
    SpawnPlacementRule {
        entity_type: &vanilla_entities::ENDERMITE,
        rule: EndermiteEntity::check_endermite_spawn_rules,
    },
    SpawnPlacementRule {
        entity_type: &vanilla_entities::PIG,
        rule: <PigEntity as Animal>::check_animal_spawn_rules,
    },
    SpawnPlacementRule {
        entity_type: &vanilla_entities::SHEEP,
        rule: <SheepEntity as Animal>::check_animal_spawn_rules,
    },
];

/// Vanilla entity-type spawn-rule dispatch used by normal mob spawners.
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

        SPAWN_PLACEMENT_RULES
            .iter()
            .find(|placement| placement.entity_type == entity_type)
            .is_none_or(|placement| (placement.rule)(world, EntitySpawnReason::Spawner, pos))
    }
}
