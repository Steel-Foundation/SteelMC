//! Item stack implementation.

use std::io::{Cursor, Result, Write};

use rand::RngExt;

use steel_utils::{
    DowncastType, Identifier,
    codec::VarInt,
    random::{Random, xoroshiro::Xoroshiro},
    serial::{ReadFrom, WriteTo},
};

use crate::{
    REGISTRY, RegistryEntry, RegistryExt,
    damage_type::DamageTypeRef,
    data_components::{
        Component, ComponentData, ComponentPatchEntry, CustomData, DataComponentMap,
        DataComponentPatch, DataComponentType,
        vanilla_components::{
            ATTACK_RANGE, ATTRIBUTE_MODIFIERS, AttackRange, BUNDLE_CONTENTS, CHARGED_PROJECTILES,
            CONTAINER, CUSTOM_DATA, DAMAGE, DAMAGE_RESISTANT, DAMAGE_TYPE, ENCHANTABLE,
            ENCHANTMENTS, EQUIPPABLE, Equippable, ItemAttributeModifiers, ItemEnchantments,
            MAX_DAMAGE, MAX_STACK_SIZE, MINIMUM_ATTACK_CHARGE, OMINOUS_BOTTLE_AMPLIFIER,
            OminousBottleAmplifier, PIERCING_WEAPON, PiercingWeapon, REPAIRABLE, TOOL, Tool,
            UNBREAKABLE, WEAPON, Weapon,
        },
    },
    enchantment_effect::EnchantmentEffectComponent,
    equipment::EquipmentSlot,
    item_stack_template::ItemStackTemplate,
    items::ItemRef,
    vanilla_items,
};

/// A stack of items with a count and component modifications.
#[derive(Debug, Clone, PartialEq)]
pub struct ItemStack {
    /// The item type. AIR represents an empty stack.
    pub item: ItemRef,
    /// The number of items in this stack.
    pub count: i32,
    /// Modifications to the prototype components.
    patch: DataComponentPatch,
}

impl Default for ItemStack {
    fn default() -> Self {
        Self::empty()
    }
}

impl ItemStack {
    /// Creates an empty item stack (using AIR).
    #[must_use]
    pub fn empty() -> Self {
        Self {
            item: &vanilla_items::AIR,
            count: 0,
            patch: DataComponentPatch::new(),
        }
    }

    /// Creates a new item stack with count 1.
    #[must_use]
    pub fn new(item: ItemRef) -> Self {
        Self::with_count(item, 1)
    }

    /// Creates a new item stack with the specified count.
    #[must_use]
    pub fn with_count(item: ItemRef, count: i32) -> Self {
        Self {
            item,
            count,
            patch: DataComponentPatch::new(),
        }
    }

    /// Creates a new item stack with the specified count and component patch.
    #[must_use]
    pub fn with_count_and_patch(item: ItemRef, count: i32, mut patch: DataComponentPatch) -> Self {
        patch.sanitize_against(&item.components);
        Self { item, count, patch }
    }

