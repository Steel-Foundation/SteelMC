use std::io::Cursor;
use std::sync::{Arc, Weak};

use glam::DVec3;
use simdnbt::borrow::{NbtCompound as NbtCompoundView, read_compound};
use simdnbt::owned::NbtCompound;
use steel_protocol::packet_traits::{CompressionInfo, EncodedPacket};
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::data_components::components::{BlockEntityData, CustomData, JukeboxPlayable};
use steel_registry::data_components::vanilla_components::{BLOCK_ENTITY_DATA, JUKEBOX_PLAYABLE};
use steel_registry::game_events::GameEventRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::level_events::{SOUND_PLAY_JUKEBOX_SONG, SOUND_STOP_JUKEBOX_SONG};
use steel_registry::packets::play::{C_LEVEL_EVENT, C_LEVEL_PARTICLES};
use steel_registry::{
    RegistryEntry as _, vanilla_block_entity_types, vanilla_blocks, vanilla_entities,
    vanilla_game_events, vanilla_items, vanilla_jukebox_songs,
};
use steel_utils::codec::VarInt;
use steel_utils::locks::SyncMutex;
use steel_utils::serial::ReadFrom as _;
use steel_utils::types::{GameType, InteractionHand, UpdateFlags};
use steel_utils::{BlockPos, ChunkPos, Direction, Downcast as _, SectionPos, WorldAabb};
use text_components::TextComponent;

use super::JukeboxBlock;
use crate::behavior::blocks::{MAX_REDSTONE_SIGNAL, MIN_REDSTONE_SIGNAL};
use crate::behavior::{
    BlockBehavior as _, BlockHitResult, BlockItem, BlockPlaceContext, InteractionResult,
    InventoryAccess, PlacementOrientation, PlacementSource,
};
use crate::block_entity::entities::JukeboxBlockEntity;
use crate::block_entity::{BlockEntity, SharedBlockEntity};
use crate::bootstrap::init_globals_once;
use crate::chunk::chunk_holder::ChunkHolder;
use crate::entity::entities::ItemEntity;
use crate::entity::{Entity as _, SharedEntity, next_entity_id};
use crate::player::connection::NetworkConnection;
use crate::player::{Player, PlayerConnection, ResetReason};
use crate::test_support::{TestPlayerBuilder, fresh_test_world, insert_ready_full_chunk};
use crate::world::game_event::{GameEventContext, GameEventListener, SharedGameEventListener};
use crate::world::{SignalGetter as _, World};

const BLOCK_CENTER_OFFSET: f64 = 0.5;
const JUKEBOX_EJECTION_Y_OFFSET: f64 = 1.01;
const JUKEBOX_EJECTION_HORIZONTAL_RADIUS: f64 = 0.35;
const ITEM_EJECTION_HORIZONTAL_SPEED_LIMIT: f64 = 0.1;
const ITEM_EJECTION_UPWARD_SPEED: f64 = 0.2;
const DEFAULT_ITEM_PICKUP_DELAY_TICKS: i32 = 10;
const TICKS_PER_SECOND: f32 = 20.0;
const PLAY_EVENT_INTERVAL_TICKS: i64 = 20;
const SONG_END_PADDING_TICKS: i64 = 20;
const SAVED_PLAYBACK_TICKS: i32 = 37;
const GAME_EVENT_LISTENER_RADIUS: i32 = 16;
const DROPPED_ITEM_SEARCH_SIZE: f64 = 3.0;

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

struct RecordingGameEventListener {
    pos: DVec3,
    events: Arc<SyncMutex<Vec<GameEventRef>>>,
}

impl GameEventListener for RecordingGameEventListener {
    fn listener_pos(&self) -> Option<DVec3> {
        Some(self.pos)
    }

    fn listener_radius(&self) -> i32 {
        GAME_EVENT_LISTENER_RADIUS
    }

