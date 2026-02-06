//! Sign block behavior implementation.
//!
//! Handles sign placement and block entity creation for all sign types.

use std::cmp::Ordering;
use std::f64::consts::PI;
use std::sync::{Arc, Weak};

use steel_registry::REGISTRY;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, Direction};
use steel_registry::blocks::shapes::SupportType;
use steel_registry::vanilla_blocks;
use steel_utils::locks::SyncMutex;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehaviour;
use crate::behavior::context::{BlockHitResult, BlockPlaceContext, InteractionResult};
use crate::block_entity::SharedBlockEntity;
use crate::block_entity::entities::SignBlockEntity;
use crate::player::Player;
use crate::world::World;

/// Converts a rotation in degrees to a 16-segment rotation value (0-15).
///
/// This is equivalent to vanilla's `RotationSegment.convertToSegment(float)`.
/// Each segment is 22.5 degrees, and rotation is measured clockwise from south.
fn convert_to_rotation_segment(degrees: f32) -> u8 {
    // Normalize to 0-360
    let normalized = degrees.rem_euclid(360.0);
    // Convert to segment (each segment is 22.5 degrees)
    // Round to nearest segment
    (((normalized / 22.5) + 0.5) as u8) & 15
}

/// Gets the nearest looking directions from the player's rotation.
///
/// Returns horizontal directions in order of how closely they match the player's look direction.
fn get_nearest_looking_directions(rotation: f32, clicked_face: Direction) -> Vec<Direction> {
    // Build list of directions in order of preference
    // Start with the opposite of the clicked face (most natural for wall signs)
    // Then add directions based on player facing
    let mut directions = Vec::with_capacity(4);

    // Add horizontal directions in order of how closely they match player's look
    let all_horizontal = [
        Direction::North,
        Direction::East,
        Direction::South,
        Direction::West,
    ];

    // Calculate angle for each direction and sort by distance to player's rotation
    let mut scored: Vec<(Direction, f32)> = all_horizontal
        .iter()
        .map(|&dir| {
            let dir_angle = dir.to_yaw();
            let diff = (rotation - dir_angle + 180.0).rem_euclid(360.0) - 180.0;
            (dir, diff.abs())
        })
        .collect();

    scored.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(Ordering::Equal));

    for (dir, _) in scored {
        directions.push(dir);
    }

    // If clicked face is horizontal, prefer placing on the opposite side
    if clicked_face.is_horizontal() {
        let opposite = clicked_face.opposite();
        if let Some(pos) = directions.iter().position(|&d| d == opposite) {
            directions.remove(pos);
            directions.insert(0, opposite);
        }
    }

    directions
}

/// Calculates whether the player is facing the front of a sign.
///
/// Uses the sign's rotation (from block state) and the player's position
/// relative to the sign to determine which side they're looking at.
fn is_facing_front_text(state: BlockStateId, pos: BlockPos, player: &Player) -> bool {
    // Get the sign's Y rotation in degrees from the block state
    let sign_y_rot = get_sign_rotation_degrees(state);

    // Calculate player's angle relative to the sign center
    let player_pos = *player.position.lock();
    let dx = player_pos.x - (f64::from(pos.0.x) + 0.5);
    let dz = player_pos.z - (f64::from(pos.0.z) + 0.5);

    // Calculate angle from sign to player (in degrees, -90 to account for Minecraft's coordinate system)
    let player_angle = (dz.atan2(dx) * 180.0 / PI) as f32 - 90.0;

    // Front text if the angle difference is <= 90 degrees
    let diff = (sign_y_rot - player_angle + 180.0).rem_euclid(360.0) - 180.0;
    diff.abs() <= 90.0
}

/// Gets the Y rotation of a sign in degrees from its block state.
fn get_sign_rotation_degrees(state: BlockStateId) -> f32 {
    // Standing signs use "rotation" property (0-15, each step is 22.5 degrees)
    if let Some(rotation) = state.try_get_value(&BlockStateProperties::ROTATION_16) {
        return f32::from(rotation) * 22.5;
    }

    // Wall signs use "facing" property
    if let Some(facing) = state.try_get_value(&BlockStateProperties::HORIZONTAL_FACING) {
        return facing.to_yaw();
    }

    0.0
}

