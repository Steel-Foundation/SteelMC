//! Vanilla `minecraft:potion_contents` item component.

use std::io::{Cursor, Error, Result, Write};

use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::codec::VarInt;
use steel_utils::hash::{ComponentHasher, HashComponent, HashEntry, sort_map_entries};
use steel_utils::nbt::NbtNumeric as _;
use steel_utils::serial::{PrefixedRead as _, PrefixedWrite as _, ReadFrom, WriteTo};

use crate::RegistryReference;
use crate::data_components::vanilla_components::POTION_CONTENTS;
use crate::item_stack::ItemStack;
use crate::items::ItemRef;
use crate::mob_effect_instance::MobEffectInstance;
use crate::potion::{Potion, PotionRef};

/// Color used when there is neither a custom color nor a visible effect.
const BASE_POTION_COLOR: i32 = -13_083_194;
/// Fully opaque alpha channel, matching vanilla `ARGB.color(r, g, b)`'s implicit
/// `ARGB.color(255, r, g, b)`.
const OPAQUE_ALPHA: i32 = 0xFF00_0000_u32 as i32;

/// A registered base potion plus optional custom display and effect data.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct PotionContents {
    potion: Option<RegistryReference<Potion>>,
    custom_color: Option<i32>,
    custom_effects: Vec<MobEffectInstance>,
    custom_name: Option<String>,
}

impl PotionContents {
    const MAX_NETWORK_STRING_LENGTH: usize = 32_767;

    #[must_use]
    pub const fn empty() -> Self {
        Self {
            potion: None,
            custom_color: None,
            custom_effects: Vec::new(),
            custom_name: None,
        }
    }

    #[must_use]
    pub const fn new(
        potion: Option<RegistryReference<Potion>>,
        custom_color: Option<i32>,
        custom_effects: Vec<MobEffectInstance>,
        custom_name: Option<String>,
    ) -> Self {
        Self {
            potion,
            custom_color,
            custom_effects,
            custom_name,
        }
    }

    #[must_use]
    pub const fn of(potion: PotionRef) -> Self {
        Self::new(Some(RegistryReference::new(potion)), None, Vec::new(), None)
    }

    #[must_use]
    pub fn create_item_stack(item: ItemRef, potion: PotionRef) -> ItemStack {
        let mut stack = ItemStack::new(item);
        stack.set(POTION_CONTENTS, Self::of(potion));
        stack
    }

    #[must_use]
    pub const fn potion(&self) -> Option<RegistryReference<Potion>> {
        self.potion
    }

    #[must_use]
    pub const fn custom_color(&self) -> Option<i32> {
        self.custom_color
    }

    #[must_use]
    pub fn custom_effects(&self) -> &[MobEffectInstance] {
        &self.custom_effects
    }

    #[must_use]
    pub fn custom_name(&self) -> Option<&str> {
        self.custom_name.as_deref()
    }

    #[must_use]
    pub fn is(&self, potion: &Potion) -> bool {
        self.potion.is_some_and(|p| p.value().key == potion.key) && self.custom_effects.is_empty()
    }

    /// Returns vanilla `PotionContents.getAllEffects()`: the base potion's
    /// effects followed by the custom effects.
    #[must_use]
    pub fn all_effects(&self) -> Vec<MobEffectInstance> {
        let mut effects = Vec::with_capacity(self.custom_effects.len() + 1);
        if let Some(potion) = self.potion {
            effects.extend(potion.value().effects.iter().map(|effect| {
                MobEffectInstance::simple(effect.effect, effect.duration, effect.amplifier)
            }));
        }
        effects.extend(self.custom_effects.iter().cloned());
        effects
    }

    /// Whether the base potion or the custom effects contain any effect.
    #[must_use]
    pub fn has_effects(&self) -> bool {
        !self.custom_effects.is_empty()
            || self
                .potion
                .is_some_and(|potion| !potion.value().effects.is_empty())
    }

    /// Returns the display color, falling back to `BASE_POTION_COLOR`.
    #[must_use]
    pub fn get_color(&self) -> i32 {
        self.get_color_or(BASE_POTION_COLOR)
    }

    /// Returns the custom color, else the color blended from the effects,
    /// else `default_color`.
    #[must_use]
    pub fn get_color_or(&self, default_color: i32) -> i32 {
        self.custom_color
            .or_else(|| Self::color_from_effects(&self.all_effects()))
            .unwrap_or(default_color)
    }

