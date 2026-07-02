use std::sync::Arc;

use crate::chunk::{
    chunk_access::ChunkStatus,
    chunk_generation_task::StaticCache2D,
    chunk_holder::ChunkHolder,
    chunk_pyramid::ChunkStep,
    light::{
        BlockLightChunkEdgeChecks, LightCacheLayout, LightCacheSetupRadius, LightLayer,
        LightSectionRange, LightWorkset, SkyLightChunkEdgeChecks, check_block_light_chunk_edges,
        check_sky_light_chunk_edges, force_load_block_light_chunk, force_load_sky_light_chunk,
        propagate_block_light_chunk, propagate_sky_light_chunk,
    },
};
use crate::worldgen::context::WorldGenContext;
use steel_utils::SectionPos;

pub(crate) fn initialize(
    _context: Arc<WorldGenContext>,
    _step: &ChunkStep,
    _cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
    holder: Arc<ChunkHolder>,
) {
    let Some(chunk) = holder.try_chunk(ChunkStatus::Features) else {
        panic!("Chunk not found at status Features");
    };

    chunk.initialize_light_sources();
}

pub(crate) fn generate(
    context: Arc<WorldGenContext>,
    _step: &ChunkStep,
    cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
    holder: Arc<ChunkHolder>,
) {
    let (sky_updates, block_updates) = run_light_stage(
        cache,
        holder.as_ref(),
        context.world().dimension_type.has_skylight,
    );
    publish_light_updates(&context, LightLayer::Sky, sky_updates);
    publish_light_updates(&context, LightLayer::Block, block_updates);
}

pub(crate) fn load(
    context: Arc<WorldGenContext>,
    _step: &ChunkStep,
    cache: &Arc<StaticCache2D<Arc<ChunkHolder>>>,
    holder: Arc<ChunkHolder>,
) {
    let (sky_updates, block_updates) = run_loaded_light_stage(
        cache,
        holder.as_ref(),
        context.world().dimension_type.has_skylight,
    );
    publish_light_updates(&context, LightLayer::Sky, sky_updates);
    publish_light_updates(&context, LightLayer::Block, block_updates);
}

fn run_light_stage(
    cache: &StaticCache2D<Arc<ChunkHolder>>,
    holder: &ChunkHolder,
    has_skylight: bool,
) -> (Vec<SectionPos>, Vec<SectionPos>) {
    let center = holder.get_pos();
    assert!(
        holder.try_chunk(ChunkStatus::InitializeLight).is_some(),
        "Chunk not found at status InitializeLight"
    );

    let Ok(range) = LightSectionRange::from_world_height(holder.min_y(), holder.height()) else {
        panic!("invalid world height for light stage");
    };

    let layout = LightCacheLayout::new(center, range);
    let Ok(workset) = LightWorkset::setup_with_scopes(
        layout,
        LightCacheSetupRadius::Full,
        true,
        |pos| {
            let holder = cache.try_get(pos.0.x, pos.0.y)?;
            holder
                .try_chunk(ChunkStatus::InitializeLight)
                .is_some()
                .then(|| Arc::clone(holder))
        },
        |cached_chunk, holder, _chunk| {
            let status = holder.persisted_status();
            let center_chunk = cached_chunk.chunk_pos == center;
            let initialized = status.is_some_and(|status| status >= ChunkStatus::InitializeLight);
            let lit = status.is_some_and(|status| status >= ChunkStatus::Light);
            (center_chunk || initialized, center_chunk || lit)
        },
    ) else {
        panic!("required light-stage chunk is missing");
    };

    let sky_updates = if has_skylight {
        match propagate_sky_light_chunk(&workset, SkyLightChunkEdgeChecks::Required) {
            Ok(result) => result.updated_sections,
            Err(error) => panic!("sky light chunk propagation failed: {error:?}"),
        }
    } else {
        Vec::new()
    };
    let block_result =
        match propagate_block_light_chunk(&workset, BlockLightChunkEdgeChecks::Required) {
            Ok(result) => result,
            Err(error) => panic!("block light chunk propagation failed: {error:?}"),
        };

    (sky_updates, block_result.updated_sections)
}

