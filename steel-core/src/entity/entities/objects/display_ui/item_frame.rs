//! Minimal persistent item-frame entity used by structure generation.

use std::sync::Weak;

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_macros::entity_behavior;
use steel_registry::data_components::vanilla_components::MAP_ID;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_blocks;
use steel_registry::vanilla_entity_data::ItemFrameEntityData;
use steel_utils::locks::SyncMutex;
use steel_utils::{BlockPos, Direction, DowncastType, DowncastTypeKey, WorldAabb, axis::Axis};

use crate::entity::block_attached_entity::{BlockAttachedEntity, BlockAttachedEntityBase};
use crate::entity::damage::DamageSource;
use crate::entity::{
    Entity, EntityBase, EntityBaseLoad, EntityBaseState, EntityMoveError, EntitySyncedData,
    ItemFrame,
};
use crate::physics::{MoveResult, MoverType};
use crate::world::World;

/// Item frame state needed by end-city structure markers.
///
/// This intentionally implements only placement, synced item/facing data,
/// persistence, and comparator integration.
///
/// TODO: Add interaction, drops, map tracking, and support checks.
#[entity_behavior(class = "ItemFrame")]
pub struct ItemFrameEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    entity_data: SyncMutex<ItemFrameEntityData>,
    block_attached_entity_base: BlockAttachedEntityBase,
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
        self.entity_data.lock().item.set(item);
        self.recalculate_position_or_warn();
        if update_comparators && let Some(world) = self.level() {
            world.update_neighbor_for_output_signal(
                self.block_attached_entity_base.pos(),
                &vanilla_blocks::AIR,
            );
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
        let block_pos = self.block_attached_entity_base.pos();
        DVec3::new(
            f64::from(block_pos.x()),
            f64::from(block_pos.y()),
            f64::from(block_pos.z()),
        )
    }

    fn synced_data(&self) -> Option<&dyn EntitySyncedData> {
        Some(&self.entity_data)
    }

    fn tick(&self) {
        self.tick_block_attached_entity();
    }

    fn is_pickable(&self) -> bool {
        self.is_pickable_block_attached_entity()
    }

    fn hurt(&self, world: &World, source: &DamageSource, amount: f32) -> bool {
        self.hurt_block_attached_entity(world, source, amount)
    }

    fn skip_attack_interaction(&self, source: &dyn Entity) -> bool {
        self.skip_attack_interaction_block_attached_entity(source)
    }

    fn push_impulse(&self, impulse: DVec3) {
        self.push_impulse_block_attached_entity(impulse);
    }

    fn move_entity(&self, mover_type: MoverType, delta: DVec3) -> Option<MoveResult> {
        self.move_entity_block_attached_entity(mover_type, delta)
    }

    fn refresh_dimensions(&self) {
        self.refresh_dimensions_block_attached_entity();
    }

    fn try_set_position(&self, pos: DVec3) -> Result<(), EntityMoveError> {
        self.try_set_position_block_attached_entity(pos)
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        self.save_block_attached_entity(nbt);

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
        self.load_block_attached_entity(nbt);

        if let Some(item_tag) = nbt.compound("Item")
            && let Some(item) = ItemStack::from_borrowed_compound(&item_tag)
        {
            self.set_item_with_update(item, false);
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

        self.recalculate_position_or_warn();
    }
}

impl BlockAttachedEntity for ItemFrameEntity {
    fn block_attached_entity_base(&self) -> &BlockAttachedEntityBase {
        &self.block_attached_entity_base
    }

    fn survives(&self) -> bool {
        true
    }

    fn drop_item(&self, _caused_by: Option<&dyn Entity>) {}

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
        frame.set_item(ItemStack::new(&vanilla_items::ELYTRA));

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

    #[test]
    fn analog_output_uses_item_presence_and_rotation() {
        let frame = ItemFrameEntity::new_attached(
            &vanilla_entities::ITEM_FRAME,
            1,
            BlockPos::new(12, 80, 14),
            Direction::West,
            Weak::new(),
        );
        assert_eq!(frame.analog_output(), 0);

        frame.set_item_with_update(ItemStack::new(&vanilla_items::ELYTRA), false);
        assert_eq!(frame.analog_output(), 1);
        frame.entity_data.lock().rotation.set(7);
        assert_eq!(frame.analog_output(), 8);
    }
}
