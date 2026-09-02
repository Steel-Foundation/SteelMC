//! Normal furnace block entity and cooking state.

use std::{
    mem,
    sync::{Arc, Weak},
};

use glam::DVec3;
use simdnbt::borrow::{BaseNbtCompound as BorrowedNbtCompound, NbtCompound as NbtCompoundView};
use simdnbt::owned::{NbtCompound, NbtTag};
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::data_components::DataComponentMap;
use steel_registry::item_stack::ItemStack;
use steel_registry::{REGISTRY, vanilla_block_entity_types, vanilla_items};
use steel_utils::types::UpdateFlags;
use steel_utils::{
    BlockPos, BlockStateId, DowncastType, DowncastTypeKey, Identifier, locks::SyncMutex,
    nbt::NbtNumeric as _, translations::CONTAINER_FURNACE,
};
use text_components::TextComponent;

use crate::block_entity::base_container::BaseContainer;
use crate::block_entity::{
    BlockEntity, BlockEntityBase, BlockEntityComponentInput, vanilla_fuel_values,
};
use crate::entity::entities::ExperienceOrbEntity;
use crate::inventory::container::Container;
use crate::inventory::lock::{ContainerRef, SharedContainer};
use crate::world::World;

/// Number of inventory slots in a furnace.
pub const FURNACE_SLOTS: usize = 3;
/// Furnace ingredient slot index.
pub const FURNACE_INPUT_SLOT: usize = 0;
/// Furnace fuel slot index.
pub const FURNACE_FUEL_SLOT: usize = 1;
/// Furnace result slot index.
pub const FURNACE_RESULT_SLOT: usize = 2;

const DEFAULT_COOKING_TIME: i32 = 200;
const BURN_COOL_SPEED: i32 = 2;

/// Server state for a normal furnace.
pub struct FurnaceBlockEntity {
    base: Arc<BlockEntityBase>,
    container: Arc<SyncMutex<FurnaceContainer>>,
    container_ref: ContainerRef,
}

/// Independently lockable furnace inventory and cooking data.
pub(crate) struct FurnaceContainer {
    base: BaseContainer,
    lit_time_remaining: i32,
    lit_total_time: i32,
    cooking_timer: i32,
    cooking_total_time: i32,
    recipes_used: Vec<(Identifier, i32)>,
}

#[derive(Clone, Copy)]
struct FurnaceTickResult {
    changed: bool,
    lit_changed: bool,
    is_lit: bool,
}

// SAFETY: This key is owned by Steel and uniquely identifies `FurnaceBlockEntity`.
unsafe impl DowncastType for FurnaceBlockEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:block_entity/furnace");
}

// SAFETY: This key is owned by Steel and uniquely identifies normal-furnace
// inventory and cooking state.
unsafe impl DowncastType for FurnaceContainer {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:container/furnace");
}

impl FurnaceBlockEntity {
    /// Creates a normal furnace block entity.
    #[must_use]
    pub fn new(level: Weak<World>, pos: BlockPos, state: BlockStateId) -> Self {
        let base = Arc::new(BlockEntityBase::new(
            &vanilla_block_entity_types::FURNACE,
            level,
            pos,
            state,
        ));
        let container = Arc::new(SyncMutex::new(FurnaceContainer::new()));
        let shared_container: SharedContainer = container.clone();
        Self {
            container_ref: ContainerRef::owned_by_block_entity(shared_container, Arc::clone(&base)),
            base,
            container,
        }
    }

    /// Returns the menu title, preferring a persisted custom name.
    #[must_use]
    pub fn display_name(&self) -> TextComponent {
        self.container
            .lock()
            .display_name(TextComponent::translated(CONTAINER_FURNACE.msg()))
    }

    /// Returns whether opening can proceed without unavailable lock matching.
    #[must_use]
    pub fn menu_is_ready(&self) -> bool {
        // TODO: Use a shared `ItemPredicate` matcher, spectator bypass, and
        // Vanilla's locked-container feedback once open validation supports them.
        !self.container.lock().has_lock()
    }
}

impl BlockEntity for FurnaceBlockEntity {
    fn base(&self) -> &BlockEntityBase {
        &self.base
    }

