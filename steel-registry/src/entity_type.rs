use glam::DVec3;
use rustc_hash::FxHashMap;
use steel_utils::Identifier;

/// Mob category for spawn classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MobCategory {
    Monster,
    Creature,
    Ambient,
    Axolotls,
    UndergroundWaterCreature,
    WaterCreature,
    WaterAmbient,
    Misc,
}

impl MobCategory {
    #[must_use]
    pub const fn despawn_distance(self) -> i32 {
        match self {
            Self::WaterAmbient => 64,
            Self::Monster
            | Self::Creature
            | Self::Ambient
            | Self::Axolotls
            | Self::UndergroundWaterCreature
            | Self::WaterCreature
            | Self::Misc => 128,
        }
    }

    #[must_use]
    pub const fn no_despawn_distance(self) -> i32 {
        32
    }
}

/// Vanilla attachment point kind used by `EntityDimensions`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EntityAttachment {
    Passenger,
    Vehicle,
    NameTag,
    WardenChest,
}

/// A vanilla entity attachment point before yaw rotation is applied.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EntityAttachmentPoint {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl EntityAttachmentPoint {
    #[must_use]
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    #[must_use]
    fn scaled(self, scale_x: f32, scale_y: f32, scale_z: f32) -> DVec3 {
        DVec3::new(
            self.x * f64::from(scale_x),
            self.y * f64::from(scale_y),
            self.z * f64::from(scale_z),
        )
    }
}

/// Vanilla `EntityAttachments`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EntityAttachments {
    pub passenger: &'static [EntityAttachmentPoint],
    pub vehicle: &'static [EntityAttachmentPoint],
    pub name_tag: &'static [EntityAttachmentPoint],
    pub warden_chest: &'static [EntityAttachmentPoint],
    scale_x: f32,
    scale_y: f32,
    scale_z: f32,
}

impl EntityAttachments {
    #[must_use]
    pub const fn new(
        passenger: &'static [EntityAttachmentPoint],
        vehicle: &'static [EntityAttachmentPoint],
        name_tag: &'static [EntityAttachmentPoint],
        warden_chest: &'static [EntityAttachmentPoint],
    ) -> Self {
        Self {
            passenger,
            vehicle,
            name_tag,
            warden_chest,
            scale_x: 1.0,
            scale_y: 1.0,
            scale_z: 1.0,
        }
    }

    #[must_use]
    pub const fn fallback() -> Self {
        Self::new(&[], &[], &[], &[])
    }

    #[must_use]
    pub fn scale(self, width_factor: f32, height_factor: f32) -> Self {
        Self {
            scale_x: self.scale_x * width_factor,
            scale_y: self.scale_y * height_factor,
            scale_z: self.scale_z * width_factor,
            ..self
        }
    }

    #[must_use]
    pub fn get_clamped(
        self,
        attachment: EntityAttachment,
        index: usize,
        yaw_degrees: f32,
        dimensions: EntityDimensions,
    ) -> DVec3 {
        let point = self.points(attachment).map_or_else(
            || fallback_point(attachment, dimensions),
            |points| {
                points[index.min(points.len() - 1)].scaled(self.scale_x, self.scale_y, self.scale_z)
            },
        );
        rotate_attachment_point(point, yaw_degrees)
    }

    #[must_use]
    pub fn get_average(self, attachment: EntityAttachment, dimensions: EntityDimensions) -> DVec3 {
        let Some(points) = self.points(attachment) else {
            return fallback_point(attachment, dimensions);
        };

        points.iter().fold(DVec3::ZERO, |sum, point| {
            sum + point.scaled(self.scale_x, self.scale_y, self.scale_z)
        }) / points.len() as f64
    }

    fn points(self, attachment: EntityAttachment) -> Option<&'static [EntityAttachmentPoint]> {
        let points = match attachment {
            EntityAttachment::Passenger => self.passenger,
            EntityAttachment::Vehicle => self.vehicle,
            EntityAttachment::NameTag => self.name_tag,
            EntityAttachment::WardenChest => self.warden_chest,
        };
        (!points.is_empty()).then_some(points)
    }
}

fn fallback_point(attachment: EntityAttachment, dimensions: EntityDimensions) -> DVec3 {
    match attachment {
        EntityAttachment::Passenger | EntityAttachment::NameTag => {
            DVec3::new(0.0, f64::from(dimensions.height), 0.0)
        }
        EntityAttachment::Vehicle => DVec3::ZERO,
        EntityAttachment::WardenChest => DVec3::new(0.0, f64::from(dimensions.height) / 2.0, 0.0),
    }
}

fn rotate_attachment_point(point: DVec3, yaw_degrees: f32) -> DVec3 {
    let radians = f64::from(-yaw_degrees).to_radians();
    let cos = radians.cos();
    let sin = radians.sin();
    DVec3::new(
        point.x.mul_add(cos, point.z * sin),
        point.y,
        point.z.mul_add(cos, -(point.x * sin)),
    )
}

