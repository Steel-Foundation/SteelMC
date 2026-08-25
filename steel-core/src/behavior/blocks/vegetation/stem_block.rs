use std::sync::Arc;

use rand::{Rng, RngExt};
use steel_macros::block_behavior;
use steel_registry::{
    blocks::{
        BlockRef,
        block_state_ext::BlockStateExt,
        properties::{BlockStateProperties, EnumProperty, IntProperty},
    },
    item_stack::ItemStack,
    items::ItemRef,
};
use steel_utils::{BlockPos, BlockStateId, Direction, Identifier, types::UpdateFlags};

use crate::{
    behavior::{
        BlockBehavior, BlockPlaceContext,
        blocks::vegetation::{
            Vegetation,
            crop_block::{CROP_GROWTH_CHANCE_BASE, crop_growth_speed},
            default_surviving_state,
            vegetation_block::{survival_update_shape, vegetation_can_survive},
        },
    },
    world::{LevelAccessor, LevelReader, ScheduledTickAccess, World},
};

const AGE: &IntProperty = &BlockStateProperties::AGE_7;
const FACING: &EnumProperty<Direction> = &BlockStateProperties::HORIZONTAL_FACING;
const MAX_AGE: u8 = 7;

/// Vanilla pumpkin and melon stem behavior.
#[block_behavior]
pub struct StemBlock {
    block: BlockRef,
    #[json_arg(vanilla_blocks)]
    fruit: BlockRef,
    #[json_arg(vanilla_blocks)]
    attached_stem: BlockRef,
    #[json_arg(vanilla_items)]
    seed: ItemRef,
    #[json_arg(vanilla_block_tags)]
    stem_support_blocks: Identifier,
    #[json_arg(vanilla_block_tags)]
    fruit_support_blocks: Identifier,
}

impl StemBlock {
    /// Creates a stem with its extracted fruit, attached stem, seed, and support tags.
    #[must_use]
    pub const fn new(
        block: BlockRef,
        fruit: BlockRef,
        attached_stem: BlockRef,
        seed: ItemRef,
        stem_support_blocks: Identifier,
        fruit_support_blocks: Identifier,
    ) -> Self {
        Self {
            block,
            fruit,
            attached_stem,
            seed,
            stem_support_blocks,
            fruit_support_blocks,
        }
    }

    fn age_after_bonemeal(age: u8, increase: u8) -> u8 {
        age.saturating_add(increase).min(MAX_AGE)
    }

    fn random_tick_with_rng(
        &self,
        state: BlockStateId,
        world: &dyn LevelAccessor,
        pos: BlockPos,
        rng: &mut dyn Rng,
    ) {
        if world.raw_brightness(pos, 0) < 9 {
            return;
        }

        let growth_speed = crop_growth_speed(self.block, world, pos);
        let growth_chance = (CROP_GROWTH_CHANCE_BASE / growth_speed) as u32 + 1;
        if rng.random_range(0..growth_chance) != 0 {
            return;
        }

        let age = state.get_value(AGE);
        if age < MAX_AGE {
            world.set_block_state(
                pos,
                state.set_value(AGE, age + 1),
                UpdateFlags::UPDATE_CLIENTS,
            );
            return;
        }

        let direction = Direction::HORIZONTAL[rng.random_range(0..Direction::HORIZONTAL.len())];
        let fruit_pos = pos.relative(direction);
        if !world.get_block_state(fruit_pos).is_air()
            || !world
                .get_block_state(fruit_pos.below())
                .get_block()
                .has_tag(&self.fruit_support_blocks)
        {
            return;
        }

        world.set_block_state(
            fruit_pos,
            self.fruit.default_state(),
            UpdateFlags::UPDATE_ALL,
        );
        world.set_block_state(
            pos,
            self.attached_stem
                .default_state()
                .set_value(FACING, direction),
            UpdateFlags::UPDATE_ALL,
        );
    }

    fn perform_bonemeal_with_rng(
        &self,
        state: BlockStateId,
        world: &dyn LevelAccessor,
        rng: &mut dyn Rng,
        pos: BlockPos,
    ) {
        let age = state.get_value(AGE);
        let new_age = Self::age_after_bonemeal(age, rng.random_range(2..=5));
        let new_state = state.set_value(AGE, new_age);
        world.set_block_state(pos, new_state, UpdateFlags::UPDATE_CLIENTS);

        if new_age == MAX_AGE {
            self.random_tick_with_rng(new_state, world, pos, rng);
        }
    }
}

