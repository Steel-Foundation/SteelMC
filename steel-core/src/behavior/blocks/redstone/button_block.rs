//! Button block behavior.
//!
//! Buttons are face-attached blocks that emit a redstone signal when pressed.
//! They automatically unpress after a delay via the scheduled tick system.
//!
//! Vanilla equivalent: `ButtonBlock` + `FaceAttachedHorizontalDirectionalBlock`.

use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, Direction};
use steel_registry::sound_event::SoundEventRef;
use steel_registry::vanilla_game_events;
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId};

use super::face_attached_horizontal_directional_block::FaceAttachedHorizontalDirectionalBlock;
use crate::behavior::InventoryAccess;
use crate::behavior::block::BlockBehavior;
use crate::behavior::context::{BlockHitResult, BlockPlaceContext, InteractionResult};
use crate::entity::{Entity, InsideBlockEffectCollector, SharedEntity};
use crate::player::Player;
use crate::world::{
    LevelReader, ScheduledTickAccess, SignalQueryContext, World,
    game_event_context::GameEventContext,
};

/// Behavior for all button block variants.
///
/// Stone buttons stay pressed for 20 ticks, wood buttons for 30 ticks.
/// Each variant has its own click on/off sounds determined by the block set type.
#[block_behavior]
pub struct ButtonBlock {
    face_attached: FaceAttachedHorizontalDirectionalBlock,
    #[json_arg(value)]
    ticks_to_stay_pressed: i32,
    #[json_arg(value, json = "type_can_button_be_activated_by_arrows")]
    arrow_sensitive: bool,
    #[json_arg(sound_events, json = "type_button_click_on")]
    sound_click_on: SoundEventRef,
    #[json_arg(sound_events, json = "type_button_click_off")]
    sound_click_off: SoundEventRef,
}

impl ButtonBlock {
    /// Creates a new button block behavior.
    ///
    /// Parameters are provided by the build system from `classes.json`.
    #[must_use]
    pub const fn new(
        block: BlockRef,
        ticks_to_stay_pressed: i32,
        arrow_sensitive: bool,
        sound_click_on: SoundEventRef,
        sound_click_off: SoundEventRef,
    ) -> Self {
        Self {
            face_attached: FaceAttachedHorizontalDirectionalBlock::new(block),
            ticks_to_stay_pressed,
            arrow_sensitive,
            sound_click_on,
            sound_click_off,
        }
    }

    /// Updates neighbors at both the button position and the support block position.
    ///
    /// Vanilla equivalent: `ButtonBlock.updateNeighbors()`.
    fn update_button_neighbors(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        world.update_neighbors_at(pos, self.face_attached.block);
        let support_dir =
            FaceAttachedHorizontalDirectionalBlock::connected_direction(state).opposite();
        let support_pos = support_dir.relative(pos);
        world.update_neighbors_at(support_pos, self.face_attached.block);
    }

    /// Presses the button: sets POWERED=true, updates neighbors, schedules unpress tick,
    /// and plays the click sound.
    fn press(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        player: Option<&Player>,
    ) {
        let powered_state = state.set_value(&BlockStateProperties::POWERED, true);
        world.set_block(pos, powered_state, UpdateFlags::UPDATE_ALL);
        self.update_button_neighbors(powered_state, world, pos);
        world.schedule_block_tick_default(
            pos,
            self.face_attached.block,
            self.ticks_to_stay_pressed,
        );
        world.play_block_sound(self.sound_click_on, pos, 1.0, 1.0, player.map(Player::id));
        world.game_event(
            &vanilla_game_events::BLOCK_ACTIVATE,
            pos,
            &GameEventContext::new(player.map(|player| player as &dyn Entity), None),
        );
    }

    fn first_arrow(
        &self,
        state: BlockStateId,
        world: &World,
        pos: BlockPos,
    ) -> Option<SharedEntity> {
        if !self.arrow_sensitive {
            return None;
        }
        let bounds = state.get_outline_shape_at(pos).bounds()?.at_block(pos);
        world
            .get_entities_in_aabb_matching(&bounds, Entity::is_abstract_arrow)
            .into_iter()
            .next()
    }

    fn check_pressed(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        let first_arrow = self.first_arrow(state, world, pos);
        let should_be_pressed = first_arrow.is_some();
        let was_pressed = state.get_value(&BlockStateProperties::POWERED);
        if should_be_pressed != was_pressed {
            world.set_block(
                pos,
                state.set_value(&BlockStateProperties::POWERED, should_be_pressed),
                UpdateFlags::UPDATE_ALL,
            );
            self.update_button_neighbors(state, world, pos);
            world.play_block_sound(
                if should_be_pressed {
                    self.sound_click_on
                } else {
                    self.sound_click_off
                },
                pos,
                1.0,
                1.0,
                None,
            );
            world.game_event(
                if should_be_pressed {
                    &vanilla_game_events::BLOCK_ACTIVATE
                } else {
                    &vanilla_game_events::BLOCK_DEACTIVATE
                },
                pos,
                &GameEventContext::new(first_arrow.as_deref(), None),
            );
        }

        if should_be_pressed {
            world.schedule_block_tick_default(
                pos,
                self.face_attached.block,
                self.ticks_to_stay_pressed,
            );
        }
    }
}

