//! Components backed by registry holder sets.

use std::io::{Cursor, Result, Write};

use simdnbt::owned::{NbtCompound, NbtTag};
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::hash::{ComponentHasher, HashComponent};
use steel_utils::serial::{ReadFrom, WriteTo};

use crate::RegistryHolderSet;
use crate::banner_pattern::BannerPattern;
use crate::damage_type::{DamageType, DamageTypeRef};
use crate::item_stack::ItemStack;
use crate::items::Item;

/// Banner patterns unlocked by an ingredient in the loom.
pub type ProvidesBannerPatterns = RegistryHolderSet<BannerPattern>;

/// Damage types that cannot hurt an item stack.
#[derive(Debug, Clone, PartialEq)]
pub struct DamageResistant {
    types: RegistryHolderSet<DamageType>,
}

impl DamageResistant {
    #[must_use]
    pub const fn new(types: RegistryHolderSet<DamageType>) -> Self {
        Self { types }
    }

    #[must_use]
    pub const fn types(&self) -> &RegistryHolderSet<DamageType> {
        &self.types
    }

    /// Returns whether this component protects against `damage_type`.
    #[must_use]
    pub fn is_resistant_to(&self, damage_type: DamageTypeRef) -> bool {
        self.types.contains(damage_type)
    }
}

impl WriteTo for DamageResistant {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        self.types.write(writer)
    }
}

impl ReadFrom for DamageResistant {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self::new(RegistryHolderSet::read(data)?))
    }
}

impl ToNbtTag for DamageResistant {
    fn to_nbt_tag(self) -> NbtTag {
        let mut compound = NbtCompound::new();
        compound.insert("types", self.types.to_nbt_tag());
        NbtTag::Compound(compound)
    }
}

impl FromNbtTag for DamageResistant {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag<'_, '_>) -> Option<Self> {
        let compound = tag.compound()?;
        Some(Self::new(RegistryHolderSet::from_nbt_tag(
            compound.get("types")?,
        )?))
    }
}

impl HashComponent for DamageResistant {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        self.clone().to_nbt_tag().hash_component(hasher);
    }
}

/// Items accepted as repair materials for an item stack.
#[derive(Debug, Clone, PartialEq)]
pub struct Repairable {
    items: RegistryHolderSet<Item>,
}

impl Repairable {
    #[must_use]
    pub const fn new(items: RegistryHolderSet<Item>) -> Self {
        Self { items }
    }

    #[must_use]
    pub const fn items(&self) -> &RegistryHolderSet<Item> {
        &self.items
    }

    /// Returns whether `repair_item` belongs to this component's holder set.
    #[must_use]
    pub fn is_valid_repair_item(&self, repair_item: &ItemStack) -> bool {
        self.items.contains(repair_item.item())
    }
}

impl WriteTo for Repairable {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        self.items.write(writer)
    }
}

impl ReadFrom for Repairable {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self::new(RegistryHolderSet::read(data)?))
    }
}

impl ToNbtTag for Repairable {
    fn to_nbt_tag(self) -> NbtTag {
        let mut compound = NbtCompound::new();
        compound.insert("items", self.items.to_nbt_tag());
        NbtTag::Compound(compound)
    }
}

impl FromNbtTag for Repairable {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag<'_, '_>) -> Option<Self> {
        let compound = tag.compound()?;
        Some(Self::new(RegistryHolderSet::from_nbt_tag(
            compound.get("items")?,
        )?))
    }
}

impl HashComponent for Repairable {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        self.clone().to_nbt_tag().hash_component(hasher);
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use simdnbt::borrow::read_tag;
    use simdnbt::{FromNbtTag, ToNbtTag as _};
    use steel_utils::hash::HashComponent as _;
    use steel_utils::serial::{ReadFrom as _, WriteTo as _};

    use super::{DamageResistant, ProvidesBannerPatterns, Repairable};
    use crate::REGISTRY;
    use crate::RegistryHolderSet;
    use crate::data_components::vanilla_components::{
        DAMAGE_RESISTANT, PROVIDES_BANNER_PATTERNS, REPAIRABLE,
    };
    use crate::item_stack::ItemStack;
    use crate::test_support::init_test_registry;
    use crate::vanilla_banner_pattern_tags::BannerPatternTag;
    use crate::vanilla_banner_patterns;
    use crate::vanilla_damage_type_tags::DamageTypeTag;
    use crate::vanilla_damage_types;
    use crate::vanilla_item_tags::ItemTag;
    use crate::vanilla_items;

    fn parse<T: FromNbtTag>(tag: simdnbt::owned::NbtTag) -> Option<T> {
        let mut bytes = Vec::new();
        tag.write(&mut bytes);
        let borrowed = read_tag(&mut Cursor::new(bytes.as_slice())).ok()?;
        T::from_nbt_tag(borrowed.as_tag())
    }

