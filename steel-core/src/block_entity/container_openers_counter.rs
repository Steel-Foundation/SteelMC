//! Shared viewer counting for animated block containers.

use std::sync::Arc;

use glam::DVec3;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::vanilla_game_events;
use steel_utils::{BlockPos, BlockStateId, geometry::WorldAabb, locks::SyncMutex, types::GameType};

use crate::{
    entity::Entity as _,
    inventory::lock::ContainerId,
    player::Player,
    world::{World, game_event::GameEventContext},
};

use super::BlockEntity;

const CHECK_TICK_DELAY: i32 = 5;
const SEARCH_DISTANCE_BUFFER: f64 = 4.0;

#[derive(Default)]
struct OpenersState {
    open_count: i32,
    max_interaction_range: f64,
}

/// Vanilla-compatible viewer count and periodic stale-viewer reconciliation.
#[derive(Default)]
pub struct ContainerOpenersCounter {
    state: SyncMutex<OpenersState>,
}

impl ContainerOpenersCounter {
    fn increment(&self, owner: &(impl ContainerOpeners + ?Sized), player: &Player) {
        if owner.base().is_removed() || player.game_mode() == GameType::Spectator {
            return;
        }
        let Some(world) = owner.get_level() else {
            return;
        };
        let pos = owner.get_block_pos();
        let block_state = owner.get_block_state();
        let interaction_range = player.block_interaction_range();
        let (previous, current) = {
            let mut state = self.state.lock();
            let previous = state.open_count;
            state.open_count += 1;
            state.max_interaction_range = state.max_interaction_range.max(interaction_range);
            (previous, state.open_count)
        };

        if previous == 0 {
            owner.on_open(&world, pos, block_state);
            world.game_event(
                &vanilla_game_events::CONTAINER_OPEN,
                pos,
                &GameEventContext::new(Some(player), None),
            );
            Self::schedule_recheck(&world, pos, block_state);
        }
        owner.opener_count_changed(&world, pos, block_state, previous, current);
    }

    fn decrement(&self, owner: &(impl ContainerOpeners + ?Sized), player: &Player) {
        if owner.base().is_removed() || player.game_mode() == GameType::Spectator {
            return;
        }
        let Some(world) = owner.get_level() else {
            return;
        };
        let pos = owner.get_block_pos();
        let block_state = owner.get_block_state();
        let (previous, current) = {
            let mut state = self.state.lock();
            let previous = state.open_count;
            state.open_count -= 1;
            if state.open_count == 0 {
                state.max_interaction_range = 0.0;
            }
            (previous, state.open_count)
        };

        if current == 0 {
            owner.on_close(&world, pos, block_state);
            world.game_event(
                &vanilla_game_events::CONTAINER_CLOSE,
                pos,
                &GameEventContext::new(Some(player), None),
            );
        }
        owner.opener_count_changed(&world, pos, block_state, previous, current);
    }

    fn recheck(&self, owner: &(impl ContainerOpeners + ?Sized)) {
        if owner.base().is_removed() {
            return;
        }
        let Some(world) = owner.get_level() else {
            return;
        };
        let pos = owner.get_block_pos();
        let block_state = owner.get_block_state();
        let search_range = self.state.lock().max_interaction_range + SEARCH_DISTANCE_BUFFER;
        let min = DVec3::new(f64::from(pos.x()), f64::from(pos.y()), f64::from(pos.z()));
        let search_box = WorldAabb::from_min_max(min, min + DVec3::ONE).inflate(search_range);
        let mut open_count = 0;
        let mut max_interaction_range: f64 = 0.0;
        // TODO: Include non-player `ContainerUser` entities, notably copper
        // golems, once Steel has their concrete entity and container-user API.
        world.players.iter_players(|_, player| {
            if player.game_mode() != GameType::Spectator
                && search_box.intersects(player.bounding_box())
                && owner.is_own_container(player)
            {
                open_count += 1;
                max_interaction_range = max_interaction_range.max(player.block_interaction_range());
            }
            true
        });

        let previous = {
            let mut state = self.state.lock();
            let previous = state.open_count;
            state.max_interaction_range = max_interaction_range;
            previous
        };

        if previous != open_count {
            if previous == 0 && open_count != 0 {
                owner.on_open(&world, pos, block_state);
                world.game_event(
                    &vanilla_game_events::CONTAINER_OPEN,
                    pos,
                    &GameEventContext::default(),
                );
            } else if open_count == 0 {
                owner.on_close(&world, pos, block_state);
                world.game_event(
                    &vanilla_game_events::CONTAINER_CLOSE,
                    pos,
                    &GameEventContext::default(),
                );
            }
            self.state.lock().open_count = open_count;
        }
        owner.opener_count_changed(&world, pos, block_state, previous, open_count);
        if open_count > 0 {
            Self::schedule_recheck(&world, pos, block_state);
        }
    }