impl BlockBehavior for ButtonBlock {
    /// Checks if a button with the given state can survive at the given position.
    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        FaceAttachedHorizontalDirectionalBlock::can_survive(state, world, pos)
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        self.face_attached.state_for_placement(context)
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
        FaceAttachedHorizontalDirectionalBlock::update_shape(state, world, pos, direction)
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
        let powered: bool = state.get_value(&BlockStateProperties::POWERED);
        if powered {
            return InteractionResult::Consume;
        }
        self.press(state, world, pos, Some(player));
        InteractionResult::Success
    }

    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        let powered: bool = state.get_value(&BlockStateProperties::POWERED);
        if !powered {
            return;
        }
        self.check_pressed(state, world, pos);
    }

    fn entity_inside(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _entity: &dyn Entity,
        _effect_collector: &mut InsideBlockEffectCollector,
        _is_precise: bool,
    ) {
        if self.arrow_sensitive && !state.get_value(&BlockStateProperties::POWERED) {
            self.check_pressed(state, world, pos);
        }
    }

    fn affect_neighbors_after_removal(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        moved_by_piston: bool,
    ) {
        if moved_by_piston {
            return;
        }
        let powered: bool = state.get_value(&BlockStateProperties::POWERED);
        if !powered {
            return;
        }
        self.update_button_neighbors(state, world, pos);
    }

    fn is_signal_source(&self, _state: BlockStateId, _context: SignalQueryContext) -> bool {
        true
    }

    fn get_own_signal(
        &self,
        state: BlockStateId,
        _world: &dyn LevelReader,
        _pos: BlockPos,
        _context: SignalQueryContext,
    ) -> i32 {
        if state.get_value(&BlockStateProperties::POWERED) {
            15
        } else {
            0
        }
    }

    fn get_direct_signal(
        &self,
        state: BlockStateId,
        _world: &dyn LevelReader,
        _pos: BlockPos,
        direction: Direction,
        _context: SignalQueryContext,
    ) -> i32 {
        if state.get_value(&BlockStateProperties::POWERED)
            && FaceAttachedHorizontalDirectionalBlock::connected_direction(state) == direction
        {
            15
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use glam::DVec3;
    use steel_registry::test_support::init_test_registry;
    use steel_registry::{vanilla_blocks, vanilla_entities};
    use steel_utils::ChunkPos;

    use super::*;
    use crate::behavior::{BLOCK_BEHAVIORS, init_behaviors};
    use crate::entity::entities::RawEntity;
    use crate::entity::{InsideBlockEffectCollector, SharedEntity};
    use crate::test_support::{fresh_test_world, insert_ready_full_chunk};

    #[test]
    fn wooden_button_stays_pressed_while_arrow_intersects_its_shape() {
        init_test_registry();
        init_behaviors();
        let world = fresh_test_world("wooden_button_arrow");
        let pos = BlockPos::new(8, 64, 8);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));
        let state = vanilla_blocks::OAK_BUTTON.default_state();
        assert!(world.set_block(pos, state, UpdateFlags::UPDATE_NONE));

        let bounds = state
            .get_outline_shape_at(pos)
            .bounds()
            .expect("button outline should be non-empty")
            .at_block(pos);
        let arrow_pos = DVec3::new(
            f64::midpoint(bounds.min_x(), bounds.max_x()),
            bounds.min_y(),
            f64::midpoint(bounds.min_z(), bounds.max_z()),
        );
        let arrow: SharedEntity = Arc::new(RawEntity::new(
            7_001,
            arrow_pos,
            Arc::downgrade(&world),
            &vanilla_entities::ARROW,
        ));
        world
            .try_add_entity(Arc::clone(&arrow))
            .expect("test arrow should enter loaded chunk");

        let mut effects = InsideBlockEffectCollector::new();
        BLOCK_BEHAVIORS
            .get_behavior(&vanilla_blocks::OAK_BUTTON)
            .entity_inside(state, &world, pos, arrow.as_ref(), &mut effects, true);

        assert!(
            world
                .get_block_state(pos)
                .get_value(&BlockStateProperties::POWERED)
        );
        assert!(world.has_scheduled_block_tick(pos, &vanilla_blocks::OAK_BUTTON));
    }
}