/// Checks if a block state can support a standing sign.
///
/// Vanilla uses `isSolid()` which checks if the collision shape is a full cube.
/// This means signs cannot be placed on other signs, fences, walls, etc.
fn can_support_standing_sign(world: &World, pos: &BlockPos) -> bool {
    let below_pos = BlockPos::new(pos.x(), pos.y() - 1, pos.z());
    let below_state = world.get_block_state(&below_pos);
    below_state.is_solid()
}

/// Checks if a wall sign can survive at the given position with the given facing.
///
/// Vanilla uses `isSolid()` which allows wall signs to be placed on other signs
/// (since signs have `forceSolidOn`).
fn can_wall_sign_survive(world: &World, pos: &BlockPos, facing: Direction) -> bool {
    // Wall sign needs a solid block behind it
    let behind_pos = facing.opposite().relative(pos);
    let behind_state = world.get_block_state(&behind_pos);
    behind_state.is_solid()
}

/// Checks if a ceiling hanging sign can survive at the given position.
fn can_ceiling_hanging_sign_survive(world: &World, pos: &BlockPos) -> bool {
    let above_pos = BlockPos::new(pos.x(), pos.y() + 1, pos.z());
    let above_state = world.get_block_state(&above_pos);
    above_state.is_face_sturdy_for(Direction::Down, SupportType::Center)
}

/// Checks if a wall hanging sign can attach to a neighboring block.
///
/// Vanilla's `WallHangingSignBlock.canAttachTo` checks:
/// 1. If the neighbor is a wall hanging sign on the same axis, allow attachment
/// 2. Otherwise, check if the face is sturdy with FULL support type
fn can_attach_to(
    world: &World,
    sign_facing: Direction,
    attach_pos: &BlockPos,
    attach_face: Direction,
) -> bool {
    let attach_state = world.get_block_state(attach_pos);
    let attach_block = REGISTRY.blocks.by_state_id(attach_state);

    // Check if it's another wall hanging sign (vanilla uses BlockTags.WALL_HANGING_SIGNS)
    if let Some(block) = attach_block
        && block.key.path.contains("wall_hanging_sign")
    {
        // Wall hanging signs can chain if they're on the same axis
        if let Some(neighbor_facing) =
            attach_state.try_get_value(&BlockStateProperties::HORIZONTAL_FACING)
        {
            return neighbor_facing.axis() == sign_facing.axis();
        }
    }

    // Otherwise, check for sturdy face with FULL support
    attach_state.is_face_sturdy_for(attach_face, SupportType::Full)
}

/// Checks if a wall hanging sign can survive at the given position.
///
/// Wall hanging signs need support on at least one side perpendicular to facing.
/// This matches vanilla's `WallHangingSignBlock.canPlace`.
fn can_wall_hanging_sign_survive(world: &World, pos: &BlockPos, facing: Direction) -> bool {
    let clockwise = facing.rotate_y_clockwise();
    let counter_clockwise = facing.rotate_y_counter_clockwise();

    let can_attach_clockwise = {
        let attach_pos = clockwise.relative(pos);
        can_attach_to(world, facing, &attach_pos, counter_clockwise)
    };

    let can_attach_counter = {
        let attach_pos = counter_clockwise.relative(pos);
        can_attach_to(world, facing, &attach_pos, clockwise)
    };

    can_attach_clockwise || can_attach_counter
}

