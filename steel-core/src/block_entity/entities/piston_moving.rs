//! Vanilla moving-piston block entity.

use std::cell::Cell;
use std::sync::{Arc, Weak};

use glam::DVec3;
use simdnbt::borrow::{BaseNbtCompound as BorrowedNbtCompound, NbtCompound as NbtCompoundView};
use simdnbt::owned::NbtCompound;
use steel_registry::blocks::behavior::PushReaction;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{BlockStateProperties, Direction, PistonType};
use steel_registry::{vanilla_block_entity_types, vanilla_blocks};
use steel_utils::axis::Axis;
use steel_utils::types::UpdateFlags;
use steel_utils::{
    BlockLocalAabb, BlockPos, BlockStateId, DowncastType, DowncastTypeKey, WorldAabb,
    locks::SyncMutex,
};

use crate::behavior::{BLOCK_BEHAVIORS, BlockCollisionBoxes, BlockCollisionContext};
use crate::block_entity::block_state_nbt;
use crate::block_entity::{BlockEntity, BlockEntityBase, BlockEntityLifecycleExt as _};
use crate::entity::Entity;
use crate::physics::MoverType;
use crate::world::{LevelReader, World};

const PUSH_OFFSET: f64 = 0.01;

thread_local! {
    static NOCLIP: Cell<Option<Direction>> = const { Cell::new(None) };
}

struct NoClipGuard;

impl NoClipGuard {
    fn set(direction: Direction) -> Self {
        NOCLIP.set(Some(direction));
        Self
    }
}

impl Drop for NoClipGuard {
    fn drop(&mut self) {
        NOCLIP.set(None);
    }
}

/// Vanilla `PistonMovingBlockEntity`.
pub struct PistonMovingBlockEntity {
    base: BlockEntityBase,
    moving: SyncMutex<PistonMovingState>,
}

#[derive(Clone, Copy)]
struct PistonMovingState {
    moved_state: BlockStateId,
    direction: Direction,
    extending: bool,
    source_piston: bool,
    progress: f32,
    progress_o: f32,
    last_ticked: i64,
}

// SAFETY: This key is owned by Steel and uniquely identifies `PistonMovingBlockEntity`.
unsafe impl DowncastType for PistonMovingBlockEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:block_entity/piston_moving");
}

impl PistonMovingBlockEntity {
    /// Creates the default instance used while loading a piston block entity.
    #[must_use]
    pub fn new(world: Weak<World>, pos: BlockPos, state: BlockStateId) -> Self {
        Self::new_moving(
            world,
            pos,
            state,
            vanilla_blocks::AIR.default_state(),
            Direction::Down,
            false,
            false,
        )
    }

    /// Creates a moving block or source-piston entity.
    #[must_use]
    pub fn new_moving(
        world: Weak<World>,
        pos: BlockPos,
        state: BlockStateId,
        moved_state: BlockStateId,
        direction: Direction,
        extending: bool,
        source_piston: bool,
    ) -> Self {
        Self {
            base: BlockEntityBase::new(&vanilla_block_entity_types::PISTON, world, pos, state),
            moving: SyncMutex::new(PistonMovingState {
                moved_state,
                direction,
                extending,
                source_piston,
                progress: 0.0,
                progress_o: 0.0,
                last_ticked: 0,
            }),
        }
    }

    /// Returns whether this block is extending.
    #[must_use]
    pub fn is_extending(&self) -> bool {
        self.moving.lock().extending
    }

    /// Returns the piston facing direction.
    #[must_use]
    pub fn direction(&self) -> Direction {
        self.moving.lock().direction
    }

    /// Returns whether this entity represents the source piston or its head.
    #[must_use]
    pub fn is_source_piston(&self) -> bool {
        self.moving.lock().source_piston
    }

    /// Returns the state being moved.
    #[must_use]
    pub fn moved_state(&self) -> BlockStateId {
        self.moving.lock().moved_state
    }

