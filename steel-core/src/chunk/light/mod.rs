//! Light storage primitives used by chunk and world lighting.

use steel_registry::blocks::{block_state_ext::BlockStateExt, shapes::VoxelShape};
use steel_utils::{BlockStateId, Direction, SectionPos};

use crate::physics::shapes::{face_shape_occludes, merged_face_occludes};

/// Maximum light value stored by vanilla lighting.
pub const MAX_LIGHT_LEVEL: u8 = 15;
/// Minimum opacity used while propagating vanilla light.
pub const MIN_LIGHT_OPACITY: u8 = 1;
/// Opacity returned when a block face fully blocks light.
pub const LIGHT_BLOCKED: u8 = MAX_LIGHT_LEVEL + 1;
/// Vanilla stores one extra light section below and above the build height.
pub const LIGHT_SECTION_PADDING: i32 = 1;

/// Number of blocks along one edge of a light section.
pub const DATA_LAYER_EDGE: usize = 16;
/// Number of blocks in a light section.
pub const DATA_LAYER_BLOCK_COUNT: usize = DATA_LAYER_EDGE * DATA_LAYER_EDGE * DATA_LAYER_EDGE;
/// Number of packed bytes in a light section.
pub const DATA_LAYER_SIZE: usize = DATA_LAYER_BLOCK_COUNT / 2;
const CHUNK_EDGE: usize = 16;
const CHUNK_COLUMN_COUNT: usize = CHUNK_EDGE * CHUNK_EDGE;
const NEGATIVE_INFINITY: i32 = i32::MIN;

/// Vanilla light layer kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LightLayer {
    /// Sky light propagated from dimensions with skylight.
    Sky,
    /// Block light emitted by blocks.
    Block,
}

/// Real chunk-section emptiness transition that must be applied before block checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LightSectionEmptinessChange {
    /// World section whose real block-section emptiness changed.
    pub section_pos: SectionPos,
    /// New emptiness value for the real block section.
    pub empty: bool,
}

/// Returns whether vanilla must re-check lighting after a block-state change.
#[must_use]
pub fn has_different_light_properties(old_state: BlockStateId, new_state: BlockStateId) -> bool {
    old_state != new_state
        && (old_state.get_light_dampening() != new_state.get_light_dampening()
            || old_state.get_light_emission() != new_state.get_light_emission()
            || old_state.use_shape_for_light_occlusion()
            || new_state.use_shape_for_light_occlusion())
}

/// Returns vanilla's simple opacity for light propagation.
///
/// Vanilla clamps block light dampening to at least one while propagating
/// through neighbors.
#[must_use]
pub fn get_light_opacity(state: BlockStateId) -> u8 {
    state.get_light_dampening().max(MIN_LIGHT_OPACITY)
}

/// Returns the occlusion shape vanilla lighting uses for a block state.
#[must_use]
pub fn light_occlusion_shape(state: BlockStateId) -> VoxelShape {
    if !state.get_block().config.can_occlude || !state.use_shape_for_light_occlusion() {
        return VoxelShape::EMPTY;
    }

    state.get_occlusion_shape()
}

/// Returns vanilla's `LightEngine.getLightDampeningInto` result.
#[must_use]
pub fn get_light_block_into(
    from_state: BlockStateId,
    to_state: BlockStateId,
    direction: Direction,
    simple_opacity: u8,
) -> u8 {
    let from_shape = light_occlusion_shape(from_state);
    let to_shape = light_occlusion_shape(to_state);
    if from_shape.is_empty() && to_shape.is_empty() {
        return simple_opacity;
    }

    if merged_face_occludes(from_shape, to_shape, direction) {
        LIGHT_BLOCKED
    } else {
        simple_opacity
    }
}

/// Returns whether the selected state faces fully occlude light.
#[must_use]
pub fn light_face_occludes(
    from_state: BlockStateId,
    to_state: BlockStateId,
    direction: Direction,
) -> bool {
    let from_shape = light_occlusion_shape(from_state);
    let to_shape = light_occlusion_shape(to_state);
    face_shape_occludes(from_shape, direction, to_shape, direction.opposite())
}

mod cache;
mod data_layer;
mod packet;
mod propagation;
mod queue;
mod section_storage;
mod sky_propagation;
mod sky_sources;
mod storage;
mod work_gate;
mod workset;

