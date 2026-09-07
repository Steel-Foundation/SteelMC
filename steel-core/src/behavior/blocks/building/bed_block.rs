use std::sync::Arc;

use crate::{
    behavior::{
        BlockBehavior, BlockHitResult, BlockPlaceContext, BlockStateBehaviorExt as _,
        EntityFallDamage, EntityFallOnContext, EntityLandingContext, InteractionResult,
        InventoryAccess, PlacementSource,
    },
    entity::{Entity, ai::path::PathComputationType, damage::DamageSource, dismount_helper},
    player::Player,
    world::{ScheduledTickAccess, World, explosion::ExplosionInteraction},
};
use glam::DVec3;
use steel_macros::block_behavior;
use steel_registry::blocks::properties::{BedPart, BoolProperty, EnumProperty};
use steel_registry::blocks::{
    BlockRef, block_state_ext::BlockStateExt, properties::BlockStateProperties,
};
use steel_registry::vanilla_blocks;
use steel_utils::{BlockPos, BlockStateId, Direction, types::UpdateFlags};
use text_components::TextComponent;
use text_components::translation::TranslatedMessage;

/// Radius of the blast a bed makes where it cannot be slept in.
const EXPLOSION_RADIUS: f32 = 5.0;
const BED_BOUNCE_SCALE: f64 = 0.660_000_026_226_043_7;
const BED_PART: &EnumProperty<BedPart> = &BlockStateProperties::BED_PART;
const FACING: &EnumProperty<Direction> = &BlockStateProperties::HORIZONTAL_FACING;
const OCCUPIED: &BoolProperty = &BlockStateProperties::OCCUPIED;
/// Behavior for beds
///
/// TODO: Mirror vanilla `BedBlock.kickVillagerOutOfBed` once villager sleeping
/// entities exist.
#[block_behavior]
pub struct BedBlock {
    block: BlockRef,
}

