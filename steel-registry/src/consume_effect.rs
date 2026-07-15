//! Registry-dispatched effects applied after consuming or saving an item from death.

use std::fmt::{self, Debug, Formatter};
use std::io::{Cursor, Error, Result, Write};

use rustc_hash::FxHashMap;
use simdnbt::ToNbtTag as _;
use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use steel_utils::codec::VarInt;
use steel_utils::hash::{ComponentHasher, HashComponent, HashEntry, sort_map_entries};
use steel_utils::nbt::NbtNumeric as _;
use steel_utils::serial::{ReadFrom, WriteTo};
use steel_utils::{Downcast as _, DowncastType, DowncastTypeKey, ErasedType, Identifier};

use crate::mob_effect::MobEffect;
use crate::mob_effect_instance::MobEffectInstance;
use crate::sound_event::SoundEventHolder;
use crate::{REGISTRY, RegistryEntry, RegistryExt, RegistryHolderSet};

/// Concrete payload behavior required by a registered consume-effect type.
pub trait ConsumeEffectCodec:
    DowncastType + Clone + Debug + PartialEq + Send + Sync + 'static
{
    fn read_fields(compound: &NbtCompound) -> Option<Self>;
    fn write_fields(&self, compound: &mut NbtCompound);
    fn read_network(data: &mut Cursor<&[u8]>) -> Result<Self>;
    fn write_network(&self, writer: &mut Vec<u8>) -> Result<()>;
    fn hash_fields(&self, entries: &mut Vec<HashEntry>);
}

trait ErasedConsumeEffect: ErasedType + Debug + Send + Sync {
    fn clone_effect(&self) -> Box<dyn ErasedConsumeEffect>;
    fn effect_eq(&self, other: &dyn ErasedConsumeEffect) -> bool;
}

impl<T: ConsumeEffectCodec> ErasedConsumeEffect for T {
    fn clone_effect(&self) -> Box<dyn ErasedConsumeEffect> {
        Box::new(self.clone())
    }

    fn effect_eq(&self, other: &dyn ErasedConsumeEffect) -> bool {
        other.downcast_ref::<T>() == Some(self)
    }
}

type PersistentReader = fn(&NbtCompound) -> Option<Box<dyn ErasedConsumeEffect>>;
type PersistentWriter = fn(&dyn ErasedConsumeEffect, &mut NbtCompound);
type NetworkReader = fn(&mut Cursor<&[u8]>) -> Result<Box<dyn ErasedConsumeEffect>>;
type NetworkWriter = fn(&dyn ErasedConsumeEffect, &mut Vec<u8>) -> Result<()>;
type FieldsHasher = fn(&dyn ErasedConsumeEffect, &mut Vec<HashEntry>);

/// A registered consume-effect discriminator and its typed codecs.
pub struct ConsumeEffectType {
    pub key: Identifier,
    expected_type_key: DowncastTypeKey,
    persistent_reader: PersistentReader,
    persistent_writer: PersistentWriter,
    network_reader: NetworkReader,
    network_writer: NetworkWriter,
    fields_hasher: FieldsHasher,
}

impl ConsumeEffectType {
    #[must_use]
    pub const fn of<T: ConsumeEffectCodec>(key: Identifier) -> Self {
        Self {
            key,
            expected_type_key: T::TYPE_KEY,
            persistent_reader: read_persistent::<T>,
            persistent_writer: write_persistent::<T>,
            network_reader: read_network::<T>,
            network_writer: write_network::<T>,
            fields_hasher: hash_fields::<T>,
        }
    }
}

impl Debug for ConsumeEffectType {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ConsumeEffectType")
            .field("key", &self.key)
            .field("expected_type_key", &self.expected_type_key)
            .finish_non_exhaustive()
    }
}

pub type ConsumeEffectTypeRef = &'static ConsumeEffectType;

/// One type-erased but keyed consume-effect value.
pub struct ConsumeEffectData {
    effect_type: ConsumeEffectTypeRef,
    value: Box<dyn ErasedConsumeEffect>,
}

impl ConsumeEffectData {
    #[must_use]
    pub fn new<T: ConsumeEffectCodec>(effect_type: ConsumeEffectTypeRef, value: T) -> Self {
        assert_eq!(
            effect_type.expected_type_key,
            T::TYPE_KEY,
            "consume effect value does not match its registered type"
        );
        Self {
            effect_type,
            value: Box::new(value),
        }
    }

