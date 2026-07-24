//! Game event listener registration and dispatch.

use std::{
    mem,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use glam::DVec3;
use rustc_hash::FxHashMap;
use steel_registry::game_events::GameEventRef;
use steel_utils::BlockPos;
#[cfg(test)]
use steel_utils::SectionPos;
use steel_utils::locks::SyncMutex;

use crate::world::World;
use crate::world::game_event_context::GameEventContext;

/// Controls when a listener receives an event during dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameEventDeliveryMode {
    /// Handle the event immediately while scanning listeners.
    Unspecified,
    /// Queue the event and handle it after sorting by source distance.
    ByDistance,
}

/// A receiver for vanilla game events.
pub trait GameEventListener: Send + Sync {
    /// Returns the current world position of this listener.
    fn listener_pos(&self) -> Option<DVec3>;

    /// Returns the maximum block distance this listener can hear.
    fn listener_radius(&self) -> i32;

    /// Returns how this listener should be ordered during dispatch.
    fn delivery_mode(&self) -> GameEventDeliveryMode {
        GameEventDeliveryMode::Unspecified
    }

    /// Handles a game event from `source_pos`.
    fn handle_game_event(
        &self,
        world: &Arc<World>,
        event: GameEventRef,
        context: &GameEventContext<'_>,
        source_pos: DVec3,
    ) -> bool;
}

/// Shared game event listener handle.
pub type SharedGameEventListener = Arc<dyn GameEventListener>;

/// World-shared count of physical listener entries retained by chunk registries.
///
/// This is only a zero-listener dispatch fast path. Retained inaccessible chunks remain counted,
/// so stale positive values can cause harmless extra chunk lookups while zero always means that no
/// registry can contain a listener.
#[derive(Default)]
pub(crate) struct GameEventListenerCount {
    entries: AtomicUsize,
}

impl GameEventListenerCount {
    #[must_use]
    pub(crate) fn shared() -> Arc<Self> {
        Arc::new(Self::default())
    }

    #[must_use]
    pub(crate) fn has_any(&self) -> bool {
        self.entries.load(Ordering::Acquire) != 0
    }

    fn add(&self, amount: usize) {
        if amount == 0 {
            return;
        }
        let updated = self
            .entries
            .try_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                current.checked_add(amount)
            });
        assert!(updated.is_ok(), "game-event listener count overflowed");
    }

    fn remove(&self, amount: usize) {
        if amount == 0 {
            return;
        }
        let updated = self
            .entries
            .try_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                current.checked_sub(amount)
            });
        assert!(updated.is_ok(), "game-event listener count underflowed");
    }

    #[cfg(test)]
    fn get(&self) -> usize {
        self.entries.load(Ordering::Acquire)
    }
}

struct QueuedListener {
    listener: SharedGameEventListener,
    distance_sq: f64,
}

/// Per-event delivery state shared across all visited chunk registries.
pub(crate) struct GameEventDispatcher<'world, 'context> {
    world: &'world Arc<World>,
    event: GameEventRef,
    source_pos: DVec3,
    context: &'world GameEventContext<'context>,
    by_distance: Vec<QueuedListener>,
}

impl<'world, 'context> GameEventDispatcher<'world, 'context> {
    #[must_use]
    pub(crate) const fn new(
        world: &'world Arc<World>,
        event: GameEventRef,
        source_pos: DVec3,
        context: &'world GameEventContext<'context>,
    ) -> Self {
        Self {
            world,
            event,
            source_pos,
            context,
            by_distance: Vec::new(),
        }
    }

    /// Visits one accessible chunk's vertical registry range.
    pub(crate) fn visit_chunk(
        &mut self,
        storage: &GameEventListenerStorage,
        section_min_y: i32,
        section_max_y: i32,
    ) {
        storage.visit_in_range(self.source_pos, section_min_y, section_max_y, |queued| {
            if queued.listener.delivery_mode() == GameEventDeliveryMode::ByDistance {
                self.by_distance.push(queued);
            } else {
                let _ = queued.listener.handle_game_event(
                    self.world,
                    self.event,
                    self.context,
                    self.source_pos,
                );
            }
        });
    }