    fn pre_remove_side_effects(&self, pos: BlockPos, _state: BlockStateId) {
        let (items, recipes_used) = {
            let mut container = self.container.lock();
            (container.take_items(), container.take_recipes_used())
        };
        let Some(world) = self.get_level() else {
            return;
        };
        for item in items {
            world.drop_item_stack(pos, item);
        }
        FurnaceContainer::pop_recipe_experience(
            &world,
            DVec3::new(
                f64::from(pos.x()) + 0.5,
                f64::from(pos.y()) + 0.5,
                f64::from(pos.z()) + 0.5,
            ),
            recipes_used,
        );
    }

    fn load_additional(&self, nbt: &BorrowedNbtCompound<'_>) {
        let nbt_view: NbtCompoundView<'_, '_> = nbt.into();
        self.container.lock().load(&nbt_view);
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        self.container.lock().save(nbt);
    }

    fn apply_implicit_components(&self, components: &BlockEntityComponentInput<'_>) {
        self.container
            .lock()
            .base
            .apply_implicit_components(components);
    }

    fn collect_implicit_components(&self, components: &mut DataComponentMap) {
        self.container
            .lock()
            .base
            .collect_implicit_components(components);
    }

    fn remove_components_from_tag(&self, nbt: &mut NbtCompound) {
        BaseContainer::remove_components_from_tag(nbt);
    }

    fn tick(&self, world: &Arc<World>) {
        let result = self.container.lock().server_tick();
        if result.lit_changed {
            let state = self.get_block_state();
            let _ = world.set_block(
                self.get_block_pos(),
                state.set_value(&BlockStateProperties::LIT, result.is_lit),
                UpdateFlags::UPDATE_ALL,
            );
        }
        if result.changed {
            self.set_changed();
        }
    }

    fn container_ref(&self) -> Option<ContainerRef> {
        Some(self.container_ref.clone())
    }
}

impl FurnaceContainer {
    fn new() -> Self {
        Self {
            base: BaseContainer::new(FURNACE_SLOTS),
            lit_time_remaining: 0,
            lit_total_time: 0,
            cooking_timer: 0,
            cooking_total_time: 0,
            recipes_used: Vec::new(),
        }
    }

    fn load(&mut self, nbt: &NbtCompoundView<'_, '_>) {
        self.base.load_metadata(nbt);
        self.base.load_items(nbt);
        self.cooking_timer = i32::from(nbt.short("cooking_time_spent").unwrap_or(0));
        self.cooking_total_time = i32::from(nbt.short("cooking_total_time").unwrap_or(0));
        self.lit_time_remaining = i32::from(nbt.short("lit_time_remaining").unwrap_or(0));
        self.lit_total_time = i32::from(nbt.short("lit_total_time").unwrap_or(0));
        self.recipes_used = Self::load_recipes_used(nbt);
    }

    fn save(&self, nbt: &mut NbtCompound) {
        self.base.save_metadata(nbt);
        nbt.insert("cooking_time_spent", self.cooking_timer as i16);
        nbt.insert("cooking_total_time", self.cooking_total_time as i16);
        nbt.insert("lit_time_remaining", self.lit_time_remaining as i16);
        nbt.insert("lit_total_time", self.lit_total_time as i16);
        self.base.save_items(nbt);

        let mut recipes_used = NbtCompound::new();
        for (id, count) in &self.recipes_used {
            recipes_used.insert(id.to_string(), NbtTag::Int(*count));
        }
        nbt.insert("RecipesUsed", recipes_used);
    }

    fn load_recipes_used(nbt: &NbtCompoundView<'_, '_>) -> Vec<(Identifier, i32)> {
        let Some(compound) = nbt.compound("RecipesUsed") else {
            return Vec::new();
        };
        let mut recipes = Vec::with_capacity(compound.len());
        for (key, value) in compound.iter() {
            let Ok(id) = key.to_str().parse::<Identifier>() else {
                return Vec::new();
            };
            let Some(count) = value.codec_i32() else {
                return Vec::new();
            };
            if recipes.iter().any(|(existing, _)| existing == &id) {
                return Vec::new();
            }
            recipes.push((id, count));
        }
        recipes
    }

    #[must_use]
    pub(crate) fn display_name(&self, default: TextComponent) -> TextComponent {
        self.base.display_name(default)
    }

    #[must_use]
    pub(crate) fn has_lock(&self) -> bool {
        self.base.has_lock()
    }

