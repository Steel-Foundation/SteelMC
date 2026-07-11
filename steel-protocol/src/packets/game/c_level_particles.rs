use steel_macros::{ClientPacket, WriteTo};
use steel_registry::{entity_data::ParticleData, packets::play::C_LEVEL_PARTICLES};

/// Sends particle effects at a world position
///
/// vanilla in "ClientboundLevelParticlesPacket"
#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_LEVEL_PARTICLES)]
pub struct CLevelParticles {
    pub override_limiter: bool,
    pub always_show: bool,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub x_dist: f32,
    pub y_dist: f32,
    pub z_dist: f32,
    pub max_speed: f32,
    pub count: i32,
    pub particle: ParticleData,
}

impl CLevelParticles {
    #[must_use]
    pub const fn new(
        particle: ParticleData,
        override_limiter: bool,
        always_show: bool,
        pos: glam::DVec3,
        offset: glam::Vec3,
        max_speed: f32,
        count: i32,
    ) -> Self {
        Self {
            override_limiter,
            always_show,
            x: pos.x,
            y: pos.y,
            z: pos.z,
            x_dist: offset.x,
            y_dist: offset.y,
            z_dist: offset.z,
            max_speed,
            count,
            particle,
        }
    }
}
