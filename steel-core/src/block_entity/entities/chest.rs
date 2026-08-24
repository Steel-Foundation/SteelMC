//! Chest block entity (`ChestBlockEntity`).

use std::{
    mem,
    str::FromStr,
    sync::{Arc, Weak},
};

use rand::{Rng, SeedableRng as _, rngs::StdRng};
use simdnbt::ToNbtTag;
use simdnbt::borrow::{BaseNbtCompound as BorrowedNbtCompound, NbtCompound as NbtCompoundView};
use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use steel_registry::item_stack::ItemStack;
use steel_registry::loot_table::{LootContext, LootTableRef};
use steel_registry::vanilla_block_entity_types;
use steel_registry::{REGISTRY, RegistryExt};
use steel_utils::Identifier;
use steel_utils::{BlockPos, BlockStateId, DowncastType, DowncastTypeKey, locks::SyncMutex};

use crate::block_entity::{BlockEntity, BlockEntityBase};
use crate::entity::entity_loot_ref;
use crate::entity::{Entity, LivingEntity};
use crate::inventory::container::{Container, fill_container};
use crate::inventory::lock::{ContainerRef, SharedContainer};
use crate::player::Player;
use crate::world::World;

/// Number of slots in a single chest (3 rows of 9).
pub const CHEST_SLOTS: usize = 27;

/// Chest block entity.
pub struct ChestBlockEntity {
    base: Arc<BlockEntityBase>,
    container: Arc<SyncMutex<ChestContainer>>,
    container_ref: ContainerRef,
}

struct ChestContainer {
    items: Vec<ItemStack>,
    loot_table: Option<Identifier>,
    loot_table_seed: i64,
}

// SAFETY: This key is owned by Steel and uniquely identifies `ChestBlockEntity`.
unsafe impl DowncastType for ChestBlockEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:block_entity/chest");
}

// SAFETY: This key is owned by Steel and uniquely identifies the independently
// lockable inventory data used by a chest block entity.
unsafe impl DowncastType for ChestContainer {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:container/chest");
}

impl ChestBlockEntity {
    /// Creates a new chest block entity.
    #[must_use]
    pub fn new(level: Weak<World>, pos: BlockPos, state: BlockStateId) -> Self {
        let base = Arc::new(BlockEntityBase::new(
            &vanilla_block_entity_types::CHEST,
            level,
            pos,
            state,
        ));
        let container = Arc::new(SyncMutex::new(ChestContainer {
            items: vec![ItemStack::empty(); CHEST_SLOTS],
            loot_table: None,
            loot_table_seed: 0,
        }));
        let shared_container: SharedContainer = container.clone();
        Self {
            container_ref: ContainerRef::owned_by_block_entity(shared_container, Arc::clone(&base)),
            base,
            container,
        }
    }

    /// Vanilla `RandomizableContainer.canOpen`.
    #[must_use]
    pub fn can_open(&self, player: &Player) -> bool {
        let has_loot = self.container.lock().loot_table.is_some();
        !(has_loot && player.is_spectator())
    }

    /// Vanilla `RandomizableContainer.unpackLootTable`.
    pub fn unpack_loot_table(&self, player: Option<&Player>) {
        let (loot_table_key, seed) = {
            let mut container = self.container.lock();
            match container.loot_table.take() {
                Some(key) => (key, mem::take(&mut container.loot_table_seed)),
                None => return,
            }
        };

        if self.get_level().is_none() {
            let mut container = self.container.lock();
            container.loot_table = Some(loot_table_key);
            container.loot_table_seed = seed;
            return;
        }

        let loot_table = REGISTRY.loot_tables.by_key(&loot_table_key);
        let origin = self.get_block_pos();
        let origin = (
            f64::from(origin.x()) + 0.5,
            f64::from(origin.y()) + 0.5,
            f64::from(origin.z()) + 0.5,
        );

        if seed == 0 {
            let mut rng = rand::rng();
            self.fill_from_loot(loot_table, player, origin, &mut rng);
        } else {
            let mut rng = StdRng::seed_from_u64(seed as u64);
            self.fill_from_loot(loot_table, player, origin, &mut rng);
        }
        self.set_changed();
    }

    fn fill_from_loot<R: Rng>(
        &self,
        loot_table: Option<LootTableRef>,
        player: Option<&Player>,
        origin: (f64, f64, f64),
        rng: &mut R,
    ) {
        let items = match loot_table {
            Some(table) => {
                let mut ctx = LootContext::new(rng).with_origin(origin.0, origin.1, origin.2);
                if let Some(player) = player {
                    ctx = ctx
                        .with_luck(player.get_luck())
                        .with_this_entity(entity_loot_ref(player));
                }
                table.get_random_items(&mut ctx)
            }
            None => Vec::new(),
        };

        let mut container = self.container.lock();
        fill_container(&mut *container, items, rng);
    }
}

impl BlockEntity for ChestBlockEntity {
    fn base(&self) -> &BlockEntityBase {
        &self.base
    }

