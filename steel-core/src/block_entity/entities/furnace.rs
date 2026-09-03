//! Furnace block entity implementation.
//!
//! Mirrors vanilla `AbstractFurnaceBlockEntity`: three slots (input, fuel,
//! result), burn/cook timers driven by `serverTick`, smelting recipe lookups
//! against the vanilla recipe registry, and `RecipesUsed` experience tracking.

use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, Weak};

use simdnbt::borrow::{BaseNbtCompound as BorrowedNbtCompound, NbtCompound as NbtCompoundView};
use simdnbt::owned::NbtCompound;
use steel_registry::{
    REGISTRY, item_stack::ItemStack, vanilla_block_entity_types, vanilla_fuel_values, vanilla_items,
};
use steel_utils::BlockStateId;
use steel_utils::{BlockPos, DowncastType, DowncastTypeKey, Identifier, locks::SyncMutex};

use crate::block_entity::{BlockEntity, BlockEntityBase};
use crate::inventory::{
    container::Container,
    lock::{ContainerRef, SharedContainer},
};
use crate::world::World;

/// Container slot index holding the smelting ingredient (vanilla `SLOT_INPUT`).
pub const SLOT_INPUT: usize = 0;
/// Container slot index holding the fuel item (vanilla `SLOT_FUEL`).
pub const SLOT_FUEL: usize = 1;
/// Container slot index holding the smelted output (vanilla `SLOT_RESULT`).
pub const SLOT_RESULT: usize = 2;
/// Machine slot count: input, fuel, result.
pub const FURNACE_SLOTS: usize = 3;

/// Snapshot of furnace state for menu data slot synchronization.
#[derive(Debug, Clone, Copy)]
pub struct FurnaceState {
    pub lit_time_remaining: i32,
    pub lit_total_time: i32,
    pub cooking_timer: i32,
    pub cooking_total_time: i32,
}

/// Cook duration applied when the input stack does not resolve to a recipe
/// (the `orElse(200)` of vanilla `AbstractFurnaceBlockEntity.getTotalCookTime`).
const DEFAULT_COOK_TIME: i32 = 200;

/// Tile NBT tag list of items, each tagged with its `Slot`.
///
/// TODO: Used by the container item load/save paths once those TODOs land.
#[expect(dead_code, reason = "reserved for the container NBT TODOs")]
const ITEMS_TAG: &str = "Items";
/// Tile NBT short: completed cook ticks (`cooking_time_spent`).
const COOKING_TIMER_TAG: &str = "CookTime";
/// Tile NBT short: total cook ticks for the current item (`cooking_total_time`).
const COOKING_TOTAL_TIME_TAG: &str = "CookTimeTotal";
/// Tile NBT short: remaining fuel burn ticks (`lit_time_remaining`).
const BURN_TIME_TAG: &str = "BurnTime";
/// Tile NBT short: total fuel burn ticks (`lit_total_time`).
const LIT_DURATION_TAG: &str = "LitDuration";
/// Tile NBT compound: recipe id -> completed smelt count (`RecipesUsed`).
///
/// TODO: Used by the recipes_used save/load TODOs.
#[expect(dead_code, reason = "reserved for the recipes_used save/load TODOs")]
const RECIPES_USED_TAG: &str = "RecipesUsed";

/// A furnace block entity.
///
/// Ticks smelting exactly like vanilla `AbstractFurnaceBlockEntity.serverTick`,
/// including lighting only when a recipe can finish into an open output slot.
pub struct FurnaceBlockEntity {
    base: Arc<BlockEntityBase>,
    container: Arc<SyncMutex<FurnaceContainer>>,
    container_ref: ContainerRef,
}

