#![expect(
    clippy::too_many_arguments,
    reason = "tree decorators mirror vanilla decorator context"
)]

use super::super::super::prelude::*;
use super::super::super::runner::FeatureDecorationRunner;
use super::super::super::vanilla_collections::JavaBlockPosSet;
use super::TreePlacement;

use crate::block_entity::entities::BeehiveBlockEntity;

const BEEHIVE_WORLDGEN_FACING: Direction = Direction::South;
const BEEHIVE_SPAWN_DIRECTIONS: [Direction; 3] =
    [Direction::East, Direction::South, Direction::West];

impl FeatureDecorationRunner {
    pub(super) fn place_tree_decorators(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        decorators: &[TreeDecorator],
        placement: &mut TreePlacement,
        biome_zoom_seed: i64,
    ) {
        for decorator in decorators {
            match decorator {
                TreeDecorator::AlterGround { provider } => {
                    Self::place_alter_ground_tree_decorator(
                        region, registry, random, provider, placement,
                    );
                }
                TreeDecorator::Beehive { probability } => {
                    Self::place_beehive_tree_decorator(
                        region,
                        registry,
                        random,
                        *probability,
                        placement,
                    );
                }
                TreeDecorator::Cocoa { probability } => {
                    Self::place_cocoa_tree_decorator(
                        region,
                        registry,
                        random,
                        *probability,
                        placement,
                    );
                }
                TreeDecorator::LeaveVine { probability } => {
                    Self::place_leave_vine_tree_decorator(region, random, *probability, placement);
                }
                TreeDecorator::TrunkVine => {
                    Self::place_trunk_vine_tree_decorator(region, random, placement);
                }
                TreeDecorator::PlaceOnGround(decorator) => {
                    Self::place_on_ground_tree_decorator(
                        region, registry, random, decorator, placement,
                    );
                }
                TreeDecorator::AttachedToLeaves(decorator) => {
                    Self::place_attached_to_leaves_tree_decorator(
                        region, registry, random, decorator, placement,
                    );
                }
                TreeDecorator::AttachedToLogs(decorator) => {
                    Self::place_attached_to_logs_tree_decorator(
                        region, registry, random, decorator, placement,
                    );
                }
                TreeDecorator::PaleMoss {
                    leaves_probability,
                    trunk_probability,
                    ground_probability,
                } => {
                    Self::place_pale_moss_tree_decorator(
                        region,
                        registry,
                        random,
                        *leaves_probability,
                        *trunk_probability,
                        *ground_probability,
                        placement,
                        biome_zoom_seed,
                    );
                }
                TreeDecorator::CreakingHeart { probability } => {
                    Self::place_creaking_heart_tree_decorator(
                        region,
                        registry,
                        random,
                        *probability,
                        placement,
                    );
                }
            }
        }
    }

    fn place_alter_ground_tree_decorator(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        provider: &BlockStateProvider,
        placement: &mut TreePlacement,
    ) {
        let positions = Self::lowest_tree_trunks_or_roots(placement);
        let Some(first_pos) = positions.first() else {
            return;
        };
        let min_y = first_pos.y();

        for pos in positions.into_iter().filter(|pos| pos.y() == min_y) {
            Self::place_alter_ground_circle(
                region,
                registry,
                random,
                provider,
                pos.offset(-1, 0, -1),
                placement,
            );
            Self::place_alter_ground_circle(
                region,
                registry,
                random,
                provider,
                pos.offset(2, 0, -1),
                placement,
            );
            Self::place_alter_ground_circle(
                region,
                registry,
                random,
                provider,
                pos.offset(-1, 0, 2),
                placement,
            );
            Self::place_alter_ground_circle(
                region,
                registry,
                random,
                provider,
                pos.offset(2, 0, 2),
                placement,
            );

            for _ in 0..5 {
                let placement_offset = random.next_i32_bounded(64);
                let x = placement_offset % 8;
                let z = placement_offset / 8;
                if x == 0 || x == 7 || z == 0 || z == 7 {
                    Self::place_alter_ground_circle(
                        region,
                        registry,
                        random,
                        provider,
                        pos.offset(-3 + x, 0, -3 + z),
                        placement,
                    );
                }
            }
        }
    }