    #[must_use]
    pub(crate) const fn menu_data(&self) -> [i16; 4] {
        [
            self.lit_time_remaining as i16,
            self.lit_total_time as i16,
            self.cooking_timer as i16,
            self.cooking_total_time as i16,
        ]
    }

    #[must_use]
    pub(crate) fn take_recipes_used(&mut self) -> Vec<(Identifier, i32)> {
        mem::take(&mut self.recipes_used)
    }

    pub(crate) fn pop_recipe_experience(
        world: &Arc<World>,
        position: DVec3,
        recipes_used: Vec<(Identifier, i32)>,
    ) {
        for (id, amount) in recipes_used {
            let Some(recipe) = REGISTRY.recipes.get_smelting(&id) else {
                continue;
            };
            let exact_reward = amount as f32 * recipe.experience;
            let floored_reward = exact_reward.floor();
            let mut reward = floored_reward as i32;
            let fractional = exact_reward - floored_reward;
            if fractional != 0.0 && rand::random::<f32>() < fractional {
                reward += 1;
            }
            ExperienceOrbEntity::award(world, position, reward);
        }
    }

    fn total_cook_time(input: &ItemStack) -> i32 {
        REGISTRY
            .recipes
            .find_smelting_recipe(input)
            .map_or(DEFAULT_COOKING_TIME, |recipe| recipe.cooking_time)
    }

    fn can_burn(&self, burn_result: &ItemStack) -> bool {
        if burn_result.is_empty() {
            return false;
        }
        let output = &self.base.items()[FURNACE_RESULT_SLOT];
        if output.is_empty() {
            return true;
        }
        if !ItemStack::is_same_item_same_components(output, burn_result) {
            return false;
        }
        let result_count = output.count() + burn_result.count();
        result_count <= 99.min(burn_result.max_stack_size())
    }

    fn consume_fuel(&mut self) {
        let fuel_item = self.base.items()[FURNACE_FUEL_SLOT].item();
        self.base.items_mut()[FURNACE_FUEL_SLOT].shrink(1);
        if self.base.items()[FURNACE_FUEL_SLOT].is_empty() {
            self.base.items_mut()[FURNACE_FUEL_SLOT] = fuel_item.get_crafting_remainder();
        }
    }

    fn burn(&mut self, result: ItemStack) {
        let items = self.base.items_mut();
        if items[FURNACE_RESULT_SLOT].is_empty() {
            items[FURNACE_RESULT_SLOT] = result;
        } else {
            items[FURNACE_RESULT_SLOT].grow(result.count());
        }

        if items[FURNACE_INPUT_SLOT].is(&vanilla_items::WET_SPONGE)
            && items[FURNACE_FUEL_SLOT].is(&vanilla_items::BUCKET)
        {
            items[FURNACE_FUEL_SLOT] = ItemStack::new(&vanilla_items::WATER_BUCKET);
        }
        items[FURNACE_INPUT_SLOT].shrink(1);
    }

    fn record_recipe(&mut self, id: &Identifier) {
        if let Some((_, count)) = self
            .recipes_used
            .iter_mut()
            .find(|(existing, _)| existing == id)
        {
            *count = count.wrapping_add(1);
        } else {
            self.recipes_used.push((id.clone(), 1));
        }
    }

