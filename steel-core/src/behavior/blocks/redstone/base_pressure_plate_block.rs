//! Shared vanilla pressure-plate behavior.

use std::sync::Arc;

use steel_registry::blocks::BlockRef;
use steel_registry::blocks::properties::Direction;
use steel_registry::blocks::shapes::SupportType;
use steel_registry::sound_event::SoundEventRef;
use steel_registry::{vanilla_blocks, vanilla_game_events};
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId, WorldAabb};

use crate::behavior::BlockPlaceContext;
use crate::entity::Entity;
use crate::world::game_event_context::GameEventContext;
use crate::world::{LevelAccessor, LevelReader, World};

const TOUCH_INSET: f64 = 1.0 / 16.0;
const TOUCH_HEIGHT: f64 = 4.0 / 16.0;

/// Common server-side behavior inherited from vanilla's `BasePressurePlateBlock`.
pub(super) struct BasePressurePlateBlock {
    pub(super) block: BlockRef,
}

impl BasePressurePlateBlock {
    #[must_use]
    pub(super) const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    pub(super) fn can_survive(level: &dyn LevelReader, pos: BlockPos) -> bool {
        let below_pos = pos.below();
        let below_state = level.get_block_state(below_pos);
        level.is_face_sturdy_for(below_state, below_pos, Direction::Up, SupportType::Rigid)
            || level.is_face_sturdy_for(below_state, below_pos, Direction::Up, SupportType::Center)
    }

    pub(super) fn state_for_placement(
        &self,
        context: &BlockPlaceContext<'_>,
    ) -> Option<BlockStateId> {
        let state = self.block.default_state();
        Self::can_survive(context.world.as_ref(), context.place_pos()).then_some(state)
    }

    pub(super) fn update_shape(
        state: BlockStateId,
        level: &dyn LevelReader,
        pos: BlockPos,
        direction: Direction,
    ) -> BlockStateId {
        if direction == Direction::Down && !Self::can_survive(level, pos) {
            vanilla_blocks::AIR.default_state()
        } else {
            state
        }
    }

    fn update_neighbors(&self, world: &Arc<World>, pos: BlockPos) {
        world.update_neighbors_at(pos, self.block);
        world.update_neighbors_at(pos.below(), self.block);
    }

    pub(super) fn affect_neighbors_after_removal(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        moved_by_piston: bool,
        signal: i32,
    ) {
        if !moved_by_piston && signal > 0 {
            self.update_neighbors(world, pos);
        }
    }

    pub(super) fn entity_count(
        world: &World,
        pos: BlockPos,
        mut class_filter: impl FnMut(&dyn Entity) -> bool,
    ) -> usize {
        let min_x = f64::from(pos.x()) + TOUCH_INSET;
        let min_y = f64::from(pos.y());
        let min_z = f64::from(pos.z()) + TOUCH_INSET;
        let bounds = WorldAabb::new(
            min_x,
            min_y,
            min_z,
            f64::from(pos.x() + 1) - TOUCH_INSET,
            min_y + TOUCH_HEIGHT,
            f64::from(pos.z() + 1) - TOUCH_INSET,
        );
        world
            .get_entities_in_aabb_matching(&bounds, |entity| {
                !entity.is_spectator()
                    && !entity.is_ignoring_block_triggers()
                    && class_filter(entity)
            })
            .len()
    }

    fn emit_transition_effects(
        level: &dyn LevelAccessor,
        source_entity: Option<&dyn Entity>,
        pos: BlockPos,
        is_pressed: bool,
        sound_click_on: SoundEventRef,
        sound_click_off: SoundEventRef,
    ) {
        level.play_block_sound(
            if is_pressed {
                sound_click_on
            } else {
                sound_click_off
            },
            pos,
            1.0,
            1.0,
            None,
        );
        level.game_event(
            if is_pressed {
                &vanilla_game_events::BLOCK_ACTIVATE
            } else {
                &vanilla_game_events::BLOCK_DEACTIVATE
            },
            pos,
            &GameEventContext::new(source_entity, None),
        );
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "arguments mirror vanilla checkPressed state and variant hooks"
    )]
    pub(super) fn check_pressed(
        &self,
        source_entity: Option<&dyn Entity>,
        world: &Arc<World>,
        pos: BlockPos,
        old_signal: i32,
        signal: i32,
        new_state: BlockStateId,
        pressed_time: i32,
        sound_click_on: SoundEventRef,
        sound_click_off: SoundEventRef,
    ) {
        let was_pressed = old_signal > 0;
        let is_pressed = signal > 0;
        if old_signal != signal {
            world.set_block(pos, new_state, UpdateFlags::UPDATE_CLIENTS);
            self.update_neighbors(world, pos);
            // Vanilla's `setBlocksDirty` is client rendering bookkeeping and
            // is a server-side no-op.
        }

        if is_pressed != was_pressed {
            Self::emit_transition_effects(
                world,
                source_entity,
                pos,
                is_pressed,
                sound_click_on,
                sound_click_off,
            );
        }

        if is_pressed {
            world.schedule_block_tick_default(pos, self.block, pressed_time);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Weak;

    use glam::DVec3;
    use steel_registry::{
        sound_events, test_support::init_test_registry, vanilla_entities, vanilla_game_events,
    };

    use super::*;
    use crate::entity::entities::RawEntity;
    use crate::test_support::TestLevel;

    #[test]
    fn redstone_transition_side_effects_match_vanilla_for_pressure_plates() {
        init_test_registry();
        let level = TestLevel::default();
        let source = RawEntity::new(42, DVec3::ZERO, Weak::new(), &vanilla_entities::ARMOR_STAND);
        let pos = BlockPos::new(3, 64, -2);

        BasePressurePlateBlock::emit_transition_effects(
            &level,
            Some(&source),
            pos,
            true,
            &sound_events::BLOCK_STONE_PRESSURE_PLATE_CLICK_ON,
            &sound_events::BLOCK_STONE_PRESSURE_PLATE_CLICK_OFF,
        );
        BasePressurePlateBlock::emit_transition_effects(
            &level,
            None,
            pos,
            false,
            &sound_events::BLOCK_STONE_PRESSURE_PLATE_CLICK_ON,
            &sound_events::BLOCK_STONE_PRESSURE_PLATE_CLICK_OFF,
        );

        let sounds = level.block_sounds.borrow();
        assert_eq!(sounds.len(), 2);
        assert_eq!(
            sounds[0].sound,
            &sound_events::BLOCK_STONE_PRESSURE_PLATE_CLICK_ON
        );
        assert_eq!(sounds[0].exclude, None);
        assert_eq!(
            sounds[1].sound,
            &sound_events::BLOCK_STONE_PRESSURE_PLATE_CLICK_OFF
        );
        assert_eq!(sounds[1].exclude, None);

        let events = level.game_events.borrow();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event, &vanilla_game_events::BLOCK_ACTIVATE);
        assert_eq!(events[0].source_entity_id, Some(42));
        assert_eq!(events[1].event, &vanilla_game_events::BLOCK_DEACTIVATE);
        assert_eq!(events[1].source_entity_id, None);
    }
}