impl BedBlock {
    /// Creates a bed block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    #[must_use]
    fn fall_context(context: EntityFallOnContext<'_>) -> EntityFallOnContext<'_> {
        context.with_fall_distance(context.fall_distance * 0.5)
    }

    #[must_use]
    fn velocity_after_fall(context: EntityLandingContext) -> DVec3 {
        if context.velocity.y >= 0.0 {
            return context.velocity;
        }

        let entity_factor = if context.is_living_entity { 1.0 } else { 0.8 };
        DVec3::new(
            context.velocity.x,
            -context.velocity.y * BED_BOUNCE_SCALE * entity_factor,
            context.velocity.z,
        )
    }

    /// Blows the bed up, as sleeping in the wrong dimension does.
    ///
    /// Mirrors the `bedRule.explodes()` branch of vanilla `BedBlock.useWithoutItem`.
    /// Both halves are removed first so the blast does not have to break them, and the
    /// damage carries no entity, since the bed itself is to blame.
    fn explode(
        &self,
        world: &Arc<World>,
        head_state: BlockStateId,
        head_pos: BlockPos,
        player: &Player,
    ) {
        if let Some(key) = world.dimension_type.bed_rule.error_message_key {
            player.send_overlay_message(&TextComponent::translated(TranslatedMessage {
                key: key.into(),
                fallback: None,
                args: None,
            }));
        }

        world.remove_block(head_pos, false);
        let foot_pos = head_state.get_value(FACING).opposite().relative(head_pos);
        if world.get_block_state(foot_pos).get_block() == self.block {
            world.remove_block(foot_pos, false);
        }

        let (x, y, z) = head_pos.get_center();
        let center = DVec3::new(x, y, z);
        world.explode(
            None,
            Some(DamageSource::bad_respawn_point(center)),
            None,
            center,
            EXPLOSION_RADIUS,
            true,
            ExplosionInteraction::Block,
        );
    }

    fn head_state_and_pos(
        &self,
        world: &Arc<World>,
        state: BlockStateId,
        pos: BlockPos,
    ) -> Option<(BlockStateId, BlockPos)> {
        if state.get_value(BED_PART) == BedPart::Head {
            return Some((state, pos));
        }

        let head_pos = state.get_value(FACING).relative(pos);
        let head_state = world.get_block_state(head_pos);
        (head_state.get_block() == self.block).then_some((head_state, head_pos))
    }

    const fn neighbor_direction(part: &BedPart, facing: Direction) -> Direction {
        match part {
            BedPart::Foot => facing,
            BedPart::Head => facing.opposite(),
        }
    }

    pub(crate) fn find_standup_position(
        world: &Arc<World>,
        entity: &dyn Entity,
        forward_dir: Direction,
        block_pos: BlockPos,
    ) -> Option<DVec3> {
        Self::find_standup_position_with_yaw(
            world,
            entity,
            forward_dir,
            block_pos,
            entity.rotation().0,
        )
    }

    pub(crate) fn find_standup_position_with_yaw(
        world: &Arc<World>,
        entity: &dyn Entity,
        forward_dir: Direction,
        block_pos: BlockPos,
        yaw: f32,
    ) -> Option<DVec3> {
        let right = forward_dir.rotate_y_clockwise();
        let side = if right.is_facing_yaw(yaw) {
            right.opposite()
        } else {
            right
        };

        if world.get_block_state(block_pos.below()).is_bed() {
            Self::find_bunk_bed_standup_position(world, entity, forward_dir, side, block_pos)
        } else {
            let offsets = Self::bed_standup_offsets(forward_dir, side);

            if let Some(safe_pos) =
                Self::find_standup_position_at_offset(world, entity, block_pos, &offsets, true)
            {
                return Some(safe_pos);
            }

            Self::find_standup_position_at_offset(world, entity, block_pos, &offsets, false)
        }
    }

    fn find_bunk_bed_standup_position(
        world: &Arc<World>,
        entity: &dyn Entity,
        forward_dir: Direction,
        side_dir: Direction,
        block_pos: BlockPos,
    ) -> Option<DVec3> {
        let offsets = Self::bed_surround_standup_offsets(forward_dir, side_dir);
        let below = block_pos.below();
        let above_offsets = Self::bed_above_standup_offsets(forward_dir);

        for check_dangerous in [true, false] {
            for (pos, offsets) in [
                (block_pos, offsets.as_slice()),
                (below, offsets.as_slice()),
                (block_pos, above_offsets.as_slice()),
            ] {
                if let Some(pos) = Self::find_standup_position_at_offset(
                    world,
                    entity,
                    pos,
                    offsets,
                    check_dangerous,
                ) {
                    return Some(pos);
                }
            }
        }

        None
    }

    fn find_standup_position_at_offset(
        world: &Arc<World>,
        entity: &dyn Entity,
        pos: BlockPos,
        offsets: &[(i32, i32)],
        check_dangerous: bool,
    ) -> Option<DVec3> {
        for &(off_x, off_z) in offsets {
            let offset_pos = BlockPos::new(pos.x() + off_x, pos.y(), pos.z() + off_z);
            if let Some(position) = dismount_helper::find_safe_dismount_location(
                world,
                entity,
                offset_pos,
                check_dangerous,
            ) {
                return Some(position);
            }
        }

        None
    }

    #[must_use]
    const fn bed_standup_offsets(forward: Direction, side: Direction) -> [(i32, i32); 12] {
        let surround = Self::bed_surround_standup_offsets(forward, side);
        let above = Self::bed_above_standup_offsets(forward);

        [
            surround[0],
            surround[1],
            surround[2],
            surround[3],
            surround[4],
            surround[5],
            surround[6],
            surround[7],
            surround[8],
            surround[9],
            above[0],
            above[1],
        ]
    }

    #[must_use]
    const fn bed_surround_standup_offsets(forward: Direction, side: Direction) -> [(i32, i32); 10] {
        let (fx, fz) = forward.offset_xz();
        let (sx, sz) = side.offset_xz();

        [
            (sx, sz),
            (sx - fx, sz - fz),
            (sx - fx * 2, sz - fz * 2),
            (-fx * 2, -fz * 2),
            (-sx - fx * 2, -sz - fz * 2),
            (-sx - fx, -sz - fz),
            (-sx, -sz),
            (-sx + fx, -sz + fz),
            (fx, fz),
            (sx + fx, sz + fz),
        ]
    }

    #[must_use]
    const fn bed_above_standup_offsets(forward: Direction) -> [(i32, i32); 2] {
        let (fx, fz) = forward.offset_xz();
        [(0, 0), (-fx, -fz)]
    }
}

impl BlockBehavior for BedBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let facing = context.horizontal_direction();
        let head_pos = facing.relative(context.place_pos());
        let head_state = context.world.get_block_state(head_pos);
        if !head_state.can_be_replaced(context)
            || !context.world.is_block_within_world_border(head_pos)
        {
            return None;
        }