impl BlockBehavior for StemBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        default_surviving_state(self.block, self, context)
    }

    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        vegetation_can_survive(self, state, world, pos)
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
        survival_update_shape(self, state, world, pos)
    }

    fn random_tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        self.random_tick_with_rng(state, world, pos, &mut rand::rng());
    }

    fn get_clone_item_stack(
        &self,
        _block: BlockRef,
        _state: BlockStateId,
        _include_data: bool,
    ) -> Option<ItemStack> {
        Some(ItemStack::new(self.seed))
    }

    fn as_bonemealable(&self) -> Option<&dyn super::bonemealable::Bonemealable> {
        Some(self)
    }
}

impl Vegetation for StemBlock {
    fn may_place_on(&self, state: BlockStateId, _world: &dyn LevelReader, _pos: BlockPos) -> bool {
        state.get_block().has_tag(&self.stem_support_blocks)
    }
}

impl super::bonemealable::Bonemealable for StemBlock {
    fn get_bonemeal_age_increase(&self, _world: &Arc<World>, rng: &mut dyn Rng) -> u8 {
        rng.random_range(2..=5)
    }

    fn is_valid_bonemeal_target(
        &self,
        state: BlockStateId,
        _world: &dyn LevelReader,
        _pos: BlockPos,
    ) -> bool {
        state.get_value(AGE) != MAX_AGE
    }

    fn perform_bonemeal(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        rng: &mut dyn Rng,
        pos: BlockPos,
    ) {
        self.perform_bonemeal_with_rng(state, world, rng, pos);
    }
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;

    use rand::TryRng;
    use steel_registry::{
        init_vanilla_registry, vanilla_block_tags::BlockTag, vanilla_blocks, vanilla_items,
    };

    use crate::{chunk::light::MAX_LIGHT_LEVEL, test_support::TestLevel};

    use super::super::bonemealable::Bonemealable;
    use super::*;

    #[derive(Default)]
    struct ZeroRng;

    impl TryRng for ZeroRng {
        type Error = Infallible;

        fn try_next_u32(&mut self) -> Result<u32, Self::Error> {
            Ok(0)
        }

        fn try_next_u64(&mut self) -> Result<u64, Self::Error> {
            Ok(0)
        }

        fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<(), Self::Error> {
            dst.fill(0);
            Ok(())
        }
    }

    fn pumpkin_stem() -> StemBlock {
        StemBlock::new(
            &vanilla_blocks::PUMPKIN_STEM,
            &vanilla_blocks::PUMPKIN,
            &vanilla_blocks::ATTACHED_PUMPKIN_STEM,
            &vanilla_items::PUMPKIN_SEEDS,
            BlockTag::SUPPORTS_PUMPKIN_STEM,
            BlockTag::SUPPORTS_PUMPKIN_STEM_FRUIT,
        )
    }

    fn melon_stem() -> StemBlock {
        StemBlock::new(
            &vanilla_blocks::MELON_STEM,
            &vanilla_blocks::MELON,
            &vanilla_blocks::ATTACHED_MELON_STEM,
            &vanilla_items::MELON_SEEDS,
            BlockTag::SUPPORTS_MELON_STEM,
            BlockTag::SUPPORTS_MELON_STEM_FRUIT,
        )
    }

    #[test]
    fn survival_uses_extracted_support_without_a_light_requirement() {
        init_vanilla_registry();
        let stem = pumpkin_stem();
        let state = vanilla_blocks::PUMPKIN_STEM.default_state();
        let farmland = TestLevel::default()
            .with_block(
                BlockPos::ZERO.below(),
                vanilla_blocks::FARMLAND.default_state(),
            )
            .with_raw_brightness(0);
        let dirt = TestLevel::default()
            .with_block(BlockPos::ZERO.below(), vanilla_blocks::DIRT.default_state())
            .with_raw_brightness(MAX_LIGHT_LEVEL);

        assert!(stem.can_survive(state, &farmland, BlockPos::ZERO));
        assert!(!stem.can_survive(state, &dirt, BlockPos::ZERO));
    }

