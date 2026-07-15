//! Vanilla `minecraft:tooltip_display` item component.

use std::io::{Cursor, Error, Result, Write};
use std::str::FromStr;

use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::Identifier;
use steel_utils::codec::VarInt;
use steel_utils::hash::{ComponentHasher, HashComponent, HashEntry, sort_map_entries};
use steel_utils::nbt::NbtNumeric as _;
use steel_utils::serial::{ReadFrom, WriteTo};

use crate::data_components::DataComponentType;
use crate::{REGISTRY, RegistryEntry, RegistryExt};

/// Global tooltip visibility and the ordered set of hidden component types.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TooltipDisplay {
    pub hide_tooltip: bool,
    hidden_components: Vec<Identifier>,
}

impl TooltipDisplay {
    pub const DEFAULT: Self = Self {
        hide_tooltip: false,
        hidden_components: Vec::new(),
    };

    #[must_use]
    pub const fn new(hide_tooltip: bool) -> Self {
        Self {
            hide_tooltip,
            hidden_components: Vec::new(),
        }
    }

    #[must_use]
    pub fn hidden_components(&self) -> &[Identifier] {
        &self.hidden_components
    }

    /// Returns a copy with `component` hidden or shown.
    #[must_use]
    pub fn with_hidden<T>(&self, component: DataComponentType<T>, hidden: bool) -> Self {
        self.with_hidden_key(component.key, hidden)
    }

    #[must_use]
    pub(crate) fn with_hidden_key(&self, component: Identifier, hidden: bool) -> Self {
        let mut result = self.clone();
        let index = result
            .hidden_components
            .iter()
            .position(|key| key == &component);
        match (index, hidden) {
            (None, true) => result.hidden_components.push(component),
            (Some(index), false) => {
                result.hidden_components.remove(index);
            }
            _ => {}
        }
        result
    }

    #[must_use]
    pub fn shows<T>(&self, component: DataComponentType<T>) -> bool {
        !self.hide_tooltip && !self.hidden_components.contains(&component.key)
    }

    fn from_hidden_components(
        hide_tooltip: bool,
        components: impl IntoIterator<Item = Identifier>,
    ) -> Self {
        let mut hidden_components = Vec::new();
        for component in components {
            if !hidden_components.contains(&component) {
                hidden_components.push(component);
            }
        }
        Self {
            hide_tooltip,
            hidden_components,
        }
    }
}

impl WriteTo for TooltipDisplay {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        self.hide_tooltip.write(writer)?;
        let count = i32::try_from(self.hidden_components.len())
            .map_err(|_| Error::other("Too many hidden tooltip components"))?;
        VarInt(count).write(writer)?;
        for component in &self.hidden_components {
            let entry = REGISTRY
                .data_components
                .by_key(component)
                .ok_or_else(|| Error::other(format!("Unknown data component type: {component}")))?;
            let id = entry.try_id().ok_or_else(|| {
                Error::other(format!("Unregistered data component type: {component}"))
            })?;
            let id = i32::try_from(id)
                .map_err(|_| Error::other(format!("Data component ID is too large: {id}")))?;
            VarInt(id).write(writer)?;
        }
        Ok(())
    }
}

impl ReadFrom for TooltipDisplay {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let hide_tooltip = bool::read(data)?;
        let count = usize::try_from(VarInt::read(data)?.0)
            .map_err(|_| Error::other("Negative hidden tooltip component count"))?;
        let mut hidden_components = Vec::with_capacity(count.min(65_536));
        for _ in 0..count {
            let id = usize::try_from(VarInt::read(data)?.0)
                .map_err(|_| Error::other("Negative data component ID"))?;
            let component = REGISTRY
                .data_components
                .by_id(id)
                .ok_or_else(|| Error::other(format!("Unknown data component ID: {id}")))?;
            if !hidden_components.contains(&component.key) {
                hidden_components.push(component.key.clone());
            }
        }
        Ok(Self {
            hide_tooltip,
            hidden_components,
        })
    }
}