    fn pre_remove_side_effects(&self, pos: BlockPos, _state: BlockStateId) {
        self.unpack_loot_table(None);
        let items = {
            let mut container = self.container.lock();
            mem::replace(&mut container.items, vec![ItemStack::empty(); CHEST_SLOTS])
        };
        let Some(world) = self.get_level() else {
            return;
        };
        for item in items {
            world.drop_item_stack(pos, item);
        }
    }

    fn load_additional(&self, nbt: &BorrowedNbtCompound<'_>) {
        let nbt_view: NbtCompoundView<'_, '_> = nbt.into();
        let mut container = self.container.lock();
        container.items.fill(ItemStack::empty());
        container.loot_table = nbt_view
            .string("LootTable")
            .and_then(|value| Identifier::from_str(&value.to_string()).ok());
        container.loot_table_seed = nbt_view.long("LootTableSeed").unwrap_or(0);

        if container.loot_table.is_some() {
            return;
        }

        if let Some(items_list) = nbt_view.list("Items")
            && let Some(compounds) = items_list.compounds()
        {
            for compound in compounds {
                if let Some(slot) = compound.byte("Slot") {
                    let slot = slot as usize;
                    if slot < CHEST_SLOTS
                        && let Some(item) = ItemStack::from_borrowed_compound(&compound)
                    {
                        container.items[slot] = item;
                    }
                }
            }
        }
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        let container = self.container.lock();
        if let Some(loot_table) = &container.loot_table {
            nbt.insert("LootTable", loot_table.to_string());
            if container.loot_table_seed != 0 {
                nbt.insert("LootTableSeed", container.loot_table_seed);
            }
            return;
        }

        let mut items: Vec<NbtCompound> = Vec::new();
        for (slot, item) in container.items.iter().enumerate() {
            if !item.is_empty()
                && let NbtTag::Compound(mut item_nbt) = item.clone().to_nbt_tag()
            {
                item_nbt.insert("Slot", slot as i8);
                items.push(item_nbt);
            }
        }
        nbt.insert("Items", NbtList::Compound(items));
    }

    fn get_update_tag(&self) -> Option<NbtCompound> {
        None
    }

    fn container_ref(&self) -> Option<ContainerRef> {
        Some(self.container_ref.clone())
    }
}

impl Container for ChestContainer {
    fn items(&self) -> &[ItemStack] {
        &self.items
    }

    fn items_mut(&mut self) -> &mut [ItemStack] {
        &mut self.items
    }

    fn get_container_size(&self) -> usize {
        CHEST_SLOTS
    }

    fn set_item(&mut self, slot: usize, mut stack: ItemStack) {
        if slot < CHEST_SLOTS {
            let max_stack_size = self.get_max_stack_size_for_item(&stack);
            if !stack.is_empty() && stack.count() > max_stack_size {
                stack.set_count(max_stack_size);
            }
            self.items[slot] = stack;
        }
    }

    fn set_changed(&mut self) {}
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use simdnbt::borrow::read_compound;
    use steel_registry::{init_vanilla_registry, vanilla_blocks, vanilla_items};

    use super::*;

    fn test_chest() -> ChestBlockEntity {
        init_vanilla_registry();
        ChestBlockEntity::new(
            Weak::new(),
            BlockPos::new(1, 2, 3),
            vanilla_blocks::CHEST.default_state(),
        )
    }

    #[test]
    fn loot_table_nbt_round_trips_and_skips_items() {
        let chest = test_chest();
        {
            let mut container = chest.container.lock();
            container.loot_table = Some(Identifier::vanilla_static("chests/simple_dungeon"));
            container.loot_table_seed = 42;
            container.items[0] = ItemStack::new(&vanilla_items::STONE);
        }

        let mut saved = NbtCompound::new();
        chest.save_additional(&mut saved);
        assert_eq!(
            saved.string("LootTable").map(ToString::to_string),
            Some("minecraft:chests/simple_dungeon".to_owned())
        );
        assert_eq!(saved.long("LootTableSeed"), Some(42));
        assert!(saved.list("Items").is_none());

        let loaded = test_chest();
        let mut bytes = Vec::new();
        saved.write(&mut bytes);
        let borrowed =
            read_compound(&mut Cursor::new(&bytes)).expect("saved chest nbt should reborrow");
        loaded.load_additional(&borrowed);
        let container = loaded.container.lock();
        assert_eq!(
            container.loot_table.as_ref().map(ToString::to_string),
            Some("minecraft:chests/simple_dungeon".to_owned())
        );
        assert_eq!(container.loot_table_seed, 42);
        assert!(container.items.iter().all(ItemStack::is_empty));
    }

    #[test]
    fn item_nbt_round_trips_when_loot_table_is_absent() {
        let chest = test_chest();
        chest
            .container
            .lock()
            .set_item(3, ItemStack::with_count(&vanilla_items::STONE, 4));

        let mut saved = NbtCompound::new();
        chest.save_additional(&mut saved);

        let loaded = test_chest();
        let mut bytes = Vec::new();
        saved.write(&mut bytes);
        let borrowed =
            read_compound(&mut Cursor::new(&bytes)).expect("saved chest nbt should reborrow");
        loaded.load_additional(&borrowed);
        let item = loaded.container.lock().get_item(3).clone();
        assert!(item.is(&vanilla_items::STONE));
        assert_eq!(item.count(), 4);
    }
}
