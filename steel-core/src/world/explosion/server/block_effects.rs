//! Block callbacks, drops, and fire creation for server explosions.

use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::item_stack::ItemStack;
use steel_utils::BlockPos;
use steel_utils::random::Random;
use steel_utils::types::UpdateFlags;

use crate::behavior::BLOCK_BEHAVIORS;
use crate::behavior::blocks::FireBlock;
use crate::chunk::gameplay_chunk_lookup_cache::LocalFullChunkHolderCache;
use crate::entity::entities::ItemEntity;

use super::ServerExplosion;

const FIRE_CHANCE_DENOMINATOR: i32 = 3;
const MAX_DROPS_PER_COMBINED_STACK: i32 = 16;

impl ServerExplosion<'_> {
    pub(super) fn interact_with_blocks(&self, affected: &mut [BlockPos]) {
        self.world.with_random(|random| {
            vanilla_shuffle(affected, |bound| random.next_i32_bounded(bound));
        });
        let mut stacks = Vec::new();
        let mut full_chunks = LocalFullChunkHolderCache::new();

        for &pos in affected.iter() {
            let state = self
                .world
                .get_block_state_with_local_holder_cache(pos, &mut full_chunks);
            BLOCK_BEHAVIORS
                .get_behavior(state.get_block())
                .on_explosion_hit(state, self.world, pos, self, &mut |stack, stack_pos| {
                    add_or_append_stack(&mut stacks, stack, stack_pos);
                });
        }

        for stack in stacks {
            self.world.pop_resource(stack.pos, stack.stack);
        }
    }

    pub(super) fn create_fire(&self, affected: &[BlockPos]) {
        self.create_fire_with(affected, || {
            self.world
                .with_random(|random| random.next_i32_bounded(FIRE_CHANCE_DENOMINATOR))
        });
    }

    fn create_fire_with(&self, affected: &[BlockPos], mut next_int: impl FnMut() -> i32) {
        for &pos in affected {
            if next_int() == 0
                && self.world.get_block_state(pos).is_air()
                && self.world.get_block_state(pos.below()).is_solid_render()
            {
                self.world.set_block(
                    pos,
                    FireBlock::get_state(self.world.as_ref(), pos),
                    UpdateFlags::UPDATE_ALL,
                );
            }
        }
    }
}

fn vanilla_shuffle<T>(values: &mut [T], mut next_index: impl FnMut(i32) -> i32) {
    let Ok(length) = i32::try_from(values.len()) else {
        return;
    };
    for remaining in (2..=length).rev() {
        let swap_index = next_index(remaining) as usize;
        values.swap(remaining as usize - 1, swap_index);
    }
}

struct StackCollector {
    pos: BlockPos,
    stack: ItemStack,
}

fn add_or_append_stack(stacks: &mut Vec<StackCollector>, mut stack: ItemStack, pos: BlockPos) {
    for collector in stacks.iter_mut() {
        if ItemEntity::are_mergeable(&collector.stack, &stack) {
            let available = collector
                .stack
                .max_stack_size()
                .min(MAX_DROPS_PER_COMBINED_STACK)
                - collector.stack.count();
            let transferred = available.min(stack.count());
            collector.stack = collector
                .stack
                .copy_with_count(collector.stack.count() + transferred);
            stack.shrink(transferred);
            if stack.is_empty() {
                return;
            }
        }
    }
    stacks.push(StackCollector { pos, stack });
}

#[cfg(test)]
mod tests {
    use glam::DVec3;
    use steel_registry::item_stack::ItemStack;
    use steel_registry::{init_vanilla_registry, vanilla_blocks, vanilla_items};
    use steel_utils::types::UpdateFlags;
    use steel_utils::{BlockPos, ChunkPos};

    use super::*;
    use crate::behavior::init_behaviors;
    use crate::test_support::{fresh_test_world, insert_ready_full_chunk};
    use crate::world::explosion::BlockInteraction;

    #[test]
    fn vanilla_shuffle_uses_descending_bounded_draws() {
        let mut values = [0, 1, 2, 3];
        let mut bounds = Vec::new();
        let drawn_indexes = [1, 0, 1];
        let mut draw = 0;

        vanilla_shuffle(&mut values, |bound| {
            bounds.push(bound);
            let index = drawn_indexes[draw];
            draw += 1;
            index
        });

        let expected_descending_bounds = [4, 3, 2];
        let expected_shuffled_values = [2, 3, 0, 1];
        assert_eq!(bounds, expected_descending_bounds);
        assert_eq!(values, expected_shuffled_values);
    }

    #[test]
    fn fire_creation_draws_before_testing_air_and_support() {
        init_vanilla_registry();
        init_behaviors();
        let world = fresh_test_world("explosion_fire_order");
        insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
        let supported = BlockPos::new(1, 64, 1);
        let unsupported = supported.east();
        let occupied = unsupported.east();
        assert!(world.set_block(
            supported.below(),
            vanilla_blocks::STONE.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));
        assert!(world.set_block(
            occupied,
            vanilla_blocks::STONE.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));
        let explosion = ServerExplosion::new(
            &world,
            None,
            None,
            None,
            None,
            DVec3::ZERO,
            1.0,
            true,
            BlockInteraction::Destroy,
        );
        let mut draws = 0;

        explosion.create_fire_with(&[unsupported, occupied, supported], || {
            draws += 1;
            0
        });

        assert_eq!(draws, 3);
        assert!(world.get_block_state(unsupported).is_air());
        assert_eq!(
            world.get_block_state(occupied).get_block(),
            &vanilla_blocks::STONE
        );
        assert!(!world.get_block_state(supported).is_air());
    }

    #[test]
    fn combined_explosion_drops_never_exceed_vanilla_stack_limit() {
        const INPUT_STACK_SIZE: i32 = 10;
        const EXPECTED_STACK_COUNT: usize = 2;

        init_vanilla_registry();
        let stack = ItemStack::with_count(&vanilla_items::STONE, INPUT_STACK_SIZE);
        let mut stacks = Vec::new();

        add_or_append_stack(&mut stacks, stack.clone(), BlockPos::ZERO);
        add_or_append_stack(&mut stacks, stack, BlockPos::ZERO);

        assert_eq!(stacks.len(), EXPECTED_STACK_COUNT);
        assert_eq!(stacks[0].stack.count(), MAX_DROPS_PER_COMBINED_STACK);
        assert_eq!(
            stacks[1].stack.count(),
            INPUT_STACK_SIZE * 2 - MAX_DROPS_PER_COMBINED_STACK
        );
    }
}
