//! Vanilla `ItemFrame`.

use std::sync::{Arc, Weak};

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_macros::entity_behavior;
use steel_math::DEGREE_90;
use steel_registry::data_components::vanilla_components::MAP_ID;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_blocks;
use steel_registry::vanilla_entity_data::ItemFrameEntityData;
use steel_utils::locks::CheckedMutex;
use steel_utils::nbt::NbtNumeric as _;
use steel_utils::nbt::NbtValueInput as _;
use steel_utils::{BlockPos, Direction, WorldAabb, axis::Axis};

use crate::entity::EntityTable;
use crate::entity::block_attached::{self, BlockAttachment};
use crate::entity::prelude::*;
use crate::entity::{
    Entity, EntityBase, EntityBaseLoad, EntityBaseState, EntityMoveError, EntityProps, ItemFrame,
    WeakSelf,
};
use crate::world::World;

/// An item frame with its framed item, rotation and facing.
///
/// TODO: add the `HangingEntity` support checks, interaction, drops, fixed
/// and invisible frames, and map tracking.
#[entity_behavior(class = "ItemFrame")]
#[derive(Entity)]
#[entity(save = Self::save_item_frame, load = Self::load_item_frame)]
#[entity(key = "steel:entity/item_frame")]
pub struct ItemFrameEntity {
    #[entity(base)]
    base: EntityBase,
    #[entity(entity_type)]
    entity_type: EntityTypeRef,
    #[entity(component)]
    attachment: BlockAttachment,
    #[entity(synced)]
    entity_data: CheckedMutex<ItemFrameEntityData>,
}

impl EntityTable for ItemFrameEntity {
    const ENTITY: EntityProps<Self> = EntityProps {
        spawn_data: |frame| direction_3d_data_value(frame.direction()),
        ..block_attached::entity_props()
    };
}

impl ItemFrameEntity {
    /// Creates a frame from the generic entity factory path.
    #[must_use]
    pub fn new(
        entity_type: EntityTypeRef,
        id: i32,
        position: DVec3,
        world: Weak<World>,
    ) -> Arc<Self> {
        Self::new_attached(
            entity_type,
            id,
            BlockPos::containing(position.x, position.y, position.z),
            Direction::South,
            world,
        )
    }

    /// Creates a frame on `block_pos` facing `direction`.
    #[must_use]
    pub fn new_attached(
        entity_type: EntityTypeRef,
        id: i32,
        block_pos: BlockPos,
        direction: Direction,
        world: Weak<World>,
    ) -> Arc<Self> {
        let mut entity_data = ItemFrameEntityData::new();
        entity_data.hanging_entity.direction.set(direction);
        WeakSelf::build(|this| Self {
            base: EntityBase::new_with_state(
                id,
                EntityBaseState::new_with_bounding_box(
                    Self::frame_center(block_pos, direction),
                    entity_type.dimensions,
                    Self::frame_bounding_box(block_pos, direction, false),
                )
                .with_rotation(Self::rotation_for_direction(direction)),
                world,
                this,
            ),
            entity_type,
            attachment: BlockAttachment::new(block_pos),
            entity_data: CheckedMutex::new(entity_data),
        })
    }

    /// Reconstructs a frame from saved base data.
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Arc<Self> {
        let position = load.position;
        WeakSelf::build(|this| Self {
            base: EntityBase::from_load(load, entity_type.dimensions, this),
            entity_type,
            attachment: BlockAttachment::new(BlockPos::containing(
                position.x, position.y, position.z,
            )),
            entity_data: CheckedMutex::new(ItemFrameEntityData::new()),
        })
    }

    /// Sets the framed item, keeping a single item.
    pub fn set_item(&self, item: ItemStack) {
        self.set_item_with_update(item, true);
    }

