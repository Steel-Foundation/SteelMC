//! Vanilla `minecraft:blocks_attacks` item component.

use std::io::{Cursor, Error, Result, Write};

use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::codec::VarInt;
use steel_utils::hash::{ComponentHasher, HashComponent, HashEntry, sort_map_entries};
use steel_utils::nbt::NbtNumeric as _;
use steel_utils::serial::{ReadFrom, WriteTo};

use crate::RegistryHolderSet;
use crate::damage_type::DamageType;
use crate::sound_event::SoundEventHolder;

/// One directional damage-reduction rule.
#[derive(Debug, Clone)]
pub struct DamageReduction {
    horizontal_blocking_angle: f32,
    damage_types: Option<RegistryHolderSet<DamageType>>,
    base: f32,
    factor: f32,
}

impl DamageReduction {
    pub const DEFAULT_HORIZONTAL_BLOCKING_ANGLE: f32 = 90.0;

    pub fn new(
        horizontal_blocking_angle: f32,
        damage_types: Option<RegistryHolderSet<DamageType>>,
        base: f32,
        factor: f32,
    ) -> Result<Self> {
        if !is_positive_float(horizontal_blocking_angle) {
            return Err(Error::other("Horizontal blocking angle must be positive"));
        }
        Ok(Self {
            horizontal_blocking_angle,
            damage_types,
            base,
            factor,
        })
    }

    #[must_use]
    pub const fn default_rule() -> Self {
        Self {
            horizontal_blocking_angle: Self::DEFAULT_HORIZONTAL_BLOCKING_ANGLE,
            damage_types: None,
            base: 0.0,
            factor: 1.0,
        }
    }

    #[must_use]
    pub const fn horizontal_blocking_angle(&self) -> f32 {
        self.horizontal_blocking_angle
    }

    #[must_use]
    pub const fn damage_types(&self) -> Option<&RegistryHolderSet<DamageType>> {
        self.damage_types.as_ref()
    }

    #[must_use]
    pub const fn base(&self) -> f32 {
        self.base
    }

    #[must_use]
    pub const fn factor(&self) -> f32 {
        self.factor
    }

    fn to_nbt_compound(&self) -> NbtCompound {
        let mut compound = NbtCompound::new();
        if !float_equals(
            self.horizontal_blocking_angle,
            Self::DEFAULT_HORIZONTAL_BLOCKING_ANGLE,
        ) {
            compound.insert("horizontal_blocking_angle", self.horizontal_blocking_angle);
        }
        if let Some(damage_types) = &self.damage_types {
            compound.insert("type", damage_types.clone().to_nbt_tag());
        }
        compound.insert("base", self.base);
        compound.insert("factor", self.factor);
        compound
    }

    fn from_owned_nbt(tag: &NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        let angle = optional_f32(
            compound.get("horizontal_blocking_angle"),
            Self::DEFAULT_HORIZONTAL_BLOCKING_ANGLE,
        )?;
        let damage_types = match compound.get("type") {
            Some(tag) => Some(RegistryHolderSet::from_owned_nbt(tag)?),
            None => None,
        };
        Self::new(
            angle,
            damage_types,
            compound.get("base")?.codec_f32()?,
            compound.get("factor")?.codec_f32()?,
        )
        .ok()
    }
}

impl PartialEq for DamageReduction {
    fn eq(&self, other: &Self) -> bool {
        float_equals(
            self.horizontal_blocking_angle,
            other.horizontal_blocking_angle,
        ) && self.damage_types == other.damage_types
            && float_equals(self.base, other.base)
            && float_equals(self.factor, other.factor)
    }
}

impl WriteTo for DamageReduction {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        self.horizontal_blocking_angle.write(writer)?;
        self.damage_types.is_some().write(writer)?;
        if let Some(damage_types) = &self.damage_types {
            damage_types.write(writer)?;
        }
        self.base.write(writer)?;
        self.factor.write(writer)
    }
}

impl ReadFrom for DamageReduction {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let angle = f32::read(data)?;
        let damage_types = if bool::read(data)? {
            Some(RegistryHolderSet::read(data)?)
        } else {
            None
        };
        Self::new(angle, damage_types, f32::read(data)?, f32::read(data)?)
    }
}

