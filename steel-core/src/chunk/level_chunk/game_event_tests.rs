use std::{
    slice,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering as AtomicOrdering},
    },
};

use glam::DVec3;
use simdnbt::{borrow::BaseNbtCompound as BorrowedNbtCompound, owned::NbtCompound};
use steel_registry::{
    game_events::GameEventRef, test_support::init_test_registry, vanilla_block_entity_types,
    vanilla_blocks, vanilla_game_events,
};
use steel_utils::{
    BlockPos, BlockStateId, ChunkPos, DowncastType, DowncastTypeKey, locks::SyncMutex,
    types::UpdateFlags,
};

use super::*;
use crate::behavior::init_behaviors;
use crate::block_entity::{BlockEntity, BlockEntityBase, SharedBlockEntity};
use crate::chunk::{
    chunk_access::{ChunkAccess, ChunkStatus},
    chunk_holder::ChunkHolder,
    chunk_ticket_manager::ChunkTicketLevel,
    proto_chunk::ProtoChunk,
    section::{ChunkSection, Sections},
};
use crate::test_support::{fresh_test_world, insert_ready_full_chunk};
use crate::world::World;
use crate::world::game_event_context::GameEventContext;
use crate::world::game_event_listener::{GameEventListener, SharedGameEventListener};

struct RecordingGameEventListener {
    pos: DVec3,
    id: u8,
    events: Arc<SyncMutex<Vec<u8>>>,
}

impl GameEventListener for RecordingGameEventListener {
    fn listener_pos(&self) -> Option<DVec3> {
        Some(self.pos)
    }

    fn listener_radius(&self) -> i32 {
        16
    }

    fn handle_game_event(
        &self,
        _world: &Arc<World>,
        _event: GameEventRef,
        _context: &GameEventContext<'_>,
        _source_pos: DVec3,
    ) -> bool {
        self.events.lock().push(self.id);
        true
    }
}

struct ListenerBlockEntity {
    base: BlockEntityBase,
    listener: SharedGameEventListener,
    selections: Arc<AtomicUsize>,
}

// SAFETY: This test-only key uniquely identifies this concrete test implementation.
unsafe impl DowncastType for ListenerBlockEntity {
    const TYPE_KEY: DowncastTypeKey =
        DowncastTypeKey::new("steel:test/block_entity/game_event_listener");
}

impl BlockEntity for ListenerBlockEntity {
    fn base(&self) -> &BlockEntityBase {
        &self.base
    }

    fn game_event_listener(&self) -> Option<SharedGameEventListener> {
        self.selections.fetch_add(1, AtomicOrdering::Relaxed);
        Some(Arc::clone(&self.listener))
    }

    fn load_additional(&self, _nbt: &BorrowedNbtCompound<'_>) {}

    fn save_additional(&self, _nbt: &mut NbtCompound) {}
}

fn listener_block_entity(
    world: &Arc<World>,
    pos: BlockPos,
    state: BlockStateId,
    id: u8,
    events: &Arc<SyncMutex<Vec<u8>>>,
    selections: &Arc<AtomicUsize>,
) -> SharedBlockEntity {
    let listener: SharedGameEventListener = Arc::new(RecordingGameEventListener {
        pos: DVec3::new(
            f64::from(pos.x()) + 0.5,
            f64::from(pos.y()) + 0.5,
            f64::from(pos.z()) + 0.5,
        ),
        id,
        events: Arc::clone(events),
    });
    Arc::new(ListenerBlockEntity {
        base: BlockEntityBase::new(
            &vanilla_block_entity_types::CHEST,
            Arc::downgrade(world),
            pos,
            state,
        ),
        listener,
        selections: Arc::clone(selections),
    })
}

#[test]
fn active_block_entity_listener_uses_stored_selection_for_removal() {
    init_test_registry();
    init_behaviors();
    let world = fresh_test_world("active_block_entity_listener");
    let pos = BlockPos::new(1, 64, 1);
    let holder = insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));
    let state = vanilla_blocks::CHEST.default_state();
    let events = Arc::new(SyncMutex::new(Vec::new()));
    let selections = Arc::new(AtomicUsize::new(0));
    let block_entity = listener_block_entity(&world, pos, state, 1, &events, &selections);

    {
        let Some(chunk) = holder.try_chunk(ChunkStatus::Full) else {
            panic!("test holder should remain full");
        };
        let Some(chunk) = chunk.as_full() else {
            panic!("test holder should contain a level chunk");
        };
        assert!(
            chunk
                .set_block_state(pos, state, UpdateFlags::UPDATE_NONE)
                .is_some()
        );
        assert!(chunk.add_and_register_block_entity(Arc::clone(&block_entity)));
    }

    assert_eq!(selections.load(AtomicOrdering::Relaxed), 1);
    world.game_event(
        &vanilla_game_events::BLOCK_CHANGE,
        pos,
        &GameEventContext::default(),
    );
    assert_eq!(*events.lock(), [1]);

    {
        let Some(chunk) = holder.try_chunk(ChunkStatus::Full) else {
            panic!("test holder should remain full");
        };
        let Some(chunk) = chunk.as_full() else {
            panic!("test holder should contain a level chunk");
        };
        assert!(chunk.remove_block_entity(pos));
    }
    world.game_event(
        &vanilla_game_events::BLOCK_CHANGE,
        pos,
        &GameEventContext::default(),
    );

    assert_eq!(*events.lock(), [1]);
    assert_eq!(selections.load(AtomicOrdering::Relaxed), 1);
}

