use std::f32::consts::PI;
use std::sync::Arc;

use crate::entity::entities::LeashFenceKnotEntity;
use crate::entity::{Entity, Mob, SharedEntity, WeakEntity};
use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::{NbtCompound, NbtTag};
use steel_protocol::packets::game::SoundSource;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_game_rules::ENTITY_DROPS;
use steel_registry::{sound_events, vanilla_items};
use steel_utils::locks::SyncMutex;
use steel_utils::{BlockPos, Downcast, UuidExt as _};
use uuid::Uuid;

pub const LEASH_SNAP_DISTANCE: f64 = 12.0;
pub const LEASH_ELASTIC_DISTANCE: f64 = 6.0;
pub const LEASH_AXIS_SPECIFIC_ELASTICITY: DVec3 = DVec3::new(0.8, 0.2, 0.8);
pub const LEASH_SPRING_DAMPENING: f64 = 0.7;
pub const LEASH_TORSIONAL_ELASTICITY: f64 = 10.0;
pub const LEASH_STIFFNESS: f64 = 0.11;
pub const ENTITY_LEASH_ATTACHMENT_POINT: DVec3 = DVec3::new(0.0, 0.5, 0.5);
pub const LEASHER_ATTACHMENT_POINT: DVec3 = DVec3::new(0.0, 0.5, 0.0);
pub const DELAYED_LEASH_DROP_TICKS: i32 = 100;
pub const BASE_HORIZONTAL_FRICTION: f64 = 0.91;

/// Vanilla behavior shared by entities that extend `Leashable`.
///
/// Leashable entities can be leashed to an entity holding a lead, or a fence holding a lead.
pub trait Leashable: Entity {
    /// Returns the shared leash data (if any).
    fn leash_data(&self) -> &SyncMutex<Option<LeashData>>;

    /// Returns whether this entity is leashed.
    fn is_leashed(&self) -> bool {
        self.leash_holder().is_some()
    }

    /// Returns whether this entity can be leashed with respect to its leash state.
    ///
    /// In other words, this returns `false` if the entity is already leashed and `true` if not.
    ///
    /// See also: [`Leashable::can_be_leashed`]
    fn may_be_leashed(&self) -> bool {
        self.leash_data().lock().is_some()
    }

    /// Returns the entity holding this entity with a leash, if any.
    fn leash_holder(&self) -> Option<SharedEntity> {
        self.leash_data()
            .lock()
            .as_ref()
            .and_then(LeashData::holder)
    }

    fn leash_attachment(&self) -> Option<LeashAttachment> {
        self.leash_data()
            .lock()
            .as_ref()
            .and_then(LeashData::attachment)
    }

    fn set_delayed_leash_attachment(&self, attachment: LeashAttachment) {
        *self.leash_data().lock() = Some(LeashData::from_delayed_attachment(attachment));
        self.remove_leash();
    }

    /// Returns whether this entity can be leashed with respect to the entity's type or classification.
    ///
    /// For example, mobs like dolphins and hoglins return `true` for this, while `villagers` return `false`.
    ///
    /// See also: [`Leashable::may_be_leashed`]
    fn can_be_leashed(&self) -> bool {
        // TODO: Return false for enemy mobs once hostile mob foundations exist.
        true
    }

    /// Returns the distance between the bounding box's center of the entity and that of `holder`.
    ///
    /// Despite the same, this function does not check for any leash state.
    fn leash_distance_to(&self, holder: &dyn Entity) -> f64 {
        leash_bounding_box_center(self.as_entity_event_source())
            .distance(leash_bounding_box_center(holder))
    }

    /// Returns the minimum leash distance for which a leash will snap.
    fn leash_snap_distance(&self) -> f64 {
        LEASH_SNAP_DISTANCE
    }

    /// Returns the minimum leash distance for which a leash behaves elastically
    /// (it pulls the leashed entity towards the holder).
    fn leash_elastic_distance(&self) -> f64 {
        LEASH_ELASTIC_DISTANCE
    }

