use crate::items::ItemRef;
pub use crate::loot_table::EquipmentSlotGroup;
use crate::{REGISTRY, RegistryEntry, RegistryExt, TaggedRegistryExt};
use rustc_hash::FxHashMap;
use simdnbt::ToNbtTag;
use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use steel_utils::Identifier;

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
        desc.insert(
            "translate",
            format!("enchantment.{}.{}", self.key.namespace, self.key.path).as_str(),
        );
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

/// Parses a tag reference string like `"#minecraft:foo"` into an `Identifier`.
fn parse_tag_ref(tag_ref: &str) -> Option<Identifier> {
    let without_hash = tag_ref.strip_prefix('#')?;
    Some(if let Some((ns, path)) = without_hash.split_once(':') {
        Identifier::new(ns.to_owned(), path.to_owned())
    } else {
        Identifier::vanilla(without_hash.to_owned())
    })
}

impl Enchantment {
    /// Checks if this enchantment can be applied to the given item via `supported_items` tag.
    pub fn can_enchant(&self, item: ItemRef) -> bool {
        let Some(tag) = parse_tag_ref(self.supported_items) else {
            return false;
        };
        REGISTRY.items.is_in_tag(item, &tag)
    }

    /// Checks if two enchantments are compatible (neither's `exclusive_set` contains the other).
    pub fn are_compatible(a: EnchantmentRef, b: EnchantmentRef) -> bool {
        if a == b {
            return false;
        }
        if let Some(set) = a.exclusive_set
            && let Some(tag) = parse_tag_ref(set)
            && REGISTRY.enchantments.is_in_tag(b, &tag)
        {
            return false;
        }
        if let Some(set) = b.exclusive_set
            && let Some(tag) = parse_tag_ref(set)
            && REGISTRY.enchantments.is_in_tag(a, &tag)
        {
            return false;
        }
        true
    }

    /// Checks if this enchantment is compatible with all existing enchantments on an item.
    pub fn is_compatible_with_existing(
        enchantment: EnchantmentRef,
        item: &crate::item_stack::ItemStack,
    ) -> bool {
        let Some(enchantments) = item.get_enchantments() else {
            return true;
        };
        for (existing_key, _) in enchantments.iter() {
            if *existing_key == enchantment.key {
                continue;
            }
            let Some(existing) = REGISTRY.enchantments.by_key(existing_key) else {
                continue;
            };
            if !Self::are_compatible(enchantment, existing) {
                return false;
            }
        }
        true
    }
}

pub type EnchantmentRef = &'static Enchantment;

impl PartialEq for EnchantmentRef {
    #[expect(clippy::disallowed_methods)] // This IS the PartialEq impl; ptr::eq is correct here
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
