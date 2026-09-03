//! Shears item behavior (`ShearsItem`).
//!
//! Right-clicking a growing plant head that is not at max age crops it, matching
//! vanilla `ShearsItem.useOn`.

use steel_macros::item_behavior;
use steel_registry::{blocks::block_state_ext::BlockStateExt, sound_events, vanilla_game_events};
use steel_utils::{BlockPos, BlockStateId, types::UpdateFlags};

use crate::behavior::blocks::vegetation::GrowingPlantHeadBehavior;
use crate::behavior::{BLOCK_BEHAVIORS, InteractionResult, ItemBehavior, UseOnContext};
use crate::entity::Entity;
use crate::world::game_event::GameEventContext;

/// Behavior for shears.
#[item_behavior]
pub struct ShearsItem;

impl ShearsItem {
    /// Crops a growing plant head to max age so `random_tick` no longer extends it.
    fn block_age(
        context: &UseOnContext<'_>,
        plant: &dyn GrowingPlantHeadBehavior,
        pos: BlockPos,
        state: BlockStateId,
    ) {
        let new_state = plant.get_max_age_state(state);
        context
            .world
            .set_block(pos, new_state, UpdateFlags::UPDATE_ALL);
        context.world.game_event(
            &vanilla_game_events::BLOCK_CHANGE,
            pos,
            &GameEventContext::new(Some(context.player), Some(new_state)),
        );
    }
}

impl ItemBehavior for ShearsItem {
    fn use_on(&self, context: &mut UseOnContext) -> InteractionResult {
        let pos = context.hit_result.block_pos;
        let state = context.world.get_block_state(pos);
        let behavior = BLOCK_BEHAVIORS.get_behavior(state.get_block());
        let Some(plant) = behavior.as_growing_plant_head() else {
            return InteractionResult::Pass;
        };
        if plant.is_max_age(state) {
            return InteractionResult::Pass;
        }

        // TODO: Trigger ITEM_USED_ON_BLOCK once advancement criteria dispatch is implemented.
        context.world.play_block_sound(
            &sound_events::BLOCK_GROWING_PLANT_CROP,
            pos,
            1.0,
            1.0,
            Some(context.player.id()),
        );
        Self::block_age(context, plant, pos, state);

        let has_infinite_materials = context.player.has_infinite_materials();
        context
            .inv
            .with_item(|item| item.hurt_and_break(1, has_infinite_materials));

        InteractionResult::Success
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::sync::Arc;

    use glam::DVec3;
    use steel_protocol::packet_traits::{CompressionInfo, EncodedPacket};
    use steel_registry::blocks::block_state_ext::BlockStateExt;
    use steel_registry::blocks::properties::BlockStateProperties;
    use steel_registry::game_events::GameEventRef;
    use steel_registry::item_stack::ItemStack;
    use steel_registry::packets::play::C_SOUND;
    use steel_registry::{sound_events, vanilla_blocks, vanilla_game_events, vanilla_items};
    use steel_utils::codec::VarInt;
    use steel_utils::locks::SyncMutex;
    use steel_utils::serial::ReadFrom;
    use steel_utils::types::{InteractionHand, UpdateFlags};
    use steel_utils::{BlockPos, ChunkPos, Direction, SectionPos};
    use text_components::TextComponent;

    use super::ShearsItem;
    use crate::behavior::blocks::vegetation::MAX_AGE;
    use crate::behavior::{BlockHitResult, InteractionResult, ItemBehavior, UseOnContext};
    use crate::bootstrap::init_globals_once;
    use crate::chunk::chunk_holder::ChunkHolder;
    use crate::entity::Entity;
    use crate::player::connection::NetworkConnection;
    use crate::player::{Player, PlayerConnection, ResetReason};
    use crate::test_support::{TestPlayerBuilder, fresh_test_world, insert_ready_full_chunk};
    use crate::world::World;
    use crate::world::game_event::{GameEventContext, GameEventListener, SharedGameEventListener};

    const LISTENER_RADIUS: i32 = 16;

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
            LISTENER_RADIUS
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

    struct ShearsFixture {
        world: Arc<World>,
        _holder: Arc<ChunkHolder>,
        player: Arc<Player>,
        _observer: Arc<Player>,
        packets: Arc<SyncMutex<Vec<EncodedPacket>>>,
        events: Arc<SyncMutex<Vec<GameEventRef>>>,
        pos: BlockPos,
    }

    fn block_center(pos: BlockPos) -> DVec3 {
        pos.0.as_dvec3().map(|coordinate| coordinate + 0.5)
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

    fn create_fixture(key: &'static str, age: u8) -> ShearsFixture {
        init_globals_once();
        let world = fresh_test_world(key);
        let pos = BlockPos::new(8, 64, 8);
        let holder = insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));
        let state = vanilla_blocks::KELP
            .default_state()
            .set_value(&BlockStateProperties::AGE_25, age);
        assert!(world.set_block(pos, state, UpdateFlags::UPDATE_NONE));

        let player = TestPlayerBuilder::new(Arc::clone(&world), "ShearsTester", 1).build();
        player.inventory.lock().set_item_in_hand(
            InteractionHand::MainHand,
            ItemStack::new(&vanilla_items::SHEARS),
        );

        let packets = Arc::new(SyncMutex::new(Vec::new()));
        let connection = Arc::new(PlayerConnection::Other(Box::new(RecordingConnection {
            packets: Arc::clone(&packets),
        })));
        let observer = TestPlayerBuilder::new(Arc::clone(&world), "ShearsObserver", 2)
            .connection(connection)
            .build();
        assert!(observer.try_set_position(block_center(pos)).is_ok());
        assert!(world.add_player(Arc::clone(&observer), ResetReason::InitialJoin));
        packets.lock().clear();

        let events = Arc::new(SyncMutex::new(Vec::new()));
        let listener: SharedGameEventListener = Arc::new(RecordingGameEventListener {
            pos: block_center(pos),
            events: Arc::clone(&events),
        });
        world.register_game_event_listener(SectionPos::from_block_pos(pos), listener);

        ShearsFixture {
            world,
            _holder: holder,
            player,
            _observer: observer,
            packets,
            events,
            pos,
        }
    }

