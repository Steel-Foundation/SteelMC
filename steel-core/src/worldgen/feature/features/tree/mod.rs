use super::super::prelude::*;
use super::super::runner::FeatureDecorationRunner;

// TODO: Register `minecraft:tree` after proto chunks can carry worldgen block entities.
// Vanilla beehive decorators populate bee occupants while the tree is being placed.

const LEAF_DISTANCE_LIMIT: usize = 7;

impl FeatureDecorationRunner {
    pub(crate) fn place_tree_feature(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut Xoroshiro,
        config: &TreeConfiguration,
        origin: BlockPos,
    ) -> bool {
        if !config.decorators.is_empty() {
            panic!(
                "tree decorators require runtime support before minecraft:tree can be registered"
            );
        }

        let mut placement = TreePlacement::default();
        let placed = Self::do_place_tree(region, registry, random, config, origin, &mut placement);
        if !placed || (placement.trunks.is_empty() && placement.foliage.is_empty()) {
            return false;
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

    fn tree_foliage_offset(random: &mut Xoroshiro, foliage_placer: &FoliagePlacer) -> i32 {
        match foliage_placer {
            FoliagePlacer::Blob(placer) => placer.offset.sample(random),
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
                if !Self::blob_foliage_should_skip_location(
                    random,
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

    fn blob_foliage_should_skip_location(
        random: &mut Xoroshiro,
        dx: i32,
        y: i32,
        dz: i32,
        current_radius: i32,
        double_trunk: bool,
    ) -> bool {
        let (dx, dz) = Self::foliage_signed_distances(dx, dz, double_trunk);
        dx == current_radius && dz == current_radius && (random.next_i32_bounded(2) == 0 || y == 0)
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
