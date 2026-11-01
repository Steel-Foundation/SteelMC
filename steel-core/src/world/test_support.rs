//! Controlled world operations for native integration tests using real clocks.

use steel_utils::Identifier;

use super::World;

/// Resolves and advances the dimension's default clock to an extracted time marker.
pub fn move_to_time_marker(world: &World, marker: &Identifier) -> Result<(), String> {
    let clock = world
        .dimension_type
        .default_clock
        .ok_or("dimension has no default clock")?;
    match world.move_clock_to_time_marker(clock, marker) {
        Some(true) => Ok(()),
        Some(false) => Err(format!(
            "time marker {marker} is not defined for {}",
            clock.key
        )),
        None => Err("world clock is unavailable".into()),
    }
}
