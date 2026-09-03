use glam::DVec3;
use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_EXPLODE;
use steel_registry::particle_type::ParticleData;
use steel_registry::sound_event::SoundEventRef;

/// Vanilla `ExplosionParticleInfo`: a particle type plus its scale and speed multipliers.
#[derive(WriteTo, Clone, Debug)]
pub struct ExplosionParticleInfo {
    pub particle: ParticleData,
    pub scaling: f32,
    pub speed: f32,
}

impl ExplosionParticleInfo {
    /// Creates info with vanilla's default scale and speed multipliers.
    #[must_use]
    pub fn new(particle: ParticleData) -> Self {
        Self {
            particle,
            scaling: 1.0,
            speed: 1.0,
        }
    }
}

/// One weighted entry of the explosion's block-particle list.
///
/// Vanilla encodes `WeightedList<ExplosionParticleInfo>` as a `VarInt`-length list of
/// `VarInt` weight followed by the entry payload.
#[derive(WriteTo, Clone, Debug)]
pub struct WeightedExplosionParticle {
    #[write(as = VarInt)]
    pub weight: i32,
    pub info: ExplosionParticleInfo,
}

/// Sent when an explosion happens. The client plays the particle, sound, block-particle
/// trail, and screen shake locally and applies its own knockback from `player_knockback`.
#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_EXPLODE)]
pub struct CExplode {
    pub center: DVec3,
    pub radius: f32,
    /// Number of block positions the server destroyed; drives the client's block-particle trail.
    pub block_count: i32,
    /// Velocity applied to the receiving player, present only when they were hit.
    pub player_knockback: Option<DVec3>,
    pub explosion_particle: ParticleData,
    /// The holder-encoded sound event ID (`VarInt`).
    #[write(as = VarInt)]
    pub sound_id: i32,
    #[write(as = Prefixed(VarInt))]
    pub block_particles: Vec<WeightedExplosionParticle>,
}

impl CExplode {
    /// Creates an explosion packet with no per-player knockback and no block-particle trail.
    #[must_use]
    pub fn new(
        center: DVec3,
        radius: f32,
        block_count: i32,
        explosion_particle: ParticleData,
        sound: SoundEventRef,
    ) -> Self {
        Self {
            center,
            radius,
            block_count,
            player_knockback: None,
            explosion_particle,
            sound_id: sound.packet_holder_id(),
            block_particles: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::sound_events;
    use steel_registry::{RegistryEntry, init_vanilla_registry, vanilla_particle_types};
    use steel_utils::codec::VarInt;
    use steel_utils::serial::WriteTo;

    use super::*;

    #[test]
    fn writes_fields_in_vanilla_wire_order() {
        init_vanilla_registry();

        let packet = CExplode::new(
            DVec3::new(1.0, 2.0, 3.0),
            3.0,
            7,
            ParticleData::simple(&vanilla_particle_types::EXPLOSION),
            &sound_events::ENTITY_GENERIC_EXPLODE,
        );

        let mut encoded = Vec::new();
        let Ok(()) = packet.write(&mut encoded) else {
            panic!("explode packet should encode");
        };

        let mut expected = Vec::new();
        expected.extend_from_slice(&1.0_f64.to_be_bytes());
        expected.extend_from_slice(&2.0_f64.to_be_bytes());
        expected.extend_from_slice(&3.0_f64.to_be_bytes());
        expected.extend_from_slice(&3.0_f32.to_be_bytes());
        expected.extend_from_slice(&7_i32.to_be_bytes());
        expected.push(0); // Optional<Vec3> absent
        let Ok(explosion_id) = i32::try_from(vanilla_particle_types::EXPLOSION.id()) else {
            panic!("explosion particle id should fit in i32");
        };
        let Ok(()) = VarInt(explosion_id).write(&mut expected) else {
            panic!("particle id should encode");
        };
        let Ok(()) = VarInt(sound_events::ENTITY_GENERIC_EXPLODE.packet_holder_id())
            .write(&mut expected)
        else {
            panic!("sound id should encode");
        };
        let Ok(()) = VarInt(0).write(&mut expected) else {
            panic!("empty weighted list length should encode");
        };

        assert_eq!(encoded, expected);
    }

    #[test]
    fn writes_player_knockback_when_present() {
        init_vanilla_registry();

        let mut packet = CExplode::new(
            DVec3::ZERO,
            4.0,
            0,
            ParticleData::simple(&vanilla_particle_types::EXPLOSION_EMITTER),
            &sound_events::ENTITY_GENERIC_EXPLODE,
        );
        packet.player_knockback = Some(DVec3::new(0.5, -0.25, 1.0));

        let mut encoded = Vec::new();
        let Ok(()) = packet.write(&mut encoded) else {
            panic!("explode packet should encode");
        };

        let mut expected = Vec::new();
        expected.extend_from_slice(&0.0_f64.to_be_bytes());
        expected.extend_from_slice(&0.0_f64.to_be_bytes());
        expected.extend_from_slice(&0.0_f64.to_be_bytes());
        expected.extend_from_slice(&4.0_f32.to_be_bytes());
        expected.extend_from_slice(&0_i32.to_be_bytes());
        expected.push(1);
        expected.extend_from_slice(&0.5_f64.to_be_bytes());
        expected.extend_from_slice(&(-0.25_f64).to_be_bytes());
        expected.extend_from_slice(&1.0_f64.to_be_bytes());

        assert_eq!(&encoded[..expected.len()], expected.as_slice());
    }
}
