use crate::behavior::InteractionResult;
use crate::entity::BorrowedNbtCompoundView;
use crate::entity::damage::DamageSource;
use crate::entity::{Entity, EntityBase, EntityBaseLoad, EntitySyncedData};
use crate::player::Player;
use crate::world::World;
use glam::DVec3;
use parking_lot::MutexGuard;
use simdnbt::owned::{NbtCompound, NbtTag};
use simdnbt::{FromNbtTag, ToNbtTag};
use std::sync::Weak;
use steel_macros::entity_behavior;
use steel_registry::blocks::behavior::PushReaction;
use steel_registry::entity_data::EntityPose;
use steel_registry::entity_type::{EntityDimensions, EntityTypeRef};
use steel_registry::vanilla_entity_data::InteractionEntityData;
use steel_utils::locks::SyncMutex;
use steel_utils::types::InteractionHand;
use steel_utils::{DowncastType, DowncastTypeKey, UuidExt, WorldAabb};
use uuid::Uuid;

const DEFAULT_WIDTH: f32 = 1.0;
const DEFAULT_HEIGHT: f32 = 1.0;
const DEFAULT_RESPONSE: bool = false;

const TAG_WIDTH: &str = "width";
const TAG_HEIGHT: &str = "height";
const TAG_ATTACK: &str = "attack";
const TAG_INTERACTION: &str = "interaction";
const TAG_RESPONSE: &str = "response";

const TAG_PLAYER: &str = "player";
const TAG_TIMESTAMP: &str = "timestamp";

/// An invisible, invincible, interactable entity which records when a player clicks on its bounding
/// box. It is used in map-making or data packs, and its bounding box is customizable.
///
/// The dimensions of its bounding box (`width` and `height`) and whether it triggers a response from
/// the player (`response`) can be accessed and/or modified with [`InteractionEntity::with_entity_data`].
#[entity_behavior(class = "Interaction")]
pub struct InteractionEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    entity_data: SyncMutex<InteractionEntityData>,
    interaction: SyncMutex<Option<PlayerAction>>,
    attack: SyncMutex<Option<PlayerAction>>,
}

// SAFETY: This key is owned by Steel and uniquely identifies `InteractionEntity`.
unsafe impl DowncastType for InteractionEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/interaction");
}

impl InteractionEntity {
    /// Creates a new interaction entity.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self {
            base: EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
            entity_data: SyncMutex::new(InteractionEntityData::new()),
            attack: SyncMutex::new(None),
            interaction: SyncMutex::new(None),
        }
    }

    /// Creates a new interaction entity with a specific UUID.
    #[must_use]
    pub fn with_uuid(
        entity_type: EntityTypeRef,
        id: i32,
        position: DVec3,
        uuid: Uuid,
        world: Weak<World>,
    ) -> Self {
        Self {
            base: EntityBase::with_uuid(id, uuid, position, entity_type.dimensions, world),
            entity_type,
            entity_data: SyncMutex::new(InteractionEntityData::new()),
            attack: SyncMutex::new(None),
            interaction: SyncMutex::new(None),
        }
    }

    /// Creates an interaction entity from saved data.
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        Self {
            base: EntityBase::from_load(load, entity_type.dimensions),
            entity_type,
            entity_data: SyncMutex::new(InteractionEntityData::new()),
            attack: SyncMutex::new(None),
            interaction: SyncMutex::new(None),
        }
    }

    /// Provides an exclusive view to the synced entity data to the given closure, which includes
    /// the dimensions of its bounding box (`width` and `height`), and whether it triggers a response
    /// from the player (`response`).
    ///
    /// Do not attempt to lock entity data within the closure provided.
    pub fn with_entity_data<R>(
        &self,
        f: impl FnOnce(&mut InteractionEntityDataView<'_>) -> R,
    ) -> R {
        let (value, dimensions_changed) = {
            let mut view = InteractionEntityDataView {
                guard: self.entity_data.lock(),
                dimensions_changed: false,
            };
            let value = f(&mut view);
            (value, view.dimensions_changed)
        };
        if dimensions_changed {
            self.refresh_dimensions();
        }
        value
    }

    /// Provides the latest attack (right-click) on this entity.
    pub fn last_attack(&self) -> Option<PlayerAction> {
        *self.attack.lock()
    }

    /// Provides the latest interaction (right-click) on this entity.
    pub fn last_interaction(&self) -> Option<PlayerAction> {
        *self.interaction.lock()
    }
}