    fn schedule_recheck(world: &Arc<World>, pos: BlockPos, block_state: BlockStateId) {
        world.schedule_block_tick_default(pos, block_state.get_block(), CHECK_TICK_DELAY);
    }

    /// Returns the current viewer count.
    #[must_use]
    pub fn opener_count(&self) -> i32 {
        self.state.lock().open_count
    }
}

/// Capability implemented by block entities whose clients animate while viewed.
pub trait ContainerOpeners: BlockEntity {
    /// Returns the independently locked viewer counter.
    fn openers_counter(&self) -> &ContainerOpenersCounter;

    /// Returns the inventory identity used to recognize this entity's menu.
    fn opener_container_id(&self) -> ContainerId;

    /// Handles the zero-to-one viewer transition.
    fn on_open(&self, world: &Arc<World>, pos: BlockPos, block_state: BlockStateId);

    /// Handles the one-to-zero viewer transition.
    fn on_close(&self, world: &Arc<World>, pos: BlockPos, block_state: BlockStateId);

    /// Publishes a changed viewer count to block-specific state or events.
    fn opener_count_changed(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        block_state: BlockStateId,
        previous: i32,
        current: i32,
    );

    /// Returns whether the player's current menu references this entity.
    fn is_own_container(&self, player: &Player) -> bool {
        player.has_open_container(self.opener_container_id())
    }

    /// Registers one viewer and fires the Vanilla opening transition if needed.
    fn start_open(&self, player: &Player) {
        self.openers_counter().increment(self, player);
    }

    /// Unregisters one viewer and fires the Vanilla closing transition if needed.
    fn stop_open(&self, player: &Player) {
        self.openers_counter().decrement(self, player);
    }

