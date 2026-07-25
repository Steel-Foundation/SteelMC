use super::*;

#[test]
fn proto_carving_mask_presence_roundtrips_when_empty() {
    init_test_registry();

    let pos = ChunkPos::new(3, -4);
    let proto = ProtoChunk::new(single_empty_section(), pos, 0, 16, Weak::new());
    proto.set_status(ChunkStatus::Carvers);
    drop(proto.get_or_create_carving_mask());
    let chunk = ChunkAccess::Proto(proto);

    let Some(prepared) = ChunkStorage::prepare_chunk_save(&chunk, &[], false) else {
        panic!("dirty proto chunk should prepare for saving");
    };
    assert_eq!(prepared.persistent.carving_mask, Some(Vec::new()));

    let loaded = ChunkStorage::persistent_to_chunk(
        &prepared.persistent,
        pos,
        ChunkStatus::Carvers,
        0,
        16,
        Weak::new(),
    );
    let ChunkAccess::Proto(loaded_proto) = loaded.chunk else {
        panic!("carvers status should load as proto chunk");
    };

    assert!(loaded_proto.carving_mask.read().is_some());
}

#[test]
fn proto_carving_mask_bits_roundtrip_through_persistent_chunk() {
    init_test_registry();

    let pos = ChunkPos::new(3, -4);
    let proto = ProtoChunk::new(single_empty_section(), pos, 0, 16, Weak::new());
    proto.set_status(ChunkStatus::Carvers);
    {
        let mut mask = proto.get_or_create_carving_mask();
        mask.set(7, 5, 11);
    }
    let chunk = ChunkAccess::Proto(proto);

    let Some(prepared) = ChunkStorage::prepare_chunk_save(&chunk, &[], false) else {
        panic!("dirty proto chunk should prepare for saving");
    };
    assert!(
        prepared
            .persistent
            .carving_mask
            .as_ref()
            .is_some_and(|packed| !packed.is_empty())
    );

    let loaded = ChunkStorage::persistent_to_chunk(
        &prepared.persistent,
        pos,
        ChunkStatus::Carvers,
        0,
        16,
        Weak::new(),
    );
    let ChunkAccess::Proto(loaded_proto) = loaded.chunk else {
        panic!("carvers status should load as proto chunk");
    };

    let mask_guard = loaded_proto.carving_mask.read();
    let Some(mask) = mask_guard.as_ref() else {
        panic!("carving mask should restore from persistent chunk");
    };
    assert!(mask.get(7, 5, 11));
    assert!(!mask.get(8, 5, 11));
}

#[test]
fn proto_postprocessing_roundtrips_through_persistent_chunk() {
    init_test_registry();

    let pos = ChunkPos::new(-2, 1);
    let marked = BlockPos::new(-17, -63, 31);
    let proto = ProtoChunk::new(single_empty_section(), pos, -64, 16, Weak::new());
    proto.set_status(ChunkStatus::Noise);
    proto.mark_pos_for_postprocessing(marked);
    let packed = ProtoChunk::pack_postprocessing_offset(marked);
    let chunk = ChunkAccess::Proto(proto);

    let Some(prepared) = ChunkStorage::prepare_chunk_save(&chunk, &[], false) else {
        panic!("dirty proto chunk should prepare for saving");
    };

    assert_eq!(prepared.persistent.postprocessing, vec![vec![packed]]);

    let loaded = ChunkStorage::persistent_to_chunk(
        &prepared.persistent,
        pos,
        ChunkStatus::Noise,
        -64,
        16,
        Weak::new(),
    );
    let ChunkAccess::Proto(loaded_proto) = loaded.chunk else {
        panic!("noise status should load as proto chunk");
    };

    assert_eq!(loaded_proto.postprocessing.read()[0], vec![packed]);
}

#[test]
fn full_chunk_postprocessing_roundtrips_through_persistent_chunk() {
    init_test_registry();
    init_runtime_registries();

    let pos = ChunkPos::new(-2, 1);
    let marked = BlockPos::new(-17, -63, 31);
    let packed = ProtoChunk::pack_postprocessing_offset(marked);
    let persistent = ChunkStorage::to_persistent(
        &single_empty_section(),
        &[],
        &[],
        &[],
        Vec::new(),
        Vec::new(),
        Vec::new(),
        PersistentLightData::default(),
        None,
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
    let ChunkAccess::Full(loaded_full) = loaded.chunk else {
        panic!("full status should load as a full chunk");
    };
    assert_eq!(
        loaded_full.postprocessing_for_serialization(),
        vec![vec![packed]]
    );

    let chunk = ChunkAccess::Full(loaded_full);
    chunk.mark_dirty();
    let Some(prepared) = ChunkStorage::prepare_chunk_save(&chunk, &[], false) else {
        panic!("dirty full chunk should prepare for saving");
    };

    assert_eq!(prepared.persistent.postprocessing, vec![vec![packed]]);
}
