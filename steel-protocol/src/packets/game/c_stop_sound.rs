use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_STOP_SOUND;
use steel_utils::Identifier;

use super::SoundSource;

const SOURCE_FLAG: u8 = 1;
const SOUND_FLAG: u8 = 2;

#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_STOP_SOUND)]
pub struct CStopSound {
    pub flags: u8,

    #[write(as = Unprefixed)]
    pub source: Option<SoundSource>,

    #[write(as = Unprefixed)]
    pub sound: Option<Identifier>,
}

impl CStopSound {
    #[must_use]
    pub const fn new(source: Option<SoundSource>, sound: Option<Identifier>) -> Self {
        let mut flags = 0;

        if source.is_some() {
            flags |= SOURCE_FLAG;
        }

        if sound.is_some() {
            flags |= SOUND_FLAG;
        }

        Self {
            flags,
            source,
            sound,
        }
    }
}
