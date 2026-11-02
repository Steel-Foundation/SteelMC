//! Fluids registered through `client_content`, run by the vanilla spread algorithm.
//!
//! Vanilla gives each flowing fluid its own `FlowingFluid` subclass. A registered
//! fluid instead supplies its spread parameters as data and optional gameplay
//! hooks, so spreading stays in the host and only rare events cross into an
//! extension.

use std::sync::Arc;

use glam::DVec3;
use steel_registry::blocks::properties::Direction;
use steel_registry::client_content::ClientFluid;
use steel_registry::vanilla_fluids;
use steel_utils::{BlockPos, BlockStateId};

use crate::entity::{Entity, InsideBlockEffectCollector, InsideBlockEffectType};
use crate::fluid::{
    FlowingFluid, FluidBehavior, FluidRef, FluidState, get_flow as flowing_fluid_flow,
};
use crate::world::World;

/// Gameplay hooks an owner attaches to a registered fluid.
///
/// Every hook runs on the world tick thread that owns `world`.
pub trait FluidHooks: Send + Sync {
    /// An entity's bounding box is inside the fluid at `pos`.
    fn entity_inside(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
        entity: &dyn Entity,
    );

    /// A random tick hit the fluid at `pos` (only for randomly ticking fluids).
    fn random_tick(&self, world: &Arc<World>, pos: BlockPos, state: BlockStateId);

    /// The fluid is about to flow into `replaced`. Returns `true` to drop its
    /// resources like water does.
    fn before_destroying_block(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        replaced: BlockStateId,
    ) -> bool;

    /// Neighbors changed before spreading. Returns `false` if the hook converted
    /// the fluid's block, which then must not spread.
    fn react_with_neighbors(&self, world: &Arc<World>, pos: BlockPos, state: BlockStateId) -> bool;
}

/// Behavior of both variants of one registered fluid.
pub struct ExtensionFluid {
    fluid: &'static ClientFluid,
    hooks: Option<Arc<dyn FluidHooks>>,
}

impl ExtensionFluid {
    /// Creates the behavior, with hooks when the owner installed them.
    #[must_use]
    pub fn new(fluid: &'static ClientFluid, hooks: Option<Arc<dyn FluidHooks>>) -> Self {
        Self { fluid, hooks }
    }

    fn displayed_as_water(&self) -> bool {
        self.fluid.client_source == &vanilla_fluids::WATER
    }
}

impl FluidBehavior for ExtensionFluid {
    fn fluid_type(&self) -> FluidRef {
        self.fluid.source
    }

    fn is_same(&self, other: FluidRef) -> bool {
        std::ptr::eq(other, self.fluid.source) || std::ptr::eq(other, self.fluid.flowing)
    }

    fn tick_delay(&self, _world: &Arc<World>) -> i32 {
        i32::from(self.fluid.spread.tick_delay)
    }

    fn drop_off(&self, _world: &Arc<World>) -> u8 {
        self.fluid.spread.drop_off
    }

    fn slope_find_distance(&self, _world: &Arc<World>) -> u8 {
        self.fluid.spread.slope_find_distance
    }

    fn explosion_resistance(&self) -> f32 {
        self.fluid.source.explosion_resistance
    }

    fn can_convert_to_source(&self, _world: &Arc<World>) -> bool {
        self.fluid.spread.converts_to_source
    }

    fn get_flow(&self, world: &Arc<World>, pos: BlockPos, fluid_state: FluidState) -> DVec3 {
        flowing_fluid_flow(world, pos, fluid_state)
    }

    /// Water's rule: only a different fluid falling from above replaces it.
    fn can_be_replaced_with(
        &self,
        _fluid_state: FluidState,
        _world: &Arc<World>,
        _pos: BlockPos,
        other_fluid: FluidRef,
        direction: Direction,
    ) -> bool {
        direction == Direction::Down && !self.is_same(other_fluid)
    }

    fn before_destroying_block(&self, world: &Arc<World>, pos: BlockPos, state: BlockStateId) {
        let drops = self
            .hooks
            .as_ref()
            .is_none_or(|hooks| hooks.before_destroying_block(world, pos, state));
        if drops {
            world.drop_resources(state, pos);
        }
    }

    /// Applies the displayed fluid's base effect first: a fluid that looks like
    /// water puts out burning entities, as the client expects.
    fn entity_inside(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        entity: &dyn Entity,
        effect_collector: &mut InsideBlockEffectCollector,
    ) {
        if self.displayed_as_water() {
            effect_collector.apply(InsideBlockEffectType::Extinguish);
        }
        if let Some(hooks) = &self.hooks {
            hooks.entity_inside(world, pos, world.get_block_state(pos), entity);
        }
    }

    fn random_tick(&self, world: &Arc<World>, pos: BlockPos) {
        if let Some(hooks) = &self.hooks {
            hooks.random_tick(world, pos, world.get_block_state(pos));
        }
    }

    fn react_with_neighbors(&self, world: &Arc<World>, pos: BlockPos) -> bool {
        self.hooks
            .as_ref()
            .is_none_or(|hooks| hooks.react_with_neighbors(world, pos, world.get_block_state(pos)))
    }

    fn tick(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        block_state: BlockStateId,
        fluid_state: FluidState,
    ) {
        self.base_tick(world, pos, block_state, fluid_state);
    }

    fn spread(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        block_state: BlockStateId,
        fluid_state: FluidState,
    ) {
        self.base_spread(world, pos, block_state, fluid_state);
    }
}

impl FlowingFluid for ExtensionFluid {}
