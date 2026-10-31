//! Concrete host adapters for ABI-safe extension capabilities.
use super::{
    capabilities::{
        BonemealableDyn, DynBonemealable, DynFallable, DynGrowingPlantHeadBehavior,
        DynRailBehavior, FallableDyn, GrowingPlantHeadBehaviorDyn, RailBehaviorDyn,
        RegisteredBonemealable, RegisteredFallable, RegisteredGrowingPlantHeadBehavior,
        RegisteredRailBehavior,
    },
    core_types,
    types::{FallingBlockEntityRef, LevelReader, Random, SharedFallingBlockEntity},
};
use crate::{BlockPos, BlockState, World};
use steel_utils::locks::SyncMutex;
pub(super) mod bonemealable {
    use super::{
        BlockPos, BlockState, BonemealableDyn, DynBonemealable, LevelReader, Random,
        RegisteredBonemealable, SyncMutex, World, core_types,
    };
    static INSTANCES: SyncMutex<Vec<&'static Adapter>> = SyncMutex::new(Vec::new());
    pub(crate) struct Adapter(DynBonemealable);
    pub(crate) fn create(implementation: DynBonemealable) -> RegisteredBonemealable {
        let value = Box::leak(Box::new(Adapter(implementation)));
        let mut instances = INSTANCES.lock();
        let id = instances.len();
        instances.push(value);
        RegisteredBonemealable { id }
    }
    pub(crate) fn get(id: usize) -> &'static dyn core_types::Bonemealable {
        match INSTANCES.lock().get(id) {
            Some(value) => *value,
            None => panic!("invalid registered bonemealable capability"),
        }
    }
    impl core_types::Bonemealable for Adapter {
        fn get_bonemeal_age_increase(
            &self,
            world: &core_types::Arc<core_types::World>,
            rng: &mut dyn core_types::Rng,
        ) -> u8 {
            self.0
                .get_bonemeal_age_increase(World::from_host(world), Random::from_host(rng))
        }
        fn is_valid_bonemeal_target(
            &self,
            state: core_types::BlockStateId,
            world: &dyn core_types::LevelReader,
            pos: core_types::BlockPos,
        ) -> bool {
            self.0.is_valid_bonemeal_target(
                BlockState { id: state.0 },
                LevelReader::from_host(world),
                BlockPos {
                    x: pos.x(),
                    y: pos.y(),
                    z: pos.z(),
                },
            )
        }
        fn is_bonemeal_success(
            &self,
            state: core_types::BlockStateId,
            world: &core_types::Arc<core_types::World>,
            rng: &mut dyn core_types::Rng,
            pos: core_types::BlockPos,
        ) -> bool {
            self.0.is_bonemeal_success(
                BlockState { id: state.0 },
                World::from_host(world),
                Random::from_host(rng),
                BlockPos {
                    x: pos.x(),
                    y: pos.y(),
                    z: pos.z(),
                },
            )
        }
        fn perform_bonemeal(
            &self,
            state: core_types::BlockStateId,
            world: &core_types::Arc<core_types::World>,
            rng: &mut dyn core_types::Rng,
            pos: core_types::BlockPos,
        ) {
            self.0.perform_bonemeal(
                BlockState { id: state.0 },
                World::from_host(world),
                Random::from_host(rng),
                BlockPos {
                    x: pos.x(),
                    y: pos.y(),
                    z: pos.z(),
                },
            );
        }
        fn bonemeal_action_type(&self) -> core_types::BonemealAction {
            let result = self.0.bonemeal_action_type();
            result.into_host()
        }
    }
}
pub(super) mod fallable {
    use super::{
        BlockPos, BlockState, DynFallable, FallableDyn, FallingBlockEntityRef, RegisteredFallable,
        SharedFallingBlockEntity, SyncMutex, World, core_types,
    };
    static INSTANCES: SyncMutex<Vec<&'static Adapter>> = SyncMutex::new(Vec::new());
    pub(crate) struct Adapter(DynFallable);
    pub(crate) fn create(implementation: DynFallable) -> RegisteredFallable {
        let value = Box::leak(Box::new(Adapter(implementation)));
        let mut instances = INSTANCES.lock();
        let id = instances.len();
        instances.push(value);
        RegisteredFallable { id }
    }
    pub(crate) fn get(id: usize) -> &'static dyn core_types::Fallable {
        match INSTANCES.lock().get(id) {
            Some(value) => *value,
            None => panic!("invalid registered fallable capability"),
        }
    }
    impl core_types::Fallable for Adapter {
        fn on_land(
            &self,
            world: &core_types::Arc<core_types::World>,
            pos: core_types::BlockPos,
            state: core_types::BlockStateId,
            replaced_state: core_types::BlockStateId,
            entity: &core_types::FallingBlockEntity,
        ) {
            self.0.on_land(
                World::from_host(world),
                BlockPos {
                    x: pos.x(),
                    y: pos.y(),
                    z: pos.z(),
                },
                BlockState { id: state.0 },
                BlockState {
                    id: replaced_state.0,
                },
                FallingBlockEntityRef::from_host(entity),
            );
        }
        fn on_broken_after_fall(
            &self,
            world: &core_types::Arc<core_types::World>,
            pos: core_types::BlockPos,
            entity: &core_types::FallingBlockEntity,
        ) {
            self.0.on_broken_after_fall(
                World::from_host(world),
                BlockPos {
                    x: pos.x(),
                    y: pos.y(),
                    z: pos.z(),
                },
                FallingBlockEntityRef::from_host(entity),
            );
        }
        fn get_fall_damage_source(
            &self,
            entity: &core_types::Arc<core_types::FallingBlockEntity>,
        ) -> core_types::DamageSource {
            let result = self
                .0
                .get_fall_damage_source(SharedFallingBlockEntity::from_host(entity));
            result.into_host()
        }
        fn is_concrete_powder(&self) -> bool {
            self.0.is_concrete_powder()
        }
    }
}
pub(super) mod growing_plant_head {
    use super::{
        BlockState, DynGrowingPlantHeadBehavior, GrowingPlantHeadBehaviorDyn,
        RegisteredGrowingPlantHeadBehavior, SyncMutex, core_types,
    };
    static INSTANCES: SyncMutex<Vec<&'static Adapter>> = SyncMutex::new(Vec::new());
    pub(crate) struct Adapter(DynGrowingPlantHeadBehavior);
    pub(crate) fn create(
        implementation: DynGrowingPlantHeadBehavior,
    ) -> RegisteredGrowingPlantHeadBehavior {
        let value = Box::leak(Box::new(Adapter(implementation)));
        let mut instances = INSTANCES.lock();
        let id = instances.len();
        instances.push(value);
        RegisteredGrowingPlantHeadBehavior { id }
    }
    pub(crate) fn get(id: usize) -> &'static dyn core_types::GrowingPlantHeadBehavior {
        match INSTANCES.lock().get(id) {
            Some(value) => *value,
            None => panic!("invalid registered growing_plant_head capability"),
        }
    }
    impl core_types::GrowingPlantHeadBehavior for Adapter {
        fn is_max_age(&self, state: core_types::BlockStateId) -> bool {
            self.0.is_max_age(BlockState { id: state.0 })
        }
        fn get_max_age_state(&self, state: core_types::BlockStateId) -> core_types::BlockStateId {
            let result = self.0.get_max_age_state(BlockState { id: state.0 });
            core_types::BlockStateId(result.id)
        }
    }
}
pub(super) mod rail {
    use super::{DynRailBehavior, RailBehaviorDyn, RegisteredRailBehavior, SyncMutex, core_types};
    static INSTANCES: SyncMutex<Vec<&'static Adapter>> = SyncMutex::new(Vec::new());
    pub(crate) struct Adapter(DynRailBehavior);
    pub(crate) fn create(implementation: DynRailBehavior) -> RegisteredRailBehavior {
        let value = Box::leak(Box::new(Adapter(implementation)));
        let mut instances = INSTANCES.lock();
        let id = instances.len();
        instances.push(value);
        RegisteredRailBehavior { id }
    }
    pub(crate) fn get(id: usize) -> &'static dyn core_types::RailBehavior {
        match INSTANCES.lock().get(id) {
            Some(value) => *value,
            None => panic!("invalid registered rail capability"),
        }
    }
    impl core_types::RailBehavior for Adapter {
        fn is_straight(&self) -> bool {
            self.0.is_straight()
        }
    }
}
