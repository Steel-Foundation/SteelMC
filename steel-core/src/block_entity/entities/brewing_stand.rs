//! Brewing stand block entity and inventory.

use std::array::from_fn;
use std::mem;
use std::sync::{Arc, Weak};

use simdnbt::ToNbtTag as _;
use simdnbt::borrow::{BaseNbtCompound as BorrowedNbtCompound, NbtCompound as NbtCompoundView};
use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use smallvec::SmallVec;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{BlockStateProperties, BoolProperty, Direction};
use steel_registry::data_components::vanilla_components::{BREWING_FUEL, BrewingFuel, CONTAINER};
use steel_registry::item_stack::ItemStack;
use steel_registry::items::ItemRef;
use steel_registry::level_events::SOUND_BREWING_STAND_BREW;
use steel_registry::recipe::{
    BrewingInput, BrewingRecipe, CachedRecipeCheck, PotionIngredient, vanilla_recipe_types,
};
use steel_registry::{REGISTRY, vanilla_block_entity_types, vanilla_items};
use steel_utils::locks::IntoShared;
use steel_utils::nbt::NbtValueInput as _;
use steel_utils::translations;
use steel_utils::types::UpdateFlags;
use steel_utils::{
    BlockPos, BlockStateId, DowncastType, DowncastTypeKey, TICKS_PER_SECOND, locks::SyncMutex,
};
use text_components::TextComponent;

use crate::block_entity::container_properties::ContainerProperties;
use crate::block_entity::{BlockEntity, BlockEntityBase, ImplicitComponentGetter};
use crate::inventory::container::{Container, load_all_items};
use crate::inventory::lock::{ContainerRef, SharedContainer};
use crate::nameable::Nameable;
use crate::player::Player;
use crate::world::World;

/// Number of slots in a brewing stand.
pub const BREWING_STAND_SLOTS: usize = 5;
pub const SLOT_POTION_0: usize = 0;
pub const SLOT_POTION_1: usize = 1;
pub const SLOT_POTION_2: usize = 2;
pub const SLOT_INGREDIENT: usize = 3;
pub const SLOT_FUEL: usize = 4;

pub const POTION_SLOTS: [usize; 3] = [SLOT_POTION_0, SLOT_POTION_1, SLOT_POTION_2];

const SLOTS_FOR_UP: &[usize] = &[SLOT_INGREDIENT];
const SLOTS_FOR_DOWN: &[usize] = &[SLOT_POTION_0, SLOT_POTION_1, SLOT_POTION_2, SLOT_INGREDIENT];
const SLOTS_FOR_SIDES: &[usize] = &[SLOT_POTION_0, SLOT_POTION_1, SLOT_POTION_2, SLOT_FUEL];

pub const DATA_BREW_TIME: usize = 0;
pub const DATA_FUEL_USES: usize = 1;
pub const DATA_TOTAL_BREW_TIME: usize = 2;
pub const DATA_TOTAL_FUEL_USES: usize = 3;
pub const BREWING_DATA_SLOTS: usize = 4;

const BREWING_TIME_SECONDS: i32 = 20;
const BREW_TICKS: f32 = (BREWING_TIME_SECONDS * TICKS_PER_SECOND) as f32;
const DEFAULT_BREW_TIME: i32 = 0;
const DEFAULT_TOTAL_BREW_TIME: i32 = 400;
const DEFAULT_FUEL: i32 = 0;
const DEFAULT_FUEL_USES: i32 = 20;
const DEFAULT_SPEED_MULTIPLIER: f32 = 1.0;

const HAS_BOTTLE: [&BoolProperty; 3] = [
    &BlockStateProperties::HAS_BOTTLE_0,
    &BlockStateProperties::HAS_BOTTLE_1,
    &BlockStateProperties::HAS_BOTTLE_2,
];

/// Result of one brewing-stand server tick.
struct BrewingStandTick {
    changed: bool,
    brewed: bool,
    dropped: SmallVec<[ItemStack; 1]>,
    potion_bits: [bool; 3],
    potion_bits_changed: bool,
}

/// Independently lockable brewing-stand inventory and progress data.
pub struct BrewingStandContainer {
    items: [ItemStack; BREWING_STAND_SLOTS],
    brew_time: i32,
    total_brew_time: i32,
    fuel: i32,
    total_fuel: i32,
    speed_multiplier: f32,
    last_potion_count: Option<[bool; 3]>,
    ingredient: ItemRef,
    quick_check: CachedRecipeCheck<BrewingRecipe, BrewingInput>,
}

