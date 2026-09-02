//! Shared storage and persistence for loot-backed block containers.

mod exploration;

use std::{str::FromStr, sync::Arc};

use simdnbt::borrow::NbtCompound as NbtCompoundView;
use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use steel_registry::{
    REGISTRY, RegistryExt,
    data_components::{
        DataComponentMap, MapId,
        vanilla_components::{CONTAINER_LOOT, MAP_ID, SeededContainerLoot},
    },
    item_stack::ItemStack,
    loot_table::{
        ExplorationMapRequest, ExplorationMapResolver, LootContext, LootError, LootResult,
    },
    vanilla_attributes, vanilla_items,
};
use steel_utils::{Downcast as _, DowncastType, DowncastTypeKey, Identifier};
use text_components::TextComponent;

use crate::block_entity::BlockEntityComponentInput;
use crate::block_entity::base_container::BaseContainer;
use crate::entity::{Entity as _, LivingEntity as _, entity_loot_ref};
use crate::inventory::container::{
    Container, ContainerAccessContext, ContainerAccessResult, ContainerPreparation,
    ContainerPreparationTask, ContainerReadiness,
};
use crate::inventory::lock::ContainerRef;
use crate::player::Player;

use self::exploration::ExplorationMapJob;

const PENDING_LOOT_KEY: &str = "steel:pending_loot";
const PENDING_MAPS_KEY: &str = "Maps";
const FALLBACK_ITEM_KEY: &str = "Fallback";

/// Inventory data shared by Vanilla randomizable block containers.
pub(crate) struct RandomizableContainer {
    base: BaseContainer,
    loot_table: Option<Identifier>,
    loot_table_seed: i64,
    pending_loot: Option<PendingLoot>,
    running_token: Option<u64>,
    next_token: u64,
    removed: bool,
}

#[derive(Clone)]
pub(super) struct PendingExplorationMap {
    pub(super) marker: i32,
    pub(super) request: ExplorationMapRequest,
    fallback: ItemStack,
}

pub(super) struct PendingLoot {
    pub(super) items: Vec<ItemStack>,
    pub(super) maps: Vec<PendingExplorationMap>,
}

enum PreparationSource {
    LootTable {
        key: Identifier,
        seed: i64,
        size: usize,
    },
    Pending {
        maps: Vec<PendingExplorationMap>,
    },
}

struct RandomizableContainerPreparation {
    token: u64,
    source: PreparationSource,
}

struct EvaluatedLoot {
    items: Vec<ItemStack>,
    maps: Vec<PendingExplorationMap>,
}

#[derive(Default)]
struct PendingMapResolver {
    maps: Vec<PendingExplorationMap>,
}

// SAFETY: This Steel-owned key uniquely identifies the concrete storage shared
// by randomizable block-container implementations.
unsafe impl DowncastType for RandomizableContainer {
    const TYPE_KEY: DowncastTypeKey =
        DowncastTypeKey::new("steel:container/randomizable_block_entity");
}

impl RandomizableContainer {
    #[must_use]
    pub(crate) fn new(size: usize) -> Self {
        Self {
            base: BaseContainer::new(size),
            loot_table: None,
            loot_table_seed: 0,
            pending_loot: None,
            running_token: None,
            next_token: 0,
            removed: false,
        }
    }

    pub(crate) fn load(&mut self, nbt: &NbtCompoundView<'_, '_>) {
        self.base.load_metadata(nbt);
        self.loot_table = nbt
            .string("LootTable")
            .and_then(|value| value.to_str().parse().ok());
        self.loot_table_seed = nbt.long("LootTableSeed").unwrap_or(0);
        self.pending_loot = None;
        self.running_token = None;
        self.removed = false;

        if self.loot_table.is_some() {
            self.base.clear_items();
            if let Some(pending) = nbt.compound(PENDING_LOOT_KEY) {
                self.pending_loot = Self::load_pending_loot(&pending, self.base.items().len());
                if self.pending_loot.is_none() {
                    log::error!("Discarding malformed pending container loot state");
                }
            }
        } else {
            self.base.load_items(nbt);
        }
    }

