//! Block behavior trait and registry.

use std::sync::{Arc, Weak};

use glam::DVec3;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, Direction};
use steel_registry::blocks::shapes::{
    BooleanOp, ShapeChannel, VoxelShape, is_shape_full_block, join_unoptimized_boxes,
};
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::fluid::{FluidRef, FluidState};
use steel_registry::item_stack::ItemStack;
use steel_registry::items::ItemRef;
use steel_registry::sound_event::SoundEventRef;
use steel_registry::vanilla_entities;
use steel_registry::{REGISTRY, RegistryEntry, RegistryExt, sound_events, vanilla_blocks};
use steel_registry::{vanilla_damage_types, vanilla_items};
use steel_utils::types::{GameType, InteractionHand, UpdateFlags};
use steel_utils::{BlockPos, BlockStateId, WorldAabb, axis::Axis};

use crate::behavior::InventoryAccess;
use crate::behavior::blocks::vegetation::bonemealable::Bonemealable;
use crate::behavior::context::{BlockHitResult, BlockPlaceContext, InteractionResult};
use crate::behavior::{BLOCK_BEHAVIORS, BlockStateBehaviorExt};
use crate::block_entity::SharedBlockEntity;
use crate::entity::ai::path::PathComputationType;
use crate::entity::{Entity, InsideBlockEffectCollector, damage::DamageSource};
use crate::fluid::is_water_fluid;
use crate::physics::collide;
use crate::player::Player;
use crate::world::{LevelAccessor, LevelReader, ScheduledTickAccess, World};
use steel_registry::vanilla_fluids;

pub struct PickupResult {
    pub filled_bucket: ItemRef,
    pub sound: Option<SoundEventRef>,
}

#[must_use]
pub(crate) fn drained_waterlogged_state(state: BlockStateId) -> Option<BlockStateId> {
    (state.try_get_value(&BlockStateProperties::WATERLOGGED) == Some(true))
        .then(|| state.set_value(&BlockStateProperties::WATERLOGGED, false))
}

pub(crate) fn pickup_waterlogged_block(
    behavior: &dyn BlockBehavior,
    world: &Arc<World>,
    pos: BlockPos,
    state: BlockStateId,
    player: Option<&Player>,
) -> Option<PickupResult> {
    let new_state = drained_waterlogged_state(state)?;
    if !can_pick_up_drained_waterlogged_state(state, player) {
        return None;
    }

    world.set_block(pos, new_state, UpdateFlags::UPDATE_ALL);

    if !behavior.can_survive(new_state, world, pos) {
        world.destroy_block(pos, true);
    }

    Some(PickupResult {
        filled_bucket: &vanilla_items::ITEMS.water_bucket,
        sound: Some(&sound_events::ITEM_BUCKET_FILL),
    })
}

fn can_pick_up_drained_waterlogged_state(state: BlockStateId, player: Option<&Player>) -> bool {
    // Vanilla BarrierBlock only delegates to SimpleWaterloggedBlock for creative players;
    // sponge passes no user and therefore must not drain it.
    if state.get_block() != &vanilla_blocks::BARRIER {
        return true;
    }

    player.is_some_and(|player| player.game_mode() == GameType::Creative)
}

pub(crate) fn schedule_placed_liquid_tick(
    level: &dyn LevelAccessor,
    pos: BlockPos,
    fluid_state: FluidState,
) {
    let delay = level.fluid_tick_delay(fluid_state.fluid_id);
    level.schedule_fluid_tick_default(pos, fluid_state.fluid_id, delay);
}

pub(crate) fn place_simple_waterlogged_liquid(
    level: &dyn LevelAccessor,
    pos: BlockPos,
    state: BlockStateId,
    fluid_state: FluidState,
) -> bool {
    if state.try_get_value(&BlockStateProperties::WATERLOGGED) != Some(false)
        || fluid_state.fluid_id != &vanilla_fluids::WATER
    {
        return false;
    }

    let new_state = state.set_value(&BlockStateProperties::WATERLOGGED, true);
    level.set_block_state(pos, new_state, UpdateFlags::UPDATE_ALL);
    schedule_placed_liquid_tick(level, pos, fluid_state);
    true
}

pub(crate) fn simple_waterlogged_is_liquid_container(state: BlockStateId) -> bool {
    state
        .try_get_value(&BlockStateProperties::WATERLOGGED)
        .is_some()
}

const COLLISION_CONTEXT_ABOVE_EPSILON: f64 = 1.0e-5;

/// Entity facts used by vanilla `CollisionContext` for block collision shapes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlockCollisionContext {
    entity_bottom: Option<f64>,
    fall_distance: f64,
    can_walk_on_powder_snow: bool,
    is_falling_block: bool,
    descending: bool,
    placement: bool,
}