    /// Called when this entity is leashed to `holder`.
    fn when_leashed_to(&self, holder: &dyn Entity) {
        holder.notify_leash_holder(self.as_entity_event_source());
    }

    /// Called every tick this entity's leash is stretched too far (this entity is too far from its holder).
    fn leash_too_far_behaviour(&self) {
        self.drop_leash();
    }

    /// Called every tick this entity's leash starts acting elastic (it pulls the leashed entity towards the holder).
    fn on_elastic_leash_pull(&self) {
        self.check_fall_distance_accumulation();
    }

    /// Called every tick this leash is not elastic (pulling) or snappable.
    fn close_range_leash_behaviour(&self, _holder: &dyn Entity) {}

    /// Performs the calculations to pull this entity towards its leash holder and applies
    /// velocity to it.
    fn check_elastic_interactions(&self, holder: &dyn Entity) -> bool {
        let Some(wrench) = compute_elastic_interaction(
            self.as_entity_event_source(),
            holder,
            self.leash_elastic_distance(),
        ) else {
            return false;
        };

        {
            let mut leash_data = self.leash_data().lock();
            let Some(leash_data) = leash_data.as_mut() else {
                return false;
            };
            leash_data.angular_momentum += LEASH_TORSIONAL_ELASTICITY * wrench.torque;
        }

        let relative_velocity_to_leasher =
            leash_holder_movement(holder) - leash_holder_movement(self.as_entity_event_source());
        self.push_impulse(
            axis_specific_leash_elasticity(wrench.force)
                + relative_velocity_to_leasher * LEASH_STIFFNESS,
        );
        true
    }

    /// Applies some angular momentum by the leash for rotation purposes.
    fn apply_leash_angular_momentum(&self) -> bool {
        let angular_friction = self.leash_angular_friction();
        let angular_momentum = {
            let mut leash_data = self.leash_data().lock();
            let Some(leash_data) = leash_data.as_mut() else {
                return false;
            };
            let angular_momentum = leash_data.angular_momentum;
            leash_data.angular_momentum *= angular_friction;
            angular_momentum
        };
        self.rotate_by_leash_angular_momentum(angular_momentum);
        true
    }

    /// Rotates this entity with the provided angular momentum value.
    fn rotate_by_leash_angular_momentum(&self, angular_momentum: f64) {
        let (yaw, pitch) = self.rotation();
        self.set_rotation((yaw - angular_momentum as f32, pitch));
    }

    /// Returns the angular momentum experienced by this entity (if it is leashed).
    fn leash_angular_momentum(&self) -> Option<f64> {
        self.leash_data()
            .lock()
            .as_ref()
            .map(|leash_data| leash_data.angular_momentum)
    }

    /// Returns the friction multiplier for calculating the angular momentum of a leash.
    ///
    /// This is multiplied with the base angular momentum to get the final angular momentum.
    fn leash_angular_friction(&self) -> f64 {
        if self.on_ground() {
            let Some(world) = self.level() else {
                return BASE_HORIZONTAL_FRICTION;
            };
            let Some(pos) = self.block_pos_below_that_affects_movement() else {
                return BASE_HORIZONTAL_FRICTION;
            };
            return f64::from(
                world.get_block_state(pos).get_block().config.friction
                    * BASE_HORIZONTAL_FRICTION as f32,
            );
        }

        if self.is_in_water() || self.is_in_lava() {
            return 0.8;
        }

        BASE_HORIZONTAL_FRICTION
    }

    /// Returns whether this entity can have a leash attached to another. Mirrors Vanilla's `Leashable.canHaveALeashAttachedTo`.
    fn can_have_a_leash_attached_to(&self, holder: &dyn Entity) -> bool {
        self.id() != holder.id()
            && self.leash_distance_to(holder) <= self.leash_snap_distance()
            && self.can_be_leashed()
    }