    fn handle_game_event(
        &self,
        _world: &Arc<World>,
        event: GameEventRef,
        _context: &GameEventContext<'_>,
        _source_pos: DVec3,
    ) -> bool {
        self.events.lock().push(event);
        true
    }
}

fn jukebox_world(key: &'static str) -> (Arc<World>, Arc<ChunkHolder>, BlockPos, JukeboxBlock) {
    init_globals_once();
    let world = fresh_test_world(key);
    let pos = BlockPos::new(8, 64, 8);
    let holder = insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));
    assert!(world.set_block(
        pos,
        vanilla_blocks::JUKEBOX.default_state(),
        UpdateFlags::UPDATE_NONE,
    ));
    assert!(world.get_block_entity(pos).is_some());
    (
        world,
        holder,
        pos,
        JukeboxBlock::new(&vanilla_blocks::JUKEBOX),
    )
}

fn jukebox_entity(world: &World, pos: BlockPos) -> SharedBlockEntity {
    let Some(block_entity) = world.get_block_entity(pos) else {
        panic!("jukebox should have a block entity");
    };
    assert!(block_entity.downcast_ref::<JukeboxBlockEntity>().is_some());
    block_entity
}

fn test_player(world: &Arc<World>, id: i32) -> Arc<Player> {
    TestPlayerBuilder::new(Arc::clone(world), format!("JukeboxTester{id}"), id).build()
}

fn block_bottom_center(pos: BlockPos) -> DVec3 {
    DVec3::new(
        f64::from(pos.x()) + BLOCK_CENTER_OFFSET,
        f64::from(pos.y()),
        f64::from(pos.z()) + BLOCK_CENTER_OFFSET,
    )
}

fn block_center(pos: BlockPos) -> DVec3 {
    block_bottom_center(pos) + DVec3::Y * BLOCK_CENTER_OFFSET
}

fn hit_result(pos: BlockPos) -> BlockHitResult {
    BlockHitResult {
        location: block_center(pos),
        direction: Direction::Up,
        block_pos: pos,
        miss: false,
        inside: false,
        world_border_hit: false,
    }
}

fn stored_record(block_entity: &dyn BlockEntity) -> Option<ItemStack> {
    let nbt = block_entity.save_custom_only();
    let mut bytes = Vec::new();
    nbt.write(&mut bytes);
    let mut cursor = Cursor::new(bytes.as_slice());
    let borrowed = read_compound(&mut cursor).ok()?;
    let view: NbtCompoundView<'_, '_> = (&borrowed).into();
    let record = view.compound("RecordItem")?;
    ItemStack::from_borrowed_compound(&record)
}

fn saved_ticks(block_entity: &dyn BlockEntity) -> Option<i64> {
    let nbt = block_entity.save_custom_only();
    let mut bytes = Vec::new();
    nbt.write(&mut bytes);
    let mut cursor = Cursor::new(bytes.as_slice());
    let borrowed = read_compound(&mut cursor).ok()?;
    let view: NbtCompoundView<'_, '_> = (&borrowed).into();
    view.long("ticks_since_song_started")
}

fn song_comparator(stack: &ItemStack) -> i32 {
    let Some(playable) = stack.get(JUKEBOX_PLAYABLE) else {
        panic!("test record should carry a jukebox song");
    };
    playable.song().value().comparator_output
}

fn assert_within_radius(value: f64, center: f64, radius: f64) {
    assert!(
        (center - radius..center + radius).contains(&value),
        "{value} should be within {radius} of {center}"
    );
}

fn dropped_items(world: &World, pos: BlockPos) -> Vec<SharedEntity> {
    let search_center = block_bottom_center(pos) + DVec3::Y * (DROPPED_ITEM_SEARCH_SIZE / 2.0);
    world.get_entities_in_aabb_matching(
        &WorldAabb::of_size(
            search_center,
            DROPPED_ITEM_SEARCH_SIZE,
            DROPPED_ITEM_SEARCH_SIZE,
            DROPPED_ITEM_SEARCH_SIZE,
        ),
        |entity| entity.entity_type() == &vanilla_entities::ITEM,
    )
}