    /// Reconciles the count against nearby players and schedules another check.
    fn recheck_open(&self) {
        self.openers_counter().recheck(self);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use glam::DVec3;
    use simdnbt::{borrow::BaseNbtCompound as BorrowedNbtCompound, owned::NbtCompound};
    use steel_registry::{
        test_support::init_test_registry, vanilla_block_entity_types, vanilla_blocks,
    };
    use steel_utils::{
        ChunkPos, DowncastType, DowncastTypeKey,
        locks::{IntoShared as _, SyncMutex},
    };
    use uuid::Uuid;

    use crate::{
        block_entity::{BlockEntityBase, SharedBlockEntity},
        inventory::{
            container::SimpleContainer,
            lock::{ContainerRef, SharedContainer},
            menu::kinds::chest_with_openers,
        },
        player::ResetReason,
        test_support::{TestPlayerBuilder, fresh_test_world, insert_ready_full_chunk},
    };

    use super::*;

    struct RecordingOpeners {
        base: Arc<BlockEntityBase>,
        container_ref: ContainerRef,
        counter: ContainerOpenersCounter,
        transitions: SyncMutex<Vec<(&'static str, i32)>>,
    }

    // SAFETY: This test-owned key uniquely identifies `RecordingOpeners`.
    unsafe impl DowncastType for RecordingOpeners {
        const TYPE_KEY: DowncastTypeKey =
            DowncastTypeKey::new("steel:test/block_entity/recording_openers");
    }

    impl RecordingOpeners {
        fn new(world: &Arc<World>, pos: BlockPos, container_ref: ContainerRef) -> Self {
            Self {
                base: Arc::new(BlockEntityBase::new(
                    &vanilla_block_entity_types::BARREL,
                    Arc::downgrade(world),
                    pos,
                    vanilla_blocks::BARREL.default_state(),
                )),
                container_ref,
                counter: ContainerOpenersCounter::default(),
                transitions: SyncMutex::new(Vec::new()),
            }
        }

        fn record(&self, transition: &'static str) {
            self.transitions
                .lock()
                .push((transition, self.counter.opener_count()));
        }
    }

    impl BlockEntity for RecordingOpeners {
        fn base(&self) -> &BlockEntityBase {
            &self.base
        }

        fn load_additional(&self, _nbt: &BorrowedNbtCompound<'_>) {}

        fn save_additional(&self, _nbt: &mut NbtCompound) {}

        fn container_ref(&self) -> Option<ContainerRef> {
            Some(self.container_ref.clone())
        }

        fn container_openers(&self) -> Option<&dyn ContainerOpeners> {
            Some(self)
        }
    }

    impl ContainerOpeners for RecordingOpeners {
        fn openers_counter(&self) -> &ContainerOpenersCounter {
            &self.counter
        }

        fn opener_container_id(&self) -> ContainerId {
            self.container_ref.container_id()
        }

        fn on_open(&self, _world: &Arc<World>, _pos: BlockPos, _block_state: BlockStateId) {
            self.record("open");
        }

        fn on_close(&self, _world: &Arc<World>, _pos: BlockPos, _block_state: BlockStateId) {
            self.record("close");
        }

        fn opener_count_changed(
            &self,
            _world: &Arc<World>,
            _pos: BlockPos,
            _block_state: BlockStateId,
            _previous: i32,
            _current: i32,
        ) {
        }
    }

    #[test]
    fn recheck_callbacks_observe_the_vanilla_previous_count() {
        init_test_registry();
        let world = fresh_test_world("container_opener_recheck_order");
        let pos = BlockPos::new(3, 64, 3);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));
        let container = SimpleContainer::new(9).into_shared();
        let shared_container: SharedContainer = container;
        let container_ref = ContainerRef::from(shared_container);
        let owner = Arc::new(RecordingOpeners::new(&world, pos, container_ref.clone()));
        let player =
            TestPlayerBuilder::new(Arc::clone(&world), "OpenerViewer", 1)
                .uuid(Uuid::from_u128(1))
                .build();
        player.base().set_position_local(DVec3::new(3.5, 64.0, 3.5));
        assert!(world.add_player(Arc::clone(&player), ResetReason::InitialJoin));
        let inventory = Arc::clone(&player.inventory);
        let opener: SharedBlockEntity = owner.clone();
        player.open_menu("Recording opener", move |context| {
            chest_with_openers(
                inventory,
                context.container_id,
                vec![(container_ref, 9)],
                1,
                vec![opener],
            )
        });

        assert!(
            player
                .base()
                .try_set_position(DVec3::new(15.0, 64.0, 3.5))
                .is_ok()
        );
        owner.recheck_open();
        assert_eq!(owner.counter.opener_count(), 0);

        assert!(
            player
                .base()
                .try_set_position(DVec3::new(3.5, 64.0, 3.5))
                .is_ok()
        );
        owner.recheck_open();
        assert_eq!(owner.counter.opener_count(), 1);

        player.close_container();

        assert_eq!(
            *owner.transitions.lock(),
            [("open", 1), ("close", 1), ("open", 0), ("close", 0)]
        );
    }
}