fn run_loaded_light_stage(
    cache: &StaticCache2D<Arc<ChunkHolder>>,
    holder: &ChunkHolder,
    has_skylight: bool,
) -> (Vec<SectionPos>, Vec<SectionPos>) {
    let center = holder.get_pos();
    assert!(
        holder.try_chunk(ChunkStatus::Light).is_some(),
        "Chunk not found at status Light"
    );

    let Ok(range) = LightSectionRange::from_world_height(holder.min_y(), holder.height()) else {
        panic!("invalid world height for loaded light stage");
    };

    let layout = LightCacheLayout::new(center, range);
    let Ok(workset) = LightWorkset::setup(
        layout,
        LightCacheSetupRadius::Full,
        true,
        |pos| {
            let holder = cache.try_get(pos.0.x, pos.0.y)?;
            holder
                .try_chunk(ChunkStatus::Light)
                .is_some()
                .then(|| Arc::clone(holder))
        },
        |_| true,
    ) else {
        panic!("required loaded light-stage chunk is missing");
    };

    let mut sky_updates = if has_skylight {
        match force_load_sky_light_chunk(&workset) {
            Ok(result) => result.updated_sections,
            Err(error) => panic!("loaded sky light force-load failed: {error:?}"),
        }
    } else {
        Vec::new()
    };
    let mut block_updates = match force_load_block_light_chunk(&workset) {
        Ok(result) => result.updated_sections,
        Err(error) => panic!("loaded block light force-load failed: {error:?}"),
    };

    if has_skylight {
        match check_sky_light_chunk_edges(&workset) {
            Ok(result) => sky_updates.extend(result.updated_sections),
            Err(error) => panic!("loaded sky light edge validation failed: {error:?}"),
        }
    }
    match check_block_light_chunk_edges(&workset) {
        Ok(result) => block_updates.extend(result.updated_sections),
        Err(error) => panic!("loaded block light edge validation failed: {error:?}"),
    }

    (sky_updates, block_updates)
}

