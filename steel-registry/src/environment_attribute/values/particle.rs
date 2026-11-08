use crate::particle_type::ParticleData;

use super::AttributeList;

/// A particle spawned around players with a per-tick probability, as
/// vanilla's `AmbientParticle`.
#[derive(Debug, Clone)]
pub struct AmbientParticle {
    pub particle: ParticleData,
    pub probability: f32,
}

impl AmbientParticle {
    /// Scales the probability, as the cross-fade of vanilla's `AMBIENT_PARTICLES` type.
    #[must_use]
    pub fn with_scaled_probability(&self, alpha: f32) -> Self {
        Self {
            particle: self.particle.clone(),
            probability: self.probability * alpha,
        }
    }
}

/// Keyframe and weather lerp of vanilla's `AMBIENT_PARTICLES` attribute type.
#[must_use]
pub fn lerp_ambient_particles(
    alpha: f32,
    from: &AttributeList<AmbientParticle>,
    to: &AttributeList<AmbientParticle>,
) -> AttributeList<AmbientParticle> {
    super::list::lerp_list_cross_fade(alpha, from, to, AmbientParticle::with_scaled_probability)
}
