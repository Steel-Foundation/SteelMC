//! Item-frame persistence, interaction, attachment geometry, and map markers.

use std::sync::Weak;

use glam::DVec3;
use steel_macros::entity_behavior;
use steel_math::DEGREE_90;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::data_components::vanilla_components::{CUSTOM_NAME, MAP_ID};
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_entity_data::ItemFrameEntityData;
use steel_registry::vanilla_game_rules::ENTITY_DROPS;
use steel_registry::{
    sound_events, vanilla_blocks, vanilla_entities, vanilla_game_events, vanilla_items,
};
use steel_utils::locks::SyncMutex;
use steel_utils::{BlockPos, Direction, DowncastType, DowncastTypeKey, WorldAabb, axis::Axis};

use crate::entity::block_attached_entity::{BlockAttachedEntity, BlockAttachedEntityBase};
use crate::entity::{
    Entity, EntityBase, EntityBaseLoad, EntityBaseState, EntityMoveError, HangingEntity, ItemFrame,
};
use crate::world::World;
use crate::{behavior::BLOCK_BEHAVIORS, maps::MapFrame, player::Player};

/// Displayed items, including map hit bounds, shared markers, and comparator output.
#[entity_behavior(class = "ItemFrame")]
pub struct ItemFrameEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    entity_data: SyncMutex<ItemFrameEntityData>,
    block_attached_entity_base: BlockAttachedEntityBase,
    frame_state: SyncMutex<FrameState>,
}

struct FrameState {
    fixed: bool,
    drop_chance: f32,
}

impl Default for FrameState {
    fn default() -> Self {
        Self {
            fixed: false,
            drop_chance: 1.0,
        }
    }
}

// SAFETY: This key is owned by Steel and uniquely identifies `ItemFrameEntity`.
unsafe impl DowncastType for ItemFrameEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/item_frame");
}

