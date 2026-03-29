use crate::{REGISTRY, RegistryEntry, RegistryExt};
use rustc_hash::FxHashMap;
use steel_utils::Identifier;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EquipmentSlotGroup {
    Any,
    Hand,
    Mainhand,
    Offhand,
    Armor,
    Head,
    Chest,
    Legs,
    Feet,
    Body,
}

/// Enchanting cost formula: `base + per_level_above_first * (level - 1)`.
#[derive(Debug, Clone, Copy)]
pub struct EnchantmentCost {
    pub base: i32,
    pub per_level_above_first: i32,
}

#[derive(Debug)]
pub struct Enchantment {
    pub key: Identifier,
    pub max_level: u32,
    pub min_cost: EnchantmentCost,
    pub max_cost: EnchantmentCost,
    pub anvil_cost: i32,
    pub weight: u32,
    pub slots: &'static [EquipmentSlotGroup],
    pub supported_items: &'static str,
    pub primary_items: Option<&'static str>,
    pub exclusive_set: Option<&'static str>,
    // TODO: effects (data-driven, complex nested JSON structures)
}

impl RegistryEntry for Enchantment {
    fn key(&self) -> &Identifier {
        &self.key
    }

    fn try_id(&self) -> Option<usize> {
        REGISTRY.enchantments.id_from_key(&self.key)
    }
}

pub type EnchantmentRef = &'static Enchantment;

impl PartialEq for EnchantmentRef {
    #[allow(clippy::disallowed_methods)]
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(*self, *other)
    }
}

impl Eq for EnchantmentRef {}

pub struct EnchantmentRegistry {
    enchantments_by_id: Vec<EnchantmentRef>,
    enchantments_by_key: FxHashMap<Identifier, usize>,
    allows_registering: bool,
}

impl EnchantmentRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            enchantments_by_id: Vec::new(),
            enchantments_by_key: FxHashMap::default(),
            allows_registering: true,
        }
    }

    pub fn register(&mut self, enchantment: EnchantmentRef) -> usize {
        assert!(
            self.allows_registering,
            "Cannot register enchantments after the registry has been frozen"
        );

        let id = self.enchantments_by_id.len();
        self.enchantments_by_key.insert(enchantment.key.clone(), id);
        self.enchantments_by_id.push(enchantment);
        id
    }

    #[must_use]
    pub fn replace(&mut self, enchantment: EnchantmentRef, id: usize) -> bool {
        if id >= self.enchantments_by_id.len() {
            return false;
        }
        let old = self.enchantments_by_id[id];
        self.enchantments_by_key.remove(&old.key);
        self.enchantments_by_key.insert(enchantment.key.clone(), id);
        self.enchantments_by_id[id] = enchantment;
        true
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, EnchantmentRef)> + '_ {
        self.enchantments_by_id
            .iter()
            .enumerate()
            .map(|(id, &ench)| (id, ench))
    }
}

crate::impl_registry_ext!(
    EnchantmentRegistry,
    Enchantment,
    enchantments_by_id,
    enchantments_by_key
);

impl Default for EnchantmentRegistry {
    fn default() -> Self {
        Self::new()
    }
}
