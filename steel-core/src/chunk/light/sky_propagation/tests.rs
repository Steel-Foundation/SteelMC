use std::sync::{Arc, Weak};

use steel_registry::{init_vanilla_registry, vanilla_blocks};
use steel_utils::types::UpdateFlags;

use super::*;
use crate::behavior::init_behaviors;
use crate::chunk::{
    Chunk,
    chunk_holder::ChunkHolder,
    chunk_ticket_manager::ChunkTicketLevel,
    light::{LightCacheSetupRadius, LightSection, LightSectionData, LightSectionRange},
    section::{ChunkSection, Sections},
    status::ChunkStatus,
};

fn init_tests() {
    init_vanilla_registry();
    init_behaviors();
}

fn range() -> LightSectionRange {
    let Ok(range) = LightSectionRange::from_world_height(0, 16) else {
        panic!("test height should create a valid light range");
    };
    range
}

fn holder_with_section(pos: ChunkPos, section: ChunkSection) -> Arc<ChunkHolder> {
    holder_with_sections(pos, vec![section])
}

fn holder_with_sections(pos: ChunkPos, sections: Vec<ChunkSection>) -> Arc<ChunkHolder> {
    let height = (sections.len() * 16) as i32;
    let proto = Chunk::new(
        Sections::from_owned(sections.into_boxed_slice()),
        pos,
        0,
        height,
        Weak::new(),
    );
    proto.initialize_light_sources();
    let holder = Arc::new(ChunkHolder::new(
        pos,
        ChunkTicketLevel::FULL_CHUNK,
        Some(ChunkTicketLevel::FULL_CHUNK),
        0,
        height,
    ));
    holder.insert_chunk(proto, ChunkStatus::Light);
    holder
}

fn empty_holder_with_section_count(pos: ChunkPos, section_count: usize) -> Arc<ChunkHolder> {
    holder_with_sections(
        pos,
        (0..section_count)
            .map(|_| ChunkSection::new_empty())
            .collect(),
    )
}

fn horizontal_empty_neighbors(
    center: ChunkPos,
    section_count: usize,
) -> Vec<(ChunkPos, Arc<ChunkHolder>)> {
    [
        ChunkPos::new(center.0.x, center.0.y - 1),
        ChunkPos::new(center.0.x, center.0.y + 1),
        ChunkPos::new(center.0.x - 1, center.0.y),
        ChunkPos::new(center.0.x + 1, center.0.y),
    ]
    .into_iter()
    .map(|pos| (pos, empty_holder_with_section_count(pos, section_count)))
    .collect()
}

fn roofed_holder(
    pos: ChunkPos,
    section_count: usize,
    roof_section_index: usize,
    roof_local_y: usize,
) -> Arc<ChunkHolder> {
    let mut sections = (0..section_count)
        .map(|_| ChunkSection::new_empty())
        .collect::<Vec<_>>();
    for z in 0..16 {
        for x in 0..16 {
            sections[roof_section_index].set_block_state(
                x,
                roof_local_y,
                z,
                vanilla_blocks::STONE.default_state(),
            );
        }
    }
    holder_with_sections(pos, sections)
}

fn roofed_holder_square(
    center: ChunkPos,
    radius: i32,
    section_count: usize,
    roof_section_index: usize,
    roof_local_y: usize,
) -> Vec<(ChunkPos, Arc<ChunkHolder>)> {
    let mut holders = Vec::new();
    for z in -radius..=radius {
        for x in -radius..=radius {
            let pos = ChunkPos::new(center.0.x + x, center.0.y + z);
            holders.push((
                pos,
                roofed_holder(pos, section_count, roof_section_index, roof_local_y),
            ));
        }
    }
    holders
}

fn find_holder(
    holders: &[(ChunkPos, Arc<ChunkHolder>)],
    pos: ChunkPos,
) -> Option<Arc<ChunkHolder>> {
    holders
        .iter()
        .find(|(holder_pos, _)| *holder_pos == pos)
        .map(|(_, holder)| Arc::clone(holder))
}

fn set_visible_sky_light(
    holder: &ChunkHolder,
    section_y: i32,
    x: usize,
    y: usize,
    z: usize,
    level: u8,
) {
    let mut data = LightSectionData::homogeneous(0);
    data.set(x, y, z, level);
    set_sky_light_section(holder, section_y, LightSection::visible(data));
}

fn set_sky_light_section(holder: &ChunkHolder, section_y: i32, section: LightSection) {
    let Some(chunk) = holder.try_chunk(ChunkStatus::Empty) else {
        panic!("test chunk should be available");
    };
    let mut light = chunk.light_mut();
    let Some(target) = light.sky.section_mut(section_y) else {
        panic!("test section should be inside light range");
    };
    *target = section;
}

fn sky_light_at(holder: &ChunkHolder, pos: BlockPos) -> u8 {
    let Some(chunk) = holder.try_chunk(ChunkStatus::Empty) else {
        panic!("test chunk should be available");
    };
    chunk.light().get_light_value(LightLayer::Sky, pos)
}

