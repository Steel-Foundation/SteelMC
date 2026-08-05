use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_LOGIN;
use steel_utils::{GlobalPos, Identifier, types::GameType};

#[derive(Clone, Debug, WriteTo)]
pub struct CommonPlayerSpawnInfo {
    #[write(as = VarInt)]
    pub dimension_type: i32,
    pub dimension: Identifier,
    pub seed: i64,
    #[write(as = Byte)]
    pub game_type: GameType,
    #[write(as = OptionByte)]
    pub previous_game_type: Option<GameType>,
    pub is_debug: bool,
    pub is_flat: bool,
    pub last_death_location: Option<GlobalPos>,
    #[write(as = VarInt)]
    pub portal_cooldown: i32,
    #[write(as = VarInt)]
    pub sea_level: i32,
}

#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_LOGIN)]
pub struct CLogin {
    pub player_id: i32,
    pub hardcore: bool,
    #[write(as = Prefixed(VarInt))]
    pub levels: Vec<Identifier>,
    #[write(as = VarInt)]
    pub max_players: i32,
    #[write(as = VarInt)]
    pub chunk_radius: i32,
    #[write(as = VarInt)]
    pub simulation_distance: i32,
    pub reduced_debug_info: bool,
    pub show_death_screen: bool,
    pub do_limited_crafting: bool,
    pub common_player_spawn_info: CommonPlayerSpawnInfo,
    pub online_mode: bool,
    pub enforces_secure_chat: bool,
}
