use super::*;
use crate::behavior::init_behaviors;
use crate::block_entity::{
    SharedBlockEntity,
    entities::{ComparatorBlockEntity, SignBlockEntity},
};
use crate::chunk::Chunk;
use crate::chunk::full_chunk::FullChunkRef;
use crate::chunk::heightmap::ChunkHeightmaps;
use crate::chunk::light::ChunkLightData;
use crate::chunk::section::{ChunkSection, Sections};
use crate::chunk_saver::RamOnlyStorage;
use crate::player::connection::NetworkConnection;
use crate::player::{PlayerConnection, ResetReason};
use crate::test_support::{TestPlayerBuilder, fresh_test_world, insert_ready_full_chunk};
use crate::world::tick_scheduler::{BlockTickList, FluidTickList, SavedTick, TickPriority};
use crate::worldgen::EmptyChunkGenerator;
use std::io::Cursor;
use std::thread;
use steel_protocol::packet_traits::CompressionInfo;
use steel_registry::{
    init_vanilla_registry,
    packets::play::{C_BLOCK_CHANGED_ACK, C_BLOCK_UPDATE},
    vanilla_blocks,
    vanilla_dimension_types::OVERWORLD,
    vanilla_fluids,
};
use steel_utils::codec::VarInt;
use steel_utils::serial::ReadFrom;
use steel_utils::types::UpdateFlags;
use steel_worldgen::structure::{StructureReferenceMap, StructureStartMap};
use text_components::TextComponent;
use uuid::Uuid;

struct RecordingConnection {
    packets: Arc<SyncMutex<Vec<EncodedPacket>>>,
}

impl NetworkConnection for RecordingConnection {
    fn compression(&self) -> Option<CompressionInfo> {
        None
    }

    fn send_encoded(&self, packet: EncodedPacket) {
        self.packets.lock().push(packet);
    }

    fn send_encoded_bundle(&self, packets: Vec<EncodedPacket>) {
        self.packets.lock().extend(packets);
    }

    fn disconnect_with_reason(&self, _reason: TextComponent) {}

    fn tick(&self) {}

    fn latency(&self) -> i32 {
        0
    }

    fn close(&self) {}

    fn closed(&self) -> bool {
        false
    }
}

fn recording_player(world: &Arc<World>) -> (Arc<Player>, Arc<SyncMutex<Vec<EncodedPacket>>>) {
    let packets = Arc::new(SyncMutex::new(Vec::new()));
    let connection = Arc::new(PlayerConnection::Other(Box::new(RecordingConnection {
        packets: Arc::clone(&packets),
    })));
    let player = TestPlayerBuilder::new(Arc::clone(world), Uuid::from_u128(1), "TestPlayer", 1)
        .connection(connection)
        .build();
    (player, packets)
}

fn packet_id(packet: &EncodedPacket) -> i32 {
    let mut cursor = Cursor::new(packet.encoded_data.as_slice());
    assert!(
        VarInt::read(&mut cursor).is_ok(),
        "packet length should decode"
    );
    match VarInt::read(&mut cursor) {
        Ok(packet_id) => packet_id.0,
        Err(error) => panic!("packet id should decode: {error}"),
    }
}

fn advance_until_revision(chunk_map: &Arc<ChunkMap>, revision: ChunkTicketRevision) {
    for _ in 0..10_000 {
        chunk_map.advance_scheduling();
        if chunk_map.is_ticket_revision_committed(revision) {
            return;
        }
        thread::sleep(Duration::from_millis(1));
    }
    panic!("chunk ticket revision did not commit");
}

fn add_test_comparator(full: FullChunkRef<'_>, pos: BlockPos) -> SharedBlockEntity {
    let Ok(relative_y) = usize::try_from(pos.y() - full.min_y()) else {
        panic!("test comparator position must be inside the chunk height");
    };
    let state = vanilla_blocks::COMPARATOR.default_state();
    full.common().sections.set_relative_block(
        (pos.x() & 15) as usize,
        relative_y,
        (pos.z() & 15) as usize,
        state,
    );
    let block_entity: SharedBlockEntity =
        Arc::new(ComparatorBlockEntity::new(full.level_weak(), pos, state));
    assert!(full.add_and_register_block_entity(Arc::clone(&block_entity)));
    block_entity
}

fn add_test_sign(full: FullChunkRef<'_>, pos: BlockPos) -> SharedBlockEntity {
    let Ok(relative_y) = usize::try_from(pos.y() - full.min_y()) else {
        panic!("test sign position must be inside the chunk height");
    };
    let state = vanilla_blocks::OAK_SIGN.default_state();
    full.common().sections.set_relative_block(
        (pos.x() & 15) as usize,
        relative_y,
        (pos.z() & 15) as usize,
        state,
    );
    let block_entity: SharedBlockEntity =
        Arc::new(SignBlockEntity::new(full.level_weak(), pos, state));
    assert!(full.add_and_register_block_entity(Arc::clone(&block_entity)));
    block_entity
}