    #[must_use]
    const fn prototype(&self) -> &'static DataComponentMap {
        &self.item.components
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.item == &*vanilla_items::AIR || self.count <= 0
    }

    #[must_use]
    pub fn item(&self) -> ItemRef {
        if self.is_empty() {
            &vanilla_items::AIR
        } else {
            self.item
        }
    }

    #[must_use]
    pub fn count(&self) -> i32 {
        if self.is_empty() { 0 } else { self.count }
    }

    #[must_use]
    pub const fn components_patch(&self) -> &DataComponentPatch {
        &self.patch
    }

    pub const fn set_count(&mut self, count: i32) {
        self.count = count;
    }

    /// Increases the count by the given amount.
    pub const fn grow(&mut self, amount: i32) {
        self.count += amount;
    }

    /// Decreases the count by the given amount.
    pub const fn shrink(&mut self, amount: i32) {
        self.count -= amount;
    }

    /// Splits off the specified amount from this stack and returns it as a new stack.
    ///
    /// If the amount is greater than or equal to the current count, this stack becomes
    /// empty and the entire contents are returned.
    pub fn split(&mut self, amount: i32) -> Self {
        let take = amount.min(self.count);
        let result = Self {
            item: self.item,
            count: take,
            patch: self.patch.clone(),
        };
        self.shrink(take);
        result
    }

    /// Copies the identity (item type and patch) from another stack.
    ///
    /// Used when splitting stacks to preserve components.
    #[must_use]
    pub fn copy_with_count(&self, count: i32) -> Self {
        if self.is_empty() {
            Self::empty()
        } else {
            Self {
                item: self.item,
                count,
                patch: self.patch.clone(),
            }
        }
    }

    /// Returns true if this item can stack (max stack size > 1 and not damaged).
    /// Damaged items cannot stack.
    #[must_use]
    pub fn is_stackable(&self) -> bool {
        self.max_stack_size() > 1 && (!self.is_damageable_item() || !self.is_damaged())
    }

    /// Returns true if this item can take damage.
    #[must_use]
    pub fn is_damageable_item(&self) -> bool {
        self.has(MAX_DAMAGE) && !self.has(UNBREAKABLE) && self.has(DAMAGE)
    }

    /// Returns true if this item has taken damage.
    #[must_use]
    pub fn is_damaged(&self) -> bool {
        self.is_damageable_item() && self.get_damage_value() > 0
    }

    /// Gets the current damage value of this item.
    #[must_use]
    pub fn get_damage_value(&self) -> i32 {
        self.get(DAMAGE)
            .copied()
            .unwrap_or(0)
            .clamp(0, self.get_max_damage())
    }

    /// Sets the damage value of this item.
    pub fn set_damage_value(&mut self, value: i32) {
        let clamped = value.clamp(0, self.get_max_damage());
        self.set(DAMAGE, clamped);
    }

    /// Gets the maximum damage this item can take before breaking.
    #[must_use]
    pub fn get_max_damage(&self) -> i32 {
        self.get(MAX_DAMAGE).copied().unwrap_or(0)
    }

    /// Returns true if the item is broken (damage >= max damage).
    #[must_use]
    pub fn is_broken(&self) -> bool {
        self.is_damageable_item() && self.get_damage_value() >= self.get_max_damage()
    }

    /// Returns vanilla `ItemStack.nextDamageWillBreak()`.
    #[must_use]
    pub fn next_damage_will_break(&self) -> bool {
        self.is_damageable_item() && self.get_damage_value() >= self.get_max_damage() - 1
    }

    /// Damages the item and breaks it if durability reaches zero.
    ///
    /// Returns `true` if the item broke and should be removed/replaced.
    pub fn hurt_and_break(&mut self, amount: i32, has_infinite_materials: bool) -> bool {
        let mut random = Xoroshiro::from_seed_unmixed(rand::rng().random());
        self.hurt_and_break_with_random(amount, has_infinite_materials, &mut random)
    }

    /// Damages the item using the supplied random source for data-driven
    /// `minecraft:item_damage` enchantment effects.
    ///
    /// Returns `true` if the item broke and should be removed/replaced.
    pub fn hurt_and_break_with_random(
        &mut self,
        amount: i32,
        has_infinite_materials: bool,
        random: &mut impl Random,
    ) -> bool {
        if !self.is_damageable_item() {
            return false;
        }

        if has_infinite_materials {
            return false;
        }

        let effective_amount = self.process_durability_change(amount, random);

        if effective_amount == 0 {
            return false;
        }

        let new_damage = self.get_damage_value() + effective_amount;

        // TODO: Trigger ITEM_DURABILITY_CHANGED advancement criteria

        self.set_damage_value(new_damage);

        if self.is_broken() {
            // TODO: Call onEquippedItemBroken callback which:
            // - Broadcasts entity event (byte 47 for mainhand) for break sound/particles
            // - Stops location-based effects (removes attribute modifiers)
            self.shrink(1);
            return true;
        }

        false
    }

    fn process_durability_change(&self, amount: i32, random: &mut impl Random) -> i32 {
        if amount <= 0 {
            return amount;
        }

        let Some(enchantments) = self.get_enchantments() else {
            return amount;
        };

        let mut value = amount as f32;
        for (key, level) in enchantments.iter() {
            if *level == 0 {
                continue;
            }
            let Some(enchantment) = REGISTRY.enchantments.by_key(key) else {
                continue;
            };

            for effect in enchantment.effects.item_damage {
                if effect.requirements.is_some_and(|requirements| {
                    requirements.matches_item_context(self.item()) != Some(true)
                }) {
                    continue;
                }
                value = effect
                    .effect
                    .process_with_random(*level as i32, random, value);
            }
        }

        value as i32
    }

    /// Returns true if this item has the specified component (by type).
    #[must_use]
    pub fn has<T: 'static>(&self, component: DataComponentType<T>) -> bool {
        self.has_component(&component.key)
    }

    /// Returns true if this item has the specified component (by key).
    #[must_use]
    pub fn has_component(&self, key: &Identifier) -> bool {
        match self.patch.get_entry(key) {
            Some(ComponentPatchEntry::Set(_)) => true,
            Some(ComponentPatchEntry::Removed) => false,
            None => self.prototype().get_raw(key).is_some(),
        }
    }

    #[must_use]
    pub fn is_same_item(a: &Self, b: &Self) -> bool {
        a.item().key == b.item().key
    }

    /// Checks if two stacks have the same item and components.
    #[must_use]
    pub fn is_same_item_same_components(a: &Self, b: &Self) -> bool {
        if !Self::is_same_item(a, b) {
            return false;
        }
        if a.is_empty() && b.is_empty() {
            return true;
        }
        a.components_equal(b)
    }

    #[must_use]
    pub fn matches(a: &Self, b: &Self) -> bool {
        a.count() == b.count() && Self::is_same_item_same_components(a, b)
    }

    #[must_use]
    pub fn is(&self, item: ItemRef) -> bool {
        self.item().key == item.key
    }

    #[must_use]
    pub fn max_stack_size(&self) -> i32 {
        self.get(MAX_STACK_SIZE).copied().unwrap_or(1)
    }

    /// Validates the complete stack constraints enforced by Vanilla's `ItemStack.validateStrict`.
    pub fn validate_strict(&self) -> Result<()> {
        let max_stack_size = self.max_stack_size();
        if self.has(MAX_DAMAGE) && max_stack_size > 1 {
            return Err(std::io::Error::other(
                "Item cannot be both damageable and stackable",
            ));
        }

        if let Some(container) = self.get(CONTAINER) {
            validate_contained_item_sizes(container.items().iter().flatten())?;
        }

        if let Some(bundle) = self.get(BUNDLE_CONTENTS) {
            validate_contained_item_sizes(bundle.items())?;
            bundle.validate_weight()?;
        }

        if let Some(projectiles) = self.get(CHARGED_PROJECTILES) {
            validate_contained_item_sizes(projectiles.items())?;
        }

        if self.count > max_stack_size {
            return Err(std::io::Error::other(format!(
                "Item stack with stack size of {} was larger than maximum: {max_stack_size}",
                self.count
            )));
        }
        Ok(())
    }

    /// Returns the equippable component if this item has one.
    #[must_use]
    pub fn get_equippable(&self) -> Option<&Equippable> {
        self.get(EQUIPPABLE)
    }

    /// Returns the item attribute modifiers component.
    #[must_use]
    pub fn get_attribute_modifiers(&self) -> Option<&ItemAttributeModifiers> {
        self.get(ATTRIBUTE_MODIFIERS)
    }

    /// Returns the equipment slot this item can be equipped to, if any.
    #[must_use]
    pub fn get_equippable_slot(&self) -> Option<EquipmentSlot> {
        self.get_equippable().map(|e| e.slot)
    }

    /// Returns true if this item can be equipped in the given slot.
    #[must_use]
    pub fn is_equippable_in_slot(&self, slot: EquipmentSlot) -> bool {
        self.get_equippable_slot() == Some(slot)
    }

    /// Gets the raw component data by key.
    #[must_use]
    pub fn get_effective_value_raw(&self, key: &Identifier) -> Option<&ComponentData> {
        match self.patch.get_entry(key) {
            Some(ComponentPatchEntry::Set(data)) => Some(data),
            Some(ComponentPatchEntry::Removed) => None,
            None => self.prototype().get_raw(key),
        }
    }

    /// Gets the effective value of a component, considering the patch and prototype.
    /// Returns `None` if the component is not present or has been removed.
    #[must_use]
    pub fn get<T: Component + DowncastType>(&self, component: DataComponentType<T>) -> Option<&T> {
        let data = self.get_effective_value_raw(&component.key)?;
        data.downcast_ref::<T>()
    }

    /// Gets the effective value of a component, or returns the default value if not present.
    #[must_use]
    pub fn get_or_default<T: Component + DowncastType + Clone>(
        &self,
        component: DataComponentType<T>,
        default: T,
    ) -> T {
        self.get(component).cloned().unwrap_or(default)
    }

    /// Sets a component value in this item's patch, overriding the prototype.
    pub fn set<T: Component + DowncastType>(&mut self, component: DataComponentType<T>, value: T) {
        let value = ComponentData::new(value);
        let is_default = self.prototype().get_raw(&component.key) == Some(&value);
        if is_default {
            self.patch.clear(component);
        } else {
            self.patch.set_component_data(component.key, value);
        }
    }

    /// Removes a component from this item (marks it as removed in the patch).
    /// This will hide the component even if it exists in the prototype.
    pub fn remove<T: 'static>(&mut self, component: DataComponentType<T>) {
        if self.prototype().get_raw(&component.key).is_some() {
            self.patch.remove(component);
        } else {
            self.patch.clear(component);
        }
    }

    /// Clears any patch entry for this component (neither set nor removed).
    /// The prototype value will be visible again.
    pub fn clear<T: 'static>(&mut self, component: DataComponentType<T>) {
        self.patch.clear(component);
    }

    /// Returns a reference to the component patch.
    #[must_use]
    pub const fn patch(&self) -> &DataComponentPatch {
        &self.patch
    }

    /// Gets the Tool component if present.
    #[must_use]
    pub fn get_tool(&self) -> Option<&Tool> {
        self.get(TOOL)
    }

    /// Gets the Weapon component if present.
    #[must_use]
    pub fn get_weapon(&self) -> Option<&Weapon> {
        self.get(WEAPON)
    }

    /// Gets the `AttackRange` component if present.
    #[must_use]
    pub fn get_attack_range(&self) -> Option<&AttackRange> {
        self.get(ATTACK_RANGE)
    }

    /// Returns vanilla `DataComponents.MINIMUM_ATTACK_CHARGE`, defaulting to 0.
    #[must_use]
    pub fn minimum_attack_charge(&self) -> f32 {
        self.get(MINIMUM_ATTACK_CHARGE).copied().unwrap_or(0.0)
    }

    /// Gets the vanilla damage type component if present.
    #[must_use]
    pub fn get_damage_type(&self) -> Option<DamageTypeRef> {
        self.get(DAMAGE_TYPE).map(|component| component.damage_type)
    }

    /// Returns vanilla `ItemStack.canBeHurtBy` for a damage type.
    #[must_use]
    pub fn can_be_hurt_by(&self, damage_type: DamageTypeRef) -> bool {
        self.get(DAMAGE_RESISTANT)
            .is_none_or(|resistance| !resistance.is_resistant_to(damage_type))
    }

    /// Returns vanilla `ItemStack.isValidRepairItem`.
    #[must_use]
    pub fn is_valid_repair_item(&self, repair_item: &Self) -> bool {
        self.get(REPAIRABLE)
            .is_some_and(|repairable| repairable.is_valid_repair_item(repair_item))
    }

    /// Returns whether this item has the vanilla piercing weapon component.
    #[must_use]
    pub fn is_piercing_weapon(&self) -> bool {
        self.has(PIERCING_WEAPON)
    }

    /// Gets the `PiercingWeapon` component if present.
    #[must_use]
    pub fn get_piercing_weapon(&self) -> Option<&PiercingWeapon> {
        self.get(PIERCING_WEAPON)
    }

    /// Returns the mining speed for the given block state ID.
    /// If no Tool component is present, returns 1.0 (hand speed).
    #[must_use]
    pub fn get_destroy_speed(&self, block_state_id: steel_utils::BlockStateId) -> f32 {
        self.get_tool()
            .map_or(1.0, |tool| tool.get_mining_speed(block_state_id))
    }

    /// Returns true if this tool is correct for getting drops from the block.
    #[must_use]
    pub fn is_correct_tool_for_drops(&self, block_state_id: steel_utils::BlockStateId) -> bool {
        self.get_tool()
            .is_some_and(|tool| tool.is_correct_for_drops(block_state_id))
    }

    /// Returns the damage per block for this tool (how much durability is consumed per block mined).
    /// Returns 0 if no Tool component is present.
    #[must_use]
    pub fn get_tool_damage_per_block(&self) -> i32 {
        self.get_tool().map_or(0, |tool| tool.damage_per_block)
    }

    /// Returns true if this tool can destroy blocks in creative mode.
    /// Returns true if no Tool component is present (default behavior).
    #[must_use]
    pub fn can_destroy_blocks_in_creative(&self) -> bool {
        self.get_tool()
            .is_none_or(|tool| tool.can_destroy_blocks_in_creative)
    }

    #[must_use]
    pub fn get_enchantment_level(&self, enchantment: &Identifier) -> i32 {
        self.get_enchantments()
            .map_or(0, |e| e.get_level(enchantment) as i32)
    }

    #[must_use]
    pub fn get_enchantments(&self) -> Option<&ItemEnchantments> {
        self.get(ENCHANTMENTS)
    }

    /// Mirrors Vanilla's component-based `ItemStack.isEnchantable` check.
    #[must_use]
    pub fn is_enchantable(&self) -> bool {
        self.has(ENCHANTABLE)
            && self
                .get(ENCHANTMENTS)
                .is_some_and(ItemEnchantments::is_empty)
    }

    #[must_use]
    pub fn has_enchantment_effect(&self, component: EnchantmentEffectComponent) -> bool {
        let Some(enchantments) = self.get_enchantments() else {
            return false;
        };

        for (key, level) in enchantments.iter() {
            if *level == 0 {
                continue;
            }
            let Some(enchantment) = REGISTRY.enchantments.by_key(key) else {
                continue;
            };
            if enchantment.effects.has(component) {
                return true;
            }
        }

        false
    }

    #[must_use]
    pub fn apply_unconditional_enchantment_value_effects(
        &self,
        component: EnchantmentEffectComponent,
        input: f32,
    ) -> f32 {
        let Some(enchantments) = self.get_enchantments() else {
            return input;
        };

        let mut value = input;
        for (key, level) in enchantments.iter() {
            if *level == 0 {
                continue;
            }
            let Some(enchantment) = REGISTRY.enchantments.by_key(key) else {
                continue;
            };
            let level = *level as i32;

            for effect in enchantment.effects.value_effects(component) {
                if !effect.is_unconditional() {
                    continue;
                }
                if let Some(updated) = effect.effect.process_without_random(level, value) {
                    value = updated;
                }
            }

            let Some(effect) = enchantment.effects.single_value_effect(component) else {
                continue;
            };
            if let Some(updated) = effect.process_without_random(level, value) {
                value = updated;
            }
        }

        value
    }

    /// Sets the damage/durability as a fraction (0.0 = broken, 1.0 = full).
    /// If `add` is true, adds to current damage instead of setting.
    pub const fn set_damage_fraction(&mut self, _fraction: f32, _add: bool) {
        // TODO: Implement when damage component system is ready
        // let max_damage = self.get_max_damage();
        // let damage_value = ((1.0 - fraction) * max_damage as f32) as i32;
        // self.set_component(DAMAGE, damage_value);
    }

    /// Enchants this item randomly with enchantments from the given options.
    pub const fn enchant_randomly<R: rand::Rng>(
        &mut self,
        _options: &crate::loot_table::EnchantmentOptions,
        _rng: &mut R,
    ) {
        // TODO: Implement when enchantment registry and system are ready
        // 1. Get list of valid enchantments from options (tag or list)
        // 2. Filter to enchantments that can apply to this item
        // 3. Pick one randomly
        // 4. Pick a random level for that enchantment
        // 5. Add to ENCHANTMENTS component
    }

    /// Enchants this item as if using an enchanting table at the given level.
    pub const fn enchant_with_levels<R: rand::Rng>(
        &mut self,
        _level: i32,
        _options: &crate::loot_table::EnchantmentOptions,
        _rng: &mut R,
    ) {
        // TODO: Implement when enchantment registry and system are ready
        // This simulates the enchanting table algorithm:
        // 1. Calculate modified level based on item enchantability
        // 2. Generate list of possible enchantments for that level
        // 3. Filter by options (tag or list)
        // 4. Apply enchantments with proper weights
    }

    /// Copies components from a source (block entity, attacker, etc.) to this item.
    pub const fn copy_components<R: rand::Rng>(
        &mut self,
        _source: crate::loot_table::CopySource,
        _include: &[Identifier],
        _ctx: &crate::loot_table::LootContext<'_, R>,
    ) {
        // TODO: Implement when block entity system is ready
        // 1. Get the source entity/block entity from context
        // 2. For each component in `include`, copy it to this item's patch
    }

    /// Copies block state properties to this item (for blocks like `note_block`).
    pub const fn copy_block_state<R: rand::Rng>(
        &mut self,
        _block: &Identifier,
        _properties: &[&str],
        _ctx: &crate::loot_table::LootContext<'_, R>,
    ) {
        // TODO: Implement block state copying
        // 1. Get block state from context
        // 2. For each property, store it in the item's BLOCK_STATE component
    }

    /// Sets components from a JSON string representation.
    pub const fn set_components_from_json(&mut self, _components: &str) {
        // TODO: Implement component parsing from JSON
        // Parse the JSON and set each component in the patch
    }

    /// Merges custom NBT data into this item's `custom_data` component.
    pub fn set_custom_data(&mut self, value: &CustomData) {
        let merged = self
            .get(CUSTOM_DATA)
            .cloned()
            .unwrap_or_default()
            .merged_with(value);
        if merged.is_empty() {
            self.remove(CUSTOM_DATA);
        } else {
            self.set(CUSTOM_DATA, merged);
        }
    }

    /// Applies furnace smelting to convert this item (e.g., raw iron -> iron ingot).
    pub fn apply_furnace_smelt(&mut self, use_input_count: bool) {
        if let Some(result) = REGISTRY.recipes.find_smelting_result(self, use_input_count) {
            *self = result;
        }
    }

    /// Creates an exploration map pointing to a structure.
    pub const fn create_exploration_map(
        &mut self,
        _destination: &Identifier,
        _decoration: &Identifier,
        _zoom: i32,
        _skip_existing_chunks: bool,
    ) {
        // TODO: Implement exploration map creation
        // 1. Change item to filled_map
        // 2. Set MAP_DECORATIONS component
        // 3. Set destination structure tag
        // This requires world access to find the structure
    }

    /// Sets the custom name or item name of this item.
    pub const fn set_name(&mut self, _name: &str, _target: crate::loot_table::NameTarget) {
        // TODO: Implement name setting
        // Parse the name as a text component and set CUSTOM_NAME or ITEM_NAME
    }

    /// Sets the ominous bottle amplifier component.
    pub fn set_ominous_bottle_amplifier(&mut self, amplifier: i32) {
        self.set(
            OMINOUS_BOTTLE_AMPLIFIER,
            OminousBottleAmplifier::new(amplifier),
        );
    }

    /// Sets the potion type for this item.
    pub const fn set_potion(&mut self, _id: &Identifier) {
        // TODO: Implement potion type setting
        // Set the POTION_CONTENTS component with the potion ID
    }

    /// Sets the suspicious stew effects for this item.
    pub const fn set_stew_effects<R: rand::Rng>(
        &mut self,
        _effects: &[crate::loot_table::StewEffect],
        _rng: &mut R,
    ) {
        // TODO: Implement stew effect setting
        // Set the SUSPICIOUS_STEW_EFFECTS component
        // Duration is determined by each effect's NumberProvider
    }

    pub fn set_enchantments(&mut self, enchantments: &[(Identifier, u32)], add: bool) {
        let mut current = self
            .get(ENCHANTMENTS)
            .cloned()
            .unwrap_or_else(ItemEnchantments::empty);

        for (key, level) in enchantments {
            if add {
                let existing = current.get_level(key);
                current.set(key.clone(), existing + *level);
            } else {
                current.set(key.clone(), *level);
            }
        }

        self.set(ENCHANTMENTS, current);
    }

    /// Vanilla `ItemStack.enchant` → `Mutable.upgrade`: keeps the higher of existing vs new level.
    pub fn upgrade_enchantment(&mut self, enchantment: Identifier, level: u32) {
        let mut current = self
            .get(ENCHANTMENTS)
            .cloned()
            .unwrap_or_else(ItemEnchantments::empty);
        current.upgrade(enchantment, level);
        self.set(ENCHANTMENTS, current);
    }

    /// Changes the item type entirely.
    pub fn set_item(&mut self, new_item: &Identifier) {
        if let Some(item_ref) = REGISTRY.items.by_key(new_item) {
            self.item = item_ref;
            self.patch.sanitize_against(&item_ref.components);
        }
    }

    /// Copies the name from a source entity/block to this item.
    pub const fn copy_name<R: rand::Rng>(
        &mut self,
        _source: crate::loot_table::CopySource,
        _ctx: &crate::loot_table::LootContext<'_, R>,
    ) {
        // TODO: Implement when entity/block entity name access is available
        // Get name from source (block_entity.custom_name or entity.custom_name)
        // Set as CUSTOM_NAME component
    }

    /// Sets lore lines on this item.
    pub const fn set_lore(&mut self, _lore: &[&str], _mode: crate::loot_table::ListOperation) {
        // TODO: Implement lore setting
        // Parse lore strings as text components and set LORE component
        // Apply mode (replace, append, insert, etc.)
    }

    /// Sets container inventory contents.
    pub const fn set_contents<R: rand::Rng>(
        &mut self,
        _entries: &[crate::loot_table::LootEntry],
        _component_type: &Identifier,
        _ctx: &mut crate::loot_table::LootContext<'_, R>,
    ) {
        // TODO: Implement container contents setting
        // Generate items from entries and set as CONTAINER component
    }

    /// Modifies existing container contents.
    pub const fn modify_contents<R: rand::Rng>(
        &mut self,
        _modifier: &[crate::loot_table::ConditionalLootFunction],
        _component_type: &Identifier,
        _ctx: &mut crate::loot_table::LootContext<'_, R>,
    ) {
        // TODO: Implement container contents modification
        // Apply modifier functions to existing container contents
    }

    /// Sets the container's loot table reference.
    pub const fn set_loot_table(&mut self, _loot_table: &Identifier, _seed: Option<i64>) {
        // TODO: Implement loot table reference setting
        // Set CONTAINER_LOOT component with table reference and seed
    }

    /// Sets attribute modifiers on this item.
    pub const fn set_attributes<R: rand::Rng>(
        &mut self,
        _modifiers: &[crate::loot_table::AttributeModifier],
        _replace: bool,
        _rng: &mut R,
    ) {
        // TODO: Implement attribute modifier setting
        // Set ATTRIBUTE_MODIFIERS component
    }

    /// Fills a player head with texture from an entity.
    pub const fn fill_player_head<R: rand::Rng>(
        &mut self,
        _entity: crate::loot_table::LootContextEntity,
        _ctx: &crate::loot_table::LootContext<'_, R>,
    ) {
        // TODO: Implement player head texture filling
        // Get player profile from entity and set PROFILE component
    }

    /// Copies custom NBT data from a source.
    pub const fn copy_custom_data<R: rand::Rng>(
        &mut self,
        _source: crate::loot_table::CopySource,
        _operations: &[crate::loot_table::CopyDataOperation],
        _ctx: &crate::loot_table::LootContext<'_, R>,
    ) {
        // TODO: Implement custom data copying
        // Copy NBT paths from source to item's CUSTOM_DATA component
    }

    /// Sets banner pattern layers.
    pub const fn set_banner_pattern(
        &mut self,
        _patterns: &[crate::loot_table::BannerPattern],
        _append: bool,
    ) {
        // TODO: Implement banner pattern setting
        // Set BANNER_PATTERNS component
    }

    /// Sets firework rocket properties.
    pub const fn set_fireworks(
        &mut self,
        _explosions: Option<&[crate::loot_table::FireworkExplosion]>,
        _flight_duration: Option<i32>,
    ) {
        // TODO: Implement firework setting
        // Set FIREWORKS component
    }

    /// Sets firework star explosion properties.
    pub const fn set_firework_explosion(
        &mut self,
        _explosion: &crate::loot_table::FireworkExplosion,
    ) {
        // TODO: Implement firework explosion setting
        // Set FIREWORK_EXPLOSION component
    }

    /// Sets book cover (title/author for written books).
    pub const fn set_book_cover(
        &mut self,
        _title: Option<&str>,
        _author: Option<&str>,
        _generation: Option<i32>,
    ) {
        // TODO: Implement book cover setting
        // Set WRITTEN_BOOK_CONTENT component fields
    }

    /// Sets written book page contents.
    pub const fn set_written_book_pages(
        &mut self,
        _pages: &[&str],
        _mode: crate::loot_table::ListOperation,
    ) {
        // TODO: Implement written book pages setting
        // Set WRITTEN_BOOK_CONTENT pages
    }

    /// Sets writable book page contents.
    pub const fn set_writable_book_pages(
        &mut self,
        _pages: &[&str],
        _mode: crate::loot_table::ListOperation,
    ) {
        // TODO: Implement writable book pages setting
        // Set WRITABLE_BOOK_CONTENT pages
    }

    /// Runs vanilla `ToggleTooltips`: each boolean says whether the component is shown.
    pub fn toggle_tooltips(&mut self, toggles: &[(Identifier, bool)]) {
        use crate::data_components::vanilla_components::{TOOLTIP_DISPLAY, TooltipDisplay};

        let mut display = self
            .get(TOOLTIP_DISPLAY)
            .cloned()
            .unwrap_or(TooltipDisplay::DEFAULT);
        for (component, shown) in toggles {
            display = display.with_hidden_key(component.clone(), !shown);
        }
        self.set(TOOLTIP_DISPLAY, display);
    }

    #[must_use]
    pub fn components_equal(&self, other: &Self) -> bool {
        let mut all_keys = rustc_hash::FxHashSet::default();

        for key in self.prototype().keys() {
            if !self.patch.is_removed(key) {
                all_keys.insert(key);
            }
        }
        for (key, entry) in self.patch.iter() {
            if matches!(entry, ComponentPatchEntry::Set(_)) {
                all_keys.insert(key);
            }
        }
        for key in other.prototype().keys() {
            if !other.patch.is_removed(key) {
                all_keys.insert(key);
            }
        }
        for (key, entry) in other.patch.iter() {
            if matches!(entry, ComponentPatchEntry::Set(_)) {
                all_keys.insert(key);
            }
        }
        for key in all_keys {
            let val_a = self.get_effective_value_raw(key);
            let val_b = other.get_effective_value_raw(key);

            match (val_a, val_b) {
                (Some(a), Some(b)) => {
                    if a != b {
                        return false;
                    }
                }
                (None, None) => {}
                _ => return false,
            }
        }

        true
    }
}

