//! Minimal persistent item-frame entity used by structure generation.

use std::sync::Weak;

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::{NbtCompound, NbtTag};
use steel_macros::entity_behavior;
use steel_registry::data_components::vanilla_components::MAP_ID;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_entities;
use steel_registry::vanilla_entity_data::ItemFrameEntityData;
use steel_utils::{BlockPos, Direction, WorldAabb, axis::Axis};

use crate::entity::{
    Entity, EntityBase, EntityBaseLoad, EntitySyncedData, SharedEntity, next_entity_id,
};
use crate::world::World;

/// Item frame state needed by end-city structure markers.
///
/// This intentionally implements only placement, synced item/facing data, and
/// persistence. Interaction, drops, map tracking, and support checks belong to
/// the full item-frame entity implementation.
#[entity_behavior(class = "item_frame")]
pub struct ItemFrame {
    base: Weak<EntityBase>,
    entity_data: ItemFrameEntityData,
    block_pos: BlockPos,
}

impl ItemFrame {
    fn build(base: Weak<EntityBase>, block_pos: BlockPos, direction: Direction) -> Self {
        let mut entity_data = ItemFrameEntityData::new();
        entity_data.hanging_entity.set_direction(direction);
        Self {
            base,
            entity_data,
            block_pos,
        }
    }

    /// Creates a fresh item frame (generated factory convention).
    #[must_use]
    pub fn new(id: i32, position: DVec3, world: Weak<World>) -> SharedEntity {
        EntityBase::pack_with(
            id,
            position,
            vanilla_entities::ITEM_FRAME.dimensions,
            world,
            |base| Self::build(base, BlockPos::ZERO, Direction::South),
        )
    }

    /// Creates a fresh item frame attached to `block_pos` facing `direction`.
    #[must_use]
    pub fn new_attached(block_pos: BlockPos, direction: Direction) -> SharedEntity {
        EntityBase::pack_with(
            next_entity_id(),
            ItemFrame::frame_center(block_pos, direction),
            vanilla_entities::ITEM_FRAME.dimensions,
            Weak::new(),
            |base| Self::build(base, block_pos, direction),
        )
    }

    /// Creates an item frame from persistent entity data.
    #[must_use]
    pub fn from_saved(load: EntityBaseLoad) -> SharedEntity {
        let position = load.position;
        let block_pos = BlockPos::new(
            position.x.floor() as i32,
            position.y.floor() as i32,
            position.z.floor() as i32,
        );
        EntityBase::pack_loaded_with(load, vanilla_entities::ITEM_FRAME.dimensions, |base| {
            Self::build(base, block_pos, Direction::South)
        })
    }

    /// Sets the framed item, matching vanilla by storing a single item.
    pub fn set_item(&mut self, mut item: ItemStack) {
        if !item.is_empty() {
            item.set_count(1);
        }
        self.entity_data.set_item(item);
        self.recalculate_position();
    }

    fn set_direction(&mut self, direction: Direction) {
        self.entity_data.hanging_entity.set_direction(direction);
        if let Some(base) = self.base.upgrade() {
            base.set_rotation(Self::rotation_for_direction(direction));
        }
        self.recalculate_position();
    }

    fn recalculate_position(&self) {
        let Some(base) = self.base.upgrade() else {
            return;
        };
        let block_pos = self.block_pos;
        let direction = *self.entity_data.hanging_entity.direction.get();
        let position = Self::frame_center(block_pos, direction);
        if let Err(error) = base.try_set_position(position) {
            panic!(
                "failed to commit item frame {} position recalculation: {error}",
                base.id()
            );
        }
        base.set_bounding_box(Self::frame_bounding_box(
            block_pos,
            direction,
            self.has_framed_map(),
        ));
    }

    fn has_framed_map(&self) -> bool {
        self.entity_data.item.get().has(MAP_ID)
    }

