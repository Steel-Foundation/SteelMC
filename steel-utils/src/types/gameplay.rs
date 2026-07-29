use std::io::{self, Cursor, Write};

use bitflags::bitflags;
use serde::{Deserialize, Serialize};

use crate::{
    codec::VarInt,
    serial::{ReadFrom, WriteTo},
};

/// The game type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[expect(missing_docs, reason = "variant names are self-explanatory")]
pub enum GameType {
    Survival = 0,
    Creative = 1,
    Adventure = 2,
    Spectator = 3,
}

impl GameType {
    /// Returns the name of the game type.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            GameType::Survival => "survival",
            GameType::Creative => "creative",
            GameType::Adventure => "adventure",
            GameType::Spectator => "spectator",
        }
    }
}

impl ReadFrom for GameType {
    fn read(data: &mut Cursor<&[u8]>) -> io::Result<Self> {
        let value = VarInt::read(data)?.0;
        match value {
            0 => Ok(GameType::Survival),
            1 => Ok(GameType::Creative),
            2 => Ok(GameType::Adventure),
            3 => Ok(GameType::Spectator),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid GameType",
            )),
        }
    }
}

impl From<GameType> for i8 {
    fn from(value: GameType) -> Self {
        value as i8
    }
}

impl From<GameType> for i32 {
    fn from(value: GameType) -> Self {
        value as i32
    }
}

impl From<GameType> for f32 {
    fn from(value: GameType) -> Self {
        f32::from(value as i8)
    }
}

impl From<i8> for GameType {
    fn from(value: i8) -> Self {
        match value {
            1 => GameType::Creative,
            2 => GameType::Adventure,
            3 => GameType::Spectator,
            _ => GameType::Survival,
        }
    }
}

impl From<i32> for GameType {
    fn from(value: i32) -> Self {
        match value {
            1 => GameType::Creative,
            2 => GameType::Adventure,
            3 => GameType::Spectator,
            _ => GameType::Survival,
        }
    }
}

impl From<f32> for GameType {
    fn from(value: f32) -> Self {
        match value {
            1. => GameType::Creative,
            2. => GameType::Adventure,
            3. => GameType::Spectator,
            _ => GameType::Survival,
        }
    }
}

/// World difficulty level.
///
/// Controls starvation damage thresholds, mob spawning behavior,
/// and various other gameplay tweaks.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Difficulty {
    /// No hostile mobs, no starvation, health regenerates quickly.
    Peaceful = 0,
    /// Hostile mobs deal less damage, starvation stops at 10 HP.
    Easy = 1,
    /// Default difficulty, starvation stops at 1 HP.
    #[default]
    Normal = 2,
    /// Hostile mobs deal more damage, starvation can kill.
    Hard = 3,
}

#[expect(clippy::match_same_arms, reason = "cause it looks better")]
impl From<u8> for Difficulty {
    fn from(value: u8) -> Self {
        match value {
            0 => Difficulty::Peaceful,
            1 => Difficulty::Easy,
            2 => Difficulty::Normal,
            3 => Difficulty::Hard,
            _ => Difficulty::Normal,
        }
    }
}

impl From<Difficulty> for u8 {
    fn from(value: Difficulty) -> Self {
        value as u8
    }
}

impl ReadFrom for Difficulty {
    fn read(data: &mut Cursor<&[u8]>) -> io::Result<Self> {
        let value = <u8 as ReadFrom>::read(data)?;
        match value {
            0 => Ok(Difficulty::Peaceful),
            1 => Ok(Difficulty::Easy),
            2 => Ok(Difficulty::Normal),
            3 => Ok(Difficulty::Hard),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Invalid Difficulty: {value}"),
            )),
        }
    }
}

impl WriteTo for Difficulty {
    fn write(&self, writer: &mut impl Write) -> io::Result<()> {
        (*self as u8).write(writer)
    }
}

impl Serialize for Difficulty {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_u8(*self as u8)
    }
}

impl<'de> Deserialize<'de> for Difficulty {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let id = u8::deserialize(deserializer)?;
        Ok(Self::from(id))
    }
}

/// Represents the hand used for an interaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InteractionHand {
    /// The main hand.
    MainHand,
    /// The off hand.
    OffHand,
}

impl ReadFrom for InteractionHand {
    fn read(data: &mut Cursor<&[u8]>) -> io::Result<Self> {
        let id = VarInt::read(data)?.0;
        match id {
            0 => Ok(InteractionHand::MainHand),
            1 => Ok(InteractionHand::OffHand),
            _ => Err(io::Error::other("Invalid InteractionHand id")),
        }
    }
}

/// Flags that control how a block update is processed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UpdateFlags(u16);

bitflags! {
    impl UpdateFlags: u16 {
        const UPDATE_NEIGHBORS = 1;
        const UPDATE_CLIENTS = 1 << 1;
        const UPDATE_INVISIBLE = 1 << 2;
        const UPDATE_IMMEDIATE = 1 << 3;
        const UPDATE_KNOWN_SHAPE = 1 << 4;
        const UPDATE_SUPPRESS_DROPS = 1 << 5;
        const UPDATE_MOVE_BY_PISTON = 1 << 6;
        const UPDATE_SKIP_SHAPE_UPDATE_ON_WIRE = 1 << 7;
        const UPDATE_SKIP_BLOCK_ENTITY_SIDEEFFECTS = 1 << 8;
        const UPDATE_SKIP_ON_PLACE = 1 << 9;

        const UPDATE_NONE = Self::UPDATE_INVISIBLE.bits() | Self::UPDATE_SKIP_BLOCK_ENTITY_SIDEEFFECTS.bits();
        const UPDATE_ALL = Self::UPDATE_NEIGHBORS.bits() | Self::UPDATE_CLIENTS.bits();
        const UPDATE_ALL_IMMEDIATE = Self::UPDATE_ALL.bits() | Self::UPDATE_IMMEDIATE.bits();
    }
}
