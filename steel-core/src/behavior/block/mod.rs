//! Block behavior trait and registry.

use std::sync::{Arc, Weak};

use glam::DVec3;
use rand::rngs::ThreadRng;
use smallvec::SmallVec;
use steel_registry::block_entity_type::BlockEntityTypeRef;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, Direction};
use steel_registry::blocks::shapes::{
    BooleanOp, ShapeChannel, SupportType, VoxelShape, is_block_local_face_sturdy,
    is_shape_full_block, join_unoptimized_boxes,
};
use steel_registry::enchantment_effect::EnchantmentEffectComponent;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::fluid::{FluidRef, FluidState};
use steel_registry::item_stack::ItemStack;
use steel_registry::loot_table::{LootContext, LootTableRef};
use steel_registry::sound_event::SoundEventRef;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_entities;
use steel_registry::{REGISTRY, RegistryEntry, RegistryExt, sound_events, vanilla_blocks};
use steel_registry::{vanilla_damage_types, vanilla_items};
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::types::{GameType, InteractionHand, UpdateFlags};
use steel_utils::value_providers::IntProvider;
use steel_utils::{BlockLocalAabb, BlockPos, BlockStateId, Identifier, WorldAabb, axis::Axis};

use crate::behavior::BLOCK_BEHAVIORS;
use crate::behavior::blocks::vegetation::GrowingPlantHeadBehavior;
use crate::behavior::blocks::vegetation::bonemealable::Bonemealable;
use crate::behavior::context::{BlockHitResult, BlockPlaceContext, InteractionResult};
use crate::behavior::{InventoryAccess, PlacementSource};
use crate::block_entity::{BlockEntity, BlockEntityTicker, SharedBlockEntity};
use crate::entity::ai::path::PathComputationType;
use crate::entity::projectile::Projectile;
use crate::entity::{Entity, InsideBlockEffectCollector, damage::DamageSource, entity_loot_ref};
use crate::fluid::is_water_fluid;
use crate::physics::collide;
use crate::player::Player;
use crate::world::game_event::SharedGameEventListener;
use crate::world::{
    ClipHitResult, ConditionalBlockSetResult, LevelAccessor, LevelReader, ScheduledTickAccess,
    SignalQueryContext, World,
};
use steel_registry::vanilla_fluids;

/// Vanilla `BlockBehaviour.canBeReplaced(BlockState, BlockPlaceContext)`.
pub(crate) fn default_can_be_replaced(
    state: BlockStateId,
    context: &BlockPlaceContext<'_>,
) -> bool {
    state.is_replaceable()
        && context.with_item(|item| {
            item.is_empty() || item.item() != REGISTRY.items.by_block(state.get_block())
        })
}

/// Gets random loot from a given loot table reference and other factors, and returns
/// each item from it in a [`Vec`].
#[must_use]
pub(crate) fn drop_from_block_interact_loot_table(
    key: LootTableRef,
    interacted_block_state: BlockStateId,
    _interacted_block_entity: Option<SharedBlockEntity>,
    tool: Option<&ItemStack>,
    interacting_entity: Option<&dyn Entity>,
    rng: &mut ThreadRng,
) -> Vec<ItemStack> {
    let mut ctx = LootContext::new(rng).with_block_state(interacted_block_state);

    // TODO: Add the block entity to the context when it can be done.

    if let Some(interacting_entity) = interacting_entity {
        ctx = ctx.with_interacting_entity(entity_loot_ref(interacting_entity));
    }

    if let Some(tool) = tool {
        ctx = ctx.with_tool(tool);
    }

    key.get_random_items(&mut ctx)
}

