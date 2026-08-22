use steel_registry::vanilla_block_tags::BlockTag;

use super::super::prelude::*;
use super::super::runner::FeatureDecorationRunner;
use super::super::vanilla_collections::JavaBlockPosSet;
use steel_utils::ChunkPos;

use rustc_hash::FxHashMap;
use std::{
    cell::{Cell, RefCell},
    sync::Arc,
};

use crate::block_entity::SharedBlockEntity;
use crate::world::{ScheduledTickAccess, World};

mod decorators;
mod fallen;
mod foliage;
mod leaves;
mod root_system;
mod roots;
mod trunk;

/// Level operations used by tree placement in worldgen and live worlds.
pub(crate) trait TreeLevel: LevelAccessor {
    fn block_state(&self, pos: BlockPos) -> BlockStateId {
        self.get_block_state(pos)
    }

    fn block_entity(&self, pos: BlockPos) -> Option<SharedBlockEntity> {
        self.get_block_entity(pos)
    }

    fn height_at(&self, heightmap_type: HeightmapType, x: i32, z: i32) -> i32;

    fn can_write_to_chunk(&self, chunk_x: i32, chunk_z: i32) -> bool;

    fn requires_live_write_preflight(&self) -> bool {
        false
    }

    fn place_nested_configured_feature(
        &mut self,
        registry: &Registry,
        random: &mut WorldgenRandom,
        kind: &ConfiguredFeatureKind,
        origin: BlockPos,
        biome_zoom_seed: i64,
    ) -> bool;
}

impl TreeLevel for WorldGenRegion<'_> {
    fn height_at(&self, heightmap_type: HeightmapType, x: i32, z: i32) -> i32 {
        WorldGenRegion::height_at(self, heightmap_type, x, z)
    }

    fn can_write_to_chunk(&self, chunk_x: i32, chunk_z: i32) -> bool {
        WorldGenRegion::can_write_to_chunk(self, chunk_x, chunk_z)
    }

    fn place_nested_configured_feature(
        &mut self,
        registry: &Registry,
        random: &mut WorldgenRandom,
        kind: &ConfiguredFeatureKind,
        origin: BlockPos,
        biome_zoom_seed: i64,
    ) -> bool {
        FeatureDecorationRunner::place_configured_feature_kind(
            self,
            registry,
            random,
            kind,
            origin,
            biome_zoom_seed,
        )
    }
}

impl TreeLevel for Arc<World> {
    fn height_at(&self, heightmap_type: HeightmapType, x: i32, z: i32) -> i32 {
        self.as_ref()
            .height_at(heightmap_type, x, z)
            .unwrap_or_else(|| self.min_y())
    }

    fn can_write_to_chunk(&self, chunk_x: i32, chunk_z: i32) -> bool {
        self.chunk_map
            .with_full_chunk(ChunkPos::new(chunk_x, chunk_z), |_| ())
            .is_some()
    }

    fn requires_live_write_preflight(&self) -> bool {
        true
    }

    fn place_nested_configured_feature(
        &mut self,
        _registry: &Registry,
        _random: &mut WorldgenRandom,
        _kind: &ConfiguredFeatureKind,
        _origin: BlockPos,
        _biome_zoom_seed: i64,
    ) -> bool {
        false
    }
}