impl BlockCollisionContext {
    /// Collision context for source-less collision queries.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            entity_bottom: None,
            fall_distance: 0.0,
            can_walk_on_powder_snow: false,
            is_falling_block: false,
            descending: false,
            placement: false,
        }
    }

    /// Collision context for normal entity movement.
    #[must_use]
    pub const fn entity(entity_bottom: f64, descending: bool) -> Self {
        Self {
            entity_bottom: Some(entity_bottom),
            fall_distance: 0.0,
            can_walk_on_powder_snow: false,
            is_falling_block: false,
            descending,
            placement: false,
        }
    }

    /// Collision context for vanilla pre-move collision validation.
    #[must_use]
    pub const fn pre_move(entity_bottom: f64, descending: bool) -> Self {
        Self {
            entity_bottom: Some(entity_bottom),
            fall_distance: 0.0,
            can_walk_on_powder_snow: false,
            is_falling_block: false,
            descending,
            placement: true,
        }
    }

    /// Collision context for vanilla `CollisionContext.positionContext(y)`.
    #[must_use]
    pub const fn position_context(y: f64) -> Self {
        Self {
            entity_bottom: Some(y),
            fall_distance: 0.0,
            can_walk_on_powder_snow: false,
            is_falling_block: false,
            descending: false,
            placement: false,
        }
    }

    /// Returns a copy with vanilla accumulated fall distance.
    #[must_use]
    pub const fn with_fall_distance(mut self, fall_distance: f64) -> Self {
        self.fall_distance = fall_distance;
        self
    }

    /// Returns a copy with vanilla powder-snow walkability.
    #[must_use]
    pub const fn with_can_walk_on_powder_snow(mut self, can_walk_on_powder_snow: bool) -> Self {
        self.can_walk_on_powder_snow = can_walk_on_powder_snow;
        self
    }

    /// Returns a copy with vanilla falling-block collision context.
    #[must_use]
    pub const fn with_falling_block(mut self, is_falling_block: bool) -> Self {
        self.is_falling_block = is_falling_block;
        self
    }

    /// Returns accumulated vanilla fall distance for context-sensitive block collision.
    #[must_use]
    pub const fn fall_distance(self) -> f64 {
        self.fall_distance
    }

    /// Returns whether the source entity can walk on powder snow.
    #[must_use]
    pub const fn can_walk_on_powder_snow(self) -> bool {
        self.can_walk_on_powder_snow
    }

    /// Returns whether the source entity is a vanilla falling block.
    #[must_use]
    pub const fn is_falling_block(self) -> bool {
        self.is_falling_block
    }

    /// Returns whether the source entity is descending through context-sensitive blocks.
    #[must_use]
    pub const fn is_descending(self) -> bool {
        self.descending
    }

    /// Returns whether this context is used for placement-style collision checks.
    #[must_use]
    pub const fn is_placement(self) -> bool {
        self.placement
    }

    /// Vanilla `EntityCollisionContext.isAbove`.
    #[must_use]
    pub fn is_above(self, shape: VoxelShape, pos: BlockPos, default_value: bool) -> bool {
        let Some(entity_bottom) = self.entity_bottom else {
            return default_value;
        };

        entity_bottom > f64::from(pos.y()) + shape.max(Axis::Y) - COLLISION_CONTEXT_ABOVE_EPSILON
    }
}

/// Entity facts needed by `Block.updateEntityMovementAfterFallOn`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EntityLandingContext {
    /// Entity velocity before the block landing hook adjusts it.
    pub velocity: DVec3,
    /// Whether the entity uses vanilla living-entity bounce behavior.
    pub is_living_entity: bool,
    /// Whether vanilla bounce behavior should be suppressed.
    pub suppresses_bounce: bool,
}

/// Entity facts needed by `Block.fallOn`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EntityFallOnFacts {
    /// Vanilla entity type of the landing entity.
    pub entity_type: EntityTypeRef,
    /// Whether the landing entity implements vanilla living-entity behavior.
    pub is_living_entity: bool,
    /// Current entity bounding-box X/Z width.
    pub bounding_box_width: f64,
    /// Current entity bounding-box height.
    pub bounding_box_height: f64,
    /// Vanilla small and big living-entity fall sounds.
    pub fall_sounds: (SoundEventRef, SoundEventRef),
}

impl EntityFallOnFacts {
    /// Creates fall-on facts from explicit entity values.
    #[must_use]
    pub const fn new(
        entity_type: EntityTypeRef,
        is_living_entity: bool,
        bounding_box_width: f64,
        bounding_box_height: f64,
        fall_sounds: (SoundEventRef, SoundEventRef),
    ) -> Self {
        Self {
            entity_type,
            is_living_entity,
            bounding_box_width,
            bounding_box_height,
            fall_sounds,
        }
    }

    /// Creates fall-on facts from an entity.
    #[must_use]
    pub fn from_entity(entity: &dyn Entity) -> Self {
        let bounding_box = entity.bounding_box();
        Self::new(
            entity.entity_type(),
            entity.is_living_entity(),
            bounding_box.width(),
            bounding_box.height(),
            entity.fall_sounds(),
        )
    }

    /// Returns true for vanilla players.
    #[must_use]
    pub fn is_player(self) -> bool {
        self.entity_type == &vanilla_entities::PLAYER
    }

    /// Vanilla farmland trampling size check:
    /// `getBbWidth() * getBbWidth() * getBbHeight()`.
    #[must_use]
    pub fn bounding_box_width_squared_height(self) -> f64 {
        self.bounding_box_width * self.bounding_box_width * self.bounding_box_height
    }
}

/// Entity facts needed by `Block.fallOn`.
#[derive(Clone, Copy)]
pub struct EntityFallOnContext<'a> {
    /// Accumulated vanilla fall distance at landing time.
    pub fall_distance: f64,
    /// Whether vanilla bounce behavior should be suppressed.
    pub suppresses_bounce: bool,
    /// Entity facts available to vanilla fall-on hooks.
    pub entity: EntityFallOnFacts,
    /// Source entity for vanilla side effects such as game events.
    pub source_entity: Option<&'a dyn Entity>,
}

impl<'a> EntityFallOnContext<'a> {
    /// Creates a fall-on context for a ground collision.
    #[must_use]
    pub const fn new(
        fall_distance: f64,
        suppresses_bounce: bool,
        entity: EntityFallOnFacts,
        source_entity: Option<&'a dyn Entity>,
    ) -> Self {
        Self {
            fall_distance,
            suppresses_bounce,
            entity,
            source_entity,
        }
    }

