use super::{
    ATTRIBUTE_MODIFIERS, BREAK_SOUND, Component, ComponentData, DataComponentType, Debug,
    DowncastType, ENCHANTMENTS, FxHashMap, Identifier, ItemAttributeModifiers, ItemEnchantments,
    ItemLore, LORE, MAX_STACK_SIZE, RARITY, REPAIR_COST, Rarity, SWING_ANIMATION, SoundEventHolder,
    SwingAnimation, TOOLTIP_DISPLAY, TooltipDisplay, USE_EFFECTS, UseEffects, sound_events,
};

/// Storage for component values.
///
/// Maps component keys to their values. Used on items to store their data components.
#[derive(Debug, Clone)]
pub struct DataComponentMap {
    pub(super) map: FxHashMap<Identifier, ComponentData>,
}

impl Default for DataComponentMap {
    fn default() -> Self {
        Self::new()
    }
}

impl DataComponentMap {
    #[must_use]
    pub fn new() -> Self {
        Self {
            map: FxHashMap::default(),
        }
    }

    /// Creates a map with common item components pre-populated.
    #[must_use]
    pub fn common_item_components() -> Self {
        let mut map = FxHashMap::default();
        map.insert(MAX_STACK_SIZE.key.clone(), ComponentData::new(64_i32));
        map.insert(LORE.key.clone(), ComponentData::new(ItemLore::empty()));
        map.insert(
            ENCHANTMENTS.key.clone(),
            ComponentData::new(ItemEnchantments::empty()),
        );
        map.insert(REPAIR_COST.key.clone(), ComponentData::new(0_i32));
        map.insert(
            USE_EFFECTS.key.clone(),
            ComponentData::new(UseEffects::DEFAULT),
        );
        map.insert(
            ATTRIBUTE_MODIFIERS.key.clone(),
            ComponentData::new(ItemAttributeModifiers::empty()),
        );
        map.insert(RARITY.key.clone(), ComponentData::new(Rarity::Common));
        map.insert(
            BREAK_SOUND.key.clone(),
            ComponentData::new(SoundEventHolder::registry(&sound_events::ENTITY_ITEM_BREAK)),
        );
        map.insert(
            TOOLTIP_DISPLAY.key.clone(),
            ComponentData::new(TooltipDisplay::DEFAULT),
        );
        map.insert(
            SWING_ANIMATION.key.clone(),
            ComponentData::new(SwingAnimation::DEFAULT),
        );
        Self { map }
    }

    /// Sets a component value (builder pattern).
    #[must_use]
    pub fn builder_set<T: Component + DowncastType>(
        mut self,
        component: DataComponentType<T>,
        value: Option<T>,
    ) -> Self {
        self.set(component, value);
        self
    }

    /// Sets a component value, or removes it if `None`.
    pub fn set<T: Component + DowncastType>(
        &mut self,
        component: DataComponentType<T>,
        value: Option<T>,
    ) {
        if let Some(v) = value {
            self.map
                .insert(component.key.clone(), ComponentData::new(v));
        } else {
            self.map.remove(&component.key);
        }
    }

    /// Gets a component value by type.
    #[must_use]
    pub fn get<T: Component + DowncastType + Clone>(
        &self,
        component: DataComponentType<T>,
    ) -> Option<T> {
        let data = self.map.get(&component.key)?;
        data.downcast_ref::<T>().cloned()
    }

    /// Gets a reference to a component value.
    #[must_use]
    pub fn get_ref<T: Component + DowncastType>(
        &self,
        component: DataComponentType<T>,
    ) -> Option<&T> {
        let data = self.map.get(&component.key)?;
        data.downcast_ref::<T>()
    }

    /// Checks if a component is present.
    #[must_use]
    pub fn has<T>(&self, component: DataComponentType<T>) -> bool {
        self.map.contains_key(&component.key)
    }

    /// Returns the number of components.
    #[must_use]
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Returns true if empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Iterates over component keys.
    pub fn keys(&self) -> impl Iterator<Item = &Identifier> {
        self.map.keys()
    }

    /// Iterates over component keys and their erased values.
    pub fn iter(&self) -> impl Iterator<Item = (&Identifier, &ComponentData)> {
        self.map.iter()
    }

    /// Gets raw component data by key (for plugin use).
    #[must_use]
    pub fn get_raw(&self, key: &Identifier) -> Option<&ComponentData> {
        self.map.get(key)
    }

    /// Sets raw component data (for plugin use).
    ///
    /// Returns `true` if the data was set successfully, or `false` if the key is
    /// unregistered or the data type does not match it.
    ///
    /// This prevents plugins from setting invalid types on vanilla components.
    pub fn set_raw(&mut self, key: Identifier, data: ComponentData) -> bool {
        use crate::{REGISTRY, RegistryExt};

        let Some(entry) = REGISTRY.data_components.by_key(&key) else {
            return false;
        };
        if !entry.validates(&data) {
            return false;
        }

        self.map.insert(key, data);
        true
    }

    /// Removes a component by key.
    pub fn remove(&mut self, key: &Identifier) -> Option<ComponentData> {
        self.map.remove(key)
    }
}