impl HashComponent for DamageReduction {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::with_capacity(4);
        if !float_equals(
            self.horizontal_blocking_angle,
            Self::DEFAULT_HORIZONTAL_BLOCKING_ANGLE,
        ) {
            push_hash_entry(
                &mut entries,
                "horizontal_blocking_angle",
                &self.horizontal_blocking_angle,
            );
        }
        if let Some(damage_types) = &self.damage_types {
            push_hash_entry(&mut entries, "type", damage_types);
        }
        push_hash_entry(&mut entries, "base", &self.base);
        push_hash_entry(&mut entries, "factor", &self.factor);
        hash_entries(hasher, &mut entries);
    }
}

/// Durability damage applied after blocking an attack.
#[derive(Debug, Clone, Copy)]
pub struct ItemDamageFunction {
    threshold: f32,
    base: f32,
    factor: f32,
}

impl ItemDamageFunction {
    pub const DEFAULT: Self = Self {
        threshold: 1.0,
        base: 0.0,
        factor: 1.0,
    };

    pub fn new(threshold: f32, base: f32, factor: f32) -> Result<Self> {
        if !is_non_negative_float(threshold) {
            return Err(Error::other(
                "Block item-damage threshold must be non-negative",
            ));
        }
        Ok(Self {
            threshold,
            base,
            factor,
        })
    }

    pub(crate) const fn from_extracted(threshold: f32, base: f32, factor: f32) -> Self {
        assert!(
            is_non_negative_float(threshold),
            "extracted block item-damage threshold must be non-negative"
        );
        Self {
            threshold,
            base,
            factor,
        }
    }

    #[must_use]
    pub const fn threshold(self) -> f32 {
        self.threshold
    }

    #[must_use]
    pub const fn base(self) -> f32 {
        self.base
    }

    #[must_use]
    pub const fn factor(self) -> f32 {
        self.factor
    }

    fn to_nbt_compound(self) -> NbtCompound {
        let mut compound = NbtCompound::new();
        compound.insert("threshold", self.threshold);
        compound.insert("base", self.base);
        compound.insert("factor", self.factor);
        compound
    }

    fn from_owned_nbt(tag: &NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        Self::new(
            compound.get("threshold")?.codec_f32()?,
            compound.get("base")?.codec_f32()?,
            compound.get("factor")?.codec_f32()?,
        )
        .ok()
    }
}

impl PartialEq for ItemDamageFunction {
    fn eq(&self, other: &Self) -> bool {
        float_equals(self.threshold, other.threshold)
            && float_equals(self.base, other.base)
            && float_equals(self.factor, other.factor)
    }
}

impl WriteTo for ItemDamageFunction {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        self.threshold.write(writer)?;
        self.base.write(writer)?;
        self.factor.write(writer)
    }
}

impl ReadFrom for ItemDamageFunction {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Self::new(f32::read(data)?, f32::read(data)?, f32::read(data)?)
    }
}

impl HashComponent for ItemDamageFunction {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::with_capacity(3);
        push_hash_entry(&mut entries, "threshold", &self.threshold);
        push_hash_entry(&mut entries, "base", &self.base);
        push_hash_entry(&mut entries, "factor", &self.factor);
        hash_entries(hasher, &mut entries);
    }
}

/// Complete blocking behavior configured on an item.
#[derive(Debug, Clone)]
pub struct BlocksAttacks {
    block_delay_seconds: f32,
    disable_cooldown_scale: f32,
    damage_reductions: Vec<DamageReduction>,
    item_damage: ItemDamageFunction,
    bypassed_by: Option<RegistryHolderSet<DamageType>>,
    block_sound: Option<SoundEventHolder>,
    disabled_sound: Option<SoundEventHolder>,
}

impl BlocksAttacks {
    pub fn new(
        block_delay_seconds: f32,
        disable_cooldown_scale: f32,
        damage_reductions: Vec<DamageReduction>,
        item_damage: ItemDamageFunction,
        bypassed_by: Option<RegistryHolderSet<DamageType>>,
        block_sound: Option<SoundEventHolder>,
        disabled_sound: Option<SoundEventHolder>,
    ) -> Result<Self> {
        if !is_non_negative_float(block_delay_seconds) {
            return Err(Error::other("Block delay must be non-negative"));
        }
        if !is_non_negative_float(disable_cooldown_scale) {
            return Err(Error::other("Disable cooldown scale must be non-negative"));
        }
        Ok(Self {
            block_delay_seconds,
            disable_cooldown_scale,
            damage_reductions,
            item_damage,
            bypassed_by,
            block_sound,
            disabled_sound,
        })
    }

