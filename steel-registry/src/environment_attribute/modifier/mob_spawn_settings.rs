use steel_math::keyframe_track::LerpFunction;

use super::{AttributeModifier, OperationId};
use crate::entity_type::MobCategory;
use crate::environment_attribute::{EnvironmentAttribute, MobSpawnSettings};

/// Layers spawn settings over each other, as vanilla's
/// `MobSpawnSettingsModifier.Overlay`: categories and costs the argument
/// defines replace the subject's, the rest are kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MobSpawnSettingsOverlayModifier;

impl AttributeModifier<MobSpawnSettings> for MobSpawnSettingsOverlayModifier {
    type Argument = MobSpawnSettings;

    fn operation_id(&self) -> OperationId {
        OperationId::Overlay
    }

    fn apply(&self, first: MobSpawnSettings, second: &MobSpawnSettings) -> MobSpawnSettings {
        let first_costs = first.all_spawn_costs();
        let second_costs = second.all_spawn_costs();
        if first.defined_categories().next().is_none() && first_costs.is_empty() {
            return second.clone();
        }
        if second.defined_categories().next().is_none() && second_costs.is_empty() {
            return first;
        }
        let second_covers_categories = first
            .defined_categories()
            .all(|category| second.get_mobs_in_category(category).is_some());
        let second_covers_costs = first_costs
            .iter()
            .all(|(entity_type, _)| second.get_mob_spawn_cost(entity_type).is_some());
        if second_covers_categories && second_covers_costs {
            return second.clone();
        }

        let spawns = MobCategory::ALL
            .into_iter()
            .filter_map(|category| {
                let spawns = second
                    .get_mobs_in_category(category)
                    .or_else(|| first.get_mobs_in_category(category))?;
                Some((category, spawns.to_vec().into()))
            })
            .collect();

        let mut costs = first_costs.to_vec();
        for (entity_type, cost) in second_costs {
            match costs
                .iter_mut()
                .find(|(existing, _)| existing.key == entity_type.key)
            {
                Some((_, existing_cost)) => *existing_cost = *cost,
                None => costs.push((entity_type, *cost)),
            }
        }
        MobSpawnSettings::owned(spawns, costs)
    }

    fn argument_keyframe_lerp(
        &self,
        attribute: &EnvironmentAttribute<MobSpawnSettings>,
    ) -> LerpFunction<MobSpawnSettings> {
        attribute.attribute_type().keyframe_lerp()
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use steel_utils::value_providers::IntProvider;

    use super::{AttributeModifier as _, MobSpawnSettingsOverlayModifier};
    use crate::entity_type::{EntityTypeRef, MobCategory};
    use crate::environment_attribute::{
        CategorySpawns, MobSpawnCost, MobSpawnSettings, SpawnerData, Weighted,
    };
    use crate::vanilla_entities::{COW, SHEEP, ZOMBIE};

    static ZOMBIES: [Weighted<SpawnerData>; 1] = [Weighted {
        value: SpawnerData::new(&ZOMBIE, IntProvider::Constant(4)),
        weight: 100,
    }];
    static SHEEP_SPAWNS: [Weighted<SpawnerData>; 1] = [Weighted {
        value: SpawnerData::new(&SHEEP, IntProvider::Constant(4)),
        weight: 12,
    }];
    static COWS: [Weighted<SpawnerData>; 1] = [Weighted {
        value: SpawnerData::new(&COW, IntProvider::Constant(4)),
        weight: 8,
    }];
    static FIRST_CATEGORIES: [CategorySpawns; 2] = [
        (MobCategory::Monster, Cow::Borrowed(&ZOMBIES)),
        (MobCategory::Creature, Cow::Borrowed(&SHEEP_SPAWNS)),
    ];
    static SECOND_CATEGORIES: [CategorySpawns; 1] = [(MobCategory::Creature, Cow::Borrowed(&COWS))];
    static FIRST_COSTS: [(EntityTypeRef, MobSpawnCost); 1] = [(
        &ZOMBIE,
        MobSpawnCost {
            energy_budget: 1.0,
            charge: 0.1,
        },
    )];
    static SECOND_COSTS: [(EntityTypeRef, MobSpawnCost); 1] = [(
        &COW,
        MobSpawnCost {
            energy_budget: 2.0,
            charge: 0.2,
        },
    )];

    #[test]
    fn overlay_replaces_defined_categories_and_merges_costs() {
        let first = MobSpawnSettings::new(&FIRST_CATEGORIES, &FIRST_COSTS);
        let second = MobSpawnSettings::new(&SECOND_CATEGORIES, &SECOND_COSTS);

        let result = MobSpawnSettingsOverlayModifier.apply(first, &second);

        let monsters = result.get_mobs_to_spawn(MobCategory::Monster);
        assert_eq!(monsters[0].value.entity_type().key, ZOMBIE.key);
        let creatures = result.get_mobs_to_spawn(MobCategory::Creature);
        assert_eq!(creatures.len(), 1);
        assert_eq!(creatures[0].value.entity_type().key, COW.key);
        let costs: Vec<_> = result
            .all_spawn_costs()
            .iter()
            .map(|(entity_type, _)| entity_type.key.clone())
            .collect();
        assert_eq!(costs, [ZOMBIE.key.clone(), COW.key.clone()]);
    }
}