    fn place_alter_ground_circle(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        provider: &BlockStateProvider,
        pos: BlockPos,
        placement: &mut TreePlacement,
    ) {
        for x in -2i32..=2 {
            for z in -2i32..=2 {
                if x.abs() != 2 || z.abs() != 2 {
                    Self::place_alter_ground_block_at(
                        region,
                        registry,
                        random,
                        provider,
                        pos.offset(x, 0, z),
                        placement,
                    );
                }
            }
        }
    }

    fn place_alter_ground_block_at(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        provider: &BlockStateProvider,
        pos: BlockPos,
        placement: &mut TreePlacement,
    ) {
        for y in (-3..=2).rev() {
            let cursor = pos.above_n(y);
            if let Some(state) = Self::sample_block_state_provider_optional(
                region, registry, random, provider, cursor,
            ) {
                placement.set_decoration(region, cursor, state);
                break;
            }

            if !region.block_state(cursor).is_air() && y < 0 {
                break;
            }
        }
    }

    fn place_on_ground_tree_decorator(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        decorator: &PlaceOnGroundDecorator,
        placement: &mut TreePlacement,
    ) {
        let positions = Self::lowest_tree_trunks_or_roots(placement);
        let Some(origin) = positions.first() else {
            return;
        };
        let min_y = origin.y();
        let mut min_x = origin.x();
        let mut max_x = origin.x();
        let mut min_z = origin.z();
        let mut max_z = origin.z();

        for position in positions {
            if position.y() == min_y {
                min_x = min_x.min(position.x());
                max_x = max_x.max(position.x());
                min_z = min_z.min(position.z());
                max_z = max_z.max(position.z());
            }
        }

        min_x -= decorator.radius;
        max_x += decorator.radius;
        min_z -= decorator.radius;
        max_z += decorator.radius;
        let min_y = min_y - decorator.height;
        let max_y = min_y + decorator.height * 2;

        for _ in 0..decorator.tries {
            let pos = BlockPos::new(
                random.next_i32_between(min_x, max_x),
                random.next_i32_between(min_y, max_y),
                random.next_i32_between(min_z, max_z),
            );
            Self::attempt_place_tree_ground_decorator(
                region,
                registry,
                random,
                &decorator.block_state_provider,
                pos,
                placement,
            );
        }
    }

    fn attempt_place_tree_ground_decorator(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        provider: &BlockStateProvider,
        pos: BlockPos,
        placement: &mut TreePlacement,
    ) {
        let above = pos.above();
        let above_state = region.block_state(above);
        if !above_state.is_air() && above_state.get_block() != &vanilla_blocks::VINE {
            return;
        }
        if !region.block_state(pos).is_solid_render() {
            return;
        }
        if region.height_at(HeightmapType::MotionBlockingNoLeaves, pos.x(), pos.z()) > above.y() {
            return;
        }

        let state = Self::sample_block_state_provider(region, registry, random, provider, above);
        placement.set_decoration(region, above, state);
    }

    fn place_trunk_vine_tree_decorator(
        region: &mut WorldGenRegion<'_>,
        random: &mut WorldgenRandom,
        placement: &mut TreePlacement,
    ) {
        for log in Self::sorted_tree_positions(&placement.trunks) {
            if random.next_i32_bounded(3) > 0 {
                Self::try_place_tree_vine(
                    region,
                    placement,
                    log.relative(Direction::West),
                    Direction::East,
                );
            }
            if random.next_i32_bounded(3) > 0 {
                Self::try_place_tree_vine(
                    region,
                    placement,
                    log.relative(Direction::East),
                    Direction::West,
                );
            }
            if random.next_i32_bounded(3) > 0 {
                Self::try_place_tree_vine(
                    region,
                    placement,
                    log.relative(Direction::North),
                    Direction::South,
                );
            }
            if random.next_i32_bounded(3) > 0 {
                Self::try_place_tree_vine(
                    region,
                    placement,
                    log.relative(Direction::South),
                    Direction::North,
                );
            }
        }
    }

