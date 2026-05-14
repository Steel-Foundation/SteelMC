use super::super::prelude::*;
use super::super::runner::FeatureDecorationRunner;

use crate::block_entity::entities::BeehiveBlockEntity;

// TODO: Register `minecraft:tree` after the required trunk/foliage/root placers
// and tree decorators are implemented.

const LEAF_DISTANCE_LIMIT: usize = 7;
const BEEHIVE_WORLDGEN_FACING: Direction = Direction::South;
const BEEHIVE_SPAWN_DIRECTIONS: [Direction; 3] =
    [Direction::East, Direction::South, Direction::West];

impl FeatureDecorationRunner {
    pub(crate) fn place_tree_feature(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut Xoroshiro,
        config: &TreeConfiguration,
        origin: BlockPos,
    ) -> bool {
        let mut placement = TreePlacement::default();
        let placed = Self::do_place_tree(region, registry, random, config, origin, &mut placement);
        if !placed || (placement.trunks.is_empty() && placement.foliage.is_empty()) {
            return false;
        }

        if !config.decorators.is_empty() {
            Self::place_tree_decorators(region, registry, random, config, &mut placement);
        }

        let Some(bounds) = TreeBounds::from_placement(&placement) else {
            return false;
        };
        Self::update_tree_leaves(region, bounds, &placement);
        true
    }

    fn do_place_tree(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut Xoroshiro,
        config: &TreeConfiguration,
        origin: BlockPos,
        placement: &mut TreePlacement,
    ) -> bool {
        if config.root_placer.is_some() {
            panic!(
                "tree root placers require runtime support before minecraft:tree can be registered"
            );
        }

        let tree_height = Self::tree_height(random, &config.trunk_placer);
        let foliage_height = Self::tree_foliage_height(random, &config.foliage_placer);
        let trunk_height = tree_height - foliage_height;
        let leaf_radius = Self::tree_foliage_radius(random, &config.foliage_placer, trunk_height);
        let trunk_origin = origin;
        let min_y = origin.y().min(trunk_origin.y());
        let max_y = origin.y().max(trunk_origin.y()) + tree_height + 1;

        if min_y < region.min_y() + 1 || max_y > region.max_y_exclusive() + 1 {
            return false;
        }

        let clipped_tree_height =
            Self::max_free_tree_height(region, registry, tree_height, trunk_origin, config);
        let min_clipped_height = Self::tree_min_clipped_height(&config.minimum_size);
        if clipped_tree_height < tree_height
            && min_clipped_height.is_none_or(|height| clipped_tree_height < height)
        {
            return false;
        }

        let foliage_attachments = Self::place_tree_trunk(
            region,
            registry,
            random,
            clipped_tree_height,
            trunk_origin,
            config,
            placement,
        );
        for foliage_attachment in foliage_attachments {
            Self::create_tree_foliage(
                region,
                registry,
                random,
                config,
                clipped_tree_height,
                foliage_attachment,
                foliage_height,
                leaf_radius,
                placement,
            );
        }

        true
    }

    fn place_tree_decorators(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut Xoroshiro,
        config: &TreeConfiguration,
        placement: &mut TreePlacement,
    ) {
        for decorator in &config.decorators {
            match decorator {
                TreeDecorator::Beehive { probability } => {
                    Self::place_beehive_tree_decorator(
                        region,
                        registry,
                        random,
                        *probability,
                        placement,
                    );
                }
                TreeDecorator::AlterGround { .. }
                | TreeDecorator::Cocoa { .. }
                | TreeDecorator::CreakingHeart { .. }
                | TreeDecorator::LeaveVine { .. }
                | TreeDecorator::TrunkVine
                | TreeDecorator::AttachedToLeaves(_)
                | TreeDecorator::AttachedToLogs(_)
                | TreeDecorator::PlaceOnGround(_)
                | TreeDecorator::PaleMoss { .. } => {
                    panic!(
                        "tree decorator requires runtime support before minecraft:tree can be registered"
                    );
                }
            }
        }
    }

