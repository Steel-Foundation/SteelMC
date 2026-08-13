use std::io::{self, Write};

use glam::DVec3;
use steel_macros::{ClientPacket, WriteTo};
use steel_registry::{
    packets::play::C_EXPLODE, particle_type::ParticleData, sound_event::SoundEventHolder,
};
use thiserror::Error;

/// A block-debris particle and the client-side scaling applied to it.
#[derive(WriteTo, Clone, Debug)]
pub struct ExplosionParticleInfo {
    pub particle: ParticleData,
    pub scaling: f32,
    pub speed: f32,
}

/// A weighted entry in an explosion's block-particle palette.
#[derive(WriteTo, Clone, Debug)]
pub struct WeightedExplosionParticleInfo {
    value: ExplosionParticleInfo,
    #[write(as = VarInt)]
    weight: i32,
}

impl WeightedExplosionParticleInfo {
    /// Creates an entry with Vanilla's non-negative weight invariant.
    pub fn try_new(
        value: ExplosionParticleInfo,
        weight: i32,
    ) -> Result<Self, ExplosionParticleWeightError> {
        if weight < 0 {
            return Err(ExplosionParticleWeightError::Negative(weight));
        }
        Ok(Self { value, weight })
    }
}

/// A validated weighted palette for an explosion's block particles.
#[derive(Clone, Debug)]
pub struct ExplosionParticlePalette(Vec<WeightedExplosionParticleInfo>);

impl ExplosionParticlePalette {
    /// Validates Vanilla's signed 32-bit total-weight limit.
    pub fn try_new(
        entries: Vec<WeightedExplosionParticleInfo>,
    ) -> Result<Self, ExplosionParticleWeightError> {
        validate_entry_count(entries.len())?;
        let mut total = 0_i32;
        for entry in &entries {
            total = total
                .checked_add(entry.weight)
                .ok_or(ExplosionParticleWeightError::TotalOverflow)?;
        }
        Ok(Self(entries))
    }
}

impl steel_utils::serial::WriteTo for ExplosionParticlePalette {
    fn write(&self, writer: &mut impl Write) -> io::Result<()> {
        let count = i32::try_from(self.0.len()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "explosion particle palette exceeds i32::MAX entries",
            )
        })?;
        steel_utils::serial::WriteTo::write(&steel_utils::codec::VarInt(count), writer)?;
        for entry in &self.0 {
            steel_utils::serial::WriteTo::write(entry, writer)?;
        }
        Ok(())
    }
}

fn validate_entry_count(entry_count: usize) -> Result<(), ExplosionParticleWeightError> {
    if i32::try_from(entry_count).is_err() {
        return Err(ExplosionParticleWeightError::TooManyEntries(entry_count));
    }
    Ok(())
}

/// An invalid block-particle weight supplied for an explosion packet.
#[derive(Error, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExplosionParticleWeightError {
    /// Vanilla rejects individual negative weights.
    #[error("explosion particle weight cannot be negative: {0}")]
    Negative(i32),
    /// Vanilla rejects weighted lists whose total exceeds `i32::MAX`.
    #[error("total explosion particle weight exceeds i32::MAX")]
    TotalOverflow,
    /// Vanilla's list codec uses a signed 32-bit entry count.
    #[error("explosion particle palette has too many entries: {0}")]
    TooManyEntries(usize),
}

/// Sent to apply the client-visible effects of a server-side explosion.
#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_EXPLODE)]
pub struct CExplode {
    pub center: DVec3,
    pub radius: f32,
    pub block_count: i32,
    pub player_knockback: Option<DVec3>,
    pub explosion_particle: ParticleData,
    pub explosion_sound: SoundEventHolder,
    pub block_particles: ExplosionParticlePalette,
}

