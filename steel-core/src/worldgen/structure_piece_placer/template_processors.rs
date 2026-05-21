use std::collections::BTreeMap;

use steel_registry::shared_structs::BlockStateData;
use steel_registry::structure::{OceanRuinBiomeTempData, RuinedPortalPlacementData};
use steel_registry::structure_processor::{
    PosRuleTestData, ProcessorRuleData, RuleBlockEntityModifierData, StructureProcessorKind,
    StructureRuleTestData,
};
use steel_registry::{Registry, RegistryExt, vanilla_block_tags};
use steel_utils::Identifier;
use steel_utils::value_providers::IntProvider;

use crate::world::structure::{RuinedPortalProperties, TemplateProcessorList};

use super::StructurePiecePlacer;

impl StructurePiecePlacer {
    pub(super) fn template_processors<'a>(
        registry: &'a Registry,
        processors: &'a TemplateProcessorList,
        hardcoded_processors: &'a mut Vec<StructureProcessorKind>,
    ) -> &'a [StructureProcessorKind] {
        match processors {
            TemplateProcessorList::Empty => &[],
            TemplateProcessorList::Registry(key) => {
                let Some(processor_list) = registry.structure_processors.by_key(key) else {
                    panic!("template piece references unknown processor list {key}");
                };
                &processor_list.data.processors
            }
            TemplateProcessorList::OceanRuin {
                biome_temp,
                integrity,
            } => {
                hardcoded_processors.extend(Self::ocean_ruin_processors(*biome_temp, *integrity));
                hardcoded_processors.as_slice()
            }
            TemplateProcessorList::RuinedPortal {
                vertical_placement,
                properties,
            } => {
                hardcoded_processors.extend(Self::ruined_portal_processors(
                    *vertical_placement,
                    *properties,
                ));
                hardcoded_processors.as_slice()
            }
        }
    }

    fn ocean_ruin_processors(
        biome_temp: OceanRuinBiomeTempData,
        integrity: f32,
    ) -> Vec<StructureProcessorKind> {
        let (source, target, loot_table) = match biome_temp {
            OceanRuinBiomeTempData::Warm => (
                "sand",
                "suspicious_sand",
                Identifier::vanilla_static("archaeology/ocean_ruin_warm"),
            ),
            OceanRuinBiomeTempData::Cold => (
                "gravel",
                "suspicious_gravel",
                Identifier::vanilla_static("archaeology/ocean_ruin_cold"),
            ),
        };

        vec![
            StructureProcessorKind::BlockRot {
                rottable_blocks: None,
                integrity,
            },
            StructureProcessorKind::Capped {
                delegate: Box::new(StructureProcessorKind::Rule {
                    rules: vec![Self::append_loot_replace_rule(source, target, loot_table)],
                }),
                limit: IntProvider::Constant(5),
            },
        ]
    }

    fn ruined_portal_processors(
        vertical_placement: RuinedPortalPlacementData,
        properties: RuinedPortalProperties,
    ) -> Vec<StructureProcessorKind> {
        let mut rules = vec![
            Self::random_block_replace_rule("gold_block", 0.3, "air"),
            Self::ruined_portal_lava_rule(vertical_placement, properties),
        ];
        if !properties.cold {
            rules.push(Self::random_block_replace_rule(
                "netherrack",
                0.07,
                "magma_block",
            ));
        }

        let mut processors = vec![
            StructureProcessorKind::Rule { rules },
            StructureProcessorKind::BlockAge {
                mossiness: properties.mossiness,
            },
            StructureProcessorKind::ProtectedBlocks {
                cannot_replace: vanilla_block_tags::FEATURES_CANNOT_REPLACE_TAG,
            },
            StructureProcessorKind::LavaSubmergedBlock,
        ];
        if properties.replace_with_blackstone {
            processors.push(StructureProcessorKind::BlackstoneReplace);
        }
        processors
    }

    fn ruined_portal_lava_rule(
        vertical_placement: RuinedPortalPlacementData,
        properties: RuinedPortalProperties,
    ) -> ProcessorRuleData {
        if vertical_placement == RuinedPortalPlacementData::OnOceanFloor {
            Self::block_replace_rule("lava", "magma_block")
        } else if properties.cold {
            Self::block_replace_rule("lava", "netherrack")
        } else {
            Self::random_block_replace_rule("lava", 0.2, "magma_block")
        }
    }

    const fn block_replace_rule(source: &'static str, target: &'static str) -> ProcessorRuleData {
        ProcessorRuleData {
            input_predicate: StructureRuleTestData::BlockMatch {
                block: Identifier::vanilla_static(source),
            },
            location_predicate: StructureRuleTestData::AlwaysTrue,
            position_predicate: PosRuleTestData::AlwaysTrue,
            output_state: Self::block_state_data(target),
            block_entity_modifier: RuleBlockEntityModifierData::Passthrough,
        }
    }

    const fn random_block_replace_rule(
        source: &'static str,
        probability: f32,
        target: &'static str,
    ) -> ProcessorRuleData {
        ProcessorRuleData {
            input_predicate: StructureRuleTestData::RandomBlockMatch {
                block: Identifier::vanilla_static(source),
                probability,
            },
            location_predicate: StructureRuleTestData::AlwaysTrue,
            position_predicate: PosRuleTestData::AlwaysTrue,
            output_state: Self::block_state_data(target),
            block_entity_modifier: RuleBlockEntityModifierData::Passthrough,
        }
    }

    const fn append_loot_replace_rule(
        source: &'static str,
        target: &'static str,
        loot_table: Identifier,
    ) -> ProcessorRuleData {
        ProcessorRuleData {
            input_predicate: StructureRuleTestData::BlockMatch {
                block: Identifier::vanilla_static(source),
            },
            location_predicate: StructureRuleTestData::AlwaysTrue,
            position_predicate: PosRuleTestData::AlwaysTrue,
            output_state: Self::block_state_data(target),
            block_entity_modifier: RuleBlockEntityModifierData::AppendLoot { loot_table },
        }
    }

    const fn block_state_data(block: &'static str) -> BlockStateData {
        BlockStateData {
            name: Identifier::vanilla_static(block),
            properties: BTreeMap::new(),
        }
    }
}
