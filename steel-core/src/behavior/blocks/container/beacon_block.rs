//! Beacon block behavior implementation.
//!
//! Opens the beacon menu when right-clicked and owns the block entity and its
//! server ticker.

use std::sync::{Arc, Weak};

use steel_macros::block_behavior;
use steel_registry::block_entity_type::BlockEntityTypeRef;
use steel_registry::blocks::BlockRef;
use steel_registry::{DyeColor, vanilla_block_entity_types, vanilla_custom_stats};
use steel_utils::{BlockPos, BlockStateId, Downcast as _, translations};
use text_components::TextComponent;

use crate::behavior::InventoryAccess;
use crate::behavior::block::{BlockBehavior, BlockEntityCreation};
use crate::behavior::context::{BlockHitResult, BlockPlaceContext, InteractionResult};
use crate::block_entity::{BLOCK_ENTITIES, BlockEntityTicker, entities::BeaconBlockEntity};
use crate::inventory::menu::kinds::beacon;
use crate::player::Player;
use crate::world::World;

/// Behavior for the beacon block.
#[block_behavior]
pub struct BeaconBlock {
    block: BlockRef,
}

impl BeaconBlock {
    /// Creates a new beacon block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for BeaconBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }

    fn use_without_item(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        player: &Player,
        _hit_result: &BlockHitResult,
        _inv: &mut InventoryAccess,
    ) -> InteractionResult {
        // Vanilla `useWithoutItem` returns SUCCESS unconditionally, so a missing or mismatched
        // block entity still swallows the interaction rather than falling through to item use.
        // Deliberately not the `Pass` most other container blocks return.
        let Some(block_entity) = world.get_block_entity(pos) else {
            return InteractionResult::Success;
        };
        let Some(beacon_entity) = block_entity.downcast_ref::<BeaconBlockEntity>() else {
            return InteractionResult::Success;
        };
        let state = beacon_entity.state();
        let base = beacon_entity.base_handle();

        let inventory = player.inventory.clone();
        let world = Arc::clone(world);
        player.open_menu(
            TextComponent::translated(translations::CONTAINER_BEACON.msg()),
            move |context| beacon(inventory, context.container_id, pos, &world, state, base),
        );

        player.award_custom_stat(&vanilla_custom_stats::INTERACT_WITH_BEACON);
        InteractionResult::Success
    }

    /// Vanilla `BeaconBlock implements BeaconBeamBlock` with `DyeColor.WHITE`; the beacon is the
    /// first block its own scan visits, so this seeds the beam's initial section.
    fn beacon_beam_color(&self, _state: BlockStateId) -> Option<DyeColor> {
        Some(DyeColor::White)
    }

    fn new_block_entity(
        &self,
        level: Weak<World>,
        pos: BlockPos,
        state: BlockStateId,
    ) -> BlockEntityCreation {
        BlockEntityCreation::from_registered_factory(BLOCK_ENTITIES.create(
            &vanilla_block_entity_types::BEACON,
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
            &vanilla_block_entity_types::BEACON,
        )
    }
}
