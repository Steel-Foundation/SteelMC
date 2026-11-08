//! Value types of vanilla's environment attribute types.

pub mod argb;
mod audio;
mod bed_rule;
mod list;
mod mob_spawn_settings;
mod moon_phase;
mod particle;
mod tri_state;

pub use audio::{
    AmbientAdditionsSettings, AmbientMoodSettings, AmbientSounds, BackgroundMusic, Music,
};
pub use bed_rule::{BedRule, BedRuleValue};
pub use list::{AttributeList, lerp_list_cross_fade};
pub use mob_spawn_settings::{
    CategorySpawns, MobSpawnCost, MobSpawnSettings, SpawnerData, Weighted,
};
pub use moon_phase::MoonPhase;
pub use particle::{AmbientParticle, lerp_ambient_particles};
pub use tri_state::TriState;