/// All mutable furnace state behind one lock: the three slots plus burn and
/// cook timing.
///
/// Vanilla keeps these as direct block-entity fields; bundling them here means
/// menu data-slot sync and ticking read one consistent snapshot through the
/// same mutex instead of juggling a second lock and lock-order hazard.
pub struct FurnaceContainer {
    items: Vec<ItemStack>,
    /// Remaining fuel burn ticks (`BurnTime`).
    pub lit_time_remaining: AtomicI32,
    /// Total burn ticks of the currently burning fuel (`LitDuration`).
    pub lit_total_time: AtomicI32,
    /// Completed cook ticks for the current input (`CookTime`).
    pub cooking_timer: AtomicI32,
    /// Total cook ticks required for the current input (`CookTimeTotal`).
    pub cooking_total_time: AtomicI32,
    /// Completed smelts awaiting their XP payout, keyed by recipe id (vanilla
    /// `recipesUsed`). Persisted and drained when output is taken.
    // TODO: persisted and read for XP-on-break once the furnace advancement/XP
    // TODOs land.
    #[expect(dead_code, reason = "reserved for the furnace XP TODOs")]
    recipes_used: Vec<(Identifier, i32)>,
}

// SAFETY: This key is owned by Steel and uniquely identifies `FurnaceBlockEntity`.
unsafe impl DowncastType for FurnaceBlockEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:block_entity/furnace");
}

// SAFETY: This key is owned by Steel and uniquely identifies the independently
// lockable inventory and machine state of a furnace block entity.
unsafe impl DowncastType for FurnaceContainer {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:container/furnace");
}

impl FurnaceBlockEntity {
    /// Creates a new furnace block entity with vanilla's three machine slots.
    #[must_use]
    pub fn new(level: Weak<World>, pos: BlockPos, state: BlockStateId) -> Self {
        tracing::info!("Creating furnace block entity at {:?}", pos);
        let base = Arc::new(BlockEntityBase::new(
            &vanilla_block_entity_types::FURNACE,
            level,
            pos,
            state,
        ));
        let container = Arc::new(SyncMutex::new(FurnaceContainer {
            items: vec![ItemStack::empty(); FURNACE_SLOTS],
            lit_time_remaining: AtomicI32::new(0),
            lit_total_time: AtomicI32::new(0),
            cooking_timer: AtomicI32::new(0),
            cooking_total_time: AtomicI32::new(DEFAULT_COOK_TIME),
            recipes_used: Vec::new(),
        }));
        let shared_container: SharedContainer = container.clone();
        Self {
            container_ref: ContainerRef::owned_by_block_entity(shared_container, Arc::clone(&base)),
            base,
            container,
        }
    }

    /// Returns the 4 data slots for menu synchronization:
    /// 0: lit_time_remaining (fire icon)
    /// 1: lit_total_time (max fire)
    /// 2: cooking_timer (progress arrow)
    /// 3: cooking_total_time (max progress)
    pub fn get_data(&self, index: i32) -> i32 {
        let container = self.container.lock();
        match index {
            0 => container.lit_time_remaining.load(Ordering::Relaxed),
            1 => container.lit_total_time.load(Ordering::Relaxed),
            2 => container.cooking_timer.load(Ordering::Relaxed),
            3 => container.cooking_total_time.load(Ordering::Relaxed),
            _ => 0,
        }
    }

    /// Returns a clone of the container Arc for menu data slot updates
    pub fn container(&self) -> Arc<SyncMutex<FurnaceContainer>> {
        Arc::clone(&self.container)
    }

    /// Sets the data.
    pub fn set_data(&self, index: i32, value: i32) {
        let container = self.container.lock();
        match index {
            0 => container.lit_time_remaining.store(value, Ordering::Relaxed),
            1 => container.lit_total_time.store(value, Ordering::Relaxed),
            2 => container.cooking_timer.store(value, Ordering::Relaxed),
            3 => container.cooking_total_time.store(value, Ordering::Relaxed),
            _ => {}
        }
    }

    /// Returns a snapshot of the furnace state for menu data slot updates.
    pub fn furnace_state(&self) -> FurnaceState {
        let container = self.container.lock();
        FurnaceState {
            lit_time_remaining: container.lit_time_remaining.load(Ordering::Relaxed),
            lit_total_time: container.lit_total_time.load(Ordering::Relaxed),
            cooking_timer: container.cooking_timer.load(Ordering::Relaxed),
            cooking_total_time: container.cooking_total_time.load(Ordering::Relaxed),
        }
    }
}

