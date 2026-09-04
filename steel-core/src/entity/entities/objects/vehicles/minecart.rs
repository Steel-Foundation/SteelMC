//! Rideable minecart entity implementation.

use std::sync::{Arc, Weak};

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_macros::entity_behavior;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_entity_data::AbstractMinecartEntityData;
use steel_registry::vanilla_items;
use steel_utils::axis::Axis;
use steel_utils::block_util::FoundRectangle;
use steel_utils::locks::SyncMutex;
use steel_utils::types::InteractionHand;
use steel_utils::{BlockPos, Direction, DowncastType, DowncastTypeKey};

use crate::behavior::InteractionResult;
use crate::entity::damage::DamageSource;
use crate::entity::{
    Entity, EntityBase, EntityBaseLoad, EntitySyncedData, RemovalReason, dismount_helper,
    reset_forward_direction_of_relative_portal_position,
};
use crate::player::Player;
use crate::portal::portal_shape::PortalShape;
use crate::world::World;

/// Rideable vanilla minecart entity.
#[entity_behavior(class = "Minecart")]
pub struct MinecartEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    state: SyncMutex<MinecartState>,
    entity_data: SyncMutex<AbstractMinecartEntityData>,
    behavior: SyncMutex<Box<dyn super::MinecartBehavior>>,
}

// SAFETY: Key uniquely identifies `MinecartEntity` within Steel codebase.
unsafe impl DowncastType for MinecartEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/minecart");
}

#[derive(Debug, Clone, PartialEq)]
struct MinecartState {
    first_tick: bool,
    damage: f32,
    on_rails: bool,
}

impl MinecartState {
    const fn new(first_tick: bool) -> Self {
        Self {
            first_tick,
            damage: 0.0,
            on_rails: false,
        }
    }
}

impl MinecartEntity {
    /// Creates a new minecart entity.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self {
            base: EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
            state: SyncMutex::new(MinecartState::new(true)),
            entity_data: SyncMutex::new(AbstractMinecartEntityData::new()),
            behavior: SyncMutex::new(Box::new(super::OldMinecartBehavior::new())),
        }
    }

    /// Creates a minecart entity from saved data.
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        Self {
            base: EntityBase::from_load(load, entity_type.dimensions),
            entity_type,
            state: SyncMutex::new(MinecartState::new(false)),
            entity_data: SyncMutex::new(AbstractMinecartEntityData::new()),
            behavior: SyncMutex::new(Box::new(super::OldMinecartBehavior::new())),
        }
    }

    const fn nbt_bool(value: bool) -> i8 {
        if value { 1 } else { 0 }
    }

    fn eject_passengers(&self) {
        for passenger in self.passengers() {
            passenger.stop_riding();
        }
    }
}

impl Entity for MinecartEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn tick(&self) {
        self.default_tick();
        {
            let mut data = self.entity_data.lock();
            let vehicle = data.vehicle_entity_mut();
            let hurt = *vehicle.id_hurt.get();
            if hurt > 0 {
                vehicle.id_hurt.set(hurt - 1);
            }
            let current_damage = *vehicle.id_damage.get();
            if current_damage > 0.0 {
                vehicle.id_damage.set((current_damage - 1.0).max(0.0));
            }
        }
        if let Some(world) = self.level() {
            self.behavior.lock().tick(self, &world);
        }
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    fn is_pickable(&self) -> bool {
        !self.is_removed()
    }

    fn is_pushable(&self) -> bool {
        true
    }

    fn is_on_rails(&self) -> bool {
        self.state.lock().on_rails
    }

    fn set_on_rails(&self, on_rails: bool) {
        self.state.lock().on_rails = on_rails;
    }

    fn can_be_collided_with(&self, _other: Option<&dyn Entity>) -> bool {
        false
    }

    fn can_collide_with(&self, other: &dyn Entity) -> bool {
        (other.can_be_collided_with(Some(self)) || other.is_pushable())
            && !self.is_passenger_of_same_vehicle(other)
    }

    fn push_entity(&self, entity: &dyn Entity) {
        if self.is_passenger_of_same_vehicle(entity) || entity.no_physics() || self.no_physics() {
            return;
        }

        let mut dx = entity.position().x - self.position().x;
        let mut dz = entity.position().z - self.position().z;
        let distance_sq = dx * dx + dz * dz;
        if distance_sq < 0.0001 {
            return;
        }

        let distance = distance_sq.sqrt();
        dx /= distance;
        dz /= distance;
        let scale = (1.0 / distance).min(1.0) * 0.05;
        dx *= scale;
        dz *= scale;

        if entity.entity_type().is_abstract_minecart {
            let collision_vec = DVec3::new(dx, 0.0, dz).normalize_or_zero();
            let (yaw, _) = self.rotation();
            let yaw_rad = f64::from(yaw).to_radians();
            let facing_vec = DVec3::new(yaw_rad.cos(), 0.0, yaw_rad.sin()).normalize_or_zero();

            if collision_vec.dot(facing_vec).abs() < 0.8 {
                return;
            }

            let v1 = self.velocity();
            let v2 = entity.velocity();

            let avg_x = v1.x.midpoint(v2.x);
            let avg_z = v1.z.midpoint(v2.z);

            self.set_velocity(DVec3::new(v1.x * 0.2, v1.y, v1.z * 0.2));
            self.push_impulse(DVec3::new(avg_x - dx, 0.0, avg_z - dz));
            entity.set_velocity(DVec3::new(v2.x * 0.2, v2.y, v2.z * 0.2));
            entity.push_impulse(DVec3::new(avg_x + dx, 0.0, avg_z + dz));
        } else {
            self.push_impulse(DVec3::new(-dx, 0.0, -dz));
            if entity.is_pushable() {
                entity.push_impulse(DVec3::new(dx * 0.25, 0.0, dz * 0.25));
            }
        }
    }

