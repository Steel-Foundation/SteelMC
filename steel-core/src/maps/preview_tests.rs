use std::{
    future::Future,
    pin::pin,
    sync::Arc,
    task::{Context, Poll, Waker},
};

use futures::FutureExt;
use steel_registry::{RegistryEntry, vanilla_biomes};
use steel_utils::{BlockPos, ChunkPos, locks::SyncMutex};
use steel_worldgen::biomes::obfuscate_biome_seed;

use super::{MAP_SIZE, MapItemSavedData, chunk_io::MapChunk};
use crate::{
    bootstrap::init_globals,
    chunk::{
        Chunk,
        chunk_request::ChunkTicketKind,
        chunk_ticket_manager::ticket_level_for_status,
        paletted_container::BiomePalette,
        section::{ChunkSection, Sections},
        status::ChunkStatus,
    },
    test_support::{fresh_test_world, insert_ready_full_chunk},
    world::{LevelReader, World},
    worldgen::generator::vanilla::fuzzed_biome_quart_position,
};

fn preview(world: &World, scale: i8) -> Arc<SyncMutex<MapItemSavedData>> {
    Arc::new(SyncMutex::new(MapItemSavedData::create_fresh(
        0.0,
        0.0,
        scale,
        true,
        true,
        world.key.clone(),
    )))
}

#[test]
fn biome_previews_sample_missing_chunks_without_loading_them_at_every_map_scale() {
    init_globals();
    let fixture = fresh_test_world("map_preview_no_loads");
    let world = &fixture.world;
    assert_eq!(world.chunk_map.chunks.len(), 0);
    for scale in 0..=4 {
        let data = preview(world, scale);
        MapItemSavedData::render_biome_preview(&data, world)
            .now_or_never()
            .expect("missing chunks use generator samples immediately")
            .expect("biome preview");
        assert!(data.lock().colors().iter().all(|color| *color == 0));
        assert_eq!(world.chunk_map.chunks.len(), 0);
    }
}

#[test]
fn biome_previews_preserve_available_biomes_fuzzed_boundaries_and_loading_permissions() {
    init_globals();
    let fixture = fresh_test_world("map_preview_available_biomes");
    let world = &fixture.world;
    let holder = insert_ready_full_chunk(world, ChunkPos::new(0, 0));
    let ocean = u16::try_from(vanilla_biomes::OCEAN.try_id().expect("ocean biome"))
        .expect("biome palette id");
    for section in &holder
        .try_chunk(ChunkStatus::Biomes)
        .expect("biomes")
        .sections()
        .sections
    {
        section.write().biomes = BiomePalette::Homogeneous(ocean);
    }
    let level = ticket_level_for_status(ChunkStatus::Biomes);
    holder.swap_load_level(level);
    holder.update_highest_allowed_status(Some(level));
    let data = preview(world, 0);
    let seed = obfuscate_biome_seed(world.seed());
    let mut watery = [false; MAP_SIZE * MAP_SIZE];
    for z in 0..MAP_SIZE {
        for x in 0..MAP_SIZE {
            let sample = BlockPos::new(x as i32 - 64, world.sea_level(), z as i32 - 64);
            let quart = fuzzed_biome_quart_position(seed, sample);
            watery[z * MAP_SIZE + x] = quart.x >> 2 == 0 && quart.z >> 2 == 0;
        }
    }
    let mut expected = MapItemSavedData::create_fresh(0.0, 0.0, 0, true, true, world.key.clone());
    expected.draw_biome_preview(&watery);
    assert!(expected.colors().iter().any(|color| *color != 0));
    MapItemSavedData::render_biome_preview(&data, world)
        .now_or_never()
        .expect("available biomes need no I/O")
        .expect("biome preview");
    assert_eq!(data.lock().colors(), expected.colors());
    assert_eq!(world.chunk_map.chunks.len(), 1);

    let level = ticket_level_for_status(ChunkStatus::StructureStarts);
    holder.swap_load_level(level);
    holder.update_highest_allowed_status(Some(level));
    let data = preview(world, 0);
    MapItemSavedData::render_biome_preview(&data, world)
        .now_or_never()
        .expect("disallowed biomes use the generator")
        .expect("biome preview");
    assert!(data.lock().colors().iter().all(|color| *color == 0));
    assert_eq!(world.chunk_map.chunks.len(), 1);
}

#[test]
fn existing_biome_work_resumes_on_readiness_or_unload_without_expanding_the_loading_area() {
    init_globals();
    for (key, publish) in [("map_preview_pending", true), ("map_preview_unload", false)] {
        let fixture = fresh_test_world(key);
        let world = &fixture.world;
        world.chunk_map.stop_generation_refill_loop();
        let pos = ChunkPos::new(0, 0);
        let request = world
            .chunk_map
            .request_chunk(pos, ChunkStatus::Biomes, ChunkTicketKind::Map);
        world.chunk_map.advance_scheduling();
        let holder = world
            .chunk_map
            .chunks
            .read_sync(&pos, |_, holder| Arc::clone(holder))
            .expect("existing permitted holder");
        let count = world.chunk_map.chunks.len();
        let mut future = pin!(MapChunk::load_if_present(world, pos, ChunkStatus::Biomes));
        let mut task = Context::from_waker(Waker::noop());
        assert!(future.as_mut().poll(&mut task).is_pending());
        if publish {
            let sections = (0..world.get_height() / 16)
                .map(|_| ChunkSection::new_empty())
                .collect::<Vec<_>>()
                .into_boxed_slice();
            let chunk = Chunk::new(
                Sections::from_owned(sections),
                pos,
                world.get_min_y(),
                world.get_height(),
                Arc::downgrade(world),
            );
            holder.insert_chunk(chunk, ChunkStatus::Biomes);
            assert!(matches!(
                future.as_mut().poll(&mut task),
                Poll::Ready(Some(_))
            ));
        } else {
            holder.clear_load_level();
            holder.update_highest_allowed_status(None);
            holder.wake_all_watchers();
            assert!(matches!(future.as_mut().poll(&mut task), Poll::Ready(None)));
        }
        assert_eq!(world.chunk_map.chunks.len(), count);
        world.chunk_map.stop_generation_refill_loop();
        drop(request);
    }
}