pub use cache::{
    CachedLightBlock, CachedLightChunk, CachedLightSection, LIGHT_CACHE_CHUNK_SLOTS,
    LIGHT_CACHE_DIAMETER, LIGHT_CACHE_RADIUS, LIGHT_CACHE_SECTION_RADIUS, LightCacheChunkScope,
    LightCacheLayout, LightCacheSetupChunks, LightCacheSetupRadius, LightChunkSectionSlots,
    LightChunkSlotArray, LightSectionSlotArray, LightUpdateNotificationCache, PackedLightBlockPos,
};
pub use data_layer::{DataLayer, DataLayerLengthError};
pub use packet::{build_chunk_light_update_packet, build_chunk_light_update_packet_for_sections};
pub use propagation::{
    BlockLightChunkEdgeChecks, BlockLightPropagationContext, BlockLightPropagationContextError,
    BlockLightUpdateResult, check_block_light_chunk_edges, force_load_block_light_chunk,
    load_block_light_chunk, propagate_block_light_changes,
    propagate_block_light_changes_with_empty_sections, propagate_block_light_chunk,
};
pub use queue::{
    LightAxisDirection, LightDirectionSet, LightDirectionSetIter, LightPropagationQueue,
    LightPropagationQueues, LightQueueEntry, LightQueueFlags, PackedLightPropagationQueue,
    PackedLightPropagationQueues, PackedLightQueueEntry, QueuedLightUpdate,
};
pub use section_storage::{LightSectionRange, LightSectionRangeError};
pub use sky_propagation::{
    SkyLightChunkEdgeChecks, SkyLightPropagationContext, SkyLightPropagationContextError,
    SkyLightUpdateResult, check_sky_light_chunk_edges, force_load_sky_light_chunk,
    load_sky_light_chunk, propagate_sky_light_changes,
    propagate_sky_light_changes_with_empty_sections, propagate_sky_light_chunk,
    propagate_sky_light_chunk_without_edge_checks,
};
pub use sky_sources::ChunkSkyLightSources;
pub use storage::{
    ChunkLightData, ChunkLightEmptinessMapLengthError, ChunkLightLayerStorage, LightSection,
    LightSectionData,
};
pub(crate) use work_gate::{LightWorkWindowGate, LightWorkWindowReservation};
pub use workset::{
    LightChunkReadCache, LightLayerEdit, LightSectionReadCache, LightWorkset,
    LightWorksetSetupError,
};

#[cfg(test)]
mod tests {
    use steel_registry::{
        blocks::{block_state_ext::BlockStateExt, properties::BlockStateProperties},
        test_support::init_test_registry,
        vanilla_blocks,
    };
    use steel_utils::BlockStateId;
    use steel_utils::{BlockPos, ChunkPos, SectionPos};

    use crate::{
        behavior::init_behaviors,
        chunk::section::{ChunkSection, Sections},
    };

    use super::{
        ChunkLightData, ChunkSkyLightSources, DATA_LAYER_SIZE, DataLayer, LightLayer, LightSection,
        LightSectionData, LightSectionRange, MAX_LIGHT_LEVEL, build_chunk_light_update_packet,
        build_chunk_light_update_packet_for_sections, get_light_opacity,
        has_different_light_properties,
    };

    fn init_light_tests() {
        init_test_registry();
        init_behaviors();
    }

    fn empty_sections(section_count: usize) -> Sections {
        let sections: Vec<ChunkSection> = (0..section_count)
            .map(|_| ChunkSection::new_empty())
            .collect();
        Sections::from_owned(sections.into_boxed_slice())
    }

    fn single_section_with_block(local_y: usize, state: BlockStateId) -> Sections {
        let mut section = ChunkSection::new_empty();
        section.set_block_state(0, local_y, 0, state);
        Sections::from_owned(vec![section].into_boxed_slice())
    }

    fn new_test_sky_sources() -> ChunkSkyLightSources {
        let Ok(sources) = ChunkSkyLightSources::new(0, 16) else {
            panic!("valid single-section height rejected");
        };
        sources
    }

    fn mask_bit(mask: &[u64], index: usize) -> bool {
        (mask[index / 64] & (1 << (index % 64))) != 0
    }

    #[test]
    fn data_layer_uses_vanilla_low_nibble_first_order() {
        let mut layer = DataLayer::new();

        layer.set(0, 0, 0, 5);
        layer.set(1, 0, 0, 12);
        layer.set(1, 2, 3, 31);

        assert_eq!(layer.get(0, 0, 0), 5);
        assert_eq!(layer.get(1, 0, 0), 12);
        assert_eq!(layer.get(1, 2, 3), MAX_LIGHT_LEVEL);
        assert_eq!(layer.get(2, 2, 3), 0);

        let bytes = layer.to_bytes();
        assert_eq!(bytes[0], 0xC5);
    }

