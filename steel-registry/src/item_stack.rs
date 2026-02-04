//! Item stack implementation.

use std::io::{Cursor, Result, Write};

use steel_utils::{
    Identifier,
    codec::VarInt,
    serial::{ReadFrom, WriteTo},
};

use crate::{
    REGISTRY,
    data_components::{
        Component, ComponentData, ComponentPatchEntry, DataComponentMap, DataComponentPatch,
        DataComponentType,
        vanilla_components::{
            DAMAGE, EQUIPPABLE, Equippable, EquippableSlot, MAX_DAMAGE, MAX_STACK_SIZE, TOOL, Tool,
            UNBREAKABLE,
        },
    },
    items::ItemRef,
    vanilla_items::ITEMS,
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
            item: &ITEMS.air,
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
    pub fn with_count_and_patch(item: ItemRef, count: i32, patch: DataComponentPatch) -> Self {
        Self { item, count, patch }
    }

    #[must_use]
    fn prototype(&self) -> &'static DataComponentMap {
        &self.item.components
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        std::ptr::eq(self.item, &ITEMS.air) || self.count <= 0
    }

    #[must_use]
    pub fn item(&self) -> ItemRef {
        if self.is_empty() {
            &ITEMS.air
        } else {
            self.item
        }
    }

    #[must_use]
    pub fn count(&self) -> i32 {
        if self.is_empty() { 0 } else { self.count }
    }

    pub fn set_count(&mut self, count: i32) {
        self.count = count;
    }

    /// Increases the count by the given amount.
    pub fn grow(&mut self, amount: i32) {
        self.count += amount;
    }

    /// Decreases the count by the given amount.
    pub fn shrink(&mut self, amount: i32) {
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

    /// Damages the item and breaks it if durability reaches zero.
    ///
    /// Returns `true` if the item broke and should be removed/replaced.
    ///
    /// # Arguments
    /// * `amount` - The amount of damage to apply
    /// * `has_infinite_materials` - If true (creative mode), skip damage entirely
    ///
    /// This handles:
    /// - Checking if the item is damageable
    /// - Skipping damage for players with infinite materials (creative mode)
    /// - Applying unbreaking enchantment (TODO: when enchantments are implemented)
    /// - Breaking the item when durability reaches zero
    pub fn hurt_and_break(&mut self, amount: i32, has_infinite_materials: bool) -> bool {
        if !self.is_damageable_item() || amount <= 0 {
            return false;
        }

        // Creative mode players don't consume durability
        if has_infinite_materials {
            return false;
        }

        // TODO: Apply unbreaking enchantment
        // let unbreaking_level = self.get_enchantment_level_by_name("unbreaking");
        // Vanilla formula: chance to not consume durability = unbreaking_level / (unbreaking_level + 1)
        // For tools: 100% / (level + 1) chance to consume durability
        let effective_amount = amount;

        let new_damage = self.get_damage_value() + effective_amount;

        // TODO: Trigger ITEM_DURABILITY_CHANGED advancement criteria

        self.set_damage_value(new_damage);

        // Check if item broke
        if self.is_broken() {
            // TODO: Call onEquippedItemBroken callback which:
            // - Broadcasts entity event (byte 47 for mainhand) for break sound/particles
            // - Stops location-based effects (removes attribute modifiers)
            self.shrink(1);
            return true;
        }

        false
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

    pub fn max_stack_size(&self) -> i32 {
        self.get(MAX_STACK_SIZE).copied().unwrap_or(64)
    }

    /// Returns the equippable component if this item has one.
    #[must_use]
    pub fn get_equippable(&self) -> Option<&Equippable> {
        self.get(EQUIPPABLE)
    }

    /// Returns the equipment slot this item can be equipped to, if any.
    #[must_use]
    pub fn get_equippable_slot(&self) -> Option<EquippableSlot> {
        self.get_equippable().map(|e| e.slot)
    }

    /// Returns true if this item can be equipped in the given slot.
    #[must_use]
    pub fn is_equippable_in_slot(&self, slot: EquippableSlot) -> bool {
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
    pub fn get<T: Component>(&self, component: DataComponentType<T>) -> Option<&T> {
        let data = self.get_effective_value_raw(&component.key)?;
        T::from_data_ref(data)
    }

    /// Gets the effective value of a component, or returns the default value if not present.
    #[must_use]
    pub fn get_or_default<T: Component>(&self, component: DataComponentType<T>, default: T) -> T {
        self.get(component).cloned().unwrap_or(default)
    }

    /// Sets a component value in this item's patch, overriding the prototype.
    pub fn set<T: Component>(&mut self, component: DataComponentType<T>, value: T) {
        self.patch.set(component, value);
    }

    /// Removes a component from this item (marks it as removed in the patch).
    /// This will hide the component even if it exists in the prototype.
    pub fn remove<T: 'static>(&mut self, component: DataComponentType<T>) {
        self.patch.remove(component);
    }

    /// Clears any patch entry for this component (neither set nor removed).
    /// The prototype value will be visible again.
    pub fn clear<T: 'static>(&mut self, component: DataComponentType<T>) {
        self.patch.clear(component);
    }

    /// Returns a reference to the component patch.
    #[must_use]
    pub fn patch(&self) -> &DataComponentPatch {
        &self.patch
    }

    /// Gets the Tool component if present.
    #[must_use]
    pub fn get_tool(&self) -> Option<&Tool> {
        self.get(TOOL)
    }

    /// Returns the mining speed for the given block state ID.
    /// If no Tool component is present, returns 1.0 (hand speed).
    #[must_use]
    pub fn get_destroy_speed(&self, block_state_id: steel_utils::BlockStateId) -> f32 {
        self.get_tool()
            .map(|tool| tool.get_mining_speed(block_state_id))
            .unwrap_or(1.0)
    }

    /// Returns true if this tool is correct for getting drops from the block.
    #[must_use]
    pub fn is_correct_tool_for_drops(&self, block_state_id: steel_utils::BlockStateId) -> bool {
        self.get_tool()
            .map(|tool| tool.is_correct_for_drops(block_state_id))
            .unwrap_or(false)
    }

    /// Returns the damage per block for this tool (how much durability is consumed per block mined).
    /// Returns 0 if no Tool component is present.
    #[must_use]
    pub fn get_tool_damage_per_block(&self) -> i32 {
        self.get_tool()
            .map(|tool| tool.damage_per_block)
            .unwrap_or(0)
    }

    /// Returns true if this tool can destroy blocks in creative mode.
    /// Returns true if no Tool component is present (default behavior).
    #[must_use]
    pub fn can_destroy_blocks_in_creative(&self) -> bool {
        self.get_tool()
            .map(|tool| tool.can_destroy_blocks_in_creative)
            .unwrap_or(true)
    }

    /// Gets the level of an enchantment on this item by identifier.
    /// Returns 0 if the enchantment is not present.
    #[must_use]
    pub fn get_enchantment_level(&self, _enchantment: &Identifier) -> i32 {
        // TODO: Implement proper enchantment lookup once ENCHANTMENTS component is implemented
        // For now, return 0 (no enchantment)
        0
    }

    /// Gets the level of an enchantment on this item by name (e.g., "silk_touch", "fortune").
    /// Returns 0 if the enchantment is not present.
    #[must_use]
    pub fn get_enchantment_level_by_name(&self, _name: &str) -> i32 {
        // TODO: Implement proper enchantment lookup once ENCHANTMENTS component is implemented
        // For now, return 0 (no enchantment)
        0
    }

    /// Sets the damage/durability as a fraction (0.0 = broken, 1.0 = full).
    /// If `add` is true, adds to current damage instead of setting.
    pub fn set_damage_fraction(&mut self, _fraction: f32, _add: bool) {
        // TODO: Implement when damage component system is ready
        // let max_damage = self.get_max_damage();
        // let damage_value = ((1.0 - fraction) * max_damage as f32) as i32;
        // self.set_component(DAMAGE, damage_value);
    }

    /// Enchants this item randomly with enchantments from the given options.
    pub fn enchant_randomly<R: rand::Rng>(
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
    pub fn enchant_with_levels<R: rand::Rng>(
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
    pub fn copy_components<R: rand::Rng>(
        &mut self,
        _source: crate::loot_table::CopySource,
        _include: &[Identifier],
        _ctx: &crate::loot_table::LootContext<'_, R>,
    ) {
        // TODO: Implement when block entity system is ready
        // 1. Get the source entity/block entity from context
        // 2. For each component in `include`, copy it to this item's patch
    }

    /// Copies block state properties to this item (for blocks like note_block).
    pub fn copy_block_state<R: rand::Rng>(
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
    pub fn set_components_from_json(&mut self, _components: &str) {
        // TODO: Implement component parsing from JSON
        // Parse the JSON and set each component in the patch
    }

    /// Sets custom NBT data on this item (merges with existing custom_data).
    pub fn set_custom_data(&mut self, _tag: &str) {
        // TODO: Implement when NBT/SNBT parsing is available
        // 1. Parse the tag string as SNBT (Stringified NBT)
        // 2. Merge it with existing CUSTOM_DATA component
        // 3. Set the merged result as the new CUSTOM_DATA
    }

    /// Applies furnace smelting to convert this item (e.g., raw iron -> iron ingot).
    pub fn apply_furnace_smelt(&mut self) {
        // TODO: Implement smelting recipe lookup
        // 1. Look up this item in smelting recipes
        // 2. If found, replace self.item with the result item
        // Note: This changes the item type, not just components
    }

    /// Creates an exploration map pointing to a structure.
    pub fn create_exploration_map(
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
    pub fn set_name(&mut self, _name: &str, _target: crate::loot_table::NameTarget) {
        // TODO: Implement name setting
        // Parse the name as a text component and set CUSTOM_NAME or ITEM_NAME
    }

    /// Sets the ominous bottle amplifier.
    pub fn set_ominous_bottle_amplifier(&mut self, _amplifier: i32) {
        // TODO: Implement ominous bottle amplifier component
        // Set the OMINOUS_BOTTLE_AMPLIFIER component
    }

    /// Sets the potion type for this item.
    pub fn set_potion(&mut self, _id: &Identifier) {
        // TODO: Implement potion type setting
        // Set the POTION_CONTENTS component with the potion ID
    }

    /// Sets the suspicious stew effects for this item.
    pub fn set_stew_effects<R: rand::Rng>(
        &mut self,
        _effects: &[crate::loot_table::StewEffect],
        _rng: &mut R,
    ) {
        // TODO: Implement stew effect setting
        // Set the SUSPICIOUS_STEW_EFFECTS component
        // Duration is determined by each effect's NumberProvider
    }

    /// Sets the instrument for a goat horn.
    pub fn set_instrument<R: rand::Rng>(&mut self, _options: &Identifier, _rng: &mut R) {
        // TODO: Implement instrument setting
        // Pick a random instrument from the tag and set INSTRUMENT component
    }

    /// Sets enchantments on this item.
    pub fn set_enchantments<R: rand::Rng>(
        &mut self,
        _enchantments: &[(Identifier, crate::loot_table::NumberProvider)],
        _add: bool,
        _rng: &mut R,
    ) {
        // TODO: Implement enchantment setting
        // For each enchantment, get the level from NumberProvider
        // If add is true, add to existing levels; otherwise replace
    }

    /// Changes the item type entirely.
    pub fn set_item(&mut self, new_item: &Identifier) {
        if let Some(item_ref) = REGISTRY.items.by_key(new_item) {
            self.item = item_ref;
            // Note: Components patch may need adjustment for new item type
        }
    }

    /// Copies the name from a source entity/block to this item.
    pub fn copy_name<R: rand::Rng>(
        &mut self,
        _source: crate::loot_table::CopySource,
        _ctx: &crate::loot_table::LootContext<'_, R>,
    ) {
        // TODO: Implement when entity/block entity name access is available
        // Get name from source (block_entity.custom_name or entity.custom_name)
        // Set as CUSTOM_NAME component
    }

    /// Sets lore lines on this item.
    pub fn set_lore(&mut self, _lore: &[&str], _mode: crate::loot_table::ListOperation) {
        // TODO: Implement lore setting
        // Parse lore strings as text components and set LORE component
        // Apply mode (replace, append, insert, etc.)
    }

    /// Sets container inventory contents.
    pub fn set_contents<R: rand::Rng>(
        &mut self,
        _entries: &[crate::loot_table::LootEntry],
        _component_type: &Identifier,
        _ctx: &mut crate::loot_table::LootContext<'_, R>,
    ) {
        // TODO: Implement container contents setting
        // Generate items from entries and set as CONTAINER component
    }

    /// Modifies existing container contents.
    pub fn modify_contents<R: rand::Rng>(
        &mut self,
        _modifier: &[crate::loot_table::ConditionalLootFunction],
        _component_type: &Identifier,
        _ctx: &mut crate::loot_table::LootContext<'_, R>,
    ) {
        // TODO: Implement container contents modification
        // Apply modifier functions to existing container contents
    }

    /// Sets the container's loot table reference.
    pub fn set_loot_table(&mut self, _loot_table: &Identifier, _seed: Option<i64>) {
        // TODO: Implement loot table reference setting
        // Set CONTAINER_LOOT component with table reference and seed
    }

    /// Sets attribute modifiers on this item.
    pub fn set_attributes<R: rand::Rng>(
        &mut self,
        _modifiers: &[crate::loot_table::AttributeModifier],
        _replace: bool,
        _rng: &mut R,
    ) {
        // TODO: Implement attribute modifier setting
        // Set ATTRIBUTE_MODIFIERS component
    }

    /// Fills a player head with texture from an entity.
    pub fn fill_player_head<R: rand::Rng>(
        &mut self,
        _entity: crate::loot_table::LootContextEntity,
        _ctx: &crate::loot_table::LootContext<'_, R>,
    ) {
        // TODO: Implement player head texture filling
        // Get player profile from entity and set PROFILE component
    }

    /// Copies custom NBT data from a source.
    pub fn copy_custom_data<R: rand::Rng>(
        &mut self,
        _source: crate::loot_table::CopySource,
        _operations: &[crate::loot_table::CopyDataOperation],
        _ctx: &crate::loot_table::LootContext<'_, R>,
    ) {
        // TODO: Implement custom data copying
        // Copy NBT paths from source to item's CUSTOM_DATA component
    }

    /// Sets banner pattern layers.
    pub fn set_banner_pattern(
        &mut self,
        _patterns: &[crate::loot_table::BannerPattern],
        _append: bool,
    ) {
        // TODO: Implement banner pattern setting
        // Set BANNER_PATTERNS component
    }

    /// Sets firework rocket properties.
    pub fn set_fireworks(
        &mut self,
        _explosions: Option<&[crate::loot_table::FireworkExplosion]>,
        _flight_duration: Option<i32>,
    ) {
        // TODO: Implement firework setting
        // Set FIREWORKS component
    }

    /// Sets firework star explosion properties.
    pub fn set_firework_explosion(&mut self, _explosion: &crate::loot_table::FireworkExplosion) {
        // TODO: Implement firework explosion setting
        // Set FIREWORK_EXPLOSION component
    }

    /// Sets book cover (title/author for written books).
    pub fn set_book_cover(
        &mut self,
        _title: Option<&str>,
        _author: Option<&str>,
        _generation: Option<i32>,
    ) {
        // TODO: Implement book cover setting
        // Set WRITTEN_BOOK_CONTENT component fields
    }

    /// Sets written book page contents.
    pub fn set_written_book_pages(
        &mut self,
        _pages: &[&str],
        _mode: crate::loot_table::ListOperation,
    ) {
        // TODO: Implement written book pages setting
        // Set WRITTEN_BOOK_CONTENT pages
    }

    /// Sets writable book page contents.
    pub fn set_writable_book_pages(
        &mut self,
        _pages: &[&str],
        _mode: crate::loot_table::ListOperation,
    ) {
        // TODO: Implement writable book pages setting
        // Set WRITABLE_BOOK_CONTENT pages
    }

    /// Toggles tooltip visibility for components.
    pub fn toggle_tooltips(&mut self, _toggles: &[(Identifier, bool)]) {
        // TODO: Implement tooltip toggling
        // For each component, set its show_in_tooltip flag
    }

    /// Sets custom model data.
    pub fn set_custom_model_data(&mut self, _value: i32) {
        // TODO: Implement custom model data setting
        // Set CUSTOM_MODEL_DATA component
    }

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
            let item_id = *REGISTRY.items.get_id(self.item);
            VarInt(item_id as i32).write(writer)?;
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

        let item_id = VarInt::read(data)?.0 as usize;
        let item = REGISTRY.items.by_id(item_id).unwrap_or(&ITEMS.air);

        // Read DataComponentPatch
        let patch = DataComponentPatch::read(data)?;

        Ok(Self { item, count, patch })
    }
}

// ==================== NBT Serialization ====================

use simdnbt::{
    FromNbtTag, ToNbtTag,
    borrow::{NbtCompound as NbtCompoundView, NbtTag as BorrowedNbtTag},
    owned::NbtCompound,
};

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
        if self.is_empty() {
            // Empty stacks are represented as an empty compound
            return simdnbt::owned::NbtTag::Compound(NbtCompound::new());
        }

        let mut compound = NbtCompound::new();

        // id: The item identifier
        compound.insert("id", self.item.key.to_string());

        // count: The stack count (vanilla uses Int for NBT storage)
        compound.insert("count", self.count);

        // components: The component patch (only if non-empty)
        if !self.patch.is_empty() {
            compound.insert("components", self.patch.to_nbt_tag());
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

        // Get the count (default to 1 if not present)
        let count = compound.get("count").and_then(|t| t.int()).unwrap_or(1);

        // Parse components if present
        let patch = compound
            .get("components")
            .and_then(DataComponentPatch::from_nbt_tag)
            .unwrap_or_default();

        Some(Self { item, count, patch })
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

        // Get the count (default to 1 if not present)
        let count = compound.int("count").unwrap_or(1);

        // Parse components if present
        let patch = compound
            .get("components")
            .and_then(DataComponentPatch::from_nbt_tag)
            .unwrap_or_default();

        Some(Self::with_count_and_patch(item, count, patch))
    }
}