    pub(crate) fn save(&self, nbt: &mut NbtCompound) {
        self.base.save_metadata(nbt);
        if let Some(loot_table) = &self.loot_table {
            nbt.insert("LootTable", loot_table.to_string());
            if self.loot_table_seed != 0 {
                nbt.insert("LootTableSeed", self.loot_table_seed);
            }
            if let Some(pending) = &self.pending_loot {
                nbt.insert(PENDING_LOOT_KEY, Self::save_pending_loot(pending));
            }
            return;
        }
        self.base.save_items(nbt);
    }

    /// Marks this storage removed and takes any already-realized items.
    pub(crate) fn remove_and_take_ready_items(&mut self) -> Vec<ItemStack> {
        self.removed = true;
        if self.loot_table.is_some() || self.pending_loot.is_some() {
            Vec::new()
        } else {
            self.base.take_items()
        }
    }

    #[must_use]
    pub(crate) fn display_name(&self, default: TextComponent) -> TextComponent {
        self.base.display_name(default)
    }

    #[must_use]
    pub(crate) const fn has_custom_name(&self) -> bool {
        self.base.has_custom_name()
    }

    #[cfg(test)]
    fn has_lock(&self) -> bool {
        self.base.has_lock()
    }

    /// Mirrors `RandomizableContainerBlockEntity.canOpen`: spectators may not
    /// unpack loot, and everyone else must satisfy the lock.
    #[must_use]
    pub(crate) fn can_open(&self, player: &Player, main_hand: &ItemStack) -> bool {
        (self.loot_table.is_none() || !player.is_spectator())
            && self.base.can_open(player, main_hand)
    }

    /// Mirrors `RandomizableContainerBlockEntity.applyImplicitComponents`.
    pub(crate) fn apply_implicit_components(&mut self, components: &BlockEntityComponentInput<'_>) {
        self.base.apply_implicit_components(components);
        if let Some(loot) = components.get(CONTAINER_LOOT) {
            self.loot_table = Some(loot.loot_table().clone());
            self.loot_table_seed = loot.seed();
            self.pending_loot = None;
            self.running_token = None;
        }
    }

    /// Mirrors `RandomizableContainerBlockEntity.collectImplicitComponents`.
    pub(crate) fn collect_implicit_components(&self, components: &mut DataComponentMap) {
        self.base.collect_implicit_components(components);
        if let Some(loot_table) = &self.loot_table {
            components.set(
                CONTAINER_LOOT,
                Some(SeededContainerLoot::new(
                    loot_table.clone(),
                    self.loot_table_seed,
                )),
            );
        }
    }

    /// Mirrors `RandomizableContainerBlockEntity.removeComponentsFromTag`.
    ///
    /// Steel's in-progress exploration-map state is dropped as well because the
    /// `CONTAINER_LOOT` component re-rolls the table when the item is placed.
    pub(crate) fn remove_components_from_tag(nbt: &mut NbtCompound) {
        BaseContainer::remove_components_from_tag(nbt);
        nbt.remove("LootTable");
        nbt.remove("LootTableSeed");
        nbt.remove(PENDING_LOOT_KEY);
    }

    #[cfg(test)]
    const fn has_pending_loot(&self) -> bool {
        self.loot_table.is_some() || self.pending_loot.is_some()
    }

    const fn next_preparation_token(&mut self) -> u64 {
        self.next_token = self.next_token.wrapping_add(1);
        if self.next_token == 0 {
            self.next_token = 1;
        }
        self.next_token
    }

