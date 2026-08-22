use std::{
    mem,
    sync::{Arc, Weak},
};

use simdnbt::borrow::{BaseNbtCompound as BorrowedNbtCompound, NbtCompound as NbtCompoundView};
use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use simdnbt::ToNbtTag;
use steel_registry::{
    item_stack::ItemStack,
    recipe::BlastingRecipe,
    vanilla_block_entity_types,
    vanilla_items,
    REGISTRY,
};
use steel_utils::{BlockPos, BlockStateId, DowncastType, DowncastTypeKey, locks::SyncMutex};

use crate::block_entity::{BlockEntity, BlockEntityBase, BlockEntityLifecycleExt};
use crate::inventory::container::Container;
use crate::inventory::lock::{ContainerRef, SharedContainer};
use crate::world::World;

/// Total number of slots in a blast furnace.
pub const BLAST_FURNACE_SLOTS: usize = 3;
/// Slot index for ingredient input.
pub const SLOT_INPUT: usize = 0;
/// Slot index for fuel.
pub const SLOT_FUEL: usize = 1;
/// Slot index for cooking output.
pub const SLOT_OUTPUT: usize = 2;

/// Blast furnace block entity managing inventory and smelting operations.
pub struct BlastFurnaceBlockEntity {
    base: Arc<BlockEntityBase>,
    container: Arc<SyncMutex<BlastFurnaceContainer>>,
    container_ref: ContainerRef,
}

/// Backing inventory and timer state for a blast furnace.
pub struct BlastFurnaceContainer {
    items: Vec<ItemStack>,
    /// Remaining burn time for currently consumed fuel.
    pub lit_time: i32,
    /// Total burn duration of the currently consumed fuel.
    pub lit_duration: i32,
    /// Current cook progress in ticks.
    pub cooking_progress: i32,
    /// Total cook time required for the current recipe.
    pub cooking_total_time: i32,
}

unsafe impl DowncastType for BlastFurnaceBlockEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:block_entity/blast_furnace");
}

unsafe impl DowncastType for BlastFurnaceContainer {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:container/blast_furnace");
}

impl BlastFurnaceBlockEntity {
    /// Creates a new blast furnace block entity.
    #[must_use]
    pub fn new(level: Weak<World>, pos: BlockPos, state: BlockStateId) -> Self {
        let base = Arc::new(BlockEntityBase::new(
            &vanilla_block_entity_types::BLAST_FURNACE,
            level,
            pos,
            state,
        ));
        let container = Arc::new(SyncMutex::new(BlastFurnaceContainer {
            items: vec![ItemStack::empty(); BLAST_FURNACE_SLOTS],
            lit_time: 0,
            lit_duration: 0,
            cooking_progress: 0,
            cooking_total_time: 0,
        }));
        let shared_container: SharedContainer = container.clone();
        Self {
            container_ref: ContainerRef::owned_by_block_entity(shared_container, Arc::clone(&base)),
            base,
            container,
        }
    }
}

pub fn get_fuel_burn_time(item: &ItemStack) -> i32 {
    if item.is_empty() {
        return 0;
    }
    
    let item_type = item.item();
    if item_type == &*vanilla_items::COAL || item_type == &*vanilla_items::CHARCOAL {
        return 1600;
    }
    if item_type == &*vanilla_items::COAL_BLOCK {
        return 16000;
    }
    if item_type == &*vanilla_items::BLAZE_ROD {
        return 2400;
    }
    if item_type == &*vanilla_items::LAVA_BUCKET {
        return 20000;
    }
    if item_type == &*vanilla_items::DRIED_KELP_BLOCK {
        return 4000;
    }
    if item_type == &*vanilla_items::STICK {
        return 100;
    }
    if item_type == &*vanilla_items::BAMBOO {
        return 50;
    }
    if item_type == &*vanilla_items::BAMBOO_MOSAIC {
        return 300;
    }
    
    let name = item_type.key.path.as_ref();
    if name.contains("planks") || name.contains("log") || name.contains("wooden") || name.contains("wood") {
        return 300;
    }

    0
}

