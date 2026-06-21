//! Block configuration and static properties.
//!
//! This module contains constant/static data about blocks. Dynamic behavior
//! has been moved to `steel-core::behavior`.

pub use crate::blocks::properties::NoteBlockInstrument;
use glam::DVec3;
use steel_utils::{BlockPos, random::get_seed};

use crate::sound_types::SoundType;

/// How a block reacts when pushed by a piston.
#[derive(Debug, Clone, Copy)]
pub enum PushReaction {
    Normal,
    Destroy,
    Block,
    Ignore,
    PushOnly,
}

/// Vanilla `BlockBehavior.OffsetType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OffsetType {
    None,
    Xz,
    Xyz,
}

/// Static configuration for a block type.
///
/// This contains constant properties that don't change based on game state.
/// Dynamic behavior is handled by `BlockBehavior` in steel-core.
#[derive(Debug)]
pub struct BlockConfig {
    pub has_collision: bool,
    pub can_occlude: bool,
    pub explosion_resistance: f32,
    pub is_randomly_ticking: bool,
    pub force_solid_off: bool,
    pub force_solid_on: bool,
    pub push_reaction: PushReaction,
    pub friction: f32,
    pub speed_factor: f32,
    pub jump_factor: f32,
    pub dynamic_shape: bool,
    pub offset_type: OffsetType,
    pub max_horizontal_offset: f32,
    pub max_vertical_offset: f32,
    pub destroy_time: f32,
    pub ignited_by_lava: bool,
    pub liquid: bool,
    pub is_air: bool,
    pub requires_correct_tool_for_drops: bool,
    pub instrument: NoteBlockInstrument,
    pub replaceable: bool,
    pub sound_type: SoundType,
}

impl BlockConfig {
    /// Starts building a new set of block properties.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            has_collision: true,
            can_occlude: true,
            explosion_resistance: 0.0,
            is_randomly_ticking: false,
            force_solid_off: false,
            force_solid_on: false,
            push_reaction: PushReaction::Normal,
            friction: 0.6,
            speed_factor: 1.0,
            jump_factor: 1.0,
            dynamic_shape: false,
            offset_type: OffsetType::None,
            max_horizontal_offset: 0.25,
            max_vertical_offset: 0.2,
            destroy_time: 0.0,
            ignited_by_lava: false,
            liquid: false,
            is_air: false,
            requires_correct_tool_for_drops: false,
            instrument: NoteBlockInstrument::Harp,
            replaceable: false,
            sound_type: crate::sound_types::STONE,
        }
    }

    #[must_use]
    pub const fn has_collision(mut self, has_collision: bool) -> Self {
        self.has_collision = has_collision;
        self
    }

    #[must_use]
    pub const fn can_occlude(mut self, can_occlude: bool) -> Self {
        self.can_occlude = can_occlude;
        self
    }

    #[must_use]
    pub const fn explosion_resistance(mut self, resistance: f32) -> Self {
        self.explosion_resistance = resistance;
        self
    }

    #[must_use]
    pub const fn set_is_randomly_ticking(mut self, ticking: bool) -> Self {
        self.is_randomly_ticking = ticking;
        self
    }

    #[must_use]
    pub const fn force_solid_off(mut self, force: bool) -> Self {
        self.force_solid_off = force;
        self
    }

    #[must_use]
    pub const fn force_solid_on(mut self, force: bool) -> Self {
        self.force_solid_on = force;
        self
    }

    #[must_use]
    pub const fn push_reaction(mut self, reaction: PushReaction) -> Self {
        self.push_reaction = reaction;
        self
    }

    #[must_use]
    pub const fn friction(mut self, friction: f32) -> Self {
        self.friction = friction;
        self
    }

    #[must_use]
    pub const fn speed_factor(mut self, factor: f32) -> Self {
        self.speed_factor = factor;
        self
    }

    #[must_use]
    pub const fn jump_factor(mut self, factor: f32) -> Self {
        self.jump_factor = factor;
        self
    }

    #[must_use]
    pub const fn dynamic_shape(mut self, dynamic: bool) -> Self {
        self.dynamic_shape = dynamic;
        self
    }

    #[must_use]
    pub const fn offset_type(mut self, offset_type: OffsetType) -> Self {
        self.offset_type = offset_type;
        self
    }

    #[must_use]
    pub const fn max_horizontal_offset(mut self, offset: f32) -> Self {
        self.max_horizontal_offset = offset;
        self
    }

    #[must_use]
    pub const fn max_vertical_offset(mut self, offset: f32) -> Self {
        self.max_vertical_offset = offset;
        self
    }

    #[must_use]
    pub const fn destroy_time(mut self, time: f32) -> Self {
        self.destroy_time = time;
        self
    }

    #[must_use]
    pub const fn ignited_by_lava(mut self, ignited: bool) -> Self {
        self.ignited_by_lava = ignited;
        self
    }

    #[must_use]
    pub const fn liquid(mut self, liquid: bool) -> Self {
        self.liquid = liquid;
        self
    }

    #[must_use]
    pub const fn set_is_air(mut self, is_air: bool) -> Self {
        self.is_air = is_air;
        self
    }

    #[must_use]
    pub const fn requires_correct_tool_for_drops(mut self, requires: bool) -> Self {
        self.requires_correct_tool_for_drops = requires;
        self
    }

    #[must_use]
    pub const fn instrument(mut self, instrument: NoteBlockInstrument) -> Self {
        self.instrument = instrument;
        self
    }

    #[must_use]
    pub const fn replaceable(mut self, replaceable: bool) -> Self {
        self.replaceable = replaceable;
        self
    }

    #[must_use]
    pub const fn sound_type(mut self, sound_type: SoundType) -> Self {
        self.sound_type = sound_type;
        self
    }

    /// Returns the vanilla positional offset for this block config.
    #[must_use]
    pub fn offset_at(&self, pos: BlockPos) -> DVec3 {
        let seed = get_seed(pos.x(), 0, pos.z());
        let x = horizontal_offset_component(seed & 15, self.max_horizontal_offset);
        let z = horizontal_offset_component((seed >> 8) & 15, self.max_horizontal_offset);

        match self.offset_type {
            OffsetType::None => DVec3::ZERO,
            OffsetType::Xz => DVec3::new(x, 0.0, z),
            OffsetType::Xyz => {
                let y = (f64::from(((seed >> 4) & 15) as f32 / 15.0) - 1.0)
                    * f64::from(self.max_vertical_offset);
                DVec3::new(x, y, z)
            }
        }
    }
}

