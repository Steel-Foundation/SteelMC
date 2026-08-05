use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_GAME_EVENT;

#[derive(WriteTo, Copy, Clone, Debug)]
#[write(as = VarInt)]
pub enum GameEventType {
    NoRespawnBlockAvailable = 0,
    StartRaining = 1,
    StopRaining = 2,
    ChangeGameMode = 3,
    WinGame = 4,
    DemoEvent = 5,
    PlayArrowHitSound = 6,
    RainLevelChange = 7,
    ThunderLevelChange = 8,
    PufferFishSting = 9,
    GuardianElderEffect = 10,
    ImmediateRespawn = 11,
    LimitedCrafting = 12,
    LevelChunksLoadStart = 13,
}

#[derive(WriteTo, ClientPacket, Clone, Debug)]
#[packet_id(Play = C_GAME_EVENT)]
pub struct CGameEvent {
    pub event: GameEventType,
    pub data: f32,
}
