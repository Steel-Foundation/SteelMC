use steel_utils::random::Random;
use text_components::TextComponent;

use super::{
    DyeColor, EquipmentSlotGroup, Identifier, InstrumentRef, ItemStack, LootCondition, LootContext,
    LootContextEntity, LootEntry, LootError, LootResult, NumberProvider, REGISTRY, RegistryExt,
    TaggedRegistryExt, ToolPredicate, test_all,
};
use crate::enchantment::EnchantmentRef;

/// Options for selecting enchantments - either a tag reference or explicit list.
#[derive(Debug, Clone)]
pub enum EnchantmentOptions {
    /// Reference to an enchantment tag (e.g., "`on_random_loot`").
    Tag(Identifier),
    /// Explicit list of enchantment IDs.
    List(&'static [Identifier]),
}

impl EnchantmentOptions {
    pub(crate) fn resolve(&self) -> LootResult<Vec<EnchantmentRef>> {
        match self {
            Self::Tag(tag) => {
                REGISTRY
                    .enchantments
                    .get_tag(tag)
                    .ok_or_else(|| LootError::UnknownRegistryTag {
                        registry: "enchantment",
                        key: tag.clone(),
                    })
            }
            Self::List(keys) => keys
                .iter()
                .map(|key| {
                    REGISTRY.enchantments.by_key(key).ok_or_else(|| {
                        LootError::UnknownRegistryValue {
                            registry: "enchantment",
                            key: key.clone(),
                        }
                    })
                })
                .collect(),
        }
    }
}

/// Options for selecting an instrument from a registry tag or explicit list.
#[derive(Debug, Clone)]
pub enum InstrumentOptions {
    Tag(Identifier),
    Direct(&'static [InstrumentRef]),
}

impl InstrumentOptions {
    fn get_random<R: Random>(&self, rng: &mut R) -> LootResult<Option<InstrumentRef>> {
        Ok(match self {
            Self::Tag(tag) => {
                let instruments = REGISTRY.instruments.get_tag(tag).ok_or_else(|| {
                    LootError::UnknownRegistryTag {
                        registry: "instrument",
                        key: tag.clone(),
                    }
                })?;
                let Ok(bound) = i32::try_from(instruments.len()) else {
                    return Ok(None);
                };
                if bound == 0 {
                    return Ok(None);
                }
                let Ok(index) = usize::try_from(rng.next_i32_bounded(bound)) else {
                    return Ok(None);
                };
                instruments.get(index).copied()
            }
            Self::Direct(instruments) => {
                let Ok(bound) = i32::try_from(instruments.len()) else {
                    return Ok(None);
                };
                if bound == 0 {
                    return Ok(None);
                }
                let Ok(index) = usize::try_from(rng.next_i32_bounded(bound)) else {
                    return Ok(None);
                };
                instruments.get(index).copied()
            }
        })
    }
}

/// A function with optional conditions.
#[derive(Debug, Clone)]
pub struct ConditionalLootFunction {
    pub function: LootFunction,
    pub conditions: &'static [LootCondition],
}

/// A function that modifies loot items.
#[derive(Debug, Clone)]
pub enum LootFunction {
    /// Set the count of the item.
    SetCount { count: NumberProvider, add: bool },
    /// Apply explosion decay - each item has 1/radius chance to survive.
    ExplosionDecay,
    /// Apply bonus count based on enchantment level.
    ApplyBonus {
        enchantment: Identifier,
        formula: BonusFormula,
    },
    /// Increase count based on enchantment (like looting).
    EnchantedCountIncrease {
        enchantment: Identifier,
        count: NumberProvider,
        limit: i32,
    },
    /// Limit the count to a range.
    LimitCount { min: Option<i32>, max: Option<i32> },
    /// Set the damage of the item (0.0 = broken, 1.0 = full durability).
    SetDamage { damage: NumberProvider, add: bool },
    /// Enchant the item randomly with enchantments from options.
    EnchantRandomly { options: EnchantmentOptions },
    /// Enchant the item as if using an enchanting table at the specified level.
    EnchantWithLevels {
        levels: NumberProvider,
        options: EnchantmentOptions,
    },
    /// Copy components from the source to the item.
    ///
    /// `None` copies every component, mirroring Vanilla's optional `include` list.
    CopyComponents {
        source: CopySource,
        include: Option<&'static [Identifier]>,
    },
    /// Copy block state properties to the item.
    CopyState {
        block: Identifier,
        properties: &'static [&'static str],
    },
    /// Set components on the item.
    SetComponents { components: &'static str },
    /// Set custom NBT data on the item (merges with existing `custom_data`).
    SetCustomData {
        tag: fn() -> crate::data_components::CustomData,
    },
    /// Smelt the item (convert raw to cooked, ore to ingot, etc.).
    FurnaceSmelt { use_input_count: bool },
    /// Create an exploration map pointing to a structure.
    ExplorationMap {
        destination: Identifier,
        decoration: Identifier,
        zoom: i32,
        search_radius: i32,
        skip_existing_chunks: bool,
    },
    /// Set the custom name of the item.
    SetName { name: LootText, target: NameTarget },
    /// Set the ominous bottle amplifier.
    SetOminousBottleAmplifier { amplifier: NumberProvider },
    /// Set the potion type.
    SetPotion { id: Identifier },
    /// Set the suspicious stew effects.
    SetStewEffect { effects: &'static [StewEffect] },
    /// Set the instrument for goat horns.
    SetInstrument { options: InstrumentOptions },
    /// Set enchantments on the item.
    SetEnchantments {
        enchantments: &'static [(Identifier, NumberProvider)],
        add: bool,
    },
    /// Change the item type entirely.
    SetItem { item: Identifier },
    /// Copy name from source entity/block to item.
    CopyName { source: CopySource },
    /// Add lore lines to the item.
    SetLore {
        lore: &'static [&'static str],
        mode: ListOperation,
    },
    /// Set container inventory contents.
    SetContents {
        entries: &'static [LootEntry],
        component_type: Identifier,
    },
    /// Modify existing container contents.
    ModifyContents {
        modifier: &'static [ConditionalLootFunction],
        component_type: Identifier,
    },
    /// Set container's loot table reference.
    SetLootTable {
        loot_table: Identifier,
        seed: Option<i64>,
    },
    /// Set attribute modifiers on the item.
    SetAttributes {
        modifiers: &'static [AttributeModifier],
        replace: bool,
    },
    /// Fill player head with texture from entity.
    FillPlayerHead { entity: LootContextEntity },
    /// Copy NBT/custom data from source.
    CopyCustomData {
        source: CopySource,
        operations: &'static [CopyDataOperation],
    },
    /// Set banner pattern layers.
    SetBannerPattern {
        patterns: &'static [BannerPattern],
        append: bool,
    },
    /// Set firework rocket properties.
    SetFireworks {
        explosions: Option<&'static [FireworkExplosion]>,
        flight_duration: Option<i32>,
    },
    /// Set firework star explosion properties.
    SetFireworkExplosion { explosion: FireworkExplosion },
    /// Set book cover (title/author for written books).
    SetBookCover {
        title: Option<&'static str>,
        author: Option<&'static str>,
        generation: Option<i32>,
    },
    /// Set written book page contents.
    SetWrittenBookPages {
        pages: &'static [&'static str],
        mode: ListOperation,
    },
    /// Set writable book page contents.
    SetWritableBookPages {
        pages: &'static [&'static str],
        mode: ListOperation,
    },
    /// Toggle tooltip visibility.
    ToggleTooltips {
        toggles: &'static [(Identifier, bool)],
    },
    /// Discard/delete the item entirely.
    Discard,
    /// Reference to a named function in the registry.
    Reference(Identifier),
    /// Apply multiple functions in sequence.
    Sequence {
        functions: &'static [ConditionalLootFunction],
    },
    /// Conditionally apply function to specific item predicate matches.
    Filtered {
        item_filter: ToolPredicate,
        modifier: &'static ConditionalLootFunction,
    },
}

