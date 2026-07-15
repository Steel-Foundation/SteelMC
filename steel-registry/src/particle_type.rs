use std::fmt::{self, Debug, Formatter};
use std::io::{Cursor, Error, Result, Write};

use glam::DVec3;
use rustc_hash::FxHashMap;
use steel_utils::codec::VarInt;
use steel_utils::serial::{ReadFrom, WriteTo};
use steel_utils::{
    ArgbColor, BlockStateId, Downcast as _, DowncastType, DowncastTypeKey, ErasedType, Identifier,
    RgbColor,
};

use crate::item_stack_template::ItemStackTemplate;
use crate::position_source::PositionSource;
use crate::{REGISTRY, RegistryExt};

/// Concrete network payload behavior for a registered particle type.
pub trait ParticleOptions:
    DowncastType + Clone + Debug + PartialEq + Send + Sync + 'static
{
    fn read_network(data: &mut Cursor<&[u8]>) -> Result<Self>;
    fn write_network(&self, writer: &mut Vec<u8>) -> Result<()>;
}

trait ErasedParticleOptions: ErasedType + Debug + Send + Sync {
    fn clone_options(&self) -> Box<dyn ErasedParticleOptions>;
    fn options_eq(&self, other: &dyn ErasedParticleOptions) -> bool;
}

impl<T: ParticleOptions> ErasedParticleOptions for T {
    fn clone_options(&self) -> Box<dyn ErasedParticleOptions> {
        Box::new(self.clone())
    }

    fn options_eq(&self, other: &dyn ErasedParticleOptions) -> bool {
        other.downcast_ref::<T>() == Some(self)
    }
}

type NetworkReader = fn(&mut Cursor<&[u8]>) -> Result<Box<dyn ErasedParticleOptions>>;
type NetworkWriter = fn(&dyn ErasedParticleOptions, &mut Vec<u8>) -> Result<()>;

/// A registered particle discriminator, limiter behavior, and payload codec.
pub struct ParticleType {
    pub key: Identifier,
    pub override_limiter: bool,
    expected_type_key: DowncastTypeKey,
    network_reader: NetworkReader,
    network_writer: NetworkWriter,
}

impl ParticleType {
    #[must_use]
    pub const fn of<T: ParticleOptions>(key: Identifier, override_limiter: bool) -> Self {
        Self {
            key,
            override_limiter,
            expected_type_key: T::TYPE_KEY,
            network_reader: read_network::<T>,
            network_writer: write_network::<T>,
        }
    }
}

impl Debug for ParticleType {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ParticleType")
            .field("key", &self.key)
            .field("override_limiter", &self.override_limiter)
            .field("expected_type_key", &self.expected_type_key)
            .finish_non_exhaustive()
    }
}

pub type ParticleTypeRef = &'static ParticleType;

/// One registry-dispatched particle value, including its concrete payload.
pub struct ParticleData {
    particle_type: ParticleTypeRef,
    options: Box<dyn ErasedParticleOptions>,
}

impl ParticleData {
    #[must_use]
    pub fn new<T: ParticleOptions>(particle_type: ParticleTypeRef, options: T) -> Self {
        assert_eq!(
            particle_type.expected_type_key,
            T::TYPE_KEY,
            "particle options do not match their registered type"
        );
        Self {
            particle_type,
            options: Box::new(options),
        }
    }

    #[must_use]
    pub fn simple(particle_type: ParticleTypeRef) -> Self {
        Self::new(particle_type, SimpleParticleOptions)
    }

    #[must_use]
    pub const fn particle_type(&self) -> ParticleTypeRef {
        self.particle_type
    }

    #[must_use]
    pub fn downcast_ref<T: DowncastType>(&self) -> Option<&T> {
        self.options.downcast_ref::<T>()
    }
}

impl Clone for ParticleData {
    fn clone(&self) -> Self {
        Self {
            particle_type: self.particle_type,
            options: self.options.clone_options(),
        }
    }
}

impl Debug for ParticleData {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ParticleData")
            .field("particle_type", &self.particle_type.key)
            .field("options", &self.options)
            .finish()
    }
}

impl PartialEq for ParticleData {
    fn eq(&self, other: &Self) -> bool {
        self.particle_type.key == other.particle_type.key
            && self.options.options_eq(other.options.as_ref())
    }
}

