use glam::DVec3;
use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_EXPLODE;
use steel_registry::particle_type::{ExplosionParticleInfo, ParticleData};
use steel_registry::sound_event::SoundEventHolder;
use steel_utils::random::weighted::Weighted;

/// Tells the client an explosion happened, so it can play the effect.
///
/// The server does not send the resulting block changes here; those arrive as ordinary
/// block updates. `player_knockback` differs per recipient, so this packet is built once
/// per player rather than broadcast verbatim.
#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_EXPLODE)]
pub struct CExplode {
    pub center: DVec3,
    pub radius: f32,
    /// How many blocks the blast destroyed, which scales the client's particle burst.
    ///
    /// Vanilla writes this with `ByteBufCodecs.INT`, so it is a plain big-endian `i32`
    /// rather than the `VarInt` most counts use.
    pub block_count: i32,
    /// The velocity this recipient should adopt, when the blast threw them.
    pub player_knockback: Option<DVec3>,
    pub explosion_particle: ParticleData,
    pub explosion_sound: SoundEventHolder,
    /// The spread the client picks from when painting debris.
    pub block_particles: Vec<Weighted<ExplosionParticleInfo>>,
}

#[cfg(test)]
mod tests {
    use glam::DVec3;
    use steel_registry::particle_type::{ExplosionParticleInfo, ParticleData};
    use steel_registry::sound_event::SoundEventHolder;
    use steel_registry::{
        RegistryEntry, init_vanilla_registry, sound_events, vanilla_particle_types,
    };
    use steel_utils::random::weighted::Weighted;
    use steel_utils::{codec::VarInt, serial::WriteTo};

    use super::CExplode;

    #[test]
    fn writes_fields_in_vanilla_wire_order() {
        init_vanilla_registry();

        let packet = CExplode {
            center: DVec3::new(1.25, -2.5, 3.75),
            radius: 4.0,
            block_count: 17,
            player_knockback: Some(DVec3::new(0.5, 0.25, -0.5)),
            explosion_particle: ParticleData::simple(&vanilla_particle_types::EXPLOSION),
            explosion_sound: SoundEventHolder::registry(&sound_events::ENTITY_GENERIC_EXPLODE),
            block_particles: vec![Weighted::unit(ExplosionParticleInfo::new(
                ParticleData::simple(&vanilla_particle_types::POOF),
                0.5,
                1.0,
            ))],
        };

        let mut encoded = Vec::new();
        packet
            .write(&mut encoded)
            .expect("explode packet should encode");

        let mut expected = Vec::new();
        for component in [1.25_f64, -2.5, 3.75] {
            expected.extend_from_slice(&component.to_be_bytes());
        }
        expected.extend_from_slice(&4.0_f32.to_be_bytes());
        // A big-endian i32, not a VarInt; this is the field a port gets wrong.
        expected.extend_from_slice(&17_i32.to_be_bytes());

        expected.push(1);
        for component in [0.5_f64, 0.25, -0.5] {
            expected.extend_from_slice(&component.to_be_bytes());
        }

        write_particle_id(&mut expected, &vanilla_particle_types::EXPLOSION);
        VarInt(sound_events::ENTITY_GENERIC_EXPLODE.packet_holder_id())
            .write(&mut expected)
            .expect("sound holder id should encode");

        VarInt(1)
            .write(&mut expected)
            .expect("list length should encode");
        write_particle_id(&mut expected, &vanilla_particle_types::POOF);
        expected.extend_from_slice(&0.5_f32.to_be_bytes());
        expected.extend_from_slice(&1.0_f32.to_be_bytes());
        VarInt(Weighted::<()>::DEFAULT_WEIGHT)
            .write(&mut expected)
            .expect("weight should encode");

        assert_eq!(encoded, expected);
    }

    fn write_particle_id(
        buffer: &mut Vec<u8>,
        particle: steel_registry::particle_type::ParticleTypeRef,
    ) {
        let id = i32::try_from(particle.id()).expect("particle id should fit in i32");
        VarInt(id).write(buffer).expect("particle id should encode");
    }
}
