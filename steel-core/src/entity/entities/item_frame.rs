//! Minimal persistent item-frame entity used by structure generation.

use std::sync::Weak;

use crossbeam::atomic::AtomicCell;
use glam::DVec3;
use simdnbt::borrow::{BaseNbtCompound as BorrowedNbtCompound, NbtCompound as NbtCompoundView};
use simdnbt::owned::{NbtCompound, NbtTag};
use steel_registry::blocks::shapes::AABBd;
use steel_registry::data_components::vanilla_components::MAP_ID;
use steel_registry::entity_data::DataValue;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_entities;
use steel_registry::vanilla_entity_data::ItemFrameEntityData;
use steel_utils::locks::SyncMutex;
use steel_utils::{BlockPos, Direction, axis::Axis};
use uuid::Uuid;

use crate::entity::{Entity, EntityBase};
use crate::world::World;

/// Item frame state needed by end-city structure markers.
///
/// This intentionally implements only placement, synced item/facing data, and
/// persistence. Interaction, drops, map tracking, and support checks belong to
/// the full item-frame entity implementation.
pub struct ItemFrameEntity {
    base: EntityBase,
    entity_data: SyncMutex<ItemFrameEntityData>,
    block_pos: SyncMutex<BlockPos>,
    rotation: AtomicCell<(f32, f32)>,
}

impl ItemFrameEntity {
    /// Creates a fresh item frame attached to `block_pos`.
    #[must_use]
    pub fn new(id: i32, block_pos: BlockPos, direction: Direction, world: Weak<World>) -> Self {
        let entity = Self {
            base: EntityBase::new(id, Self::frame_center(block_pos, direction), world),
            entity_data: SyncMutex::new(ItemFrameEntityData::new()),
            block_pos: SyncMutex::new(block_pos),
            rotation: AtomicCell::new(Self::rotation_for_direction(direction)),
        };
        entity.entity_data.lock().direction.set(direction);
        entity
    }

    /// Creates an item frame from persistent entity data.
    #[must_use]
    pub fn from_saved(
        id: i32,
        position: DVec3,
        uuid: Uuid,
        rotation: (f32, f32),
        world: Weak<World>,
    ) -> Self {
        Self {
            base: EntityBase::with_uuid(id, uuid, position, world),
            entity_data: SyncMutex::new(ItemFrameEntityData::new()),
            block_pos: SyncMutex::new(BlockPos::new(
                position.x.floor() as i32,
                position.y.floor() as i32,
                position.z.floor() as i32,
            )),
            rotation: AtomicCell::new(rotation),
        }
    }

    /// Sets the framed item, matching vanilla by storing a single item.
    pub fn set_item(&self, mut item: ItemStack) {
        if !item.is_empty() {
            item.set_count(1);
        }
        self.entity_data.lock().item.set(item);
        self.recalculate_position();
    }

    fn set_direction(&self, direction: Direction) {
        self.entity_data.lock().direction.set(direction);
        self.rotation.store(Self::rotation_for_direction(direction));
        self.recalculate_position();
    }

    fn recalculate_position(&self) {
        let block_pos = *self.block_pos.lock();
        let direction = *self.entity_data.lock().direction.get();
        self.set_position(Self::frame_center(block_pos, direction));
    }

    fn has_framed_map(&self) -> bool {
        self.entity_data.lock().item.get().has(MAP_ID)
    }

    fn frame_center(block_pos: BlockPos, direction: Direction) -> DVec3 {
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
}

impl Entity for ItemFrameEntity {
    fn base(&self) -> Option<&EntityBase> {
        Some(&self.base)
    }

    fn entity_type(&self) -> EntityTypeRef {
        &vanilla_entities::ITEM_FRAME
    }

    fn bounding_box(&self) -> AABBd {
        let block_pos = *self.block_pos.lock();
        let direction = *self.entity_data.lock().direction.get();
        let center = Self::frame_center(block_pos, direction);
        let size = if self.has_framed_map() { 1.0 } else { 0.75 };
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
        AABBd {
            min_x: center.x - x_size / 2.0,
            min_y: center.y - y_size / 2.0,
            min_z: center.z - z_size / 2.0,
            max_x: center.x + x_size / 2.0,
            max_y: center.y + y_size / 2.0,
            max_z: center.z + z_size / 2.0,
        }
    }

    fn rotation(&self) -> (f32, f32) {
        self.rotation.load()
    }

    fn spawn_data(&self) -> i32 {
        direction_3d_data_value(*self.entity_data.lock().direction.get())
    }

    fn pack_dirty_entity_data(&self) -> Option<Vec<DataValue>> {
        self.entity_data.lock().pack_dirty()
    }

    fn pack_all_entity_data(&self) -> Vec<DataValue> {
        self.entity_data.lock().pack_all()
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        let block_pos = *self.block_pos.lock();
        nbt.insert(
            "block_pos",
            NbtTag::IntArray(vec![block_pos.x(), block_pos.y(), block_pos.z()]),
        );

        let entity_data = self.entity_data.lock();
        let item = entity_data.item.get();
        if !item.is_empty() {
            nbt.insert("Item", item.to_nbt_tag_ref());
        }
        nbt.insert("ItemRotation", *entity_data.rotation.get() as i8);
        nbt.insert("ItemDropChance", 1.0_f32);
        nbt.insert(
            "Facing",
            direction_3d_data_value(*entity_data.direction.get()) as i8,
        );
        nbt.insert("Invisible", 0_i8);
        nbt.insert("Fixed", 0_i8);
    }

    fn load_additional(&self, nbt: &BorrowedNbtCompound<'_>) {
        let nbt: NbtCompoundView<'_, '_> = nbt.into();

        if let Some(block_pos) = nbt.int_array("block_pos")
            && block_pos.len() == 3
        {
            *self.block_pos.lock() = BlockPos::new(block_pos[0], block_pos[1], block_pos[2]);
        }

        if let Some(item_tag) = nbt.compound("Item")
            && let Some(item) = ItemStack::from_borrowed_compound(&item_tag)
        {
            self.set_item(item);
        }

        if let Some(item_rotation) = nbt.byte("ItemRotation") {
            self.entity_data
                .lock()
                .rotation
                .set(i32::from(item_rotation).rem_euclid(8));
        }

        let facing = nbt
            .byte("Facing")
            .and_then(|value| direction_from_3d_data_value(i32::from(value)))
            .or_else(|| nbt.int("Facing").and_then(direction_from_3d_data_value));
        if let Some(direction) = facing {
            self.set_direction(direction);
        }
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
        let frame =
            ItemFrameEntity::new(1, BlockPos::new(12, 80, 14), Direction::West, Weak::new());
        frame.set_item(ItemStack::new(&vanilla_items::ITEMS.elytra));

        let mut nbt = NbtCompound::new();
        frame.save_additional(&mut nbt);

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
}
