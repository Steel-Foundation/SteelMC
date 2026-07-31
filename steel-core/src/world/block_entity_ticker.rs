//! World-owned block-entity ticker ordering.

use std::{
    mem,
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, Ordering},
    },
};

use arc_swap::ArcSwapOption;
use rustc_hash::FxHashMap;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_utils::{BlockPos, locks::SyncMutex};

use crate::{
    block_entity::{BlockEntityLifecycleExt as _, BlockEntityTicker, SharedBlockEntity},
    chunk::{chunk_holder::ChunkHolder, chunk_ticket_manager::ChunkTicketLevel},
};

use super::{World, border::WorldBorderSnapshot};

/// One immutable revision of a rebindable ticker.
pub(crate) struct BoundBlockEntityTicker {
    holder: Weak<ChunkHolder>,
    entity: SharedBlockEntity,
    ticker: BlockEntityTicker,
    logged_invalid_state: AtomicBool,
}

impl BoundBlockEntityTicker {
    fn new(
        holder: &Arc<ChunkHolder>,
        entity: SharedBlockEntity,
        ticker: BlockEntityTicker,
    ) -> Self {
        Self {
            holder: Arc::downgrade(holder),
            entity,
            ticker,
            logged_invalid_state: AtomicBool::new(false),
        }
    }

    fn belongs_to(&self, holder: &Arc<ChunkHolder>) -> bool {
        self.holder.as_ptr() == Arc::as_ptr(holder)
    }
}

/// Stable global-list entry whose concrete entity/ticker can change in place.
pub(crate) struct RebindableBlockEntityTicker {
    binding: ArcSwapOption<BoundBlockEntityTicker>,
}

impl RebindableBlockEntityTicker {
    fn new(binding: Arc<BoundBlockEntityTicker>) -> Self {
        Self {
            binding: ArcSwapOption::from(Some(binding)),
        }
    }

    fn rebind(&self, binding: Option<Arc<BoundBlockEntityTicker>>) {
        self.binding.store(binding);
    }
}

#[derive(Default)]
struct TickerState {
    active: Vec<Arc<RebindableBlockEntityTicker>>,
    pending: Vec<Arc<RebindableBlockEntityTicker>>,
    by_pos: FxHashMap<BlockPos, Arc<RebindableBlockEntityTicker>>,
    ticking: bool,
}

/// Vanilla-shaped world-global active and pending block-entity ticker lists.
#[derive(Default)]
pub(crate) struct WorldBlockEntityTickers {
    state: SyncMutex<TickerState>,
}

impl WorldBlockEntityTickers {
    #[must_use]
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Reconciles one live storage owner with its state-selected ticker.
    ///
    /// Rebinding an existing position preserves its global registration slot.
    /// Disabling then enabling creates a new tail, matching Vanilla.
    pub(crate) fn reconcile(
        &self,
        holder: &Arc<ChunkHolder>,
        entity: SharedBlockEntity,
        ticker: Option<BlockEntityTicker>,
    ) {
        let pos = entity.get_block_pos();
        let mut state = self.state.lock();

        let existing = state.by_pos.get(&pos).cloned();
        let Some(ticker) = ticker else {
            if let Some(wrapper) = existing
                && wrapper
                    .binding
                    .load()
                    .as_ref()
                    .is_some_and(|binding| binding.belongs_to(holder))
            {
                state.by_pos.remove(&pos);
                wrapper.rebind(None);
            }
            return;
        };

        let binding = Arc::new(BoundBlockEntityTicker::new(holder, entity, ticker));
        if let Some(wrapper) = existing {
            let same_holder = wrapper
                .binding
                .load()
                .as_ref()
                .is_some_and(|current| current.belongs_to(holder));
            if same_holder {
                wrapper.rebind(Some(binding));
                return;
            }
            wrapper.rebind(None);
            state.by_pos.remove(&pos);
        }

        let wrapper = Arc::new(RebindableBlockEntityTicker::new(binding));
        state.by_pos.insert(pos, Arc::clone(&wrapper));
        if state.ticking {
            state.pending.push(wrapper);
        } else {
            state.active.push(wrapper);
        }
    }

    /// Removes the wrapper only when it still belongs to `holder`.
    pub(crate) fn remove(&self, holder: &Arc<ChunkHolder>, pos: BlockPos) {
        let mut state = self.state.lock();
        let Some(wrapper) = state.by_pos.get(&pos).cloned() else {
            return;
        };
        if !wrapper
            .binding
            .load()
            .as_ref()
            .is_some_and(|binding| binding.belongs_to(holder))
        {
            return;
        }
        state.by_pos.remove(&pos);
        wrapper.rebind(None);
    }

