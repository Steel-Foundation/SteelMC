use crate::{REGISTRY, RegistryEntry, RegistryExt};
use rustc_hash::FxHashMap;
use simdnbt::ToNbtTag;
use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
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

impl EquipmentSlotGroup {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Any => "any",
            Self::Hand => "hand",
            Self::Mainhand => "mainhand",
            Self::Offhand => "offhand",
            Self::Armor => "armor",
            Self::Head => "head",
            Self::Chest => "chest",
            Self::Legs => "legs",
            Self::Feet => "feet",
            Self::Body => "body",
        }
    }
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

impl ToNbtTag for &Enchantment {
    fn to_nbt_tag(self) -> NbtTag {
        let mut compound = NbtCompound::new();

        // description: translatable text component {"translate": "enchantment.minecraft.<key>"}
        let mut desc = NbtCompound::new();
        desc.insert("translate", format!("enchantment.{}", self.key).as_str());
        compound.insert("description", NbtTag::Compound(desc));

        // Definition fields (inlined, not nested)
        compound.insert("supported_items", self.supported_items);
        if let Some(primary) = self.primary_items {
            compound.insert("primary_items", primary);
        }
        compound.insert("weight", self.weight as i32);
        compound.insert("max_level", self.max_level as i32);

        let mut min_cost = NbtCompound::new();
        min_cost.insert("base", self.min_cost.base);
        min_cost.insert("per_level_above_first", self.min_cost.per_level_above_first);
        compound.insert("min_cost", NbtTag::Compound(min_cost));

        let mut max_cost = NbtCompound::new();
        max_cost.insert("base", self.max_cost.base);
        max_cost.insert("per_level_above_first", self.max_cost.per_level_above_first);
        compound.insert("max_cost", NbtTag::Compound(max_cost));

        compound.insert("anvil_cost", self.anvil_cost);

        let slots: Vec<String> = self.slots.iter().map(|s| s.as_str().to_owned()).collect();
        compound.insert("slots", NbtTag::List(NbtList::from(slots)));

        if let Some(exclusive) = self.exclusive_set {
            compound.insert("exclusive_set", exclusive);
        }

        // TODO: effects (data-driven, complex nested JSON structures)

        NbtTag::Compound(compound)
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
    tags: FxHashMap<Identifier, Vec<Identifier>>,
    allows_registering: bool,
}

impl EnchantmentRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            enchantments_by_id: Vec::new(),
            enchantments_by_key: FxHashMap::default(),
            tags: FxHashMap::default(),
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

crate::impl_tagged_registry!(EnchantmentRegistry, enchantments_by_key, "enchantment");

impl Default for EnchantmentRegistry {
    fn default() -> Self {
        Self::new()
    }
}
