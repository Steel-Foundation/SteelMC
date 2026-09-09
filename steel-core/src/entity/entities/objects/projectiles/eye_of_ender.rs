//! Eye of ender entity implementation (`EyeOfEnderEntity`).
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

const LIFESPAN_TICKS: u32 = 80;

const TOO_FAR_DISTANCE: f64 = 12.0;

const TOO_FAR_SIGNAL_HEIGHT: f64 = 8.0;

const VELOCITY_LERP_ALPHA: f64 = 0.0025;

const NEAR_TARGET_THRESHOLD: f64 = 1.0;

const NEAR_TARGET_DAMPING: f64 = 0.8;

const VERTICAL_NUDGE: f64 = 0.015;

struct EyeOfEnderState {
    target_pos: Option<DVec3>,

    lifespan: u32,

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
#[entity_behavior(class = "EyeOfEnder")]
pub struct EyeOfEnderEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    entity_data: SyncMutex<EyeOfEnderEntityData>,
    state: SyncMutex<EyeOfEnderState>,
}

// SAFETY: This key is owned by Steel and uniquely identifies `EyeOfEnderEntity`.
unsafe impl DowncastType for EyeOfEnderEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/eye_of_ender");
}

impl EyeOfEnderEntity {
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

        let target = if horizontal_dist > TOO_FAR_DISTANCE {
            self.position()
                + DVec3::new(
                    diff.x / horizontal_dist * TOO_FAR_DISTANCE,
                    TOO_FAR_SIGNAL_HEIGHT,
                    diff.z / horizontal_dist * TOO_FAR_DISTANCE,
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
    ///
    /// Deliberate divergence: vanilla computes `lv.multiply(e / d)` without
    /// guarding `d`, so an eye sitting exactly on its target's X/Z divides by
    /// zero and poisons its own position with `NaN`. Steel asserts positions are
    /// finite, which would turn that into a world-tick panic, so the horizontal
    /// term is skipped when there is no horizontal distance left to cover. The
    /// vertical nudge still applies, matching what vanilla does for every
    /// non-degenerate input.
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

        let vertical = DVec3::new(0.0, f + (g - f) * VERTICAL_NUDGE, 0.0);
        if d == 0.0 {
            return vertical;
        }

        horizontal * (e / d) + vertical
    }
}

impl Entity for EyeOfEnderEntity {
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
