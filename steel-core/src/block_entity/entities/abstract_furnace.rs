//! Shared furnace, blast-furnace, and smoker block-entity implementation.

use std::array::from_fn;
use std::mem;
use std::sync::{Arc, Weak};

use glam::DVec3;
use rustc_hash::FxHashMap;
use simdnbt::ToNbtTag as _;
use simdnbt::borrow::{BaseNbtCompound as BorrowedNbtCompound, NbtCompound as NbtCompoundView};
use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use steel_registry::block_entity_type::BlockEntityTypeRef;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{BlockStateProperties, BoolProperty, Direction};
use steel_registry::item_stack::ItemStack;
use steel_registry::recipe::{
    CachedRecipeCheck, CookingRecipe, RecipeType, SingleItemRecipeInput, TypedRecipeRef,
    vanilla_recipe_types,
};
use steel_registry::{REGISTRY, vanilla_block_entity_types, vanilla_items};
use steel_utils::locks::IntoShared;
use steel_utils::{
    BlockPos, BlockStateId, DowncastType, DowncastTypeKey, Identifier, locks::SyncMutex,
    types::UpdateFlags,
};

use crate::block_entity::{BlockEntity, BlockEntityBase};
use crate::entity::entities::ExperienceOrbEntity;
use crate::inventory::container::Container;
use crate::inventory::fuel_values::VANILLA_FUEL_VALUES;
use crate::inventory::lock::{ContainerRef, SharedContainer};
use crate::world::World;

pub const FURNACE_SLOTS: usize = 3;
pub const SLOT_INPUT: usize = 0;
pub const SLOT_FUEL: usize = 1;
pub const SLOT_RESULT: usize = 2;

const SLOTS_FOR_UP: &[usize] = &[SLOT_INPUT];
const SLOTS_FOR_DOWN: &[usize] = &[SLOT_RESULT, SLOT_FUEL];
const SLOTS_FOR_SIDES: &[usize] = &[SLOT_FUEL];
const DEFAULT_COOKING_TIME: i32 = 200;
const BURN_COOL_SPEED: i32 = 2;
const LIT: &BoolProperty = &BlockStateProperties::LIT;

/// The operational differences between Vanilla's three furnace block entities.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FurnaceKind {
    /// Standard smelting furnace.
    Furnace,
    /// Ore and metal blasting furnace.
    BlastFurnace,
    /// Food smoking furnace.
    Smoker,
}

impl FurnaceKind {
    /// Returns the recipe type processed by this furnace.
    #[must_use]
    pub const fn recipe_type(self) -> &'static RecipeType<CookingRecipe, SingleItemRecipeInput> {
        match self {
            Self::Furnace => &vanilla_recipe_types::SMELTING,
            Self::BlastFurnace => &vanilla_recipe_types::BLASTING,
            Self::Smoker => &vanilla_recipe_types::SMOKING,
        }
    }

    #[must_use]
    const fn fuel_duration(self, vanilla_duration: i32) -> i32 {
        match self {
            Self::Furnace => vanilla_duration,
            Self::BlastFurnace | Self::Smoker => vanilla_duration / 2,
        }
    }
}

/// Independently lockable furnace inventory and progress data.
pub struct FurnaceContainer {
    kind: FurnaceKind,
    items: [ItemStack; FURNACE_SLOTS],
    lit_time_remaining: i32,
    lit_total_time: i32,
    cooking_timer: i32,
    cooking_total_time: i32,
    recipes_used: FxHashMap<Identifier, i32>,
    quick_check: CachedRecipeCheck<CookingRecipe, SingleItemRecipeInput>,
}

struct FurnaceTickResult {
    changed: bool,
    lit_changed: bool,
    is_lit: bool,
}

// SAFETY: This Steel-owned key uniquely identifies furnace inventory/progress storage.
unsafe impl DowncastType for FurnaceContainer {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:container/abstract_furnace");
}