    fn evaluate_loot_table(
        context: &ContainerAccessContext,
        loot_table_key: &Identifier,
        loot_table_seed: i64,
        size: usize,
    ) -> LootResult<EvaluatedLoot> {
        let Some(loot_table) = REGISTRY.loot_tables.by_key(loot_table_key) else {
            // Vanilla resolves an unknown table to LootTable.EMPTY.
            return Ok(EvaluatedLoot {
                items: vec![ItemStack::empty(); size],
                maps: Vec::new(),
            });
        };
        loot_table.requirements()?;

        let luck = context.player().map_or(0.0, |player| {
            player
                .attributes()
                .lock()
                .get_value(vanilla_attributes::LUCK)
                .unwrap_or(0.0) as f32
        });
        // TODO: Trigger `GENERATE_LOOT` once Steel has advancement criteria.
        context.world().try_with_loot_random(
            loot_table_seed,
            loot_table.random_sequence.as_ref(),
            |random| {
                let mut resolver = PendingMapResolver::default();
                let mut items = vec![ItemStack::empty(); size];
                {
                    let mut loot_context = LootContext::new(random)
                        .with_origin(
                            f64::from(context.pos().x()) + 0.5,
                            f64::from(context.pos().y()) + 0.5,
                            f64::from(context.pos().z()) + 0.5,
                        )
                        .with_level(context.world())
                        .with_exploration_maps(&mut resolver);
                    if let Some(player) = context.player() {
                        loot_context = loot_context
                            .with_luck(luck)
                            .with_this_entity(entity_loot_ref(player));
                    }
                    loot_table.fill(&mut items, &mut loot_context)?;
                }
                Ok(EvaluatedLoot {
                    items,
                    maps: resolver.maps,
                })
            },
        )
    }

    fn load_pending_loot(nbt: &NbtCompoundView<'_, '_>, size: usize) -> Option<PendingLoot> {
        let items = BaseContainer::items_from_nbt(nbt, size);
        let compounds = nbt.list(PENDING_MAPS_KEY)?.compounds()?;
        let mut maps = Vec::new();
        for compound in compounds {
            let marker = compound.int("Marker")?;
            if marker >= 0
                || maps
                    .iter()
                    .any(|map: &PendingExplorationMap| map.marker == marker)
            {
                return None;
            }
            let destination =
                Identifier::from_str(&compound.string("Destination")?.to_str()).ok()?;
            let decoration = Identifier::from_str(&compound.string("Decoration")?.to_str()).ok()?;
            let fallback =
                ItemStack::from_borrowed_compound(&compound.compound(FALLBACK_ITEM_KEY)?)?;
            maps.push(PendingExplorationMap {
                marker,
                request: ExplorationMapRequest {
                    destination,
                    decoration,
                    zoom: compound.int("Zoom")?,
                    search_radius: compound.int("SearchRadius")?,
                    skip_existing_chunks: compound.byte("SkipExistingChunks")? != 0,
                },
                fallback,
            });
        }
        if maps.is_empty()
            || !maps.iter().all(|map| {
                items
                    .iter()
                    .filter(|item| item.get(MAP_ID).is_some_and(|id| id.id() == map.marker))
                    .count()
                    == 1
            })
        {
            return None;
        }
        Some(PendingLoot { items, maps })
    }

    fn save_pending_loot(pending: &PendingLoot) -> NbtCompound {
        let mut nbt = NbtCompound::new();
        BaseContainer::save_item_slice(&mut nbt, &pending.items);
        let maps = pending
            .maps
            .iter()
            .filter_map(|map| {
                let NbtTag::Compound(fallback) = map.fallback.to_nbt_tag_ref() else {
                    return None;
                };
                let mut encoded = NbtCompound::new();
                encoded.insert("Marker", map.marker);
                encoded.insert("Destination", map.request.destination.to_string());
                encoded.insert("Decoration", map.request.decoration.to_string());
                encoded.insert("Zoom", map.request.zoom);
                encoded.insert("SearchRadius", map.request.search_radius);
                encoded.insert(
                    "SkipExistingChunks",
                    i8::from(map.request.skip_existing_chunks),
                );
                encoded.insert(FALLBACK_ITEM_KEY, fallback);
                Some(encoded)
            })
            .collect();
        nbt.insert(PENDING_MAPS_KEY, NbtList::Compound(maps));
        nbt
    }

    pub(super) fn is_running(&self, token: u64) -> bool {
        self.running_token == Some(token)
    }

    pub(super) fn pending_markers_are_valid(&self, token: u64) -> bool {
        self.is_running(token)
            && self.pending_loot.as_ref().is_some_and(|pending| {
                pending.maps.iter().all(|map| {
                    pending
                        .items
                        .iter()
                        .filter(|item| item.get(MAP_ID).is_some_and(|id| id.id() == map.marker))
                        .count()
                        == 1
                })
            })
    }

    pub(super) fn reset_running(&mut self, token: u64) {
        if self.is_running(token) {
            self.running_token = None;
        }
    }