// TODO: Implement sign applicators (use_with_item):
// - Dye items: Change sign text color (front or back based on player facing)
//   - Check if sign is not waxed
//   - Get the SignText for the side player is facing
//   - If color differs from dye color, update it and consume the dye
//   - Play DYE_USE sound
// - Glow Ink Sac: Make sign text glow
//   - Check if sign is not waxed
//   - If text is not already glowing, set has_glowing_text = true
//   - Consume the ink sac
//   - Play GLOW_INK_SAC_USE sound
// - Ink Sac: Remove glow from sign text
//   - Check if sign is not waxed
//   - If text is glowing, set has_glowing_text = false
//   - Consume the ink sac
//   - Play INK_SAC_USE sound
// - Honeycomb: Wax the sign (prevents future edits)
//   - If sign is not already waxed, set is_waxed = true
//   - Consume the honeycomb
//   - Play HONEYCOMB_WAX_ON sound
//   - Spawn WAX_ON particles

/// Attempts to open the sign editor for a player.
///
/// Checks all conditions required by vanilla:
/// 1. Block entity exists and is a sign
/// 2. Sign is not waxed
/// 3. No other player is currently editing
/// 4. Player has build permission (`may_build`)
///
/// Returns `Success` if the editor was opened, `Pass` otherwise.
fn try_open_sign_editor(
    state: BlockStateId,
    world: &World,
    pos: BlockPos,
    player: &Player,
) -> InteractionResult {
    // Get the block entity
    let Some(block_entity) = world.get_block_entity(&pos) else {
        return InteractionResult::Pass;
    };

    let mut guard = block_entity.lock();
    let Some(sign) = guard.as_any_mut().downcast_mut::<SignBlockEntity>() else {
        return InteractionResult::Pass;
    };

    // Check 1: Is the sign waxed?
    if sign.is_waxed {
        // TODO: Play waxed sign interaction fail sound
        return InteractionResult::Success; // Vanilla returns SUCCESS even when waxed
    }

    // Check 2: Is another player editing?
    if sign.is_other_player_editing(player.gameprofile.id) {
        return InteractionResult::Pass;
    }

    // Check 3: Player must have build permission
    // TODO: Implement may_build check properly
    // if !player.may_build() {
    //     return InteractionResult::Pass;
    // }

    // Determine which side the player is facing
    let is_front_text = is_facing_front_text(state, pos, player);

    // Set the editing player lock
    sign.set_player_who_may_edit(Some(player.gameprofile.id));

    // Release lock before calling player method
    drop(guard);

    // Open the editor
    player.open_sign_editor(pos, is_front_text);
    InteractionResult::Success
}

/// Behavior for standing sign blocks (placed on ground).
pub struct StandingSignBlock {
    block: BlockRef,
}

impl StandingSignBlock {
    /// Creates a new standing sign block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehaviour for StandingSignBlock {
    fn update_shape(
        &self,
        state: BlockStateId,
        world: &World,
        pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        // Standing signs break when the block below is removed
        if direction == Direction::Down && !can_support_standing_sign(world, &pos) {
            return REGISTRY.blocks.get_default_state_id(vanilla_blocks::AIR);
        }
        state
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        // Check if we can place on the block below
        if !can_support_standing_sign(context.world, &context.relative_pos) {
            return None;
        }

        // Calculate rotation from player's yaw
        // Vanilla: RotationSegment.convertToSegment(context.getRotation() + 180.0F)
        let rotation = convert_to_rotation_segment(context.rotation + 180.0);

        Some(
            self.block
                .default_state()
                .set_value(&BlockStateProperties::ROTATION_16, rotation),
        )
    }

    fn has_block_entity(&self) -> bool {
        true
    }

    fn new_block_entity(
        &self,
        level: Weak<World>,
        pos: BlockPos,
        state: BlockStateId,
    ) -> Option<SharedBlockEntity> {
        Some(Arc::new(SyncMutex::new(SignBlockEntity::new(
            level, pos, state,
        ))))
    }

    fn should_keep_block_entity(&self, _old_state: BlockStateId, _new_state: BlockStateId) -> bool {
        // Signs don't keep their block entity when changing to a different block
        false
    }

    fn use_without_item(
        &self,
        state: BlockStateId,
        world: &World,
        pos: BlockPos,
        player: &Player,
        _hit_result: &BlockHitResult,
    ) -> InteractionResult {
        try_open_sign_editor(state, world, pos, player)
    }
}

/// Behavior for wall sign blocks (attached to walls).
pub struct WallSignBlock {
    block: BlockRef,
}

