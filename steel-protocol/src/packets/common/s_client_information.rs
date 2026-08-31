use steel_macros::{ReadFrom, ServerPacket};
pub use steel_registry::entity_data::HumanoidArm;

#[derive(ReadFrom, Clone, Debug)]
pub enum ChatVisibility {
    Full = 0,
    System = 1,
    Hidden = 2,
}

#[derive(ReadFrom, Clone, Debug)]
pub enum ParticleStatus {
    All = 0,
    Depraced = 1,
    Minimal = 2,
}

#[derive(ReadFrom, ServerPacket, Clone, Debug)]
pub struct SClientInformation {
    #[read(as = Prefixed(VarInt), bound = 16)]
    pub language: String,
    pub view_distance: i8,
    pub chat_visibility: ChatVisibility,
    pub chat_colors: bool,
    pub model_customization: u8,
    pub main_hand: HumanoidArm,
    pub text_filtering_enabled: bool,
    pub allows_listing: bool,
    pub particle_status: ParticleStatus,
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use steel_utils::serial::ReadFrom as _;

    use super::{ChatVisibility, HumanoidArm, ParticleStatus, SClientInformation};

    #[test]
    fn reads_vanilla_byte_fields_without_consuming_following_settings() {
        const SIGNED_VIEW_DISTANCE: i8 = -2;
        const MODEL_CUSTOMIZATION_WITH_HIGH_BIT_SET: u8 = 0xff;

        let bytes = [
            5,
            b'e',
            b'n',
            b'_',
            b'u',
            b's',
            SIGNED_VIEW_DISTANCE.cast_unsigned(),
            0,
            1,
            MODEL_CUSTOMIZATION_WITH_HIGH_BIT_SET,
            1,
            0,
            1,
            2,
        ];
        let mut cursor = Cursor::new(bytes.as_slice());

        let packet = SClientInformation::read(&mut cursor)
            .unwrap_or_else(|error| panic!("client information should decode: {error}"));

        assert_eq!(packet.language, "en_us");
        assert_eq!(packet.view_distance, SIGNED_VIEW_DISTANCE);
        assert!(matches!(packet.chat_visibility, ChatVisibility::Full));
        assert!(packet.chat_colors);
        assert_eq!(
            packet.model_customization,
            MODEL_CUSTOMIZATION_WITH_HIGH_BIT_SET
        );
        assert_eq!(packet.main_hand, HumanoidArm::Right);
        assert!(!packet.text_filtering_enabled);
        assert!(packet.allows_listing);
        assert!(matches!(packet.particle_status, ParticleStatus::Minimal));
        assert_eq!(cursor.position() as usize, bytes.len());
    }
}