    /// Creates a fall-on context from a landing entity.
    #[must_use]
    pub fn from_entity(fall_distance: f64, entity: &'a dyn Entity) -> Self {
        Self::new(
            fall_distance,
            entity.is_suppressing_bounce(),
            EntityFallOnFacts::from_entity(entity),
            Some(entity),
        )
    }

    /// Returns this context with a transformed fall distance.
    #[must_use]
    pub const fn with_fall_distance(mut self, fall_distance: f64) -> Self {
        self.fall_distance = fall_distance;
        self
    }

    /// Returns the source entity for vanilla side effects.
    #[must_use]
    pub const fn source_entity(self) -> Option<&'a dyn Entity> {
        self.source_entity
    }
}

/// Fall damage requested by a block landing hook.
#[derive(Debug, Clone)]
pub struct EntityFallDamage {
    /// Fall distance to pass to `Entity.causeFallDamage`.
    pub fall_distance: f64,
    /// Block-specific damage multiplier.
    pub damage_modifier: f32,
    /// Damage source for this landing.
    pub source: DamageSource,
}

impl EntityFallDamage {
    /// Creates a fall-damage action.
    #[must_use]
    pub const fn new(fall_distance: f64, damage_modifier: f32, source: DamageSource) -> Self {
        Self {
            fall_distance,
            damage_modifier,
            source,
        }
    }
}

impl EntityLandingContext {
    /// Creates a landing context for a vertical movement collision.
    #[must_use]
    pub const fn new(velocity: DVec3, is_living_entity: bool, suppresses_bounce: bool) -> Self {
        Self {
            velocity,
            is_living_entity,
            suppresses_bounce,
        }
    }

    /// Vanilla default `Block.updateEntityMovementAfterFallOn` result.
    #[must_use]
    pub const fn default_velocity_after_fall_on(self) -> DVec3 {
        DVec3::new(self.velocity.x, 0.0, self.velocity.z)
    }
}

/// Vanilla `Block.pushEntitiesUp` for block-state replacements that add collision.
///
/// Returns `new_state` so callers can mirror vanilla call sites that transform
/// the replacement state before setting it in the world.
pub(crate) fn push_entities_up(
    old_state: BlockStateId,
    new_state: BlockStateId,
    world: &Arc<World>,
    pos: BlockPos,
) -> BlockStateId {
    let added_collision = added_collision_boxes(old_state, new_state, world, pos);
    let Some(query_box) = world_aabb_bounds(&added_collision) else {
        return new_state;
    };

    for entity in world.get_entities_in_aabb(&query_box) {
        let offset = collide(
            Axis::Y,
            &entity.bounding_box().translate(DVec3::ZERO.with_y(1.0)),
            &added_collision,
            -1.0,
        );
        if let Err(error) =
            entity.try_set_position(entity.position() + DVec3::new(0.0, 1.0 + offset, 0.0))
        {
            log::debug!(
                "Failed to push entity {} up after block collision change at {pos:?}: {error}",
                entity.id()
            );
        }
    }

    new_state
}

fn added_collision_boxes(
    old_state: BlockStateId,
    new_state: BlockStateId,
    world: &Arc<World>,
    pos: BlockPos,
) -> Vec<WorldAabb> {
    let context = BlockCollisionContext::empty();
    let old_shape = BLOCK_BEHAVIORS
        .get_behavior(old_state.get_block())
        .get_collision_shape(old_state, world.as_ref(), pos, context);
    let new_shape = BLOCK_BEHAVIORS
        .get_behavior(new_state.get_block())
        .get_collision_shape(new_state, world.as_ref(), pos, context);

    join_unoptimized_boxes(old_shape, new_shape, BooleanOp::OnlySecond)
        .into_iter()
        .map(|aabb| aabb.at_block(pos))
        .collect()
}

fn world_aabb_bounds(boxes: &[WorldAabb]) -> Option<WorldAabb> {
    let first = boxes.first()?;
    let mut min_x = first.min_x();
    let mut min_y = first.min_y();
    let mut min_z = first.min_z();
    let mut max_x = first.max_x();
    let mut max_y = first.max_y();
    let mut max_z = first.max_z();

    for aabb in boxes {
        min_x = min_x.min(aabb.min_x());
        min_y = min_y.min(aabb.min_y());
        min_z = min_z.min(aabb.min_z());
        max_x = max_x.max(aabb.max_x());
        max_y = max_y.max(aabb.max_y());
        max_z = max_z.max(aabb.max_z());
    }

    Some(WorldAabb::new(min_x, min_y, min_z, max_x, max_y, max_z))
}

