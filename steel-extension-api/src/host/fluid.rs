//! Host side of fluid hook registration.

use std::sync::Arc;

use steel_core::behavior::{FLUID_BEHAVIORS, FluidBehaviorRegistry};
use steel_core::entity::Entity as CoreEntity;
use steel_core::fluid::FluidHooks;
use steel_core::world::World as CoreWorld;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_utils::{BlockPos as CoreBlockPos, BlockStateId, Identifier};

use super::HookRegistration;
use crate::block_behavior::types::Entity;
use crate::fluid_behavior::{DynFluidBehavior, FluidBehaviorDyn, api::FluidBehaviors};
use crate::{ApiResult, BlockPos, BlockState, World};

pub(crate) struct FluidRegistration<'a> {
    pub registry: &'a mut FluidBehaviorRegistry,
    pub config: &'a str,
}

pub(crate) fn subscribe(
    hooks: &mut HookRegistration<'_>,
    callback: extern "C" fn(FluidBehaviors<'_>) -> ApiResult<()>,
) -> Result<(), String> {
    hooks.extension.begin_registration();
    let extension = Arc::clone(hooks.extension);
    let result = FluidBehaviorRegistry::on_registration(move |registry| {
        extension.run_registration("fluid", || {
            let mut access = FluidRegistration {
                registry,
                config: &extension.config,
            };
            callback(FluidBehaviors::from_host(&mut access))
        });
    });
    if result.is_err() {
        hooks.extension.cancel_registration();
    }
    result
}

pub(crate) fn set_fluid_behavior(
    registration: &mut FluidRegistration<'_>,
    fluid: &str,
    behavior: DynFluidBehavior,
) -> Result<(), String> {
    let key = fluid.parse::<Identifier>().map_err(ToString::to_string)?;
    registration
        .registry
        .set_fluid_hooks(&key, Arc::new(Adapter(behavior)))
}

pub(crate) fn schedule_fluid_tick(world: &Arc<CoreWorld>, pos: BlockPos) -> Result<(), String> {
    let pos = CoreBlockPos::new(pos.x, pos.y, pos.z);
    if !world.is_in_valid_bounds(pos) {
        return Err(format!("{pos:?} is outside the world"));
    }
    let fluid = world.get_block_state(pos).get_fluid_state().fluid_id;
    if fluid.is_empty {
        return Ok(());
    }
    let delay = FLUID_BEHAVIORS.get_behavior(fluid).tick_delay(world);
    world.schedule_fluid_tick_default(pos, fluid, delay);
    Ok(())
}

const fn api_pos(pos: CoreBlockPos) -> BlockPos {
    BlockPos {
        x: pos.x(),
        y: pos.y(),
        z: pos.z(),
    }
}

const fn api_state(state: BlockStateId) -> BlockState {
    BlockState { id: state.0 }
}

/// Calls an extension's hooks from the host's fluid behavior.
struct Adapter(DynFluidBehavior);

impl FluidHooks for Adapter {
    fn entity_inside(
        &self,
        world: &Arc<CoreWorld>,
        pos: CoreBlockPos,
        state: BlockStateId,
        entity: &dyn CoreEntity,
    ) {
        self.0.entity_inside(
            World::from_host(world),
            api_pos(pos),
            api_state(state),
            Entity::from_host(entity),
        );
    }

    fn random_tick(&self, world: &Arc<CoreWorld>, pos: CoreBlockPos, state: BlockStateId) {
        self.0
            .random_tick(World::from_host(world), api_pos(pos), api_state(state));
    }

    fn before_destroying_block(
        &self,
        world: &Arc<CoreWorld>,
        pos: CoreBlockPos,
        replaced: BlockStateId,
    ) -> bool {
        self.0
            .before_destroying_block(World::from_host(world), api_pos(pos), api_state(replaced))
    }

    fn react_with_neighbors(
        &self,
        world: &Arc<CoreWorld>,
        pos: CoreBlockPos,
        state: BlockStateId,
    ) -> bool {
        self.0
            .react_with_neighbors(World::from_host(world), api_pos(pos), api_state(state))
    }
}