    fn place_leave_vine_tree_decorator(
        region: &mut WorldGenRegion<'_>,
        random: &mut WorldgenRandom,
        probability: f32,
        placement: &mut TreePlacement,
    ) {
        for leaf in Self::sorted_tree_positions(&placement.foliage) {
            if random.next_f32() < probability {
                Self::try_place_hanging_tree_vine(
                    region,
                    placement,
                    leaf.relative(Direction::West),
                    Direction::East,
                );
            }
            if random.next_f32() < probability {
                Self::try_place_hanging_tree_vine(
                    region,
                    placement,
                    leaf.relative(Direction::East),
                    Direction::West,
                );
            }
            if random.next_f32() < probability {
                Self::try_place_hanging_tree_vine(
                    region,
                    placement,
                    leaf.relative(Direction::North),
                    Direction::South,
                );
            }
            if random.next_f32() < probability {
                Self::try_place_hanging_tree_vine(
                    region,
                    placement,
                    leaf.relative(Direction::South),
                    Direction::North,
                );
            }
        }
    }

    fn try_place_hanging_tree_vine(
        region: &mut WorldGenRegion<'_>,
        placement: &mut TreePlacement,
        pos: BlockPos,
        vine_face: Direction,
    ) {
        if !Self::try_place_tree_vine(region, placement, pos, vine_face) {
            return;
        }

        let mut pos = pos.below();
        let mut max_length = 4;
        while region.block_state(pos).is_air() && max_length > 0 {
            Self::place_tree_vine(region, placement, pos, vine_face);
            pos = pos.below();
            max_length -= 1;
        }
    }

    fn place_cocoa_tree_decorator(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        probability: f32,
        placement: &mut TreePlacement,
    ) {
        if random.next_f32() >= probability {
            return;
        }

        let logs = Self::sorted_tree_positions(&placement.trunks);
        let Some(first_log) = logs.first() else {
            return;
        };
        let tree_y = first_log.y();

        for log in logs.into_iter().filter(|pos| pos.y() - tree_y <= 2) {
            for direction in Self::VANILLA_HORIZONTAL_DIRECTIONS {
                if random.next_f32() > 0.25 {
                    continue;
                }

                let cocoa_pos = log.relative(direction.opposite());
                if !region.block_state(cocoa_pos).is_air() {
                    continue;
                }

                let age = random.next_i32_bounded(3) as u8;
                let cocoa_state = registry
                    .blocks
                    .get_default_state_id(&vanilla_blocks::COCOA)
                    .set_value(&BlockStateProperties::AGE_2, age)
                    .set_value(&BlockStateProperties::HORIZONTAL_FACING, direction);
                placement.set_decoration(region, cocoa_pos, cocoa_state);
            }
        }
    }

    fn try_place_tree_vine(
        region: &mut WorldGenRegion<'_>,
        placement: &mut TreePlacement,
        pos: BlockPos,
        vine_face: Direction,
    ) -> bool {
        if !region.block_state(pos).is_air() {
            return false;
        }

        Self::place_tree_vine(region, placement, pos, vine_face);
        true
    }

    fn place_tree_vine(
        region: &mut WorldGenRegion<'_>,
        placement: &mut TreePlacement,
        pos: BlockPos,
        vine_face: Direction,
    ) {
        placement.set_decoration(region, pos, Self::vine_state_for_face(vine_face));
    }

    fn lowest_tree_trunks_or_roots(placement: &TreePlacement) -> Vec<BlockPos> {
        let roots = Self::sorted_tree_positions(&placement.roots);
        let logs = Self::sorted_tree_positions(&placement.trunks);
        if roots.is_empty() {
            return logs;
        }

        if logs.first().is_some_and(|log| roots[0].y() == log.y()) {
            let mut positions = logs;
            positions.extend(roots);
            positions
        } else {
            roots
        }
    }

