use super::*;

#[test]
fn persisted_light_is_ignored_before_light_status() {
    let persistent = PersistentLightData {
        block: vec![PersistentLightSection::Initialized {
            section_index: 1,
            data: vec![0xFF; DATA_LAYER_SIZE],
        }],
        sky: vec![PersistentLightSection::Initialized {
            section_index: 1,
            data: vec![0xFF; DATA_LAYER_SIZE],
        }],
    };

    let light = ChunkStorage::persistent_to_light(&persistent, 0, 16, ChunkStatus::InitializeLight);

    assert!(matches!(
        light.block.section(0),
        Some(LightSection::Missing)
    ));
    assert!(matches!(light.sky.section(0), Some(LightSection::Missing)));
}

#[test]
fn loaded_sky_light_fills_missing_sections_below_loaded_data_with_zero() {
    let persistent = PersistentLightData {
        block: Vec::new(),
        sky: vec![PersistentLightSection::Initialized {
            section_index: 2,
            data: vec![0xFF; DATA_LAYER_SIZE],
        }],
    };

    let light = ChunkStorage::persistent_to_light(&persistent, 0, 16, ChunkStatus::Light);

    assert_eq!(visible_homogeneous_value(light.sky.section(1)), Some(15));
    assert_eq!(visible_homogeneous_value(light.sky.section(0)), Some(0));
    assert_eq!(visible_homogeneous_value(light.sky.section(-1)), Some(0));
}

#[test]
fn chunk_light_persistence_canonicalizes_visible_and_internal_sections() {
    let mut light = ChunkLightData::for_valid_world_height(0, 16);
    let mut block_data = LightSectionData::homogeneous(0);
    block_data.set(1, 2, 3, 12);
    *light.block.section_mut(0).expect("real section in range") = LightSection::visible(block_data);
    *light.sky.section_mut(-1).expect("bottom section in range") =
        LightSection::internal(LightSectionData::homogeneous(7));
    *light.sky.section_mut(0).expect("real section in range") =
        LightSection::visible(LightSectionData::homogeneous(0));
    *light.sky.section_mut(1).expect("top section in range") =
        LightSection::internal(LightSectionData::homogeneous(0));

    let persistent = ChunkStorage::light_to_persistent(&light);

    assert_eq!(persistent.block.len(), 1);
    match &persistent.block[0] {
        PersistentLightSection::Initialized {
            section_index,
            data,
        } => {
            assert_eq!(*section_index, 1);
            assert_eq!(data.len(), DATA_LAYER_SIZE);
            assert_eq!(data[280], 0xC0);
        }
        _ => panic!("block light should persist initialized data"),
    }

    assert_eq!(persistent.sky.len(), 2);
    assert!(matches!(
        persistent.sky[0],
        PersistentLightSection::Internal {
            section_index: 0,
            ..
        }
    ));
    assert!(matches!(
        persistent.sky[1],
        PersistentLightSection::Uninitialized { section_index: 1 }
    ));
}

#[test]
fn persistent_chunk_loads_chunk_owned_light_into_full_chunk() {
    init_globals_once();
    let mut light = ChunkLightData::for_valid_world_height(0, 16);
    *light.block.section_mut(0).expect("real section in range") =
        LightSection::visible(LightSectionData::homogeneous(12));
    let persistent_light = ChunkStorage::light_to_persistent(&light);
    let persistent = ChunkStorage::to_persistent(
        &single_empty_section(),
        &[],
        &[],
        &[],
        Vec::new(),
        Vec::new(),
        Vec::new(),
        persistent_light,
        None,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        ChunkPos::new(0, 0),
    );

    let loaded = ChunkStorage::persistent_to_chunk(
        &persistent,
        ChunkPos::new(0, 0),
        ChunkStatus::Full,
        0,
        16,
        Weak::new(),
    );

    let chunk = loaded.chunk;
    let light = chunk.light.read();
    assert_eq!(visible_homogeneous_value(light.block.section(0)), Some(12));
    assert_eq!(light.block.section_empty(0), Some(true));
}
