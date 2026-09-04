use rustc_hash::FxHashSet;
use steel_protocol::packets::game::LightUpdatePacketData;
use steel_utils::{ChunkPos, SectionPos, codec::BitSet};

use super::{ChunkLightData, ChunkLightLayerStorage, LightSectionData};

/// Builds protocol light-update data from chunk-owned light sections.
#[must_use]
pub fn build_chunk_light_update_packet(
    light: &ChunkLightData,
    has_skylight: bool,
) -> LightUpdatePacketData {
    let range = light.sky.range();
    let mut sky_y_mask = range.empty_bit_set();
    let mut block_y_mask = range.empty_bit_set();
    let mut empty_sky_y_mask = range.empty_bit_set();
    let mut empty_block_y_mask = range.empty_bit_set();
    let mut sky_updates = Vec::new();
    let mut block_updates = Vec::new();

    for section_index in 0..range.section_count() {
        if has_skylight {
            prepare_chunk_section_data(
                &light.sky,
                section_index,
                &mut sky_y_mask,
                &mut empty_sky_y_mask,
                &mut sky_updates,
            );
        }
        prepare_chunk_section_data(
            &light.block,
            section_index,
            &mut block_y_mask,
            &mut empty_block_y_mask,
            &mut block_updates,
        );
    }

    LightUpdatePacketData {
        sky_y_mask,
        block_y_mask,
        empty_sky_y_mask,
        empty_block_y_mask,
        sky_updates,
        block_updates,
    }
}

/// Builds protocol light-update data for the changed sections of one chunk column.
///
/// Vanilla writes update payloads in ascending light-section-index order. Keep
/// that order here even though callers pass sets/vectors of changed sections.
#[must_use]
pub fn build_chunk_light_update_packet_for_sections(
    chunk_pos: ChunkPos,
    light: &ChunkLightData,
    has_skylight: bool,
    sky_sections: &[SectionPos],
    block_sections: &[SectionPos],
) -> LightUpdatePacketData {
    let range = light.sky.range();
    let mut sky_y_mask = range.empty_bit_set();
    let mut block_y_mask = range.empty_bit_set();
    let mut empty_sky_y_mask = range.empty_bit_set();
    let mut empty_block_y_mask = range.empty_bit_set();
    let mut sky_updates = Vec::new();
    let mut block_updates = Vec::new();

    // Small deltas are scanned linearly; larger ones use a set so the per-section
    // membership test stays O(1) while walking every section of the column.
    let use_sky_set = has_skylight && sky_sections.len() > 4;
    let use_block_set = block_sections.len() > 4;
    let sky_set: FxHashSet<SectionPos> = if use_sky_set {
        sky_sections.iter().copied().collect()
    } else {
        FxHashSet::default()
    };
    let block_set: FxHashSet<SectionPos> = if use_block_set {
        block_sections.iter().copied().collect()
    } else {
        FxHashSet::default()
    };

    for section_index in 0..range.section_count() {
        let Some(section_y) = range.section_y(section_index) else {
            continue;
        };
        let section_pos = SectionPos::new(chunk_pos.0.x, section_y, chunk_pos.0.y);

        let sky_matched = if use_sky_set {
            sky_set.contains(&section_pos)
        } else {
            sky_sections.contains(&section_pos)
        };
        let block_matched = if use_block_set {
            block_set.contains(&section_pos)
        } else {
            block_sections.contains(&section_pos)
        };

        if sky_matched {
            prepare_chunk_section_data(
                &light.sky,
                section_index,
                &mut sky_y_mask,
                &mut empty_sky_y_mask,
                &mut sky_updates,
            );
        }

        if block_matched {
            prepare_chunk_section_data(
                &light.block,
                section_index,
                &mut block_y_mask,
                &mut empty_block_y_mask,
                &mut block_updates,
            );
        }
    }

    LightUpdatePacketData {
        sky_y_mask,
        block_y_mask,
        empty_sky_y_mask,
        empty_block_y_mask,
        sky_updates,
        block_updates,
    }
}

fn prepare_chunk_section_data(
    storage: &ChunkLightLayerStorage,
    section_index: usize,
    mask: &mut BitSet,
    empty_mask: &mut BitSet,
    updates: &mut Vec<Vec<u8>>,
) {
    let Some(section) = storage.sections().get(section_index) else {
        return;
    };
    let Some(data) = section.visible_data() else {
        return;
    };

    prepare_section_data(data, section_index, mask, empty_mask, updates);
}

fn prepare_section_data(
    data: &LightSectionData,
    section_index: usize,
    mask: &mut BitSet,
    empty_mask: &mut BitSet,
    updates: &mut Vec<Vec<u8>>,
) {
    if data.is_empty() {
        empty_mask.set(section_index, true);
        return;
    }

    let bytes = data.to_bytes();
    mask.set(section_index, true);
    updates.push(bytes.as_ref().to_vec());
}
