//! Block configuration and static properties.
//!
//! This module contains constant/static data about blocks. Dynamic behavior
//! has been moved to `steel-core::behavior`.

pub use crate::blocks::properties::NoteBlockInstrument;
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

/// Static configuration for a block type.
///
/// This contains constant properties that don't change based on game state.
/// Dynamic behavior is handled by `BlockBehaviour` in steel-core.
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
    pub const fn is_randomly_ticking(mut self, ticking: bool) -> Self {
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
    pub const fn is_air(mut self, is_air: bool) -> Self {
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
}

impl Default for BlockConfig {
    fn default() -> Self {
        Self::new()
    }
}