    pub(crate) fn from_extracted_shield(
        block_delay_seconds: f32,
        item_damage: ItemDamageFunction,
        bypassed_by: RegistryHolderSet<DamageType>,
        block_sound: SoundEventHolder,
        disabled_sound: SoundEventHolder,
    ) -> Self {
        assert!(
            is_non_negative_float(block_delay_seconds),
            "extracted shield block delay must be non-negative"
        );
        Self {
            block_delay_seconds,
            disable_cooldown_scale: 1.0,
            damage_reductions: vec![DamageReduction::default_rule()],
            item_damage,
            bypassed_by: Some(bypassed_by),
            block_sound: Some(block_sound),
            disabled_sound: Some(disabled_sound),
        }
    }

    #[must_use]
    pub const fn block_delay_seconds(&self) -> f32 {
        self.block_delay_seconds
    }

    #[must_use]
    pub const fn disable_cooldown_scale(&self) -> f32 {
        self.disable_cooldown_scale
    }

    #[must_use]
    pub fn damage_reductions(&self) -> &[DamageReduction] {
        &self.damage_reductions
    }

    #[must_use]
    pub const fn item_damage(&self) -> ItemDamageFunction {
        self.item_damage
    }

    #[must_use]
    pub const fn bypassed_by(&self) -> Option<&RegistryHolderSet<DamageType>> {
        self.bypassed_by.as_ref()
    }

    #[must_use]
    pub const fn block_sound(&self) -> Option<&SoundEventHolder> {
        self.block_sound.as_ref()
    }

    #[must_use]
    pub const fn disabled_sound(&self) -> Option<&SoundEventHolder> {
        self.disabled_sound.as_ref()
    }

    fn to_nbt_tag_ref(&self) -> NbtTag {
        let mut compound = NbtCompound::new();
        if !float_equals(self.block_delay_seconds, 0.0) {
            compound.insert("block_delay_seconds", self.block_delay_seconds);
        }
        if !float_equals(self.disable_cooldown_scale, 1.0) {
            compound.insert("disable_cooldown_scale", self.disable_cooldown_scale);
        }
        if self.damage_reductions != [DamageReduction::default_rule()] {
            compound.insert(
                "damage_reductions",
                NbtList::Compound(
                    self.damage_reductions
                        .iter()
                        .map(DamageReduction::to_nbt_compound)
                        .collect(),
                ),
            );
        }
        if self.item_damage != ItemDamageFunction::DEFAULT {
            compound.insert(
                "item_damage",
                NbtTag::Compound(self.item_damage.to_nbt_compound()),
            );
        }
        if let Some(bypassed_by) = &self.bypassed_by {
            compound.insert("bypassed_by", bypassed_by.clone().to_nbt_tag());
        }
        if let Some(block_sound) = &self.block_sound {
            compound.insert("block_sound", block_sound.clone().to_nbt_tag());
        }
        if let Some(disabled_sound) = &self.disabled_sound {
            compound.insert("disabled_sound", disabled_sound.clone().to_nbt_tag());
        }
        NbtTag::Compound(compound)
    }

    fn from_owned_nbt(tag: &NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        let damage_reductions = match compound.get("damage_reductions") {
            Some(tag) => tag
                .list()?
                .as_nbt_tags()
                .iter()
                .map(DamageReduction::from_owned_nbt)
                .collect::<Option<Vec<_>>>()?,
            None => vec![DamageReduction::default_rule()],
        };
        let item_damage = match compound.get("item_damage") {
            Some(tag) => ItemDamageFunction::from_owned_nbt(tag)?,
            None => ItemDamageFunction::DEFAULT,
        };
        let bypassed_by = match compound.get("bypassed_by") {
            Some(tag) => Some(RegistryHolderSet::from_owned_nbt(tag)?),
            None => None,
        };
        let block_sound = match compound.get("block_sound") {
            Some(tag) => Some(SoundEventHolder::from_owned_nbt(tag)?),
            None => None,
        };
        let disabled_sound = match compound.get("disabled_sound") {
            Some(tag) => Some(SoundEventHolder::from_owned_nbt(tag)?),
            None => None,
        };
        Self::new(
            optional_f32(compound.get("block_delay_seconds"), 0.0)?,
            optional_f32(compound.get("disable_cooldown_scale"), 1.0)?,
            damage_reductions,
            item_damage,
            bypassed_by,
            block_sound,
            disabled_sound,
        )
        .ok()
    }
}

