use rustc_hash::FxHashMap;
use simdnbt::ToNbtTag;
use simdnbt::owned::NbtTag;
use steel_utils::Identifier;

use crate::RegistryTags;
use crate::sound_event::SoundEventRef;
use crate::sound_events;

/// Represents a damage type definition from a data pack JSON file.
#[derive(Debug)]
pub struct DamageType {
    pub key: Identifier,
    pub message_id: &'static str,
    pub scaling: DamageScaling,
    pub exhaustion: f32,
    pub effects: DamageEffects,
    pub death_message_type: DeathMessageType,
}

/// How the damage scales with difficulty.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageScaling {
    Always,
    WhenCausedByLivingNonPlayer,
    Never,
}

/// The sound effects played when an entity is damaged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageEffects {
    Hurt,
    Thorns,
    Drowning,
    Burning,
    Poking,
    Freezing,
}

impl DamageEffects {
    /// The sound a player makes when hurt with these effects.
    #[must_use]
    pub const fn sound(self) -> SoundEventRef {
        match self {
            Self::Hurt | Self::Thorns => &sound_events::ENTITY_PLAYER_HURT,
            Self::Drowning => &sound_events::ENTITY_PLAYER_HURT_DROWN,
            Self::Burning => &sound_events::ENTITY_PLAYER_HURT_ON_FIRE,
            Self::Poking => &sound_events::ENTITY_PLAYER_HURT_SWEET_BERRY_BUSH,
            Self::Freezing => &sound_events::ENTITY_PLAYER_HURT_FREEZE,
        }
    }
}

/// How the death message is formatted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeathMessageType {
    Default,
    FallVariants,
    IntentionalGameDesign,
}

impl ToNbtTag for &DamageType {
    fn to_nbt_tag(self) -> NbtTag {
        use simdnbt::owned::NbtCompound;
        let mut compound = NbtCompound::new();
        compound.insert("message_id", self.message_id);
        compound.insert(
            "scaling",
            match self.scaling {
                DamageScaling::Always => "always",
                DamageScaling::WhenCausedByLivingNonPlayer => "when_caused_by_living_non_player",
                DamageScaling::Never => "never",
            },
        );
        compound.insert("exhaustion", self.exhaustion);
        compound.insert(
            "effects",
            match self.effects {
                DamageEffects::Hurt => "hurt",
                DamageEffects::Thorns => "thorns",
                DamageEffects::Drowning => "drowning",
                DamageEffects::Burning => "burning",
                DamageEffects::Poking => "poking",
                DamageEffects::Freezing => "freezing",
            },
        );
        compound.insert(
            "death_message_type",
            match self.death_message_type {
                DeathMessageType::Default => "default",
                DeathMessageType::FallVariants => "fall_variants",
                DeathMessageType::IntentionalGameDesign => "intentional_game_design",
            },
        );
        NbtTag::Compound(compound)
    }
}

pub type DamageTypeRef = &'static DamageType;

pub struct DamageTypeRegistry {
    damage_types_by_id: Vec<DamageTypeRef>,
    damage_types_by_key: FxHashMap<Identifier, usize>,
    tags: RegistryTags,
    allows_registering: bool,
}

impl DamageTypeRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            damage_types_by_id: Vec::new(),
            damage_types_by_key: FxHashMap::default(),
            allows_registering: true,
            tags: RegistryTags::default(),
        }
    }
}

crate::impl_standard_methods!(
    DamageTypeRegistry,
    DamageTypeRef,
    damage_types_by_id,
    damage_types_by_key,
    allows_registering
);

crate::impl_registry!(
    DamageTypeRegistry,
    DamageType,
    damage_types_by_id,
    damage_types_by_key,
    damage_types
);
crate::impl_tagged_registry!(DamageTypeRegistry, damage_types_by_key, "damage type");
