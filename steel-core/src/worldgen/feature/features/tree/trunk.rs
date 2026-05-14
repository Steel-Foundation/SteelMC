use super::super::super::prelude::*;
use super::super::super::runner::FeatureDecorationRunner;
use super::{FoliageAttachment, TreePlacement};

impl FeatureDecorationRunner {
    pub(super) fn tree_height(random: &mut Xoroshiro, placer: &TrunkPlacer) -> i32 {
        match placer {
            TrunkPlacer::Straight(base)
            | TrunkPlacer::Giant(base)
            | TrunkPlacer::Fancy(base)
            | TrunkPlacer::Forking(base)
            | TrunkPlacer::DarkOak(base)
            | TrunkPlacer::MegaJungle(base) => Self::sample_tree_height(
                random,
                base.base_height,
                base.height_rand_a,
                base.height_rand_b,
            ),
            TrunkPlacer::Bending(placer) => Self::sample_tree_height(
                random,
                placer.base_height,
                placer.height_rand_a,
                placer.height_rand_b,
            ),
            TrunkPlacer::UpwardsBranching(placer) => Self::sample_tree_height(
                random,
                placer.base_height,
                placer.height_rand_a,
                placer.height_rand_b,
            ),
            TrunkPlacer::Cherry(placer) => Self::sample_tree_height(
                random,
                placer.base_height,
                placer.height_rand_a,
                placer.height_rand_b,
            ),
        }
    }

    fn sample_tree_height(
        random: &mut Xoroshiro,
        base_height: i32,
        height_rand_a: i32,
        height_rand_b: i32,
    ) -> i32 {
        base_height
            + random.next_i32_bounded(height_rand_a + 1)
            + random.next_i32_bounded(height_rand_b + 1)
    }

    pub(super) fn place_tree_trunk(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut Xoroshiro,
        tree_height: i32,
        origin: BlockPos,
        config: &TreeConfiguration,
        placement: &mut TreePlacement,
    ) -> Vec<FoliageAttachment> {
        match &config.trunk_placer {
            TrunkPlacer::Straight(_) => Self::place_straight_tree_trunk(
                region,
                registry,
                random,
                tree_height,
                origin,
                config,
                placement,
            ),
            TrunkPlacer::Forking(_) => Self::place_forking_tree_trunk(
                region,
                registry,
                random,
                tree_height,
                origin,
                config,
                placement,
            ),
            _ => {
                panic!(
                    "tree trunk placer requires runtime support before minecraft:tree can be registered"
                )
            }
        }
    }

    fn place_straight_tree_trunk(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut Xoroshiro,
        tree_height: i32,
        origin: BlockPos,
        config: &TreeConfiguration,
        placement: &mut TreePlacement,
    ) -> Vec<FoliageAttachment> {
        Self::place_below_trunk_block(region, registry, random, origin.below(), config, placement);

        for y in 0..tree_height {
            let pos = origin.above_n(y);
            let _ = Self::place_tree_log(region, registry, random, pos, config, placement);
        }

        vec![FoliageAttachment {
            pos: origin.above_n(tree_height),
            radius_offset: 0,
            double_trunk: false,
        }]
    }

    fn place_forking_tree_trunk(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut Xoroshiro,
        tree_height: i32,
        origin: BlockPos,
        config: &TreeConfiguration,
        placement: &mut TreePlacement,
    ) -> Vec<FoliageAttachment> {
        Self::place_below_trunk_block(region, registry, random, origin.below(), config, placement);

        let mut attachments = Vec::new();
        let lean_direction = Self::random_horizontal_direction(random);
        let lean_height = tree_height - random.next_i32_bounded(4) - 1;
        let mut lean_steps = 3 - random.next_i32_bounded(3);
        let mut trunk_x = origin.x();
        let mut trunk_z = origin.z();
        let mut foliage_y = None;

        for y_offset in 0..tree_height {
            let y = origin.y() + y_offset;
            if y_offset >= lean_height && lean_steps > 0 {
                let (dx, _, dz) = lean_direction.offset();
                trunk_x += dx;
                trunk_z += dz;
                lean_steps -= 1;
            }

            let pos = BlockPos::new(trunk_x, y, trunk_z);
            if Self::place_tree_log(region, registry, random, pos, config, placement) {
                foliage_y = Some(y + 1);
            }
        }

        if let Some(y) = foliage_y {
            attachments.push(FoliageAttachment {
                pos: BlockPos::new(trunk_x, y, trunk_z),
                radius_offset: 1,
                double_trunk: false,
            });
        }

        trunk_x = origin.x();
        trunk_z = origin.z();
        let branch_direction = Self::random_horizontal_direction(random);
        if branch_direction != lean_direction {
            let mut branch_y_offset = lean_height - random.next_i32_bounded(2) - 1;
            let mut branch_steps = 1 + random.next_i32_bounded(3);
            foliage_y = None;

            while branch_y_offset < tree_height && branch_steps > 0 {
                if branch_y_offset >= 1 {
                    let y = origin.y() + branch_y_offset;
                    let (dx, _, dz) = branch_direction.offset();
                    trunk_x += dx;
                    trunk_z += dz;
                    let pos = BlockPos::new(trunk_x, y, trunk_z);
                    if Self::place_tree_log(region, registry, random, pos, config, placement) {
                        foliage_y = Some(y + 1);
                    }
                }

                branch_y_offset += 1;
                branch_steps -= 1;
            }

            if let Some(y) = foliage_y {
                attachments.push(FoliageAttachment {
                    pos: BlockPos::new(trunk_x, y, trunk_z),
                    radius_offset: 0,
                    double_trunk: false,
                });
            }
        }

        attachments
    }

    fn place_below_trunk_block(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut Xoroshiro,
        pos: BlockPos,
        config: &TreeConfiguration,
        placement: &mut TreePlacement,
    ) {
        let Some(state) = Self::sample_block_state_provider_optional(
            region,
            registry,
            random,
            &config.below_trunk_provider,
            pos,
        ) else {
            return;
        };
        placement.set_trunk(region, pos, state);
    }

    fn place_tree_log(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut Xoroshiro,
        pos: BlockPos,
        config: &TreeConfiguration,
        placement: &mut TreePlacement,
    ) -> bool {
        if !Self::tree_valid_pos(region, registry, pos) {
            return false;
        }

        let state = Self::sample_block_state_provider(
            region,
            registry,
            random,
            &config.trunk_provider,
            pos,
        );
        placement.set_trunk(region, pos, state);
        true
    }
}
