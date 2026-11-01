use std::sync::Arc;

use crate::{
    behavior::{
        BlockBehavior, BlockHitResult, BlockPlaceContext, BlockStateBehaviorExt as _,
        InteractionResult, InventoryAccess, PlacementSource,
    },
    entity::{Entity, ai::path::PathComputationType, dismount_helper},
    player::Player,
    world::{LevelReader, ScheduledTickAccess, World},
};
use glam::DVec3;
use steel_macros::block_behavior;
use steel_registry::blocks::properties::{BedPart, BoolProperty, EnumProperty};
use steel_registry::blocks::{
    BlockRef, block_state_ext::BlockStateExt, properties::BlockStateProperties,
};
use steel_registry::{vanilla_blocks, vanilla_custom_stats};
use steel_utils::{
    BlockPos, BlockStateId, Direction, axis::Axis, translations, types::UpdateFlags,
};

const BED_PART: &EnumProperty<BedPart> = &BlockStateProperties::BED_PART;
const FACING: &EnumProperty<Direction> = &BlockStateProperties::HORIZONTAL_FACING;
const OCCUPIED: &BoolProperty = &BlockStateProperties::OCCUPIED;
/// Behavior for beds
///
/// TODO: Mirror vanilla `BedBlock.useWithoutItem` invalid-dimension explosion
/// once Steel has a strict `World::explode` foundation: show the bed-rule error
/// message, remove both bed halves, and use bad-respawn-point explosion damage.
/// TODO: Mirror vanilla `BedBlock.kickVillagerOutOfBed` once villager sleeping
/// entities exist.
#[block_behavior]
pub struct BedBlock {
    pub(super) block: BlockRef,
}

impl BedBlock {
    /// Creates a bed block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    pub(super) fn head_state_and_pos(
        &self,
        world: &Arc<World>,
        state: BlockStateId,
        pos: BlockPos,
    ) -> Option<(BlockStateId, BlockPos)> {
        let part = state.get_value(BED_PART);
        let other_pos = Self::neighbor_direction(part, state.get_value(FACING)).relative(pos);
        let other_state = world.get_block_state(other_pos);
        if other_state.get_block() != self.block || other_state.get_value(BED_PART) == part {
            return None;
        }

        match part {
            BedPart::Head => Some((state, pos)),
            BedPart::Foot => Some((other_state, other_pos)),
        }
    }

    const fn neighbor_direction(part: BedPart, facing: Direction) -> Direction {
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

    fn get_sleep_height(
        &self,
        state: BlockStateId,
        _world: &dyn LevelReader,
        _pos: BlockPos,
    ) -> Option<f64> {
        let shape = state.get_static_outline_shape();
        (!shape.is_empty()).then(|| shape.max(Axis::Y))
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
        let head_pos = Self::neighbor_direction(BedPart::Foot, facing).relative(pos);
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
        if direction != Self::neighbor_direction(part, facing) {
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

        if world.dimension_type.bed_rule.destroy_on_use {
            // TODO: When WOrld::explode foundation exists display the bedrule error remove both halves and create the bad respawn point explosion
            return InteractionResult::SuccessServer;
        }

        if head_state.get_value(OCCUPIED) {
            // TODO: Mirror vanilla `kickVillagerOutOfBed`: find a sleeping
            // villager in this bed AABB and call `stopSleeping` once villager
            // sleeping exists.
            player.send_overlay_message(
                &translations::BLOCK_MINECRAFT_BED_OCCUPIED.msg().component(),
            );
            return InteractionResult::SuccessServer;
        }

        if let Err(problem) = player.start_sleep_in_bed(
            head_pos,
            &world.dimension_type.bed_rule,
            &vanilla_custom_stats::SLEEP_IN_BED,
        ) && let Some(message) = problem.message()
        {
            player.send_overlay_message(message);
        }

        InteractionResult::SuccessServer
    }
}