impl WallSignBlock {
    /// Creates a new wall sign block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehaviour for WallSignBlock {
    fn update_shape(
        &self,
        state: BlockStateId,
        world: &World,
        pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        // Wall signs break when the block they're attached to is removed
        // The sign is attached to the block opposite of its facing direction
        if let Some(facing) = state.try_get_value(&BlockStateProperties::HORIZONTAL_FACING)
            && direction.opposite() == facing
            && !can_wall_sign_survive(world, &pos, facing)
        {
            return REGISTRY.blocks.get_default_state_id(vanilla_blocks::AIR);
        }
        state
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        // Try each horizontal direction based on player's look direction
        let directions = get_nearest_looking_directions(context.rotation, context.clicked_face);

        for direction in directions {
            // The sign faces the opposite direction of where it's attached
            let facing = direction.opposite();

            // Check if sign can survive with this facing
            if can_wall_sign_survive(context.world, &context.relative_pos, facing) {
                return Some(
                    self.block
                        .default_state()
                        .set_value(&BlockStateProperties::HORIZONTAL_FACING, facing),
                );
            }
        }

        // No valid placement found
        None
    }

    fn has_block_entity(&self) -> bool {
        true
    }

    fn new_block_entity(
        &self,
        level: Weak<World>,
        pos: BlockPos,
        state: BlockStateId,
    ) -> Option<SharedBlockEntity> {
        Some(Arc::new(SyncMutex::new(SignBlockEntity::new(
            level, pos, state,
        ))))
    }

    fn should_keep_block_entity(&self, _old_state: BlockStateId, _new_state: BlockStateId) -> bool {
        false
    }

    fn use_without_item(
        &self,
        state: BlockStateId,
        world: &World,
        pos: BlockPos,
        player: &Player,
        _hit_result: &BlockHitResult,
    ) -> InteractionResult {
        try_open_sign_editor(state, world, pos, player)
    }
}

/// Behavior for ceiling hanging sign blocks.
pub struct CeilingHangingSignBlock {
    block: BlockRef,
}

impl CeilingHangingSignBlock {
    /// Creates a new ceiling hanging sign block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehaviour for CeilingHangingSignBlock {
    fn update_shape(
        &self,
        state: BlockStateId,
        world: &World,
        pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        // Ceiling hanging signs break when the block above is removed
        if direction == Direction::Up && !can_ceiling_hanging_sign_survive(world, &pos) {
            return REGISTRY.blocks.get_default_state_id(vanilla_blocks::AIR);
        }
        state
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        // Check if we can hang from the block above
        if !can_ceiling_hanging_sign_survive(context.world, &context.relative_pos) {
            return None;
        }

        let above_pos = BlockPos::new(
            context.relative_pos.x(),
            context.relative_pos.y() + 1,
            context.relative_pos.z(),
        );
        let above_state = context.world.get_block_state(&above_pos);

        // Determine if we should attach to the middle or not based on block above
        let direction = Direction::from_yaw(context.rotation);
        let is_above_full = above_state.is_face_sturdy_for(Direction::Down, SupportType::Full);

        // Check if block above is also a hanging sign
        let above_block = REGISTRY.blocks.by_state_id(above_state);
        let is_below_hanging_sign =
            above_block.is_some_and(|b| b.key.path.contains("hanging_sign"));

        // Determine if attached to middle based on vanilla logic
        let attached_to_middle = if is_below_hanging_sign {
            // When below another hanging sign, check if we can chain
            if let Some(above_facing) =
                above_state.try_get_value(&BlockStateProperties::HORIZONTAL_FACING)
            {
                // Wall hanging sign above - check axis alignment
                above_facing.axis() != direction.axis()
            } else if let Some(above_rotation) =
                above_state.try_get_value(&BlockStateProperties::ROTATION_16)
            {
                // Ceiling hanging sign above - check if we can align
                let above_direction = rotation_to_direction(above_rotation);
                above_direction.is_none_or(|d| d.axis() != direction.axis())
            } else {
                !is_above_full
            }
        } else {
            !is_above_full
        };

        // Calculate rotation
        let rotation = if attached_to_middle {
            // Attached to middle - use player rotation
            convert_to_rotation_segment(context.rotation + 180.0)
        } else {
            // Attached to chains - align with direction
            convert_to_rotation_segment(direction.opposite().to_yaw())
        };

        Some(
            self.block
                .default_state()
                .set_value(&BlockStateProperties::ROTATION_16, rotation)
                .set_value(&BlockStateProperties::ATTACHED, attached_to_middle),
        )
    }