impl FurnaceContainer {
    fn new(kind: FurnaceKind) -> Self {
        Self {
            kind,
            items: from_fn(|_| ItemStack::empty()),
            lit_time_remaining: 0,
            lit_total_time: 0,
            cooking_timer: 0,
            cooking_total_time: 0,
            recipes_used: FxHashMap::default(),
            quick_check: CachedRecipeCheck::new(kind.recipe_type()),
        }
    }

    #[must_use]
    pub(crate) const fn data(&self) -> [i16; 4] {
        [
            self.lit_time_remaining as i16,
            self.lit_total_time as i16,
            self.cooking_timer as i16,
            self.cooking_total_time as i16,
        ]
    }

    fn recipe_for_input(&mut self) -> Option<TypedRecipeRef<CookingRecipe, SingleItemRecipeInput>> {
        let input = SingleItemRecipeInput::new(self.items[SLOT_INPUT].clone());
        self.quick_check.find_match(&REGISTRY.recipes, &input)
    }

    fn reset_cooking_for_input(&mut self) {
        self.cooking_total_time = self
            .recipe_for_input()
            .map_or(DEFAULT_COOKING_TIME, |recipe| recipe.data().cooking_time);
        self.cooking_timer = 0;
    }

    fn can_burn(&self, result: &ItemStack) -> bool {
        if result.is_empty() {
            return false;
        }
        let current = &self.items[SLOT_RESULT];
        if current.is_empty() {
            return true;
        }
        if !ItemStack::is_same_item_same_components(current, result) {
            return false;
        }
        current.count() + result.count() <= self.get_max_stack_size_for_item(result)
    }

    fn consume_fuel(&mut self) {
        let fuel_item = self.items[SLOT_FUEL].item();
        self.items[SLOT_FUEL].shrink(1);
        if self.items[SLOT_FUEL].is_empty() {
            self.items[SLOT_FUEL] = fuel_item.get_crafting_remainder();
        }
    }

    fn burn(&mut self, recipe: TypedRecipeRef<CookingRecipe, SingleItemRecipeInput>) {
        let result = recipe.data().result.create();
        if self.items[SLOT_RESULT].is_empty() {
            self.items[SLOT_RESULT] = result;
        } else {
            self.items[SLOT_RESULT].grow(result.count());
        }

        if self.items[SLOT_INPUT].is(&vanilla_items::WET_SPONGE)
            && self.items[SLOT_FUEL].is(&vanilla_items::BUCKET)
        {
            self.items[SLOT_FUEL] = ItemStack::new(&vanilla_items::WATER_BUCKET);
        }
        self.items[SLOT_INPUT].shrink(1);
        *self.recipes_used.entry(recipe.key().clone()).or_default() += 1;
    }

    fn tick(&mut self) -> FurnaceTickResult {
        let was_lit = self.lit_time_remaining > 0;
        if was_lit {
            self.lit_time_remaining -= 1;
        }
        let mut is_lit = self.lit_time_remaining > 0;
        let mut changed = false;
        let has_ingredient = !self.items[SLOT_INPUT].is_empty();
        let has_fuel = !self.items[SLOT_FUEL].is_empty();

        if is_lit || (has_fuel && has_ingredient) {
            if let Some(recipe) = self.recipe_for_input() {
                let result = recipe.data().result.create();
                if self.can_burn(&result) {
                    if !is_lit {
                        let vanilla_duration =
                            VANILLA_FUEL_VALUES.burn_duration(self.items[SLOT_FUEL].item());
                        let duration = self.kind.fuel_duration(vanilla_duration);
                        self.lit_time_remaining = duration;
                        self.lit_total_time = duration;
                        if duration > 0 {
                            self.consume_fuel();
                            is_lit = true;
                            changed = true;
                        }
                    }

                    if is_lit {
                        self.cooking_timer += 1;
                        if self.cooking_timer == self.cooking_total_time {
                            self.cooking_timer = 0;
                            self.cooking_total_time = recipe.data().cooking_time;
                            self.burn(recipe);
                            changed = true;
                        }
                    } else {
                        self.cooking_timer = 0;
                    }
                } else {
                    self.cooking_timer = 0;
                }
            } else {
                self.cooking_timer = 0;
            }
        } else if self.cooking_timer > 0 {
            self.cooking_timer =
                (self.cooking_timer - BURN_COOL_SPEED).clamp(0, self.cooking_total_time);
        }

        FurnaceTickResult {
            changed,
            lit_changed: was_lit != is_lit,
            is_lit,
        }
    }