    fn place_beehive_tree_decorator(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        probability: f32,
        placement: &mut TreePlacement,
    ) {
        let logs = Self::sorted_tree_positions(&placement.trunks);
        if logs.is_empty() || random.next_f32() >= probability {
            return;
        }

        let leaves = Self::sorted_tree_positions(&placement.foliage);
        let hive_y = if let Some(first_leaf) = leaves.first() {
            (first_leaf.y() - 1).max(logs[0].y() + 1)
        } else {
            let log_y = logs[0].y() + 1 + random.next_i32_bounded(3);
            let last_log_y = logs[logs.len() - 1].y();
            log_y.min(last_log_y)
        };

        let mut hive_placements = Vec::new();
        for log in logs.iter().copied().filter(|pos| pos.y() == hive_y) {
            for direction in BEEHIVE_SPAWN_DIRECTIONS {
                hive_placements.push(log.relative(direction));
            }
        }

        if hive_placements.is_empty() {
            return;
        }

        Self::shuffle_tree_positions(random, &mut hive_placements);
        let hive_pos = hive_placements.into_iter().find(|pos| {
            region.block_state(*pos).is_air()
                && region
                    .block_state(pos.relative(BEEHIVE_WORLDGEN_FACING))
                    .is_air()
        });
        let Some(hive_pos) = hive_pos else {
            return;
        };

        let hive_state = registry
            .blocks
            .get_default_state_id(&vanilla_blocks::BEE_NEST)
            .set_value(
                &BlockStateProperties::HORIZONTAL_FACING,
                BEEHIVE_WORLDGEN_FACING,
            );
        placement.set_decoration(region, hive_pos, hive_state);

        let Some(block_entity) = region.block_entity(hive_pos) else {
            return;
        };
        let mut block_entity = block_entity.lock();
        let Some(beehive) = block_entity
            .as_any_mut()
            .downcast_mut::<BeehiveBlockEntity>()
        else {
            return;
        };

        let num_bees = 2 + random.next_i32_bounded(2);
        for _ in 0..num_bees {
            beehive.store_worldgen_bee(random.next_i32_bounded(599));
        }
    }

    fn place_attached_to_leaves_tree_decorator(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        decorator: &AttachedToLeavesDecorator,
        placement: &mut TreePlacement,
    ) {
        let mut blacklist = FxHashSet::default();
        let mut leaves = Self::sorted_tree_positions(&placement.foliage);
        Self::shuffle_tree_positions(random, &mut leaves);

        for leaf in leaves {
            let direction = Self::random_tree_decorator_direction(random, &decorator.directions);
            let place_pos = leaf.relative(direction);
            if blacklist.contains(&place_pos)
                || random.next_f32() >= decorator.probability
                || !Self::tree_decorator_has_required_empty_blocks(
                    region,
                    leaf,
                    direction,
                    decorator.required_empty_blocks,
                )
            {
                continue;
            }

            Self::blacklist_attached_tree_decoration_area(
                &mut blacklist,
                place_pos,
                decorator.exclusion_radius_xz,
                decorator.exclusion_radius_y,
            );
            let state = Self::sample_block_state_provider(
                region,
                registry,
                random,
                &decorator.block_provider,
                place_pos,
            );
            placement.set_decoration(region, place_pos, state);
        }
    }

    fn place_attached_to_logs_tree_decorator(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        decorator: &AttachedToLogsDecorator,
        placement: &mut TreePlacement,
    ) {
        let mut logs = Self::sorted_tree_positions(&placement.trunks);
        Self::shuffle_tree_positions(random, &mut logs);

        for log in logs {
            let direction = Self::random_tree_decorator_direction(random, &decorator.directions);
            let place_pos = log.relative(direction);
            if random.next_f32() > decorator.probability || !region.block_state(place_pos).is_air()
            {
                continue;
            }

            let state = Self::sample_block_state_provider(
                region,
                registry,
                random,
                &decorator.block_provider,
                place_pos,
            );
            placement.set_decoration(region, place_pos, state);
        }
    }

    fn place_pale_moss_tree_decorator(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        leaves_probability: f32,
        trunk_probability: f32,
        ground_probability: f32,
        placement: &mut TreePlacement,
        biome_zoom_seed: i64,
    ) {
        let mut shuffled_logs = Self::sorted_tree_positions(&placement.trunks);
        Self::shuffle_tree_positions(random, &mut shuffled_logs);
        let Some(origin) = shuffled_logs.into_iter().min_by_key(BlockPos::y) else {
            return;
        };

        if random.next_f32() < ground_probability {
            let pale_moss_patch_key = Identifier::vanilla_static("pale_moss_patch");
            let Some(pale_moss_patch) = registry.configured_features.by_key(&pale_moss_patch_key)
            else {
                panic!(
                    "pale moss tree decorator references unknown configured feature {pale_moss_patch_key}"
                );
            };
            Self::place_configured_feature_kind(
                region,
                registry,
                random,
                &pale_moss_patch.kind,
                origin.above(),
                biome_zoom_seed,
            );
        }

        for log in Self::sorted_tree_positions(&placement.trunks) {
            if random.next_f32() < trunk_probability {
                let down = log.below();
                if region.block_state(down).is_air() {
                    Self::add_pale_moss_hanger(region, random, down, placement);
                }
            }
        }

        for leaf in Self::sorted_tree_positions(&placement.foliage) {
            if random.next_f32() < leaves_probability {
                let down = leaf.below();
                if region.block_state(down).is_air() {
                    Self::add_pale_moss_hanger(region, random, down, placement);
                }
            }
        }
    }

