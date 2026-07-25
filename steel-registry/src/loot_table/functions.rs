use super::{
    DyeColor, EquipmentSlotGroup, Identifier, InstrumentRef, ItemStack, LootCondition, LootContext,
    LootContextEntity, LootEntry, NumberProvider, REGISTRY, RngExt, TaggedRegistryExt,
    ToolPredicate,
};

/// Options for selecting enchantments - either a tag reference or explicit list.
#[derive(Debug, Clone)]
pub enum EnchantmentOptions {
    /// Reference to an enchantment tag (e.g., "`on_random_loot`").
    Tag(Identifier),
    /// Explicit list of enchantment IDs.
    List(&'static [Identifier]),
}

/// Options for selecting an instrument from a registry tag or explicit list.
#[derive(Debug, Clone)]
pub enum InstrumentOptions {
    Tag(Identifier),
    Direct(&'static [InstrumentRef]),
}

impl InstrumentOptions {
    fn get_random<R: rand::Rng>(&self, rng: &mut R) -> Option<InstrumentRef> {
        match self {
            Self::Tag(tag) => {
                let instruments = REGISTRY.instruments.get_tag(tag)?;
                (!instruments.is_empty()).then(|| {
                    let index = rng.random_range(0..instruments.len());
                    instruments[index]
                })
            }
            Self::Direct(instruments) => (!instruments.is_empty()).then(|| {
                let index = rng.random_range(0..instruments.len());
                instruments[index]
            }),
        }
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
    /// Copy components from the block entity to the item.
    CopyComponents {
        source: CopySource,
        include: &'static [Identifier],
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
        skip_existing_chunks: bool,
    },
    /// Set the custom name of the item.
    SetName {
        name: &'static str,
        target: NameTarget,
    },
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
    pub fn apply<R: rand::Rng>(&self, item: &mut ItemStack, ctx: &mut LootContext<'_, R>) {
        match self {
            LootFunction::SetCount {
                count: provider,
                add,
            } => {
                let value = provider.get_int(ctx.rng);
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
                        if ctx.rng.random::<f32>() <= probability {
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
                    let bonus = (provider.get_simple(ctx.rng) * level as f32).round() as i32;
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
                item.set_damage_fraction(damage.get_simple(ctx.rng), *add);
            }
            LootFunction::EnchantRandomly { options } => {
                // TODO: Implement when enchantment system is ready
                item.enchant_randomly(options, ctx.rng);
            }
            LootFunction::EnchantWithLevels { levels, options } => {
                // TODO: Implement when enchantment system is ready
                let level = levels.get_int(ctx.rng);
                item.enchant_with_levels(level, options, ctx.rng);
            }
            LootFunction::CopyComponents { source, include } => {
                // TODO: Implement when block entity system is ready
                item.copy_components(*source, include, ctx);
            }
            LootFunction::CopyState { block, properties } => {
                // TODO: Implement block state copying
                item.copy_block_state(block, properties, ctx);
            }
            LootFunction::SetComponents { components } => {
                // TODO: Implement component setting from JSON
                item.set_components_from_json(components);
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
                skip_existing_chunks,
            } => {
                // TODO: Implement exploration map creation
                item.create_exploration_map(destination, decoration, *zoom, *skip_existing_chunks);
            }
            LootFunction::SetName { name, target } => {
                // TODO: Implement name setting
                item.set_name(name, *target);
            }
            LootFunction::SetOminousBottleAmplifier { amplifier } => {
                let amp = amplifier.get_int(ctx.rng).clamp(
                    crate::data_components::OminousBottleAmplifier::MIN_AMPLIFIER,
                    crate::data_components::OminousBottleAmplifier::MAX_AMPLIFIER,
                );
                item.set_ominous_bottle_amplifier(amp);
            }
            LootFunction::SetPotion { id } => {
                item.set_potion(id);
            }
            LootFunction::SetStewEffect { effects } => {
                item.set_stew_effects(effects, ctx.rng);
            }
            LootFunction::SetInstrument { options } => {
                if let Some(instrument) = options.get_random(ctx.rng) {
                    item.set(
                        crate::data_components::vanilla_components::INSTRUMENT,
                        crate::data_components::InstrumentComponent::new(
                            crate::RegistryHolder::reference(instrument),
                        ),
                    );
                }
            }
            LootFunction::SetEnchantments { enchantments, add } => {
                let resolved: Vec<_> = enchantments
                    .iter()
                    .map(|(key, provider)| (key.clone(), provider.get_int(ctx.rng).max(0) as u32))
                    .collect();
                item.set_enchantments(&resolved, *add);
            }
            LootFunction::SetItem { item: new_item } => {
                item.set_item(new_item);
            }
            LootFunction::CopyName { source } => {
                item.copy_name(*source, ctx);
            }
            LootFunction::SetLore { lore, mode } => {
                item.set_lore(lore, *mode);
            }
            LootFunction::SetContents {
                entries,
                component_type,
            } => {
                item.set_contents(entries, component_type, ctx);
            }
            LootFunction::ModifyContents {
                modifier,
                component_type,
            } => {
                item.modify_contents(modifier, component_type, ctx);
            }
            LootFunction::SetLootTable { loot_table, seed } => {
                item.set_loot_table(loot_table, *seed);
            }
            LootFunction::SetAttributes { modifiers, replace } => {
                item.set_attributes(modifiers, *replace, ctx.rng);
            }
            LootFunction::FillPlayerHead { entity } => {
                item.fill_player_head(*entity, ctx);
            }
            LootFunction::CopyCustomData { source, operations } => {
                item.copy_custom_data(*source, operations, ctx);
            }
            LootFunction::SetBannerPattern { patterns, append } => {
                item.set_banner_pattern(patterns, *append);
            }
            LootFunction::SetFireworks {
                explosions,
                flight_duration,
            } => {
                item.set_fireworks(*explosions, *flight_duration);
            }
            LootFunction::SetFireworkExplosion { explosion } => {
                item.set_firework_explosion(explosion);
            }
            LootFunction::SetBookCover {
                title,
                author,
                generation,
            } => {
                item.set_book_cover(*title, *author, *generation);
            }
            LootFunction::SetWrittenBookPages { pages, mode } => {
                item.set_written_book_pages(pages, *mode);
            }
            LootFunction::SetWritableBookPages { pages, mode } => {
                item.set_writable_book_pages(pages, *mode);
            }
            LootFunction::ToggleTooltips { toggles } => {
                item.toggle_tooltips(toggles);
            }
            LootFunction::Discard => {
                item.count = 0;
            }
            LootFunction::Reference(_name) => {
                // TODO: Implement function registry lookup
            }
            LootFunction::Sequence { functions } => {
                for cond_func in *functions {
                    if cond_func.conditions.iter().all(|c| c.test(ctx)) {
                        cond_func.function.apply(item, ctx);
                    }
                }
            }
            LootFunction::Filtered {
                item_filter,
                modifier,
            } => {
                if item_filter.test(item, ctx) && modifier.conditions.iter().all(|c| c.test(ctx)) {
                    modifier.function.apply(item, ctx);
                }
            }
        }
    }
}

impl BonusFormula {
    /// Apply the bonus formula to calculate new count.
    pub fn apply<R: rand::Rng>(&self, count: i32, level: i32, rng: &mut R) -> i32 {
        match self {
            BonusFormula::OreDrops => {
                if level > 0 {
                    // Vanilla: count * (max(0, random(0..level+2) - 1) + 1)
                    let bonus = rng.random_range(0..level + 2) - 1;
                    let multiplier = bonus.max(0) + 1;
                    count * multiplier
                } else {
                    count
                }
            }
            BonusFormula::UniformBonusCount { bonus_multiplier } => {
                // Vanilla: count + random(0..bonusMultiplier * level + 1)
                if level > 0 {
                    count + rng.random_range(0..bonus_multiplier * level + 1)
                } else {
                    count
                }
            }
            BonusFormula::BinomialWithBonusCount { extra, probability } => {
                // Vanilla: for each of (level + extra) trials, probability p to add 1
                let trials = level + extra;
                let mut bonus = 0;
                for _ in 0..trials {
                    if rng.random::<f32>() < *probability {
                        bonus += 1;
                    }
                }
                count + bonus
            }
        }
    }
}
