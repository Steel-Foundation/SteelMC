use std::sync::Arc;

use rand::RngExt;
use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, SpeleothemThickness};
use steel_registry::blocks::shapes::{BooleanOp, VoxelShape, join_is_not_empty};
use steel_registry::{
    fluid::FluidRef, level_events, vanilla_block_tags::BlockTag, vanilla_blocks,
    vanilla_damage_types, vanilla_entities, vanilla_fluids, vanilla_game_events,
};
use steel_utils::{BlockLocalAabb, BlockPos, BlockStateId, Direction, types::UpdateFlags};

use crate::behavior::BLOCK_BEHAVIORS;
use crate::behavior::block::{
    BlockBehavior, BlockCollisionContext, EntityFallDamage, EntityFallOnContext, push_entities_up,
};
use crate::behavior::context::BlockPlaceContext;
use crate::entity::damage::DamageSource;
use crate::entity::projectile::Projectile;
use crate::fluid::FluidStateExt as _;
use crate::world::game_event::GameEventContext;
use crate::world::{ClipHitResult, World};
use crate::world::{LevelReader, ScheduledTickAccess};

use super::BlockRef;

/// Vanilla `PointedDripstoneBlock` survival and thickness updates.
///
/// Survival mirrors vanilla's `isValidPointedDripstonePlacement`: the block
/// opposite the tip direction must be face-sturdy on the face pointing toward
/// us, or be another pointed dripstone with the same `vertical_direction`.
// TODO: Implement falling stalactites after falling block entities exist.
#[block_behavior]
pub struct PointedDripstoneBlock {
    block: BlockRef,
}

impl PointedDripstoneBlock {
    /// Creates a new pointed dripstone block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    fn fall_damage_for_state(state: BlockStateId, fall_distance: f64) -> Option<EntityFallDamage> {
        if state.get_value(&BlockStateProperties::VERTICAL_DIRECTION) != Direction::Up
            || state.get_value(&BlockStateProperties::SPELEOTHEM_THICKNESS)
                != SpeleothemThickness::Tip
        {
            return None;
        }

        Some(EntityFallDamage::new(
            fall_distance + 2.5,
            2.0,
            DamageSource::environment(&vanilla_damage_types::STALAGMITE),
        ))
    }

    const fn speleothem(&self) -> SpeleothemBlockBehavior {
        SpeleothemBlockBehavior {
            block: self.block,
            kind: SpeleothemKind::PointedDripstone,
        }
    }
}

impl BlockBehavior for PointedDripstoneBlock {
    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        self.speleothem().can_survive(state, world, pos)
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        self.speleothem().state_for_placement(context)
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: Direction,
        neighbor_pos: BlockPos,
        neighbor_state: BlockStateId,
    ) -> BlockStateId {
        self.speleothem()
            .update_shape(state, world, pos, direction, neighbor_pos, neighbor_state)
    }

    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        self.speleothem().tick(state, world, pos);
    }

    fn random_tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        self.speleothem().random_tick(state, world, pos);
    }

    fn fall_on(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        context: EntityFallOnContext<'_>,
    ) -> Option<EntityFallDamage> {
        Self::fall_damage_for_state(state, context.fall_distance)
            .or_else(|| self.default_fall_on(state, world, pos, context))
    }

    fn on_projectile_hit(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        hit: &ClipHitResult,
        projectile: &dyn Projectile,
    ) {
        SpeleothemBlockBehavior::on_projectile_hit(world, hit.block_pos, projectile);
    }
}

/// Vanilla `SulfurSpikeBlock` behavior
#[block_behavior]
pub struct SulfurSpikeBlock {
    block: BlockRef,
}

impl SulfurSpikeBlock {
    /// Creates a new sulfur spike block
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    const fn speleothem(&self) -> SpeleothemBlockBehavior {
        SpeleothemBlockBehavior {
            block: self.block,
            kind: SpeleothemKind::Sulfur,
        }
    }
}

impl BlockBehavior for SulfurSpikeBlock {
    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        self.speleothem().can_survive(state, world, pos)
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        self.speleothem().state_for_placement(context)
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: Direction,
        neighbor_pos: BlockPos,
        neighbor_state: BlockStateId,
    ) -> BlockStateId {
        self.speleothem()
            .update_shape(state, world, pos, direction, neighbor_pos, neighbor_state)
    }

    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        self.speleothem().tick(state, world, pos);
    }

    fn random_tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        self.speleothem().random_tick(state, world, pos);
    }

    fn on_projectile_hit(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        hit: &ClipHitResult,
        projectile: &dyn Projectile,
    ) {
        SpeleothemBlockBehavior::on_projectile_hit(world, hit.block_pos, projectile);
    }
}

struct SpeleothemBlockBehavior {
    block: BlockRef,
    kind: SpeleothemKind,
}

#[derive(Clone, Copy)]
enum SpeleothemKind {
    PointedDripstone,
    Sulfur,
}