    /// Unbinds tickers at concrete positions owned by one finalized chunk holder.
    pub(crate) fn remove_positions(&self, holder: &Arc<ChunkHolder>, positions: &[BlockPos]) {
        let mut state = self.state.lock();
        for pos in positions {
            let belongs = state.by_pos.get(pos).is_some_and(|wrapper| {
                wrapper
                    .binding
                    .load()
                    .as_ref()
                    .is_some_and(|binding| binding.belongs_to(holder))
            });
            if belongs && let Some(wrapper) = state.by_pos.remove(pos) {
                wrapper.rebind(None);
            }
        }
    }

    /// Runs one global ticker phase while keeping registration locks out of callbacks.
    pub(crate) fn tick(&self, world: &Arc<World>, runs_normally: bool) {
        let border = runs_normally.then(|| world.world_border_snapshot());
        self.tick_phase(runs_normally, |binding| {
            if let Some(border) = border {
                Self::tick_binding(world, border, binding);
            }
        });
    }

    fn tick_phase(
        &self,
        runs_normally: bool,
        mut tick_binding: impl FnMut(&Arc<BoundBlockEntityTicker>),
    ) {
        let mut current = {
            let mut state = self.state.lock();
            debug_assert!(!state.ticking, "block-entity ticker phase re-entered");
            state.ticking = true;
            let pending = mem::take(&mut state.pending);
            state.active.extend(pending);
            mem::take(&mut state.active)
        };

        current.retain(|wrapper| {
            let binding_guard = wrapper.binding.load();
            let Some(binding) = binding_guard.as_ref() else {
                return false;
            };
            if binding.entity.is_removed() {
                self.remove_if_same(wrapper, binding);
                return false;
            }
            if runs_normally {
                tick_binding(binding);
            }
            drop(binding_guard);
            wrapper.binding.load().is_some()
        });

        let mut state = self.state.lock();
        debug_assert!(state.active.is_empty());
        state.active = current;
        state.ticking = false;
    }

    fn tick_binding(
        world: &Arc<World>,
        border: WorldBorderSnapshot,
        binding: &Arc<BoundBlockEntityTicker>,
    ) {
        let Some(holder) = binding.holder.upgrade() else {
            return;
        };
        if !holder
            .simulation_level()
            .is_some_and(ChunkTicketLevel::is_block_ticking)
            || !holder.ticking_readiness_snapshot().is_block_ticking()
        {
            return;
        }
        let pos = binding.entity.get_block_pos();
        if !border.is_block_within_bounds(pos)
            || !world.entity_manager().is_chunk_loaded(holder.get_pos())
        {
            return;
        }

        let Some(state) =
            world
                .chunk_map
                .block_entity_tick_state_if_owned(&holder, pos, &binding.entity)
        else {
            return;
        };

        if !binding.entity.is_valid_block_state(state) {
            if !binding.logged_invalid_state.swap(true, Ordering::Relaxed) {
                tracing::warn!(
                    block_entity_type = %binding.entity.get_type().key,
                    ?pos,
                    block = %state.get_block().key,
                    "Block entity has an invalid state for ticking"
                );
            }
            return;
        }

        binding.logged_invalid_state.store(false, Ordering::Relaxed);
        binding
            .ticker
            .tick(world, pos, state, binding.entity.as_ref());
    }