    fn server_tick(&mut self) -> FurnaceTickResult {
        let was_lit = self.lit_time_remaining > 0;
        if was_lit {
            self.lit_time_remaining -= 1;
        }
        let mut is_lit = self.lit_time_remaining > 0;
        let mut changed = false;

        let has_fuel = !self.base.items()[FURNACE_FUEL_SLOT].is_empty();
        let has_ingredient = !self.base.items()[FURNACE_INPUT_SLOT].is_empty();
        if is_lit || has_fuel && has_ingredient {
            if has_ingredient {
                let recipe = REGISTRY
                    .recipes
                    .find_smelting_recipe(&self.base.items()[FURNACE_INPUT_SLOT]);
                if let Some(recipe) = recipe {
                    let burn_result = recipe.assemble_result(1, false);
                    if self.can_burn(&burn_result) {
                        if !is_lit {
                            let new_lit_time = vanilla_fuel_values()
                                .burn_duration(&self.base.items()[FURNACE_FUEL_SLOT]);
                            self.lit_time_remaining = new_lit_time;
                            self.lit_total_time = new_lit_time;
                            if new_lit_time > 0 {
                                self.consume_fuel();
                                is_lit = true;
                                changed = true;
                            }
                        }

                        if is_lit {
                            self.cooking_timer = self.cooking_timer.wrapping_add(1);
                            if self.cooking_timer == self.cooking_total_time {
                                self.cooking_timer = 0;
                                self.cooking_total_time = recipe.cooking_time;
                                self.burn(burn_result);
                                self.record_recipe(&recipe.id);
                                changed = true;
                            }
                        } else {
                            self.cooking_timer = 0;
                        }
                    } else {
                        self.cooking_timer = 0;
                    }
                }
            } else {
                self.cooking_timer = 0;
            }
        } else if self.cooking_timer > 0 {
            self.cooking_timer =
                (self.cooking_timer - BURN_COOL_SPEED).clamp(0, self.cooking_total_time);
        }

        let lit_changed = was_lit != is_lit;
        if lit_changed {
            changed = true;
        }
        FurnaceTickResult {
            changed,
            lit_changed,
            is_lit,
        }
    }

    fn take_items(&mut self) -> Vec<ItemStack> {
        self.base.take_items()
    }
}

impl Container for FurnaceContainer {
    fn items(&self) -> &[ItemStack] {
        self.base.items()
    }

    fn items_mut(&mut self) -> &mut [ItemStack] {
        self.base.items_mut()
    }

    fn set_item(&mut self, slot: usize, stack: ItemStack) {
        if slot >= FURNACE_SLOTS {
            return;
        }
        let same = !stack.is_empty()
            && ItemStack::is_same_item_same_components(self.get_item(slot), &stack);
        self.base.set_item(slot, stack);
        if slot == FURNACE_INPUT_SLOT && !same {
            self.cooking_total_time = Self::total_cook_time(self.get_item(FURNACE_INPUT_SLOT));
            self.cooking_timer = 0;
        }
    }

    fn set_changed(&mut self) {}