impl WriteTo for ParticleData {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        let (id, particle_type) = REGISTRY
            .particle_types
            .registered_entry_with_id(self.particle_type)
            .ok_or_else(|| {
                Error::other(format!(
                    "Particle type is not the registered value for key: {}",
                    self.particle_type.key
                ))
            })?;
        let id = i32::try_from(id)
            .map_err(|_| Error::other(format!("Particle type id out of range: {id}")))?;
        VarInt(id).write(writer)?;

        let mut payload = Vec::new();
        (particle_type.network_writer)(self.options.as_ref(), &mut payload)?;
        writer.write_all(&payload)
    }
}

impl ReadFrom for ParticleData {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let id = VarInt::read(data)?.0;
        let id = usize::try_from(id)
            .map_err(|_| Error::other(format!("Negative particle type id: {id}")))?;
        let particle_type = REGISTRY
            .particle_types
            .by_id(id)
            .ok_or_else(|| Error::other(format!("Unknown particle type id: {id}")))?;
        let options = (particle_type.network_reader)(data)?;
        Ok(Self {
            particle_type,
            options,
        })
    }
}

pub struct ParticleTypeRegistry {
    particle_types_by_id: Vec<ParticleTypeRef>,
    particle_types_by_key: FxHashMap<Identifier, usize>,
    allows_registering: bool,
}

impl ParticleTypeRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            particle_types_by_id: Vec::new(),
            particle_types_by_key: FxHashMap::default(),
            allows_registering: true,
        }
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "network dispatch requires exact registered particle type identity"
    )]
    fn registered_entry_with_id(&self, entry: ParticleTypeRef) -> Option<(usize, ParticleTypeRef)> {
        let id = self.particle_types_by_key.get(&entry.key).copied()?;
        let registered = self.particle_types_by_id.get(id).copied()?;
        std::ptr::eq(registered, entry).then_some((id, registered))
    }
}

crate::impl_standard_methods!(
    ParticleTypeRegistry,
    ParticleTypeRef,
    particle_types_by_id,
    particle_types_by_key,
    allows_registering,
    "Cannot register duplicate particle type key: {}"
);

crate::impl_registry!(
    ParticleTypeRegistry,
    ParticleType,
    particle_types_by_id,
    particle_types_by_key,
    particle_types
);

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SimpleParticleOptions;

// SAFETY: This Steel-owned key uniquely identifies the concrete particle payload.
unsafe impl DowncastType for SimpleParticleOptions {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:particle_options/simple");
}