/// Operation mode for list modifications (lore, book pages).
#[derive(Debug, Clone, Copy)]
pub enum ListOperation {
    /// Replace all existing entries.
    ReplaceAll,
    /// Replace a section of entries.
    ReplaceSection { offset: i32, size: Option<i32> },
    /// Insert before existing entries.
    InsertBefore { offset: i32 },
    /// Insert after existing entries.
    InsertAfter { offset: i32 },
    /// Append to the end.
    Append,
}

/// An attribute modifier for `SetAttributes` function.
#[derive(Debug, Clone)]
pub struct AttributeModifier {
    pub attribute: Identifier,
    pub operation: AttributeOperation,
    pub amount: NumberProvider,
    pub id: Identifier,
    pub slot: EquipmentSlotGroup,
}

/// Attribute modifier operation type.
#[expect(clippy::enum_variant_names, reason = "matches Vanilla naming")]
#[derive(Debug, Clone, Copy)]
pub enum AttributeOperation {
    AddValue,
    AddMultipliedBase,
    AddMultipliedTotal,
}

/// Copy data operation for `CopyCustomData`.
#[derive(Debug, Clone)]
pub struct CopyDataOperation {
    pub source_path: &'static str,
    pub target_path: &'static str,
    pub op: CopyDataOp,
}

