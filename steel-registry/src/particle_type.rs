use rustc_hash::FxHashMap;
use steel_utils::Identifier;

/// Built-in particle type registry entry used by vanilla particle option codecs.
#[derive(Debug)]
pub struct ParticleType {
    pub key: Identifier,
    pub override_limiter: bool,
}

pub type ParticleTypeRef = &'static ParticleType;

pub struct ParticleTypeRegistry {
    particle_types_by_id: Vec<ParticleTypeRef>,
    particle_types_by_key: FxHashMap<Identifier, usize>,
    allows_registering: bool,
}

impl ParticleTypeRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            particle_types_by_id: Vec::new(),
            particle_types_by_key: FxHashMap::default(),
            allows_registering: true,
        }
    }
}

crate::impl_standard_methods!(
    ParticleTypeRegistry,
    ParticleTypeRef,
    particle_types_by_id,
    particle_types_by_key,
    allows_registering
);

crate::impl_registry!(
    ParticleTypeRegistry,
    ParticleType,
    particle_types_by_id,
    particle_types_by_key,
    particle_types
);
