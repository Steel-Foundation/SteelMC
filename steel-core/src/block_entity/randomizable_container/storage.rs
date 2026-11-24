use std::{
    mem,
    sync::{Arc, Weak},
};

use simdnbt::ToNbtTag;
use simdnbt::borrow::{BaseNbtCompound as BorrowedNbtCompound, NbtCompound as NbtCompoundView};
use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use steel_registry::data_components::vanilla_components::{
    CONTAINER, CONTAINER_LOOT, SeededContainerLoot,
};
use steel_registry::item_stack::ItemStack;
use steel_registry::{RegistryExt, block_entity_type::BlockEntityTypeRef};
use steel_utils::{BlockPos, BlockStateId, DowncastType, DowncastTypeKey, locks::SyncMutex};

use crate::block_entity::{BlockEntityBase, ImplicitComponentGetter, RandomizableContainer};
use crate::entity::LivingEntity;
use crate::inventory::container::{Container, load_all_items};
use crate::inventory::lock::{ContainerRef, SharedContainer};
use crate::loot::{
    LOOT_TABLES, LootCompletion, LootEntitySnapshot, LootParams, OperationError, OperationResult,
};
use crate::player::Player;
use crate::world::World;

/// Shared inventory, pending loot, and persistence for randomizable block containers.
pub(crate) struct RandomizableContainerStorage {
    pub(crate) base: Arc<BlockEntityBase>,
    pub(crate) container: Arc<SyncMutex<RandomizableContainerInventory>>,
    pub(crate) container_ref: ContainerRef,
}

pub(crate) struct RandomizableContainerInventory {
    items: Vec<ItemStack>,
    loot_table: Option<SeededContainerLoot>,
    generation: u64,
    pending: Option<LootCompletion>,
}

// SAFETY: This key is owned by Steel and uniquely identifies the independently
// lockable inventory data used by randomizable block containers.
unsafe impl DowncastType for RandomizableContainerInventory {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:container/randomizable");
}

impl RandomizableContainerStorage {
    /// Creates independent inventory storage owned by one concrete block entity.
    #[must_use]
    pub(crate) fn new(
        entity_type: BlockEntityTypeRef,
        slots: usize,
        level: Weak<World>,
        pos: BlockPos,
        state: BlockStateId,
    ) -> Self {
        let base = Arc::new(BlockEntityBase::new(entity_type, level, pos, state));
        let container = Arc::new(SyncMutex::new(RandomizableContainerInventory {
            items: vec![ItemStack::empty(); slots],
            loot_table: None,
            generation: 0,
            pending: None,
        }));
        let shared_container: SharedContainer =
            Arc::<SyncMutex<RandomizableContainerInventory>>::clone(&container);
        let loot_base = Arc::clone(&base);
        let loot_container = Arc::clone(&container);
        Self {
            container_ref: ContainerRef::owned_by_block_entity(shared_container, Arc::clone(&base))
                .with_access_callback(Arc::new(move || {
                    Self::unpack(&loot_base, &loot_container, None)
                })),
            base,
            container,
        }
    }

    pub(crate) fn pre_remove_side_effects(&self, pos: BlockPos) {
        let Some(world) = self.base.level() else {
            Self::take_items(&self.container);
            return;
        };
        match self.unpack_loot_table(None) {
            Ok(()) => Self::drop_items(&world, &self.container, pos),
            Err(OperationError::Pending(completion)) => {
                let storage = Arc::clone(&self.container);
                let drop_world = Arc::clone(&world);
                completion.on_complete(move |result| {
                    if result.is_ok() {
                        Self::drop_items(&drop_world, &storage, pos);
                    }
                });
            }
            Err(error) => {
                log::error!("Could not unpack container loot at {pos:?} before removal: {error}");
            }
        }
    }

    fn take_items(storage: &SyncMutex<RandomizableContainerInventory>) -> Vec<ItemStack> {
        let mut container = storage.lock();
        let slots = container.items.len();
        mem::replace(&mut container.items, vec![ItemStack::empty(); slots])
    }

    fn drop_items(
        world: &Arc<World>,
        storage: &SyncMutex<RandomizableContainerInventory>,
        pos: BlockPos,
    ) {
        for item in Self::take_items(storage) {
            world.drop_item_stack(pos, item);
        }
    }

    pub(crate) fn load_additional(&self, nbt: &BorrowedNbtCompound<'_>) {
        // Convert to NbtCompound view for accessing methods
        let nbt_view: NbtCompoundView<'_, '_> = nbt.into();
        let mut container = self.container.lock();
        container.items.fill(ItemStack::empty());
        container.generation = container.generation.wrapping_add(1);
        container.pending = None;
        container.loot_table = nbt_view
            .string("LootTable")
            .and_then(|key| match key.to_string().parse() {
                Ok(key) => Some(key),
                Err(error) => {
                    log::warn!("Invalid container LootTable: {error}");
                    None
                }
            })
            .map(|key| {
                let seed = nbt_view
                    .get("LootTableSeed")
                    .and_then(|tag| {
                        tag.long()
                            .or_else(|| tag.int().map(i64::from))
                            .or_else(|| tag.short().map(i64::from))
                            .or_else(|| tag.byte().map(i64::from))
                            .or_else(|| tag.float().map(|value| value as i64))
                            .or_else(|| tag.double().map(|value| value.floor() as i64))
                    })
                    .unwrap_or(0);
                SeededContainerLoot::new(key, seed)
            });
        if container.loot_table.is_some() {
            return;
        }

        load_all_items(nbt_view, &mut container.items);
    }

