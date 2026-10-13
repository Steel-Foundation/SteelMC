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
struct WeightedExplosionParticleInfo {
    value: ExplosionParticleInfo,
    #[write(as = VarInt)]
    weight: i32,
}

/// A validated weighted palette for an explosion's block particles.
#[derive(WriteTo, Clone, Debug)]
pub struct ExplosionParticlePalette(Vec<WeightedExplosionParticleInfo>);

impl ExplosionParticlePalette {
    /// Validates Vanilla's non-negative weights and signed 32-bit list limits.
    pub fn try_new(
        entries: Vec<(ExplosionParticleInfo, i32)>,
    ) -> Result<Self, ExplosionParticleWeightError> {
        if i32::try_from(entries.len()).is_err() {
            return Err(ExplosionParticleWeightError::TooManyEntries(entries.len()));
        }

        let mut total = 0_i32;
        let mut weighted = Vec::with_capacity(entries.len());
        for (value, weight) in entries {
            if weight < 0 {
                return Err(ExplosionParticleWeightError::Negative(weight));
            }
            total = total
                .checked_add(weight)
                .ok_or(ExplosionParticleWeightError::TotalOverflow)?;
            weighted.push(WeightedExplosionParticleInfo { value, weight });
        }
        Ok(Self(weighted))
    }
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

#[cfg(test)]
mod tests {
    use glam::DVec3;
    use steel_registry::{
        RegistryEntry, init_vanilla_registry, particle_type::ParticleData,
        sound_event::SoundEventHolder, sound_events, vanilla_particle_types,
    };
    use steel_utils::{Identifier, codec::VarInt, serial::WriteTo};

    use super::{
        CExplode, ExplosionParticleInfo, ExplosionParticlePalette, ExplosionParticleWeightError,
    };

    #[test]
    fn writes_fields_in_vanilla_wire_order() {
        init_vanilla_registry();
        let packet = CExplode {
            center: DVec3::new(1.25, -2.5, 3.75),
            radius: 4.0,
            block_count: 17,
            player_knockback: Some(DVec3::new(-0.25, 0.5, 1.0)),
            explosion_particle: ParticleData::simple(&vanilla_particle_types::EXPLOSION),
            explosion_sound: SoundEventHolder::registry(&sound_events::ENTITY_GENERIC_EXPLODE),
            block_particles: palette(vec![
                (
                    ExplosionParticleInfo {
                        particle: ParticleData::simple(&vanilla_particle_types::POOF),
                        scaling: 0.5,
                        speed: 1.0,
                    },
                    2,
                ),
                (
                    ExplosionParticleInfo {
                        particle: ParticleData::simple(&vanilla_particle_types::SMOKE),
                        scaling: 1.0,
                        speed: 0.75,
                    },
                    3,
                ),
            ]),
        };

        let mut encoded = Vec::new();
        assert!(packet.write(&mut encoded).is_ok());

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
        write_registry_id(&mut expected, vanilla_particle_types::EXPLOSION.id());
        assert!(
            VarInt(sound_events::ENTITY_GENERIC_EXPLODE.packet_holder_id())
                .write(&mut expected)
                .is_ok()
        );

        assert!(VarInt(2).write(&mut expected).is_ok());
        write_registry_id(&mut expected, vanilla_particle_types::POOF.id());
        expected.extend_from_slice(&0.5_f32.to_be_bytes());
        expected.extend_from_slice(&1.0_f32.to_be_bytes());
        assert!(VarInt(2).write(&mut expected).is_ok());
        write_registry_id(&mut expected, vanilla_particle_types::SMOKE.id());
        expected.extend_from_slice(&1.0_f32.to_be_bytes());
        expected.extend_from_slice(&0.75_f32.to_be_bytes());
        assert!(VarInt(3).write(&mut expected).is_ok());

        assert_eq!(encoded, expected);
    }

    #[test]
    fn writes_direct_explosion_sound_holder() {
        init_vanilla_registry();
        let sound_id = Identifier::from_steel("test_explosion");
        let packet = CExplode {
            center: DVec3::ZERO,
            radius: 0.0,
            block_count: 0,
            player_knockback: None,
            explosion_particle: ParticleData::simple(&vanilla_particle_types::EXPLOSION),
            explosion_sound: SoundEventHolder::Direct {
                sound_id: sound_id.clone(),
                fixed_range: Some(24.0),
            },
            block_particles: palette(Vec::new()),
        };

        let mut encoded = Vec::new();
        assert!(packet.write(&mut encoded).is_ok());

        let mut expected = vec![0; 24];
        expected.extend_from_slice(&0.0_f32.to_be_bytes());
        expected.extend_from_slice(&0_i32.to_be_bytes());
        expected.push(0);
        write_registry_id(&mut expected, vanilla_particle_types::EXPLOSION.id());
        assert!(VarInt(0).write(&mut expected).is_ok());
        assert!(sound_id.write(&mut expected).is_ok());
        expected.push(1);
        expected.extend_from_slice(&24.0_f32.to_be_bytes());
        assert!(VarInt(0).write(&mut expected).is_ok());

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
            ExplosionParticlePalette::try_new(vec![(particle(), -1)]),
            Err(ExplosionParticleWeightError::Negative(-1))
        ));
        assert!(matches!(
            ExplosionParticlePalette::try_new(vec![(particle(), i32::MAX), (particle(), 1),]),
            Err(ExplosionParticleWeightError::TotalOverflow)
        ));
    }

    fn palette(entries: Vec<(ExplosionParticleInfo, i32)>) -> ExplosionParticlePalette {
        let Ok(palette) = ExplosionParticlePalette::try_new(entries) else {
            panic!("test explosion particle palette should be valid");
        };
        palette
    }

    fn write_registry_id(output: &mut Vec<u8>, id: usize) {
        let Ok(id) = i32::try_from(id) else {
            panic!("test registry id should fit in i32");
        };
        assert!(VarInt(id).write(output).is_ok());
    }
}