const GROWTH_PROBABILITY_PER_RANDOM_TICK: f32 = 0.011_377_778;
const MAX_GROWTH_LENGTH: i32 = 7;
const MAX_STALAGMITE_SEARCH_RANGE_WHEN_GROWING: i32 = 10;
const WATER_TRANSFER_PROBABILITY_PER_RANDOM_TICK: f32 = 45.0 / 256.0;
const LAVA_TRANSFER_PROBABILITY_PER_RANDOM_TICK: f32 = 15.0 / 256.0;
const MAX_SEARCH_LENGTH_WHEN_CHECKING_DRIP_TYPE: i32 = 11;
const MAX_SEARCH_LENGTH_BETWEEN_STALACTITE_TIP_AND_CAULDRON: i32 = 11;
const DRIP_THROUGH_COLUMN_BOXES: &[BlockLocalAabb] =
    &[BlockLocalAabb::new(0.375, 0.0, 0.375, 0.625, 1.0, 0.625)];
const REQUIRED_SPACE_TO_DRIP_THROUGH_NON_SOLID_BLOCK: VoxelShape =
    VoxelShape::from_boxes(DRIP_THROUGH_COLUMN_BOXES);

struct FluidInfo {
    pos: BlockPos,
    fluid: FluidRef,
    source_state: BlockStateId,
}

impl SpeleothemBlockBehavior {
    fn projectile_can_break(projectile: &dyn Projectile, world: &World, pos: BlockPos) -> bool {
        projectile.projectile_may_interact(world, pos)
            && projectile.may_break(world)
            && projectile.entity_type() == &vanilla_entities::TRIDENT
            && projectile.velocity().length() > 0.6
    }

    fn on_projectile_hit(world: &Arc<World>, pos: BlockPos, projectile: &dyn Projectile) {
        if Self::projectile_can_break(projectile, world, pos) {
            world.destroy_block(pos, true);
        }
    }

    fn state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let default_tip_direction = context.get_nearest_looking_vertical_direction().opposite();
        let tip_direction = self.calculate_tip_direction(
            context.world.as_ref(),
            context.place_pos(),
            default_tip_direction,
        )?;
        let merge_opposing_tips = !context.is_secondary_use_active();
        let thickness = self.calculate_thickness(
            context.world.as_ref(),
            context.place_pos(),
            tip_direction,
            merge_opposing_tips,
        );
        let state = self
            .block
            .default_state()
            .set_value(&BlockStateProperties::VERTICAL_DIRECTION, tip_direction)
            .set_value(
                &BlockStateProperties::WATERLOGGED,
                context.is_water_source(),
            );

