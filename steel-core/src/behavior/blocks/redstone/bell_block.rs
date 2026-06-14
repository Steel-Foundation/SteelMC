use std::sync::{Arc, Weak};

use steel_macros::block_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BellAttachType, BlockStateProperties};
use steel_registry::blocks::shapes::SupportType;
use steel_registry::sound_events;
use steel_registry::vanilla_blocks;
use steel_registry::vanilla_game_events;
use steel_utils::locks::SyncMutex;
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId, Direction, axis::Axis};

use crate::behavior::BlockBehavior;
use crate::behavior::InventoryAccess;
use crate::behavior::context::{BlockHitResult, BlockPlaceContext, InteractionResult};
use crate::block_entity::SharedBlockEntity;
use crate::block_entity::entities::BellBlockEntity;
use crate::entity::Entity;
use crate::player::Player;
use crate::world::game_event_context::GameEventContext;
use crate::world::{LevelReader, ScheduledTickAccess, World};

/// Bell
#[block_behavior]
pub struct BellBlock {
    block: BlockRef,
}

impl BellBlock {
    /// Creates new bell block
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    /// Returns the direction the bell is connected to (ceiling/floor/wall).
    fn get_connected_direction(state: BlockStateId) -> Direction {
        let attachment = state
            .try_get_value(&BlockStateProperties::BELL_ATTACHMENT)
            .unwrap_or(BellAttachType::Floor);
        match attachment {
            BellAttachType::Floor => Direction::Up,
            BellAttachType::Ceiling => Direction::Down,
            _ => state
                .try_get_value(&BlockStateProperties::FACING)
                .unwrap_or(Direction::North),
        }
    }

    /// Checks whether the hit was on the correct side of the bell.
    fn is_proper_hit(state: BlockStateId, clicked_direction: Direction, click_y: f64) -> bool {
        if clicked_direction.axis() == Axis::Y || click_y > 0.8124 {
            return false;
        }

        let facing = state
            .try_get_value(&BlockStateProperties::FACING)
            .unwrap_or(Direction::North);
        let attachment = state
            .try_get_value(&BlockStateProperties::BELL_ATTACHMENT)
            .unwrap_or(BellAttachType::Floor);

        match attachment {
            BellAttachType::Floor => facing.axis() == clicked_direction.axis(),
            BellAttachType::SingleWall | BellAttachType::DoubleWall => {
                facing.axis() != clicked_direction.axis()
            }
            BellAttachType::Ceiling => true,
        }
    }

    /// Central ring logic: notifies the block entity and plays a sound.
    fn attempt_to_ring(
        &self,
        ringing_entity: Option<&dyn Entity>,
        world: &Arc<World>,
        pos: BlockPos,
        direction: Option<Direction>,
    ) -> bool {
        let block_entity = world.get_block_entity(pos);
        if let Some(be) = block_entity {
            let mut bell_be = be.lock();
            if let Some(bell) = bell_be.as_any_mut().downcast_mut::<BellBlockEntity>() {
                let dir = direction.unwrap_or_else(|| {
                    world
                        .get_block_state(pos)
                        .try_get_value(&BlockStateProperties::FACING)
                        .unwrap_or(Direction::North)
                });
                bell.on_hit(dir);
                world.play_sound(
                    &sound_events::BLOCK_BELL_USE,
                    SoundSource::Blocks,
                    pos,
                    2.0,
                    1.0,
                    None,
                );
                world.block_event(pos, self.block, 1, dir as u8);
                world.game_event(
                    &vanilla_game_events::BLOCK_CHANGE,
                    pos,
                    &GameEventContext::new(ringing_entity, None),
                );
                return true;
            }
        }
        false
    }
}

