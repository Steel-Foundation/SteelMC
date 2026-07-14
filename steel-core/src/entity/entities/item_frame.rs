//! Minimal persistent item-frame entity used by structure generation.

use std::sync::Weak;

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::{NbtCompound, NbtTag};
use steel_macros::entity_behavior;
use steel_registry::data_components::vanilla_components::MAP_ID;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_entity_data::ItemFrameEntityData;
use steel_utils::locks::SyncMutex;
use steel_utils::{BlockPos, Direction, DowncastType, DowncastTypeKey, WorldAabb, axis::Axis};

use crate::entity::{Entity, EntityBase, EntityBaseLoad, EntityBaseState, EntitySyncedData};
use crate::world::World;

/// Item frame state needed by end-city structure markers.
///
/// This intentionally implements only placement, synced item/facing data, and
/// persistence. Interaction, drops, map tracking, and support checks belong to
/// the full item-frame entity implementation.
#[entity_behavior(class = "ItemFrame")]
pub struct ItemFrameEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    entity_data: SyncMutex<ItemFrameEntityData>,
    block_pos: SyncMutex<BlockPos>,
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
            block_pos: SyncMutex::new(block_pos),
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
            block_pos: SyncMutex::new(BlockPos::new(
                position.x.floor() as i32,
                position.y.floor() as i32,
                position.z.floor() as i32,
            )),
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
        self.entity_data
            .lock()
            .hanging_entity
            .direction
            .set(direction);
        self.base
            .set_rotation(Self::rotation_for_direction(direction));
        self.recalculate_position();
    }

    fn recalculate_position(&self) {
        let block_pos = *self.block_pos.lock();
        let direction = *self.entity_data.lock().hanging_entity.direction.get();
        let position = Self::frame_center(block_pos, direction);
        if let Err(error) = self.base.try_set_position(position) {
            panic!(
                "failed to commit item frame {} position recalculation: {error}",
                self.base.id()
            );
        }
        self.base.set_bounding_box(Self::frame_bounding_box(
            block_pos,
            direction,
            self.has_framed_map(),
        ));
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

impl Entity for ItemFrameEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    fn spawn_data(&self) -> i32 {
        direction_3d_data_value(*self.entity_data.lock().hanging_entity.direction.get())
    }

    fn spawn_position(&self) -> DVec3 {
        let block_pos = *self.block_pos.lock();
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
            direction_3d_data_value(*entity_data.hanging_entity.direction.get()) as i8,
        );
        nbt.insert("Invisible", 0_i8);
        nbt.insert("Fixed", 0_i8);
    }

    fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
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
    use steel_registry::{vanilla_entities, vanilla_items};

    #[test]
    fn item_frame_persists_structure_marker_state() {
        let frame = ItemFrameEntity::new_attached(
            &vanilla_entities::ITEM_FRAME,
            1,
            BlockPos::new(12, 80, 14),
            Direction::West,
            Weak::new(),
        );
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

    #[test]
    fn item_frame_is_pickable_like_vanilla() {
        let frame = ItemFrameEntity::new_attached(
            &vanilla_entities::ITEM_FRAME,
            1,
            BlockPos::new(12, 80, 14),
            Direction::West,
            Weak::new(),
        );

        assert!(frame.is_pickable());
    }
}
