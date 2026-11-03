use super::super::prelude::*;
use super::super::runner::FeatureDecorationRunner;

impl FeatureDecorationRunner {
    /// `feature` is the nested placed feature that places one decorated coral
    /// block and fails where coral cannot go; every segment grows through it.
    pub(in crate::worldgen::feature) fn place_coral_claw_feature(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        feature: &PlacedFeatureRef,
        origin: BlockPos,
        biome_zoom_seed: i64,
    ) -> bool {
        if !Self::place_placed_feature_ref(
            region,
            registry,
            random,
            origin,
            feature,
            biome_zoom_seed,
        ) {
            return false;
        }

        let claw_direction = Self::random_horizontal_direction(random);
        let n_branches = random.next_i32_bounded(2) + 2;
        let possible_directions = Self::shuffled_directions(
            random,
            [
                claw_direction,
                claw_direction.rotate_y_clockwise(),
                claw_direction.rotate_y_counter_clockwise(),
            ],
        );

        for branch_direction in possible_directions.into_iter().take(n_branches as usize) {
            let mut pos = origin.relative(branch_direction);
            let sideway_length = random.next_i32_bounded(2) + 1;
            let segment_direction;
            let inway_length;
            if branch_direction == claw_direction {
                segment_direction = claw_direction;
                inway_length = random.next_i32_bounded(3) + 2;
            } else {
                pos = pos.above();
                segment_direction = if random.next_i32_bounded(2) == 0 {
                    branch_direction
                } else {
                    Direction::Up
                };
                inway_length = random.next_i32_bounded(3) + 3;
            }

            for _ in 0..sideway_length {
                if !Self::place_placed_feature_ref(
                    region,
                    registry,
                    random,
                    pos,
                    feature,
                    biome_zoom_seed,
                ) {
                    break;
                }
                pos = pos.relative(segment_direction);
            }

            pos = pos.relative(segment_direction.opposite()).above();

            for _ in 0..inway_length {
                pos = pos.relative(claw_direction);
                if !Self::place_placed_feature_ref(
                    region,
                    registry,
                    random,
                    pos,
                    feature,
                    biome_zoom_seed,
                ) {
                    break;
                }

                if random.next_f32() < 0.25 {
                    pos = pos.above();
                }
            }
        }

        true
    }

    /// `feature` is the nested placed feature that places one decorated coral
    /// block and fails where coral cannot go; trunk and branches grow through it.
    pub(in crate::worldgen::feature) fn place_coral_tree_feature(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        feature: &PlacedFeatureRef,
        origin: BlockPos,
        biome_zoom_seed: i64,
    ) -> bool {
        let mut pos = origin;
        let trunk_height = random.next_i32_bounded(3) + 1;
        for _ in 0..trunk_height {
            if !Self::place_placed_feature_ref(
                region,
                registry,
                random,
                pos,
                feature,
                biome_zoom_seed,
            ) {
                return true;
            }
            pos = pos.above();
        }

        let trunk_top = pos;
        let n_branches = random.next_i32_bounded(3) + 2;
        let directions = Self::shuffled_directions(random, Self::VANILLA_HORIZONTAL_DIRECTIONS);

        for branch_direction in directions.into_iter().take(n_branches as usize) {
            pos = trunk_top.relative(branch_direction);
            let branch_height = random.next_i32_bounded(5) + 2;
            let mut segment_length = 0;

            for j in 0..branch_height {
                if !Self::place_placed_feature_ref(
                    region,
                    registry,
                    random,
                    pos,
                    feature,
                    biome_zoom_seed,
                ) {
                    break;
                }
                segment_length += 1;
                pos = pos.above();
                if j == 0 || (segment_length >= 2 && random.next_f32() < 0.25) {
                    pos = pos.relative(branch_direction);
                    segment_length = 0;
                }
            }
        }

        true
    }
}