    /// Returns the last game time at which this entity ticked.
    #[must_use]
    pub fn last_ticked(&self) -> i64 {
        self.moving.lock().last_ticked
    }

    /// Returns interpolated movement progress.
    #[must_use]
    pub fn progress(&self, partial_tick: f32) -> f32 {
        let partial_tick = partial_tick.min(1.0);
        let moving = self.moving.lock();
        (moving.progress - moving.progress_o).mul_add(partial_tick, moving.progress_o)
    }

    /// Returns the movement direction, which reverses while retracting.
    #[must_use]
    pub fn movement_direction(&self) -> Direction {
        self.moving.lock().movement_direction()
    }

    /// Returns the direction used for the final neighbor notification.
    #[must_use]
    pub fn push_direction(&self) -> Direction {
        self.moving.lock().movement_direction()
    }

    /// Resolves the transient collision boxes at the current progress.
    #[must_use]
    pub fn collision_boxes(&self, world: &dyn LevelReader, pos: BlockPos) -> BlockCollisionBoxes {
        let moving = *self.moving.lock();
        moving.collision_boxes(world, pos)
    }

    /// Completes an in-flight moving block before its piston starts retracting.
    pub fn final_tick(&self, world: &Arc<World>) -> bool {
        let moving = {
            let mut moving = self.moving.lock();
            if moving.progress_o >= 1.0 {
                return false;
            }
            moving.progress = 1.0;
            moving.progress_o = 1.0;
            *moving
        };
        let pos = self.get_block_pos();
        let owned_position = world.remove_block_entity_if_same(self);
        self.set_removed();
        if owned_position {
            moving.finish_movement_early(world, pos);
        }
        true
    }
}

impl PistonMovingState {
    // Vanilla's two-argument `EntityGetter.getEntities` applies
    // `EntitySelector.NO_SPECTATORS` before the piston loop.
    fn can_move_collided_entity(entity: &dyn Entity, cause_bounce: bool) -> bool {
        !entity.is_spectator()
            && entity.piston_push_reaction() != PushReaction::Ignore
            && (!cause_bounce || entity.as_player().is_none())
    }

    const fn movement_direction(self) -> Direction {
        if self.extending {
            self.direction
        } else {
            self.direction.opposite()
        }
    }

    fn extended_progress(self, progress: f32) -> f32 {
        if self.extending {
            progress - 1.0
        } else {
            1.0 - progress
        }
    }

    fn collision_related_state(self) -> BlockStateId {
        let behavior = BLOCK_BEHAVIORS.get_behavior(self.moved_state.get_block());
        if !self.extending && self.source_piston && behavior.is_piston_base() {
            vanilla_blocks::PISTON_HEAD
                .default_state()
                .set_value(&BlockStateProperties::SHORT, self.progress > 0.25)
                .set_value(
                    &BlockStateProperties::PISTON_TYPE,
                    if self.moved_state.get_block() == &vanilla_blocks::STICKY_PISTON {
                        PistonType::Sticky
                    } else {
                        PistonType::Normal
                    },
                )
                .set_value(
                    &BlockStateProperties::FACING,
                    self.moved_state.get_value(&BlockStateProperties::FACING),
                )
        } else {
            self.moved_state
        }
    }

    fn state_collision_boxes(
        state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
    ) -> BlockCollisionBoxes {
        BLOCK_BEHAVIORS
            .get_behavior(state.get_block())
            .get_collision_boxes(state, world, pos, BlockCollisionContext::empty())
    }

    fn boxes_bounds(boxes: &BlockCollisionBoxes) -> Option<BlockLocalAabb> {
        let mut boxes = boxes.iter().filter(|aabb| !aabb.is_empty());
        let mut bounds = *boxes.next()?;
        for aabb in boxes {
            bounds = BlockLocalAabb::encapsulating(&bounds, aabb);
        }
        Some(bounds)
    }