/// Entity dimensions used for bounding box calculation.
/// Bounding box is centered on X/Z with Y at entity feet.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EntityDimensions {
    pub width: f32,
    pub height: f32,
    pub eye_height: f32,
    pub attachments: EntityAttachments,
}

impl EntityDimensions {
    /// Creates new entity dimensions.
    #[must_use]
    pub const fn new(width: f32, height: f32, eye_height: f32) -> Self {
        Self {
            width,
            height,
            eye_height,
            attachments: EntityAttachments::fallback(),
        }
    }

    /// Creates new entity dimensions with vanilla attachment points.
    #[must_use]
    pub const fn new_with_attachments(
        width: f32,
        height: f32,
        eye_height: f32,
        attachments: EntityAttachments,
    ) -> Self {
        Self {
            width,
            height,
            eye_height,
            attachments,
        }
    }

    /// Scale dimensions by a factor (for baby entities, etc.)
    #[must_use]
    pub fn scale(&self, factor: f32) -> Self {
        Self {
            width: self.width * factor,
            height: self.height * factor,
            eye_height: self.eye_height * factor,
            attachments: self.attachments.scale(factor, factor),
        }
    }

    /// Get the half-width for bounding box calculation.
    #[must_use]
    pub fn half_width(&self) -> f32 {
        self.width / 2.0
    }
}

/// Behavioral flags for entity collision and interaction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EntityFlags {
    pub is_pushable: bool,
    pub is_attackable: bool,
    pub is_pickable: bool,
    pub can_be_collided_with: bool,
    pub is_pushed_by_fluid: bool,
    pub can_freeze: bool,
    pub can_be_hit_by_projectile: bool,
    pub is_sensitive_to_water: bool,
    pub can_breathe_underwater: bool,
    pub can_be_seen_as_enemy: bool,
}

#[derive(Debug)]
pub struct EntityType {
    pub key: Identifier,
    pub client_tracking_range: i32,
    pub update_interval: i32,
    /// Whether vanilla `ServerEntity` tracks velocity deltas for this type.
    pub track_deltas: bool,

    /// Default entity dimensions.
    pub dimensions: EntityDimensions,
    /// If true, dimensions cannot be scaled.
    pub fixed: bool,

    /// Mob category for spawn classification.
    pub mob_category: MobCategory,
    /// Whether this entity is immune to fire damage.
    pub fire_immune: bool,
    /// Whether this entity can be summoned via commands.
    pub summonable: bool,
    /// Whether this entity type is allowed to exist in Peaceful difficulty.
    pub allowed_in_peaceful: bool,
    /// Whether this entity can spawn far from players.
    pub can_spawn_far_from_player: bool,
    /// Whether this entity type can be serialized to disk.
    /// Set to false for transient entities (lightning, fishing hooks, players).
    pub can_serialize: bool,
    /// Whether vanilla class hierarchy makes this entity an `AbstractBoat`.
    pub is_abstract_boat: bool,
    /// Whether vanilla class hierarchy makes this entity an `AbstractMinecart`.
    pub is_abstract_minecart: bool,
    /// Whether vanilla class hierarchy makes this entity a `VehicleEntity`.
    pub is_vehicle_entity: bool,
    /// Whether vanilla class hierarchy makes this entity a `Projectile`.
    pub is_projectile: bool,

    /// Behavioral flags for collision and interaction.
    pub flags: EntityFlags,

    /// Default attribute base values for this entity type
    /// Empty for entities that don't have attributes (projectiles, items, displays, etc.)
    pub default_attributes: &'static [(&'static str, f64)],
}

pub type EntityTypeRef = &'static EntityType;

pub struct EntityTypeRegistry {
    types_by_id: Vec<EntityTypeRef>,
    types_by_key: FxHashMap<Identifier, usize>,
    tags: FxHashMap<Identifier, Vec<Identifier>>,
    allows_registering: bool,
}

impl Default for EntityTypeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl EntityTypeRegistry {
    // Creates a new, empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self {
            types_by_id: Vec::new(),
            types_by_key: FxHashMap::default(),
            tags: FxHashMap::default(),
            allows_registering: true,
        }
    }

    /// Registers a new entity type
    pub fn register(&mut self, entity_type: EntityTypeRef) {
        assert!(
            self.allows_registering,
            "Cannot register entity types after the registry has been frozen"
        );
        let idx = self.types_by_id.len();
        self.types_by_key.insert(entity_type.key.clone(), idx);
        self.types_by_id.push(entity_type);
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, EntityTypeRef)> + '_ {
        self.types_by_id
            .iter()
            .enumerate()
            .map(|(id, &et)| (id, et))
    }
}

crate::impl_registry!(
    EntityTypeRegistry,
    EntityType,
    types_by_id,
    types_by_key,
    entity_types
);

