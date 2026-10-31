#[stabby::host_api]
pub mod api {
    use crate::block_behavior::{
        capabilities::{
            DynBonemealable, DynFallable, DynGrowingPlantHeadBehavior, DynRailBehavior,
            RegisteredBonemealable, RegisteredFallable, RegisteredGrowingPlantHeadBehavior,
            RegisteredRailBehavior,
        },
        types::{
            BonemealAction, BonemealableRef, DamageSource, FallableRef, FallingBlockEntityRef,
            GrowingPlantHeadRef, LevelReader, RailRef, Random, SharedFallingBlockEntity,
        },
    };
    #[cfg(feature = "host")]
    use crate::block_behavior::{capability_host, core_types};
    use crate::{BlockPos, BlockState, Registry, World};
    impl<'a> Registry<'a> {
        /// Registers a extension-defined Bonemealable implementation for the process lifetime.
        #[cfg_attr(
            feature = "host",
            expect(
                clippy::unused_self,
                reason = "capabilities may only be created during extension initialization"
            )
        )]
        #[must_use]
        pub fn create_bonemealable(
            &mut self,
            implementation: DynBonemealable,
        ) -> RegisteredBonemealable {
            capability_host::bonemealable::create(implementation)
        }
        /// Registers a extension-defined Fallable implementation for the process lifetime.
        #[cfg_attr(
            feature = "host",
            expect(
                clippy::unused_self,
                reason = "capabilities may only be created during extension initialization"
            )
        )]
        #[must_use]
        pub fn create_fallable(&mut self, implementation: DynFallable) -> RegisteredFallable {
            capability_host::fallable::create(implementation)
        }
        /// Registers a extension-defined `GrowingPlantHeadBehavior` implementation for the process lifetime.
        #[cfg_attr(
            feature = "host",
            expect(
                clippy::unused_self,
                reason = "capabilities may only be created during extension initialization"
            )
        )]
        #[must_use]
        pub fn create_growing_plant_head(
            &mut self,
            implementation: DynGrowingPlantHeadBehavior,
        ) -> RegisteredGrowingPlantHeadBehavior {
            capability_host::growing_plant_head::create(implementation)
        }
        /// Registers a extension-defined `RailBehavior` implementation for the process lifetime.
        #[cfg_attr(
            feature = "host",
            expect(
                clippy::unused_self,
                reason = "capabilities may only be created during extension initialization"
            )
        )]
        #[must_use]
        pub fn create_rail(&mut self, implementation: DynRailBehavior) -> RegisteredRailBehavior {
            capability_host::rail::create(implementation)
        }
    }
    impl RegisteredBonemealable {
        /// Borrows this registered capability without transferring its implementation.
        #[must_use]
        pub fn borrow(self) -> BonemealableRef {
            BonemealableRef::from_host(capability_host::bonemealable::get(self.id))
        }
    }
    impl BonemealableRef {
        /// Calls this capability implementation.
        #[must_use]
        pub fn get_bonemeal_age_increase<'a>(&self, world: World<'a>, rng: Random<'a>) -> u8 {
            self.host()
                .get_bonemeal_age_increase(world.host(), rng.into_host())
        }
        /// Calls this capability implementation.
        #[must_use]
        pub fn is_valid_bonemeal_target(
            &self,
            state: BlockState,
            world: LevelReader<'_>,
            pos: BlockPos,
        ) -> bool {
            self.host().is_valid_bonemeal_target(
                core_types::BlockStateId(state.id),
                world.into_host(),
                core_types::BlockPos::new(pos.x, pos.y, pos.z),
            )
        }
        /// Calls this capability implementation.
        #[must_use]
        pub fn is_bonemeal_success<'a>(
            &self,
            state: BlockState,
            world: World<'a>,
            rng: Random<'a>,
            pos: BlockPos,
        ) -> bool {
            self.host().is_bonemeal_success(
                core_types::BlockStateId(state.id),
                world.host(),
                rng.into_host(),
                core_types::BlockPos::new(pos.x, pos.y, pos.z),
            )
        }
        /// Calls this capability implementation.
        pub fn perform_bonemeal<'a>(
            &self,
            state: BlockState,
            world: World<'a>,
            rng: Random<'a>,
            pos: BlockPos,
        ) {
            self.host().perform_bonemeal(
                core_types::BlockStateId(state.id),
                world.host(),
                rng.into_host(),
                core_types::BlockPos::new(pos.x, pos.y, pos.z),
            );
        }
        /// Calls this capability implementation.
        #[must_use]
        pub fn bonemeal_action_type(&self) -> BonemealAction {
            let result = self.host().bonemeal_action_type();
            BonemealAction::from_host(result)
        }
    }
    impl RegisteredFallable {
        /// Borrows this registered capability without transferring its implementation.
        #[must_use]
        pub fn borrow(self) -> FallableRef {
            FallableRef::from_host(capability_host::fallable::get(self.id))
        }
    }
    impl FallableRef {
        /// Calls this capability implementation.
        pub fn on_land<'a>(
            &self,
            world: World<'a>,
            pos: BlockPos,
            state: BlockState,
            replaced_state: BlockState,
            entity: FallingBlockEntityRef<'a>,
        ) {
            self.host().on_land(
                world.host(),
                core_types::BlockPos::new(pos.x, pos.y, pos.z),
                core_types::BlockStateId(state.id),
                core_types::BlockStateId(replaced_state.id),
                entity.into_host(),
            );
        }
        /// Calls this capability implementation.
        pub fn on_broken_after_fall<'a>(
            &self,
            world: World<'a>,
            pos: BlockPos,
            entity: FallingBlockEntityRef<'a>,
        ) {
            self.host().on_broken_after_fall(
                world.host(),
                core_types::BlockPos::new(pos.x, pos.y, pos.z),
                entity.into_host(),
            );
        }
        /// Calls this capability implementation.
        #[must_use]
        pub fn get_fall_damage_source(&self, entity: SharedFallingBlockEntity<'_>) -> DamageSource {
            let result = self.host().get_fall_damage_source(entity.into_host());
            DamageSource::from_host(result)
        }
        /// Calls this capability implementation.
        #[must_use]
        pub fn is_concrete_powder(&self) -> bool {
            self.host().is_concrete_powder()
        }
    }
    impl RegisteredGrowingPlantHeadBehavior {
        /// Borrows this registered capability without transferring its implementation.
        #[must_use]
        pub fn borrow(self) -> GrowingPlantHeadRef {
            GrowingPlantHeadRef::from_host(capability_host::growing_plant_head::get(self.id))
        }
    }
    impl GrowingPlantHeadRef {
        /// Calls this capability implementation.
        #[must_use]
        pub fn is_max_age(&self, state: BlockState) -> bool {
            self.host().is_max_age(core_types::BlockStateId(state.id))
        }
        /// Calls this capability implementation.
        #[must_use]
        pub fn get_max_age_state(&self, state: BlockState) -> BlockState {
            let result = self
                .host()
                .get_max_age_state(core_types::BlockStateId(state.id));
            BlockState { id: result.0 }
        }
    }
    impl RegisteredRailBehavior {
        /// Borrows this registered capability without transferring its implementation.
        #[must_use]
        pub fn borrow(self) -> RailRef {
            RailRef::from_host(capability_host::rail::get(self.id))
        }
    }
    impl RailRef {
        /// Calls this capability implementation.
        #[must_use]
        pub fn is_straight(&self) -> bool {
            self.host().is_straight()
        }
    }
}