    #[test]
    fn random_growth_requires_light_nine_and_advances_one_age() {
        init_vanilla_registry();
        let stem = pumpkin_stem();
        let state = vanilla_blocks::PUMPKIN_STEM
            .default_state()
            .set_value(AGE, 3);
        let dark = TestLevel::default()
            .with_block(
                BlockPos::ZERO.below(),
                vanilla_blocks::FARMLAND.default_state(),
            )
            .with_raw_brightness(8);
        stem.random_tick_with_rng(state, &dark, BlockPos::ZERO, &mut ZeroRng);
        assert!(dark.placed_blocks.borrow().is_empty());

        let bright = TestLevel::default()
            .with_block(
                BlockPos::ZERO.below(),
                vanilla_blocks::FARMLAND.default_state(),
            )
            .with_raw_brightness(9);
        stem.random_tick_with_rng(state, &bright, BlockPos::ZERO, &mut ZeroRng);
        let placed = bright.placed_blocks.borrow();
        assert_eq!(placed.len(), 1);
        assert_eq!(placed[0].state.get_value(AGE), 4);
        assert_eq!(placed[0].flags, UpdateFlags::UPDATE_CLIENTS);
    }

    #[test]
    fn mature_stems_place_their_own_fruit_and_attached_family() {
        init_vanilla_registry();

        for (stem, mature_state, fruit, attached) in [
            (
                pumpkin_stem(),
                vanilla_blocks::PUMPKIN_STEM
                    .default_state()
                    .set_value(AGE, MAX_AGE),
                &vanilla_blocks::PUMPKIN,
                &vanilla_blocks::ATTACHED_PUMPKIN_STEM,
            ),
            (
                melon_stem(),
                vanilla_blocks::MELON_STEM
                    .default_state()
                    .set_value(AGE, MAX_AGE),
                &vanilla_blocks::MELON,
                &vanilla_blocks::ATTACHED_MELON_STEM,
            ),
        ] {
            let fruit_pos = BlockPos::ZERO.north();
            let level = TestLevel::default()
                .with_block(
                    BlockPos::ZERO.below(),
                    vanilla_blocks::FARMLAND.default_state(),
                )
                .with_block(fruit_pos.below(), vanilla_blocks::DIRT.default_state())
                .with_raw_brightness(9);
            stem.random_tick_with_rng(mature_state, &level, BlockPos::ZERO, &mut ZeroRng);

            let placed = level.placed_blocks.borrow();
            assert_eq!(placed.len(), 2);
            assert_eq!(placed[0].pos, fruit_pos);
            assert_eq!(placed[0].state.get_block(), fruit);
            assert_eq!(placed[0].flags, UpdateFlags::UPDATE_ALL);
            assert_eq!(placed[1].pos, BlockPos::ZERO);
            assert_eq!(placed[1].state.get_block(), attached);
            assert_eq!(placed[1].state.get_value(FACING), Direction::North);
            assert_eq!(placed[1].flags, UpdateFlags::UPDATE_ALL);
        }
    }

    #[test]
    fn mature_growth_requires_air_and_fruit_support_and_does_not_scan() {
        init_vanilla_registry();
        let stem = pumpkin_stem();
        let mature = vanilla_blocks::PUMPKIN_STEM
            .default_state()
            .set_value(AGE, MAX_AGE);

        let unsupported = TestLevel::default()
            .with_block(
                BlockPos::ZERO.below(),
                vanilla_blocks::FARMLAND.default_state(),
            )
            .with_block(
                BlockPos::ZERO.north().below(),
                vanilla_blocks::STONE.default_state(),
            )
            .with_raw_brightness(9);
        stem.random_tick_with_rng(mature, &unsupported, BlockPos::ZERO, &mut ZeroRng);
        assert!(unsupported.placed_blocks.borrow().is_empty());

        let blocked_north = TestLevel::default()
            .with_block(
                BlockPos::ZERO.below(),
                vanilla_blocks::FARMLAND.default_state(),
            )
            .with_block(
                BlockPos::ZERO.north(),
                vanilla_blocks::STONE.default_state(),
            )
            .with_block(
                BlockPos::ZERO.east().below(),
                vanilla_blocks::DIRT.default_state(),
            )
            .with_block(
                BlockPos::ZERO.south().below(),
                vanilla_blocks::DIRT.default_state(),
            )
            .with_block(
                BlockPos::ZERO.west().below(),
                vanilla_blocks::DIRT.default_state(),
            )
            .with_raw_brightness(9);
        stem.random_tick_with_rng(mature, &blocked_north, BlockPos::ZERO, &mut ZeroRng);
        assert!(blocked_north.placed_blocks.borrow().is_empty());
    }