    #[must_use]
    pub const fn effect_type(&self) -> ConsumeEffectTypeRef {
        self.effect_type
    }

    #[must_use]
    pub fn downcast_ref<T: DowncastType>(&self) -> Option<&T> {
        self.value.downcast_ref::<T>()
    }

    pub(crate) fn to_nbt_tag_ref(&self) -> NbtTag {
        let mut compound = NbtCompound::new();
        compound.insert("type", self.effect_type.key.to_string());
        (self.effect_type.persistent_writer)(self.value.as_ref(), &mut compound);
        NbtTag::Compound(compound)
    }

    pub(crate) fn from_owned_nbt(tag: &NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        let key = compound.get("type")?.string()?.to_string().parse().ok()?;
        let effect_type = REGISTRY.consume_effect_types.by_key(&key)?;
        let value = (effect_type.persistent_reader)(compound)?;
        Some(Self { effect_type, value })
    }
}

impl Clone for ConsumeEffectData {
    fn clone(&self) -> Self {
        Self {
            effect_type: self.effect_type,
            value: self.value.clone_effect(),
        }
    }
}

impl Debug for ConsumeEffectData {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ConsumeEffectData")
            .field("effect_type", &self.effect_type.key)
            .field("value", &self.value)
            .finish()
    }
}

impl PartialEq for ConsumeEffectData {
    fn eq(&self, other: &Self) -> bool {
        self.effect_type.key == other.effect_type.key && self.value.effect_eq(other.value.as_ref())
    }
}

impl WriteTo for ConsumeEffectData {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        let id = self.effect_type.try_id().ok_or_else(|| {
            Error::other(format!(
                "Unknown consume effect type: {}",
                self.effect_type.key
            ))
        })?;
        let id = i32::try_from(id)
            .map_err(|_| Error::other(format!("Consume effect type id out of range: {id}")))?;
        VarInt(id).write(writer)?;
        let mut payload = Vec::new();
        (self.effect_type.network_writer)(self.value.as_ref(), &mut payload)?;
        writer.write_all(&payload)
    }
}

impl ReadFrom for ConsumeEffectData {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let id = VarInt::read(data)?.0;
        let id = usize::try_from(id)
            .map_err(|_| Error::other(format!("Negative consume effect type id: {id}")))?;
        let effect_type = REGISTRY
            .consume_effect_types
            .by_id(id)
            .ok_or_else(|| Error::other(format!("Unknown consume effect type id: {id}")))?;
        let value = (effect_type.network_reader)(data)?;
        Ok(Self { effect_type, value })
    }
}

impl HashComponent for ConsumeEffectData {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::new();
        push_hash_entry(&mut entries, "type", &self.effect_type.key);
        (self.effect_type.fields_hasher)(self.value.as_ref(), &mut entries);
        hash_entries(hasher, &mut entries);
    }
}

pub struct ConsumeEffectTypeRegistry {
    types_by_id: Vec<ConsumeEffectTypeRef>,
    types_by_key: FxHashMap<Identifier, usize>,
    allows_registering: bool,
}

impl ConsumeEffectTypeRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            types_by_id: Vec::new(),
            types_by_key: FxHashMap::default(),
            allows_registering: true,
        }
    }
}

crate::impl_standard_methods!(
    ConsumeEffectTypeRegistry,
    ConsumeEffectTypeRef,
    types_by_id,
    types_by_key,
    allows_registering
);
crate::impl_registry!(
    ConsumeEffectTypeRegistry,
    ConsumeEffectType,
    types_by_id,
    types_by_key,
    consume_effect_types
);

#[derive(Debug, Clone)]
pub struct ApplyStatusEffectsConsumeEffect {
    effects: Vec<MobEffectInstance>,
    probability: f32,
}

impl ApplyStatusEffectsConsumeEffect {
    pub fn new(effects: Vec<MobEffectInstance>, probability: f32) -> Result<Self> {
        if !is_float_in_unit_range(probability) {
            return Err(Error::other("Consume-effect probability must be in 0..=1"));
        }
        Ok(Self {
            effects,
            probability,
        })
    }

    pub(crate) const fn from_extracted(effects: Vec<MobEffectInstance>, probability: f32) -> Self {
        assert!(
            is_float_in_unit_range(probability),
            "extracted consume-effect probability must be in 0..=1"
        );
        Self {
            effects,
            probability,
        }
    }