impl PartialEq for BlocksAttacks {
    fn eq(&self, other: &Self) -> bool {
        float_equals(self.block_delay_seconds, other.block_delay_seconds)
            && float_equals(self.disable_cooldown_scale, other.disable_cooldown_scale)
            && self.damage_reductions == other.damage_reductions
            && self.item_damage == other.item_damage
            && self.bypassed_by == other.bypassed_by
            && self.block_sound == other.block_sound
            && self.disabled_sound == other.disabled_sound
    }
}

impl WriteTo for BlocksAttacks {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        self.block_delay_seconds.write(writer)?;
        self.disable_cooldown_scale.write(writer)?;
        write_count(self.damage_reductions.len(), writer)?;
        for reduction in &self.damage_reductions {
            reduction.write(writer)?;
        }
        self.item_damage.write(writer)?;
        self.bypassed_by.is_some().write(writer)?;
        if let Some(bypassed_by) = &self.bypassed_by {
            bypassed_by.write(writer)?;
        }
        self.block_sound.is_some().write(writer)?;
        if let Some(block_sound) = &self.block_sound {
            block_sound.write(writer)?;
        }
        self.disabled_sound.is_some().write(writer)?;
        if let Some(disabled_sound) = &self.disabled_sound {
            disabled_sound.write(writer)?;
        }
        Ok(())
    }
}

impl ReadFrom for BlocksAttacks {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let block_delay_seconds = f32::read(data)?;
        let disable_cooldown_scale = f32::read(data)?;
        let count = read_count(data)?;
        let mut damage_reductions = Vec::with_capacity(count.min(65_536));
        for _ in 0..count {
            damage_reductions.push(DamageReduction::read(data)?);
        }
        let item_damage = ItemDamageFunction::read(data)?;
        let bypassed_by = if bool::read(data)? {
            Some(RegistryHolderSet::read(data)?)
        } else {
            None
        };
        let block_sound = if bool::read(data)? {
            Some(SoundEventHolder::read(data)?)
        } else {
            None
        };
        let disabled_sound = if bool::read(data)? {
            Some(SoundEventHolder::read(data)?)
        } else {
            None
        };
        Self::new(
            block_delay_seconds,
            disable_cooldown_scale,
            damage_reductions,
            item_damage,
            bypassed_by,
            block_sound,
            disabled_sound,
        )
    }
}

impl ToNbtTag for BlocksAttacks {
    fn to_nbt_tag(self) -> NbtTag {
        self.to_nbt_tag_ref()
    }
}

impl FromNbtTag for BlocksAttacks {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        Self::from_owned_nbt(&tag.to_owned())
    }
}

impl HashComponent for BlocksAttacks {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::with_capacity(7);
        if !float_equals(self.block_delay_seconds, 0.0) {
            push_hash_entry(
                &mut entries,
                "block_delay_seconds",
                &self.block_delay_seconds,
            );
        }
        if !float_equals(self.disable_cooldown_scale, 1.0) {
            push_hash_entry(
                &mut entries,
                "disable_cooldown_scale",
                &self.disable_cooldown_scale,
            );
        }
        if self.damage_reductions != [DamageReduction::default_rule()] {
            push_hash_entry(
                &mut entries,
                "damage_reductions",
                &DamageReductionList(&self.damage_reductions),
            );
        }
        if self.item_damage != ItemDamageFunction::DEFAULT {
            push_hash_entry(&mut entries, "item_damage", &self.item_damage);
        }
        if let Some(bypassed_by) = &self.bypassed_by {
            push_hash_entry(&mut entries, "bypassed_by", bypassed_by);
        }
        if let Some(block_sound) = &self.block_sound {
            push_hash_entry(&mut entries, "block_sound", block_sound);
        }
        if let Some(disabled_sound) = &self.disabled_sound {
            push_hash_entry(&mut entries, "disabled_sound", disabled_sound);
        }
        hash_entries(hasher, &mut entries);
    }
}

struct DamageReductionList<'a>(&'a [DamageReduction]);

impl HashComponent for DamageReductionList<'_> {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        hasher.start_list();
        for reduction in self.0 {
            hasher.put_component_hash(reduction);
        }
        hasher.end_list();
    }
}

const fn float_equals(left: f32, right: f32) -> bool {
    (left.is_nan() && right.is_nan()) || left.to_bits() == right.to_bits()
}

const fn is_non_negative_float(value: f32) -> bool {
    value.is_finite() && !value.is_sign_negative()
}

