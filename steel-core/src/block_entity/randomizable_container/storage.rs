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

use crate::block_entity::{BlockEntityBase, RandomizableContainer};
use crate::entity::{LivingEntity, entity_loot_ref};
use crate::inventory::container::Container;
use crate::inventory::lock::{ContainerRef, SharedContainer};
use crate::loot::{LOOT_TABLES, LootContext, LootError, LootResult};
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
    unpacking: bool,
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
            unpacking: false,
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
}

impl RandomizableContainerStorage {
    pub(crate) fn pre_remove_side_effects(&self, pos: BlockPos) {
        if let Err(error) = self.unpack_loot_table(None) {
            log::error!("Could not unpack container loot at {pos:?} before removal: {error}");
            return;
        }
        let items = {
            let mut container = self.container.lock();
            let slots = container.items.len();
            mem::replace(&mut container.items, vec![ItemStack::empty(); slots])
        };
        let Some(world) = self.base.level() else {
            return;
        };
        for item in items {
            world.drop_item_stack(pos, item);
        }
    }

    pub(crate) fn load_additional(&self, nbt: &BorrowedNbtCompound<'_>) {
        // Convert to NbtCompound view for accessing methods
        let nbt_view: NbtCompoundView<'_, '_> = nbt.into();
        let mut container = self.container.lock();
        container.items.fill(ItemStack::empty());
        container.generation = container.generation.wrapping_add(1);
        container.unpacking = false;
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
                            .or_else(|| tag.double().map(|value| value as i64))
                    })
                    .unwrap_or(0);
                SeededContainerLoot::new(key, seed)
            });
        if container.loot_table.is_some() {
            return;
        }

        // Load items from NBT using borrowed NBT for proper ItemStack parsing
        if let Some(items_list) = nbt_view.list("Items")
            && let Some(compounds) = items_list.compounds()
        {
            for compound in compounds {
                let slot = match compound.get("Slot") {
                    None => Some(0),
                    Some(tag) => tag
                        .byte()
                        .map(|value| value as u8)
                        .or_else(|| tag.short().map(|value| value as u8))
                        .or_else(|| tag.int().map(|value| value as u8))
                        .or_else(|| tag.long().map(|value| value as u8))
                        .or_else(|| tag.float().map(|value| value as i32 as u8))
                        .or_else(|| tag.double().map(|value| value as i32 as u8)),
                };
                if let Some(slot) = slot {
                    let slot = usize::from(slot);
                    if slot < container.items.len() {
                        // Parse item directly from the borrowed compound
                        if let Some(item) = ItemStack::from_borrowed_compound(&compound) {
                            container.items[slot] = item;
                        }
                    }
                }
            }
        }
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

    pub(crate) fn apply_components_from_item(&self, item: &ItemStack) {
        let mut container = self.container.lock();
        if let Some(contents) = item.get(CONTAINER) {
            container.items.fill(ItemStack::empty());
            for (slot, template) in contents
                .items()
                .iter()
                .take(container.items.len())
                .enumerate()
            {
                if let Some(template) = template {
                    container.items[slot] = ItemStack::with_count_and_patch(
                        template.item(),
                        template.count(),
                        template.components().clone(),
                    );
                }
            }
        }
        if let Some(loot) = item.get(CONTAINER_LOOT) {
            container.loot_table = Some(loot.clone());
            container.generation = container.generation.wrapping_add(1);
            container.unpacking = false;
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
        container.unpacking = false;
    }

    fn unpack_loot_table(&self, player: Option<&Player>) -> LootResult<()> {
        Self::unpack(&self.base, &self.container, player)
    }
}

impl RandomizableContainerStorage {
    fn unpack(
        base: &BlockEntityBase,
        storage: &SyncMutex<RandomizableContainerInventory>,
        player: Option<&Player>,
    ) -> LootResult<()> {
        let Some(world) = base.level() else {
            return Ok(());
        };
        let (loot, generation) = {
            let mut container = storage.lock();
            let Some(loot) = container.loot_table.clone() else {
                return Ok(());
            };
            if container.unpacking {
                // TODO: Coordinate concurrent access with vanilla-compatible
                // ordering. This error currently reaches comparator zero fallbacks.
                return Err(LootError::ContainerChanged);
            }
            container.unpacking = true;
            container.generation = container.generation.wrapping_add(1);
            (loot, container.generation)
        };
        let result = (|| {
            let table = LOOT_TABLES
                .by_key(loot.loot_table())
                .ok_or_else(|| LootError::MissingTable(loot.loot_table().clone()))?;
            let pos = base.pos();
            world.with_loot_random(table, loot.seed(), |random| {
                let mut context = LootContext::new(random).with_world(&world).with_origin(
                    f64::from(pos.x()) + 0.5,
                    f64::from(pos.y()) + 0.5,
                    f64::from(pos.z()) + 0.5,
                );
                if let Some(player) = player {
                    // TODO: Trigger GENERATE_LOOT once advancement criteria exist.
                    context = context
                        .with_luck(player.get_luck())
                        .with_this_entity(entity_loot_ref(player));
                }
                let generated = table.generate_for_container(&mut context)?;
                let mut container = storage.lock();
                if container.generation != generation {
                    return Err(LootError::ContainerChanged);
                }
                container.loot_table = None;
                generated.fill(&mut *container);
                Ok(())
            })
        })();
        {
            let mut container = storage.lock();
            if container.generation == generation {
                container.unpacking = false;
            }
        }
        if result.is_ok() {
            base.set_changed();
        }
        result
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