fn publish_light_updates(
    context: &WorldGenContext,
    layer: LightLayer,
    updated_sections: Vec<SectionPos>,
) {
    if updated_sections.is_empty() {
        return;
    }

    let world = context.world();
    for section_pos in updated_sections {
        world.chunk_map.light_changed(layer, section_pos);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Weak};

    use steel_registry::{test_support::init_test_registry, vanilla_blocks};
    use steel_utils::{BlockPos, ChunkPos};

    use super::*;
    use crate::behavior::init_behaviors;
    use crate::chunk::{
        chunk_access::{ChunkAccess, ChunkStatus},
        chunk_ticket_manager::ChunkTicketLevel,
        light::{LightSection, LightSectionData},
        proto_chunk::ProtoChunk,
        section::{ChunkSection, Sections},
    };

    fn init_tests() {
        init_test_registry();
        init_behaviors();
    }

    fn holder_with_section(
        pos: ChunkPos,
        status: ChunkStatus,
        section: ChunkSection,
    ) -> Arc<ChunkHolder> {
        let sections = Sections::from_owned(vec![section].into_boxed_slice());
        let proto = ProtoChunk::new(sections, pos, 0, 16, Weak::new());
        proto.initialize_light_sources();
        let holder = Arc::new(ChunkHolder::new(
            pos,
            ChunkTicketLevel::FULL_CHUNK,
            Some(ChunkTicketLevel::FULL_CHUNK),
            0,
            16,
        ));
        holder.insert_chunk(ChunkAccess::Proto(proto), status);
        holder
    }

    fn empty_holder(pos: ChunkPos, status: ChunkStatus) -> Arc<ChunkHolder> {
        holder_with_section(pos, status, ChunkSection::new_empty())
    }

    fn cache_with_center(
        center_pos: ChunkPos,
        center_holder: &Arc<ChunkHolder>,
        neighbor_status: ChunkStatus,
    ) -> StaticCache2D<Arc<ChunkHolder>> {
        let center_holder = Arc::clone(center_holder);
        StaticCache2D::create(center_pos.0.x, center_pos.0.y, 2, move |x, z| {
            let pos = ChunkPos::new(x, z);
            if pos == center_pos {
                return Arc::clone(&center_holder);
            }

            empty_holder(pos, neighbor_status)
        })
    }

    fn light_value(holder: &ChunkHolder, layer: LightLayer, pos: BlockPos) -> u8 {
        let Some(chunk) = holder.try_chunk(ChunkStatus::Empty) else {
            panic!("test chunk should be available");
        };
        chunk.light().get_light_value(layer, pos)
    }

    fn set_visible_light(
        holder: &ChunkHolder,
        layer: LightLayer,
        section_y: i32,
        x: usize,
        y: usize,
        z: usize,
        level: u8,
    ) {
        let Some(chunk) = holder.try_chunk(ChunkStatus::Empty) else {
            panic!("test chunk should be available");
        };
        let mut light = chunk.light_mut();
        let storage = match layer {
            LightLayer::Sky => &mut light.sky,
            LightLayer::Block => &mut light.block,
        };
        let Some(target) = storage.section_mut(section_y) else {
            panic!("test section should be inside light range");
        };
        let mut data = LightSectionData::homogeneous(0);
        data.set(x, y, z, level);
        *target = LightSection::visible(data);
    }

    #[test]
    fn light_stage_generates_center_sky_and_block_light() {
        init_tests();
        let center_pos = ChunkPos::new(0, 0);
        let source_pos = BlockPos::new(1, 1, 1);
        let mut section = ChunkSection::new_empty();
        section.set_block_state(1, 1, 1, vanilla_blocks::LIGHT.default_state());
        let center_holder = holder_with_section(center_pos, ChunkStatus::InitializeLight, section);
        let cache = cache_with_center(center_pos, &center_holder, ChunkStatus::Light);

        let (sky_updates, block_updates) = run_light_stage(&cache, &center_holder, true);

        assert!(sky_updates.contains(&SectionPos::new(0, 0, 0)));
        assert!(block_updates.contains(&SectionPos::new(0, 0, 0)));
        assert_eq!(light_value(&center_holder, LightLayer::Sky, source_pos), 15);
        assert_eq!(
            light_value(&center_holder, LightLayer::Block, source_pos),
            15
        );
        assert_eq!(
            light_value(&center_holder, LightLayer::Block, BlockPos::new(2, 1, 1)),
            14
        );
    }

    #[test]
    fn light_stage_skips_sky_when_dimension_has_no_skylight() {
        init_tests();
        let center_pos = ChunkPos::new(0, 0);
        let source_pos = BlockPos::new(1, 1, 1);
        let mut section = ChunkSection::new_empty();
        section.set_block_state(1, 1, 1, vanilla_blocks::LIGHT.default_state());
        let center_holder = holder_with_section(center_pos, ChunkStatus::InitializeLight, section);
        let cache = cache_with_center(center_pos, &center_holder, ChunkStatus::Light);

        let (sky_updates, block_updates) = run_light_stage(&cache, &center_holder, false);

        assert!(sky_updates.is_empty());
        assert!(block_updates.contains(&SectionPos::new(0, 0, 0)));
        assert_eq!(
            light_value(&center_holder, LightLayer::Block, source_pos),
            15
        );
    }

    #[test]
    fn loaded_light_stage_preserves_persisted_interior_block_light() {
        init_tests();
        let center_pos = ChunkPos::new(0, 0);
        let block_pos = BlockPos::new(8, 1, 8);
        let mut section = ChunkSection::new_empty();
        section.set_block_state(0, 0, 0, vanilla_blocks::STONE.default_state());
        let center_holder = holder_with_section(center_pos, ChunkStatus::Light, section);
        set_visible_light(&center_holder, LightLayer::Block, 0, 8, 1, 8, 7);
        let cache = cache_with_center(center_pos, &center_holder, ChunkStatus::Light);

        let (sky_updates, _block_updates) = run_loaded_light_stage(&cache, &center_holder, false);

        assert!(sky_updates.is_empty());
        assert_eq!(light_value(&center_holder, LightLayer::Block, block_pos), 7);
    }

    #[test]
    fn loaded_light_stage_validates_persisted_sky_light() {
        init_tests();
        let center_pos = ChunkPos::new(0, 0);
        let sky_pos = BlockPos::new(8, 15, 8);
        let mut section = ChunkSection::new_empty();
        section.set_block_state(0, 0, 0, vanilla_blocks::STONE.default_state());
        let center_holder = holder_with_section(center_pos, ChunkStatus::Light, section);
        set_visible_light(&center_holder, LightLayer::Sky, 0, 8, 15, 8, 15);
        let cache = cache_with_center(center_pos, &center_holder, ChunkStatus::Light);

        let _ = run_loaded_light_stage(&cache, &center_holder, true);

        assert_eq!(light_value(&center_holder, LightLayer::Sky, sky_pos), 15);
    }
}
