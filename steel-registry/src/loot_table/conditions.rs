use steel_utils::random::Random;
use steel_utils::{BlockPos, BlockStateId};

use super::{
    BlockStateExt, DamageSourceInfo, DyeColor, EntityEquipmentRef, EntityRef, EntityRefFlags,
    Identifier, ItemStack, LootContext, LootContextEntity, LootError, LootResult, NumberProvider,
    NumberProviderRange, REGISTRY, RegistryExt, TaggedRegistryExt,
};
use crate::RegistryHolderSet;
use crate::biome::BiomeRef;
use crate::item_predicate::BlockPredicate;

/// A property check for block state conditions.
#[derive(Debug, Clone)]
pub struct PropertyCheck {
    pub name: &'static str,
    pub value: &'static str,
}

/// A condition that must be met for a loot entry or pool to apply.
#[derive(Debug, Clone)]
#[expect(clippy::large_enum_variant)]
pub enum LootCondition {
    /// The loot survives explosion damage (random chance based on explosion radius).
    /// Vanilla: 1/radius chance to pass. If no explosion, always passes.
    SurvivesExplosion,
    /// Check block state properties match expected values.
    BlockStateProperty {
        block: Identifier,
        properties: &'static [PropertyCheck],
    },
    /// Simple random chance (0.0 to 1.0).
    RandomChance(f32),
    /// Random chance affected by an enchantment (e.g., looting).
    /// Vanilla 1.21+: uses `enchanted_chance` which can be constant or linear.
    RandomChanceWithEnchantedBonus {
        enchantment: Identifier,
        unenchanted_chance: f32,
        /// For linear formula: chance = base + `per_level` * (level - 1)
        enchanted_chance: EnchantedChance,
    },
    /// Match tool predicate - checks if the tool matches certain criteria.
    MatchTool(ToolPredicate),
    /// Table bonus condition - chance based on enchantment level from a table.
    /// The chances array is indexed by enchantment level (0 = no enchant, 1 = level 1, etc.)
    TableBonus {
        enchantment: Identifier,
        chances: &'static [f32],
    },
    /// Inverted condition - passes if the inner condition fails.
    Inverted(&'static LootCondition),
    /// Any of the conditions pass (OR logic).
    AnyOf(&'static [LootCondition]),
    /// All of the conditions pass (AND logic).
    AllOf(&'static [LootCondition]),
    /// Killed by player condition.
    KilledByPlayer,
    /// Entity properties condition - checks entity predicates.
    EntityProperties {
        entity: LootContextEntity,
        predicate: EntityPredicate,
    },
    /// Damage source properties condition - checks how the entity was damaged.
    DamageSourceProperties { predicate: DamageSourcePredicate },
    /// Location check condition - checks the location predicate.
    LocationCheck {
        offset_x: i32,
        offset_y: i32,
        offset_z: i32,
        predicate: LocationPredicate,
    },
    /// Weather check condition - checks current weather.
    WeatherCheck {
        raining: Option<bool>,
        thundering: Option<bool>,
    },
    /// Time check condition - checks game time.
    TimeCheck {
        value: NumberProviderRange,
        period: Option<i64>,
    },
    /// Value check condition - compares a number provider value against a range.
    ValueCheck {
        value: NumberProvider,
        range: NumberProviderRange,
    },
    /// Check if a specific enchantment is active.
    EnchantmentActiveCheck {
        enchantment: Identifier,
        active: bool,
    },
    /// Check entity scoreboard scores.
    EntityScores {
        entity: LootContextEntity,
        scores: &'static [(&'static str, NumberProviderRange)],
    },
    /// Reference to a named condition in the registry.
    Reference(Identifier),
}

/// Enchanted chance calculation method.
#[derive(Debug, Clone, Copy)]
pub enum EnchantedChance {
    /// Constant chance regardless of enchantment level.
    Constant(f32),
    /// Linear formula: base + `per_level_above_first` * (level - 1)
    Linear {
        base: f32,
        per_level_above_first: f32,
    },
}

/// Predicate for matching tools.
#[derive(Debug, Clone)]
pub enum ToolPredicate {
    /// Match a specific item.
    Item(Identifier),
    /// Match items with a specific enchantment at minimum level.
    HasEnchantment {
        enchantment: Identifier,
        min_level: i32,
    },
    /// Match items in a tag.
    Tag(Identifier),
    /// Always matches (no predicate specified).
    Any,
}

/// Predicate for checking location/block properties.
#[derive(Debug, Clone)]
pub struct LocationPredicate {
    pub biomes: Option<BiomeOptions>,
    /// Canonical Vanilla block predicate. Loot evaluation currently supports
    /// its holder-set and state-property portions; preflight rejects NBT and
    /// component matchers until location access exposes block-entity data.
    pub block: Option<BlockPredicate>,
}

/// Biomes accepted by a location predicate.
#[derive(Debug, Clone)]
pub enum BiomeOptions {
    Tag(Identifier),
    List(&'static [Identifier]),
}

impl BiomeOptions {
    pub(crate) fn validate(&self) -> LootResult<()> {
        match self {
            Self::Tag(tag) => REGISTRY.biomes.get_tag(tag).map(|_| ()).ok_or_else(|| {
                LootError::UnknownRegistryTag {
                    registry: "biome",
                    key: tag.clone(),
                }
            }),
            Self::List(keys) => {
                for key in *keys {
                    if REGISTRY.biomes.by_key(key).is_none() {
                        return Err(LootError::UnknownRegistryValue {
                            registry: "biome",
                            key: key.clone(),
                        });
                    }
                }
                Ok(())
            }
        }
    }