#[test]
fn full_activation_registers_listener_without_block_ticking_readiness() {
    init_test_registry();
    init_behaviors();
    let world = fresh_test_world("full_block_entity_listener_activation");
    let min_y = world.get_min_y();
    let height = world.get_height();
    let pos = BlockPos::new(1, 64, 1);
    let chunk_pos = ChunkPos::from_block_pos(pos);
    let sections = (0..height / 16)
        .map(|_| ChunkSection::new_empty())
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let proto = ProtoChunk::new(
        Sections::from_owned(sections),
        chunk_pos,
        min_y,
        height,
        Arc::downgrade(&world),
    );
    let chunk = LevelChunk::from_proto(proto, min_y, height, Arc::downgrade(&world)).chunk;
    let state = vanilla_blocks::CHEST.default_state();
    let events = Arc::new(SyncMutex::new(Vec::new()));
    let selections = Arc::new(AtomicUsize::new(0));
    assert!(
        chunk
            .set_block_state(pos, state, UpdateFlags::UPDATE_NONE)
            .is_some()
    );
    assert!(chunk.add_and_register_block_entity(listener_block_entity(
        &world,
        pos,
        state,
        1,
        &events,
        &selections,
    )));
    assert_eq!(selections.load(AtomicOrdering::Relaxed), 0);

    let holder = Arc::new(ChunkHolder::new(
        chunk_pos,
        ChunkTicketLevel::FULL_CHUNK,
        None,
        min_y,
        height,
    ));
    holder.insert_chunk(ChunkAccess::Full(chunk), ChunkStatus::Full);
    let _ = world
        .chunk_map
        .chunks
        .insert_sync(chunk_pos, Arc::clone(&holder));
    world
        .chunk_map
        .activate_block_entities(slice::from_ref(&holder));

    assert_eq!(selections.load(AtomicOrdering::Relaxed), 1);
    world.game_event(
        &vanilla_game_events::BLOCK_CHANGE,
        pos,
        &GameEventContext::default(),
    );
    assert_eq!(*events.lock(), [1]);
    assert!(holder.simulation_level().is_none());
}