/// Samples and applies enchantment effects to a block experience drop.
///
/// Mirrors vanilla `Block.tryDropExperience`. Mining experience is incidental
/// live-gameplay randomness, so Steel samples it from an unseeded runtime source.
pub(crate) fn try_drop_experience(
    world: &Arc<World>,
    pos: BlockPos,
    tool: &ItemStack,
    experience: &IntProvider,
) {
    let mut random = LegacyRandom::from_seed(rand::random());
    let base_experience = experience.sample(&mut random);
    let experience = tool.apply_unconditional_enchantment_value_effects(
        EnchantmentEffectComponent::BlockExperience,
        base_experience as f32,
    ) as i32;
    if experience > 0 {
        world.pop_experience(pos, experience);
    }
}

mod context;

pub use context::{
    BlockCollisionBoxes, BlockCollisionContext, BlockEntityCreation, BlockLootContext,
    EntityFallDamage, EntityFallOnContext, EntityFallOnFacts, EntityLandingContext, Fallable,
    PickupResult, RailBehavior,
};

/// Data exposed by blocks that support vanilla archaeology brushing.
#[derive(Clone, Copy, Debug)]
pub struct BrushableData {
    /// Block produced after brushing completes.
    pub turns_into: BlockRef,
    /// Sound played during a successful brush stroke.
    pub brush_sound: SoundEventRef,
    /// Sound played when brushing completes.
    pub brush_completed_sound: SoundEventRef,
}

mod waterlogging;

#[cfg(test)]
use waterlogging::{can_pick_up_drained_waterlogged_state, drained_waterlogged_state};
pub(crate) use waterlogging::{
    pickup_waterlogged_block, place_simple_waterlogged_liquid, schedule_placed_liquid_tick,
    schedule_water_tick_if_waterlogged, simple_waterlogged_is_liquid_container,
};

mod collision;

