// Every core method must be listed here; the drift test checks completeness.
contract::block_behavior_contract! {
    #[must_use]
    fn pickup_block(
        world: &core_types::Arc<core_types::World> => World<'_>,
        pos: core_types::BlockPos => BlockPos,
        state: core_types::BlockStateId => BlockState,
        player: Option<&core_types::Player> => StableOption<Player<'_>>,
    ) -> Option<core_types::PickupResult> => StableOption<PickupResult>;
    #[must_use]
    fn update_shape(
        state: core_types::BlockStateId => BlockState,
        world: &dyn core_types::ScheduledTickAccess => ScheduledTickAccess<'_>,
        pos: core_types::BlockPos => BlockPos,
        direction: core_types::Direction => Direction,
        neighbor_pos: core_types::BlockPos => BlockPos,
        neighbor_state: core_types::BlockStateId => BlockState,
    ) -> core_types::BlockStateId => BlockState;
    fn update_indirect_neighbour_shapes(
        state: core_types::BlockStateId => BlockState,
        world: &core_types::Arc<core_types::World> => World<'_>,
        pos: core_types::BlockPos => BlockPos,
        flags: core_types::UpdateFlags => UpdateFlags,
        update_limit: i32 => i32,
    );
    #[must_use]
    fn can_survive(
        state: core_types::BlockStateId => BlockState,
        world: &dyn core_types::LevelReader => LevelReader<'_>,
        pos: core_types::BlockPos => BlockPos,
    ) -> bool => bool;
    #[must_use]
    fn is_valid_spawn(
        state: core_types::BlockStateId => BlockState,
        world: &dyn core_types::LevelReader => LevelReader<'_>,
        pos: core_types::BlockPos => BlockPos,
        entity_type: core_types::EntityTypeRef => EntityType,
    ) -> bool => bool;
    #[must_use]
    fn is_possible_to_respawn_in_this(
        state: core_types::BlockStateId => BlockState,
    ) -> bool => bool;
    #[must_use]
    fn can_be_replaced(
        state: core_types::BlockStateId => BlockState,
        context: &core_types::BlockPlaceContext<'_> => BlockPlaceContext<'_>,
    ) -> bool => bool;
    #[must_use]
    fn get_state_for_placement(
        context: &core_types::BlockPlaceContext<'_> => BlockPlaceContext<'_>,
    ) -> Option<core_types::BlockStateId> => StableOption<BlockState>;
    fn on_place(
        state: core_types::BlockStateId => BlockState,
        world: &core_types::Arc<core_types::World> => World<'_>,
        pos: core_types::BlockPos => BlockPos,
        old_state: core_types::BlockStateId => BlockState,
        moved_by_piston: bool => bool,
    );
    fn set_placed_by(
        state: core_types::BlockStateId => BlockState,
        world: &core_types::Arc<core_types::World> => World<'_>,
        pos: core_types::BlockPos => BlockPos,
        source: &core_types::PlacementSource<'_> => PlacementSource<'_>,
    );
    fn attack(
        state: core_types::BlockStateId => BlockState,
        world: &core_types::Arc<core_types::World> => World<'_>,
        pos: core_types::BlockPos => BlockPos,
        player: &core_types::Player => Player<'_>,
    );
    fn player_destroy(
        world: &core_types::Arc<core_types::World> => World<'_>,
        player: &core_types::Player => Player<'_>,
        pos: core_types::BlockPos => BlockPos,
        state: core_types::BlockStateId => BlockState,
        block_entity: Option<&core_types::SharedBlockEntity> => StableOption<BlockEntityBorrow<'_>>,
        tool: &core_types::ItemStack => ItemStackRef<'_>,
    );
    #[must_use]
    fn player_will_destroy(
        state: core_types::BlockStateId => BlockState,
        world: &core_types::Arc<core_types::World> => World<'_>,
        pos: core_types::BlockPos => BlockPos,
        player: &core_types::Player => Player<'_>,
    ) -> core_types::BlockStateId => BlockState;
    fn destroy(
        state: core_types::BlockStateId => BlockState,
        world: &core_types::Arc<core_types::World> => World<'_>,
        pos: core_types::BlockPos => BlockPos,
    );
    #[must_use]
    fn get_drops(
        state: core_types::BlockStateId => BlockState,
        context: &core_types::BlockLootContext<'_> => BlockLootContext<'_>,
    ) -> Option<Vec<core_types::ItemStack>> => StableOption<ItemStacks>;
    fn spawn_after_break(
        state: core_types::BlockStateId => BlockState,
        world: &core_types::Arc<core_types::World> => World<'_>,
        pos: core_types::BlockPos => BlockPos,
        tool: &core_types::ItemStack => ItemStackRef<'_>,
        drop_experience: bool => bool,
    );
    fn on_explosion_hit(
        state: core_types::BlockStateId => BlockState,
        world: &core_types::Arc<core_types::World> => World<'_>,
        pos: core_types::BlockPos => BlockPos,
        explosion: &dyn core_types::Explosion => Explosion<'_>,
        on_hit: &mut dyn FnMut(core_types::ItemStack, core_types::BlockPos) => ExplosionDropConsumer<'_>,
    );
    fn default_on_explosion_hit(
        state: core_types::BlockStateId => BlockState,
        world: &core_types::Arc<core_types::World> => World<'_>,
        pos: core_types::BlockPos => BlockPos,
        explosion: &dyn core_types::Explosion => Explosion<'_>,
        on_hit: &mut dyn FnMut(core_types::ItemStack, core_types::BlockPos) => ExplosionDropConsumer<'_>,
    );
    fn was_exploded(
        state: core_types::BlockStateId => BlockState,
        world: &core_types::Arc<core_types::World> => World<'_>,
        pos: core_types::BlockPos => BlockPos,
        explosion: &dyn core_types::Explosion => Explosion<'_>,
    );
    fn drop_from_explosion(
        explosion: &dyn core_types::Explosion => Explosion<'_>,
    ) -> bool => bool;
    fn affect_neighbors_after_removal(
        state: core_types::BlockStateId => BlockState,
        world: &core_types::Arc<core_types::World> => World<'_>,
        pos: core_types::BlockPos => BlockPos,
        moved_by_piston: bool => bool,
    );
    #[must_use]
    fn use_item_on(
        state: core_types::BlockStateId => BlockState,
        world: &core_types::Arc<core_types::World> => World<'_>,
        pos: core_types::BlockPos => BlockPos,
        player: &core_types::Player => Player<'_>,
        hand: core_types::InteractionHand => InteractionHand,
        hit_result: &core_types::BlockHitResult => BlockHit<'_>,
        inv: &mut core_types::InventoryAccess => Inventory<'_>,
    ) -> core_types::InteractionResult => InteractionResult;
    #[must_use]
    fn use_without_item(
        state: core_types::BlockStateId => BlockState,
        world: &core_types::Arc<core_types::World> => World<'_>,
        pos: core_types::BlockPos => BlockPos,
        player: &core_types::Player => Player<'_>,
        hit_result: &core_types::BlockHitResult => BlockHit<'_>,
        inv: &mut core_types::InventoryAccess => Inventory<'_>,
    ) -> core_types::InteractionResult => InteractionResult;
    fn handle_neighbor_changed(
        state: core_types::BlockStateId => BlockState,
        world: &core_types::Arc<core_types::World> => World<'_>,
        pos: core_types::BlockPos => BlockPos,
        source_block: core_types::BlockRef => Block,
        moved_by_piston: bool => bool,
    );
    #[must_use]
    fn is_signal_source(
        state: core_types::BlockStateId => BlockState,
        context: core_types::SignalQueryContext => SignalQueryContext,
    ) -> bool => bool;
    #[must_use]
    fn is_diode(
    ) -> bool => bool;
    #[must_use]
    fn is_trapdoor(
    ) -> bool => bool;
    #[must_use]
    fn is_rail(
    ) -> bool => bool;
    #[must_use]
    fn is_piston_base(
    ) -> bool => bool;
    #[must_use]
    fn get_own_signal(
        state: core_types::BlockStateId => BlockState,
        world: &dyn core_types::LevelReader => LevelReader<'_>,
        pos: core_types::BlockPos => BlockPos,
        context: core_types::SignalQueryContext => SignalQueryContext,
    ) -> i32 => i32;
    #[must_use]
    fn get_signal(
        state: core_types::BlockStateId => BlockState,
        world: &dyn core_types::LevelReader => LevelReader<'_>,
        pos: core_types::BlockPos => BlockPos,
        direction: core_types::Direction => Direction,
        context: core_types::SignalQueryContext => SignalQueryContext,
    ) -> i32 => i32;
    #[must_use]
    fn get_direct_signal(
        state: core_types::BlockStateId => BlockState,
        world: &dyn core_types::LevelReader => LevelReader<'_>,
        pos: core_types::BlockPos => BlockPos,
        direction: core_types::Direction => Direction,
        context: core_types::SignalQueryContext => SignalQueryContext,
    ) -> i32 => i32;
    #[must_use]
    fn is_redstone_conductor(
        state: core_types::BlockStateId => BlockState,
        world: &dyn core_types::LevelReader => LevelReader<'_>,
        pos: core_types::BlockPos => BlockPos,
    ) -> bool => bool;
    #[must_use]
    fn trigger_event(
        state: core_types::BlockStateId => BlockState,
        world: &core_types::Arc<core_types::World> => World<'_>,
        pos: core_types::BlockPos => BlockPos,
        param_a: i32 => i32,
        param_b: i32 => i32,
    ) -> bool => bool;
    #[must_use]
    fn get_clone_item_stack(
        block: core_types::BlockRef => Block,
        state: core_types::BlockStateId => BlockState,
        block_entity: Option<core_types::SharedBlockEntity> => StableOption<BlockEntity>,
        include_data: bool => bool,
    ) -> Option<core_types::ItemStack> => StableOption<ItemStack>;
    #[must_use]
    fn is_pathfindable(
        state: core_types::BlockStateId => BlockState,
        computation_type: core_types::PathComputationType => PathComputationType,
    ) -> bool => bool;
    #[must_use]
    fn is_bed(
    ) -> bool => bool;
    #[must_use]
    fn get_sleep_height(
        state: core_types::BlockStateId => BlockState,
        world: &dyn core_types::LevelReader => LevelReader<'_>,
        pos: core_types::BlockPos => BlockPos,
    ) -> Option<f64> => StableOption<f64>;
    fn on_stop_sleeping(
        state: core_types::BlockStateId => BlockState,
        world: &core_types::Arc<core_types::World> => World<'_>,
        pos: core_types::BlockPos => BlockPos,
    );
    #[must_use]
    fn is_liquid_block(
    ) -> bool => bool;
    #[must_use]
    fn is_wooden_door(
        state: core_types::BlockStateId => BlockState,
    ) -> bool => bool;
    #[must_use]
    fn set_door_open(
        state: core_types::BlockStateId => BlockState,
        world: &core_types::Arc<core_types::World> => World<'_>,
        pos: core_types::BlockPos => BlockPos,
        source_entity: Option<&dyn core_types::Entity> => StableOption<Entity<'_>>,
        open: bool => bool,
    ) -> bool => bool;
    #[must_use]
    fn default_get_collision_shape(
        state: core_types::BlockStateId => BlockState,
        world: &dyn core_types::LevelReader => LevelReader<'_>,
        pos: core_types::BlockPos => BlockPos,
        context: core_types::BlockCollisionContext => CollisionContext,
    ) -> core_types::VoxelShape => VoxelShape;
    #[must_use]
    fn get_collision_shape(
        state: core_types::BlockStateId => BlockState,
        world: &dyn core_types::LevelReader => LevelReader<'_>,
        pos: core_types::BlockPos => BlockPos,
        context: core_types::BlockCollisionContext => CollisionContext,
    ) -> core_types::VoxelShape => VoxelShape;
    #[must_use]
    fn get_collision_shape_offset(
        state: core_types::BlockStateId => BlockState,
        world: &dyn core_types::LevelReader => LevelReader<'_>,
        pos: core_types::BlockPos => BlockPos,
        context: core_types::BlockCollisionContext => CollisionContext,
    ) -> core_types::DVec3 => Vector3;
    #[must_use]
    fn get_collision_boxes(
        state: core_types::BlockStateId => BlockState,
        world: &dyn core_types::LevelReader => LevelReader<'_>,
        pos: core_types::BlockPos => BlockPos,
        context: core_types::BlockCollisionContext => CollisionContext,
    ) -> core_types::BlockCollisionBoxes => CollisionBoxes;
    #[must_use]
    fn get_block_support_boxes(
        state: core_types::BlockStateId => BlockState,
        world: &dyn core_types::LevelReader => LevelReader<'_>,
        pos: core_types::BlockPos => BlockPos,
    ) -> core_types::BlockCollisionBoxes => CollisionBoxes;
    #[must_use]
    fn is_face_sturdy(
        state: core_types::BlockStateId => BlockState,
        world: &dyn core_types::LevelReader => LevelReader<'_>,
        pos: core_types::BlockPos => BlockPos,
        direction: core_types::Direction => Direction,
        support_type: core_types::SupportType => SupportType,
    ) -> bool => bool;
    #[must_use]
    fn default_get_entity_inside_collision_shape(
        state: core_types::BlockStateId => BlockState,
        world: &dyn core_types::LevelReader => LevelReader<'_>,
        pos: core_types::BlockPos => BlockPos,
        entity: &dyn core_types::Entity => Entity<'_>,
    ) -> core_types::VoxelShape => VoxelShape;
    #[must_use]
    fn get_entity_inside_collision_shape(
        state: core_types::BlockStateId => BlockState,
        world: &dyn core_types::LevelReader => LevelReader<'_>,
        pos: core_types::BlockPos => BlockPos,
        entity: &dyn core_types::Entity => Entity<'_>,
    ) -> core_types::VoxelShape => VoxelShape;
    fn random_tick(
        state: core_types::BlockStateId => BlockState,
        world: &core_types::Arc<core_types::World> => World<'_>,
        pos: core_types::BlockPos => BlockPos,
    );
    fn tick(
        state: core_types::BlockStateId => BlockState,
        world: &core_types::Arc<core_types::World> => World<'_>,
        pos: core_types::BlockPos => BlockPos,
    );
    fn on_projectile_hit(
        state: core_types::BlockStateId => BlockState,
        world: &core_types::Arc<core_types::World> => World<'_>,
        hit: &core_types::ClipHitResult => ClipHit<'_>,
        projectile: &dyn core_types::Projectile => Projectile<'_>,
    );
    fn default_entity_inside(
        state: core_types::BlockStateId => BlockState,
        world: &core_types::Arc<core_types::World> => World<'_>,
        pos: core_types::BlockPos => BlockPos,
        entity: &dyn core_types::Entity => Entity<'_>,
        effect_collector: &mut core_types::InsideBlockEffectCollector => InsideBlockEffects<'_>,
        is_precise: bool => bool,
    );
    fn entity_inside(
        state: core_types::BlockStateId => BlockState,
        world: &core_types::Arc<core_types::World> => World<'_>,
        pos: core_types::BlockPos => BlockPos,
        entity: &dyn core_types::Entity => Entity<'_>,
        effect_collector: &mut core_types::InsideBlockEffectCollector => InsideBlockEffects<'_>,
        is_precise: bool => bool,
    );
    #[must_use]
    fn default_fall_on(
        state: core_types::BlockStateId => BlockState,
        world: &core_types::Arc<core_types::World> => World<'_>,
        pos: core_types::BlockPos => BlockPos,
        context: core_types::EntityFallOnContext<'_> => FallOnContext<'_>,
    ) -> Option<core_types::EntityFallDamage> => StableOption<FallDamage>;
    #[must_use]
    fn fall_on(
        state: core_types::BlockStateId => BlockState,
        world: &core_types::Arc<core_types::World> => World<'_>,
        pos: core_types::BlockPos => BlockPos,
        context: core_types::EntityFallOnContext<'_> => FallOnContext<'_>,
    ) -> Option<core_types::EntityFallDamage> => StableOption<FallDamage>;
    fn bounce_on(
        state: core_types::BlockStateId => BlockState,
        world: &core_types::Arc<core_types::World> => World<'_>,
        pos: core_types::BlockPos => BlockPos,
        context: core_types::EntityFallOnContext<'_> => FallOnContext<'_>,
    );
    fn after_fall_on_damage(
        state: core_types::BlockStateId => BlockState,
        world: &core_types::Arc<core_types::World> => World<'_>,
        pos: core_types::BlockPos => BlockPos,
        entity: &dyn core_types::Entity => Entity<'_>,
        fall_damage: &core_types::EntityFallDamage => FallDamageRef<'_>,
        damage_applied: bool => bool,
    );
    #[must_use]
    fn default_update_entity_movement_after_fall_on(
        state: core_types::BlockStateId => BlockState,
        world: &core_types::Arc<core_types::World> => World<'_>,
        pos: core_types::BlockPos => BlockPos,
        context: core_types::EntityLandingContext => LandingContext,
    ) -> core_types::DVec3 => Vector3;
    #[must_use]
    fn update_entity_movement_after_fall_on(
        state: core_types::BlockStateId => BlockState,
        world: &core_types::Arc<core_types::World> => World<'_>,
        pos: core_types::BlockPos => BlockPos,
        context: core_types::EntityLandingContext => LandingContext,
    ) -> (core_types::DVec3, bool) => MovementResult;
    fn default_step_on(
        state: core_types::BlockStateId => BlockState,
        world: &core_types::Arc<core_types::World> => World<'_>,
        pos: core_types::BlockPos => BlockPos,
        entity: &dyn core_types::Entity => Entity<'_>,
    );
    fn step_on(
        state: core_types::BlockStateId => BlockState,
        world: &core_types::Arc<core_types::World> => World<'_>,
        pos: core_types::BlockPos => BlockPos,
        entity: &dyn core_types::Entity => Entity<'_>,
    );
    #[must_use]
    fn new_block_entity(
        level: core_types::Weak<core_types::World> => WeakWorld,
        pos: core_types::BlockPos => BlockPos,
        state: core_types::BlockStateId => BlockState,
    ) -> core_types::BlockEntityCreation => BlockEntityCreation;
    #[must_use]
    fn beacon_beam_color(
        state: core_types::BlockStateId => BlockState,
    ) -> Option<core_types::DyeColor> => StableOption<DyeColor>;
    #[must_use]
    fn get_block_entity_ticker(
        world: &core_types::Arc<core_types::World> => World<'_>,
        state: core_types::BlockStateId => BlockState,
        block_entity_type: core_types::BlockEntityTypeRef => BlockEntityType,
    ) -> Option<core_types::BlockEntityTicker> => StableOption<BlockEntityTicker>;
    #[must_use]
    fn get_game_event_listener(
        world: &core_types::Arc<core_types::World> => World<'_>,
        block_entity: &dyn core_types::BlockEntity => BlockEntityRef<'_>,
    ) -> Option<core_types::SharedGameEventListener> => StableOption<GameEventListener>;
    #[must_use]
    fn should_keep_block_entity(
        old_state: core_types::BlockStateId => BlockState,
        new_state: core_types::BlockStateId => BlockState,
    ) -> bool => bool;
    #[must_use]
    fn brushable_data(
        state: core_types::BlockStateId => BlockState,
    ) -> Option<core_types::BrushableData> => StableOption<BrushableData>;
    #[must_use]
    fn has_analog_output_signal(
        state: core_types::BlockStateId => BlockState,
    ) -> bool => bool;
    #[must_use]
    fn get_analog_output_signal(
        state: core_types::BlockStateId => BlockState,
        world: &dyn core_types::LevelReader => LevelReader<'_>,
        pos: core_types::BlockPos => BlockPos,
        direction: core_types::Direction => Direction,
    ) -> i32 => i32;
    #[must_use]
    fn is_liquid_container(
        state: core_types::BlockStateId => BlockState,
    ) -> bool => bool;
    #[must_use]
    fn can_place_liquid(
        state: core_types::BlockStateId => BlockState,
        fluid: core_types::FluidRef => Fluid,
    ) -> bool => bool;
    #[must_use]
    fn can_place_liquid_with_player(
        state: core_types::BlockStateId => BlockState,
        fluid: core_types::FluidRef => Fluid,
        player: Option<&core_types::Player> => StableOption<Player<'_>>,
    ) -> bool => bool;
    #[must_use]
    fn can_be_replaced_by_fluid(
        state: core_types::BlockStateId => BlockState,
        fluid_block: core_types::BlockRef => Block,
    ) -> bool => bool;
    #[must_use]
    fn place_liquid(
        level: &dyn core_types::LevelAccessor => LevelAccessor<'_>,
        pos: core_types::BlockPos => BlockPos,
        state: core_types::BlockStateId => BlockState,
        fluid_state: core_types::FluidState => FluidState,
    ) -> bool => bool;
    #[must_use]
    fn as_bonemealable(
    ) -> Option<&dyn core_types::Bonemealable> => StableOption<BonemealableRef>;
    #[must_use]
    fn as_growing_plant_head(
    ) -> Option<&dyn core_types::GrowingPlantHeadBehavior> => StableOption<GrowingPlantHeadRef>;
    #[must_use]
    fn as_fallable(
    ) -> Option<&dyn core_types::Fallable> => StableOption<FallableRef>;
    #[must_use]
    fn as_rail(
    ) -> Option<&dyn core_types::RailBehavior> => StableOption<RailRef>;
    #[must_use]
    fn fits_inside_container_items(
    ) -> bool => bool;
}