fn recording_player(
    world: &Arc<World>,
    pos: BlockPos,
) -> (Arc<Player>, Arc<SyncMutex<Vec<EncodedPacket>>>) {
    let packets = Arc::new(SyncMutex::new(Vec::new()));
    let connection = Arc::new(PlayerConnection::Other(Box::new(RecordingConnection {
        packets: Arc::clone(&packets),
    })));
    let player = TestPlayerBuilder::new(Arc::clone(world), "JukeboxObserver", next_entity_id())
        .connection(connection)
        .build();
    let moved = player.try_set_position(block_bottom_center(pos.above()));
    assert!(moved.is_ok(), "test player should move beside jukebox");
    assert!(world.add_player(Arc::clone(&player), ResetReason::InitialJoin));
    packets.lock().clear();
    (player, packets)
}

fn packet_id(packet: &EncodedPacket) -> Option<i32> {
    let mut cursor = Cursor::new(packet.encoded_data.as_slice());
    VarInt::read(&mut cursor).ok()?;
    Some(VarInt::read(&mut cursor).ok()?.0)
}

fn level_events(packets: &SyncMutex<Vec<EncodedPacket>>) -> Vec<(i32, BlockPos, i32, bool)> {
    packets
        .lock()
        .iter()
        .filter_map(|packet| {
            let mut cursor = Cursor::new(packet.encoded_data.as_slice());
            VarInt::read(&mut cursor).ok()?;
            if VarInt::read(&mut cursor).ok()?.0 != C_LEVEL_EVENT {
                return None;
            }
            Some((
                i32::read(&mut cursor).ok()?,
                BlockPos::read(&mut cursor).ok()?,
                i32::read(&mut cursor).ok()?,
                bool::read(&mut cursor).ok()?,
            ))
        })
        .collect()
}

fn tick_block_entities(world: &Arc<World>, ticks: i64) {
    for _ in 0..ticks {
        world.block_entity_tickers().tick(world, true);
    }
}