const fn is_positive_float(value: f32) -> bool {
    value > 0.0 && value <= f32::MAX
}

fn optional_f32(tag: Option<&NbtTag>, default: f32) -> Option<f32> {
    match tag {
        Some(tag) => tag.codec_f32(),
        None => Some(default),
    }
}

fn write_count(count: usize, writer: &mut impl Write) -> Result<()> {
    let count =
        i32::try_from(count).map_err(|_| Error::other("Damage reduction list too large"))?;
    VarInt(count).write(writer)
}

fn read_count(data: &mut Cursor<&[u8]>) -> Result<usize> {
    let count = VarInt::read(data)?.0;
    usize::try_from(count)
        .map_err(|_| Error::other(format!("Negative damage reduction count: {count}")))
}

fn push_hash_entry<T: HashComponent + ?Sized>(entries: &mut Vec<HashEntry>, key: &str, value: &T) {
    let mut key_hasher = ComponentHasher::new();
    key_hasher.put_string(key);
    let mut value_hasher = ComponentHasher::new();
    value.hash_component(&mut value_hasher);
    entries.push(HashEntry::new(key_hasher, value_hasher));
}

fn hash_entries(hasher: &mut ComponentHasher, entries: &mut [HashEntry]) {
    sort_map_entries(entries);
    hasher.start_map();
    for entry in entries {
        hasher.put_raw_bytes(&entry.key_bytes);
        hasher.put_raw_bytes(&entry.value_bytes);
    }
    hasher.end_map();
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use simdnbt::{FromNbtTag as _, ToNbtTag as _};
    use steel_utils::serial::{ReadFrom as _, WriteTo as _};

    use super::{BlocksAttacks, DamageReduction, ItemDamageFunction};
    use crate::data_components::vanilla_components::BLOCKS_ATTACKS;
    use crate::test_support::init_test_registry;
    use crate::{REGISTRY, RegistryExt};

    fn parse(tag: simdnbt::owned::NbtTag) -> Option<BlocksAttacks> {
        let mut bytes = Vec::new();
        tag.write(&mut bytes);
        let borrowed = simdnbt::borrow::read_tag(&mut Cursor::new(bytes.as_slice())).ok()?;
        BlocksAttacks::from_nbt_tag(borrowed.as_tag())
    }

    #[test]
    fn shield_component_round_trips_both_codecs() {
        init_test_registry();
        let shield = REGISTRY
            .items
            .by_key(&steel_utils::Identifier::vanilla_static("shield"))
            .expect("shield should be registered");
        let value = shield
            .components
            .get(BLOCKS_ATTACKS)
            .expect("shield should block attacks");
        assert_eq!(value.block_delay_seconds(), 0.25);
        assert_eq!(
            value.item_damage(),
            ItemDamageFunction::new(3.0, 1.0, 1.0).expect("valid item damage")
        );

        let nbt = value.clone().to_nbt_tag();
        assert_eq!(parse(nbt), Some(value.clone()));
        let mut network = Vec::new();
        value
            .write(&mut network)
            .expect("blocks_attacks should encode");
        assert_eq!(
            BlocksAttacks::read(&mut Cursor::new(network.as_slice()))
                .expect("blocks_attacks should decode"),
            value
        );
    }

    #[test]
    fn persistence_constraints_reject_unsavable_values() {
        assert!(ItemDamageFunction::new(-1.0, 0.0, 1.0).is_err());
        assert!(
            BlocksAttacks::new(
                -1.0,
                1.0,
                Vec::new(),
                ItemDamageFunction::DEFAULT,
                None,
                None,
                None,
            )
            .is_err()
        );

        for invalid in [-0.0, f32::INFINITY, f32::NEG_INFINITY, f32::NAN] {
            assert!(ItemDamageFunction::new(invalid, 0.0, 1.0).is_err());
            assert!(
                BlocksAttacks::new(
                    invalid,
                    1.0,
                    Vec::new(),
                    ItemDamageFunction::DEFAULT,
                    None,
                    None,
                    None,
                )
                .is_err()
            );
        }
        assert!(ItemDamageFunction::new(0.0, 0.0, 1.0).is_ok());

        for invalid in [0.0, -0.0, f32::INFINITY, f32::NEG_INFINITY, f32::NAN] {
            assert!(DamageReduction::new(invalid, None, 0.0, 1.0).is_err());
        }
        assert!(DamageReduction::new(f32::MAX, None, 0.0, 1.0).is_ok());
    }
}
