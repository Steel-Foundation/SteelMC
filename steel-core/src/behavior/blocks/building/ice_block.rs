use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_enchantment_tags::EnchantmentTag;
use steel_registry::{REGISTRY, RegistryExt, TaggedRegistryExt, vanilla_blocks};
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::{BlockBehavior, BlockPlaceContext};
use crate::block_entity::SharedBlockEntity;
use crate::chunk::light::LightLayer;
use crate::player::Player;
use crate::world::World;

/// Vanilla `IceBlock` behavior.
#[block_behavior]
pub struct IceBlock {
    block: BlockRef,
}

pub const BASE_MELT_LIGHT_LEVEL: u8 = 11;

impl IceBlock {
    /// Creates an ice block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    /// Vanilla `IceBlock.meltsInto`.
    #[must_use]
    pub fn melts_into() -> BlockStateId {
        vanilla_blocks::WATER.default_state()
    }

    /// Vanilla `IceBlock.melt`.
    pub fn melt(_state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        if world.dimension_type.water_evaporates {
            world.set_block(
                pos,
                vanilla_blocks::AIR.default_state(),
                UpdateFlags::UPDATE_ALL,
            );
        } else {
            world.set_block(pos, Self::melts_into(), UpdateFlags::UPDATE_ALL);
            world.update_neighbors_at(pos, Self::melts_into().get_block());
        }
    }

    /// Checks if tool prevents ice from melting (e.g. Silk Touch / `PREVENTS_ICE_MELTING` tag).
    #[must_use]
    pub fn prevents_ice_melting(tool: &ItemStack) -> bool {
        let Some(enchantments) = tool.get_enchantments() else {
            return false;
        };
        enchantments.iter().any(|(key, _)| {
            REGISTRY.enchantments.by_key(key).is_some_and(|ench| {
                REGISTRY
                    .enchantments
                    .is_in_tag(ench, &EnchantmentTag::PREVENTS_ICE_MELTING)
            })
        })
    }
}

impl BlockBehavior for IceBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }

    fn player_destroy(
        &self,
        world: &Arc<World>,
        _player: &Player,
        pos: BlockPos,
        _state: BlockStateId,
        _block_entity: Option<&SharedBlockEntity>,
        tool: &ItemStack,
    ) {
        if !Self::prevents_ice_melting(tool) {
            if world.dimension_type.water_evaporates {
                world.set_block(
                    pos,
                    vanilla_blocks::AIR.default_state(),
                    UpdateFlags::UPDATE_ALL,
                );
                return;
            }

            let below_state = world.get_block_state(pos.below());
            if below_state.blocks_motion()
                || !below_state.get_fluid_state().is_empty()
                || below_state.get_block().config.liquid
            {
                world.set_block(pos, Self::melts_into(), UpdateFlags::UPDATE_ALL);
            }
        }
    }
    fn random_tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        if world.light_value_at(LightLayer::Block, pos)
            > BASE_MELT_LIGHT_LEVEL - state.get_light_dampening()
        {
            Self::melt(state, world, pos);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::IceBlock;
    use steel_registry::item_stack::ItemStack;
    use steel_registry::test_support::init_test_registry;
    use steel_registry::{vanilla_blocks, vanilla_enchantments, vanilla_items};

    #[test]
    fn melts_into_water_by_default() {
        init_test_registry();
        assert_eq!(
            IceBlock::melts_into(),
            vanilla_blocks::WATER.default_state()
        );
    }

    #[test]
    fn prevents_ice_melting_with_silk_touch() {
        init_test_registry();
        let mut tool = ItemStack::new(&vanilla_items::DIAMOND_PICKAXE);
        assert!(!IceBlock::prevents_ice_melting(&tool));

        tool.upgrade_enchantment(vanilla_enchantments::SILK_TOUCH.key.clone(), 1);
        assert!(IceBlock::prevents_ice_melting(&tool));
    }
}
