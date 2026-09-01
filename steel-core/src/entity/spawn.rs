use steel_registry::entity_variant::FoxVariant;

/// Vanilla `EntitySpawnReason`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntitySpawnReason {
    Natural,
    ChunkGeneration,
    Spawner,
    Structure,
    Breeding,
    MobSummoned,
    Jockey,
    Event,
    Conversion,
    Reinforcement,
    Triggered,
    Bucket,
    SpawnItemUse,
    Command,
    Dispenser,
    Patrol,
    TrialSpawner,
    Load,
    DimensionTravel,
}

impl EntitySpawnReason {
    #[must_use]
    pub const fn is_spawner(self) -> bool {
        matches!(self, Self::Spawner | Self::TrialSpawner)
    }

    #[must_use]
    pub const fn ignores_light_requirements(self) -> bool {
        matches!(self, Self::TrialSpawner)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SpawnGroupData {
    AgeableMob(AgeableMobGroupData),
    Fox(FoxGroupData),
}

impl SpawnGroupData {
    /// The ageable group data every spawn group carries, so a generic ageable
    /// finalize can drive the shared baby roll no matter the concrete group type.
    #[must_use]
    pub const fn ageable_group_data(self) -> AgeableMobGroupData {
        match self {
            Self::AgeableMob(data) => data,
            Self::Fox(data) => data.ageable,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AgeableMobGroupData {
    group_size: i32,
    should_spawn_baby: bool,
    baby_spawn_chance: f32,
}

impl AgeableMobGroupData {
    pub const DEFAULT_BABY_SPAWN_CHANCE: f32 = 0.05;

    #[must_use]
    pub const fn new(should_spawn_baby: bool, baby_spawn_chance: f32) -> Self {
        Self {
            group_size: 0,
            should_spawn_baby,
            baby_spawn_chance,
        }
    }

    #[must_use]
    pub const fn with_should_spawn_baby(should_spawn_baby: bool) -> Self {
        Self::new(should_spawn_baby, Self::DEFAULT_BABY_SPAWN_CHANCE)
    }

    #[must_use]
    pub const fn with_baby_spawn_chance(baby_spawn_chance: f32) -> Self {
        Self::new(true, baby_spawn_chance)
    }

    #[must_use]
    pub const fn group_size(self) -> i32 {
        self.group_size
    }

    #[must_use]
    pub const fn should_spawn_baby(self) -> bool {
        self.should_spawn_baby
    }

    #[must_use]
    pub const fn baby_spawn_chance(self) -> f32 {
        self.baby_spawn_chance
    }

    pub const fn increase_group_size_by_one(&mut self) {
        self.group_size += 1;
    }

    #[must_use]
    pub const fn needs_baby_spawn_roll(self) -> bool {
        self.should_spawn_baby && self.group_size > 0
    }

    pub fn finalize_ageable_spawn(&mut self, baby_roll: impl FnOnce() -> f32) -> bool {
        let spawn_baby = self.needs_baby_spawn_roll() && baby_roll() <= self.baby_spawn_chance;
        self.increase_group_size_by_one();
        spawn_baby
    }
}

/// Vanilla `Fox.FoxGroupData`: an ageable spawn group that shares one coat variant
/// and disables the generic baby roll, so a fox group takes a uniform coat and only
/// its third and later members spawn as kits.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FoxGroupData {
    variant: FoxVariant,
    ageable: AgeableMobGroupData,
}

impl FoxGroupData {
    /// Vanilla `new FoxGroupData(variant)`, whose `super(false)` disables the baby roll.
    #[must_use]
    pub const fn new(variant: FoxVariant) -> Self {
        Self {
            variant,
            ageable: AgeableMobGroupData::with_should_spawn_baby(false),
        }
    }

    /// The coat every fox in this group wears (the first fox's biome variant).
    #[must_use]
    pub const fn variant(self) -> FoxVariant {
        self.variant
    }

    /// How many foxes in the group have been spawned before this one.
    #[must_use]
    pub const fn group_size(self) -> i32 {
        self.ageable.group_size()
    }

    /// Grows the shared group after a fox spawns (vanilla `super.finalizeSpawn`).
    /// The baby roll stays disabled, so this never spawns a kit by chance; the fox
    /// itself makes the third and later members kits.
    pub fn advance_group(&mut self, baby_roll: impl FnOnce() -> f32) {
        let _ = self.ageable.finalize_ageable_spawn(baby_roll);
    }
}

#[cfg(test)]
mod tests {
    use super::AgeableMobGroupData;

    #[test]
    fn ageable_group_data_increments_before_later_baby_rolls_can_apply() {
        let mut group_data = AgeableMobGroupData::with_should_spawn_baby(true);

        assert!(!group_data.finalize_ageable_spawn(|| {
            panic!("first group member should not roll for baby spawn")
        }));
        assert_eq!(group_data.group_size(), 1);

        assert!(group_data.finalize_ageable_spawn(|| 0.05));
        assert_eq!(group_data.group_size(), 2);
    }

    #[test]
    fn ageable_group_data_can_disable_baby_spawns() {
        let mut group_data = AgeableMobGroupData::with_should_spawn_baby(false);

        assert!(
            !group_data
                .finalize_ageable_spawn(|| { panic!("disabled baby spawning should not roll") })
        );
        assert!(
            !group_data
                .finalize_ageable_spawn(|| { panic!("disabled baby spawning should not roll") })
        );
        assert_eq!(group_data.group_size(), 2);
    }
}