fn validate_contained_item_sizes<'a>(
    items: impl IntoIterator<Item = &'a ItemStackTemplate>,
) -> Result<()> {
    for item in items {
        let max_stack_size = item.max_stack_size();
        if item.count() > max_stack_size {
            return Err(std::io::Error::other(format!(
                "Item stack with count of {} was larger than maximum: {max_stack_size}",
                item.count()
            )));
        }
    }
    Ok(())
}

impl std::fmt::Display for ItemStack {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_empty() {
            write!(f, "Empty")
        } else {
            write!(f, "{} {}", self.count, self.item.key)
        }
    }
}

impl WriteTo for ItemStack {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        if self.is_empty() {
            VarInt(0).write(writer)?;
        } else {
            VarInt(self.count).write(writer)?;
            // Write item ID as VarInt
            VarInt(self.item.id() as i32).write(writer)?;
            // Write DataComponentPatch
            self.patch.write(writer)?;
        }
        Ok(())
    }
}

impl ReadFrom for ItemStack {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let count = VarInt::read(data)?.0;
        if count <= 0 {
            return Ok(Self::empty());
        }

        let item_id = VarInt::read(data)?.0;
        let item_id = usize::try_from(item_id)
            .map_err(|_| std::io::Error::other(format!("Negative item id: {item_id}")))?;
        let item = REGISTRY
            .items
            .by_id(item_id)
            .ok_or_else(|| std::io::Error::other(format!("Unknown item id: {item_id}")))?;

