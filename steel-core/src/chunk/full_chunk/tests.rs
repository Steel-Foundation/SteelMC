use std::{
    sync::{Arc, Barrier, Weak},
    thread,
};

use simdnbt::{borrow::BaseNbtCompound as BorrowedNbtCompound, owned::NbtCompound};
use steel_registry::{
    blocks::properties::BlockStateProperties, init_vanilla_registry, vanilla_block_entity_types,
    vanilla_blocks, vanilla_fluids,
};
use steel_utils::{ChunkPos, Downcast as _, DowncastType, DowncastTypeKey, locks::SyncMutex};

use super::*;
use crate::behavior::init_behaviors;
use crate::block_entity::entities::ComparatorBlockEntity;
use crate::block_entity::{BlockEntityBase, SharedBlockEntity, entities::RawBlockEntity};
use crate::chunk::{
    Chunk,
    chunk_ticket_manager::ChunkTicketLevel,
    heightmap::{ChunkHeightmaps, HeightmapType},
    light::{ChunkLightData, LightSection, LightSectionData},
    section::{ChunkSection, Sections},
    status::ChunkStatus,
};
use crate::world::tick_scheduler::{BlockTickList, FluidTickList};
use steel_worldgen::structure::{StructureReferenceMap, StructureStartMap};

fn test_chunk() -> Arc<Chunk> {
    let chunk = Arc::new(Chunk::new(
        Sections::from_owned(vec![ChunkSection::new_empty()].into_boxed_slice()),
        ChunkPos::new(0, 0),
        0,
        16,
        Weak::new(),
    ));
    let _ = chunk.promote_to_full();
    chunk
}

#[test]
fn promotion_installs_full_runtime_once() {
    init_vanilla_registry();
    init_behaviors();
    let proto = Chunk::new(
        Sections::from_owned(vec![ChunkSection::new_empty()].into_boxed_slice()),
        ChunkPos::new(0, 0),
        0,
        16,
        Weak::new(),
    );
    assert!(proto.full_runtime().is_none());

    let full = proto.promote_to_full().chunk;

    assert!(full.common().full_runtime().is_some());
    let replacement = FullChunkRuntime::new(GameEventListenerCount::shared());
    assert!(full.common().initialize_full_runtime(replacement).is_err());
}

#[test]
fn full_disk_construction_returns_initialized_runtime() {
    init_vanilla_registry();
    init_behaviors();
    let full = Chunk::from_full_disk(
        Sections::from_owned(vec![ChunkSection::new_empty()].into_boxed_slice()),
        ChunkPos::new(0, 0),
        0,
        16,
        Weak::new(),
        BlockTickList::new(),
        FluidTickList::new(),
        ChunkHeightmaps::with_types(HeightmapType::final_types(), 0, 16),
        Vec::new(),
        StructureStartMap::default(),
        StructureReferenceMap::default(),
        ChunkLightData::for_valid_world_height(0, 16),
    );

    assert!(full.full_runtime().is_some());
    let _ = FullChunkRef::from_full_context(&full).game_event_listeners();
}

#[test]
fn lava_random_tick_classification_includes_block_and_fluid_hooks() {
    init_vanilla_registry();
    init_behaviors();
    let Some((tick_block, tick_fluid)) = random_tick_kinds(vanilla_blocks::LAVA.default_state())
    else {
        panic!("lava should be eligible for random ticking");
    };

    assert!(tick_block);
    assert_eq!(tick_fluid, Some(&vanilla_fluids::LAVA));
    assert!(random_tick_kinds(vanilla_blocks::WATER.default_state()).is_none());
}

#[test]
fn block_random_positions_match_vanilla_lcg_layout() {
    let mut positions = BlockRandomPositionGenerator::from_seed(0);

    assert_eq!(positions.next_local(), (7, 11, 12));
    assert_eq!(positions.next_local(), (15, 14, 3));
    assert_eq!(positions.next_local(), (4, 8, 6));
    assert_eq!(positions.next_local(), (6, 5, 1));
    assert_eq!(positions.next_local(), (9, 12, 1));
}