    fn has_block_entity(&self) -> bool {
        true
    }

    fn new_block_entity(
        &self,
        level: Weak<World>,
        pos: BlockPos,
        state: BlockStateId,
    ) -> Option<SharedBlockEntity> {
        Some(Arc::new(SyncMutex::new(SignBlockEntity::new_hanging(
            level, pos, state,
        ))))
    }

    fn should_keep_block_entity(&self, _old_state: BlockStateId, _new_state: BlockStateId) -> bool {
        false
    }

    fn use_without_item(
        &self,
        state: BlockStateId,
        world: &World,
        pos: BlockPos,
        player: &Player,
        _hit_result: &BlockHitResult,
    ) -> InteractionResult {
        try_open_sign_editor(state, world, pos, player)
    }
}

/// Converts a rotation segment (0-15) to a cardinal direction, if applicable.
const fn rotation_to_direction(rotation: u8) -> Option<Direction> {
    match rotation {
        0 => Some(Direction::South),
        4 => Some(Direction::West),
        8 => Some(Direction::North),
        12 => Some(Direction::East),
        _ => None,
    }
}

/// Behavior for wall hanging sign blocks.
pub struct WallHangingSignBlock {
    block: BlockRef,
}

impl WallHangingSignBlock {
    /// Creates a new wall hanging sign block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehaviour for WallHangingSignBlock {
    fn update_shape(
        &self,
        state: BlockStateId,
        world: &World,
        pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        // Wall hanging signs break when blocks on the perpendicular axis are removed
        // and they can no longer survive
        if let Some(facing) = state.try_get_value(&BlockStateProperties::HORIZONTAL_FACING) {
            // Check if the change is on the perpendicular axis (clockwise/counterclockwise)
            if direction.axis() == facing.rotate_y_clockwise().axis()
                && !can_wall_hanging_sign_survive(world, &pos, facing)
            {
                return REGISTRY.blocks.get_default_state_id(vanilla_blocks::AIR);
            }
        }
        state
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        // Try each horizontal direction based on player's look direction
        let directions = get_nearest_looking_directions(context.rotation, context.clicked_face);

        for direction in directions {
            // Wall hanging signs face perpendicular to the wall they're attached to
            // Skip if the clicked face is on the same axis
            if direction.axis() == context.clicked_face.axis() {
                continue;
            }

            let facing = direction.opposite();

            // Check if sign can survive with this facing
            if can_wall_hanging_sign_survive(context.world, &context.relative_pos, facing) {
                return Some(
                    self.block
                        .default_state()
                        .set_value(&BlockStateProperties::HORIZONTAL_FACING, facing),
                );
            }
        }

        // No valid placement found
        None
    }

    fn has_block_entity(&self) -> bool {
        true
    }

    fn new_block_entity(
        &self,
        level: Weak<World>,
        pos: BlockPos,
        state: BlockStateId,
    ) -> Option<SharedBlockEntity> {
        Some(Arc::new(SyncMutex::new(SignBlockEntity::new_hanging(
            level, pos, state,
        ))))
    }

    fn should_keep_block_entity(&self, _old_state: BlockStateId, _new_state: BlockStateId) -> bool {
        false
    }

    fn use_without_item(
        &self,
        state: BlockStateId,
        world: &World,
        pos: BlockPos,
        player: &Player,
        _hit_result: &BlockHitResult,
    ) -> InteractionResult {
        try_open_sign_editor(state, world, pos, player)
    }
}