    /// Delivers listeners whose Vanilla mode orders them by exact source distance.
    pub(crate) fn finish(mut self) {
        // Stable sorting retains chunk/section/list order for Vanilla's equal-distance ties.
        self.by_distance
            .sort_by(|left, right| left.distance_sq.total_cmp(&right.distance_sq));
        for queued in self.by_distance {
            let _ = queued.listener.handle_game_event(
                self.world,
                self.event,
                self.context,
                self.source_pos,
            );
        }
    }
}

#[derive(Default)]
struct SectionListeners {
    listeners: Vec<SharedGameEventListener>,
    pending_additions: Vec<SharedGameEventListener>,
    pending_removals: Vec<SharedGameEventListener>,
    pending_removal_indices: Vec<usize>,
    processing_depth: usize,
}

struct SectionProcessingResult {
    removed: usize,
    detached: Vec<SharedGameEventListener>,
}

impl SectionListeners {
    fn register(&mut self, listener: SharedGameEventListener) {
        if self.processing_depth == 0 {
            self.listeners.push(listener);
            return;
        }
        self.pending_additions.push(listener);
    }

    fn unregister(
        &mut self,
        listener: &SharedGameEventListener,
    ) -> (bool, Option<SharedGameEventListener>) {
        if self.processing_depth == 0 {
            let Some(index) = self
                .listeners
                .iter()
                .position(|existing| Arc::ptr_eq(existing, listener))
            else {
                return (false, None);
            };
            return (true, Some(self.listeners.remove(index)));
        }

        let is_registered = contains_listener(&self.listeners, listener)
            || contains_listener(&self.pending_additions, listener);
        if !contains_listener(&self.pending_removals, listener) {
            self.pending_removals.push(Arc::clone(listener));
        }
        (is_registered, None)
    }

    const fn begin_processing(&mut self) {
        self.processing_depth += 1;
    }

    fn end_processing(&mut self) -> SectionProcessingResult {
        self.processing_depth -= 1;
        if self.processing_depth != 0 {
            return SectionProcessingResult {
                removed: 0,
                detached: Vec::new(),
            };
        }

        let mut removed = 0;
        let mut detached = Vec::new();
        self.pending_removal_indices.sort_unstable();
        self.pending_removal_indices.dedup();
        for index in self.pending_removal_indices.drain(..).rev() {
            if index < self.listeners.len() {
                detached.push(self.listeners.remove(index));
                removed += 1;
            }
        }

        self.listeners.append(&mut self.pending_additions);

        if !self.pending_removals.is_empty() {
            let pending_removals = mem::take(&mut self.pending_removals);
            let mut retained = Vec::with_capacity(self.listeners.len());
            for listener in self.listeners.drain(..) {
                if contains_listener(&pending_removals, &listener) {
                    detached.push(listener);
                    removed += 1;
                } else {
                    retained.push(listener);
                }
            }
            self.listeners = retained;
            detached.extend(pending_removals);
        }
        SectionProcessingResult { removed, detached }
    }

    fn is_empty(&self) -> bool {
        self.listeners.is_empty()
            && self.pending_additions.is_empty()
            && self.pending_removals.is_empty()
    }

    fn physical_len(&self) -> usize {
        self.listeners.len() + self.pending_additions.len()
    }
}

fn contains_listener(
    listeners: &[SharedGameEventListener],
    listener: &SharedGameEventListener,
) -> bool {
    listeners
        .iter()
        .any(|existing| Arc::ptr_eq(existing, listener))
}

struct SectionProcessingGuard<'a> {
    storage: &'a GameEventListenerStorage,
    section_y: i32,
}

impl Drop for SectionProcessingGuard<'_> {
    fn drop(&mut self) {
        self.storage.end_section_processing(self.section_y);
    }
}

