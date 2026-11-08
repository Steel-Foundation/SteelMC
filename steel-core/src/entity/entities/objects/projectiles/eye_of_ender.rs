//! Vanilla `EyeOfEnder`: a thrown ender eye flying toward a stronghold.
//!
//! Despite its package, vanilla's eye is a plain entity that steers itself,
//! not a projectile.

use std::sync::Arc;

use glam::DVec3;
use simdnbt::ToNbtTag;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_macros::entity_behavior;
use steel_math::lerp;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_entity_data::EyeOfEnderEntityData;
use steel_registry::{level_events, sound_events, vanilla_entities, vanilla_items};
use steel_utils::BlockPos;
use steel_utils::locks::CheckedMutex;

use crate::entity::entities::ItemEntity;
use crate::entity::prelude::*;
use crate::entity::{EntityBase, EntityProps, RemovalReason, SharedEntity, next_entity_id};

/// Targets further than this horizontally are replaced by a point this far
/// along the way and [`TOO_FAR_SIGNAL_HEIGHT`] up.
const TOO_FAR_DISTANCE: f64 = 12.0;
const TOO_FAR_SIGNAL_HEIGHT: f64 = 8.0;

#[derive(Default)]
struct Flight {
    target: Option<DVec3>,
    life: u32,
    survive_after_death: bool,
}

/// A thrown eye of ender.
#[entity_behavior(class = "EyeOfEnder")]
#[derive(Entity)]
#[entity(hooks = Self::ENTITY, save = Self::save_item, load = Self::load_item)]
#[entity(key = "steel:entity/eye_of_ender", constructors)]
pub struct EyeOfEnderEntity {
    #[entity(base)]
    base: EntityBase,
    #[entity(entity_type)]
    entity_type: EntityTypeRef,
    flight: CheckedMutex<Flight>,
    #[entity(synced)]
    entity_data: CheckedMutex<EyeOfEnderEntityData>,
}

impl EyeOfEnderEntity {
    const ENTITY: EntityProps<Self> = EntityProps {
        tick: |eye, entity| eye.entity_tick(entity),
        attackable: |_| false,
        ..EntityProps::new()
    };

    fn from_base(base: EntityBase, entity_type: EntityTypeRef) -> Self {
        let mut entity_data = EyeOfEnderEntityData::new();
        entity_data.item_stack.set(Self::default_item());
        Self {
            base,
            entity_type,
            flight: CheckedMutex::new(Flight::default()),
            entity_data: CheckedMutex::new(entity_data),
        }
    }

    fn default_item() -> ItemStack {
        ItemStack::new(&vanilla_items::ENDER_EYE)
    }

    /// The item this eye is shown as.
    #[must_use]
    pub fn item(&self) -> ItemStack {
        self.entity_data.lock().item_stack.get().clone()
    }

    /// Shows this eye as a single `item`, or as a plain ender eye when empty.
    pub fn set_item(&self, item: &ItemStack) {
        let item = if item.is_empty() {
            Self::default_item()
        } else {
            item.copy_with_count(1)
        };
        self.entity_data.lock().item_stack.set(item);
    }

    /// `EyeOfEnder.signalTo`: flies toward `target`, or toward a point up and
    /// along the way when it is far, and decides whether to drop on expiry.
    pub fn signal_to(&self, target: DVec3) {
        let position = self.position();
        let delta = target - position;
        let horizontal_distance = DVec3::new(delta.x, 0.0, delta.z).length();
        let target = if horizontal_distance > TOO_FAR_DISTANCE {
            position
                + DVec3::new(
                    delta.x / horizontal_distance * TOO_FAR_DISTANCE,
                    TOO_FAR_SIGNAL_HEIGHT,
                    delta.z / horizontal_distance * TOO_FAR_DISTANCE,
                )
        } else {
            target
        };

        let mut flight = self.flight.lock();
        flight.target = Some(target);
        flight.life = 0;
        flight.survive_after_death = rand::random_range(0..5) > 0;
    }

    /// `EyeOfEnder.tick`: flies toward its target and, when its life ends,
    /// drops as an item or shatters.
    fn entity_tick(&self, entity: &SharedEntity) {
        Arc::clone(entity).tick_default();
        let new_position = self.position() + self.velocity();
        let target = self.flight.lock().target;
        if let Some(target) = target {
            self.set_velocity(Self::update_delta_movement(
                self.velocity(),
                new_position,
                target,
            ));
        }
        // VANILLA CLIENT-LOCAL: the trail particles.
        if self.try_set_position(new_position).is_err() {
            self.set_removed(RemovalReason::Discarded);
            return;
        }

        let (life, survive_after_death) = {
            let mut flight = self.flight.lock();
            flight.life += 1;
            (flight.life, flight.survive_after_death)
        };
        if life <= 80 {
            return;
        }
        self.play_sound(&sound_events::ENTITY_ENDER_EYE_DEATH, 1.0, 1.0);
        self.set_removed(RemovalReason::Discarded);
        let Some(world) = self.level() else {
            return;
        };
        if survive_after_death {
            let item = ItemEntity::with_item(
                &vanilla_entities::ITEM,
                next_entity_id(),
                self.position(),
                self.item(),
                Arc::downgrade(&world),
            );
            if let Err(error) = world.try_add_entity(Arc::new(item)) {
                log::warn!("failed to drop eye of ender item: {error}");
            }
        } else {
            world.level_event(
                level_events::PARTICLES_EYE_OF_ENDER_DEATH,
                BlockPos::from(self.position()),
                0,
                None,
            );
        }
    }

    /// `EyeOfEnder.updateDeltaMovement`.
    ///
    /// Vanilla divides by the horizontal distance unguarded, which yields NaN
    /// when the eye is exactly above its target; Steel rejects non-finite
    /// positions, so only the vertical part applies then.
    fn update_delta_movement(old_movement: DVec3, position: DVec3, target: DVec3) -> DVec3 {
        let horizontal_delta = DVec3::new(target.x - position.x, 0.0, target.z - position.z);
        let horizontal_length = horizontal_delta.length();
        let mut wanted_speed = lerp(
            0.0025,
            DVec3::new(old_movement.x, 0.0, old_movement.z).length(),
            horizontal_length,
        );
        let mut movement_y = old_movement.y;
        if horizontal_length < 1.0 {
            wanted_speed *= 0.8;
            movement_y *= 0.8;
        }
        let wanted_movement_y = if position.y - old_movement.y < target.y {
            1.0
        } else {
            -1.0
        };
        let vertical = DVec3::new(
            0.0,
            movement_y + (wanted_movement_y - movement_y) * 0.015,
            0.0,
        );
        if horizontal_length == 0.0 {
            return vertical;
        }
        horizontal_delta * (wanted_speed / horizontal_length) + vertical
    }

    fn save_item(&self, nbt: &mut NbtCompound) {
        nbt.insert("Item", self.item().to_nbt_tag());
    }

    fn load_item(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        let item = nbt
            .compound("Item")
            .and_then(|tag| ItemStack::from_borrowed_compound(&tag))
            .unwrap_or_else(Self::default_item);
        self.set_item(&item);
    }
}