    /// Todo
    pub fn frame_center(block_pos: BlockPos, direction: Direction) -> DVec3 {
        let (dx, dy, dz) = direction.offset();
        DVec3::new(
            f64::from(block_pos.x()) + 0.5 - f64::from(dx) * 0.46875,
            f64::from(block_pos.y()) + 0.5 - f64::from(dy) * 0.46875,
            f64::from(block_pos.z()) + 0.5 - f64::from(dz) * 0.46875,
        )
    }

    fn rotation_for_direction(direction: Direction) -> (f32, f32) {
        if direction.is_horizontal() {
            (f32::from(direction_2d_data_value(direction)) * 90.0, 0.0)
        } else {
            let pitch = match direction {
                Direction::Up => -90.0,
                Direction::Down => 90.0,
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

impl Entity for ItemFrame {
    fn base_weak(&self) -> &Weak<EntityBase> {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        &vanilla_entities::ITEM_FRAME
    }

    fn spawn_data(&self) -> i32 {
        direction_3d_data_value(*self.entity_data.hanging_entity.direction.get())
    }

    fn spawn_position(&self) -> DVec3 {
        let block_pos = self.block_pos;
        DVec3::new(
            f64::from(block_pos.x()),
            f64::from(block_pos.y()),
            f64::from(block_pos.z()),
        )
    }

    fn is_pickable(&self) -> bool {
        true
    }

    fn synced_data(&self) -> Option<&dyn EntitySyncedData> {
        Some(&self.entity_data)
    }

    fn synced_data_mut(&mut self) -> Option<&mut dyn EntitySyncedData> {
        Some(&mut self.entity_data)
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        let block_pos = self.block_pos;
        nbt.insert(
            "block_pos",
            NbtTag::IntArray(vec![block_pos.x(), block_pos.y(), block_pos.z()]),
        );

        let entity_data = &self.entity_data;
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

    fn load_additional(&mut self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        if let Some(block_pos) = nbt.int_array("block_pos")
            && block_pos.len() == 3
        {
            self.block_pos = BlockPos::new(block_pos[0], block_pos[1], block_pos[2]);
        }

        if let Some(item_tag) = nbt.compound("Item")
            && let Some(item) = ItemStack::from_borrowed_compound(&item_tag)
        {
            self.set_item(item);
        }

        if let Some(item_rotation) = nbt.byte("ItemRotation") {
            self.entity_data
                .set_rotation(i32::from(item_rotation).rem_euclid(8));
        }

        let facing = nbt
            .byte("Facing")
            .and_then(|value| direction_from_3d_data_value(i32::from(value)))
            .or_else(|| nbt.int("Facing").and_then(direction_from_3d_data_value));
        if let Some(direction) = facing {
            self.set_direction(direction);
        }

        self.recalculate_position();
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

const fn direction_from_3d_data_value(value: i32) -> Option<Direction> {
    match value {
        0 => Some(Direction::Down),
        1 => Some(Direction::Up),
        2 => Some(Direction::North),
        3 => Some(Direction::South),
        4 => Some(Direction::West),
        5 => Some(Direction::East),
        _ => None,
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
    use super::*;
    use std::string::ToString;
    use steel_registry::vanilla_items;

    #[test]
    fn item_frame_persists_structure_marker_state() {
        let frame = ItemFrame::new_attached(BlockPos::new(12, 80, 14), Direction::West);
        {
            let mut frame = frame.lock_entity();
            let frame: &mut ItemFrame = frame.downcast().unwrap();
            frame.set_item(ItemStack::new(&vanilla_items::ITEMS.elytra));
        }

        let mut nbt = NbtCompound::new();
        frame.with_entity(|e| e.save_additional(&mut nbt));

        assert_eq!(nbt.byte("Facing"), Some(4));
        assert_eq!(nbt.byte("ItemRotation"), Some(0));
        assert_eq!(nbt.float("ItemDropChance"), Some(1.0));
        assert_eq!(nbt.byte("Invisible"), Some(0));
        assert_eq!(nbt.byte("Fixed"), Some(0));
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
    fn item_frame_is_pickable_like_vanilla() {
        let frame = ItemFrame::new_attached(BlockPos::new(12, 80, 14), Direction::West);

        assert!(frame.with_entity(|e| e.is_pickable()));
    }
}
