use crate::entity::BorrowedNbtCompoundView;
use std::sync::Weak;
use glam::DVec3;
use parking_lot::MutexGuard;
use simdnbt::owned::{NbtCompound, NbtTag};
use simdnbt::{FromNbtTag, ToNbtTag};
use uuid::Uuid;
use steel_macros::entity_behavior;
use steel_registry::blocks::behavior::PushReaction;
use steel_registry::entity_data::EntityPose;
use steel_registry::entity_type::{EntityDimensions, EntityTypeRef};
use steel_registry::vanilla_entity_data::InteractionEntityData;
use steel_utils::{DowncastType, DowncastTypeKey, UuidExt, WorldAabb};
use steel_utils::locks::SyncMutex;
use steel_utils::types::InteractionHand;
use crate::behavior::InteractionResult;
use crate::entity::{Entity, EntityBase, EntityBaseLoad, EntitySyncedData};
use crate::entity::damage::DamageSource;
use crate::player::Player;
use crate::world::World;

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

/// An invisible, invincible, interactable entity which records when a player clicks in its bounding
/// box. It is used in map-making or data packs, and its bounding box is customizable.
#[entity_behavior(class = "Interaction")]
pub struct InteractionEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    entity_data: SyncMutex<InteractionEntityData>,
    interaction: SyncMutex<Option<PlayerAction>>,
    attack: SyncMutex<Option<PlayerAction>>
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

    /// Creates a interaction entity from saved data.
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

    /// Gets a reference to the entity data for reading/modifying synced state.
    pub const fn entity_data(&self) -> &SyncMutex<InteractionEntityData> {
        &self.entity_data
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
        *self.attack.lock() = Some(PlayerAction {
            player: player.uuid(),
            timestamp: player.get_world().game_time()
        });
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

    fn interact(&self, player: &Player, _hand: InteractionHand, _location: DVec3) -> InteractionResult {
        *self.interaction.lock() = Some(PlayerAction {
            player: player.uuid(),
            timestamp: player.get_world().game_time()
        });
        InteractionResult::Consume
    }

    fn no_physics(&self) -> bool {
        true
    }

    fn dimensions_for_pose(&self, _pose: EntityPose) -> EntityDimensions {
        let guard = self.entity_data.lock();
        EntityDimensions::with_default_eye_height(
            *guard.width.get(),
            *guard.height.get()
        )
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
            guard.width.set(nbt.float(TAG_WIDTH).unwrap_or(DEFAULT_WIDTH));
            guard.height.set(nbt.float(TAG_HEIGHT).unwrap_or(DEFAULT_HEIGHT));
            guard.response.set(nbt.byte(TAG_RESPONSE).map(|b| b != 0).unwrap_or(DEFAULT_RESPONSE));
        }
        *self.attack.lock() = nbt.get(TAG_ATTACK).and_then(PlayerAction::from_nbt_tag);
        *self.interaction.lock() = nbt.get(TAG_INTERACTION).and_then(PlayerAction::from_nbt_tag);
    }

    fn hurt(&self, _world: &World, _source: &DamageSource, _amount: f32) -> bool {
        false
    }
}

/// Represents an action of a player.
#[derive(Debug, Copy, Clone)]
pub struct PlayerAction {
    /// The player who did the action.
    player: Uuid,

    /// The game time (in ticks) when the player did the action.
    timestamp: i64
}

impl FromNbtTag for PlayerAction {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        Some(
            Self {
                player: Uuid::from_int_array(&*compound.int_array(TAG_PLAYER)?)?,
                timestamp: compound.long(TAG_TIMESTAMP)?
            }
        )
    }
}

impl ToNbtTag for &PlayerAction {
    fn to_nbt_tag(self) -> NbtTag {
        let mut compound = NbtCompound::new();
        compound.insert(TAG_PLAYER, NbtTag::IntArray(self.player.to_int_array().to_vec()));
        compound.insert(TAG_TIMESTAMP, self.timestamp);
        NbtTag::Compound(compound)
    }
}

/// Provides the view of an interaction entity.
pub struct InteractionEntityView<'a> {
    guard: MutexGuard<'a, InteractionEntityData>
}