        // Read DataComponentPatch
        let patch = DataComponentPatch::read(data)?;

        Ok(Self::with_count_and_patch(item, count, patch))
    }
}

impl ItemStack {
    /// Reads an item stack using the delimited (untrusted) component format.
    ///
    /// Vanilla uses this for serverbound packets where component data is
    /// length-prefixed (e.g., `ServerboundSetCreativeModeSlotPacket`).
    pub fn read_untrusted(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let count = VarInt::read(data)?.0;
        if count <= 0 {
            return Ok(Self::empty());
        }

        let item_id = VarInt::read(data)?.0;
        let item_id = usize::try_from(item_id)
            .map_err(|_| std::io::Error::other(format!("Negative item id: {item_id}")))?;
        let item = REGISTRY
            .items
            .by_id(item_id)
            .ok_or_else(|| std::io::Error::other(format!("Unknown item id: {item_id}")))?;
        let patch = DataComponentPatch::read_delimited(data)?;

        let stack = Self::with_count_and_patch(item, count, patch);
        stack.validate_persistent_encoding()?;
        Ok(stack)
    }
}

use simdnbt::{
    FromNbtTag, ToNbtTag,
    borrow::{NbtCompound as NbtCompoundView, NbtTag as BorrowedNbtTag},
    owned::NbtCompound,
};
use steel_utils::nbt::NbtNumeric as _;

