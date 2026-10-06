//! Clientbound update mob effect packet.

use steel_macros::{ClientPacket, WriteTo};
use steel_registry::mob_effect::MobEffectRef;
use steel_registry::packets::play::C_UPDATE_MOB_EFFECT;

const FLAG_AMBIENT: u8 = 0x01;
const FLAG_VISIBLE: u8 = 0x02;
const FLAG_SHOW_ICON: u8 = 0x04;
const FLAG_BLEND: u8 = 0x08;

/// Sent when the client should add or update an entity mob effect.
///
/// Vanilla: `ClientboundUpdateMobEffectPacket`.
#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_UPDATE_MOB_EFFECT)]
pub struct CUpdateMobEffect {
    #[write(as = VarInt)]
    pub entity_id: i32,
    /// Holder-registry mob effect id.
    #[write(as = VarInt)]
    pub effect_id: i32,
    #[write(as = VarInt)]
    pub amplifier: i32,
    #[write(as = VarInt)]
    pub duration: i32,
    pub flags: u8,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MobEffectPacketFlags {
    pub ambient: bool,
    pub visible: bool,
    pub show_icon: bool,
    pub blend: bool,
}

impl CUpdateMobEffect {
    #[must_use]
    pub fn new(
        entity_id: i32,
        effect: MobEffectRef,
        amplifier: i32,
        duration: i32,
        flags: MobEffectPacketFlags,
    ) -> Self {
        Self {
            entity_id,
            effect_id: effect.packet_holder_id(),
            amplifier,
            duration,
            flags: mob_effect_flags(flags),
        }
    }
}

const fn mob_effect_flags(packet_flags: MobEffectPacketFlags) -> u8 {
    let mut flags = 0;
    if packet_flags.ambient {
        flags |= FLAG_AMBIENT;
    }
    if packet_flags.visible {
        flags |= FLAG_VISIBLE;
    }
    if packet_flags.show_icon {
        flags |= FLAG_SHOW_ICON;
    }
    if packet_flags.blend {
        flags |= FLAG_BLEND;
    }
    flags
}

#[cfg(test)]
mod tests {

    use steel_registry::init_vanilla_registry;
    use steel_registry::vanilla_mob_effects;

    use super::*;

    #[test]
    fn update_mob_effect_uses_raw_holder_registry_id() {
        init_vanilla_registry();

        let packet = CUpdateMobEffect::new(
            42,
            vanilla_mob_effects::SPEED,
            0,
            100,
            MobEffectPacketFlags::default(),
        );

        assert_eq!(packet.effect_id, 0);
    }
}