    /// Resolves the transient collision boxes at the current progress.
    #[must_use]
    pub fn collision_boxes(&self, world: &dyn LevelReader, pos: BlockPos) -> BlockCollisionBoxes {
        let mut result = BlockCollisionBoxes::new();
        let moved_behavior = BLOCK_BEHAVIORS.get_behavior(self.moved_state.get_block());
        if !self.extending && self.source_piston && moved_behavior.is_piston_base() {
            let extended = self
                .moved_state
                .set_value(&BlockStateProperties::EXTENDED, true);
            result.extend(Self::state_collision_boxes(extended, world, pos));
        }

        let no_clip_direction = NOCLIP.get();
        if self.progress < 1.0 && no_clip_direction == Some(self.movement_direction()) {
            return result;
        }

        let moving_state = if self.source_piston {
            vanilla_blocks::PISTON_HEAD
                .default_state()
                .set_value(&BlockStateProperties::FACING, self.direction)
                .set_value(
                    &BlockStateProperties::SHORT,
                    self.extending != ((1.0 - self.progress) < 0.25),
                )
        } else {
            self.moved_state
        };
        let amount = f64::from(self.extended_progress(self.progress));
        let (x, y, z) = self.direction.offset();
        let offset = DVec3::new(
            f64::from(x) * amount,
            f64::from(y) * amount,
            f64::from(z) * amount,
        );
        result.extend(
            Self::state_collision_boxes(moving_state, world, pos)
                .into_iter()
                .map(|aabb| aabb.translate(offset)),
        );
        result
    }

    fn move_by_position_and_progress(&self, pos: BlockPos, aabb: BlockLocalAabb) -> WorldAabb {
        let amount = f64::from(self.extended_progress(self.progress));
        let (x, y, z) = self.direction.offset();
        aabb.at_block(pos).translate(DVec3::new(
            f64::from(x) * amount,
            f64::from(y) * amount,
            f64::from(z) * amount,
        ))
    }

    fn movement_area(aabb: WorldAabb, direction: Direction, amount: f64) -> WorldAabb {
        let signed_amount = if matches!(
            direction,
            Direction::West | Direction::Down | Direction::North
        ) {
            -amount
        } else {
            amount
        };
        let min = signed_amount.min(0.0);
        let max = signed_amount.max(0.0);
        match direction {
            Direction::West => WorldAabb::new(
                aabb.min_x() + min,
                aabb.min_y(),
                aabb.min_z(),
                aabb.min_x() + max,
                aabb.max_y(),
                aabb.max_z(),
            ),
            Direction::East => WorldAabb::new(
                aabb.max_x() + min,
                aabb.min_y(),
                aabb.min_z(),
                aabb.max_x() + max,
                aabb.max_y(),
                aabb.max_z(),
            ),
            Direction::Down => WorldAabb::new(
                aabb.min_x(),
                aabb.min_y() + min,
                aabb.min_z(),
                aabb.max_x(),
                aabb.min_y() + max,
                aabb.max_z(),
            ),
            Direction::Up => WorldAabb::new(
                aabb.min_x(),
                aabb.max_y() + min,
                aabb.min_z(),
                aabb.max_x(),
                aabb.max_y() + max,
                aabb.max_z(),
            ),
            Direction::North => WorldAabb::new(
                aabb.min_x(),
                aabb.min_y(),
                aabb.min_z() + min,
                aabb.max_x(),
                aabb.max_y(),
                aabb.min_z() + max,
            ),
            Direction::South => WorldAabb::new(
                aabb.min_x(),
                aabb.min_y(),
                aabb.max_z() + min,
                aabb.max_x(),
                aabb.max_y(),
                aabb.max_z() + max,
            ),
        }
    }

    fn overlap_movement(outside: WorldAabb, movement: Direction, entity: WorldAabb) -> f64 {
        match movement {
            Direction::East => outside.max_x() - entity.min_x(),
            Direction::West => entity.max_x() - outside.min_x(),
            Direction::Up => outside.max_y() - entity.min_y(),
            Direction::Down => entity.max_y() - outside.min_y(),
            Direction::South => outside.max_z() - entity.min_z(),
            Direction::North => entity.max_z() - outside.min_z(),
        }
    }