    pub(crate) fn save_additional(&self, nbt: &mut NbtCompound) {
        // Save items to NBT (only non-empty slots)
        let container = self.container.lock();
        if let Some(loot) = &container.loot_table {
            nbt.insert("LootTable", loot.loot_table().to_string());
            if loot.seed() != 0 {
                nbt.insert("LootTableSeed", loot.seed());
            }
            return;
        }
        let mut items: Vec<NbtCompound> = Vec::new();
        for (slot, item) in container.items.iter().enumerate() {
            if !item.is_empty() {
                // Use ItemStack's ToNbtTag implementation for proper component serialization
                if let NbtTag::Compound(mut item_nbt) = item.clone().to_nbt_tag() {
                    item_nbt.insert("Slot", slot as i8);
                    items.push(item_nbt);
                }
            }
        }
        nbt.insert("Items", NbtList::Compound(items));
    }

    pub(crate) fn apply_implicit_components(&self, components: &mut ImplicitComponentGetter<'_>) {
        let mut container = self.container.lock();
        if let Some(contents) = components.get(CONTAINER) {
            container.items.fill(ItemStack::empty());
            for (slot, template) in contents
                .items()
                .iter()
                .take(container.items.len())
                .enumerate()
            {
                if let Some(template) = template {
                    container.items[slot] = template.create();
                }
            }
        }
        if let Some(loot) = components.get(CONTAINER_LOOT) {
            container.loot_table = Some(loot);
            container.generation = container.generation.wrapping_add(1);
            container.pending = None;
        }
    }
}

impl RandomizableContainer for RandomizableContainerStorage {
    fn loot_table(&self) -> Option<SeededContainerLoot> {
        self.container.lock().loot_table.clone()
    }

    fn set_loot_table(&self, loot: Option<SeededContainerLoot>) {
        let mut container = self.container.lock();
        container.loot_table = loot;
        container.generation = container.generation.wrapping_add(1);
        container.pending = None;
    }

    fn unpack_loot_table(&self, player: Option<&Player>) -> OperationResult<()> {
        Self::unpack(&self.base, &self.container, player)
    }
}

impl RandomizableContainerStorage {
    fn unpack(
        base: &Arc<BlockEntityBase>,
        storage: &Arc<SyncMutex<RandomizableContainerInventory>>,
        player: Option<&Player>,
    ) -> OperationResult<()> {
        let Some(world) = base.level() else {
            return Ok(());
        };
        let (loot, generation, completion) = {
            let mut container = storage.lock();
            if let Some(pending) = &container.pending {
                return pending.try_result();
            }
            let Some(loot) = container.loot_table.clone() else {
                return Ok(());
            };
            let completion = LootCompletion::default();
            container.pending = Some(completion.clone());
            container.generation = container.generation.wrapping_add(1);
            (loot, container.generation, completion)
        };
        let params = LootParams {
            origin: Some(base.pos().get_center()),
            luck: player.map_or(0.0, LivingEntity::get_luck),
            this_entity: player.map(|player| LootEntitySnapshot::capture(player)),
            ..LootParams::default()
        };
        // TODO: Trigger GENERATE_LOOT once advancement criteria exist.
        let work = world.prepare_loot(LOOT_TABLES.by_key(loot.loot_table()), loot.seed(), params);
        let storage = Arc::clone(storage);
        let base = Arc::clone(base);
        world.run_loot_with_completion(work, completion.clone(), move |result| {
            let generated = result?;
            {
                let mut container = storage.lock();
                if container.generation != generation {
                    return Err(OperationError::ContainerChanged);
                }
                container.loot_table = None;
                container.pending = None;
                generated.fill(&mut *container);
            }
            base.set_changed();
            Ok(())
        })?;
        completion.try_result()
    }
}

impl Container for RandomizableContainerInventory {
    fn items(&self) -> &[ItemStack] {
        &self.items
    }

    fn items_mut(&mut self) -> &mut [ItemStack] {
        &mut self.items
    }

    fn set_item(&mut self, slot: usize, mut stack: ItemStack) {
        if slot < self.items.len() {
            let max_stack_size = self.get_max_stack_size_for_item(&stack);
            if !stack.is_empty() && stack.count() > max_stack_size {
                stack.set_count(max_stack_size);
            }
            self.items[slot] = stack;
        }
    }

    fn set_changed(&mut self) {}
}