impl CExplode {
    #[must_use]
    pub const fn new(
        center: DVec3,
        radius: f32,
        block_count: i32,
        player_knockback: Option<DVec3>,
        explosion_particle: ParticleData,
        explosion_sound: SoundEventHolder,
        block_particles: ExplosionParticlePalette,
    ) -> Self {
        Self {
            center,
            radius,
            block_count,
            player_knockback,
            explosion_particle,
            explosion_sound,
            block_particles,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Once;

    use glam::DVec3;
    use steel_registry::{
        REGISTRY, Registry, RegistryEntry, particle_type::ParticleData,
        sound_event::SoundEventHolder, sound_events, vanilla_particle_types,
    };
    use steel_utils::{Identifier, codec::VarInt, serial::WriteTo};

    use super::{
        CExplode, ExplosionParticleInfo, ExplosionParticlePalette, ExplosionParticleWeightError,
        WeightedExplosionParticleInfo, validate_entry_count,
    };

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

        let packet = CExplode::new(
            DVec3::new(1.25, -2.5, 3.75),
            4.0,
            17,
            Some(DVec3::new(-0.25, 0.5, 1.0)),
            ParticleData::simple(&vanilla_particle_types::EXPLOSION),
            SoundEventHolder::registry(&sound_events::ENTITY_GENERIC_EXPLODE),
            palette(vec![
                weighted(
                    ExplosionParticleInfo {
                        particle: ParticleData::simple(&vanilla_particle_types::POOF),
                        scaling: 0.5,
                        speed: 1.0,
                    },
                    2,
                ),
                weighted(
                    ExplosionParticleInfo {
                        particle: ParticleData::simple(&vanilla_particle_types::SMOKE),
                        scaling: 1.0,
                        speed: 0.75,
                    },
                    3,
                ),
            ]),
        );

        let mut encoded = Vec::new();
        let Ok(()) = packet.write(&mut encoded) else {
            panic!("explode packet should encode");
        };

        let mut expected = Vec::new();
        expected.extend_from_slice(&1.25_f64.to_be_bytes());
        expected.extend_from_slice(&(-2.5_f64).to_be_bytes());
        expected.extend_from_slice(&3.75_f64.to_be_bytes());
        expected.extend_from_slice(&4.0_f32.to_be_bytes());
        expected.extend_from_slice(&17_i32.to_be_bytes());
        expected.push(1);
        expected.extend_from_slice(&(-0.25_f64).to_be_bytes());
        expected.extend_from_slice(&0.5_f64.to_be_bytes());
        expected.extend_from_slice(&1.0_f64.to_be_bytes());

        write_registry_id(
            &mut expected,
            vanilla_particle_types::EXPLOSION.id(),
            "explosion particle",
        );
        let Ok(()) =
            VarInt(sound_events::ENTITY_GENERIC_EXPLODE.packet_holder_id()).write(&mut expected)
        else {
            panic!("explosion sound holder id should encode");
        };

        let Ok(()) = VarInt(2).write(&mut expected) else {
            panic!("block particle count should encode");
        };
        write_registry_id(
            &mut expected,
            vanilla_particle_types::POOF.id(),
            "poof particle",
        );
        expected.extend_from_slice(&0.5_f32.to_be_bytes());
        expected.extend_from_slice(&1.0_f32.to_be_bytes());
        let Ok(()) = VarInt(2).write(&mut expected) else {
            panic!("poof weight should encode");
        };
        write_registry_id(
            &mut expected,
            vanilla_particle_types::SMOKE.id(),
            "smoke particle",
        );
        expected.extend_from_slice(&1.0_f32.to_be_bytes());
        expected.extend_from_slice(&0.75_f32.to_be_bytes());
        let Ok(()) = VarInt(3).write(&mut expected) else {
            panic!("smoke weight should encode");
        };

        assert_eq!(encoded, expected);
    }

    #[test]
    fn writes_direct_explosion_sound_holder() {
        init_registry();

        let sound_id = Identifier::from_steel("test_explosion");
        let packet = CExplode::new(
            DVec3::ZERO,
            0.0,
            0,
            None,
            ParticleData::simple(&vanilla_particle_types::EXPLOSION),
            SoundEventHolder::Direct {
                sound_id: sound_id.clone(),
                fixed_range: Some(24.0),
            },
            palette(Vec::new()),
        );

        let mut encoded = Vec::new();
        let Ok(()) = packet.write(&mut encoded) else {
            panic!("explode packet with a direct sound should encode");
        };

        let mut expected = vec![0; 24];
        expected.extend_from_slice(&0.0_f32.to_be_bytes());
        expected.extend_from_slice(&0_i32.to_be_bytes());
        expected.push(0);
        write_registry_id(
            &mut expected,
            vanilla_particle_types::EXPLOSION.id(),
            "explosion particle",
        );
        let Ok(()) = VarInt(0).write(&mut expected) else {
            panic!("direct sound discriminator should encode");
        };
        let Ok(()) = sound_id.write(&mut expected) else {
            panic!("direct sound identifier should encode");
        };
        expected.push(1);
        expected.extend_from_slice(&24.0_f32.to_be_bytes());
        let Ok(()) = VarInt(0).write(&mut expected) else {
            panic!("empty block particle palette should encode");
        };

        assert_eq!(encoded, expected);
    }

    #[test]
    fn rejects_invalid_explosion_particle_weights() {
        let particle = || ExplosionParticleInfo {
            particle: ParticleData::simple(&vanilla_particle_types::POOF),
            scaling: 1.0,
            speed: 1.0,
        };

        assert!(matches!(
            WeightedExplosionParticleInfo::try_new(particle(), -1),
            Err(ExplosionParticleWeightError::Negative(-1))
        ));

        let maximum = weighted(particle(), i32::MAX);
        let one = weighted(particle(), 1);
        assert!(matches!(
            ExplosionParticlePalette::try_new(vec![maximum, one]),
            Err(ExplosionParticleWeightError::TotalOverflow)
        ));
        assert!(matches!(
            validate_entry_count(i32::MAX as usize + 1),
            Err(ExplosionParticleWeightError::TooManyEntries(_))
        ));
    }

    fn weighted(value: ExplosionParticleInfo, weight: i32) -> WeightedExplosionParticleInfo {
        let Ok(entry) = WeightedExplosionParticleInfo::try_new(value, weight) else {
            panic!("test explosion particle weight should be valid");
        };
        entry
    }

    fn palette(entries: Vec<WeightedExplosionParticleInfo>) -> ExplosionParticlePalette {
        let Ok(palette) = ExplosionParticlePalette::try_new(entries) else {
            panic!("test explosion particle palette should be valid");
        };
        palette
    }

    fn write_registry_id(output: &mut Vec<u8>, id: usize, label: &str) {
        let Ok(id) = i32::try_from(id) else {
            panic!("{label} id should fit in i32");
        };
        let Ok(()) = VarInt(id).write(output) else {
            panic!("{label} id should encode");
        };
    }
}