#[test]
fn context_requires_sky_layer() {
    init_tests();
    let center = ChunkPos::new(0, 0);
    let holder = holder_with_section(center, ChunkSection::new_empty());
    let layout = LightCacheLayout::new(center, range());
    let Ok(workset) = LightWorkset::setup(
        layout,
        LightCacheSetupRadius::Inner,
        true,
        |pos| (pos == center).then(|| Arc::clone(&holder)),
        |_| true,
    ) else {
        panic!("relaxed setup should accept missing neighbors");
    };

    workset.with_chunk_read_cache(|chunk_cache| {
        chunk_cache.with_section_read_cache(|section_cache| {
            chunk_cache.with_light_edit(LightLayer::Block, |mut light_edit| {
                let mut queues = PackedLightPropagationQueues::new();
                let result =
                    SkyLightPropagationContext::new(section_cache, &mut light_edit, &mut queues);

                assert_eq!(
                    result.err(),
                    Some(SkyLightPropagationContextError::WrongLayer {
                        layer: LightLayer::Block,
                    })
                );
            });
        });
    });
}

#[test]
fn sky_light_chunk_without_edge_checks_propagates_down_air_column() {
    init_tests();
    let center = ChunkPos::new(0, 0);
    let mut section = ChunkSection::new_empty();
    section.set_block_state(1, 0, 1, vanilla_blocks::STONE.default_state());
    let holder = holder_with_section(center, section);
    let neighbors = horizontal_empty_neighbors(center, 1);
    let layout = LightCacheLayout::new(center, range());
    let Ok(workset) = LightWorkset::setup(
        layout,
        LightCacheSetupRadius::Inner,
        true,
        |pos| {
            if pos == center {
                Some(Arc::clone(&holder))
            } else {
                find_holder(&neighbors, pos)
            }
        },
        |_| true,
    ) else {
        panic!("relaxed setup should accept missing neighbors");
    };

    let Ok(result) = propagate_sky_light_chunk_without_edge_checks(&workset) else {
        panic!("matching sky caches should run sky chunk lighting");
    };

    assert!(result.updated_sections.contains(&SectionPos::new(0, 0, 0)));
    assert_eq!(sky_light_at(&holder, BlockPos::new(1, 15, 1)), 15);
    assert_eq!(sky_light_at(&holder, BlockPos::new(1, 1, 1)), 15);
    assert_eq!(sky_light_at(&holder, BlockPos::new(1, 0, 1)), 0);
}

#[test]
fn sky_light_chunk_without_edge_checks_keeps_sealed_roof_dark() {
    init_tests();
    let center = ChunkPos::new(0, 0);
    let mut section = ChunkSection::new_empty();
    for z in 0..16 {
        for x in 0..16 {
            section.set_block_state(x, 15, z, vanilla_blocks::STONE.default_state());
        }
    }
    let holder = holder_with_section(center, section);
    let neighbors = roofed_holder_square(center, 2, 1, 0, 15);
    let layout = LightCacheLayout::new(center, range());
    let Ok(workset) = LightWorkset::setup_with_scopes(
        layout,
        LightCacheSetupRadius::Full,
        true,
        |pos| {
            if pos == center {
                Some(Arc::clone(&holder))
            } else {
                find_holder(&neighbors, pos)
            }
        },
        |cached_chunk, _, _| (true, cached_chunk.chunk_pos == center),
    ) else {
        panic!("relaxed setup should accept missing neighbors");
    };

    let Ok(result) = propagate_sky_light_chunk_without_edge_checks(&workset) else {
        panic!("matching sky caches should run sky chunk lighting");
    };

    assert!(result.updated_sections.contains(&SectionPos::new(0, 0, 0)));
    assert_eq!(sky_light_at(&holder, BlockPos::new(8, 14, 8)), 0);
    assert_eq!(sky_light_at(&holder, BlockPos::new(8, 15, 8)), 0);
}

