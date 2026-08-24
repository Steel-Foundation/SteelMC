//! Vanilla adventure-mode item component checks.

use steel_registry::{
    blocks::properties::Direction,
    data_components::{
        AdventureModePredicate, BlockPredicate,
        vanilla_components::{CAN_BREAK, CAN_PLACE_ON},
    },
    item_stack::ItemStack,
};
use steel_utils::{BlockPos, nbt::compare_nbt_compounds};

use crate::{player::Player, world::World};

impl Player {
    /// Mirrors vanilla `Player.mayUseItemAt`.
    #[must_use]
    pub(crate) fn may_use_item_at(
        &self,
        pos: BlockPos,
        direction: Direction,
        item: &ItemStack,
    ) -> bool {
        if self.abilities.lock().may_build {
            return true;
        }

        let world = self.get_world();
        can_place_on(item, &world, direction.opposite().relative(pos))
    }
}

/// Mirrors `ItemStack.canBreakBlockInAdventureMode`.
#[must_use]
pub(super) fn can_break(item: &ItemStack, world: &World, pos: BlockPos) -> bool {
    item.get(CAN_BREAK)
        .is_some_and(|predicate| matches(predicate, world, pos))
}

/// Mirrors `ItemStack.canPlaceOnBlockInAdventureMode`.
#[must_use]
pub(super) fn can_place_on(item: &ItemStack, world: &World, pos: BlockPos) -> bool {
    item.get(CAN_PLACE_ON)
        .is_some_and(|predicate| matches(predicate, world, pos))
}

fn matches(predicate: &AdventureModePredicate, world: &World, pos: BlockPos) -> bool {
    predicate
        .predicates()
        .iter()
        .any(|predicate| block_predicate_matches(predicate, world, pos))
}

fn block_predicate_matches(predicate: &BlockPredicate, world: &World, pos: BlockPos) -> bool {
    if !predicate.matches_state(world.get_block_state(pos)) {
        return false;
    }

    let Some(expected_nbt) = predicate.nbt() else {
        return true;
    };
    let Some(block_entity) = world.get_block_entity(pos) else {
        return false;
    };

    // Vanilla's `BlockPredicate.matches(BlockInWorld)` deliberately ignores
    // data-component matchers and evaluates only the block entity's full NBT.
    let actual_nbt = block_entity.save_with_full_metadata();
    compare_nbt_compounds(expected_nbt.tag(), &actual_nbt, true)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use steel_registry::{
        RegistryHolderSet,
        blocks::properties::Direction,
        data_component_predicate::{DataComponentExactPredicate, DataComponentMatchers},
        data_components::{
            AdventureModePredicate, BlockPredicate, DataComponentMap,
            vanilla_components::{CAN_BREAK, CAN_PLACE_ON, DAMAGE},
        },
        init_vanilla_registry,
        item_predicate::{
            StatePropertiesPredicate, StatePropertyMatcher, StatePropertyValueMatcher,
        },
        item_stack::ItemStack,
        vanilla_blocks, vanilla_items,
    };
    use steel_utils::{
        BlockPos, ChunkPos,
        types::{GameType, UpdateFlags},
    };

    use crate::{
        behavior::init_behaviors,
        test_support::{TestPlayerBuilder, fresh_test_world, insert_ready_full_chunk},
    };

    use super::can_break;

    #[test]
    fn may_use_item_at_tests_the_block_behind_the_placement_face() {
        init_vanilla_registry();
        init_behaviors();
        let world = fresh_test_world("adventure_mode_may_use_item_at");
        let clicked_pos = BlockPos::new(1, 64, 0);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(clicked_pos));
        assert!(world.set_block(
            clicked_pos,
            vanilla_blocks::STONE.default_state(),
            UpdateFlags::UPDATE_ALL,
        ));

        let player = TestPlayerBuilder::new(Arc::clone(&world), "TestPlayer", 1).build();
        player.restore_game_modes(GameType::Adventure, None);
        player
            .abilities
            .lock()
            .update_for_game_mode(GameType::Adventure);

        let placement_pos = clicked_pos.above();
        let plain_bucket = ItemStack::new(&vanilla_items::WATER_BUCKET);
        assert!(!player.may_use_item_at(placement_pos, Direction::Up, &plain_bucket));

        let predicate = BlockPredicate::new(
            Some(RegistryHolderSet::Direct(vec![&vanilla_blocks::STONE])),
            None,
            None,
            DataComponentMatchers::ANY,
        );
        let mut allowed_bucket = plain_bucket.clone();
        allowed_bucket.set(
            CAN_PLACE_ON,
            AdventureModePredicate::new(vec![predicate]).expect("one block predicate is valid"),
        );
        assert!(player.may_use_item_at(placement_pos, Direction::Up, &allowed_bucket));

        player.abilities.lock().may_build = true;
        assert!(player.may_use_item_at(placement_pos, Direction::Up, &plain_bucket));
    }

    #[test]
    fn block_in_world_predicates_ignore_component_matchers() {
        init_vanilla_registry();
        init_behaviors();
        let world = fresh_test_world("adventure_mode_ignores_component_matchers");
        let pos = BlockPos::new(1, 64, 0);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));
        assert!(world.set_block(
            pos,
            vanilla_blocks::OAK_LOG.default_state(),
            UpdateFlags::UPDATE_ALL,
        ));

        let state_properties = StatePropertiesPredicate::new(vec![StatePropertyMatcher::new(
            "axis".to_owned(),
            StatePropertyValueMatcher::Exact("y".to_owned()),
        )])
        .expect("one state property has a unique name");
        let mut expected_components = DataComponentMap::new();
        expected_components.set(DAMAGE, Some(1));
        let component_matchers = DataComponentMatchers::new(
            DataComponentExactPredicate::all_of(&expected_components)
                .expect("damage is persistable"),
            Vec::new(),
        )
        .expect("one exact matcher is valid");
        let predicate = BlockPredicate::new(
            Some(RegistryHolderSet::Direct(vec![&vanilla_blocks::OAK_LOG])),
            Some(state_properties),
            None,
            component_matchers,
        );
        let mut item = ItemStack::new(&vanilla_items::DIAMOND_PICKAXE);
        item.set(
            CAN_BREAK,
            AdventureModePredicate::new(vec![predicate]).expect("one block predicate is valid"),
        );

        assert!(can_break(&item, &world, pos));
    }
}