impl BlockEntity for FurnaceBlockEntity {
    fn base(&self) -> &BlockEntityBase {
        &self.base
    }

    fn load_additional(&self, nbt: &BorrowedNbtCompound<'_>) {
        let view: NbtCompoundView<'_, '_> = nbt.into();
        let mut container = self.container.lock();
        container.load_nbt(&view);
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        let container = self.container.lock();
        container.save_nbt(nbt);
    }

    fn pre_remove_side_effects(&self, pos: BlockPos, _state: BlockStateId) {
        let items = {
            let mut container = self.container.lock();
            std::mem::replace(
                &mut container.items,
                vec![ItemStack::empty(); FURNACE_SLOTS],
            )
        };
        let Some(world) = self.get_level() else {
            return;
        };
        for item in items {
            world.drop_item_stack(pos, item);
        }
    }

    fn container_ref(&self) -> Option<ContainerRef> {
        Some(self.container_ref.clone())
    }

    fn tick(&self, world: &Arc<World>) {
        tracing::info!("Furnace tick called at {:?}", self.get_block_pos());
        let mut container = self.container.lock();
        let was_lit = container.is_lit();

        container.server_tick(world, self.get_block_pos());

        let is_lit = container.is_lit();

        // Update block lit state if changed
        if was_lit != is_lit {
            // TODO: Update block state lit property when API is available
            // For now this is a known limitation
        }
    }
}

impl FurnaceContainer {
    fn is_lit(&self) -> bool {
        self.lit_time_remaining.load(Ordering::Relaxed) > 0
    }

    /// Main furnace tick logic - mirrors vanilla AbstractFurnaceBlockEntity.serverTick
    fn server_tick(&mut self, _world: &Arc<World>, _pos: BlockPos) {
        let is_burning = self.is_lit();
        // TODO: mark the block entity changed when `changed` is true once the
        // furnace dirty-flag TODO lands.
        let mut changed = false;

        // Decrement burn time
        if is_burning {
            self.lit_time_remaining.fetch_sub(1, Ordering::Relaxed);
        }

        let input = self.items[SLOT_INPUT].clone();
        let fuel = self.items[SLOT_FUEL].clone();
        let has_input = !input.is_empty();
        let has_fuel = !fuel.is_empty();

        // Find recipe for input
        let recipe = if has_input {
            let result = REGISTRY.recipes.find_smelting_result(&input, false);
            if result.is_none() {
                tracing::info!("No smelting recipe found for item: {:?}", input.item().id);
            } else {
                tracing::info!(
                    "Found smelting recipe for {:?} -> {:?}",
                    input.item().id,
                    result.as_ref().unwrap().item().id
                );
            }
            result
        } else {
            None
        };

        let max_stack_size = self.items[SLOT_RESULT].max_stack_size();
        let can_burn = self.can_burn(&recipe, max_stack_size);

        // Start burning fuel if needed and possible
        if !is_burning && has_fuel && has_input {
            if can_burn {
                // Consume fuel
                let burn_time = vanilla_fuel_values::burn_duration(fuel.item());
                if burn_time > 0 {
                    self.lit_time_remaining.store(burn_time, Ordering::Relaxed);
                    self.lit_total_time.store(burn_time, Ordering::Relaxed);
                    changed = true;

                    // Consume fuel item
                    let new_count = self.items[SLOT_FUEL].count() - 1;
                    self.items[SLOT_FUEL].set_count(new_count);

                    // Handle fuel remainder (e.g., bucket after lava bucket)
                    if self.items[SLOT_FUEL].is_empty() {
                        // TODO: Check vanilla fuel remainders
                        // For now, buckets return empty buckets
                        if fuel.item() == &*vanilla_items::LAVA_BUCKET {
                            self.items[SLOT_FUEL] =
                                ItemStack::with_count(&vanilla_items::BUCKET, 1);
                        }
                    }
                }
            }
        }

        // Progress cooking
        if self.is_lit() && can_burn {
            self.cooking_timer.fetch_add(1, Ordering::Relaxed);

            let cook_time = self.cooking_timer.load(Ordering::Relaxed);
            let cook_total = self.cooking_total_time.load(Ordering::Relaxed);

            if cook_time >= cook_total {
                // Smelt complete!
                self.cooking_timer.store(0, Ordering::Relaxed);
                if self.burn(&recipe) {
                    changed = true;
                }
            }
        } else {
            // Reset cooking progress if can't cook
            self.cooking_timer.store(0, Ordering::Relaxed);
        }

        // TODO: mark the block entity changed when `changed` is true once the
        // furnace dirty-flag TODO lands.
        let _ = changed;
    }

