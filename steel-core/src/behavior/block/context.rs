use crate::block_entity::BlockEntity;

use super::{
    Arc, Axis, BlockLocalAabb, BlockPos, BlockStateId, DVec3, DamageSource, Entity, EntityTypeRef,
    ItemStack, SharedBlockEntity, SmallVec, SoundEventRef, VoxelShape, World, vanilla_damage_types,
    vanilla_entities,
};
use crate::entity::entities::FallingBlockEntity;

pub struct PickupResult {
    pub filled_bucket: ItemStack,
    pub sound: Option<SoundEventRef>,
}

/// Result of invoking a block's Vanilla block-entity factory.
///
/// `NoEntity` is distinct from `Unimplemented`: some Vanilla blocks, notably the moving-piston
/// placeholder, intentionally return no entity from normal block creation even though their
/// state accepts an explicitly-created block entity.
pub enum BlockEntityCreation {
    /// The block factory created its entity.
    Created(SharedBlockEntity),
    /// The implemented Vanilla factory intentionally created no entity.
    NoEntity,
    /// Steel has not implemented this block's factory yet.
    Unimplemented,
}

impl BlockEntityCreation {
    /// Converts an optional registered implementation into a factory result.
    #[must_use]
    pub fn from_registered_factory(entity: Option<SharedBlockEntity>) -> Self {
        entity.map_or(Self::Unimplemented, Self::Created)
    }

    /// Returns the created entity, if the factory produced one.
    #[must_use]
    pub fn into_created(self) -> Option<SharedBlockEntity> {
        match self {
            Self::Created(entity) => Some(entity),
            Self::NoEntity | Self::Unimplemented => None,
        }
    }
}

/// Shared behavior exposed by blocks in vanilla's `BaseRailBlock` hierarchy.
///
/// Rail topology uses this capability in addition to the `minecraft:rails` tag.
/// This keeps class-hierarchy checks extensible without relying on concrete
/// downcasts.
pub trait RailBehavior: Send + Sync {
    /// Returns whether this rail forbids curved shapes.
    fn is_straight(&self) -> bool;
}

/// Shared behavior exposed by blocks implementing vanilla's `Fallable` interface.
///
/// Falling entities use this capability for landing, failed-placement, and
/// damage-source callbacks without depending on one concrete Rust block type.
pub trait Fallable: Send + Sync {
    /// Called after a falling entity successfully places its carried state.
    fn on_land(
        &self,
        _world: &Arc<World>,
        _pos: BlockPos,
        _state: BlockStateId,
        _replaced_state: BlockStateId,
        _entity: &FallingBlockEntity,
    ) {
    }

    /// Called when a falling entity breaks instead of placing its carried state.
    fn on_broken_after_fall(
        &self,
        _world: &Arc<World>,
        _pos: BlockPos,
        _entity: &FallingBlockEntity,
    ) {
    }

    /// Returns the damage source used when this falling block hurts entities.
    fn get_fall_damage_source(&self, entity: &FallingBlockEntity) -> DamageSource {
        DamageSource::environment(&vanilla_damage_types::FALLING_BLOCK)
            .with_direct_entity(entity.id())
            .with_causing_entity(entity.id())
    }

    /// Returns whether this behavior is in vanilla's `ConcretePowderBlock` hierarchy.
    fn is_concrete_powder(&self) -> bool {
        false
    }
}

/// Resolved block-local collision boxes for a live block state.
///
/// Most blocks materialize their extracted static voxel shape here. Dynamic
/// blocks such as moving pistons can instead return boxes computed from live
/// world data without forcing runtime shapes into the static registry.
pub type BlockCollisionBoxes = SmallVec<[BlockLocalAabb; 4]>;

/// Live parameters used to resolve a block's loot.
///
/// This is the Steel counterpart to vanilla's block `LootParams`. Behaviors can
/// override loot generation while retaining the original tool, entity, luck,
/// and position when delegating to another block state.
pub struct BlockLootContext<'a> {
    world: &'a Arc<World>,
    pos: BlockPos,
    entity: Option<&'a dyn Entity>,
    block_entity: Option<&'a dyn BlockEntity>,
    tool: Option<&'a ItemStack>,
    luck: f32,
}

impl<'a> BlockLootContext<'a> {
    /// Creates a no-tool block loot context.
    #[must_use]
    pub const fn new(world: &'a Arc<World>, pos: BlockPos) -> Self {
        Self {
            world,
            pos,
            entity: None,
            block_entity: None,
            tool: None,
            luck: 0.0,
        }
    }

    /// Adds the entity responsible for destroying the block.
    #[must_use]
    pub const fn with_entity(mut self, entity: Option<&'a dyn Entity>) -> Self {
        self.entity = entity;
        self
    }

    /// Adds the entity responsible for destroying the block.
    #[must_use]
    pub const fn with_block_entity(mut self, block_entity: Option<&'a dyn BlockEntity>) -> Self {
        self.block_entity = block_entity;
        self
    }

    /// Adds the tool used to destroy the block.
    #[must_use]
    pub const fn with_tool(mut self, tool: &'a ItemStack) -> Self {
        self.tool = Some(tool);
        self
    }

    /// Adds the luck used to evaluate the loot table.
    #[must_use]
    pub const fn with_luck(mut self, luck: f32) -> Self {
        self.luck = luck;
        self
    }

    /// Returns the world containing the block.
    #[must_use]
    pub const fn world(&self) -> &'a Arc<World> {
        self.world
    }

    /// Returns the block position whose loot is being resolved.
    #[must_use]
    pub const fn pos(&self) -> BlockPos {
        self.pos
    }

    /// Resolves loot for another state with the same vanilla loot parameters.
    #[must_use]
    pub fn get_drops(&self, state: BlockStateId) -> Vec<ItemStack> {
        World::block_drops(state, self)
    }

    pub(crate) const fn entity(&self) -> Option<&'a dyn Entity> {
        self.entity
    }

    pub(crate) const fn block_entity(&self) -> Option<&'a dyn BlockEntity> {
        self.block_entity
    }

    pub(crate) const fn tool(&self) -> Option<&'a ItemStack> {
        self.tool
    }

    pub(crate) const fn luck(&self) -> f32 {
        self.luck
    }
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

    /// Collision context for vanilla `CollisionContext.withPosition(entity, position)`.
    ///
    /// In Steel's reduced representation this also matches
    /// `CollisionContext.placementContext`.
    #[must_use]
    pub const fn with_position(entity_bottom: f64, descending: bool) -> Self {
        Self {
            entity_bottom: Some(entity_bottom),
            fall_distance: 0.0,
            can_walk_on_powder_snow: false,
            is_falling_block: false,
            descending,
            placement: true,
        }
    }

    /// Placement obstruction context when no entity initiated the placement.
    ///
    /// This matches vanilla `CollisionContext.placementContext(null)`.
    #[must_use]
    pub const fn placement_without_entity() -> Self {
        Self::with_position(f64::MIN, false)
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