    fn place_beehive_tree_decorator(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut Xoroshiro,
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

    fn sorted_tree_positions(positions: &FxHashSet<BlockPos>) -> Vec<BlockPos> {
        let mut positions = positions.iter().copied().collect::<Vec<_>>();
        positions.sort_by_key(BlockPos::y);
        positions
    }

    fn shuffle_tree_positions(random: &mut Xoroshiro, positions: &mut [BlockPos]) {
        for i in (1..positions.len()).rev() {
            let Ok(bound) = i32::try_from(i + 1) else {
                panic!("tree decorator shuffle length exceeds i32 range");
            };
            let j = random.next_i32_bounded(bound) as usize;
            positions.swap(i, j);
        }
    }

    fn tree_height(random: &mut Xoroshiro, placer: &TrunkPlacer) -> i32 {
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

    fn tree_foliage_height(random: &mut Xoroshiro, foliage_placer: &FoliagePlacer) -> i32 {
        match foliage_placer {
            FoliagePlacer::Blob(placer) => placer.height.sample(random),
            FoliagePlacer::Acacia(_) => 0,
            _ => panic!(
                "tree foliage placer requires runtime support before minecraft:tree can be registered"
            ),
        }
    }

    fn tree_foliage_radius(
        random: &mut Xoroshiro,
        foliage_placer: &FoliagePlacer,
        _trunk_height: i32,
    ) -> i32 {
        match foliage_placer {
            FoliagePlacer::Blob(placer) => placer.radius.sample(random),
            FoliagePlacer::Acacia(placer) => placer.radius.sample(random),
            _ => panic!(
                "tree foliage placer requires runtime support before minecraft:tree can be registered"
            ),
        }
    }

    const fn tree_min_clipped_height(feature_size: &FeatureSize) -> Option<i32> {
        match feature_size {
            FeatureSize::TwoLayers(size) => size.min_clipped_height,
            FeatureSize::ThreeLayers(size) => size.min_clipped_height,
        }
    }

    const fn tree_size_at_height(feature_size: &FeatureSize, tree_height: i32, y: i32) -> i32 {
        match feature_size {
            FeatureSize::TwoLayers(size) => {
                if y < size.limit {
                    size.lower_size
                } else {
                    size.upper_size
                }
            }
            FeatureSize::ThreeLayers(size) => {
                if y < size.limit {
                    size.lower_size
                } else if y >= tree_height - size.upper_limit {
                    size.upper_size
                } else {
                    size.middle_size
                }
            }
        }
    }

    fn max_free_tree_height(
        region: &WorldGenRegion<'_>,
        registry: &Registry,
        max_tree_height: i32,
        tree_pos: BlockPos,
        config: &TreeConfiguration,
    ) -> i32 {
        for y in 0..=max_tree_height + 1 {
            let radius = Self::tree_size_at_height(&config.minimum_size, max_tree_height, y);
            for x in -radius..=radius {
                for z in -radius..=radius {
                    let pos = tree_pos.offset(x, y, z);
                    if !Self::tree_is_free(region, registry, pos)
                        || (!config.ignore_vines && Self::tree_is_vine(region, pos))
                    {
                        return y - 2;
                    }
                }
            }
        }

        max_tree_height
    }

    fn place_tree_trunk(
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

    fn create_tree_foliage(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut Xoroshiro,
        config: &TreeConfiguration,
        _tree_height: i32,
        attachment: FoliageAttachment,
        foliage_height: i32,
        leaf_radius: i32,
        placement: &mut TreePlacement,
    ) {
        match &config.foliage_placer {
            FoliagePlacer::Blob(placer) => Self::create_blob_tree_foliage(
                region,
                registry,
                random,
                config,
                placer,
                attachment,
                foliage_height,
                leaf_radius,
                placement,
            ),
            FoliagePlacer::Acacia(_) => Self::create_acacia_tree_foliage(
                region,
                registry,
                random,
                config,
                attachment,
                foliage_height,
                leaf_radius,
                placement,
            ),
            _ => panic!(
                "tree foliage placer requires runtime support before minecraft:tree can be registered"
            ),
        }
    }

    fn create_blob_tree_foliage(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut Xoroshiro,
        config: &TreeConfiguration,
        _placer: &BlobFoliagePlacer,
        attachment: FoliageAttachment,
        foliage_height: i32,
        leaf_radius: i32,
        placement: &mut TreePlacement,
    ) {
        let offset = Self::tree_foliage_offset(random, &config.foliage_placer);
        for y in (offset - foliage_height..=offset).rev() {
            let current_radius = (leaf_radius + attachment.radius_offset - 1 - y / 2).max(0);
            Self::place_tree_leaves_row(
                region,
                registry,
                random,
                config,
                attachment.pos,
                current_radius,
                y,
                attachment.double_trunk,
                placement,
            );
        }
    }

    fn create_acacia_tree_foliage(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut Xoroshiro,
        config: &TreeConfiguration,
        attachment: FoliageAttachment,
        foliage_height: i32,
        leaf_radius: i32,
        placement: &mut TreePlacement,
    ) {
        let offset = Self::tree_foliage_offset(random, &config.foliage_placer);
        let foliage_pos = attachment.pos.above_n(offset);
        Self::place_tree_leaves_row(
            region,
            registry,
            random,
            config,
            foliage_pos,
            leaf_radius + attachment.radius_offset,
            -1 - foliage_height,
            attachment.double_trunk,
            placement,
        );
        Self::place_tree_leaves_row(
            region,
            registry,
            random,
            config,
            foliage_pos,
            leaf_radius - 1,
            -foliage_height,
            attachment.double_trunk,
            placement,
        );
        Self::place_tree_leaves_row(
            region,
            registry,
            random,
            config,
            foliage_pos,
            leaf_radius + attachment.radius_offset - 1,
            0,
            attachment.double_trunk,
            placement,
        );
    }

    fn tree_foliage_offset(random: &mut Xoroshiro, foliage_placer: &FoliagePlacer) -> i32 {
        match foliage_placer {
            FoliagePlacer::Blob(placer) => placer.offset.sample(random),
            FoliagePlacer::Acacia(placer) => placer.offset.sample(random),
            _ => panic!(
                "tree foliage placer requires runtime support before minecraft:tree can be registered"
            ),
        }
    }

    fn place_tree_leaves_row(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut Xoroshiro,
        config: &TreeConfiguration,
        origin: BlockPos,
        current_radius: i32,
        y: i32,
        double_trunk: bool,
        placement: &mut TreePlacement,
    ) {
        let offset = if double_trunk { 1 } else { 0 };
        for dx in -current_radius..=current_radius + offset {
            for dz in -current_radius..=current_radius + offset {
                if !Self::tree_foliage_should_skip_location(
                    random,
                    &config.foliage_placer,
                    dx,
                    y,
                    dz,
                    current_radius,
                    double_trunk,
                ) {
                    let pos = origin.offset(dx, y, dz);
                    let _ =
                        Self::try_place_tree_leaf(region, registry, random, config, pos, placement);
                }
            }
        }
    }

    fn tree_foliage_should_skip_location(
        random: &mut Xoroshiro,
        foliage_placer: &FoliagePlacer,
        dx: i32,
        y: i32,
        dz: i32,
        current_radius: i32,
        double_trunk: bool,
    ) -> bool {
        let (dx, dz) = Self::foliage_signed_distances(dx, dz, double_trunk);
        match foliage_placer {
            FoliagePlacer::Blob(_) => {
                Self::blob_foliage_should_skip_location(random, dx, y, dz, current_radius)
            }
            FoliagePlacer::Acacia(_) => {
                Self::acacia_foliage_should_skip_location(dx, y, dz, current_radius)
            }
            _ => {
                panic!(
                    "tree foliage placer requires runtime support before minecraft:tree can be registered"
                )
            }
        }
    }

    fn blob_foliage_should_skip_location(
        random: &mut Xoroshiro,
        dx: i32,
        y: i32,
        dz: i32,
        current_radius: i32,
    ) -> bool {
        dx == current_radius && dz == current_radius && (random.next_i32_bounded(2) == 0 || y == 0)
    }

    const fn acacia_foliage_should_skip_location(
        dx: i32,
        y: i32,
        dz: i32,
        current_radius: i32,
    ) -> bool {
        if y == 0 {
            (dx > 1 || dz > 1) && dx != 0 && dz != 0
        } else {
            dx == current_radius && dz == current_radius && current_radius > 0
        }
    }

    fn foliage_signed_distances(dx: i32, dz: i32, double_trunk: bool) -> (i32, i32) {
        if double_trunk {
            (
                abs_i32(dx).min(abs_i32(dx - 1)),
                abs_i32(dz).min(abs_i32(dz - 1)),
            )
        } else {
            (abs_i32(dx), abs_i32(dz))
        }
    }

    fn try_place_tree_leaf(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut Xoroshiro,
        config: &TreeConfiguration,
        pos: BlockPos,
        placement: &mut TreePlacement,
    ) -> bool {
        let current_state = region.block_state(pos);
        let is_persistent = current_state
            .try_get_value(&BlockStateProperties::PERSISTENT)
            .unwrap_or(false);
        if is_persistent || !Self::tree_valid_pos(region, registry, pos) {
            return false;
        }

        let foliage_state = Self::sample_block_state_provider(
            region,
            registry,
            random,
            &config.foliage_provider,
            pos,
        );
        let foliage_state = Self::copy_waterlogged_from(region, pos, foliage_state);
        placement.set_foliage(region, pos, foliage_state);
        true
    }

    fn tree_valid_pos(region: &WorldGenRegion<'_>, registry: &Registry, pos: BlockPos) -> bool {
        let state = region.block_state(pos);
        state.is_air()
            || registry.blocks.is_in_tag(
                state.get_block(),
                &vanilla_block_tags::REPLACEABLE_BY_TREES_TAG,
            )
    }

    fn tree_is_free(region: &WorldGenRegion<'_>, registry: &Registry, pos: BlockPos) -> bool {
        let state = region.block_state(pos);
        state.is_air()
            || registry.blocks.is_in_tag(
                state.get_block(),
                &vanilla_block_tags::REPLACEABLE_BY_TREES_TAG,
            )
            || registry
                .blocks
                .is_in_tag(state.get_block(), &vanilla_block_tags::LOGS_TAG)
    }

    fn tree_is_vine(region: &WorldGenRegion<'_>, pos: BlockPos) -> bool {
        region.block_state(pos).get_block() == &vanilla_blocks::VINE
    }

    fn update_tree_leaves(
        region: &mut WorldGenRegion<'_>,
        bounds: TreeBounds,
        placement: &TreePlacement,
    ) {
        let mut shape = FxHashSet::default();
        for &pos in placement.decorations.iter().chain(placement.roots.iter()) {
            if bounds.contains(pos) {
                shape.insert(pos);
            }
        }

        let mut frontiers = (0..LEAF_DISTANCE_LIMIT)
            .map(|_| FxHashSet::default())
            .collect::<Vec<_>>();
        frontiers[0].extend(placement.trunks.iter().copied());
        let mut smallest_distance = 0;

        loop {
            while smallest_distance < LEAF_DISTANCE_LIMIT && frontiers[smallest_distance].is_empty()
            {
                smallest_distance += 1;
            }
            if smallest_distance >= LEAF_DISTANCE_LIMIT {
                break;
            }

            let Some(pos) = take_frontier_position(&mut frontiers[smallest_distance]) else {
                continue;
            };
            if !bounds.contains(pos) {
                continue;
            }

            if smallest_distance != 0 {
                let state = region.block_state(pos);
                if state
                    .try_get_value(&BlockStateProperties::DISTANCE)
                    .is_some()
                {
                    let distance = smallest_distance as u8;
                    Self::set_tree_block(
                        region,
                        pos,
                        state.set_value(&BlockStateProperties::DISTANCE, distance),
                    );
                }
            }

            shape.insert(pos);

            for direction in Self::VANILLA_DIRECTION_VALUES {
                let neighbor_pos = pos.relative(direction);
                if !bounds.contains(neighbor_pos) || shape.contains(&neighbor_pos) {
                    continue;
                }

                let state = region.block_state(neighbor_pos);
                let Some(distance) = state.try_get_value(&BlockStateProperties::DISTANCE) else {
                    continue;
                };
                let new_distance = distance.min((smallest_distance + 1) as u8);
                if new_distance < LEAF_DISTANCE_LIMIT as u8 {
                    frontiers[usize::from(new_distance)].insert(neighbor_pos);
                    smallest_distance = smallest_distance.min(usize::from(new_distance));
                }
            }
        }

        // TODO: Mirror `StructureTemplate.updateShapeAtEdge` once block behaviors expose
        // vanilla shape update hooks for generation-time leaf edges.
    }

    fn set_tree_block(region: &mut WorldGenRegion<'_>, pos: BlockPos, state: BlockStateId) {
        let flags = UpdateFlags::UPDATE_NEIGHBORS
            | UpdateFlags::UPDATE_CLIENTS
            | UpdateFlags::UPDATE_KNOWN_SHAPE;
        let _ = region.set_block_state(pos, state, flags);
    }
}

#[derive(Clone, Copy)]
struct FoliageAttachment {
    pos: BlockPos,
    radius_offset: i32,
    double_trunk: bool,
}

#[derive(Default)]
struct TreePlacement {
    roots: FxHashSet<BlockPos>,
    trunks: FxHashSet<BlockPos>,
    foliage: FxHashSet<BlockPos>,
    decorations: FxHashSet<BlockPos>,
}

impl TreePlacement {
    fn set_trunk(&mut self, region: &mut WorldGenRegion<'_>, pos: BlockPos, state: BlockStateId) {
        self.trunks.insert(pos);
        FeatureDecorationRunner::set_tree_block(region, pos, state);
    }

    fn set_foliage(&mut self, region: &mut WorldGenRegion<'_>, pos: BlockPos, state: BlockStateId) {
        self.foliage.insert(pos);
        FeatureDecorationRunner::set_tree_block(region, pos, state);
    }

    fn set_decoration(
        &mut self,
        region: &mut WorldGenRegion<'_>,
        pos: BlockPos,
        state: BlockStateId,
    ) {
        self.decorations.insert(pos);
        FeatureDecorationRunner::set_tree_block(region, pos, state);
    }
}

#[derive(Clone, Copy)]
struct TreeBounds {
    min_x: i32,
    min_y: i32,
    min_z: i32,
    max_x: i32,
    max_y: i32,
    max_z: i32,
}

impl TreeBounds {
    fn from_placement(placement: &TreePlacement) -> Option<Self> {
        let mut bounds: Option<Self> = None;
        for &pos in placement
            .roots
            .iter()
            .chain(placement.trunks.iter())
            .chain(placement.foliage.iter())
            .chain(placement.decorations.iter())
        {
            match &mut bounds {
                Some(bounds) => bounds.include(pos),
                None => bounds = Some(Self::new(pos)),
            }
        }
        bounds
    }

    const fn new(pos: BlockPos) -> Self {
        Self {
            min_x: pos.x(),
            min_y: pos.y(),
            min_z: pos.z(),
            max_x: pos.x(),
            max_y: pos.y(),
            max_z: pos.z(),
        }
    }

    fn include(&mut self, pos: BlockPos) {
        self.min_x = self.min_x.min(pos.x());
        self.min_y = self.min_y.min(pos.y());
        self.min_z = self.min_z.min(pos.z());
        self.max_x = self.max_x.max(pos.x());
        self.max_y = self.max_y.max(pos.y());
        self.max_z = self.max_z.max(pos.z());
    }

    const fn contains(self, pos: BlockPos) -> bool {
        pos.x() >= self.min_x
            && pos.x() <= self.max_x
            && pos.y() >= self.min_y
            && pos.y() <= self.max_y
            && pos.z() >= self.min_z
            && pos.z() <= self.max_z
    }
}

fn take_frontier_position(frontier: &mut FxHashSet<BlockPos>) -> Option<BlockPos> {
    let pos = frontier.iter().next().copied()?;
    frontier.remove(&pos);
    Some(pos)
}

const fn abs_i32(value: i32) -> i32 {
    if value < 0 { -value } else { value }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acacia_top_layer_keeps_cross_and_inner_corners() {
        assert!(FeatureDecorationRunner::acacia_foliage_should_skip_location(2, 0, 2, 2));
        assert!(FeatureDecorationRunner::acacia_foliage_should_skip_location(1, 0, 2, 2));
        assert!(!FeatureDecorationRunner::acacia_foliage_should_skip_location(0, 0, 2, 2));
        assert!(!FeatureDecorationRunner::acacia_foliage_should_skip_location(1, 0, 1, 2));
    }

    #[test]
    fn acacia_lower_layers_skip_only_outer_corners() {
        assert!(FeatureDecorationRunner::acacia_foliage_should_skip_location(2, -1, 2, 2));
        assert!(!FeatureDecorationRunner::acacia_foliage_should_skip_location(1, -1, 2, 2));
        assert!(!FeatureDecorationRunner::acacia_foliage_should_skip_location(0, -1, 0, 0));
    }
}