    fn sound_ids(packets: &SyncMutex<Vec<EncodedPacket>>) -> Vec<i32> {
        packets
            .lock()
            .iter()
            .filter_map(|packet| {
                let mut cursor = Cursor::new(packet.encoded_data.as_slice());
                VarInt::read(&mut cursor).ok()?;
                if VarInt::read(&mut cursor).ok()?.0 != C_SOUND {
                    return None;
                }
                Some(VarInt::read(&mut cursor).ok()?.0)
            })
            .collect()
    }

    fn use_shears(fixture: &ShearsFixture) -> InteractionResult {
        let mut context = UseOnContext::new(
            &fixture.player,
            InteractionHand::MainHand,
            hit_result(fixture.pos),
            &fixture.world,
            Arc::clone(&fixture.player.inventory),
        );
        ShearsItem.use_on(&mut context)
    }

    #[test]
    fn use_on_crops_growing_head_and_emits_vanilla_effects() {
        let non_max_age = MAX_AGE - 1;
        let fixture = create_fixture("shears_crop", non_max_age);

        assert_eq!(use_shears(&fixture), InteractionResult::Success);
        assert_eq!(
            fixture
                .world
                .get_block_state(fixture.pos)
                .get_value(&BlockStateProperties::AGE_25),
            MAX_AGE,
        );
        assert_eq!(
            fixture
                .player
                .inventory
                .lock()
                .get_item_in_hand(InteractionHand::MainHand)
                .get_damage_value(),
            1,
        );
        assert_eq!(
            sound_ids(&fixture.packets),
            [sound_events::BLOCK_GROWING_PLANT_CROP.packet_holder_id()],
        );
        assert_eq!(
            fixture.events.lock().as_slice(),
            [&vanilla_game_events::BLOCK_CHANGE],
        );
    }

    #[test]
    fn use_on_max_age_head_passes_without_effects() {
        let fixture = create_fixture("shears_max_age", MAX_AGE);
        let original_state = fixture.world.get_block_state(fixture.pos);

        assert_eq!(use_shears(&fixture), InteractionResult::Pass);
        assert_eq!(fixture.world.get_block_state(fixture.pos), original_state);
        assert_eq!(
            fixture
                .player
                .inventory
                .lock()
                .get_item_in_hand(InteractionHand::MainHand)
                .get_damage_value(),
            0,
        );
        assert_eq!(sound_ids(&fixture.packets), Vec::<i32>::new());
        let no_events: &[GameEventRef] = &[];
        assert_eq!(fixture.events.lock().as_slice(), no_events);
    }
}
