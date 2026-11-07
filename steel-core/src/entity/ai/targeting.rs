use std::sync::Arc;

use steel_utils::types::Difficulty;

use crate::entity::Living;
use crate::world::World;

const MIN_VISIBILITY_DISTANCE_FOR_INVISIBLE_TARGET: f64 = 2.0;

pub(crate) type TargetingSelector = Arc<dyn Fn(&Living<'_>, &World) -> bool + Send + Sync>;

#[derive(Clone)]
pub(crate) struct TargetingConditions {
    is_combat: bool,
    range: f64,
    check_line_of_sight: bool,
    test_invisible: bool,
    selector: Option<TargetingSelector>,
}

impl TargetingConditions {
    #[must_use]
    pub(crate) const fn for_combat() -> Self {
        Self::new(true)
    }

    #[must_use]
    pub(crate) const fn for_non_combat() -> Self {
        Self::new(false)
    }

    const fn new(is_combat: bool) -> Self {
        Self {
            is_combat,
            range: -1.0,
            check_line_of_sight: true,
            test_invisible: true,
            selector: None,
        }
    }

    #[must_use]
    pub(crate) const fn range(mut self, range: f64) -> Self {
        self.range = range;
        self
    }

    #[must_use]
    pub(crate) const fn ignore_line_of_sight(mut self) -> Self {
        self.check_line_of_sight = false;
        self
    }

    #[must_use]
    pub(crate) const fn ignore_invisibility_testing(mut self) -> Self {
        self.test_invisible = false;
        self
    }

    #[must_use]
    pub(crate) fn selector(
        mut self,
        selector: impl Fn(&Living<'_>, &World) -> bool + Send + Sync + 'static,
    ) -> Self {
        self.selector = Some(Arc::new(selector));
        self
    }

    #[must_use]
    pub(crate) fn test(
        &self,
        world: &World,
        targeter: Option<&Living<'_>>,
        target: &Living<'_>,
    ) -> bool {
        if targeter.is_some_and(|targeter| targeter.uuid() == target.uuid()) {
            return false;
        }
        if !target.can_be_seen_by_anyone() {
            return false;
        }
        if let Some(selector) = &self.selector
            && !selector(target, world)
        {
            return false;
        }

        let Some(targeter) = targeter else {
            return !self.is_combat
                || target.can_be_seen_as_enemy() && world.difficulty() != Difficulty::Peaceful;
        };

        if self.is_combat
            && (!targeter.can_attack(target) || targeter.is_allied_to(target.entity()))
        {
            return false;
        }

        if self.range > 0.0 {
            let modifier = if self.test_invisible {
                target.get_visibility_percent(Some(targeter.entity()))
            } else {
                1.0
            };
            let visibility_distance =
                (self.range * modifier).max(MIN_VISIBILITY_DISTANCE_FOR_INVISIBLE_TARGET);
            if targeter.position().distance_squared(target.position())
                > visibility_distance * visibility_distance
            {
                return false;
            }
        }

        if self.check_line_of_sight
            && let Some(mob) = targeter.as_mob()
            && !mob.has_line_of_sight_cached(target.entity())
        {
            return false;
        }

        true
    }
}

impl Default for TargetingConditions {
    fn default() -> Self {
        Self::for_combat()
    }
}

#[cfg(test)]
mod tests {
    use crate::entity::LivingProps;
    use crate::test_support::synced_with_health;
    use std::sync::{Arc, Weak};

    use glam::DVec3;
    use steel_registry::entity_type::EntityTypeRef;
    use steel_registry::{init_vanilla_registry, vanilla_blocks, vanilla_entities};
    use steel_utils::locks::CheckedMutex;
    use steel_utils::types::UpdateFlags;
    use steel_utils::{BlockPos, ChunkPos};

    use super::*;

    use crate::entity::LivingParts as _;

    use crate::behavior::init_behaviors;
    use crate::entity::MobParts as _;
    use crate::entity::component::Has;
    use crate::entity::component::definition::MobDefinition;
    use crate::entity::{Entity, EntityBase, LivingEntityBase, MobBase};
    use crate::entity::{EntityHooks, EntityProps};
    use crate::entity::{EntitySyncedData, MobProps, mob};
    use crate::test_support::{fresh_test_world, insert_ready_full_chunk};
    use crate::world::World;
    use std::sync::LazyLock;
    use steel_registry::vanilla_entity_data::MobEntityData;
    use steel_utils::{DowncastType as _, DowncastTypeKey, ErasedType};

    struct HoveringTestMob {
        base: EntityBase,
        living_base: LivingEntityBase,
        mob_base: MobBase,
        entity_data: CheckedMutex<MobEntityData>,
    }

    impl HoveringTestMob {
        fn new(id: i32, position: DVec3, world: Weak<World>) -> Self {
            init_vanilla_registry();
            Self {
                base: EntityBase::new(id, position, vanilla_entities::PIG.dimensions, world),
                living_base: LivingEntityBase::without_synced_data(
                    &vanilla_entities::PIG,
                    &HoveringTestMob::LIVING,
                ),
                mob_base: MobBase::new(&HoveringTestMob::MOB, HoveringTestMob::definition()),
                entity_data: CheckedMutex::new(synced_with_health(MobEntityData::new(), 10.0)),
            }
        }
    }

    crate::entity::impl_test_downcast_type!(HoveringTestMob);

    impl HoveringTestMob {
        const MOB: MobProps<Self> = MobProps::new();

        fn definition() -> &'static MobDefinition<Self> {
            static DEFINITION: LazyLock<MobDefinition<HoveringTestMob>> =
                LazyLock::new(mob::stages::definition);
            &DEFINITION
        }
    }

    impl HoveringTestMob {
        const ENTITY: EntityProps<Self> = EntityProps {
            ..mob::entity_props()
        };
    }

    impl Entity for HoveringTestMob {
        fn erased_component(&self, key: DowncastTypeKey) -> Option<&dyn ErasedType> {
            match key {
                MobBase::TYPE_KEY => Some(&self.mob_base),
                LivingEntityBase::TYPE_KEY => Some(&self.living_base),
                _ => None,
            }
        }

        fn synced_data(&self) -> Option<&dyn EntitySyncedData> {
            Some(&self.entity_data)
        }

        fn base(&self) -> &EntityBase {
            &self.base
        }

        fn entity_type(&self) -> EntityTypeRef {
            &vanilla_entities::PIG
        }

        fn entity_hooks(&self) -> &'static dyn EntityHooks {
            &Self::ENTITY
        }
    }

    impl HoveringTestMob {
        const LIVING: LivingProps<Self> = LivingProps {
            ..LivingProps::new()
        };
    }

    impl Has<LivingEntityBase> for HoveringTestMob {
        fn component(&self) -> &LivingEntityBase {
            &self.living_base
        }
    }

    impl Has<MobBase> for HoveringTestMob {
        fn component(&self) -> &MobBase {
            &self.mob_base
        }
    }

    #[test]
    fn sight_check_applies_to_a_mob_that_does_not_pathfind() {
        init_vanilla_registry();
        init_behaviors();
        let world_fixture = fresh_test_world("targeting_sight_check_scope");
        let world = &world_fixture.world;
        let wall = BlockPos::new(2, 64, 0);
        insert_ready_full_chunk(world, ChunkPos::from_block_pos(wall));
        let targeter = HoveringTestMob::new(1, DVec3::new(0.5, 64.0, 0.5), Arc::downgrade(world));
        let target = HoveringTestMob::new(2, DVec3::new(4.5, 64.0, 0.5), Arc::downgrade(world));

        assert!(
            targeter.as_pathfinder_mob().is_none(),
            "the targeter must not pathfind or this test proves nothing"
        );

        assert!(
            TargetingConditions::for_non_combat().test(
                world.as_ref(),
                Some(&targeter.living()),
                &target.living()
            ),
            "a mob with a clear view should pick its target"
        );

        assert!(world.set_block(
            wall,
            vanilla_blocks::STONE.default_state(),
            UpdateFlags::UPDATE_ALL,
        ));
        targeter.mob().mob_base().sensing().lock().tick();
        assert!(
            !TargetingConditions::for_non_combat().test(
                world.as_ref(),
                Some(&targeter.living()),
                &target.living()
            ),
            "a mob that cannot see its target should not pick it, whether or not it pathfinds"
        );
        assert!(
            TargetingConditions::for_non_combat()
                .ignore_line_of_sight()
                .test(world.as_ref(), Some(&targeter.living()), &target.living()),
            "the same target should be picked once sight is not required"
        );
    }
}