#[test]
fn full_demotion_hides_listener_without_reordering_on_revival() {
    init_test_registry();
    init_behaviors();
    let world = fresh_test_world("retained_block_entity_listeners");
    let first_pos = BlockPos::new(1, 64, 1);
    let second_pos = BlockPos::new(2, 64, 1);
    let chunk_pos = ChunkPos::from_block_pos(first_pos);
    let holder = insert_ready_full_chunk(&world, chunk_pos);
    let state = vanilla_blocks::CHEST.default_state();
    let events = Arc::new(SyncMutex::new(Vec::new()));
    let first_selections = Arc::new(AtomicUsize::new(0));
    let second_selections = Arc::new(AtomicUsize::new(0));

    {
        let Some(chunk) = holder.try_chunk(ChunkStatus::Full) else {
            panic!("test holder should remain full");
        };
        let Some(chunk) = chunk.as_full() else {
            panic!("test holder should contain a level chunk");
        };
        for (pos, id, selections) in [
            (first_pos, 1, &first_selections),
            (second_pos, 2, &second_selections),
        ] {
            assert!(
                chunk
                    .set_block_state(pos, state, UpdateFlags::UPDATE_NONE)
                    .is_some()
            );
            assert!(chunk.add_and_register_block_entity(listener_block_entity(
                &world, pos, state, id, &events, selections,
            )));
        }
    }

    world.game_event(
        &vanilla_game_events::BLOCK_CHANGE,
        first_pos,
        &GameEventContext::default(),
    );
    assert_eq!(*events.lock(), [1, 2]);

    let _ = holder.swap_load_level(ChunkTicketLevel::MAX);
    holder.update_highest_allowed_status(Some(ChunkTicketLevel::MAX));
    assert!(!world.has_full_chunk(chunk_pos));
    world.game_event(
        &vanilla_game_events::BLOCK_CHANGE,
        first_pos,
        &GameEventContext::default(),
    );
    assert_eq!(*events.lock(), [1, 2]);

    let _ = holder.swap_load_level(ChunkTicketLevel::FULL_CHUNK);
    holder.update_highest_allowed_status(Some(ChunkTicketLevel::FULL_CHUNK));
    assert!(world.has_full_chunk(chunk_pos));
    world.game_event(
        &vanilla_game_events::BLOCK_CHANGE,
        first_pos,
        &GameEventContext::default(),
    );

    assert_eq!(*events.lock(), [1, 2, 1, 2]);

    {
        let Some(chunk) = holder.try_chunk(ChunkStatus::Full) else {
            panic!("test holder should remain full");
        };
        let Some(chunk) = chunk.as_full() else {
            panic!("test holder should contain a level chunk");
        };
        chunk.suspend_block_entities(&holder);
    }
    let Some((_, removed)) = world.chunk_map.chunks.remove_sync(&chunk_pos) else {
        panic!("test holder should be active before suspension");
    };
    assert!(Arc::ptr_eq(&removed, &holder));
    world.game_event(
        &vanilla_game_events::BLOCK_CHANGE,
        first_pos,
        &GameEventContext::default(),
    );
    assert_eq!(*events.lock(), [1, 2, 1, 2]);

    let _ = world
        .chunk_map
        .chunks
        .insert_sync(chunk_pos, Arc::clone(&holder));
    world
        .chunk_map
        .activate_block_entities(slice::from_ref(&holder));
    world.game_event(
        &vanilla_game_events::BLOCK_CHANGE,
        first_pos,
        &GameEventContext::default(),
    );

    assert_eq!(*events.lock(), [1, 2, 1, 2, 1, 2]);
    assert_eq!(first_selections.load(AtomicOrdering::Relaxed), 1);
    assert_eq!(second_selections.load(AtomicOrdering::Relaxed), 1);
}

#[test]
fn retained_block_entity_state_does_not_reselect_listener() {
    init_test_registry();
    init_behaviors();
    let world = fresh_test_world("retained_block_entity_listener_state");
    let pos = BlockPos::new(1, 64, 1);
    let holder = insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));
    let copper = vanilla_blocks::COPPER_CHEST.default_state();
    let exposed = vanilla_blocks::EXPOSED_COPPER_CHEST.default_state();
    let events = Arc::new(SyncMutex::new(Vec::new()));
    let selections = Arc::new(AtomicUsize::new(0));

    {
        let Some(chunk) = holder.try_chunk(ChunkStatus::Full) else {
            panic!("test holder should remain full");
        };
        let Some(chunk) = chunk.as_full() else {
            panic!("test holder should contain a level chunk");
        };
        assert!(
            chunk
                .set_block_state(pos, copper, UpdateFlags::UPDATE_NONE)
                .is_some()
        );
        assert!(chunk.add_and_register_block_entity(listener_block_entity(
            &world,
            pos,
            copper,
            1,
            &events,
            &selections,
        )));
        assert_eq!(
            chunk.set_block_state(pos, exposed, UpdateFlags::UPDATE_NONE),
            Some(copper)
        );
    }

    assert_eq!(selections.load(AtomicOrdering::Relaxed), 1);
    world.game_event(
        &vanilla_game_events::BLOCK_CHANGE,
        pos,
        &GameEventContext::default(),
    );
    assert_eq!(*events.lock(), [1]);
}