        Some(Self::with_thickness(state, thickness))
    }

    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        let tip_direction = state.get_value(&BlockStateProperties::VERTICAL_DIRECTION);
        let behind_pos = pos.relative(tip_direction.opposite());
        let behind_state = world.get_block_state(behind_pos);

        world.is_face_sturdy(behind_state, behind_pos, tip_direction)
            || (Self::is_speleothem_with_direction(behind_state, tip_direction)
                && behind_state.get_block() == self.block)
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        if state.get_value(&BlockStateProperties::WATERLOGGED) {
            let delay = world.fluid_tick_delay(&vanilla_fluids::WATER);
            let _ = world.schedule_fluid_tick_default(pos, &vanilla_fluids::WATER, delay);
        }

        if direction != Direction::Up && direction != Direction::Down {
            return state;
        }

        let tip_direction = state.get_value(&BlockStateProperties::VERTICAL_DIRECTION);
        if tip_direction == Direction::Down && world.has_scheduled_block_tick(pos, self.block) {
            return state;
        }

        if direction == tip_direction.opposite() && !self.can_survive(state, world, pos) {
            let delay = if tip_direction == Direction::Down {
                2
            } else {
                1
            };
            let _ = world.schedule_block_tick_default(pos, self.block, delay);
            return state;
        }

        let merge_opposing_tips = Self::thickness(state) == SpeleothemThickness::TipMerge;
        let thickness = self.calculate_thickness(world, pos, tip_direction, merge_opposing_tips);
        Self::with_thickness(state, thickness)
    }

    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        if Self::is_stalagmite(state) && !self.can_survive(state, world.as_ref(), pos) {
            world.destroy_block(pos, true);
        }
    }

    fn calculate_tip_direction(
        &self,
        world: &dyn LevelReader,
        pos: BlockPos,
        default_tip_direction: Direction,
    ) -> Option<Direction> {
        let default_state = self.block.default_state().set_value(
            &BlockStateProperties::VERTICAL_DIRECTION,
            default_tip_direction,
        );
        if self.can_survive(default_state, world, pos) {
            return Some(default_tip_direction);
        }

        let opposite_tip_direction = default_tip_direction.opposite();
        let opposite_state = self.block.default_state().set_value(
            &BlockStateProperties::VERTICAL_DIRECTION,
            opposite_tip_direction,
        );
        self.can_survive(opposite_state, world, pos)
            .then_some(opposite_tip_direction)
    }

    fn calculate_thickness(
        &self,
        world: &dyn LevelReader,
        pos: BlockPos,
        tip_direction: Direction,
        merge_opposing_tips: bool,
    ) -> SpeleothemThickness {
        let base_direction = tip_direction.opposite();
        let in_front_state = world.get_block_state(pos.relative(tip_direction));
        if Self::is_speleothem_with_direction(in_front_state, base_direction)
            && in_front_state.get_block() == self.block
        {
            if merge_opposing_tips
                || Self::thickness(in_front_state) == SpeleothemThickness::TipMerge
            {
                return SpeleothemThickness::TipMerge;
            }
            return SpeleothemThickness::Tip;
        }

        if !Self::is_speleothem_with_direction(in_front_state, tip_direction) {
            return SpeleothemThickness::Tip;
        }

        let in_front_thickness = Self::thickness(in_front_state);
        if matches!(
            in_front_thickness,
            SpeleothemThickness::Tip | SpeleothemThickness::TipMerge
        ) {
            return SpeleothemThickness::Frustum;
        }

        let behind_state = world.get_block_state(pos.relative(base_direction));
        if !Self::is_speleothem_with_direction(behind_state, tip_direction) {
            return SpeleothemThickness::Base;
        }
        SpeleothemThickness::Middle
    }

    fn is_speleothem_with_direction(state: BlockStateId, tip_direction: Direction) -> bool {
        state.get_block().has_tag(&BlockTag::SPELEOTHEMS)
            && state.get_value(&BlockStateProperties::VERTICAL_DIRECTION) == tip_direction
    }

    fn is_stalagmite(state: BlockStateId) -> bool {
        Self::is_speleothem_with_direction(state, Direction::Up)
    }

    fn is_stalactite(state: BlockStateId) -> bool {
        Self::is_speleothem_with_direction(state, Direction::Down)
    }

    fn is_stalactite_start_pos(
        &self,
        state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
    ) -> bool {
        Self::is_stalactite(state) && world.get_block_state(pos.above()).get_block() != self.block
    }

    fn random_tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        let mut rng = rand::rng();
        if matches!(self.kind, SpeleothemKind::PointedDripstone) {
            let random_value = rng.random::<f32>();
            if (random_value <= WATER_TRANSFER_PROBABILITY_PER_RANDOM_TICK
                || random_value <= LAVA_TRANSFER_PROBABILITY_PER_RANDOM_TICK)
                && self.is_stalactite_start_pos(state, world.as_ref(), pos)
            {
                self.maybe_transfer_fluid(state, world, pos, random_value);
            }
        }

        if rng.random::<f32>() < GROWTH_PROBABILITY_PER_RANDOM_TICK
            && self.is_stalactite_start_pos(state, world.as_ref(), pos)
        {
            self.grow_stalactite_or_stalagmite_if_possible(state, world, pos, &mut rng);
        }
    }

    fn grow_stalactite_or_stalagmite_if_possible<R: RngExt + ?Sized>(
        &self,
        stalactite_start_state: BlockStateId,
        world: &Arc<World>,
        stalactite_start_pos: BlockPos,
        rng: &mut R,
    ) {
        if !self.can_grow(world.as_ref(), stalactite_start_pos) {
            return;
        }

        let Some(stalactite_tip_pos) = self.find_tip(
            stalactite_start_state,
            world.as_ref(),
            stalactite_start_pos,
            MAX_GROWTH_LENGTH,
            false,
        ) else {
            return;
        };

        let stalactite_tip_state = world.get_block_state(stalactite_tip_pos);
        if !Self::is_free_hanging_stalactite(stalactite_tip_state)
            || !self.can_tip_grow(stalactite_tip_state, world, stalactite_tip_pos)
        {
            return;
        }

        if rng.random::<bool>() {
            self.grow(world, stalactite_tip_pos, Direction::Down);
        } else {
            self.grow_stalagmite_below(world, stalactite_tip_pos);
        }
    }

    fn can_grow(&self, world: &dyn LevelReader, pos: BlockPos) -> bool {
        if world.get_block_state(pos.above()).get_block() != self.block_to_grow_on() {
            return false;
        }

        if !matches!(self.kind, SpeleothemKind::PointedDripstone) {
            return true;
        }

        let fluid_state = world.get_block_state(pos.above_n(2)).get_fluid_state();
        fluid_state.is_water() && fluid_state.is_source()
    }

    fn block_to_grow_on(&self) -> BlockRef {
        match self.kind {
            SpeleothemKind::PointedDripstone => &vanilla_blocks::DRIPSTONE_BLOCK,
            SpeleothemKind::Sulfur => &vanilla_blocks::SULFUR,
        }
    }

    fn find_tip(
        &self,
        speleothem_state: BlockStateId,
        world: &dyn LevelReader,
        speleothem_pos: BlockPos,
        max_search_length: i32,
        include_merged_tip: bool,
    ) -> Option<BlockPos> {
        if Self::is_tip(speleothem_state, include_merged_tip) {
            return Some(speleothem_pos);
        }

        let search_direction =
            speleothem_state.get_value(&BlockStateProperties::VERTICAL_DIRECTION);
        let mut current_pos = speleothem_pos;
        for _ in 1..max_search_length {
            current_pos = current_pos.relative(search_direction);
            let state = world.get_block_state(current_pos);
            if Self::is_tip(state, include_merged_tip) {
                return Some(current_pos);
            }

            if world.is_outside_build_height(current_pos.y())
                || state.get_block() != self.block
                || state.get_value(&BlockStateProperties::VERTICAL_DIRECTION) != search_direction
            {
                return None;
            }
        }

        None
    }

    fn is_tip(state: BlockStateId, include_merged_tip: bool) -> bool {
        if !state.get_block().has_tag(&BlockTag::SPELEOTHEMS) {
            return false;
        }

        let thickness = Self::thickness(state);
        thickness == SpeleothemThickness::Tip
            || (include_merged_tip && thickness == SpeleothemThickness::TipMerge)
    }

    fn is_free_hanging_stalactite(state: BlockStateId) -> bool {
        Self::is_stalactite(state)
            && Self::thickness(state) == SpeleothemThickness::Tip
            && !state.get_value(&BlockStateProperties::WATERLOGGED)
    }

    fn can_tip_grow(&self, tip_state: BlockStateId, world: &Arc<World>, tip_pos: BlockPos) -> bool {
        let grow_direction = tip_state.get_value(&BlockStateProperties::VERTICAL_DIRECTION);
        let grow_pos = tip_pos.relative(grow_direction);
        let state_at_grow_pos = world.get_block_state(grow_pos);
        if state_at_grow_pos.has_fluid() {
            return false;
        }

        state_at_grow_pos.is_air()
            || self.is_unmerged_tip_with_direction(state_at_grow_pos, grow_direction.opposite())
    }

    fn is_unmerged_tip_with_direction(
        &self,
        state: BlockStateId,
        tip_direction: Direction,
    ) -> bool {
        Self::is_tip(state, false)
            && state.get_block() == self.block
            && state.get_value(&BlockStateProperties::VERTICAL_DIRECTION) == tip_direction
    }

    fn grow(&self, world: &Arc<World>, grow_from_pos: BlockPos, grow_to_direction: Direction) {
        let target_pos = grow_from_pos.relative(grow_to_direction);
        let existing_state_at_target_pos = world.get_block_state(target_pos);
        if self.is_unmerged_tip_with_direction(
            existing_state_at_target_pos,
            grow_to_direction.opposite(),
        ) {
            self.create_merged_tips(existing_state_at_target_pos, world, target_pos);
            return;
        }

        if existing_state_at_target_pos.is_air()
            || existing_state_at_target_pos.get_block() == &vanilla_blocks::WATER
        {
            self.create_speleothem(
                world,
                target_pos,
                grow_to_direction,
                SpeleothemThickness::Tip,
            );
        }
    }

    fn create_speleothem(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        direction: Direction,
        thickness: SpeleothemThickness,
    ) {
        let waterlogged = world.get_block_state(pos).get_fluid_state().is_water();
        let state = self
            .block
            .default_state()
            .set_value(&BlockStateProperties::VERTICAL_DIRECTION, direction)
            .set_value(&BlockStateProperties::SPELEOTHEM_THICKNESS, thickness)
            .set_value(&BlockStateProperties::WATERLOGGED, waterlogged);
        world.set_block(pos, state, UpdateFlags::UPDATE_ALL);
    }

    fn create_merged_tips(&self, tip_state: BlockStateId, world: &Arc<World>, tip_pos: BlockPos) {
        let (stalactite_pos, stalagmite_pos) =
            if tip_state.get_value(&BlockStateProperties::VERTICAL_DIRECTION) == Direction::Up {
                (tip_pos.above(), tip_pos)
            } else {
                (tip_pos, tip_pos.below())
            };

        self.create_speleothem(
            world,
            stalactite_pos,
            Direction::Down,
            SpeleothemThickness::TipMerge,
        );
        self.create_speleothem(
            world,
            stalagmite_pos,
            Direction::Up,
            SpeleothemThickness::TipMerge,
        );
    }

    fn grow_stalagmite_below(&self, world: &Arc<World>, pos_above_stalagmite: BlockPos) {
        let mut pos = pos_above_stalagmite;
        for _ in 0..MAX_STALAGMITE_SEARCH_RANGE_WHEN_GROWING {
            pos = pos.below();
            let state = world.get_block_state(pos);
            if state.has_fluid() {
                return;
            }

            if self.is_unmerged_tip_with_direction(state, Direction::Up)
                && self.can_tip_grow(state, world, pos)
            {
                self.grow(world, pos, Direction::Up);
                return;
            }

            let placement_state = self
                .block
                .default_state()
                .set_value(&BlockStateProperties::VERTICAL_DIRECTION, Direction::Up);
            if self.can_survive(placement_state, world.as_ref(), pos)
                && !Self::is_water_at(world, pos.below())
            {
                self.grow(world, pos.below(), Direction::Up);
                return;
            }

            if self.blocks_stalagmite_scan(world.as_ref(), pos, state) {
                return;
            }
        }
    }

    fn is_water_at(world: &Arc<World>, pos: BlockPos) -> bool {
        world.get_block_state(pos).get_fluid_state().is_water()
    }

    fn blocks_stalagmite_scan(
        &self,
        world: &dyn LevelReader,
        pos: BlockPos,
        state: BlockStateId,
    ) -> bool {
        match self.kind {
            SpeleothemKind::PointedDripstone => !Self::can_drip_through(world, pos, state),
            SpeleothemKind::Sulfur => false,
        }
    }

    fn can_drip_through(world: &dyn LevelReader, pos: BlockPos, state: BlockStateId) -> bool {
        if state.is_air() {
            return true;
        }

        if state.is_solid_render() || state.has_fluid() {
            return false;
        }

        let collision_shape = BLOCK_BEHAVIORS
            .get_behavior(state.get_block())
            .get_collision_shape(state, world, pos, BlockCollisionContext::empty());
        !join_is_not_empty(
            REQUIRED_SPACE_TO_DRIP_THROUGH_NON_SOLID_BLOCK,
            collision_shape,
            BooleanOp::And,
        )
    }

    fn maybe_transfer_fluid(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        random_value: f32,
    ) {
        let Some(fluid_info) = self.get_fluid_above_stalactite(world, pos, state) else {
            return;
        };

        let transfer_probability = if fluid_info.fluid == &vanilla_fluids::WATER {
            WATER_TRANSFER_PROBABILITY_PER_RANDOM_TICK
        } else if fluid_info.fluid == &vanilla_fluids::LAVA {
            LAVA_TRANSFER_PROBABILITY_PER_RANDOM_TICK
        } else {
            return;
        };

        if random_value >= transfer_probability {
            return;
        }

        let Some(tip_pos) = self.find_tip(
            state,
            world.as_ref(),
            pos,
            MAX_SEARCH_LENGTH_WHEN_CHECKING_DRIP_TYPE,
            false,
        ) else {
            return;
        };

        if fluid_info.source_state.get_block() == &vanilla_blocks::MUD
            && fluid_info.fluid == &vanilla_fluids::WATER
        {
            if !world.dimension_type.water_evaporates {
                let clay_state = vanilla_blocks::CLAY.default_state();
                let _ =
                    push_entities_up(fluid_info.source_state, clay_state, world, fluid_info.pos);
                world.set_block(fluid_info.pos, clay_state, UpdateFlags::UPDATE_ALL);
                world.game_event(
                    &vanilla_game_events::BLOCK_CHANGE,
                    fluid_info.pos,
                    &GameEventContext::new(None, Some(clay_state)),
                );
                world.level_event(level_events::DRIPSTONE_DRIP, tip_pos, 0, None);
            }
            return;
        }

        let Some(cauldron_pos) =
            Self::find_fillable_cauldron_below_stalactite_tip(world, tip_pos, fluid_info.fluid)
        else {
            return;
        };

        world.level_event(level_events::DRIPSTONE_DRIP, tip_pos, 0, None);
        let fall_distance = tip_pos.y() - cauldron_pos.y();
        let delay = 50 + fall_distance;
        let cauldron_state = world.get_block_state(cauldron_pos);
        let _ = world.schedule_block_tick_default(cauldron_pos, cauldron_state.get_block(), delay);
    }

    fn get_fluid_above_stalactite(
        &self,
        world: &Arc<World>,
        stalactite_pos: BlockPos,
        stalactite_state: BlockStateId,
    ) -> Option<FluidInfo> {
        if !Self::is_stalactite(stalactite_state) {
            return None;
        }

        let root_pos = self.find_root_block(
            world,
            stalactite_pos,
            stalactite_state,
            MAX_SEARCH_LENGTH_WHEN_CHECKING_DRIP_TYPE,
        )?;
        let above_pos = root_pos.above();
        let above_state = world.get_block_state(above_pos);

        let fluid = if above_state.get_block() == &vanilla_blocks::MUD
            && !world.dimension_type.water_evaporates
        {
            &vanilla_fluids::WATER
        } else {
            above_state.get_fluid_state().fluid_id
        };

        Some(FluidInfo {
            pos: above_pos,
            fluid,
            source_state: above_state,
        })
    }

    fn find_root_block(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        dripstone_state: BlockStateId,
        max_search_length: i32,
    ) -> Option<BlockPos> {
        let tip_direction = dripstone_state.get_value(&BlockStateProperties::VERTICAL_DIRECTION);
        let search_direction = tip_direction.opposite();
        let mut current_pos = pos;
        for _ in 1..max_search_length {
            current_pos = current_pos.relative(search_direction);
            let state = world.get_block_state(current_pos);
            if state.get_block() != self.block {
                return Some(current_pos);
            }
            if state.get_value(&BlockStateProperties::VERTICAL_DIRECTION) != tip_direction {
                return None;
            }
            if world.is_outside_build_height(current_pos.y()) {
                return None;
            }
        }
        None
    }

    fn find_fillable_cauldron_below_stalactite_tip(
        world: &Arc<World>,
        tip_pos: BlockPos,
        fluid: FluidRef,
    ) -> Option<BlockPos> {
        let mut current_pos = tip_pos;
        for _ in 1..MAX_SEARCH_LENGTH_BETWEEN_STALACTITE_TIP_AND_CAULDRON {
            current_pos = current_pos.below();
            let state = world.get_block_state(current_pos);

            if Self::is_fillable_cauldron(state, fluid) {
                return Some(current_pos);
            }

            if !Self::can_drip_through(world.as_ref(), current_pos, state) {
                return None;
            }

            if world.is_outside_build_height(current_pos.y()) {
                return None;
            }
        }
        None
    }

    fn is_fillable_cauldron(state: BlockStateId, fluid: FluidRef) -> bool {
        let block = state.get_block();
        if fluid == &vanilla_fluids::WATER {
            block == &vanilla_blocks::CAULDRON
                || (block == &vanilla_blocks::WATER_CAULDRON
                    && state.get_value(&BlockStateProperties::LEVEL_CAULDRON)
                        < BlockStateProperties::LEVEL_CAULDRON.max)
        } else if fluid == &vanilla_fluids::LAVA {
            block == &vanilla_blocks::CAULDRON
        } else {
            false
        }
    }

    fn thickness(state: BlockStateId) -> SpeleothemThickness {
        state.get_value(&BlockStateProperties::SPELEOTHEM_THICKNESS)
    }

    fn with_thickness(state: BlockStateId, thickness: SpeleothemThickness) -> BlockStateId {
        state.set_value(&BlockStateProperties::SPELEOTHEM_THICKNESS, thickness)
    }
}

