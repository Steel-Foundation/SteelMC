//! Viewer tracking shared by container block entities.
//!
//! Mirrors Vanilla `ContainerOpenersCounter`: the first viewer opens the
//! container, the last one closes it, and a periodic recheck drops viewers that
//! stopped looking at it without closing their menu.

use std::sync::Arc;

use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::vanilla_game_events;
use steel_utils::{BlockPos, BlockStateId, WorldAabb, locks::SyncMutex};

use crate::entity::Entity as _;
use crate::inventory::lock::ContainerId;
use crate::player::Player;
use crate::world::World;
use crate::world::game_event::GameEventContext;

/// Vanilla `ContainerOpenersCounter.CHECK_TICK_DELAY`.
const CHECK_TICK_DELAY: i32 = 5;

/// Vanilla's fixed slack added to the largest viewer interaction range when
/// searching for entities that still have the container open.
const VIEWER_SEARCH_BUFFER: f64 = 4.0;

/// The container block entity that owns a [`ContainerOpenersCounter`].
///
/// Vanilla implements these as abstract methods on the counter itself; Steel
/// keeps the counter concrete and calls back into the owning block entity.
pub trait ContainerOpenersHost {
    /// Vanilla `onOpen`: runs when the viewer count rises above zero.
    fn on_open(&self, world: &Arc<World>, pos: BlockPos, state: BlockStateId);

    /// Vanilla `onClose`: runs when the viewer count drops back to zero.
    fn on_close(&self, world: &Arc<World>, pos: BlockPos, state: BlockStateId);

    /// Vanilla `openerCountChanged`: runs on every viewer count transition.
    fn opener_count_changed(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
        previous: i32,
        current: i32,
    );

    /// The container this counter tracks viewers for.
    ///
    /// Vanilla's `isOwnContainer` compares the viewer's menu container against
    /// the block entity. Steel menus reference containers by
    /// [`ContainerId`], so a viewer counts while its open menu still views this
    /// container. A double chest reports both halves, so each half's counter
    /// recognizes the shared menu.
    fn opener_container_id(&self) -> ContainerId;
}

#[derive(Default)]
struct OpenersState {
    open_count: i32,
    max_interaction_range: f64,
}

/// Tracks how many viewers currently have a container block entity open.
#[derive(Default)]
pub struct ContainerOpenersCounter {
    state: SyncMutex<OpenersState>,
}

impl ContainerOpenersCounter {
    /// Creates a counter with no viewers.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Vanilla `getOpenerCount`.
    #[must_use]
    pub fn opener_count(&self) -> i32 {
        self.state.lock().open_count
    }

    /// Vanilla `incrementOpeners`.
    pub fn increment_openers(
        &self,
        host: &dyn ContainerOpenersHost,
        player: &Player,
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
    ) {
        let interaction_range = player.block_interaction_range();
        let (previous, current) = {
            let mut counter = self.state.lock();
            let previous = counter.open_count;
            counter.open_count += 1;
            counter.max_interaction_range = counter.max_interaction_range.max(interaction_range);
            (previous, counter.open_count)
        };

        if previous == 0 {
            host.on_open(world, pos, state);
            world.game_event(
                &vanilla_game_events::CONTAINER_OPEN,
                pos,
                &GameEventContext::new(Some(player), Some(state)),
            );
            schedule_recheck(world, pos, state);
        }

        host.opener_count_changed(world, pos, state, previous, current);
    }

    /// Vanilla `decrementOpeners`.
    pub fn decrement_openers(
        &self,
        host: &dyn ContainerOpenersHost,
        player: &Player,
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
    ) {
        let (previous, current) = {
            let mut counter = self.state.lock();
            let previous = counter.open_count;
            counter.open_count -= 1;
            if counter.open_count == 0 {
                counter.max_interaction_range = 0.0;
            }
            (previous, counter.open_count)
        };

        if current == 0 {
            host.on_close(world, pos, state);
            world.game_event(
                &vanilla_game_events::CONTAINER_CLOSE,
                pos,
                &GameEventContext::new(Some(player), Some(state)),
            );
        }

        host.opener_count_changed(world, pos, state, previous, current);
    }

    /// Vanilla `recheckOpeners`, run from the owning block's scheduled tick.
    pub fn recheck_openers(
        &self,
        host: &dyn ContainerOpenersHost,
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
    ) {
        let viewers = self.players_with_container_open(host, world, pos);
        let open_count = i32::try_from(viewers.len()).unwrap_or(i32::MAX);

        let previous = {
            let mut counter = self.state.lock();
            counter.max_interaction_range = viewers
                .iter()
                .map(|player| player.block_interaction_range())
                .fold(0.0, f64::max);
            let previous = counter.open_count;
            if previous != open_count {
                counter.open_count = open_count;
            }
            previous
        };

        if previous != open_count {
            if open_count != 0 && previous == 0 {
                host.on_open(world, pos, state);
                world.game_event(
                    &vanilla_game_events::CONTAINER_OPEN,
                    pos,
                    &GameEventContext::new(None, Some(state)),
                );
            } else if open_count == 0 {
                host.on_close(world, pos, state);
                world.game_event(
                    &vanilla_game_events::CONTAINER_CLOSE,
                    pos,
                    &GameEventContext::new(None, Some(state)),
                );
            }
        }

        host.opener_count_changed(world, pos, state, previous, open_count);
        if open_count > 0 {
            schedule_recheck(world, pos, state);
        }
    }

    /// Vanilla `getEntitiesWithContainerOpen`, restricted to players.
    ///
    /// Vanilla searches the entity list because copper golems are container
    /// users too. Steel keeps players outside the entity manager and has no
    /// copper golem yet, so this walks the world's players and applies the same
    /// range and viewer checks.
    #[must_use]
    pub fn players_with_container_open(
        &self,
        host: &dyn ContainerOpenersHost,
        world: &Arc<World>,
        pos: BlockPos,
    ) -> Vec<Arc<Player>> {
        let range = self.state.lock().max_interaction_range + VIEWER_SEARCH_BUFFER;
        let container_id = host.opener_container_id();
        let search_box = WorldAabb::new(
            f64::from(pos.x()),
            f64::from(pos.y()),
            f64::from(pos.z()),
            f64::from(pos.x()) + 1.0,
            f64::from(pos.y()) + 1.0,
            f64::from(pos.z()) + 1.0,
        )
        .inflate(range);

        let mut viewers = Vec::new();
        world.players.iter_players(|_, player| {
            if !player.is_spectator()
                && search_box.intersects(player.bounding_box())
                && player.views_container(container_id)
            {
                viewers.push(Arc::clone(player));
            }
            true
        });
        viewers
    }
}

fn schedule_recheck(world: &Arc<World>, pos: BlockPos, state: BlockStateId) {
    world.schedule_block_tick_default(pos, state.get_block(), CHECK_TICK_DELAY);
}