fn recorded_game_event_count(
    events: &SyncMutex<Vec<GameEventRef>>,
    expected: GameEventRef,
) -> usize {
    events
        .lock()
        .iter()
        .filter(|event| **event == expected)
        .count()
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "the end-to-end insertion lifecycle is clearest as one sequential test"
)]
fn insertion_consumption_ejection_and_no_duplication_match_vanilla() {
    let (world, _holder, pos, behavior) = jukebox_world("jukebox_insert_eject");
    let player = test_player(&world, 1);
    let mut access = InventoryAccess::new(Arc::clone(&player.inventory), InteractionHand::MainHand);
    let hit = hit_result(pos);

    player.inventory.lock().set_item_in_hand(
        InteractionHand::MainHand,
        ItemStack::new(&vanilla_items::STONE),
    );
    let empty_state = world.get_block_state(pos);
    assert_eq!(
        behavior.use_item_on(
            empty_state,
            &world,
            pos,
            &player,
            InteractionHand::MainHand,
            &hit,
            &mut access,
        ),
        InteractionResult::TryEmptyHandInteraction,
    );
    assert_eq!(
        player
            .inventory
            .lock()
            .get_item_in_hand(InteractionHand::MainHand)
            .count(),
        1,
    );
    assert!(
        !world
            .get_block_state(pos)
            .get_value(&BlockStateProperties::HAS_RECORD)
    );
    assert_eq!(
        behavior.use_without_item(empty_state, &world, pos, &player, &hit, &mut access),
        InteractionResult::Pass,
    );

    let cat = ItemStack::new(&vanilla_items::MUSIC_DISC_CAT);
    let cat_output = song_comparator(&cat);
    player
        .inventory
        .lock()
        .set_item_in_hand(InteractionHand::MainHand, cat);
    assert_eq!(
        behavior.use_item_on(
            empty_state,
            &world,
            pos,
            &player,
            InteractionHand::MainHand,
            &hit,
            &mut access,
        ),
        InteractionResult::Success,
    );
    assert!(
        player
            .inventory
            .lock()
            .get_item_in_hand(InteractionHand::MainHand)
            .is_empty()
    );
    let block_entity = jukebox_entity(&world, pos);
    let Some(jukebox) = block_entity.downcast_ref::<JukeboxBlockEntity>() else {
        panic!("jukebox should retain its concrete entity");
    };
    let Some(record) = stored_record(jukebox) else {
        panic!("inserted record should be stored");
    };
    assert!(record.is(&vanilla_items::MUSIC_DISC_CAT));
    assert_eq!(record.count(), 1);
    assert!(jukebox.is_record_playing());
    assert_eq!(
        behavior.get_analog_output_signal(
            world.get_block_state(pos),
            world.as_ref(),
            pos,
            Direction::North,
        ),
        cat_output
    );
    assert_eq!(world.get_signal(pos, Direction::North), MAX_REDSTONE_SIGNAL);
    assert_eq!(
        world.get_direct_signal(pos, Direction::North),
        MIN_REDSTONE_SIGNAL
    );

    assert_eq!(
        behavior.use_without_item(
            world.get_block_state(pos),
            &world,
            pos,
            &player,
            &hit,
            &mut access,
        ),
        InteractionResult::Success,
    );
    assert!(
        !world
            .get_block_state(pos)
            .get_value(&BlockStateProperties::HAS_RECORD)
    );
    assert!(stored_record(jukebox).is_none());
    assert!(!jukebox.is_record_playing());
    assert_eq!(world.get_signal(pos, Direction::North), MIN_REDSTONE_SIGNAL);

    let dropped = dropped_items(&world, pos);
    assert_eq!(dropped.len(), 1);
    let Some(item_entity) = dropped[0].downcast_ref::<ItemEntity>() else {
        panic!("ejected record should be an item entity");
    };
    assert!(item_entity.get_item().is(&vanilla_items::MUSIC_DISC_CAT));
    assert_eq!(
        item_entity.get_pickup_delay(),
        DEFAULT_ITEM_PICKUP_DELAY_TICKS
    );
    let item_pos = item_entity.position();
    let expected_ejection_center = DVec3::new(
        f64::from(pos.x()) + BLOCK_CENTER_OFFSET,
        f64::from(pos.y()) + JUKEBOX_EJECTION_Y_OFFSET,
        f64::from(pos.z()) + BLOCK_CENTER_OFFSET,
    );
    assert_within_radius(
        item_pos.x,
        expected_ejection_center.x,
        JUKEBOX_EJECTION_HORIZONTAL_RADIUS,
    );
    assert!((item_pos.y - expected_ejection_center.y).abs() < f64::EPSILON);
    assert_within_radius(
        item_pos.z,
        expected_ejection_center.z,
        JUKEBOX_EJECTION_HORIZONTAL_RADIUS,
    );
    let velocity = item_entity.velocity();
    assert_within_radius(velocity.x, 0.0, ITEM_EJECTION_HORIZONTAL_SPEED_LIMIT);
    assert!((velocity.y - ITEM_EJECTION_UPWARD_SPEED).abs() < f64::EPSILON);
    assert_within_radius(velocity.z, 0.0, ITEM_EJECTION_HORIZONTAL_SPEED_LIMIT);
    jukebox.pop_out_the_item();
    assert_eq!(dropped_items(&world, pos).len(), 1);

    player.restore_game_modes(GameType::Creative, Some(GameType::Survival));
    let pigstep = ItemStack::new(&vanilla_items::MUSIC_DISC_PIGSTEP);
    let pigstep_output = song_comparator(&pigstep);
    assert_ne!(cat_output, pigstep_output);
    player
        .inventory
        .lock()
        .set_item_in_hand(InteractionHand::MainHand, pigstep);
    assert_eq!(
        behavior.use_item_on(
            world.get_block_state(pos),
            &world,
            pos,
            &player,
            InteractionHand::MainHand,
            &hit,
            &mut access,
        ),
        InteractionResult::Success,
    );
    assert_eq!(
        player
            .inventory
            .lock()
            .get_item_in_hand(InteractionHand::MainHand)
            .count(),
        1,
    );
    let Some(record) = stored_record(jukebox) else {
        panic!("creative insertion should store one record");
    };
    assert!(record.is(&vanilla_items::MUSIC_DISC_PIGSTEP));
    assert_eq!(record.count(), 1);
    assert_eq!(
        behavior.get_analog_output_signal(
            world.get_block_state(pos),
            world.as_ref(),
            pos,
            Direction::South,
        ),
        pigstep_output
    );
}