    #[test]
    fn damage_resistant_uses_the_vanilla_record_and_holder_set_codecs() {
        init_test_registry();

        let component = DamageResistant::new(RegistryHolderSet::Tag(DamageTypeTag::IS_FIRE));
        assert!(component.is_resistant_to(&vanilla_damage_types::IN_FIRE));
        assert!(!component.is_resistant_to(&vanilla_damage_types::GENERIC));
        assert_eq!(
            parse::<DamageResistant>(component.clone().to_nbt_tag()),
            Some(component.clone())
        );
        assert_eq!(
            component.compute_hash(),
            component.clone().to_nbt_tag().compute_hash()
        );

        let mut bytes = Vec::new();
        component.write(&mut bytes).expect("component should write");
        assert_eq!(
            DamageResistant::read(&mut Cursor::new(bytes.as_slice()))
                .expect("component should read"),
            component
        );
    }

    #[test]
    fn repairable_accepts_tagged_and_direct_repair_items() {
        init_test_registry();

        let wooden = Repairable::new(RegistryHolderSet::Tag(ItemTag::WOODEN_TOOL_MATERIALS));
        assert!(wooden.is_valid_repair_item(&ItemStack::new(&vanilla_items::OAK_PLANKS)));
        assert!(!wooden.is_valid_repair_item(&ItemStack::new(&vanilla_items::DIAMOND)));

        let direct = Repairable::new(RegistryHolderSet::Direct(vec![
            &vanilla_items::PHANTOM_MEMBRANE,
        ]));
        assert!(direct.is_valid_repair_item(&ItemStack::new(&vanilla_items::PHANTOM_MEMBRANE)));
        assert!(!direct.is_valid_repair_item(&ItemStack::new(&vanilla_items::BREEZE_ROD)));
        assert_eq!(
            parse::<Repairable>(direct.clone().to_nbt_tag()),
            Some(direct.clone())
        );

        let mut bytes = Vec::new();
        direct.write(&mut bytes).expect("component should write");
        assert_eq!(
            Repairable::read(&mut Cursor::new(bytes.as_slice())).expect("component should read"),
            direct
        );
    }

    #[test]
    fn provides_banner_patterns_uses_fixed_registry_holder_sets() {
        init_test_registry();

        let component = ProvidesBannerPatterns::Tag(BannerPatternTag::PATTERN_ITEM_FLOWER);
        assert!(component.contains(&vanilla_banner_patterns::FLOWER));
        assert_eq!(
            component.clone().to_nbt_tag(),
            simdnbt::owned::NbtTag::String("#minecraft:pattern_item/flower".into())
        );
        assert_eq!(
            parse::<ProvidesBannerPatterns>(component.clone().to_nbt_tag()),
            Some(component.clone())
        );
        assert_eq!(
            component.compute_hash(),
            component.clone().to_nbt_tag().compute_hash()
        );

        let mut bytes = Vec::new();
        component.write(&mut bytes).expect("component should write");
        assert_eq!(
            ProvidesBannerPatterns::read(&mut Cursor::new(bytes.as_slice()))
                .expect("component should read"),
            component
        );
    }

    #[test]
    fn extracted_item_prototypes_include_all_holder_set_components() {
        init_test_registry();

        let damage_resistant_count = REGISTRY
            .items
            .iter()
            .filter(|(_, item)| item.components.has(DAMAGE_RESISTANT))
            .count();
        let repairable_count = REGISTRY
            .items
            .iter()
            .filter(|(_, item)| item.components.has(REPAIRABLE))
            .count();
        assert_eq!(damage_resistant_count, 17);
        assert_eq!(repairable_count, 75);

        let banner_pattern_count = REGISTRY
            .items
            .iter()
            .filter(|(_, item)| item.components.has(PROVIDES_BANNER_PATTERNS))
            .count();
        assert_eq!(banner_pattern_count, 10);

        let flower_pattern = ItemStack::new(&vanilla_items::FLOWER_BANNER_PATTERN);
        assert!(
            flower_pattern
                .get(PROVIDES_BANNER_PATTERNS)
                .is_some_and(|patterns| patterns.contains(&vanilla_banner_patterns::FLOWER))
        );

        let netherite = ItemStack::new(&vanilla_items::NETHERITE_INGOT);
        assert!(!netherite.can_be_hurt_by(&vanilla_damage_types::IN_FIRE));
        assert!(netherite.can_be_hurt_by(&vanilla_damage_types::GENERIC));

        let nether_star = ItemStack::new(&vanilla_items::NETHER_STAR);
        assert!(!nether_star.can_be_hurt_by(&vanilla_damage_types::EXPLOSION));
        assert!(nether_star.can_be_hurt_by(&vanilla_damage_types::IN_FIRE));

        let elytra = ItemStack::new(&vanilla_items::ELYTRA);
        assert!(elytra.is_valid_repair_item(&ItemStack::new(&vanilla_items::PHANTOM_MEMBRANE)));
        assert!(!elytra.is_valid_repair_item(&ItemStack::new(&vanilla_items::BREEZE_ROD)));

        let mace = ItemStack::new(&vanilla_items::MACE);
        assert!(mace.is_valid_repair_item(&ItemStack::new(&vanilla_items::BREEZE_ROD)));
        assert!(!mace.is_valid_repair_item(&ItemStack::new(&vanilla_items::PHANTOM_MEMBRANE)));
    }
}
