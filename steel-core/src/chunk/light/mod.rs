//! Light storage primitives used by chunk and world lighting.

/// Maximum light value stored by vanilla lighting.
pub const MAX_LIGHT_LEVEL: u8 = 15;
/// Vanilla stores one extra light section below and above the build height.
pub const LIGHT_SECTION_PADDING: i32 = 1;

/// Number of blocks along one edge of a light section.
pub const DATA_LAYER_EDGE: usize = 16;
/// Number of blocks in a light section.
pub const DATA_LAYER_BLOCK_COUNT: usize = DATA_LAYER_EDGE * DATA_LAYER_EDGE * DATA_LAYER_EDGE;
/// Number of packed bytes in a light section.
pub const DATA_LAYER_SIZE: usize = DATA_LAYER_BLOCK_COUNT / 2;

/// Vanilla light layer kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LightLayer {
    /// Sky light propagated from dimensions with skylight.
    Sky,
    /// Block light emitted by blocks.
    Block,
}

mod data_layer;
mod packet;
mod section_storage;
mod storage;

pub use data_layer::{DataLayer, DataLayerLengthError};
pub use packet::{build_chunk_light_update_packet, build_chunk_light_update_packet_for_sections};
pub use section_storage::{LightSectionRange, LightSectionRangeError};
pub use storage::{
    ChunkLightData, ChunkLightEmptinessMapLengthError, ChunkLightLayerStorage, LightSection,
    LightSectionData,
};

#[cfg(test)]
mod tests {
    use steel_utils::{BlockPos, ChunkPos, SectionPos};

    use super::{
        ChunkLightData, DATA_LAYER_SIZE, DataLayer, LightLayer, LightSection, LightSectionData,
        LightSectionRange, MAX_LIGHT_LEVEL, build_chunk_light_update_packet,
        build_chunk_light_update_packet_for_sections,
    };

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
}