impl ToNbtTag for ItemStack {
    /// Converts this item stack to an NBT tag for persistent storage.
    ///
    /// Format (matching vanilla Minecraft):
    /// ```text
    /// {
    ///     id: "minecraft:stone",
    ///     count: 64,
    ///     components: { ... }  // Only present if patch is non-empty
    /// }
    /// ```
    fn to_nbt_tag(self) -> simdnbt::owned::NbtTag {
        self.to_nbt_tag_ref()
    }
}

impl ItemStack {
    /// Checks that this stack can be encoded by Vanilla's persistent
    /// `ItemStack.CODEC` before untrusted network data enters server state.
    ///
    /// This is an ingress check rather than a type invariant: programmatic
    /// component mutation can still create values whose save codec reports
    /// and omits invalid fields.
    pub fn validate_persistent_encoding(&self) -> Result<()> {
        if self.is_empty() {
            return Ok(());
        }
        if !(1..=99).contains(&self.count) {
            return Err(std::io::Error::other(format!(
                "Item stack count {} is outside the persistent range 1..=99",
                self.count
            )));
        }
        self.patch.try_to_nbt_tag_ref().map(|_| ())
    }

    /// Converts this item stack to an NBT tag for persistent storage without consuming it.
    #[must_use]
    pub fn to_nbt_tag_ref(&self) -> simdnbt::owned::NbtTag {
        if self.is_empty() {
            // Empty stacks are represented as an empty compound
            return simdnbt::owned::NbtTag::Compound(NbtCompound::new());
        }

        let mut compound = NbtCompound::new();

        // id: The item identifier
        compound.insert("id", self.item.key.to_string());

        compound.insert("count", self.count);

        // components: The component patch (only if non-empty)
        if !self.patch.is_empty() {
            compound.insert("components", self.patch.to_nbt_tag_ref());
        }

        simdnbt::owned::NbtTag::Compound(compound)
    }
}