    #[must_use]
    pub fn effects(&self) -> &[MobEffectInstance] {
        &self.effects
    }

    #[must_use]
    pub const fn probability(&self) -> f32 {
        self.probability
    }
}

impl PartialEq for ApplyStatusEffectsConsumeEffect {
    fn eq(&self, other: &Self) -> bool {
        self.effects == other.effects && float_equals(self.probability, other.probability)
    }
}

// SAFETY: This Steel-owned key uniquely identifies the concrete effect payload.
unsafe impl DowncastType for ApplyStatusEffectsConsumeEffect {
    const TYPE_KEY: DowncastTypeKey =
        DowncastTypeKey::new("steel:consume_effect/apply_status_effects");
}

impl ConsumeEffectCodec for ApplyStatusEffectsConsumeEffect {
    fn read_fields(compound: &NbtCompound) -> Option<Self> {
        let effects = compound
            .get("effects")?
            .list()?
            .as_nbt_tags()
            .iter()
            .map(MobEffectInstance::from_owned_nbt)
            .collect::<Option<Vec<_>>>()?;
        Self::new(effects, optional_f32(compound.get("probability"), 1.0)?).ok()
    }

    fn write_fields(&self, compound: &mut NbtCompound) {
        compound.insert(
            "effects",
            NbtList::Compound(
                self.effects
                    .iter()
                    .map(|effect| match effect.to_nbt_tag_ref() {
                        NbtTag::Compound(compound) => compound,
                        _ => unreachable!("mob effect codec always produces a compound"),
                    })
                    .collect(),
            ),
        );
        if !float_equals(self.probability, 1.0) {
            compound.insert("probability", self.probability);
        }
    }

    fn read_network(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let count = read_count(data, "mob effect")?;
        let mut effects = Vec::with_capacity(count.min(65_536));
        for _ in 0..count {
            effects.push(MobEffectInstance::read(data)?);
        }
        Self::new(effects, f32::read(data)?)
    }

    fn write_network(&self, writer: &mut Vec<u8>) -> Result<()> {
        write_count(self.effects.len(), writer, "mob effect")?;
        for effect in &self.effects {
            effect.write(writer)?;
        }
        self.probability.write(writer)
    }

