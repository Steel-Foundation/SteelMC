use std::f32::consts::PI;
use std::sync::Arc;

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::{NbtCompound, NbtTag};
use steel_utils::{BlockPos, Downcast as _, UuidExt as _};
use uuid::Uuid;

use crate::entity::entities::LeashFenceKnotEntity;
use crate::entity::{Entity, Mob, SharedEntity, WeakEntity};

pub(super) const LEASH_SNAP_DISTANCE: f64 = 12.0;
pub(super) const LEASH_ELASTIC_DISTANCE: f64 = 6.0;
pub(super) const LEASH_AXIS_SPECIFIC_ELASTICITY: DVec3 = DVec3::new(0.8, 0.2, 0.8);
pub(super) const LEASH_SPRING_DAMPENING: f64 = 0.7;
pub(super) const LEASH_TORSIONAL_ELASTICITY: f64 = 10.0;
pub(super) const LEASH_STIFFNESS: f64 = 0.11;
pub(super) const ENTITY_LEASH_ATTACHMENT_POINT: DVec3 = DVec3::new(0.0, 0.5, 0.5);
pub(super) const LEASHER_ATTACHMENT_POINT: DVec3 = DVec3::new(0.0, 0.5, 0.0);
pub(super) const DELAYED_LEASH_DROP_TICKS: i32 = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeashAttachment {
    Entity(Uuid),
    FenceKnot(BlockPos),
}

#[derive(Debug, Clone)]
pub(super) struct LeashData {
    pub(super) attachment: LeashAttachment,
    pub(super) holder: Option<WeakEntity>,
    pub(super) angular_momentum: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct LeashWrench {
    pub(super) force: DVec3,
    pub(super) torque: f64,
}

impl LeashWrench {
    pub(super) const fn new(force: DVec3, torque: f64) -> Self {
        Self { force, torque }
    }
}

impl LeashData {
    pub(super) fn from_entity(holder: &SharedEntity) -> Self {
        let attachment = holder.downcast_ref::<LeashFenceKnotEntity>().map_or_else(
            || LeashAttachment::Entity(holder.uuid()),
            |knot| LeashAttachment::FenceKnot(knot.block_pos()),
        );
        Self {
            attachment,
            holder: Some(Arc::downgrade(holder)),
            angular_momentum: 0.0,
        }
    }

    pub(super) const fn from_delayed_attachment(attachment: LeashAttachment) -> Self {
        Self {
            attachment,
            holder: None,
            angular_momentum: 0.0,
        }
    }

    pub(super) fn holder(&self) -> Option<SharedEntity> {
        self.holder.as_ref().and_then(WeakEntity::upgrade)
    }

    pub(super) fn saved_attachment(&self) -> LeashAttachment {
        self.holder().map_or(self.attachment, |holder| {
            holder.downcast_ref::<LeashFenceKnotEntity>().map_or_else(
                || LeashAttachment::Entity(holder.uuid()),
                |knot| LeashAttachment::FenceKnot(knot.block_pos()),
            )
        })
    }

    pub(super) fn set_holder(&mut self, holder: &SharedEntity) {
        self.attachment = holder.downcast_ref::<LeashFenceKnotEntity>().map_or_else(
            || LeashAttachment::Entity(holder.uuid()),
            |knot| LeashAttachment::FenceKnot(knot.block_pos()),
        );
        self.holder = Some(Arc::downgrade(holder));
        self.angular_momentum = 0.0;
    }

    pub(super) fn save(&self, nbt: &mut NbtCompound) {
        match self.saved_attachment() {
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
