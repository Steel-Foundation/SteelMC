//! Game event listener registration and dispatch.

use std::sync::Arc;

use glam::DVec3;
use rustc_hash::FxHashMap;
use steel_registry::game_events::GameEventRef;
use steel_utils::locks::SyncMutex;
use steel_utils::{BlockPos, SectionPos};

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

struct QueuedListener {
    listener: SharedGameEventListener,
    distance_sq: f64,
}

#[derive(Default)]
struct SectionListeners {
    listeners: Vec<SharedGameEventListener>,
    pending_additions: Vec<SharedGameEventListener>,
    pending_removals: Vec<SharedGameEventListener>,
    processing_depth: usize,
}

impl SectionListeners {
    fn register(&mut self, listener: SharedGameEventListener) {
        if self.processing_depth == 0 {
            if !contains_listener(&self.listeners, &listener) {
                self.listeners.push(listener);
            }
            return;
        }

        if contains_listener(&self.listeners, &listener)
            && !contains_listener(&self.pending_removals, &listener)
        {
            return;
        }
        if !contains_listener(&self.pending_additions, &listener) {
            self.pending_additions.push(listener);
        }
    }

    fn unregister(&mut self, listener: &SharedGameEventListener) -> bool {
        if self.processing_depth == 0 {
            let old_len = self.listeners.len();
            self.listeners
                .retain(|existing| !Arc::ptr_eq(existing, listener));
            return self.listeners.len() != old_len;
        }

        let is_registered = contains_listener(&self.listeners, listener)
            || contains_listener(&self.pending_additions, listener);
        if !is_registered {
            return false;
        }

        if !contains_listener(&self.pending_removals, listener) {
            self.pending_removals.push(Arc::clone(listener));
        }
        true
    }

    const fn begin_processing(&mut self) {
        self.processing_depth += 1;
    }

    fn end_processing(&mut self) {
        self.processing_depth -= 1;
        if self.processing_depth != 0 {
            return;
        }

        for listener in self.pending_additions.drain(..) {
            if !contains_listener(&self.listeners, &listener) {
                self.listeners.push(listener);
            }
        }

        if !self.pending_removals.is_empty() {
            self.listeners
                .retain(|listener| !contains_listener(&self.pending_removals, listener));
            self.pending_removals.clear();
        }
    }

    fn is_empty(&self) -> bool {
        self.listeners.is_empty()
            && self.pending_additions.is_empty()
            && self.pending_removals.is_empty()
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
    section_pos: SectionPos,
}

impl Drop for SectionProcessingGuard<'_> {
    fn drop(&mut self) {
        self.storage.end_section_processing(self.section_pos);
    }
}

/// Section-indexed game event listener storage.
#[derive(Default)]
pub struct GameEventListenerStorage {
    listeners_by_section: SyncMutex<FxHashMap<SectionPos, SectionListeners>>,
}

impl GameEventListenerStorage {
    /// Creates empty game event listener storage.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers `listener` in `section_pos`.
    pub fn register(&self, section_pos: SectionPos, listener: SharedGameEventListener) {
        let mut listeners_by_section = self.listeners_by_section.lock();
        listeners_by_section
            .entry(section_pos)
            .or_default()
            .register(listener);
    }

    /// Unregisters `listener` from `section_pos`.
    pub fn unregister(&self, section_pos: SectionPos, listener: &SharedGameEventListener) -> bool {
        let mut listeners_by_section = self.listeners_by_section.lock();
        let Some(section_listeners) = listeners_by_section.get_mut(&section_pos) else {
            return false;
        };

        let removed = section_listeners.unregister(listener);
        if section_listeners.is_empty() {
            listeners_by_section.remove(&section_pos);
        }

        removed
    }

    /// Dispatches `event` to listeners in range and returns the handled count.
    pub fn dispatch(
        &self,
        world: &Arc<World>,
        event: GameEventRef,
        source_pos: DVec3,
        context: &GameEventContext<'_>,
    ) -> usize {
        let mut by_distance = Vec::new();
        let mut handled = 0;

        self.visit_in_range(source_pos, event.notification_radius, |queued| {
            if queued.listener.delivery_mode() == GameEventDeliveryMode::ByDistance {
                by_distance.push(queued);
            } else if queued
                .listener
                .handle_game_event(world, event, context, source_pos)
            {
                handled += 1;
            }
        });

        by_distance.sort_by(|left, right| left.distance_sq.total_cmp(&right.distance_sq));
        for queued in by_distance {
            if queued
                .listener
                .handle_game_event(world, event, context, source_pos)
            {
                handled += 1;
            }
        }

        handled
    }

    #[cfg(test)]
    fn collect_in_range(&self, source_pos: DVec3, notification_radius: i32) -> Vec<QueuedListener> {
        let mut in_range = Vec::new();
        self.visit_in_range(source_pos, notification_radius, |queued| {
            in_range.push(queued);
        });
        in_range
    }