    fn move_entity_by_piston(
        piston_direction: Direction,
        entity: &dyn Entity,
        delta: f64,
        movement: Direction,
    ) {
        let _no_clip = NoClipGuard::set(piston_direction);
        let (x, y, z) = movement.offset();
        let previous_position = entity.position();
        entity.move_entity(
            MoverType::Piston,
            DVec3::new(
                delta * f64::from(x),
                delta * f64::from(y),
                delta * f64::from(z),
            ),
        );
        entity.apply_effects_from_blocks_between(previous_position, entity.position());
        entity.remove_latest_movement_recording();
    }

    fn fix_entity_within_piston_base(
        pos: BlockPos,
        entity: &dyn Entity,
        direction: Direction,
        delta_progress: f64,
    ) {
        let entity_aabb = entity.bounding_box();
        let box_at_pos = BlockLocalAabb::FULL_BLOCK.at_block(pos);
        if !entity_aabb.intersects(box_at_pos) {
            return;
        }

        let opposite = direction.opposite();
        let delta = Self::overlap_movement(box_at_pos, opposite, entity_aabb) + PUSH_OFFSET;
        let intersection = WorldAabb::new(
            entity_aabb.min_x().max(box_at_pos.min_x()),
            entity_aabb.min_y().max(box_at_pos.min_y()),
            entity_aabb.min_z().max(box_at_pos.min_z()),
            entity_aabb.max_x().min(box_at_pos.max_x()),
            entity_aabb.max_y().min(box_at_pos.max_y()),
            entity_aabb.max_z().min(box_at_pos.max_z()),
        );
        let intersected_delta =
            Self::overlap_movement(box_at_pos, opposite, intersection) + PUSH_OFFSET;
        if (delta - intersected_delta).abs() < PUSH_OFFSET {
            let delta = delta.min(delta_progress) + PUSH_OFFSET;
            Self::move_entity_by_piston(direction, entity, delta, opposite);
        }
    }

    fn move_collided_entities(self, world: &Arc<World>, pos: BlockPos, new_progress: f32) {
        let movement = self.movement_direction();
        let delta_progress = f64::from(new_progress - self.progress);
        let shape =
            Self::state_collision_boxes(self.collision_related_state(), world.as_ref(), pos);
        let Some(bounds) = Self::boxes_bounds(&shape) else {
            return;
        };
        let aabb = self.move_by_position_and_progress(pos, bounds);
        let query =
            WorldAabb::encapsulating(&Self::movement_area(aabb, movement, delta_progress), &aabb);
        let entities = world.get_entities_in_aabb(&query);
        let cause_bounce = self.moved_state.get_block() == &vanilla_blocks::SLIME_BLOCK;

        for entity in entities {
            if !Self::can_move_collided_entity(entity.as_ref(), cause_bounce) {
                continue;
            }
            if cause_bounce {
                let mut velocity = entity.velocity();
                let (x, y, z) = movement.offset();
                match movement.axis() {
                    Axis::X => velocity.x = f64::from(x),
                    Axis::Y => velocity.y = f64::from(y),
                    Axis::Z => velocity.z = f64::from(z),
                }
                entity.set_velocity(velocity);
            }

            let mut delta: f64 = 0.0;
            let entity_aabb = entity.bounding_box();
            for shape_aabb in &shape {
                let moving_aabb = Self::movement_area(
                    self.move_by_position_and_progress(pos, *shape_aabb),
                    movement,
                    delta_progress,
                );
                if moving_aabb.intersects(entity_aabb) {
                    delta = delta.max(Self::overlap_movement(moving_aabb, movement, entity_aabb));
                    if delta >= delta_progress {
                        break;
                    }
                }
            }

            if delta <= 0.0 {
                continue;
            }
            let delta = delta.min(delta_progress) + PUSH_OFFSET;
            Self::move_entity_by_piston(movement, entity.as_ref(), delta, movement);
            if !self.extending && self.source_piston {
                Self::fix_entity_within_piston_base(pos, entity.as_ref(), movement, delta_progress);
            }
        }
    }