    pub(super) fn restore_fallback_item(
        placeholder: &ItemStack,
        pending_map: &PendingExplorationMap,
    ) -> ItemStack {
        let mut fallback = pending_map
            .fallback
            .copy_with_count(pending_map.fallback.count());
        let mut later_changes = placeholder.components_patch().clone();
        later_changes.clear(MAP_ID);
        fallback.apply_components_patch(&later_changes);
        fallback
    }

    pub(super) fn finish_removed_as_fallback(&mut self, token: u64) -> Vec<ItemStack> {
        if !self.removed || !self.is_running(token) {
            self.reset_running(token);
            return Vec::new();
        }
        let Some(mut pending) = self.pending_loot.take() else {
            self.running_token = None;
            return Vec::new();
        };
        for map in &pending.maps {
            let Some(slot) = pending
                .items
                .iter()
                .position(|item| item.get(MAP_ID).is_some_and(|id| id.id() == map.marker))
            else {
                continue;
            };
            pending.items[slot] = Self::restore_fallback_item(&pending.items[slot], map);
        }
        self.loot_table = None;
        self.running_token = None;
        pending.items
    }
}

impl Container for RandomizableContainer {
    fn begin_prepare_access(&mut self) -> ContainerPreparation {
        if self.loot_table.is_none() {
            return ContainerPreparation::Ready { changed: false };
        }
        if self.running_token.is_some() {
            return ContainerPreparation::Pending;
        }

        let token = self.next_preparation_token();
        self.running_token = Some(token);
        let source = if let Some(pending) = &self.pending_loot {
            PreparationSource::Pending {
                maps: pending.maps.clone(),
            }
        } else {
            let Some(key) = self.loot_table.as_ref() else {
                self.running_token = None;
                return ContainerPreparation::Ready { changed: false };
            };
            PreparationSource::LootTable {
                key: key.clone(),
                seed: self.loot_table_seed,
                size: self.base.items().len(),
            }
        };
        ContainerPreparation::Start(Box::new(RandomizableContainerPreparation { token, source }))
    }

    fn preparation_readiness(&self) -> ContainerReadiness {
        if self.loot_table.is_none() {
            ContainerReadiness::Ready
        } else if self.running_token.is_some() {
            ContainerReadiness::Pending
        } else {
            ContainerReadiness::NeedsPreparation
        }
    }

    fn items(&self) -> &[ItemStack] {
        self.base.items()
    }

    fn items_mut(&mut self) -> &mut [ItemStack] {
        self.base.items_mut()
    }

    fn set_item(&mut self, slot: usize, stack: ItemStack) {
        self.base.set_item(slot, stack);
    }

    fn set_changed(&mut self) {}
}

impl ExplorationMapResolver for PendingMapResolver {
    fn resolve(
        &mut self,
        request: &ExplorationMapRequest,
        original: &ItemStack,
    ) -> LootResult<Option<ItemStack>> {
        let index =
            i32::try_from(self.maps.len()).map_err(|_| LootError::TooManyExplorationMaps)?;
        let marker = -1_i32
            .checked_sub(index)
            .ok_or(LootError::TooManyExplorationMaps)?;
        let mut placeholder = ItemStack::new(&vanilla_items::FILLED_MAP);
        placeholder.set(MAP_ID, MapId::new(marker));
        self.maps.push(PendingExplorationMap {
            marker,
            request: request.clone(),
            fallback: original.copy_with_count(original.count()),
        });
        Ok(Some(placeholder))
    }
}