    /// Returns vanilla `PotionContents.getColorOptional`: the amplifier-weighted
    /// average of every visible effect's color, or `None` when no effect is visible.
    fn color_from_effects(effects: &[MobEffectInstance]) -> Option<i32> {
        let mut red: i64 = 0;
        let mut green: i64 = 0;
        let mut blue: i64 = 0;
        let mut total_weight: i64 = 0;

        for effect in effects {
            if !effect.show_particles() {
                continue;
            }
            let color = effect.effect().color;
            let weight = i64::from(effect.amplifier() + 1);
            red += weight * i64::from(color.red());
            green += weight * i64::from(color.green());
            blue += weight * i64::from(color.blue());
            total_weight += weight;
        }

        if total_weight == 0 {
            None
        } else {
            let r = (red / total_weight) as i32;
            let g = (green / total_weight) as i32;
            let b = (blue / total_weight) as i32;
            Some(OPAQUE_ALPHA | (r << 16) | (g << 8) | b)
        }
    }

    fn to_nbt_tag_ref(&self) -> NbtTag {
        let mut compound = NbtCompound::new();
        if let Some(potion) = self.potion {
            compound.insert("potion", potion.to_nbt_tag());
        }
        if let Some(custom_color) = self.custom_color {
            compound.insert("custom_color", custom_color);
        }
        if !self.custom_effects.is_empty() {
            compound.insert(
                "custom_effects",
                NbtList::Compound(
                    self.custom_effects
                        .iter()
                        .map(|effect| match effect.to_nbt_tag_ref() {
                            NbtTag::Compound(compound) => compound,
                            _ => unreachable!("mob effect codec always produces a compound"),
                        })
                        .collect(),
                ),
            );
        }
        if let Some(custom_name) = &self.custom_name {
            compound.insert("custom_name", custom_name.clone());
        }
        NbtTag::Compound(compound)
    }

    fn from_owned_nbt(tag: &NbtTag) -> Option<Self> {
        if tag.string().is_some() {
            return registry_reference_from_owned_nbt(tag)
                .map(|potion| Self::new(Some(potion), None, Vec::new(), None));
        }

        let compound = tag.compound()?;
        let potion = match compound.get("potion") {
            Some(tag) => Some(registry_reference_from_owned_nbt(tag)?),
            None => None,
        };
        let custom_color = match compound.get("custom_color") {
            Some(tag) => Some(tag.codec_i32()?),
            None => None,
        };
        let custom_effects = match compound.get("custom_effects") {
            Some(tag) => tag
                .list()?
                .as_nbt_tags()
                .iter()
                .map(MobEffectInstance::from_owned_nbt)
                .collect::<Option<Vec<_>>>()?,
            None => Vec::new(),
        };
        let custom_name = match compound.get("custom_name") {
            Some(tag) => Some(tag.string()?.to_string()),
            None => None,
        };
        Some(Self::new(potion, custom_color, custom_effects, custom_name))
    }
}

impl WriteTo for PotionContents {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        self.potion.is_some().write(writer)?;
        if let Some(potion) = self.potion {
            potion.write(writer)?;
        }
        self.custom_color.is_some().write(writer)?;
        if let Some(custom_color) = self.custom_color {
            custom_color.write(writer)?;
        }
        write_count(self.custom_effects.len(), writer)?;
        for effect in &self.custom_effects {
            effect.write(writer)?;
        }
        self.custom_name.is_some().write(writer)?;
        if let Some(custom_name) = &self.custom_name {
            write_network_string(custom_name, writer)?;
        }
        Ok(())
    }
}

impl ReadFrom for PotionContents {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let potion = if bool::read(data)? {
            Some(RegistryReference::read(data)?)
        } else {
            None
        };
        let custom_color = if bool::read(data)? {
            Some(i32::read(data)?)
        } else {
            None
        };
        let count = read_count(data)?;
        let mut custom_effects = Vec::with_capacity(count.min(65_536));
        for _ in 0..count {
            custom_effects.push(MobEffectInstance::read(data)?);
        }
        let custom_name = if bool::read(data)? {
            Some(read_network_string(data)?)
        } else {
            None
        };
        Ok(Self::new(potion, custom_color, custom_effects, custom_name))
    }
}

impl ToNbtTag for PotionContents {
    fn to_nbt_tag(self) -> NbtTag {
        self.to_nbt_tag_ref()
    }
}

impl FromNbtTag for PotionContents {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        Self::from_owned_nbt(&tag.to_owned())
    }
}