impl ItemFrameEntity {
    /// Creates a fresh item frame from the generic entity factory path.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self::new_attached(
            entity_type,
            id,
            BlockPos::new(
                position.x.floor() as i32,
                position.y.floor() as i32,
                position.z.floor() as i32,
            ),
            Direction::South,
            world,
        )
    }

    /// Creates a fresh item frame attached to `block_pos`.
    #[must_use]
    pub fn new_attached(
        entity_type: EntityTypeRef,
        id: i32,
        block_pos: BlockPos,
        direction: Direction,
        world: Weak<World>,
    ) -> Self {
        let entity = Self {
            base: EntityBase::new_with_state(
                id,
                EntityBaseState::new_with_bounding_box(
                    Self::frame_center(block_pos, direction),
                    entity_type.dimensions,
                    Self::frame_bounding_box(block_pos, direction, false),
                )
                .with_rotation(Self::rotation_for_direction(direction)),
                world,
            ),
            entity_type,
            entity_data: SyncMutex::new(ItemFrameEntityData::new()),
            block_attached_entity_base: BlockAttachedEntityBase::new(block_pos),
            frame_state: SyncMutex::new(FrameState::default()),
        };
        entity
            .entity_data
            .lock()
            .hanging_entity
            .direction
            .set(direction);
        entity
    }

    /// Creates an item frame from persistent entity data.
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        let position = load.position;
        Self {
            base: EntityBase::from_load(load, entity_type.dimensions),
            entity_type,
            entity_data: SyncMutex::new(ItemFrameEntityData::new()),
            block_attached_entity_base: BlockAttachedEntityBase::new(BlockPos::new(
                position.x.floor() as i32,
                position.y.floor() as i32,
                position.z.floor() as i32,
            )),
            frame_state: SyncMutex::new(FrameState::default()),
        }
    }

    /// Sets the framed item, matching vanilla by storing a single item.
    pub fn set_item(&self, item: ItemStack) {
        self.set_item_with_update(item, true);
    }

    /// Sets the framed item and optionally notifies nearby comparators.
    pub(crate) fn set_item_with_update(&self, mut item: ItemStack, update_comparators: bool) {
        if !item.is_empty() {
            item.set_count(1);
        }
        let has_item = !item.is_empty();
        self.entity_data.lock().item.set(item);
        self.recalculate_position_or_warn();
        if has_item {
            self.play_sound(
                if self.entity_type == &vanilla_entities::GLOW_ITEM_FRAME {
                    &sound_events::ENTITY_GLOW_ITEM_FRAME_ADD_ITEM
                } else {
                    &sound_events::ENTITY_ITEM_FRAME_ADD_ITEM
                },
                1.0,
                1.0,
            );
        }
        if update_comparators && let Some(world) = self.level() {
            world.update_neighbor_for_output_signal(
                self.block_attached_entity_base.pos(),
                &vanilla_blocks::AIR,
            );
        }
    }

    /// Sets the displayed rotation, preserving Java's signed remainder behavior.
    pub fn set_rotation(&self, rotation: i32) {
        self.entity_data.lock().rotation.set(rotation % 8);
        if let Some(world) = self.level() {
            world.update_neighbor_for_output_signal(
                self.block_attached_entity_base.pos(),
                &vanilla_blocks::AIR,
            );
        }
    }

    fn remove_framed_map(&self, stack: &ItemStack) {
        let Some(world) = self.level() else { return };
        let Some(id) = stack.get(MAP_ID) else { return };
        let Some(data) = world.get_map_data(*id) else {
            return;
        };
        data.lock().removed_from_frame(&MapFrame {
            pos: self.block_attached_entity_base.pos(),
            rotation: self.direction().get_2d_data_value() * 90,
            entity_id: self.id(),
        });
    }

    fn frame_item(&self) -> ItemStack {
        let mut stack = ItemStack::new(if self.entity_type == &vanilla_entities::GLOW_ITEM_FRAME {
            &vanilla_items::GLOW_ITEM_FRAME
        } else {
            &vanilla_items::ITEM_FRAME
        });
        if let Some(name) = self.custom_name() {
            stack.set(CUSTOM_NAME, name);
        }
        stack
    }

    fn drop_contents(&self, caused_by: Option<&dyn Entity>, with_frame: bool) {
        let state = self.frame_state.lock();
        if state.fixed {
            return;
        }
        let chance = state.drop_chance;
        drop(state);
        let Some(world) = self.level() else { return };
        let item = self.item();
        self.set_item(ItemStack::empty());
        if !world.get_game_rule(&ENTITY_DROPS) {
            if caused_by.is_none() {
                self.remove_framed_map(&item);
            }
            return;
        }
        if caused_by
            .and_then(Entity::as_player)
            .is_some_and(Player::has_infinite_materials)
        {
            self.remove_framed_map(&item);
            return;
        }
        if with_frame {
            self.spawn_at_location(self.frame_item(), 0.0);
        }
        if !item.is_empty() {
            self.remove_framed_map(&item);
            if rand::random::<f32>() < chance {
                self.spawn_at_location(item, 0.0);
            }
        }
    }

    fn set_direction(&self, direction: Direction) {
        self.entity_data
            .lock()
            .hanging_entity
            .direction
            .set(direction);
        self.base
            .set_rotation(Self::rotation_for_direction(direction));
        self.recalculate_position_or_warn();
    }

    fn try_recalculate_position(&self) -> Result<(), EntityMoveError> {
        let block_pos = self.block_attached_entity_base.pos();
        let direction = *self.entity_data.lock().hanging_entity.direction.get();
        let position = Self::frame_center(block_pos, direction);
        self.base.try_set_position(position)?;
        self.base.set_bounding_box(Self::frame_bounding_box(
            block_pos,
            direction,
            self.has_framed_map(),
        ));

        Ok(())
    }

    fn recalculate_position_or_warn(&self) {
        if let Err(error) = self.try_recalculate_position() {
            log::warn!(
                "failed to commit item frame {} position recalculation: {error}",
                self.base.id()
            );
        }
    }

    fn has_framed_map(&self) -> bool {
        self.entity_data.lock().item.get().has(MAP_ID)
    }

    fn frame_center(block_pos: BlockPos, direction: Direction) -> DVec3 {
        let off = direction.offset_vec().as_dvec3() * 0.46875;
        block_pos.0.as_dvec3() + DVec3::splat(0.5) - off
    }

    fn rotation_for_direction(direction: Direction) -> (f32, f32) {
        if direction.is_horizontal() {
            (
                f32::from(direction_2d_data_value(direction)) * DEGREE_90,
                0.0,
            )
        } else {
            let pitch = match direction {
                Direction::Up => -DEGREE_90,
                Direction::Down => DEGREE_90,
                Direction::North | Direction::South | Direction::West | Direction::East => 0.0,
            };
            (0.0, pitch)
        }
    }

    fn frame_bounding_box(
        block_pos: BlockPos,
        direction: Direction,
        has_framed_map: bool,
    ) -> WorldAabb {
        let center = Self::frame_center(block_pos, direction);
        let size = if has_framed_map { 1.0 } else { 0.75 };
        let x_size = if direction.axis() == Axis::X {
            0.0625
        } else {
            size
        };
        let y_size = if direction.axis() == Axis::Y {
            0.0625
        } else {
            size
        };
        let z_size = if direction.axis() == Axis::Z {
            0.0625
        } else {
            size
        };
        WorldAabb::new(
            center.x - x_size / 2.0,
            center.y - y_size / 2.0,
            center.z - z_size / 2.0,
            center.x + x_size / 2.0,
            center.y + y_size / 2.0,
            center.z + z_size / 2.0,
        )
    }
}