crate::impl_tagged_registry!(EntityTypeRegistry, types_by_key, "entity type");

#[cfg(test)]
mod tests {
    use crate::vanilla_entities;

    use super::{EntityAttachment, EntityAttachmentPoint, EntityAttachments, EntityDimensions};

    fn assert_vec3_close(left: glam::DVec3, right: glam::DVec3) {
        let diff = left - right;
        assert!(
            diff.length_squared() < 1.0e-12,
            "expected {left:?} to equal {right:?}"
        );
    }

    #[test]
    fn attachment_points_clamp_index_and_rotate_like_vanilla() {
        const PASSENGERS: [EntityAttachmentPoint; 2] = [
            EntityAttachmentPoint::new(0.0, 0.5, 0.0),
            EntityAttachmentPoint::new(1.0, 0.75, 0.0),
        ];
        const ZERO: [EntityAttachmentPoint; 1] = [EntityAttachmentPoint::new(0.0, 0.0, 0.0)];
        let dimensions = EntityDimensions::new_with_attachments(
            1.0,
            2.0,
            1.7,
            EntityAttachments::new(&PASSENGERS, &ZERO, &ZERO, &ZERO),
        );

        let point =
            dimensions
                .attachments
                .get_clamped(EntityAttachment::Passenger, 99, 90.0, dimensions);

        assert_vec3_close(point, glam::DVec3::new(0.0, 0.75, 1.0));
    }

    #[test]
    fn fallback_attachment_points_match_vanilla_defaults() {
        let dimensions = EntityDimensions::new(0.6, 1.8, 1.62);

        assert_vec3_close(
            dimensions
                .attachments
                .get_clamped(EntityAttachment::Passenger, 0, 0.0, dimensions),
            glam::DVec3::new(0.0, 1.8, 0.0),
        );
        assert_vec3_close(
            dimensions
                .attachments
                .get_clamped(EntityAttachment::Vehicle, 0, 0.0, dimensions),
            glam::DVec3::ZERO,
        );
        assert_vec3_close(
            dimensions
                .attachments
                .get_clamped(EntityAttachment::WardenChest, 0, 0.0, dimensions),
            glam::DVec3::new(0.0, 0.9, 0.0),
        );
    }

    #[test]
    fn attachment_average_uses_unrotated_scaled_points() {
        const PASSENGERS: [EntityAttachmentPoint; 2] = [
            EntityAttachmentPoint::new(0.0, 0.5, 0.0),
            EntityAttachmentPoint::new(1.0, 0.75, -0.5),
        ];
        const ZERO: [EntityAttachmentPoint; 1] = [EntityAttachmentPoint::new(0.0, 0.0, 0.0)];
        let dimensions = EntityDimensions::new_with_attachments(
            1.0,
            2.0,
            1.7,
            EntityAttachments::new(&PASSENGERS, &ZERO, &ZERO, &ZERO),
        )
        .scale(2.0);

        assert_vec3_close(
            dimensions
                .attachments
                .get_average(EntityAttachment::Passenger, dimensions),
            glam::DVec3::new(1.0, 1.25, -0.5),
        );
    }

    #[test]
    fn vanilla_track_deltas_exclusions_match_entity_type_method() {
        assert!(!vanilla_entities::PLAYER.track_deltas);
        assert!(!vanilla_entities::BAT.track_deltas);
        assert!(!vanilla_entities::ITEM_FRAME.track_deltas);
        assert!(!vanilla_entities::EVOKER_FANGS.track_deltas);

        assert!(vanilla_entities::ITEM.track_deltas);
        assert!(vanilla_entities::ARROW.track_deltas);
    }

    #[test]
    fn vanilla_class_hierarchy_flags_match_representative_entities() {
        assert!(vanilla_entities::OAK_BOAT.is_abstract_boat);
        assert!(vanilla_entities::OAK_CHEST_BOAT.is_abstract_boat);
        assert!(!vanilla_entities::ITEM.is_abstract_boat);

        assert!(vanilla_entities::MINECART.is_abstract_minecart);
        assert!(vanilla_entities::CHEST_MINECART.is_abstract_minecart);
        assert!(vanilla_entities::TNT_MINECART.is_abstract_minecart);
        assert!(!vanilla_entities::ITEM.is_abstract_minecart);

        assert!(vanilla_entities::OAK_BOAT.is_vehicle_entity);
        assert!(vanilla_entities::MINECART.is_vehicle_entity);
        assert!(!vanilla_entities::ITEM.is_vehicle_entity);

        assert!(vanilla_entities::ARROW.is_projectile);
        assert!(vanilla_entities::WIND_CHARGE.is_projectile);
        assert!(vanilla_entities::FISHING_BOBBER.is_projectile);
        assert!(!vanilla_entities::ITEM.is_projectile);
    }
}
