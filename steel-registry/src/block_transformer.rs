//! `minecraft:block_transformer` registry entries.
//!
//! Vanilla moved this from an inline per-item component to a proper dynamic
//! registry (`Registries.BLOCK_TRANSFORMER`) referenced by items via
//! `Holder<BlockTransformer>`. Entries are built from datapack JSON at build
//! time; the item component itself only stores a registry reference.

use std::io::{Cursor, Error, Result, Write};
use std::str::FromStr;

use rustc_hash::FxHashMap;
use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use simdnbt::ToNbtTag;
use steel_utils::codec::VarInt;
use steel_utils::hash::{ComponentHasher, HashComponent};
use steel_utils::serial::{ReadFrom, WriteTo};
use steel_utils::{Direction, Identifier};

use crate::sound_event::SoundEventHolder;
use crate::{RegistryTags, REGISTRY};

/// Item block transforms, e.g. shovel flattening dirt into a path.
///
/// `PartialEq`/`Eq` come from [`crate::impl_registry_entry!`] (identity by key).
#[derive(Debug, Clone)]
pub struct BlockTransformer {
    pub key: Identifier,
    pub transforms: Vec<BlockTransformData>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BlockTransformData {
    pub block_state_provider: TransformStateProvider,
    pub sound: SoundEventHolder,
    pub particle: TransformParticle,
    pub disallowed_faces: Vec<Direction>,
    pub loot: Option<Identifier>,
    pub drop_strategy: DropStrategy,
    pub update_from_neighbors: bool,
    pub transform_type: TransformType,
    pub consume_on_use: bool,
    pub item_damage_per_use: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransformBlockState {
    pub block: Identifier,
    pub properties: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransformHolderSet {
    Tag(Identifier),
    Entries(Vec<Identifier>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeightedTransformBlockState {
    pub data: TransformBlockState,
    pub weight: i32,
}

/// `NormalNoise.NoiseParameters`.
#[derive(Debug, Clone, PartialEq)]
pub struct TransformNoiseParameters {
    pub first_octave: i32,
    pub amplitudes: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TransformStateProviderRule {
    pub if_true: TransformPredicate,
    pub then: TransformStateProvider,
}

/// `BlockStateProvider`. Vanilla type ids in `BlockStateProviderTypes` (this
/// version dropped the old `*_state_provider`/`*_provider` suffixes).
#[derive(Debug, Clone, PartialEq)]
pub enum TransformStateProvider {
    Simple {
        state: TransformBlockState,
    },
    Weighted {
        entries: Vec<WeightedTransformBlockState>,
    },
    NoiseThreshold {
        seed: i64,
        noise: TransformNoiseParameters,
        scale: f32,
        threshold: f32,
        high_chance: f32,
        default_state: TransformBlockState,
        low_states: Vec<TransformBlockState>,
        high_states: Vec<TransformBlockState>,
    },
    Noise {
        seed: i64,
        noise: TransformNoiseParameters,
        scale: f32,
        states: Vec<TransformBlockState>,
    },
    DualNoise {
        variety: (i32, i32),
        slow_noise: TransformNoiseParameters,
        slow_scale: f32,
        seed: i64,
        noise: TransformNoiseParameters,
        scale: f32,
        states: Vec<TransformBlockState>,
    },
    /// `RotatedBlockProvider`: `state` recurses into another provider, an
    /// explicit `direction` overrides the random axis/facing pick.
    RotatedBlock {
        state: Box<TransformStateProvider>,
        direction: Option<Direction>,
    },
    RandomizedInt {
        source: Box<TransformStateProvider>,
        property: String,
        // ponytail: only Constant is used by shipped data; other IntProvider
        // shapes round-trip through the generic decoder untested here.
        values: IntProviderLiteral,
    },
    RuleBased {
        fallback: Option<Box<TransformStateProvider>>,
        rules: Vec<TransformStateProviderRule>,
    },
    CopyProperties {
        source: Box<TransformStateProvider>,
    },
}

/// Minimal stand-in for `net.minecraft.util.valueproviders.IntProvider`,
/// covering only the shapes wired up for `randomized_int` block transforms.
#[derive(Debug, Clone, PartialEq)]
pub enum IntProviderLiteral {
    Constant(i32),
    Uniform { min_inclusive: i32, max_inclusive: i32 },
}

/// `BlockPredicate` (worldgen predicate, not the advancement one).
#[derive(Debug, Clone, PartialEq)]
pub enum TransformPredicate {
    MatchingBlocks {
        offset: (i32, i32, i32),
        blocks: TransformHolderSet,
    },
    MatchingBlockTag {
        offset: (i32, i32, i32),
        tag: Identifier,
    },
    MatchingFluids {
        offset: (i32, i32, i32),
        fluids: TransformHolderSet,
    },
    MatchingBiomes {
        biomes: TransformHolderSet,
    },
    HasSturdyFace {
        offset: (i32, i32, i32),
        direction: Direction,
    },
    Solid {
        offset: (i32, i32, i32),
    },
    Replaceable {
        offset: (i32, i32, i32),
    },
    WouldSurvive {
        offset: (i32, i32, i32),
        state: TransformBlockState,
    },
    InsideWorldBounds {
        offset: (i32, i32, i32),
    },
    AnyOf(Vec<TransformPredicate>),
    AllOf(Vec<TransformPredicate>),
    Not(Box<TransformPredicate>),
    True,
    Unobstructed {
        offset: (i32, i32, i32),
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TransformParticle {
    #[default]
    None,
    Scrape,
    WaxOn,
    WaxOff,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DropStrategy {
    ClickedFace,
    #[default]
    FromMiddle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TransformType {
    #[default]
    SingleBlock,
    CopperChest,
}

/// Registry-sync NBT for the `minecraft:block_transformer` dynamic registry.
/// Only encoding is needed: entries are build-time data, never decoded from
/// player-supplied NBT or network input.
impl ToNbtTag for &BlockTransformer {
    fn to_nbt_tag(self) -> NbtTag {
        NbtTag::List(NbtList::Compound(
            self.transforms.iter().map(transform_data_nbt).collect(),
        ))
    }
}

fn transform_data_nbt(transform: &BlockTransformData) -> NbtCompound {
    let mut value = NbtCompound::new();
    value.insert(
        "block_state_provider",
        provider_nbt(&transform.block_state_provider),
    );
    value.insert("sound", sound_holder_nbt(&transform.sound));
    if transform.particle != TransformParticle::None {
        value.insert("particle", particle_name(transform.particle));
    }
    if !transform.disallowed_faces.is_empty() {
        value.insert(
            "disallowed_faces",
            NbtList::String(
                transform
                    .disallowed_faces
                    .iter()
                    .map(|face| face.as_str().into())
                    .collect(),
            ),
        );
    }
    if let Some(loot) = &transform.loot {
        value.insert("loot", loot.to_string());
    }
    if transform.drop_strategy != DropStrategy::FromMiddle {
        value.insert("drop_strategy", drop_strategy_name(transform.drop_strategy));
    }
    if !transform.update_from_neighbors {
        value.insert("update_from_neighbors", 0_i8);
    }
    if transform.transform_type != TransformType::SingleBlock {
        value.insert(
            "transform_type",
            transform_type_name(transform.transform_type),
        );
    }
    if !transform.consume_on_use {
        value.insert("consume_on_use", 0_i8);
    }
    if transform.item_damage_per_use != 0 {
        value.insert("item_damage_per_use", transform.item_damage_per_use);
    }
    value
}

fn sound_holder_nbt(sound: &SoundEventHolder) -> NbtTag {
    // SoundEventHolder's own ToNbtTag matches the persistent `Holder<SoundEvent>`
    // shape used elsewhere in this crate.
    sound.clone().to_nbt_tag()
}

fn provider_nbt(provider: &TransformStateProvider) -> NbtTag {
    let mut value = NbtCompound::new();
    match provider {
        TransformStateProvider::Simple { state } => {
            value.insert("state", block_state_nbt(state));
            value.insert("type", "minecraft:simple");
        }
        TransformStateProvider::Weighted { entries } => {
            value.insert(
                "entries",
                NbtList::Compound(
                    entries
                        .iter()
                        .map(|entry| {
                            let mut entry_nbt = NbtCompound::new();
                            entry_nbt.insert("data", block_state_nbt(&entry.data));
                            entry_nbt.insert("weight", entry.weight);
                            entry_nbt
                        })
                        .collect(),
                ),
            );
            value.insert("type", "minecraft:weighted");
        }
        TransformStateProvider::NoiseThreshold {
            seed,
            noise,
            scale,
            threshold,
            high_chance,
            default_state,
            low_states,
            high_states,
        } => {
            value.insert("seed", *seed);
            value.insert("noise", noise_parameters_nbt(noise));
            value.insert("scale", *scale);
            value.insert("threshold", *threshold);
            value.insert("high_chance", *high_chance);
            value.insert("default_state", block_state_nbt(default_state));
            value.insert("low_states", block_state_list_nbt(low_states));
            value.insert("high_states", block_state_list_nbt(high_states));
            value.insert("type", "minecraft:noise_threshold");
        }
        TransformStateProvider::Noise {
            seed,
            noise,
            scale,
            states,
        } => {
            value.insert("seed", *seed);
            value.insert("noise", noise_parameters_nbt(noise));
            value.insert("scale", *scale);
            value.insert("states", block_state_list_nbt(states));
            value.insert("type", "minecraft:noise");
        }
        TransformStateProvider::DualNoise {
            variety,
            slow_noise,
            slow_scale,
            seed,
            noise,
            scale,
            states,
        } => {
            value.insert("variety", NbtList::Int(vec![variety.0, variety.1]));
            value.insert("slow_noise", noise_parameters_nbt(slow_noise));
            value.insert("slow_scale", *slow_scale);
            value.insert("seed", *seed);
            value.insert("noise", noise_parameters_nbt(noise));
            value.insert("scale", *scale);
            value.insert("states", block_state_list_nbt(states));
            value.insert("type", "minecraft:dual_noise");
        }
        TransformStateProvider::RotatedBlock { state, direction } => {
            value.insert("state", provider_nbt(state));
            if let Some(direction) = direction {
                value.insert("direction", direction.as_str());
            }
            value.insert("type", "minecraft:rotated");
        }
        TransformStateProvider::RandomizedInt {
            source,
            property,
            values,
        } => {
            value.insert("source", provider_nbt(source));
            value.insert("property", property.as_str());
            value.insert("values", int_provider_nbt(values));
            value.insert("type", "minecraft:randomized_int");
        }
        TransformStateProvider::RuleBased { fallback, rules } => {
            if let Some(fallback) = fallback {
                value.insert("fallback", provider_nbt(fallback));
            }
            value.insert(
                "rules",
                NbtList::Compound(
                    rules
                        .iter()
                        .map(|rule| {
                            let mut rule_nbt = NbtCompound::new();
                            rule_nbt.insert("if_true", predicate_nbt(&rule.if_true));
                            rule_nbt.insert("then", provider_nbt(&rule.then));
                            rule_nbt
                        })
                        .collect(),
                ),
            );
            value.insert("type", "minecraft:rule_based");
        }
        TransformStateProvider::CopyProperties { source } => {
            value.insert("source", provider_nbt(source));
            value.insert("type", "minecraft:copy_properties");
        }
    }
    NbtTag::Compound(value)
}

fn block_state_nbt(state: &TransformBlockState) -> NbtTag {
    let mut value = NbtCompound::new();
    value.insert("id", state.block.to_string());
    if !state.properties.is_empty() {
        let mut properties = NbtCompound::new();
        for (name, property) in &state.properties {
            properties.insert(name.as_str(), property.as_str());
        }
        value.insert("properties", properties);
    }
    NbtTag::Compound(value)
}

fn block_state_list_nbt(states: &[TransformBlockState]) -> NbtList {
    NbtList::Compound(
        states
            .iter()
            .map(|state| {
                let NbtTag::Compound(state) = block_state_nbt(state) else {
                    unreachable!("block state always encodes as a compound")
                };
                state
            })
            .collect(),
    )
}

fn noise_parameters_nbt(parameters: &TransformNoiseParameters) -> NbtCompound {
    let mut value = NbtCompound::new();
    value.insert("firstOctave", parameters.first_octave);
    value.insert("amplitudes", NbtList::Double(parameters.amplitudes.clone()));
    value
}

fn int_provider_nbt(provider: &IntProviderLiteral) -> NbtTag {
    match provider {
        IntProviderLiteral::Constant(value) => NbtTag::Int(*value),
        IntProviderLiteral::Uniform {
            min_inclusive,
            max_inclusive,
        } => {
            let mut value = NbtCompound::new();
            value.insert("min_inclusive", *min_inclusive);
            value.insert("max_inclusive", *max_inclusive);
            value.insert("type", "minecraft:uniform");
            NbtTag::Compound(value)
        }
    }
}

fn predicate_nbt(predicate: &TransformPredicate) -> NbtTag {
    let mut value = NbtCompound::new();
    match predicate {
        TransformPredicate::MatchingBlocks { offset, blocks } => {
            value.insert("blocks", holder_set_nbt(blocks));
            insert_offset(&mut value, *offset);
            value.insert("type", "minecraft:matching_blocks");
        }
        TransformPredicate::MatchingBlockTag { offset, tag } => {
            insert_offset(&mut value, *offset);
            value.insert("tag", tag.to_string());
            value.insert("type", "minecraft:matching_block_tag");
        }
        TransformPredicate::MatchingFluids { offset, fluids } => {
            value.insert("fluids", holder_set_nbt(fluids));
            insert_offset(&mut value, *offset);
            value.insert("type", "minecraft:matching_fluids");
        }
        TransformPredicate::MatchingBiomes { biomes } => {
            value.insert("biomes", holder_set_nbt(biomes));
            value.insert("type", "minecraft:matching_biomes");
        }
        TransformPredicate::HasSturdyFace { offset, direction } => {
            insert_offset(&mut value, *offset);
            value.insert("direction", direction.as_str());
            value.insert("type", "minecraft:has_sturdy_face");
        }
        TransformPredicate::Solid { offset } => {
            insert_offset(&mut value, *offset);
            value.insert("type", "minecraft:solid");
        }
        TransformPredicate::Replaceable { offset } => {
            insert_offset(&mut value, *offset);
            value.insert("type", "minecraft:replaceable");
        }
        TransformPredicate::WouldSurvive { offset, state } => {
            insert_offset(&mut value, *offset);
            value.insert("state", block_state_nbt(state));
            value.insert("type", "minecraft:would_survive");
        }
        TransformPredicate::InsideWorldBounds { offset } => {
            insert_offset(&mut value, *offset);
            value.insert("type", "minecraft:inside_world_bounds");
        }
        TransformPredicate::AnyOf(predicates) => {
            value.insert(
                "predicates",
                NbtList::Compound(
                    predicates
                        .iter()
                        .map(|predicate| {
                            let NbtTag::Compound(predicate) = predicate_nbt(predicate) else {
                                unreachable!("predicate always encodes as a compound")
                            };
                            predicate
                        })
                        .collect(),
                ),
            );
            value.insert("type", "minecraft:any_of");
        }
        TransformPredicate::AllOf(predicates) => {
            value.insert(
                "predicates",
                NbtList::Compound(
                    predicates
                        .iter()
                        .map(|predicate| {
                            let NbtTag::Compound(predicate) = predicate_nbt(predicate) else {
                                unreachable!("predicate always encodes as a compound")
                            };
                            predicate
                        })
                        .collect(),
                ),
            );
            value.insert("type", "minecraft:all_of");
        }
        TransformPredicate::Not(predicate) => {
            value.insert("predicate", predicate_nbt(predicate));
            value.insert("type", "minecraft:not");
        }
        TransformPredicate::True => {
            value.insert("type", "minecraft:true");
        }
        TransformPredicate::Unobstructed { offset } => {
            insert_offset(&mut value, *offset);
            value.insert("type", "minecraft:unobstructed");
        }
    }
    NbtTag::Compound(value)
}

fn holder_set_nbt(set: &TransformHolderSet) -> NbtTag {
    match set {
        TransformHolderSet::Tag(tag) => format!("#{tag}").to_nbt_tag(),
        TransformHolderSet::Entries(entries) if entries.len() == 1 => {
            entries[0].to_string().to_nbt_tag()
        }
        TransformHolderSet::Entries(entries) => NbtTag::List(NbtList::String(
            entries.iter().map(|entry| entry.to_string().into()).collect(),
        )),
    }
}

fn insert_offset(value: &mut NbtCompound, offset: (i32, i32, i32)) {
    if offset != (0, 0, 0) {
        value.insert("offset", NbtTag::IntArray(vec![offset.0, offset.1, offset.2]));
    }
}

fn particle_name(particle: TransformParticle) -> &'static str {
    match particle {
        TransformParticle::None => "none",
        TransformParticle::Scrape => "scrape",
        TransformParticle::WaxOn => "wax_on",
        TransformParticle::WaxOff => "wax_off",
    }
}

fn drop_strategy_name(strategy: DropStrategy) -> &'static str {
    match strategy {
        DropStrategy::ClickedFace => "clicked_face",
        DropStrategy::FromMiddle => "from_middle",
    }
}

fn transform_type_name(transform_type: TransformType) -> &'static str {
    match transform_type {
        TransformType::SingleBlock => "single_block",
        TransformType::CopperChest => "copper_chest",
    }
}

pub type BlockTransformerRef = &'static BlockTransformer;

/// Item component wrapper: a `Holder<BlockTransformer>` reference, wire- and
/// NBT-encoded like other registry holders (e.g. [`crate::damage_type::DamageTypeComponent`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockTransformerComponent {
    pub block_transformer: BlockTransformerRef,
}

impl BlockTransformerComponent {
    #[must_use]
    pub const fn new(block_transformer: BlockTransformerRef) -> Self {
        Self { block_transformer }
    }
}

impl WriteTo for BlockTransformerComponent {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        let id = self.block_transformer.try_id().ok_or_else(|| {
            Error::other(format!(
                "Unknown block transformer: {}",
                self.block_transformer.key
            ))
        })?;
        let id = i32::try_from(id).map_err(|_| {
            Error::other(format!("Block transformer id out of protocol range: {id}"))
        })?;
        VarInt(id).write(writer)
    }
}

impl ReadFrom for BlockTransformerComponent {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let id = VarInt::read(data)?.0;
        let id = usize::try_from(id)
            .map_err(|_| Error::other(format!("Negative block transformer id: {id}")))?;
        let block_transformer = REGISTRY
            .block_transformers
            .by_id(id)
            .ok_or_else(|| Error::other(format!("Unknown block transformer id: {id}")))?;
        Ok(Self { block_transformer })
    }
}

impl simdnbt::ToNbtTag for BlockTransformerComponent {
    fn to_nbt_tag(self) -> NbtTag {
        self.block_transformer.key.to_string().to_nbt_tag()
    }
}

impl simdnbt::FromNbtTag for BlockTransformerComponent {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        let key = Identifier::from_str(&tag.string()?.to_str()).ok()?;
        REGISTRY
            .block_transformers
            .by_key(&key)
            .map(|block_transformer| Self { block_transformer })
    }
}

impl HashComponent for BlockTransformerComponent {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        hasher.put_string(&self.block_transformer.key.to_string());
    }
}

pub struct BlockTransformerRegistry {
    entries_by_id: Vec<BlockTransformerRef>,
    entries_by_key: FxHashMap<Identifier, usize>,
    tags: RegistryTags,
    allows_registering: bool,
}

impl BlockTransformerRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries_by_id: Vec::new(),
            entries_by_key: FxHashMap::default(),
            allows_registering: true,
            tags: RegistryTags::default(),
        }
    }
}

crate::impl_standard_methods!(
    BlockTransformerRegistry,
    BlockTransformerRef,
    entries_by_id,
    entries_by_key,
    allows_registering
);

crate::impl_registry!(
    BlockTransformerRegistry,
    BlockTransformer,
    entries_by_id,
    entries_by_key,
    block_transformers
);