// SAFETY: This Steel-owned key uniquely identifies brewing-stand inventory/progress storage.
unsafe impl DowncastType for BrewingStandContainer {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:container/brewing_stand");
}

impl BrewingStandContainer {
    fn new() -> Self {
        Self {
            items: from_fn(|_| ItemStack::empty()),
            brew_time: 0,
            total_brew_time: 0,
            fuel: 0,
            total_fuel: 0,
            speed_multiplier: DEFAULT_SPEED_MULTIPLIER,
            last_potion_count: None,
            ingredient: &vanilla_items::AIR,
            quick_check: CachedRecipeCheck::new(&vanilla_recipe_types::BREWING),
        }
    }

    /// Returns the four menu data values in data-slot order.
    #[must_use]
    pub(crate) const fn data(&self) -> [i16; BREWING_DATA_SLOTS] {
        let mut data = [0; BREWING_DATA_SLOTS];
        data[DATA_BREW_TIME] = self.brew_time as i16;
        data[DATA_FUEL_USES] = self.fuel as i16;
        data[DATA_TOTAL_BREW_TIME] = self.total_brew_time as i16;
        data[DATA_TOTAL_FUEL_USES] = self.total_fuel as i16;
        data
    }

    fn is_brewable(&mut self) -> bool {
        let ingredient = self.items[SLOT_INGREDIENT].clone();
        if ingredient.is_empty() || !REGISTRY.recipes.is_brewing_reagent(ingredient.item()) {
            return false;
        }

        for slot in POTION_SLOTS {
            let potion = self.items[slot].clone();
            if potion.is_empty() {
                continue;
            }
            let input = BrewingInput::new(potion, ingredient.clone());
            if self
                .quick_check
                .find_match(&REGISTRY.recipes, &input)
                .is_some()
            {
                return true;
            }
        }
        false
    }

    fn do_brew(&mut self, dropped: &mut SmallVec<[ItemStack; 1]>) {
        let ingredient = self.items[SLOT_INGREDIENT].clone();

        for slot in POTION_SLOTS {
            let container = self.items[slot].clone();
            let input = BrewingInput::new(container.clone(), ingredient.clone());
            self.items[slot] = self
                .quick_check
                .find_match(&REGISTRY.recipes, &input)
                .map_or(container, |recipe| recipe.data().output.create());
        }

        let remainder = ingredient.item().get_crafting_remainder();
        self.items[SLOT_INGREDIENT].shrink(1);
        if !remainder.is_empty() {
            if self.items[SLOT_INGREDIENT].is_empty() {
                self.items[SLOT_INGREDIENT] = remainder;
            } else {
                dropped.push(remainder);
            }
        }
    }

    fn consume_fuel(&mut self, brewing_fuel: &BrewingFuel, dropped: &mut SmallVec<[ItemStack; 1]>) {
        // Steel has no runtime context-provider registry, so the build script
        // folds literal providers and only preserves non-literal ones as
        // references. A reference falls back to the defaults below; every
        // built-in brewing fuel provider is literal, so this only affects
        // plugin fuel data.
        self.fuel = brewing_fuel.uses.constant_value().unwrap_or(0);
        self.total_fuel = self.fuel;
        self.speed_multiplier = brewing_fuel
            .speed_multiplier
            .constant_value()
            .unwrap_or(DEFAULT_SPEED_MULTIPLIER);

        let remainder = self.items[SLOT_FUEL].item().get_crafting_remainder();
        self.items[SLOT_FUEL].shrink(1);
        if !remainder.is_empty() {
            if self.items[SLOT_FUEL].is_empty() {
                self.items[SLOT_FUEL] = remainder;
            } else {
                dropped.push(remainder);
            }
        }
    }

    fn potion_bits(&self) -> [bool; 3] {
        from_fn(|slot| !self.items[slot].is_empty())
    }

