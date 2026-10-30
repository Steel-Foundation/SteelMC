mod bell_block;
mod button_block;
mod copper_bulb_block;
mod daylight_detector_block;
mod diode;
mod java_hash;
mod lever_block;
mod lightning_rod_block;
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
mod tnt_block;
mod tripwire;
mod weathering_lightning_rod_block;
mod wire;

/// Maximum vanilla redstone signal strength.
pub(crate) const MAX_REDSTONE_SIGNAL: i32 = 15;

/// Minimum vanilla redstone signal strength.
pub(crate) const MIN_REDSTONE_SIGNAL: i32 = 0;

pub use bell_block::*;
pub use button_block::*;
pub use copper_bulb_block::*;
pub use daylight_detector_block::*;
pub use diode::*;
pub use lever_block::*;
pub use lightning_rod_block::*;
pub use note_block::*;
pub use observer_block::*;
pub use piston::*;
pub use powered_block::*;
pub use pressure_plate::*;
pub use rail::*;
pub use redstone_lamp_block::*;
pub use redstone_ore_block::*;
pub use redstone_torch_block::*;
pub use target_block::*;
pub use tnt_block::*;
pub use tripwire::*;
pub use weathering_lightning_rod_block::*;
pub use wire::*;
