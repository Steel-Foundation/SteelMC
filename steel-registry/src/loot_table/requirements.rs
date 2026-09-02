use rustc_hash::FxHashSet;

use super::conditions::validate_block_predicate;
use super::{
    ConditionalLootFunction, CopySource, EnchantmentOptions, ExplorationMapRequest,
    InstrumentOptions, LootCondition, LootEntry, LootError, LootFunction, LootResult, LootTable,
    NumberProvider, NumberProviderRange, REGISTRY, RegistryExt, TaggedRegistryExt,
};

/// World work that must be resolved before a loot table can be evaluated.
#[derive(Debug, Clone, Default)]
pub struct LootRequirements {
    exploration_maps: Vec<ExplorationMapRequest>,
}

impl LootRequirements {
    #[must_use]
    pub fn exploration_maps(&self) -> &[ExplorationMapRequest] {
        &self.exploration_maps
    }

    fn add_exploration_map(&mut self, request: ExplorationMapRequest) {
        if !self.exploration_maps.contains(&request) {
            self.exploration_maps.push(request);
        }
    }
}

impl LootTable {
    /// Validates registry-backed inputs and collects world work without consuming RNG.
    pub fn requirements(&self) -> LootResult<LootRequirements> {
        let mut requirements = LootRequirements::default();
        let mut visiting = FxHashSet::default();
        self.collect_requirements(&mut requirements, &mut visiting)?;
        Ok(requirements)
    }

    fn collect_requirements(
        &self,
        requirements: &mut LootRequirements,
        visiting: &mut FxHashSet<steel_utils::Identifier>,
    ) -> LootResult<()> {
        if !visiting.insert(self.key.clone()) {
            return Err(LootError::UnsupportedEntry(
                "recursive loot-table reference",
            ));
        }

        let result = (|| {
            validate_functions(self.functions, requirements, visiting)?;
            for pool in self.pools {
                validate_conditions(pool.conditions)?;
                validate_number_provider(&pool.rolls)?;
                validate_functions(pool.functions, requirements, visiting)?;
                for entry in pool.entries {
                    validate_entry(entry, requirements, visiting)?;
                }
            }
            Ok(())
        })();
        visiting.remove(&self.key);
        result
    }
}

fn validate_entry(
    entry: &LootEntry,
    requirements: &mut LootRequirements,
    visiting: &mut FxHashSet<steel_utils::Identifier>,
) -> LootResult<()> {
    validate_conditions(entry.conditions())?;
    validate_functions(entry.functions(), requirements, visiting)?;
    match entry {
        LootEntry::Item { name, .. } => {
            require_registry_value(REGISTRY.items.by_key(name), "item", name)?;
        }
        LootEntry::LootTableRef { name, .. } => {
            let table =
                require_registry_value(REGISTRY.loot_tables.by_key(name), "loot table", name)?;
            table.collect_requirements(requirements, visiting)?;
        }
        LootEntry::InlineLootTable { pools, .. } => {
            for pool in *pools {
                validate_conditions(pool.conditions)?;
                validate_number_provider(&pool.rolls)?;
                validate_functions(pool.functions, requirements, visiting)?;
                for child in pool.entries {
                    validate_entry(child, requirements, visiting)?;
                }
            }
        }
        LootEntry::Tag { name, .. } => {
            REGISTRY
                .items
                .get_tag(name)
                .ok_or_else(|| LootError::UnknownRegistryTag {
                    registry: "item",
                    key: name.clone(),
                })?;
        }
        LootEntry::Alternatives { children, .. }
        | LootEntry::Group { children, .. }
        | LootEntry::Sequence { children, .. } => {
            for child in *children {
                validate_entry(child, requirements, visiting)?;
            }
        }
        LootEntry::Empty { .. } => {}
        LootEntry::Dynamic { .. } => return Err(LootError::UnsupportedEntry("dynamic")),
        LootEntry::Slots { .. } => return Err(LootError::UnsupportedEntry("slots")),
    }
    Ok(())
}

fn validate_functions(
    functions: &[ConditionalLootFunction],
    requirements: &mut LootRequirements,
    visiting: &mut FxHashSet<steel_utils::Identifier>,
) -> LootResult<()> {
    for function in functions {
        validate_conditions(function.conditions)?;
        validate_function(&function.function, requirements, visiting)?;
    }
    Ok(())
}

