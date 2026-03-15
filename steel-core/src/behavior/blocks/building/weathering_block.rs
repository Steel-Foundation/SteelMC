use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::{
    REGISTRY,
    blocks::{BlockRef, block_state_ext::BlockStateExt},
};
use steel_utils::{BlockPos, BlockStateId, types::UpdateFlags};

use crate::{
    behavior::{
        BlockBehaviour, BlockPlaceContext,
        weathering::{get_weather_state, next_copper_stage},
    },
    world::World,
};

/// Oxidation stages for copper blocks, matching vanilla's `WeatheringCopper.WeatherState`.
///
/// Ordinal values are used for age comparisons during the neighbor scan algorithm.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum WeatherState {
    /// Fresh copper, no oxidation.
    Unaffected = 0,
    /// First stage of oxidation.
    Exposed = 1,
    /// Second stage of oxidation.
    Weathered = 2,
    /// Fully oxidized, will not advance further.
    Oxidized = 3,
}

/// Scan radius for neighbor copper blocks (Manhattan distance).
const SCAN_DISTANCE: i32 = 4;

/// Base probability per random tick that a copper block even attempts to oxidize.
/// Vanilla: `0.05688889F` — roughly once per in-game day per block.
const BASE_CHANCE: f32 = 0.056_888_89;

/// Composable helper for copper weathering/oxidation logic.
///
/// Add this as a field to block implementations that should support weathering.
///
/// In `YourBlock::is_randomly_ticking` first check if self.block is a copper variant and then
/// call [`WeatheringCopper::is_randomly_ticking`]
///
/// In `YourBlock::random_tick` call [`WeatheringCopper::change_over_time`]
// TODO: Add weathering support for slabs, stairs, doors, trapdoors, grates, bars, bulbs, lanterns, chains, chests, and golem statues
pub struct WeatheringCopper {
    weather_state: WeatherState,
}

impl WeatheringCopper {
    /// Creates a new `WeatheringCopper` helper with the given oxidation stage.
    #[must_use]
    pub const fn new(weather_state: WeatherState) -> Self {
        Self { weather_state }
    }

    /// Whether this block should receive random ticks. (false if fully Oxidized)
    #[must_use]
    pub fn is_randomly_ticking(&self) -> bool {
        self.weather_state != WeatherState::Oxidized
    }

    /// Advances the weathering state and replaces the block, with a 5.7% chance.
    ///
    /// Vanilla: [`ChangeOverTimeBlock.changeOverTime`]
    pub fn change_over_time(&self, state: BlockStateId, world: &World, pos: BlockPos) {
        if rand::random::<f32>() >= BASE_CHANCE {
            return;
        }

        if let Some(next_state) = self.get_next_state(state, world, pos) {
            world.set_block(pos, next_state, UpdateFlags::UPDATE_ALL);
        }
    }

    /// Checks the neighbors and calculates the next [`BlockStateId`] for the weathering copper.
    ///
    /// 1. Scan Manhattan distance 4 for copper neighbors
    /// 2. If any younger neighbor exists, abort
    /// 3. Probability = ((older+1)/(older+same+1))² × `chance_modifier`
    /// 4. On success, advance to next oxidation stage preserving block properties
    ///
    /// Vanilla: `ChangeOverTimeBlock.getNextState`
    fn get_next_state(
        &self,
        state: BlockStateId,
        world: &World,
        pos: BlockPos,
    ) -> Option<BlockStateId> {
        let own_age = self.weather_state as i32;
        let mut same_age_count = 0i32;
        let mut older_count = 0i32;

        for dx in -SCAN_DISTANCE..=SCAN_DISTANCE {
            for dy in -SCAN_DISTANCE..=SCAN_DISTANCE {
                for dz in -SCAN_DISTANCE..=SCAN_DISTANCE {
                    if dx.abs() + dy.abs() + dz.abs() > SCAN_DISTANCE {
                        continue;
                    }
                    if dx == 0 && dy == 0 && dz == 0 {
                        continue;
                    }

                    let neighbor_pos = BlockPos::new(pos.x() + dx, pos.y() + dy, pos.z() + dz);
                    let neighbor_state = world.get_block_state(&neighbor_pos);
                    let neighbor_block = neighbor_state.get_block();

                    let Some(neighbor_age) = get_weather_state(neighbor_block) else {
                        continue;
                    };
                    let found_age = neighbor_age as i32;

                    if found_age < own_age {
                        return None;
                    }

                    if found_age > own_age {
                        older_count += 1;
                    } else {
                        same_age_count += 1;
                    }
                }
            }
        }

        let chance = (older_count + 1) as f32 / (older_count + same_age_count + 1) as f32;
        let actual_chance = chance * chance * self.get_chance_modifier();

        if rand::random::<f32>() >= actual_chance {
            return None;
        }

        let old_block = state.get_block();
        let new_block = next_copper_stage(old_block)?;
        Some(REGISTRY.blocks.copy_matching_properties(state, new_block))
    }

    fn get_chance_modifier(&self) -> f32 {
        if self.weather_state == WeatherState::Unaffected {
            0.75
        } else {
            1.0
        }
    }
}

/// Block behavior for `WeatheringCopperFullBlock`
///
/// See [`WeatherState`]
#[block_behavior]
pub struct WeatheringCopperFullBlock {
    block: BlockRef,
    #[json_arg(r#enum = "WeatherState", json = "weather_state")]
    weathering: WeatheringCopper,
}

impl WeatheringCopperFullBlock {
    /// Creates a new `WeatheringCopperFullBlock` behavior.
    #[must_use]
    pub const fn new(block: BlockRef, weather_state: WeatherState) -> Self {
        Self {
            block,
            weathering: WeatheringCopper::new(weather_state),
        }
    }
}

impl BlockBehaviour for WeatheringCopperFullBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }

    fn is_randomly_ticking(&self, _state: BlockStateId) -> bool {
        self.weathering.is_randomly_ticking()
    }

    fn random_tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        self.weathering.change_over_time(state, world, pos);
    }
}
