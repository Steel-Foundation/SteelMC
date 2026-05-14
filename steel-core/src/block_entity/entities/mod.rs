//! Block entity implementations.

mod barrel;
mod beehive;
mod sign;

pub use barrel::{BARREL_SLOTS, BarrelBlockEntity};
pub use beehive::{
    BEEHIVE_MAX_OCCUPANTS, BEEHIVE_MIN_OCCUPATION_TICKS_NECTARLESS, BeehiveBlockEntity,
};
pub use sign::{SIGN_LINES, SignBlockEntity, SignText};
