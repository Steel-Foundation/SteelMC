//! Bed block behavior impl

use std::sync::{Arc, Weak};

use glam::DVec3;
use steel_macros::block_behavior;
use steel_registry::{
    blocks::{
        BlockRef,
        block_state_ext::BlockStateExt,
        properties::{BedPart, BlockStateProperties, Direction},
    },
    vanilla_block_entity_types, vanilla_blocks, vanilla_damage_types,
};
use steel_utils::{BlockPos, BlockStateId, types::UpdateFlags};
use text_components::{TextComponent, translation::TranslatedMessage};

use crate::behavior::{
    BlockBehavior, BlockHitResult, BlockPlaceContext, InteractionResult, InventoryAccess,
};
use crate::block_entity::{BLOCK_ENTITIES, SharedBlockEntity};
use crate::entity::Entity;
use crate::entity::damage::DamageSource;
use crate::player::Player;
use crate::world::{ScheduledTickAccess, World};

/// Behavior for bed blocks
#[block_behavior]
pub struct BedBlock {
    block: BlockRef,
}

impl BedBlock {
    /// Creates a new bed block behavior
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    fn neighbor_direction(part: &BedPart, facing: Direction) -> Direction {
        if part == &BedPart::Foot {
            facing
        } else {
            facing.opposite()
        }
    }

    fn send_overlay_translation(player: &Player, key: &'static str) {
        let msg = TextComponent::translated(TranslatedMessage {
            key: key.into(),
            args: None,
            fallback: None,
        });
        player.send_overlay_message(&msg);
    }
}

impl BlockBehavior for BedBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let facing = context.horizontal_direction;
        let head_pos = context.relative_pos.relative(facing);

        if !context.world.is_in_valid_bounds(head_pos) {
            return None;
        }
        if !context.world.get_block_state(head_pos).is_replaceable() {
            return None;
        }

        Some(
            self.block
                .default_state()
                .set_value(&BlockStateProperties::HORIZONTAL_FACING, facing)
                .set_value(&BlockStateProperties::BED_PART, BedPart::Foot)
                .set_value(&BlockStateProperties::OCCUPIED, false),
        )
    }

    fn set_placed_by(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _player: Option<&Player>,
        _inv: &InventoryAccess,
    ) {
        let facing = state.get_value(&BlockStateProperties::HORIZONTAL_FACING);
        let head_pos = pos.relative(facing);
        let head_state = state.set_value(&BlockStateProperties::BED_PART, BedPart::Head);
        world.set_block(head_pos, head_state, UpdateFlags::UPDATE_ALL);
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
        let part = state.get_value(&BlockStateProperties::BED_PART);
        let facing = state.get_value(&BlockStateProperties::HORIZONTAL_FACING);

        if direction == Self::neighbor_direction(&part, facing) {
            if neighbor_state.get_block() == self.block
                && neighbor_state.get_value(&BlockStateProperties::BED_PART) != part
            {
                return state.set_value(
                    &BlockStateProperties::OCCUPIED,
                    neighbor_state.get_value(&BlockStateProperties::OCCUPIED),
                );
            }
            return vanilla_blocks::AIR.default_state();
        }

        state
    }

    fn player_will_destroy(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        player: &Player,
    ) -> BlockStateId {
        if player.has_infinite_materials()
            && state.get_value(&BlockStateProperties::BED_PART) == BedPart::Foot
        {
            let facing = state.get_value(&BlockStateProperties::HORIZONTAL_FACING);
            let head_pos = pos.relative(Self::neighbor_direction(&BedPart::Foot, facing));
            let head_state = world.get_block_state(head_pos);

            if head_state.get_block() == self.block
                && head_state.get_value(&BlockStateProperties::BED_PART) == BedPart::Head
            {
                world.set_block(
                    head_pos,
                    vanilla_blocks::AIR.default_state(),
                    UpdateFlags::UPDATE_ALL | UpdateFlags::UPDATE_SUPPRESS_DROPS,
                );
                world.destroy_block_effect(head_pos, u32::from(head_state.0), Some(player.id));
            }
        }

        state
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
        let mut bed_state = state;

        if bed_state.get_value(&BlockStateProperties::BED_PART) != BedPart::Head {
            let facing = bed_state.get_value(&BlockStateProperties::HORIZONTAL_FACING);
            bed_state = world.get_block_state(pos.relative(facing));
            if bed_state.get_block() != self.block {
                return InteractionResult::Fail;
            }
        }

        let bed_rule = &world.dimension_type.bed_rule;
        if bed_rule.explodes {
            if let Some(key) = bed_rule.error_message_key {
                Self::send_overlay_translation(player, key);
            }
            // TODO: Trigger bad respawn point explosion once world explosion exists
            return InteractionResult::Fail;
        }

        if bed_state.get_value(&BlockStateProperties::OCCUPIED) {
            // TODO: Kick villagers out of occupied beds once villager sleep
            Self::send_overlay_translation(player, "block.minecraft.bed.occupied");
            return InteractionResult::Success;
        }

        // TODO: Call Player::start_sleep_in_bed once full sleep foundations
        InteractionResult::Fail
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
        BLOCK_ENTITIES.create(&vanilla_block_entity_types::BED, level, pos, state)
    }

    fn fall_on(
        &self,
        _state: BlockStateId,
        _world: &Arc<World>,
        _pos: BlockPos,
        entity: &dyn Entity,
        fall_distance: f32,
    ) {
        entity.cause_fall_damage(
            fall_distance * 0.5,
            1.0,
            &DamageSource::environment(&vanilla_damage_types::FALL),
        );
    }

    fn update_entity_movement_after_fall_on(&self, _world: &Arc<World>, entity: &dyn Entity) {
        if entity.is_suppressing_bounce() {
            let velocity = entity.velocity();
            entity.set_velocity(DVec3::new(velocity.x, 0.0, velocity.z));
            return;
        }

        let velocity = entity.velocity();
        if velocity.y < 0.0 {
            let factor = if entity.is_living() { 1.0 } else { 0.8 };
            entity.set_velocity(DVec3::new(
                velocity.x,
                -velocity.y * f64::from(0.66_f32) * factor,
                velocity.z,
            ));
        }
    }
}