/// Trait defining the behavior of a block.
///
/// This trait handles all dynamic/functional aspects of blocks:
/// - Placement logic
/// - Neighbor updates
/// - Player interactions
/// - State changes
pub trait BlockBehavior: Send + Sync {
    /// Returns the Rust type name of the concrete behavior implementation.
    #[cfg(feature = "flint")]
    #[must_use]
    #[expect(clippy::absolute_paths, reason = "easier for features")]
    fn type_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    /// Called when a player uses an empty bucket on this block.
    ///
    /// Should:
    /// - Remove or modify the block
    /// - Return the filled bucket item to give
    ///
    /// Return None if pickup failed.
    #[expect(
        unused_variables,
        reason = "default trait implementation ignores all params"
    )]
    fn pickup_block(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
        player: Option<&Player>,
    ) -> Option<PickupResult> {
        None
    }
    /// Called when a neighboring block changes shape.
    /// Returns the new state for this block after considering the neighbor change.
    fn update_shape(
        &self,
        state: BlockStateId,
        _world: &dyn ScheduledTickAccess,
        _pos: BlockPos,
        _direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        state
    }

    /// Returns whether this block can survive at the given position.
    ///
    /// Vanilla parity: `BlockBehavior.canSurvive(BlockState, LevelReader, BlockPos)`.
    ///
    /// Used during placement validation, shape updates (to break unsupported
    /// blocks), and when removing water from waterlogged blocks. The default
    /// returns `true`; override for blocks that require physical support
    /// (torches, buttons, candles, cactus, etc.).
    #[expect(
        unused_variables,
        reason = "default trait implementation ignores all params"
    )]
    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        true
    }

    /// Returns the block state to use when placing this block.
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId>;

    /// Called when this block is placed in the world.
    ///
    /// # Arguments
    /// * `state` - The new block state that was placed
    /// * `world` - The world the block was placed in
    /// * `pos` - The position where the block was placed
    /// * `old_state` - The previous block state at this position
    /// * `moved_by_piston` - Whether the block was moved by a piston
    #[expect(
        unused_variables,
        reason = "default trait implementation ignores all params"
    )]
    fn on_place(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        old_state: BlockStateId,
        moved_by_piston: bool,
    ) {
        // Default: no-op
    }

    /// Called by block items after this block has been placed by an entity.
    ///
    /// Vanilla parity: `Block.setPlacedBy(Level, BlockPos, BlockState, LivingEntity, ItemStack)`.
    /// Steel passes lazy inventory access instead of a borrowed stack so the
    /// caller does not hold the inventory lock while dispatching block behavior.
    /// This is intentionally separate from [`on_place`], which fires for any
    /// world block mutation.
    #[expect(
        unused_variables,
        reason = "default trait implementation ignores all params"
    )]
    fn set_placed_by(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        player: Option<&Player>,
        inv: &InventoryAccess,
    ) {
        // Default: no-op
    }

    /// Called before a player removes this block.
    ///
    /// Vanilla parity: `Block.playerWillDestroy(Level, BlockPos, BlockState, Player)`.
    /// The returned state is the state used for tool damage and loot after the
    /// block is removed.
    #[expect(
        unused_variables,
        reason = "default trait implementation ignores all params"
    )]
    fn player_will_destroy(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        player: &Player,
    ) -> BlockStateId {
        state
    }

    /// Called after this block is removed from the world, to affect neighbors.
    ///
    /// This is used for things like rails notifying neighbors when removed.
    ///
    /// # Arguments
    /// * `state` - The block state that was removed
    /// * `world` - The world the block was removed from
    /// * `pos` - The position where the block was removed
    /// * `moved_by_piston` - Whether the block was moved by a piston
    #[expect(
        unused_variables,
        reason = "default trait implementation ignores all params"
    )]
    fn affect_neighbors_after_removal(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        moved_by_piston: bool,
    ) {
        // Default: no-op
    }

    /// Called when a player uses an item on this block.
    ///
    /// Returns `TryEmptyHandInteraction` by default to fall through to item use.
    /// Override this to handle block-specific interactions (e.g., opening chests,
    /// using buttons, etc.).
    #[expect(
        unused_variables,
        clippy::too_many_arguments,
        reason = "default trait implementation ignores all params; argument count matches vanilla signature"
    )]
    fn use_item_on(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        player: &Player,
        hand: InteractionHand,
        hit_result: &BlockHitResult,
        inv: &mut InventoryAccess,
    ) -> InteractionResult {
        InteractionResult::TryEmptyHandInteraction
    }

    /// Called when a player uses this block without an item (or as a fallback
    /// when `use_item_on` returns `TryEmptyHandInteraction`).
    ///
    /// Returns `Pass` by default. Override this for blocks that have interactions
    /// without needing an item (e.g., buttons, levers, repeaters).
    #[expect(
        unused_variables,
        reason = "default trait implementation ignores all params"
    )]
    fn use_without_item(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        player: &Player,
        hit_result: &BlockHitResult,
        inv: &mut InventoryAccess,
    ) -> InteractionResult {
        InteractionResult::Pass
    }

    /// Called when a neighboring block changes (not shape-related).
    ///
    /// This is the Rust equivalent of vanilla's `BlockState.handleNeighborChanged()`.
    /// Used by redstone components, doors, and other blocks that react to neighbor changes.
    ///
    /// # Arguments
    /// * `state` - The current block state
    /// * `world` - The world
    /// * `pos` - Position of this block
    /// * `source_block` - The block type that changed
    /// * `moved_by_piston` - Whether the change was caused by a piston
    #[expect(
        unused_variables,
        reason = "default trait implementation ignores all params"
    )]
    fn handle_neighbor_changed(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        source_block: BlockRef,
        moved_by_piston: bool,
    ) {
        // Default: no-op
        // Override for redstone components, doors, etc.
    }

    /// Returns the item stack to give when a player picks this block (middle click).
    ///
    /// The default implementation looks up an item with the same key as the block.
    /// Override this for blocks where the pick item differs from the block key
    /// (e.g., crops → seeds, redstone wire → redstone dust, wall torch → torch).
    ///
    /// # Arguments
    /// * `block` - The block being picked
    /// * `_state` - The block state (some blocks vary pick item based on state)
    /// * `_include_data` - Whether to include block entity data (creative + Ctrl)
    #[expect(
        unused_variables,
        reason = "default implementation only uses `block`; state/include_data are for overrides"
    )]
    fn get_clone_item_stack(
        &self,
        block: BlockRef,
        state: BlockStateId,
        include_data: bool,
    ) -> Option<ItemStack> {
        // Default: look up item by block's key
        REGISTRY.items.by_key(&block.key).map(ItemStack::new)
    }

    /// Returns whether this block should receive random ticks.
    ///
    /// Override to return true for blocks like crops, grass, ice, fire, etc.
    /// This is used to optimize chunk ticking by skipping sections with no
    /// randomly-ticking blocks.
    #[expect(
        unused_variables,
        reason = "default trait implementation ignores all params"
    )]
    fn is_randomly_ticking(&self, state: BlockStateId) -> bool {
        false
    }

    /// Returns whether this block state is pathfindable for the supplied vanilla path computation.
    ///
    /// Vanilla baseline for `BlockBehaviour.isPathfindable`.
    fn is_pathfindable(&self, state: BlockStateId, computation_type: PathComputationType) -> bool {
        match computation_type {
            PathComputationType::Land | PathComputationType::Air => {
                !is_shape_full_block(state.get_static_collision_shape())
            }
            PathComputationType::Water => is_water_fluid(state.get_fluid_state().fluid_id),
        }
    }

    /// Mirrors vanilla `DoorBlock.isWoodenDoor`.
    ///
    /// Despite the vanilla name, this returns true for any door block type that
    /// can be opened by hand.
    #[expect(
        unused_variables,
        reason = "default trait implementation ignores all params"
    )]
    fn is_wooden_door(&self, state: BlockStateId) -> bool {
        false
    }

    /// Mirrors vanilla `DoorBlock.setOpen` for AI door goals.
    #[expect(
        unused_variables,
        reason = "default trait implementation ignores all params"
    )]
    fn set_door_open(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        source_entity: Option<&dyn Entity>,
        open: bool,
    ) -> bool {
        false
    }

    /// Returns this block state's collision shape for the supplied collision context.
    ///
    /// Vanilla baseline for `BlockState.getCollisionShape(BlockGetter, BlockPos, CollisionContext)`.
    #[expect(
        unused_variables,
        reason = "default trait implementation uses static registry shape"
    )]
    fn default_get_collision_shape(
        &self,
        state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
        context: BlockCollisionContext,
    ) -> VoxelShape {
        state.get_static_collision_shape()
    }

    /// Returns this block state's collision shape for the supplied collision context.
    ///
    /// Overrides that mirror vanilla `super.getCollisionShape(...)` should call
    /// [`Self::default_get_collision_shape`].
    fn get_collision_shape(
        &self,
        state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
        context: BlockCollisionContext,
    ) -> VoxelShape {
        self.default_get_collision_shape(state, world, pos, context)
    }

    /// Returns a block-local translation for this block state's collision shape.
    ///
    /// Vanilla baseline for `BlockState.getOffset(BlockPos)` where
    /// `getCollisionShape` delegates to the offset outline shape.
    #[expect(
        unused_variables,
        reason = "default trait implementation ignores world and collision context"
    )]
    fn get_collision_shape_offset(
        &self,
        state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
        context: BlockCollisionContext,
    ) -> DVec3 {
        if state
            .get_block()
            .shape_offsets
            .uses_offset(ShapeChannel::Collision)
        {
            return state.get_offset(pos);
        }

        DVec3::ZERO
    }

    /// Returns this block state's shape used by vanilla entity-inside effects.
    ///
    /// Vanilla baseline for
    /// `BlockState.getEntityInsideCollisionShape(BlockGetter, BlockPos, Entity)`.
    #[expect(
        unused_variables,
        reason = "vanilla default is a full block independent of state, world, position, and entity"
    )]
    fn default_get_entity_inside_collision_shape(
        &self,
        state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
        entity: &dyn Entity,
    ) -> VoxelShape {
        VoxelShape::FULL_BLOCK
    }

    /// Returns this block state's shape used by vanilla entity-inside effects.
    fn get_entity_inside_collision_shape(
        &self,
        state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
        entity: &dyn Entity,
    ) -> VoxelShape {
        self.default_get_entity_inside_collision_shape(state, world, pos, entity)
    }

    /// Called on random tick for blocks that support random ticking.
    ///
    /// This is only called if `is_randomly_ticking()` returns true.
    /// Used for crop growth, grass spread, ice melting, fire behavior, etc.
    ///
    /// # Arguments
    /// * `state` - The current block state
    /// * `world` - The world the block is in
    /// * `pos` - The position of the block
    #[expect(
        unused_variables,
        reason = "default trait implementation ignores all params"
    )]
    fn random_tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        // Default: no-op
    }

    /// Called when a scheduled tick fires for this block.
    ///
    /// Unlike `random_tick`, scheduled ticks are deterministic — they fire after
    /// a precise delay set by `World::schedule_block_tick`. Used for buttons
    /// unpressing, repeaters firing, fluids flowing, etc.
    ///
    /// # Arguments
    /// * `state` - The current block state
    /// * `world` - The world the block is in
    /// * `pos` - The position of the block
    #[expect(
        unused_variables,
        reason = "default trait implementation ignores all params"
    )]
    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        // Default: no-op
    }

    /// Default entity-inside hook.
    ///
    /// Overrides that mirror vanilla `super.entityInside(...)` should call
    /// [`Self::default_entity_inside`].
    #[expect(
        unused_variables,
        reason = "default trait implementation ignores all params"
    )]
    fn default_entity_inside(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        entity: &dyn Entity,
        effect_collector: &mut InsideBlockEffectCollector,
        is_precise: bool,
    ) {
    }

    /// Called when an entity is inside this block's collision area.
    ///
    /// Used by cactus (damage), fire (ignite), sweet berry bush (slow + damage), etc.
    ///
    /// # Arguments
    /// * `state` - The current block state
    /// * `world` - The world
    /// * `pos` - The position of the block
    /// * `entity` - The entity inside the block
    fn entity_inside(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        entity: &dyn Entity,
        effect_collector: &mut InsideBlockEffectCollector,
        is_precise: bool,
    ) {
        self.default_entity_inside(state, world, pos, entity, effect_collector, is_precise);
    }

    /// Default fall-on hook.
    ///
    /// Overrides that mirror vanilla `super.fallOn(...)` should call
    /// [`Self::default_fall_on`].
    #[expect(
        unused_variables,
        reason = "default trait implementation ignores state, world, and pos"
    )]
    fn default_fall_on(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        context: EntityFallOnContext<'_>,
    ) -> Option<EntityFallDamage> {
        Some(EntityFallDamage::new(
            context.fall_distance,
            1.0,
            DamageSource::environment(&vanilla_damage_types::FALL),
        ))
    }

    /// Called when an entity lands on this block.
    ///
    /// Vanilla parity: `Block.fallOn(Level, BlockState, BlockPos, Entity, double)`.
    fn fall_on(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        context: EntityFallOnContext<'_>,
    ) -> Option<EntityFallDamage> {
        self.default_fall_on(state, world, pos, context)
    }

    /// Called after fall damage requested by [`BlockBehavior::fall_on`] is applied.
    ///
    /// Vanilla parity hook for block-specific fall side effects that depend on
    /// whether `Entity.causeFallDamage` returned true.
    #[expect(
        unused_variables,
        reason = "default trait implementation ignores all params"
    )]
    fn after_fall_on_damage(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        entity: &dyn Entity,
        fall_damage: &EntityFallDamage,
        damage_applied: bool,
    ) {
    }

    /// Default post-fall movement hook.
    ///
    /// Overrides that mirror vanilla `super.updateEntityMovementAfterFallOn(...)`
    /// should call [`Self::default_update_entity_movement_after_fall_on`].
    #[expect(
        unused_variables,
        reason = "default trait implementation ignores state, world, and pos"
    )]
    fn default_update_entity_movement_after_fall_on(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        context: EntityLandingContext,
    ) -> DVec3 {
        context.default_velocity_after_fall_on()
    }

    /// Updates entity velocity after a vertical movement collision with this block.
    ///
    /// Vanilla mutates the entity in `Block.updateEntityMovementAfterFallOn`.
    /// Steel returns the velocity to apply so movement resolution keeps entity
    /// state changes centralized in [`Entity::move_entity`].
    fn update_entity_movement_after_fall_on(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        context: EntityLandingContext,
    ) -> DVec3 {
        self.default_update_entity_movement_after_fall_on(state, world, pos, context)
    }

    /// Default step-on hook.
    ///
    /// Overrides that mirror vanilla `super.stepOn(...)` should call
    /// [`Self::default_step_on`].
    #[expect(
        unused_variables,
        reason = "default trait implementation ignores all params"
    )]
    fn default_step_on(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        entity: &dyn Entity,
    ) {
    }

    /// Called when an entity steps on this block while on ground.
    ///
    /// Vanilla parity: `Block.stepOn(Level, BlockPos, BlockState, Entity)`.
    fn step_on(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos, entity: &dyn Entity) {
        self.default_step_on(state, world, pos, entity);
    }

    /// Returns whether this block has an associated block entity.
    ///
    /// Override to return `true` for blocks like chests, furnaces, signs, etc.
    fn has_block_entity(&self) -> bool {
        false
    }

    /// Creates a new block entity for this block.
    ///
    /// Only called if `has_block_entity()` returns `true`.
    ///
    /// # Arguments
    /// * `level` - Weak reference to the world
    /// * `pos` - The position where the block entity will be placed
    /// * `state` - The block state for this block entity
    #[expect(
        unused_variables,
        reason = "default trait implementation ignores all params"
    )]
    fn new_block_entity(
        &self,
        level: Weak<World>,
        pos: BlockPos,
        state: BlockStateId,
    ) -> Option<SharedBlockEntity> {
        None
    }

    /// Returns whether the block entity should be kept when the block state changes.
    ///
    /// This is used when a block changes to a different block type that shares
    /// the same block entity type (e.g., different chest variants).
    ///
    /// # Arguments
    /// * `old_state` - The previous block state
    /// * `new_state` - The new block state
    #[expect(
        unused_variables,
        reason = "default trait implementation ignores all params"
    )]
    fn should_keep_block_entity(&self, old_state: BlockStateId, new_state: BlockStateId) -> bool {
        false
    }

    /// Returns whether this block can provide an analog output signal to comparators.
    ///
    /// Override to return `true` for containers (chests, barrels, hoppers, etc.)
    /// and other blocks that comparators can read (composters, beehives, etc.).
    #[expect(
        unused_variables,
        reason = "default trait implementation ignores all params"
    )]
    fn has_analog_output_signal(&self, state: BlockStateId) -> bool {
        false
    }

    /// Returns the analog output signal strength (0-15) for comparators.
    ///
    /// Only called if `has_analog_output_signal()` returns `true`.
    /// For containers, this is typically based on how full they are.
    ///
    /// # Arguments
    /// * `state` - The current block state
    /// * `world` - The world
    /// * `pos` - The position of the block
    #[expect(
        unused_variables,
        reason = "default trait implementation ignores all params"
    )]
    fn get_analog_output_signal(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
    ) -> i32 {
        0
    }

    /// Returns the fluid state for this block state.
    ///
    /// Default (`SimpleWaterloggedBlock`): returns water source when `WATERLOGGED = true`,
    /// otherwise `FluidState::EMPTY`.
    ///
    /// Override for liquid blocks (water/lava) to return the appropriate fluid based on LEVEL.
    fn get_fluid_state(&self, state: BlockStateId) -> FluidState {
        if let Some(true) = state.try_get_value(&BlockStateProperties::WATERLOGGED) {
            FluidState::source(&vanilla_fluids::WATER)
        } else {
            FluidState::EMPTY
        }
    }

    /// Vanilla parity: whether this block implements `LiquidBlockContainer`.
    ///
    /// This is a block behavior capability, not just a state property. Most
    /// simple waterlogged blocks expose it through `WATERLOGGED`, but vanilla
    /// also has liquid containers without that property, such as kelp and
    /// seagrass.
    fn is_liquid_container(&self, state: BlockStateId) -> bool {
        simple_waterlogged_is_liquid_container(state)
    }

    /// Vanilla parity: `LiquidBlockContainer.canPlaceLiquid()`.
    ///
    /// Returns `true` if the given fluid type may be placed into this block at the
    /// given state.  Called by the fluid-spread logic; there is no player context
    /// here (fluid spreading has no associated player).
    ///
    /// Default (`SimpleWaterloggedBlock`): accepts source water for blocks with
    /// a `WATERLOGGED` property. Override for blocks that need different
    /// restrictions (e.g. double-slabs, barriers).
    ///
    /// Vanilla signature: `canPlaceLiquid(@Nullable LivingEntity, BlockGetter, BlockPos, BlockState, Fluid)`
    /// — the Fluid parameter is a type, not a state.
    fn can_place_liquid(&self, state: BlockStateId, fluid: FluidRef) -> bool {
        state
            .try_get_value(&BlockStateProperties::WATERLOGGED)
            .is_some()
            && fluid == &vanilla_fluids::WATER
    }

    /// Vanilla parity: `LiquidBlockContainer.canPlaceLiquid()` with a user.
    ///
    /// Runtime bucket placement supplies the acting player; fluid spread and
    /// other no-user callers should use [`can_place_liquid`].
    ///
    /// [`can_place_liquid`]: BlockBehavior::can_place_liquid
    fn can_place_liquid_with_player(
        &self,
        state: BlockStateId,
        fluid: FluidRef,
        _player: Option<&Player>,
    ) -> bool {
        self.can_place_liquid(state, fluid)
    }

    /// Vanilla parity: `BlockBehaviour.BlockStateBase.canBeReplaced(Fluid)`.
    ///
    /// This is a behavior hook because vanilla block subclasses can override the
    /// base replacement rule. The default mirrors `Block.canBeReplaced(Fluid)`.
    fn can_be_replaced_by_fluid(&self, state: BlockStateId, _fluid_block: BlockRef) -> bool {
        if state.is_air() {
            return true;
        }

        let block = state.get_block();
        block.config.replaceable || !state.is_solid()
    }

    /// Vanilla parity: `LiquidBlockContainer.placeLiquid()`.
    ///
    /// Attempts to place `fluid_state` into this block.  Returns `true` on success,
    /// `false` if placement was rejected.
    ///
    /// Default (`SimpleWaterloggedBlock`): sets `WATERLOGGED = true` and schedules
    /// a fluid tick. Vanilla's default `placeLiquid` directly accepts source
    /// water and does not delegate to `canPlaceLiquid`.
    fn place_liquid(
        &self,
        level: &dyn LevelAccessor,
        pos: BlockPos,
        state: BlockStateId,
        fluid_state: FluidState,
    ) -> bool {
        place_simple_waterlogged_liquid(level, pos, state, fluid_state)
    }

    /// Returns the trait object for Blocks that have the Bonemealable trait implemented.
    fn as_bonemealable(&self) -> Option<&dyn Bonemealable> {
        None
    }
}