    fn move_stuck_entities(self, world: &Arc<World>, pos: BlockPos, new_progress: f32) {
        if self.moved_state.get_block() != &vanilla_blocks::HONEY_BLOCK {
            return;
        }
        let movement = self.movement_direction();
        if !movement.is_horizontal() {
            return;
        }

        let collision = Self::state_collision_boxes(self.moved_state, world.as_ref(), pos);
        let sticky_top = collision
            .iter()
            .map(BlockLocalAabb::max_y)
            .fold(f64::NEG_INFINITY, f64::max);
        let local = BlockLocalAabb::new(0.0, sticky_top, 0.0, 1.0, 1.500_001, 1.0);
        let aabb = self.move_by_position_and_progress(pos, local);
        let entities = world.get_entities_in_aabb_matching(&aabb, |entity| {
            let position = entity.position();
            entity.piston_push_reaction() == PushReaction::Normal
                && entity.on_ground()
                && (entity.is_supported_by(pos)
                    || (position.x >= aabb.min_x()
                        && position.x <= aabb.max_x()
                        && position.z >= aabb.min_z()
                        && position.z <= aabb.max_z()))
        });
        let delta_progress = f64::from(new_progress - self.progress);
        for entity in entities {
            Self::move_entity_by_piston(movement, entity.as_ref(), delta_progress, movement);
        }
    }

    fn finish_tick(self, world: &Arc<World>, pos: BlockPos) {
        if world.get_block_state(pos).get_block() != &vanilla_blocks::MOVING_PISTON {
            return;
        }

        let mut new_state = world.update_from_neighbor_shapes(self.moved_state, pos);
        if new_state.get_block() == &vanilla_blocks::AIR {
            world.set_block(
                pos,
                self.moved_state,
                UpdateFlags::UPDATE_INVISIBLE
                    | UpdateFlags::UPDATE_KNOWN_SHAPE
                    | UpdateFlags::UPDATE_MOVE_BY_PISTON
                    | UpdateFlags::UPDATE_SKIP_BLOCK_ENTITY_SIDEEFFECTS,
            );
            world.update_or_destroy(
                self.moved_state,
                new_state,
                pos,
                UpdateFlags::UPDATE_ALL,
                512,
            );
            return;
        }

        if new_state.try_get_value(&BlockStateProperties::WATERLOGGED) == Some(true) {
            new_state = new_state.set_value(&BlockStateProperties::WATERLOGGED, false);
        }
        world.set_block(
            pos,
            new_state,
            UpdateFlags::UPDATE_ALL | UpdateFlags::UPDATE_MOVE_BY_PISTON,
        );
        world.neighbor_changed(pos, new_state.get_block());
    }

    fn finish_movement_early(self, world: &Arc<World>, pos: BlockPos) {
        if world.get_block_state(pos).get_block() == &vanilla_blocks::MOVING_PISTON {
            let new_state = if self.source_piston {
                vanilla_blocks::AIR.default_state()
            } else {
                world.update_from_neighbor_shapes(self.moved_state, pos)
            };
            world.set_block(pos, new_state, UpdateFlags::UPDATE_ALL);
            world.neighbor_changed(pos, new_state.get_block());
        }
    }

    const fn direction_from_legacy_id(id: i8) -> Direction {
        let remainder = (id as i32) % 6;
        let normalized = if remainder < 0 { -remainder } else { remainder };
        match normalized {
            1 => Direction::Up,
            2 => Direction::North,
            3 => Direction::South,
            4 => Direction::West,
            5 => Direction::East,
            _ => Direction::Down,
        }
    }

    const fn direction_legacy_id(direction: Direction) -> i8 {
        match direction {
            Direction::Down => 0,
            Direction::Up => 1,
            Direction::North => 2,
            Direction::South => 3,
            Direction::West => 4,
            Direction::East => 5,
        }
    }
}