/// Section-indexed game event listener storage owned by one full chunk.
///
/// Matching Vanilla's `LevelChunk`, the chunk position is supplied by the owner and only
/// section Y is indexed here. Keeping this storage attached to the retained chunk preserves
/// listener order across temporary Full-status demotion and revival.
pub struct GameEventListenerStorage {
    listeners_by_section: SyncMutex<FxHashMap<i32, SectionListeners>>,
    listener_count: Arc<GameEventListenerCount>,
}

impl Default for GameEventListenerStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for GameEventListenerStorage {
    fn drop(&mut self) {
        let retained = self
            .listeners_by_section
            .get_mut()
            .values()
            .map(SectionListeners::physical_len)
            .sum();
        self.listener_count.remove(retained);
    }
}

impl GameEventListenerStorage {
    /// Creates empty game event listener storage.
    #[must_use]
    pub fn new() -> Self {
        Self::with_count(GameEventListenerCount::shared())
    }

    /// Creates storage contributing to one world's zero-listener fast path.
    #[must_use]
    pub(crate) fn with_count(listener_count: Arc<GameEventListenerCount>) -> Self {
        Self {
            listeners_by_section: SyncMutex::new(FxHashMap::default()),
            listener_count,
        }
    }

    /// Registers `listener` in the chunk's `section_y` registry.
    pub fn register(&self, section_y: i32, listener: SharedGameEventListener) {
        // Count first so a concurrent zero check can only produce a harmless false positive.
        self.listener_count.add(1);
        let mut listeners_by_section = self.listeners_by_section.lock();
        listeners_by_section
            .entry(section_y)
            .or_default()
            .register(listener);
    }

    /// Unregisters `listener` from the chunk's `section_y` registry.
    pub fn unregister(&self, section_y: i32, listener: &SharedGameEventListener) -> bool {
        let (removed, detached) = {
            let mut listeners_by_section = self.listeners_by_section.lock();
            let Some(section_listeners) = listeners_by_section.get_mut(&section_y) else {
                return false;
            };

            let (removed, detached) = section_listeners.unregister(listener);
            if removed && section_listeners.processing_depth == 0 {
                self.listener_count.remove(1);
            }
            if section_listeners.is_empty() {
                listeners_by_section.remove(&section_y);
            }
            (removed, detached)
        };
        drop(detached);
        removed
    }

    /// Visits listeners in the requested vertical section range.
    ///
    /// The world dispatcher supplies chunks in Vanilla's X/Z order and performs final
    /// delivery-mode handling across all visited chunks.
    fn visit_in_range(
        &self,
        source_pos: DVec3,
        section_min_y: i32,
        section_max_y: i32,
        mut visit: impl FnMut(QueuedListener),
    ) {
        let source_block_pos = BlockPos::from(source_pos);
        for section_y in section_min_y..=section_max_y {
            let Some(_processing_guard) = self.begin_section_processing(section_y) else {
                continue;
            };

            let mut cursor = 0;
            while let Some(listener) = self.next_section_listener(section_y, &mut cursor) {
                let Some(listener_pos) = listener.listener_pos() else {
                    continue;
                };
                let block_distance_sq =
                    block_distance_sq(source_block_pos, BlockPos::from(listener_pos));
                let listener_radius = listener.listener_radius().max(0);
                let listener_radius_sq = i64::from(listener_radius) * i64::from(listener_radius);
                if block_distance_sq <= listener_radius_sq {
                    visit(QueuedListener {
                        listener,
                        distance_sq: exact_distance_sq(source_pos, listener_pos),
                    });
                }
            }
        }
    }

    #[cfg(test)]
    fn collect_in_range(&self, source_pos: DVec3, notification_radius: i32) -> Vec<QueuedListener> {
        let source_block_pos = BlockPos::from(source_pos);
        let section_min_y =
            SectionPos::block_to_section_coord(source_block_pos.y() - notification_radius.max(0));
        let section_max_y =
            SectionPos::block_to_section_coord(source_block_pos.y() + notification_radius.max(0));
        let mut in_range = Vec::new();
        self.visit_in_range(source_pos, section_min_y, section_max_y, |queued| {
            in_range.push(queued);
        });
        in_range
    }

