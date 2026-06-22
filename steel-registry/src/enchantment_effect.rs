use crate::attribute::{AttributeModifierOperation, AttributeRef};
use crate::damage_type::DamageTypeRef;
use crate::mob_effect::MobEffectRef;
use crate::sound_event::SoundEventRef;
use glam::DVec3;
use steel_utils::Identifier;
use steel_utils::types::GameType;

/// Vanilla enchantment effect component keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EnchantmentEffectComponent {
    DamageProtection,
    DamageImmunity,
    Damage,
    SmashDamagePerFallenBlock,
    Knockback,
    ArmorEffectiveness,
    PostAttack,
    PostPiercingAttack,
    HitBlock,
    ItemDamage,
    EquipmentDrops,
    LocationChanged,
    Tick,
    AmmoUse,
    ProjectilePiercing,
    ProjectileSpawned,
    ProjectileSpread,
    ProjectileCount,
    TridentReturnAcceleration,
    FishingTimeReduction,
    FishingLuckBonus,
    BlockExperience,
    MobExperience,
    RepairWithXp,
    Attributes,
    CrossbowChargeTime,
    CrossbowChargingSounds,
    TridentSound,
    PreventEquipmentDrop,
    PreventArmorChange,
    TridentSpinAttackStrength,
}

impl EnchantmentEffectComponent {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::DamageProtection => "minecraft:damage_protection",
            Self::DamageImmunity => "minecraft:damage_immunity",
            Self::Damage => "minecraft:damage",
            Self::SmashDamagePerFallenBlock => "minecraft:smash_damage_per_fallen_block",
            Self::Knockback => "minecraft:knockback",
            Self::ArmorEffectiveness => "minecraft:armor_effectiveness",
            Self::PostAttack => "minecraft:post_attack",
            Self::PostPiercingAttack => "minecraft:post_piercing_attack",
            Self::HitBlock => "minecraft:hit_block",
            Self::ItemDamage => "minecraft:item_damage",
            Self::EquipmentDrops => "minecraft:equipment_drops",
            Self::LocationChanged => "minecraft:location_changed",
            Self::Tick => "minecraft:tick",
            Self::AmmoUse => "minecraft:ammo_use",
            Self::ProjectilePiercing => "minecraft:projectile_piercing",
            Self::ProjectileSpawned => "minecraft:projectile_spawned",
            Self::ProjectileSpread => "minecraft:projectile_spread",
            Self::ProjectileCount => "minecraft:projectile_count",
            Self::TridentReturnAcceleration => "minecraft:trident_return_acceleration",
            Self::FishingTimeReduction => "minecraft:fishing_time_reduction",
            Self::FishingLuckBonus => "minecraft:fishing_luck_bonus",
            Self::BlockExperience => "minecraft:block_experience",
            Self::MobExperience => "minecraft:mob_experience",
            Self::RepairWithXp => "minecraft:repair_with_xp",
            Self::Attributes => "minecraft:attributes",
            Self::CrossbowChargeTime => "minecraft:crossbow_charge_time",
            Self::CrossbowChargingSounds => "minecraft:crossbow_charging_sounds",
            Self::TridentSound => "minecraft:trident_sound",
            Self::PreventEquipmentDrop => "minecraft:prevent_equipment_drop",
            Self::PreventArmorChange => "minecraft:prevent_armor_change",
            Self::TridentSpinAttackStrength => "minecraft:trident_spin_attack_strength",
        }
    }
}

/// Vanilla `LevelBasedValue`.
#[derive(Debug, PartialEq)]
pub enum LevelBasedValue {
    Constant(f32),
    Clamped {
        value: &'static LevelBasedValue,
        min: f32,
        max: f32,
    },
    Exponent {
        base: &'static LevelBasedValue,
        power: &'static LevelBasedValue,
    },
    Fraction {
        numerator: &'static LevelBasedValue,
        denominator: &'static LevelBasedValue,
    },
    LevelsSquared {
        added: f32,
    },
    Linear {
        base: f32,
        per_level_above_first: f32,
    },
    Lookup {
        values: &'static [f32],
        fallback: &'static LevelBasedValue,
    },
}