        Some(self.block.default_state().set_value(FACING, facing))
    }

    fn fall_on(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        context: EntityFallOnContext<'_>,
    ) -> Option<EntityFallDamage> {
        self.default_fall_on(state, world, pos, Self::fall_context(context))
    }

    fn update_entity_movement_after_fall_on(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        context: EntityLandingContext,
    ) -> DVec3 {
        if context.suppresses_bounce {
            return self.default_update_entity_movement_after_fall_on(state, world, pos, context);
        }

        Self::velocity_after_fall(context)
    }

    fn is_pathfindable(
        &self,
        _state: BlockStateId,
        _computation_type: PathComputationType,
    ) -> bool {
        false
    }

    fn is_bed(&self) -> bool {
        true
    }

    fn player_will_destroy(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        player: &Player,
    ) -> BlockStateId {
        if !player.has_infinite_materials() || state.get_value(BED_PART) != BedPart::Foot {
            return state;
        }

        let facing = state.get_value(FACING);
        let head_pos = Self::neighbor_direction(&BedPart::Foot, facing).relative(pos);
        let head_state = world.get_block_state(head_pos);
        if head_state.get_block() != self.block || head_state.get_value(BED_PART) != BedPart::Head {
            return state;
        }

        world.set_block(
            head_pos,
            vanilla_blocks::AIR.default_state(),
            UpdateFlags::UPDATE_ALL | UpdateFlags::UPDATE_SUPPRESS_DROPS,
        );
        world.destroy_block_effect(head_pos, u32::from(head_state.0), Some(player.id()));
        state
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        _world: &dyn ScheduledTickAccess,
        _pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        neighbor_state: BlockStateId,
    ) -> BlockStateId {
        let part = state.get_value(BED_PART);
        let facing = state.get_value(FACING);
        if direction != Self::neighbor_direction(&part, facing) {
            return state;
        }

        if neighbor_state.get_block() == self.block && neighbor_state.get_value(BED_PART) != part {
            return state.set_value(OCCUPIED, neighbor_state.get_value(OCCUPIED));
        }

        vanilla_blocks::AIR.default_state()
    }

    fn set_placed_by(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _source: &PlacementSource<'_>,
    ) {
        let facing = state.get_value(FACING);
        let head_pos = facing.relative(pos);
        let head_state = state.set_value(BED_PART, BedPart::Head);

        world.set_block(head_pos, head_state, UpdateFlags::UPDATE_ALL);
        world.update_neighbors_at(pos, &vanilla_blocks::AIR);
        world.update_neighbor_shapes_at(state, pos, UpdateFlags::UPDATE_ALL, World::UPDATE_LIMIT);
    }

    fn use_without_item(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        player: &Player,
        _hit_result: &BlockHitResult,
        _inv: &mut InventoryAccess,
    ) -> InteractionResult {
        let Some((head_state, head_pos)) = self.head_state_and_pos(world, state, pos) else {
            return InteractionResult::Consume;
        };

        if world.dimension_type.bed_rule.explodes {
            self.explode(world, head_state, head_pos, player);
            return InteractionResult::SuccessServer;
        }

        if head_state.get_value(OCCUPIED) {
            // TODO: Mirror vanilla `kickVillagerOutOfBed`: find a sleeping
            // villager in this bed AABB and call `stopSleeping` once villager
            // sleeping exists.
            player.send_overlay_message(&TextComponent::translated(TranslatedMessage {
                key: "block.minecraft.bed.occupied".into(),
                fallback: None,
                args: None,
            }));
            return InteractionResult::SuccessServer;
        }

        if let Err(problem) = player.start_sleep_in_bed(head_pos)
            && let Some(message) = problem.message()
        {
            player.send_overlay_message(message);
        }

        InteractionResult::SuccessServer
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use steel_registry::{sound_events, vanilla_entities};

    use crate::behavior::EntityFallOnFacts;

    fn landing(
        velocity: DVec3,
        is_living_entity: bool,
        suppresses_bounce: bool,
    ) -> EntityLandingContext {
        EntityLandingContext::new(velocity, is_living_entity, suppresses_bounce)
    }

    #[test]
    fn bed_halves_fall_distance_before_default_damage() {
        let context = BedBlock::fall_context(EntityFallOnContext::new(
            12.0,
            false,
            EntityFallOnFacts::new(
                &vanilla_entities::PLAYER,
                true,
                0.6,
                1.8,
                (
                    &sound_events::ENTITY_PLAYER_SMALL_FALL,
                    &sound_events::ENTITY_PLAYER_BIG_FALL,
                ),
            ),
            None,
        ));

        assert!((context.fall_distance - 6.0).abs() < f64::EPSILON);
        assert!(!context.suppresses_bounce);
        assert!(context.entity.is_player());
    }

    #[test]
    fn living_entities_bounce_with_bed_factor() {
        let velocity =
            BedBlock::velocity_after_fall(landing(DVec3::new(1.0, -3.0, -2.0), true, false));

        assert!((velocity.y - 1.980_000_078_678_131).abs() < f64::EPSILON);
        assert!((velocity.x - 1.0).abs() < f64::EPSILON);
        assert!((velocity.z + 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn non_living_entities_bounce_with_vanilla_reduction() {
        let velocity =
            BedBlock::velocity_after_fall(landing(DVec3::new(1.0, -3.0, -2.0), false, false));

        assert!((velocity.y - 1.584_000_062_942_505).abs() < f64::EPSILON);
    }

    #[test]
    fn upward_velocity_is_not_changed_by_bounce_logic() {
        let velocity =
            BedBlock::velocity_after_fall(landing(DVec3::new(1.0, 0.5, -2.0), true, false));

        assert_eq!(velocity, DVec3::new(1.0, 0.5, -2.0));
    }
}