impl FeatureDecorationRunner {
    pub(crate) fn place_tree_feature(
        region: &mut impl TreeLevel,
        registry: &Registry,
        random: &mut WorldgenRandom,
        config: &TreeConfiguration,
        origin: BlockPos,
        biome_zoom_seed: i64,
    ) -> bool {
        if region.requires_live_write_preflight() {
            let mut preflight_random = random.clone();
            let mut preflight_region = TreeWritePreflight::new(region);
            let mut preflight_placement = TreePlacement::default();
            let preflight_placed = Self::do_place_tree(
                &mut preflight_region,
                registry,
                &mut preflight_random,
                config,
                origin,
                &mut preflight_placement,
            );
            if !preflight_placed || preflight_region.failed() {
                return false;
            }
        }

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
        region: &mut impl TreeLevel,
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
        region: &impl TreeLevel,
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

    fn tree_valid_pos(region: &impl TreeLevel, pos: BlockPos) -> bool {
        let state = region.block_state(pos);
        state.is_air() || state.get_block().has_tag(&BlockTag::REPLACEABLE_BY_TREES)
    }

    fn tree_trunk_placer_is_free(
        region: &impl TreeLevel,
        pos: BlockPos,
        trunk_placer: &TrunkPlacer,
    ) -> bool {
        let state = region.block_state(pos);
        Self::tree_valid_pos_for_trunk_placer(region, pos, trunk_placer)
            || state.get_block().has_tag(&BlockTag::LOGS)
    }

    fn tree_valid_pos_for_trunk_placer(
        region: &impl TreeLevel,
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

    fn tree_valid_pos_or_tag(region: &impl TreeLevel, pos: BlockPos, tag: &Identifier) -> bool {
        let state = region.block_state(pos);
        let block = state.get_block();
        state.is_air() || block.has_tag(&BlockTag::REPLACEABLE_BY_TREES) || block.has_tag(tag)
    }

    fn tree_is_air_or_leaves(region: &impl TreeLevel, pos: BlockPos) -> bool {
        let state = region.block_state(pos);
        state.is_air() || state.get_block().has_tag(&BlockTag::LEAVES)
    }

    fn tree_is_vine(region: &impl TreeLevel, pos: BlockPos) -> bool {
        region.block_state(pos).get_block() == &vanilla_blocks::VINE
    }

    fn set_tree_block(region: &mut impl TreeLevel, pos: BlockPos, state: BlockStateId) {
        let flags = UpdateFlags::UPDATE_NEIGHBORS
            | UpdateFlags::UPDATE_CLIENTS
            | UpdateFlags::UPDATE_KNOWN_SHAPE;
        let _ = region.set_block_state(pos, state, flags);
    }
}

struct TreeWritePreflight<'a, L: TreeLevel + ?Sized> {
    level: &'a mut L,
    writes: RefCell<FxHashMap<BlockPos, BlockStateId>>,
    failed: Cell<bool>,
}

impl<'a, L: TreeLevel + ?Sized> TreeWritePreflight<'a, L> {
    fn new(level: &'a mut L) -> Self {
        Self {
            level,
            writes: RefCell::new(FxHashMap::default()),
            failed: Cell::new(false),
        }
    }

    const fn failed(&self) -> bool {
        self.failed.get()
    }
}

impl<L: TreeLevel + ?Sized> LevelReader for TreeWritePreflight<'_, L> {
    fn get_block_state(&self, pos: BlockPos) -> BlockStateId {
        self.writes
            .borrow()
            .get(&pos)
            .copied()
            .unwrap_or_else(|| self.level.get_block_state(pos))
    }

    fn get_block_entity(&self, pos: BlockPos) -> Option<SharedBlockEntity> {
        self.level.get_block_entity(pos)
    }

    fn is_face_sturdy_for(
        &self,
        state: BlockStateId,
        pos: BlockPos,
        direction: Direction,
        support_type: shapes::SupportType,
    ) -> bool {
        self.level
            .is_face_sturdy_for(state, pos, direction, support_type)
    }

    fn raw_brightness(&self, pos: BlockPos, sky_darkening: u8) -> u8 {
        self.level.raw_brightness(pos, sky_darkening)
    }

    fn can_see_sky(&self, pos: BlockPos) -> bool {
        self.level.can_see_sky(pos)
    }

    fn ambient_light(&self) -> f32 {
        self.level.ambient_light()
    }

    fn min_y(&self) -> i32 {
        self.level.min_y()
    }

    fn height(&self) -> i32 {
        self.level.height()
    }
}

impl<L: TreeLevel + ?Sized> ScheduledTickAccess for TreeWritePreflight<'_, L> {
    fn fluid_tick_delay(&self, fluid: FluidRef) -> i32 {
        self.level.fluid_tick_delay(fluid)
    }

    fn schedule_block_tick_default(&self, _pos: BlockPos, _block: BlockRef, _delay: i32) -> bool {
        false
    }

    fn schedule_fluid_tick_default(&self, _pos: BlockPos, _fluid: FluidRef, _delay: i32) -> bool {
        false
    }
}

impl<L: TreeLevel + ?Sized> LevelAccessor for TreeWritePreflight<'_, L> {
    fn set_block_state(&self, pos: BlockPos, state: BlockStateId, _flags: UpdateFlags) -> bool {
        let chunk_x = SectionPos::block_to_section_coord(pos.x());
        let chunk_z = SectionPos::block_to_section_coord(pos.z());
        if !self.level.can_write_to_chunk(chunk_x, chunk_z) {
            self.failed.set(true);
            return false;
        }

        self.writes.borrow_mut().insert(pos, state);
        true
    }
}

