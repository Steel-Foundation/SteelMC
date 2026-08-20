//! Eye of ender entity implementation (`EyeOfEnder`).
//!
//! Thrown by [`EnderEyeItem`](crate::behavior::items::EnderEyeItem) to point
//! toward the nearest stronghold. Unlike its neighbors in this module, vanilla's
//! `EyeOfEnderEntity` extends plain `Entity` (not `Projectile`/`ThrowableProjectile`)
//! and drives its own position/velocity manually toward a stored target each tick,
//! rather than using inherited projectile physics.

use std::sync::{Arc, Weak};

use glam::DVec3;
use steel_macros::entity_behavior;
use steel_math::lerp;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_entity_data::EyeOfEnderEntityData;
use steel_registry::{level_events, sound_events, vanilla_entities, vanilla_items};
use steel_utils::locks::SyncMutex;
use steel_utils::{BlockPos, DowncastType, DowncastTypeKey};

use simdnbt::ToNbtTag;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;

use crate::entity::entities::ItemEntity;
use crate::entity::{
    Entity, EntityBase, EntityBaseLoad, EntityBaseState, EntitySyncedData, RemovalReason,
    SharedEntity, next_entity_id,
};
use crate::world::World;

/// Vanilla: ticks alive before the eye plays its death sound and either drops
/// itself as an item or shatters. `EyeOfEnderEntity` field `lifespan` threshold.
const LIFESPAN_TICKS: i32 = 80;

/// Vanilla `EyeOfEnderEntity.initTargetPos`: horizontal distance beyond which
/// the eye targets a nearby point in the structure's direction instead of the
/// structure's real (and usually far away / underground) position.
const TARGET_APPROACH_DISTANCE: f64 = 12.0;

/// Vanilla: height above the thrower the eye targets when clamping to
/// `TARGET_APPROACH_DISTANCE`.
const TARGET_APPROACH_HEIGHT: f64 = 8.0;

/// Vanilla `EyeOfEnderEntity.updateVelocity`: `MathHelper.lerp` alpha applied
/// to horizontal speed each tick.
const VELOCITY_LERP_ALPHA: f64 = 0.0025;

/// Vanilla: horizontal-distance-to-target threshold below which speed is damped.
const NEAR_TARGET_THRESHOLD: f64 = 1.0;

/// Vanilla: damping factor applied to speed when within `NEAR_TARGET_THRESHOLD`.
const NEAR_TARGET_DAMPING: f64 = 0.8;

/// Vanilla: per-tick nudge applied to vertical velocity toward the target height.
const VERTICAL_NUDGE: f64 = 0.015;

/// Mutable eye-specific state that changes during flight.
struct EyeOfEnderState {
    /// Point the eye is flying toward. `None` until `init_target_pos` is called.
    target_pos: Option<DVec3>,
    /// Ticks alive. Despawns at `LIFESPAN_TICKS`.
    lifespan: i32,
    /// Whether this eye drops itself as a pickup item when it expires,
    /// instead of shattering. Rolled fresh each time `init_target_pos` is called.
    drops_item: bool,
}

impl EyeOfEnderState {
    const fn new() -> Self {
        Self {
            target_pos: None,
            lifespan: 0,
            drops_item: false,
        }
    }
}

/// A thrown eye of ender, seeking the nearest stronghold.
///
/// Mirrors vanilla's `EyeOfEnderEntity`:
/// - Flies toward a target point set by `init_target_pos`
/// - Despawns after 80 ticks, dropping itself (4/5 chance) or shattering
/// - Not attackable
#[entity_behavior]
pub struct EyeOfEnder {
    /// Common entity fields (id, uuid, position, etc.).
    base: EntityBase,

    /// Vanilla entity type registered for this implementation.
    entity_type: EntityTypeRef,

    /// Entity data containing the displayed `ItemStack`.
    entity_data: SyncMutex<EyeOfEnderEntityData>,

    /// Eye-specific mutable state.
    state: SyncMutex<EyeOfEnderState>,
}

// SAFETY: This key is owned by Steel and uniquely identifies `EyeOfEnder`.
unsafe impl DowncastType for EyeOfEnder {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/eye_of_ender");
}