    fn get_default_gravity(&self) -> f64 {
        0.04
    }

    fn blocks_building(&self) -> bool {
        true
    }

    fn dimension_changing_delay(&self) -> i32 {
        10
    }

    fn get_relative_portal_position(&self, axis: Axis, portal_area: FoundRectangle) -> DVec3 {
        reset_forward_direction_of_relative_portal_position(PortalShape::get_relative_position(
            portal_area,
            axis,
            self.position(),
            self.dimensions_for_pose(self.pose()),
        ))
    }

    fn interact(
        &self,
        player: &Player,
        _hand: InteractionHand,
        _location: DVec3,
    ) -> InteractionResult {
        if player.is_secondary_use_active() || self.is_vehicle() {
            return InteractionResult::Pass;
        }

        let Some(world) = self.level() else {
            return InteractionResult::Pass;
        };
        let Some(vehicle) = world.get_entity_by_id(self.id()) else {
            return InteractionResult::Pass;
        };

        if player.start_riding(&vehicle) {
            InteractionResult::SuccessServer
        } else {
            InteractionResult::Pass
        }
    }

    fn hurt(&self, world: &World, source: &DamageSource, amount: f32) -> bool {
        if self.is_invulnerable_to_base(source) || self.is_removed() {
            return false;
        }

        self.mark_hurt();

        let is_creative = source
            .causing_entity_id
            .and_then(|id| world.get_entity_by_id(id))
            .is_some_and(|entity| {
                entity
                    .as_player()
                    .is_some_and(Player::has_infinite_materials)
            });

        if is_creative {
            self.eject_passengers();
            self.set_removed(RemovalReason::Killed);
            return true;
        }

        let new_damage = {
            let mut state = self.state.lock();
            state.damage += amount * 10.0;
            state.damage
        };

        let mut data = self.entity_data.lock();
        let vehicle = data.vehicle_entity_mut();
        vehicle.id_hurt.set(10);
        vehicle.id_damage.set(new_damage);
        vehicle.id_hurtdir.set(-*vehicle.id_hurtdir.get());

        if new_damage > 40.0 {
            self.eject_passengers();
            self.spawn_at_location(ItemStack::new(&vanilla_items::MINECART), 0.0);
            self.set_removed(RemovalReason::Killed);
        }

        true
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        nbt.insert("FlippedRotation", Self::nbt_bool(false));
        let state = self.state.lock();
        nbt.insert("HasTicked", Self::nbt_bool(state.first_tick));
    }

    fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        let mut state = self.state.lock();
        if let Some(first_tick) = nbt.byte("HasTicked") {
            state.first_tick = first_tick != 0;
        }
    }

    fn synced_data(&self) -> Option<&dyn EntitySyncedData> {
        Some(&self.entity_data)
    }

    fn get_dismount_location_for_passenger(
        &self,
        passenger: &dyn Entity,
        world: &Arc<World>,
    ) -> DVec3 {
        let direction = self.get_motion_direction();
        if direction.axis() == Axis::Y {
            return self.position();
        }

        let offsets = dismount_helper::offsets_for_direction(direction);
        let base_pos = BlockPos::new(
            self.position().x.floor() as i32,
            self.position().y.floor() as i32,
            self.position().z.floor() as i32,
        );

        for (off_x, off_z) in offsets {
            let offset_pos =
                BlockPos::new(base_pos.x() + off_x, base_pos.y(), base_pos.z() + off_z);
            if let Some(pos) =
                dismount_helper::find_safe_dismount_location(world, passenger, offset_pos, true)
            {
                return pos;
            }
        }

        let bbox = self.bounding_box();
        DVec3::new(self.position().x, bbox.max_y(), self.position().z)
    }

    fn get_motion_direction(&self) -> Direction {
        self.direction_yaw()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::behavior::init_behaviors;
    use crate::entity::SharedEntity;
    use crate::test_support::{TestPlayerBuilder, fresh_test_world, insert_ready_full_chunk};
    use simdnbt::borrow::read_compound;
    use std::io::Cursor;
    use steel_registry::{init_vanilla_registry, vanilla_blocks, vanilla_entities};
    use steel_utils::ChunkPos;
    use steel_utils::types::UpdateFlags;

    #[test]
    fn minecart_saves_and_loads_additional_state() {
        let minecart = MinecartEntity::new(
            &vanilla_entities::MINECART,
            1,
            DVec3::new(1.5, 2.5, 3.5),
            Weak::new(),
        );

        let mut bytes = Vec::new();
        let mut nbt = NbtCompound::new();
        minecart.save_additional(&mut nbt);
        nbt.write(&mut bytes);
        let borrowed = read_compound(&mut Cursor::new(&bytes))
            .unwrap_or_else(|error| panic!("reborrow failed: {error}"));

        let loaded = MinecartEntity::new(
            &vanilla_entities::MINECART,
            2,
            DVec3::new(1.5, 2.5, 3.5),
            Weak::new(),
        );
        loaded.load_additional((&borrowed).into());

        assert!(loaded.state.lock().first_tick);
    }

    #[test]
    fn minecart_is_pickable_and_pushable_like_vanilla() {
        let minecart = MinecartEntity::new(
            &vanilla_entities::MINECART,
            1,
            DVec3::new(1.5, 2.5, 3.5),
            Weak::new(),
        );

        assert!(minecart.is_pickable());
        assert!(minecart.is_pushable());
        assert!(!minecart.can_be_collided_with(None));
        assert!(minecart.blocks_building());
    }

    #[test]
    fn minecart_relative_portal_position_resets_forward_offset() {
        let minecart = MinecartEntity::new(
            &vanilla_entities::MINECART,
            1,
            DVec3::new(12.0, 66.0, 20.75),
            Weak::new(),
        );
        let portal_area = FoundRectangle {
            min_corner: BlockPos::new(10, 64, 20),
            axis1_size: 4,
            axis2_size: 5,
        };

        assert!(
            minecart
                .get_relative_portal_position(Axis::X, portal_area)
                .z
                .abs()
                < f64::EPSILON
        );
    }

    #[test]
    fn minecart_interact_allows_riding_when_empty() {
        init_vanilla_registry();
        init_behaviors();

        let world = fresh_test_world("minecart_interact_empty");
        insert_ready_full_chunk(&world, ChunkPos::new(0, 0));

        let player = TestPlayerBuilder::new(Arc::clone(&world), "Steve", 1).build();
        world.players.insert(Arc::clone(&player));
        world
            .try_add_entity(Arc::clone(&player) as SharedEntity)
            .expect("should add player");

        let minecart: SharedEntity = Arc::new(MinecartEntity::new(
            &vanilla_entities::MINECART,
            2,
            DVec3::new(0.5, 64.0, 0.5),
            Arc::downgrade(&world),
        ));
        world
            .try_add_entity(Arc::clone(&minecart))
            .expect("should add minecart");

        let result = minecart.interact(&player, InteractionHand::MainHand, DVec3::ZERO);
        assert_eq!(result, InteractionResult::SuccessServer);
        assert!(player.is_passenger());
    }

    #[test]
    fn minecart_interact_passes_when_sneaking() {
        init_vanilla_registry();
        init_behaviors();

        let world = fresh_test_world("minecart_interact_sneaking");
        insert_ready_full_chunk(&world, ChunkPos::new(0, 0));

        let player = TestPlayerBuilder::new(Arc::clone(&world), "Steve", 1).build();
        player.set_crouching(true);
        world.players.insert(Arc::clone(&player));
        world
            .try_add_entity(Arc::clone(&player) as SharedEntity)
            .expect("should add player");

        let minecart: SharedEntity = Arc::new(MinecartEntity::new(
            &vanilla_entities::MINECART,
            2,
            DVec3::new(0.5, 64.0, 0.5),
            Arc::downgrade(&world),
        ));
        world
            .try_add_entity(Arc::clone(&minecart))
            .expect("should add minecart");

        let result = minecart.interact(&player, InteractionHand::MainHand, DVec3::ZERO);
        assert_eq!(result, InteractionResult::Pass);
        assert!(!player.is_passenger());
    }

    #[test]
    fn minecart_dismount_finds_safe_location() {
        init_vanilla_registry();
        init_behaviors();
        let world = fresh_test_world("minecart_dismount_safe");
        insert_ready_full_chunk(&world, ChunkPos::new(0, 0));

        let player = TestPlayerBuilder::new(Arc::clone(&world), "Steve", 1).build();
        world.players.insert(Arc::clone(&player));
        world
            .try_add_entity(Arc::clone(&player) as SharedEntity)
            .expect("should add player");

        let minecart: SharedEntity = Arc::new(MinecartEntity::new(
            &vanilla_entities::MINECART,
            2,
            DVec3::new(0.5, 64.0, 0.5),
            Arc::downgrade(&world),
        ));
        world
            .try_add_entity(Arc::clone(&minecart))
            .expect("should add minecart");

        let stone_floor = BlockPos::new(1, 63, 0);
        world.set_block(
            stone_floor,
            vanilla_blocks::STONE.default_state(),
            UpdateFlags::UPDATE_ALL,
        );

        let mounted = player.start_riding(&minecart);
        assert!(mounted);

        player.stop_riding();

        let player_pos = player.position();
        assert!((player_pos.x - 1.5).abs() < 0.01);
        assert!((player_pos.y - 64.0).abs() < 0.01);
        assert!((player_pos.z - 0.5).abs() < 0.01);
    }
}
