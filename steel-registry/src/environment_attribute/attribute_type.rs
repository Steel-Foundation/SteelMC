use std::fmt::{self, Debug, Formatter};

use glam::{Vec3, Vec4};
use steel_math::keyframe_track::{LerpFunction, lerp_constant, lerp_float, lerp_int, lerp_step};
use steel_utils::Identifier;

use super::values::{
    AmbientParticle, AmbientSounds, AttributeList, BackgroundMusic, BedRule, MobSpawnSettings,
    MoonPhase, TriState, argb, lerp_ambient_particles,
};
use crate::particle_type::ParticleData;

/// Value type of an environment attribute and how its values interpolate.
///
/// Vanilla also stores the value codec, modifier library, spatial lerp, and
/// partial-tick lerp here. Steel generates typed data at build time, so the
/// codec and modifier library only exist in the build script; spatial and
/// partial-tick lerps are client-side.
pub struct AttributeType<V: 'static> {
    key: Identifier,
    keyframe_lerp: LerpFunction<V>,
    state_change_lerp: LerpFunction<V>,
}

impl<V: Clone + 'static> AttributeType<V> {
    /// A type whose values step between keyframes and weather states, as
    /// vanilla's `AttributeType.ofNotInterpolated`.
    #[must_use]
    pub const fn not_interpolated(key: Identifier) -> Self {
        Self {
            key,
            keyframe_lerp: lerp_constant::<V>,
            state_change_lerp: lerp_immediate::<V>,
        }
    }

    /// A type that blends keyframes and weather states with `lerp`, as
    /// vanilla's `AttributeType.ofInterpolated`.
    #[must_use]
    pub const fn interpolated(key: Identifier, lerp: LerpFunction<V>) -> Self {
        Self {
            key,
            keyframe_lerp: lerp,
            state_change_lerp: lerp,
        }
    }

    #[must_use]
    pub const fn key(&self) -> &Identifier {
        &self.key
    }

    /// Lerp applied between timeline keyframes (override modifier tracks).
    #[must_use]
    pub const fn keyframe_lerp(&self) -> LerpFunction<V> {
        self.keyframe_lerp
    }

    /// Lerp applied when blending in weather states.
    #[must_use]
    pub const fn state_change_lerp(&self) -> LerpFunction<V> {
        self.state_change_lerp
    }
}

impl<V> Debug for AttributeType<V> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AttributeType")
            .field("key", &self.key)
            .finish_non_exhaustive()
    }
}

/// Vanilla `LerpFunction.ofStep(0.0F)`: any blend selects the target value.
fn lerp_immediate<T: Clone>(alpha: f32, from: &T, to: &T) -> T {
    lerp_step(0.0, alpha, from, to)
}

pub static BOOLEAN: AttributeType<bool> =
    AttributeType::not_interpolated(Identifier::vanilla_static("boolean"));

pub static FLOAT: AttributeType<f32> =
    AttributeType::interpolated(Identifier::vanilla_static("float"), lerp_float);

/// Vanilla only uses the degree-aware lerp for client partial ticks; keyframes
/// and weather blend angles linearly.
pub static ANGLE_DEGREES: AttributeType<f32> =
    AttributeType::interpolated(Identifier::vanilla_static("angle_degrees"), lerp_float);

pub static TRI_STATE: AttributeType<TriState> =
    AttributeType::not_interpolated(Identifier::vanilla_static("tri_state"));

pub static RGB_COLOR: AttributeType<Vec3> =
    AttributeType::interpolated(Identifier::vanilla_static("rgb_color"), argb::lerp_rgb);

pub static ARGB_COLOR: AttributeType<Vec4> =
    AttributeType::interpolated(Identifier::vanilla_static("argb_color"), argb::lerp_argb);

pub static INTEGER: AttributeType<i32> =
    AttributeType::interpolated(Identifier::vanilla_static("integer"), lerp_int);

pub static MOON_PHASE: AttributeType<MoonPhase> =
    AttributeType::not_interpolated(Identifier::vanilla_static("moon_phase"));

// TODO: Add the `activity` type once Steel has the built-in activity registry.

pub static BED_RULE: AttributeType<BedRule> =
    AttributeType::not_interpolated(Identifier::vanilla_static("bed_rule"));

pub static PARTICLE: AttributeType<ParticleData> =
    AttributeType::not_interpolated(Identifier::vanilla_static("particle"));

pub static AMBIENT_PARTICLES: AttributeType<AttributeList<AmbientParticle>> =
    AttributeType::interpolated(
        Identifier::vanilla_static("ambient_particles"),
        lerp_ambient_particles,
    );

pub static BACKGROUND_MUSIC: AttributeType<BackgroundMusic> =
    AttributeType::not_interpolated(Identifier::vanilla_static("background_music"));

pub static AMBIENT_SOUNDS: AttributeType<AmbientSounds> =
    AttributeType::not_interpolated(Identifier::vanilla_static("ambient_sounds"));

pub static MOB_SPAWN_SETTINGS: AttributeType<MobSpawnSettings> =
    AttributeType::not_interpolated(Identifier::vanilla_static("mob_spawn_settings"));