impl BlockEntity for PistonMovingBlockEntity {
    fn base(&self) -> &BlockEntityBase {
        &self.base
    }

    fn pre_remove_side_effects(&self, _pos: BlockPos, _state: BlockStateId) {
        if let Some(world) = self.get_level() {
            self.final_tick(&world);
        }
    }

    fn load_additional(&self, nbt: &BorrowedNbtCompound<'_>) {
        let view = NbtCompoundView::from(nbt);
        let mut moving = self.moving.lock();
        moving.moved_state = view
            .compound("blockState")
            .and_then(block_state_nbt::load)
            .unwrap_or_else(|| vanilla_blocks::AIR.default_state());
        moving.direction =
            PistonMovingState::direction_from_legacy_id(view.byte("facing").unwrap_or(0));
        moving.progress = view.float("progress").unwrap_or(0.0);
        moving.progress_o = moving.progress;
        moving.extending = view.byte("extending").is_some_and(|value| value != 0);
        moving.source_piston = view.byte("source").is_some_and(|value| value != 0);
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        let moving = self.moving.lock();
        nbt.insert("blockState", block_state_nbt::save(moving.moved_state));
        nbt.insert(
            "facing",
            PistonMovingState::direction_legacy_id(moving.direction),
        );
        nbt.insert("progress", moving.progress_o);
        nbt.insert("extending", i8::from(moving.extending));
        nbt.insert("source", i8::from(moving.source_piston));
    }

    fn get_update_tag(&self) -> Option<NbtCompound> {
        Some(self.save_custom_only())
    }

