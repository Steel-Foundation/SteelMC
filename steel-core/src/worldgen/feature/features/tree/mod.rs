use steel_registry::vanilla_block_tags::BlockTag;

use super::super::prelude::*;
use super::super::runner::FeatureDecorationRunner;
use super::super::vanilla_collections::JavaBlockPosSet;

mod decorators;
mod fallen;
mod foliage;
mod leaves;
mod root_system;
mod roots;
mod trunk;

impl FeatureDecorationRunner {
    pub(crate) fn place_tree_feature(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        random: &mut WorldgenRandom,
        config: &TreeConfiguration,
        origin: BlockPos,
        biome_zoom_seed: i64,
    ) -> bool {
        let mut placement = TreePlacement::default();
        let placed = Self::do_place_tree(region, registry, random, config, origin, &mut placement);
        if !placed || (placement.trunks.is_empty() && placement.foliage.is_empty()) {
            return false;
        }

        if !config.decorators.is_empty() {
            Self::place_tree_decorators(
                region,
                registry,
                random,
                &config.decorators,
                &mut placement,
                biome_zoom_seed,
            );
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
        random: &mut WorldgenRandom,
        config: &TreeConfiguration,
        origin: BlockPos,
        placement: &mut TreePlacement,
    ) -> bool {
        let tree_height = Self::tree_height(random, &config.trunk_placer);
        let foliage_height = Self::tree_foliage_height(random, tree_height, config);
        let trunk_height = tree_height - foliage_height;
        let leaf_radius = Self::tree_foliage_radius(random, &config.foliage_placer, trunk_height);
        let trunk_origin = Self::tree_root_origin(random, origin, config.root_placer.as_ref());
        let min_y = origin.y().min(trunk_origin.y());
        let max_y = origin.y().max(trunk_origin.y()) + tree_height + 1;

        if min_y < region.min_y() + 1 || max_y > region.max_y_exclusive() {
            return false;
        }

        let clipped_tree_height =
            Self::max_free_tree_height(region, tree_height, trunk_origin, config);
        let min_clipped_height = Self::tree_min_clipped_height(&config.minimum_size);
        if clipped_tree_height < tree_height
            && min_clipped_height.is_none_or(|height| clipped_tree_height < height)
        {
            return false;
        }

        if config.root_placer.is_some()
            && !Self::place_tree_roots(
                region,
                registry,
                random,
                origin,
                trunk_origin,
                config,
                placement,
            )
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
        max_tree_height: i32,
        tree_pos: BlockPos,
        config: &TreeConfiguration,
    ) -> i32 {
        for y in 0..=max_tree_height + 1 {
            let radius = Self::tree_size_at_height(&config.minimum_size, max_tree_height, y);
            for x in -radius..=radius {
                for z in -radius..=radius {
                    let pos = tree_pos.offset(x, y, z);
                    if !Self::tree_trunk_placer_is_free(region, pos, &config.trunk_placer)
                        || (!config.ignore_vines && Self::tree_is_vine(region, pos))
                    {
                        return y - 2;
                    }
                }
            }
        }

        max_tree_height
    }

    fn tree_valid_pos(region: &WorldGenRegion<'_>, pos: BlockPos) -> bool {
        let state = region.block_state(pos);
        state.is_air() || state.get_block().has_tag(&BlockTag::REPLACEABLE_BY_TREES)
    }

    fn tree_trunk_placer_is_free(
        region: &WorldGenRegion<'_>,
        pos: BlockPos,
        trunk_placer: &TrunkPlacer,
    ) -> bool {
        let state = region.block_state(pos);
        Self::tree_valid_pos_for_trunk_placer(region, pos, trunk_placer)
            || state.get_block().has_tag(&BlockTag::LOGS)
    }

    fn tree_valid_pos_for_trunk_placer(
        region: &WorldGenRegion<'_>,
        pos: BlockPos,
        trunk_placer: &TrunkPlacer,
    ) -> bool {
        match trunk_placer {
            TrunkPlacer::UpwardsBranching(placer) => {
                Self::tree_valid_pos_or_tag(region, pos, &placer.can_grow_through)
            }
            TrunkPlacer::Straight(_)
            | TrunkPlacer::Forking(_)
            | TrunkPlacer::Giant(_)
            | TrunkPlacer::Fancy(_)
            | TrunkPlacer::DarkOak(_)
            | TrunkPlacer::MegaJungle(_)
            | TrunkPlacer::Bending(_)
            | TrunkPlacer::Cherry(_) => Self::tree_valid_pos(region, pos),
        }
    }

    fn tree_valid_pos_or_tag(region: &WorldGenRegion<'_>, pos: BlockPos, tag: &Identifier) -> bool {
        let state = region.block_state(pos);
        let block = state.get_block();
        state.is_air() || block.has_tag(&BlockTag::REPLACEABLE_BY_TREES) || block.has_tag(tag)
    }

    fn tree_is_air_or_leaves(region: &WorldGenRegion<'_>, pos: BlockPos) -> bool {
        let state = region.block_state(pos);
        state.is_air() || state.get_block().has_tag(&BlockTag::LEAVES)
    }

    fn tree_is_vine(region: &WorldGenRegion<'_>, pos: BlockPos) -> bool {
        region.block_state(pos).get_block() == &vanilla_blocks::VINE
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
    roots: JavaBlockPosSet,
    trunks: JavaBlockPosSet,
    foliage: JavaBlockPosSet,
    decorations: JavaBlockPosSet,
}

impl TreePlacement {
    fn set_root(&mut self, region: &mut WorldGenRegion<'_>, pos: BlockPos, state: BlockStateId) {
        self.roots.insert(pos);
        FeatureDecorationRunner::set_tree_block(region, pos, state);
    }

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
            .insertion_order()
            .chain(placement.trunks.insertion_order())
            .chain(placement.foliage.insertion_order())
            .chain(placement.decorations.insertion_order())
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

const fn abs_i32(value: i32) -> i32 {
    if value < 0 { -value } else { value }
}