impl FromNbtTag for ItemStack {
    /// Parses an item stack from an NBT tag.
    ///
    /// Accepts the vanilla format:
    /// ```text
    /// {
    ///     id: "minecraft:stone",
    ///     count: 64,
    ///     components: { ... }
    /// }
    /// ```
    fn from_nbt_tag(tag: BorrowedNbtTag) -> Option<Self> {
        let compound = tag.compound()?;

        // Get the item ID
        let id_str = compound.get("id")?.string()?.to_str();
        let id = id_str.parse::<Identifier>().ok()?;

        // Look up the item in the registry
        let item = REGISTRY.items.by_key(&id)?;

        let count = decode_persistent_count(compound.get("count"))?;

        let patch = match compound.get("components") {
            Some(tag) => DataComponentPatch::from_nbt_tag(tag)?,
            None => DataComponentPatch::new(),
        };

        Some(Self::with_count_and_patch(item, count, patch))
    }
}

impl ItemStack {
    /// Parses an `ItemStack` from a borrowed `NbtCompoundView`.
    ///
    /// This is useful for loading items from disk where we have borrowed NBT data
    /// and want to avoid the overhead of converting to an owned tag first.
    #[must_use]
    pub fn from_borrowed_compound(compound: &NbtCompoundView<'_, '_>) -> Option<Self> {
        // Get the item ID
        let id_str = compound.string("id")?.to_str();
        let id = id_str.parse::<Identifier>().ok()?;

        // Look up the item in the registry
        let item = REGISTRY.items.by_key(&id)?;

        let count = decode_persistent_count(compound.get("count"))?;

        let patch = match compound.get("components") {
            Some(tag) => DataComponentPatch::from_nbt_tag(tag)?,
            None => DataComponentPatch::new(),
        };

        Some(Self::with_count_and_patch(item, count, patch))
    }
}

fn decode_persistent_count(tag: Option<BorrowedNbtTag<'_, '_>>) -> Option<i32> {
    let count = match tag {
        Some(tag) => tag.codec_i32()?,
        None => 1,
    };
    (1..=99).contains(&count).then_some(count)
}

