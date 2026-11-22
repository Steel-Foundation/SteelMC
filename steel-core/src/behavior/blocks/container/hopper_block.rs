//! Hopper block behavior implementation.
//!
//! Vanilla `HopperBlock`: pulls items from containers and item entities above
//! and pushes them into the container it faces. Redstone power disables it.

use std::sync::{Arc, Weak};

use steel_macros::block_behavior;
use steel_registry::block_entity_type::BlockEntityTypeRef;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::Direction;
use steel_registry::vanilla_block_entity_types;
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId, Downcast};

use crate::behavior::block::{BlockBehavior, BlockEntityCreation};
use crate::behavior::context::BlockPlaceContext;
use crate::block_entity::entities::{ENABLED, FACING, HopperBlockEntity};
use crate::block_entity::{BLOCK_ENTITIES, BlockEntityTicker};
use crate::entity::{Entity, InsideBlockEffectCollector};
use crate::inventory::container::calculate_redstone_signal_from_containers;
use crate::inventory::lock::{ContainerLockGuard, ContainerRef};
use crate::world::{LevelReader, SignalGetter, World};

/// Behavior for hopper blocks.
#[block_behavior]
pub struct HopperBlock {
    block: BlockRef,
}

impl HopperBlock {
    /// Creates a new hopper block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    /// Vanilla `HopperBlock.checkPoweredState`: enabled only without a neighbor signal.
    #[expect(
        clippy::unused_self,
        reason = "trait-style helper kept on the block behavior for parity with vanilla"
    )]
    fn check_powered_state(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        let enabled = !world.has_neighbor_signal(pos);
        if state.get_value(ENABLED) != enabled {
            world.set_block(
                pos,
                state.set_value(ENABLED, enabled),
                UpdateFlags::UPDATE_CLIENTS,
            );
        }
    }
}

impl BlockBehavior for HopperBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        // Vanilla: face opposite the clicked face; vertical placements point down.
        let facing = match context.clicked_face().opposite() {
            Direction::Up | Direction::Down => Direction::Down,
            dir => dir,
        };
        Some(
            self.block
                .default_state()
                .set_value(FACING, facing)
                .set_value(ENABLED, true),
        )
    }

    fn on_place(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        old_state: BlockStateId,
        _moved_by_piston: bool,
    ) {
        if old_state.get_block() != state.get_block() {
            self.check_powered_state(state, world, pos);
        }
    }

    fn handle_neighbor_changed(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _source_block: BlockRef,
        _moved_by_piston: bool,
    ) {
        self.check_powered_state(state, world, pos);
    }

    fn new_block_entity(
        &self,
        level: Weak<World>,
        pos: BlockPos,
        state: BlockStateId,
    ) -> BlockEntityCreation {
        BlockEntityCreation::from_registered_factory(BLOCK_ENTITIES.create(
            &vanilla_block_entity_types::HOPPER,
            level,
            pos,
            state,
        ))
    }

    fn get_block_entity_ticker(
        &self,
        _world: &Arc<World>,
        _state: BlockStateId,
        block_entity_type: BlockEntityTypeRef,
    ) -> Option<BlockEntityTicker> {
        BlockEntityTicker::for_matching_entity_tick(
            block_entity_type,
            &vanilla_block_entity_types::HOPPER,
        )
    }

    fn entity_inside(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        entity: &dyn Entity,
        _effect_collector: &mut InsideBlockEffectCollector,
        _is_precise: bool,
    ) {
        let Some(block_entity) = world.get_block_entity(pos) else {
            return;
        };
        let Some(hopper) = block_entity.downcast_ref::<HopperBlockEntity>() else {
            return;
        };
        HopperBlockEntity::entity_inside(world, pos, state, entity, hopper);
    }

    fn has_analog_output_signal(&self, _state: BlockStateId) -> bool {
        true
    }

    fn get_analog_output_signal(
        &self,
        _state: BlockStateId,
        world: &World,
        pos: BlockPos,
        _direction: Direction,
    ) -> i32 {
        let Some(container_ref) = world
            .get_block_entity(pos)
            .and_then(ContainerRef::from_block_entity)
        else {
            return 0;
        };
        let Ok(guard) = ContainerLockGuard::lock_all(&[&container_ref]) else {
            return 0;
        };
        calculate_redstone_signal_from_containers(guard.get(container_ref.container_id()))
    }
}