/// Default block behavior that returns the block's default state for placement.
pub struct DefaultBlockBehavior {
    block: BlockRef,
}

impl DefaultBlockBehavior {
    /// Creates a new default block behavior for the given block.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for DefaultBlockBehavior {
    // TODO: This fallback only preserves generic block placement. Unported
    // SimpleWaterloggedBlock implementations still need vanilla placement and
    // update_shape water tick handling when they get real behaviors.
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }
}

/// Registry for block behaviors.
///
/// Created after the main registry is frozen. All blocks are initialized with
/// default behaviors, then custom behaviors are registered for specific blocks.
pub struct BlockBehaviorRegistry {
    behaviors: Vec<Box<dyn BlockBehavior>>,
}

impl BlockBehaviorRegistry {
    /// Get all behaviors.
    #[cfg(feature = "flint")]
    #[must_use]
    pub fn get_behaviors(&self) -> &[Box<dyn BlockBehavior>] {
        &self.behaviors
    }

    /// Creates a new behavior registry with default behaviors for all blocks.
    #[must_use]
    pub fn new() -> Self {
        let block_count = REGISTRY.blocks.len();
        let mut behaviors: Vec<Box<dyn BlockBehavior>> = Vec::with_capacity(block_count);

        // Initialize all blocks with default behavior
        for (_, block) in REGISTRY.blocks.iter() {
            behaviors.push(Box::new(DefaultBlockBehavior::new(block)));
        }

        Self { behaviors }
    }