impl Default for BlockConfig {
    fn default() -> Self {
        Self::new()
    }
}

fn horizontal_offset_component(seed_bits: i64, max_horizontal_offset: f32) -> f64 {
    let raw_offset = (f64::from(seed_bits as f32 / 15.0) - 0.5) * 0.5;
    raw_offset.clamp(
        -f64::from(max_horizontal_offset),
        f64::from(max_horizontal_offset),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_offset(config: &BlockConfig, pos: BlockPos, expected: DVec3) {
        let offset = config.offset_at(pos);
        assert!((offset - expected).length() < 1.0e-7);
    }

    #[test]
    fn xz_offset_matches_vanilla_clamping() {
        let config = BlockConfig::new()
            .offset_type(OffsetType::Xz)
            .max_horizontal_offset(0.125);

        assert_offset(&config, BlockPos::ZERO, DVec3::new(-0.125, 0.0, -0.125));
        assert_offset(
            &config,
            BlockPos::new(12, 64, 34),
            DVec3::new(0.125, 0.0, 0.125),
        );
    }

    #[test]
    fn xz_offset_uses_block_position_seed_without_y() {
        let config = BlockConfig::new()
            .offset_type(OffsetType::Xz)
            .max_horizontal_offset(0.125);

        assert_offset(
            &config,
            BlockPos::new(1, 64, 0),
            DVec3::new(0.116_666_666_666_666_64, 0.0, -0.016_666_666_666_666_663),
        );
        assert_offset(
            &config,
            BlockPos::new(-5, 12, 7),
            DVec3::new(-0.049_999_999_999_999_99, 0.0, 0.116_666_666_666_666_64),
        );
    }

    #[test]
    fn xyz_offset_applies_vanilla_vertical_component() {
        let config = BlockConfig::new()
            .offset_type(OffsetType::Xyz)
            .max_horizontal_offset(0.25)
            .max_vertical_offset(0.1);

        assert_offset(&config, BlockPos::ZERO, DVec3::new(-0.25, -0.1, -0.25));
        assert_offset(
            &config,
            BlockPos::new(3, 40, 9),
            DVec3::new(
                -0.049_999_999_999_999_99,
                -0.066_666_666_666_666_68,
                -0.049_999_999_999_999_99,
            ),
        );
    }
}