    /// Sets the framed item and optionally notifies nearby comparators.
    pub(crate) fn set_item_with_update(&self, mut item: ItemStack, update_comparators: bool) {
        if !item.is_empty() {
            item.set_count(1);
        }
        self.entity_data.lock().item.set(item);
        self.recalculate_position_or_warn();
        if update_comparators && let Some(world) = self.level() {
            world.update_neighbor_for_output_signal(self.block_pos(), &vanilla_blocks::AIR);
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

    fn recalculate_position_or_warn(&self) {
        if let Err(error) = self.recalculate_bounding_box() {
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
        match direction {
            Direction::Up => (0.0, -DEGREE_90),
            Direction::Down => (0.0, DEGREE_90),
            Direction::North | Direction::South | Direction::West | Direction::East => (
                f32::from(direction_2d_data_value(direction)) * DEGREE_90,
                0.0,
            ),
        }
    }

    fn frame_bounding_box(
        block_pos: BlockPos,
        direction: Direction,
        has_framed_map: bool,
    ) -> WorldAabb {
        let center = Self::frame_center(block_pos, direction);
        let size = if has_framed_map { 1.0 } else { 0.75 };
        let axis_size = |axis| {
            if direction.axis() == axis {
                0.0625
            } else {
                size
            }
        };
        let half = DVec3::new(axis_size(Axis::X), axis_size(Axis::Y), axis_size(Axis::Z)) / 2.0;
        WorldAabb::new(
            center.x - half.x,
            center.y - half.y,
            center.z - half.z,
            center.x + half.x,
            center.y + half.y,
            center.z + half.z,
        )
    }

    fn save_item_frame(&self, nbt: &mut NbtCompound) {
        let entity_data = self.entity_data.lock();
        let item = entity_data.item.get();
        if !item.is_empty() {
            nbt.insert("Item", item.to_nbt_tag_ref());
        }
        nbt.insert("ItemRotation", *entity_data.rotation.get() as i8);
        nbt.insert("ItemDropChance", 1.0_f32);
        nbt.insert(
            "Facing",
            direction_3d_data_value(*entity_data.hanging_entity.direction.get()) as i8,
        );
        nbt.insert("Invisible", 0_i8);
        nbt.insert("Fixed", 0_i8);
    }

    fn load_item_frame(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        if let Some(item_tag) = nbt.compound("Item")
            && let Some(item) = ItemStack::from_borrowed_compound(&item_tag)
        {
            self.set_item_with_update(item, false);
        }
        if let Some(item_rotation) = nbt.byte_value("ItemRotation") {
            // Java's `%` keeps the sign, so -1 stays -1.
            self.entity_data
                .lock()
                .rotation
                .set(i32::from(item_rotation) % 8);
        }
        let facing = nbt
            .get("Facing")
            .and_then(|tag| tag.codec_i8())
            .map_or(Direction::Down, |value| {
                Direction::from_3d_data_value(i32::from(value))
            });
        self.set_direction(facing);
        self.recalculate_position_or_warn();
    }
}

impl ItemFrame for ItemFrameEntity {
    fn direction(&self) -> Direction {
        *self.entity_data.lock().hanging_entity.direction.get()
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

impl block_attached::Parts for ItemFrameEntity {
    fn survives(&self) -> bool {
        true
    }

    fn drop_item(&self, _world: &World, _caused_by: Option<&dyn Entity>) {}

    fn recalculate_bounding_box(&self) -> Result<(), EntityMoveError> {
        let block_pos = self.block_pos();
        let direction = self.direction();
        self.try_set_position_base(Self::frame_center(block_pos, direction))?;
        self.base.set_bounding_box(Self::frame_bounding_box(
            block_pos,
            direction,
            self.has_framed_map(),
        ));
        Ok(())
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
mod tests {
    use steel_registry::{vanilla_entities, vanilla_items};

    use super::*;
    use crate::test_support::load_additional;

    fn frame_loaded_from(nbt: &NbtCompound) -> Arc<ItemFrameEntity> {
        let frame = frame();
        load_additional(&*frame, nbt);
        frame
    }

    #[test]
    fn load_wraps_facing_and_keeps_rotation_sign_like_java() {
        let mut nbt = NbtCompound::new();
        nbt.insert("Facing", 7_i8);
        nbt.insert("ItemRotation", -1_i8);
        let frame = frame_loaded_from(&nbt);

        let entity_data = frame.entity_data.lock();
        assert_eq!(*entity_data.hanging_entity.direction.get(), Direction::Up);
        assert_eq!(*entity_data.rotation.get(), -1);
    }

    #[test]
    fn load_defaults_missing_facing_to_down() {
        let frame = frame_loaded_from(&NbtCompound::new());

        let entity_data = frame.entity_data.lock();
        assert_eq!(*entity_data.hanging_entity.direction.get(), Direction::Down);
    }

    fn frame() -> Arc<ItemFrameEntity> {
        ItemFrameEntity::new_attached(
            &vanilla_entities::ITEM_FRAME,
            1,
            BlockPos::new(12, 80, 14),
            Direction::West,
            Weak::new(),
        )
    }

    #[test]
    fn item_frame_persists_structure_marker_state() {
        let frame = frame();
        frame.set_item(ItemStack::new(&vanilla_items::ELYTRA));

        let mut nbt = NbtCompound::new();
        frame.save_additional(&mut nbt);

        assert_eq!(nbt.int_array("block_pos"), Some([12, 80, 14].as_slice()));
        assert_eq!(nbt.byte("Facing"), Some(4));
        assert_eq!(nbt.byte("ItemRotation"), Some(0));
        let Some(item) = nbt.compound("Item") else {
            panic!("item frame should save framed item");
        };
        assert_eq!(
            item.string("id").map(ToString::to_string),
            Some("minecraft:elytra".to_owned())
        );
        assert_eq!(item.int("count"), Some(1));
    }

    #[test]
    fn analog_output_uses_item_presence_and_rotation() {
        let frame = frame();
        assert_eq!(frame.analog_output(), 0);

        frame.set_item_with_update(ItemStack::new(&vanilla_items::ELYTRA), false);
        assert_eq!(frame.analog_output(), 1);
        frame.entity_data.lock().rotation.set(7);
        assert_eq!(frame.analog_output(), 8);
    }
}
