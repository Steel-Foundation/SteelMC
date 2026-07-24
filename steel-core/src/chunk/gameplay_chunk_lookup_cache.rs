//! Scoped holder cache for synchronous gameplay chunk lookups.
//!
//! Scopes may only cover intervals where active-map membership is stable. The
//! cache retains holder identity (including map absence), but never generation
//! permission, published status, or chunk guards; those remain live per lookup.

use std::{cell::RefCell, marker::PhantomData, ptr, rc::Rc, sync::Arc};

use steel_utils::ChunkPos;

use super::{chunk_holder::ChunkHolder, chunk_map::ChunkMap};

// Vanilla's ServerChunkCache also keeps four recent synchronous lookups. Steel
// caches holders instead of ChunkAccess so concurrent status publication stays visible.
// Unlike Vanilla's insertion-only ordering, hits are promoted here because one
// holder entry serves every requested status; the cache telemetry validates that choice.
const CACHE_ENTRY_COUNT: usize = 4;

/// Lookup statistics collected without synchronization inside one cache scope.
#[derive(Debug, Default)]
pub struct GameplayChunkLookupCacheStats {
    /// Lookups served by a cached active holder.
    pub holder_hits: usize,
    /// Lookups served by a cached active-map absence.
    pub missing_hits: usize,
    /// Cache misses that consulted the active SCC map.
    pub scc_lookups: usize,
    /// Lookups for another chunk map while this scope was active.
    pub foreign_map_bypasses: usize,
    /// Least-recently-used entries displaced from a full cache.
    pub evictions: usize,
}

#[derive(PartialEq, Eq)]
struct CacheOwner(*const ());

impl CacheOwner {
    const fn for_chunk_map(chunk_map: &ChunkMap) -> Self {
        Self(ptr::from_ref(chunk_map).cast())
    }

    #[cfg(test)]
    const fn for_test<T>(owner: &T) -> Self {
        Self(ptr::from_ref(owner).cast())
    }
}

struct CacheEntry {
    pos: ChunkPos,
    holder: Option<Arc<ChunkHolder>>,
}

struct ActiveCache {
    owner: CacheOwner,
    entries: [Option<CacheEntry>; CACHE_ENTRY_COUNT],
    stats: GameplayChunkLookupCacheStats,
}

enum CacheEntryProbe {
    Hit(Option<Arc<ChunkHolder>>),
    Miss,
}

impl ActiveCache {
    fn new(owner: CacheOwner) -> Self {
        Self {
            owner,
            entries: [const { None }; CACHE_ENTRY_COUNT],
            stats: GameplayChunkLookupCacheStats::default(),
        }
    }

    #[inline]
    fn lookup(&mut self, pos: ChunkPos) -> CacheEntryProbe {
        let Some(index) = self
            .entries
            .iter()
            .position(|entry| entry.as_ref().is_some_and(|entry| entry.pos == pos))
        else {
            return CacheEntryProbe::Miss;
        };
        let holder = self.entries[index]
            .as_ref()
            .and_then(|entry| entry.holder.as_ref().map(Arc::clone));
        if holder.is_some() {
            self.stats.holder_hits += 1;
        } else {
            self.stats.missing_hits += 1;
        }
        self.promote(index);
        CacheEntryProbe::Hit(holder)
    }

    fn insert(&mut self, pos: ChunkPos, holder: Option<Arc<ChunkHolder>>) {
        if let Some(index) = self
            .entries
            .iter()
            .position(|entry| entry.as_ref().is_some_and(|entry| entry.pos == pos))
        {
            self.entries[index] = Some(CacheEntry { pos, holder });
            self.promote(index);
            return;
        }

        if self.entries[CACHE_ENTRY_COUNT - 1].is_some() {
            self.stats.evictions += 1;
        }
        for index in (1..CACHE_ENTRY_COUNT).rev() {
            self.entries[index] = self.entries[index - 1].take();
        }
        self.entries[0] = Some(CacheEntry { pos, holder });
    }

    fn promote(&mut self, index: usize) {
        if index == 0 {
            return;
        }
        let entry = self.entries[index].take();
        for target in (1..=index).rev() {
            self.entries[target] = self.entries[target - 1].take();
        }
        self.entries[0] = entry;
    }
}

thread_local! {
    static ACTIVE_CACHE: RefCell<Option<ActiveCache>> = const { RefCell::new(None) };
}

enum CacheProbe {
    Hit(Option<Arc<ChunkHolder>>),
    Miss,
    Bypass,
}