impl ParticleOptions for SimpleParticleOptions {
    fn read_network(_data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self)
    }

    fn write_network(&self, _writer: &mut Vec<u8>) -> Result<()> {
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockParticleOption {
    state: BlockStateId,
}

impl BlockParticleOption {
    #[must_use]
    pub const fn new(state: BlockStateId) -> Self {
        Self { state }
    }

    #[must_use]
    pub const fn state(&self) -> BlockStateId {
        self.state
    }
}

// SAFETY: This Steel-owned key uniquely identifies the concrete particle payload.
unsafe impl DowncastType for BlockParticleOption {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:particle_options/block");
}

impl ParticleOptions for BlockParticleOption {
    fn read_network(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let id = VarInt::read(data)?.0;
        let id = u16::try_from(id)
            .map_err(|_| Error::other(format!("Block state id out of range: {id}")))?;
        let state = BlockStateId(id);
        if REGISTRY.blocks.by_state_id(state).is_none() {
            return Err(Error::other(format!("Unknown block state id: {id}")));
        }
        Ok(Self::new(state))
    }

    fn write_network(&self, writer: &mut Vec<u8>) -> Result<()> {
        if REGISTRY.blocks.by_state_id(self.state).is_none() {
            return Err(Error::other(format!(
                "Unknown block state id: {}",
                self.state.0
            )));
        }
        self.state.write(writer)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorParticleOption {
    color: ArgbColor,
}

impl ColorParticleOption {
    #[must_use]
    pub const fn new(color: ArgbColor) -> Self {
        Self { color }
    }

    #[must_use]
    pub const fn color(&self) -> ArgbColor {
        self.color
    }
}

// SAFETY: This Steel-owned key uniquely identifies the concrete particle payload.
unsafe impl DowncastType for ColorParticleOption {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:particle_options/color");
}

impl ParticleOptions for ColorParticleOption {
    fn read_network(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self::new(ArgbColor::read(data)?))
    }

    fn write_network(&self, writer: &mut Vec<u8>) -> Result<()> {
        self.color.write(writer)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DustParticleOptions {
    color: RgbColor,
    scale: f32,
}

impl DustParticleOptions {
    #[must_use]
    pub const fn new(color: RgbColor, scale: f32) -> Self {
        Self {
            color,
            scale: scale.clamp(0.01, 4.0),
        }
    }

    #[must_use]
    pub const fn color(&self) -> RgbColor {
        self.color
    }

    #[must_use]
    pub const fn scale(&self) -> f32 {
        self.scale
    }
}

// SAFETY: This Steel-owned key uniquely identifies the concrete particle payload.
unsafe impl DowncastType for DustParticleOptions {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:particle_options/dust");
}

impl ParticleOptions for DustParticleOptions {
    fn read_network(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self::new(RgbColor::read(data)?, f32::read(data)?))
    }

    fn write_network(&self, writer: &mut Vec<u8>) -> Result<()> {
        self.color.write(writer)?;
        self.scale.write(writer)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DustColorTransitionOptions {
    from_color: RgbColor,
    to_color: RgbColor,
    scale: f32,
}

impl DustColorTransitionOptions {
    #[must_use]
    pub const fn new(from_color: RgbColor, to_color: RgbColor, scale: f32) -> Self {
        Self {
            from_color,
            to_color,
            scale: scale.clamp(0.01, 4.0),
        }
    }

    #[must_use]
    pub const fn source_color(&self) -> RgbColor {
        self.from_color
    }

    #[must_use]
    pub const fn target_color(&self) -> RgbColor {
        self.to_color
    }

    #[must_use]
    pub const fn scale(&self) -> f32 {
        self.scale
    }
}

// SAFETY: This Steel-owned key uniquely identifies the concrete particle payload.
unsafe impl DowncastType for DustColorTransitionOptions {
    const TYPE_KEY: DowncastTypeKey =
        DowncastTypeKey::new("steel:particle_options/dust_color_transition");
}

impl ParticleOptions for DustColorTransitionOptions {
    fn read_network(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self::new(
            RgbColor::read(data)?,
            RgbColor::read(data)?,
            f32::read(data)?,
        ))
    }

    fn write_network(&self, writer: &mut Vec<u8>) -> Result<()> {
        self.from_color.write(writer)?;
        self.to_color.write(writer)?;
        self.scale.write(writer)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GeyserParticleOptions {
    water_blocks: i32,
}

impl GeyserParticleOptions {
    #[must_use]
    pub const fn new(water_blocks: i32) -> Self {
        Self { water_blocks }
    }

    #[must_use]
    pub const fn water_blocks(&self) -> i32 {
        self.water_blocks
    }
}

// SAFETY: This Steel-owned key uniquely identifies the concrete particle payload.
unsafe impl DowncastType for GeyserParticleOptions {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:particle_options/geyser");
}

impl ParticleOptions for GeyserParticleOptions {
    fn read_network(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self::new(i32::read(data)?))
    }

    fn write_network(&self, writer: &mut Vec<u8>) -> Result<()> {
        self.water_blocks.write(writer)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GeyserBaseParticleOptions {
    water_blocks: i32,
    burst_impulse_base: f32,
}

impl GeyserBaseParticleOptions {
    #[must_use]
    pub const fn new(water_blocks: i32, burst_impulse_base: f32) -> Self {
        Self {
            water_blocks,
            burst_impulse_base,
        }
    }

    #[must_use]
    pub const fn water_blocks(&self) -> i32 {
        self.water_blocks
    }

    #[must_use]
    pub const fn burst_impulse_base(&self) -> f32 {
        self.burst_impulse_base
    }
}

// SAFETY: This Steel-owned key uniquely identifies the concrete particle payload.
unsafe impl DowncastType for GeyserBaseParticleOptions {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:particle_options/geyser_base");
}

impl ParticleOptions for GeyserBaseParticleOptions {
    fn read_network(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self::new(i32::read(data)?, f32::read(data)?))
    }

    fn write_network(&self, writer: &mut Vec<u8>) -> Result<()> {
        self.water_blocks.write(writer)?;
        self.burst_impulse_base.write(writer)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PowerParticleOption {
    power: f32,
}

impl PowerParticleOption {
    #[must_use]
    pub const fn new(power: f32) -> Self {
        Self { power }
    }

    #[must_use]
    pub const fn power(&self) -> f32 {
        self.power
    }
}

// SAFETY: This Steel-owned key uniquely identifies the concrete particle payload.
unsafe impl DowncastType for PowerParticleOption {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:particle_options/power");
}

impl ParticleOptions for PowerParticleOption {
    fn read_network(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self::new(f32::read(data)?))
    }

    fn write_network(&self, writer: &mut Vec<u8>) -> Result<()> {
        self.power.write(writer)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpellParticleOption {
    color: RgbColor,
    power: f32,
}

impl SpellParticleOption {
    #[must_use]
    pub const fn new(color: RgbColor, power: f32) -> Self {
        Self { color, power }
    }

    #[must_use]
    pub const fn color(&self) -> RgbColor {
        self.color
    }

    #[must_use]
    pub const fn power(&self) -> f32 {
        self.power
    }
}

// SAFETY: This Steel-owned key uniquely identifies the concrete particle payload.
unsafe impl DowncastType for SpellParticleOption {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:particle_options/spell");
}

impl ParticleOptions for SpellParticleOption {
    fn read_network(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self::new(RgbColor::read(data)?, f32::read(data)?))
    }

    fn write_network(&self, writer: &mut Vec<u8>) -> Result<()> {
        self.color.write(writer)?;
        self.power.write(writer)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ItemParticleOption {
    item: ItemStackTemplate,
}

impl ItemParticleOption {
    #[must_use]
    pub const fn new(item: ItemStackTemplate) -> Self {
        Self { item }
    }

    #[must_use]
    pub const fn item(&self) -> &ItemStackTemplate {
        &self.item
    }
}

// SAFETY: This Steel-owned key uniquely identifies the concrete particle payload.
unsafe impl DowncastType for ItemParticleOption {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:particle_options/item");
}

impl ParticleOptions for ItemParticleOption {
    fn read_network(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self::new(ItemStackTemplate::read(data)?))
    }

    fn write_network(&self, writer: &mut Vec<u8>) -> Result<()> {
        self.item.write(writer)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SculkChargeParticleOptions {
    roll: f32,
}

impl SculkChargeParticleOptions {
    #[must_use]
    pub const fn new(roll: f32) -> Self {
        Self { roll }
    }

    #[must_use]
    pub const fn roll(&self) -> f32 {
        self.roll
    }
}

// SAFETY: This Steel-owned key uniquely identifies the concrete particle payload.
unsafe impl DowncastType for SculkChargeParticleOptions {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:particle_options/sculk_charge");
}

impl ParticleOptions for SculkChargeParticleOptions {
    fn read_network(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self::new(f32::read(data)?))
    }

    fn write_network(&self, writer: &mut Vec<u8>) -> Result<()> {
        self.roll.write(writer)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShriekParticleOption {
    delay: i32,
}

impl ShriekParticleOption {
    #[must_use]
    pub const fn new(delay: i32) -> Self {
        Self { delay }
    }

    #[must_use]
    pub const fn delay(&self) -> i32 {
        self.delay
    }
}

// SAFETY: This Steel-owned key uniquely identifies the concrete particle payload.
unsafe impl DowncastType for ShriekParticleOption {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:particle_options/shriek");
}

impl ParticleOptions for ShriekParticleOption {
    fn read_network(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self::new(VarInt::read(data)?.0))
    }

    fn write_network(&self, writer: &mut Vec<u8>) -> Result<()> {
        VarInt(self.delay).write(writer)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrailParticleOption {
    target: DVec3,
    color: RgbColor,
    duration: i32,
}

impl TrailParticleOption {
    #[must_use]
    pub const fn new(target: DVec3, color: RgbColor, duration: i32) -> Self {
        Self {
            target,
            color,
            duration,
        }
    }

    #[must_use]
    pub const fn target(&self) -> DVec3 {
        self.target
    }

    #[must_use]
    pub const fn color(&self) -> RgbColor {
        self.color
    }

    #[must_use]
    pub const fn duration(&self) -> i32 {
        self.duration
    }
}

// SAFETY: This Steel-owned key uniquely identifies the concrete particle payload.
unsafe impl DowncastType for TrailParticleOption {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:particle_options/trail");
}

impl ParticleOptions for TrailParticleOption {
    fn read_network(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self::new(
            DVec3::read(data)?,
            RgbColor::read(data)?,
            VarInt::read(data)?.0,
        ))
    }

    fn write_network(&self, writer: &mut Vec<u8>) -> Result<()> {
        self.target.write(writer)?;
        self.color.write(writer)?;
        VarInt(self.duration).write(writer)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct VibrationParticleOption {
    destination: PositionSource,
    arrival_in_ticks: i32,
}

impl VibrationParticleOption {
    #[must_use]
    pub const fn new(destination: PositionSource, arrival_in_ticks: i32) -> Self {
        Self {
            destination,
            arrival_in_ticks,
        }
    }

    #[must_use]
    pub const fn destination(&self) -> &PositionSource {
        &self.destination
    }

    #[must_use]
    pub const fn arrival_in_ticks(&self) -> i32 {
        self.arrival_in_ticks
    }
}

// SAFETY: This Steel-owned key uniquely identifies the concrete particle payload.
unsafe impl DowncastType for VibrationParticleOption {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:particle_options/vibration");
}

impl ParticleOptions for VibrationParticleOption {
    fn read_network(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self::new(
            PositionSource::read(data)?,
            VarInt::read(data)?.0,
        ))
    }

    fn write_network(&self, writer: &mut Vec<u8>) -> Result<()> {
        self.destination.write(writer)?;
        VarInt(self.arrival_in_ticks).write(writer)
    }
}

fn read_network<T: ParticleOptions>(
    data: &mut Cursor<&[u8]>,
) -> Result<Box<dyn ErasedParticleOptions>> {
    Ok(Box::new(T::read_network(data)?))
}

fn write_network<T: ParticleOptions>(
    options: &dyn ErasedParticleOptions,
    writer: &mut Vec<u8>,
) -> Result<()> {
    let options = options.downcast_ref::<T>().ok_or_else(|| {
        Error::other(format!(
            "Particle options payload does not match {}",
            T::TYPE_KEY
        ))
    })?;
    options.write_network(writer)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use glam::DVec3;
    use steel_utils::codec::VarInt;
    use steel_utils::serial::{ReadFrom, WriteTo};
    use steel_utils::{ArgbColor, BlockPos, BlockStateId, Identifier, RgbColor};

    use crate::item_stack_template::ItemStackTemplate;
    use crate::position_source::{BlockPositionSource, EntityPositionSource, PositionSource};
    use crate::{
        REGISTRY, test_support::init_test_registry, vanilla_items, vanilla_particle_types,
        vanilla_position_source_types,
    };

    use super::{
        BlockParticleOption, ColorParticleOption, DustColorTransitionOptions, DustParticleOptions,
        GeyserBaseParticleOptions, GeyserParticleOptions, ItemParticleOption, ParticleData,
        ParticleOptions, ParticleType, ParticleTypeRegistry, PowerParticleOption,
        SculkChargeParticleOptions, ShriekParticleOption, SpellParticleOption, TrailParticleOption,
        VibrationParticleOption,
    };

    static FORGED_FLAME: ParticleType =
        ParticleType::of::<BlockParticleOption>(Identifier::vanilla_static("flame"), false);

    fn assert_round_trip(particle: ParticleData) {
        let expected = particle.clone();
        let mut encoded = Vec::new();
        let result = particle.write(&mut encoded);
        assert!(result.is_ok(), "{result:?}");

        let mut cursor = Cursor::new(encoded.as_slice());
        let decoded = ParticleData::read(&mut cursor);
        let Ok(decoded) = decoded else {
            panic!("failed to decode particle: {decoded:?}");
        };
        assert_eq!(cursor.position() as usize, encoded.len());
        assert_eq!(decoded, expected);
    }

    #[test]
    fn every_vanilla_particle_payload_family_round_trips() {
        init_test_registry();

        assert_round_trip(ParticleData::simple(&vanilla_particle_types::FLAME));
        assert_round_trip(ParticleData::new(
            &vanilla_particle_types::BLOCK,
            BlockParticleOption::new(BlockStateId(321)),
        ));
        assert_round_trip(ParticleData::new(
            &vanilla_particle_types::ENTITY_EFFECT,
            ColorParticleOption::new(ArgbColor::new(i32::from_be_bytes([0xAA, 0xBB, 0xCC, 0xDD]))),
        ));
        assert_round_trip(ParticleData::new(
            &vanilla_particle_types::DUST,
            DustParticleOptions::new(RgbColor::new(0x123456), 1.25),
        ));
        assert_round_trip(ParticleData::new(
            &vanilla_particle_types::DUST_COLOR_TRANSITION,
            DustColorTransitionOptions::new(RgbColor::new(0x123456), RgbColor::new(0x654321), 2.5),
        ));
        assert_round_trip(ParticleData::new(
            &vanilla_particle_types::GEYSER,
            GeyserParticleOptions::new(4),
        ));
        assert_round_trip(ParticleData::new(
            &vanilla_particle_types::GEYSER_BASE,
            GeyserBaseParticleOptions::new(7, 0.75),
        ));
        assert_round_trip(ParticleData::new(
            &vanilla_particle_types::DRAGON_BREATH,
            PowerParticleOption::new(0.4),
        ));
        assert_round_trip(ParticleData::new(
            &vanilla_particle_types::EFFECT,
            SpellParticleOption::new(RgbColor::new(0xABCDEF), 0.8),
        ));
        assert_round_trip(ParticleData::new(
            &vanilla_particle_types::ITEM,
            ItemParticleOption::new(ItemStackTemplate::new(&vanilla_items::STONE)),
        ));
        assert_round_trip(ParticleData::new(
            &vanilla_particle_types::SCULK_CHARGE,
            SculkChargeParticleOptions::new(0.25),
        ));
        assert_round_trip(ParticleData::new(
            &vanilla_particle_types::SHRIEK,
            ShriekParticleOption::new(17),
        ));
        assert_round_trip(ParticleData::new(
            &vanilla_particle_types::TRAIL,
            TrailParticleOption::new(DVec3::new(1.25, -2.5, 3.75), RgbColor::new(0x345678), 40),
        ));
        assert_round_trip(ParticleData::new(
            &vanilla_particle_types::VIBRATION,
            VibrationParticleOption::new(
                PositionSource::new(
                    &vanilla_position_source_types::BLOCK,
                    BlockPositionSource::new(BlockPos::new(12, -34, 56)),
                ),
                9,
            ),
        ));
        assert_round_trip(ParticleData::new(
            &vanilla_particle_types::VIBRATION,
            VibrationParticleOption::new(
                PositionSource::new(
                    &vanilla_position_source_types::ENTITY,
                    EntityPositionSource::new(1234, 1.5),
                ),
                22,
            ),
        ));
    }

    #[test]
    fn particle_write_rejects_noncanonical_same_key_codec() {
        init_test_registry();

        let particle = ParticleData::new(
            &FORGED_FLAME,
            BlockParticleOption::new(BlockStateId::default()),
        );
        let mut encoded = Vec::new();
        let result = particle.write(&mut encoded);

        assert!(result.is_err());
        assert!(encoded.is_empty());
    }

    #[test]
    #[should_panic(expected = "Cannot register duplicate particle type key")]
    fn particle_type_registry_rejects_duplicate_keys() {
        let mut registry = ParticleTypeRegistry::new();
        registry.register(&vanilla_particle_types::FLAME);
        registry.register(&FORGED_FLAME);
    }

    #[test]
    fn block_particle_network_codec_rejects_invalid_state_ids() {
        init_test_registry();

        for id in [-1, i32::from(u16::MAX) + 1, i32::from(u16::MAX)] {
            let mut encoded = Vec::new();
            let encoded_result = VarInt(id).write(&mut encoded);
            assert!(encoded_result.is_ok());

            let mut cursor = Cursor::new(encoded.as_slice());
            let decoded = BlockParticleOption::read_network(&mut cursor);
            assert!(decoded.is_err(), "accepted invalid block state id {id}");
        }

        let invalid_state = BlockStateId(u16::MAX);
        assert!(REGISTRY.blocks.by_state_id(invalid_state).is_none());

        let mut encoded = Vec::new();
        let result = BlockParticleOption::new(invalid_state).write_network(&mut encoded);
        assert!(result.is_err());
        assert!(encoded.is_empty());
    }

    #[test]
    fn dust_scale_matches_vanilla_constructor_clamping() {
        assert_eq!(
            DustParticleOptions::new(RgbColor::new(0), 0.0).scale(),
            0.01
        );
        assert_eq!(DustParticleOptions::new(RgbColor::new(0), 5.0).scale(), 4.0);
        assert_eq!(
            DustColorTransitionOptions::new(RgbColor::new(0), RgbColor::new(0), -1.0).scale(),
            0.01
        );
    }
}