impl ContainerPreparationTask for RandomizableContainerPreparation {
    fn start(
        self: Box<Self>,
        container: ContainerRef,
        context: ContainerAccessContext,
    ) -> ContainerAccessResult {
        let maps = match self.source {
            PreparationSource::Pending { maps } => maps,
            PreparationSource::LootTable { key, seed, size } => {
                let evaluated =
                    match RandomizableContainer::evaluate_loot_table(&context, &key, seed, size) {
                        Ok(evaluated) => evaluated,
                        Err(error) => {
                            container.with_locked_mut(|storage| {
                                if let Some(storage) =
                                    storage.downcast_mut::<RandomizableContainer>()
                                {
                                    storage.reset_running(self.token);
                                }
                            });
                            log::error!("Failed to unpack container loot table {key}: {error}");
                            return ContainerAccessResult::Failed;
                        }
                    };
                if evaluated.maps.is_empty() {
                    let mut dropped = Vec::new();
                    let published = container.with_locked_mut(|storage| {
                        let Some(storage) = storage.downcast_mut::<RandomizableContainer>() else {
                            return false;
                        };
                        if !storage.is_running(self.token) {
                            return false;
                        }
                        if storage.base.replace_items(evaluated.items).is_err() {
                            storage.reset_running(self.token);
                            return false;
                        }
                        storage.loot_table = None;
                        storage.pending_loot = None;
                        storage.running_token = None;
                        if storage.removed {
                            dropped = storage.base.take_items();
                        }
                        true
                    });
                    if !published {
                        return ContainerAccessResult::Failed;
                    }
                    if dropped.is_empty() {
                        container.notify_owner_changed();
                    } else {
                        for item in dropped {
                            context.world.drop_item_stack(context.pos, item);
                        }
                    }
                    return ContainerAccessResult::Ready;
                }

                let maps = evaluated.maps.clone();
                let installed = container.with_locked_mut(|storage| {
                    let Some(storage) = storage.downcast_mut::<RandomizableContainer>() else {
                        return false;
                    };
                    if !storage.is_running(self.token) {
                        return false;
                    }
                    storage.pending_loot = Some(PendingLoot {
                        items: evaluated.items,
                        maps: evaluated.maps,
                    });
                    true
                });
                if !installed {
                    return ContainerAccessResult::Failed;
                }
                container.notify_owner_changed();
                maps
            }
        };

        let job = ExplorationMapJob::new(
            Arc::clone(&context.world),
            context.pos,
            container.clone(),
            self.token,
            maps,
        );
        if context.world.spawn_server_job(job) {
            return ContainerAccessResult::Pending;
        }

        let dropped = container.with_locked_mut(|storage| {
            storage
                .downcast_mut::<RandomizableContainer>()
                .map_or_else(Vec::new, |storage| {
                    storage.finish_removed_as_fallback(self.token)
                })
        });
        for item in dropped {
            context.world.drop_item_stack(context.pos, item);
        }
        log::error!("Cannot continue explorer-map generation without a server job queue");
        ContainerAccessResult::Failed
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io::Cursor,
        sync::{Arc, Weak},
    };

    use simdnbt::borrow::read_compound as read_borrowed_compound;
    use steel_registry::{
        data_components::vanilla_components::{CUSTOM_NAME, ITEM_NAME},
        test_support::init_test_registry,
        vanilla_blocks, vanilla_entities, vanilla_items,
    };
    use steel_utils::{BlockPos, ChunkPos, WorldAabb, types::UpdateFlags};

    use crate::{
        behavior::init_behaviors,
        block_entity::init_block_entities,
        entity::entities::ItemEntity,
        inventory::lock::ContainerLockGuard,
        server::{Server, jobs::ServerJobQueue},
        test_support::{fresh_test_world, insert_ready_full_chunk},
    };

    use super::*;

    #[test]
    fn realized_items_round_trip_with_vanilla_slot_indices() {
        init_test_registry();
        let mut source = RandomizableContainer::new(27);
        source.set_item(17, ItemStack::with_count(&vanilla_items::STONE, 23));
        let mut saved = NbtCompound::new();
        source.save(&mut saved);

        let mut bytes = Vec::new();
        saved.write(&mut bytes);
        let borrowed = read_borrowed_compound(&mut Cursor::new(bytes.as_slice()))
            .expect("test container NBT should reborrow");
        let view = NbtCompoundView::from(&borrowed);
        let mut loaded = RandomizableContainer::new(27);
        loaded.load(&view);

        assert!(loaded.get_item(17).is(&vanilla_items::STONE));
        assert_eq!(loaded.get_item(17).count(), 23);
        assert!(loaded.get_item(0).is_empty());
    }