impl HangingEntity for ItemFrameEntity {
    fn direction(&self) -> Direction {
        *self.entity_data.lock().hanging_entity.direction.get()
    }

    fn pop_box(&self) -> WorldAabb {
        Self::frame_bounding_box(
            self.block_attached_entity_base.pos(),
            self.direction(),
            false,
        )
    }
}

impl ItemFrame for ItemFrameEntity {
    fn item(&self) -> ItemStack {
        self.entity_data.lock().item.get().clone()
    }

    fn analog_output(&self) -> i32 {
        let entity_data = self.entity_data.lock();
        if entity_data.item.get().is_empty() {
            0
        } else {
            *entity_data.rotation.get() % 8 + 1
        }
    }
}

mod entity_impl;

impl BlockAttachedEntity for ItemFrameEntity {
    fn on_killed(&self) {
        self.remove_framed_map(&self.item());
    }

    fn block_attached_entity_base(&self) -> &BlockAttachedEntityBase {
        &self.block_attached_entity_base
    }

    fn survives(&self) -> bool {
        if self.frame_state.lock().fixed {
            return true;
        }
        if self.has_level_collision() {
            return false;
        }
        let Some(world) = self.level() else {
            return false;
        };
        let direction = self.direction();
        let state = world.get_block_state(
            self.block_attached_entity_base
                .pos()
                .relative(direction.opposite()),
        );
        (state.is_solid()
            || (direction.get_axis().is_horizontal()
                && BLOCK_BEHAVIORS.get_behavior(state.get_block()).is_diode()))
            && self.can_coexist(true)
    }

    fn drop_item(&self, caused_by: Option<&dyn Entity>) {
        self.play_sound(
            if self.entity_type == &vanilla_entities::GLOW_ITEM_FRAME {
                &sound_events::ENTITY_GLOW_ITEM_FRAME_BREAK
            } else {
                &sound_events::ENTITY_ITEM_FRAME_BREAK
            },
            1.0,
            1.0,
        );
        self.drop_contents(caused_by, true);
        self.game_event_with_source_entity(&vanilla_game_events::BLOCK_CHANGE, caused_by);
    }

    fn recalculate_bounding_box(&self) -> Result<(), EntityMoveError> {
        self.try_recalculate_position()
    }
}

const fn direction_3d_data_value(direction: Direction) -> i32 {
    match direction {
        Direction::Down => 0,
        Direction::Up => 1,
        Direction::North => 2,
        Direction::South => 3,
        Direction::West => 4,
        Direction::East => 5,
    }
}

const fn direction_2d_data_value(direction: Direction) -> u8 {
    match direction {
        Direction::South | Direction::Down | Direction::Up => 0,
        Direction::West => 1,
        Direction::North => 2,
        Direction::East => 3,
    }
}

#[cfg(test)]
mod tests;
