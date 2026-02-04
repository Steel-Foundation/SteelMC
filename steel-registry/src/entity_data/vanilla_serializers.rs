//! Vanilla entity data serializer registration.
//!
//! This module registers all vanilla entity data serializers in the exact order
//! they appear in vanilla's `EntityDataSerializers.java`. The registration order
//! determines the serializer ID used in the network protocol.

use std::io;

use steel_utils::{
    codec::{VarInt, VarLong},
    serial::{PrefixedWrite, WriteTo},
};

use super::{EntityData, EntityDataSerializerRegistry};

/// Simple serializer: extract value and call `.write(buf)`.
macro_rules! ser_write {
    ($name:ident, $variant:ident) => {
        fn $name(data: &EntityData, buf: &mut Vec<u8>) -> io::Result<()> {
            match data {
                EntityData::$variant(v) => v.write(buf),
                _ => Err(io::Error::other(concat!("Expected ", stringify!($variant)))),
            }
        }
    };
}

/// Serializer that wraps value in VarInt.
macro_rules! ser_varint {
    ($name:ident, $variant:ident) => {
        fn $name(data: &EntityData, buf: &mut Vec<u8>) -> io::Result<()> {
            match data {
                EntityData::$variant(v) => VarInt(*v).write(buf),
                _ => Err(io::Error::other(concat!("Expected ", stringify!($variant)))),
            }
        }
    };
}

/// Serializer that casts enum to i32 then writes as VarInt.
macro_rules! ser_enum_varint {
    ($name:ident, $variant:ident) => {
        fn $name(data: &EntityData, buf: &mut Vec<u8>) -> io::Result<()> {
            match data {
                EntityData::$variant(v) => VarInt(*v as i32).write(buf),
                _ => Err(io::Error::other(concat!("Expected ", stringify!($variant)))),
            }
        }
    };
}

// Simple write serializers
ser_write!(ser_byte, Byte);
ser_write!(ser_float, Float);
ser_write!(ser_component, Component);
ser_write!(ser_item_stack, ItemStack);
ser_write!(ser_boolean, Boolean);
ser_write!(ser_block_state, BlockState);

// VarInt serializers (for i32 holder/registry IDs)
ser_varint!(ser_int, Int);
ser_varint!(ser_cat_variant, CatVariant);
ser_varint!(ser_cow_variant, CowVariant);
ser_varint!(ser_wolf_variant, WolfVariant);
ser_varint!(ser_wolf_sound_variant, WolfSoundVariant);
ser_varint!(ser_frog_variant, FrogVariant);
ser_varint!(ser_pig_variant, PigVariant);
ser_varint!(ser_chicken_variant, ChickenVariant);
ser_varint!(ser_zombie_nautilus_variant, ZombieNautilusVariant);
ser_varint!(ser_painting_variant, PaintingVariant);
ser_varint!(ser_copper_golem_state, CopperGolemState);
ser_varint!(ser_weathering_copper_state, WeatheringCopperState);

// Enum as VarInt serializers
ser_enum_varint!(ser_direction, Direction);
ser_enum_varint!(ser_pose, Pose);
ser_enum_varint!(ser_sniffer_state, SnifferState);
ser_enum_varint!(ser_armadillo_state, ArmadilloState);

fn ser_long(data: &EntityData, buf: &mut Vec<u8>) -> io::Result<()> {
    match data {
        EntityData::Long(v) => VarLong(*v).write(buf),
        _ => Err(io::Error::other("Expected Long")),
    }
}

fn ser_string(data: &EntityData, buf: &mut Vec<u8>) -> io::Result<()> {
    match data {
        EntityData::String(v) => v.write_prefixed::<VarInt>(buf),
        _ => Err(io::Error::other("Expected String")),
    }
}