impl HashComponent for PotionContents {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::with_capacity(4);
        if let Some(potion) = &self.potion {
            push_hash_entry(&mut entries, "potion", potion);
        }
        if let Some(custom_color) = self.custom_color {
            push_hash_entry(&mut entries, "custom_color", &custom_color);
        }
        if !self.custom_effects.is_empty() {
            push_hash_entry(
                &mut entries,
                "custom_effects",
                &MobEffectList(&self.custom_effects),
            );
        }
        if let Some(custom_name) = &self.custom_name {
            push_hash_entry(&mut entries, "custom_name", custom_name);
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

struct MobEffectList<'a>(&'a [MobEffectInstance]);

impl HashComponent for MobEffectList<'_> {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        hasher.start_list();
        for effect in self.0 {
            hasher.put_component_hash(effect);
        }
        hasher.end_list();
    }
}

fn registry_reference_from_owned_nbt(tag: &NbtTag) -> Option<RegistryReference<Potion>> {
    let key = tag.string()?.to_string().parse().ok()?;
    <Potion as crate::RegistryReferenceEntry>::reference_by_key(&key).map(RegistryReference::new)
}

fn write_count(count: usize, writer: &mut impl Write) -> Result<()> {
    let count = i32::try_from(count).map_err(|_| Error::other("Effect list is too large"))?;
    VarInt(count).write(writer)
}

fn read_count(data: &mut Cursor<&[u8]>) -> Result<usize> {
    let count = VarInt::read(data)?.0;
    usize::try_from(count).map_err(|_| Error::other(format!("Negative effect count: {count}")))
}

fn write_network_string(value: &str, writer: &mut impl Write) -> Result<()> {
    if value.encode_utf16().count() > PotionContents::MAX_NETWORK_STRING_LENGTH
        || value.len() > PotionContents::MAX_NETWORK_STRING_LENGTH * 3
    {
        return Err(Error::other("Potion custom name exceeds the network limit"));
    }
    value.write_prefixed::<VarInt>(writer)
}