/// Installs an empty cache until the scope is finished or dropped.
///
/// Nested scopes restore the prior cache, and the owned holder references are
/// released on every exit path, including unwinding. Nested guards must exit in
/// the usual last-in, first-out order.
pub(crate) struct GameplayChunkLookupCacheScope<'map> {
    previous: Option<ActiveCache>,
    active: bool,
    _chunk_map: PhantomData<&'map ChunkMap>,
    _thread_bound: PhantomData<Rc<()>>,
}

impl<'map> GameplayChunkLookupCacheScope<'map> {
    pub(crate) fn enter(chunk_map: &'map ChunkMap) -> Self {
        Self::enter_key(CacheOwner::for_chunk_map(chunk_map))
    }

    #[cfg(test)]
    fn enter_owner<T>(owner: &'map T) -> Self {
        Self::enter_key(CacheOwner::for_test(owner))
    }

    fn enter_key(owner: CacheOwner) -> Self {
        let previous = ACTIVE_CACHE.with(|cache| cache.replace(Some(ActiveCache::new(owner))));
        Self {
            previous,
            active: true,
            _chunk_map: PhantomData,
            _thread_bound: PhantomData,
        }
    }

    pub(crate) fn finish(mut self) -> GameplayChunkLookupCacheStats {
        self.restore()
            .map_or_else(GameplayChunkLookupCacheStats::default, |cache| cache.stats)
    }

    fn restore(&mut self) -> Option<ActiveCache> {
        if !self.active {
            return None;
        }
        self.active = false;
        ACTIVE_CACHE.with(|cache| cache.replace(self.previous.take()))
    }
}

impl Drop for GameplayChunkLookupCacheScope<'_> {
    fn drop(&mut self) {
        drop(self.restore());
    }
}

#[inline]
pub(crate) fn lookup_or_insert_with<F>(
    chunk_map: &ChunkMap,
    pos: ChunkPos,
    load: F,
) -> Option<Arc<ChunkHolder>>
where
    F: FnOnce() -> Option<Arc<ChunkHolder>>,
{
    lookup_or_insert_for_owner(CacheOwner::for_chunk_map(chunk_map), pos, load)
}