#[test]
fn stackable_component_backed_record_consumes_one_item() {
    let (world, _holder, pos, behavior) = jukebox_world("jukebox_component_insertion");
    let player = test_player(&world, 2);
    let mut access = InventoryAccess::new(Arc::clone(&player.inventory), InteractionHand::MainHand);
    let mut stacked_record = ItemStack::with_count(&vanilla_items::STONE, 2);
    stacked_record.set(
        JUKEBOX_PLAYABLE,
        JukeboxPlayable::new(&vanilla_jukebox_songs::CAT),
    );
    player
        .inventory
        .lock()
        .set_item_in_hand(InteractionHand::MainHand, stacked_record);

    assert_eq!(
        behavior.use_item_on(
            world.get_block_state(pos),
            &world,
            pos,
            &player,
            InteractionHand::MainHand,
            &hit_result(pos),
            &mut access,
        ),
        InteractionResult::Success,
    );
    assert_eq!(
        player
            .inventory
            .lock()
            .get_item_in_hand(InteractionHand::MainHand)
            .count(),
        1,
    );
    let block_entity = jukebox_entity(&world, pos);
    let Some(jukebox) = block_entity.downcast_ref::<JukeboxBlockEntity>() else {
        panic!("jukebox should retain its concrete entity");
    };
    let Some(stored) = stored_record(jukebox) else {
        panic!("component-backed record should be stored");
    };
    assert!(stored.is(&vanilla_items::STONE));
    assert_eq!(stored.count(), 1);
    assert!(jukebox.is_record_playing());
    assert_eq!(jukebox.analog_output_signal(), song_comparator(&stored));
}