    fn tick(&self, world: &Arc<World>) {
        let game_time = world.game_time();
        let (moving, new_progress) = {
            let mut moving = self.moving.lock();
            moving.last_ticked = game_time;
            moving.progress_o = moving.progress;
            let new_progress = (moving.progress_o < 1.0).then(|| moving.progress + 0.5);
            (*moving, new_progress)
        };
        let pos = self.get_block_pos();
        let Some(new_progress) = new_progress else {
            let owned_position = world.remove_block_entity_if_same(self);
            self.set_removed();
            if owned_position {
                moving.finish_tick(world, pos);
            }
            return;
        };

        moving.move_collided_entities(world, pos, new_progress);
        moving.move_stuck_entities(world, pos, new_progress);
        self.moving.lock().progress = new_progress.min(1.0);
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use glam::DVec3;
    use simdnbt::borrow::read_compound as read_borrowed_compound;
    use simdnbt::owned::NbtTag;
    use steel_protocol::packet_traits::{CompressionInfo, EncodedPacket};
    use steel_registry::{test_support::init_test_registry, vanilla_entities};
    use steel_utils::{ChunkPos, types::GameType};
    use text_components::TextComponent;
    use uuid::Uuid;

    use super::*;
    use crate::behavior::init_behaviors;
    use crate::block_entity::SharedBlockEntity;
    use crate::config::RuntimeConfig;
    use crate::entity::{SharedEntity, entities::RawEntity};
    use crate::player::connection::NetworkConnection;
    use crate::player::{ClientInformation, GameProfile, Player, PlayerConnection};
    use crate::server::Server;
    use crate::test_support::{fresh_test_world, insert_ready_full_chunk};

    struct TestConnection;

    impl NetworkConnection for TestConnection {
        fn compression(&self) -> Option<CompressionInfo> {
            None
        }

        fn send_encoded(&self, _packet: EncodedPacket) {}

        fn send_encoded_bundle(&self, _packets: Vec<EncodedPacket>) {}

        fn disconnect_with_reason(&self, _reason: TextComponent) {}

        fn tick(&self) {}

        fn latency(&self) -> i32 {
            0
        }

        fn close(&self) {}

        fn closed(&self) -> bool {
            false
        }
    }

    fn test_player(world: Arc<World>) -> Arc<Player> {
        let connection = Arc::new(PlayerConnection::Other(Box::new(TestConnection)));
        let config = Arc::new(RuntimeConfig {
            max_players: 1,
            view_distance: 2,
            simulation_distance: 2,
            max_chained_neighbor_updates: 1_000_000,
            online_mode: false,
            auth_server: None,
            profile_server: None,
            encryption: false,
            allow_flight: false,
            motd: String::new(),
            use_favicon: false,
            favicon: String::new(),
            enforce_secure_chat: false,
            chat_spam_threshold_seconds: 10,
            command_spam_threshold_seconds: 10,
            compression: None,
            server_links: None,
            packet_workers: Some(1),
            chunk_generation_threads: Some(1),
            chunk_encoding_threads: Some(1),
        });
        Arc::new_cyclic(|weak_player| {
            Player::new(
                GameProfile {
                    id: Uuid::from_u128(1),
                    name: "PistonTestPlayer".to_owned(),
                    properties: Vec::new(),
                    profile_actions: None,
                },
                connection,
                world,
                Weak::<Server>::new(),
                config,
                1,
                weak_player,
                ClientInformation::default(),
            )
        })
    }

    #[test]
    fn moving_state_and_progress_round_trip_with_vanilla_keys() {
        init_test_registry();
        let state = vanilla_blocks::MOVING_PISTON
            .default_state()
            .set_value(&BlockStateProperties::FACING, Direction::West)
            .set_value(&BlockStateProperties::PISTON_TYPE, PistonType::Sticky);
        let moved = vanilla_blocks::PISTON_HEAD
            .default_state()
            .set_value(&BlockStateProperties::FACING, Direction::West)
            .set_value(&BlockStateProperties::PISTON_TYPE, PistonType::Sticky)
            .set_value(&BlockStateProperties::SHORT, true);
        let source = PistonMovingBlockEntity::new_moving(
            Weak::new(),
            BlockPos::new(8, 64, -3),
            state,
            moved,
            Direction::West,
            true,
            true,
        );
        source.moving.lock().progress_o = 0.5;

        let mut nbt = NbtCompound::new();
        source.save_additional(&mut nbt);
        assert!(matches!(nbt.get("facing"), Some(NbtTag::Byte(4))));
        let mut bytes = Vec::new();
        nbt.write(&mut bytes);
        let borrowed = read_borrowed_compound(&mut Cursor::new(bytes.as_slice()))
            .expect("test NBT should reborrow");
        let view = NbtCompoundView::from(&borrowed);
        assert_eq!(view.byte("facing"), Some(4));
        assert_eq!(view.int("facing"), None);

        let loaded = PistonMovingBlockEntity::new(Weak::new(), source.get_block_pos(), state);
        loaded.load_additional(&borrowed);
        assert_eq!(loaded.moved_state(), moved);
        assert_eq!(loaded.direction(), Direction::West);
        assert!(loaded.is_extending());
        assert!(loaded.is_source_piston());
        assert!((loaded.progress(0.0) - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn collided_entity_filter_matches_vanilla_player_and_spectator_rules() {
        init_test_registry();
        init_behaviors();
        let world = fresh_test_world("piston_entity_filter");
        let player = test_player(Arc::clone(&world));

        assert!(PistonMovingState::can_move_collided_entity(
            player.as_ref(),
            false
        ));
        assert!(!PistonMovingState::can_move_collided_entity(
            player.as_ref(),
            true
        ));

        player.restore_game_modes(GameType::Spectator, None);
        assert!(!PistonMovingState::can_move_collided_entity(
            player.as_ref(),
            false
        ));

        let raw = RawEntity::new(
            8_000,
            DVec3::ZERO,
            Arc::downgrade(&world),
            &vanilla_entities::MINECART,
        );
        assert!(PistonMovingState::can_move_collided_entity(&raw, true));
    }

    #[test]
    fn movement_area_matches_vanilla_directional_sweep() {
        let aabb = WorldAabb::new(1.0, 2.0, 3.0, 2.0, 3.0, 4.0);
        assert_eq!(
            PistonMovingState::movement_area(aabb, Direction::East, 0.5),
            WorldAabb::new(2.0, 2.0, 3.0, 2.5, 3.0, 4.0)
        );
        assert_eq!(
            PistonMovingState::movement_area(aabb, Direction::North, 0.5),
            WorldAabb::new(1.0, 2.0, 2.5, 2.0, 3.0, 3.0)
        );
    }

    #[test]
    fn piston_entity_move_can_reenter_moving_block_collision() {
        init_test_registry();
        init_behaviors();
        let world = fresh_test_world("piston_collision_reentry");
        let pos = BlockPos::new(8, 64, 8);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));
        let state = vanilla_blocks::MOVING_PISTON
            .default_state()
            .set_value(&BlockStateProperties::FACING, Direction::East);
        assert!(world.set_block(pos, state, UpdateFlags::UPDATE_NONE));

        let piston = Arc::new(PistonMovingBlockEntity::new_moving(
            Arc::downgrade(&world),
            pos,
            state,
            vanilla_blocks::STONE.default_state(),
            Direction::East,
            true,
            false,
        ));
        let block_entity: SharedBlockEntity = piston.clone();
        assert!(world.set_block_entity(block_entity));

        let start = DVec3::new(f64::from(pos.x()) + 0.1, f64::from(pos.y()), 8.5);
        let entity: SharedEntity = Arc::new(RawEntity::new(
            8_001,
            start,
            Arc::downgrade(&world),
            &vanilla_entities::MINECART,
        ));
        world
            .try_add_entity(Arc::clone(&entity))
            .expect("test entity should enter the loaded chunk");

        piston.tick(&world);

        assert!((piston.progress(1.0) - 0.5).abs() < f32::EPSILON);
        assert!(entity.position().x > start.x);
    }

    #[test]
    fn final_tick_marks_a_detached_moving_entity_removed() {
        init_test_registry();
        init_behaviors();
        let world = fresh_test_world("detached_piston_final_tick");
        let pos = BlockPos::new(8, 64, 8);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));
        let state = vanilla_blocks::MOVING_PISTON
            .default_state()
            .set_value(&BlockStateProperties::FACING, Direction::East);
        let piston = PistonMovingBlockEntity::new_moving(
            Arc::downgrade(&world),
            pos,
            state,
            vanilla_blocks::STONE.default_state(),
            Direction::East,
            true,
            false,
        );

        assert!(piston.final_tick(&world));
        assert!(piston.is_removed());
    }

