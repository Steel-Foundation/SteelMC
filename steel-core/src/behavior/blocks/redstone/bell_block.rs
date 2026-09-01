//! Vanilla bell behavior.

use std::sync::{Arc, Weak};

use steel_macros::block_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::block_entity_type::BlockEntityTypeRef;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{
    BellAttachType, BlockStateProperties, BoolProperty, Direction, EnumProperty,
};
use steel_registry::vanilla_custom_stats::BELL_RING;
use steel_registry::{
    sound_events, vanilla_block_entity_types, vanilla_blocks, vanilla_game_events,
};
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId, Downcast as _};

use crate::behavior::{
    BlockBehavior, BlockEntityCreation, BlockHitResult, BlockPlaceContext, InteractionResult,
    InventoryAccess,
};
use crate::block_entity::entities::BellBlockEntity;
use crate::block_entity::{BLOCK_ENTITIES, BlockEntityTicker};
use crate::entity::Entity;
use crate::entity::ai::path::PathComputationType;
use crate::entity::projectile::Projectile;
use crate::player::Player;
use crate::world::game_event::GameEventContext;
use crate::world::{ClipHitResult, LevelReader, ScheduledTickAccess, SignalGetter as _, World};

const MAX_HIT_HEIGHT: f64 = 0.8125;
const FACING: &EnumProperty<Direction> = &BlockStateProperties::FACING;
const BELL_ATTACHMENT: &EnumProperty<BellAttachType> = &BlockStateProperties::BELL_ATTACHMENT;
const POWERED: &BoolProperty = &BlockStateProperties::POWERED;

/// Vanilla `BellBlock`, including placement, ringing, and redstone activation.
#[block_behavior]
pub struct BellBlock {
    block: BlockRef,
}

impl BellBlock {
    /// Creates bell behavior for `block`.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    fn connected_direction(state: BlockStateId) -> Direction {
        match state.get_value(BELL_ATTACHMENT) {
            BellAttachType::Floor => Direction::Down,
            BellAttachType::Ceiling => Direction::Up,
            BellAttachType::SingleWall | BellAttachType::DoubleWall => state.get_value(FACING),
        }
    }

    fn has_support(world: &dyn LevelReader, pos: BlockPos, direction: Direction) -> bool {
        let support_pos = pos.relative(direction);
        let support = world.get_block_state(support_pos);

        world.is_face_sturdy(support, support_pos, direction.opposite())
    }

    // TODO: Notify villagers when the villager AI is fully implemented.
    fn ring(source: Option<&dyn Entity>, world: &Arc<World>, pos: BlockPos, direction: Direction) {
        let Some(block_entity) = world.get_block_entity(pos) else {
            return;
        };
        let Some(bell) = block_entity.downcast_ref::<BellBlockEntity>() else {
            return;
        };
        bell.on_hit(direction);

        world.play_sound(
            &sound_events::BLOCK_BELL_USE,
            SoundSource::Blocks,
            pos,
            2.0,
            1.0,
            None,
        );
        world.game_event(
            &vanilla_game_events::BLOCK_CHANGE,
            pos,
            &GameEventContext::new(source, None),
        );
        if let Some(entity) = source
            && let Some(player) = entity.as_player()
        {
            player.award_custom_stat(&BELL_RING);
        }
    }

    fn is_proper_hit(state: BlockStateId, direction: Direction, height: f64) -> bool {
        if direction.axis().is_vertical() {
            return false;
        }

        if height > MAX_HIT_HEIGHT {
            return false;
        }

        let facing = state.get_value(FACING);
        let attachment = state.get_value(BELL_ATTACHMENT);

        match attachment {
            BellAttachType::Floor => facing.axis() == direction.axis(),
            BellAttachType::Ceiling => true,
            BellAttachType::SingleWall | BellAttachType::DoubleWall => {
                facing.axis() != direction.axis()
            }
        }
    }

    fn update_powered(state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        let powered = world.has_neighbor_signal(pos);
        let old_powered = state.get_value(POWERED);

        if powered == old_powered {
            return;
        }

        if powered {
            let direction = state.get_value(FACING);
            Self::ring(None, world, pos, direction);
        }

        let new_state = state.set_value(POWERED, powered);
        world.set_block(pos, new_state, UpdateFlags::UPDATE_ALL);
    }
}