    pub(crate) fn take_recipes_used(&mut self) -> FxHashMap<Identifier, i32> {
        mem::take(&mut self.recipes_used)
    }
}

impl Container for FurnaceContainer {
    fn items(&self) -> &[ItemStack] {
        &self.items
    }

    fn items_mut(&mut self) -> &mut [ItemStack] {
        &mut self.items
    }

    fn set_item(&mut self, slot: usize, mut stack: ItemStack) {
        if slot >= FURNACE_SLOTS {
            return;
        }
        let same_input = slot == SLOT_INPUT
            && !stack.is_empty()
            && ItemStack::is_same_item_same_components(&self.items[slot], &stack);
        let max_size = self.get_max_stack_size_for_item(&stack);
        if stack.count() > max_size {
            stack.set_count(max_size);
        }
        self.items[slot] = stack;
        if slot == SLOT_INPUT && !same_input {
            self.reset_cooking_for_input();
        }
    }

    fn get_max_stack_size(&self) -> i32 {
        64
    }

    fn set_changed(&mut self) {}

    fn can_place_item(&self, slot: usize, stack: &ItemStack) -> bool {
        match slot {
            SLOT_RESULT => false,
            SLOT_FUEL => {
                VANILLA_FUEL_VALUES.is_fuel(stack.item())
                    || (stack.is(&vanilla_items::BUCKET)
                        && !self.items[SLOT_FUEL].is(&vanilla_items::BUCKET))
            }
            _ => true,
        }
    }

    fn slots_for_face(&self, direction: Direction) -> Option<&'static [usize]> {
        Some(match direction {
            Direction::Down => SLOTS_FOR_DOWN,
            Direction::Up => SLOTS_FOR_UP,
            _ => SLOTS_FOR_SIDES,
        })
    }

    fn can_take_item_through_face(
        &self,
        slot: usize,
        stack: &ItemStack,
        direction: Direction,
    ) -> bool {
        direction != Direction::Down
            || slot != SLOT_FUEL
            || stack.is(&vanilla_items::WATER_BUCKET)
            || stack.is(&vanilla_items::BUCKET)
    }
}

/// Shared implementation owned by one of the three concrete furnace entities.
pub struct AbstractFurnaceBlockEntity {
    base: Arc<BlockEntityBase>,
    container: Arc<SyncMutex<FurnaceContainer>>,
    container_ref: ContainerRef,
}

impl AbstractFurnaceBlockEntity {
    fn new(
        block_entity_type: BlockEntityTypeRef,
        kind: FurnaceKind,
        level: Weak<World>,
        pos: BlockPos,
        state: BlockStateId,
    ) -> Self {
        let base = Arc::new(BlockEntityBase::new(block_entity_type, level, pos, state));
        let container = FurnaceContainer::new(kind).into_shared();
        let shared: SharedContainer = container.clone();
        Self {
            container_ref: ContainerRef::owned_by_block_entity(shared, Arc::clone(&base)),
            base,
            container,
        }
    }

    #[must_use]
    pub fn container_ref(&self) -> ContainerRef {
        self.container_ref.clone()
    }

    fn server_tick(&self, world: &Arc<World>) {
        let result = self.container.lock().tick();

        if result.lit_changed {
            let state = self.base.block_state().set_value(LIT, result.is_lit);
            world.set_block(self.base.pos(), state, UpdateFlags::UPDATE_ALL);
        }
        if result.changed || result.lit_changed {
            self.base.set_changed();
        }
    }

