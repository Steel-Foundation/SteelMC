use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_LEVEL_PARTICLES;

/// Spawns particles at a position on the client.
///
/// Based on vanilla `ClientboundLevelParticlesPacket`. Only simple particle
/// types (no extra options payload) are supported — the trailing particle data
/// is just the particle's registry id as a `VarInt`.
#[derive(WriteTo, ClientPacket, Clone, Debug)]
#[packet_id(Play = C_LEVEL_PARTICLES)]
pub struct CLevelParticles {
    /// If true, particles render past the client's particle-count limit.
    pub override_limiter: bool,
    /// If true, particles show even under the "minimal" particle setting.
    pub always_show: bool,
    /// Center X.
    pub x: f64,
    /// Center Y.
    pub y: f64,
    /// Center Z.
    pub z: f64,
    /// X spread (applied as a gaussian offset client-side).
    pub x_dist: f32,
    /// Y spread.
    pub y_dist: f32,
    /// Z spread.
    pub z_dist: f32,
    /// Particle speed / velocity scale.
    pub max_speed: f32,
    /// Number of particles to spawn.
    pub count: i32,
    /// The particle's registry id.
    #[write(as = VarInt)]
    pub particle_id: i32,
}