impl BlockBehavior for BellBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let pos = context.place_pos();
        let clicked_face = context.clicked_face();
        let player_facing = context.horizontal_direction();

        let mut state = self.block.default_state();
        state = state.set_value(FACING, player_facing);

        if clicked_face == Direction::Up {
            state = state.set_value(BELL_ATTACHMENT, BellAttachType::Floor);
        } else if clicked_face == Direction::Down {
            state = state.set_value(BELL_ATTACHMENT, BellAttachType::Ceiling);
        } else {
            let wall_facing = clicked_face.opposite();

            state = state.set_value(FACING, wall_facing);

            let opposite = wall_facing.opposite();

            let double_wall = Self::has_support(context.world, pos, wall_facing)
                && Self::has_support(context.world, pos, opposite);

            let attachment = if double_wall {
                BellAttachType::DoubleWall
            } else {
                BellAttachType::SingleWall
            };

            state = state.set_value(BELL_ATTACHMENT, attachment);

            if self.can_survive(state, context.world, pos) {
                return Some(state);
            }

            let can_attach_below = Self::has_support(context.world, pos, Direction::Down);
            let fallback = if can_attach_below {
                BellAttachType::Floor
            } else {
                BellAttachType::Ceiling
            };
            state = state.set_value(BELL_ATTACHMENT, fallback);
        }

        self.can_survive(state, context.world, pos).then_some(state)
    }

    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        let direction = Self::connected_direction(state);
        Self::has_support(world, pos, direction)
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
        let attachment = state.get_value(BELL_ATTACHMENT);
        let support_direction = Self::connected_direction(state);

        if support_direction == direction
            && attachment != BellAttachType::DoubleWall
            && !Self::has_support(world, pos, support_direction)
        {
            return vanilla_blocks::AIR.default_state();
        }

        let facing = state.get_value(FACING);

        if direction.axis() == facing.axis() {
            if attachment == BellAttachType::DoubleWall
                && !world.is_face_sturdy(neighbor_state, neighbor_pos, direction)
            {
                return state
                    .set_value(FACING, direction.opposite())
                    .set_value(BELL_ATTACHMENT, BellAttachType::SingleWall);
            }

            if attachment == BellAttachType::SingleWall
                && support_direction.opposite() == direction
                && world.is_face_sturdy(neighbor_state, neighbor_pos, facing)
            {
                return state.set_value(BELL_ATTACHMENT, BellAttachType::DoubleWall);
            }
        }

        state
    }

    fn handle_neighbor_changed(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _source_block: BlockRef,
        _moved_by_piston: bool,
    ) {
        Self::update_powered(state, world, pos);
    }

    fn use_without_item(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        player: &Player,
        hit: &BlockHitResult,
        _inv: &mut InventoryAccess,
    ) -> InteractionResult {
        let height = hit.location.y - f64::from(pos.y());
        if !Self::is_proper_hit(state, hit.direction, height) {
            return InteractionResult::Pass;
        }

        Self::ring(Some(player), world, pos, hit.direction);
        InteractionResult::Success
    }

    fn on_projectile_hit(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        hit: &ClipHitResult,
        projectile: &dyn Projectile,
    ) {
        let height = hit.location.y - f64::from(hit.block_pos.y());
        if !Self::is_proper_hit(state, hit.direction, height) {
            return;
        }

        let owner = projectile.get_owner();
        let source = owner
            .as_deref()
            .filter(|entity| entity.as_player().is_some());
        Self::ring(source, world, hit.block_pos, hit.direction);
    }

    fn new_block_entity(
        &self,
        level: Weak<World>,
        pos: BlockPos,
        state: BlockStateId,
    ) -> BlockEntityCreation {
        BlockEntityCreation::from_registered_factory(BLOCK_ENTITIES.create(
            &vanilla_block_entity_types::BELL,
            level,
            pos,
            state,
        ))
    }

    fn get_block_entity_ticker(
        &self,
        _world: &Arc<World>,
        _state: BlockStateId,
        block_entity_type: BlockEntityTypeRef,
    ) -> Option<BlockEntityTicker> {
        BlockEntityTicker::for_matching_entity_tick(
            block_entity_type,
            &vanilla_block_entity_types::BELL,
        )
    }

    fn trigger_event(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        event: i32,
        data: i32,
    ) -> bool {
        let Some(block_entity) = world.get_block_entity(pos) else {
            return false;
        };
        block_entity.trigger_event(event, data)
    }

    fn is_pathfindable(
        &self,
        _state: BlockStateId,
        _computation_type: PathComputationType,
    ) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::init_vanilla_registry;
    use steel_utils::ChunkPos;

    use super::*;
    use crate::behavior::{BLOCK_BEHAVIORS, init_behaviors};
    use crate::block_entity::init_block_entities;
    use crate::test_support::{fresh_test_world, insert_ready_full_chunk};

    #[test]
    fn wall_attachment_uses_facing_as_support_direction() {
        init_vanilla_registry();
        let state = vanilla_blocks::BELL
            .default_state()
            .set_value(BELL_ATTACHMENT, BellAttachType::SingleWall)
            .set_value(FACING, Direction::West);

        assert_eq!(BellBlock::connected_direction(state), Direction::West);
    }

    #[test]
    fn generated_registry_bell_behavior_creates_registered_typed_entity() {
        init_vanilla_registry();
        init_block_entities();
        init_behaviors();
        let behavior = BLOCK_BEHAVIORS.get_behavior(&vanilla_blocks::BELL);
        let entity = behavior
            .new_block_entity(
                Weak::new(),
                BlockPos::new(0, 64, 0),
                vanilla_blocks::BELL.default_state(),
            )
            .into_created()
            .expect("bell should create its registered block entity");

        assert!(BLOCK_ENTITIES.has_factory(&vanilla_block_entity_types::BELL));
        assert!(entity.downcast_ref::<BellBlockEntity>().is_some());
    }

    #[test]
    fn placed_bell_is_stored_with_its_ticker() {
        init_vanilla_registry();
        init_block_entities();
        init_behaviors();
        let world = fresh_test_world("placed_bell_entity");
        let pos = BlockPos::new(4, 64, 4);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));
        assert!(world.set_block(
            pos.relative(Direction::Down),
            vanilla_blocks::STONE.default_state(),
            UpdateFlags::UPDATE_ALL,
        ));
        let state = vanilla_blocks::BELL.default_state();
        assert!(world.set_block(pos, state, UpdateFlags::UPDATE_ALL));

        let entity = world
            .get_block_entity(pos)
            .expect("placed bell should be stored as a block entity");
        assert!(entity.downcast_ref::<BellBlockEntity>().is_some());
        assert!(
            BLOCK_BEHAVIORS
                .get_behavior(&vanilla_blocks::BELL)
                .get_block_entity_ticker(&world, state, entity.get_type())
                .is_some()
        );
    }
}