    /// Check if we can smelt (output slot can accept result)
    fn can_burn(&self, recipe_result: &Option<ItemStack>, max_stack_size: i32) -> bool {
        let Some(result) = recipe_result else {
            return false;
        };

        if self.items[SLOT_INPUT].is_empty() {
            return false;
        }

        let output = &self.items[SLOT_RESULT];

        if output.is_empty() {
            return true;
        }

        // Check if output matches and has room
        if output.item() != result.item() {
            return false;
        }

        output.count() < max_stack_size && output.count() < output.max_stack_size()
    }

    /// Perform the actual smelting (consume input, add output)
    fn burn(&mut self, recipe_result: &Option<ItemStack>) -> bool {
        let Some(result) = recipe_result else {
            return false;
        };

        if !self.can_burn(recipe_result, self.items[SLOT_RESULT].max_stack_size()) {
            return false;
        }

        // Consume input
        let input_count = self.items[SLOT_INPUT].count();
        self.items[SLOT_INPUT].set_count(input_count - 1);

        // Add output
        if self.items[SLOT_RESULT].is_empty() {
            self.items[SLOT_RESULT] = result.clone();
        } else {
            let output_count = self.items[SLOT_RESULT].count();
            let result_count = result.count();
            self.items[SLOT_RESULT].set_count(output_count + result_count);
        }

        // TODO: Track recipe usage for XP
        // self.recipes_used.push((recipe.id.clone(), 1));

        true
    }

    fn load_nbt(&mut self, nbt: &NbtCompoundView<'_, '_>) {
        // Load timers
        if let Some(burn_time) = nbt.short(BURN_TIME_TAG) {
            self.lit_time_remaining
                .store(burn_time as i32, Ordering::Relaxed);
        }
        if let Some(lit_duration) = nbt.short(LIT_DURATION_TAG) {
            self.lit_total_time
                .store(lit_duration as i32, Ordering::Relaxed);
        }
        if let Some(cook_time) = nbt.short(COOKING_TIMER_TAG) {
            self.cooking_timer
                .store(cook_time as i32, Ordering::Relaxed);
        }
        if let Some(cook_total) = nbt.short(COOKING_TOTAL_TIME_TAG) {
            self.cooking_total_time
                .store(cook_total as i32, Ordering::Relaxed);
        }

        // TODO: Load items from NBT
        // TODO: Load recipes_used
    }

    fn save_nbt(&self, nbt: &mut NbtCompound) {
        // Save timers
        nbt.insert(
            BURN_TIME_TAG,
            self.lit_time_remaining.load(Ordering::Relaxed) as i16,
        );
        nbt.insert(
            LIT_DURATION_TAG,
            self.lit_total_time.load(Ordering::Relaxed) as i16,
        );
        nbt.insert(
            COOKING_TIMER_TAG,
            self.cooking_timer.load(Ordering::Relaxed) as i16,
        );
        nbt.insert(
            COOKING_TOTAL_TIME_TAG,
            self.cooking_total_time.load(Ordering::Relaxed) as i16,
        );

        // TODO: Save items to NBT
        // TODO: Save recipes_used
    }
}

impl Container for FurnaceContainer {
    fn items(&self) -> &[ItemStack] {
        &self.items
    }

    fn items_mut(&mut self) -> &mut [ItemStack] {
        &mut self.items
    }

    fn set_changed(&mut self) {
        // Mark dirty - handled by block entity
    }
}