/// Searches upward from cauldron position to find an overhanging stalactite tip.
#[must_use]
pub fn find_stalactite_tip_above_cauldron(
    world: &dyn LevelReader,
    cauldron_pos: BlockPos,
) -> Option<BlockPos> {
    let mut current_pos = cauldron_pos;
    for _ in 1..MAX_SEARCH_LENGTH_BETWEEN_STALACTITE_TIP_AND_CAULDRON {
        current_pos = current_pos.above();
        let state = world.get_block_state(current_pos);

        if SpeleothemBlockBehavior::is_free_hanging_stalactite(state) {
            return Some(current_pos);
        }

        if !SpeleothemBlockBehavior::can_drip_through(world, current_pos, state) {
            return None;
        }

        if world.is_outside_build_height(current_pos.y()) {
            return None;
        }
    }
    None
}

/// Determines which fluid (water or lava) a stalactite would fill a cauldron with.
#[must_use]
pub fn get_cauldron_fill_fluid_type(
    world: &Arc<World>,
    stalactite_pos: BlockPos,
) -> Option<FluidRef> {
    let state = world.get_block_state(stalactite_pos);
    let dripstone = SpeleothemBlockBehavior {
        block: &vanilla_blocks::POINTED_DRIPSTONE,
        kind: SpeleothemKind::PointedDripstone,
    };
    let fluid = dripstone
        .get_fluid_above_stalactite(world, stalactite_pos, state)?
        .fluid;

    if fluid == &vanilla_fluids::WATER || fluid == &vanilla_fluids::LAVA {
        Some(fluid)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Weak;

    use super::*;
    use glam::DVec3;
    use steel_utils::ChunkPos;

    use steel_registry::{
        entity_type::EntityTypeRef, test_support::init_test_registry, vanilla_blocks,
        vanilla_damage_types,
    };

    use crate::{
        behavior::init_behaviors,
        entity::{Entity, EntityBase, projectile::ProjectileBase},
        test_support::{fresh_test_world, insert_ready_full_chunk, test_world},
    };

    struct TestProjectile {
        base: EntityBase,
        projectile_base: ProjectileBase,
        entity_type: EntityTypeRef,
    }

    impl TestProjectile {
        fn new(entity_type: EntityTypeRef, velocity: DVec3) -> Self {
            let projectile = Self {
                base: EntityBase::new(1, DVec3::ZERO, entity_type.dimensions, Weak::new()),
                projectile_base: ProjectileBase::new(),
                entity_type,
            };
            projectile.set_velocity(velocity);
            projectile
        }
    }

    crate::entity::impl_test_downcast_type!(TestProjectile);

    impl Entity for TestProjectile {
        fn base(&self) -> &EntityBase {
            &self.base
        }

        fn entity_type(&self) -> EntityTypeRef {
            self.entity_type
        }
    }

    impl Projectile for TestProjectile {
        fn projectile_base(&self) -> &ProjectileBase {
            &self.projectile_base
        }
    }

    fn pointed_dripstone_state(
        direction: Direction,
        thickness: SpeleothemThickness,
    ) -> BlockStateId {
        init_test_registry();
        vanilla_blocks::POINTED_DRIPSTONE
            .default_state()
            .set_value(&BlockStateProperties::VERTICAL_DIRECTION, direction)
            .set_value(&BlockStateProperties::SPELEOTHEM_THICKNESS, thickness)
    }

    #[test]
    fn upward_tip_uses_stalagmite_fall_damage() {
        let state = pointed_dripstone_state(Direction::Up, SpeleothemThickness::Tip);
        let fall_damage = PointedDripstoneBlock::fall_damage_for_state(state, 4.0)
            .expect("upward tip should request stalagmite damage");

        assert!((fall_damage.fall_distance - 6.5).abs() < f64::EPSILON);
        assert!((fall_damage.damage_modifier - 2.0).abs() < f32::EPSILON);
        assert_eq!(
            &fall_damage.source.damage_type.key,
            &vanilla_damage_types::STALAGMITE.key,
        );
    }

    #[test]
    fn non_tip_uses_default_fall_damage() {
        let state = pointed_dripstone_state(Direction::Up, SpeleothemThickness::Frustum);

        assert!(PointedDripstoneBlock::fall_damage_for_state(state, 4.0).is_none());
    }

    #[test]
    fn downward_tip_uses_default_fall_damage() {
        let state = pointed_dripstone_state(Direction::Down, SpeleothemThickness::Tip);

        assert!(PointedDripstoneBlock::fall_damage_for_state(state, 4.0).is_none());
    }

    #[test]
    fn only_fast_tridents_break_speleothems() {
        init_test_registry();
        let pos = BlockPos::ZERO;
        let fast_trident = TestProjectile::new(&vanilla_entities::TRIDENT, DVec3::X * 0.61);
        let threshold_trident = TestProjectile::new(&vanilla_entities::TRIDENT, DVec3::X * 0.6);
        let firework = TestProjectile::new(&vanilla_entities::FIREWORK_ROCKET, DVec3::X);

        assert!(SpeleothemBlockBehavior::projectile_can_break(
            &fast_trident,
            test_world(),
            pos,
        ));
        assert!(!SpeleothemBlockBehavior::projectile_can_break(
            &threshold_trident,
            test_world(),
            pos,
        ));
        assert!(!SpeleothemBlockBehavior::projectile_can_break(
            &firework,
            test_world(),
            pos,
        ));
    }

    fn stalactite_setup(
        world: &Arc<World>,
        pos: BlockPos,
    ) -> (SpeleothemBlockBehavior, BlockStateId) {
        let dripstone = vanilla_blocks::POINTED_DRIPSTONE
            .default_state()
            .set_value(&BlockStateProperties::VERTICAL_DIRECTION, Direction::Down)
            .set_value(
                &BlockStateProperties::SPELEOTHEM_THICKNESS,
                SpeleothemThickness::Tip,
            );
        let root = vanilla_blocks::DRIPSTONE_BLOCK.default_state();
        assert!(world.set_block(pos, dripstone, UpdateFlags::UPDATE_NONE));
        assert!(world.set_block(pos.above(), root, UpdateFlags::UPDATE_NONE));
        let behavior = SpeleothemBlockBehavior {
            block: &vanilla_blocks::POINTED_DRIPSTONE,
            kind: SpeleothemKind::PointedDripstone,
        };
        (behavior, dripstone)
    }

    #[test]
    fn stalactite_drip_converts_mud_to_clay() {
        init_test_registry();
        init_behaviors();
        let world = fresh_test_world("mud_to_clay");
        let pos = BlockPos::new(8, 64, 8);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));

        let mud = vanilla_blocks::MUD.default_state();
        assert!(world.set_block(pos.above_n(2), mud, UpdateFlags::UPDATE_NONE));

        let (behavior, dripstone) = stalactite_setup(&world, pos);
        behavior.maybe_transfer_fluid(dripstone, &world, pos, 0.0);

        assert_eq!(
            world.get_block_state(pos.above_n(2)).get_block(),
            &vanilla_blocks::CLAY,
        );
    }

    #[test]
    fn stalactite_drip_fills_empty_cauldron_with_water() {
        init_test_registry();
        init_behaviors();
        let world = fresh_test_world("water_cauldron_fill");
        let pos = BlockPos::new(8, 64, 8);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));

        let source_water = vanilla_blocks::WATER.default_state();
        assert!(world.set_block(pos.above_n(2), source_water, UpdateFlags::UPDATE_NONE));
        assert!(world.set_block(
            pos.below(),
            vanilla_blocks::CAULDRON.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));

        let (behavior, dripstone) = stalactite_setup(&world, pos);
        behavior.maybe_transfer_fluid(dripstone, &world, pos, 0.0);

        world.level_data.write().set_game_time(51);
        world.chunk_map.tick_game(&world, 51, 0, true);

        let cauldron_state = world.get_block_state(pos.below());
        assert_eq!(cauldron_state.get_block(), &vanilla_blocks::WATER_CAULDRON);
        assert_eq!(
            cauldron_state.get_value(&BlockStateProperties::LEVEL_CAULDRON),
            1,
        );
    }

    #[test]
    fn stalactite_drip_increments_layered_water_cauldron() {
        init_test_registry();
        init_behaviors();
        let world = fresh_test_world("water_cauldron_inc");
        let pos = BlockPos::new(8, 64, 8);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));

        let source_water = vanilla_blocks::WATER.default_state();
        assert!(world.set_block(pos.above_n(2), source_water, UpdateFlags::UPDATE_NONE));
        let water_cauldron = vanilla_blocks::WATER_CAULDRON
            .default_state()
            .set_value(&BlockStateProperties::LEVEL_CAULDRON, 1);
        assert!(world.set_block(pos.below(), water_cauldron, UpdateFlags::UPDATE_NONE));

        let (behavior, dripstone) = stalactite_setup(&world, pos);
        behavior.maybe_transfer_fluid(dripstone, &world, pos, 0.0);

        world.level_data.write().set_game_time(51);
        world.chunk_map.tick_game(&world, 51, 0, true);

        let cauldron_state = world.get_block_state(pos.below());
        assert_eq!(cauldron_state.get_block(), &vanilla_blocks::WATER_CAULDRON);
        assert_eq!(
            cauldron_state.get_value(&BlockStateProperties::LEVEL_CAULDRON),
            2,
        );
    }

    #[test]
    fn stalactite_drip_does_not_overflow_full_water_cauldron() {
        init_test_registry();
        init_behaviors();
        let world = fresh_test_world("water_cauldron_full");
        let pos = BlockPos::new(8, 64, 8);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));

        let source_water = vanilla_blocks::WATER.default_state();
        assert!(world.set_block(pos.above_n(2), source_water, UpdateFlags::UPDATE_NONE));
        let full_cauldron = vanilla_blocks::WATER_CAULDRON
            .default_state()
            .set_value(&BlockStateProperties::LEVEL_CAULDRON, 3);
        assert!(world.set_block(pos.below(), full_cauldron, UpdateFlags::UPDATE_NONE));

        let (behavior, dripstone) = stalactite_setup(&world, pos);
        behavior.maybe_transfer_fluid(dripstone, &world, pos, 0.0);

        let cauldron_state = world.get_block_state(pos.below());
        assert_eq!(cauldron_state.get_block(), &vanilla_blocks::WATER_CAULDRON);
        assert_eq!(
            cauldron_state.get_value(&BlockStateProperties::LEVEL_CAULDRON),
            3,
        );
    }

    #[test]
    fn stalactite_drip_fills_empty_cauldron_with_lava() {
        init_test_registry();
        init_behaviors();
        let world = fresh_test_world("lava_cauldron_fill");
        let pos = BlockPos::new(8, 64, 8);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));

        let source_lava = vanilla_blocks::LAVA.default_state();
        assert!(world.set_block(pos.above_n(2), source_lava, UpdateFlags::UPDATE_NONE));
        assert!(world.set_block(
            pos.below(),
            vanilla_blocks::CAULDRON.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));

        let (behavior, dripstone) = stalactite_setup(&world, pos);
        behavior.maybe_transfer_fluid(dripstone, &world, pos, 0.0);

        world.level_data.write().set_game_time(51);
        world.chunk_map.tick_game(&world, 51, 0, true);

        assert_eq!(
            world.get_block_state(pos.below()).get_block(),
            &vanilla_blocks::LAVA_CAULDRON,
        );
    }

    #[test]
    fn stalactite_drip_does_not_fill_lava_cauldron() {
        init_test_registry();
        init_behaviors();
        let world = fresh_test_world("lava_cauldron_skip");
        let pos = BlockPos::new(8, 64, 8);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));

        let source_lava = vanilla_blocks::LAVA.default_state();
        assert!(world.set_block(pos.above_n(2), source_lava, UpdateFlags::UPDATE_NONE));
        assert!(world.set_block(
            pos.below(),
            vanilla_blocks::LAVA_CAULDRON.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));

        let (behavior, dripstone) = stalactite_setup(&world, pos);
        behavior.maybe_transfer_fluid(dripstone, &world, pos, 0.0);

        assert_eq!(
            world.get_block_state(pos.below()).get_block(),
            &vanilla_blocks::LAVA_CAULDRON,
        );
    }

    #[test]
    fn stalactite_drip_skips_drip_when_random_value_exceeds_water_probability() {
        init_test_registry();
        init_behaviors();
        let world = fresh_test_world("high_random_water");
        let pos = BlockPos::new(8, 64, 8);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));

        let source_water = vanilla_blocks::WATER.default_state();
        assert!(world.set_block(pos.above_n(2), source_water, UpdateFlags::UPDATE_NONE));
        assert!(world.set_block(
            pos.below(),
            vanilla_blocks::CAULDRON.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));

        let (behavior, dripstone) = stalactite_setup(&world, pos);
        behavior.maybe_transfer_fluid(
            dripstone,
            &world,
            pos,
            WATER_TRANSFER_PROBABILITY_PER_RANDOM_TICK + 0.01,
        );

        assert_eq!(
            world.get_block_state(pos.below()).get_block(),
            &vanilla_blocks::CAULDRON,
        );
    }

    #[test]
    fn stalactite_drip_skips_drip_when_random_value_exceeds_lava_probability() {
        init_test_registry();
        init_behaviors();
        let world = fresh_test_world("high_random_lava");
        let pos = BlockPos::new(8, 64, 8);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));

        let source_lava = vanilla_blocks::LAVA.default_state();
        assert!(world.set_block(pos.above_n(2), source_lava, UpdateFlags::UPDATE_NONE));
        assert!(world.set_block(
            pos.below(),
            vanilla_blocks::CAULDRON.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));

        let (behavior, dripstone) = stalactite_setup(&world, pos);
        behavior.maybe_transfer_fluid(
            dripstone,
            &world,
            pos,
            LAVA_TRANSFER_PROBABILITY_PER_RANDOM_TICK + 0.01,
        );

        assert_eq!(
            world.get_block_state(pos.below()).get_block(),
            &vanilla_blocks::CAULDRON,
        );
    }
}