    /// Sets this entity to be leashed to a holder, removing the old holder's connection, if any.
    fn set_leashed_to(&self, holder: &SharedEntity) -> bool {
        if self.id() == holder.id() {
            return false;
        }

        let old_holder = self.leash_holder();
        {
            let mut leash_data = self.leash_data().lock();
            if let Some(leash_data) = leash_data.as_mut() {
                leash_data.set_holder(holder);
            } else {
                *leash_data = Some(LeashData::from_entity(holder));
            }
        }

        if self.is_passenger() {
            self.stop_riding();
        }
        if let Some(old_holder) = old_holder
            && old_holder.id() != holder.id()
        {
            old_holder.notify_leashee_removed(self.as_entity_event_source());
        }
        true
    }

    /// Updates the delayed leash info to use the entity's current context to resolve
    /// the entity's actual leash connection (whether it be an external entity or a fence knot).
    fn restore_leash_from_save(&self) {
        if let Some(attachment) = self.leash_attachment()
            && let Some(world) = self.level()
        {
            match attachment {
                LeashAttachment::Entity(uuid) => {
                    if let Some(holder) = world.get_entity_by_uuid(&uuid) {
                        let _ = self.set_leashed_to(&holder);
                        return;
                    }
                }
                LeashAttachment::FenceKnot(pos) => {
                    if let Some(holder) = LeashFenceKnotEntity::get_or_create_knot(&world, pos) {
                        let _ = self.set_leashed_to(&holder);
                        return;
                    }
                }
            }

            if self.tick_count() > DELAYED_LEASH_DROP_TICKS {
                let _ = self.spawn_at_location(ItemStack::new(&vanilla_items::LEAD), 0.0);
                self.remove_leash_state();
            }
        }
    }

    /// Ticks the leash *holding* this entity. Mirrors Vanilla's `Leashable.tickLeash`.
    fn tick_leash(&self) {
        self.restore_leash_from_save();

        if let Some(holder) = self.leash_holder() {
            if !self.can_interact_with_level() || !holder.can_interact_with_level() {
                if let Some(world) = self.level()
                    && world.get_game_rule(&ENTITY_DROPS)
                {
                    self.drop_leash();
                } else {
                    self.remove_leash();
                }
                return;
            }
            if let Some(holder) = self.leash_holder()
                && holder.level().map(|level| level.key.clone())
                    == self.level().map(|level| level.key.clone())
            {
                let distance_to = self.leash_distance_to(holder.as_ref());
                self.when_leashed_to(holder.as_ref());
                let angular_momentum_before_distance_action = self.leash_angular_momentum();
                if distance_to > self.leash_snap_distance() {
                    if let Some(world) = self.level() {
                        world.play_sound_at(
                            &sound_events::ITEM_LEAD_BREAK,
                            SoundSource::Neutral,
                            holder.position(),
                            1.0,
                            1.0,
                            None,
                        );
                    }
                    self.leash_too_far_behaviour();
                } else if distance_to
                    > self.leash_elastic_distance()
                        - f64::from(holder.base().dimensions().width)
                        - f64::from(self.base().dimensions().width)
                    && self.check_elastic_interactions(holder.as_ref())
                {
                    self.on_elastic_leash_pull();
                } else {
                    self.close_range_leash_behaviour(holder.as_ref());
                }
                if !self.apply_leash_angular_momentum()
                    && let Some(angular_momentum) = angular_momentum_before_distance_action
                {
                    self.rotate_by_leash_angular_momentum(angular_momentum);
                }
            }
        }
    }

    /// Breaks the leash and drops a lead item. Mirrors Vanilla's `Leashable.dropLeash`.
    fn drop_leash(&self) {
        if self.leash_holder().is_none() {
            return;
        }

        let holder = self.remove_leash_state();
        let _ = self.spawn_at_location(ItemStack::new(&vanilla_items::LEAD), 0.0);
        if let Some(holder) = holder {
            holder.notify_leashee_removed(self.as_entity_event_source());
        }
    }