    fn tick(&mut self) -> BrewingStandTick {
        let mut changed = false;
        let mut brewed = false;
        let mut dropped = SmallVec::new();

        let fuel = self.items[SLOT_FUEL].clone();
        if self.fuel <= 0
            && let Some(brewing_fuel) = fuel.get(BREWING_FUEL)
        {
            self.consume_fuel(brewing_fuel, &mut dropped);
            changed = true;
        }

        let brewable = self.is_brewable();
        let is_brewing = self.brew_time > 0;
        if is_brewing {
            self.brew_time -= 1;
            let done_brewing = self.brew_time == 0;
            if done_brewing && brewable {
                self.do_brew(&mut dropped);
                brewed = true;
            } else if !brewable || !self.items[SLOT_INGREDIENT].is(self.ingredient) {
                self.brew_time = 0;
            }
            changed = true;
        } else if brewable && self.fuel > 0 {
            let speed_multiplier = if self.speed_multiplier > 0.0 {
                self.speed_multiplier
            } else {
                DEFAULT_SPEED_MULTIPLIER
            };
            self.fuel -= 1;
            self.brew_time = (BREW_TICKS / speed_multiplier).ceil() as i32;
            self.total_brew_time = self.brew_time;
            self.ingredient = self.items[SLOT_INGREDIENT].item();
            changed = true;
        }

        let potion_bits = self.potion_bits();
        let potion_bits_changed = self.last_potion_count != Some(potion_bits);
        if potion_bits_changed {
            self.last_potion_count = Some(potion_bits);
        }

        BrewingStandTick {
            changed,
            brewed,
            dropped,
            potion_bits,
            potion_bits_changed,
        }
    }
}

impl Container for BrewingStandContainer {
    fn items(&self) -> &[ItemStack] {
        &self.items
    }

    fn items_mut(&mut self) -> &mut [ItemStack] {
        &mut self.items
    }

    fn get_max_stack_size(&self) -> i32 {
        64
    }

    fn set_changed(&mut self) {}

    fn can_place_item(&self, slot: usize, stack: &ItemStack) -> bool {
        match slot {
            SLOT_FUEL => stack.has(BREWING_FUEL),
            SLOT_INGREDIENT => REGISTRY.recipes.is_brewing_reagent(stack.item()),
            SLOT_POTION_0 | SLOT_POTION_1 | SLOT_POTION_2 => {
                PotionIngredient::is_potion_input(stack) && self.items[slot].is_empty()
            }
            _ => false,
        }
    }

    fn slots_for_face(&self, direction: Direction) -> Option<&'static [usize]> {
        Some(match direction {
            Direction::Up => SLOTS_FOR_UP,
            Direction::Down => SLOTS_FOR_DOWN,
            _ => SLOTS_FOR_SIDES,
        })
    }

    fn can_take_item_through_face(
        &self,
        slot: usize,
        stack: &ItemStack,
        _direction: Direction,
    ) -> bool {
        slot != SLOT_INGREDIENT || stack.is(&vanilla_items::GLASS_BOTTLE)
    }
}

/// Brewing stand block entity.
pub struct BrewingStandBlockEntity {
    base: Arc<BlockEntityBase>,
    container: Arc<SyncMutex<BrewingStandContainer>>,
    container_ref: ContainerRef,
    properties: ContainerProperties,
}

// SAFETY: This Steel-owned key uniquely identifies the brewing-stand block entity.
unsafe impl DowncastType for BrewingStandBlockEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:block_entity/brewing_stand");
}

impl BrewingStandBlockEntity {
    /// Creates a brewing-stand block entity at a live world position.
    #[must_use]
    pub fn new(level: Weak<World>, pos: BlockPos, state: BlockStateId) -> Self {
        let base = Arc::new(BlockEntityBase::new(
            &vanilla_block_entity_types::BREWING_STAND,
            level,
            pos,
            state,
        ));
        let container = BrewingStandContainer::new().into_shared();
        let shared: SharedContainer = Arc::<SyncMutex<BrewingStandContainer>>::clone(&container);
        Self {
            container_ref: ContainerRef::owned_by_block_entity(shared, Arc::clone(&base)),
            base,
            container,
            properties: ContainerProperties::default(),
        }
    }

    /// Checks the container lock before a player opens this brewing stand.
    pub(crate) fn can_open(&self, player: &Player) -> bool {
        self.properties.can_open(player)
    }