fn ser_optional_component(data: &EntityData, buf: &mut Vec<u8>) -> io::Result<()> {
    match data {
        EntityData::OptionalComponent(v) => match v {
            Some(comp) => {
                true.write(buf)?;
                comp.write(buf)
            }
            None => false.write(buf),
        },
        _ => Err(io::Error::other("Expected OptionalComponent")),
    }
}

fn ser_rotations(data: &EntityData, buf: &mut Vec<u8>) -> io::Result<()> {
    match data {
        EntityData::Rotations(v) => {
            v.x.write(buf)?;
            v.y.write(buf)?;
            v.z.write(buf)
        }
        _ => Err(io::Error::other("Expected Rotations")),
    }
}

fn ser_block_pos(data: &EntityData, buf: &mut Vec<u8>) -> io::Result<()> {
    match data {
        EntityData::BlockPos(v) => v.as_i64().write(buf),
        _ => Err(io::Error::other("Expected BlockPos")),
    }
}

fn ser_optional_block_pos(data: &EntityData, buf: &mut Vec<u8>) -> io::Result<()> {
    match data {
        EntityData::OptionalBlockPos(v) => match v {
            Some(pos) => {
                true.write(buf)?;
                pos.as_i64().write(buf)
            }
            None => false.write(buf),
        },
        _ => Err(io::Error::other("Expected OptionalBlockPos")),
    }
}

fn ser_optional_living_entity_reference(data: &EntityData, buf: &mut Vec<u8>) -> io::Result<()> {
    match data {
        EntityData::OptionalLivingEntityRef(v) => match v {
            Some(uuid) => {
                true.write(buf)?;
                uuid.write(buf)
            }
            None => false.write(buf),
        },
        _ => Err(io::Error::other("Expected OptionalLivingEntityRef")),
    }
}

fn ser_optional_block_state(data: &EntityData, buf: &mut Vec<u8>) -> io::Result<()> {
    match data {
        EntityData::OptionalBlockState(v) => {
            // Vanilla encodes: 0 = absent, otherwise raw block state ID
            match v {
                Some(state) => VarInt(i32::from(state.0)).write(buf),
                None => VarInt(0).write(buf),
            }
        }
        _ => Err(io::Error::other("Expected OptionalBlockState")),
    }
}

fn ser_particle(data: &EntityData, buf: &mut Vec<u8>) -> io::Result<()> {
    match data {
        EntityData::Particle(v) => {
            VarInt(v.particle_type).write(buf)
            // TODO: Write particle-specific options based on type
        }
        _ => Err(io::Error::other("Expected Particle")),
    }
}

fn ser_particles(data: &EntityData, buf: &mut Vec<u8>) -> io::Result<()> {
    match data {
        EntityData::Particles(v) => {
            VarInt(v.particles.len() as i32).write(buf)?;
            for particle in &v.particles {
                VarInt(particle.particle_type).write(buf)?;
                // TODO: Write particle-specific options
            }
            Ok(())
        }
        _ => Err(io::Error::other("Expected Particles")),
    }
}

fn ser_villager_data(data: &EntityData, buf: &mut Vec<u8>) -> io::Result<()> {
    match data {
        EntityData::VillagerData(v) => {
            VarInt(v.villager_type).write(buf)?;
            VarInt(v.profession).write(buf)?;
            VarInt(v.level).write(buf)
        }
        _ => Err(io::Error::other("Expected VillagerData")),
    }
}

fn ser_optional_unsigned_int(data: &EntityData, buf: &mut Vec<u8>) -> io::Result<()> {
    match data {
        EntityData::OptionalUnsignedInt(v) => {
            // Encoded as VarInt: 0 = absent, otherwise value + 1
            VarInt(v.map(|x| x as i32 + 1).unwrap_or(0)).write(buf)
        }
        _ => Err(io::Error::other("Expected OptionalUnsignedInt")),
    }
}

