//! Non-empty item stack templates used by recursive Vanilla codecs.

use std::cell::Cell;
use std::io::{Cursor, Error, Result, Write};
use std::str::FromStr;

use simdnbt::owned::{NbtCompound, NbtTag};
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::codec::VarInt;
use steel_utils::hash::{ComponentHasher, HashComponent, HashEntry, sort_map_entries};
use steel_utils::nbt::NbtNumeric as _;
use steel_utils::serial::{ReadFrom, WriteTo};
use steel_utils::{DowncastType, Identifier};

use crate::data_components::vanilla_components::{
    BUNDLE_CONTENTS, CHARGED_PROJECTILES, CONTAINER, MAX_DAMAGE, MAX_STACK_SIZE,
};
use crate::data_components::{
    Component, ComponentPatchEntry, DataComponentPatch, DataComponentType,
};
use crate::item_stack::ItemStack;
use crate::items::ItemRef;
use crate::{REGISTRY, RegistryEntry, RegistryExt, vanilla_items};

// Vanilla's stream codec has no explicit guard, but deeper values already fail
// its persistent NBT codec. Use that 512-level limit to avoid stack exhaustion.
const MAX_TEMPLATE_CODEC_DEPTH: usize = 512;

thread_local! {
    static TEMPLATE_CODEC_DEPTH: Cell<usize> = const { Cell::new(0) };
}

struct TemplateDepthGuard;

impl TemplateDepthGuard {
    fn enter() -> Result<Self> {
        TEMPLATE_CODEC_DEPTH.with(|depth| {
            let current = depth.get();
            if current >= MAX_TEMPLATE_CODEC_DEPTH {
                return Err(Error::other(format!(
                    "Item stack template nesting exceeds {MAX_TEMPLATE_CODEC_DEPTH}"
                )));
            }
            depth.set(current + 1);
            Ok(Self)
        })
    }
}

impl Drop for TemplateDepthGuard {
    fn drop(&mut self) {
        TEMPLATE_CODEC_DEPTH.with(|depth| depth.set(depth.get() - 1));
    }
}

/// A persistable, non-empty item identity, count, and component patch.
#[derive(Debug, Clone, PartialEq)]
pub struct ItemStackTemplate {
    item: ItemRef,
    count: i32,
    components: DataComponentPatch,
    components_hash: i32,
}

impl ItemStackTemplate {
    pub const MIN_COUNT: i32 = 1;
    pub const MAX_COUNT: i32 = 99;

    /// Creates the common count-one template with no component changes.
    #[must_use]
    pub fn new(item: ItemRef) -> Self {
        assert!(
            item != &*vanilla_items::AIR,
            "Item stack template item must be non-empty"
        );
        Self {
            item,
            count: 1,
            components: DataComponentPatch::new(),
            components_hash: empty_map_hash(),
        }
    }

    /// Creates a template after validating its complete persistent codec shape.
    pub fn try_with_count_and_patch(
        item: ItemRef,
        count: i32,
        components: DataComponentPatch,
    ) -> Result<Self> {
        if item == &*vanilla_items::AIR {
            return Err(Error::other("Item stack template item must be non-empty"));
        }
        if !(Self::MIN_COUNT..=Self::MAX_COUNT).contains(&count) {
            return Err(Error::other(format!(
                "Item stack template count {count} is outside the persistent range {}..={}",
                Self::MIN_COUNT,
                Self::MAX_COUNT
            )));
        }
        components.try_to_nbt_tag_ref()?;
        let components_hash = components.compute_persistent_hash()?;
        Ok(Self {
            item,
            count,
            components,
            components_hash,
        })
    }

    /// Copies a non-empty stack into its immutable template representation.
    pub fn from_stack(stack: &ItemStack) -> Result<Self> {
        if stack.is_empty() {
            return Err(Error::other("Stack must be non-empty"));
        }
        Self::try_with_count_and_patch(stack.item, stack.count, stack.components_patch().clone())
    }

    #[must_use]
    pub const fn item(&self) -> ItemRef {
        self.item
    }

    #[must_use]
    pub const fn count(&self) -> i32 {
        self.count
    }

    #[must_use]
    pub const fn components(&self) -> &DataComponentPatch {
        &self.components
    }