    fn begin_section_processing(&self, section_y: i32) -> Option<SectionProcessingGuard<'_>> {
        let mut listeners_by_section = self.listeners_by_section.lock();
        let section_listeners = listeners_by_section.get_mut(&section_y)?;
        section_listeners.begin_processing();
        Some(SectionProcessingGuard {
            storage: self,
            section_y,
        })
    }

    fn next_section_listener(
        &self,
        section_y: i32,
        cursor: &mut usize,
    ) -> Option<SharedGameEventListener> {
        enum NextListener {
            End,
            Removed(SharedGameEventListener),
            Found(SharedGameEventListener),
        }

        loop {
            let next = {
                let mut listeners_by_section = self.listeners_by_section.lock();
                let section_listeners = listeners_by_section.get_mut(&section_y)?;
                if *cursor >= section_listeners.listeners.len() {
                    NextListener::End
                } else {
                    let index = *cursor;
                    *cursor += 1;
                    if section_listeners.pending_removal_indices.contains(&index) {
                        continue;
                    }
                    let listener = &section_listeners.listeners[index];
                    if let Some(removal_index) = section_listeners
                        .pending_removals
                        .iter()
                        .position(|pending| Arc::ptr_eq(pending, listener))
                    {
                        let removed = section_listeners
                            .pending_removals
                            .swap_remove(removal_index);
                        section_listeners.pending_removal_indices.push(index);
                        NextListener::Removed(removed)
                    } else {
                        NextListener::Found(Arc::clone(listener))
                    }
                }
            };
            match next {
                NextListener::End => return None,
                NextListener::Removed(listener) => drop(listener),
                NextListener::Found(listener) => return Some(listener),
            }
        }
    }

    fn end_section_processing(&self, section_y: i32) {
        let detached = {
            let mut listeners_by_section = self.listeners_by_section.lock();
            let Some(section_listeners) = listeners_by_section.get_mut(&section_y) else {
                return;
            };
            let result = section_listeners.end_processing();
            self.listener_count.remove(result.removed);
            if section_listeners.is_empty() {
                listeners_by_section.remove(&section_y);
            }
            result.detached
        };
        drop(detached);
    }
}

fn block_distance_sq(left: BlockPos, right: BlockPos) -> i64 {
    let dx = i64::from(left.x()) - i64::from(right.x());
    let dy = i64::from(left.y()) - i64::from(right.y());
    let dz = i64::from(left.z()) - i64::from(right.z());
    dx * dx + dy * dy + dz * dz
}