fn ser_optional_global_pos(data: &EntityData, buf: &mut Vec<u8>) -> io::Result<()> {
    match data {
        EntityData::OptionalGlobalPos(v) => match v {
            Some(global_pos) => {
                true.write(buf)?;
                global_pos.dimension.write(buf)?;
                global_pos.pos.as_i64().write(buf)
            }
            None => false.write(buf),
        },
        _ => Err(io::Error::other("Expected OptionalGlobalPos")),
    }
}

fn ser_vector3(data: &EntityData, buf: &mut Vec<u8>) -> io::Result<()> {
    match data {
        EntityData::Vector3(v) => {
            v.x.write(buf)?;
            v.y.write(buf)?;
            v.z.write(buf)
        }
        _ => Err(io::Error::other("Expected Vector3")),
    }
}

fn ser_quaternion(data: &EntityData, buf: &mut Vec<u8>) -> io::Result<()> {
    match data {
        EntityData::Quaternion(v) => {
            v.x.write(buf)?;
            v.y.write(buf)?;
            v.z.write(buf)?;
            v.w.write(buf)
        }
        _ => Err(io::Error::other("Expected Quaternion")),
    }
}

fn ser_resolvable_profile(data: &EntityData, buf: &mut Vec<u8>) -> io::Result<()> {
    match data {
        EntityData::ResolvableProfile(_v) => {
            // TODO: Implement proper profile serialization
            // For now, write "no name, no UUID" which is the empty profile
            false.write(buf)?; // has name
            false.write(buf) // has UUID
        }
        _ => Err(io::Error::other("Expected ResolvableProfile")),
    }
}

fn ser_humanoid_arm(data: &EntityData, buf: &mut Vec<u8>) -> io::Result<()> {
    match data {
        // Vanilla uses ByteBufCodecs.idMapper which writes as VarInt
        EntityData::HumanoidArm(v) => VarInt(*v as i32).write(buf),
        _ => Err(io::Error::other("Expected HumanoidArm")),
    }
}

// ==================== Registration ====================

