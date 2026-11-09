use crate::block_entity::BlockEntity;
use steel_math::lerp;

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
    fn get_fall_damage_source(&self, entity: &Arc<FallingBlockEntity>) -> DamageSource {
        DamageSource::direct(
            &vanilla_damage_types::FALLING_BLOCK,
            Arc::<FallingBlockEntity>::clone(entity),
        )
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
    explosion_radius: Option<f32>,
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
            explosion_radius: None,
            luck: 0.0,
        }
    }

    /// Adds the entity responsible for destroying the block.
    #[must_use]
    pub const fn with_entity(mut self, entity: Option<&'a dyn Entity>) -> Self {
        self.entity = entity;
        self
    }

    /// Adds the block entity at the broken position.
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

    /// Adds the explosion radius used by explosion-decay loot functions.
    #[must_use]
    pub const fn with_explosion(mut self, radius: f32) -> Self {
        self.explosion_radius = Some(radius);
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

    pub(crate) const fn explosion_radius(&self) -> Option<f32> {
        self.explosion_radius
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

    /// Non-placement collision context anchored at a fixed Y position, with
    /// no descent or fall distance.
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

    /// Returns whether the entity is above the given shape at the position.
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
    /// Entity velocity before the block landing hook adjusts it (`currentMovement` in vanilla).
    pub velocity: DVec3,
    /// Vertical movement actually achieved this tick after collision clipping
    pub achieved_movement_y: f64,
    /// Whether the entity uses vanilla living-entity bounce behavior.
    pub is_living_entity: bool,
    /// Whether vanilla bounce behavior should be suppressed.
    pub suppresses_bounce: bool,
    /// The gravity applied to the entity.
    pub gravity: f64,
    /// The air drag of the entity.
    pub air_drag: f64,
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
    pub const fn new(
        velocity: DVec3,
        achieved_movement_y: f64,
        is_living_entity: bool,
        suppresses_bounce: bool,
        gravity: f64,
        air_drag: f64,
    ) -> Self {
        Self {
            velocity,
            achieved_movement_y,
            is_living_entity,
            suppresses_bounce,
            gravity,
            air_drag,
        }
    }

    /// Vanilla default `Block.updateEntityMovementAfterFallOn` result.
    #[must_use]
    pub const fn default_velocity_after_fall_on(self) -> DVec3 {
        DVec3::new(self.velocity.x, 0.0, self.velocity.z)
    }

    /// Computes the Y velocity after bouncing on a block.
    /// doesn't test if the bounce is valid
    #[must_use]
    pub fn velocity_after_bounce(&self, restitution: f32) -> DVec3 {
        let portion_with_movement = if self.velocity.y == 0.0 {
            1.0
        } else {
            self.achieved_movement_y / self.velocity.y
        };

        let gravity_compensation = portion_with_movement * self.gravity;
        let effective_drag = lerp(portion_with_movement, 1.0, self.air_drag);

        let entity_factor: f32 = if self.is_living_entity { 1.0 } else { 0.8 };
        let restitution = f64::from(restitution * entity_factor);

        DVec3::new(
            self.velocity.x,
            (gravity_compensation - self.velocity.y) * effective_drag * restitution,
            self.velocity.z,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{DVec3, EntityLandingContext};

    fn landing(velocity_y: f64, achieved_movement_y: f64) -> EntityLandingContext {
        EntityLandingContext::new(
            DVec3::new(0.25, velocity_y, -0.5),
            achieved_movement_y,
            true,
            false,
            0.08,
            0.98,
        )
    }

    #[test]
    fn bounce_uses_fraction_of_requested_movement_actually_achieved() {
        // portionWithMovement = -0.75 / -1.5 = 0.5
        // gravityCompensation = 0.5 * 0.08 = 0.04
        // effectiveDrag = lerp(0.5, 1.0, 0.98) = 0.99
        // result.y = (0.04 - (-1.5)) * 0.99 * 1.0 = 1.5246
        let context = landing(-1.5, -0.75);
        let result = context.velocity_after_bounce(1.0);
        assert!((result.y - 1.5246).abs() < 1.0e-6, "got {}", result.y);
    }

    #[test]
    fn non_living_entities_bounce_lower() {
        let living = landing(-1.5, -0.75).velocity_after_bounce(1.0);
        let non_living = EntityLandingContext::new(
            DVec3::new(0.25, -1.5, -0.5),
            -0.75,
            false,
            false,
            0.08,
            0.98,
        )
        .velocity_after_bounce(1.0);

        assert!(
            (non_living.y - living.y * 0.8).abs() < 1.0e-6,
            "non-living {} vs living {}",
            non_living.y,
            living.y
        );
    }
}