impl LevelBasedValue {
    #[must_use]
    pub fn calculate(&self, level: i32) -> f32 {
        match self {
            Self::Constant(value) => *value,
            Self::Clamped { value, min, max } => value.calculate(level).clamp(*min, *max),
            Self::Exponent { base, power } => base.calculate(level).powf(power.calculate(level)),
            Self::Fraction {
                numerator,
                denominator,
            } => {
                let denominator = denominator.calculate(level);
                if denominator == 0.0 {
                    0.0
                } else {
                    numerator.calculate(level) / denominator
                }
            }
            Self::LevelsSquared { added } => level.pow(2) as f32 + added,
            Self::Linear {
                base,
                per_level_above_first,
            } => base + per_level_above_first * (level - 1) as f32,
            Self::Lookup { values, fallback } => {
                if level <= 0 {
                    return fallback.calculate(level);
                }
                let index = (level - 1) as usize;
                values
                    .get(index)
                    .copied()
                    .unwrap_or_else(|| fallback.calculate(level))
            }
        }
    }
}

/// Vanilla `EnchantmentValueEffect`.
#[derive(Debug, PartialEq)]
pub enum EnchantmentValueEffect {
    Add { value: &'static LevelBasedValue },
    Set { value: &'static LevelBasedValue },
    Multiply { factor: &'static LevelBasedValue },
    RemoveBinomial { chance: &'static LevelBasedValue },
}

impl EnchantmentValueEffect {
    /// Applies effects that do not require a random source.
    ///
    /// `remove_binomial` is intentionally excluded until callers provide a
    /// vanilla-compatible random source and binomial implementation.
    #[must_use]
    pub fn process_without_random(&self, level: i32, input: f32) -> Option<f32> {
        match self {
            Self::Add { value } => Some(input + value.calculate(level)),
            Self::Set { value } => Some(value.calculate(level)),
            Self::Multiply { factor } => Some(input * factor.calculate(level)),
            Self::RemoveBinomial { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnchantmentEntityTarget {
    This,
    Attacker,
    DirectAttacker,
}

#[derive(Debug, PartialEq, Eq)]
pub enum EntityTypePredicate {
    Any,
    Type(Identifier),
    Tag(Identifier),
    Unsupported,
}

#[derive(Debug, PartialEq, Eq)]
pub struct EntityPredicate {
    pub entity_type: EntityTypePredicate,
    pub vehicle: EntityVehiclePredicate,
    pub flags: EntityFlagsPredicate,
    pub type_specific: EntityTypeSpecificPredicate,
    pub unsupported: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityVehiclePredicate {
    Any,
    Present,
    Unsupported,
}

#[derive(Debug, PartialEq, Eq)]
pub struct EntityFlagsPredicate {
    pub is_fall_flying: Option<bool>,
    pub is_in_water: Option<bool>,
    pub unsupported: bool,
}

impl EntityFlagsPredicate {
    #[must_use]
    pub const fn any() -> Self {
        Self {
            is_fall_flying: None,
            is_in_water: None,
            unsupported: false,
        }
    }

    #[must_use]
    pub const fn has_constraints(&self) -> bool {
        self.is_fall_flying.is_some() || self.is_in_water.is_some() || self.unsupported
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum EntityTypeSpecificPredicate {
    Any,
    Player(PlayerPredicate),
    Unsupported,
}

#[derive(Debug, PartialEq, Eq)]
pub struct PlayerPredicate {
    pub game_modes: &'static [GameType],
    pub food_level_min: Option<i32>,
    pub unsupported: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub struct DamageSourceTagPredicate {
    pub tag: Identifier,
    pub expected: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub struct DamageSourcePredicate {
    pub tags: &'static [DamageSourceTagPredicate],
    pub is_direct: Option<bool>,
}

/// Vanilla loot condition subset used by generated enchantment effects.
#[derive(Debug, PartialEq)]
pub enum EnchantmentEffectRequirements {
    AllOf(&'static [&'static EnchantmentEffectRequirements]),
    AnyOf(&'static [&'static EnchantmentEffectRequirements]),
    Inverted(&'static EnchantmentEffectRequirements),
    EntityProperties {
        entity: EnchantmentEntityTarget,
        predicate: EntityPredicate,
    },
    DamageSourceProperties(DamageSourcePredicate),
    RandomChance {
        chance: &'static LevelBasedValue,
    },
    Unsupported {
        condition: Identifier,
    },
}

#[derive(Debug, PartialEq)]
pub struct ConditionalEnchantmentEffect<T> {
    pub effect: T,
    pub requirements: Option<&'static EnchantmentEffectRequirements>,
}

impl<T> ConditionalEnchantmentEffect<T> {
    #[must_use]
    pub const fn is_unconditional(&self) -> bool {
        self.requirements.is_none()
    }
}

#[derive(Debug, PartialEq)]
pub struct ConditionalDamageImmunityEffect {
    pub requirements: Option<&'static EnchantmentEffectRequirements>,
}

impl ConditionalDamageImmunityEffect {
    #[must_use]
    pub const fn is_unconditional(&self) -> bool {
        self.requirements.is_none()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnchantmentTarget {
    Attacker,
    DamagingEntity,
    Victim,
}

#[derive(Debug)]
pub enum MobEffectSelection {
    Single(MobEffectRef),
    UnsupportedTag(Identifier),
}

#[derive(Debug)]
pub enum EnchantmentEntityEffect {
    AllOf(&'static [&'static EnchantmentEntityEffect]),
    ChangeItemDamage {
        amount: &'static LevelBasedValue,
    },
    ApplyExhaustion {
        amount: &'static LevelBasedValue,
    },
    ApplyImpulse {
        direction: DVec3,
        coordinate_scale: DVec3,
        magnitude: &'static LevelBasedValue,
    },
    PlaySound {
        sounds: &'static [SoundEventRef],
        volume: f32,
        pitch: f32,
    },
    DamageEntity {
        min_damage: &'static LevelBasedValue,
        max_damage: &'static LevelBasedValue,
        damage_type: DamageTypeRef,
    },
    Ignite {
        duration: &'static LevelBasedValue,
    },
    ApplyMobEffect {
        to_apply: MobEffectSelection,
        min_duration: &'static LevelBasedValue,
        max_duration: &'static LevelBasedValue,
        min_amplifier: &'static LevelBasedValue,
        max_amplifier: &'static LevelBasedValue,
    },
    Unsupported {
        effect_type: Identifier,
    },
}

#[derive(Debug)]
pub struct TargetedConditionalEnchantmentEffect<T> {
    pub effect: T,
    pub enchanted: EnchantmentTarget,
    pub affected: EnchantmentTarget,
    pub requirements: Option<&'static EnchantmentEffectRequirements>,
}

#[derive(Debug)]
pub struct EnchantmentAttributeEffect {
    pub amount: &'static LevelBasedValue,
    pub attribute: AttributeRef,
    pub id: Identifier,
    pub operation: AttributeModifierOperation,
}

#[derive(Debug)]
pub struct CrossbowChargingSounds {
    pub start: Option<SoundEventRef>,
    pub mid: Option<SoundEventRef>,
    pub end: Option<SoundEventRef>,
}

#[derive(Debug)]
pub struct EnchantmentEffects {
    pub damage_protection: &'static [ConditionalEnchantmentEffect<EnchantmentValueEffect>],
    pub damage_immunity: &'static [ConditionalDamageImmunityEffect],
    pub damage: &'static [ConditionalEnchantmentEffect<EnchantmentValueEffect>],
    pub smash_damage_per_fallen_block:
        &'static [ConditionalEnchantmentEffect<EnchantmentValueEffect>],
    pub knockback: &'static [ConditionalEnchantmentEffect<EnchantmentValueEffect>],
    pub armor_effectiveness: &'static [ConditionalEnchantmentEffect<EnchantmentValueEffect>],
    pub post_attack: &'static [TargetedConditionalEnchantmentEffect<EnchantmentEntityEffect>],
    pub post_piercing_attack: &'static [ConditionalEnchantmentEffect<EnchantmentEntityEffect>],
    pub hit_block: bool,
    pub item_damage: &'static [ConditionalEnchantmentEffect<EnchantmentValueEffect>],
    pub equipment_drops: &'static [TargetedConditionalEnchantmentEffect<EnchantmentValueEffect>],
    pub location_changed: bool,
    pub tick: bool,
    pub ammo_use: &'static [ConditionalEnchantmentEffect<EnchantmentValueEffect>],
    pub projectile_piercing: &'static [ConditionalEnchantmentEffect<EnchantmentValueEffect>],
    pub projectile_spawned: bool,
    pub projectile_spread: &'static [ConditionalEnchantmentEffect<EnchantmentValueEffect>],
    pub projectile_count: &'static [ConditionalEnchantmentEffect<EnchantmentValueEffect>],
    pub trident_return_acceleration:
        &'static [ConditionalEnchantmentEffect<EnchantmentValueEffect>],
    pub fishing_time_reduction: &'static [ConditionalEnchantmentEffect<EnchantmentValueEffect>],
    pub fishing_luck_bonus: &'static [ConditionalEnchantmentEffect<EnchantmentValueEffect>],
    pub block_experience: &'static [ConditionalEnchantmentEffect<EnchantmentValueEffect>],
    pub mob_experience: &'static [ConditionalEnchantmentEffect<EnchantmentValueEffect>],
    pub repair_with_xp: &'static [ConditionalEnchantmentEffect<EnchantmentValueEffect>],
    pub attributes: &'static [EnchantmentAttributeEffect],
    pub crossbow_charge_time: Option<EnchantmentValueEffect>,
    pub crossbow_charging_sounds: &'static [CrossbowChargingSounds],
    pub trident_sound: &'static [SoundEventRef],
    pub prevent_equipment_drop: bool,
    pub prevent_armor_change: bool,
    pub trident_spin_attack_strength: Option<EnchantmentValueEffect>,
}

impl EnchantmentEffects {
    pub const EMPTY: Self = Self {
        damage_protection: &[],
        damage_immunity: &[],
        damage: &[],
        smash_damage_per_fallen_block: &[],
        knockback: &[],
        armor_effectiveness: &[],
        post_attack: &[],
        post_piercing_attack: &[],
        hit_block: false,
        item_damage: &[],
        equipment_drops: &[],
        location_changed: false,
        tick: false,
        ammo_use: &[],
        projectile_piercing: &[],
        projectile_spawned: false,
        projectile_spread: &[],
        projectile_count: &[],
        trident_return_acceleration: &[],
        fishing_time_reduction: &[],
        fishing_luck_bonus: &[],
        block_experience: &[],
        mob_experience: &[],
        repair_with_xp: &[],
        attributes: &[],
        crossbow_charge_time: None,
        crossbow_charging_sounds: &[],
        trident_sound: &[],
        prevent_equipment_drop: false,
        prevent_armor_change: false,
        trident_spin_attack_strength: None,
    };

    #[must_use]
    pub fn has(&self, component: EnchantmentEffectComponent) -> bool {
        match component {
            EnchantmentEffectComponent::DamageProtection => !self.damage_protection.is_empty(),
            EnchantmentEffectComponent::DamageImmunity => !self.damage_immunity.is_empty(),
            EnchantmentEffectComponent::Damage => !self.damage.is_empty(),
            EnchantmentEffectComponent::SmashDamagePerFallenBlock => {
                !self.smash_damage_per_fallen_block.is_empty()
            }
            EnchantmentEffectComponent::Knockback => !self.knockback.is_empty(),
            EnchantmentEffectComponent::ArmorEffectiveness => !self.armor_effectiveness.is_empty(),
            EnchantmentEffectComponent::PostAttack => !self.post_attack.is_empty(),
            EnchantmentEffectComponent::PostPiercingAttack => !self.post_piercing_attack.is_empty(),
            EnchantmentEffectComponent::HitBlock => self.hit_block,
            EnchantmentEffectComponent::ItemDamage => !self.item_damage.is_empty(),
            EnchantmentEffectComponent::EquipmentDrops => !self.equipment_drops.is_empty(),
            EnchantmentEffectComponent::LocationChanged => self.location_changed,
            EnchantmentEffectComponent::Tick => self.tick,
            EnchantmentEffectComponent::AmmoUse => !self.ammo_use.is_empty(),
            EnchantmentEffectComponent::ProjectilePiercing => !self.projectile_piercing.is_empty(),
            EnchantmentEffectComponent::ProjectileSpawned => self.projectile_spawned,
            EnchantmentEffectComponent::ProjectileSpread => !self.projectile_spread.is_empty(),
            EnchantmentEffectComponent::ProjectileCount => !self.projectile_count.is_empty(),
            EnchantmentEffectComponent::TridentReturnAcceleration => {
                !self.trident_return_acceleration.is_empty()
            }
            EnchantmentEffectComponent::FishingTimeReduction => {
                !self.fishing_time_reduction.is_empty()
            }
            EnchantmentEffectComponent::FishingLuckBonus => !self.fishing_luck_bonus.is_empty(),
            EnchantmentEffectComponent::BlockExperience => !self.block_experience.is_empty(),
            EnchantmentEffectComponent::MobExperience => !self.mob_experience.is_empty(),
            EnchantmentEffectComponent::RepairWithXp => !self.repair_with_xp.is_empty(),
            EnchantmentEffectComponent::Attributes => !self.attributes.is_empty(),
            EnchantmentEffectComponent::CrossbowChargeTime => self.crossbow_charge_time.is_some(),
            EnchantmentEffectComponent::CrossbowChargingSounds => {
                !self.crossbow_charging_sounds.is_empty()
            }
            EnchantmentEffectComponent::TridentSound => !self.trident_sound.is_empty(),
            EnchantmentEffectComponent::PreventEquipmentDrop => self.prevent_equipment_drop,
            EnchantmentEffectComponent::PreventArmorChange => self.prevent_armor_change,
            EnchantmentEffectComponent::TridentSpinAttackStrength => {
                self.trident_spin_attack_strength.is_some()
            }
        }
    }

    #[must_use]
    pub fn value_effects(
        &self,
        component: EnchantmentEffectComponent,
    ) -> &'static [ConditionalEnchantmentEffect<EnchantmentValueEffect>] {
        match component {
            EnchantmentEffectComponent::DamageProtection => self.damage_protection,
            EnchantmentEffectComponent::Damage => self.damage,
            EnchantmentEffectComponent::SmashDamagePerFallenBlock => {
                self.smash_damage_per_fallen_block
            }
            EnchantmentEffectComponent::Knockback => self.knockback,
            EnchantmentEffectComponent::ArmorEffectiveness => self.armor_effectiveness,
            EnchantmentEffectComponent::ItemDamage => self.item_damage,
            EnchantmentEffectComponent::AmmoUse => self.ammo_use,
            EnchantmentEffectComponent::ProjectilePiercing => self.projectile_piercing,
            EnchantmentEffectComponent::ProjectileSpread => self.projectile_spread,
            EnchantmentEffectComponent::ProjectileCount => self.projectile_count,
            EnchantmentEffectComponent::TridentReturnAcceleration => {
                self.trident_return_acceleration
            }
            EnchantmentEffectComponent::FishingTimeReduction => self.fishing_time_reduction,
            EnchantmentEffectComponent::FishingLuckBonus => self.fishing_luck_bonus,
            EnchantmentEffectComponent::BlockExperience => self.block_experience,
            EnchantmentEffectComponent::MobExperience => self.mob_experience,
            EnchantmentEffectComponent::RepairWithXp => self.repair_with_xp,
            _ => &[],
        }
    }

    #[must_use]
    pub fn targeted_value_effects(
        &self,
        component: EnchantmentEffectComponent,
    ) -> &'static [TargetedConditionalEnchantmentEffect<EnchantmentValueEffect>] {
        match component {
            EnchantmentEffectComponent::EquipmentDrops => self.equipment_drops,
            _ => &[],
        }
    }

    #[must_use]
    pub fn single_value_effect(
        &self,
        component: EnchantmentEffectComponent,
    ) -> Option<&EnchantmentValueEffect> {
        match component {
            EnchantmentEffectComponent::CrossbowChargeTime => self.crossbow_charge_time.as_ref(),
            EnchantmentEffectComponent::TridentSpinAttackStrength => {
                self.trident_spin_attack_strength.as_ref()
            }
            _ => None,
        }
    }
}
