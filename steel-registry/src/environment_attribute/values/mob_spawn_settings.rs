use std::borrow::Cow;

use steel_utils::value_providers::IntProvider;
use steel_utils::{DowncastType, DowncastTypeKey};

use crate::entity_type::{EntityTypeRef, MobCategory};
use crate::vanilla_entities;

/// A value with a selection weight, as vanilla's `util.random.Weighted`.
#[derive(Debug, Clone)]
pub struct Weighted<T> {
    pub value: T,
    pub weight: i32,
}

/// An entity type spawned in groups of `count`, as vanilla's
/// `MobSpawnSettings.SpawnerData`.
#[derive(Debug, Clone)]
pub struct SpawnerData {
    entity_type: EntityTypeRef,
    count: IntProvider,
}

impl SpawnerData {
    /// Creates spawner data. Like vanilla, `misc` category entities are
    /// replaced by pigs.
    #[must_use]
    pub const fn new(entity_type: EntityTypeRef, count: IntProvider) -> Self {
        let entity_type = if matches!(entity_type.mob_category, MobCategory::Misc) {
            &vanilla_entities::PIG
        } else {
            entity_type
        };
        Self { entity_type, count }
    }

    #[must_use]
    pub const fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    #[must_use]
    pub const fn count(&self) -> &IntProvider {
        &self.count
    }
}

/// Spawn potential cost of an entity type, as vanilla's `MobSpawnSettings.MobSpawnCost`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MobSpawnCost {
    pub energy_budget: f64,
    pub charge: f64,
}

/// Weighted spawn lists of the categories this value defines.
pub type CategorySpawns = (MobCategory, Cow<'static, [Weighted<SpawnerData>]>);

/// Natural spawn settings, as vanilla's `MobSpawnSettings`.
///
/// A category without an entry is undefined, which the overlay modifier
/// distinguishes from a defined empty list. Entries are kept in
/// [`MobCategory::ALL`] order.
#[derive(Debug, Clone)]
pub struct MobSpawnSettings {
    spawns_by_category: Cow<'static, [CategorySpawns]>,
    mob_spawn_costs: Cow<'static, [(EntityTypeRef, MobSpawnCost)]>,
}

impl MobSpawnSettings {
    pub const EMPTY: Self = Self::new(&[], &[]);

    #[must_use]
    pub const fn new(
        spawns_by_category: &'static [CategorySpawns],
        mob_spawn_costs: &'static [(EntityTypeRef, MobSpawnCost)],
    ) -> Self {
        Self {
            spawns_by_category: Cow::Borrowed(spawns_by_category),
            mob_spawn_costs: Cow::Borrowed(mob_spawn_costs),
        }
    }

    #[must_use]
    pub const fn owned(
        spawns_by_category: Vec<CategorySpawns>,
        mob_spawn_costs: Vec<(EntityTypeRef, MobSpawnCost)>,
    ) -> Self {
        Self {
            spawns_by_category: Cow::Owned(spawns_by_category),
            mob_spawn_costs: Cow::Owned(mob_spawn_costs),
        }
    }

    /// Spawns of `category`, empty when the category is undefined.
    #[must_use]
    pub fn get_mobs_to_spawn(&self, category: MobCategory) -> &[Weighted<SpawnerData>] {
        self.get_mobs_in_category(category).unwrap_or_default()
    }

    /// Spawns of `category`, or `None` when the category is undefined.
    #[must_use]
    pub fn get_mobs_in_category(&self, category: MobCategory) -> Option<&[Weighted<SpawnerData>]> {
        self.spawns_by_category
            .iter()
            .find(|(defined, _)| *defined == category)
            .map(|(_, spawns)| spawns.as_ref())
    }

    pub fn defined_categories(&self) -> impl Iterator<Item = MobCategory> + '_ {
        self.spawns_by_category
            .iter()
            .map(|(category, _)| *category)
    }

    #[must_use]
    pub fn get_mob_spawn_cost(&self, entity_type: EntityTypeRef) -> Option<&MobSpawnCost> {
        self.mob_spawn_costs
            .iter()
            .find(|(candidate, _)| candidate.key == entity_type.key)
            .map(|(_, cost)| cost)
    }

    #[must_use]
    pub fn all_spawn_costs(&self) -> &[(EntityTypeRef, MobSpawnCost)] {
        &self.mob_spawn_costs
    }
}

// SAFETY: This Steel-owned key uniquely identifies `MobSpawnSettings` within the process.
unsafe impl DowncastType for MobSpawnSettings {
    const TYPE_KEY: DowncastTypeKey =
        DowncastTypeKey::new("steel:environment_attribute/mob_spawn_settings");
}
