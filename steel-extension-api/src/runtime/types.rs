//! Commands and runtime operations on temporary, domain-isolated worlds and players.
use crate::BlockPos;
use stabby::{option::Option as StableOption, string::String as ApiString};
#[cfg(feature = "host")]
use steel_utils::types::{Difficulty as CoreDifficulty, GameType as CoreGameMode};
/// Host-resolved player identity. A lookup always checks current session ownership.
#[stabby::stabby]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PlayerId {
    pub(crate) bytes: [u8; 16],
    pub(crate) entity_id: i32,
}
/// World coordinates in blocks.
#[stabby::stabby]
#[derive(Debug, Clone, Copy, Default)]
pub struct Position {
    /// X coordinate.
    pub x: f64,
    /// Y coordinate.
    pub y: f64,
    /// Z coordinate.
    pub z: f64,
}
/// Snapshot of a resident player, including server-controlled actors.
#[stabby::stabby]
pub struct PlayerInfo {
    /// Runtime player identity.
    pub id: PlayerId,
    /// Display name.
    pub name: ApiString,
    /// Current world key.
    pub world: ApiString,
    /// Position at snapshot time.
    pub position: Position,
    /// Whether the host supplies actions.
    pub bot: bool,
    /// Current health.
    pub health: f32,
}
/// World operation identity; completed results are consumed by `operation_status`.
#[stabby::stabby]
#[derive(Debug, Clone, Copy)]
pub struct Operation {
    pub(crate) id: u64,
}
/// State of a world operation.
#[stabby::stabby]
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationState {
    /// Preparation or cleanup is still running.
    Pending,
    /// Completed successfully.
    Ready,
    /// Failed with a diagnostic.
    Failed,
}
/// A completed status consumes the operation result.
#[stabby::stabby]
pub struct OperationStatus {
    /// World/domain key on success, diagnostic on failure; empty while pending.
    pub value: ApiString,
    /// Current operation state.
    pub state: OperationState,
}
/// Player game mode.
#[stabby::stabby]
#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum GameMode {
    /// Normal survival rules.
    Survival,
    /// Creative controls and inventory rules.
    Creative,
    /// Adventure permissions.
    Adventure,
    /// Flying observer without collisions.
    Spectator,
}
/// Initial world difficulty.
#[stabby::stabby]
#[repr(u8)]
#[derive(Debug, Clone, Copy)]
pub enum Difficulty {
    /// Peaceful rules.
    Peaceful,
    /// Easy rules.
    Easy,
    /// Normal rules.
    Normal,
    /// Hard rules.
    Hard,
}
/// Explicit gameplay and spawn settings for a temporary world.
#[stabby::stabby]
#[derive(Debug, Clone, Copy)]
pub struct WorldSettings {
    /// Seed passed to the registered generator.
    pub seed: i64,
    /// Spawn chosen by the map author.
    pub spawn: BlockPos,
    /// Game mode for players visiting this world for the first time.
    pub default_game_mode: GameMode,
    /// Initial world difficulty.
    pub difficulty: Difficulty,
}
#[cfg(feature = "host")]
impl GameMode {
    pub(crate) const fn into_host(self) -> CoreGameMode {
        match self {
            Self::Survival => CoreGameMode::Survival,
            Self::Creative => CoreGameMode::Creative,
            Self::Adventure => CoreGameMode::Adventure,
            Self::Spectator => CoreGameMode::Spectator,
        }
    }
}
#[cfg(feature = "host")]
impl Difficulty {
    pub(crate) const fn into_host(self) -> CoreDifficulty {
        match self {
            Self::Peaceful => CoreDifficulty::Peaceful,
            Self::Easy => CoreDifficulty::Easy,
            Self::Normal => CoreDifficulty::Normal,
            Self::Hard => CoreDifficulty::Hard,
        }
    }
}
/// Controller intent for one bot tick.
#[stabby::stabby]
#[derive(Debug, Clone, Copy, Default)]
pub struct PlayerControls {
    /// Move forward.
    pub forward: bool,
    /// Move backward.
    pub backward: bool,
    /// Strafe left.
    pub left: bool,
    /// Strafe right.
    pub right: bool,
    /// Jump.
    pub jump: bool,
    /// Sneak.
    pub sneak: bool,
    /// Sprint.
    pub sprint: bool,
}
/// Bot controls expire after one actor tick unless supplied again.
#[stabby::stabby]
pub struct BotInput {
    /// Horizontal rotation in degrees.
    pub yaw: f32,
    /// Vertical rotation in degrees.
    pub pitch: f32,
    /// Movement intent.
    pub movement: PlayerControls,
    /// Block to mine with normal tool speed and reach.
    pub dig: StableOption<BlockPos>,
}
