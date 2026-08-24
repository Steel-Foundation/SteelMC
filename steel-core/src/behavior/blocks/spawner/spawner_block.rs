//! Mob spawner block behavior - handles spawn egg interaction.

use std::sync::{Arc, Weak};

use steel_macros::block_behavior;
use steel_protocol::packets::game::CSystemChat;
use steel_registry::blocks::BlockRef;
use steel_registry::data_components::components::EntityData;
use steel_registry::data_components::vanilla_components;
use steel_registry::vanilla_block_entity_types;
use steel_registry::{vanilla_game_events, vanilla_game_rules};
use steel_utils::translations;
use steel_utils::types::{Difficulty, InteractionHand};
use steel_utils::{BlockPos, BlockStateId, Downcast as _};

use crate::{
    behavior::{
        BlockBehavior, BlockEntityCreation, BlockHitResult, BlockPlaceContext, InteractionResult,
        InventoryAccess,
    },
    block_entity::{BLOCK_ENTITIES, BlockEntity as _, entities::SpawnerBlockEntity},
    player::Player,
    world::{World, game_event::GameEventContext},
};

/// Vanilla `SpawnerBlock` behavior.
#[block_behavior]
pub struct SpawnerBlock {
    block: BlockRef,
}

impl SpawnerBlock {
    /// Creates spawner behavior for `block`.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for SpawnerBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }

    fn use_item_on(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        player: &Player,
        _hand: InteractionHand,
        _hit: &BlockHitResult,
        inv: &mut InventoryAccess,
    ) -> InteractionResult {
        // Vanilla routes this interaction through `SpawnEggItem.useOn`, keyed by
        // the held stack's ENTITY_DATA component; Steel dispatches item-on-block
        // use through the block behavior instead.
        let Some(entity_type) = inv.with_item(|stack| {
            stack
                .get(vanilla_components::ENTITY_DATA)
                .map(EntityData::entity_type)
        }) else {
            return InteractionResult::Pass;
        };

        // Vanilla `EntityType.canSpawn`: hostile types are rejected on Peaceful.
        // Feature-flag gating is omitted because Steel always runs with every
        // feature enabled.
        if !entity_type.allowed_in_peaceful && world.difficulty() == Difficulty::Peaceful {
            return InteractionResult::Fail;
        }

        let Some(block_entity) = world.get_block_entity(pos) else {
            return InteractionResult::Pass;
        };
        let Some(spawner) = block_entity.downcast_ref::<SpawnerBlockEntity>() else {
            return InteractionResult::Pass;
        };

        if !world.get_game_rule(&vanilla_game_rules::SPAWNER_BLOCKS_WORK) {
            player.send_packet(CSystemChat {
                content: translations::ADV_MODE_NOT_ENABLED_SPAWNER.msg().component(),
                overlay: false,
            });
            return InteractionResult::Fail;
        }

        spawner.set_entity_id(entity_type);
        if let Some(update_tag) = spawner.get_update_tag() {
            world.broadcast_block_entity_update(pos, spawner.get_type(), update_tag);
        }
        world.game_event(
            &vanilla_game_events::BLOCK_CHANGE,
            pos,
            &GameEventContext::new(Some(player), None),
        );

        inv.with_item(|stack| stack.shrink(1));

        InteractionResult::Success
    }

    fn new_block_entity(
        &self,
        level: Weak<World>,
        pos: BlockPos,
        state: BlockStateId,
    ) -> BlockEntityCreation {
        BlockEntityCreation::from_registered_factory(BLOCK_ENTITIES.create(
            &vanilla_block_entity_types::MOB_SPAWNER,
            level,
            pos,
            state,
        ))
    }
}