struct ActivationRecordingBlockEntity {
    base: BlockEntityBase,
    events: SyncMutex<Vec<&'static str>>,
}

// SAFETY: This test-only key uniquely identifies this concrete test implementation.
unsafe impl DowncastType for ActivationRecordingBlockEntity {
    const TYPE_KEY: DowncastTypeKey =
        DowncastTypeKey::new("steel:test/block_entity/activation_recording");
}

impl BlockEntity for ActivationRecordingBlockEntity {
    fn base(&self) -> &BlockEntityBase {
        &self.base
    }

    fn on_clear_removed(&self) {
        self.events.lock().push("cleared");
    }

    fn load_additional(&self, _nbt: &BorrowedNbtCompound<'_>) {}

    fn save_additional(&self, _nbt: &mut NbtCompound) {}
}

#[test]
fn inactive_chunk_stages_lifecycle_callbacks_until_activation() {
    init_vanilla_registry();
    init_behaviors();
    let proto = Chunk::new(
        Sections::from_owned(vec![ChunkSection::new_empty()].into_boxed_slice()),
        ChunkPos::new(0, 0),
        0,
        16,
        Weak::new(),
    );
    let full = proto.promote_to_full().chunk;
    let pos = BlockPos::new(1, 2, 3);
    let state = vanilla_blocks::OAK_SIGN.default_state();
    assert!(
        full.set_block_state(pos, state, UpdateFlags::UPDATE_NONE)
            .is_some()
    );

    let concrete = Arc::new(ActivationRecordingBlockEntity {
        base: BlockEntityBase::new(&vanilla_block_entity_types::SIGN, Weak::new(), pos, state),
        events: SyncMutex::new(Vec::new()),
    });
    concrete.set_removed();
    let entity: SharedBlockEntity = concrete.clone();
    assert!(full.add_and_register_block_entity(entity));
    assert!(concrete.events.lock().is_empty());

    let holder = Arc::new(ChunkHolder::new(
        ChunkPos::new(0, 0),
        ChunkTicketLevel::FULL_CHUNK,
        None,
        0,
        16,
    ));
    holder.insert_chunk(proto, ChunkStatus::Full);
    let batch = {
        holder
            .try_full_chunk()
            .and_then(|chunk| chunk.prepare_block_entity_activation(&holder))
            .expect("first activation should produce a batch")
    };
    assert!(concrete.events.lock().is_empty());
    for block_entity in batch.lifecycle_dispatchers {
        block_entity.dispatch_lifecycle_events();
    }
    assert_eq!(*concrete.events.lock(), ["cleared"]);
}

#[test]
fn extract_light_data_uses_chunk_owned_light_and_skylight_flag() {
    init_vanilla_registry();
    init_behaviors();
    let proto = Chunk::new(
        Sections::from_owned(vec![ChunkSection::new_empty()].into_boxed_slice()),
        ChunkPos::new(0, 0),
        0,
        16,
        Weak::new(),
    );
    let chunk = proto.promote_to_full().chunk;

    {
        let mut light = chunk.common().light.write();
        let Some(sky_section) = light.sky.section_mut(0) else {
            panic!("single-section light range should contain section 0");
        };
        *sky_section = LightSection::visible(LightSectionData::homogeneous(15));

        let Some(block_section) = light.block.section_mut(0) else {
            panic!("single-section light range should contain section 0");
        };
        let mut block_data = LightSectionData::homogeneous(0);
        block_data.set(1, 2, 3, 12);
        *block_section = LightSection::visible(block_data);
    }

    let with_sky = chunk.extract_light_data(true);
    assert_eq!(with_sky.sky_y_mask.0[0] & 0b10, 0b10);
    assert_eq!(with_sky.block_y_mask.0[0] & 0b10, 0b10);
    assert_eq!(with_sky.sky_updates.len(), 1);
    assert_eq!(with_sky.block_updates.len(), 1);
    assert!(with_sky.sky_updates[0].iter().all(|byte| *byte == 0xff));

    let without_sky = chunk.extract_light_data(false);
    assert_eq!(without_sky.sky_y_mask.0[0], 0);
    assert_eq!(without_sky.sky_updates.len(), 0);
    assert_eq!(without_sky.block_y_mask.0[0] & 0b10, 0b10);
    assert_eq!(without_sky.block_updates.len(), 1);
}

#[test]
fn empty_and_out_of_range_sections_return_air() {
    init_vanilla_registry();
    init_behaviors();
    let proto = Chunk::new(
        Sections::from_owned(vec![ChunkSection::new_empty()].into_boxed_slice()),
        ChunkPos::new(0, 0),
        0,
        16,
        Weak::new(),
    );
    let chunk = proto.promote_to_full().chunk;

    assert_eq!(
        chunk.get_block_state(BlockPos::new(0, 0, 0)),
        vanilla_blocks::AIR.default_state()
    );
    assert_eq!(
        chunk.get_block_state(BlockPos::new(0, 16, 0)),
        vanilla_blocks::AIR.default_state()
    );
}

#[test]
fn draining_postprocessing_marks_full_chunk_dirty() {
    init_vanilla_registry();
    init_behaviors();
    let proto = Chunk::new(
        Sections::from_owned(vec![ChunkSection::new_empty()].into_boxed_slice()),
        ChunkPos::new(0, 0),
        0,
        16,
        Weak::new(),
    );
    proto.mark_pos_for_postprocessing(BlockPos::new(1, 2, 3));
    let chunk = proto.promote_to_full().chunk;
    chunk.common().dirty.store(false, Ordering::Release);

    assert!(chunk.take_postprocessing().is_some());
    assert!(chunk.common().dirty.load(Ordering::Acquire));
    assert_eq!(chunk.postprocessing_for_serialization()[0].len(), 0);
}

#[test]
fn conditional_block_set_rejects_a_stale_state() {
    init_vanilla_registry();
    init_behaviors();
    let chunk_owner = test_chunk();
    let chunk = FullChunkRef::from_full_context(&chunk_owner);
    let pos = BlockPos::new(0, 0, 0);
    let stone = vanilla_blocks::STONE.default_state();
    let dirt = vanilla_blocks::DIRT.default_state();
    assert_eq!(
        chunk.set_block_state(pos, stone, UpdateFlags::UPDATE_NONE),
        Some(vanilla_blocks::AIR.default_state())
    );

    assert_eq!(
        chunk.set_block_state_if_unchanged(
            pos,
            vanilla_blocks::AIR.default_state(),
            dirt,
            UpdateFlags::UPDATE_NONE,
        ),
        Some(FullChunkBlockSetResult::Stale(stone))
    );
    assert_eq!(chunk.get_block_state(pos), stone);
    assert_eq!(
        chunk.set_block_state_if_unchanged(pos, stone, stone, UpdateFlags::UPDATE_NONE),
        Some(FullChunkBlockSetResult::Unchanged)
    );
}

#[test]
fn concurrent_consumers_cannot_both_claim_the_same_block_state() {
    init_vanilla_registry();
    init_behaviors();
    let chunk_owner = test_chunk();
    let chunk = FullChunkRef::from_full_context(&chunk_owner);
    let pos = BlockPos::new(0, 0, 0);
    let stone = vanilla_blocks::STONE.default_state();
    assert!(
        chunk
            .set_block_state(pos, stone, UpdateFlags::UPDATE_NONE)
            .is_some()
    );

    let barrier = Arc::new(Barrier::new(3));
    let first = {
        let chunk_owner = Arc::clone(&chunk_owner);
        let barrier = Arc::clone(&barrier);
        thread::spawn(move || {
            barrier.wait();
            let chunk = FullChunkRef::from_full_context(&chunk_owner);
            chunk.set_block_state_if_unchanged(
                pos,
                stone,
                vanilla_blocks::DIRT.default_state(),
                UpdateFlags::UPDATE_NONE,
            )
        })
    };
    let second = {
        let chunk_owner = Arc::clone(&chunk_owner);
        let barrier = Arc::clone(&barrier);
        thread::spawn(move || {
            barrier.wait();
            let chunk = FullChunkRef::from_full_context(&chunk_owner);
            chunk.set_block_state_if_unchanged(
                pos,
                stone,
                vanilla_blocks::COBBLESTONE.default_state(),
                UpdateFlags::UPDATE_NONE,
            )
        })
    };
    barrier.wait();

    let Ok(first) = first.join() else {
        panic!("first conditional block-set worker should finish");
    };
    let Ok(second) = second.join() else {
        panic!("second conditional block-set worker should finish");
    };
    let changed = [first, second]
        .into_iter()
        .filter(|result| matches!(result, Some(FullChunkBlockSetResult::Changed(_))))
        .count();

    assert_eq!(changed, 1);
    assert_ne!(chunk.get_block_state(pos), stone);
}

#[test]
fn block_change_replaces_a_structurally_valid_raw_entity_with_the_new_factory() {
    init_vanilla_registry();
    init_behaviors();
    let chunk_owner = test_chunk();
    let chunk = FullChunkRef::from_full_context(&chunk_owner);
    let pos = BlockPos::new(1, 2, 3);
    let chest = vanilla_blocks::CHEST.default_state();
    let comparator = vanilla_blocks::COMPARATOR.default_state();
    assert!(
        chunk
            .set_block_state(pos, chest, UpdateFlags::UPDATE_NONE)
            .is_some()
    );
    let old: SharedBlockEntity = Arc::new(RawBlockEntity::new(
        &vanilla_block_entity_types::CHEST,
        Weak::new(),
        pos,
        chest,
    ));
    assert!(chunk.add_and_register_block_entity(Arc::clone(&old)));

    assert_eq!(
        chunk.set_block_state(pos, comparator, UpdateFlags::UPDATE_NONE),
        Some(chest)
    );
    let Some(replacement) = chunk.get_block_entity(pos) else {
        panic!("comparator behavior should create its concrete block entity");
    };
    assert!(old.is_removed());
    assert!(
        replacement
            .downcast_ref::<ComparatorBlockEntity>()
            .is_some()
    );
}

#[test]
fn breaking_an_unimplemented_entity_block_removes_its_raw_entity() {
    init_vanilla_registry();
    init_behaviors();
    let chunk_owner = test_chunk();
    let chunk = FullChunkRef::from_full_context(&chunk_owner);
    let pos = BlockPos::new(2, 2, 2);
    let chest = vanilla_blocks::CHEST.default_state();
    assert!(
        chunk
            .set_block_state(pos, chest, UpdateFlags::UPDATE_NONE)
            .is_some()
    );
    let old: SharedBlockEntity = Arc::new(RawBlockEntity::new(
        &vanilla_block_entity_types::CHEST,
        Weak::new(),
        pos,
        chest,
    ));
    assert!(chunk.add_and_register_block_entity(Arc::clone(&old)));

    assert_eq!(
        chunk.set_block_state(
            pos,
            vanilla_blocks::STONE.default_state(),
            UpdateFlags::UPDATE_NONE,
        ),
        Some(chest)
    );
    assert!(old.is_removed());
    assert!(chunk.get_block_entity(pos).is_none());
}

#[test]
fn copper_chest_transformation_preserves_entity_identity() {
    init_vanilla_registry();
    init_behaviors();
    let chunk_owner = test_chunk();
    let chunk = FullChunkRef::from_full_context(&chunk_owner);
    let pos = BlockPos::new(3, 2, 1);
    let copper = vanilla_blocks::COPPER_CHEST.default_state();
    let exposed = vanilla_blocks::EXPOSED_COPPER_CHEST.default_state();
    assert!(
        chunk
            .set_block_state(pos, copper, UpdateFlags::UPDATE_NONE)
            .is_some()
    );
    let mut data = NbtCompound::new();
    data.insert("test_marker", 37_i32);
    let original: SharedBlockEntity = Arc::new(RawBlockEntity::with_data(
        &vanilla_block_entity_types::CHEST,
        Weak::new(),
        pos,
        copper,
        data,
    ));
    assert!(chunk.add_and_register_block_entity(Arc::clone(&original)));

    assert_eq!(
        chunk.set_block_state(pos, exposed, UpdateFlags::UPDATE_NONE),
        Some(copper)
    );
    let Some(transformed) = chunk.get_block_entity(pos) else {
        panic!("copper chest transformation should retain its entity");
    };
    assert!(Arc::ptr_eq(&original, &transformed));
    assert_eq!(transformed.get_block_state(), exposed);
    assert!(!transformed.is_removed());
    let mut saved = NbtCompound::new();
    transformed.save_additional(&mut saved);
    assert_eq!(saved.int("test_marker"), Some(37));
}

#[test]
fn same_block_property_change_preserves_entity_data_and_updates_cached_state() {
    init_vanilla_registry();
    init_behaviors();
    let chunk_owner = test_chunk();
    let chunk = FullChunkRef::from_full_context(&chunk_owner);
    let pos = BlockPos::new(3, 2, 2);
    let comparator = vanilla_blocks::COMPARATOR.default_state();
    assert!(
        chunk
            .set_block_state(pos, comparator, UpdateFlags::UPDATE_NONE)
            .is_some()
    );
    let Some(original) = chunk.get_block_entity(pos) else {
        panic!("comparator placement should create its entity");
    };
    let Some(original_comparator) = original.downcast_ref::<ComparatorBlockEntity>() else {
        panic!("comparator should use its concrete entity");
    };
    original_comparator.set_output_signal(11);
    let powered = comparator.set_value(&BlockStateProperties::POWERED, true);

    assert_eq!(
        chunk.set_block_state(pos, powered, UpdateFlags::UPDATE_NONE),
        Some(comparator)
    );
    let Some(updated) = chunk.get_block_entity(pos) else {
        panic!("property update should retain the comparator entity");
    };
    assert!(Arc::ptr_eq(&original, &updated));
    assert_eq!(updated.get_block_state(), powered);
    assert_eq!(original_comparator.output_signal(), 11);
}

#[test]
fn shared_entity_type_does_not_imply_cross_block_preservation() {
    init_vanilla_registry();
    init_behaviors();
    let chunk_owner = test_chunk();
    let chunk = FullChunkRef::from_full_context(&chunk_owner);
    let pos = BlockPos::new(4, 2, 1);
    let chest = vanilla_blocks::CHEST.default_state();
    let trapped_chest = vanilla_blocks::TRAPPED_CHEST.default_state();
    assert!(
        chunk
            .set_block_state(pos, chest, UpdateFlags::UPDATE_NONE)
            .is_some()
    );
    let original: SharedBlockEntity = Arc::new(RawBlockEntity::new(
        &vanilla_block_entity_types::CHEST,
        Weak::new(),
        pos,
        chest,
    ));
    assert!(chunk.add_and_register_block_entity(Arc::clone(&original)));

    assert_eq!(
        chunk.set_block_state(pos, trapped_chest, UpdateFlags::UPDATE_NONE),
        Some(chest)
    );
    assert!(original.is_removed());
    assert!(
        !chunk
            .get_block_entity(pos)
            .is_some_and(|entity| Arc::ptr_eq(&original, &entity))
    );
}

#[test]
fn insertion_rejects_an_entity_owned_by_another_chunk() {
    init_vanilla_registry();
    init_behaviors();
    let chunk_owner = test_chunk();
    let chunk = FullChunkRef::from_full_context(&chunk_owner);
    let local_pos = BlockPos::new(0, 2, 0);
    let foreign_pos = BlockPos::new(16, 2, 0);
    let chest = vanilla_blocks::CHEST.default_state();
    assert!(
        chunk
            .set_block_state(local_pos, chest, UpdateFlags::UPDATE_NONE)
            .is_some()
    );
    let foreign: SharedBlockEntity = Arc::new(RawBlockEntity::new(
        &vanilla_block_entity_types::CHEST,
        Weak::new(),
        foreign_pos,
        chest,
    ));

    assert!(!chunk.add_and_register_block_entity(foreign));
    assert!(chunk.get_block_entity(local_pos).is_none());
    assert!(chunk.get_block_entity(foreign_pos).is_none());
}

#[test]
fn insertion_below_world_does_not_alias_the_bottom_section() {
    init_vanilla_registry();
    init_behaviors();
    let chunk_owner = test_chunk();
    let chunk = FullChunkRef::from_full_context(&chunk_owner);
    let bottom_section_pos = BlockPos::new(0, 15, 0);
    let below_world_pos = BlockPos::new(0, -1, 0);
    let chest = vanilla_blocks::CHEST.default_state();
    assert!(
        chunk
            .set_block_state(bottom_section_pos, chest, UpdateFlags::UPDATE_NONE)
            .is_some()
    );
    let below_world: SharedBlockEntity = Arc::new(RawBlockEntity::new(
        &vanilla_block_entity_types::CHEST,
        Weak::new(),
        below_world_pos,
        chest,
    ));

    assert!(!chunk.add_and_register_block_entity(below_world));
    assert!(chunk.get_block_entity(below_world_pos).is_none());
}

#[test]
fn stale_no_entity_promotion_cannot_consume_a_replacement_marker() {
    init_vanilla_registry();
    init_behaviors();
    let chunk_owner = test_chunk();
    let chunk = FullChunkRef::from_full_context(&chunk_owner);
    let pos = BlockPos::new(2, 3, 4);
    let moving_piston = vanilla_blocks::MOVING_PISTON.default_state();
    assert!(
        chunk
            .set_block_state(pos, moving_piston, UpdateFlags::UPDATE_NONE)
            .is_some()
    );
    chunk.set_pending_block_entity(pos);

    let chest = vanilla_blocks::CHEST.default_state();
    assert_eq!(
        chunk.set_block_state(pos, chest, UpdateFlags::UPDATE_NONE),
        Some(moving_piston)
    );
    assert_eq!(chunk.pending_block_entity_positions(), [pos]);

    assert!(matches!(
        chunk.commit_pending_creation(pos, moving_piston, BlockEntityCreation::NoEntity),
        PendingPromotionCommit::Retry
    ));
    assert_eq!(chunk.pending_block_entity_positions(), [pos]);
}

#[test]
fn immediate_lookup_recovers_a_missing_implemented_entity() {
    init_vanilla_registry();
    init_behaviors();
    let chunk_owner = test_chunk();
    let chunk = FullChunkRef::from_full_context(&chunk_owner);
    let pos = BlockPos::new(2, 3, 4);
    let sign = vanilla_blocks::OAK_SIGN.default_state();
    assert!(
        chunk
            .set_block_state(pos, sign, UpdateFlags::UPDATE_NONE)
            .is_some()
    );
    assert!(chunk.remove_block_entity(pos));
    assert!(chunk.get_block_entity(pos).is_none());

    let Some(recovered) = chunk.get_block_entity_immediate(pos) else {
        panic!("immediate lookup should recreate the sign entity");
    };
    assert_eq!(recovered.get_block_state(), sign);
    assert!(!recovered.is_removed());
}