    #[test]
    fn stale_final_tick_cannot_remove_or_finish_a_replacement() {
        init_test_registry();
        init_behaviors();
        let world = fresh_test_world("stale_piston_final_tick");
        let pos = BlockPos::new(8, 64, 8);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));
        let state = vanilla_blocks::MOVING_PISTON
            .default_state()
            .set_value(&BlockStateProperties::FACING, Direction::East);
        assert!(world.set_block(pos, state, UpdateFlags::UPDATE_NONE));
        let stale_piston = Arc::new(PistonMovingBlockEntity::new_moving(
            Arc::downgrade(&world),
            pos,
            state,
            vanilla_blocks::STONE.default_state(),
            Direction::East,
            true,
            false,
        ));
        let stale_entity: SharedBlockEntity = stale_piston.clone();
        assert!(world.set_block_entity(stale_entity));
        let replacement = Arc::new(PistonMovingBlockEntity::new_moving(
            Arc::downgrade(&world),
            pos,
            state,
            vanilla_blocks::GOLD_BLOCK.default_state(),
            Direction::East,
            true,
            false,
        ));
        let replacement_entity: SharedBlockEntity = replacement.clone();
        assert!(world.set_block_entity(Arc::clone(&replacement_entity)));

        assert!(stale_piston.final_tick(&world));

        let Some(current) = world.get_block_entity(pos) else {
            panic!("the replacement should remain stored");
        };
        assert!(Arc::ptr_eq(&current, &replacement_entity));
        assert_eq!(world.get_block_state(pos), state);
        assert!(!replacement.is_removed());
    }
}