    #[test]
    fn data_layer_preserves_homogeneous_non_zero_without_backing_bytes() {
        let layer = DataLayer::filled(15);

        assert!(layer.is_homogeneous());
        assert!(!layer.is_empty());
        assert_eq!(layer.homogeneous_value(), Some(15));
        assert!(layer.to_bytes().iter().all(|byte| *byte == 0xFF));
    }

    #[test]
    fn light_section_range_matches_vanilla_padded_section_range() {
        let range = LightSectionRange::from_world_height(-64, 384)
            .expect("vanilla overworld height should produce a light range");

        assert_eq!(range.min_section_y(), -5);
        assert_eq!(range.max_section_y_exclusive(), 21);
        assert_eq!(range.section_count(), 26);
        assert_eq!(range.chunk_section_count(), 24);
        assert_eq!(range.section_index(-5), Some(0));
        assert_eq!(range.section_y(25), Some(20));
        assert_eq!(range.section_index(21), None);
    }

    #[test]
    fn chunk_light_packet_omits_missing_and_internal_sections() {
        let mut light = ChunkLightData::for_valid_world_height(0, 16);
        *light.sky.section_mut(0).expect("real section in range") =
            LightSection::internal(LightSectionData::homogeneous(15));
        *light.block.section_mut(0).expect("real section in range") = LightSection::missing();

        let packet = build_chunk_light_update_packet(&light, true);

        assert!(!mask_bit(&packet.sky_y_mask.0, 1));
        assert!(!mask_bit(&packet.empty_sky_y_mask.0, 1));
        assert!(packet.sky_updates.is_empty());
        assert!(!mask_bit(&packet.block_y_mask.0, 1));
        assert!(!mask_bit(&packet.empty_block_y_mask.0, 1));
        assert!(packet.block_updates.is_empty());
    }

    #[test]
    fn chunk_light_packet_uses_empty_mask_for_visible_zero_sections() {
        let mut light = ChunkLightData::for_valid_world_height(0, 16);
        *light.block.section_mut(0).expect("real section in range") =
            LightSection::visible(LightSectionData::homogeneous(0));

        let packet = build_chunk_light_update_packet(&light, true);

        assert!(!mask_bit(&packet.block_y_mask.0, 1));
        assert!(mask_bit(&packet.empty_block_y_mask.0, 1));
        assert!(packet.block_updates.is_empty());
    }

    #[test]
    fn chunk_light_packet_expands_visible_homogeneous_non_zero_sections() {
        let mut light = ChunkLightData::for_valid_world_height(0, 16);
        *light.sky.section_mut(0).expect("real section in range") =
            LightSection::visible(LightSectionData::homogeneous(15));

        let packet = build_chunk_light_update_packet(&light, true);

        assert!(mask_bit(&packet.sky_y_mask.0, 1));
        assert!(!mask_bit(&packet.empty_sky_y_mask.0, 1));
        assert_eq!(packet.sky_updates.len(), 1);
        assert_eq!(packet.sky_updates[0].len(), DATA_LAYER_SIZE);
        assert!(packet.sky_updates[0].iter().all(|byte| *byte == 0xFF));
    }

    #[test]
    fn chunk_light_packet_omits_sky_layer_when_dimension_has_no_skylight() {
        let mut light = ChunkLightData::for_valid_world_height(0, 16);
        *light.sky.section_mut(0).expect("real section in range") =
            LightSection::visible(LightSectionData::homogeneous(15));

        let packet = build_chunk_light_update_packet(&light, false);

        assert!(packet.sky_updates.is_empty());
        assert!(!mask_bit(&packet.sky_y_mask.0, 1));
        assert!(!mask_bit(&packet.empty_sky_y_mask.0, 1));
    }

    #[test]
    fn changed_section_packet_preserves_ascending_light_section_order() {
        let chunk_pos = ChunkPos::new(3, -2);
        let mut light = ChunkLightData::for_valid_world_height(0, 48);
        *light.block.section_mut(2).expect("upper section in range") =
            LightSection::visible(LightSectionData::homogeneous(3));
        *light.block.section_mut(0).expect("lower section in range") =
            LightSection::visible(LightSectionData::homogeneous(7));

        let packet = build_chunk_light_update_packet_for_sections(
            chunk_pos,
            &light,
            true,
            &[],
            &[
                SectionPos::new(chunk_pos.0.x, 2, chunk_pos.0.y),
                SectionPos::new(chunk_pos.0.x, 0, chunk_pos.0.y),
            ],
        );

        assert_eq!(packet.block_updates.len(), 2);
        assert!(packet.block_updates[0].iter().all(|byte| *byte == 0x77));
        assert!(packet.block_updates[1].iter().all(|byte| *byte == 0x33));
    }

