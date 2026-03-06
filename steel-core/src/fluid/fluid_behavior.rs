use crate::entity::Entity;
use crate::world::World;
use steel_registry::blocks::properties::Direction;
use steel_registry::fluid::{FluidRef, FluidState};
use steel_registry::items::ItemRef;
use steel_utils::{BlockPos, BlockStateId};

/// Trait for fluid behavior implementations.
/// Conceptual equivalent of Minecraft's `Fluid` class.
pub trait FluidBehavior: Send + Sync {

    /// Gets the fluid type for this behaviour.
    fn fluid_type(&self) -> FluidRef;

    /// Checks if this fluid is the same type as another fluid ref.
    /// This is used to determine if fluids can flow into each other, etc.
    /// By default, it compares the fluid refs by pointer equality, which works for registry fluids.
    fn is_same(&self, other: FluidRef) -> bool {
        std::ptr::eq(self.fluid_type(), other)
    }

    /// Gets the number of ticks between fluid updates.
    fn tick_delay(&self, world: &World) -> i32;
    /// Gets the amount of fluid level drop per horizontal block.
    /// Takes `world` because some fluids (lava) differ by dimension.
    fn drop_off(&self, world: &World) -> u8;
    /// Gets the slope-search distance for horizontal spread.
    /// Takes `world` because some fluids (lava) differ by dimension.
    fn slope_find_distance(&self, world: &World) -> u8;

    /// Called every tick for fluid blocks.
    fn tick(&self, world: &World, pos: BlockPos);
    /// Called to calculate fluid spreading each tick.
    fn spread(&self, world: &World, pos: BlockPos, fluid_state: FluidState);

    /// Checks if this fluid can be replaced by another fluid.
    /// This is used to determine if a fluid can flow into a block occupied by another fluid.
    fn can_be_replaced_with(
        &self,
        fluid_state: FluidState,
        world: &World,
        pos: BlockPos,
        other_fluid: FluidRef,
        direction: Direction,
    ) -> bool;

    /// Gets the sound event ID for when this fluid is picked up with a bucket.
    fn pickup_sound(&self) -> Option<i32> {
        None
    }

    /// Gets the item that is dropped when this fluid is picked up with a bucket.
    fn bucket_item(&self) -> Option<ItemRef> {
        None
    }

    /// Called before a block is destroyed by this fluid.
    fn before_destroying_block(&self, _world: &World, _pos: BlockPos, _replaced: BlockStateId) {
        // default: do nothing
    }

    /// Called at tick time to play ambient animations (sounds, particles).
    #[allow(unused_variables)]
    fn animate_tick(&self, world: &World, pos: BlockPos, fluid_state: FluidState) {}

    /// Checks if this fluid can convert to a source block at the given position.
    fn can_convert_to_source(&self, _world: &World) -> bool {
        false
    }

    /// Called when an entity is inside this fluid.
    fn entity_inside(&self, _world: &mut World, _pos: BlockPos, _entity: &mut dyn Entity) {}

    /// Gets the explosion resistance of this fluid.
    fn explosion_resistance(&self) -> f32 {
        0.0
    }

    /// Called on random tick for this fluid's block.
    /// will be used for fire spread in lava
    #[allow(unused_variables)]
    fn random_tick(&self, world: &World, pos: BlockPos) {}

    /// Returns the tick delay to use when scheduling a newly-spread block,
    /// taking into account the old and new fluid states.
    #[allow(unused_variables)]
    fn get_spread_delay(
        &self,
        world: &World,
        _pos: BlockPos,
        old_state: steel_registry::fluid::FluidState,
        new_state: steel_registry::fluid::FluidState,
    ) -> i32 {
        self.tick_delay(world)
    }

    /// Returns the flow velocity vector at a position (used for entity physics).
    ///
    /// Determines how strongly and in which direction entities/items are pushed.
    ///
    /// Default returns a zero vector (no push).
    /// Returns the x component of the flow velocity vector.
    #[allow(unused_variables)]
    fn get_flow_x(&self, _world: &World, _pos: BlockPos) -> f64 {
        0.0
    }

    /// Returns the z component of the flow velocity vector.
    #[allow(unused_variables)]
    fn get_flow_z(&self, _world: &World, _pos: BlockPos) -> f64 {
        0.0
    }
}
