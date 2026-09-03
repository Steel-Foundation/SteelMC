//! Trial spawner configuration registry values extracted from Vanilla.
//!
//! Mirrors vanilla's `minecraft:trial_spawner` data registry
//! (`TrialSpawnerConfig`), which trial spawner block entities reference from
//! their `normal_config`/`ominous_config` NBT fields.

use rustc_hash::FxHashMap;
use steel_utils::Identifier;

/// An extra NBT tag carried by a spawn potential's entity payload
/// (vanilla `SpawnData.entity`), e.g. `IsBaby: 1`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TrialSpawnerEntityTag {
    Int { name: &'static str, value: i32 },
    String { name: &'static str, value: &'static str },
}

/// Equipment applied to a spawned mob (vanilla `SpawnData.equipment`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrialSpawnerEquipment {
    pub loot_table: &'static str,
    pub slot_drop_chances: f32,
}

/// One weighted entry of vanilla `WeightedList<SpawnData>`.
#[derive(Debug, Clone, Copy)]
pub struct TrialSpawnerSpawnPotential {
    pub weight: u32,
    /// Full entity id, including namespace (e.g. `minecraft:zombie`).
    pub entity_id: &'static str,
    pub extra_tags: &'static [TrialSpawnerEntityTag],
    pub equipment: Option<TrialSpawnerEquipment>,
}

/// One weighted loot table entry of vanilla `WeightedList<ResourceKey<LootTable>>`.
#[derive(Debug, Clone, Copy)]
pub struct TrialSpawnerEjectLootTable {
    pub weight: u32,
    pub loot_table: &'static str,
}

/// Registered trial spawner config definition (vanilla `TrialSpawnerConfig`).
#[derive(Debug)]
pub struct TrialSpawnerConfig {
    pub key: Identifier,
    pub spawn_range: i32,
    pub total_mobs: f32,
    pub simultaneous_mobs: f32,
    pub total_mobs_added_per_player: f32,
    pub simultaneous_mobs_added_per_player: f32,
    pub ticks_between_spawn: i32,
    pub spawn_potentials: &'static [TrialSpawnerSpawnPotential],
    pub loot_tables_to_eject: &'static [TrialSpawnerEjectLootTable],
    pub items_to_drop_when_ominous: &'static str,
}

pub type TrialSpawnerConfigRef = &'static TrialSpawnerConfig;

impl TrialSpawnerConfig {
    #[must_use]
    pub const fn new(
        key: Identifier,
        spawn_range: i32,
        total_mobs: f32,
        simultaneous_mobs: f32,
        total_mobs_added_per_player: f32,
        simultaneous_mobs_added_per_player: f32,
        ticks_between_spawn: i32,
        spawn_potentials: &'static [TrialSpawnerSpawnPotential],
        loot_tables_to_eject: &'static [TrialSpawnerEjectLootTable],
        items_to_drop_when_ominous: &'static str,
    ) -> Self {
        Self {
            key,
            spawn_range,
            total_mobs,
            simultaneous_mobs,
            total_mobs_added_per_player,
            simultaneous_mobs_added_per_player,
            ticks_between_spawn,
            spawn_potentials,
            loot_tables_to_eject,
            items_to_drop_when_ominous,
        }
    }

    /// Vanilla `TrialSpawnerConfig.calculateTargetTotalMobs`.
    #[must_use]
    pub fn calculate_target_total_mobs(&self, additional_players: i32) -> i32 {
        (self.total_mobs + self.total_mobs_added_per_player * additional_players as f32).floor()
            as i32
    }

    /// Vanilla `TrialSpawnerConfig.calculateTargetSimultaneousMobs`.
    #[must_use]
    pub fn calculate_target_simultaneous_mobs(&self, additional_players: i32) -> i32 {
        (self.simultaneous_mobs
            + self.simultaneous_mobs_added_per_player * additional_players as f32)
            .floor() as i32
    }

    /// Vanilla `TrialSpawnerConfig.ticksBetweenItemSpawners` (code-defined constant).
    #[must_use]
    pub const fn ticks_between_item_spawners(&self) -> i64 {
        160
    }

    /// Vanilla `TrialSpawnerConfig.DEFAULT`.
    #[must_use]
    pub const fn default_config() -> TrialSpawnerConfig {
        const DEFAULT_EJECT: [TrialSpawnerEjectLootTable; 2] = [
            TrialSpawnerEjectLootTable {
                weight: 1,
                loot_table: "minecraft:spawners/trial_chamber/consumables",
            },
            TrialSpawnerEjectLootTable {
                weight: 1,
                loot_table: "minecraft:spawners/trial_chamber/key",
            },
        ];
        Self::new(
            Identifier::vanilla_static("default"),
            4,
            6.0,
            2.0,
            2.0,
            1.0,
            40,
            &[],
            &DEFAULT_EJECT,
            "minecraft:spawners/trial_chamber/items_to_drop_when_ominous",
        )
    }
}

/// Manual `PartialEq`/`Eq` impls removed: `impl_registry!` derives them by key.

/// The shared vanilla default config, usable as a registry-fallback reference.
pub static DEFAULT: TrialSpawnerConfig = TrialSpawnerConfig::default_config();

pub struct TrialSpawnerConfigRegistry {
    configs_by_id: Vec<TrialSpawnerConfigRef>,
    configs_by_key: FxHashMap<Identifier, usize>,
    allows_registering: bool,
}

impl TrialSpawnerConfigRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            configs_by_id: Vec::new(),
            configs_by_key: FxHashMap::default(),
            allows_registering: true,
        }
    }

    pub fn register(&mut self, config: TrialSpawnerConfigRef) -> usize {
        assert!(
            self.allows_registering,
            "Cannot register trial spawner configs after the registry has been frozen"
        );

        let id = self.configs_by_id.len();
        self.configs_by_key.insert(config.key.clone(), id);
        self.configs_by_id.push(config);
        id
    }
}

impl Default for TrialSpawnerConfigRegistry {
    fn default() -> Self {
        Self::new()
    }
}

crate::impl_registry!(
    TrialSpawnerConfigRegistry,
    TrialSpawnerConfig,
    configs_by_id,
    configs_by_key,
    trial_spawner_configs
);

#[cfg(test)]
mod tests {
    use steel_utils::Identifier;

    use super::TrialSpawnerConfig;

    #[test]
    fn target_mob_counts_match_vanilla_flooring() {
        let config = TrialSpawnerConfig::default_config();
        assert_eq!(config.calculate_target_total_mobs(0), 6);
        assert_eq!(config.calculate_target_total_mobs(2), 10);
        assert_eq!(config.calculate_target_simultaneous_mobs(0), 2);
        assert_eq!(config.calculate_target_simultaneous_mobs(1), 3);
        assert_eq!(
            config.key,
            Identifier::vanilla_static("default")
        );
    }
}