fn has_recipe(container: &BlastFurnaceContainer, recipe: Option<&BlastingRecipe>) -> bool {
    let input = &container.items[SLOT_INPUT];
    if input.is_empty() {
        return false;
    }
    
    let Some(recipe) = recipe else {
        return false;
    };
    
    let result = recipe.assemble_result(input.count(), false);
    if result.is_empty() {
        return false;
    }

    let current_output = &container.items[SLOT_OUTPUT];
    if current_output.is_empty() {
        return true;
    }

    if !current_output.is(result.item()) {
        return false;
    }

    let max_stack = current_output.max_stack_size();
    current_output.count() + result.count() <= max_stack
}

fn smelt_item(container: &mut BlastFurnaceContainer, recipe: Option<&BlastingRecipe>) {
    if !has_recipe(container, recipe) {
        return;
    }
    
    let recipe = recipe.unwrap();
    let result = recipe.assemble_result(container.items[SLOT_INPUT].count(), false);

    if container.items[SLOT_OUTPUT].is_empty() {
        container.items[SLOT_OUTPUT] = result;
    } else if container.items[SLOT_OUTPUT].is(result.item()) {
        container.items[SLOT_OUTPUT].grow(result.count());
    }

    container.items[SLOT_INPUT].shrink(1);
}

impl BlockEntity for BlastFurnaceBlockEntity {
    fn base(&self) -> &BlockEntityBase {
        &self.base
    }

    fn pre_remove_side_effects(&self, pos: BlockPos, _state: BlockStateId) {
        let items = {
            let mut container = self.container.lock();
            mem::replace(&mut container.items, vec![ItemStack::empty(); BLAST_FURNACE_SLOTS])
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

        if let Some(items_list) = nbt_view.list("Items")
            && let Some(compounds) = items_list.compounds()
        {
            for compound in compounds {
                if let Some(slot) = compound.byte("Slot") {
                    let slot = slot as usize;
                    if slot < BLAST_FURNACE_SLOTS {
                        if let Some(item) = ItemStack::from_borrowed_compound(&compound) {
                            container.items[slot] = item;
                        }
                    }
                }
            }
        }
        
        container.lit_time = nbt_view.short("BurnTime").unwrap_or(0) as i32;
        container.cooking_progress = nbt_view.short("CookTime").unwrap_or(0) as i32;
        container.cooking_total_time = nbt_view.short("CookTimeTotal").unwrap_or(0) as i32;
        container.lit_duration = get_fuel_burn_time(&container.items[SLOT_FUEL]);
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        let container = self.container.lock();
        let mut items: Vec<NbtCompound> = Vec::new();
        for (slot, item) in container.items.iter().enumerate() {
            if !item.is_empty() {
                if let NbtTag::Compound(mut item_nbt) = item.clone().to_nbt_tag() {
                    item_nbt.insert("Slot", slot as i8);
                    items.push(item_nbt);
                }
            }
        }
        nbt.insert("Items", NbtList::Compound(items));
        nbt.insert("BurnTime", container.lit_time as i16);
        nbt.insert("CookTime", container.cooking_progress as i16);
        nbt.insert("CookTimeTotal", container.cooking_total_time as i16);
    }

    fn tick(&self, _world: &Arc<World>) {
        let mut container = self.container.lock();
        
        let was_lit = container.lit_time > 0;
        if container.lit_time > 0 {
            container.lit_time -= 1;
        }

        let input_stack = container.items[SLOT_INPUT].clone();
        
        let recipe = if !input_stack.is_empty() {
            REGISTRY.recipes.iter_blasting().find(|r| r.matches(&input_stack))
        } else {
            None
        };

        let has_valid_recipe = has_recipe(&container, recipe);
        let mut state_changed = false;

        if container.lit_time == 0 && has_valid_recipe {
            let fuel_time = get_fuel_burn_time(&container.items[SLOT_FUEL]);
            if fuel_time > 0 {
                container.lit_time = fuel_time;
                container.lit_duration = fuel_time;
                
                let fuel_stack = &mut container.items[SLOT_FUEL];
                if fuel_stack.item() == &*vanilla_items::LAVA_BUCKET {
                    *fuel_stack = ItemStack::new(&vanilla_items::BUCKET);
                } else {
                    fuel_stack.shrink(1);
                }
                state_changed = true;
            }
        }

        if container.lit_time > 0 && has_valid_recipe {
            container.cooking_progress += 1;
            
            let recipe_time = recipe.map_or(100, |r| r.cooking_time);
            if container.cooking_total_time != recipe_time {
                container.cooking_total_time = recipe_time;
            }

            if container.cooking_progress >= container.cooking_total_time {
                container.cooking_progress = 0;
                container.cooking_total_time = recipe_time;
                smelt_item(&mut container, recipe);
                state_changed = true;
            }
        } else {
            if container.cooking_progress > 0 {
                container.cooking_progress = (container.cooking_progress - 2).max(0);
            }
            if !has_valid_recipe {
                container.cooking_total_time = 0;
            }
        }

        let is_lit = container.lit_time > 0;
        if was_lit != is_lit {
            use steel_registry::blocks::properties::BlockStateProperties;
            use steel_registry::blocks::block_state_ext::BlockStateExt as _;
            let state = self.get_block_state().set_value(&BlockStateProperties::LIT, is_lit);
            drop(container);
            self.set_block_state(state);
            return;
        }

        if state_changed {
            drop(container);
            self.set_changed();
        }
    }

    fn container_ref(&self) -> Option<ContainerRef> {
        Some(self.container_ref.clone())
    }
}

impl Container for BlastFurnaceContainer {
    fn items(&self) -> &[ItemStack] {
        &self.items
    }