impl EyeOfEnder {
    /// Creates a new eye of ender with the default (plain ender eye) displayed item.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self::with_item(entity_type, id, position, Self::default_item(), world)
    }

    /// Creates a new eye of ender with the specified displayed item.
    #[must_use]
    pub fn with_item(
        entity_type: EntityTypeRef,
        id: i32,
        position: DVec3,
        item: ItemStack,
        world: Weak<World>,
    ) -> Self {
        let mut entity_data = EyeOfEnderEntityData::new();
        entity_data.item_stack.set(item);

        Self {
            base: EntityBase::new_with_state(
                id,
                EntityBaseState::new(position, entity_type.dimensions),
                world,
            ),
            entity_type,
            entity_data: SyncMutex::new(entity_data),
            state: SyncMutex::new(EyeOfEnderState::new()),
        }
    }

    /// Creates an eye of ender from saved data with restored base state.
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        let mut entity_data = EyeOfEnderEntityData::new();
        entity_data.item_stack.set(Self::default_item());

        Self {
            base: EntityBase::from_load(load, entity_type.dimensions),
            entity_type,
            entity_data: SyncMutex::new(entity_data),
            state: SyncMutex::new(EyeOfEnderState::new()),
        }
    }

    /// Vanilla `EyeOfEnderEntity.getItem` (private default): a plain ender eye.
    fn default_item() -> ItemStack {
        ItemStack::new(&vanilla_items::ENDER_EYE)
    }

    /// Gets a clone of the displayed item stack.
    #[must_use]
    pub fn get_item(&self) -> ItemStack {
        self.entity_data.lock().item_stack.get().clone()
    }

    /// Sets the displayed item stack.
    pub fn set_item(&self, item: ItemStack) {
        self.entity_data.lock().item_stack.set(item);
    }

    /// Sets the point this eye flies toward, resets its lifespan, and rerolls
    /// whether it will drop itself when it expires.
    ///
    /// Mirrors vanilla `EyeOfEnderEntity.initTargetPos`.
    pub fn init_target_pos(&self, pos: DVec3) {
        let diff = pos - self.position();
        let horizontal_dist = DVec3::new(diff.x, 0.0, diff.z).length();

        let target = if horizontal_dist > TARGET_APPROACH_DISTANCE {
            self.position()
                + DVec3::new(
                    diff.x / horizontal_dist * TARGET_APPROACH_DISTANCE,
                    TARGET_APPROACH_HEIGHT,
                    diff.z / horizontal_dist * TARGET_APPROACH_DISTANCE,
                )
        } else {
            pos
        };

        let mut state = self.state.lock();
        state.target_pos = Some(target);
        state.lifespan = 0;
        state.drops_item = rand::random_range(0..5) > 0;
    }

    /// Vanilla `EyeOfEnderEntity.updateVelocity` (static).
    fn update_velocity(velocity: DVec3, current_pos: DVec3, target_pos: DVec3) -> DVec3 {
        let horizontal = DVec3::new(
            target_pos.x - current_pos.x,
            0.0,
            target_pos.z - current_pos.z,
        );
        let d = horizontal.length();
        let mut e = lerp(
            VELOCITY_LERP_ALPHA,
            DVec3::new(velocity.x, 0.0, velocity.z).length(),
            d,
        );
        let mut f = velocity.y;
        if d < NEAR_TARGET_THRESHOLD {
            e *= NEAR_TARGET_DAMPING;
            f *= NEAR_TARGET_DAMPING;
        }
        let g = if current_pos.y - velocity.y < target_pos.y {
            1.0
        } else {
            -1.0
        };

        horizontal * (e / d) + DVec3::new(0.0, f + (g - f) * VERTICAL_NUDGE, 0.0)
    }
}

impl Entity for EyeOfEnder {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    fn synced_data(&self) -> Option<&dyn EntitySyncedData> {
        Some(&self.entity_data)
    }

    fn attackable(&self) -> bool {
        false
    }

    fn tick(&self) {
        let next_pos = self.position() + self.velocity();

        let target_pos = self.state.lock().target_pos;
        if let Some(target_pos) = target_pos {
            self.set_velocity(Self::update_velocity(self.velocity(), next_pos, target_pos));
        }

        if self.base().try_set_position(next_pos).is_err() {
            self.set_removed(RemovalReason::Discarded);
            return;
        }

        let (lifespan, drops_item) = {
            let mut state = self.state.lock();
            state.lifespan += 1;
            (state.lifespan, state.drops_item)
        };

        if lifespan <= LIFESPAN_TICKS {
            return;
        }

        self.play_sound(&sound_events::ENTITY_ENDER_EYE_DEATH, 1.0, 1.0);
        self.set_removed(RemovalReason::Discarded);

        let Some(world) = self.level() else {
            return;
        };

        if drops_item {
            let item = ItemEntity::with_item(
                &vanilla_entities::ITEM,
                next_entity_id(),
                self.position(),
                self.get_item(),
                Arc::downgrade(&world),
            );
            let entity: SharedEntity = Arc::new(item);
            if let Err(error) = world.try_add_entity(entity) {
                log::debug!("failed to drop eye of ender item: {error}");
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

    fn save_additional(&self, nbt: &mut NbtCompound) {
        // Mirrors vanilla `EyeOfEnderEntity.writeCustomData`: only the displayed item persists.
        nbt.insert("Item", self.get_item().to_nbt_tag());
    }

    fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        // Mirrors vanilla `EyeOfEnderEntity.readCustomData`: falls back to the
        // current item (a plain ender eye by default) if absent/unreadable.
        if let Some(item_tag) = nbt.compound("Item")
            && let Some(item) = ItemStack::from_borrowed_compound(&item_tag)
        {
            self.set_item(item);
        }
    }
}
