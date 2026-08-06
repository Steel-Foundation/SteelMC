//! `CampfireBlockEntity` for campfire cooking.

use std::array::from_fn;
use std::sync::{Arc, Weak};

use simdnbt::ToNbtTag;
use simdnbt::borrow::{BaseNbtCompound as BorrowedNbtCompound, NbtCompound as NbtCompoundView};
use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_block_entity_types;
use steel_registry::{REGISTRY, vanilla_game_events};
use steel_utils::{BlockPos, BlockStateId, DowncastType, DowncastTypeKey, locks::SyncMutex};

use crate::block_entity::{BlockEntity, BlockEntityBase};
use crate::world::World;
use crate::world::game_event::GameEventContext;

/// Number of cooking slots on a campfire.
pub const CAMPFIRE_SLOTS: usize = 4;

/// Ticks a cooled cooking slot loses per campfire tick, mirroring vanilla `BURN_COOL_SPEED`.
const BURN_COOL_SPEED: i32 = 2;

/// Block entity for `campfire` blocks.
pub struct CampfireBlockEntity {
    base: BlockEntityBase,
    data: SyncMutex<CampfireData>,
}

struct CampfireData {
    /// The items cooking on the fire, one per slot (each held as a single item).
    items: [ItemStack; CAMPFIRE_SLOTS],
    /// Ticks each slot has cooked so far.
    cooking_progress: [i32; CAMPFIRE_SLOTS],
    /// Total cooking time in ticks for each slot's current recipe.
    cooking_time: [i32; CAMPFIRE_SLOTS],
}

impl Default for CampfireData {
    fn default() -> Self {
        Self {
            items: from_fn(|_| ItemStack::empty()),
            cooking_progress: [0; CAMPFIRE_SLOTS],
            cooking_time: [0; CAMPFIRE_SLOTS],
        }
    }
}

// SAFETY: This key is owned by Steel and uniquely identifies `CampfireBlockEntity`.
unsafe impl DowncastType for CampfireBlockEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:block_entity/campfire");
}

impl CampfireBlockEntity {
    /// Creates a new `CampfireBlockEntity`.
    #[must_use]
    pub fn new(world: Weak<World>, pos: BlockPos, state: BlockStateId) -> Self {
        Self {
            base: BlockEntityBase::new(&vanilla_block_entity_types::CAMPFIRE, world, pos, state),
            data: SyncMutex::new(CampfireData::default()),
        }
    }

    /// Places a single `stack` into the first empty slot and starts cooking it.
    ///
    /// Mirrors `CampfireBlockEntity.placeFood`. The caller supplies `recipe_time`
    /// (the recipe's cooking time) after a cookable lookup succeeds.
    ///
    /// # Returns
    /// `true` if the item was placed, `false` if all slots are occupied.
    #[must_use]
    pub fn place_food(&self, mut stack: ItemStack, recipe_time: i32) -> bool {
        stack.set_count(1);
        let mut data = self.data.lock();
        for slot in 0..CAMPFIRE_SLOTS {
            if data.items[slot].is_empty() {
                data.items[slot] = stack;
                data.cooking_time[slot] = recipe_time;
                data.cooking_progress[slot] = 0;
                return true;
            }
        }
        false
    }

    /// Advances cooking for each occupied slot while the campfire is lit.
    ///
    /// Mirrors vanilla `CampfireBlockEntity.cookTick`. When a slot reaches its
    /// recipe's cooking time, the cooked result is dropped at the campfire.
    fn tick_cooking(&self, world: &Arc<World>, state: BlockStateId) {
        let pos = self.get_block_pos();
        let mut changed = false;
        let mut finished = Vec::new();
        {
            let mut data = self.data.lock();
            for slot in 0..CAMPFIRE_SLOTS {
                if data.items[slot].is_empty() {
                    continue;
                }
                changed = true;
                data.cooking_progress[slot] += 1;
                if data.cooking_progress[slot] < data.cooking_time[slot] {
                    continue;
                }
                let result = REGISTRY
                    .recipes
                    .find_campfire_recipe(&data.items[slot])
                    .map_or_else(
                        || data.items[slot].clone(),
                        |recipe| recipe.assemble_result(data.items[slot].count(), false),
                    );
                data.items[slot] = ItemStack::empty();
                finished.push(result);
            }
        }

        let has_finished = !finished.is_empty();
        if changed {
            self.set_changed();
        }
        for result in finished {
            world.pop_resource(pos, result);
        }
        if has_finished {
            world.send_block_updated(pos);
            let context = GameEventContext::new(None, Some(state));
            world.game_event(&vanilla_game_events::BLOCK_CHANGE, pos, &context);
        }
    }

    /// Cools down cooking progress while the campfire is unlit.
    ///
    /// Mirrors vanilla `CampfireBlockEntity.cooldownTick`.
    fn tick_cooldown(&self) {
        let mut changed = false;
        {
            let mut data = self.data.lock();
            for slot in 0..CAMPFIRE_SLOTS {
                if data.cooking_progress[slot] <= 0 {
                    continue;
                }
                changed = true;
                data.cooking_progress[slot] = data.cooking_progress[slot]
                    .saturating_sub(BURN_COOL_SPEED)
                    .min(data.cooking_time[slot]);
            }
        }
        if changed {
            self.set_changed();
        }
    }