impl ToNbtTag for TooltipDisplay {
    fn to_nbt_tag(self) -> NbtTag {
        let mut compound = NbtCompound::new();
        if self.hide_tooltip {
            compound.insert("hide_tooltip", true);
        }
        if !self.hidden_components.is_empty() {
            compound.insert(
                "hidden_components",
                NbtList::String(
                    self.hidden_components
                        .into_iter()
                        .map(|key| key.to_string().into())
                        .collect(),
                ),
            );
        }
        NbtTag::Compound(compound)
    }
}

impl FromNbtTag for TooltipDisplay {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        let hide_tooltip = compound
            .get("hide_tooltip")
            .map_or(Some(false), |tag| tag.codec_bool())?;
        let Some(tag) = compound.get("hidden_components") else {
            return Some(Self::new(hide_tooltip));
        };
        let list = tag.list()?;
        if list.empty() {
            return Some(Self::new(hide_tooltip));
        }
        let strings = list.strings()?;
        if strings.is_empty() {
            return Some(Self::new(hide_tooltip));
        }
        let hidden_components = strings
            .iter()
            .map(|value| Identifier::from_str(&value.to_str()).ok())
            .collect::<Option<Vec<_>>>()?;
        if hidden_components
            .iter()
            .any(|key| REGISTRY.data_components.by_key(key).is_none())
        {
            return None;
        }
        Some(Self::from_hidden_components(
            hide_tooltip,
            hidden_components,
        ))
    }
}

impl HashComponent for TooltipDisplay {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::new();
        if self.hide_tooltip {
            push_hash_entry(&mut entries, "hide_tooltip", &true);
        }
        if !self.hidden_components.is_empty() {
            let mut key_hasher = ComponentHasher::new();
            key_hasher.put_string("hidden_components");
            let mut value_hasher = ComponentHasher::new();
            value_hasher.start_list();
            for component in &self.hidden_components {
                value_hasher.put_string(&component.to_string());
            }
            value_hasher.end_list();
            entries.push(HashEntry::new(key_hasher, value_hasher));
        }
        sort_map_entries(&mut entries);
        hasher.start_map();
        for entry in entries {
            hasher.put_raw_bytes(&entry.key_bytes);
            hasher.put_raw_bytes(&entry.value_bytes);
        }
        hasher.end_map();
    }
}

fn push_hash_entry<T: HashComponent + ?Sized>(entries: &mut Vec<HashEntry>, key: &str, value: &T) {
    let mut key_hasher = ComponentHasher::new();
    key_hasher.put_string(key);
    let mut value_hasher = ComponentHasher::new();
    value.hash_component(&mut value_hasher);
    entries.push(HashEntry::new(key_hasher, value_hasher));
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use simdnbt::borrow::read_tag;
    use simdnbt::{FromNbtTag, ToNbtTag};
    use steel_utils::serial::{ReadFrom as _, WriteTo as _};

    use crate::data_components::vanilla_components::{DAMAGE, ENCHANTMENTS, LORE};
    use crate::test_support::init_test_registry;

    use super::TooltipDisplay;

    #[test]
    fn hidden_components_preserve_first_insertion_order() {
        let display = TooltipDisplay::DEFAULT
            .with_hidden(LORE, true)
            .with_hidden(DAMAGE, true)
            .with_hidden(LORE, true);

        assert_eq!(display.hidden_components(), &[LORE.key, DAMAGE.key]);
        assert!(!display.shows(LORE));
        assert!(display.shows(ENCHANTMENTS));
    }

    #[test]
    fn codecs_round_trip_registered_component_types() {
        init_test_registry();

        let display = TooltipDisplay::DEFAULT
            .with_hidden(LORE, true)
            .with_hidden(DAMAGE, true);

        let mut network = Vec::new();
        display.write(&mut network).expect("display should encode");
        assert_eq!(
            TooltipDisplay::read(&mut Cursor::new(network.as_slice()))
                .expect("display should decode"),
            display
        );

        let tag = display.clone().to_nbt_tag();
        let mut bytes = Vec::new();
        tag.write(&mut bytes);
        let borrowed =
            read_tag(&mut Cursor::new(bytes.as_slice())).expect("display NBT should parse");
        assert_eq!(
            TooltipDisplay::from_nbt_tag(borrowed.as_tag()),
            Some(display)
        );
    }
}