    #[test]
    fn bonemeal_bounds_age_and_runs_the_mature_tick_with_the_same_rng() {
        init_vanilla_registry();
        let stem = pumpkin_stem();
        assert_eq!(StemBlock::age_after_bonemeal(0, 2), 2);
        assert_eq!(StemBlock::age_after_bonemeal(6, 5), MAX_AGE);

        let mature = vanilla_blocks::PUMPKIN_STEM
            .default_state()
            .set_value(AGE, MAX_AGE);
        assert!(!stem.is_valid_bonemeal_target(mature, &TestLevel::default(), BlockPos::ZERO));

        let state = vanilla_blocks::PUMPKIN_STEM
            .default_state()
            .set_value(AGE, 5);
        let fruit_pos = BlockPos::ZERO.north();
        let level = TestLevel::default()
            .with_block(
                BlockPos::ZERO.below(),
                vanilla_blocks::FARMLAND.default_state(),
            )
            .with_block(fruit_pos.below(), vanilla_blocks::DIRT.default_state())
            .with_raw_brightness(9);
        stem.perform_bonemeal_with_rng(state, &level, &mut ZeroRng, BlockPos::ZERO);

        let placed = level.placed_blocks.borrow();
        assert_eq!(placed.len(), 3);
        assert_eq!(placed[0].state.get_value(AGE), MAX_AGE);
        assert_eq!(placed[0].flags, UpdateFlags::UPDATE_CLIENTS);
        assert_eq!(placed[1].state.get_block(), &vanilla_blocks::PUMPKIN);
        assert_eq!(
            placed[2].state.get_block(),
            &vanilla_blocks::ATTACHED_PUMPKIN_STEM
        );
    }

    #[test]
    fn clone_items_match_each_stem_family() {
        init_vanilla_registry();
        let pumpkin = pumpkin_stem()
            .get_clone_item_stack(
                &vanilla_blocks::PUMPKIN_STEM,
                vanilla_blocks::PUMPKIN_STEM.default_state(),
                false,
            )
            .expect("pumpkin stem has a clone item");
        let melon = melon_stem()
            .get_clone_item_stack(
                &vanilla_blocks::MELON_STEM,
                vanilla_blocks::MELON_STEM.default_state(),
                false,
            )
            .expect("melon stem has a clone item");

        assert_eq!(pumpkin.item(), &*vanilla_items::PUMPKIN_SEEDS);
        assert_eq!(melon.item(), &*vanilla_items::MELON_SEEDS);
    }

    #[test]
    fn extracted_constructor_mappings_cover_both_families() {
        let classes: serde_json::Value =
            serde_json::from_str(include_str!("../../../../build/classes.json"))
                .expect("extracted classes.json is valid JSON");
        let blocks = classes["blocks"]
            .as_array()
            .expect("classes.json contains blocks");

        let expected = [
            (
                "pumpkin_stem",
                "pumpkin",
                "attached_pumpkin_stem",
                "pumpkin_seeds",
                "supports_pumpkin_stem",
                "supports_pumpkin_stem_fruit",
            ),
            (
                "melon_stem",
                "melon",
                "attached_melon_stem",
                "melon_seeds",
                "supports_melon_stem",
                "supports_melon_stem_fruit",
            ),
        ];

        for (name, fruit, attached, seed, stem_support, fruit_support) in expected {
            let block = blocks
                .iter()
                .find(|block| block["name"] == name)
                .expect("stem mapping exists");
            assert_eq!(block["fruit"], fruit);
            assert_eq!(block["attached_stem"], attached);
            assert_eq!(block["seed"], seed);
            assert_eq!(block["stem_support_blocks"], stem_support);
            assert_eq!(block["fruit_support_blocks"], fruit_support);
        }
    }
}