    #[test]
    fn chunk_light_data_reads_visible_block_and_sky_light() {
        let mut light = ChunkLightData::for_valid_world_height(0, 16);
        let pos = BlockPos::new(1, 2, 3);
        let mut data = LightSectionData::homogeneous(0);
        data.set(1, 2, 3, 12);
        *light.block.section_mut(0).expect("real section in range") = LightSection::visible(data);

        assert_eq!(light.get_light_value(LightLayer::Block, pos), 12);
        assert_eq!(light.get_light_value(LightLayer::Sky, pos), 15);
    }

    #[test]
    fn sections_collect_block_light_sources_in_scalable_lux_order() {
        init_light_tests();

        let torch = vanilla_blocks::TORCH.default_state();
        let lantern = vanilla_blocks::SEA_LANTERN.default_state();
        let mut lower = ChunkSection::new_empty();
        lower.set_block_state(3, 4, 5, torch);
        lower.set_block_state(1, 0, 0, lantern);
        let mut upper = ChunkSection::new_empty();
        upper.set_block_state(15, 15, 15, lantern);
        let sections =
            Sections::from_owned(vec![lower, ChunkSection::new_empty(), upper].into_boxed_slice());

        assert_eq!(
            sections.block_light_sources(ChunkPos::new(2, -3), -16),
            vec![
                BlockPos::new(33, -16, -48),
                BlockPos::new(35, -12, -43),
                BlockPos::new(47, 31, -33),
            ]
        );
    }

    #[test]
    fn light_opacity_uses_vanilla_minimum_opacity() {
        init_light_tests();
        let air = vanilla_blocks::AIR.default_state();
        let stone = vanilla_blocks::STONE.default_state();

        assert_eq!(get_light_opacity(air), 1);
        assert_eq!(get_light_opacity(stone), 15);
    }

    #[test]
    fn different_light_properties_match_vanilla_conditions() {
        init_light_tests();
        let air = vanilla_blocks::AIR.default_state();
        let stone = vanilla_blocks::STONE.default_state();

        assert!(!has_different_light_properties(air, air));
        assert!(has_different_light_properties(air, stone));

        let light = vanilla_blocks::LIGHT.default_state();
        let dim_light = light.set_value(&BlockStateProperties::LEVEL, 7);
        assert!(has_different_light_properties(light, dim_light));
    }

    #[test]
    fn sky_light_sources_empty_chunk_extends_below_world() {
        init_light_tests();
        let sections = empty_sections(1);
        let mut sources = new_test_sky_sources();

        sources.fill_from_sections(&sections);

        assert_eq!(sources.get_lowest_source_y(0, 0), i32::MIN);
        assert_eq!(sources.get_lowest_source_y(15, 15), i32::MIN);
        assert_eq!(sources.get_highest_lowest_source_y(), i32::MIN);
    }

    #[test]
    fn sky_light_sources_find_lowest_occluding_edge() {
        init_light_tests();
        let stone = vanilla_blocks::STONE.default_state();
        let sections = single_section_with_block(4, stone);
        let mut sources = new_test_sky_sources();

        sources.fill_from_sections(&sections);

        assert_eq!(sources.get_lowest_source_y(0, 0), 5);
        assert_eq!(sources.get_lowest_source_y(1, 0), i32::MIN);
        assert_eq!(sources.get_highest_lowest_source_y(), 5);
    }

    #[test]
    fn sky_light_sources_update_adds_and_removes_occluding_edge() {
        init_light_tests();
        let air = vanilla_blocks::AIR.default_state();
        let stone = vanilla_blocks::STONE.default_state();
        let sections = empty_sections(1);
        let mut sources = new_test_sky_sources();
        sources.fill_from_sections(&sections);

        let added = sources.update(0, 4, 0, |_x, y, _z| if y == 4 { stone } else { air });

        assert!(added);
        assert_eq!(sources.get_lowest_source_y(0, 0), 5);

        let removed = sources.update(0, 4, 0, |_x, _y, _z| air);

        assert!(removed);
        assert_eq!(sources.get_lowest_source_y(0, 0), i32::MIN);
    }

    #[test]
    fn sky_light_sources_update_ignores_changes_below_current_source_edge() {
        init_light_tests();
        let stone = vanilla_blocks::STONE.default_state();
        let sections = single_section_with_block(10, stone);
        let mut sources = new_test_sky_sources();
        sources.fill_from_sections(&sections);

        let changed = sources.update(0, 4, 0, |_x, _y, _z| stone);

        assert!(!changed);
        assert_eq!(sources.get_lowest_source_y(0, 0), 11);
    }
}
