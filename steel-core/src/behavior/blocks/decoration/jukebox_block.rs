//! Vanilla jukebox block behavior.

use std::ptr;
use std::sync::{Arc, Weak};

use steel_macros::block_behavior;
use steel_registry::block_entity_type::BlockEntityTypeRef;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{BlockStateProperties, Direction};
use steel_registry::data_components::vanilla_components;
use steel_registry::{vanilla_block_entity_types, vanilla_game_events};
use steel_utils::types::{InteractionHand, UpdateFlags};
use steel_utils::{BlockPos, BlockStateId, Downcast as _};

use crate::behavior::blocks::{MAX_REDSTONE_SIGNAL, MIN_REDSTONE_SIGNAL};
use crate::behavior::{
    BlockBehavior, BlockEntityCreation, BlockHitResult, BlockPlaceContext, InteractionResult,
    InventoryAccess, PlacementSource,
};
use crate::block_entity::entities::JukeboxBlockEntity;
use crate::block_entity::{BLOCK_ENTITIES, BlockEntityTicker};
use crate::entity::Entity;
use crate::player::Player;
use crate::world::game_event::GameEventContext;
use crate::world::{LevelReader, SignalQueryContext, World};

/// Vanilla `JukeboxBlock` behavior.
#[block_behavior]
pub struct JukeboxBlock {
    block: BlockRef,
}

impl JukeboxBlock {
    /// Creates jukebox behavior for `block`.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for JukeboxBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }

    fn set_placed_by(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        source: &PlacementSource<'_>,
    ) {
        let (block_entity_data, has_record) = source.with_item(|stack| {
            let Some(data) = stack.get(vanilla_components::BLOCK_ENTITY_DATA) else {
                return (None, false);
            };
            let has_record = data.data().as_compound().get("RecordItem").is_some();
            (
                Some((data.block_entity_type(), data.data().copy_tag())),
                has_record,
            )
        });

        if let Some((block_entity_type, data)) = block_entity_data
            && ptr::eq(
                block_entity_type,
                &raw const vanilla_block_entity_types::JUKEBOX,
            )
            && let Some(block_entity) = world.get_block_entity(pos)
            && let Some(jukebox) = block_entity.downcast_ref::<JukeboxBlockEntity>()
        {
            jukebox.apply_item_block_entity_data(data);
        }

        // Vanilla derives this flag from the raw placement data, even when its
        // declared block-entity type does not match the placed jukebox.
        if has_record {
            world.set_block(
                pos,
                state.set_value(&BlockStateProperties::HAS_RECORD, true),
                UpdateFlags::UPDATE_CLIENTS,
            );
        }
    }

    fn affect_neighbors_after_removal(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _moved_by_piston: bool,
    ) {
        world.update_neighbor_for_output_signal(pos, self.block);
    }

    fn use_item_on(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        player: &Player,
        _hand: InteractionHand,
        _hit_result: &BlockHitResult,
        inv: &mut InventoryAccess,
    ) -> InteractionResult {
        if state.get_value(&BlockStateProperties::HAS_RECORD) {
            return InteractionResult::TryEmptyHandInteraction;
        }

        let record = inv.with_item(|stack| {
            stack.get(vanilla_components::JUKEBOX_PLAYABLE)?;
            Some(if player.has_infinite_materials() {
                stack.copy_with_count(1)
            } else {
                stack.split(1)
            })
        });
        let Some(record) = record else {
            return InteractionResult::TryEmptyHandInteraction;
        };

        if let Some(block_entity) = world.get_block_entity(pos)
            && let Some(jukebox) = block_entity.downcast_ref::<JukeboxBlockEntity>()
        {
            jukebox.set_the_item(record);
            world.game_event(
                &vanilla_game_events::BLOCK_CHANGE,
                pos,
                &GameEventContext::new(Some(player as &dyn Entity), Some(state)),
            );
        }
        InteractionResult::Success
    }

    fn use_without_item(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _player: &Player,
        _hit_result: &BlockHitResult,
        _inv: &mut InventoryAccess,
    ) -> InteractionResult {
        if !state.get_value(&BlockStateProperties::HAS_RECORD) {
            return InteractionResult::Pass;
        }
        let Some(block_entity) = world.get_block_entity(pos) else {
            return InteractionResult::Pass;
        };
        let Some(jukebox) = block_entity.downcast_ref::<JukeboxBlockEntity>() else {
            return InteractionResult::Pass;
        };

        jukebox.pop_out_the_item();
        InteractionResult::Success
    }

    fn is_signal_source(&self, _state: BlockStateId, _context: SignalQueryContext) -> bool {
        true
    }

    fn get_own_signal(
        &self,
        _state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
        _context: SignalQueryContext,
    ) -> i32 {
        let Some(block_entity) = world.get_block_entity(pos) else {
            return MIN_REDSTONE_SIGNAL;
        };
        let Some(jukebox) = block_entity.downcast_ref::<JukeboxBlockEntity>() else {
            return MIN_REDSTONE_SIGNAL;
        };
        if jukebox.is_record_playing() {
            MAX_REDSTONE_SIGNAL
        } else {
            MIN_REDSTONE_SIGNAL
        }
    }

    fn has_analog_output_signal(&self, _state: BlockStateId) -> bool {
        true
    }

    fn get_analog_output_signal(
        &self,
        _state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
        _direction: Direction,
    ) -> i32 {
        let Some(block_entity) = world.get_block_entity(pos) else {
            return MIN_REDSTONE_SIGNAL;
        };
        let Some(jukebox) = block_entity.downcast_ref::<JukeboxBlockEntity>() else {
            return MIN_REDSTONE_SIGNAL;
        };
        jukebox.analog_output_signal()
    }

    fn new_block_entity(
        &self,
        level: Weak<World>,
        pos: BlockPos,
        state: BlockStateId,
    ) -> BlockEntityCreation {
        BlockEntityCreation::from_registered_factory(BLOCK_ENTITIES.create(
            &vanilla_block_entity_types::JUKEBOX,
            level,
            pos,
            state,
        ))
    }

    fn get_block_entity_ticker(
        &self,
        _world: &Arc<World>,
        state: BlockStateId,
        block_entity_type: BlockEntityTypeRef,
    ) -> Option<BlockEntityTicker> {
        if !state.get_value(&BlockStateProperties::HAS_RECORD) {
            return None;
        }
        BlockEntityTicker::for_matching_entity_tick(
            block_entity_type,
            &vanilla_block_entity_types::JUKEBOX,
        )
    }
}