#[inline]
fn lookup_or_insert_for_owner<F>(
    owner: CacheOwner,
    pos: ChunkPos,
    load: F,
) -> Option<Arc<ChunkHolder>>
where
    F: FnOnce() -> Option<Arc<ChunkHolder>>,
{
    let probe = ACTIVE_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        let Some(cache) = cache.as_mut() else {
            return CacheProbe::Bypass;
        };
        if cache.owner != owner {
            cache.stats.foreign_map_bypasses += 1;
            return CacheProbe::Bypass;
        }
        match cache.lookup(pos) {
            CacheEntryProbe::Hit(holder) => return CacheProbe::Hit(holder),
            CacheEntryProbe::Miss => {}
        }
        cache.stats.scc_lookups += 1;
        CacheProbe::Miss
    });

    match probe {
        CacheProbe::Hit(holder) => holder,
        CacheProbe::Bypass => load(),
        CacheProbe::Miss => {
            let holder = load();
            ACTIVE_CACHE.with(|cache| {
                let mut cache = cache.borrow_mut();
                let Some(cache) = cache.as_mut() else {
                    return;
                };
                if cache.owner == owner {
                    cache.insert(pos, holder.as_ref().map(Arc::clone));
                }
            });
            holder
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::chunk_ticket_manager::ChunkTicketLevel;

    fn holder(pos: ChunkPos) -> Arc<ChunkHolder> {
        Arc::new(ChunkHolder::new(
            pos,
            ChunkTicketLevel::FULL_CHUNK,
            None,
            0,
            16,
        ))
    }

    #[test]
    fn four_entry_cache_uses_most_recently_used_eviction() {
        let owner = 0_u8;
        let scope = GameplayChunkLookupCacheScope::enter_owner(&owner);
        let holders = (0..=4)
            .map(|x| holder(ChunkPos::new(x, 0)))
            .collect::<Vec<_>>();
        let mut loads = 0;

        for (x, holder) in holders.iter().take(4).enumerate() {
            let loaded = lookup_or_insert_for_owner(
                CacheOwner::for_test(&owner),
                ChunkPos::new(x as i32, 0),
                || {
                    loads += 1;
                    Some(Arc::clone(holder))
                },
            );
            drop(loaded);
        }
        drop(lookup_or_insert_for_owner(
            CacheOwner::for_test(&owner),
            ChunkPos::new(0, 0),
            || panic!("the most-recently-used entry should hit"),
        ));
        drop(lookup_or_insert_for_owner(
            CacheOwner::for_test(&owner),
            ChunkPos::new(4, 0),
            || {
                loads += 1;
                Some(Arc::clone(&holders[4]))
            },
        ));
        drop(lookup_or_insert_for_owner(
            CacheOwner::for_test(&owner),
            ChunkPos::new(0, 0),
            || panic!("the promoted entry should remain cached"),
        ));
        drop(lookup_or_insert_for_owner(
            CacheOwner::for_test(&owner),
            ChunkPos::new(1, 0),
            || {
                loads += 1;
                Some(Arc::clone(&holders[1]))
            },
        ));

        let stats = scope.finish();
        assert_eq!(loads, 6);
        assert_eq!(stats.holder_hits, 2);
        assert_eq!(stats.scc_lookups, 6);
        assert_eq!(stats.evictions, 2);
    }

    #[test]
    fn missing_holder_is_cached_within_scope() {
        let owner = 0_u8;
        let scope = GameplayChunkLookupCacheScope::enter_owner(&owner);
        let pos = ChunkPos::new(3, -7);
        let mut loads = 0;

        assert!(
            lookup_or_insert_for_owner(CacheOwner::for_test(&owner), pos, || {
                loads += 1;
                None
            })
            .is_none()
        );
        assert!(
            lookup_or_insert_for_owner(CacheOwner::for_test(&owner), pos, || {
                panic!("a cached missing holder should not reload")
            })
            .is_none()
        );

        let stats = scope.finish();
        assert_eq!(loads, 1);
        assert_eq!(stats.missing_hits, 1);
        assert_eq!(stats.scc_lookups, 1);
    }

    #[test]
    fn nested_scope_restores_outer_entries_and_releases_holders() {
        let outer_owner = 0_u8;
        let inner_owner = 1_u8;
        let pos = ChunkPos::new(2, 5);
        let holder = holder(pos);
        let outer = GameplayChunkLookupCacheScope::enter_owner(&outer_owner);

        drop(lookup_or_insert_for_owner(
            CacheOwner::for_test(&outer_owner),
            pos,
            || Some(Arc::clone(&holder)),
        ));
        assert_eq!(Arc::strong_count(&holder), 2);

        let inner = GameplayChunkLookupCacheScope::enter_owner(&inner_owner);
        assert!(
            lookup_or_insert_for_owner(
                CacheOwner::for_test(&inner_owner),
                ChunkPos::new(-1, -1),
                || None,
            )
            .is_none()
        );
        let inner_stats = inner.finish();
        assert_eq!(inner_stats.scc_lookups, 1);

        drop(lookup_or_insert_for_owner(
            CacheOwner::for_test(&outer_owner),
            pos,
            || panic!("the outer entry should be restored"),
        ));
        let outer_stats = outer.finish();
        assert_eq!(outer_stats.holder_hits, 1);
        assert_eq!(Arc::strong_count(&holder), 1);
    }

    #[test]
    fn dropping_scope_releases_entries_and_next_scope_starts_empty() {
        let owner = 0_u8;
        let pos = ChunkPos::new(-6, 11);
        let holder = holder(pos);

        {
            let _scope = GameplayChunkLookupCacheScope::enter_owner(&owner);
            drop(lookup_or_insert_for_owner(
                CacheOwner::for_test(&owner),
                pos,
                || Some(Arc::clone(&holder)),
            ));
            assert_eq!(Arc::strong_count(&holder), 2);
        }
        assert_eq!(Arc::strong_count(&holder), 1);

        let scope = GameplayChunkLookupCacheScope::enter_owner(&owner);
        let mut loads = 0;
        drop(lookup_or_insert_for_owner(
            CacheOwner::for_test(&owner),
            pos,
            || {
                loads += 1;
                Some(Arc::clone(&holder))
            },
        ));
        let stats = scope.finish();
        assert_eq!(loads, 1);
        assert_eq!(stats.scc_lookups, 1);
    }

    #[test]
    fn foreign_owner_bypasses_active_cache() {
        let owner = 0_u8;
        let foreign_owner = 1_u8;
        let scope = GameplayChunkLookupCacheScope::enter_owner(&owner);
        let pos = ChunkPos::new(8, 9);
        let holder = holder(pos);
        let mut loads = 0;

        for _ in 0..2 {
            drop(lookup_or_insert_for_owner(
                CacheOwner::for_test(&foreign_owner),
                pos,
                || {
                    loads += 1;
                    Some(Arc::clone(&holder))
                },
            ));
        }

        let stats = scope.finish();
        assert_eq!(loads, 2);
        assert_eq!(stats.foreign_map_bypasses, 2);
        assert_eq!(stats.scc_lookups, 0);
    }
}