    fn items_mut(&mut self) -> &mut [ItemStack] {
        &mut self.items
    }

    fn get_container_size(&self) -> usize {
        BLAST_FURNACE_SLOTS
    }

    fn set_item(&mut self, slot: usize, mut stack: ItemStack) {
        if slot < BLAST_FURNACE_SLOTS {
            let max_stack = self.get_max_stack_size_for_item(&stack);
            if !stack.is_empty() && stack.count() > max_stack {
                stack.set_count(max_stack);
            }
            
            if slot == SLOT_INPUT {
                let recipe_matches = if !stack.is_empty() {
                    REGISTRY.recipes.iter_blasting().any(|r| r.matches(&stack))
                } else {
                    false
                };
                if !recipe_matches {
                    self.cooking_total_time = 0;
                }
            }

            self.items[slot] = stack;
        }
    }

    fn get_max_stack_size(&self) -> i32 {
        64
    }

    fn set_changed(&mut self) {}
}

#[cfg(test)]
mod tests {
    use steel_registry::{init_vanilla_registry, vanilla_blocks, vanilla_items};
    use crate::test_support::fresh_test_world;
    use super::*;

    fn setup_test_entity(world: &Arc<World>) -> BlastFurnaceBlockEntity {
        init_vanilla_registry();
        BlastFurnaceBlockEntity::new(
            Arc::downgrade(world),
            BlockPos::new(0, 0, 0),
            vanilla_blocks::BLAST_FURNACE.default_state(),
        )
    }

    #[test]
    fn fuel_burn_times() {
        init_vanilla_registry();
        assert_eq!(get_fuel_burn_time(&ItemStack::new(&vanilla_items::COAL)), 1600);
        assert_eq!(get_fuel_burn_time(&ItemStack::new(&vanilla_items::LAVA_BUCKET)), 20000);
        assert_eq!(get_fuel_burn_time(&ItemStack::new(&vanilla_items::OAK_PLANKS)), 300);
        assert_eq!(get_fuel_burn_time(&ItemStack::new(&vanilla_items::IRON_INGOT)), 0);
    }

