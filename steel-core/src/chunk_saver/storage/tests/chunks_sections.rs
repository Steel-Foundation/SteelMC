use super::*;

#[test]
#[should_panic(expected = "persisted chunk status must match its Full runtime state")]
fn chunk_save_rejects_full_status_for_proto_data() {
    init_vanilla_registry();

    let chunk = Chunk::new(
        single_empty_section(),
        ChunkPos::new(0, 0),
        0,
        16,
        Weak::new(),
    );
    let _ = ChunkStorage::prepare_chunk_save(&chunk, ChunkStatus::Full, &[], true);
}

#[test]
fn unknown_referenced_block_state_is_corruption_instead_of_air_recovery() {
    init_vanilla_registry();

    let pos = ChunkPos::new(0, 0);
    let chunk = Chunk::new(single_empty_section(), pos, 0, 16, Weak::new());
    let Some(mut prepared) =
        ChunkStorage::prepare_chunk_save(&chunk, ChunkStatus::Empty, &[], true)
    else {
        panic!("forced chunk save should produce a payload");
    };
    let Some(block_state) = prepared.persistent.block_states.first_mut() else {
        panic!("an empty section should still persist its air state");
    };
    block_state.name = Identifier::new_static("steel_test", "missing_block");

    let Err(error) = ChunkStorage::try_persistent_to_chunk(
        &prepared.persistent,
        pos,
        ChunkStatus::Empty,
        0,
        16,
        Weak::new(),
    ) else {
        panic!("unknown referenced block state must reject the complete payload");
    };
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
}

#[test]
fn unknown_referenced_biome_is_corruption_instead_of_plains_recovery() {
    init_vanilla_registry();

    let pos = ChunkPos::new(0, 0);
    let chunk = Chunk::new(single_empty_section(), pos, 0, 16, Weak::new());
    let Some(mut prepared) =
        ChunkStorage::prepare_chunk_save(&chunk, ChunkStatus::Empty, &[], true)
    else {
        panic!("forced chunk save should produce a payload");
    };
    let Some(biome) = prepared.persistent.biomes.first_mut() else {
        panic!("an empty section should still persist its biome");
    };
    *biome = Identifier::new_static("steel_test", "missing_biome");

    let Err(error) = ChunkStorage::try_persistent_to_chunk(
        &prepared.persistent,
        pos,
        ChunkStatus::Empty,
        0,
        16,
        Weak::new(),
    ) else {
        panic!("unknown referenced biome must reject the complete payload");
    };
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
}

#[test]
fn proto_heightmap_save_preserves_existing_maps_and_load_primes_missing_maps() {
    init_vanilla_registry();

    let pos = ChunkPos::new(3, -4);
    let proto = Chunk::new(single_empty_section(), pos, 0, 16, Weak::new());
    proto.heightmaps.write().prime_from_sections(
        &[HeightmapType::WorldSurfaceWg],
        0,
        16,
        &proto.sections.sections,
    );
    let chunk = proto;

    let Some(prepared) = ChunkStorage::prepare_chunk_save(&chunk, ChunkStatus::Biomes, &[], false)
    else {
        panic!("dirty proto chunk should prepare for saving");
    };
    assert_eq!(prepared.persistent.heightmaps.len(), 1);
    assert_eq!(
        prepared.persistent.heightmaps[0].heightmap_type,
        HeightmapType::WorldSurfaceWg.persistence_id()
    );

    let loaded = ChunkStorage::persistent_to_chunk(
        &prepared.persistent,
        pos,
        ChunkStatus::Biomes,
        0,
        16,
        Weak::new(),
    );
    let loaded = loaded.chunk;
    let heightmaps = loaded.heightmaps.read();
    assert!(heightmaps.get(HeightmapType::WorldSurfaceWg).is_some());
    assert!(heightmaps.get(HeightmapType::OceanFloorWg).is_some());
}