    /// Removes the leash without dropping a lead item. Mirrors Vanilla's `Leashable.removeLeash`.
    fn remove_leash(&self) {
        if self.leash_holder().is_some()
            && let Some(holder) = self.remove_leash_state()
        {
            holder.notify_leashee_removed(self.as_entity_event_source());
        }
    }

    /// Removes the leash state of this entity, returning its holder before the leash's removal, if any.
    fn remove_leash_state(&self) -> Option<SharedEntity> {
        self.leash_data()
            .lock()
            .take()
            .and_then(|leash_data| leash_data.holder())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeashAttachment {
    Entity(Uuid),
    FenceKnot(BlockPos),
}

#[derive(Debug, Clone)]
pub struct LeashData {
    pub holder: LeashHolder,
    pub angular_momentum: f64,
}

/// Represents the holder of a leash (entity holding the leash).
#[derive(Debug, Clone)]
pub enum LeashHolder {
    /// A direct entity reference to the leash holder.
    Entity(WeakEntity),
    /// An indirect attachment (reference) to the leash holder, which can be resolved later.
    Delayed(LeashAttachment),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct LeashWrench {
    pub(super) force: DVec3,
    pub(super) torque: f64,
}

impl LeashWrench {
    pub const fn new(force: DVec3, torque: f64) -> Self {
        Self { force, torque }
    }
}

impl LeashData {
    pub(crate) fn from_entity(holder: &SharedEntity) -> Self {
        Self {
            holder: LeashHolder::Entity(Arc::downgrade(holder)),
            angular_momentum: 0.0,
        }
    }

    pub(crate) const fn from_delayed_attachment(attachment: LeashAttachment) -> Self {
        Self {
            holder: LeashHolder::Delayed(attachment),
            angular_momentum: 0.0,
        }
    }

    pub(super) fn holder(&self) -> Option<SharedEntity> {
        let LeashHolder::Entity(entity) = &self.holder else {
            return None;
        };
        entity.upgrade()
    }

    pub(super) const fn attachment(&self) -> Option<LeashAttachment> {
        let LeashHolder::Delayed(attachment) = self.holder else {
            return None;
        };
        Some(attachment)
    }

    pub(super) fn saved_attachment(&self) -> Option<LeashAttachment> {
        match &self.holder {
            LeashHolder::Entity(holder) => {
                let upgraded = holder.upgrade()?;
                if let Some(knot) = upgraded.downcast_ref::<LeashFenceKnotEntity>() {
                    // This is a leash knot. Store a position.
                    Some(LeashAttachment::FenceKnot(knot.block_pos()))
                } else {
                    // This is a normal entity. Store its UUID.
                    Some(LeashAttachment::Entity(upgraded.uuid()))
                }
            }
            LeashHolder::Delayed(attachment) => Some(*attachment),
        }
    }

    pub(super) fn set_holder(&mut self, holder: &SharedEntity) {
        self.holder = LeashHolder::Entity(Arc::downgrade(holder));
        self.angular_momentum = 0.0;
    }

    pub(super) fn save(&self, nbt: &mut NbtCompound) {
        if let Some(attachment) = self.saved_attachment() {
            match attachment {
                LeashAttachment::Entity(uuid) => {
                    let mut leash = NbtCompound::new();
                    leash.insert("UUID", NbtTag::IntArray(uuid.to_int_array().to_vec()));
                    nbt.insert("leash", NbtTag::Compound(leash));
                }
                LeashAttachment::FenceKnot(pos) => {
                    nbt.insert("leash", NbtTag::IntArray(vec![pos.x(), pos.y(), pos.z()]));
                }
            }
        }
    }

    pub(super) fn load(nbt: BorrowedNbtCompoundView<'_, '_>) -> Option<Self> {
        if let Some(leash) = nbt.compound("leash")
            && let Some(uuid_array) = leash.int_array("UUID")
            && let Some(uuid) = Uuid::from_int_array(&uuid_array)
        {
            return Some(Self::from_delayed_attachment(LeashAttachment::Entity(uuid)));
        }

        nbt.int_array("leash")
            .filter(|position| position.len() == 3)
            .map(|position| {
                Self::from_delayed_attachment(LeashAttachment::FenceKnot(BlockPos::new(
                    position[0],
                    position[1],
                    position[2],
                )))
            })
    }
}

pub(super) fn leash_dimensions(entity: &dyn Entity) -> DVec3 {
    let dimensions = entity.base().dimensions();
    DVec3::new(
        f64::from(dimensions.width),
        f64::from(dimensions.height),
        f64::from(dimensions.width),
    )
}

pub(super) fn leash_bounding_box_center(entity: &dyn Entity) -> DVec3 {
    let bounding_box = entity.bounding_box();
    DVec3::new(
        f64::midpoint(bounding_box.min_x(), bounding_box.max_x()),
        f64::midpoint(bounding_box.min_y(), bounding_box.max_y()),
        f64::midpoint(bounding_box.min_z(), bounding_box.max_z()),
    )
}

pub(super) fn leash_holder_movement(entity: &dyn Entity) -> DVec3 {
    if entity.as_mob().is_some_and(Mob::is_no_ai) {
        return DVec3::ZERO;
    }

    entity.known_movement()
}

pub(super) fn rotate_y(vector: DVec3, radians: f32) -> DVec3 {
    let cos = f64::from(radians.cos());
    let sin = f64::from(radians.sin());
    DVec3::new(
        vector.x * cos + vector.z * sin,
        vector.y,
        vector.z * cos - vector.x * sin,
    )
}

pub(super) fn axis_specific_leash_elasticity(force: DVec3) -> DVec3 {
    force * LEASH_AXIS_SPECIFIC_ELASTICITY
}

pub(super) fn compute_elastic_interaction(
    entity: &dyn Entity,
    holder: &dyn Entity,
    slack_distance: f64,
) -> Option<LeashWrench> {
    let entity_y_rot = entity.rotation().0 * PI / 180.0;
    let entity_attach_vector = rotate_y(
        ENTITY_LEASH_ATTACHMENT_POINT * leash_dimensions(entity),
        -entity_y_rot,
    );
    let entity_attach_pos = entity.position() + entity_attach_vector;

    let holder_y_rot = holder.rotation().0 * PI / 180.0;
    let holder_attach_vector = rotate_y(
        LEASHER_ATTACHMENT_POINT * leash_dimensions(holder),
        -holder_y_rot,
    );
    let holder_attach_pos = holder.position() + holder_attach_vector;

    compute_dampened_spring_interaction(
        holder_attach_pos,
        entity_attach_pos,
        slack_distance,
        leash_holder_movement(entity),
        entity_attach_vector,
    )
}

pub(super) fn compute_dampened_spring_interaction(
    pivot_point: DVec3,
    object_position: DVec3,
    spring_slack: f64,
    object_motion: DVec3,
    lever_arm: DVec3,
) -> Option<LeashWrench> {
    let distance = object_position.distance(pivot_point);
    if distance < spring_slack {
        return None;
    }

    let mut displacement = (pivot_point - object_position).normalize() * (distance - spring_slack);
    let torque = torque_from_force(lever_arm, displacement);
    if object_motion.dot(displacement) >= 0.0 {
        displacement *= 1.0 - LEASH_SPRING_DAMPENING;
    }

    Some(LeashWrench::new(displacement, torque))
}

pub(super) fn torque_from_force(lever_arm: DVec3, force: DVec3) -> f64 {
    lever_arm.z * force.x - lever_arm.x * force.z
}