    /// Clears and returns all cooked/uncooked items, dropping them on removal.
    #[must_use]
    fn clear_items(&self) -> Vec<ItemStack> {
        let mut data = self.data.lock();
        let items: Vec<ItemStack> = data
            .items
            .iter()
            .filter(|item| !item.is_empty())
            .cloned()
            .collect();
        *data = CampfireData::default();
        items
    }
}

impl BlockEntity for CampfireBlockEntity {
    fn base(&self) -> &BlockEntityBase {
        &self.base
    }

    fn load_additional(&self, nbt: &BorrowedNbtCompound<'_>) {
        let nbt_view: NbtCompoundView<'_, '_> = nbt.into();
        let mut data = self.data.lock();
        *data = CampfireData::default();

        if let Some(items_list) = nbt_view.list("Items")
            && let Some(compounds) = items_list.compounds()
        {
            for compound in compounds {
                if let Some(slot) = compound.byte("Slot")
                    && let Some(item) = ItemStack::from_borrowed_compound(&compound)
                    && (slot as usize) < CAMPFIRE_SLOTS
                {
                    data.items[slot as usize] = item;
                }
            }
        }

        if let Some(times) = nbt_view.int_array("CookingTimes") {
            for (i, value) in times.iter().enumerate().take(CAMPFIRE_SLOTS) {
                data.cooking_progress[i] = *value;
            }
        }
        if let Some(times) = nbt_view.int_array("CookingTotalTimes") {
            for (i, value) in times.iter().enumerate().take(CAMPFIRE_SLOTS) {
                data.cooking_time[i] = *value;
            }
        }
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        let data = self.data.lock();
        let mut items: Vec<NbtCompound> = Vec::new();
        for (slot, item) in data.items.iter().enumerate() {
            if !item.is_empty()
                && let NbtTag::Compound(mut item_nbt) = item.clone().to_nbt_tag()
            {
                item_nbt.insert("Slot", slot as i8);
                items.push(item_nbt);
            }
        }
        nbt.insert("Items", NbtList::Compound(items));
        nbt.insert(
            "CookingTimes",
            NbtTag::IntArray(data.cooking_progress.to_vec()),
        );
        nbt.insert(
            "CookingTotalTimes",
            NbtTag::IntArray(data.cooking_time.to_vec()),
        );
    }

    fn pre_remove_side_effects(&self, pos: BlockPos, _state: BlockStateId) {
        let items = self.clear_items();
        let Some(world) = self.get_level() else {
            return;
        };
        for item in items {
            world.pop_resource(pos, item);
        }
    }

    fn get_update_tag(&self) -> Option<NbtCompound> {
        Some(self.save_custom_only())
    }

    fn tick(&self, world: &Arc<World>) {
        let state = self.get_block_state();
        if state.get_value(&BlockStateProperties::LIT) {
            self.tick_cooking(world, state);
        } else {
            self.tick_cooldown();
        }
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::test_support::init_test_registry;
    use steel_registry::{vanilla_blocks, vanilla_items};
    use steel_utils::{ChunkPos, types::UpdateFlags};

    use super::*;
    use crate::behavior::init_behaviors;
    use crate::test_support::{fresh_test_world, insert_ready_full_chunk};

    fn lit_campfire_entity(
        world: &Arc<World>,
        pos: BlockPos,
    ) -> (BlockStateId, CampfireBlockEntity) {
        let state = vanilla_blocks::CAMPFIRE
            .default_state()
            .set_value(&BlockStateProperties::LIT, true);
        world.set_block(pos, state, UpdateFlags::UPDATE_ALL);
        (
            state,
            CampfireBlockEntity::new(Arc::downgrade(world), pos, state),
        )
    }

    #[test]
    fn lit_campfire_cooks_item_and_clears_the_slot_when_done() {
        init_test_registry();
        init_behaviors();
        let world = fresh_test_world("campfire_cook");
        let pos = BlockPos::new(4, 64, 4);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));
        let (state, entity) = lit_campfire_entity(&world, pos);

        assert!(entity.place_food(ItemStack::new(&vanilla_items::BEEF), 2));
        assert!(!entity.data.lock().items[0].is_empty());

        entity.tick(&world);
        assert!(!entity.data.lock().items[0].is_empty());
        assert_eq!(entity.data.lock().cooking_progress[0], 1);

        entity.tick(&world);
        assert!(entity.data.lock().items[0].is_empty());
        assert!(state.get_value(&BlockStateProperties::LIT));
    }

    #[test]
    fn unlit_campfire_cools_down_progress_without_dropping() {
        init_test_registry();
        init_behaviors();
        let world = fresh_test_world("campfire_cooldown");
        let pos = BlockPos::new(4, 64, 4);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));
        let state = vanilla_blocks::CAMPFIRE
            .default_state()
            .set_value(&BlockStateProperties::LIT, false);
        world.set_block(pos, state, UpdateFlags::UPDATE_ALL);
        let entity = CampfireBlockEntity::new(Arc::downgrade(&world), pos, state);

        let _ = entity.place_food(ItemStack::new(&vanilla_items::BEEF), 10);
        entity.data.lock().cooking_progress[0] = 6;

        entity.tick(&world);
        assert_eq!(entity.data.lock().cooking_progress[0], 4);
        assert!(!entity.data.lock().items[0].is_empty());
    }
}