impl<L: TreeLevel + ?Sized> TreeLevel for TreeWritePreflight<'_, L> {
    fn block_state(&self, pos: BlockPos) -> BlockStateId {
        self.get_block_state(pos)
    }

    fn height_at(&self, heightmap_type: HeightmapType, x: i32, z: i32) -> i32 {
        self.level.height_at(heightmap_type, x, z)
    }

    fn can_write_to_chunk(&self, chunk_x: i32, chunk_z: i32) -> bool {
        self.level.can_write_to_chunk(chunk_x, chunk_z)
    }

    fn place_nested_configured_feature(
        &mut self,
        _registry: &Registry,
        _random: &mut WorldgenRandom,
        _kind: &ConfiguredFeatureKind,
        _origin: BlockPos,
        _biome_zoom_seed: i64,
    ) -> bool {
        false
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
    fn set_root(&mut self, region: &mut impl TreeLevel, pos: BlockPos, state: BlockStateId) {
        self.roots.insert(pos);
        FeatureDecorationRunner::set_tree_block(region, pos, state);
    }

    fn set_trunk(&mut self, region: &mut impl TreeLevel, pos: BlockPos, state: BlockStateId) {
        self.trunks.insert(pos);
        FeatureDecorationRunner::set_tree_block(region, pos, state);
    }

    fn set_foliage(&mut self, region: &mut impl TreeLevel, pos: BlockPos, state: BlockStateId) {
        self.foliage.insert(pos);
        FeatureDecorationRunner::set_tree_block(region, pos, state);
    }

    fn set_decoration(&mut self, region: &mut impl TreeLevel, pos: BlockPos, state: BlockStateId) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use steel_registry::{init_vanilla_registry, vanilla_blocks};

    struct WriteTestLevel {
        can_write: bool,
    }

    impl LevelReader for WriteTestLevel {
        fn get_block_state(&self, _pos: BlockPos) -> BlockStateId {
            vanilla_blocks::AIR.default_state()
        }

        fn raw_brightness(&self, _pos: BlockPos, _sky_darkening: u8) -> u8 {
            0
        }

        fn min_y(&self) -> i32 {
            -64
        }

        fn height(&self) -> i32 {
            384
        }
    }

    impl ScheduledTickAccess for WriteTestLevel {
        fn fluid_tick_delay(&self, _fluid: FluidRef) -> i32 {
            0
        }

        fn schedule_block_tick_default(
            &self,
            _pos: BlockPos,
            _block: BlockRef,
            _delay: i32,
        ) -> bool {
            false
        }

        fn schedule_fluid_tick_default(
            &self,
            _pos: BlockPos,
            _fluid: FluidRef,
            _delay: i32,
        ) -> bool {
            false
        }
    }

    impl LevelAccessor for WriteTestLevel {
        fn set_block_state(
            &self,
            _pos: BlockPos,
            _state: BlockStateId,
            _flags: UpdateFlags,
        ) -> bool {
            self.can_write
        }
    }

    impl TreeLevel for WriteTestLevel {
        fn height_at(&self, _heightmap_type: HeightmapType, _x: i32, _z: i32) -> i32 {
            0
        }

        fn can_write_to_chunk(&self, _chunk_x: i32, _chunk_z: i32) -> bool {
            self.can_write
        }

        fn place_nested_configured_feature(
            &mut self,
            _registry: &Registry,
            _random: &mut WorldgenRandom,
            _kind: &ConfiguredFeatureKind,
            _origin: BlockPos,
            _biome_zoom_seed: i64,
        ) -> bool {
            false
        }
    }

    #[test]
    fn tree_write_preflight_rejects_unwritable_chunks() {
        init_vanilla_registry();
        let mut level = WriteTestLevel { can_write: false };
        let preflight_state = vanilla_blocks::OAK_LOG.default_state();
        let preflight = TreeWritePreflight::new(&mut level);

        assert!(!preflight.set_block_state(
            BlockPos::ZERO,
            preflight_state,
            UpdateFlags::UPDATE_ALL,
        ));
        assert!(preflight.failed());
        assert_eq!(
            preflight.get_block_state(BlockPos::ZERO),
            vanilla_blocks::AIR.default_state()
        );
    }

    #[test]
    fn tree_write_preflight_overlays_accepted_writes() {
        init_vanilla_registry();
        let mut level = WriteTestLevel { can_write: true };
        let preflight_state = vanilla_blocks::OAK_LOG.default_state();
        let preflight = TreeWritePreflight::new(&mut level);

        assert!(preflight.set_block_state(
            BlockPos::ZERO,
            preflight_state,
            UpdateFlags::UPDATE_ALL,
        ));
        assert!(!preflight.failed());
        assert_eq!(preflight.get_block_state(BlockPos::ZERO), preflight_state);
    }
}