    fn server_tick(&self, world: &Arc<World>) {
        let tick = self.container.lock().tick();
        let pos = self.base.pos();

        for item in tick.dropped {
            world.drop_item_stack(pos, item);
        }
        if tick.brewed {
            world.level_event(SOUND_BREWING_STAND_BREW, pos, 0, None);
        }
        if tick.potion_bits_changed {
            let mut state = self.base.block_state();
            for (property, has_bottle) in HAS_BOTTLE.into_iter().zip(tick.potion_bits) {
                state = state.set_value(property, has_bottle);
            }
            world.set_block(pos, state, UpdateFlags::UPDATE_CLIENTS);
        }
        if tick.changed {
            self.base.set_changed();
        }
    }
}

impl BlockEntity for BrewingStandBlockEntity {
    fn base(&self) -> &BlockEntityBase {
        &self.base
    }

    fn pre_remove_side_effects(&self, pos: BlockPos, _state: BlockStateId) {
        let items = {
            let mut container = self.container.lock();
            mem::replace(&mut container.items, from_fn(|_| ItemStack::empty()))
        };
        if let Some(world) = self.get_level() {
            for item in items {
                world.drop_item_stack(pos, item);
            }
        }
    }

    fn load_additional(&self, nbt: &BorrowedNbtCompound<'_>) {
        self.properties.load(nbt);
        let nbt: NbtCompoundView<'_, '_> = nbt.into();
        let mut container = self.container.lock();
        container.items.fill(ItemStack::empty());
        load_all_items(nbt, &mut container.items);
        container.brew_time = nbt.int_value("BrewTime").unwrap_or(DEFAULT_BREW_TIME);
        container.total_brew_time = nbt
            .int_value("total_brew_time")
            .unwrap_or(DEFAULT_TOTAL_BREW_TIME);
        container.ingredient = if container.brew_time > 0 {
            container.items[SLOT_INGREDIENT].item()
        } else {
            &vanilla_items::AIR
        };
        container.fuel = nbt.int_value("Fuel").unwrap_or(DEFAULT_FUEL);
        container.total_fuel = nbt.int_value("total_fuel").unwrap_or(DEFAULT_FUEL_USES);
        container.speed_multiplier = nbt
            .float_value("speed_multiplier")
            .unwrap_or(DEFAULT_SPEED_MULTIPLIER);
        // The previous potion-bits snapshot is not persisted.
        container.last_potion_count = None;
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        self.properties.save(nbt);
        let container = self.container.lock();
        nbt.insert("BrewTime", container.brew_time);
        nbt.insert("total_brew_time", container.total_brew_time);

        let mut items = Vec::new();
        for (slot, stack) in container.items.iter().enumerate() {
            if !stack.is_empty()
                && let NbtTag::Compound(mut item) = stack.clone().to_nbt_tag()
            {
                item.insert("Slot", slot as i8);
                items.push(item);
            }
        }
        nbt.insert("Items", NbtList::Compound(items));

        nbt.insert("Fuel", container.fuel);
        nbt.insert("total_fuel", container.total_fuel);
        nbt.insert("speed_multiplier", container.speed_multiplier);
    }

    fn apply_implicit_components(&self, components: &mut ImplicitComponentGetter<'_>) {
        self.properties.apply_components(components);
        let Some(contents) = components.get(CONTAINER) else {
            return;
        };
        let mut container = self.container.lock();
        container.items.fill(ItemStack::empty());
        for (slot, template) in contents
            .items()
            .iter()
            .take(BREWING_STAND_SLOTS)
            .enumerate()
        {
            if let Some(template) = template {
                container.items[slot] = template.create();
            }
        }
    }

    fn tick(&self, world: &Arc<World>) {
        self.server_tick(world);
    }

    fn container_ref(&self) -> Option<ContainerRef> {
        Some(self.container_ref.clone())
    }

    fn as_nameable(&self) -> Option<&dyn Nameable> {
        Some(self)
    }
}

impl Nameable for BrewingStandBlockEntity {
    fn custom_name(&self) -> Option<TextComponent> {
        self.properties.custom_name()
    }