    // TODO: Expose Vanilla's face-specific slot and extraction rules once
    // Steel has a shared sided-container/hopper automation capability.
    fn can_place_item(&self, slot: usize, stack: &ItemStack) -> bool {
        if slot == FURNACE_RESULT_SLOT {
            return false;
        }
        if slot != FURNACE_FUEL_SLOT {
            return true;
        }
        vanilla_fuel_values().is_fuel(stack)
            || stack.is(&vanilla_items::BUCKET)
                && !self.base.items()[FURNACE_FUEL_SLOT].is(&vanilla_items::BUCKET)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use simdnbt::ToNbtTag as _;
    use simdnbt::borrow::read_compound as read_borrowed_compound;
    use steel_registry::test_support::init_test_registry;

    use super::*;

    fn furnace() -> FurnaceContainer {
        init_test_registry();
        FurnaceContainer::new()
    }

    #[test]
    fn smelting_advances_vanilla_timers_and_records_the_recipe() {
        let mut furnace = furnace();
        furnace.set_item(FURNACE_INPUT_SLOT, ItemStack::new(&vanilla_items::RAW_IRON));
        furnace.set_item(FURNACE_FUEL_SLOT, ItemStack::new(&vanilla_items::COAL));

        for _ in 0..200 {
            furnace.server_tick();
        }

        assert!(
            furnace
                .get_item(FURNACE_RESULT_SLOT)
                .is(&vanilla_items::IRON_INGOT)
        );
        assert!(furnace.get_item(FURNACE_INPUT_SLOT).is_empty());
        assert_eq!(furnace.lit_time_remaining, 1_401);
        assert_eq!(furnace.lit_total_time, 1_600);
        assert_eq!(furnace.cooking_timer, 0);
        assert_eq!(furnace.cooking_total_time, 200);
        assert_eq!(furnace.recipes_used.len(), 1);
        assert_eq!(furnace.recipes_used[0].1, 1);
    }

    #[test]
    fn full_output_prevents_ignition_and_fuel_consumption() {
        let mut furnace = furnace();
        furnace.set_item(FURNACE_INPUT_SLOT, ItemStack::new(&vanilla_items::RAW_IRON));
        furnace.set_item(FURNACE_FUEL_SLOT, ItemStack::new(&vanilla_items::COAL));
        furnace.set_item(
            FURNACE_RESULT_SLOT,
            ItemStack::with_count(&vanilla_items::IRON_INGOT, 64),
        );

        let result = furnace.server_tick();

        assert!(!result.is_lit);
        assert_eq!(furnace.lit_time_remaining, 0);
        assert_eq!(furnace.get_item(FURNACE_FUEL_SLOT).count(), 1);
        assert_eq!(furnace.cooking_timer, 0);
    }

    #[test]
    fn wet_sponge_fills_the_consumed_lava_bucket_remainder() {
        let mut furnace = furnace();
        furnace.set_item(
            FURNACE_INPUT_SLOT,
            ItemStack::new(&vanilla_items::WET_SPONGE),
        );
        furnace.set_item(
            FURNACE_FUEL_SLOT,
            ItemStack::new(&vanilla_items::LAVA_BUCKET),
        );

        for _ in 0..200 {
            furnace.server_tick();
        }

        assert!(
            furnace
                .get_item(FURNACE_RESULT_SLOT)
                .is(&vanilla_items::SPONGE)
        );
        assert!(
            furnace
                .get_item(FURNACE_FUEL_SLOT)
                .is(&vanilla_items::WATER_BUCKET)
        );
    }

    #[test]
    fn stalled_progress_cools_by_two_ticks() {
        let mut furnace = furnace();
        furnace.cooking_timer = 17;
        furnace.cooking_total_time = 200;

        furnace.server_tick();

        assert_eq!(furnace.cooking_timer, 15);
    }

    #[test]
    fn unmatched_ingredient_with_fuel_preserves_loaded_progress() {
        let mut furnace = furnace();
        furnace.set_item(FURNACE_INPUT_SLOT, ItemStack::new(&vanilla_items::DIRT));
        furnace.set_item(FURNACE_FUEL_SLOT, ItemStack::new(&vanilla_items::COAL));
        furnace.cooking_timer = 17;
        furnace.cooking_total_time = 200;

        furnace.server_tick();

        assert_eq!(furnace.cooking_timer, 17);
        assert_eq!(furnace.get_item(FURNACE_FUEL_SLOT).count(), 1);
    }

    #[test]
    fn persistence_round_trips_common_data_timers_and_recipes() {
        let mut source_nbt = NbtCompound::new();
        source_nbt.insert(
            "CustomName",
            TextComponent::plain("Saved Furnace").to_nbt_tag(),
        );
        let mut lock = NbtCompound::new();
        lock.insert("count", 2_i32);
        source_nbt.insert("lock", lock);
        source_nbt.insert("cooking_time_spent", 123_i16);
        source_nbt.insert("cooking_total_time", 200_i16);
        source_nbt.insert("lit_time_remaining", 456_i16);
        source_nbt.insert("lit_total_time", 1_600_i16);
        let mut recipes = NbtCompound::new();
        recipes.insert("minecraft:iron_ingot_from_smelting_raw_iron", 3_i32);
        source_nbt.insert("RecipesUsed", recipes);
        let mut bytes = Vec::new();
        source_nbt.write(&mut bytes);
        let borrowed = read_borrowed_compound(&mut Cursor::new(bytes.as_slice()))
            .expect("test furnace NBT should reborrow");
        let view = NbtCompoundView::from(&borrowed);

        let mut furnace = furnace();
        furnace.load(&view);
        furnace.set_item(
            FURNACE_RESULT_SLOT,
            ItemStack::new(&vanilla_items::IRON_INGOT),
        );
        let mut saved = NbtCompound::new();
        furnace.save(&mut saved);

        assert!(furnace.has_lock());
        assert_eq!(
            furnace.display_name(TextComponent::plain("Default")),
            TextComponent::plain("Saved Furnace")
        );
        assert_eq!(furnace.cooking_timer, 123);
        assert_eq!(furnace.lit_time_remaining, 456);
        assert_eq!(
            saved
                .compound("RecipesUsed")
                .and_then(|recipes| recipes.int("minecraft:iron_ingot_from_smelting_raw_iron")),
            Some(3)
        );
        assert_eq!(
            saved.compound("lock").and_then(|lock| lock.int("count")),
            Some(2)
        );
        assert!(saved.list("Items").is_some());
    }
}