impl Entity for InteractionEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    fn is_pickable(&self) -> bool {
        true
    }

    fn skip_attack_interaction(&self, source: &dyn Entity) -> bool {
        let Some(player) = source.as_player() else {
            return false;
        };
        if let Some(world) = self.level() {
            *self.attack.lock() = Some(PlayerAction {
                player: player.uuid(),
                timestamp: world.game_time(),
            });
        }
        !self.entity_data.lock().response.get()
    }

    fn is_ignoring_block_triggers(&self) -> bool {
        true
    }

    fn piston_push_reaction(&self) -> PushReaction {
        PushReaction::Ignore
    }

    fn can_be_hit_by_projectile(&self) -> bool {
        false
    }

    fn make_bounding_box_at(&self, position: DVec3) -> WorldAabb {
        let guard = self.entity_data.lock();
        WorldAabb::entity_box(
            position.x,
            position.y,
            position.z,
            f64::from(*guard.width.get() / 2.0),
            f64::from(*guard.height.get()),
        )
    }

    fn tick(&self) {}

    fn synced_data(&self) -> Option<&dyn EntitySyncedData> {
        Some(&self.entity_data)
    }

    fn interact(
        &self,
        player: &Player,
        _hand: InteractionHand,
        _location: DVec3,
    ) -> InteractionResult {
        if let Some(world) = self.level() {
            *self.interaction.lock() = Some(PlayerAction {
                player: player.uuid(),
                timestamp: world.game_time(),
            });
        }
        InteractionResult::Consume
    }

    fn no_physics(&self) -> bool {
        true
    }

    fn dimensions_for_pose(&self, _pose: EntityPose) -> EntityDimensions {
        let guard = self.entity_data.lock();
        EntityDimensions::with_default_eye_height(*guard.width.get(), *guard.height.get())
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        {
            let guard = self.entity_data.lock();
            nbt.insert(TAG_WIDTH, *guard.width.get());
            nbt.insert(TAG_HEIGHT, *guard.height.get());
            nbt.insert(TAG_RESPONSE, *guard.response.get());
        }
        {
            let guard = self.attack.lock();
            if let Some(attack) = guard.as_ref() {
                nbt.insert(TAG_ATTACK, attack);
            }
        }
        {
            let guard = self.interaction.lock();
            if let Some(interaction) = guard.as_ref() {
                nbt.insert(TAG_INTERACTION, interaction);
            }
        }
    }

    fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        {
            let mut guard = self.entity_data.lock();
            guard
                .width
                .set(nbt.float(TAG_WIDTH).unwrap_or(DEFAULT_WIDTH));
            guard
                .height
                .set(nbt.float(TAG_HEIGHT).unwrap_or(DEFAULT_HEIGHT));
            guard
                .response
                .set(nbt.byte(TAG_RESPONSE).map_or(DEFAULT_RESPONSE, |b| b != 0));
        }
        *self.attack.lock() = nbt.get(TAG_ATTACK).and_then(PlayerAction::from_nbt_tag);
        *self.interaction.lock() = nbt
            .get(TAG_INTERACTION)
            .and_then(PlayerAction::from_nbt_tag);
    }

    fn hurt(&self, _world: &World, _source: &DamageSource, _amount: f32) -> bool {
        false
    }
}

/// Represents an action of a player.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct PlayerAction {
    /// The player who executed the action.
    player: Uuid,

    /// The game time (in ticks) when the player executed the action.
    timestamp: i64,
}

impl PlayerAction {
    /// Returns the unique ID of the player who executed this action.
    pub const fn player(&self) -> Uuid {
        self.player
    }

    /// Returns the game time (in ticks) when the player executed the action.
    pub const fn timestamp(&self) -> i64 {
        self.timestamp
    }
}

impl FromNbtTag for PlayerAction {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        Some(Self {
            player: Uuid::from_int_array(&compound.int_array(TAG_PLAYER)?)?,
            timestamp: compound.long(TAG_TIMESTAMP)?,
        })
    }
}

impl ToNbtTag for &PlayerAction {
    fn to_nbt_tag(self) -> NbtTag {
        let mut compound = NbtCompound::new();
        compound.insert(
            TAG_PLAYER,
            NbtTag::IntArray(self.player.to_int_array().to_vec()),
        );
        compound.insert(TAG_TIMESTAMP, self.timestamp);
        NbtTag::Compound(compound)
    }
}

/// Provides an exclusive view of the synchronized entity data of an interaction entity.
/// This includes its `width`, its `height`, and the `response` boolean.
pub struct InteractionEntityDataView<'a> {
    guard: MutexGuard<'a, InteractionEntityData>,
    dimensions_changed: bool,
}

impl InteractionEntityDataView<'_> {
    /// Gets the width of the bounding box of the interaction entity.
    pub fn width(&self) -> f32 {
        *self.guard.width.get()
    }

    /// Sets the width of the bounding box of the interaction entity to the provided value.
    pub fn set_width(&mut self, width: f32) {
        self.guard.width.set(width);
        if self.guard.width.is_dirty() {
            self.dimensions_changed = true;
        }
    }

    /// Gets the height of the bounding box of the interaction entity.
    pub fn height(&self) -> f32 {
        *self.guard.height.get()
    }

    /// Sets the height of the bounding box of the interaction entity to the provided value.
    pub fn set_height(&mut self, height: f32) {
        self.guard.height.set(height);
        if self.guard.height.is_dirty() {
            self.dimensions_changed = true;
        }
    }

    /// Gets whether interacting with the interaction entity will trigger a response
    /// from the player. If `true`, this means that
    /// - for left clicks, the player will play an attack animation, and
    /// - for right clicks, the player's hand will swing.
    ///
    /// If `false`, no animation plays.
    pub fn response(&self) -> bool {
        *self.guard.response.get()
    }

    /// Sets whether interacting with the interaction entity will trigger a response
    /// from the player. If set to `true`, this means that
    /// - for left clicks, the player will play an attack animation, and
    /// - for right clicks, the player's hand will swing.
    ///
    /// If set to `false`, no animation plays.
    pub fn set_response(&mut self, response: bool) {
        self.guard.response.set(response);
    }
}