#[cfg(test)]
mod durability_tests {
    use steel_utils::random::xoroshiro::Xoroshiro;

    use super::ItemStack;
    use crate::data_components::vanilla_components::{ENCHANTMENTS, ItemEnchantments};
    use crate::test_support::init_test_registry;
    use crate::{vanilla_enchantments, vanilla_items};

    fn with_unbreaking(item: crate::items::ItemRef, level: u32) -> ItemStack {
        let mut stack = ItemStack::new(item);
        let mut enchantments = ItemEnchantments::empty();
        enchantments.set(vanilla_enchantments::UNBREAKING.key.clone(), level);
        stack.set(ENCHANTMENTS, enchantments);
        stack
    }

    #[test]
    fn item_damage_uses_generated_unbreaking_tool_requirements() {
        init_test_registry();
        let mut armor = with_unbreaking(&vanilla_items::DIAMOND_CHESTPLATE, 3);
        let mut tool = with_unbreaking(&vanilla_items::DIAMOND_PICKAXE, 3);
        let mut armor_random = Xoroshiro::from_seed_unmixed(42);
        let mut tool_random = Xoroshiro::from_seed_unmixed(42);

        assert!(!armor.hurt_and_break_with_random(100, false, &mut armor_random));
        assert!(!tool.hurt_and_break_with_random(100, false, &mut tool_random));
        assert_eq!(armor.get_damage_value(), 68);
        assert_eq!(tool.get_damage_value(), 18);

        let effects = vanilla_enchantments::UNBREAKING.effects.item_damage;
        assert_eq!(
            effects[0].requirements.and_then(|requirements| {
                requirements.matches_item_context(&vanilla_items::DIAMOND_CHESTPLATE)
            }),
            Some(true)
        );
        assert_eq!(
            effects[1].requirements.and_then(|requirements| {
                requirements.matches_item_context(&vanilla_items::DIAMOND_CHESTPLATE)
            }),
            Some(false)
        );
        assert_eq!(
            effects[0].requirements.and_then(|requirements| {
                requirements.matches_item_context(&vanilla_items::DIAMOND_PICKAXE)
            }),
            Some(false)
        );
        assert_eq!(
            effects[1].requirements.and_then(|requirements| {
                requirements.matches_item_context(&vanilla_items::DIAMOND_PICKAXE)
            }),
            Some(true)
        );
    }

    #[test]
    fn match_tool_supports_generated_direct_item_sets() {
        init_test_registry();
        let requirements = vanilla_enchantments::INFINITY.effects.ammo_use[0]
            .requirements
            .expect("Infinity ammo use should have a match_tool requirement");

        assert_eq!(
            requirements.matches_item_context(&vanilla_items::ARROW),
            Some(true)
        );
        assert_eq!(
            requirements.matches_item_context(&vanilla_items::SPECTRAL_ARROW),
            Some(false)
        );
    }
}

#[cfg(test)]
mod persistence_tests {
    use std::io::Cursor;

    use simdnbt::FromNbtTag;
    use simdnbt::borrow::{NbtTag as BorrowedNbtTag, read_tag};
    use simdnbt::owned::{NbtCompound, NbtTag};
    use steel_utils::codec::VarInt;
    use steel_utils::serial::WriteTo;

    use super::ItemStack;
    use crate::data_components::vanilla_components::{
        CUSTOM_DATA, JUKEBOX_PLAYABLE, LORE, MAX_DAMAGE, MAX_STACK_SIZE, TOOLTIP_DISPLAY,
    };
    use crate::data_components::{CustomData, JukeboxPlayable};
    use crate::test_support::init_test_registry;
    use crate::{REGISTRY, RegistryEntry, RegistryExt, vanilla_items, vanilla_jukebox_songs};

    fn with_borrowed_tag<R>(tag: NbtTag, visitor: impl FnOnce(BorrowedNbtTag<'_, '_>) -> R) -> R {
        let mut bytes = Vec::new();
        tag.write(&mut bytes);
        let borrowed =
            read_tag(&mut Cursor::new(bytes.as_slice())).expect("owned test tag should parse");
        visitor(borrowed.as_tag())
    }

    fn parse_stack(compound: NbtCompound) -> Option<ItemStack> {
        with_borrowed_tag(NbtTag::Compound(compound), ItemStack::from_nbt_tag)
    }

    fn stone_stack_nbt() -> NbtCompound {
        let mut compound = NbtCompound::new();
        compound.insert("id", "minecraft:stone");
        compound
    }

    fn untrusted_stack_bytes(
        count: i32,
        component: Option<(&steel_utils::Identifier, Vec<u8>)>,
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        VarInt(count)
            .write(&mut bytes)
            .expect("test stack count should encode");
        VarInt(vanilla_items::STONE.id() as i32)
            .write(&mut bytes)
            .expect("test item id should encode");

        if let Some((component, value)) = component {
            VarInt(1)
                .write(&mut bytes)
                .expect("added component count should encode");
            VarInt(0)
                .write(&mut bytes)
                .expect("removed component count should encode");
            let component_id = REGISTRY
                .data_components
                .id_from_key(component)
                .expect("test component should be registered");
            VarInt(component_id as i32)
                .write(&mut bytes)
                .expect("component id should encode");
            VarInt(value.len() as i32)
                .write(&mut bytes)
                .expect("component length should encode");
            bytes.extend_from_slice(&value);
        } else {
            VarInt(0)
                .write(&mut bytes)
                .expect("added component count should encode");
            VarInt(0)
                .write(&mut bytes)
                .expect("removed component count should encode");
        }
        bytes
    }

    #[test]
    fn persistent_item_count_uses_vanilla_integer_codec() {
        init_test_registry();
        let mut compound = stone_stack_nbt();
        compound.insert("count", 5.9_f64);
        assert_eq!(parse_stack(compound).map(|stack| stack.count()), Some(5));

        let mut compound = stone_stack_nbt();
        compound.insert("count", 100);
        assert!(parse_stack(compound).is_none());

        let mut compound = stone_stack_nbt();
        compound.insert("count", "5");
        assert!(parse_stack(compound).is_none());
    }

    #[test]
    fn malformed_present_component_patch_rejects_the_item_stack() {
        init_test_registry();
        let mut components = NbtCompound::new();
        components.insert("minecraft:max_stack_size", 0);
        let mut compound = stone_stack_nbt();
        compound.insert("components", components);

        assert!(parse_stack(compound).is_none());
    }

