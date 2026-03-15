//! Fluid behavior trait and related types.
//! Fluids like `WaterFluid` and `LavaFluid` implement this trait to inherit behavior.
use std::sync::Arc;

use crate::entity::Entity;
use crate::world::World;
use steel_registry::blocks::properties::Direction;
use steel_registry::fluid::{FluidRef, FluidState};
use steel_utils::{BlockPos, BlockStateId};

/// Trait for fluid behavior implementations.
/// Conceptual equivalent of Minecraft's `Fluid` class.
pub trait FluidBehavior: Send + Sync {
    /// Gets the fluid type for this behavior.
    fn fluid_type(&self) -> FluidRef;

    /// Checks if this fluid is the same type as another fluid ref.
    ///
    /// Used to determine if fluids can flow into each other.
    ///
    /// **Override required** for any fluid that has both a source and a flowing variant
    fn is_same(&self, other: FluidRef) -> bool {
        self.fluid_type() == other
    }

    /// Gets the number of ticks between fluid updates.
    fn tick_delay(&self, world: &Arc<World>) -> i32;
    /// Gets the amount of fluid level drop per horizontal block.
    /// Takes `world` because some fluids (lava) differ by dimension.
    fn drop_off(&self, world: &Arc<World>) -> u8;
    /// Gets the slope-search distance for horizontal spread.
    /// Takes `world` because some fluids (lava) differ by dimension.
    fn slope_find_distance(&self, world: &Arc<World>) -> u8;

    /// Called every tick for fluid blocks.
    fn tick(&self, world: &Arc<World>, pos: BlockPos);
    /// Called to calculate fluid spreading each tick.
    fn spread(&self, world: &Arc<World>, pos: BlockPos, fluid_state: FluidState);

    /// Checks if this fluid can be replaced by another fluid.
    /// This is used to determine if a fluid can flow into a block occupied by another fluid.
    fn can_be_replaced_with(
        &self,
        fluid_state: FluidState,
        world: &Arc<World>,
        pos: BlockPos,
        other_fluid: FluidRef,
        direction: Direction,
    ) -> bool;

    /// Called before a block is destroyed by this fluid.
    fn before_destroying_block(
        &self,
        _world: &Arc<World>,
        _pos: BlockPos,
        _replaced: BlockStateId,
    ) {
        // default: do nothing
    }

    /// Called at tick time to play ambient animations (sounds, particles).
    #[allow(unused_variables)]
    fn animate_tick(&self, world: &Arc<World>, pos: BlockPos, fluid_state: FluidState) {}

    /// Checks if this fluid can convert to a source block at the given position.
    fn can_convert_to_source(&self, _world: &Arc<World>) -> bool {
        false
    }

    /// Called when an entity is inside this fluid.
    fn entity_inside(&self, _world: &mut World, _pos: BlockPos, _entity: &mut dyn Entity) {}

    /// Gets the explosion resistance of this fluid.
    fn explosion_resistance(&self) -> f32 {
        0.0
    }

    /// Returns whether this fluid should receive random ticks.
    /// Vanilla: only lava returns true (for fire spread).
    fn is_randomly_ticking(&self) -> bool {
        false
    }

    /// Called on random tick for this fluid's block.
    /// Used for lava fire spread.
    #[allow(unused_variables)]
    fn random_tick(&self, world: &Arc<World>, pos: BlockPos) {}

    /// Returns the tick delay to use when scheduling a newly-spread block,
    /// taking into account the old and new fluid states.
    #[allow(unused_variables)]
    fn get_spread_delay(
        &self,
        world: &Arc<World>,
        _pos: BlockPos,
        old_state: FluidState,
        new_state: FluidState,
    ) -> i32 {
        self.tick_delay(world)
    }

    /// Returns the x component of the flow velocity at a position (used for entity physics).
    ///
    /// Determines how strongly entities/items are pushed horizontally.
    // TODO: implement flow velocity for entity interactions (pushing, drowning).
    #[allow(unused_variables)]
    fn get_flow_x(&self, _world: &Arc<World>, _pos: BlockPos) -> f64 {
        0.0
    }

    /// Returns the z component of the flow velocity at a position (used for entity physics).
    // TODO: implement flow velocity for entity interactions (pushing, drowning).
    #[allow(unused_variables)]
    fn get_flow_z(&self, _world: &Arc<World>, _pos: BlockPos) -> f64 {
        0.0
    }
}
