//! Campfire cooking block entity.

use std::array::from_fn;
use std::mem;
use std::sync::{Arc, Weak};

use simdnbt::ToNbtTag as _;
use simdnbt::borrow::{BaseNbtCompound as BorrowedNbtCompound, NbtCompound as NbtCompoundView};
use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{BlockStateProperties, BoolProperty};
use steel_registry::item_stack::ItemStack;
use steel_registry::recipe::{SingleItemRecipeInput, vanilla_recipe_types};
use steel_registry::{REGISTRY, vanilla_block_entity_types, vanilla_game_events};
use steel_utils::{BlockPos, BlockStateId, DowncastType, DowncastTypeKey, locks::SyncMutex};

use crate::block_entity::{BlockEntity, BlockEntityBase};
use crate::entity::Entity;
use crate::player::Player;
use crate::world::World;
use crate::world::game_event::GameEventContext;

/// Number of independently cooking item positions on a campfire.
pub const CAMPFIRE_SLOTS: usize = 4;
const BURN_COOL_SPEED: i32 = 2;
const LIT: &BoolProperty = &BlockStateProperties::LIT;

pub struct CampfireCookingState {
    items: [ItemStack; CAMPFIRE_SLOTS],
    cooking_progress: [i32; CAMPFIRE_SLOTS],
    cooking_time: [i32; CAMPFIRE_SLOTS],
}

impl CampfireCookingState {
    fn new() -> Self {
        Self {
            items: from_fn(|_| ItemStack::empty()),
            cooking_progress: [0; CAMPFIRE_SLOTS],
            cooking_time: [0; CAMPFIRE_SLOTS],
        }
    }
}

/// Stores four independently cooking campfire items.
pub struct CampfireBlockEntity {
    base: Arc<BlockEntityBase>,
    cooking: SyncMutex<CampfireCookingState>,
}

// SAFETY: This Steel-owned key uniquely identifies `CampfireBlockEntity`.
unsafe impl DowncastType for CampfireBlockEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:block_entity/campfire");
}

impl CampfireBlockEntity {
    /// Creates a campfire block entity.
    #[must_use]
    pub fn new(level: Weak<World>, pos: BlockPos, state: BlockStateId) -> Self {
        Self {
            base: Arc::new(BlockEntityBase::new(
                &vanilla_block_entity_types::CAMPFIRE,
                level,
                pos,
                state,
            )),
            cooking: SyncMutex::new(CampfireCookingState::new()),
        }
    }

    /// Inserts one item when a campfire recipe exists and an empty cooking slot is available.
    pub fn place_food(&self, player: &Player, stack: ItemStack) -> bool {
        let input = SingleItemRecipeInput::new(stack.clone());
        let Some(recipe) = REGISTRY
            .recipes
            .find_match(&vanilla_recipe_types::CAMPFIRE_COOKING, &input)
        else {
            return false;
        };
        {
            let mut cooking = self.cooking.lock();
            let Some(slot) = cooking.items.iter().position(ItemStack::is_empty) else {
                return false;
            };
            cooking.cooking_time[slot] = recipe.data().cooking_time;
            cooking.cooking_progress[slot] = 0;
            cooking.items[slot] = stack;
        }

        self.set_changed();
        let Some(world) = self.get_level() else {
            return true;
        };
        world.game_event(
            &vanilla_game_events::BLOCK_CHANGE,
            self.get_block_pos(),
            &GameEventContext::new(Some(player as &dyn Entity), Some(self.get_block_state())),
        );
        world.send_block_updated(self.get_block_pos());
        true
    }

    fn cook_tick(&self, world: &Arc<World>) {
        let pos = self.get_block_pos();
        let state = self.get_block_state();
        let mut completed = Vec::new();
        let changed = {
            let mut cooking = self.cooking.lock();
            let mut changed = false;
            for slot in 0..CAMPFIRE_SLOTS {
                if cooking.items[slot].is_empty() {
                    continue;
                }
                changed = true;
                cooking.cooking_progress[slot] += 1;
                if cooking.cooking_progress[slot] < cooking.cooking_time[slot] {
                    continue;
                }

                let item = mem::take(&mut cooking.items[slot]);
                let input = SingleItemRecipeInput::new(item.clone());
                let result = REGISTRY
                    .recipes
                    .find_match(&vanilla_recipe_types::CAMPFIRE_COOKING, &input)
                    .map_or(item, |recipe| recipe.data().result.create());
                completed.push(result);
            }
            changed
        };

        for result in completed {
            world.drop_item_stack(pos, result);
            world.send_block_updated(pos);
            world.game_event(
                &vanilla_game_events::BLOCK_CHANGE,
                pos,
                &GameEventContext::new(None, Some(state)),
            );
        }
        if changed {
            self.set_changed();
        }
    }