fn validate_function(
    function: &LootFunction,
    requirements: &mut LootRequirements,
    visiting: &mut FxHashSet<steel_utils::Identifier>,
) -> LootResult<()> {
    match function {
        LootFunction::SetCount { count, .. }
        | LootFunction::EnchantedCountIncrease { count, .. } => validate_number_provider(count)?,
        LootFunction::SetDamage { damage, .. } => validate_number_provider(damage)?,
        LootFunction::EnchantRandomly { options } => validate_enchantment_options(options)?,
        LootFunction::EnchantWithLevels { levels, options } => {
            validate_number_provider(levels)?;
            validate_enchantment_options(options)?;
        }
        LootFunction::ExplorationMap {
            destination,
            decoration,
            zoom,
            search_radius,
            skip_existing_chunks,
        } => {
            i8::try_from(*zoom).map_err(|_| LootError::InvalidExplorationMapZoom(*zoom))?;
            REGISTRY.structures.get_tag(destination).ok_or_else(|| {
                LootError::UnknownRegistryTag {
                    registry: "structure",
                    key: destination.clone(),
                }
            })?;
            require_registry_value(
                REGISTRY.map_decoration_types.by_key(decoration),
                "map decoration type",
                decoration,
            )?;
            requirements.add_exploration_map(ExplorationMapRequest {
                destination: destination.clone(),
                decoration: decoration.clone(),
                zoom: *zoom,
                search_radius: *search_radius,
                skip_existing_chunks: *skip_existing_chunks,
            });
        }
        LootFunction::SetOminousBottleAmplifier { amplifier } => {
            validate_number_provider(amplifier)?;
        }
        LootFunction::SetPotion { id } => {
            require_registry_value(REGISTRY.potions.by_key(id), "potion", id)?;
        }
        LootFunction::SetStewEffect { effects } => {
            for effect in *effects {
                require_registry_value(
                    REGISTRY.mob_effects.by_key(&effect.effect_type),
                    "mob effect",
                    &effect.effect_type,
                )?;
                validate_number_provider(&effect.duration)?;
            }
        }
        LootFunction::SetInstrument { options } => match options {
            InstrumentOptions::Tag(tag) => {
                REGISTRY
                    .instruments
                    .get_tag(tag)
                    .ok_or_else(|| LootError::UnknownRegistryTag {
                        registry: "instrument",
                        key: tag.clone(),
                    })?;
            }
            InstrumentOptions::Direct(_) => {}
        },
        LootFunction::SetEnchantments { enchantments, .. } => {
            for (key, provider) in *enchantments {
                require_registry_value(REGISTRY.enchantments.by_key(key), "enchantment", key)?;
                validate_number_provider(provider)?;
            }
        }
        LootFunction::SetItem { item } => {
            require_registry_value(REGISTRY.items.by_key(item), "item", item)?;
        }
        LootFunction::Sequence { functions } => {
            validate_functions(functions, requirements, visiting)?;
        }
        LootFunction::Filtered { modifier, .. } => {
            validate_functions(std::slice::from_ref(*modifier), requirements, visiting)?;
        }
        LootFunction::ExplosionDecay
        | LootFunction::ApplyBonus { .. }
        | LootFunction::LimitCount { .. }
        | LootFunction::SetCustomData { .. }
        | LootFunction::FurnaceSmelt { .. }
        | LootFunction::SetName { .. }
        | LootFunction::Discard
        | LootFunction::CopyComponents {
            source: CopySource::BlockEntity,
            ..
        } => {}
        LootFunction::CopyComponents { .. } => unsupported_function("copy_components")?,
        LootFunction::CopyState { .. } => unsupported_function("copy_state")?,
        LootFunction::SetComponents { .. } => unsupported_function("set_components")?,
        LootFunction::CopyName { .. } => unsupported_function("copy_name")?,
        LootFunction::SetLore { .. } => unsupported_function("set_lore")?,
        LootFunction::SetContents { .. } => unsupported_function("set_contents")?,
        LootFunction::ModifyContents { .. } => unsupported_function("modify_contents")?,
        LootFunction::SetLootTable { .. } => unsupported_function("set_loot_table")?,
        LootFunction::SetAttributes { .. } => unsupported_function("set_attributes")?,
        LootFunction::FillPlayerHead { .. } => unsupported_function("fill_player_head")?,
        LootFunction::CopyCustomData { .. } => unsupported_function("copy_custom_data")?,
        LootFunction::SetBannerPattern { .. } => unsupported_function("set_banner_pattern")?,
        LootFunction::SetFireworks { .. } => unsupported_function("set_fireworks")?,
        LootFunction::SetFireworkExplosion { .. } => {
            unsupported_function("set_firework_explosion")?;
        }
        LootFunction::SetBookCover { .. } => unsupported_function("set_book_cover")?,
        LootFunction::SetWrittenBookPages { .. } => unsupported_function("set_written_book_pages")?,
        LootFunction::SetWritableBookPages { .. } => {
            unsupported_function("set_writable_book_pages")?;
        }
        LootFunction::ToggleTooltips { .. } => unsupported_function("toggle_tooltips")?,
        LootFunction::Reference(_) => unsupported_function("reference")?,
    }
    Ok(())
}