    fn visit_in_range(
        &self,
        source_pos: DVec3,
        notification_radius: i32,
        mut visit: impl FnMut(QueuedListener),
    ) {
        let notification_radius = notification_radius.max(0);
        let source_block_pos = BlockPos::from(source_pos);
        let section_min_x =
            SectionPos::block_to_section_coord(source_block_pos.x() - notification_radius);
        let section_min_y =
            SectionPos::block_to_section_coord(source_block_pos.y() - notification_radius);
        let section_min_z =
            SectionPos::block_to_section_coord(source_block_pos.z() - notification_radius);
        let section_max_x =
            SectionPos::block_to_section_coord(source_block_pos.x() + notification_radius);
        let section_max_y =
            SectionPos::block_to_section_coord(source_block_pos.y() + notification_radius);
        let section_max_z =
            SectionPos::block_to_section_coord(source_block_pos.z() + notification_radius);

        for section_x in section_min_x..=section_max_x {
            for section_z in section_min_z..=section_max_z {
                for section_y in section_min_y..=section_max_y {
                    let section_pos = SectionPos::new(section_x, section_y, section_z);
                    let Some(_processing_guard) = self.begin_section_processing(section_pos) else {
                        continue;
                    };

                    let mut cursor = 0;
                    while let Some(listener) = self.next_section_listener(section_pos, &mut cursor)
                    {
                        let Some(listener_pos) = listener.listener_pos() else {
                            continue;
                        };
                        let block_distance_sq =
                            block_distance_sq(source_block_pos, BlockPos::from(listener_pos));
                        let listener_radius = listener.listener_radius().max(0);
                        let listener_radius_sq =
                            i64::from(listener_radius) * i64::from(listener_radius);
                        if block_distance_sq <= listener_radius_sq {
                            visit(QueuedListener {
                                listener,
                                distance_sq: exact_distance_sq(source_pos, listener_pos),
                            });
                        }
                    }
                }
            }
        }
    }

    fn begin_section_processing(
        &self,
        section_pos: SectionPos,
    ) -> Option<SectionProcessingGuard<'_>> {
        let mut listeners_by_section = self.listeners_by_section.lock();
        let section_listeners = listeners_by_section.get_mut(&section_pos)?;
        section_listeners.begin_processing();
        Some(SectionProcessingGuard {
            storage: self,
            section_pos,
        })
    }

    fn next_section_listener(
        &self,
        section_pos: SectionPos,
        cursor: &mut usize,
    ) -> Option<SharedGameEventListener> {
        let listeners_by_section = self.listeners_by_section.lock();
        let section_listeners = listeners_by_section.get(&section_pos)?;
        while *cursor < section_listeners.listeners.len() {
            let listener = Arc::clone(&section_listeners.listeners[*cursor]);
            *cursor += 1;
            if contains_listener(&section_listeners.pending_removals, &listener) {
                continue;
            }
            return Some(listener);
        }
        None
    }

    fn end_section_processing(&self, section_pos: SectionPos) {
        let mut listeners_by_section = self.listeners_by_section.lock();
        let Some(section_listeners) = listeners_by_section.get_mut(&section_pos) else {
            return;
        };
        section_listeners.end_processing();
        if section_listeners.is_empty() {
            listeners_by_section.remove(&section_pos);
        }
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
    use std::sync::Arc;

    use glam::DVec3;
    use steel_registry::game_events::GameEventRef;
    use steel_utils::{BlockPos, SectionPos};

    use crate::world::World;
    use crate::world::game_event_context::GameEventContext;
    use crate::world::game_event_listener::{
        GameEventListener, GameEventListenerStorage, SharedGameEventListener,
    };

    struct FixedListener {
        pos: DVec3,
        radius: i32,
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
            pos: DVec3::new(32.0, 64.0, 0.0),
            radius: 16,
        });

        storage.register(
            SectionPos::from_block_pos(BlockPos::new(2, 64, 0)),
            Arc::clone(&near),
        );
        storage.register(
            SectionPos::from_block_pos(BlockPos::new(32, 64, 0)),
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

        storage.register(section_pos, Arc::clone(&listener));

        assert!(storage.unregister(section_pos, &listener));
        assert!(
            storage
                .collect_in_range(DVec3::new(0.5, 64.5, 0.5), 16)
                .is_empty()
        );
    }

    #[test]
    fn collect_in_range_records_exact_distance_for_delivery_sorting() {
        let storage = GameEventListenerStorage::new();
        let listener: SharedGameEventListener = Arc::new(FixedListener {
            pos: DVec3::new(0.1, 64.5, 0.5),
            radius: 16,
        });

        storage.register(
            SectionPos::from_block_pos(BlockPos::new(0, 64, 0)),
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

        storage.register(section_pos, Arc::clone(&first));
        storage.register(section_pos, Arc::clone(&second));
        storage.register(section_pos, Arc::clone(&third));

        let mut visited = Vec::new();
        storage.visit_in_range(DVec3::new(0.5, 64.5, 0.5), 16, |queued| {
            if Arc::ptr_eq(&queued.listener, &first) {
                assert!(storage.unregister(section_pos, &second));
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

        storage.register(section_pos, Arc::clone(&first));

        let mut visited = Vec::new();
        storage.visit_in_range(DVec3::new(0.5, 64.5, 0.5), 16, |queued| {
            if Arc::ptr_eq(&queued.listener, &first) {
                storage.register(section_pos, Arc::clone(&added));
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

        storage.register(section_pos, Arc::clone(&listener));

        let mut visited = 0;
        storage.visit_in_range(DVec3::new(0.5, 64.5, 0.5), 16, |queued| {
            if Arc::ptr_eq(&queued.listener, &listener) {
                assert!(storage.unregister(section_pos, &listener));
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
}
