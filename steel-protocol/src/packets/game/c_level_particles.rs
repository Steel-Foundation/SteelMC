use steel_macros::{ClientPacket, WriteTo};
use steel_registry::{packets::play::C_LEVEL_PARTICLES, particle_type::ParticleData};

/// Sent to create particles on the client.
///
/// The client samples particle positions and velocities from the supplied
/// distribution. A count of zero has Vanilla's special single-particle
/// velocity behavior.
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

#[cfg(test)]
mod tests {
    use std::sync::Once;

    use steel_registry::{REGISTRY, Registry, RegistryEntry, vanilla_particle_types};
    use steel_utils::{codec::VarInt, serial::WriteTo};

    use super::CLevelParticles;

    static INIT_REGISTRY: Once = Once::new();

    fn init_registry() {
        INIT_REGISTRY.call_once(|| {
            let mut registry = Registry::new_vanilla();
            registry.freeze();
            let _ = REGISTRY.init(registry);
        });
    }

    #[test]
    fn writes_fields_in_vanilla_wire_order() {
        init_registry();

        let packet = CLevelParticles {
            override_limiter: true,
            always_show: false,
            x: 1.25,
            y: -2.5,
            z: 3.75,
            x_dist: 0.5,
            y_dist: -1.0,
            z_dist: 2.0,
            max_speed: 0.125,
            count: -7,
            particle: steel_registry::particle_type::ParticleData::simple(
                &vanilla_particle_types::FLAME,
            ),
        };

        let mut encoded = Vec::new();
        let Ok(()) = packet.write(&mut encoded) else {
            panic!("level particles packet should encode");
        };

        let mut expected = vec![1, 0];
        expected.extend_from_slice(&1.25_f64.to_be_bytes());
        expected.extend_from_slice(&(-2.5_f64).to_be_bytes());
        expected.extend_from_slice(&3.75_f64.to_be_bytes());
        expected.extend_from_slice(&0.5_f32.to_be_bytes());
        expected.extend_from_slice(&(-1.0_f32).to_be_bytes());
        expected.extend_from_slice(&2.0_f32.to_be_bytes());
        expected.extend_from_slice(&0.125_f32.to_be_bytes());
        expected.extend_from_slice(&(-7_i32).to_be_bytes());

        let Ok(flame_id) = i32::try_from(vanilla_particle_types::FLAME.id()) else {
            panic!("flame particle id should fit in i32");
        };
        let Ok(()) = VarInt(flame_id).write(&mut expected) else {
            panic!("flame particle id should encode");
        };

        assert_eq!(encoded, expected);
    }
}