pub(crate) use collision::push_entities_up;
#[cfg(test)]
use collision::world_aabb_bounds;

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
    /// - Return the filled bucket stack to give
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
    /// Implementations also own any block or fluid ticks that Vanilla schedules
    /// from `updateShape`; the world dispatcher does not infer them from the result.
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

    /// Queues indirect neighbor-shape updates after this state changes.
    ///
    /// Vanilla's default is a no-op. Redstone wire overrides this for vertical
    /// corner connections.
    #[expect(
        unused_variables,
        reason = "default trait implementation ignores all params"
    )]
    fn update_indirect_neighbour_shapes(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        flags: UpdateFlags,
        update_limit: i32,
    ) {
    }

    /// Returns whether this block can survive at the given position.
    ///
    /// Vanilla parity: `BlockBehavior.canSurvive(BlockState, LevelReader, BlockPos)`.
    ///
    /// Used during placement validation, shape updates (to break unsupported
    /// blocks), and when removing water from waterlogged blocks. The default
    /// returns `true`; override for blocks that require physical support
    /// (torches, buttons, candles, cactus, etc.).
    fn can_survive(&self, _state: BlockStateId, _world: &dyn LevelReader, _pos: BlockPos) -> bool {
        true
    }

    /// Returns whether this block can be occupied by a forced respawn position
    fn is_possible_to_respawn_in_this(&self, state: BlockStateId) -> bool {
        !state.is_solid() && !state.get_block().config.liquid
    }

    /// Returns whether this block can be replaced by the held item during placement.
    ///
    /// Vanilla parity: `BlockState.canBeReplaced(BlockPlaceContext)`.
    ///
    /// Default behavior mirrors `BlockBehaviour.canBeReplaced`.
    fn can_be_replaced(&self, state: BlockStateId, context: &BlockPlaceContext<'_>) -> bool {
        default_can_be_replaced(state, context)
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
    /// Steel passes the placement source instead of a borrowed stack so the
    /// caller does not hold the inventory lock while dispatching block behavior
    /// and synthetic placements can retain a directly supplied stack.
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
        source: &PlacementSource<'_>,
    ) {
        // Default: no-op
    }

    /// Called when a player starts attacking this block.
    ///
    /// Vanilla parity: `Block.attack(BlockState, Level, BlockPos, Player)`.
    #[expect(
        unused_variables,
        reason = "default trait implementation ignores all params"
    )]
    fn attack(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos, player: &Player) {}

    /// Called after a player destroys this block and drops/effects are processed.
    ///
    /// Vanilla parity: `Block.playerDestroy(Level, Player, BlockPos, BlockState, BlockEntity, ItemStack)`.
    #[expect(
        unused_variables,
        reason = "default trait implementation ignores all params"
    )]
    fn player_destroy(
        &self,
        world: &Arc<World>,
        player: &Player,
        pos: BlockPos,
        state: BlockStateId,
        block_entity: Option<&SharedBlockEntity>,
        tool: &ItemStack,
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

    /// Called after a player successfully removes this block.
    ///
    /// Mirrors vanilla `Block.destroy(LevelAccessor, BlockPos, BlockState)`.
    #[expect(
        unused_variables,
        reason = "default trait implementation ignores all params"
    )]
    fn destroy(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        // Default: no-op
    }

    /// Overrides the loot generated for this block state.
    ///
    /// Returning `None` evaluates the state's normal loot table. Returning
    /// `Some` uses the provided items, including an empty list. This mirrors
    /// vanilla's per-block `getDrops` override point.
    #[expect(
        unused_variables,
        reason = "default trait implementation ignores all params"
    )]
    fn get_drops(
        &self,
        state: BlockStateId,
        context: &BlockLootContext<'_>,
    ) -> Option<Vec<ItemStack>> {
        None
    }

    /// Called for post-break effects such as experience drops.
    ///
    /// Vanilla parity: `Block.spawnAfterBreak(BlockState, ServerLevel, BlockPos,
    /// ItemStack, boolean)`. Normal block destruction invokes this after loot;
    /// other destruction paths retain their Vanilla-specific ordering. Ore
    /// experience and similar non-item drops belong here rather than in the
    /// loot-table override.
    #[expect(
        unused_variables,
        reason = "default trait implementation ignores all params"
    )]
    fn spawn_after_break(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        tool: &ItemStack,
        drop_experience: bool,
    ) {
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

    /// Returns whether this state is a redstone signal source.
    fn is_signal_source(&self, _state: BlockStateId, _context: SignalQueryContext) -> bool {
        false
    }

    /// Returns whether this behavior is a vanilla diode block.
    ///
    /// Vanilla uses its `DiodeBlock` class hierarchy for side-input filtering.
    fn is_diode(&self) -> bool {
        false
    }

    /// Returns whether this behavior implements vanilla `TrapDoorBlock` semantics.
    ///
    /// Redstone wire uses this class-hierarchy check when deciding whether it
    /// can climb onto a neighboring block.
    fn is_trapdoor(&self) -> bool {
        false
    }

    /// Returns whether this behavior implements vanilla `BaseRailBlock` semantics.
    fn is_rail(&self) -> bool {
        self.as_rail().is_some()
    }

    /// Returns whether this behavior implements vanilla `PistonBaseBlock` semantics.
    fn is_piston_base(&self) -> bool {
        false
    }

    /// Returns this state's direction-independent redstone signal strength.
    fn get_own_signal(
        &self,
        _state: BlockStateId,
        _world: &dyn LevelReader,
        _pos: BlockPos,
        _context: SignalQueryContext,
    ) -> i32 {
        0
    }

    /// Returns the weak redstone signal emitted toward `direction`.
    fn get_signal(
        &self,
        state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
        _direction: Direction,
        context: SignalQueryContext,
    ) -> i32 {
        self.get_own_signal(state, world, pos, context)
    }

    /// Returns the direct redstone signal emitted toward `direction`.
    fn get_direct_signal(
        &self,
        _state: BlockStateId,
        _world: &dyn LevelReader,
        _pos: BlockPos,
        _direction: Direction,
        _context: SignalQueryContext,
    ) -> i32 {
        0
    }

    /// Returns whether this state conducts direct redstone power through itself.
    ///
    /// Most blocks use extracted state data. Dynamic blocks can override this with
    /// a live level/position query, matching vanilla's state predicate surface.
    fn is_redstone_conductor(
        &self,
        state: BlockStateId,
        _world: &dyn LevelReader,
        _pos: BlockPos,
    ) -> bool {
        state.is_static_redstone_conductor()
    }

    /// Handles a queued server block event.
    ///
    /// Mirrors Vanilla `BlockBehaviour.triggerEvent`. Returning `true` publishes
    /// the corresponding event packet to nearby clients.
    #[expect(
        unused_variables,
        reason = "default trait implementation ignores all params"
    )]
    fn trigger_event(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        param_a: i32,
        param_b: i32,
    ) -> bool {
        false
    }

    /// Returns the item stack to give when a player picks this block (middle click).
    ///
    /// The default implementation uses the block's registered item association.
    /// Blocks without an associated item return an empty stack. Override this when
    /// Vanilla selects the clone item from block state, block entity data, or another rule.
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
        Some(ItemStack::new(REGISTRY.items.by_block(block)))
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

    /// Returns whether this behavior implements `BedBlock`
    fn is_bed(&self) -> bool {
        false
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

    /// Resolves this block state's collision shape to owned block-local boxes.
    ///
    /// Vanilla dynamic-shape blocks may override this directly. Static blocks
    /// inherit the collision shape and positional offset hooks above.
    fn get_collision_boxes(
        &self,
        state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
        context: BlockCollisionContext,
    ) -> BlockCollisionBoxes {
        let shape = self.get_collision_shape(state, world, pos, context);
        if shape.is_empty() {
            return BlockCollisionBoxes::new();
        }

        let offset = self.get_collision_shape_offset(state, world, pos, context);
        shape
            .into_iter()
            .map(|aabb| aabb.translate(offset))
            .collect()
    }

    /// Resolves vanilla `BlockState.getBlockSupportShape` to owned block-local boxes.
    ///
    /// Most states use extracted support shapes. Dynamic blocks can override this
    /// hook to consult live world data, as vanilla does when its state cache is
    /// disabled by `dynamicShape()`.
    #[expect(
        unused_variables,
        reason = "the default support shape is extracted and independent of world data"
    )]
    fn get_block_support_boxes(
        &self,
        state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
    ) -> BlockCollisionBoxes {
        let shape = state.get_static_support_shape();
        if shape.is_empty() {
            return BlockCollisionBoxes::new();
        }

        let offset = if state
            .get_block()
            .shape_offsets
            .uses_offset(ShapeChannel::Support)
        {
            state.get_offset(pos)
        } else {
            DVec3::ZERO
        };
        shape
            .into_iter()
            .map(|aabb| aabb.translate(offset))
            .collect()
    }

    /// Mirrors vanilla `BlockState.isFaceSturdy(level, pos, direction, supportType)`.
    ///
    /// Static states retain their registry fast path. Dynamic states evaluate
    /// the live support boxes so block entities and other world-dependent
    /// shapes remain observable to attachment logic.
    fn is_face_sturdy(
        &self,
        state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
        direction: Direction,
        support_type: SupportType,
    ) -> bool {
        if !state.get_block().config.dynamic_shape {
            return state.is_face_sturdy_for_at(pos, direction, support_type);
        }

        is_block_local_face_sturdy(
            &self.get_block_support_boxes(state, world, pos),
            direction,
            support_type,
        )
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
    /// This is only called when the block state's extracted metadata marks it as randomly ticking.
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

    /// Called when a projectile hits this block.
    ///
    /// Vanilla parity: `BlockState.onProjectileHit(Level, BlockState,
    /// BlockHitResult, Projectile)`.
    fn on_projectile_hit(
        &self,
        _state: BlockStateId,
        _world: &Arc<World>,
        _hit: &ClipHitResult,
        _projectile: &dyn Projectile,
    ) {
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

    /// Creates a new block entity for this block.
    ///
    /// Structural block-entity presence comes from the extracted block-entity type registry.
    /// The result distinguishes a missing Steel implementation from a Vanilla factory that
    /// intentionally returns no entity.
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
    ) -> BlockEntityCreation {
        BlockEntityCreation::Unimplemented
    }

    /// Returns the server ticker selected by this live block state and entity type.
    ///
    /// Mirrors Vanilla `EntityBlock.getTicker`. Selection runs without chunk,
    /// section, or block-entity storage locks.
    #[expect(
        unused_variables,
        reason = "default trait implementation has no block-entity ticker"
    )]
    fn get_block_entity_ticker(
        &self,
        world: &Arc<World>,
        state: BlockStateId,
        block_entity_type: BlockEntityTypeRef,
    ) -> Option<BlockEntityTicker> {
        None
    }

    /// Returns the game-event listener exposed by this block entity.
    ///
    /// Mirrors Vanilla `EntityBlock.getListener`. Block implementations may override the
    /// provider result; the default delegates to the block entity's listener capability.
    #[expect(
        unused_variables,
        reason = "default trait implementation only delegates to the block entity"
    )]
    fn get_game_event_listener(
        &self,
        world: &Arc<World>,
        block_entity: &dyn BlockEntity,
    ) -> Option<SharedGameEventListener> {
        block_entity.game_event_listener()
    }

    /// Returns whether this new block should keep the old state's block entity.
    ///
    /// Vanilla defaults to `false`; copper chests and copper golem statues explicitly
    /// keep their entity across transformations within their respective block family.
    /// Steel checks those two extracted tags here until those block classes have their
    /// own complete behaviors, rather than registering partial block implementations.
    /// Non-Vanilla behaviors must opt in explicitly even if a plugin extends either tag.
    ///
    /// # Arguments
    /// * `old_state` - The previous block state
    /// * `new_state` - The requested replacement state
    fn should_keep_block_entity(&self, old_state: BlockStateId, new_state: BlockStateId) -> bool {
        let old_block = old_state.get_block();
        let new_block = new_state.get_block();
        new_block.key.namespace == Identifier::VANILLA_NAMESPACE
            && ((old_block.has_tag(&BlockTag::COPPER_CHESTS)
                && new_block.has_tag(&BlockTag::COPPER_CHESTS))
                || (old_block.has_tag(&BlockTag::COPPER_GOLEM_STATUES)
                    && new_block.has_tag(&BlockTag::COPPER_GOLEM_STATUES)))
    }

    /// Returns brushable-block data for archaeology brushing
    ///
    /// Vanilla keeps this on `BrushableBlock`; exposing it through block behavior lets
    /// `BrushItem` stay generic without matching concrete vanilla blocks
    fn brushable_data(&self, _state: BlockStateId) -> Option<BrushableData> {
        None
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
    /// * `direction` - The face from which the comparator reads the block
    #[expect(
        unused_variables,
        reason = "default trait implementation ignores all params"
    )]
    fn get_analog_output_signal(
        &self,
        state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
        direction: Direction,
    ) -> i32 {
        0
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

    /// Returns the shared vanilla `GrowingPlantHeadBlock` capability.
    fn as_growing_plant_head(&self) -> Option<&dyn GrowingPlantHeadBehavior> {
        None
    }

    /// Returns the shared vanilla `Fallable` capability implemented by this block.
    fn as_fallable(&self) -> Option<&dyn Fallable> {
        None
    }

    /// Returns the shared vanilla rail capability implemented by this block.
    fn as_rail(&self) -> Option<&dyn RailBehavior> {
        None
    }
}

mod registry;

pub use registry::{BlockBehaviorRegistry, DefaultBlockBehavior};

#[cfg(test)]
mod tests;