    fn contains(&self, biome: BiomeRef) -> LootResult<bool> {
        match self {
            Self::Tag(tag) => REGISTRY.biomes.get_tag(tag).map_or_else(
                || {
                    Err(LootError::UnknownRegistryTag {
                        registry: "biome",
                        key: tag.clone(),
                    })
                },
                |biomes| {
                    Ok(biomes
                        .iter()
                        .any(|candidate| std::ptr::eq(*candidate, biome)))
                },
            ),
            Self::List(keys) => Ok(keys.contains(&biome.key)),
        }
    }
}

pub(crate) fn validate_block_predicate(predicate: &BlockPredicate) -> LootResult<()> {
    if predicate.nbt().is_some() {
        return Err(LootError::UnsupportedCondition(
            "location_check block NBT predicate",
        ));
    }
    if !predicate.components().is_empty() {
        return Err(LootError::UnsupportedCondition(
            "location_check block component predicate",
        ));
    }

    match predicate.blocks() {
        Some(RegistryHolderSet::Tag(tag)) if REGISTRY.blocks.get_tag(tag).is_none() => {
            return Err(LootError::UnknownRegistryTag {
                registry: "block",
                key: tag.clone(),
            });
        }
        Some(RegistryHolderSet::Direct(blocks)) => {
            for block in blocks.iter() {
                if REGISTRY
                    .blocks
                    .by_key(&block.key)
                    .is_none_or(|registered| !std::ptr::eq(registered, *block))
                {
                    return Err(LootError::UnknownRegistryValue {
                        registry: "block",
                        key: block.key.clone(),
                    });
                }
            }
        }
        Some(RegistryHolderSet::Tag(_)) | None => {}
    }
    Ok(())
}

fn block_predicate_matches(predicate: &BlockPredicate, state: BlockStateId) -> LootResult<bool> {
    validate_block_predicate(predicate)?;
    Ok(predicate.matches_state(state))
}

/// Predicate for checking entity properties.
#[derive(Debug, Clone)]
pub struct EntityPredicate {
    pub entity_type: Option<Identifier>,
    pub flags: Option<EntityFlags>,
    pub equipment: Option<EntityEquipment>,
    /// Vanilla `minecraft:components.sheep/color` entity data component check.
    pub sheep_color: Option<DyeColor>,
    /// Vanilla `minecraft:type_specific/sheep.sheared` check.
    pub sheep_sheared: Option<bool>,
    /// Vanilla `minecraft:components.chicken/variant` entity data component check.
    pub chicken_variant: Option<Identifier>,
}

/// Entity flags (`is_on_fire`, `is_sneaking`, etc.)
#[derive(Debug, Clone)]
pub struct EntityFlags {
    pub is_on_fire: Option<bool>,
    pub is_sneaking: Option<bool>,
    pub is_sprinting: Option<bool>,
    pub is_swimming: Option<bool>,
    pub is_baby: Option<bool>,
}

/// Entity equipment predicate
#[derive(Debug, Clone)]
pub struct EntityEquipment {
    pub mainhand: Option<ToolPredicate>,
    pub offhand: Option<ToolPredicate>,
    pub head: Option<ToolPredicate>,
    pub chest: Option<ToolPredicate>,
    pub legs: Option<ToolPredicate>,
    pub feet: Option<ToolPredicate>,
}

/// Predicate for checking damage source properties.
#[derive(Debug, Clone)]
pub struct DamageSourcePredicate {
    /// Tags that must be present on the damage source.
    pub tags: &'static [DamageTagPredicate],
    /// Source entity predicate (e.g., the player/mob that caused damage).
    pub source_entity: Option<EntityPredicate>,
    /// Direct entity predicate (e.g., the arrow/fireball).
    pub direct_entity: Option<EntityPredicate>,
    /// Whether damage bypasses armor.
    pub is_direct: Option<bool>,
}

/// A tag check for damage source.
#[derive(Debug, Clone)]
pub struct DamageTagPredicate {
    pub id: Identifier,
    pub expected: bool,
}

impl LootCondition {
    /// Test if this condition passes given the loot context.
    pub fn test<R: Random>(&self, ctx: &mut LootContext<'_, R>) -> LootResult<bool> {
        Ok(match self {
            LootCondition::SurvivesExplosion => {
                if let Some(radius) = ctx.explosion_radius {
                    // Vanilla: 1/radius chance to survive
                    ctx.rng.next_f32() <= (1.0 / radius)
                } else {
                    true // No explosion, always survives
                }
            }
            LootCondition::BlockStateProperty { block, properties } => {
                if let Some(state) = ctx.block_state {
                    let state_block = state.get_block();
                    // Check block matches
                    if state_block.key != *block {
                        return Ok(false);
                    }
                    // Check all properties match
                    for prop in *properties {
                        if let Some(value) = state.get_property_str(prop.name) {
                            if value != prop.value {
                                return Ok(false);
                            }
                        } else {
                            return Ok(false); // Property doesn't exist
                        }
                    }
                    true
                } else {
                    false // No block state in context
                }
            }
            LootCondition::RandomChance(chance) => ctx.rng.next_f32() < *chance,
            LootCondition::RandomChanceWithEnchantedBonus {
                enchantment,
                unenchanted_chance,
                enchanted_chance,
            } => {
                let level = ctx.get_enchantment_level_by_id(enchantment);
                let effective_chance = if level > 0 {
                    match enchanted_chance {
                        EnchantedChance::Constant(c) => *c,
                        EnchantedChance::Linear {
                            base,
                            per_level_above_first,
                        } => base + per_level_above_first * (level - 1) as f32,
                    }
                } else {
                    *unenchanted_chance
                };
                ctx.rng.next_f32() < effective_chance
            }
            LootCondition::MatchTool(predicate) => {
                if let Some(tool) = ctx.tool {
                    predicate.test(tool, ctx)
                } else {
                    // No tool in context - only passes if predicate is Any
                    matches!(predicate, ToolPredicate::Any)
                }
            }
            LootCondition::TableBonus {
                enchantment,
                chances,
            } => {
                let level = ctx.get_enchantment_level_by_id(enchantment);
                let index = (level as usize).min(chances.len().saturating_sub(1));
                let chance = chances.get(index).copied().unwrap_or(0.0);
                ctx.rng.next_f32() < chance
            }
            LootCondition::Inverted(inner) => !inner.test(ctx)?,
            LootCondition::AnyOf(conditions) => {
                let mut matched = false;
                for condition in *conditions {
                    if condition.test(ctx)? {
                        matched = true;
                        break;
                    }
                }
                matched
            }
            LootCondition::AllOf(conditions) => test_all(conditions, ctx)?,
            LootCondition::KilledByPlayer => ctx.killed_by_player,
            LootCondition::EntityProperties { entity, predicate } => {
                let Some(entity) = ctx.get_entity(*entity) else {
                    return Ok(false);
                };
                predicate.test(entity, ctx)
            }
            LootCondition::DamageSourceProperties { predicate } => predicate.test(ctx),
            LootCondition::LocationCheck {
                offset_x,
                offset_y,
                offset_z,
                predicate,
            } => {
                let Some((x, y, z)) = ctx.origin else {
                    return Ok(false);
                };
                let pos = BlockPos::new(
                    (x.floor() as i32).wrapping_add(*offset_x),
                    (y.floor() as i32).wrapping_add(*offset_y),
                    (z.floor() as i32).wrapping_add(*offset_z),
                );
                if let Some(biomes) = &predicate.biomes {
                    let level = ctx
                        .level
                        .ok_or(LootError::MissingLevel("location_check biome predicate"))?;
                    let Some(biome) = level.biome_at(pos) else {
                        return Ok(false);
                    };
                    if !biomes.contains(biome)? {
                        return Ok(false);
                    }
                }
                if let Some(block) = &predicate.block {
                    let level = ctx
                        .level
                        .ok_or(LootError::MissingLevel("location_check block predicate"))?;
                    let Some(state) = level.block_state_at(pos) else {
                        return Ok(false);
                    };
                    if !block_predicate_matches(block, state)? {
                        return Ok(false);
                    }
                }
                true
            }
            LootCondition::WeatherCheck {
                raining,
                thundering,
            } => {
                let weather = ctx.weather.unwrap_or_default();
                raining.is_none_or(|r| r == weather.raining)
                    && thundering.is_none_or(|t| t == weather.thundering)
            }
            LootCondition::TimeCheck { value, period } => {
                let game_time = ctx.game_time.unwrap_or(0);
                let time = if let Some(p) = period {
                    game_time % p
                } else {
                    game_time
                };
                let context = super::LootContextRef { tool: ctx.tool };
                value.test(time as f32, ctx.rng, Some(&context))?
            }
            LootCondition::ValueCheck { value, range } => {
                let context = super::LootContextRef { tool: ctx.tool };
                let value = value.get(ctx.rng, Some(&context))?;
                range.test(value, ctx.rng, Some(&context))?
            }
            LootCondition::EnchantmentActiveCheck {
                enchantment,
                active,
            } => {
                let level = ctx.get_enchantment_level_by_id(enchantment);
                let is_active = level > 0;
                is_active == *active
            }
            LootCondition::EntityScores { .. } => {
                return Err(LootError::UnsupportedCondition("entity_scores"));
            }
            LootCondition::Reference(_) => {
                return Err(LootError::UnsupportedCondition("reference"));
            }
        })
    }
}

pub(super) fn test_all<R: Random>(
    conditions: &[LootCondition],
    ctx: &mut LootContext<'_, R>,
) -> LootResult<bool> {
    for condition in conditions {
        if !condition.test(ctx)? {
            return Ok(false);
        }
    }
    Ok(true)
}

impl ToolPredicate {
    /// Test if the tool matches this predicate.
    #[must_use]
    pub fn test<R: Random>(&self, tool: &ItemStack, _ctx: &LootContext<'_, R>) -> bool {
        match self {
            ToolPredicate::Item(item_id) => tool.item.key == *item_id,
            ToolPredicate::HasEnchantment {
                enchantment,
                min_level,
            } => tool_enchantment_matches(tool, enchantment, *min_level),
            ToolPredicate::Tag(tag) => {
                // Check if the tool's item is in the specified tag
                REGISTRY.items.is_in_tag(tool.item, tag)
            }
            ToolPredicate::Any => true,
        }
    }
}

fn tool_enchantment_matches(
    tool: &ItemStack,
    enchantment_or_tag: &Identifier,
    min_level: i32,
) -> bool {
    if tool.get_enchantment_level(enchantment_or_tag) >= min_level {
        return true;
    }

    let Some(enchantments) = tool.get_enchantments() else {
        return false;
    };

    for (key, level) in enchantments.iter() {
        if *level < min_level as u32 {
            continue;
        }
        let Some(enchantment) = REGISTRY.enchantments.by_key(key) else {
            continue;
        };
        if REGISTRY
            .enchantments
            .is_in_tag(enchantment, enchantment_or_tag)
        {
            return true;
        }
    }

    false
}

impl EntityPredicate {
    fn test<R: Random>(&self, entity: EntityRef<'_>, ctx: &LootContext<'_, R>) -> bool {
        if let Some(entity_type) = &self.entity_type
            && entity.entity_type != Some(entity_type)
        {
            return false;
        }

        if let Some(flags) = &self.flags
            && !flags.test(entity.flags)
        {
            return false;
        }

        if let Some(equipment) = &self.equipment
            && !equipment.test(entity.equipment, ctx)
        {
            return false;
        }

        if let Some(expected_color) = &self.sheep_color
            && entity.sheep_color != Some(*expected_color)
        {
            return false;
        }

        if let Some(expected_sheared) = &self.sheep_sheared
            && entity.sheep_sheared != Some(*expected_sheared)
        {
            return false;
        }

        if let Some(expected_variant) = &self.chicken_variant
            && entity.chicken_variant != Some(expected_variant)
        {
            return false;
        }

        true
    }
}

impl EntityFlags {
    fn test(&self, flags: EntityRefFlags) -> bool {
        self.is_on_fire
            .is_none_or(|expected| expected == flags.is_on_fire)
            && self
                .is_sneaking
                .is_none_or(|expected| expected == flags.is_sneaking)
            && self
                .is_sprinting
                .is_none_or(|expected| expected == flags.is_sprinting)
            && self
                .is_swimming
                .is_none_or(|expected| expected == flags.is_swimming)
            && self
                .is_baby
                .is_none_or(|expected| expected == flags.is_baby)
    }
}

impl EntityEquipment {
    fn test<R: Random>(
        &self,
        equipment: Option<&EntityEquipmentRef<'_>>,
        ctx: &LootContext<'_, R>,
    ) -> bool {
        let has_predicate = self.mainhand.is_some()
            || self.offhand.is_some()
            || self.head.is_some()
            || self.chest.is_some()
            || self.legs.is_some()
            || self.feet.is_some();
        if !has_predicate {
            return true;
        }

        let Some(equipment) = equipment else {
            return false;
        };

        slot_predicate_matches(&self.mainhand, equipment.mainhand, ctx)
            && slot_predicate_matches(&self.offhand, equipment.offhand, ctx)
            && slot_predicate_matches(&self.head, equipment.head, ctx)
            && slot_predicate_matches(&self.chest, equipment.chest, ctx)
            && slot_predicate_matches(&self.legs, equipment.legs, ctx)
            && slot_predicate_matches(&self.feet, equipment.feet, ctx)
    }
}

fn slot_predicate_matches<R: Random>(
    predicate: &Option<ToolPredicate>,
    item_stack: Option<&ItemStack>,
    ctx: &LootContext<'_, R>,
) -> bool {
    let Some(predicate) = predicate else {
        return true;
    };
    let Some(item_stack) = item_stack else {
        return false;
    };
    predicate.test(item_stack, ctx)
}

impl DamageSourcePredicate {
    fn test<R: Random>(&self, ctx: &LootContext<'_, R>) -> bool {
        let Some(damage_source) = ctx.damage_source else {
            return false;
        };

        for tag in self.tags {
            if damage_source_has_tag(damage_source, &tag.id) != tag.expected {
                return false;
            }
        }

        if let Some(expected) = self.is_direct
            && damage_source.is_direct != expected
        {
            return false;
        }

        if let Some(predicate) = &self.source_entity {
            let Some(entity) = ctx.killer_entity else {
                return false;
            };
            if !predicate.test(entity, ctx) {
                return false;
            }
        }

        if let Some(predicate) = &self.direct_entity {
            let Some(entity) = ctx.direct_killer_entity else {
                return false;
            };
            if !predicate.test(entity, ctx) {
                return false;
            }
        }

        true
    }
}

fn damage_source_has_tag(damage_source: DamageSourceInfo<'_>, tag: &Identifier) -> bool {
    if let Some(damage_type) = damage_source.damage_type
        && let Some(damage_type) = REGISTRY.damage_types.by_key(damage_type)
    {
        return REGISTRY.damage_types.is_in_tag(damage_type, tag);
    }

    damage_source.tags.iter().any(|candidate| candidate == tag)
}
