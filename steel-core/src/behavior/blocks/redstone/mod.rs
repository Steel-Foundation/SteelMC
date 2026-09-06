mod bell_block;
mod button_block;
mod copper_bulb_block;
mod daylight_detector_block;
mod diode;
mod face_attached_horizontal_directional_block;
mod java_hash;
mod lever_block;
mod note_block;
mod observer_block;
mod piston;
mod powered_block;
mod pressure_plate;
mod rail;
mod redstone_lamp_block;
mod redstone_ore_block;
mod redstone_torch_block;
mod target_block;
mod tripwire;
mod wire;

/// Maximum vanilla redstone signal strength.
pub(crate) const MAX_REDSTONE_SIGNAL: i32 = 15;

/// Minimum vanilla redstone signal strength.
pub(crate) const MIN_REDSTONE_SIGNAL: i32 = 0;

pub use bell_block::BellBlock;
pub use button_block::ButtonBlock;
pub use copper_bulb_block::{CopperBulbBlock, WeatheringCopperBulbBlock};
pub use daylight_detector_block::DaylightDetectorBlock;
pub use diode::{ComparatorBlock, RepeaterBlock};
pub use lever_block::LeverBlock;
pub use note_block::NoteBlock;
pub use observer_block::ObserverBlock;
pub use piston::{MovingPistonBlock, PistonBaseBlock, PistonHeadBlock};
pub use powered_block::PoweredBlock;
pub use pressure_plate::{
    PressurePlateBlock, PressurePlateSensitivity, WeightedPressurePlateBlock,
};
pub use rail::{DetectorRailBlock, PoweredRailBlock, RailBlock};
pub use redstone_lamp_block::RedstoneLampBlock;
pub use redstone_ore_block::RedStoneOreBlock;
pub use redstone_torch_block::{RedstoneTorchBlock, RedstoneWallTorchBlock};
pub use target_block::TargetBlock;
pub use tripwire::{TripWireBlock, TripWireHookBlock};
pub use wire::RedStoneWireBlock;