/// Register all vanilla entity data serializers.
///
/// **IMPORTANT**: The registration order MUST match vanilla's `EntityDataSerializers.java` exactly,
/// as the serializer's network ID is determined by its registration order.
pub fn register_vanilla_entity_data_serializers(registry: &mut EntityDataSerializerRegistry) {
    // Order matches EntityDataSerializers.java static block

    // 0: BYTE
    registry.register("byte", ser_byte);
    // 1: INT (VarInt)
    registry.register("int", ser_int);
    // 2: LONG (VarLong)
    registry.register("long", ser_long);
    // 3: FLOAT
    registry.register("float", ser_float);
    // 4: STRING
    registry.register("string", ser_string);
    // 5: COMPONENT
    registry.register("component", ser_component);
    // 6: OPTIONAL_COMPONENT
    registry.register("optional_component", ser_optional_component);
    // 7: ITEM_STACK
    registry.register("item_stack", ser_item_stack);
    // 8: BOOLEAN
    registry.register("boolean", ser_boolean);
    // 9: ROTATIONS
    registry.register("rotations", ser_rotations);
    // 10: BLOCK_POS
    registry.register("block_pos", ser_block_pos);
    // 11: OPTIONAL_BLOCK_POS
    registry.register("optional_block_pos", ser_optional_block_pos);
    // 12: DIRECTION
    registry.register("direction", ser_direction);
    // 13: OPTIONAL_LIVING_ENTITY_REFERENCE
    registry.register(
        "optional_living_entity_reference",
        ser_optional_living_entity_reference,
    );
    // 14: BLOCK_STATE
    registry.register("block_state", ser_block_state);
    // 15: OPTIONAL_BLOCK_STATE
    registry.register("optional_block_state", ser_optional_block_state);
    // 16: PARTICLE
    registry.register("particle", ser_particle);
    // 17: PARTICLES
    registry.register("particles", ser_particles);
    // 18: VILLAGER_DATA
    registry.register("villager_data", ser_villager_data);
    // 19: OPTIONAL_UNSIGNED_INT
    registry.register("optional_unsigned_int", ser_optional_unsigned_int);
    // 20: POSE
    registry.register("pose", ser_pose);
    // 21: CAT_VARIANT
    registry.register("cat_variant", ser_cat_variant);
    // 22: COW_VARIANT
    registry.register("cow_variant", ser_cow_variant);
    // 23: WOLF_VARIANT
    registry.register("wolf_variant", ser_wolf_variant);
    // 24: WOLF_SOUND_VARIANT
    registry.register("wolf_sound_variant", ser_wolf_sound_variant);
    // 25: FROG_VARIANT
    registry.register("frog_variant", ser_frog_variant);
    // 26: PIG_VARIANT
    registry.register("pig_variant", ser_pig_variant);
    // 27: CHICKEN_VARIANT
    registry.register("chicken_variant", ser_chicken_variant);
    // 28: ZOMBIE_NAUTILUS_VARIANT
    registry.register("zombie_nautilus_variant", ser_zombie_nautilus_variant);
    // 29: OPTIONAL_GLOBAL_POS
    registry.register("optional_global_pos", ser_optional_global_pos);
    // 30: PAINTING_VARIANT
    registry.register("painting_variant", ser_painting_variant);
    // 31: SNIFFER_STATE
    registry.register("sniffer_state", ser_sniffer_state);
    // 32: ARMADILLO_STATE
    registry.register("armadillo_state", ser_armadillo_state);
    // 33: COPPER_GOLEM_STATE
    registry.register("copper_golem_state", ser_copper_golem_state);
    // 34: WEATHERING_COPPER_STATE
    registry.register("weathering_copper_state", ser_weathering_copper_state);
    // 35: VECTOR3
    registry.register("vector3", ser_vector3);
    // 36: QUATERNION
    registry.register("quaternion", ser_quaternion);
    // 37: RESOLVABLE_PROFILE
    registry.register("resolvable_profile", ser_resolvable_profile);
    // 38: HUMANOID_ARM
    registry.register("humanoid_arm", ser_humanoid_arm);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serializer_registration_order() {
        let mut registry = EntityDataSerializerRegistry::new();
        register_vanilla_entity_data_serializers(&mut registry);

        // Verify key serializers have correct IDs
        assert_eq!(registry.get_id("byte"), Some(0));
        assert_eq!(registry.get_id("int"), Some(1));
        assert_eq!(registry.get_id("long"), Some(2));
        assert_eq!(registry.get_id("float"), Some(3));
        assert_eq!(registry.get_id("boolean"), Some(8));
        assert_eq!(registry.get_id("pose"), Some(20));
        assert_eq!(registry.get_id("humanoid_arm"), Some(38));

        // Total count
        assert_eq!(registry.len(), 39);
    }

    #[test]
    fn test_serializers_write_correctly() {
        let mut registry = EntityDataSerializerRegistry::new();
        register_vanilla_entity_data_serializers(&mut registry);

        // Test byte serializer
        let writer = registry.get_writer(0).unwrap();
        let mut buf = Vec::new();
        writer(&EntityData::Byte(42), &mut buf).unwrap();
        assert_eq!(buf, vec![42]);

        // Test int serializer (VarInt)
        let writer = registry.get_writer(1).unwrap();
        let mut buf = Vec::new();
        writer(&EntityData::Int(300), &mut buf).unwrap();
        assert_eq!(buf, vec![0xAC, 0x02]); // 300 as VarInt

        // Test float serializer
        let writer = registry.get_writer(3).unwrap();
        let mut buf = Vec::new();
        writer(&EntityData::Float(1.5), &mut buf).unwrap();
        assert_eq!(buf, 1.5f32.to_be_bytes().to_vec());

        // Test boolean serializer
        let writer = registry.get_writer(8).unwrap();
        let mut buf = Vec::new();
        writer(&EntityData::Boolean(true), &mut buf).unwrap();
        assert_eq!(buf, vec![1]);
    }
}