    fn hash_fields(&self, entries: &mut Vec<HashEntry>) {
        push_hash_entry(entries, "effects", &MobEffectList(&self.effects));
        if !float_equals(self.probability, 1.0) {
            push_hash_entry(entries, "probability", &self.probability);
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RemoveStatusEffectsConsumeEffect {
    effects: RegistryHolderSet<MobEffect>,
}

impl RemoveStatusEffectsConsumeEffect {
    #[must_use]
    pub const fn new(effects: RegistryHolderSet<MobEffect>) -> Self {
        Self { effects }
    }

    #[must_use]
    pub const fn effects(&self) -> &RegistryHolderSet<MobEffect> {
        &self.effects
    }
}

// SAFETY: This Steel-owned key uniquely identifies the concrete effect payload.
unsafe impl DowncastType for RemoveStatusEffectsConsumeEffect {
    const TYPE_KEY: DowncastTypeKey =
        DowncastTypeKey::new("steel:consume_effect/remove_status_effects");
}

impl ConsumeEffectCodec for RemoveStatusEffectsConsumeEffect {
    fn read_fields(compound: &NbtCompound) -> Option<Self> {
        Some(Self::new(RegistryHolderSet::from_owned_nbt(
            compound.get("effects")?,
        )?))
    }

    fn write_fields(&self, compound: &mut NbtCompound) {
        compound.insert("effects", self.effects.clone().to_nbt_tag());
    }

    fn read_network(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self::new(RegistryHolderSet::read(data)?))
    }

    fn write_network(&self, writer: &mut Vec<u8>) -> Result<()> {
        self.effects.write(writer)
    }

    fn hash_fields(&self, entries: &mut Vec<HashEntry>) {
        push_hash_entry(entries, "effects", &self.effects);
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ClearAllStatusEffectsConsumeEffect;

// SAFETY: This Steel-owned key uniquely identifies the concrete effect payload.
unsafe impl DowncastType for ClearAllStatusEffectsConsumeEffect {
    const TYPE_KEY: DowncastTypeKey =
        DowncastTypeKey::new("steel:consume_effect/clear_all_status_effects");
}

impl ConsumeEffectCodec for ClearAllStatusEffectsConsumeEffect {
    fn read_fields(_compound: &NbtCompound) -> Option<Self> {
        Some(Self)
    }

    fn write_fields(&self, _compound: &mut NbtCompound) {}

    fn read_network(_data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self)
    }

    fn write_network(&self, _writer: &mut Vec<u8>) -> Result<()> {
        Ok(())
    }

    fn hash_fields(&self, _entries: &mut Vec<HashEntry>) {}
}

#[derive(Debug, Clone, Copy)]
pub struct TeleportRandomlyConsumeEffect {
    diameter: f32,
}

impl TeleportRandomlyConsumeEffect {
    pub const DEFAULT_DIAMETER: f32 = 16.0;

    pub fn new(diameter: f32) -> Result<Self> {
        if !is_positive_float(diameter) {
            return Err(Error::other("Random teleport diameter must be positive"));
        }
        Ok(Self { diameter })
    }

    pub(crate) const fn from_extracted(diameter: f32) -> Self {
        assert!(
            is_positive_float(diameter),
            "extracted random teleport diameter must be positive"
        );
        Self { diameter }
    }

    #[must_use]
    pub const fn default_value() -> Self {
        Self {
            diameter: Self::DEFAULT_DIAMETER,
        }
    }

    #[must_use]
    pub const fn diameter(self) -> f32 {
        self.diameter
    }
}

impl PartialEq for TeleportRandomlyConsumeEffect {
    fn eq(&self, other: &Self) -> bool {
        float_equals(self.diameter, other.diameter)
    }
}

// SAFETY: This Steel-owned key uniquely identifies the concrete effect payload.
unsafe impl DowncastType for TeleportRandomlyConsumeEffect {
    const TYPE_KEY: DowncastTypeKey =
        DowncastTypeKey::new("steel:consume_effect/teleport_randomly");
}

impl ConsumeEffectCodec for TeleportRandomlyConsumeEffect {
    fn read_fields(compound: &NbtCompound) -> Option<Self> {
        Self::new(optional_f32(
            compound.get("diameter"),
            Self::DEFAULT_DIAMETER,
        )?)
        .ok()
    }

    fn write_fields(&self, compound: &mut NbtCompound) {
        if !float_equals(self.diameter, Self::DEFAULT_DIAMETER) {
            compound.insert("diameter", self.diameter);
        }
    }

    fn read_network(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Self::new(f32::read(data)?)
    }

    fn write_network(&self, writer: &mut Vec<u8>) -> Result<()> {
        self.diameter.write(writer)
    }

    fn hash_fields(&self, entries: &mut Vec<HashEntry>) {
        if !float_equals(self.diameter, Self::DEFAULT_DIAMETER) {
            push_hash_entry(entries, "diameter", &self.diameter);
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlaySoundConsumeEffect {
    sound: SoundEventHolder,
}

impl PlaySoundConsumeEffect {
    #[must_use]
    pub const fn new(sound: SoundEventHolder) -> Self {
        Self { sound }
    }

    #[must_use]
    pub const fn sound(&self) -> &SoundEventHolder {
        &self.sound
    }
}

// SAFETY: This Steel-owned key uniquely identifies the concrete effect payload.
unsafe impl DowncastType for PlaySoundConsumeEffect {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:consume_effect/play_sound");
}

impl ConsumeEffectCodec for PlaySoundConsumeEffect {
    fn read_fields(compound: &NbtCompound) -> Option<Self> {
        Some(Self::new(SoundEventHolder::from_owned_nbt(
            compound.get("sound")?,
        )?))
    }

    fn write_fields(&self, compound: &mut NbtCompound) {
        compound.insert("sound", self.sound.clone().to_nbt_tag());
    }

    fn read_network(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self::new(SoundEventHolder::read(data)?))
    }

    fn write_network(&self, writer: &mut Vec<u8>) -> Result<()> {
        self.sound.write(writer)
    }

    fn hash_fields(&self, entries: &mut Vec<HashEntry>) {
        push_hash_entry(entries, "sound", &self.sound);
    }
}

pub mod vanilla_consume_effect_types {
    use super::{
        ApplyStatusEffectsConsumeEffect, ClearAllStatusEffectsConsumeEffect, ConsumeEffectType,
        ConsumeEffectTypeRegistry, PlaySoundConsumeEffect, RemoveStatusEffectsConsumeEffect,
        TeleportRandomlyConsumeEffect,
    };
    use steel_utils::Identifier;

    pub static APPLY_EFFECTS: ConsumeEffectType =
        ConsumeEffectType::of::<ApplyStatusEffectsConsumeEffect>(Identifier::vanilla_static(
            "apply_effects",
        ));
    pub static REMOVE_EFFECTS: ConsumeEffectType =
        ConsumeEffectType::of::<RemoveStatusEffectsConsumeEffect>(Identifier::vanilla_static(
            "remove_effects",
        ));
    pub static CLEAR_ALL_EFFECTS: ConsumeEffectType =
        ConsumeEffectType::of::<ClearAllStatusEffectsConsumeEffect>(Identifier::vanilla_static(
            "clear_all_effects",
        ));
    pub static TELEPORT_RANDOMLY: ConsumeEffectType =
        ConsumeEffectType::of::<TeleportRandomlyConsumeEffect>(Identifier::vanilla_static(
            "teleport_randomly",
        ));
    pub static PLAY_SOUND: ConsumeEffectType =
        ConsumeEffectType::of::<PlaySoundConsumeEffect>(Identifier::vanilla_static("play_sound"));

    pub fn register_consume_effect_types(registry: &mut ConsumeEffectTypeRegistry) {
        registry.register(&APPLY_EFFECTS);
        registry.register(&REMOVE_EFFECTS);
        registry.register(&CLEAR_ALL_EFFECTS);
        registry.register(&TELEPORT_RANDOMLY);
        registry.register(&PLAY_SOUND);
    }
}

fn read_persistent<T: ConsumeEffectCodec>(
    compound: &NbtCompound,
) -> Option<Box<dyn ErasedConsumeEffect>> {
    T::read_fields(compound).map(|value| Box::new(value) as Box<dyn ErasedConsumeEffect>)
}

fn write_persistent<T: ConsumeEffectCodec>(
    value: &dyn ErasedConsumeEffect,
    compound: &mut NbtCompound,
) {
    let Some(value) = value.downcast_ref::<T>() else {
        panic!("registered consume effect payload type mismatch");
    };
    value.write_fields(compound);
}

fn read_network<T: ConsumeEffectCodec>(
    data: &mut Cursor<&[u8]>,
) -> Result<Box<dyn ErasedConsumeEffect>> {
    Ok(Box::new(T::read_network(data)?))
}

fn write_network<T: ConsumeEffectCodec>(
    value: &dyn ErasedConsumeEffect,
    writer: &mut Vec<u8>,
) -> Result<()> {
    value
        .downcast_ref::<T>()
        .ok_or_else(|| Error::other("Consume effect payload type mismatch"))?
        .write_network(writer)
}

fn hash_fields<T: ConsumeEffectCodec>(
    value: &dyn ErasedConsumeEffect,
    entries: &mut Vec<HashEntry>,
) {
    let Some(value) = value.downcast_ref::<T>() else {
        panic!("registered consume effect payload type mismatch");
    };
    value.hash_fields(entries);
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

const fn float_equals(left: f32, right: f32) -> bool {
    (left.is_nan() && right.is_nan()) || left.to_bits() == right.to_bits()
}

const fn is_positive_float(value: f32) -> bool {
    value > 0.0 && value <= f32::MAX
}

const fn is_float_in_unit_range(value: f32) -> bool {
    value.is_finite() && !value.is_sign_negative() && value <= 1.0
}

fn optional_f32(tag: Option<&NbtTag>, default: f32) -> Option<f32> {
    match tag {
        Some(tag) => tag.codec_f32(),
        None => Some(default),
    }
}

fn write_count(count: usize, writer: &mut Vec<u8>, name: &str) -> Result<()> {
    let count = i32::try_from(count).map_err(|_| Error::other(format!("{name} list too large")))?;
    VarInt(count).write(writer)
}

fn read_count(data: &mut Cursor<&[u8]>, name: &str) -> Result<usize> {
    let count = VarInt::read(data)?.0;
    usize::try_from(count).map_err(|_| Error::other(format!("Negative {name} count: {count}")))
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