fn exact_distance_sq(left: DVec3, right: DVec3) -> f64 {
    let dx = left.x - right.x;
    let dy = left.y - right.y;
    let dz = left.z - right.z;
    dx * dx + dy * dy + dz * dz
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Weak};

    use glam::DVec3;
    use steel_registry::{
        game_events::GameEventRef, test_support::init_test_registry, vanilla_game_events,
    };
    use steel_utils::{BlockPos, SectionPos, locks::SyncMutex};

    use crate::behavior::init_behaviors;
    use crate::test_support::{fresh_test_world, insert_ready_full_chunk};
    use crate::world::World;
    use crate::world::game_event_context::GameEventContext;
    use crate::world::game_event_listener::{
        GameEventDeliveryMode, GameEventDispatcher, GameEventListener, GameEventListenerCount,
        GameEventListenerStorage, SharedGameEventListener,
    };

    struct FixedListener {
        pos: DVec3,
        radius: i32,
    }

    struct RecordingListener {
        pos: DVec3,
        id: u8,
        delivery_mode: GameEventDeliveryMode,
        events: Arc<SyncMutex<Vec<u8>>>,
    }

    struct ReentrantDropListener {
        storage: Weak<GameEventListenerStorage>,
        self_listener: Weak<ReentrantDropListener>,
        replacement: SharedGameEventListener,
        section_y: i32,
    }

    impl GameEventListener for ReentrantDropListener {
        fn listener_pos(&self) -> Option<DVec3> {
            Some(DVec3::new(0.0, 64.0, 0.0))
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
            let Some(storage) = self.storage.upgrade() else {
                return false;
            };
            let Some(listener) = self.self_listener.upgrade() else {
                return false;
            };
            let listener: SharedGameEventListener = listener;
            storage.unregister(self.section_y, &listener)
        }
    }

    impl Drop for ReentrantDropListener {
        fn drop(&mut self) {
            if let Some(storage) = self.storage.upgrade() {
                storage.register(self.section_y, Arc::clone(&self.replacement));
            }
        }
    }

    impl GameEventListener for RecordingListener {
        fn listener_pos(&self) -> Option<DVec3> {
            Some(self.pos)
        }

        fn listener_radius(&self) -> i32 {
            16
        }

        fn delivery_mode(&self) -> GameEventDeliveryMode {
            self.delivery_mode
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

    impl GameEventListener for FixedListener {
        fn listener_pos(&self) -> Option<DVec3> {
            Some(self.pos)
        }

        fn listener_radius(&self) -> i32 {
            self.radius
        }

        fn handle_game_event(
            &self,
            _world: &Arc<World>,
            _event: GameEventRef,
            _context: &GameEventContext<'_>,
            _source_pos: DVec3,
        ) -> bool {
            false
        }
    }

    #[test]
    fn collect_in_range_filters_by_listener_radius() {
        let storage = GameEventListenerStorage::new();
        let near: SharedGameEventListener = Arc::new(FixedListener {
            pos: DVec3::new(2.0, 64.0, 0.0),
            radius: 16,
        });
        let far: SharedGameEventListener = Arc::new(FixedListener {
            pos: DVec3::new(15.0, 64.0, 0.0),
            radius: 4,
        });

        storage.register(
            SectionPos::from_block_pos(BlockPos::new(2, 64, 0)).y(),
            Arc::clone(&near),
        );
        storage.register(
            SectionPos::from_block_pos(BlockPos::new(15, 64, 0)).y(),
            Arc::clone(&far),
        );

        let matches = storage.collect_in_range(DVec3::new(0.5, 64.5, 0.5), 64);

        assert_eq!(matches.len(), 1);
        assert!(Arc::ptr_eq(&matches[0].listener, &near));
    }

    #[test]
    fn unregister_removes_empty_section_bucket() {
        let storage = GameEventListenerStorage::new();
        let listener: SharedGameEventListener = Arc::new(FixedListener {
            pos: DVec3::new(0.0, 64.0, 0.0),
            radius: 16,
        });
        let section_pos = SectionPos::new(0, 4, 0);

        storage.register(section_pos.y(), Arc::clone(&listener));

        assert!(storage.unregister(section_pos.y(), &listener));
        assert!(
            storage
                .collect_in_range(DVec3::new(0.5, 64.5, 0.5), 16)
                .is_empty()
        );
    }

    #[test]
    fn duplicate_registrations_are_removed_one_at_a_time() {
        let storage = GameEventListenerStorage::new();
        let listener: SharedGameEventListener = Arc::new(FixedListener {
            pos: DVec3::new(0.0, 64.0, 0.0),
            radius: 16,
        });
        let section_y = SectionPos::block_to_section_coord(64);
        storage.register(section_y, Arc::clone(&listener));
        storage.register(section_y, Arc::clone(&listener));

        assert_eq!(
            storage
                .collect_in_range(DVec3::new(0.5, 64.5, 0.5), 16)
                .len(),
            2
        );
        assert!(storage.unregister(section_y, &listener));
        assert_eq!(
            storage
                .collect_in_range(DVec3::new(0.5, 64.5, 0.5), 16)
                .len(),
            1
        );
    }

    #[test]
    fn repeated_deferred_unregister_decrements_physical_count_once() {
        let listener_count = GameEventListenerCount::shared();
        let storage = GameEventListenerStorage::with_count(Arc::clone(&listener_count));
        let listener: SharedGameEventListener = Arc::new(FixedListener {
            pos: DVec3::new(0.0, 64.0, 0.0),
            radius: 16,
        });
        let section_y = SectionPos::block_to_section_coord(64);
        storage.register(section_y, Arc::clone(&listener));
        storage.register(section_y, Arc::clone(&listener));
        assert_eq!(listener_count.get(), 2);

        let mut requested = false;
        storage.visit_in_range(DVec3::new(0.5, 64.5, 0.5), section_y, section_y, |_| {
            if requested {
                return;
            }
            requested = true;
            assert!(storage.unregister(section_y, &listener));
            assert!(storage.unregister(section_y, &listener));
        });

        assert!(requested);
        assert_eq!(listener_count.get(), 1);
        assert_eq!(
            storage
                .collect_in_range(DVec3::new(0.5, 64.5, 0.5), 16)
                .len(),
            1
        );
    }

    #[test]
    fn canceling_pending_addition_restores_physical_count() {
        let listener_count = GameEventListenerCount::shared();
        let storage = GameEventListenerStorage::with_count(Arc::clone(&listener_count));
        let existing: SharedGameEventListener = Arc::new(FixedListener {
            pos: DVec3::new(0.0, 64.0, 0.0),
            radius: 16,
        });
        let added: SharedGameEventListener = Arc::new(FixedListener {
            pos: DVec3::new(1.0, 64.0, 0.0),
            radius: 16,
        });
        let section_y = SectionPos::block_to_section_coord(64);
        storage.register(section_y, Arc::clone(&existing));

        storage.visit_in_range(DVec3::new(0.5, 64.5, 0.5), section_y, section_y, |_| {
            storage.register(section_y, Arc::clone(&added));
            assert_eq!(listener_count.get(), 2);
            assert!(storage.unregister(section_y, &added));
        });

        assert_eq!(listener_count.get(), 1);
        let matches = storage.collect_in_range(DVec3::new(0.5, 64.5, 0.5), 16);
        assert_eq!(matches.len(), 1);
        assert!(Arc::ptr_eq(&matches[0].listener, &existing));
    }

    #[test]
    fn dropping_retained_registry_releases_physical_count() {
        let listener_count = GameEventListenerCount::shared();
        {
            let storage = GameEventListenerStorage::with_count(Arc::clone(&listener_count));
            let listener: SharedGameEventListener = Arc::new(FixedListener {
                pos: DVec3::new(0.0, 64.0, 0.0),
                radius: 16,
            });
            let section_y = SectionPos::block_to_section_coord(64);
            storage.register(section_y, Arc::clone(&listener));
            storage.register(section_y, listener);
            assert_eq!(listener_count.get(), 2);
        }
        assert_eq!(listener_count.get(), 0);
    }

    #[test]
    fn collect_in_range_records_exact_distance_for_delivery_sorting() {
        let storage = GameEventListenerStorage::new();
        let listener: SharedGameEventListener = Arc::new(FixedListener {
            pos: DVec3::new(0.1, 64.5, 0.5),
            radius: 16,
        });

        storage.register(
            SectionPos::from_block_pos(BlockPos::new(0, 64, 0)).y(),
            Arc::clone(&listener),
        );

        let matches = storage.collect_in_range(DVec3::new(0.9, 64.5, 0.5), 16);

        assert_eq!(matches.len(), 1);
        assert!((matches[0].distance_sq - 0.64).abs() < f64::EPSILON);
    }

    #[test]
    fn visit_in_range_skips_listener_unregistered_before_turn() {
        let storage = GameEventListenerStorage::new();
        let section_pos = SectionPos::from_block_pos(BlockPos::new(0, 64, 0));
        let first: SharedGameEventListener = Arc::new(FixedListener {
            pos: DVec3::new(0.0, 64.0, 0.0),
            radius: 16,
        });
        let second: SharedGameEventListener = Arc::new(FixedListener {
            pos: DVec3::new(1.0, 64.0, 0.0),
            radius: 16,
        });
        let third: SharedGameEventListener = Arc::new(FixedListener {
            pos: DVec3::new(2.0, 64.0, 0.0),
            radius: 16,
        });

        storage.register(section_pos.y(), Arc::clone(&first));
        storage.register(section_pos.y(), Arc::clone(&second));
        storage.register(section_pos.y(), Arc::clone(&third));

        let mut visited = Vec::new();
        storage.visit_in_range(DVec3::new(0.5, 64.5, 0.5), 3, 5, |queued| {
            if Arc::ptr_eq(&queued.listener, &first) {
                assert!(storage.unregister(section_pos.y(), &second));
                visited.push(1);
            } else if Arc::ptr_eq(&queued.listener, &second) {
                visited.push(2);
            } else if Arc::ptr_eq(&queued.listener, &third) {
                visited.push(3);
            }
        });

        assert_eq!(visited, [1, 3]);
        let matches = storage.collect_in_range(DVec3::new(0.5, 64.5, 0.5), 16);
        assert_eq!(matches.len(), 2);
        assert!(
            matches
                .iter()
                .all(|queued| { !Arc::ptr_eq(&queued.listener, &second) })
        );
    }

    #[test]
    fn visit_in_range_defers_same_section_registration_until_next_visit() {
        let storage = GameEventListenerStorage::new();
        let section_pos = SectionPos::from_block_pos(BlockPos::new(0, 64, 0));
        let first: SharedGameEventListener = Arc::new(FixedListener {
            pos: DVec3::new(0.0, 64.0, 0.0),
            radius: 16,
        });
        let added: SharedGameEventListener = Arc::new(FixedListener {
            pos: DVec3::new(1.0, 64.0, 0.0),
            radius: 16,
        });

        storage.register(section_pos.y(), Arc::clone(&first));

        let mut visited = Vec::new();
        storage.visit_in_range(DVec3::new(0.5, 64.5, 0.5), 3, 5, |queued| {
            if Arc::ptr_eq(&queued.listener, &first) {
                storage.register(section_pos.y(), Arc::clone(&added));
                visited.push(1);
            } else if Arc::ptr_eq(&queued.listener, &added) {
                visited.push(2);
            }
        });

        assert_eq!(visited, [1]);
        let matches = storage.collect_in_range(DVec3::new(0.5, 64.5, 0.5), 16);
        assert_eq!(matches.len(), 2);
        assert!(
            matches
                .iter()
                .any(|queued| { Arc::ptr_eq(&queued.listener, &added) })
        );
    }

    #[test]
    fn visit_in_range_allows_listener_to_unregister_itself() {
        let storage = GameEventListenerStorage::new();
        let section_pos = SectionPos::from_block_pos(BlockPos::new(0, 64, 0));
        let listener: SharedGameEventListener = Arc::new(FixedListener {
            pos: DVec3::new(0.0, 64.0, 0.0),
            radius: 16,
        });

        storage.register(section_pos.y(), Arc::clone(&listener));

        let mut visited = 0;
        storage.visit_in_range(DVec3::new(0.5, 64.5, 0.5), 3, 5, |queued| {
            if Arc::ptr_eq(&queued.listener, &listener) {
                assert!(storage.unregister(section_pos.y(), &listener));
                visited += 1;
            }
        });

        assert_eq!(visited, 1);
        assert!(
            storage
                .collect_in_range(DVec3::new(0.5, 64.5, 0.5), 16)
                .is_empty()
        );
    }

    #[test]
    fn deferred_listener_destruction_runs_without_storage_lock() {
        init_test_registry();
        init_behaviors();
        let world = fresh_test_world("listener_drop_reentrancy");
        let storage = Arc::new(GameEventListenerStorage::new());
        let section_y = SectionPos::block_to_section_coord(64);
        let replacement: SharedGameEventListener = Arc::new(FixedListener {
            pos: DVec3::new(1.0, 64.0, 0.0),
            radius: 16,
        });
        let listener = Arc::new_cyclic(|self_listener| ReentrantDropListener {
            storage: Arc::downgrade(&storage),
            self_listener: self_listener.clone(),
            replacement: Arc::clone(&replacement),
            section_y,
        });
        let listener: SharedGameEventListener = listener;
        storage.register(section_y, listener);

        let context = GameEventContext::default();
        let mut dispatcher = GameEventDispatcher::new(
            &world,
            &vanilla_game_events::BLOCK_CHANGE,
            DVec3::new(0.5, 64.5, 0.5),
            &context,
        );
        dispatcher.visit_chunk(&storage, section_y, section_y);
        dispatcher.finish();

        let matches = storage.collect_in_range(DVec3::new(0.5, 64.5, 0.5), 16);
        assert_eq!(matches.len(), 1);
        assert!(Arc::ptr_eq(&matches[0].listener, &replacement));
    }

    #[test]
    fn world_dispatch_uses_vanilla_chunk_order_not_registration_order() {
        init_test_registry();
        init_behaviors();
        let world = fresh_test_world("game_event_chunk_order");
        insert_ready_full_chunk(&world, steel_utils::ChunkPos::new(-1, 0));
        insert_ready_full_chunk(&world, steel_utils::ChunkPos::new(0, 0));
        let events = Arc::new(SyncMutex::new(Vec::new()));
        let left_pos = DVec3::new(-0.5, 64.5, 0.5);
        let right_pos = DVec3::new(0.5, 64.5, 0.5);
        let left: SharedGameEventListener = Arc::new(RecordingListener {
            pos: left_pos,
            id: 1,
            delivery_mode: GameEventDeliveryMode::Unspecified,
            events: Arc::clone(&events),
        });
        let right: SharedGameEventListener = Arc::new(RecordingListener {
            pos: right_pos,
            id: 2,
            delivery_mode: GameEventDeliveryMode::Unspecified,
            events: Arc::clone(&events),
        });

        world.register_game_event_listener(
            SectionPos::from_block_pos(BlockPos::from(right_pos)),
            right,
        );
        world.register_game_event_listener(
            SectionPos::from_block_pos(BlockPos::from(left_pos)),
            left,
        );
        world.game_event_at(
            &vanilla_game_events::BLOCK_CHANGE,
            DVec3::new(0.0, 64.5, 0.5),
            &GameEventContext::default(),
        );

        assert_eq!(*events.lock(), [1, 2]);
    }

    #[test]
    fn equal_distance_delivery_keeps_vanilla_traversal_order() {
        init_test_registry();
        init_behaviors();
        let world = fresh_test_world("equal_distance_game_event_order");
        insert_ready_full_chunk(&world, steel_utils::ChunkPos::new(-1, 0));
        insert_ready_full_chunk(&world, steel_utils::ChunkPos::new(0, 0));
        let events = Arc::new(SyncMutex::new(Vec::new()));
        let left_pos = DVec3::new(-0.5, 64.5, 0.5);
        let right_pos = DVec3::new(0.5, 64.5, 0.5);
        let left: SharedGameEventListener = Arc::new(RecordingListener {
            pos: left_pos,
            id: 1,
            delivery_mode: GameEventDeliveryMode::ByDistance,
            events: Arc::clone(&events),
        });
        let right: SharedGameEventListener = Arc::new(RecordingListener {
            pos: right_pos,
            id: 2,
            delivery_mode: GameEventDeliveryMode::ByDistance,
            events: Arc::clone(&events),
        });

        world.register_game_event_listener(
            SectionPos::from_block_pos(BlockPos::from(right_pos)),
            right,
        );
        world.register_game_event_listener(
            SectionPos::from_block_pos(BlockPos::from(left_pos)),
            left,
        );
        world.game_event_at(
            &vanilla_game_events::BLOCK_CHANGE,
            DVec3::new(0.0, 64.5, 0.5),
            &GameEventContext::default(),
        );

        assert_eq!(*events.lock(), [1, 2]);
    }
}