/// Operation type for data copying.
#[derive(Debug, Clone, Copy)]
pub enum CopyDataOp {
    Replace,
    Append,
    Merge,
}

/// A banner pattern layer.
#[derive(Debug, Clone)]
pub struct BannerPattern {
    pub pattern: Identifier,
    pub color: DyeColor,
}

/// A firework explosion definition.
#[derive(Debug, Clone)]
pub struct FireworkExplosion {
    pub shape: FireworkShape,
    pub colors: &'static [i32],
    pub fade_colors: &'static [i32],
    pub has_trail: bool,
    pub has_twinkle: bool,
}

/// Firework explosion shape.
#[derive(Debug, Clone, Copy)]
pub enum FireworkShape {
    SmallBall,
    LargeBall,
    Star,
    Creeper,
    Burst,
}

/// Formula types for `apply_bonus` function.
#[derive(Debug, Clone, Copy)]
pub enum BonusFormula {
    /// Ore drops formula: count * (max(0, random(0..level+2) - 1) + 1)
    OreDrops,
    /// Uniform bonus: count + random(0..bonusMultiplier * level + 1)
    UniformBonusCount { bonus_multiplier: i32 },
    /// Binomial with bonus count: for each of (level + extra) trials, probability p to add 1
    BinomialWithBonusCount { extra: i32, probability: f32 },
}

/// Source for copying components.
#[derive(Debug, Clone, Copy)]
pub enum CopySource {
    BlockEntity,
    This,
    Attacker,
    DirectAttacker,
}

/// Target for `set_name` function.
#[derive(Debug, Clone, Copy)]
pub enum NameTarget {
    CustomName,
    ItemName,
}

/// Text forms validated and compiled from Vanilla loot-table data.
#[derive(Debug, Clone, Copy)]
pub enum LootText {
    Translation(fn() -> TextComponent),
}

impl LootText {
    #[must_use]
    pub fn component(self) -> TextComponent {
        match self {
            Self::Translation(factory) => factory(),
        }
    }
}

/// A stew effect for suspicious stew.
#[derive(Debug, Clone)]
pub struct StewEffect {
    pub effect_type: Identifier,
    pub duration: NumberProvider,
}