#[test]
fn level_events_periodic_game_events_and_duration_completion_match_song_data() {
    let (world, _holder, pos, behavior) = jukebox_world("jukebox_song_timing");
    let (_observer, packets) = recording_player(&world, pos);
    let events = Arc::new(SyncMutex::new(Vec::new()));
    let listener: SharedGameEventListener = Arc::new(RecordingGameEventListener {
        pos: block_center(pos),
        events: Arc::clone(&events),
    });
    world.register_game_event_listener(SectionPos::from_block_pos(pos), listener);

    let record = ItemStack::new(&vanilla_items::MUSIC_DISC_11);
    let expected_output = song_comparator(&record);
    let Some(playable) = record.get(JUKEBOX_PLAYABLE) else {
        panic!("test record should have a song");
    };
    let Some(song) = playable.song().as_reference() else {
        panic!("vanilla record should reference the typed registry");
    };
    let block_entity = jukebox_entity(&world, pos);
    let Some(jukebox) = block_entity.downcast_ref::<JukeboxBlockEntity>() else {
        panic!("jukebox should retain its concrete entity");
    };
    jukebox.set_the_item(record);

    assert!(
        level_events(&packets)
            .iter()
            .any(|event| { *event == (SOUND_PLAY_JUKEBOX_SONG, pos, song.id() as i32, false,) })
    );
    packets.lock().clear();
    events.lock().clear();

    tick_block_entities(&world, 1);
    assert_eq!(
        recorded_game_event_count(&events, &vanilla_game_events::JUKEBOX_PLAY),
        1,
    );
    assert!(
        packets
            .lock()
            .iter()
            .any(|packet| packet_id(packet) == Some(C_LEVEL_PARTICLES))
    );
    tick_block_entities(&world, PLAY_EVENT_INTERVAL_TICKS - 1);
    assert_eq!(
        recorded_game_event_count(&events, &vanilla_game_events::JUKEBOX_PLAY),
        1,
    );
    tick_block_entities(&world, 1);
    assert_eq!(
        recorded_game_event_count(&events, &vanilla_game_events::JUKEBOX_PLAY),
        2,
    );

    let finish_ticks =
        (song.value().length_in_seconds * TICKS_PER_SECOND).ceil() as i64 + SONG_END_PADDING_TICKS;
    tick_block_entities(&world, finish_ticks - (PLAY_EVENT_INTERVAL_TICKS + 1));
    assert!(jukebox.is_record_playing());
    tick_block_entities(&world, 1);
    assert!(!jukebox.is_record_playing());
    assert!(
        world
            .get_block_state(pos)
            .get_value(&BlockStateProperties::HAS_RECORD)
    );
    let Some(stored) = stored_record(jukebox) else {
        panic!("completed song should retain its record");
    };
    assert!(stored.is(&vanilla_items::MUSIC_DISC_11));
    assert_eq!(
        behavior.get_analog_output_signal(
            world.get_block_state(pos),
            world.as_ref(),
            pos,
            Direction::North,
        ),
        expected_output
    );
    assert_eq!(world.get_signal(pos, Direction::North), MIN_REDSTONE_SIGNAL);
    assert_eq!(
        world.get_direct_signal(pos, Direction::North),
        MIN_REDSTONE_SIGNAL
    );
    assert!(
        level_events(&packets)
            .iter()
            .any(|event| { event.0 == SOUND_STOP_JUKEBOX_SONG && event.1 == pos })
    );
    assert!(
        events
            .lock()
            .contains(&&vanilla_game_events::JUKEBOX_STOP_PLAY)
    );
}