    #[test]
    fn test_smelting_lifecycle() {
        let world = fresh_test_world("smelt_lifecycle");
        let entity = setup_test_entity(&world);
        
        {
            let mut container = entity.container.lock();
            container.set_item(SLOT_INPUT, ItemStack::with_count(&vanilla_items::RAW_IRON, 1));
            container.set_item(SLOT_FUEL, ItemStack::with_count(&vanilla_items::COAL, 1));
        }

        entity.tick(&world);
        {
            let container = entity.container.lock();
            assert_eq!(container.lit_time, 1600);
            assert_eq!(container.lit_duration, 1600);
            assert_eq!(container.items[SLOT_FUEL].count(), 0);
            assert_eq!(container.cooking_progress, 1);
            assert_eq!(container.cooking_total_time, 100);
        }

        for _ in 0..99 {
            entity.tick(&world);
        }

        {
            let container = entity.container.lock();
            assert_eq!(container.lit_time, 1501);
            assert_eq!(container.items[SLOT_INPUT].count(), 0);
            assert_eq!(container.items[SLOT_OUTPUT].item(), &*vanilla_items::IRON_INGOT);
            assert_eq!(container.items[SLOT_OUTPUT].count(), 1);
            assert_eq!(container.cooking_progress, 0);
        }
    }

    #[test]
    fn test_cooking_aborts_without_input() {
        let world = fresh_test_world("cooking_aborts");
        let entity = setup_test_entity(&world);

        {
            let mut container = entity.container.lock();
            container.set_item(SLOT_INPUT, ItemStack::with_count(&vanilla_items::RAW_IRON, 1));
            container.set_item(SLOT_FUEL, ItemStack::with_count(&vanilla_items::COAL, 1));
        }

        entity.tick(&world);

        {
            let mut container = entity.container.lock();
            assert_eq!(container.cooking_progress, 1);
            container.set_item(SLOT_INPUT, ItemStack::empty());
        }

        entity.tick(&world);

        {
            let container = entity.container.lock();
            assert_eq!(container.cooking_progress, 0);
            assert_eq!(container.cooking_total_time, 0);
        }
    }

    #[test]
    fn test_lava_bucket_fuel_leaves_empty_bucket() {
        let world = fresh_test_world("lava_fuel");
        let entity = setup_test_entity(&world);

        {
            let mut container = entity.container.lock();
            container.set_item(SLOT_INPUT, ItemStack::with_count(&vanilla_items::RAW_IRON, 1));
            container.set_item(SLOT_FUEL, ItemStack::new(&vanilla_items::LAVA_BUCKET));
        }

        entity.tick(&world);

        {
            let container = entity.container.lock();
            assert_eq!(container.lit_time, 20000);
            assert_eq!(container.items[SLOT_FUEL].item(), &*vanilla_items::BUCKET);
            assert_eq!(container.items[SLOT_FUEL].count(), 1);
        }
    }

    #[test]
    fn test_nbt_roundtrip() {
        let world = fresh_test_world("nbt_roundtrip");
        let entity = setup_test_entity(&world);

        {
            let mut container = entity.container.lock();
            container.set_item(SLOT_INPUT, ItemStack::with_count(&vanilla_items::RAW_IRON, 4));
            container.set_item(SLOT_OUTPUT, ItemStack::with_count(&vanilla_items::IRON_INGOT, 2));
            container.lit_time = 500;
            container.cooking_progress = 40;
            container.cooking_total_time = 100;
        }

        let mut nbt = NbtCompound::new();
        entity.save_additional(&mut nbt);

        let mut bytes = Vec::new();
        nbt.write(&mut bytes);
        let borrowed = simdnbt::borrow::read_compound(&mut std::io::Cursor::new(bytes.as_slice()))
            .expect("valid NBT compound");

        let loaded_entity = setup_test_entity(&world);
        loaded_entity.load_additional(&borrowed);

        let container = loaded_entity.container.lock();
        assert_eq!(container.items[SLOT_INPUT].count(), 4);
        assert_eq!(container.items[SLOT_INPUT].item(), &*vanilla_items::RAW_IRON);
        assert_eq!(container.items[SLOT_OUTPUT].count(), 2);
        assert_eq!(container.items[SLOT_OUTPUT].item(), &*vanilla_items::IRON_INGOT);
        assert_eq!(container.lit_time, 500);
        assert_eq!(container.cooking_progress, 40);
        assert_eq!(container.cooking_total_time, 100);
    }
}