    #[must_use]
    pub fn create(&self) -> ItemStack {
        if let Err(error) = self.validate_strict() {
            log::warn!("Can't create item stack with properties {self:?}, error: {error}");
            return ItemStack::empty();
        }
        ItemStack::with_count_and_patch(self.item, self.count, self.components.clone())
    }

    pub(crate) fn to_nbt_tag_ref(&self) -> NbtTag {
        let mut compound = NbtCompound::new();
        compound.insert("id", self.item.key.to_string());
        if self.count != 1 {
            compound.insert("count", self.count);
        }
        if !self.components.is_empty() {
            compound.insert("components", self.components.to_nbt_tag_ref());
        }
        NbtTag::Compound(compound)
    }

    pub(crate) fn from_nbt_identifier(value: &str) -> Option<Self> {
        let _depth = TemplateDepthGuard::enter().ok()?;
        let key = Identifier::from_str(value).ok()?;
        let item = REGISTRY.items.by_key(&key)?;
        (item != &*vanilla_items::AIR).then(|| Self::new(item))
    }

    pub(crate) fn from_nbt_compound(
        compound: simdnbt::borrow::NbtCompound<'_, '_>,
    ) -> Option<Self> {
        let _depth = TemplateDepthGuard::enter().ok()?;
        let key = Identifier::from_str(&compound.get("id")?.string()?.to_str()).ok()?;
        let item = REGISTRY.items.by_key(&key)?;
        let count = match compound.get("count") {
            Some(count) => count.codec_i32()?,
            None => 1,
        };
        let components = match compound.get("components") {
            Some(components) => DataComponentPatch::from_nbt_tag(components)?,
            None => DataComponentPatch::new(),
        };
        Self::try_with_count_and_patch(item, count, components).ok()
    }

    fn validate_strict(&self) -> Result<()> {
        let max_stack_size = self.max_stack_size();
        if self.get(MAX_DAMAGE).is_some() && max_stack_size > 1 {
            return Err(Error::other("Item cannot be both damageable and stackable"));
        }

        if let Some(container) = self.get(CONTAINER) {
            validate_contained_item_sizes(container.items().iter().flatten())?;
        }

        if let Some(bundle) = self.get(BUNDLE_CONTENTS) {
            validate_contained_item_sizes(bundle.items().iter())?;
            bundle.validate_weight()?;
        }

        if let Some(projectiles) = self.get(CHARGED_PROJECTILES) {
            validate_contained_item_sizes(projectiles.items().iter())?;
        }

        if self.count > max_stack_size {
            return Err(Error::other(format!(
                "Item stack with stack size of {} was larger than maximum: {max_stack_size}",
                self.count
            )));
        }
        Ok(())
    }

    pub(crate) fn get<T: Component + DowncastType>(
        &self,
        component: DataComponentType<T>,
    ) -> Option<&T> {
        match self.components.get_entry(&component.key) {
            Some(ComponentPatchEntry::Set(value)) => value.downcast_ref::<T>(),
            Some(ComponentPatchEntry::Removed) => None,
            None => self.item.components.get_ref(component),
        }
    }

    pub(crate) fn max_stack_size(&self) -> i32 {
        self.get(MAX_STACK_SIZE).copied().unwrap_or(1)
    }
}

fn validate_contained_item_sizes<'a>(
    items: impl IntoIterator<Item = &'a ItemStackTemplate>,
) -> Result<()> {
    for item in items {
        let max_stack_size = item.max_stack_size();
        if item.count > max_stack_size {
            return Err(Error::other(format!(
                "Item stack with count of {} was larger than maximum: {max_stack_size}",
                item.count
            )));
        }
    }
    Ok(())
}

impl WriteTo for ItemStackTemplate {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        let _depth = TemplateDepthGuard::enter()?;
        let item_id = i32::try_from(self.item.id())
            .map_err(|_| Error::other(format!("Item id is too large: {}", self.item.id())))?;
        VarInt(item_id).write(writer)?;
        VarInt(self.count).write(writer)?;
        self.components.write(writer)
    }
}

impl ReadFrom for ItemStackTemplate {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let _depth = TemplateDepthGuard::enter()?;
        let item_id = VarInt::read(data)?.0;
        let item_id = usize::try_from(item_id)
            .map_err(|_| Error::other(format!("Negative item id: {item_id}")))?;
        let item = REGISTRY
            .items
            .by_id(item_id)
            .ok_or_else(|| Error::other(format!("Unknown item id: {item_id}")))?;
        let count = VarInt::read(data)?.0;
        let components = DataComponentPatch::read(data)?;
        Self::try_with_count_and_patch(item, count, components)
    }
}