    fn cooldown_tick(&self) {
        let changed = {
            let mut cooking = self.cooking.lock();
            let mut changed = false;
            for slot in 0..CAMPFIRE_SLOTS {
                if cooking.cooking_progress[slot] > 0 {
                    cooking.cooking_progress[slot] = (cooking.cooking_progress[slot]
                        - BURN_COOL_SPEED)
                        .clamp(0, cooking.cooking_time[slot]);
                    changed = true;
                }
            }
            changed
        };
        if changed {
            self.set_changed();
        }
    }

    fn save_items(cooking: &CampfireCookingState) -> NbtList {
        let mut items = Vec::new();
        for (slot, stack) in cooking.items.iter().enumerate() {
            if !stack.is_empty()
                && let NbtTag::Compound(mut item) = stack.clone().to_nbt_tag()
            {
                item.insert("Slot", slot as i8);
                items.push(item);
            }
        }
        NbtList::Compound(items)
    }
}

impl BlockEntity for CampfireBlockEntity {
    fn base(&self) -> &BlockEntityBase {
        &self.base
    }

    fn pre_remove_side_effects(&self, pos: BlockPos, _state: BlockStateId) {
        let items = {
            let mut cooking = self.cooking.lock();
            mem::replace(&mut cooking.items, from_fn(|_| ItemStack::empty()))
        };
        let Some(world) = self.get_level() else {
            return;
        };
        for item in items {
            world.drop_item_stack(pos, item);
        }
    }

    fn load_additional(&self, nbt: &BorrowedNbtCompound<'_>) {
        let nbt: NbtCompoundView<'_, '_> = nbt.into();
        let mut cooking = self.cooking.lock();
        cooking.items.fill(ItemStack::empty());
        if let Some(items) = nbt.list("Items").and_then(|list| list.compounds()) {
            for compound in items {
                let Some(slot) = compound.byte("Slot").map(|slot| slot as usize) else {
                    continue;
                };
                if slot < CAMPFIRE_SLOTS
                    && let Some(stack) = ItemStack::from_borrowed_compound(&compound)
                {
                    cooking.items[slot] = stack;
                }
            }
        }
        cooking.cooking_progress = nbt
            .int_array("CookingTimes")
            .map_or([0; CAMPFIRE_SLOTS], |values| {
                from_fn(|slot| values.get(slot).copied().unwrap_or(0))
            });
        cooking.cooking_time = nbt
            .int_array("CookingTotalTimes")
            .map_or([0; CAMPFIRE_SLOTS], |values| {
                from_fn(|slot| values.get(slot).copied().unwrap_or(0))
            });
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        let cooking = self.cooking.lock();
        nbt.insert("Items", Self::save_items(&cooking));
        nbt.insert(
            "CookingTimes",
            NbtTag::IntArray(cooking.cooking_progress.to_vec()),
        );
        nbt.insert(
            "CookingTotalTimes",
            NbtTag::IntArray(cooking.cooking_time.to_vec()),
        );
    }

    fn get_update_tag(&self) -> Option<NbtCompound> {
        let mut nbt = NbtCompound::new();
        nbt.insert("Items", Self::save_items(&self.cooking.lock()));
        Some(nbt)
    }

    fn tick(&self, world: &Arc<World>) {
        if self.get_block_state().get_value(LIT) {
            self.cook_tick(world);
        } else {
            self.cooldown_tick();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use simdnbt::borrow::read_compound;
    use steel_registry::{init_vanilla_registry, vanilla_blocks, vanilla_items};

    use super::*;

    fn campfire() -> CampfireBlockEntity {
        init_vanilla_registry();
        CampfireBlockEntity::new(
            Weak::new(),
            BlockPos::new(4, 70, -2),
            vanilla_blocks::CAMPFIRE.default_state(),
        )
    }

    #[test]
    fn campfire_round_trips_all_four_progress_tracks_with_items() {
        let source = campfire();
        {
            let mut cooking = source.cooking.lock();
            cooking.items[2] = ItemStack::new(&vanilla_items::BEEF);
            cooking.cooking_progress = [0, 3, 17, 0];
            cooking.cooking_time = [0, 100, 600, 0];
        }
        let mut saved = NbtCompound::new();
        source.save_additional(&mut saved);

        let mut bytes = Vec::new();
        saved.write(&mut bytes);
        let borrowed = read_compound(&mut Cursor::new(bytes.as_slice()))
            .expect("test campfire NBT should reborrow");
        let loaded = campfire();
        loaded.load_additional(&borrowed);

        let cooking = loaded.cooking.lock();
        assert!(cooking.items[2].is(&vanilla_items::BEEF));
        assert_eq!(cooking.cooking_progress, [0, 3, 17, 0]);
        assert_eq!(cooking.cooking_time, [0, 100, 600, 0]);
    }
}