pub(crate) fn validate_conditions(conditions: &[LootCondition]) -> LootResult<()> {
    for condition in conditions {
        match condition {
            LootCondition::Inverted(inner) => validate_conditions(std::slice::from_ref(*inner))?,
            LootCondition::AnyOf(children) | LootCondition::AllOf(children) => {
                validate_conditions(children)?;
            }
            LootCondition::LocationCheck { predicate, .. } => {
                if let Some(biomes) = &predicate.biomes {
                    biomes.validate()?;
                }
                if let Some(block) = &predicate.block {
                    validate_block_predicate(block)?;
                }
            }
            LootCondition::TimeCheck { value, .. } => validate_range(value)?,
            LootCondition::ValueCheck { value, range } => {
                validate_number_provider(value)?;
                validate_range(range)?;
            }
            LootCondition::EntityScores { .. } => {
                return Err(LootError::UnsupportedCondition("entity_scores"));
            }
            LootCondition::Reference(_) => {
                return Err(LootError::UnsupportedCondition("reference"));
            }
            LootCondition::SurvivesExplosion
            | LootCondition::BlockStateProperty { .. }
            | LootCondition::RandomChance(_)
            | LootCondition::RandomChanceWithEnchantedBonus { .. }
            | LootCondition::MatchTool(_)
            | LootCondition::TableBonus { .. }
            | LootCondition::KilledByPlayer
            | LootCondition::EntityProperties { .. }
            | LootCondition::DamageSourceProperties { .. }
            | LootCondition::WeatherCheck { .. }
            | LootCondition::EnchantmentActiveCheck { .. } => {}
        }
    }
    Ok(())
}

fn validate_range(range: &NumberProviderRange) -> LootResult<()> {
    if let Some(min) = &range.min {
        validate_number_provider(min)?;
    }
    if let Some(max) = &range.max {
        validate_number_provider(max)?;
    }
    Ok(())
}

const fn validate_number_provider(provider: &NumberProvider) -> LootResult<()> {
    match provider {
        NumberProvider::Score { .. } => Err(LootError::UnsupportedNumberProvider("score")),
        NumberProvider::Storage { .. } => Err(LootError::UnsupportedNumberProvider("storage")),
        NumberProvider::Constant(_)
        | NumberProvider::Uniform { .. }
        | NumberProvider::Binomial { .. }
        | NumberProvider::EnchantmentLevel { .. } => Ok(()),
    }
}

fn validate_enchantment_options(options: &EnchantmentOptions) -> LootResult<()> {
    options.resolve().map(|_| ())
}

fn require_registry_value<'a, T>(
    value: Option<&'a T>,
    registry: &'static str,
    key: &steel_utils::Identifier,
) -> LootResult<&'a T> {
    value.ok_or_else(|| LootError::UnknownRegistryValue {
        registry,
        key: key.clone(),
    })
}

const fn unsupported_function(name: &'static str) -> LootResult<()> {
    Err(LootError::UnsupportedFunction(name))
}