    fn name(&self) -> TextComponent {
        self.properties
            .custom_name()
            .unwrap_or_else(|| translations::CONTAINER_BREWING.msg().component())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{TestPlayerBuilder, fresh_test_world};
    use steel_registry::data_components::vanilla_components::{POTION_CONTENTS, PotionContents};
    use steel_registry::potion::PotionRef;
    use steel_registry::{init_vanilla_registry, vanilla_blocks, vanilla_potions};

    fn potion(item: ItemRef, potion: PotionRef) -> ItemStack {
        PotionContents::create_item_stack(item, potion)
    }

    #[test]
    fn brewing_consumes_fuel_once_and_uses_the_recipe_speed() {
        init_vanilla_registry();
        let mut container = BrewingStandContainer::new();
        container.set_item(
            SLOT_POTION_0,
            potion(&vanilla_items::POTION, &vanilla_potions::AWKWARD),
        );
        container.set_item(
            SLOT_INGREDIENT,
            ItemStack::new(&vanilla_items::BLAZE_POWDER),
        );
        container.set_item(SLOT_FUEL, ItemStack::new(&vanilla_items::BLAZE_POWDER));

        let tick = container.tick();
        assert_eq!(container.fuel, 19);
        assert_eq!(container.total_fuel, 20);
        assert_eq!(container.brew_time, 400);
        assert!(tick.changed);
        assert!(!tick.brewed);

        for _ in 0..399 {
            container.tick();
        }
        assert_eq!(container.brew_time, 1);
        let tick = container.tick();
        assert!(tick.brewed);
        assert!(container.items[SLOT_POTION_0].is(&vanilla_items::POTION));
        assert!(
            container.items[SLOT_POTION_0]
                .get(POTION_CONTENTS)
                .is_some_and(|contents| contents.is(&vanilla_potions::STRENGTH))
        );
        assert!(container.items[SLOT_INGREDIENT].is_empty());
        assert!(container.items[SLOT_FUEL].is_empty());
    }

    #[test]
    fn changing_the_ingredient_cancels_an_active_brew() {
        init_vanilla_registry();
        let mut container = BrewingStandContainer::new();
        container.set_item(
            SLOT_POTION_0,
            potion(&vanilla_items::POTION, &vanilla_potions::AWKWARD),
        );
        container.set_item(
            SLOT_INGREDIENT,
            ItemStack::new(&vanilla_items::BLAZE_POWDER),
        );
        container.set_item(SLOT_FUEL, ItemStack::new(&vanilla_items::BLAZE_POWDER));

        assert!(container.tick().changed);
        assert!(container.brew_time > 0);

        container.set_item(SLOT_INGREDIENT, ItemStack::new(&vanilla_items::GHAST_TEAR));
        container.tick();

        assert_eq!(container.brew_time, 0);
        assert!(
            container.items[SLOT_POTION_0]
                .get(POTION_CONTENTS)
                .is_some_and(|contents| contents.is(&vanilla_potions::AWKWARD))
        );
    }

    #[test]
    fn potion_bits_follow_occupied_potion_slots() {
        init_vanilla_registry();
        let mut container = BrewingStandContainer::new();
        assert_eq!(container.tick().potion_bits, [false, false, false]);

        container.set_item(SLOT_POTION_1, ItemStack::new(&vanilla_items::GLASS_BOTTLE));
        let tick = container.tick();
        assert_eq!(tick.potion_bits, [false, true, false]);
        assert!(tick.potion_bits_changed);
    }

    #[test]
    fn custom_name_and_lock_round_trip_through_nbt() {
        init_vanilla_registry();
        let fixture = fresh_test_world("brewing_stand_name_and_lock");
        let world = &fixture.world;
        let state = vanilla_blocks::BREWING_STAND.default_state();
        let stand =
            BrewingStandBlockEntity::new(Arc::downgrade(world), BlockPos::new(8, 64, 8), state);
        let name = TextComponent::plain("Brewery");
        let mut lock = NbtCompound::new();
        lock.insert("items", "minecraft:diamond");
        let mut nbt = NbtCompound::new();
        nbt.insert("CustomName", name.to_codec_nbt());
        nbt.insert("lock", lock);

        stand
            .load_with_owned_components(&nbt)
            .expect("load custom name and lock");

        assert_eq!(stand.custom_name(), Some(name.clone()));
        assert_eq!(stand.name(), name);

        let player = TestPlayerBuilder::new(Arc::clone(world), "LockedOut", 1).build();
        assert!(!stand.can_open(&player));
        player
            .inventory
            .lock()
            .set_selected_item(ItemStack::new(&vanilla_items::DIAMOND));
        assert!(stand.can_open(&player));

        let saved = stand.save_custom_only();
        let reloaded =
            BrewingStandBlockEntity::new(Arc::downgrade(world), BlockPos::new(8, 64, 8), state);
        reloaded
            .load_with_owned_components(&saved)
            .expect("reload custom name and lock");
        assert_eq!(reloaded.name(), name);
    }
}