fn read_network_string(data: &mut Cursor<&[u8]>) -> Result<String> {
    let value =
        String::read_prefixed_bound::<VarInt>(data, PotionContents::MAX_NETWORK_STRING_LENGTH * 3)?;
    if value.encode_utf16().count() > PotionContents::MAX_NETWORK_STRING_LENGTH {
        return Err(Error::other("Potion custom name exceeds the network limit"));
    }
    Ok(value)
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

    use simdnbt::owned::NbtTag;
    use simdnbt::{FromNbtTag as _, ToNbtTag as _};
    use steel_utils::hash::HashComponent as _;
    use steel_utils::serial::{ReadFrom as _, WriteTo as _};

    use super::PotionContents;
    use crate::data_components::vanilla_components::POTION_CONTENTS;
    use crate::init_vanilla_registry;
    use crate::{REGISTRY, RegistryExt, RegistryReference, vanilla_mob_effects, vanilla_potions};

    fn parse(tag: NbtTag) -> Option<PotionContents> {
        let mut bytes = Vec::new();
        tag.write(&mut bytes);
        let borrowed = simdnbt::borrow::read_tag(&mut Cursor::new(bytes.as_slice())).ok()?;
        PotionContents::from_nbt_tag(borrowed.as_tag())
    }

    #[test]
    fn full_and_alternative_potion_codecs_round_trip() {
        init_vanilla_registry();
        let value = PotionContents::new(
            Some(RegistryReference::new(&vanilla_potions::SWIFTNESS)),
            Some(0x12_34_56),
            vec![crate::MobEffectInstance::simple(
                vanilla_mob_effects::LUCK,
                200,
                1,
            )],
            Some("custom".to_owned()),
        );
        let nbt = value.clone().to_nbt_tag();
        assert_eq!(parse(nbt.clone()), Some(value.clone()));
        // MobEffectInstance contains Codec.BOOL fields while NbtOps represents
        // those values as bytes.
        assert_ne!(value.compute_hash(), nbt.compute_hash());

        let mut network = Vec::new();
        value
            .write(&mut network)
            .expect("potion contents should encode");
        assert_eq!(
            PotionContents::read(&mut Cursor::new(network.as_slice()))
                .expect("potion contents should decode"),
            value
        );

        assert_eq!(
            parse(NbtTag::String("minecraft:water".into())),
            Some(PotionContents::new(
                Some(RegistryReference::new(&vanilla_potions::WATER)),
                None,
                Vec::new(),
                None,
            ))
        );
    }

    #[test]
    fn all_effects_combines_base_potion_then_custom_effects() {
        init_vanilla_registry();
        let custom = crate::MobEffectInstance::simple(vanilla_mob_effects::LUCK, 200, 1);
        let contents = PotionContents::new(
            Some(RegistryReference::new(&vanilla_potions::POISON)),
            None,
            vec![custom.clone()],
            None,
        );

        let base_effects = vanilla_potions::POISON.effects;
        let expected: Vec<_> = base_effects
            .iter()
            .map(|effect| {
                crate::MobEffectInstance::simple(effect.effect, effect.duration, effect.amplifier)
            })
            .chain(std::iter::once(custom))
            .collect();

        assert_eq!(contents.all_effects(), expected);
    }

    #[test]
    fn all_effects_is_empty_without_a_base_potion_or_custom_effects() {
        init_vanilla_registry();
        assert_eq!(PotionContents::empty().all_effects(), Vec::new());
    }

    #[test]
    fn has_effects_checks_base_potion_and_custom_effects_independently() {
        init_vanilla_registry();
        assert!(!PotionContents::empty().has_effects());
        assert!(
            !PotionContents::new(
                Some(RegistryReference::new(&vanilla_potions::WATER)),
                None,
                Vec::new(),
                None
            )
            .has_effects()
        );
        assert!(
            PotionContents::new(
                Some(RegistryReference::new(&vanilla_potions::POISON)),
                None,
                Vec::new(),
                None
            )
            .has_effects()
        );
        assert!(
            PotionContents::new(
                None,
                None,
                vec![crate::MobEffectInstance::simple(
                    vanilla_mob_effects::LUCK,
                    200,
                    1
                )],
                None,
            )
            .has_effects()
        );
    }

    #[test]
    fn get_color_prefers_custom_color_over_effect_blend() {
        init_vanilla_registry();
        let contents = PotionContents::new(
            Some(RegistryReference::new(&vanilla_potions::POISON)),
            Some(0x00_ff_00),
            Vec::new(),
            None,
        );
        assert_eq!(contents.get_color(), 0x00_ff_00);
    }

    #[test]
    fn get_color_falls_back_to_default_without_visible_effects() {
        init_vanilla_registry();
        assert_eq!(
            PotionContents::empty().get_color(),
            super::BASE_POTION_COLOR
        );
        assert_eq!(PotionContents::empty().get_color_or(0x11_22_33), 0x11_22_33);
    }

    #[test]
    fn get_color_blends_visible_effect_colors_by_amplifier_weight() {
        init_vanilla_registry();

        // Poison's single effect is `RgbColor::new(0x87_A3_63)` at amplifier 0, so
        // the weighted average is that color verbatim, returned as opaque ARGB
        // exactly like vanilla's `ARGB.color(r, g, b)`.
        let poison = PotionContents::new(
            Some(RegistryReference::new(&vanilla_potions::POISON)),
            None,
            Vec::new(),
            None,
        );
        assert_eq!(poison.get_color(), 0xFF_87_A3_63_u32 as i32);

        // Two custom effects with different amplifiers blend weighted by
        // `amplifier + 1`: red at weight 1 and blue at weight 3 average to
        // `(255 / 4, 0, 3 * 255 / 4)` per channel.
        let blended = PotionContents::new(
            None,
            None,
            vec![
                crate::MobEffectInstance::simple(vanilla_mob_effects::LUCK, 200, 0),
                crate::MobEffectInstance::simple(vanilla_mob_effects::UNLUCK, 200, 2),
            ],
            None,
        );
        let luck = vanilla_mob_effects::LUCK.color;
        let unluck = vanilla_mob_effects::UNLUCK.color;
        let expected_channel = |luck_channel: u8, unluck_channel: u8| -> i32 {
            (i32::from(luck_channel) + 3 * i32::from(unluck_channel)) / 4
        };
        assert_eq!(
            blended.get_color(),
            0xFF00_0000_u32 as i32
                | (expected_channel(luck.red(), unluck.red()) << 16)
                | (expected_channel(luck.green(), unluck.green()) << 8)
                | expected_channel(luck.blue(), unluck.blue())
        );
    }

    #[test]
    fn extracted_potion_items_have_empty_contents() {
        init_vanilla_registry();
        for name in [
            "potion",
            "splash_potion",
            "tipped_arrow",
            "lingering_potion",
        ] {
            let item = REGISTRY
                .items
                .by_key(&steel_utils::Identifier::vanilla(name.to_owned()))
                .expect("potion item should be registered");
            assert_eq!(
                item.components.get(POTION_CONTENTS),
                Some(PotionContents::empty())
            );
        }
    }
}
