use std::io;

use serde_json::Value;
use steel_utils::{ChunkPos, Identifier};
use thiserror::Error;
use tokio::task::JoinError;
use uuid::Uuid;

use crate::chunk::chunk_map::TestHarnessChunkError;
use crate::chunk::status::ChunkStatus;

/// Errors returned by the in-memory Steel test harness.
#[derive(Debug, Error)]
pub enum TestHarnessError {
    /// The single-thread Tokio runtime could not be created.
    #[error("failed to create the test harness runtime: {0}")]
    Runtime(#[source] io::Error),
    /// The single-thread chunk-generation pool could not be created.
    #[error("failed to create the test harness generation pool: {0}")]
    GenerationPool(#[source] rayon::ThreadPoolBuildError),
    /// Steel could not initialize the RAM-only world.
    #[error("failed to initialize the in-memory world: {0}")]
    World(#[source] io::Error),
    /// The runtime task responsible for world initialization stopped unexpectedly.
    #[error("the in-memory world initialization task stopped: {0}")]
    WorldTask(#[source] JoinError),
    /// The configured dimension height cannot be represented by whole chunk sections.
    #[error("invalid test world height {height}; it must be positive and divisible by 16")]
    InvalidWorldHeight {
        /// Configured world height.
        height: i32,
    },
    /// A chunk already existed but had not reached the required state.
    #[error("chunk {pos:?} exists at {status:?}, but the harness requires Full")]
    ChunkNotFull {
        /// Requested chunk position.
        pos: ChunkPos,
        /// Currently published status, if any.
        status: Option<ChunkStatus>,
    },
    /// A concurrent or external insertion claimed the same chunk position.
    #[error("chunk {pos:?} was inserted concurrently")]
    ChunkInsertConflict {
        /// Requested chunk position.
        pos: ChunkPos,
    },
    /// The requested Full halo extends beyond representable chunk coordinates.
    #[error("the radius-{radius} Full halo around {center:?} exceeds chunk coordinates")]
    HaloCoordinateOverflow {
        /// Requested center chunk.
        center: ChunkPos,
        /// Required halo radius.
        radius: u8,
    },
    /// A retained chunk could not enter the active lifecycle synchronously.
    #[error("chunk {pos:?} could not enter the active test-harness lifecycle")]
    ChunkLifecycleUnavailable {
        /// Requested chunk position.
        pos: ChunkPos,
    },
    /// The harness failed to publish the requested chunk ticking readiness.
    #[error("chunk {pos:?} did not become {required} ticking")]
    ChunkNotTicking {
        /// Requested chunk position.
        pos: ChunkPos,
        /// Required ticking level.
        required: &'static str,
    },
    /// The Overworld clock or day timeline is unavailable.
    #[error("the vanilla Overworld day clock is unavailable")]
    MissingOverworldClock,
    /// Daytime must be one tick within a vanilla day.
    #[error("invalid daytime {value}; expected 0..={max}")]
    InvalidDaytime {
        /// Rejected daytime value.
        value: u32,
        /// Largest accepted daytime value.
        max: u32,
    },
    /// The world's stored clock value could not be represented as daytime.
    #[error("invalid stored Overworld clock value {value}")]
    InvalidStoredDaytime {
        /// Stored total tick value.
        value: i64,
    },
    /// The tick counter exhausted its representable range.
    #[error("test harness tick counter overflowed")]
    TickOverflow,
    /// A game-rule key was not registered by Steel.
    #[error("unknown game rule {key}")]
    UnknownGameRule {
        /// Unregistered game-rule key.
        key: Identifier,
    },
    /// A serialized game-rule value did not match its registered type or limits.
    #[error("invalid value {value} for game rule {key}")]
    InvalidGameRuleValue {
        /// Registered game-rule key.
        key: Identifier,
        /// Rejected serialized value.
        value: Value,
    },
    /// A player name did not satisfy Minecraft's profile-name rules.
    #[error("invalid player name {name:?}")]
    InvalidPlayerName {
        /// Rejected profile name.
        name: String,
    },
    /// The UUID is already attached to this world.
    #[error("player UUID {uuid} is already attached to the test world")]
    DuplicatePlayerUuid {
        /// Already registered player UUID.
        uuid: Uuid,
    },
    /// The entity ID is already owned by an entity in this world.
    #[error("entity ID {entity_id} is already registered in the test world")]
    DuplicateEntityId {
        /// Already registered runtime entity ID.
        entity_id: i32,
    },
    /// Production world lifecycle rejected the player registration.
    #[error("Steel rejected player {name:?} during world attachment")]
    PlayerRegistrationRejected {
        /// Rejected player's profile name.
        name: String,
    },
}

impl From<TestHarnessChunkError> for TestHarnessError {
    fn from(error: TestHarnessChunkError) -> Self {
        match error {
            TestHarnessChunkError::InvalidWorldHeight { height } => {
                Self::InvalidWorldHeight { height }
            }
            TestHarnessChunkError::HaloCoordinateOverflow { center, radius } => {
                Self::HaloCoordinateOverflow { center, radius }
            }
            TestHarnessChunkError::ChunkNotFull { pos, status } => {
                Self::ChunkNotFull { pos, status }
            }
            TestHarnessChunkError::ChunkInsertConflict { pos } => Self::ChunkInsertConflict { pos },
            TestHarnessChunkError::ChunkLifecycleUnavailable { pos } => {
                Self::ChunkLifecycleUnavailable { pos }
            }
        }
    }
}