    fn place_creaking_heart_tree_decorator(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        probability: f32,
        placement: &mut TreePlacement,
    ) {
        if placement.trunks.is_empty() || random.next_f32() >= probability {
            return;
        }

        let mut heart_placements = Self::sorted_tree_positions(&placement.trunks);
        Self::shuffle_tree_positions(random, &mut heart_placements);
        let Some(target_pos) = heart_placements.into_iter().find(|pos| {
            Self::VANILLA_DIRECTION_VALUES.iter().all(|direction| {
                registry.blocks.is_in_tag(
                    region.block_state(pos.relative(*direction)).get_block(),
                    &vanilla_block_tags::LOGS_TAG,
                )
            })
        }) else {
            return;
        };

        let state = registry
            .blocks
            .get_default_state_id(&vanilla_blocks::CREAKING_HEART)
            .set_value(
                &BlockStateProperties::CREAKING_HEART_STATE,
                CreakingHeartState::Dormant,
            )
            .set_value(&BlockStateProperties::NATURAL, true);
        placement.set_decoration(region, target_pos, state);
    }

    fn add_pale_moss_hanger(
        region: &mut WorldGenRegion<'_>,
        random: &mut WorldgenRandom,
        mut pos: BlockPos,
        placement: &mut TreePlacement,
    ) {
        while region.block_state(pos.below()).is_air() {
            if random.next_f32() < 0.5 {
                break;
            }

            let state = vanilla_blocks::PALE_HANGING_MOSS
                .default_state()
                .set_value(&BlockStateProperties::TIP, false);
            placement.set_decoration(region, pos, state);
            pos = pos.below();
        }

        let state = vanilla_blocks::PALE_HANGING_MOSS
            .default_state()
            .set_value(&BlockStateProperties::TIP, true);
        placement.set_decoration(region, pos, state);
    }

    fn tree_decorator_has_required_empty_blocks(
        region: &WorldGenRegion<'_>,
        leaf: BlockPos,
        direction: Direction,
        required_empty_blocks: i32,
    ) -> bool {
        (1..=required_empty_blocks).all(|offset| {
            region
                .block_state(leaf.relative_n(direction, offset))
                .is_air()
        })
    }

    fn blacklist_attached_tree_decoration_area(
        blacklist: &mut FxHashSet<BlockPos>,
        center: BlockPos,
        radius_xz: i32,
        radius_y: i32,
    ) {
        for x in -radius_xz..=radius_xz {
            for y in -radius_y..=radius_y {
                for z in -radius_xz..=radius_xz {
                    blacklist.insert(center.offset(x, y, z));
                }
            }
        }
    }

    fn random_tree_decorator_direction(
        random: &mut WorldgenRandom,
        directions: &[Direction],
    ) -> Direction {
        assert!(
            !directions.is_empty(),
            "attached tree decorator direction list must not be empty"
        );
        let Ok(direction_count) = i32::try_from(directions.len()) else {
            panic!("attached tree decorator direction count exceeds i32 range");
        };
        directions[random.next_i32_bounded(direction_count) as usize]
    }

    fn sorted_tree_positions(positions: &JavaBlockPosSet) -> Vec<BlockPos> {
        let mut positions = positions.java_ordered_positions();
        positions.sort_by_key(BlockPos::y);
        positions
    }

    fn shuffle_tree_positions(random: &mut WorldgenRandom, positions: &mut [BlockPos]) {
        for i in (1..positions.len()).rev() {
            let Ok(bound) = i32::try_from(i + 1) else {
                panic!("tree decorator shuffle length exceeds i32 range");
            };
            let j = random.next_i32_bounded(bound) as usize;
            positions.swap(i, j);
        }
    }
}