#[test]
fn terrain_heightmap_save_excludes_stale_worldgen_maps() {
    init_vanilla_registry();

    let proto = Chunk::new(
        single_empty_section(),
        ChunkPos::new(3, -4),
        0,
        16,
        Weak::new(),
    );
    {
        let mut heightmaps = proto.heightmaps.write();
        heightmaps.replace(Heightmap::new(HeightmapType::WorldSurfaceWg, 0, 16));
        heightmaps.replace(Heightmap::new(HeightmapType::WorldSurface, 0, 16));
    }
    let chunk = proto;

    let Some(prepared) = ChunkStorage::prepare_chunk_save(&chunk, ChunkStatus::Terrain, &[], false)
    else {
        panic!("dirty proto chunk should prepare for saving");
    };
    assert_eq!(prepared.persistent.heightmaps.len(), 1);
    assert_eq!(
        prepared.persistent.heightmaps[0].heightmap_type,
        HeightmapType::WorldSurface.persistence_id()
    );
}

#[tokio::test]
async fn ram_only_storage_restores_the_status_bundled_with_the_prepared_save() {
    init_vanilla_registry();

    let pos = ChunkPos::new(3, -4);
    let proto = Chunk::new(single_empty_section(), pos, 0, 16, Weak::new());
    let chunk = proto;
    let Some(prepared) = ChunkStorage::prepare_chunk_save(&chunk, ChunkStatus::Terrain, &[], false)
    else {
        panic!("dirty proto chunk should prepare for saving");
    };

    let storage = RamOnlyStorage::empty_world();
    let Ok(true) = storage.save_chunk_data(prepared).await else {
        panic!("prepared chunk should save to RAM storage");
    };
    let Ok(Some(loaded)) = storage.load_chunk(pos, 0, 16, Weak::new()).await else {
        panic!("saved chunk should load from RAM storage");
    };

    assert_eq!(loaded.status, ChunkStatus::Terrain);
}

#[test]
fn proto_postprocessing_roundtrips_through_persistent_chunk() {
    init_vanilla_registry();

    let pos = ChunkPos::new(-2, 1);
    let marked = BlockPos::new(-17, -63, 31);
    let proto = Chunk::new(single_empty_section(), pos, -64, 16, Weak::new());
    proto.mark_pos_for_postprocessing(marked);
    let packed = Chunk::pack_postprocessing_offset(marked);
    let chunk = proto;

    let Some(prepared) = ChunkStorage::prepare_chunk_save(&chunk, ChunkStatus::Terrain, &[], false)
    else {
        panic!("dirty proto chunk should prepare for saving");
    };

    assert_eq!(prepared.persistent.postprocessing, vec![vec![packed]]);

    let loaded = ChunkStorage::persistent_to_chunk(
        &prepared.persistent,
        pos,
        ChunkStatus::Terrain,
        -64,
        16,
        Weak::new(),
    );
    let loaded_proto = loaded.chunk;

    assert_eq!(loaded_proto.postprocessing.lock()[0], vec![packed]);
}

#[test]
fn full_chunk_postprocessing_roundtrips_through_persistent_chunk() {
    init_globals_once();

    let pos = ChunkPos::new(-2, 1);
    let marked = BlockPos::new(-17, -63, 31);
    let packed = Chunk::pack_postprocessing_offset(marked);
    let persistent = ChunkStorage::to_persistent(
        &single_empty_section(),
        &[],
        &[],
        &[],
        Vec::new(),
        Vec::new(),
        Vec::new(),
        PersistentLightData::default(),
        vec![vec![packed]],
        Vec::new(),
        Vec::new(),
        Vec::new(),
        pos,
    );

    let loaded = ChunkStorage::persistent_to_chunk(
        &persistent,
        pos,
        ChunkStatus::Full,
        -64,
        16,
        Weak::new(),
    );
    let chunk = loaded.chunk;
    let loaded_full = FullChunkRef::from_full_context(&chunk);
    assert_eq!(
        loaded_full.postprocessing_for_serialization(),
        vec![vec![packed]]
    );

    chunk.mark_dirty();
    let Some(prepared) = ChunkStorage::prepare_chunk_save(&chunk, ChunkStatus::Full, &[], false)
    else {
        panic!("dirty full chunk should prepare for saving");
    };

    assert_eq!(prepared.persistent.postprocessing, vec![vec![packed]]);
}