    #[test]
    fn component_patches_stay_sanitized_against_the_item_prototype() {
        init_test_registry();
        let mut patch = crate::data_components::DataComponentPatch::new();
        patch.set(MAX_STACK_SIZE, 64);
        patch.remove(CUSTOM_DATA);
        let mut stack = ItemStack::with_count_and_patch(&vanilla_items::STONE, 1, patch);
        assert!(stack.components_patch().is_empty());

        stack.set(MAX_STACK_SIZE, 16);
        assert_eq!(stack.components_patch().len(), 1);
        stack.set(MAX_STACK_SIZE, 64);
        assert!(stack.components_patch().is_empty());

        stack.remove(MAX_STACK_SIZE);
        assert!(stack.components_patch().is_removed(&MAX_STACK_SIZE.key));
        stack.set(MAX_STACK_SIZE, 64);
        assert!(stack.components_patch().is_empty());

        stack.remove(CUSTOM_DATA);
        assert!(stack.components_patch().is_empty());

        stack.set(MAX_STACK_SIZE, 16);
        stack.set_item(&vanilla_items::ENDER_PEARL.key);
        assert_eq!(stack.max_stack_size(), 16);
        assert!(stack.components_patch().is_empty());
    }

    #[test]
    fn strict_validation_checks_components_even_when_the_stack_is_empty() {
        init_test_registry();
        let mut patch = crate::data_components::DataComponentPatch::new();
        patch.set(MAX_DAMAGE, 1);
        let stack = ItemStack::with_count_and_patch(&vanilla_items::STONE, 0, patch);

        assert!(stack.is_empty());
        assert!(stack.validate_strict().is_err());
    }

    #[test]
    fn default_count_is_always_present_in_persistent_encoding() {
        init_test_registry();
        let stack = ItemStack::new(&vanilla_items::STONE);
        let NbtTag::Compound(compound) = stack.to_nbt_tag_ref() else {
            panic!("item stack should encode as a compound");
        };

        assert_eq!(compound.get("count"), Some(&NbtTag::Int(1)));
    }

    #[test]
    fn untrusted_stack_rejects_direct_jukebox_holders() {
        init_test_registry();
        let mut component_bytes = Vec::new();
        VarInt(0)
            .write(&mut component_bytes)
            .expect("direct holder discriminator should encode");
        let bytes = untrusted_stack_bytes(1, Some((&JUKEBOX_PLAYABLE.key, component_bytes)));

        assert!(ItemStack::read_untrusted(&mut Cursor::new(bytes.as_slice())).is_err());
    }

    #[test]
    fn untrusted_stack_accepts_persistable_registry_holders() {
        init_test_registry();
        let reference = JukeboxPlayable::new(&vanilla_jukebox_songs::CAT);
        let mut component_bytes = Vec::new();
        reference
            .write(&mut component_bytes)
            .expect("registry holder should have a network representation");
        let bytes = untrusted_stack_bytes(1, Some((&JUKEBOX_PLAYABLE.key, component_bytes)));

        let stack = ItemStack::read_untrusted(&mut Cursor::new(bytes.as_slice()))
            .expect("persistable untrusted stack should decode");
        assert_eq!(stack.get(JUKEBOX_PLAYABLE), Some(&reference));
    }

    #[test]
    fn untrusted_stack_uses_persistent_count_range() {
        init_test_registry();
        let bytes = untrusted_stack_bytes(100, None);

        assert!(ItemStack::read_untrusted(&mut Cursor::new(bytes.as_slice())).is_err());
    }

    #[test]
    fn untrusted_stack_validates_component_persistent_constraints() {
        init_test_registry();
        let mut component_bytes = Vec::new();
        VarInt(0)
            .write(&mut component_bytes)
            .expect("max stack size should encode on the network");
        let bytes = untrusted_stack_bytes(
            1,
            Some((
                &crate::data_components::vanilla_components::MAX_STACK_SIZE.key,
                component_bytes,
            )),
        );

        assert!(ItemStack::read_untrusted(&mut Cursor::new(bytes.as_slice())).is_err());
    }

    #[test]
    fn save_omits_invalid_component_value_but_keeps_present_patch_field() {
        init_test_registry();
        let mut stack = ItemStack::new(&vanilla_items::STONE);
        stack.set(MAX_STACK_SIZE, 0);

        assert!(stack.validate_persistent_encoding().is_err());
        let NbtTag::Compound(compound) = stack.to_nbt_tag_ref() else {
            panic!("item stack should still encode as a compound");
        };
        assert_eq!(
            compound.string("id").map(|value| value.to_str()),
            Some("minecraft:stone".into())
        );
        assert!(
            compound
                .compound("components")
                .is_some_and(simdnbt::owned::NbtCompound::is_empty)
        );
    }

    #[test]
    fn toggle_tooltips_updates_the_typed_display_component() {
        init_test_registry();
        let mut stack = ItemStack::new(&vanilla_items::STONE);

        stack.toggle_tooltips(&[(LORE.key.clone(), false)]);
        let display = stack
            .get(TOOLTIP_DISPLAY)
            .expect("tooltip display should be set");
        assert!(!display.shows(LORE));

        stack.toggle_tooltips(&[(LORE.key.clone(), true)]);
        assert!(
            stack
                .get(TOOLTIP_DISPLAY)
                .expect("tooltip display should remain set")
                .shows(LORE)
        );
    }

    #[test]
    fn set_custom_data_recursively_merges_and_removes_empty_values() {
        init_test_registry();
        let mut stack = ItemStack::new(&vanilla_items::STONE);
        let empty = CustomData::default();
        stack.set_custom_data(&empty);
        assert!(stack.get(CUSTOM_DATA).is_none());

        let mut nested = NbtCompound::new();
        nested.insert("kept", 1);
        nested.insert("changed", 1);
        let mut first = NbtCompound::new();
        first.insert("nested", nested);
        stack.set_custom_data(
            &CustomData::try_from_compound(first).expect("first value should be valid"),
        );

        let mut nested = NbtCompound::new();
        nested.insert("changed", 2);
        let mut second = NbtCompound::new();
        second.insert("nested", nested);
        stack.set_custom_data(
            &CustomData::try_from_compound(second).expect("second value should be valid"),
        );

        let nested = stack
            .get(CUSTOM_DATA)
            .and_then(|data| data.as_compound().compound("nested"))
            .expect("nested custom data should remain");
        assert_eq!(nested.int("kept"), Some(1));
        assert_eq!(nested.int("changed"), Some(2));
    }
}