    fn load_additional(&self, nbt: &BorrowedNbtCompound<'_>) {
        let nbt: NbtCompoundView<'_, '_> = nbt.into();
        let mut furnace = self.container.lock();
        furnace.items.fill(ItemStack::empty());
        if let Some(items) = nbt.list("Items").and_then(|list| list.compounds()) {
            for compound in items {
                let Some(slot) = compound.byte("Slot").map(|slot| slot as usize) else {
                    continue;
                };
                if slot < FURNACE_SLOTS
                    && let Some(stack) = ItemStack::from_borrowed_compound(&compound)
                {
                    furnace.items[slot] = stack;
                }
            }
        }
        furnace.cooking_timer = i32::from(nbt.short("cooking_time_spent").unwrap_or(0));
        furnace.cooking_total_time = i32::from(nbt.short("cooking_total_time").unwrap_or(0));
        furnace.lit_time_remaining = i32::from(nbt.short("lit_time_remaining").unwrap_or(0));
        furnace.lit_total_time = i32::from(nbt.short("lit_total_time").unwrap_or(0));
        furnace.recipes_used.clear();
        if let Some(recipes) = nbt.compound("RecipesUsed") {
            for (key, value) in recipes.iter() {
                let Some(count) = value.int() else {
                    continue;
                };
                if let Ok(identifier) = key.to_string().parse() {
                    furnace.recipes_used.insert(identifier, count);
                }
            }
        }
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        let furnace = self.container.lock();
        nbt.insert("cooking_time_spent", furnace.cooking_timer as i16);
        nbt.insert("cooking_total_time", furnace.cooking_total_time as i16);
        nbt.insert("lit_time_remaining", furnace.lit_time_remaining as i16);
        nbt.insert("lit_total_time", furnace.lit_total_time as i16);

        let mut items = Vec::new();
        for (slot, stack) in furnace.items.iter().enumerate() {
            if !stack.is_empty()
                && let NbtTag::Compound(mut item) = stack.clone().to_nbt_tag()
            {
                item.insert("Slot", slot as i8);
                items.push(item);
            }
        }
        nbt.insert("Items", NbtList::Compound(items));

        let mut recipes = NbtCompound::new();
        for (key, count) in &furnace.recipes_used {
            recipes.insert(key.to_string(), *count);
        }
        nbt.insert("RecipesUsed", recipes);
    }

    fn pre_remove_side_effects(&self, world: &Arc<World>, pos: BlockPos) {
        let (items, recipes) = {
            let mut furnace = self.container.lock();
            let items = mem::replace(&mut furnace.items, from_fn(|_| ItemStack::empty()));
            (items, furnace.take_recipes_used())
        };
        for item in items {
            world.drop_item_stack(pos, item);
        }
        pop_furnace_experience(
            world,
            DVec3::new(
                f64::from(pos.x()) + 0.5,
                f64::from(pos.y()) + 0.5,
                f64::from(pos.z()) + 0.5,
            ),
            recipes,
        );
    }
}

pub(crate) fn pop_furnace_experience(
    world: &Arc<World>,
    position: DVec3,
    recipes: FxHashMap<Identifier, i32>,
) {
    for (key, amount) in recipes {
        let Some(recipe) = REGISTRY.recipes.by_key(&key) else {
            continue;
        };
        let Some(cooking) = recipe.downcast_data::<CookingRecipe>() else {
            continue;
        };
        let exact = amount as f32 * cooking.experience;
        let mut reward = exact.floor() as i32;
        if exact.fract() != 0.0 && rand::random::<f32>() < exact.fract() {
            reward += 1;
        }
        ExperienceOrbEntity::award(world, position, reward);
    }
}

