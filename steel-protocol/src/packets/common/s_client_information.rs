use steel_macros::{ReadFrom, ServerPacket};

#[derive(ReadFrom, Clone, Debug)]
pub enum ChatVisibility {
    Full = 0,
    System = 1,
    Hidden = 2,
}

#[derive(ReadFrom, Clone, Debug)]
pub enum HumanoidArm {
    Left = 0,
    Right = 1,
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
    #[read(as = VarInt)]
    pub view_distance: i32,
    pub chat_visibility: ChatVisibility,
    pub chat_colors: bool,
    #[read(as = VarInt)]
    pub model_customization: i32,
    pub main_hand: HumanoidArm,
    pub text_filtering_enabled: bool,
    pub allows_listing: bool,
    pub particle_status: ParticleStatus,
}