    #[test]
    fn pending_loot_round_trip_suppresses_realized_items() {
        init_test_registry();
        let mut loot_nbt = NbtCompound::new();
        loot_nbt.insert("LootTable", "minecraft:chests/simple_dungeon");
        loot_nbt.insert("LootTableSeed", 42_i64);
        let mut bytes = Vec::new();
        loot_nbt.write(&mut bytes);
        let borrowed = read_borrowed_compound(&mut Cursor::new(bytes.as_slice()))
            .expect("test loot NBT should reborrow");
        let view = NbtCompoundView::from(&borrowed);

        let mut container = RandomizableContainer::new(27);
        container.set_item(0, ItemStack::new(&vanilla_items::STONE));
        container.load(&view);
        let mut saved = NbtCompound::new();
        container.save(&mut saved);

        assert!(container.has_pending_loot());
        assert_eq!(
            saved.string("LootTable").map(ToString::to_string),
            Some("minecraft:chests/simple_dungeon".to_owned())
        );
        assert_eq!(saved.long("LootTableSeed"), Some(42));
        assert!(saved.list("Items").is_none());
    }

    #[test]
    fn direct_inventory_access_unpacks_pending_loot() {
        init_test_registry();
        init_behaviors();
        init_block_entities();
        let world = fresh_test_world("pending_loot_inventory_access");
        let pos = BlockPos::new(3, 64, 3);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));
        assert!(world.set_block(
            pos,
            vanilla_blocks::CHEST.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));
        let Some(block_entity) = world.get_block_entity(pos) else {
            panic!("chest placement should create its block entity");
        };

        let mut loot_nbt = NbtCompound::new();
        loot_nbt.insert("LootTable", "minecraft:chests/simple_dungeon");
        loot_nbt.insert("LootTableSeed", 42_i64);
        let mut bytes = Vec::new();
        loot_nbt.write(&mut bytes);
        let borrowed = read_borrowed_compound(&mut Cursor::new(bytes.as_slice()))
            .expect("test loot NBT should reborrow");
        block_entity.load_additional(&borrowed);

        let Some(container_ref) = block_entity.container_ref() else {
            panic!("chest should expose its inventory");
        };
        assert_eq!(
            container_ref.prepare_access(None),
            ContainerAccessResult::Ready
        );
        let container_id = container_ref.container_id();
        let guard = ContainerLockGuard::lock_all(&[&container_ref]);
        let Some(container) = guard.get_typed::<RandomizableContainer>(container_id) else {
            panic!("chest should use randomizable container storage");
        };

        assert!(!container.has_pending_loot());
        assert!(container.items().iter().any(|stack| !stack.is_empty()));
    }

    #[test]
    fn exploration_map_loot_defers_then_uses_vanilla_fallback_when_no_structure_exists() {
        init_test_registry();
        init_behaviors();
        init_block_entities();
        let world = fresh_test_world("pending_exploration_map");
        let jobs = Arc::new(ServerJobQueue::new());
        world.bind_server_jobs(Arc::downgrade(&jobs));
        let pos = BlockPos::new(3, 64, 3);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));
        assert!(world.set_block(
            pos,
            vanilla_blocks::CHEST.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));
        let Some(block_entity) = world.get_block_entity(pos) else {
            panic!("chest placement should create its block entity");
        };
        let mut loot_nbt = NbtCompound::new();
        loot_nbt.insert("LootTable", "minecraft:chests/shipwreck_map");
        loot_nbt.insert("LootTableSeed", 42_i64);
        let mut bytes = Vec::new();
        loot_nbt.write(&mut bytes);
        let borrowed = read_borrowed_compound(&mut Cursor::new(bytes.as_slice()))
            .expect("test loot NBT should reborrow");
        block_entity.load_additional(&borrowed);
        let Some(container_ref) = block_entity.container_ref() else {
            panic!("chest should expose its inventory");
        };

        assert_eq!(
            container_ref.prepare_access(None),
            ContainerAccessResult::Pending
        );
        assert_eq!(jobs.len(), 1);
        let stats = jobs.tick(Weak::<Server>::new(), 0, true);
        assert_eq!(stats.finished, 1);
        assert_eq!(
            container_ref.preparation_readiness(),
            ContainerReadiness::Ready
        );

        let guard = ContainerLockGuard::lock_all(&[&container_ref]);
        let Some(container) =
            guard.get_typed::<RandomizableContainer>(container_ref.container_id())
        else {
            panic!("chest should use randomizable container storage");
        };
        let fallback = container
            .items()
            .iter()
            .find(|item| item.is(&vanilla_items::MAP));
        assert!(
            fallback.is_some(),
            "missing structures should retain the map item"
        );
        assert!(
            container
                .items()
                .iter()
                .all(|item| { item.get(MAP_ID).is_none_or(|map_id| map_id.id() >= 0) })
        );
    }

    #[test]
    fn generated_exploration_map_work_round_trips_without_replaying_loot_rng() {
        init_test_registry();
        init_behaviors();
        init_block_entities();
        let world = fresh_test_world("persist_pending_exploration_map");
        let jobs = Arc::new(ServerJobQueue::new());
        world.bind_server_jobs(Arc::downgrade(&jobs));
        let pos = BlockPos::new(3, 64, 3);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));
        assert!(world.set_block(
            pos,
            vanilla_blocks::CHEST.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));
        let Some(block_entity) = world.get_block_entity(pos) else {
            panic!("chest placement should create its block entity");
        };
        let mut loot_nbt = NbtCompound::new();
        loot_nbt.insert("LootTable", "minecraft:chests/shipwreck_map");
        loot_nbt.insert("LootTableSeed", 42_i64);
        let mut bytes = Vec::new();
        loot_nbt.write(&mut bytes);
        let borrowed = read_borrowed_compound(&mut Cursor::new(bytes.as_slice()))
            .expect("test loot NBT should reborrow");
        block_entity.load_additional(&borrowed);
        let Some(container_ref) = block_entity.container_ref() else {
            panic!("chest should expose its inventory");
        };
        assert_eq!(
            container_ref.prepare_access(None),
            ContainerAccessResult::Pending
        );

        let mut saved = NbtCompound::new();
        block_entity.save_additional(&mut saved);
        let mut saved_bytes = Vec::new();
        saved.write(&mut saved_bytes);
        let saved_borrowed = read_borrowed_compound(&mut Cursor::new(saved_bytes.as_slice()))
            .expect("pending loot NBT should reborrow");
        let saved_view = NbtCompoundView::from(&saved_borrowed);
        let mut loaded = RandomizableContainer::new(27);
        loaded.load(&saved_view);

        let Some(pending) = loaded.pending_loot.as_ref() else {
            panic!("generated pending loot should survive a chunk save");
        };
        assert_eq!(pending.maps.len(), 1);
        assert!(
            pending
                .items
                .iter()
                .any(|item| { item.get(MAP_ID).is_some_and(|map_id| map_id.id() < 0) })
        );
        assert_eq!(
            loaded.preparation_readiness(),
            ContainerReadiness::NeedsPreparation
        );
    }

    #[test]
    fn exploration_map_branches_preserve_vanilla_counts_and_components() {
        init_test_registry();
        let mut original = ItemStack::with_count(&vanilla_items::MAP, 3);
        original.set(CUSTOM_NAME, TextComponent::plain("Before"));
        let request = ExplorationMapRequest {
            destination: Identifier::vanilla_static("on_treasure_maps"),
            decoration: Identifier::vanilla_static("red_x"),
            zoom: 1,
            search_radius: 50,
            skip_existing_chunks: false,
        };
        let mut resolver = PendingMapResolver::default();
        let Some(mut placeholder) = resolver
            .resolve(&request, &original)
            .expect("placeholder creation should succeed")
        else {
            panic!("exploration resolver should return a placeholder");
        };
        assert_eq!(placeholder.count(), 1);
        assert!(placeholder.get(CUSTOM_NAME).is_none());
        placeholder.set(ITEM_NAME, TextComponent::plain("After"));

        let fallback =
            RandomizableContainer::restore_fallback_item(&placeholder, &resolver.maps[0]);

        assert!(fallback.is(&vanilla_items::MAP));
        assert_eq!(fallback.count(), 3);
        assert_eq!(fallback.get(CUSTOM_NAME), original.get(CUSTOM_NAME));
        assert_eq!(
            fallback.get(ITEM_NAME),
            Some(&TextComponent::plain("After"))
        );
        assert!(fallback.get(MAP_ID).is_none());
    }

    #[test]
    fn destroying_unopened_chest_and_barrel_drops_generated_loot() {
        init_test_registry();
        init_behaviors();
        init_block_entities();
        let world = fresh_test_world("unopened_container_drops");
        let jobs = Arc::new(ServerJobQueue::new());
        world.bind_server_jobs(Arc::downgrade(&jobs));
        insert_ready_full_chunk(&world, ChunkPos::new(0, 0));

        for (pos, block, loot_table, loot_seed) in [
            (
                BlockPos::new(3, 64, 3),
                &vanilla_blocks::CHEST,
                "minecraft:chests/simple_dungeon",
                42_i64,
            ),
            (
                BlockPos::new(8, 64, 3),
                &vanilla_blocks::BARREL,
                "minecraft:chests/simple_dungeon",
                0_i64,
            ),
            (
                BlockPos::new(13, 64, 3),
                &vanilla_blocks::CHEST,
                "minecraft:chests/shipwreck_map",
                7_i64,
            ),
        ] {
            assert!(world.set_block(pos, block.default_state(), UpdateFlags::UPDATE_NONE));
            let Some(block_entity) = world.get_block_entity(pos) else {
                panic!("container placement should create its block entity");
            };

            let mut loot_nbt = NbtCompound::new();
            loot_nbt.insert("LootTable", loot_table);
            loot_nbt.insert("LootTableSeed", loot_seed);
            let mut bytes = Vec::new();
            loot_nbt.write(&mut bytes);
            let borrowed = read_borrowed_compound(&mut Cursor::new(bytes.as_slice()))
                .expect("test loot NBT should reborrow");
            block_entity.load_additional(&borrowed);

            assert!(world.set_block(
                pos,
                vanilla_blocks::AIR.default_state(),
                UpdateFlags::UPDATE_ALL,
            ));
            assert!(world.get_block_entity(pos).is_none());
            jobs.tick(Weak::<Server>::new(), 0, true);

            let min_x = f64::from(pos.x()) - 2.0;
            let min_y = f64::from(pos.y()) - 2.0;
            let min_z = f64::from(pos.z()) - 2.0;
            let dropped = world.get_entities_in_aabb_matching(
                &WorldAabb::new(min_x, min_y, min_z, min_x + 5.0, min_y + 5.0, min_z + 5.0),
                |entity| entity.entity_type() == &vanilla_entities::ITEM,
            );
            let generated_count = dropped
                .iter()
                .filter_map(|entity| entity.as_ref().downcast_ref::<ItemEntity>())
                .map(|entity| entity.get_item().count())
                .sum::<i32>();
            assert!(generated_count > 0, "{block:?} should drop generated loot");
        }
    }

    #[test]
    fn lock_predicate_round_trips_without_becoming_unlocked() {
        let mut predicate = NbtCompound::new();
        predicate.insert("count", 2_i32);
        let mut source = NbtCompound::new();
        source.insert("lock", predicate);
        let mut bytes = Vec::new();
        source.write(&mut bytes);
        let borrowed = read_borrowed_compound(&mut Cursor::new(bytes.as_slice()))
            .expect("test lock NBT should reborrow");
        let view = NbtCompoundView::from(&borrowed);

        let mut container = RandomizableContainer::new(27);
        container.load(&view);
        let mut saved = NbtCompound::new();
        container.save(&mut saved);

        assert!(container.has_lock());
        assert_eq!(
            saved.compound("lock").and_then(|lock| lock.int("count")),
            Some(2)
        );
    }

    #[test]
    fn explicit_empty_lock_round_trips_but_remains_unlocked() {
        let mut source = NbtCompound::new();
        source.insert("lock", NbtCompound::new());
        let mut bytes = Vec::new();
        source.write(&mut bytes);
        let borrowed = read_borrowed_compound(&mut Cursor::new(bytes.as_slice()))
            .expect("test empty lock NBT should reborrow");
        let view = NbtCompoundView::from(&borrowed);

        let mut container = RandomizableContainer::new(27);
        container.load(&view);
        let mut saved = NbtCompound::new();
        container.save(&mut saved);

        assert!(!container.has_lock());
        assert!(saved.compound("lock").is_some_and(NbtCompound::is_empty));
    }
}