#[test]
fn placement_data_and_nonzero_persistence_resume_without_restarting_audio() {
    init_globals_once();
    let world = fresh_test_world("jukebox_placement_data");
    let support = BlockPos::new(8, 63, 8);
    let pos = support.above();
    let _holder = insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));
    assert!(world.set_block(
        support,
        vanilla_blocks::STONE.default_state(),
        UpdateFlags::UPDATE_NONE,
    ));
    let (_observer, packets) = recording_player(&world, pos);

    let record = ItemStack::new(&vanilla_items::MUSIC_DISC_CAT);
    let mut payload = NbtCompound::new();
    payload.insert("RecordItem", record.to_nbt_tag_ref());
    // Vanilla's ValueInput coerces every numeric NBT tag for `getLong`.
    payload.insert("ticks_since_song_started", SAVED_PLAYBACK_TICKS);
    let Some(custom_data) = CustomData::try_from_compound(payload) else {
        panic!("placement payload should be valid custom data");
    };
    let mut placing_stack = ItemStack::new(&vanilla_items::JUKEBOX);
    placing_stack.set(
        BLOCK_ENTITY_DATA,
        BlockEntityData::new(&vanilla_block_entity_types::JUKEBOX, custom_data),
    );
    let source = PlacementSource::direct(
        None,
        InteractionHand::MainHand,
        &mut placing_stack,
        PlacementOrientation::Directional {
            direction: Direction::North,
        },
        false,
    );
    let context = BlockPlaceContext::new(
        &world,
        source,
        &BlockHitResult {
            location: block_bottom_center(pos),
            direction: Direction::Up,
            block_pos: support,
            miss: false,
            inside: false,
            world_border_hit: false,
        },
    );
    assert_eq!(
        BlockItem::new(&vanilla_blocks::JUKEBOX).place(context),
        InteractionResult::Success,
    );
    assert!(placing_stack.is_empty());
    assert!(
        world
            .get_block_state(pos)
            .get_value(&BlockStateProperties::HAS_RECORD)
    );
    let block_entity = jukebox_entity(&world, pos);
    let Some(jukebox) = block_entity.downcast_ref::<JukeboxBlockEntity>() else {
        panic!("placed jukebox should retain its concrete entity");
    };
    assert!(jukebox.is_record_playing());
    assert_eq!(saved_ticks(jukebox), Some(i64::from(SAVED_PLAYBACK_TICKS)));
    let Some(stored) = stored_record(jukebox) else {
        panic!("placement payload should preload the record");
    };
    assert!(stored.is(&vanilla_items::MUSIC_DISC_CAT));
    assert!(
        !level_events(&packets)
            .iter()
            .any(|event| { event.0 == SOUND_PLAY_JUKEBOX_SONG && event.1 == pos })
    );

    tick_block_entities(&world, 1);
    assert_eq!(
        saved_ticks(jukebox),
        Some(i64::from(SAVED_PLAYBACK_TICKS + 1))
    );
    let saved = jukebox.save_custom_only();
    let resumed = JukeboxBlockEntity::new(
        Weak::new(),
        BlockPos::new(0, 64, 0),
        vanilla_blocks::JUKEBOX
            .default_state()
            .set_value(&BlockStateProperties::HAS_RECORD, true),
    );
    let mut bytes = Vec::new();
    saved.write(&mut bytes);
    let mut cursor = Cursor::new(bytes.as_slice());
    let Ok(borrowed) = read_compound(&mut cursor) else {
        panic!("saved jukebox data should reborrow");
    };
    resumed.load_additional(&borrowed);
    assert!(resumed.is_record_playing());
    assert_eq!(
        saved_ticks(&resumed),
        Some(i64::from(SAVED_PLAYBACK_TICKS + 1))
    );
    let Some(stored) = stored_record(&resumed) else {
        panic!("resumed jukebox should retain the record");
    };
    assert!(stored.is(&vanilla_items::MUSIC_DISC_CAT));
}

#[test]
fn breaking_a_playing_jukebox_drops_one_record_and_runs_vanilla_cleanup() {
    let (world, _holder, pos, _behavior) = jukebox_world("jukebox_break_cleanup");
    let (_observer, packets) = recording_player(&world, pos);
    let block_entity = jukebox_entity(&world, pos);
    let Some(jukebox) = block_entity.downcast_ref::<JukeboxBlockEntity>() else {
        panic!("jukebox should retain its concrete entity");
    };
    jukebox.set_the_item(ItemStack::new(&vanilla_items::MUSIC_DISC_CAT));
    packets.lock().clear();

    assert!(world.set_block(
        pos,
        vanilla_blocks::AIR.default_state(),
        UpdateFlags::UPDATE_ALL,
    ));
    assert_eq!(world.get_block_state(pos).get_block(), &vanilla_blocks::AIR);
    assert!(world.get_block_entity(pos).is_none());
    let dropped = dropped_items(&world, pos);
    assert_eq!(dropped.len(), 1);
    let Some(item_entity) = dropped[0].downcast_ref::<ItemEntity>() else {
        panic!("broken jukebox record should be an item entity");
    };
    assert!(item_entity.get_item().is(&vanilla_items::MUSIC_DISC_CAT));
    assert_eq!(
        level_events(&packets)
            .iter()
            .filter(|event| { event.0 == SOUND_STOP_JUKEBOX_SONG && event.1 == pos })
            .count(),
        2,
    );
    assert!(!world.set_block(
        pos,
        vanilla_blocks::AIR.default_state(),
        UpdateFlags::UPDATE_ALL,
    ));
    assert_eq!(dropped_items(&world, pos).len(), 1);
}