macro_rules! furnace_block_entity {
    ($name:ident, $key:literal, $type:ident, $kind:ident) => {
        #[doc = concat!("Concrete Vanilla `", stringify!($name), "` implementation.")]
        pub struct $name {
            common: AbstractFurnaceBlockEntity,
        }

        // SAFETY: This Steel-owned key uniquely identifies this concrete block entity.
        unsafe impl DowncastType for $name {
            const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new($key);
        }

        impl $name {
            /// Creates the block entity at a live world position.
            #[must_use]
            pub fn new(level: Weak<World>, pos: BlockPos, state: BlockStateId) -> Self {
                Self {
                    common: AbstractFurnaceBlockEntity::new(
                        &vanilla_block_entity_types::$type,
                        FurnaceKind::$kind,
                        level,
                        pos,
                        state,
                    ),
                }
            }
        }

        impl BlockEntity for $name {
            fn base(&self) -> &BlockEntityBase {
                &self.common.base
            }

            fn pre_remove_side_effects(&self, pos: BlockPos, _state: BlockStateId) {
                if let Some(world) = self.get_level() {
                    self.common.pre_remove_side_effects(&world, pos);
                }
            }

            fn load_additional(&self, nbt: &BorrowedNbtCompound<'_>) {
                self.common.load_additional(nbt);
            }

            fn save_additional(&self, nbt: &mut NbtCompound) {
                self.common.save_additional(nbt);
            }

            fn tick(&self, world: &Arc<World>) {
                self.common.server_tick(world);
            }

            fn container_ref(&self) -> Option<ContainerRef> {
                Some(self.common.container_ref())
            }
        }
    };
}

furnace_block_entity!(
    FurnaceBlockEntity,
    "steel:block_entity/furnace",
    FURNACE,
    Furnace
);
furnace_block_entity!(
    BlastFurnaceBlockEntity,
    "steel:block_entity/blast_furnace",
    BLAST_FURNACE,
    BlastFurnace
);
furnace_block_entity!(
    SmokerBlockEntity,
    "steel:block_entity/smoker",
    SMOKER,
    Smoker
);

#[cfg(test)]
mod tests {
    use steel_registry::{init_vanilla_registry, vanilla_items};

    use super::*;

    fn cook_iron(kind: FurnaceKind, expected_ticks: usize, expected_fuel_time: i32) {
        init_vanilla_registry();
        let mut furnace = FurnaceContainer::new(kind);
        furnace.set_item(SLOT_INPUT, ItemStack::new(&vanilla_items::IRON_ORE));
        furnace.set_item(SLOT_FUEL, ItemStack::new(&vanilla_items::COAL));

        for _ in 0..expected_ticks {
            furnace.tick();
        }

        assert!(furnace.items[SLOT_INPUT].is_empty());
        assert!(furnace.items[SLOT_FUEL].is_empty());
        assert!(furnace.items[SLOT_RESULT].is(&vanilla_items::IRON_INGOT));
        assert_eq!(furnace.items[SLOT_RESULT].count(), 1);
        assert_eq!(furnace.lit_total_time, expected_fuel_time);
        assert_eq!(furnace.recipes_used.values().sum::<i32>(), 1);
    }

    #[test]
    fn furnace_consumes_one_fuel_and_finishes_at_recipe_cooking_time() {
        cook_iron(FurnaceKind::Furnace, 200, 1600);
    }

    #[test]
    fn blast_furnace_uses_blasting_time_and_half_fuel_duration() {
        cook_iron(FurnaceKind::BlastFurnace, 100, 800);
    }

    #[test]
    fn furnace_sided_inventory_matches_vanilla_faces_and_bucket_rule() {
        init_vanilla_registry();
        let furnace = FurnaceContainer::new(FurnaceKind::Furnace);

        assert_eq!(
            furnace.slots_for_face(Direction::Up),
            Some(&[SLOT_INPUT][..])
        );
        assert_eq!(
            furnace.slots_for_face(Direction::Down),
            Some(&[SLOT_RESULT, SLOT_FUEL][..])
        );
        assert_eq!(
            furnace.slots_for_face(Direction::North),
            Some(&[SLOT_FUEL][..])
        );
        assert!(!furnace.can_take_item_through_face(
            SLOT_FUEL,
            &ItemStack::new(&vanilla_items::COAL),
            Direction::Down,
        ));
        assert!(furnace.can_take_item_through_face(
            SLOT_FUEL,
            &ItemStack::new(&vanilla_items::WATER_BUCKET),
            Direction::Down,
        ));
    }
}