impl BlockBehavior for BellBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let clicked_face = context.clicked_face;
        let pos = context.clicked_pos.relative(clicked_face);
        println!(
            "get_state_for_placement: clicked_pos={:?}, clicked_face={:?}, pos={:?}",
            context.clicked_pos, context.clicked_face, pos
        );
        let world = context.world;
        let axis = clicked_face.axis();

        if axis == Axis::Y {
            let attachment = if clicked_face == Direction::Down {
                BellAttachType::Ceiling
            } else {
                BellAttachType::Floor
            };
            let state = self
                .block
                .default_state()
                .set_value(&BlockStateProperties::BELL_ATTACHMENT, attachment)
                .set_value(&BlockStateProperties::FACING, context.horizontal_direction);
            if self.can_survive(state, world, pos) {
                return Some(state);
            }
        } else {
            let double_attached = {
                let west = world.get_block_state(pos.west());
                let east = world.get_block_state(pos.east());
                let north = world.get_block_state(pos.north());
                let south = world.get_block_state(pos.south());
                (axis == Axis::X
                    && west.is_face_sturdy(Direction::East)
                    && east.is_face_sturdy(Direction::West))
                    || (axis == Axis::Z
                        && north.is_face_sturdy(Direction::South)
                        && south.is_face_sturdy(Direction::North))
            };

            let state = self
                .block
                .default_state()
                .set_value(&BlockStateProperties::FACING, clicked_face.opposite())
                .set_value(
                    &BlockStateProperties::BELL_ATTACHMENT,
                    if double_attached {
                        BellAttachType::DoubleWall
                    } else {
                        BellAttachType::SingleWall
                    },
                );

            if self.can_survive(state, world, pos) {
                return Some(state);
            }

            // Fallback: floor/ceiling
            let below = world.get_block_state(pos.below());
            let can_attach_below = below.is_face_sturdy(Direction::Up);
            let fallback = state.set_value(
                &BlockStateProperties::BELL_ATTACHMENT,
                if can_attach_below {
                    BellAttachType::Floor
                } else {
                    BellAttachType::Ceiling
                },
            );
            if self.can_survive(fallback, world, pos) {
                return Some(fallback);
            }
        }

        None
    }

    fn handle_neighbor_changed(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _source_block: BlockRef,
        _moved_by_piston: bool,
    ) {
        // FIXME: Replace with proper neighbor signal check when World API is available.
        let signal = false; // TODO: world.has_neighbor_signal(pos);
        let powered = state
            .try_get_value(&BlockStateProperties::POWERED)
            .unwrap_or(false);

        if signal != powered {
            if signal {
                self.attempt_to_ring(None, world, pos, None);
            }
            let new_state = state.set_value(&BlockStateProperties::POWERED, signal);
            world.set_block(pos, new_state, UpdateFlags::UPDATE_ALL);
        }
    }

    fn use_without_item(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        player: &Player,
        hit_result: &BlockHitResult,
        _inv: &mut InventoryAccess,
    ) -> InteractionResult {
        let direction = hit_result.direction;
        let click_y = hit_result.location.y - f64::from(pos.y());
        if Self::is_proper_hit(state, direction, click_y)
            && self.attempt_to_ring(Some(player as &dyn Entity), world, pos, Some(direction))
        {
            // TODO: award stat BELL_RING when stats are available
            return InteractionResult::Success;
        }
        InteractionResult::Pass
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction_to_neighbour: Direction,
        _neighbour_pos: BlockPos,
        neighbour_state: BlockStateId,
    ) -> BlockStateId {
        let attachment = state
            .try_get_value(&BlockStateProperties::BELL_ATTACHMENT)
            .unwrap_or(BellAttachType::Floor);
        let connected_dir = Self::get_connected_direction(state).opposite();

        // If the support block disappears, break unless double wall.
        if connected_dir == direction_to_neighbour
            && !self.can_survive(state, world, pos)
            && attachment != BellAttachType::DoubleWall
        {
            return vanilla_blocks::AIR.default_state();
        }

        let facing = state
            .try_get_value(&BlockStateProperties::FACING)
            .unwrap_or(Direction::North);

        if direction_to_neighbour.axis() == facing.axis() {
            if attachment == BellAttachType::DoubleWall
                && !neighbour_state.is_face_sturdy(direction_to_neighbour)
            {
                // One side lost support -> become single wall.
                return state
                    .set_value(
                        &BlockStateProperties::BELL_ATTACHMENT,
                        BellAttachType::SingleWall,
                    )
                    .set_value(
                        &BlockStateProperties::FACING,
                        direction_to_neighbour.opposite(),
                    );
            }

            if attachment == BellAttachType::SingleWall
                && connected_dir.opposite() == direction_to_neighbour
                && neighbour_state.is_face_sturdy(facing)
            {
                // Gained support on the other side -> become double wall.
                return state.set_value(
                    &BlockStateProperties::BELL_ATTACHMENT,
                    BellAttachType::DoubleWall,
                );
            }
        }

        state // No change needed
    }

    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        let support_dir = Self::get_connected_direction(state); // уже направление к опоре
        if support_dir == Direction::Down {
            // колокол висит на потолке → опора сверху
            world
                .get_block_state(pos.above())
                .is_face_sturdy_for(Direction::Down, SupportType::Center)
        } else {
            world
                .get_block_state(pos.relative(support_dir))
                .is_face_sturdy_for(support_dir.opposite(), SupportType::Full)
        }
    }

    // get_collision_shape не переопределён – используется стандартная реализация,
    // которая берёт форму из Block (state.get_collision_shape()).

    fn has_block_entity(&self) -> bool {
        true
    }

    fn new_block_entity(
        &self,
        level: Weak<World>,
        pos: BlockPos,
        state: BlockStateId,
    ) -> Option<SharedBlockEntity> {
        Some(Arc::new(SyncMutex::new(BellBlockEntity::new(
            level, pos, state,
        ))))
    }
}