impl LootFunction {
    /// Apply this function to modify the item stack.
    ///
    /// This modifies the item in place. Functions can change:
    /// - Count (`SetCount`, `ExplosionDecay`, `ApplyBonus`, etc.)
    /// - Damage/durability (`SetDamage`)
    /// - Enchantments (`EnchantRandomly`, `EnchantWithLevels`, `SetEnchantments`)
    /// - Components/NBT (`CopyComponents`, `SetComponents`, `CopyState`)
    /// - Item type (`FurnaceSmelt`)
    /// - And more...
    pub fn apply<R: Random>(
        &self,
        item: &mut ItemStack,
        ctx: &mut LootContext<'_, R>,
    ) -> LootResult<()> {
        match self {
            LootFunction::SetCount {
                count: provider,
                add,
            } => {
                let context = super::LootContextRef { tool: ctx.tool };
                let value = provider.get_int_with_ctx(ctx.rng, Some(&context))?;
                if *add {
                    item.count += value;
                } else {
                    item.count = value;
                }
            }
            LootFunction::ExplosionDecay => {
                if let Some(radius) = ctx.explosion_radius {
                    // Each item has 1/radius chance to survive
                    let probability = 1.0 / radius;
                    let mut result_count = 0;
                    for _ in 0..item.count {
                        if ctx.rng.next_f32() <= probability {
                            result_count += 1;
                        }
                    }
                    item.count = result_count;
                }
            }
            LootFunction::ApplyBonus {
                enchantment,
                formula,
            } => {
                let level = ctx.get_enchantment_level_by_id(enchantment);
                item.count = formula.apply(item.count, level, ctx.rng);
            }
            LootFunction::EnchantedCountIncrease {
                enchantment,
                count: provider,
                limit,
            } => {
                let level = ctx.get_enchantment_level_by_id(enchantment);
                if level > 0 {
                    let context = super::LootContextRef { tool: ctx.tool };
                    let sampled = provider.get(ctx.rng, Some(&context))? * level as f32;
                    let bonus = (sampled + 0.5).floor() as i32;
                    let bonus = if *limit > 0 { bonus.min(*limit) } else { bonus };
                    item.count += bonus;
                }
            }
            LootFunction::LimitCount { min, max } => {
                if let Some(min_val) = min {
                    item.count = item.count.max(*min_val);
                }
                if let Some(max_val) = max {
                    item.count = item.count.min(*max_val);
                }
            }
            LootFunction::SetDamage { damage, add } => {
                let context = super::LootContextRef { tool: ctx.tool };
                item.set_damage_fraction(damage.get(ctx.rng, Some(&context))?, *add);
            }
            LootFunction::EnchantRandomly { options } => {
                item.enchant_randomly(options, ctx.rng)?;
            }
            LootFunction::EnchantWithLevels { levels, options } => {
                let context = super::LootContextRef { tool: ctx.tool };
                let level = levels.get_int_with_ctx(ctx.rng, Some(&context))?;
                item.enchant_with_levels(level, options, ctx.rng)?;
            }
            LootFunction::CopyComponents { source, include } => {
                // Vanilla treats an absent optional source as a no-op.
                match source {
                    CopySource::BlockEntity => {
                        let Some(components) = ctx
                            .block_entity
                            .and_then(|block_entity| block_entity.components)
                        else {
                            return Ok(());
                        };
                        item.apply_components(
                            &components
                                .filter(|key| include.is_none_or(|include| include.contains(key))),
                        );
                    }
                    // TODO: Copy entity components once Steel entities expose a
                    // `DataComponentGetter` view like Vanilla `Entity`.
                    CopySource::This => {
                        if ctx.this_entity.is_some() {
                            return Err(LootError::UnsupportedFunction("copy_components"));
                        }
                    }
                    CopySource::Attacker => {
                        if ctx.killer_entity.is_some() {
                            return Err(LootError::UnsupportedFunction("copy_components"));
                        }
                    }
                    CopySource::DirectAttacker => {
                        if ctx.direct_killer_entity.is_some() {
                            return Err(LootError::UnsupportedFunction("copy_components"));
                        }
                    }
                }
            }
            LootFunction::CopyState { .. } => {
                return Err(LootError::UnsupportedFunction("copy_state"));
            }
            LootFunction::SetComponents { .. } => {
                return Err(LootError::UnsupportedFunction("set_components"));
            }
            LootFunction::SetCustomData { tag } => {
                item.set_custom_data(&tag());
            }
            LootFunction::FurnaceSmelt { use_input_count } => {
                item.apply_furnace_smelt(*use_input_count);
            }
            LootFunction::ExplorationMap {
                destination,
                decoration,
                zoom,
                search_radius,
                skip_existing_chunks,
            } => {
                if item.is(&crate::vanilla_items::MAP) && ctx.origin.is_some() {
                    let request = super::ExplorationMapRequest {
                        destination: destination.clone(),
                        decoration: decoration.clone(),
                        zoom: *zoom,
                        search_radius: *search_radius,
                        skip_existing_chunks: *skip_existing_chunks,
                    };
                    let Some(resolver) = ctx.exploration_maps.as_deref_mut() else {
                        return Err(LootError::ExplorationMapRequired(request));
                    };
                    if let Some(map) = resolver.resolve(&request, item)? {
                        *item = map;
                    }
                }
            }
            LootFunction::SetName { name, target } => {
                item.set_name(name.component(), *target);
            }
            LootFunction::SetOminousBottleAmplifier { amplifier } => {
                let context = super::LootContextRef { tool: ctx.tool };
                let amp = amplifier.get_int_with_ctx(ctx.rng, Some(&context))?.clamp(
                    crate::data_components::OminousBottleAmplifier::MIN_AMPLIFIER,
                    crate::data_components::OminousBottleAmplifier::MAX_AMPLIFIER,
                );
                item.set_ominous_bottle_amplifier(amp);
            }
            LootFunction::SetPotion { id } => {
                item.set_potion(id)?;
            }
            LootFunction::SetStewEffect { effects } => {
                item.set_stew_effects(effects, ctx)?;
            }
            LootFunction::SetInstrument { options } => {
                if let Some(instrument) = options.get_random(ctx.rng)? {
                    item.set(
                        crate::data_components::vanilla_components::INSTRUMENT,
                        crate::data_components::InstrumentComponent::new(
                            crate::RegistryHolder::reference(instrument),
                        ),
                    );
                }
            }
            LootFunction::SetEnchantments { enchantments, add } => {
                if item.is(&crate::vanilla_items::BOOK) {
                    item.set_item(&crate::vanilla_items::ENCHANTED_BOOK.key);
                }
                let mut resolved = Vec::with_capacity(enchantments.len());
                for (key, provider) in *enchantments {
                    if REGISTRY.enchantments.by_key(key).is_none() {
                        return Err(LootError::UnknownRegistryValue {
                            registry: "enchantment",
                            key: key.clone(),
                        });
                    }
                    let context = super::LootContextRef { tool: ctx.tool };
                    let provided = provider.get_int_with_ctx(ctx.rng, Some(&context))?;
                    let level = if *add {
                        let existing = item
                            .get_enchantments_for_crafting()
                            .map_or(0, |enchantments| enchantments.get_level(key));
                        (existing as i32).wrapping_add(provided)
                    } else {
                        provided
                    };
                    resolved.push((key.clone(), level.clamp(0, 255) as u32));
                }
                item.set_enchantments(&resolved, false);
            }
            LootFunction::SetItem { item: new_item } => {
                if REGISTRY.items.by_key(new_item).is_none() {
                    return Err(LootError::UnknownRegistryValue {
                        registry: "item",
                        key: new_item.clone(),
                    });
                }
                item.set_item(new_item);
            }
            LootFunction::CopyName { .. } => {
                return Err(LootError::UnsupportedFunction("copy_name"));
            }
            LootFunction::SetLore { .. } => {
                return Err(LootError::UnsupportedFunction("set_lore"));
            }
            LootFunction::SetContents { .. } => {
                return Err(LootError::UnsupportedFunction("set_contents"));
            }
            LootFunction::ModifyContents { .. } => {
                return Err(LootError::UnsupportedFunction("modify_contents"));
            }
            LootFunction::SetLootTable { .. } => {
                return Err(LootError::UnsupportedFunction("set_loot_table"));
            }
            LootFunction::SetAttributes { .. } => {
                return Err(LootError::UnsupportedFunction("set_attributes"));
            }
            LootFunction::FillPlayerHead { .. } => {
                return Err(LootError::UnsupportedFunction("fill_player_head"));
            }
            LootFunction::CopyCustomData { .. } => {
                return Err(LootError::UnsupportedFunction("copy_custom_data"));
            }
            LootFunction::SetBannerPattern { .. } => {
                return Err(LootError::UnsupportedFunction("set_banner_pattern"));
            }
            LootFunction::SetFireworks { .. } => {
                return Err(LootError::UnsupportedFunction("set_fireworks"));
            }
            LootFunction::SetFireworkExplosion { .. } => {
                return Err(LootError::UnsupportedFunction("set_firework_explosion"));
            }
            LootFunction::SetBookCover { .. } => {
                return Err(LootError::UnsupportedFunction("set_book_cover"));
            }
            LootFunction::SetWrittenBookPages { .. } => {
                return Err(LootError::UnsupportedFunction("set_written_book_pages"));
            }
            LootFunction::SetWritableBookPages { .. } => {
                return Err(LootError::UnsupportedFunction("set_writable_book_pages"));
            }
            LootFunction::ToggleTooltips { .. } => {
                return Err(LootError::UnsupportedFunction("toggle_tooltips"));
            }
            LootFunction::Discard => {
                item.count = 0;
            }
            LootFunction::Reference(_) => {
                return Err(LootError::UnsupportedFunction("reference"));
            }
            LootFunction::Sequence { functions } => {
                for cond_func in *functions {
                    if test_all(cond_func.conditions, ctx)? {
                        cond_func.function.apply(item, ctx)?;
                    }
                }
            }
            LootFunction::Filtered {
                item_filter,
                modifier,
            } => {
                if item_filter.test(item, ctx) && test_all(modifier.conditions, ctx)? {
                    modifier.function.apply(item, ctx)?;
                }
            }
        }
        Ok(())
    }
}

impl BonusFormula {
    /// Apply the bonus formula to calculate new count.
    pub fn apply<R: Random>(&self, count: i32, level: i32, rng: &mut R) -> i32 {
        match self {
            BonusFormula::OreDrops => {
                if level > 0 {
                    // Vanilla: count * (max(0, random(0..level+2) - 1) + 1)
                    let bonus = rng.next_i32_bounded(level + 2) - 1;
                    let multiplier = bonus.max(0) + 1;
                    count * multiplier
                } else {
                    count
                }
            }
            BonusFormula::UniformBonusCount { bonus_multiplier } => {
                // Vanilla: count + random(0..bonusMultiplier * level + 1)
                if level > 0 {
                    count + rng.next_i32_bounded(bonus_multiplier * level + 1)
                } else {
                    count
                }
            }
            BonusFormula::BinomialWithBonusCount { extra, probability } => {
                // Vanilla: for each of (level + extra) trials, probability p to add 1
                let trials = level + extra;
                let mut bonus = 0;
                for _ in 0..trials {
                    if rng.next_f32() < *probability {
                        bonus += 1;
                    }
                }
                count + bonus
            }
        }
    }
}