fn insert_active_full_holder(
    world: &Arc<World>,
    pos: ChunkPos,
    load_level: ChunkTicketLevel,
    postprocessing: Vec<Vec<u16>>,
) -> Arc<ChunkHolder> {
    insert_active_full_holder_with_ticks(
        world,
        pos,
        load_level,
        postprocessing,
        BlockTickList::new(),
    )
}

fn insert_active_full_holder_with_ticks(
    world: &Arc<World>,
    pos: ChunkPos,
    load_level: ChunkTicketLevel,
    postprocessing: Vec<Vec<u16>>,
    block_ticks: BlockTickList,
) -> Arc<ChunkHolder> {
    let min_y = world.chunk_map.world_gen_context.min_y();
    let height = world.chunk_map.world_gen_context.height();
    let sections = (0..height / 16)
        .map(|_| ChunkSection::new_empty())
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let chunk = Chunk::from_full_disk(
        Sections::from_owned(sections),
        pos,
        min_y,
        height,
        Arc::downgrade(world),
        block_ticks,
        FluidTickList::new(),
        ChunkHeightmaps::new(min_y, height),
        postprocessing,
        StructureStartMap::default(),
        StructureReferenceMap::default(),
        ChunkLightData::for_valid_world_height(min_y, height),
    );
    let simulation_level = load_level.is_entity_ticking().then_some(load_level);
    let holder = Arc::new(ChunkHolder::new_with_full_publications(
        pos,
        load_level,
        simulation_level,
        min_y,
        height,
        Arc::downgrade(&world.chunk_map.full_publications),
    ));
    holder.insert_chunk(chunk, ChunkStatus::Full);
    let _ = world.chunk_map.chunks.insert_sync(pos, Arc::clone(&holder));
    holder
}

fn assert_postprocessing_drained(holder: &ChunkHolder) {
    let chunk = holder
        .try_full_chunk()
        .expect("the center should remain Full");
    assert!(
        chunk
            .postprocessing_for_serialization()
            .iter()
            .all(Vec::is_empty),
        "loaded Full postprocessing should run at the r1 transition"
    );
}

fn test_chunk_map() -> Arc<ChunkMap> {
    init_vanilla_registry();
    init_behaviors();
    Arc::new(ChunkMap::new_with_storage(
        Arc::new(Runtime::new().expect("test runtime should initialize")),
        Weak::new(),
        &OVERWORLD,
        63,
        Arc::new(ChunkStorage::RamOnly(RamOnlyStorage::empty_world())),
        Arc::new(ChunkGeneratorType::Empty(EmptyChunkGenerator::new())),
        Arc::new(
            rayon::ThreadPoolBuilder::new()
                .num_threads(1)
                .build()
                .expect("test generation pool should initialize"),
        ),
    ))
}

fn unloaded_light_holder(pos: ChunkPos) -> Arc<ChunkHolder> {
    let proto = Chunk::from_disk(
        Sections::from_owned(vec![ChunkSection::new_empty()].into_boxed_slice()),
        pos,
        ChunkStatus::Light,
        0,
        16,
        ChunkHeightmaps::new(0, 16),
        StructureStartMap::default(),
        StructureReferenceMap::default(),
        None,
        Vec::new(),
        BlockTickList::new(),
        FluidTickList::new(),
        Weak::new(),
        ChunkLightData::for_valid_world_height(0, 16),
    );
    let holder = Arc::new(ChunkHolder::new(
        pos,
        ChunkTicketLevel::FULL_CHUNK,
        Some(ChunkTicketLevel::FULL_CHUNK),
        0,
        16,
    ));
    holder.insert_chunk(proto, ChunkStatus::Light);
    holder
}

fn unloaded_full_holder(pos: ChunkPos) -> Arc<ChunkHolder> {
    let chunk = Chunk::from_full_disk(
        Sections::from_owned(vec![ChunkSection::new_empty()].into_boxed_slice()),
        pos,
        0,
        16,
        Weak::new(),
        BlockTickList::new(),
        FluidTickList::new(),
        ChunkHeightmaps::new(0, 16),
        Vec::new(),
        StructureStartMap::default(),
        StructureReferenceMap::default(),
        ChunkLightData::for_valid_world_height(0, 16),
    );
    let holder = Arc::new(ChunkHolder::new(
        pos,
        ChunkTicketLevel::FULL_CHUNK,
        Some(ChunkTicketLevel::FULL_CHUNK),
        0,
        16,
    ));
    holder.insert_chunk(chunk, ChunkStatus::Full);
    holder
}

mod light_updates;
mod persistence_unloads;
mod player_tracking;
mod scheduled_ticks;
mod tickets_generation_readiness;
