//! Server-completed mining for custom blocks whose client carrier has different mining rules.

use std::sync::{Arc, Weak};

use steel_registry::{REGISTRY, blocks::block_state_ext::BlockStateExt, item_stack::ItemStack};
use steel_utils::{BlockPos, BlockStateId, types::GameType};

use crate::{player::Player, world::World};

pub(super) struct ServerMiningTarget {
    state: BlockStateId,
    tool: ItemStack,
    world: Weak<World>,
}

impl ServerMiningTarget {
    pub(super) fn new(player: &Player, world: &Arc<World>, state: BlockStateId) -> Option<Self> {
        let representation = REGISTRY.client_content.block(state.get_block())?;
        if !representation.server_mining {
            return None;
        }
        Some(Self {
            state,
            tool: player
                .inventory
                .lock()
                .get_selected_item()
                .copy_with_count(1),
            world: Arc::downgrade(world),
        })
    }

    pub(super) fn is_valid(
        &self,
        player: &Player,
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
    ) -> bool {
        state == self.state
            && self.world.ptr_eq(&Arc::downgrade(world))
            && matches!(player.game_mode(), GameType::Survival | GameType::Adventure)
            && player.is_within_block_interaction_range(pos)
            && world.may_interact(player, pos)
            && !player.block_action_restricted(world, pos)
            && ItemStack::is_same_item_same_components(
                &self.tool,
                player.inventory.lock().get_selected_item(),
            )
    }
}