    /// Sets a custom behavior for a block.
    pub fn set_behavior(&mut self, block: BlockRef, behavior: Box<dyn BlockBehavior>) {
        let id = block.id();
        self.behaviors[id] = behavior;
    }

    /// Gets the behavior for a block.
    #[must_use]
    pub fn get_behavior(&self, block: BlockRef) -> &dyn BlockBehavior {
        let id = block.id();
        self.behaviors[id].as_ref()
    }

    /// Gets the behavior for a block by its ID.
    #[must_use]
    pub fn get_behavior_by_id(&self, id: usize) -> Option<&dyn BlockBehavior> {
        self.behaviors.get(id).map(AsRef::as_ref)
    }

    /// Gets the behavior for a block state.
    #[must_use]
    pub fn get_behavior_for_state(&self, state: BlockStateId) -> Option<&dyn BlockBehavior> {
        let block = REGISTRY.blocks.by_state_id(state)?;
        Some(self.get_behavior(block))
    }
}

impl Default for BlockBehaviorRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use crate::test_support::TestLevel;

    use super::*;

    use steel_registry::blocks::block_state_ext::BlockStateExt;
    use steel_registry::blocks::properties::{BlockStateProperties, SlabType};
    use steel_registry::sound_events;
    use steel_registry::test_support::init_test_registry;
    use steel_registry::vanilla_blocks;

    #[test]
    fn drained_waterlogged_state_clears_waterlogged_property() {
        init_test_registry();
        let state = vanilla_blocks::OAK_SLAB
            .default_state()
            .set_value(&BlockStateProperties::WATERLOGGED, true);

        let drained = drained_waterlogged_state(state);

        assert_eq!(
            drained,
            Some(state.set_value(&BlockStateProperties::WATERLOGGED, false))
        );
    }

    #[test]
    fn drained_waterlogged_state_ignores_dry_or_non_waterloggable_blocks() {
        init_test_registry();
        let dry_slab = vanilla_blocks::OAK_SLAB
            .default_state()
            .set_value(&BlockStateProperties::WATERLOGGED, false);
        let stone = vanilla_blocks::STONE.default_state();

        assert_eq!(drained_waterlogged_state(dry_slab), None);
        assert_eq!(drained_waterlogged_state(stone), None);
    }

    #[test]
    fn waterlogged_barrier_pickup_requires_player_context() {
        init_test_registry();
        let waterlogged_slab = vanilla_blocks::OAK_SLAB
            .default_state()
            .set_value(&BlockStateProperties::WATERLOGGED, true);
        let waterlogged_barrier = vanilla_blocks::BARRIER
            .default_state()
            .set_value(&BlockStateProperties::WATERLOGGED, true);

        assert!(can_pick_up_drained_waterlogged_state(
            waterlogged_slab,
            None
        ));
        assert!(!can_pick_up_drained_waterlogged_state(
            waterlogged_barrier,
            None
        ));
    }

    #[test]
    fn default_fluid_replacement_does_not_use_waterloggable_property_alone() {
        init_test_registry();
        let double_slab = vanilla_blocks::OAK_SLAB
            .default_state()
            .set_value(&BlockStateProperties::SLAB_TYPE, SlabType::Double)
            .set_value(&BlockStateProperties::WATERLOGGED, false);
        let behavior = DefaultBlockBehavior::new(&vanilla_blocks::OAK_SLAB);

        assert!(!behavior.can_be_replaced_by_fluid(double_slab, &vanilla_blocks::WATER));
    }

    #[test]
    fn default_behavior_preserves_unported_simple_waterlogged_blocks() {
        init_test_registry();
        let dry_ladder = vanilla_blocks::LADDER
            .default_state()
            .set_value(&BlockStateProperties::WATERLOGGED, false);
        let wet_ladder = dry_ladder.set_value(&BlockStateProperties::WATERLOGGED, true);
        let behavior = DefaultBlockBehavior::new(&vanilla_blocks::LADDER);
        let level = TestLevel::default();

        assert_eq!(
            behavior.get_fluid_state(wet_ladder),
            FluidState::source(&vanilla_fluids::WATER)
        );
        assert!(behavior.is_liquid_container(dry_ladder));
        assert!(behavior.can_place_liquid(dry_ladder, &vanilla_fluids::WATER));
        assert!(behavior.place_liquid(
            &level,
            BlockPos::ZERO,
            dry_ladder,
            FluidState::source(&vanilla_fluids::WATER),
        ));
        assert_eq!(level.last_placed_state(), Some(wet_ladder));
        assert!(level.scheduled_water_tick());
    }

    #[test]
    fn fall_on_facts_use_vanilla_width_squared_height_formula() {
        let facts = EntityFallOnFacts::new(
            &vanilla_entities::PLAYER,
            true,
            0.6,
            1.8,
            (
                &sound_events::ENTITY_PLAYER_SMALL_FALL,
                &sound_events::ENTITY_PLAYER_BIG_FALL,
            ),
        );

        assert!(facts.is_player());
        assert!(facts.is_living_entity);
        assert!((facts.bounding_box_width_squared_height() - 0.648).abs() < f64::EPSILON);
    }

    #[test]
    fn world_aabb_bounds_contains_all_boxes() {
        let bounds = world_aabb_bounds(&[
            WorldAabb::new(1.0, 2.0, 3.0, 2.0, 3.0, 4.0),
            WorldAabb::new(-1.0, 4.0, 2.0, 0.0, 5.0, 6.0),
        ])
        .expect("non-empty boxes should have bounds");

        assert_eq!(bounds, WorldAabb::new(-1.0, 2.0, 2.0, 2.0, 5.0, 6.0));
    }
}