impl ToNbtTag for ItemStackTemplate {
    fn to_nbt_tag(self) -> NbtTag {
        self.to_nbt_tag_ref()
    }
}

impl FromNbtTag for ItemStackTemplate {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        if let Some(value) = tag.string() {
            return Self::from_nbt_identifier(&value.to_str());
        }
        Self::from_nbt_compound(tag.compound()?)
    }
}

impl HashComponent for ItemStackTemplate {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::with_capacity(3);
        push_hash_entry(&mut entries, "id", self.item.key.to_string().compute_hash());
        if self.count != 1 {
            push_hash_entry(&mut entries, "count", self.count.compute_hash());
        }
        if !self.components.is_empty() {
            push_hash_entry(&mut entries, "components", self.components_hash);
        }
        sort_map_entries(&mut entries);
        hasher.start_map();
        for entry in &entries {
            hasher.put_raw_bytes(&entry.key_bytes);
            hasher.put_raw_bytes(&entry.value_bytes);
        }
        hasher.end_map();
    }
}

fn empty_map_hash() -> i32 {
    let mut hasher = ComponentHasher::new();
    hasher.start_map();
    hasher.end_map();
    hasher.finish()
}

fn push_hash_entry(entries: &mut Vec<HashEntry>, key: &str, value_hash: i32) {
    let key_hash = key.compute_hash() as u32;
    let value_hash = value_hash as u32;
    entries.push(HashEntry {
        key_hash: i64::from(key_hash),
        value_hash: i64::from(value_hash),
        key_bytes: key_hash.to_le_bytes(),
        value_bytes: value_hash.to_le_bytes(),
    });
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use simdnbt::borrow::read_tag;
    use simdnbt::owned::{NbtCompound, NbtTag};
    use simdnbt::{FromNbtTag as _, ToNbtTag as _};
    use steel_utils::hash::HashComponent as _;
    use steel_utils::serial::{ReadFrom as _, WriteTo as _};

    use super::ItemStackTemplate;
    use crate::data_components::DataComponentPatch;
    use crate::data_components::components::{
        BundleContents, ChargedProjectiles, ItemContainerContents,
    };
    use crate::data_components::vanilla_components::{
        BUNDLE_CONTENTS, CHARGED_PROJECTILES, CONTAINER, ENCHANTMENT_GLINT_OVERRIDE, MAX_DAMAGE,
        MAX_STACK_SIZE,
    };
    use crate::test_support::init_test_registry;
    use crate::vanilla_items;

    fn parse(tag: NbtTag) -> Option<ItemStackTemplate> {
        let mut bytes = Vec::new();
        tag.write(&mut bytes);
        let borrowed = read_tag(&mut Cursor::new(bytes.as_slice())).ok()?;
        ItemStackTemplate::from_nbt_tag(borrowed.as_tag())
    }

    #[test]
    fn item_only_alternative_decodes_and_primary_codec_omits_defaults() {
        init_test_registry();
        let template = ItemStackTemplate::new(&vanilla_items::STICK);
        let mut expected = NbtCompound::new();
        expected.insert("id", "minecraft:stick");
        let expected = NbtTag::Compound(expected);
        assert_eq!(template.clone().to_nbt_tag(), expected);
        assert_eq!(template.compute_hash(), expected.compute_hash());
        assert_eq!(parse(NbtTag::String("stick".into())), Some(template));
    }

    #[test]
    fn complete_templates_round_trip_both_codecs() {
        init_test_registry();
        let mut patch = DataComponentPatch::new();
        patch.set(ENCHANTMENT_GLINT_OVERRIDE, true);
        let template =
            ItemStackTemplate::try_with_count_and_patch(&vanilla_items::DIAMOND, 3, patch)
                .expect("valid template should construct");
        assert_eq!(parse(template.clone().to_nbt_tag()), Some(template.clone()));

        let mut network = Vec::new();
        template
            .write(&mut network)
            .expect("template should encode");
        assert_eq!(
            ItemStackTemplate::read(&mut Cursor::new(network.as_slice()))
                .expect("template should decode"),
            template
        );
    }

    #[test]
    fn template_invariants_reject_empty_items_and_unpersistable_counts() {
        init_test_registry();
        for count in [-1, 0, 100] {
            assert!(
                ItemStackTemplate::try_with_count_and_patch(
                    &vanilla_items::STICK,
                    count,
                    DataComponentPatch::new(),
                )
                .is_err()
            );
        }
        assert!(
            ItemStackTemplate::try_with_count_and_patch(
                &vanilla_items::AIR,
                1,
                DataComponentPatch::new(),
            )
            .is_err()
        );
    }

    #[test]
    fn item_only_air_alternative_returns_codec_failure() {
        init_test_registry();

        assert!(parse(NbtTag::String("minecraft:air".into())).is_none());
    }

    #[test]
    fn create_rejects_invalid_effective_stack_constraints() {
        init_test_registry();

        let mut oversized_patch = DataComponentPatch::new();
        oversized_patch.set(MAX_STACK_SIZE, 1);
        let oversized =
            ItemStackTemplate::try_with_count_and_patch(&vanilla_items::STONE, 2, oversized_patch)
                .expect("template codec permits counts above the effective stack maximum");
        assert!(oversized.create().is_empty());

        let mut damageable_patch = DataComponentPatch::new();
        damageable_patch.set(MAX_DAMAGE, 1);
        let damageable =
            ItemStackTemplate::try_with_count_and_patch(&vanilla_items::STONE, 1, damageable_patch)
                .expect("individually valid components should construct a template");
        assert!(damageable.create().is_empty());

        assert!(
            !ItemStackTemplate::new(&vanilla_items::STONE)
                .create()
                .is_empty()
        );
    }

    #[test]
    fn create_rejects_oversized_recursive_contents() {
        init_test_registry();

        let mut container_patch = DataComponentPatch::new();
        container_patch.set(
            CONTAINER,
            ItemContainerContents::new(vec![Some(oversized_stone_template())])
                .expect("one container slot should be valid"),
        );
        let container =
            ItemStackTemplate::try_with_count_and_patch(&vanilla_items::STONE, 1, container_patch)
                .expect("container component should persist");
        assert!(container.create().is_empty());

        let mut bundle_patch = DataComponentPatch::new();
        bundle_patch.set(
            BUNDLE_CONTENTS,
            BundleContents::new(vec![oversized_stone_template()]),
        );
        let bundle =
            ItemStackTemplate::try_with_count_and_patch(&vanilla_items::STONE, 1, bundle_patch)
                .expect("bundle component should persist");
        assert!(bundle.create().is_empty());

        let mut projectile_patch = DataComponentPatch::new();
        projectile_patch.set(
            CHARGED_PROJECTILES,
            ChargedProjectiles::new(vec![oversized_stone_template()])
                .expect("one charged projectile should be valid"),
        );
        let projectiles =
            ItemStackTemplate::try_with_count_and_patch(&vanilla_items::STONE, 1, projectile_patch)
                .expect("charged-projectiles component should persist");
        assert!(projectiles.create().is_empty());
    }

    #[test]
    fn create_rejects_excessive_bundle_weight_arithmetic() {
        init_test_registry();

        let items = [97, 89, 83, 79, 73]
            .into_iter()
            .map(|max_stack_size| {
                let mut patch = DataComponentPatch::new();
                patch.set(MAX_STACK_SIZE, max_stack_size);
                ItemStackTemplate::try_with_count_and_patch(&vanilla_items::STONE, 1, patch)
                    .expect("prime max stack size should be persistable")
            })
            .collect();
        let mut patch = DataComponentPatch::new();
        patch.set(BUNDLE_CONTENTS, BundleContents::new(items));
        let template = ItemStackTemplate::try_with_count_and_patch(&vanilla_items::STONE, 1, patch)
            .expect("bundle with individually valid entries should persist");

        assert!(template.create().is_empty());
    }

    fn oversized_stone_template() -> ItemStackTemplate {
        let mut patch = DataComponentPatch::new();
        patch.set(MAX_STACK_SIZE, 1);
        ItemStackTemplate::try_with_count_and_patch(&vanilla_items::STONE, 2, patch)
            .expect("template codec permits counts above the effective stack maximum")
    }
}
