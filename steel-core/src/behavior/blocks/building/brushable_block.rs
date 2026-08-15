//! Brushable block behavior for suspicious sand and suspicious gravel.

use std::sync::{Arc, Weak};

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{BlockStateProperties, Direction, IntProperty};
use steel_registry::sound_event::SoundEventRef;
use steel_registry::{vanilla_block_entity_types, vanilla_game_events};
use steel_utils::Downcast as _;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::blocks::FallingBlock;
use crate::behavior::{
    BlockBehavior, BlockEntityCreation, BlockPlaceContext, BrushableData, Fallable,
};
use crate::block_entity::BLOCK_ENTITIES;
use crate::block_entity::entities::BrushableBlockEntity;
use crate::entity::entities::FallingBlockEntity;
use crate::entity::{Entity as _, EntityEventSource as _};
use crate::world::game_event::GameEventContext;
use crate::world::{ScheduledTickAccess, World};

/// Vanilla archaeology block behavior for suspicious sand and suspicious gravel.
#[block_behavior]
pub struct BrushableBlock {
    block: BlockRef,
    #[json_arg(vanilla_blocks, json = "turns_into")]
    turns_into: BlockRef,
    #[json_arg(sound_events, json = "brush_sound")]
    brush_sound: SoundEventRef,
    #[json_arg(sound_events, json = "brush_completed_sound")]
    brush_completed_sound: SoundEventRef,
}

const DUSTED: &IntProperty = &BlockStateProperties::DUSTED;

impl BrushableBlock {
    /// Creates a brushable block behavior from extracted vanilla block arguments.
    #[must_use]
    pub const fn new(
        block: BlockRef,
        turns_into: BlockRef,
        brush_sound: SoundEventRef,
        brush_completed_sound: SoundEventRef,
    ) -> Self {
        Self {
            block,
            turns_into,
            brush_sound,
            brush_completed_sound,
        }
    }
}

impl BlockBehavior for BrushableBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state().set_value(DUSTED, 0))
    }

    fn on_place(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _old_state: BlockStateId,
        _moved_by_piston: bool,
    ) {
        world.schedule_block_tick_default(pos, self.block, 2);
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        _direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        let _ = world.schedule_block_tick_default(pos, self.block, 2);
        state
    }

    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        if let Some(block_entity) = world.get_block_entity(pos)
            && let Some(brushable) = block_entity.downcast_ref::<BrushableBlockEntity>()
        {
            let mutation = brushable.check_reset(world);
            mutation.apply(world, pos);
        }

        if let Some(entity) = FallingBlock::tick(state, world, pos) {
            entity.disable_drop();
        }
    }

    fn new_block_entity(
        &self,
        level: Weak<World>,
        pos: BlockPos,
        state: BlockStateId,
    ) -> BlockEntityCreation {
        BlockEntityCreation::from_registered_factory(BLOCK_ENTITIES.create(
            &vanilla_block_entity_types::BRUSHABLE_BLOCK,
            level,
            pos,
            state,
        ))
    }

    fn should_keep_block_entity(&self, old_state: BlockStateId, new_state: BlockStateId) -> bool {
        old_state.get_block() == new_state.get_block()
    }

    fn brushable_data(&self, _state: BlockStateId) -> Option<BrushableData> {
        Some(BrushableData {
            turns_into: self.turns_into,
            brush_sound: self.brush_sound,
            brush_completed_sound: self.brush_completed_sound,
        })
    }

    fn as_fallable(&self) -> Option<&dyn Fallable> {
        Some(self)
    }
}

impl Fallable for BrushableBlock {
    fn on_broken_after_fall(
        &self,
        world: &Arc<World>,
        _pos: BlockPos,
        entity: &FallingBlockEntity,
    ) {
        let center = entity.bounding_box().center();
        world.destroy_block_effect(
            BlockPos::from(center),
            u32::from(entity.block_state().0),
            None,
        );
        world.game_event_at(
            &vanilla_game_events::BLOCK_DESTROY,
            center,
            &GameEventContext::new(
                Some(entity.as_entity_event_source()),
                Some(entity.block_state()),
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::blocks::BlockRef;
    use steel_registry::{init_vanilla_registry, vanilla_blocks};
    use steel_utils::types::UpdateFlags;
    use steel_utils::{BlockPos, ChunkPos, WorldAabb};

    use super::*;
    use crate::behavior::{BLOCK_BEHAVIORS, init_behaviors};
    use crate::block_entity::init_block_entities;
    use crate::entity::SharedEntity;
    use crate::entity::entities::ItemEntity;
    use crate::test_support::{fresh_test_world, insert_ready_full_chunk};

    fn brushable_test_world(key: &'static str) -> Arc<World> {
        init_vanilla_registry();
        init_behaviors();
        init_block_entities();
        let world = fresh_test_world(key);
        insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
        world
    }

    fn spawn_from_scheduled_tick(
        world: &Arc<World>,
        block: BlockRef,
        pos: BlockPos,
    ) -> SharedEntity {
        let state = block.default_state();
        assert!(world.set_block(pos, state, UpdateFlags::UPDATE_ALL));
        BLOCK_BEHAVIORS.get_behavior(block).tick(state, world, pos);

        let query = WorldAabb::new(
            f64::from(pos.x()),
            f64::from(pos.y()),
            f64::from(pos.z()),
            f64::from(pos.x() + 1),
            f64::from(pos.y() + 1),
            f64::from(pos.z() + 1),
        );
        let Some(entity) = world
            .get_entities_in_aabb(&query)
            .into_iter()
            .find(|entity| {
                entity
                    .as_ref()
                    .downcast_ref::<FallingBlockEntity>()
                    .is_some()
            })
        else {
            panic!("brushable block tick should spawn a falling block entity");
        };
        entity
    }

    fn tick_until_removed(entity: &SharedEntity) {
        for _ in 0..240 {
            if entity.is_removed() {
                return;
            }
            entity.set_old_position_to_current();
            entity.advance_tick_count();
            entity.tick();
        }
        panic!("falling brushable block did not settle within the test limit");
    }

    #[test]
    fn suspicious_sand_and_gravel_fall_then_break_without_drops() {
        let world = brushable_test_world("brushable_blocks_fall");

        for (x, block) in [
            (4, &vanilla_blocks::SUSPICIOUS_SAND),
            (8, &vanilla_blocks::SUSPICIOUS_GRAVEL),
        ] {
            let ground = BlockPos::new(x, 64, 4);
            assert!(world.set_block(
                ground,
                vanilla_blocks::STONE.default_state(),
                UpdateFlags::UPDATE_ALL,
            ));
            let entity = spawn_from_scheduled_tick(&world, block, BlockPos::new(x, 72, 4));

            let Some(falling) = entity.as_ref().downcast_ref::<FallingBlockEntity>() else {
                panic!("spawned entity should be a falling block");
            };
            assert_eq!(falling.block_state(), block.default_state());

            tick_until_removed(&entity);
            assert!(world.get_block_state(ground.above()).is_air());

            let query = WorldAabb::new(
                f64::from(ground.x() - 2),
                f64::from(ground.y()),
                f64::from(ground.z() - 2),
                f64::from(ground.x() + 3),
                f64::from(ground.y() + 4),
                f64::from(ground.z() + 3),
            );
            assert!(
                world
                    .get_entities_in_aabb(&query)
                    .iter()
                    .all(|entity| { entity.as_ref().downcast_ref::<ItemEntity>().is_none() })
            );
        }
    }
}
