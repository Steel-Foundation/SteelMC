//! Vault block behavior.
//!
//! Vanilla `VaultBlock.useWithoutItem` routes to `VaultServer.tryInsertKey`:
//! inserting a trial key into an unlocked vault consumes it and starts the
//! reward ejection.

use std::sync::{Arc, Weak};

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::vanilla_block_entity_types;
use steel_utils::{BlockPos, BlockStateId, Downcast as _};

use crate::behavior::InventoryAccess;
use crate::behavior::block::{BlockBehavior, BlockEntityCreation};
use crate::behavior::context::{BlockHitResult, BlockPlaceContext, InteractionResult};
use crate::block_entity::{BlockEntity as _, BLOCK_ENTITIES, BlockEntityTicker, entities::VaultBlockEntity};
use crate::player::Player;
use crate::world::World;

/// Vanilla `VaultBlock` behavior.
#[block_behavior]
pub struct VaultBlock {
    block: BlockRef,
}

impl VaultBlock {
    /// Creates vault behavior for `block`.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for VaultBlock {
    fn get_block_entity_ticker(
        &self,
        _world: &Arc<World>,
        _state: BlockStateId,
        block_entity_type: steel_registry::block_entity_type::BlockEntityTypeRef,
    ) -> Option<BlockEntityTicker> {
        BlockEntityTicker::for_matching_entity_tick(
            block_entity_type,
            &vanilla_block_entity_types::VAULT,
        )
    }

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
        inv: &mut InventoryAccess,
    ) -> InteractionResult {
        let Some(block_entity) = world.get_block_entity(pos) else {
            return InteractionResult::Pass;
        };
        let Some(vault) = block_entity.downcast_ref::<VaultBlockEntity>() else {
            return InteractionResult::Pass;
        };

        // Vanilla inspects and consumes the held stack through the hand slot.
        let inserted = inv.with_item(|stack| vault.try_insert_key(world, pos, player, stack));
        if inserted {
            if let Some(update_tag) = vault.get_update_tag() {
                world.broadcast_block_entity_update(pos, vault.get_type(), update_tag);
            }
            return InteractionResult::Success;
        }
        InteractionResult::Pass
    }

    fn new_block_entity(
        &self,
        level: Weak<World>,
        pos: BlockPos,
        state: BlockStateId,
    ) -> BlockEntityCreation {
        BlockEntityCreation::from_registered_factory(BLOCK_ENTITIES.create(
            &vanilla_block_entity_types::VAULT,
            level,
            pos,
            state,
        ))
    }
}