#[test]
fn sky_light_changes_add_and_remove_air_column_shadow() {
    init_tests();
    let center = ChunkPos::new(0, 0);
    let holder = holder_with_section(center, ChunkSection::new_empty());
    let changed_pos = BlockPos::new(1, 14, 1);
    let layout = LightCacheLayout::new(center, range());

    let Some(chunk) = holder.try_chunk(ChunkStatus::Empty) else {
        panic!("test chunk should be available");
    };
    assert!(
        chunk
            .set_block_state_for_generation(
                ChunkStatus::Light,
                changed_pos,
                vanilla_blocks::STONE.default_state(),
                UpdateFlags::UPDATE_CLIENTS,
            )
            .is_some()
    );

    let Ok(workset) = LightWorkset::setup(
        layout,
        LightCacheSetupRadius::Inner,
        true,
        |pos| (pos == center).then(|| Arc::clone(&holder)),
        |_| true,
    ) else {
        panic!("relaxed setup should accept missing neighbors");
    };

    let Ok(result) = propagate_sky_light_changes_with_empty_sections(
        &workset,
        [changed_pos],
        [LightSectionEmptinessChange {
            section_pos: SectionPos::new(0, 0, 0),
            empty: false,
        }],
    ) else {
        panic!("matching sky caches should run sky block changes");
    };

    assert!(result.updated_sections.contains(&SectionPos::new(0, 0, 0)));
    assert_eq!(sky_light_at(&holder, BlockPos::new(1, 15, 1)), 15);
    assert_eq!(sky_light_at(&holder, changed_pos), 0);
    assert_eq!(sky_light_at(&holder, BlockPos::new(1, 13, 1)), 14);

    let Some(chunk) = holder.try_chunk(ChunkStatus::Empty) else {
        panic!("test chunk should be available");
    };
    assert!(
        chunk
            .set_block_state_for_generation(
                ChunkStatus::Light,
                changed_pos,
                vanilla_blocks::AIR.default_state(),
                UpdateFlags::UPDATE_CLIENTS,
            )
            .is_some()
    );

    let Ok(workset) = LightWorkset::setup(
        layout,
        LightCacheSetupRadius::Inner,
        true,
        |pos| (pos == center).then(|| Arc::clone(&holder)),
        |_| true,
    ) else {
        panic!("relaxed setup should accept missing neighbors");
    };
    let Ok(result) = propagate_sky_light_changes_with_empty_sections(
        &workset,
        [changed_pos],
        [LightSectionEmptinessChange {
            section_pos: SectionPos::new(0, 0, 0),
            empty: true,
        }],
    ) else {
        panic!("matching sky caches should run sky block changes");
    };

    assert!(result.updated_sections.contains(&SectionPos::new(0, 0, 0)));
    assert_eq!(sky_light_at(&holder, BlockPos::new(1, 15, 1)), 15);
    assert_eq!(sky_light_at(&holder, changed_pos), 15);
    assert_eq!(sky_light_at(&holder, BlockPos::new(1, 13, 1)), 15);
}

#[test]
fn sky_light_chunk_edge_checks_pull_neighbor_under_ceiling() {
    init_tests();
    let center = ChunkPos::new(0, 0);
    let east_chunk = ChunkPos::new(1, 0);
    let mut center_section = ChunkSection::new_empty();
    for z in 0..16 {
        for x in 0..16 {
            center_section.set_block_state(x, 15, z, vanilla_blocks::STONE.default_state());
        }
    }
    let center_holder = holder_with_section(center, center_section);
    let east_holder = holder_with_section(east_chunk, ChunkSection::new_empty());
    let neighbors = horizontal_empty_neighbors(center, 1);
    set_visible_sky_light(&east_holder, 0, 0, 14, 1, 15);
    let layout = LightCacheLayout::new(center, range());
    let Ok(workset) = LightWorkset::setup(
        layout,
        LightCacheSetupRadius::Inner,
        true,
        |pos| {
            if pos == center {
                Some(Arc::clone(&center_holder))
            } else if pos == east_chunk {
                Some(Arc::clone(&east_holder))
            } else {
                find_holder(&neighbors, pos)
            }
        },
        |_| true,
    ) else {
        panic!("relaxed setup should accept missing neighbors");
    };

    let Ok(result) = propagate_sky_light_chunk(&workset, SkyLightChunkEdgeChecks::Required) else {
        panic!("matching sky caches should run sky chunk lighting");
    };

    assert!(result.updated_sections.contains(&SectionPos::new(0, 0, 0)));
    assert_eq!(sky_light_at(&center_holder, BlockPos::new(15, 14, 1)), 14);
    assert_eq!(sky_light_at(&center_holder, BlockPos::new(14, 14, 1)), 13);
    assert_eq!(sky_light_at(&center_holder, BlockPos::new(15, 15, 1)), 0);
}

#[test]
fn sky_light_chunk_requires_center_chunk() {
    init_tests();
    let center = ChunkPos::new(0, 0);
    let layout = LightCacheLayout::new(center, range());
    let Ok(workset) = LightWorkset::setup(
        layout,
        LightCacheSetupRadius::Inner,
        true,
        |_| None,
        |_| true,
    ) else {
        panic!("relaxed setup should accept missing chunks");
    };

    assert_eq!(
        propagate_sky_light_chunk_without_edge_checks(&workset).err(),
        Some(SkyLightPropagationContextError::MissingCenterChunk { chunk_pos: center })
    );
}

#[test]
fn sky_light_changes_skip_missing_center_chunk() {
    init_tests();
    let center = ChunkPos::new(0, 0);
    let layout = LightCacheLayout::new(center, range());
    let Ok(workset) = LightWorkset::setup(
        layout,
        LightCacheSetupRadius::Full,
        true,
        |_| None,
        |_| true,
    ) else {
        panic!("relaxed setup should accept missing chunks");
    };

    let Ok(result) = propagate_sky_light_changes_with_empty_sections(
        &workset,
        [BlockPos::new(1, 1, 1)],
        [LightSectionEmptinessChange {
            section_pos: SectionPos::new(0, 0, 0),
            empty: true,
        }],
    ) else {
        panic!("dynamic sky changes should skip a missing center chunk");
    };

    assert_eq!(result.updated_sections.len(), 0);
}