    fn remove_if_same(
        &self,
        wrapper: &Arc<RebindableBlockEntityTicker>,
        binding: &Arc<BoundBlockEntityTicker>,
    ) {
        let pos = binding.entity.get_block_pos();
        let mut state = self.state.lock();
        let still_same_wrapper = state
            .by_pos
            .get(&pos)
            .is_some_and(|current| Arc::ptr_eq(current, wrapper));
        let still_same_binding = wrapper
            .binding
            .load()
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, binding));
        if still_same_wrapper && still_same_binding {
            state.by_pos.remove(&pos);
            wrapper.rebind(None);
        }
    }

    #[cfg(test)]
    pub(crate) fn registered_len(&self) -> usize {
        self.state.lock().by_pos.len()
    }

    #[cfg(test)]
    pub(crate) fn active_positions(&self) -> Vec<BlockPos> {
        self.state
            .lock()
            .active
            .iter()
            .filter_map(|wrapper| {
                wrapper
                    .binding
                    .load()
                    .as_ref()
                    .map(|binding| binding.entity.get_block_pos())
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Weak};

    use steel_registry::{
        test_support::init_test_registry, vanilla_block_entity_types, vanilla_blocks,
    };
    use steel_utils::{BlockPos, ChunkPos};

    use super::*;
    use crate::{
        block_entity::entities::SignBlockEntity,
        chunk::{chunk_holder::ChunkHolder, chunk_ticket_manager::ChunkTicketLevel},
    };

    fn holder() -> Arc<ChunkHolder> {
        Arc::new(ChunkHolder::new(
            ChunkPos::new(0, 0),
            ChunkTicketLevel::BLOCK_TICKING_CHUNK,
            Some(ChunkTicketLevel::BLOCK_TICKING_CHUNK),
            -64,
            384,
        ))
    }

    fn sign(pos: BlockPos) -> SharedBlockEntity {
        Arc::new(SignBlockEntity::new(
            Weak::new(),
            pos,
            vanilla_blocks::OAK_SIGN.default_state(),
        ))
    }

    fn sign_ticker() -> BlockEntityTicker {
        BlockEntityTicker::for_entity_tick(&vanilla_block_entity_types::SIGN)
    }

    #[test]
    fn additions_during_phase_wait_and_follow_between_phase_additions() {
        init_test_registry();
        let manager = WorldBlockEntityTickers::new();
        let holder = holder();
        let first = sign(BlockPos::new(1, 2, 3));
        let pending = sign(BlockPos::new(2, 2, 3));
        let between = sign(BlockPos::new(3, 2, 3));
        manager.reconcile(&holder, Arc::clone(&first), Some(sign_ticker()));

        let mut added = false;
        manager.tick_phase(true, |_| {
            if !added {
                added = true;
                manager.reconcile(&holder, Arc::clone(&pending), Some(sign_ticker()));
            }
        });
        manager.reconcile(&holder, Arc::clone(&between), Some(sign_ticker()));

        let mut observed = Vec::new();
        manager.tick_phase(true, |binding| {
            observed.push(binding.entity.get_block_pos());
        });
        assert_eq!(
            observed,
            [
                first.get_block_pos(),
                between.get_block_pos(),
                pending.get_block_pos(),
            ]
        );
    }

    #[test]
    fn rebind_before_turn_uses_new_owner_in_the_original_slot() {
        init_test_registry();
        let manager = WorldBlockEntityTickers::new();
        let holder = holder();
        let first = sign(BlockPos::new(1, 2, 3));
        let old_second = sign(BlockPos::new(2, 2, 3));
        let new_second = sign(BlockPos::new(2, 2, 3));
        manager.reconcile(&holder, Arc::clone(&first), Some(sign_ticker()));
        manager.reconcile(&holder, Arc::clone(&old_second), Some(sign_ticker()));

        let mut observed = Vec::new();
        manager.tick_phase(true, |binding| {
            if Arc::ptr_eq(&binding.entity, &first) {
                manager.reconcile(&holder, Arc::clone(&new_second), Some(sign_ticker()));
                observed.push("first");
            } else if Arc::ptr_eq(&binding.entity, &new_second) {
                observed.push("new");
            } else {
                observed.push("old");
            }
        });

        assert_eq!(observed, ["first", "new"]);
        assert_eq!(manager.registered_len(), 2);
    }

    #[test]
    fn remove_then_add_during_phase_creates_a_pending_tail() {
        init_test_registry();
        let manager = WorldBlockEntityTickers::new();
        let holder = holder();
        let first = sign(BlockPos::new(1, 2, 3));
        let old_second = sign(BlockPos::new(2, 2, 3));
        let new_second = sign(BlockPos::new(2, 2, 3));
        manager.reconcile(&holder, Arc::clone(&first), Some(sign_ticker()));
        manager.reconcile(&holder, Arc::clone(&old_second), Some(sign_ticker()));

        let mut changed = false;
        let mut first_phase = Vec::new();
        manager.tick_phase(true, |binding| {
            first_phase.push(Arc::clone(&binding.entity));
            if !changed && Arc::ptr_eq(&binding.entity, &first) {
                changed = true;
                manager.remove(&holder, old_second.get_block_pos());
                manager.reconcile(&holder, Arc::clone(&new_second), Some(sign_ticker()));
            }
        });
        assert_eq!(first_phase.len(), 1);
        assert!(Arc::ptr_eq(&first_phase[0], &first));

        let mut second_phase = Vec::new();
        manager.tick_phase(true, |binding| {
            second_phase.push(Arc::clone(&binding.entity));
        });
        assert_eq!(second_phase.len(), 2);
        assert!(Arc::ptr_eq(&second_phase[0], &first));
        assert!(Arc::ptr_eq(&second_phase[1], &new_second));
    }

    #[test]
    fn frozen_phase_merges_pending_and_prunes_unbound_without_callbacks() {
        init_test_registry();
        let manager = WorldBlockEntityTickers::new();
        let holder = holder();
        let first = sign(BlockPos::new(1, 2, 3));
        let pending = sign(BlockPos::new(2, 2, 3));
        manager.reconcile(&holder, Arc::clone(&first), Some(sign_ticker()));

        manager.tick_phase(true, |_| {
            manager.reconcile(&holder, Arc::clone(&pending), Some(sign_ticker()));
            manager.remove(&holder, first.get_block_pos());
        });
        manager.tick_phase(false, |_| panic!("frozen phase must suppress callbacks"));

        let mut observed = Vec::new();
        manager.tick_phase(true, |binding| {
            observed.push(Arc::clone(&binding.entity));
        });
        assert_eq!(observed.len(), 1);
        assert!(Arc::ptr_eq(&observed[0], &pending));
    }
}
